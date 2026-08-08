//! Regression tests for the three 2026-08-01 speed bugs.
//!
//! All three were found by real-dataset probes, not unit tests. These encode
//! them as deterministic synthetic cases so a future change cannot silently
//! reintroduce any of them without failing here first.
//!
//! 1. PHANTOM SEGMENTS: `has_proper_self_crossing_sweep` flattens exterior +
//!    holes into one coord slice. The segment at index end-1 of ring r spans
//!    (closure vertex of r) → (first vertex of ring r+1) — a line that does
//!    not exist in the geometry. It must be excluded from the R-tree build,
//!    the query loop, AND the adjacency check. A valid polygon whose phantom
//!    segment would cross a real edge must NOT be flagged.
//! 2. TINY-EDGE COLLINEARITY: `rings_share_collinear_edge` must use the
//!    RELATIVE tolerance 1e-12 * da_len * db_len, never an absolute threshold
//!    with a 1.0 floor. Two tiny non-collinear edges (measured: two 1.6e-7
//!    edges, cross 2.8e-14) were falsely flagged as collinear → false DIR.
//! 3. BOUNDARY-TOUCHING HOLES: GEOS IsValidOp accepts a hole touching the
//!    shell at a vertex. The large-valid fast-path gate must use
//!    `holes_are_valid_inclusive`, not the exclusive probe, or GEOS-valid
//!    polygons get sent into the subtract pipeline (measured: 857 holes on a
//!    159k shell = 11.8s wasted) or damaged.

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::MakeValid;

/// Ring coordinates as a rotation/reversal-insensitive set of (x,y) pairs.
/// The library normalizes OGC winding on every result (exterior CCW, holes
/// CW) at `make_valid_impl` exit — so "passed through unchanged" must be
/// asserted modulo orientation, not byte-identical.
fn ring_key(ring: &LineString<f64>) -> std::collections::BTreeSet<(i64, i64)> {
    ring.0
        .iter()
        .map(|c| (c.x.to_bits() as i64, c.y.to_bits() as i64))
        .collect()
}

/// Assert that two polygons have the same rings as unordered coordinate
/// sets, ignoring orientation, rotation, and ring order.
fn assert_geometrically_equal(a: &Geometry<f64>, b: &Geometry<f64>) {
    match (a, b) {
        (Geometry::Polygon(pa), Geometry::Polygon(pb)) => {
            let mut rings_a: Vec<_> = std::iter::once(pa.exterior())
                .chain(pa.interiors())
                .map(ring_key)
                .collect();
            let mut rings_b: Vec<_> = std::iter::once(pb.exterior())
                .chain(pb.interiors())
                .map(ring_key)
                .collect();
            rings_a.sort();
            rings_b.sort();
            assert_eq!(
                rings_a, rings_b,
                "polygons must have identical ring coordinate sets\n a={pa:?}\n b={pb:?}"
            );
        }
        _ => panic!("expected two polygons, got {a:?} vs {b:?}"),
    }
}

// ============================================================================
// 1. Phantom segments
// ============================================================================

/// Valid polygon where the phantom segment (exterior closure → first vertex of
/// hole 1) = (0,0)→(50,50) properly crosses hole 2's real edge (10,20)→(20,10)
/// at (15,15). If phantom segments were included in the sweep, this valid
/// polygon would be flagged as self-crossing.
fn phantom_crossing_polygon() -> Polygon<f64> {
    let shell = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 100.0, y: 0.0 },
        Coord { x: 100.0, y: 100.0 },
        Coord { x: 0.0, y: 100.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);
    // Hole 1 starts at (50,50): the exterior phantom is (0,0)→(50,50).
    let hole1 = LineString::new(vec![
        Coord { x: 50.0, y: 50.0 },
        Coord { x: 60.0, y: 50.0 },
        Coord { x: 55.0, y: 60.0 },
        Coord { x: 50.0, y: 50.0 },
    ]);
    // Hole 2 contains the real edge (10,20)→(20,10) which crosses the phantom.
    let hole2 = LineString::new(vec![
        Coord { x: 10.0, y: 20.0 },
        Coord { x: 20.0, y: 10.0 },
        Coord { x: 15.0, y: 5.0 },
        Coord { x: 10.0, y: 20.0 },
    ]);
    Polygon::new(shell, vec![hole1, hole2])
}

#[test]
fn phantom_segment_excluded_from_sweep() {
    let poly = phantom_crossing_polygon();
    // Sanity: the phantom really would cross hole 2's edge.
    // Segment (0,0)→(50,50) and (10,20)→(20,10) intersect at (15,15), interior
    // to both. Assert the two line segments properly cross.
    let phantom = geo::Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 50.0, y: 50.0 });
    let real = geo::Line::new(Coord { x: 10.0, y: 20.0 }, Coord { x: 20.0, y: 10.0 });
    assert!(
        geo_repair::structure::fix_ring::segments_properly_cross_seg(
            phantom.start,
            phantom.end,
            real.start,
            real.end,
        ),
        "test fixture broken: phantom and real edge must properly cross"
    );
    // The polygon itself must not be flagged.
    assert!(
        !geo_repair::structure::has_proper_self_crossing(&poly),
        "valid polygon flagged for phantom-segment crossing"
    );
}

/// Same polygon through the public repair entry: must come back with the
/// same ring coordinates (winding may be normalized to OGC) — never
/// re-repaired into something else.
#[test]
fn phantom_segment_polygon_passes_fast_path() {
    let poly = phantom_crossing_polygon();
    let g: Geometry<f64> = poly.clone().into();
    let out = g.make_valid();
    // The fast path returns the input clone; `make_valid_impl` exit then
    // normalizes OGC winding (holes CW). So rings must be bit-identical as
    // coordinate SETS, but orientation may differ from the input.
    assert_geometrically_equal(&out, &g);
}

// ============================================================================
// 2. Tiny-edge collinearity (relative tolerance)
// ============================================================================

/// Two tiny rings (edge length 1.6e-7) stacked with a 1.6e-7 gap. Their
/// closest edges are parallel but NOT collinear — the old absolute 1e-12
/// threshold with a 1.0 floor saw cross2 = 2.56e-14 < 1e-12 and reported a
/// false collinear edge share (false DIR).
fn tiny_parallel_offset_rings() -> (Vec<Coord<f64>>, Vec<Coord<f64>>) {
    let e = 1.6e-7;
    let a = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: e, y: 0.0 },
        Coord { x: e, y: -e },
        Coord { x: 0.0, y: -e },
        Coord { x: 0.0, y: 0.0 },
    ];
    let b = vec![
        Coord { x: 0.0, y: e },
        Coord { x: e, y: e },
        Coord { x: e, y: 2.0 * e },
        Coord { x: 0.0, y: 2.0 * e },
        Coord { x: 0.0, y: e },
    ];
    (a, b)
}

#[test]
fn tiny_offset_edges_not_collinear() {
    let (a, b) = tiny_parallel_offset_rings();
    assert!(
        !geo_repair::arrange::rings_share_collinear_edge_test(&a, &b),
        "parallel tiny edges with a gap must NOT be a collinear share"
    );
}

/// Positive control: two tiny rings sharing a real collinear edge with
/// positive-length overlap MUST be detected (true DIR trigger).
#[test]
fn tiny_shared_edge_is_collinear() {
    let e = 1.6e-7;
    let a = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: e, y: 0.0 },
        Coord { x: e, y: -e },
        Coord { x: 0.0, y: -e },
        Coord { x: 0.0, y: 0.0 },
    ];
    // Shares a's top edge [0,e]×{0} with positive overlap.
    let b = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: e, y: 0.0 },
        Coord { x: e, y: e },
        Coord { x: 0.0, y: e },
        Coord { x: 0.0, y: 0.0 },
    ];
    assert!(
        geo_repair::arrange::rings_share_collinear_edge_test(&a, &b),
        "rings sharing a collinear edge must be detected"
    );
}

/// Vertex-only touch between tiny edges is legal (GEOS IsValidOp accepts it)
/// and must NOT be reported as an edge share.
#[test]
fn tiny_vertex_touch_not_collinear() {
    let e = 1.6e-7;
    let a = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: e, y: 0.0 },
        Coord { x: e, y: -e },
        Coord { x: 0.0, y: -e },
        Coord { x: 0.0, y: 0.0 },
    ];
    // Touches a only at the shared vertex (0,0).
    let b = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: -e, y: 0.0 },
        Coord { x: -e, y: e },
        Coord { x: 0.0, y: e },
        Coord { x: 0.0, y: 0.0 },
    ];
    assert!(
        !geo_repair::arrange::rings_share_collinear_edge_test(&a, &b),
        "vertex-only touch must not be an edge share"
    );
}

// ============================================================================
// 3. Boundary-touching holes (inclusive validation)
// ============================================================================

/// Shell square with a hole whose first vertex (50,0) lies exactly ON the
/// bottom shell edge. GEOS IsValidOp accepts this (OGC-valid).
fn boundary_touching_hole_polygon() -> Polygon<f64> {
    let shell = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 100.0, y: 0.0 },
        Coord { x: 100.0, y: 100.0 },
        Coord { x: 0.0, y: 100.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);
    let hole = LineString::new(vec![
        Coord { x: 50.0, y: 0.0 },
        Coord { x: 60.0, y: 10.0 },
        Coord { x: 40.0, y: 10.0 },
        Coord { x: 50.0, y: 0.0 },
    ]);
    Polygon::new(shell, vec![hole])
}

#[test]
fn inclusive_accepts_boundary_touching_hole() {
    let poly = boundary_touching_hole_polygon();
    assert!(
        geo_repair::arrange::holes_are_valid_inclusive(&poly),
        "GEOS-valid hole touching shell at a vertex must pass inclusive check"
    );
    assert!(
        !geo_repair::arrange::holes_are_valid(&poly),
        "exclusive check must reject the same hole (that is the divergence)"
    );
}

/// The large-valid fast-path gate (verts > FAST_PATH_MAX_VERTS) uses the
/// inclusive check. A >50k-vertex valid shell with a boundary-touching hole
/// must pass through UNCHANGED — never sent into the subtract pipeline.
#[test]
fn large_valid_gate_accepts_boundary_touching_hole() {
    let n = 50_001usize;
    let r = 100.0f64;
    let mut shell_coords: Vec<Coord<f64>> = Vec::with_capacity(n + 1);
    for i in 0..n {
        let t = (i as f64) / (n as f64) * std::f64::consts::TAU;
        shell_coords.push(Coord {
            x: r * t.cos(),
            y: r * t.sin(),
        });
    }
    shell_coords.push(shell_coords[0]);
    let shell = LineString::new(shell_coords);
    // Hole apex at shell vertex (100,0), base strictly inside.
    let hole = LineString::new(vec![
        Coord { x: 100.0, y: 0.0 },
        Coord { x: 95.0, y: 5.0 },
        Coord { x: 95.0, y: -5.0 },
        Coord { x: 100.0, y: 0.0 },
    ]);
    let poly = Polygon::new(shell, vec![hole]);
    let total_verts: usize =
        poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
    assert!(total_verts > 50_000, "test must exceed FAST_PATH_MAX_VERTS");

    // Gate predicates.
    assert!(
        !geo_repair::structure::has_proper_self_crossing(&poly),
        "valid large shell must have no proper crossings"
    );
    assert!(
        geo_repair::arrange::holes_are_valid_inclusive(&poly),
        "large gate must accept the boundary-touching hole"
    );

    // Repair must return the polygon byte-identical.
    let g: Geometry<f64> = poly.clone().into();
    let cfg = geo_repair::MakeValidConfig {
        poly_method: geo_repair::PolyMethod::Structure,
        ..Default::default()
    };
    let out = g.make_valid_with_config(&cfg);
    // Same ring coordinate sets; winding may be OGC-normalized.
    assert_geometrically_equal(&out, &g);
    assert!(
        matches!(out, Geometry::Polygon(_)),
        "GEOS-valid large polygon with touching hole must stay a Polygon"
    );
}

/// Same gate shape but the hole is strictly interior: also unchanged, and the
/// exclusive check agrees here (no divergence on fully-interior holes).
#[test]
fn large_valid_gate_interior_hole() {
    let n = 50_001usize;
    let r = 100.0f64;
    let mut shell_coords: Vec<Coord<f64>> = Vec::with_capacity(n + 1);
    for i in 0..n {
        let t = (i as f64) / (n as f64) * std::f64::consts::TAU;
        shell_coords.push(Coord {
            x: r * t.cos(),
            y: r * t.sin(),
        });
    }
    shell_coords.push(shell_coords[0]);
    let shell = LineString::new(shell_coords);
    let hole = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 10.0, y: 0.0 },
        Coord { x: 5.0, y: 8.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);
    let poly = Polygon::new(shell, vec![hole]);
    assert!(
        geo_repair::arrange::holes_are_valid(&poly),
        "strictly-interior hole passes both checks"
    );
    let g: Geometry<f64> = poly.clone().into();
    let cfg = geo_repair::MakeValidConfig {
        poly_method: geo_repair::PolyMethod::Structure,
        ..Default::default()
    };
    let out = g.make_valid_with_config(&cfg);
    assert_geometrically_equal(&out, &g);
}

// ============================================================================
// 4. i_overlay is_fill_top panic (batch killer)
// ============================================================================

/// Seed cc 8785284c (fuzz.proptest-regressions): shell with one 6-vertex hole
/// where the offset hole partially overlaps the shell. geo's boolean engine
/// (i_overlay 4.5.2) asserts `is_fill_top(link.fill)` internally on this
/// input — a panic inside a rayon batch kills the whole run. The repair
/// pipeline must catch it and fall back, never panic.
#[test]
fn is_fill_top_seed_no_panic() {
    let ox = -22.030823457293746;
    let oy = 47.83760522304267;
    let o = |c: Coord<f64>| Coord {
        x: c.x + ox,
        y: c.y + oy,
    };
    let shell = LineString::new(vec![
        Coord {
            x: 54.361268007782414,
            y: 0.0,
        },
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: 18.545352431810002,
            y: 82.90780436757512,
        },
        Coord {
            x: -48.9188106442245,
            y: 33.436234810694,
        },
        Coord {
            x: 54.361268007782414,
            y: 0.0,
        },
    ]);
    let hole = LineString::new(vec![
        o(Coord {
            x: -99.23612848939614,
            y: 36.62847235863089,
        }),
        o(Coord {
            x: -67.18243348803865,
            y: -91.73029466898309,
        }),
        o(Coord {
            x: 71.0940832095457,
            y: 73.26259559237775,
        }),
        o(Coord {
            x: 0.0,
            y: -78.08037623050924,
        }),
        o(Coord {
            x: -17.305041547050905,
            y: 9.846852235332795,
        }),
        o(Coord {
            x: 56.86075455834379,
            y: 26.443175029348033,
        }),
        o(Coord {
            x: -99.23612848939614,
            y: 36.62847235863089,
        }),
    ]);
    let poly = Polygon::new(shell, vec![hole]);
    let g: Geometry<f64> = poly.clone().into();
    let cfgs = [
        ("auto", geo_repair::MakeValidConfig::default()),
        (
            "structure",
            geo_repair::MakeValidConfig {
                poly_method: geo_repair::PolyMethod::Structure,
                ..Default::default()
            },
        ),
        (
            "arrange",
            geo_repair::MakeValidConfig {
                poly_method: geo_repair::PolyMethod::Arrange,
                ..Default::default()
            },
        ),
        (
            "auto+keep",
            geo_repair::MakeValidConfig {
                keep_collapsed: true,
                ..Default::default()
            },
        ),
    ];
    for (name, cfg) in &cfgs {
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            g.make_valid_with_config(cfg)
        }));
        assert!(
            out.is_ok(),
            "[{name}] make_valid panicked on is_fill_top seed — batch killer"
        );
    }
}

// ============================================================================
// 5. Sub-ULP spike collapse (mixed-magnitude ring, Structure-only gap)
// ============================================================================

/// Seed from invariant_mixed_magnitude_polygon: 5 large coords (~1e8) + 3
/// tiny coords (~1e-9) in ONE ring. The tiny segment (0,0)→(0,5.089e-9) is
/// sub-ULP relative to the ring bbox (~4.3e-8) — a degenerate spike that
/// used to survive into noding, chop the big triangle face, and drop 62% of
/// the area (Structure: 1.55e15 vs GEOS 4.10e15, HoleOutsideShell).
/// basic_cleanup must collapse it before noding.
#[test]
fn sub_ulp_spike_mixed_magnitude_ring() {
    let mut ring = vec![
        Coord {
            x: 84956205.27307954,
            y: -45986769.5228732,
        },
        Coord {
            x: -99794971.69789362,
            y: 4896957.693364016,
        },
        Coord {
            x: 95593402.35083151,
            y: -37252189.83613572,
        },
        Coord {
            x: 37149609.09726282,
            y: -63327990.14115548,
        },
        Coord {
            x: 78418546.04729833,
            y: 69380301.01700698,
        },
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: 0.0,
            y: 5.089116040917129e-9,
        },
    ];
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    let poly = Polygon::new(LineString::new(ring), Vec::new());
    let g: Geometry<f64> = poly.clone().into();
    for (name, cfg) in [
        ("auto", geo_repair::MakeValidConfig::default()),
        (
            "structure",
            geo_repair::MakeValidConfig {
                poly_method: geo_repair::PolyMethod::Structure,
                ..Default::default()
            },
        ),
        (
            "arrange",
            geo_repair::MakeValidConfig {
                poly_method: geo_repair::PolyMethod::Arrange,
                ..Default::default()
            },
        ),
    ] {
        let out = g.make_valid_with_config(&cfg);
        let v = geo_repair::validation::GeoValidation::validate(&out);
        assert!(
            v.valid,
            "[{name}] mixed-magnitude ring invalid: {:?}",
            v.errors
        );
    }
    // Area must match GEOS (4096768239996903.5), modulo the collapsed sliver.
    use geo::Area;
    let cfg = geo_repair::MakeValidConfig {
        poly_method: geo_repair::PolyMethod::Structure,
        ..Default::default()
    };
    let out = g.make_valid_with_config(&cfg);
    let area: f64 = match &out {
        Geometry::Polygon(p) => p.unsigned_area(),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
        _ => 0.0,
    };
    let geos_area = 4096768239996903.5f64;
    assert!(
        (area - geos_area).abs() / geos_area < 1e-5,
        "Structure area {area:.4} diverges from GEOS {geos_area:.4}"
    );
}

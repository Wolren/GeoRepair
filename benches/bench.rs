//! Sweep structure parallel vs GEOS across all geometry shapes.
//! Usage: cargo bench --bench bench (no GEOS — serial + parallel columns)
//!        cargo bench --features bench-geos --bench bench (GEOS from source)
//!        cargo bench --features bench-geos-system --bench bench (system GEOS)
use std::time::Instant;

use geo::{Coord, Geometry, Line, LineString, MultiLineString, MultiPolygon, Polygon};
#[cfg(feature = "parallel")]
use geo_repair::parallel::par_fix_polygon_batch_owned;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
use geos::Geom;
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
use rayon::prelude::*;

/// Convert a geo Geometry to a GEOS Geometry via CoordSeq direct construction
/// (no WKT round-trip).
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn geometry_to_geos(geom: &Geometry<f64>) -> Option<geos::Geometry> {
    use geos::{CoordSeq, CoordType};
    match geom {
        Geometry::Point(p) => {
            let mut cs = CoordSeq::new(1, CoordType::XY).ok()?;
            cs.set_x(0, p.0.x).ok()?;
            cs.set_y(0, p.0.y).ok()?;
            geos::Geometry::create_point(cs).ok()
        }
        Geometry::Line(l) => {
            let mut cs = CoordSeq::new(2, CoordType::XY).ok()?;
            cs.set_x(0, l.start.x).ok()?;
            cs.set_y(0, l.start.y).ok()?;
            cs.set_x(1, l.end.x).ok()?;
            cs.set_y(1, l.end.y).ok()?;
            geos::Geometry::create_line_string(cs).ok()
        }
        Geometry::LineString(ls) => {
            let n = ls.0.len() as u32;
            let mut cs = CoordSeq::new(n, CoordType::XY).ok()?;
            for (i, c) in ls.0.iter().enumerate() {
                cs.set_x(i, c.x).ok()?;
                cs.set_y(i, c.y).ok()?;
            }
            geos::Geometry::create_line_string(cs).ok()
        }
        Geometry::Polygon(p) => polygon_to_geos(p),
        Geometry::MultiPoint(mp) => {
            let geoms: Vec<_> =
                mp.0.iter()
                    .filter_map(|p| geometry_to_geos(&Geometry::Point(*p)))
                    .collect();
            if geoms.is_empty() {
                None
            } else {
                geos::Geometry::create_multipoint(geoms).ok()
            }
        }
        Geometry::MultiLineString(mls) => {
            let geoms: Vec<_> = mls
                .0
                .iter()
                .filter_map(|ls| geometry_to_geos(&Geometry::LineString(ls.clone())))
                .collect();
            if geoms.is_empty() {
                None
            } else {
                geos::Geometry::create_multiline_string(geoms).ok()
            }
        }
        Geometry::MultiPolygon(mp) => {
            let geoms: Vec<_> = mp.0.iter().filter_map(|p| polygon_to_geos(p)).collect();
            if geoms.is_empty() {
                None
            } else {
                geos::Geometry::create_multipolygon(geoms).ok()
            }
        }
        _ => None,
    }
}

/// Convert a geo Polygon to a GEOS Polygon via CoordSeq direct construction.
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn polygon_to_geos(poly: &Polygon<f64>) -> Option<geos::Geometry> {
    use geos::{CoordSeq, CoordType};
    fn coords_to_ring(coords: &[Coord<f64>]) -> Option<geos::Geometry> {
        let n = coords.len() as u32;
        let mut cs = CoordSeq::new(n, CoordType::XY).ok()?;
        for (i, c) in coords.iter().enumerate() {
            cs.set_x(i, c.x).ok()?;
            cs.set_y(i, c.y).ok()?;
        }
        geos::Geometry::create_linear_ring(cs).ok()
    }
    let ring = coords_to_ring(&poly.exterior().0)?;
    let holes: Vec<geos::Geometry> = poly
        .interiors()
        .iter()
        .filter_map(|h| coords_to_ring(&h.0))
        .collect();
    geos::Geometry::create_polygon(ring, holes).ok()
}

#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn run_geos_batch(geoms: &[geos::Geometry]) -> f64 {
    let t0 = Instant::now();
    geoms.par_iter().for_each(|gg| {
        let _ = gg.make_valid();
    });
    t0.elapsed().as_secs_f64()
}

/// GEOS line REPAIR reference: `GEOSMakeValid` does not node LineStrings
/// (verified against GeometryFixer.cpp - it strips repeated points and
/// clones; a non-simple line passes through non-simple). The operation
/// GEOS users call to actually fix linework is `UnaryUnion` (noding +
/// dissolve). Used for the invalid-line bench cases so the comparison is
/// repair-vs-repair, not repair-vs-passthrough.
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn run_geos_noding_batch(geoms: &[geos::Geometry]) -> f64 {
    let t0 = Instant::now();
    geoms.par_iter().for_each(|gg| {
        let _ = gg.unary_union();
    });
    t0.elapsed().as_secs_f64()
}

fn make_valid_ring(n: usize, r: f64) -> Polygon<f64> {
    let mut coords = Vec::with_capacity(n);
    for i in 0..n - 1 {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
        coords.push(Coord {
            x: r * angle.cos() + 500.0,
            y: r * angle.sin() + 500.0,
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn make_bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
        ]),
        Vec::new(),
    )
}

/// Bowtie at `n` vertices: the 4v bowtie's four edges, each subdivided
/// into n/4 collinear segments. Same single proper crossing at the
/// diagonal midpoints, but the shell carries n vertices - the repair
/// must strip the collinear runs and node the crossing at scale.
fn make_bowtie_n(n: usize) -> Polygon<f64> {
    let k = (n / 4).max(1);
    let edges: [(f64, f64); 4] = [(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
    let mut coords = Vec::with_capacity(n + 1);
    for e in 0..4 {
        let (ax, ay) = edges[e];
        let (bx, by) = edges[(e + 1) % 4];
        for i in 0..k {
            let t = i as f64 / k as f64;
            coords.push(Coord {
                x: ax + (bx - ax) * t,
                y: ay + (by - ay) * t,
            });
        }
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

/// Deterministic "spaghetti" ring: a torus-wrapped random walk that
/// crosses itself many times. LCG-seeded, no external rand dependency.
/// Invalid by construction (multiple proper crossings).
fn make_spaghetti_ring(n: usize) -> Polygon<f64> {
    let span = (n as f64).sqrt().ceil() as i64;
    let mut x = 0i64;
    let mut y = 0i64;
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as u32
    };
    let mut coords = Vec::with_capacity(n + 1);
    coords.push(Coord {
        x: x as f64,
        y: y as f64,
    });
    for _ in 1..n {
        match next() % 4 {
            0 => x += 1,
            1 => x -= 1,
            2 => y += 1,
            _ => y -= 1,
        }
        x = x.rem_euclid(span);
        y = y.rem_euclid(span);
        coords.push(Coord {
            x: x as f64,
            y: y as f64,
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn run_ser(polys: &[Polygon<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    for p in polys {
        let _ = p.make_valid_with_config(cfg);
    }
    t0.elapsed().as_secs_f64()
}

#[cfg(feature = "parallel")]
fn run_par(polys: &[Polygon<f64>], cfg: &MakeValidConfig) -> f64 {
    // Owned batch: MOVES the polygons through the zero-copy fast path —
    // the fair comparison against GEOS (shared-geometry return, no copy).
    // The borrowed API would charge a full ring clone per polygon inside
    // the timer. The batch is duplicated once OUTSIDE the timer to stand
    // for the pipeline already owning its data (the real-world bench has
    // used par_fix_polygon_batch_owned throughout; 2026-08-09).
    let owned: Vec<Polygon<f64>> = polys.to_vec();
    let t0 = Instant::now();
    let _ = par_fix_polygon_batch_owned(owned, cfg);
    t0.elapsed().as_secs_f64()
}

#[cfg(not(feature = "parallel"))]
fn run_par(_polys: &[Polygon<f64>], _cfg: &MakeValidConfig) -> f64 {
    0.0
}

fn run_line_ser(items: &[Geometry<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    for g in items {
        let _ = g.make_valid_with_config(cfg);
    }
    t0.elapsed().as_secs_f64()
}

#[cfg(feature = "parallel")]
fn run_line_par(items: &[Geometry<f64>], cfg: &MakeValidConfig) -> f64 {
    use rayon::prelude::*;
    let t0 = Instant::now();
    let _: Vec<_> = items
        .par_iter()
        .map(|g| g.make_valid_with_config(cfg))
        .collect();
    t0.elapsed().as_secs_f64()
}

#[cfg(not(feature = "parallel"))]
fn run_line_par(_items: &[Geometry<f64>], _cfg: &MakeValidConfig) -> f64 {
    0.0
}

fn make_line(coords: [(f64, f64); 2]) -> Geometry<f64> {
    Geometry::Line(Line::new(
        Coord {
            x: coords[0].0,
            y: coords[0].1,
        },
        Coord {
            x: coords[1].0,
            y: coords[1].1,
        },
    ))
}

fn make_linestring(coords: &[(f64, f64)]) -> Geometry<f64> {
    let ls: Vec<Coord<f64>> = coords.iter().map(|&(x, y)| Coord { x, y }).collect();
    Geometry::LineString(LineString::new(ls))
}

fn make_multilinestring(parts: &[Vec<(f64, f64)>]) -> Geometry<f64> {
    let lines: Vec<LineString<f64>> = parts
        .iter()
        .map(|coords| {
            let cs: Vec<Coord<f64>> = coords.iter().map(|&(x, y)| Coord { x, y }).collect();
            LineString::new(cs)
        })
        .collect();
    Geometry::MultiLineString(MultiLineString::new(lines))
}

// ─── Specialized shape generators ─────────────────────────────────

fn make_starburst(spikes: usize, r: f64) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(spikes * 2 + 1);
    for i in 0..spikes {
        let a = 2.0 * std::f64::consts::PI * i as f64 / spikes as f64;
        coords.push((0.0, 0.0));
        coords.push((r * a.cos(), r * a.sin()));
    }
    coords.push((0.0, 0.0));
    make_linestring(&coords)
}

fn make_collinear_overlap(segments: usize) -> Geometry<f64> {
    let mut coords = Vec::new();
    for i in 0..segments {
        let x = i as f64 * 10.0;
        coords.push((x, 0.0));
        // overlap back by 5 units
        coords.push((x + 10.0, 0.0));
        if i < segments - 1 {
            coords.push((x + 5.0, 0.0));
        }
    }
    make_linestring(&coords)
}

fn make_extreme_mixed_scale(n: usize) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let x = if i % 2 == 0 { 1e12 } else { 1e-12 };
        let y = if i % 3 == 0 { -1e12 } else { 1e-12 };
        coords.push((x + i as f64, y + i as f64));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn make_tight_ringing(n: usize, amplitude: f64) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let x = i as f64 * 0.1;
        let y = amplitude * ((i as f64 * 3.0).sin() + (i as f64 * 5.0).sin());
        coords.push((x, y));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn hilbert_coords(order: u32) -> Vec<(f64, f64)> {
    fn d2xy(n: u32, d: u32) -> (f64, f64) {
        let mut x = 0i32;
        let mut y = 0i32;
        let mut t = d;
        let mut s = 1i32;
        while s < n as i32 {
            let rx = (t >> 1) & s as u32;
            let ry = (t ^ rx) & s as u32;
            if ry == 0 {
                if rx != 0 {
                    x = s - 1 - x;
                    y = s - 1 - y;
                }
                std::mem::swap(&mut x, &mut y);
            }
            x += rx as i32;
            y += ry as i32;
            t >>= 2;
            s <<= 1;
        }
        (x as f64 * 10.0, y as f64 * 10.0)
    }
    let n = 1u32 << order;
    let total = n * n;
    let mut coords = Vec::with_capacity(total as usize + 1);
    for i in 0..total {
        coords.push(d2xy(n, i));
    }
    coords.push(coords[0]);
    coords
}

fn make_lissajous(n: usize, a: f64, b: f64, scale: f64) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        coords.push((scale * (a * t).sin(), scale * (b * t).sin()));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn make_spoke_wheel(spokes: usize, r: f64) -> Geometry<f64> {
    let mut coords = Vec::new();
    for i in 0..spokes {
        let a = 2.0 * std::f64::consts::PI * i as f64 / spokes as f64;
        coords.push((r * a.cos(), r * a.sin()));
        coords.push((0.0, 0.0));
    }
    coords.push((r, 0.0));
    make_linestring(&coords)
}

fn make_star_comb(spikes: usize) -> Geometry<f64> {
    // Boost Geometry classic worst-case: alternating long/short radii
    // Non-adjacent spikes have near-intersections — NO shared endpoints
    // (Unlike star-burst where all edges share origin)
    let mut coords = Vec::with_capacity(spikes + 1);
    for i in 0..spikes {
        let a = 2.0 * std::f64::consts::PI * i as f64 / spikes as f64;
        let r = if i % 2 == 0 { 1000.0 } else { 50.0 };
        coords.push((r * a.cos(), r * a.sin()));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn make_self_touching_polygon() -> Polygon<f64> {
    // Shell that touches itself at (150,0), forming a hole ("banana polygon")
    Polygon::new(
        LineString::new(vec![
            Coord { x: 100.0, y: 0.0 },
            Coord { x: 100.0, y: 100.0 },
            Coord { x: 200.0, y: 100.0 },
            Coord { x: 200.0, y: 0.0 },
            Coord { x: 150.0, y: 0.0 },
            Coord { x: 170.0, y: 40.0 },
            Coord { x: 130.0, y: 40.0 },
            Coord { x: 150.0, y: 0.0 },
            Coord { x: 100.0, y: 0.0 },
        ]),
        Vec::new(),
    )
}

fn make_nearly_collinear_polygon() -> Polygon<f64> {
    // Shewchuk classic stress case: nearly-collinear vertices
    // small perturbation from collinear stresses orient2d adaptive precision
    Polygon::new(
        LineString::new(vec![
            Coord { x: -0.01, y: -0.59 },
            Coord { x: 0.01, y: 0.57 },
            Coord { x: 5000.0, y: 0.0 }, // long edge to create large bounding box
            Coord { x: 0.0, y: -0.01 },
            Coord { x: -0.01, y: -0.59 },
        ]),
        Vec::new(),
    )
}

fn make_collapsed_polygon() -> Polygon<f64> {
    // Shell with a backtrack edge — partial zero area, tests collapsed output
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 }, // backtrack — zero-area spike
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )
}

fn make_dense_overlap_grid(per_side: usize) -> Geometry<f64> {
    // Grid of small overlapping squares as MultiPolygon
    // Each square overlaps neighbors by ~30% — tests batch overlap performance
    let size = 10.0;
    let stride = size * 0.7;
    let polys: Vec<Polygon<f64>> = (0..per_side)
        .flat_map(|i| {
            (0..per_side).map(move |j| {
                let x = i as f64 * stride;
                let y = j as f64 * stride;
                Polygon::new(
                    LineString::new(vec![
                        Coord { x, y },
                        Coord { x: x + size, y },
                        Coord {
                            x: x + size,
                            y: y + size,
                        },
                        Coord { x, y: y + size },
                        Coord { x, y },
                    ]),
                    Vec::new(),
                )
            })
        })
        .collect();
    Geometry::MultiPolygon(MultiPolygon::new(polys))
}

fn make_large_coord_polygon() -> Polygon<f64> {
    // Polygon with coordinates at ±1e12 — tests numerical stability
    Polygon::new(
        LineString::new(vec![
            Coord { x: -1e12, y: -1e12 },
            Coord { x: 1e12, y: -1e12 },
            Coord { x: 1e12, y: 1e12 },
            Coord { x: -1e12, y: 1e12 },
            Coord { x: -1e12, y: -1e12 },
        ]),
        Vec::new(),
    )
}

/// Subdivide the exterior ring of `poly` to exactly `n` vertices, walking
/// the perimeter by arc length so the shape class (self-touch banana,
/// collapsed spike, large-coord square) is preserved at every size.
/// Interior rings are kept as-is. A degenerate zero-length edge is skipped.
fn subdivide_polygon(poly: &Polygon<f64>, n: usize) -> Polygon<f64> {
    let ring = &poly.exterior().0;
    let edges = ring.len() - 1;
    let lens: Vec<f64> = (0..edges)
        .map(|e| {
            let a = ring[e];
            let b = ring[e + 1];
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
        })
        .collect();
    let total: f64 = lens.iter().sum();
    let mut out = Vec::with_capacity(n + 1);
    let mut e = 0usize;
    let mut edge_off = 0.0f64; // arc distance from edge e's start
    for j in 0..n {
        while e < edges && lens[e] <= 0.0 {
            e += 1;
        }
        if e >= edges {
            break;
        }
        let target = (j as f64 / n as f64) * total;
        while e + 1 < edges && edge_off + lens[e] < target {
            edge_off += lens[e];
            e += 1;
        }
        let a = ring[e];
        let b = ring[e + 1];
        let t = if lens[e] > 0.0 {
            ((target - edge_off) / lens[e]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(Coord {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        });
    }
    if out.len() < 3 || out.first() != out.last() {
        out.push(out[0]);
    }
    Polygon::new(LineString::new(out), poly.interiors().to_vec())
}

/// Subdivide a closed line coordinate list to exactly `n` vertices by arc
/// length (used for the figure-8 self-intersection ladder).
fn subdivide_ls(coords: &[(f64, f64)], n: usize) -> Vec<(f64, f64)> {
    let edges = coords.len() - 1;
    let lens: Vec<f64> = (0..edges)
        .map(|e| {
            let (ax, ay) = coords[e];
            let (bx, by) = coords[e + 1];
            ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt()
        })
        .collect();
    let total: f64 = lens.iter().sum();
    let mut out = Vec::with_capacity(n);
    let mut e = 0usize;
    let mut edge_off = 0.0f64;
    for j in 0..n {
        while e < edges && lens[e] <= 0.0 {
            e += 1;
        }
        if e >= edges {
            break;
        }
        let target = (j as f64 / n as f64) * total;
        while e + 1 < edges && edge_off + lens[e] < target {
            edge_off += lens[e];
            e += 1;
        }
        let (ax, ay) = coords[e];
        let (bx, by) = coords[e + 1];
        let t = if lens[e] > 0.0 {
            ((target - edge_off) / lens[e]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push((ax + (bx - ax) * t, ay + (by - ay) * t));
    }
    out
}

/// Shewchuk near-collinear stress at scale: n vertices on the long edge,
/// each perturbed ~1e-6 off the line so every triple is nearly-collinear
/// (the fixed 5v shape never stressed the orient2d path at size).
fn make_nearly_collinear_polygon_n(n: usize) -> Polygon<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    coords.push(Coord { x: -0.01, y: -0.59 });
    coords.push(Coord { x: 0.01, y: 0.57 });
    for i in 0..n.saturating_sub(3) {
        let t = (i as f64 + 1.0) / (n as f64 - 2.0);
        let x = 5000.0 * t;
        let y = 1e-6 * ((i as f64 * 1.7).sin() + (i as f64 * 3.1).sin());
        coords.push(Coord { x, y });
    }
    coords.push(Coord { x: 5000.0, y: 0.0 });
    coords.push(Coord { x: 0.0, y: -0.01 });
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

/// LineString of consecutive duplicate pairs — n pairs = 2n vertices.
fn make_duped_ls(pairs: usize) -> Geometry<f64> {
    let mut coords = Vec::new();
    for i in 0..pairs {
        coords.push((i as f64, 0.0));
        coords.push((i as f64, 0.0));
    }
    make_linestring(&coords)
}

/// MultiLineString of `parts` short 3-vertex components.
fn make_mls_parts(parts: usize) -> Geometry<f64> {
    let parts_v: Vec<Vec<(f64, f64)>> = (0..parts)
        .map(|i| {
            let base = i as f64 * 200.0;
            vec![(base, 0.0), (base + 100.0, 50.0), (base + 200.0, 0.0)]
        })
        .collect();
    make_multilinestring(&parts_v)
}

/// MultiLineString of `parts` self-intersecting 4-vertex components.
fn make_mls_selfint_parts(parts: usize) -> Geometry<f64> {
    let parts_v: Vec<Vec<(f64, f64)>> = (0..parts)
        .map(|i| {
            let base = i as f64 * 30.0;
            vec![
                (base, 0.0),
                (base + 10.0, 10.0),
                (base + 10.0, 0.0),
                (base, 10.0),
            ]
        })
        .collect();
    make_multilinestring(&parts_v)
}

fn make_multipoly_overlap(n: usize, size: f64) -> Geometry<f64> {
    let polys: Vec<Polygon<f64>> = (0..n)
        .map(|i| {
            let off = i as f64 * size * 0.3;
            Polygon::new(
                LineString::new(vec![
                    Coord { x: off, y: off },
                    Coord {
                        x: off + size,
                        y: off,
                    },
                    Coord {
                        x: off + size,
                        y: off + size,
                    },
                    Coord {
                        x: off,
                        y: off + size,
                    },
                    Coord { x: off, y: off },
                ]),
                Vec::new(),
            )
        })
        .collect();
    Geometry::MultiPolygon(MultiPolygon::new(polys))
}

fn make_hole_hierarchy(n_holes: usize, size: f64) -> Polygon<f64> {
    let shell = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: size, y: 0.0 },
        Coord { x: size, y: size },
        Coord { x: 0.0, y: size },
        Coord { x: 0.0, y: 0.0 },
    ]);
    let grid = (n_holes as f64).sqrt().ceil() as usize;
    let cell = size / (grid + 1) as f64;
    let mut holes = Vec::new();
    let mut idx = 0usize;
    for gi in 0..grid {
        for gj in 0..grid {
            if idx >= n_holes {
                break;
            }
            let cx = (gi + 1) as f64 * cell;
            let cy = (gj + 1) as f64 * cell;
            let hs = cell * 0.3;
            holes.push(LineString::new(vec![
                Coord {
                    x: cx - hs,
                    y: cy - hs,
                },
                Coord {
                    x: cx + hs,
                    y: cy - hs,
                },
                Coord {
                    x: cx + hs,
                    y: cy + hs,
                },
                Coord {
                    x: cx - hs,
                    y: cy + hs,
                },
                Coord {
                    x: cx - hs,
                    y: cy - hs,
                },
            ]));
            idx += 1;
        }
        if idx >= n_holes {
            break;
        }
    }
    Polygon::new(shell, holes)
}

fn make_sliver_polygon(segments: usize, gap: f64) -> Polygon<f64> {
    let n = segments;
    let mut coords = Vec::with_capacity(n * 2 + 1);
    for i in 0..n {
        let x = i as f64 * 10.0;
        let y = (i as f64 * 0.3).sin() * gap;
        coords.push(Coord { x, y });
    }
    for i in (0..n).rev() {
        let x = i as f64 * 10.0;
        let y = (i as f64 * 0.3).sin() * gap - gap;
        coords.push(Coord { x, y });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn bench_line(label: &str, g: &Geometry<f64>, batch: usize, cfg: &MakeValidConfig) {
    if !gate_filtered(label) {
        return;
    }
    let items: Vec<Geometry<f64>> = (0..batch).map(|_| g.clone()).collect();
    let par = run_line_par(&items, cfg);
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        // CoordSeq direct construction — no WKT overhead
        let geos_geoms: Vec<geos::Geometry> = items.iter().filter_map(geometry_to_geos).collect();
        // GEOS reference by geometry class, not by label prefix (the
        // prefix routing silently swapped the reference for the
        // MultiPolygon rows - dense grid / overlap mp - and collapsed
        // their ratios ~46x, 2026-08-09):
        //  - valid inputs: makeValid is the honest check reference (it
        //    returns the valid input unchanged)
        //  - invalid LINES: makeValid is a passthrough (GeometryFixer.cpp
        //    strips repeated points and clones; non-simple passes
        //    through) - the honest repair reference is UnaryUnion
        //  - invalid POLYGONS/MPs: makeValid is the repair reference
        //    users call (UnaryUnion is ~46x cheaper on overlapping
        //    shells - measured 13ms vs 600ms at 400 shells - and would
        //    flatter our MP rows)
        let is_line_geom = matches!(
            g,
            Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_)
        );
        let geos = if label.starts_with("valid") || !is_line_geom {
            run_geos_batch(&geos_geoms)
        } else {
            run_geos_noding_batch(&geos_geoms)
        };
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            par * 1_000_000.0 / batch as f64,
            geos * 1_000_000.0 / batch as f64,
        );
        gate_emit(
            label,
            None,
            par * 1_000_000.0 / batch as f64,
            Some(geos * 1_000_000.0 / batch as f64),
        );
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        let ser = run_line_ser(&items, cfg);
        let ser_us = ser * 1_000_000.0 / batch as f64;
        let par_us = par * 1_000_000.0 / batch as f64;
        eprintln!("  {:<20} {:>10.3} {:>10.3} µs", label, ser_us, par_us);
        gate_emit(label, Some(ser_us), par_us, None);
    }
}

/// CI bench-gate support: BENCH_SUBSET (comma-separated label prefixes,
/// empty = all) keeps the gate run short; BENCH_JSON (path) appends one
/// JSON row per measured case so scripts/bench_gate.py can compare against
/// a committed baseline.
fn gate_filtered(label: &str) -> bool {
    match std::env::var("BENCH_SUBSET") {
        Ok(sub) => sub
            .split(',')
            .map(str::trim)
            .any(|p| !p.is_empty() && label.starts_with(p)),
        Err(_) => true,
    }
}

fn gate_emit(label: &str, ser_us: Option<f64>, par_us: f64, geos_us: Option<f64>) {
    if let Ok(path) = std::env::var("BENCH_JSON") {
        let ser = ser_us
            .map(|v| format!("{v}"))
            .unwrap_or_else(|| "null".to_string());
        let geos = geos_us
            .map(|v| format!("{v}"))
            .unwrap_or_else(|| "null".to_string());
        let row = format!(
            "{{\"label\": {label:?}, \"ser_us\": {ser}, \"par_us\": {par_us}, \"geos_us\": {geos}}}\n"
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = f.write_all(row.as_bytes());
        }
    }
}

fn bench_polygons(label: &str, polys: &[Polygon<f64>], batch: usize, cfg: &MakeValidConfig) {
    if !gate_filtered(label) {
        return;
    }
    let par = run_par(polys, cfg);
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        // CoordSeq direct construction — no WKT overhead
        let geos_geoms: Vec<geos::Geometry> = polys.iter().filter_map(polygon_to_geos).collect();
        let geos = run_geos_batch(&geos_geoms);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            par * 1_000_000.0 / batch as f64,
            geos * 1_000_000.0 / batch as f64,
        );
        gate_emit(
            label,
            None,
            par * 1_000_000.0 / batch as f64,
            Some(geos * 1_000_000.0 / batch as f64),
        );
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        let ser = run_ser(polys, cfg);
        let par_us = par * 1_000_000.0 / batch as f64;
        let ser_us = ser * 1_000_000.0 / batch as f64;
        eprintln!("  {:<20} {:>10.3} {:>10.3} µs", label, ser_us, par_us);
        gate_emit(label, Some(ser_us), par_us, None);
    }
}

#[cfg_attr(
    feature = "hotpath",
    hotpath::main(
        format = "json-pretty",
        output_path = "target/profiling/bench_hotpath_report.json"
    )
)]
fn main() {
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };

    // Warmup
    let poly4 = make_valid_ring(4, 100.0);
    let mut warm: Vec<Polygon<f64>> = Vec::new();
    for _ in 0..10000 {
        warm.push(poly4.clone());
    }
    run_ser(&warm, &cfg);
    run_par(&warm, &cfg);

    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    let header = ("geo-repair", "geos");
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    let header = ("serial", "parallel");

    eprintln!(
        "  {:<20} {:>10} {:>10} {:>10}",
        "test", header.0, header.1, "unit"
    );
    eprintln!("{}", "-".repeat(55));

    // Valid polygons: small batch (50K) for cheap tests, 1000 for expensive
    for &(n, batch) in &[
        (4, 50000usize),
        (10, 50000),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 1000),
        (5000, 1000),
        (10000, 1000),
    ] {
        let poly = make_valid_ring(n, 100.0);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("valid polygon {:>5}v", n), &polys, batch, &cfg);
    }

    // Invalid bowtie 4v
    {
        let poly = make_bowtie();
        let polys: Vec<Polygon<f64>> = (0..50000)
            .map(|i| {
                let mut p = poly.clone();
                p.exterior_mut(|ext| {
                    for c in &mut ext.0 {
                        c.x += i as f64 * 20.0;
                        c.y += i as f64 * 20.0;
                    }
                });
                p
            })
            .collect();
        bench_polygons("invalid bowtie 4v", &polys, 50000, &cfg);
    }

    // Invalid bowtie at scale: same bowtie with subdivided edges, 50/100/
    // 500/1000 vertices (the user's explicit ask 2026-08-07 - the bench
    // only had the 4v bowtie, which never stressed the repair at scale).
    for &(n, batch) in &[(50usize, 5000usize), (100, 2000), (500, 200), (1000, 100)] {
        let poly = make_bowtie_n(n);
        let polys: Vec<Polygon<f64>> = (0..batch)
            .map(|i| {
                let mut p = poly.clone();
                p.exterior_mut(|ext| {
                    for c in &mut ext.0 {
                        c.x += i as f64 * 20.0;
                        c.y += i as f64 * 20.0;
                    }
                });
                p
            })
            .collect();
        bench_polygons(&format!("invalid bowtie {:>4}v", n), &polys, batch, &cfg);
    }

    // Star polygon (spiky, r alternating 100/50 every 3rd vertex): VALID
    // geometry at every size - verified with geosop isValid (2026-08-07).
    // The old "invalid star 100v" label was a misnomer; this row measures
    // the valid-repair fast path on a hard spiky shape at scale.
    for &(n, batch) in &[(100usize, 1000usize), (500, 200), (1000, 100)] {
        let mut coords = Vec::with_capacity(n);
        for i in 0..n - 1 {
            let angle = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
            let r = if i % 3 == 0 { 100.0 } else { 50.0 };
            coords.push(Coord {
                x: r * angle.cos(),
                y: r * angle.sin(),
            });
        }
        coords.push(coords[0]);
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        let polys: Vec<Polygon<f64>> = (0..batch)
            .map(|i| {
                let mut p = poly.clone();
                p.exterior_mut(|ext| {
                    for c in &mut ext.0 {
                        c.x += i as f64 * 300.0;
                        c.y += i as f64 * 300.0;
                    }
                });
                p
            })
            .collect();
        bench_polygons(&format!("star poly {:>4}v", n), &polys, batch, &cfg);
    }

    // Spaghetti rings: torus-wrapped random walks, dozens of proper
    // crossings each - the "many crossings at scale" repair class
    // (geosop isValid: false).
    for &(n, batch) in &[(500usize, 100usize), (2000, 20)] {
        let poly = make_spaghetti_ring(n);
        let polys: Vec<Polygon<f64>> = (0..batch)
            .map(|i| {
                let mut p = poly.clone();
                p.exterior_mut(|ext| {
                    for c in &mut ext.0 {
                        c.x += i as f64 * (n as f64);
                        c.y += i as f64 * (n as f64);
                    }
                });
                p
            })
            .collect();
        bench_polygons(&format!("spaghetti {:>4}v", n), &polys, batch, &cfg);
    }

    // Self-touching (banana) polygon — tests self-touch forming hole.
    // Arc-length subdivision keeps the touch point at every size so a
    // size-scaling regression in the touch path is measurable.
    for &(n, batch) in &[(100usize, 5000usize), (500, 1000), (1000, 500)] {
        let poly = subdivide_polygon(&make_self_touching_polygon(), n);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("self-touch {:>4}v", n), &polys, batch, &cfg);
    }

    // Collapsed polygon (zero-area spike) — tests collapsed output handling
    for &(n, batch) in &[(100usize, 5000usize), (500, 1000), (1000, 500)] {
        let poly = subdivide_polygon(&make_collapsed_polygon(), n);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("collapsed {:>4}v", n), &polys, batch, &cfg);
    }

    // Nearly-collinear polygon — Shewchuk orient2d stress case
    for &(n, batch) in &[(100usize, 5000usize), (500, 1000), (1000, 200)] {
        let poly = make_nearly_collinear_polygon_n(n);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("near-collinear {:>4}v", n), &polys, batch, &cfg);
    }

    // Large coordinate polygon (±1e12) — tests numerical stability vs GEOS
    for &(n, batch) in &[(100usize, 5000usize), (500, 1000), (1000, 500)] {
        let poly = subdivide_polygon(&make_large_coord_polygon(), n);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("large coord 1e12 {:>4}v", n), &polys, batch, &cfg);
    }

    // ─── Line benchmarks ─────────────────────────────────────────────
    eprintln!("{}", "-".repeat(55));

    // Single Line tests
    bench_line(
        "valid line",
        &make_line([(0.0, 0.0), (1000.0, 500.0)]),
        100000,
        &cfg,
    );
    bench_line(
        "zero-length line",
        &make_line([(5.0, 5.0), (5.0, 5.0)]),
        100000,
        &cfg,
    );

    // Valid LineString vertex sweep
    for &(n, batch) in &[
        (4, 50000usize),
        (10, 50000),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 1000),
    ] {
        let coords: Vec<(f64, f64)> = (0..n).map(|i| (i as f64, (i as f64).sin())).collect();
        let g = make_linestring(&coords);
        bench_line(&format!("valid ls {:>4}v", n), &g, batch, &cfg);
    }

    // Collinear LineString vertex sweep
    for &(n, batch) in &[
        (4, 50000usize),
        (10, 50000),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 1000),
    ] {
        let coords: Vec<(f64, f64)> = (0..n).map(|i| (i as f64, 0.0)).collect();
        let g = make_linestring(&coords);
        bench_line(&format!("collinear ls {:>4}v", n), &g, batch, &cfg);
    }

    // Convoluted: zigzag (alternating y)
    for &(n, batch) in &[
        (10, 50000usize),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 500),
    ] {
        let coords: Vec<(f64, f64)> = (0..n)
            .map(|i| (i as f64, if i % 2 == 0 { 0.0 } else { 1000.0 }))
            .collect();
        let g = make_linestring(&coords);
        bench_line(&format!("zigzag ls {:>4}v", n), &g, batch, &cfg);
    }

    // Convoluted: spiral (tightly wound)
    for &(n, batch) in &[
        (10, 50000usize),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 500),
    ] {
        let mut coords = Vec::new();
        for i in 0..n {
            let t = i as f64 * 0.5;
            let r = 100.0 + t * 2.0;
            coords.push((r * t.cos(), r * t.sin()));
        }
        let g = make_linestring(&coords);
        bench_line(&format!("spiral ls {:>4}v", n), &g, batch, &cfg);
    }

    // Self-intersecting: figure-8, arc-length subdivided to n vertices so
    // the crossing class is preserved at scale (the fixed 5v shape never
    // measured size scaling of the noding path).
    for &(n, batch) in &[(100usize, 5000usize), (500, 500), (1000, 200)] {
        let base = vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ];
        let g = make_linestring(&subdivide_ls(&base, n));
        bench_line(&format!("self-int ls {:>4}v", n), &g, batch, &cfg);
    }

    // Self-intersecting: many crossing edges (dense bowtie chain)
    for &(n, batch) in &[
        (10, 50000usize),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 500),
    ] {
        let mut coords = Vec::new();
        for i in 0..n {
            let x = i as f64 * 10.0;
            let y = if i % 2 == 0 { 0.0 } else { 1000.0 } + (i as f64).sin() * 50.0;
            coords.push((x, y));
        }
        let g = make_linestring(&coords);
        bench_line(&format!("dense self ls {:>4}v", n), &g, batch, &cfg);
    }

    // LineString with consecutive duplicates — n pairs = 2n vertices
    for &(pairs, batch) in &[(50usize, 50000usize), (250, 10000), (500, 1000)] {
        let g = make_duped_ls(pairs);
        bench_line(&format!("duped ls {:>4}v", pairs * 2), &g, batch, &cfg);
    }

    // MultiLineString: many short parts
    for &(parts, batch) in &[(50usize, 10000usize), (250, 2000), (500, 1000)] {
        let g = make_mls_parts(parts);
        bench_line(&format!("mls {}x3v", parts), &g, batch, &cfg);
    }

    // MultiLineString with many self-intersecting components
    for &(parts, batch) in &[(50usize, 10000usize), (250, 2000), (500, 500)] {
        let g = make_mls_selfint_parts(parts);
        bench_line(&format!("self-int mls {}x4v", parts), &g, batch, &cfg);
    }

    // ─── Special shapes ────────────────────────────────────────────
    eprintln!("{}", "-".repeat(55));

    // Star-burst: all edges from/to center — stresses duplicate-vertex detection
    for &(spikes, batch) in &[
        (10usize, 50000usize),
        (50, 10000),
        (100, 5000),
        (500, 100),
        (1000, 50),
    ] {
        let g = make_starburst(spikes, 1000.0);
        bench_line(&format!("star-burst {}sp", spikes), &g, batch, &cfg);
    }

    // Collinear overlap: segments on same line with partial overlap (regression test)
    for &(segments, batch) in &[
        (10usize, 50000usize),
        (50, 10000),
        (100, 5000),
        (500, 500),
        (1000, 200),
    ] {
        let g = make_collinear_overlap(segments);
        bench_line(&format!("collinear ov {}seg", segments), &g, batch, &cfg);
    }

    // Extreme mixed scale: alternates 1e12 and 1e-12 coords — tests epsilon robustness
    for &(n, batch) in &[
        (10usize, 50000usize),
        (50, 1000),
        (100, 100),
        (500, 50),
        (1000, 20),
    ] {
        let g = make_extreme_mixed_scale(n);
        bench_line(&format!("x-scale {}v", n), &g, batch, &cfg);
    }

    // Tight ringing: dense near-miss oscillations — stresses orient2d near-boundary
    for &(n, batch) in &[(100usize, 10000usize), (500, 50), (1000, 50)] {
        let g = make_tight_ringing(n, 1e-8);
        bench_line(&format!("ringing {}v", n), &g, batch, &cfg);
    }

    // Hilbert curve (order 4 = 256v, order 5 = 1024v) — space-filling, grid stress
    {
        let g = make_linestring(&hilbert_coords(4));
        bench_line("hilbert 256v", &g, 2000, &cfg);
    }
    {
        let g = make_linestring(&hilbert_coords(5));
        bench_line("hilbert 1024v", &g, 200, &cfg);
    }

    // Lissajous curve: complex self-intersection pattern (5:3 ratio)
    for &(n, batch) in &[
        (200usize, 5000usize),
        (500, 1000),
        (1000, 100),
        (2000, 50),
        (5000, 10),
    ] {
        let g = make_lissajous(n, 5.0, 3.0, 1000.0);
        bench_line(&format!("lissajous {}v", n), &g, batch, &cfg);
    }
    // Lissajous 7:4 ratio (different crossing pattern)
    {
        let g = make_lissajous(500, 7.0, 4.0, 1000.0);
        bench_line("lissajous 7:4 500v", &g, 1000, &cfg);
    }

    // Spoke wheel: all edges converge at origin — stresses noding at common point
    for &(spokes, batch) in &[
        (10usize, 50000usize),
        (50, 5000),
        (100, 500),
        (500, 50),
        (1000, 25),
    ] {
        let g = make_spoke_wheel(spokes, 1000.0);
        bench_line(&format!("spoke {}sp", spokes), &g, batch, &cfg);
    }

    // Star comb: alternating long/short spikes — NO shared endpoints (differs from star-burst)
    for &(spikes, batch) in &[(20usize, 50000usize), (100, 5000), (500, 100), (1000, 50)] {
        let g = make_star_comb(spikes);
        bench_line(&format!("star-comb {}sp", spikes), &g, batch, &cfg);
    }

    // ─── MultiPolygon / hole hierarchy / sliver ────────────────────
    eprintln!("{}", "-".repeat(55));

    // Hole hierarchy: shell with many nested holes
    for &(nh, batch) in &[(5usize, 10000usize), (20, 1000), (50, 200), (100, 100)] {
        let poly = make_hole_hierarchy(nh, 500.0);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("hole hier {:>3}h", nh), &polys, batch, &cfg);
    }

    // MultiPolygon with overlapping shells
    for &(ns, batch) in &[(5usize, 1000usize), (20, 200), (50, 50), (100, 50)] {
        let g = make_multipoly_overlap(ns, 100.0);
        bench_line(&format!("overlap mp {:>3}sh", ns), &g, batch, &cfg);
    }

    // Dense grid of overlapping small polygons
    {
        let g = make_dense_overlap_grid(5);
        bench_line("dense grid 5x5=25", &g, 1000, &cfg);
    }
    {
        let g = make_dense_overlap_grid(10);
        bench_line("dense grid 10x10=100", &g, 200, &cfg);
    }
    {
        let g = make_dense_overlap_grid(20);
        bench_line("dense grid 20x20=400", &g, 50, &cfg);
    }
    {
        let g = make_dense_overlap_grid(30);
        bench_line("dense grid 30x30=900", &g, 20, &cfg);
    }

    // Sliver edges: near-collinear, very thin polygon
    for &(n, batch) in &[(100usize, 1000usize), (500, 100), (1000, 100)] {
        let poly = make_sliver_polygon(n, 0.001);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("sliver {:>4}v", n), &polys, batch, &cfg);
    }

    // ─── Arrange pipeline (CDT fallback) ───────────────────────────
    #[cfg(feature = "arrange")]
    {
        eprintln!("{}", "-".repeat(55));
        let acfg = MakeValidConfig {
            poly_method: PolyMethod::Arrange,
            ..Default::default()
        };

        for &(n, batch) in &[
            (4usize, 10000usize),
            (10, 5000),
            (50, 1000),
            (100, 500),
            (500, 100),
            (1000, 50),
        ] {
            let poly = make_valid_ring(n, 100.0);
            let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
            bench_polygons(&format!("arrange valid {:>4}v", n), &polys, batch, &acfg);
        }

        {
            let poly = make_bowtie();
            let polys: Vec<Polygon<f64>> = (0..5000)
                .map(|i| {
                    let mut p = poly.clone();
                    p.exterior_mut(|ext| {
                        for c in &mut ext.0 {
                            c.x += i as f64 * 20.0;
                            c.y += i as f64 * 20.0;
                        }
                    });
                    p
                })
                .collect();
            bench_polygons("arrange bowtie 4v", &polys, 5000, &acfg);
        }
        {
            let poly = make_bowtie_n(100);
            let polys: Vec<Polygon<f64>> = (0..500)
                .map(|i| {
                    let mut p = poly.clone();
                    p.exterior_mut(|ext| {
                        for c in &mut ext.0 {
                            c.x += i as f64 * 20.0;
                            c.y += i as f64 * 20.0;
                        }
                    });
                    p
                })
                .collect();
            bench_polygons("arrange bowtie 100v", &polys, 500, &acfg);
        }

        // Star polygon through Arrange (challenging for CDT)
        for &(spikes, batch) in &[(10usize, 5000usize), (50, 500), (100, 200), (500, 50)] {
            let g = make_starburst(spikes, 1000.0);
            bench_line(&format!("arrange star {:>3}sp", spikes), &g, batch, &acfg);
        }
    }
}

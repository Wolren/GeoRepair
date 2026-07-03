//! Internal function stress: test through public API with inputs crafted
//! to exercise internal function code paths at extreme fp values.
//!
//! Strategy: create geometries whose repair would exercise each internal
//! function with the most extreme coordinates possible, then verify no
//! panic occurs. This tests the building blocks indirectly via the
//! pipeline, which is the strongest guarantee of correctness.

use geo::{Coord, Geometry, LineString, MultiPolygon, Point, Polygon};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::panic::{self, AssertUnwindSafe};

const FP_SPECIALS: &[(f64, f64, &str)] = &[
    (f64::NAN, f64::NAN, "nan"),
    (f64::INFINITY, f64::INFINITY, "inf"),
    (f64::NEG_INFINITY, f64::NEG_INFINITY, "neginf"),
    (f64::MAX, f64::MAX, "max"),
    (f64::MIN, f64::MIN, "min"),
    (f64::MIN_POSITIVE, -f64::MIN_POSITIVE, "sub"),
    (1e300, -1e300, "1e300"),
    (-1e300, 1e300, "neg1e300"),
    (0.0, 0.0, "zero"),
    (f64::EPSILON, -f64::EPSILON, "eps"),
];

fn assert_no_panic<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
    let r = panic::catch_unwind(AssertUnwindSafe(f));
    assert!(r.is_ok(), "PANIC detected! Fix the pipeline.");
}

fn cfg_all() -> Vec<MakeValidConfig> {
    vec![
        MakeValidConfig::default(),
        MakeValidConfig { keep_collapsed: true, ..Default::default() },
        MakeValidConfig { poly_method: PolyMethod::Arrange, ..Default::default() },
        MakeValidConfig { poly_method: PolyMethod::Structure, ..Default::default() },
    ]
}

// ========================================================================
// 1. ring_dup_fingerprint stress (tested through Polygon validation
//    which calls this function for duplicate ring detection)
// ========================================================================

#[test]
fn stress_ring_dup_detection_empty() {
    // Empty polygon should not panic the duplicate ring detector
    for cfg in &cfg_all() {
        assert_no_panic(move || {
            let poly = Polygon::new(LineString::new(Vec::new()), Vec::new());
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn stress_ring_dup_at_extreme_coords() {
    for &(x, y, _name) in FP_SPECIALS {
        for cfg in &cfg_all() {
            assert_no_panic(move || {
                // Two shells that are identical — triggers duplicate detection
                let shell = LineString::new(vec![
                    Coord { x, y },
                    Coord { x: x + 10.0, y },
                    Coord { x: x + 10.0, y: y + 10.0 },
                    Coord { x, y: y + 10.0 },
                    Coord { x, y },
                ]);
                let mp = MultiPolygon::new(vec![
                    Polygon::new(shell.clone(), Vec::new()),
                    Polygon::new(shell, Vec::new()),
                ]);
                let _ = mp.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 2. point_in_ring_exclusive stress (tested through validator's
//    hole containment check and repair's vertex-inside check)
// ========================================================================

#[test]
fn stress_hole_containment_at_extreme() {
    for &(x, y, _name) in FP_SPECIALS {
        for cfg in &cfg_all() {
            assert_no_panic(move || {
                // Shell at extreme + hole also at extreme — triggers containment check
                let poly = Polygon::new(
                    LineString::new(vec![
                        Coord { x, y },
                        Coord { x: x + 100.0, y },
                        Coord { x: x + 100.0, y: y + 100.0 },
                        Coord { x, y: y + 100.0 },
                        Coord { x, y },
                    ]),
                    vec![LineString::new(vec![
                        Coord { x: x + 10.0, y: y + 10.0 },
                        Coord { x: x + 40.0, y: y + 10.0 },
                        Coord { x: x + 40.0, y: y + 40.0 },
                        Coord { x: x + 10.0, y: y + 40.0 },
                        Coord { x: x + 10.0, y: y + 10.0 },
                    ])],
                );
                let _ = poly.make_valid_with_config(cfg);
            });
        }
    }
}

#[test]
fn stress_hole_hole_overlap_at_extreme() {
    // Two holes that overlap — triggers hole-hole intersection check
    for &(x, y, _name) in FP_SPECIALS {
        for cfg in &cfg_all() {
            assert_no_panic(move || {
                let poly = Polygon::new(
                    LineString::new(vec![
                        Coord { x, y },
                        Coord { x: x + 100.0, y },
                        Coord { x: x + 100.0, y: y + 100.0 },
                        Coord { x, y: y + 100.0 },
                        Coord { x, y },
                    ]),
                    vec![
                        LineString::new(vec![
                            Coord { x: x + 10.0, y: y + 10.0 },
                            Coord { x: x + 60.0, y: y + 10.0 },
                            Coord { x: x + 60.0, y: y + 60.0 },
                            Coord { x: x + 10.0, y: y + 60.0 },
                            Coord { x: x + 10.0, y: y + 10.0 },
                        ]),
                        LineString::new(vec![
                            Coord { x: x + 30.0, y: y + 30.0 },
                            Coord { x: x + 80.0, y: y + 30.0 },
                            Coord { x: x + 80.0, y: y + 80.0 },
                            Coord { x: x + 30.0, y: y + 80.0 },
                            Coord { x: x + 30.0, y: y + 30.0 },
                        ]),
                    ],
                );
                let _ = poly.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 3. orient2d stress (tested through bowtie repair which exercises
//    orientation predicates at extreme coordinate ratios)
// ========================================================================

#[test]
fn stress_orient2d_at_extreme_ratio() {
    // Coordinates where orient2d may have precision issues due to
    // extreme magnitude differences between coordinates
    for &(x, y, _name) in FP_SPECIALS {
        for cfg in &cfg_all() {
            assert_no_panic(move || {
                // Bowtie with extreme coordinates — forces orient2d in
                // segment intersection detection
                let poly = Polygon::new(
                    LineString::new(vec![
                        Coord { x, y },
                        Coord { x: x + 10.0, y: y + 10.0 },
                        Coord { x: x + 10.0, y },
                        Coord { x, y: y + 10.0 },
                        Coord { x, y },
                    ]),
                    Vec::new(),
                );
                let _ = poly.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 4. MultiPolygon bbox overlap + vertex containment at extreme
// ========================================================================

#[test]
fn stress_bbox_vertex_at_extreme_ratio() {
    // Two polygons with extreme ratio between their coordinate magnitudes.
    // The bbox check may overflow when computing min/max.
    for cfg in &cfg_all() {
        assert_no_panic(move || {
            let p1 = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::MAX, y: f64::MAX },
                    Coord { x: f64::MAX, y: f64::MAX - 100.0 },
                    Coord { x: f64::MAX - 100.0, y: f64::MAX - 100.0 },
                    Coord { x: f64::MAX - 100.0, y: f64::MAX },
                    Coord { x: f64::MAX, y: f64::MAX },
                ]),
                Vec::new(),
            );
            let p2 = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::MIN, y: f64::MIN },
                    Coord { x: f64::MIN, y: f64::MIN + 0.001 },
                    Coord { x: f64::MIN + 0.001, y: f64::MIN + 0.001 },
                    Coord { x: f64::MIN + 0.001, y: f64::MIN },
                    Coord { x: f64::MIN, y: f64::MIN },
                ]),
                Vec::new(),
            );
            let mp = MultiPolygon::new(vec![p1, p2]);
            let _ = mp.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 5. Massive geometry with alternating extreme/normal vertices
// ========================================================================

#[test]
fn stress_alternating_extreme_vertices() {
    // A large ring where every other vertex is f64::MAX vs 0.0, creating
    // extreme coordinate differences between adjacent vertices
    let mut coords = Vec::new();
    for i in 0..50 {
        if i % 2 == 0 {
            coords.push(Coord { x: f64::MAX - i as f64, y: f64::MAX - i as f64 });
        } else {
            coords.push(Coord { x: 0.0, y: 0.0 });
        }
    }
    coords.push(coords[0]);
    let poly = Polygon::new(LineString::new(coords), Vec::new());
    for cfg in &cfg_all() {
        let poly = poly.clone();
        assert_no_panic(move || {
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 6. Ring where NaN filtering leaves exactly 2 coords (edge case for
//    point_in_ring_exclusive)
// ========================================================================

#[test]
fn stress_nan_filter_leaves_two_coords() {
    // Ring: valid, NaN, NaN, valid — after NaN filter: 2 coords
    // This specifically exercises the n < 2 guard in point_in_ring_exclusive
    for cfg in &cfg_all() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: 10.0, y: 10.0 },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 7. MultiPolygon where unary_union would have extreme input
// ========================================================================

#[test]
fn stress_unary_union_at_extreme() {
    // Two overlapping polygons at extreme coordinates — forces unary_union
    // which internally uses orient2d and segment intersection
    for cfg in &cfg_all() {
        assert_no_panic(move || {
            let p1 = Polygon::new(
                LineString::new(vec![
                    Coord { x: 1e200, y: 1e200 },
                    Coord { x: 1e200 + 100.0, y: 1e200 },
                    Coord { x: 1e200 + 100.0, y: 1e200 + 100.0 },
                    Coord { x: 1e200, y: 1e200 + 100.0 },
                    Coord { x: 1e200, y: 1e200 },
                ]),
                Vec::new(),
            );
            let p2 = Polygon::new(
                LineString::new(vec![
                    Coord { x: 1e200 + 50.0, y: 1e200 + 50.0 },
                    Coord { x: 1e200 + 150.0, y: 1e200 + 50.0 },
                    Coord { x: 1e200 + 150.0, y: 1e200 + 150.0 },
                    Coord { x: 1e200 + 50.0, y: 1e200 + 150.0 },
                    Coord { x: 1e200 + 50.0, y: 1e200 + 50.0 },
                ]),
                Vec::new(),
            );
            let mp = MultiPolygon::new(vec![p1, p2]);
            let _ = mp.make_valid_with_config(cfg);
        });
    }
}

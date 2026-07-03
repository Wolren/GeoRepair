//! Panic stress tests: push every geometry type and every algorithm path
//! to its absolute fp limits. The goal is to prove that no input, no matter
//! how degenerate, can cause a Rust panic.
//!
//! Test matrix:
//! - 8 geometry types × 7 fp classes × 4 configs = 224 base cases
//! - Plus combinatorial stress: mixed fp classes in same geometry
//! - Plus degenerate ring structures (identified as panic-prone during dev)
//! - Plus massive coordinate ranges (overflow stress)
//!
//! Each test catches panics with `std::panic::catch_unwind`.
//! A panic is treated as a FAILURE — the library must never panic.

use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::panic::{self, AssertUnwindSafe};

// ========================================================================
// Panic-catch helper
// ========================================================================

/// Assert that an expression does NOT panic.
fn assert_no_panic<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
    let r = panic::catch_unwind(AssertUnwindSafe(f));
    assert!(r.is_ok(), "PANIC detected! Fix the pipeline.");
}

// ========================================================================
// FP class generators
// ========================================================================

/// All the fp value classes that could cause panics.
struct FpClass {
    x: f64,
    y: f64,
}

const FP_CLASSES: &[FpClass] = &[
    FpClass { x: f64::NAN, y: f64::NAN },
    FpClass { x: f64::INFINITY, y: f64::INFINITY },
    FpClass { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY },
    FpClass { x: f64::MAX, y: f64::MAX },
    FpClass { x: f64::MIN, y: f64::MIN },
    FpClass { x: f64::MIN_POSITIVE, y: f64::MIN_POSITIVE * 2.0 },
    FpClass { x: -f64::MIN_POSITIVE, y: -f64::MIN_POSITIVE * 2.0 },
    FpClass { x: 1e15, y: 1e-15 },
    FpClass { x: 1e-15, y: 1e15 },
    FpClass { x: 0.0, y: 0.0 },
    FpClass { x: f64::EPSILON, y: -f64::EPSILON },
    FpClass { x: -1e300, y: -1e300 },
    FpClass { x: 1e300, y: 1e300 },
];

fn all_configs() -> Vec<MakeValidConfig> {
    let auto = MakeValidConfig::default();
    let auto_keep = MakeValidConfig { keep_collapsed: true, ..Default::default() };
    let arrange = MakeValidConfig { poly_method: PolyMethod::Arrange, ..Default::default() };
    let structure = MakeValidConfig { poly_method: PolyMethod::Structure, ..Default::default() };
    vec![auto, auto_keep, arrange, structure]
}

// ========================================================================
// 1. Point with extreme fp values
// ========================================================================

#[test]
fn panic_point_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let pt = Point::new(fp.x, fp.y);
                let _ = pt.make_valid_with_config(cfg);
            });
        }
    }
}

#[test]
fn panic_point_mixed_coords() {
    // x extreme, y normal
    for &x in &[f64::NAN, f64::INFINITY, f64::MAX, f64::MIN] {
        for &y in &[-1e6, 0.0, 1e6] {
            for cfg in &all_configs() {
                assert_no_panic(move || {
                    let pt = Point::new(x, y);
                    let _ = pt.make_valid_with_config(cfg);
                });
            }
        }
    }
}

// ========================================================================
// 2. Line with extreme fp values
// ========================================================================

#[test]
fn panic_line_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let line = Line::new(Coord { x: fp.x, y: fp.y }, Coord { x: fp.y, y: fp.x });
                let _ = line.make_valid_with_config(cfg);
            });
        }
    }
}

#[test]
fn panic_line_zero_length_with_extreme() {
    for &x in &[f64::NAN, f64::INFINITY, f64::MAX, f64::MIN, 0.0, 1e300] {
        let cfg = MakeValidConfig::default();
        assert_no_panic(move || {
            let line = Line::new(Coord { x, y: x }, Coord { x, y: x });
            let _ = line.make_valid_with_config(&cfg);
        });
    }
}

// ========================================================================
// 3. LineString with extreme fp values
// ========================================================================

fn make_ls_with(fp: &FpClass, n: usize) -> LineString<f64> {
    let coords: Vec<Coord<f64>> = (0..n)
        .map(|i| {
            let t = i as f64;
            Coord { x: fp.x + t, y: fp.y - t }
        })
        .collect();
    LineString::new(coords)
}

#[test]
fn panic_linestring_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let ls = make_ls_with(fp, 5);
                let _ = ls.make_valid_with_config(cfg);
            });
        }
    }
}

#[test]
fn panic_linestring_single_point_all_fp() {
    for fp in FP_CLASSES {
        let cfg = MakeValidConfig::default();
        assert_no_panic(move || {
            let ls = LineString::new(vec![Coord { x: fp.x, y: fp.y }]);
            let _ = ls.make_valid_with_config(&cfg);
        });
    }
}

#[test]
fn panic_linestring_two_points_all_fp() {
    for fp in FP_CLASSES {
        let cfg = MakeValidConfig::default();
        assert_no_panic(move || {
            let ls = LineString::new(vec![
                Coord { x: fp.x, y: fp.y },
                Coord { x: fp.y, y: fp.x },
            ]);
            let _ = ls.make_valid_with_config(&cfg);
        });
    }
}

// ========================================================================
// 4. Polygon with extreme fp values
// ========================================================================

fn make_poly_with(fp: &FpClass, n: usize) -> Polygon<f64> {
    let coords: Vec<Coord<f64>> = (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            Coord {
                x: fp.x + a.cos() * 10.0,
                y: fp.y + a.sin() * 10.0,
            }
        })
        .collect();
    let mut ring = coords;
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    Polygon::new(LineString::new(ring), Vec::new())
}

#[test]
fn panic_polygon_all_fp_classes() {
    for fp in FP_CLASSES {
        for n in [3, 4, 10] {
            for cfg in &all_configs() {
                assert_no_panic(move || {
                    let poly = make_poly_with(fp, n);
                    let _ = poly.make_valid_with_config(cfg);
                });
            }
        }
    }
}

#[test]
fn panic_polygon_all_nan_hole() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 }, Coord { x: 100.0, y: 0.0 },
                    Coord { x: 100.0, y: 100.0 }, Coord { x: 0.0, y: 100.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                vec![LineString::new(vec![
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                ])],
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn panic_polygon_inf_hole() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 }, Coord { x: 100.0, y: 0.0 },
                    Coord { x: 100.0, y: 100.0 }, Coord { x: 0.0, y: 100.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                vec![LineString::new(vec![
                    Coord { x: f64::INFINITY, y: f64::INFINITY },
                    Coord { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY },
                    Coord { x: f64::INFINITY, y: f64::NEG_INFINITY },
                ])],
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn panic_polygon_max_coords_multiple_holes() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let holes: Vec<LineString<f64>> = (0..5)
                .map(|i| {
                    let base = if i % 2 == 0 { f64::MAX } else { f64::MIN };
                    LineString::new(vec![
                        Coord { x: base + i as f64, y: base - i as f64 },
                        Coord { x: base - i as f64, y: base + i as f64 },
                        Coord { x: base + i as f64 * 2.0, y: base - i as f64 * 2.0 },
                        Coord { x: base + i as f64, y: base - i as f64 },
                    ])
                })
                .collect();
            let shell = LineString::new(vec![
                Coord { x: f64::MAX, y: f64::MAX },
                Coord { x: -f64::MAX, y: f64::MAX },
                Coord { x: -f64::MAX, y: -f64::MAX },
                Coord { x: f64::MAX, y: -f64::MAX },
                Coord { x: f64::MAX, y: f64::MAX },
            ]);
            let poly = Polygon::new(shell, holes);
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 5. MultiPoint with extreme fp
// ========================================================================

#[test]
fn panic_multipoint_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let mp = MultiPoint::new(vec![
                    Point::new(fp.x, fp.y),
                    Point::new(fp.y, fp.x),
                    Point::new(fp.x * 0.5, fp.y * 0.5),
                ]);
                let _ = mp.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 6. MultiLineString with extreme fp
// ========================================================================

#[test]
fn panic_multilinestring_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let mls = MultiLineString::new(vec![
                    make_ls_with(fp, 3),
                    make_ls_with(fp, 7),
                ]);
                let _ = mls.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 7. MultiPolygon with extreme fp
// ========================================================================

#[test]
fn panic_multipolygon_all_fp_classes() {
    for fp in FP_CLASSES {
        for n in [3, 6] {
            for cfg in &all_configs() {
                assert_no_panic(move || {
                    let mp = MultiPolygon::new(vec![
                        make_poly_with(fp, n),
                        make_poly_with(fp, n + 1),
                    ]);
                    let _ = mp.make_valid_with_config(cfg);
                });
            }
        }
    }
}

#[test]
fn panic_multipolygon_extreme_bbox_overlap() {
    // Two polygons at f64::MAX that have overlapping bboxes
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let mp = MultiPolygon::new(vec![
                Polygon::new(
                    LineString::new(vec![
                        Coord { x: f64::MAX, y: f64::MAX },
                        Coord { x: f64::MAX, y: f64::MAX - 100.0 },
                        Coord { x: f64::MAX - 100.0, y: f64::MAX - 100.0 },
                        Coord { x: f64::MAX, y: f64::MAX },
                    ]),
                    Vec::new(),
                ),
                Polygon::new(
                    LineString::new(vec![
                        Coord { x: f64::MAX, y: f64::MAX },
                        Coord { x: f64::MAX - 100.0, y: f64::MAX },
                        Coord { x: f64::MAX - 100.0, y: f64::MAX - 100.0 },
                        Coord { x: f64::MAX, y: f64::MAX },
                    ]),
                    Vec::new(),
                ),
            ]);
            let _ = mp.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 8. GeometryCollection with extreme fp
// ========================================================================

#[test]
fn panic_gc_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let gc = GeometryCollection(vec![
                    Geometry::Point(Point::new(fp.x, fp.y)),
                    Geometry::LineString(make_ls_with(fp, 4)),
                    Geometry::Polygon(make_poly_with(fp, 4)),
                ]);
                let _ = gc.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 9. Rect and Triangle with extreme fp
// ========================================================================

#[test]
fn panic_rect_all_fp_classes() {
    for fp in FP_CLASSES {
        let cfg = MakeValidConfig::default();
        assert_no_panic(move || {
            let r = Rect::new(
                Coord { x: fp.x, y: fp.y },
                Coord { x: fp.x + 10.0, y: fp.y + 10.0 },
            );
            let _ = r.make_valid_with_config(&cfg);
        });
    }
}

#[test]
fn panic_triangle_all_fp_classes() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let tri = Triangle::new(
                    Coord { x: fp.x, y: fp.y },
                    Coord { x: fp.x + 10.0, y: fp.y },
                    Coord { x: fp.x, y: fp.y + 10.0 },
                );
                let _ = tri.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 10. Degenerate rings (structural panic sources)
// ========================================================================

#[test]
fn panic_ring_with_only_nans_removed_to_empty() {
    // Ring where all coords are NaN — after filtering, < 3 coords remain
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn panic_ring_with_mixed_nan_and_valid() {
    // Ring with NaNs interleaved — filtering leaves 2 or fewer coords
    let patterns = [
        // 3 coords: valid, NaN, valid → after filter: 2 coords (edges)
        vec![Coord { x: 0.0, y: 0.0 }, Coord { x: f64::NAN, y: f64::NAN }, Coord { x: 10.0, y: 10.0 }],
        // 5 coords with valid at positions 0 and 4 only
        vec![
            Coord { x: 0.0, y: 0.0 }, Coord { x: f64::NAN, y: f64::NAN },
            Coord { x: f64::NAN, y: f64::NAN }, Coord { x: f64::NAN, y: f64::NAN },
            Coord { x: 10.0, y: 10.0 },
        ],
    ];
    for pattern in &patterns {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let poly = Polygon::new(LineString::new(pattern.clone()), Vec::new());
                let _ = poly.make_valid_with_config(cfg);
            });
        }
    }
}

#[test]
fn panic_ring_with_inf_and_neg_inf() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::INFINITY, y: f64::NEG_INFINITY },
                    Coord { x: f64::NEG_INFINITY, y: f64::INFINITY },
                    Coord { x: f64::INFINITY, y: f64::INFINITY },
                    Coord { x: f64::INFINITY, y: f64::NEG_INFINITY },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn panic_ring_with_f64_max_overflow_ops() {
    // Values that might overflow when subtracted or added
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::MAX, y: 0.0 },
                    Coord { x: 0.0, y: f64::MIN },
                    Coord { x: f64::MIN, y: 0.0 },
                    Coord { x: 0.0, y: f64::MAX },
                    Coord { x: f64::MAX, y: 0.0 },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 11. Combinatorial stress: mixed fp classes in same geometry
// ========================================================================

#[test]
fn panic_polygon_mixed_fp_classes_in_same_ring() {
    // Each vertex is a different fp class
    let vertices = [
        ("nan+nan", Coord { x: f64::NAN, y: f64::NAN }),
        ("inf+inf", Coord { x: f64::INFINITY, y: f64::INFINITY }),
        ("max+min", Coord { x: f64::MAX, y: f64::MIN }),
        ("zero+eps", Coord { x: 0.0, y: f64::EPSILON }),
        ("large+small", Coord { x: 1e300, y: 1e-300 }),
        ("neg+pos", Coord { x: -1e200, y: 1e200 }),
        ("subnormal+subnormal", Coord { x: f64::MIN_POSITIVE, y: f64::MIN_POSITIVE * 2.0 }),
    ];
    // Test each vertex as the first vertex (can trigger different paths)
    for i in 0..vertices.len() {
        for cfg in &all_configs() {
            let verts = vertices.to_vec();
            assert_no_panic(move || {
                let rotated: Vec<Coord<f64>> = verts.iter().cycle().skip(i).take(4).map(|(_, c)| *c).collect();
                let poly = Polygon::new(LineString::new(rotated), Vec::new());
                let _ = poly.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 12. Empty geometry types — indexing panics
// ========================================================================

#[test]
fn panic_empty_all_types() {
    let empties: Vec<Geometry<f64>> = vec![
        Geometry::Point(Point::new(f64::NAN, f64::NAN)),
        Geometry::Line(Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 0.0, y: 0.0 })),
        Geometry::LineString(LineString::new(Vec::new())),
        Geometry::Polygon(Polygon::new(LineString::new(Vec::new()), Vec::new())),
        Geometry::MultiPoint(MultiPoint::new(Vec::new())),
        Geometry::MultiLineString(MultiLineString::new(Vec::new())),
        Geometry::MultiPolygon(MultiPolygon::new(Vec::new())),
        Geometry::GeometryCollection(GeometryCollection(Vec::new())),
    ];
    for g in &empties {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let _ = g.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 13. Polygon with hole that becomes degenerate after NaN removal
// ========================================================================

#[test]
fn panic_hole_collapses_after_nan_removal() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 },
                    Coord { x: 10.0, y: 10.0 }, Coord { x: 0.0, y: 10.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                vec![LineString::new(vec![
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: f64::NAN, y: f64::NAN },
                ])],
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 14. Very large coordinate values with additive/subtractive ops
// ========================================================================

#[test]
fn panic_f64_max_subtraction_chain() {
    // Coordinates near f64::MAX that involve subtraction in orient2d
    let cfg = MakeValidConfig::default();
    assert_no_panic(move || {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: f64::MAX - 1.0e200, y: f64::MAX - 1.0e200 },
                Coord { x: f64::MAX, y: f64::MAX - 1.0e200 },
                Coord { x: f64::MAX, y: f64::MAX },
                Coord { x: f64::MAX - 1.0e200, y: f64::MAX },
                Coord { x: f64::MAX - 1.0e200, y: f64::MAX - 1.0e200 },
            ]),
            Vec::new(),
        );
        let _ = poly.make_valid_with_config(&cfg);
    });
}

// ========================================================================
// 15. Coordinates at exactly 0, 0 with extreme second coordinate
// ========================================================================

#[test]
fn panic_origin_to_extreme() {
    for &x in &[f64::MAX, f64::MIN, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 1e300, -1e300] {
        for &y in &[f64::MAX, f64::MIN, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 1e300, -1e300] {
            for cfg in &all_configs() {
                assert_no_panic(move || {
                    let poly = Polygon::new(
                        LineString::new(vec![
                            Coord { x: 0.0, y: 0.0 },
                            Coord { x, y: 0.0 },
                            Coord { x: 0.0, y },
                            Coord { x: 0.0, y: 0.0 },
                        ]),
                        Vec::new(),
                    );
                    let _ = poly.make_valid_with_config(cfg);
                });
            }
        }
    }
}

// ========================================================================
// 16. Saturated subtraction: x - y with both near f64::MAX
// ========================================================================

#[test]
fn panic_near_max_subtraction() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::MAX, y: f64::MAX },
                    Coord { x: f64::MAX - 1e150, y: f64::MAX - 2e150 },
                    Coord { x: f64::MAX - 3e150, y: f64::MAX - 4e150 },
                    Coord { x: f64::MAX, y: f64::MAX },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 17. All three coords identical but at extreme fp
// ========================================================================

#[test]
fn panic_single_vertex_at_extreme() {
    for &val in &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN, 0.0, 1e300, -1e300] {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let poly = Polygon::new(
                    LineString::new(vec![Coord { x: val, y: val }; 4]),
                    Vec::new(),
                );
                let _ = poly.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 18. CDT-specific panic tests (known to crash spade)
// ========================================================================

#[test]
fn panic_cdt_all_collinear() {
    // CDT can panic on all-collinear input
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 },
                    Coord { x: 50.0, y: 0.0 },
                    Coord { x: 100.0, y: 0.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn panic_cdt_all_collinear_with_nan() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::NAN, y: f64::NAN },
                    Coord { x: 50.0, y: 0.0 },
                    Coord { x: 100.0, y: 0.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

#[test]
fn panic_cdt_extreme_collinear() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord { x: f64::MAX, y: f64::MAX },
                    Coord { x: f64::MAX + 1e150, y: f64::MAX + 1e150 },
                    Coord { x: f64::MAX + 2e150, y: f64::MAX + 2e150 },
                    Coord { x: f64::MAX, y: f64::MAX },
                ]),
                Vec::new(),
            );
            let _ = poly.make_valid_with_config(cfg);
        });
    }
}

// ========================================================================
// 19. ValidateOrFix panic stress
// ========================================================================

#[test]
fn panic_validate_or_fix_all_extreme() {
    use geo_repair::ValidateAndFix;
    for fp in FP_CLASSES {
        assert_no_panic(move || {
            let poly = make_poly_with(fp, 4);
            let _ = poly.validate_or_fix();
        });
    }
}

// ========================================================================
// 20. Massive ring size with degenerate coordinates
// ========================================================================

#[test]
fn panic_massive_ring_1000_vertices_all_nan() {
    let cfg = MakeValidConfig::default();
    assert_no_panic(move || {
        let coords = vec![Coord { x: f64::NAN, y: f64::NAN }; 1000];
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        let _ = poly.make_valid_with_config(&cfg);
    });
}

#[test]
fn panic_massive_ring_1000_vertices_all_max() {
    let cfg = MakeValidConfig::default();
    assert_no_panic(move || {
        let coords = vec![Coord { x: f64::MAX, y: f64::MAX }; 1000];
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        let _ = poly.make_valid_with_config(&cfg);
    });
}

#[test]
fn panic_massive_ring_1000_vertices_mixed_extreme() {
    let cfg = MakeValidConfig::default();
    assert_no_panic(move || {
        let coords: Vec<Coord<f64>> = (0..1000)
            .map(|i| {
                if i % 2 == 0 {
                    Coord { x: f64::MAX - i as f64, y: f64::MIN + i as f64 }
                } else {
                    Coord { x: f64::NAN, y: f64::NAN }
                }
            })
            .collect();
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        let _ = poly.make_valid_with_config(&cfg);
    });
}

// ========================================================================
// 20. Cross-module stress: GC with deeply nested extreme coords
// ========================================================================

#[test]
fn panic_gc_nested_deep_with_extreme() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                // Build GC with 5 levels of nesting, each containing extreme coords
                let mut gc = Geometry::Point(Point::new(fp.x, fp.y));
                for _ in 0..5 {
                    gc = Geometry::GeometryCollection(GeometryCollection(vec![gc]));
                }
                let _ = gc.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 21. GC with mixed types containing extreme fp values
// ========================================================================

#[test]
fn panic_gc_mixed_types_extreme() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let gc = GeometryCollection(vec![
                    Geometry::Point(Point::new(fp.x, fp.y)),
                    Geometry::Line(Line::new(
                        Coord { x: fp.x, y: fp.y },
                        Coord { x: fp.y, y: fp.x },
                    )),
                    Geometry::LineString(make_ls_with(fp, 5)),
                    Geometry::Polygon(make_poly_with(fp, 4)),
                    Geometry::MultiPoint(MultiPoint::new(vec![
                        Point::new(fp.x, fp.y), Point::new(fp.y, fp.x),
                    ])),
                    Geometry::MultiLineString(MultiLineString::new(vec![
                        make_ls_with(fp, 3), make_ls_with(fp, 4),
                    ])),
                    Geometry::MultiPolygon(MultiPolygon::new(vec![
                        make_poly_with(fp, 3), make_poly_with(fp, 4),
                    ])),
                ]);
                let _ = gc.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 22. ValidateOrFix on multiple geometry types with extreme fp
// ========================================================================

#[test]
fn panic_validate_or_fix_all_types_extreme() {
    use geo_repair::ValidateAndFix;
    for fp in FP_CLASSES {
        assert_no_panic(move || {
            let pt = Point::new(fp.x, fp.y);
            let _ = pt.validate_or_fix();
        });
        assert_no_panic(move || {
            let ls = make_ls_with(fp, 4);
            let _ = ls.validate_or_fix();
        });
        assert_no_panic(move || {
            let poly = make_poly_with(fp, 4);
            let _ = poly.validate_or_fix();
        });
        assert_no_panic(move || {
            let mp = MultiPolygon::new(vec![make_poly_with(fp, 3), make_poly_with(fp, 4)]);
            let _ = mp.validate_or_fix();
        });
        assert_no_panic(move || {
            let mls = MultiLineString::new(vec![make_ls_with(fp, 3), make_ls_with(fp, 5)]);
            let _ = mls.validate_or_fix();
        });
    }
}

// ========================================================================
// 23. MultiPolygon extreme overlapping shells with fp extremes
// ========================================================================

#[test]
fn panic_mp_extreme_overlap() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let mp = MultiPolygon::new(vec![
                    Polygon::new(
                        LineString::new(vec![
                            Coord { x: fp.x, y: fp.y },
                            Coord { x: fp.x + 10.0, y: fp.y },
                            Coord { x: fp.x + 10.0, y: fp.y + 10.0 },
                            Coord { x: fp.x, y: fp.y + 10.0 },
                            Coord { x: fp.x, y: fp.y },
                        ]),
                        Vec::new(),
                    ),
                    Polygon::new(
                        LineString::new(vec![
                            Coord { x: fp.x + 5.0, y: fp.y + 5.0 },
                            Coord { x: fp.x + 15.0, y: fp.y + 5.0 },
                            Coord { x: fp.x + 15.0, y: fp.y + 15.0 },
                            Coord { x: fp.x + 5.0, y: fp.y + 15.0 },
                            Coord { x: fp.x + 5.0, y: fp.y + 5.0 },
                        ]),
                        Vec::new(),
                    ),
                ]);
                let _ = mp.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 24. Rect and Triangle with invalid/extreme fp dimensions
// ========================================================================

#[test]
fn panic_rect_triangle_extreme() {
    for fp in FP_CLASSES {
        for cfg in &all_configs() {
            assert_no_panic(move || {
                let r = Rect::new(
                    Coord { x: fp.x, y: fp.y },
                    Coord { x: fp.x + 10.0, y: fp.y + 10.0 },
                );
                let _ = r.make_valid_with_config(cfg);
            });
            assert_no_panic(move || {
                let tri = Triangle::new(
                    Coord { x: fp.x, y: fp.y },
                    Coord { x: fp.x + 10.0, y: fp.y },
                    Coord { x: fp.x, y: fp.y + 10.0 },
                );
                let _ = tri.make_valid_with_config(cfg);
            });
        }
    }
}

// ========================================================================
// 25. Empty GC with all-mixed-empty sub-geometries
// ========================================================================

#[test]
fn panic_gc_all_empty_types() {
    for cfg in &all_configs() {
        assert_no_panic(move || {
            let gc = GeometryCollection(vec![
                Geometry::Point(Point::new(f64::NAN, f64::NAN)),
                Geometry::LineString(LineString::new(Vec::new())),
                Geometry::Polygon(Polygon::new(LineString::new(Vec::new()), Vec::new())),
                Geometry::MultiPoint(MultiPoint::new(Vec::new())),
                Geometry::MultiLineString(MultiLineString::new(Vec::new())),
                Geometry::MultiPolygon(MultiPolygon::new(Vec::new())),
                Geometry::GeometryCollection(GeometryCollection(Vec::new())),
            ]);
            let _ = gc.make_valid_with_config(cfg);
        });
    }
}

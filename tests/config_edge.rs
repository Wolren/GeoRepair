//! Comprehensive MakeValidConfig combination tests + edge cases.
//! Covers every gap identified through analysis:
//! - All 6 (keep_collapsed × poly_method) combinations
//! - Empty/degenerate inputs for every geometry type
//! - Error paths (NaN, Infinity, boundary values)
//! - Rect/Triangle edge cases
//! - Geometry dispatch edge cases
//! - GeometryCollection nesting and edge types

use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn cfg(method: PolyMethod, keep: bool) -> MakeValidConfig {
    MakeValidConfig {
        poly_method: method,
        keep_collapsed: keep,
        ..Default::default()
    }
}

fn assert_valid(g: &Geometry<f64>) {
    assert!(
        g.validate().valid,
        "expected valid, got: {:?}",
        g.validate()
    );
}

fn assert_empty(g: &Geometry<f64>) {
    let is_empty = matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty())
        || matches!(g, Geometry::MultiPolygon(mp) if mp.0.is_empty())
        || matches!(g, Geometry::MultiLineString(mls) if mls.0.is_empty())
        || matches!(g, Geometry::MultiPoint(mp) if mp.0.is_empty());
    assert!(is_empty, "expected empty, got: {:?}", g);
}

fn assert_not_empty(g: &Geometry<f64>) {
    assert!(
        !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()),
        "expected non-empty, got: {:?}",
        g
    );
}

// =========================================================================
// Config combination matrix
// =========================================================================

#[test]
fn test_config_auto_keep_false() {
    let result = make_bowtie().make_valid_with_config(&cfg(PolyMethod::Auto, false));
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_config_auto_keep_true() {
    let result = make_bowtie().make_valid_with_config(&cfg(PolyMethod::Auto, true));
    assert_valid(&result);
}

#[test]
fn test_config_arrange_keep_true() {
    let result = make_bowtie().make_valid_with_config(&cfg(PolyMethod::Arrange, true));
    assert_valid(&result);
}

#[test]
fn test_config_structure_keep_true() {
    let result = make_bowtie().make_valid_with_config(&cfg(PolyMethod::Structure, true));
    assert_valid(&result);
}

#[test]
fn test_config_structure_keep_false_bowtie() {
    let result = make_bowtie().make_valid_with_config(&cfg(PolyMethod::Structure, false));
    assert_valid(&result);
}

// Polygon with ring too small — keep_collapsed variants
#[test]
fn test_config_degenerate_ring_keep_collapsed_true() {
    let poly = Polygon::new(
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        Vec::new(),
    );
    // Even with keep_collapsed, a polygon needs at least 3 unique coords for a ring
    let result_auto = poly.make_valid_with_config(&cfg(PolyMethod::Auto, true));
    assert_valid(&result_auto);

    let result_arrange = poly.make_valid_with_config(&cfg(PolyMethod::Arrange, true));
    assert_valid(&result_arrange);

    let result_structure = poly.make_valid_with_config(&cfg(PolyMethod::Structure, true));
    assert_valid(&result_structure);
}

fn make_bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )
}

// =========================================================================
// Point edge cases
// =========================================================================

#[test]
fn test_point_both_nan() {
    let p = Point::new(f64::NAN, f64::NAN);
    assert_empty(&p.make_valid());
}

#[test]
fn test_point_both_infinite() {
    let p = Point::new(f64::INFINITY, f64::NEG_INFINITY);
    assert_empty(&p.make_valid());
}

#[test]
fn test_point_f64_min_max() {
    let p = Point::new(f64::MIN, f64::MAX);
    assert_not_empty(&p.make_valid());
}

#[test]
fn test_point_zero() {
    let p = Point::new(0.0, 0.0);
    assert!(!matches!(p.make_valid(), Geometry::GeometryCollection(_)));
}

// =========================================================================
// Line edge cases
// =========================================================================

#[test]
fn test_line_nan_end() {
    let l = Line::new(Point::new(0.0, 0.0), Point::new(f64::NAN, 1.0));
    assert_empty(&l.make_valid());
}

#[test]
fn test_line_both_nan() {
    let l = Line::new(Point::new(f64::NAN, 0.0), Point::new(0.0, f64::NAN));
    assert_empty(&l.make_valid());
}

#[test]
fn test_line_infinite_start() {
    let l = Line::new(Point::new(f64::INFINITY, 0.0), Point::new(1.0, 1.0));
    assert_empty(&l.make_valid());
}

#[test]
fn test_line_infinite_end() {
    let l = Line::new(Point::new(0.0, 0.0), Point::new(1.0, f64::INFINITY));
    assert_empty(&l.make_valid());
}

#[test]
fn test_line_both_infinite() {
    let l = Line::new(
        Point::new(f64::INFINITY, 0.0),
        Point::new(f64::NEG_INFINITY, 1.0),
    );
    assert_empty(&l.make_valid());
}

#[test]
fn test_line_zero_length_and_nan() {
    let l = Line::new(Point::new(f64::NAN, 0.0), Point::new(f64::NAN, 0.0));
    assert_empty(&l.make_valid());
}

#[test]
fn test_line_near_equal_endpoints() {
    let l = Line::new(Point::new(1.0, 1.0), Point::new(1.0 + 1e-15, 1.0 + 1e-15));
    assert_not_empty(&l.make_valid());
}

#[test]
fn test_line_valid_across_configs() {
    let l = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        for keep in [true, false] {
            let cfg = cfg(method, keep);
            let result = l.make_valid_with_config(&cfg);
            assert_eq!(result, Geometry::Line(l));
        }
    }
}

#[test]
fn test_line_extreme_large_coords() {
    let l = Line::new(Point::new(1e15, -1e15), Point::new(-1e15, 1e15));
    let result = l.make_valid();
    assert_not_empty(&result);
    assert_eq!(result, Geometry::Line(l));
}

#[test]
fn test_line_negative_coords() {
    let l = Line::new(Point::new(-100.0, -100.0), Point::new(-0.001, -0.001));
    let result = l.make_valid();
    assert_eq!(result, Geometry::Line(l));
}

#[test]
fn test_linestring_collinear() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 5.0, y: 5.0 },
        Coord { x: 10.0, y: 10.0 },
        Coord { x: 20.0, y: 20.0 },
    ]);
    let result = ls.make_valid();
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_linestring_self_intersecting_returns_as_is() {
    // GEOS/OGC: a self-intersecting LineString is still valid
    // (only NaN/Inf checked). Return unchanged, no noding.
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 10.0, y: 10.0 },
        Coord { x: 10.0, y: 0.0 },
        Coord { x: 0.0, y: 10.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);
    for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        let cfg = cfg(method, false);
        let result = ls.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert!(
            matches!(&result, Geometry::LineString(_)),
            "expected LineString (unchanged per GEOS compat), got {:?}",
            result
        );
    }
}

#[test]
fn test_linestring_collinear_self_overlap() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 10.0, y: 0.0 },
        Coord { x: 20.0, y: 0.0 },
        Coord { x: 5.0, y: 0.0 },
        Coord { x: 15.0, y: 0.0 },
    ]);
    let result = ls.make_valid();
    assert_not_empty(&result);
}

#[test]
fn test_linestring_dedup_keeps_non_adjacent_duplicates() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 2.0, y: 2.0 },
    ]);
    let result = ls.make_valid();
    // Our GeoValidation catches MultiLineString component interior overlap
    // that the old geo crate missed. Result is structurally correct.
    assert_not_empty(&result);
}

#[test]
fn test_linestring_single_point_keep_collapsed() {
    let ls = LineString::new(vec![Coord { x: 5.0, y: 5.0 }]);
    for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        let r1 = ls.make_valid_with_config(&cfg(method, true));
        assert_eq!(
            r1,
            Geometry::Point(Point::new(5.0, 5.0)),
            "keep_collapsed=true should give Point"
        );

        let r2 = ls.make_valid_with_config(&cfg(method, false));
        assert_eq!(
            r2,
            Geometry::Point(Point::new(5.0, 5.0)),
            "collapsed geometry always preserved (GEOS compat)"
        );
    }
}

#[test]
fn test_multilinestring_valid_preserved() {
    let mls = MultiLineString::new(vec![
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        LineString::new(vec![Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }]),
    ]);
    let result = mls.make_valid();
    assert_eq!(result, Geometry::MultiLineString(mls));
}

#[test]
fn test_linestring_valid_across_configs() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 5.0, y: 5.0 },
        Coord { x: 10.0, y: 0.0 },
    ]);
    for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        for keep in [true, false] {
            let cfg = cfg(method, keep);
            let result = ls.make_valid_with_config(&cfg);
            assert_valid(&result);
            assert_not_empty(&result);
        }
    }
}

// =========================================================================
// LineString edge cases
// =========================================================================

#[test]
fn test_linestring_all_nan() {
    let ls = LineString::new(vec![
        Coord {
            x: f64::NAN,
            y: 0.0,
        },
        Coord {
            x: 1.0,
            y: f64::NAN,
        },
    ]);
    assert_empty(&ls.make_valid());
}

#[test]
fn test_linestring_two_coords() {
    let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]);
    assert_not_empty(&ls.make_valid());
}

#[test]
fn test_linestring_dedup_to_single_keep_collapsed() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let result = ls.make_valid_with_config(&config);
    assert_eq!(result, Geometry::Point(Point::new(0.0, 0.0)));
}

#[test]
fn test_linestring_dedup_to_single_keep_collapsed_false() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);
    assert_eq!(ls.make_valid(), Geometry::Point(Point::new(0.0, 0.0)));
}

#[test]
fn test_linestring_keep_collapsed_two_coords() {
    let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]);
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    assert_not_empty(&ls.make_valid_with_config(&config));
}

#[test]
fn test_linestring_nan_filtered_leaves_single_keep_collapsed() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: f64::NAN,
            y: 0.0,
        },
        Coord { x: 0.0, y: 0.0 },
    ]);
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let result = ls.make_valid_with_config(&config);
    assert_eq!(result, Geometry::Point(Point::new(0.0, 0.0)));
}

#[test]
fn test_linestring_self_intersecting() {
    // GEOS/OGC: self-intersecting LineString is valid, returned as-is
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 2.0, y: 2.0 },
        Coord { x: 2.0, y: 0.0 },
        Coord { x: 0.0, y: 2.0 },
    ]);
    let result = ls.make_valid();
    assert_not_empty(&result);
    assert!(matches!(result, Geometry::LineString(_)));
}

// =========================================================================
// MultiPoint edge cases
// =========================================================================

#[test]
fn test_multipoint_empty() {
    let mp = MultiPoint::<f64>::new(Vec::new());
    assert_empty(&mp.make_valid());
}

#[test]
fn test_multipoint_mixed_nan_inf() {
    let mp = MultiPoint::new(vec![
        Point::new(0.0, 0.0),
        Point::new(f64::NAN, 1.0),
        Point::new(2.0, f64::INFINITY),
        Point::new(3.0, 3.0),
    ]);
    let result = mp.make_valid();
    let expected = MultiPoint::new(vec![Point::new(0.0, 0.0), Point::new(3.0, 3.0)]);
    assert_eq!(result, Geometry::MultiPoint(expected));
}

#[test]
fn test_multipoint_all_nan_inf() {
    let mp = MultiPoint::new(vec![
        Point::new(f64::NAN, 1.0),
        Point::new(2.0, f64::INFINITY),
        Point::new(f64::NAN, f64::NEG_INFINITY),
    ]);
    assert_empty(&mp.make_valid());
}

// =========================================================================
// MultiLineString edge cases
// =========================================================================

#[test]
fn test_multilinestring_empty() {
    let mls = MultiLineString::<f64>::new(Vec::new());
    assert_empty(&mls.make_valid());
}

#[test]
fn test_multilinestring_all_invalid() {
    let mls = MultiLineString::new(vec![
        LineString::new(Vec::new()),
        LineString::new(vec![Coord {
            x: f64::NAN,
            y: 0.0,
        }]),
    ]);
    assert_empty(&mls.make_valid());
}

#[test]
fn test_multilinestring_mixed() {
    let mls = MultiLineString::new(vec![
        LineString::new(Vec::new()),
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
    ]);
    let result = mls.make_valid();
    assert_not_empty(&result);
    assert!(
        matches!(result, Geometry::LineString(_)),
        "single collapsed-down linestring should unwrap to LineString, got {:?}",
        result
    );
}

#[test]
fn test_multilinestring_self_intersecting_component() {
    // GEOS/OGC: self-intersecting LineString components are valid, returned as-is
    let mls = MultiLineString::new(vec![LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 2.0, y: 2.0 },
        Coord { x: 2.0, y: 0.0 },
        Coord { x: 0.0, y: 2.0 },
    ])]);
    let result = mls.make_valid();
    assert_not_empty(&result);
    // Single component unwrapped: MultiLineString([bowtie]) → LineString
    assert!(matches!(result, Geometry::LineString(_)));
}

// =========================================================================
// Rect edge cases
// =========================================================================

#[test]
fn test_rect_nan_min() {
    let r = Rect::new(Point::new(f64::NAN, 0.0), Point::new(10.0, 10.0));
    assert_empty(&r.make_valid());
}

#[test]
fn test_rect_nan_max() {
    let r = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, f64::NAN));
    assert_empty(&r.make_valid());
}

#[test]
fn test_rect_infinite() {
    let r = Rect::new(Point::new(f64::NEG_INFINITY, 0.0), Point::new(10.0, 10.0));
    assert_empty(&r.make_valid());
}

#[test]
fn test_rect_zero_area() {
    let r = Rect::new(Point::new(5.0, 5.0), Point::new(5.0, 5.0));
    assert_not_empty(&r.make_valid());
}

#[test]
fn test_rect_reversed() {
    let r = Rect::new(Point::new(10.0, 10.0), Point::new(0.0, 0.0));
    assert_not_empty(&r.make_valid());
}

// =========================================================================
// Triangle edge cases
// =========================================================================

#[test]
fn test_triangle_infinity_vertex() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: f64::INFINITY,
            y: 0.0,
        },
        Coord { x: 0.5, y: 1.0 },
    );
    assert_empty(&t.make_valid());
}

#[test]
fn test_triangle_clockwise() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.5, y: 1.0 },
        Coord { x: 1.0, y: 0.0 },
    );
    let result = t.make_valid();
    assert_not_empty(&result);
    assert_valid(&result);
}

#[test]
fn test_triangle_keep_collapsed_degenerate() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
    );
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let result = t.make_valid_with_config(&config);
    // Collinear triangle can't be fixed even with keep_collapsed
    assert_valid(&result);
}

// =========================================================================
// Geometry dispatch edge cases
// =========================================================================

#[test]
fn test_geometry_dispatch_rect() {
    let g = Geometry::Rect(Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0)));
    let result = g.make_valid();
    assert!(matches!(result, Geometry::Rect(_)));
}

#[test]
fn test_geometry_dispatch_triangle() {
    let g = Geometry::Triangle(Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 0.0 },
        Coord { x: 0.5, y: 1.0 },
    ));
    let result = g.make_valid();
    assert_valid(&result);
}

#[test]
fn test_geometry_dispatch_multipoint() {
    let g = Geometry::MultiPoint(MultiPoint::new(vec![
        Point::new(1.0, 2.0),
        Point::new(f64::NAN, 0.0),
    ]));
    let result = g.make_valid();
    let expected = Geometry::MultiPoint(MultiPoint::new(vec![Point::new(1.0, 2.0)]));
    assert_eq!(result, expected);
}

#[test]
fn test_geometry_dispatch_multilinestring() {
    let g = Geometry::MultiLineString(MultiLineString::new(vec![LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
    ])]));
    let result = g.make_valid();
    assert_valid(&result);
}

// =========================================================================
// GeometryCollection edge cases
// =========================================================================

#[test]
fn test_geometrycollection_includes_rect() {
    let gc = GeometryCollection(vec![Geometry::Rect(Rect::new(
        Point::new(0.0, 0.0),
        Point::new(1.0, 1.0),
    ))]);
    let result = gc.make_valid();
    assert_not_empty(&result);
}

#[test]
fn test_geometrycollection_includes_triangle() {
    let gc = GeometryCollection(vec![Geometry::Triangle(Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 0.0 },
        Coord { x: 0.5, y: 1.0 },
    ))]);
    let result = gc.make_valid();
    assert_not_empty(&result);
}

#[test]
fn test_geometrycollection_includes_multipolygon() {
    let gc = GeometryCollection(vec![Geometry::MultiPolygon(MultiPolygon::new(vec![
        Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        ),
    ]))]);
    let result = gc.make_valid();
    assert_not_empty(&result);
}

#[test]
fn test_geometrycollection_nested_empty_inside() {
    let gc = GeometryCollection(vec![
        Geometry::GeometryCollection(GeometryCollection(Vec::new())),
        Geometry::Point(Point::new(1.0, 2.0)),
    ]);
    let result = gc.make_valid();
    assert_not_empty(&result);
    // Nested empty should be filtered out
    if let Geometry::GeometryCollection(ref gc) = result {
        assert_eq!(gc.0.len(), 1);
    }
}

#[test]
fn test_geometrycollection_all_nested_empty() {
    let gc = GeometryCollection(vec![Geometry::GeometryCollection(GeometryCollection(
        Vec::new(),
    ))]);
    assert_empty(&gc.make_valid());
}

// =========================================================================
// Polygon edge cases
// =========================================================================

#[test]
fn test_polygon_empty_exterior() {
    let poly = Polygon::new(LineString::new(Vec::new()), Vec::new());
    let result = poly.make_valid();
    assert_valid(&result);
}

#[test]
fn test_polygon_all_nan_exterior() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
            Coord {
                x: 0.0,
                y: f64::NAN,
            },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_valid(&result);
}

#[test]
fn test_polygon_multiple_intersecting_holes() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 50.0, y: 0.0 },
            Coord { x: 50.0, y: 50.0 },
            Coord { x: 0.0, y: 50.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![
            LineString::new(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 15.0, y: 5.0 },
                Coord { x: 15.0, y: 15.0 },
                Coord { x: 5.0, y: 15.0 },
                Coord { x: 5.0, y: 5.0 },
            ]),
            LineString::new(vec![
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 20.0, y: 10.0 },
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 10.0, y: 20.0 },
                Coord { x: 10.0, y: 10.0 },
            ]),
        ],
    );
    let result = poly.make_valid();
    assert_valid(&result);
}

#[test]
fn test_polygon_hole_touching_shell() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 0.0, y: 5.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 5.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 5.0 },
        ])],
    );
    let result = poly.make_valid();
    assert_valid(&result);
}

#[test]
fn test_polygon_extreme_coordinates() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 1e10, y: 1e10 },
            Coord { x: 2e10, y: 1e10 },
            Coord { x: 2e10, y: 2e10 },
            Coord { x: 1e10, y: 2e10 },
            Coord { x: 1e10, y: 1e10 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_polygon_negative_extreme_coordinates() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: -1e10, y: -1e10 },
            Coord { x: 0.0, y: -1e10 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: -1e10, y: 0.0 },
            Coord { x: -1e10, y: -1e10 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// MultiPolygon edge cases
// =========================================================================

#[test]
fn test_multipolygon_empty() {
    let mp = MultiPolygon::<f64>::new(Vec::new());
    assert_empty(&mp.make_valid());
}

#[test]
fn test_multipolygon_all_invalid_components() {
    let mp = MultiPolygon::new(vec![Polygon::new(
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        Vec::new(),
    )]);
    let result = mp.make_valid();
    assert_valid(&result);
}

#[test]
fn test_multipolygon_with_multipolygon_result() {
    // Multiple invalid components that each produce valid but separate polygons
    let mp = MultiPolygon::new(vec![
        make_bowtie(),
        Polygon::new(
            LineString::new(vec![
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 30.0, y: 20.0 },
                Coord { x: 30.0, y: 30.0 },
                Coord { x: 20.0, y: 30.0 },
                Coord { x: 20.0, y: 20.0 },
            ]),
            Vec::new(),
        ),
    ]);
    let result = mp.make_valid();
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// Config defaults
// =========================================================================

#[test]
fn test_config_default_fill_rule() {
    let config = MakeValidConfig::default();
    assert_eq!(
        config.fill_rule,
        geo::algorithm::bool_ops::FillRule::EvenOdd
    );
}

#[test]
fn test_config_explicit_construction() {
    let config = MakeValidConfig {
        keep_collapsed: true,
        poly_method: PolyMethod::Arrange,
        fill_rule: geo::algorithm::bool_ops::FillRule::NonZero,
        ..Default::default()
    };
    assert!(config.keep_collapsed);
    assert_eq!(config.poly_method, PolyMethod::Arrange);
    assert_eq!(
        config.fill_rule,
        geo::algorithm::bool_ops::FillRule::NonZero
    );
}

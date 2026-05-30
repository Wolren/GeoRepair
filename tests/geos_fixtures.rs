//! GEOS MakeValid regression test fixtures.
//!
//! Ported from GEOS `MakeValidTest` and JTS `GeometryFixerTest`.
//! Each test case is a WKT input string and an expected output assertion
//! (validity, non-empty, sometimes specific WKT output).
//!
//! See GEOS source: `tests/unit/operation/overlayng/` and
//! JTS source: `modules/core/src/test/java/org/locationtech/jts/geom/util/`.

#[path = "common/mod.rs"]
mod common;
use common::*;

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use geo_repair::MakeValid;

// ---------------------------------------------------------------------------
// Self-touching ring that forms a ring-within-ring
// ---------------------------------------------------------------------------

#[test]
fn geos_self_touching_ring() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with zero area (all collinear)
// ---------------------------------------------------------------------------

#[test]
fn geos_zero_area_collinear() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 20 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
}

#[test]
fn geos_single_vertex() {
    let g = geom_from_wkt("POLYGON ((0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

#[test]
fn geos_two_vertex() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 1))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with duplicate consecutive points
// ---------------------------------------------------------------------------

#[test]
fn geos_duplicate_consecutive_points() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 0, 10 0, 10 10, 10 10, 0 10, 0 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// MultiPolygon with empty component
// ---------------------------------------------------------------------------

#[test]
fn geos_multipolygon_with_empty() {
    let g = geom_from_wkt("MULTIPOLYGON (EMPTY, ((0 0, 5 0, 5 5, 0 5, 0 0)))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Self-intersecting multi-ring polygon (interior ring crosses exterior)
// ---------------------------------------------------------------------------

#[test]
fn geos_hole_crosses_shell() {
    let g = geom_from_wkt("POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), (5 5, 15 5, 5 15, 15 15, 5 5))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Nested polygons (geometry collection with multi-level nesting)
// ---------------------------------------------------------------------------

#[test]
fn geos_nested_geometry_collection() {
    let g = geom_from_wkt(
        "GEOMETRYCOLLECTION (\
            GEOMETRYCOLLECTION (POINT (1 2)), \
            POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))\
        )",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Extremely large coordinate values
// ---------------------------------------------------------------------------

#[test]
fn geos_extreme_coords() {
    let g =
        geom_from_wkt("POLYGON ((-1e12 -1e12, 1e12 -1e12, 1e12 1e12, -1e12 1e12, -1e12 -1e12))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Polygon with only NaN coordinates
// ---------------------------------------------------------------------------

#[test]
fn geos_nan_polygon() {
    let g = Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            Coord {
                x: f64::NAN,
                y: f64::NAN,
            },
            Coord {
                x: f64::NAN,
                y: f64::NAN,
            },
            Coord {
                x: f64::NAN,
                y: f64::NAN,
            },
            Coord {
                x: f64::NAN,
                y: f64::NAN,
            },
            Coord {
                x: f64::NAN,
                y: f64::NAN,
            },
        ]),
        Vec::new(),
    ));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// LineString self-intersection
// ---------------------------------------------------------------------------

#[test]
fn geos_self_intersecting_linestring() {
    let g = geom_from_wkt("LINESTRING (0 0, 2 2, 2 0, 0 2)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// MultiLineString with one self-intersecting component
// ---------------------------------------------------------------------------

#[test]
fn geos_multilinestring_with_self_intersection() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 1 0), (0 0, 2 2, 2 0, 0 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    assert!(matches!(
        &result,
        Geometry::MultiLineString(_) | Geometry::GeometryCollection(_)
    ));
}

// ---------------------------------------------------------------------------
// Multiple polygon types with Arrange method
// ---------------------------------------------------------------------------

#[test]
fn geos_overlapping_shells_arrange() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (\
            ((0 0, 5 0, 5 5, 0 5, 0 0)), \
            ((3 3, 8 3, 8 8, 3 8, 3 3))\
        )",
    );
    let result = g.make_valid_with_config(&cfg_arrange());
    assert_valid_ogc(&result);
    assert!(matches!(&result, Geometry::MultiPolygon(_)));
}

// ---------------------------------------------------------------------------
// Polygon that remains empty after fix
// ---------------------------------------------------------------------------

#[test]
fn geos_empty_after_fix() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 0, 0 0, 0 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// Complex bowtie with additional points
// ---------------------------------------------------------------------------

#[test]
fn geos_complex_bowtie() {
    let g = geom_from_wkt("POLYGON ((0 0, 5 5, 10 0, 10 10, 5 5, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_multipolygon_count(&result, 2);
}

// ---------------------------------------------------------------------------
// Polygon with hole that exactly matches shell border
// ---------------------------------------------------------------------------

#[test]
fn geos_hole_exactly_matches_shell() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (0 0, 10 0, 10 10, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// Valid linestring should pass through unchanged
// ---------------------------------------------------------------------------

#[test]
fn geos_valid_linestring_unchanged() {
    let g = geom_from_wkt("LINESTRING (0 0, 1 1, 2 2)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    assert_eq!(result, g);
}

// ---------------------------------------------------------------------------
// Bowtie / self-intersection regression
// ---------------------------------------------------------------------------

#[test]
fn geos_bowtie_arrange() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_arrange());
    assert_valid_ogc(&result);
    assert_multipolygon_count(&result, 2);
}

#[test]
fn geos_bowtie_auto() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn geos_figure_eight() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Ring self-intersection (hourglass)
// ---------------------------------------------------------------------------

#[test]
fn geos_hourglass() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 5 5, 10 10, 10 0, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_multipolygon_count(&result, 2);
}

// ---------------------------------------------------------------------------
// Spike (co-linear edge with spike tip)
// ---------------------------------------------------------------------------

#[test]
fn geos_spike() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 5 5, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Hole outside shell
// ---------------------------------------------------------------------------

#[test]
fn geos_hole_outside() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (20 20, 25 20, 25 25, 20 25, 20 20))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Self-touching ring (figure-8 without crossing)
// ---------------------------------------------------------------------------

#[test]
fn geos_self_touching() {
    let g = geom_from_wkt("POLYGON ((0 0, 5 10, 10 0, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Micro / nearly-collapsed polygon
// ---------------------------------------------------------------------------

#[test]
fn geos_micro_polygon() {
    let g = geom_from_wkt("POLYGON ((0 0, 1e-15 0, 1e-15 1e-15, 0 1e-15, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
}

// ---------------------------------------------------------------------------
// Reversed ring (clockwise exterior)
// ---------------------------------------------------------------------------

#[test]
fn geos_clockwise_exterior() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// MultiPolygon with invalid component
// ---------------------------------------------------------------------------

#[test]
fn geos_multipolygon_with_invalid() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((0 0, 10 10, 10 0, 0 10, 0 0)), ((20 20, 30 20, 30 30, 20 30, 20 20)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert!(count_geometries(&result) >= 2);
}

// ---------------------------------------------------------------------------
// GeometryCollection with mixed valid/invalid
// ---------------------------------------------------------------------------

#[test]
fn geos_collection_mixed() {
    let g = geom_from_wkt("GEOMETRYCOLLECTION (POINT (1 1), POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0)), LINESTRING (0 0, 10 10))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Repeated consecutive coordinates
// ---------------------------------------------------------------------------

#[test]
fn geos_repeated_coords() {
    let g =
        geom_from_wkt("POLYGON ((0 0, 0 0, 5 5, 5 5, 10 0, 10 0, 10 10, 10 10, 0 10, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Non-closed ring
// ---------------------------------------------------------------------------

#[test]
fn geos_unclosed_ring() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Nested bowties
// ---------------------------------------------------------------------------

#[test]
fn geos_nested_bowties() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (\
            ((0 0, 5 5, 5 0, 0 5, 0 0)), \
            ((10 10, 15 15, 15 10, 10 15, 10 10))\
        )",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert!(matches!(&result, Geometry::MultiPolygon(_)));
}

// ---------------------------------------------------------------------------
// Large coordinate values
// ---------------------------------------------------------------------------

#[test]
fn geos_large_coords() {
    let g = geom_from_wkt("POLYGON ((1e6 1e6, 2e6 1e6, 2e6 2e6, 1e6 2e6, 1e6 1e6))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Negative coordinate values
// ---------------------------------------------------------------------------

#[test]
fn geos_negative_coords() {
    let g = geom_from_wkt("POLYGON ((-10 -10, 0 -10, 0 0, -10 0, -10 -10))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Complex MultiPolygon with overlapping shells
// ---------------------------------------------------------------------------

#[test]
fn geos_overlapping_shells() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (\
            ((0 0, 5 0, 5 5, 0 5, 0 0)), \
            ((3 3, 8 3, 8 8, 3 8, 3 3))\
        )",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert!(matches!(&result, Geometry::MultiPolygon(_)));
}

// =========================================================================
// JTS GeometryFixer tests: NaN/Inf, collapse, regression
// =========================================================================

// ---------------------------------------------------------------------------
// Point with NaN coordinate -> empty
// ---------------------------------------------------------------------------

#[test]
fn geos_point_nan() {
    let g = Geometry::Point(Point::new(f64::NAN, 0.0));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// Point with Inf coordinate -> empty
// ---------------------------------------------------------------------------

#[test]
fn geos_point_inf() {
    let g = Geometry::Point(Point::new(0.0, f64::INFINITY));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// MultiPoint with all NaN -> empty
// ---------------------------------------------------------------------------

#[test]
fn geos_multipoint_nan() {
    let g = Geometry::MultiPoint(MultiPoint(vec![Point::new(f64::NAN, 0.0)]));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// LineString with NaN coordinate -> empty
// ---------------------------------------------------------------------------

#[test]
fn geos_linestring_nan_collapse() {
    let g = Geometry::LineString(LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: 1.0,
            y: f64::NAN,
        },
        Coord { x: 0.0, y: 0.0 },
    ]));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// LineString with many repeated coords simplified
// ---------------------------------------------------------------------------

#[test]
fn geos_linestring_many_repeated() {
    let g = geom_from_wkt("LINESTRING (0 0, 0 0, 0 0, 0 0, 0 0, 1 1)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert!(matches!(&result, Geometry::LineString(_)));
}

// ---------------------------------------------------------------------------
// Polygon with zero-area overlapping holes (JTS GeometryFixerTest)
// ---------------------------------------------------------------------------

#[test]
fn geos_polygon_zero_area_overlap_holes() {
    let g = geom_from_wkt(
        "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), \
                  (80 70, 30 70, 30 20, 30 70, 80 70), \
                  (70 80, 70 30, 20 30, 70 30, 70 80))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
}

// ---------------------------------------------------------------------------
// Polygon with positive and negative winding overlap
// ---------------------------------------------------------------------------

#[test]
fn geos_polygon_pos_neg_overlap() {
    let g = geom_from_wkt(
        "POLYGON ((10 90, 50 90, 50 30, 70 30, 70 50, 30 50, 30 70, 90 70, 90 10, 10 10, 10 90))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Complex holes touching at vertices (JTS testHolesTouching)
// ---------------------------------------------------------------------------

#[test]
fn geos_holes_touching_complex() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 0 5, 6 5, 6 0, 0 0), \
                  (3 1, 4 1, 4 2, 3 2, 3 1), \
                  (3 2, 1 4, 5 4, 4 2, 4 3, 3 2, 2 3, 3 2))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Empty MultiPolygon (GEOS MakeValidTest test<3>)
// ---------------------------------------------------------------------------

#[test]
fn geos_empty_multipolygon() {
    let g = Geometry::MultiPolygon(MultiPolygon(Vec::new()));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// MULTIPOLYGON (EMPTY, EMPTY)
// ---------------------------------------------------------------------------

#[test]
fn geos_multipolygon_multi_empty() {
    let g = geom_from_wkt("MULTIPOLYGON (EMPTY, EMPTY)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// GEOMETRYCOLLECTION EMPTY
// ---------------------------------------------------------------------------

#[test]
fn geos_geometry_collection_empty() {
    let g = Geometry::GeometryCollection(GeometryCollection(Vec::new()));
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// GC with all empty sub-geometries
// ---------------------------------------------------------------------------

#[test]
fn geos_gc_all_empty() {
    let g = geom_from_wkt("GEOMETRYCOLLECTION (POINT EMPTY, LINESTRING EMPTY, POLYGON EMPTY)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// Real-world regression: JTS Issue #852 case 1
// ---------------------------------------------------------------------------

#[test]
fn geos_issue_852_case1() {
    let g = geom_from_wkt(
        "POLYGON ((42.565844354657436 -72.61247966084643, \
                   42.56484510561062 -72.61202938126273, \
                   42.56384585656381 -72.61247966084643, \
                   42.563637679679054 -72.61276108558623, \
                   42.562055535354936 -72.61366164475362, \
                   42.5631796905326 -72.61259223074235, \
                   42.565844354657436 -72.61214195115866, \
                   42.566510520688645 -72.61259223074235, \
                   42.565844354657436 -72.61247966084643))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Real-world regression: JTS Issue #852 case 2
// ---------------------------------------------------------------------------

#[test]
fn geos_issue_852_case2() {
    let g = geom_from_wkt(
        "POLYGON ((50.69544005538049 4.587126197745181, \
                   50.699035986722194 4.592752502415541, \
                   50.699395579856365 4.592049214331746, \
                   50.699125885005735 4.590501980547397, \
                   50.69867639358802 4.591064611014433, \
                   50.69795720731968 4.591064611014433, \
                   50.69759761418551 4.590501980547397, \
                   50.69759761418551 4.589376719613325, \
                   50.69831680045385 4.588251458679252, \
                   50.69723802105134 4.586563567278144, \
                   50.69579964851466 4.586563567278144, \
                   50.69544005538049 4.587126197745181))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// GEOS issue #265 regression (exact coordinates from MakeValidTest.cpp)
// ---------------------------------------------------------------------------

#[test]
fn geos_issue_265_actual() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 2.22, y: 2.28 },
            Coord { x: 7.67, y: 2.06 },
            Coord { x: 10.98, y: 7.70 },
            Coord { x: 9.39, y: 5.00 },
            Coord { x: 7.96, y: 7.12 },
            Coord { x: 6.77, y: 5.16 },
            Coord { x: 7.43, y: 6.24 },
            Coord { x: 3.70, y: 7.22 },
            Coord { x: 5.72, y: 5.77 },
            Coord { x: 4.18, y: 10.74 },
            Coord { x: 2.20, y: 6.83 },
            Coord { x: 2.22, y: 2.28 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

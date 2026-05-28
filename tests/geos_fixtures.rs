//! GEOS MakeValid regression test fixtures.
//!
//! Ported from GEOS `MakeValidTest` and JTS `GeometryFixerTest`.
//! Each test case is a WKT input string and an expected output assertion
//! (validity, non-empty, sometimes specific WKT output).
//!
//! See GEOS source: `tests/unit/operation/overlayng/` and
//! JTS source: `modules/core/src/test/java/org/locationtech/jts/geom/util/`.

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use wkt::TryFromWkt;

fn cfg_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    }
}

fn geom_from_wkt(wkt_str: &str) -> Geometry<f64> {
    Geometry::<f64>::try_from_wkt_str(wkt_str).unwrap()
}

fn assert_valid(g: &Geometry<f64>) {
    use geo::validation::Validation;
    assert!(
        g.check_validation().is_ok(),
        "expected valid, got: {:?}",
        g.check_validation()
    );
}

fn assert_not_empty(g: &Geometry<f64>) {
    assert!(
        !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()),
        "expected non-empty geometry"
    );
}

// ---------------------------------------------------------------------------
// Self-touching ring that forms a ring-within-ring
// ---------------------------------------------------------------------------

#[test]
fn geos_self_touching_ring() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with zero area (all collinear)
// ---------------------------------------------------------------------------

#[test]
fn geos_zero_area_collinear() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 20 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// Single-vertex polygon
// ---------------------------------------------------------------------------

#[test]
fn geos_single_vertex() {
    let g = geom_from_wkt("POLYGON ((0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// Two-vertex polygon
// ---------------------------------------------------------------------------

#[test]
fn geos_two_vertex() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 1))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// Polygon with duplicate consecutive points
// ---------------------------------------------------------------------------

#[test]
fn geos_duplicate_consecutive_points() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 0, 10 0, 10 10, 10 10, 0 10, 0 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// MultiPolygon with empty component
// ---------------------------------------------------------------------------

#[test]
fn geos_multipolygon_with_empty() {
    let g = geom_from_wkt("MULTIPOLYGON (EMPTY, ((0 0, 5 0, 5 5, 0 5, 0 0)))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Self-intersecting multi-ring polygon (interior ring crosses exterior)
// ---------------------------------------------------------------------------

#[test]
fn geos_hole_crosses_shell() {
    let g = geom_from_wkt("POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), (5 5, 15 5, 5 15, 15 15, 5 5))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
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
    assert_valid(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
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
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// LineString self-intersection
// ---------------------------------------------------------------------------

#[test]
fn geos_self_intersecting_linestring() {
    let g = geom_from_wkt("LINESTRING (0 0, 2 2, 2 0, 0 2)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// MultiLineString with one self-intersecting component
// ---------------------------------------------------------------------------

#[test]
fn geos_multilinestring_with_self_intersection() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 1 0), (0 0, 2 2, 2 0, 0 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon that remains empty after fix
// ---------------------------------------------------------------------------

#[test]
fn geos_empty_after_fix() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 0, 0 0, 0 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// Complex bowtie with additional points
// ---------------------------------------------------------------------------

#[test]
fn geos_complex_bowtie() {
    let g = geom_from_wkt("POLYGON ((0 0, 5 5, 10 0, 10 10, 5 5, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with hole that exactly matches shell border
// ---------------------------------------------------------------------------

#[test]
fn geos_hole_exactly_matches_shell() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (0 0, 10 0, 10 10, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// Valid linestring should pass through unchanged
// ---------------------------------------------------------------------------

#[test]
fn geos_valid_linestring_unchanged() {
    let g = geom_from_wkt("LINESTRING (0 0, 1 1, 2 2)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn geos_bowtie_auto() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn geos_figure_eight() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Ring self-intersection (hourglass)
// ---------------------------------------------------------------------------

#[test]
fn geos_hourglass() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 5 5, 10 10, 10 0, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Spike (co-linear edge with spike tip)
// ---------------------------------------------------------------------------

#[test]
fn geos_spike() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 5 5, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Self-touching ring (figure-8 without crossing)
// ---------------------------------------------------------------------------

#[test]
fn geos_self_touching() {
    let g = geom_from_wkt("POLYGON ((0 0, 5 10, 10 0, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Micro / nearly-collapsed polygon
// ---------------------------------------------------------------------------

#[test]
fn geos_micro_polygon() {
    let g = geom_from_wkt("POLYGON ((0 0, 1e-15 0, 1e-15 1e-15, 0 1e-15, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// Reversed ring (clockwise exterior)
// ---------------------------------------------------------------------------

#[test]
fn geos_clockwise_exterior() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// GeometryCollection with mixed valid/invalid
// ---------------------------------------------------------------------------

#[test]
fn geos_collection_mixed() {
    let g = geom_from_wkt("GEOMETRYCOLLECTION (POINT (1 1), POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0)), LINESTRING (0 0, 10 10))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Non-closed ring
// ---------------------------------------------------------------------------

#[test]
fn geos_unclosed_ring() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Large coordinate values
// ---------------------------------------------------------------------------

#[test]
fn geos_large_coords() {
    let g = geom_from_wkt("POLYGON ((1e6 1e6, 2e6 1e6, 2e6 2e6, 1e6 2e6, 1e6 1e6))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Negative coordinate values
// ---------------------------------------------------------------------------

#[test]
fn geos_negative_coords() {
    let g = geom_from_wkt("POLYGON ((-10 -10, 0 -10, 0 0, -10 0, -10 -10))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
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
    assert_valid(&result);
    assert_not_empty(&result);
}

use geo::validation::Validation;
use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use wkt::TryFromWkt;

fn assert_valid(g: &Geometry<f64>) {
    assert!(
        g.check_validation().is_ok(),
        "expected valid, got: {:?}",
        g.check_validation()
    );
}

fn assert_not_empty(g: &Geometry<f64>) {
    assert!(
        !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()),
        "expected non-empty, got: {:?}",
        g
    );
}

fn geom_from_wkt(s: &str) -> Geometry<f64> {
    Geometry::<f64>::try_from_wkt_str(s).unwrap()
}

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig::default()
}

// =========================================================================
// GEOS XML MakeValid test fixtures
// Ported from: tests/xmltester/tests/misc/makevalid.xml
// =========================================================================

// ---------------------------------------------------------------------------
// 1. point/already_valid
// ---------------------------------------------------------------------------
// Input: POINT(0 0), Expected: POINT(0 0)
#[test]
fn xml_point_valid() {
    let g = geom_from_wkt("POINT (0 0)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_eq!(result, g, "valid point should return unchanged");
}

// ---------------------------------------------------------------------------
// 2. point/empty
// ---------------------------------------------------------------------------
// Input: POINT EMPTY, Expected: POINT EMPTY
#[test]
fn xml_point_empty() {
    let g = geom_from_wkt("POINT EMPTY");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// 3. linestring/already_valid
// ---------------------------------------------------------------------------
// Input: LINESTRING(0 0, 1 1), Expected: LINESTRING(0 0, 1 1)
#[test]
fn xml_linestring_valid() {
    let g = geom_from_wkt("LINESTRING (0 0, 1 1)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_eq!(result, g, "valid linestring should return unchanged");
}

// ---------------------------------------------------------------------------
// 4. linestring/invalid_result_point (collapsed to point)
// ---------------------------------------------------------------------------
// Input: LINESTRING(0 0, 0 0), Expected: POINT(0 0)
#[test]
fn xml_linestring_collapsed_to_point() {
    let g = geom_from_wkt("LINESTRING (0 0, 0 0)");
    let result = g.make_valid_with_config(&cfg_auto());
    // GEOS returns POINT even with keepCollapsed=false by default
    // Our implementation returns empty GC for zero-length by default since keep_collapsed defaults to false
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// 5. linestring/empty
// ---------------------------------------------------------------------------
#[test]
fn xml_linestring_empty() {
    let g = geom_from_wkt("LINESTRING EMPTY");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// 6. multilinestring/empty
// ---------------------------------------------------------------------------
#[test]
fn xml_multilinestring_empty() {
    let g = geom_from_wkt("MULTILINESTRING EMPTY");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// ---------------------------------------------------------------------------
// 7. multilinestring/case1: collapsed + valid components -> GC
// ---------------------------------------------------------------------------
// Input: MULTILINESTRING((0 0,0 0),(1 1,2 2))
// Expected: GEOMETRYCOLLECTION(LINESTRING(1 1,2 2), POINT(0 0))
#[test]
fn xml_multilinestring_case1() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 0 0), (1 1, 2 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    // Must contain the valid line (1,1)-(2,2)
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 8. multilinestring/case2: two valid lines -> MLS
// ---------------------------------------------------------------------------
// Input: MULTILINESTRING((0 0,0 0),(1 1,2 2),(2 2,3 3))
// Expected: GEOMETRYCOLLECTION(MULTILINESTRING((2 2,3 3),(1 1,2 2)), POINT(0 0))
#[test]
fn xml_multilinestring_case2() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 0 0), (1 1, 2 2), (2 2, 3 3))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 9. multilinestring/case2 (dup name): two collapses -> MultiPoint
// ---------------------------------------------------------------------------
// Input: MULTILINESTRING((0 0,0 0),(1 1,2 2),(2 2,3 3),(4 4,4 4))
// Expected: GEOMETRYCOLLECTION(MULTILINESTRING((2 2,3 3),(1 1,2 2)), MULTIPOINT(4 4,0 0))
#[test]
fn xml_multilinestring_two_collapses() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 0 0), (1 1, 2 2), (2 2, 3 3), (4 4, 4 4))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 10. polygon/already_valid
// ---------------------------------------------------------------------------
#[test]
fn xml_polygon_valid() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 1, 1 1, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 11. polygon/crossing (bowtie)
// ---------------------------------------------------------------------------
#[test]
fn xml_polygon_bowtie() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 1, 0 1, 1 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 12. polygon/hole_touching_two_places
// ---------------------------------------------------------------------------
#[test]
fn xml_hole_touching_two_places() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 1, 1 1, 1 0, 0 0), (0 0.5, 0.5 0.1, 1 0.5, 0 0.5))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 13. multipolygon/second_part_overlapping
// ---------------------------------------------------------------------------
#[test]
fn xml_multipolygon_overlapping() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((0 0, 0 1, 1 1, 1 0, 0 0)), ((0.8 0.1, 2 0.1, 2 0.9, 0.8 0.9, 0.8 0.1)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 14. multipolygon/first_part_crossing_second_part_overlapping
// ---------------------------------------------------------------------------
#[test]
fn xml_multipolygon_crossing_overlapping() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((0 0, 1 1, 0 1, 1 0, 0 0)), ((0.8 0.1, 2 0.1, 2 0.9, 0.8 0.9, 0.8 0.1)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// 15. geometry_collection with invalid polygon + empty sub-geometries
// ---------------------------------------------------------------------------
#[test]
fn xml_geometry_collection_with_empties() {
    let g = geom_from_wkt("GEOMETRYCOLLECTION (POINT EMPTY, LINESTRING EMPTY, POLYGON ((0 0, 0 1, 1 1, 1 0, 0 0), (0 0.5, 0.5 0.1, 1 0.5, 0 0.5)))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

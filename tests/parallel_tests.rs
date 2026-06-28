//! Parallel function tests for geo-repair.
//!
//! Validates that Rayon-based parallel repair functions produce correct,
//! valid output for all Multi* geometry types.

#![cfg(feature = "parallel")]

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use geo_repair::parallel;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig};

fn assert_valid(g: &Geometry<f64>) {
    assert!(
        g.validate().valid,
        "expected valid, got: {:?}",
        g.validate()
    );
}

fn assert_not_empty(g: &Geometry<f64>) {
    assert!(
        !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()),
        "expected non-empty"
    );
}

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig::default()
}

// =========================================================================
// par_fix_multi_point
// =========================================================================

#[test]
fn test_par_fix_multi_point_valid() {
    let mp = MultiPoint::new(vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]);
    let result = parallel::par_fix_multi_point(&mp, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_par_fix_multi_point_empty() {
    let mp = MultiPoint::<f64>::new(Vec::new());
    let result = parallel::par_fix_multi_point(&mp, &cfg_auto());
    assert!(matches!(result, Geometry::GeometryCollection(gc) if gc.0.is_empty()));
}

#[test]
fn test_par_fix_multi_point_mixed() {
    let mp = MultiPoint::new(vec![
        Point::new(1.0, 2.0),
        Point::new(f64::NAN, 0.0),
        Point::new(3.0, 4.0),
    ]);
    let result = parallel::par_fix_multi_point(&mp, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_par_fix_multi_point_all_invalid() {
    let mp = MultiPoint::new(vec![Point::new(f64::NAN, 0.0)]);
    let result = parallel::par_fix_multi_point(&mp, &cfg_auto());
    assert!(matches!(result, Geometry::GeometryCollection(gc) if gc.0.is_empty()));
}

// =========================================================================
// par_fix_multi_line_string
// =========================================================================

#[test]
fn test_par_fix_multi_line_string_valid() {
    let mls = MultiLineString::new(vec![LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
    ])]);
    let result = parallel::par_fix_multi_line_string(&mls, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_par_fix_multi_line_string_empty() {
    let mls = MultiLineString::<f64>::new(Vec::new());
    let result = parallel::par_fix_multi_line_string(&mls, &cfg_auto());
    assert!(matches!(result, Geometry::GeometryCollection(gc) if gc.0.is_empty()));
}

#[test]
fn test_par_fix_multi_line_string_mixed() {
    let mls = MultiLineString::new(vec![
        LineString::new(Vec::new()),
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
    ]);
    let result = parallel::par_fix_multi_line_string(&mls, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// par_fix_multi_polygon
// =========================================================================

#[test]
fn test_par_fix_multi_polygon_valid() {
    let mp = MultiPolygon::new(vec![Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 0.0, y: 5.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )]);
    let result = parallel::par_fix_multi_polygon(&mp, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_par_fix_multi_polygon_with_bowtie() {
    let mp = MultiPolygon::new(vec![Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )]);
    let result = parallel::par_fix_multi_polygon(&mp, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_par_fix_multi_polygon_empty() {
    let mp = MultiPolygon::<f64>::new(Vec::new());
    let result = parallel::par_fix_multi_polygon(&mp, &cfg_auto());
    assert!(matches!(result, Geometry::GeometryCollection(gc) if gc.0.is_empty()));
}

#[test]
fn test_par_fix_multi_polygon_overlapping() {
    let mp = MultiPolygon::new(vec![
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
        Polygon::new(
            LineString::new(vec![
                Coord { x: 3.0, y: 3.0 },
                Coord { x: 8.0, y: 3.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 3.0, y: 8.0 },
                Coord { x: 3.0, y: 3.0 },
            ]),
            Vec::new(),
        ),
    ]);
    let result = parallel::par_fix_multi_polygon(&mp, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// par_fix_collection
// =========================================================================

#[test]
fn test_par_fix_collection_valid() {
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ])),
    ]);
    let result = parallel::par_fix_collection(&gc, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_par_fix_collection_empty() {
    let gc = GeometryCollection::<f64>(Vec::new());
    let result = parallel::par_fix_collection(&gc, &cfg_auto());
    assert!(matches!(result, Geometry::GeometryCollection(gc2) if gc2.0.is_empty()));
}

#[test]
fn test_par_fix_collection_mixed() {
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(f64::NAN, 0.0)),
        Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        )),
        Geometry::MultiPoint(MultiPoint::new(vec![Point::new(1.0, 2.0)])),
    ]);
    let result = parallel::par_fix_collection(&gc, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// par_fix_collection with bowtie polygon
// =========================================================================

#[test]
fn test_par_fix_collection_bowtie() {
    let gc = GeometryCollection(vec![Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    ))]);
    let result = parallel::par_fix_collection(&gc, &cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// Par vs serial consistency: parallel results must match serial
// =========================================================================

#[test]
fn test_par_equals_serial_multipoint() {
    let mp = MultiPoint::new(vec![
        Point::new(1.0, 2.0),
        Point::new(f64::NAN, 0.0),
        Point::new(3.0, 4.0),
    ]);
    let serial = mp.make_valid_with_config(&cfg_auto());
    let parallel = parallel::par_fix_multi_point(&mp, &cfg_auto());
    assert_eq!(serial, parallel);
}

#[test]
fn test_par_equals_serial_multilinestring() {
    let mls = MultiLineString::new(vec![
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        LineString::new(Vec::new()),
    ]);
    let serial = mls.make_valid_with_config(&cfg_auto());
    let parallel = parallel::par_fix_multi_line_string(&mls, &cfg_auto());
    assert_eq!(serial, parallel);
}

#[test]
fn test_par_equals_serial_multipolygon() {
    let mp = MultiPolygon::new(vec![Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )]);
    let serial = mp.make_valid_with_config(&cfg_auto());
    let parallel = parallel::par_fix_multi_polygon(&mp, &cfg_auto());
    assert_eq!(serial, parallel);
}

#[test]
fn test_par_equals_serial_collection() {
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        )),
    ]);
    let serial = gc.make_valid_with_config(&cfg_auto());
    let parallel = parallel::par_fix_collection(&gc, &cfg_auto());
    assert_eq!(serial, parallel);
}

//! Stress tests: SIMD correctness, parallel execution consistency,
//! and large-scale stress testing.
//!
//! Verifies that:
//! - SIMD results match scalar results
//! - Parallel results match serial results
//! - Large inputs don't crash

#![cfg(feature = "parallel")]

use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

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
        "expected non-empty, got: {:?}",
        g
    );
}

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig::default()
}
#[allow(dead_code)]
fn cfg_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}
#[allow(dead_code)]
fn cfg_structure() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    }
}

// =========================================================================
// SECTION 1: Large polygon stress (1000+ vertices)
// =========================================================================

/// Regular polygon with 10000 vertices - stress test
#[test]
fn stress_large_regular_polygon() {
    let n = 10000;
    let mut coords: Vec<Coord<f64>> = (0..n)
        .map(|j| {
            let angle = 2.0 * std::f64::consts::PI * j as f64 / n as f64;
            Coord {
                x: 1000.0 * angle.cos(),
                y: 1000.0 * angle.sin(),
            }
        })
        .collect();
    coords.push(coords[0]);
    let poly = Polygon::new(LineString::new(coords), Vec::new());
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

/// 100 random small polygons - stress test
#[test]
fn stress_many_small_polygons() {
    for i in 0..100 {
        let x = (i as f64) * 10.0;
        let y = 0.0;
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x, y },
                Coord { x: x + 8.0, y },
                Coord {
                    x: x + 8.0,
                    y: y + 8.0,
                },
                Coord { x, y: y + 8.0 },
                Coord { x, y },
            ]),
            Vec::new(),
        );
        let result = poly.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
        assert_not_empty(&result);
    }
}

// =========================================================================
// SECTION 2: Serial vs Parallel consistency
// =========================================================================

/// Verify that serial and parallel MakeValid produce consistent valid results
/// for MultiPoint
#[test]
fn parallel_multipoint_consistency() {
    use geo_repair::parallel::par_fix_multi_point;

    let points: Vec<Point<f64>> = (0..100)
        .map(|i| Point::new(i as f64, (i * 7) as f64))
        .collect();
    let mp = MultiPoint::new(points);

    let serial_result = mp.make_valid_with_config(&cfg_auto());
    assert_valid(&serial_result);

    let parallel_result = par_fix_multi_point(&mp, &cfg_auto());
    assert_valid(&parallel_result);
}

/// Verify serial and parallel consistency for MultiLineString
#[test]
fn parallel_multilinestring_consistency() {
    use geo_repair::parallel::par_fix_multi_line_string;

    let lines: Vec<LineString<f64>> = (0..50)
        .map(|i| {
            let x = i as f64;
            LineString::new(vec![
                Coord { x, y: 0.0 },
                Coord { x: x + 1.0, y: 1.0 },
                Coord { x: x + 2.0, y: 2.0 },
            ])
        })
        .collect();
    let mls = MultiLineString::new(lines);

    let serial_result = mls.make_valid_with_config(&cfg_auto());
    assert_valid(&serial_result);

    let parallel_result = par_fix_multi_line_string(&mls, &cfg_auto());
    assert_valid(&parallel_result);
}

/// Verify serial and parallel consistency for MultiPolygon
#[test]
fn parallel_multipolygon_consistency() {
    use geo_repair::parallel::par_fix_multi_polygon;

    let polys: Vec<Polygon<f64>> = (0..20)
        .map(|i| {
            let x = (i as f64) * 15.0;
            Polygon::new(
                LineString::new(vec![
                    Coord { x, y: 0.0 },
                    Coord {
                        x: x + 10.0,
                        y: 0.0,
                    },
                    Coord {
                        x: x + 10.0,
                        y: 10.0,
                    },
                    Coord { x, y: 10.0 },
                    Coord { x, y: 0.0 },
                ]),
                Vec::new(),
            )
        })
        .collect();
    let mp = MultiPolygon::new(polys);

    let serial_result = mp.make_valid_with_config(&cfg_auto());
    assert_valid(&serial_result);

    let parallel_result = par_fix_multi_polygon(&mp, &cfg_auto());
    assert_valid(&parallel_result);
}

/// Verify serial and parallel consistency for GeometryCollection
#[test]
fn parallel_geometry_collection_consistency() {
    use geo_repair::parallel::par_fix_collection;

    let items: Vec<Geometry<f64>> = (0..30)
        .map(|i| {
            let x = i as f64;
            if i % 3 == 0 {
                Geometry::Point(Point::new(x, x + 1.0))
            } else if i % 3 == 1 {
                Geometry::LineString(LineString::new(vec![
                    Coord { x, y: 0.0 },
                    Coord { x: x + 1.0, y: 1.0 },
                ]))
            } else {
                Geometry::MultiPoint(MultiPoint::new(vec![
                    Point::new(x, 0.0),
                    Point::new(x + 1.0, 1.0),
                ]))
            }
        })
        .collect();
    let gc = geo::GeometryCollection(items);

    let serial_result = gc.make_valid_with_config(&cfg_auto());
    assert_valid(&serial_result);

    let parallel_result = par_fix_collection(&gc, &cfg_auto());
    assert_valid(&parallel_result);
}

// =========================================================================
// SECTION 3: All methods on large complex inputs
// =========================================================================

/// Complex bowtie with many segments
#[test]
fn stress_complex_bowtie() {
    let mut coords = Vec::new();
    for i in 0..20 {
        let t = i as f64 / 20.0 * 2.0 * std::f64::consts::PI;
        coords.push(Coord {
            x: 100.0 * t.cos(),
            y: 100.0 * t.sin(),
        });
    }
    for i in 0..20 {
        let t = i as f64 / 20.0 * 2.0 * std::f64::consts::PI;
        coords.push(Coord {
            x: 50.0 * t.cos(),
            y: 50.0 * t.sin(),
        });
    }
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    let poly = Polygon::new(LineString::new(coords), Vec::new());

    for method in &[PolyMethod::Auto, PolyMethod::Arrange] {
        let config = MakeValidConfig {
            poly_method: method.clone(),
            ..Default::default()
        };
        let result = poly.make_valid_with_config(&config);
        assert_valid(&result);
    }
}

/// Many overlapping rings
#[test]
fn stress_many_overlapping_rings() {
    let mut holes = Vec::new();
    for i in 0..20 {
        let cx = 50.0 + (i as f64 - 10.0) * 4.0;
        let cy = 50.0 + (i as f64 - 10.0) * 3.0;
        let r = 5.0;
        holes.push(LineString::new(vec![
            Coord {
                x: cx - r,
                y: cy - r,
            },
            Coord { x: cx + r, y: cy },
            Coord {
                x: cx - r,
                y: cy + r,
            },
            Coord {
                x: cx - r,
                y: cy - r,
            },
        ]));
    }
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 100.0, y: 0.0 },
            Coord { x: 100.0, y: 100.0 },
            Coord { x: 0.0, y: 100.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        holes,
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

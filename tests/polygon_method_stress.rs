use geo::validation::Validation;
use geo::{Coord, Geometry, LineString, Polygon};
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
        "expected non-empty geometry"
    );
}

#[allow(dead_code)]
fn geom_from_wkt(s: &str) -> Geometry<f64> {
    Geometry::<f64>::try_from_wkt_str(s).unwrap()
}

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig::default()
}

fn cfg_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}

fn cfg_structure() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    }
}

fn assert_valid_for_all_methods(poly: &Polygon<f64>) {
    for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let config = MakeValidConfig {
            poly_method: method.clone(),
            ..Default::default()
        };
        let result = poly.make_valid_with_config(&config);
        assert_valid(&result);
    }
}

// =========================================================================
// SECTION 1: Method consistency
// =========================================================================

#[test]
fn test_all_methods_simple_square() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    assert_valid_for_all_methods(&poly);
}

#[test]
fn test_all_methods_with_hole() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 20.0, y: 0.0 },
            Coord { x: 20.0, y: 20.0 },
            Coord { x: 0.0, y: 20.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 15.0, y: 5.0 },
            Coord { x: 15.0, y: 15.0 },
            Coord { x: 5.0, y: 15.0 },
            Coord { x: 5.0, y: 5.0 },
        ])],
    );
    assert_valid_for_all_methods(&poly);
}

#[test]
fn test_all_methods_triangle() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 5.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    assert_valid_for_all_methods(&poly);
}

#[test]
fn test_all_methods_concave() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 5.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 5.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    assert_valid_for_all_methods(&poly);
}

// =========================================================================
// SECTION 2: Stress tests
// =========================================================================

#[test]
fn test_stress_many_vertices() {
    let n = 1000;
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

#[test]
fn test_stress_many_holes() {
    let mut holes = Vec::new();
    for i in 0..10 {
        let x0 = 2.0 + i as f64 * 8.0;
        let y0 = 2.0;
        holes.push(LineString::new(vec![
            Coord { x: x0, y: y0 },
            Coord { x: x0 + 3.0, y: y0 },
            Coord {
                x: x0 + 3.0,
                y: y0 + 3.0,
            },
            Coord { x: x0, y: y0 + 3.0 },
            Coord { x: x0, y: y0 },
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
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 3: Large coordinate stress tests
// =========================================================================

#[test]
fn test_stress_large_coords() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 1e7, y: 1e7 },
            Coord {
                x: 1e7 + 1000.0,
                y: 1e7,
            },
            Coord {
                x: 1e7 + 1000.0,
                y: 1e7 + 1000.0,
            },
            Coord {
                x: 1e7,
                y: 1e7 + 1000.0,
            },
            Coord { x: 1e7, y: 1e7 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_stress_near_zero() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 1e-10, y: 1e-10 },
            Coord {
                x: 1e-10 + 1e-8,
                y: 1e-10,
            },
            Coord {
                x: 1e-10 + 1e-8,
                y: 1e-10 + 1e-8,
            },
            Coord {
                x: 1e-10,
                y: 1e-10 + 1e-8,
            },
            Coord { x: 1e-10, y: 1e-10 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
}

// =========================================================================
// SECTION 4: Negative coordinate stress
// =========================================================================

#[test]
fn test_stress_negative_coords() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord {
                x: -1000.0,
                y: -1000.0,
            },
            Coord {
                x: 1000.0,
                y: -1000.0,
            },
            Coord {
                x: 1000.0,
                y: 1000.0,
            },
            Coord {
                x: -1000.0,
                y: 1000.0,
            },
            Coord {
                x: -1000.0,
                y: -1000.0,
            },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_stress_all_negative() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: -50.0, y: -50.0 },
            Coord { x: -10.0, y: -50.0 },
            Coord { x: -10.0, y: -10.0 },
            Coord { x: -50.0, y: -10.0 },
            Coord { x: -50.0, y: -50.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 5: Configuration edge cases
// =========================================================================

#[test]
fn test_explicit_default_config() {
    let config = MakeValidConfig {
        poly_method: PolyMethod::Auto,
        keep_collapsed: false,
        fill_rule: geo::algorithm::bool_ops::FillRule::EvenOdd,
        ..Default::default()
    };
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config);
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_arrange_explicit() {
    let config = MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        keep_collapsed: false,
        fill_rule: geo::algorithm::bool_ops::FillRule::EvenOdd,
        ..Default::default()
    };
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config);
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_structure_explicit() {
    let config = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        keep_collapsed: false,
        fill_rule: geo::algorithm::bool_ops::FillRule::EvenOdd,
        ..Default::default()
    };
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config);
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 6: Extreme winding orders
// =========================================================================

#[test]
fn test_cw_square_arrange() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_arrange());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_cw_square_structure() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_cw_hole_arrange() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 20.0, y: 0.0 },
            Coord { x: 20.0, y: 20.0 },
            Coord { x: 0.0, y: 20.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 5.0, y: 15.0 },
            Coord { x: 15.0, y: 15.0 },
            Coord { x: 15.0, y: 5.0 },
            Coord { x: 5.0, y: 5.0 },
        ])],
    );
    let result = poly.make_valid_with_config(&cfg_arrange());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 7: Polygon with adjacent-but-not-nested rings from bowtie split
// =========================================================================

#[test]
fn test_bowtie_reversed_vertices() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 8: Spike polygon
// =========================================================================

#[test]
fn test_polygon_with_spike_arrange() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_arrange());
    assert_valid(&result);
}

#[test]
fn test_polygon_with_spike_structure() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid(&result);
}

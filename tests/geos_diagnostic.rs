//! Compare GeoRepair output against GEOS makeValid for each failing input.
//! Spits WKT for geosop: cargo test --test geos_diagnostic 2>&1 | grep WKT

use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig};

#[test]
fn geos_compare_failing_seeds() {
    // Each entry: (test_name, coords_as_f64_2d)
    let cases: Vec<(&str, Vec<[f64; 2]>)> = vec![
        // DegenerateExterior: collinear_ring
        ("collinear_ring", vec![[0.0, 0.0], [40.10194631387683, 0.0], [0.0, 0.0], [0.0, 0.0]]),
        // DegenerateExterior: empty_geometries
        ("empty_geometries", vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]),
        // DegenerateExterior: flat_ring
        ("flat_ring", vec![[0.0, 0.0], [40.10194631387683, 0.0], [0.0, 0.0], [0.0, 0.0]]),
        // RingTooFewPoints found 1: coord_wrap_around
        ("coord_wrap_around", vec![[0.0, 0.0], [1e-5, 0.0], [0.0, 1e-5]]),
        // RingTooFewPoints found 1: denormal_coords
        ("denormal_coords", vec![[0.0, 0.0], [0.0, 4.854e-301], [0.0, 8.442e-301]]),
        // NestedHoles: barely_closed_ring
        ("barely_closed_ring", vec![
            [-45.02527790548379, 73.82889649367472],
            [66.78007044676131, 0.0],
            [-86.24311858707979, 61.71143828608508],
            [76.81983592697404, 70.51446481261368],
            [0.0, 0.0],
        ]),
    ];

    let cfg = MakeValidConfig::default();

    for (name, coords) in &cases {
        let pts: Vec<Coord<f64>> = coords.iter().map(|&[x, y]| Coord { x, y }).collect();
        let mut ring = pts.clone();
        if ring.first() != ring.last() {
            ring.push(ring[0]);
        }
        let poly = Polygon::new(LineString::new(ring), Vec::new());
        let result = poly.make_valid_with_config(&cfg);
        let valid = result.validate().valid;

        // WKT output for GEOS testing
        let wkt = coords_to_wkt(coords);
        eprintln!("WKT|{name}|{wkt}|repair_valid={valid}|errors={:?}",
            if !valid { result.validate().errors } else { vec![] });
    }
}

fn coords_to_wkt(coords: &[[f64; 2]]) -> String {
    let pts: Vec<String> = coords.iter()
        .map(|[x, y]| format!("{x} {y}"))
        .collect();
    format!("POLYGON(({}))", pts.join(", "))
}

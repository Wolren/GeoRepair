use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

// Regression: fuzz found Auto mode shipping a LINESTRING that fails our own
// validator, for a mixed-magnitude ring (1e15 scale + 3.5e-236 coordinate).
#[test]
fn fuzz_mixed_magnitude_collapse_linestring_valid() {
    let ring = vec![
        Coord { x: 1e15, y: 1e15 },
        Coord { x: 1e15, y: 1e15 + 1.0 },
        Coord { x: 3.4917537446497764e-236, y: 999999999574016.0 },
        Coord { x: 1e15 + 1.0, y: 1e15 },
    ];
    let poly = Polygon::new(LineString::new(ring), Vec::new());

    for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        let cfg = MakeValidConfig { poly_method: method, ..Default::default() };
        let out = poly.make_valid_with_config(&cfg);
        assert!(
            out.validate().valid,
            "invalid output in mode {method:?}: {out:?} (input {poly:?})"
        );
    }
}

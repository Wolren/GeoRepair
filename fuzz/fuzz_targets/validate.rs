//! Fuzz the validator itself on arbitrary single-ring polygons.
//!
//! Contracts:
//!   1. validate() must never panic on arbitrary (including NaN/Inf)
//!      coordinate data.
//!   2. Consistency: if validate() reports the input valid, the repair must
//!      preserve it: a polygon we call valid must not be emptied or
//!      decomposed by make_valid (modulo OGC winding normalization).
#![no_main]
use libfuzzer_sys::fuzz_target;

use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fuzz_target!(|data: &[u8]| {
    if data.len() < 32 || data.len() > 16 * 64 || data.len() % 16 != 0 {
        return;
    }
    let mut coords: Vec<Coord<f64>> = Vec::with_capacity(data.len() / 16 + 1);
    for chunk in data.chunks_exact(16) {
        let mut xb = [0u8; 8];
        let mut yb = [0u8; 8];
        xb.copy_from_slice(&chunk[0..8]);
        yb.copy_from_slice(&chunk[8..16]);
        coords.push(Coord {
            x: f64::from_le_bytes(xb),
            y: f64::from_le_bytes(yb),
        });
    }
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    let poly = Polygon::new(LineString::new(coords), Vec::new());

    // Contract 1: no panic in the validator.
    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poly.validate()));
    let verdict = match verdict {
        Ok(v) => v,
        Err(_) => panic!("validate() panicked on arbitrary input"),
    };

    // Contract 2: valid inputs must not be destroyed by repair.
    if verdict.valid {
        let cfg = MakeValidConfig { poly_method: PolyMethod::Auto, ..Default::default() };
        let out = poly.make_valid_with_config(&cfg);
        assert!(
            matches!(
                &out,
                geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_)
            ),
            "valid polygon was collapsed to {out:?}"
        );
        assert!(out.validate().valid, "repair of valid input shipped invalid output");
    }
});

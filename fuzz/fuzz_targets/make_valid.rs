//! Fuzz the full repair pipeline (all three strategy modes) on arbitrary
//! single-ring polygons built from raw little-endian f64 coordinate pairs.
//!
//! The input buffer is interpreted as a stream of (x, y) pairs. The repair
//! contract under fuzz:
//!   1. make_valid must never panic (panic containment guards the foreign
//!      i_overlay boolean path; a panic here means the guard missed a path).
//!   2. The output must be valid per our own validator in every mode (the
//!      "valid or empty" dispatch contract: Auto, Arrange, Structure all
//!      funnel through the gated chain).
//!
//! The dense-repair and i_overlay paths are the primary targets: mixed
//! magnitudes, self-crossings, collinear spikes, and near-degenerate
//! coordinates exercise the boolean fallback that proptest reaches rarely.
#![no_main]
use libfuzzer_sys::fuzz_target;

use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn coords_from_bytes(data: &[u8]) -> Vec<Coord<f64>> {
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
    coords
}

fuzz_target!(|data: &[u8]| {
    // 2..=32 coordinates (a closed ring needs >= 3 distinct points; larger
    // inputs are returned early to keep per-input repair time bounded).
    if data.len() < 32 || data.len() > 16 * 32 || data.len() % 16 != 0 {
        return;
    }
    let mut coords = coords_from_bytes(data);
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    let poly = Polygon::new(LineString::new(coords), Vec::new());

    for method in [PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let cfg = MakeValidConfig { poly_method: method, ..Default::default() };
        // The library contains panic containment; a panic escaping here
        // means a dispatch site or FFI-adjacent path is not covered.
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            poly.make_valid_with_config(&cfg)
        }));
        let out = match out {
            Ok(g) => g,
            Err(_) => {
                panic!("make_valid panicked on mode {method:?}");
            }
        };
        // Valid-or-empty contract: no dispatch arm may ship geometry our
        // validator rejects.
        assert!(out.validate().valid, "invalid output in mode {method:?}: {out:?}");
    }
});

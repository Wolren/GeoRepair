//! Fuzz the WKB parse + repair pipeline: arbitrary bytes are interpreted
//! as a WKB document (single geometry), parsed by our own reader, and
//! repaired.
//!
//! Complements wkt_repair: WKB carries count fields (rings, points,
//! sub-geometries) that drive allocations, so it exercises a different
//! corruption class - oversized counts must be rejected (InconsistentCount),
//! never allocated (the 2026-08-04 OOM abort was a crafted MultiPoint
//! count requesting 120 GB).
#![no_main]
use libfuzzer_sys::fuzz_target;

use geo_repair::io::wkb::read_wkb;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fuzz_target!(|data: &[u8]| {
    let parsed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(data))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkb panicked on {} bytes", data.len()),
    };
    let Ok(geom) = parsed else {
        return; // parse rejection is fine
    };
    for method in [PolyMethod::Auto, PolyMethod::Structure] {
        let cfg = MakeValidConfig { poly_method: method, ..Default::default() };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            geom.make_valid_with_config(&cfg)
        }));
        let out = match out {
            Ok(g) => g,
            Err(_) => panic!("make_valid panicked on WKB in mode {method:?}"),
        };
        assert!(out.validate().valid, "invalid output in mode {method:?}");
    }
});

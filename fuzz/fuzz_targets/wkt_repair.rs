//! Fuzz the WKT parse + repair pipeline: arbitrary bytes are interpreted as
//! a lossy-UTF-8 WKT document, parsed by our own reader, and repaired.
//!
//! This exercises the IO layer (read_wkt) together with the repair chain:
//! structured inputs (POLYGON with holes, MULTIPOLYGON, degenerate rings)
//! reach the boolean and CDT paths differently than raw coordinate streams.
#![no_main]
use libfuzzer_sys::fuzz_target;

use geo_repair::io::wkt::read_wkt;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fuzz_target!(|data: &[u8]| {
    let text: String = data.iter().map(|&b| b as char).collect();
    // Cap the document size the same way the coordinate targets do: a
    // single WKT ring can be arbitrarily large, but per-input time must stay
    // bounded.
    if text.len() > 4096 {
        return;
    }
    let parsed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(&text))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkt panicked on {text:?}"),
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
            Err(_) => panic!("make_valid panicked on WKT {text:?} in mode {method:?}"),
        };
        assert!(out.validate().valid, "invalid output in mode {method:?} for WKT {text:?}");
    }
});

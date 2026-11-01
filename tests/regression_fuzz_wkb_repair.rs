//! Regression: CI fuzz smoke found "invalid output in mode Auto" in
//! wkb_repair with this exact WKB input (crash-d94363571e4e15ac00c1f7891b5522fa6df7ea6a).
//! Mirrors the fuzz target contract: parse must not panic, repair must not
//! panic, and the output must validate.
use geo_repair::io::wkb::read_wkb;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn input_bytes() -> Vec<u8> {
    vec![
        1, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 20, 64, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 36, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]
}

#[test]
fn fuzz_wkb_repair_degenerate_line() {
    let bytes = input_bytes();
    let geom = read_wkb(&bytes).expect("parse");
    for method in [PolyMethod::Auto, PolyMethod::Structure] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let out = geom.make_valid_with_config(&cfg);
        assert!(
            out.validate().valid,
            "invalid output in mode {method:?}: {out:?}"
        );
    }
}

/// Second seed of the same class (crash-1299f6ac): y rises by denormal steps, so the line
/// looked monotone to the fast noder's exact comparison while the eps gate sees the last
/// vertex land back on the first. Repair must node the sliver, not echo the input.
#[test]
fn fuzz_wkb_repair_monotone_eps_return() {
    let bytes: [u8; 57] = [
        1, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 190, 0, 0,
        0, 0, 64, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 64, 0, 0, 0, 0, 0, 0, 36, 64, 0, 0,
    ];
    let geom = read_wkb(&bytes).expect("parse");
    for method in [PolyMethod::Auto, PolyMethod::Structure] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let out = geom.make_valid_with_config(&cfg);
        assert!(
            out.validate().valid,
            "invalid output in mode {method:?}: {out:?}"
        );
        let again = out.make_valid_with_config(&cfg);
        assert_eq!(again, out, "repair is not idempotent in mode {method:?}");
    }
}

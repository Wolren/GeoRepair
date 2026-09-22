//! Regression tests for the fuzz-nightly contract breaches observed on the
//! 2026-09-19, 2026-09-20, 2026-09-21 and 2026-09-22 nightly runs.
//!
//! Each test decodes a committed fuzz/corpus seed EXACTLY like the matching
//! fuzz target (raw little-endian f64 pairs, 16 bytes/coord, auto-close ring)
//! and runs the same asserts the fuzz target runs:
//!
//!   * validate target, contract 2: a polygon validate() calls valid must not
//!     be emptied or decomposed by make_valid (runs 35430371542 and
//!     35703833356: "valid polygon was collapsed to GEOMETRYCOLLECTION EMPTY").
//!   * make_valid target, contract 1: make_valid must never panic (run
//!     35499180884: i_overlay extract_ogc is_fill_top assertion escaped the
//!     containment guards).
//!   * make_valid target, contract 2: no dispatch arm may ship geometry our
//!     own validator rejects (run 35578735657: "invalid output in mode Auto").
//!
//! Windows cannot link the libFuzzer engine, so this harness is how the crash
//! seeds are replayed under a plain `cargo test`.
use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::path::PathBuf;

/// Byte decoder shared by both fuzz targets: raw LE f64 (x, y) pairs.
fn coords_from_bytes(data: &[u8]) -> Vec<Coord<f64>> {
    let mut coords: Vec<Coord<f64>> = Vec::with_capacity(data.len() / 16 + 1);
    for chunk in data.as_chunks::<16>().0 {
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
    coords
}

fn seed_path(target: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fuzz/corpus")
        .join(target)
        .join(name)
}

fn poly_from_seed(target: &str, name: &str) -> Polygon<f64> {
    let data = std::fs::read(seed_path(target, name)).expect("read corpus seed");
    let coords = coords_from_bytes(&data);
    Polygon::new(LineString::new(coords), Vec::new())
}

/// validate target contract 2: valid inputs must survive repair as a
/// Polygon/MultiPolygon that our own validator accepts.
fn assert_valid_input_not_collapsed(poly: &Polygon<f64>, label: &str) {
    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poly.validate()));
    let verdict = match verdict {
        Ok(v) => v,
        Err(_) => panic!("validate() panicked on {label}"),
    };
    assert!(
        verdict.valid,
        "corpus seed {label} is not reported valid, cannot pin the collapse class: {:?}",
        verdict.errors
    );
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    };
    let out = poly.make_valid_with_config(&cfg);
    assert!(
        matches!(&out, Geometry::Polygon(_) | Geometry::MultiPolygon(_)),
        "valid polygon ({label}) was collapsed to {out:?}"
    );
    assert!(
        out.validate().valid,
        "repair of valid input ({label}) shipped invalid output: {out:?}"
    );
}

#[test]
fn fuzz_validate_collapse_seed_huge_y() {
    let poly = poly_from_seed("validate", "regression_valid_collapse_gc_empty_a.bin");
    assert_valid_input_not_collapsed(&poly, "run 35430371542 seed");
}

#[test]
fn fuzz_validate_collapse_seed_subnormal_x() {
    let poly = poly_from_seed("validate", "regression_valid_collapse_gc_empty_b.bin");
    assert_valid_input_not_collapsed(&poly, "run 35703833356 seed");
}

/// make_valid target contracts 1 and 2: no panic, and every dispatch arm
/// (Auto, Arrange, Structure) ships output our validator accepts.
fn assert_all_modes_valid(poly: &Polygon<f64>, label: &str) {
    for method in [PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            poly.make_valid_with_config(&cfg)
        }));
        let out = match out {
            Ok(g) => g,
            Err(_) => panic!("make_valid panicked on {label} in mode {method:?}"),
        };
        assert!(
            out.validate().valid,
            "invalid output on {label} in mode {method:?}: {out:?}"
        );
    }
}

#[test]
fn fuzz_make_valid_i_overlay_panic_contained() {
    let poly = poly_from_seed("make_valid", "regression_i_overlay_fill_top_escape.bin");
    assert_all_modes_valid(&poly, "run 35499180884 seed");
}

#[test]
fn fuzz_make_valid_retrace_ring_output_valid() {
    let poly = poly_from_seed("make_valid", "regression_auto_retrace_ring.bin");
    assert_all_modes_valid(&poly, "run 35578735657 seed");
}

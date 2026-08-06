//! WebAssembly runtime tests (wasm-bindgen-test, run in Node or a browser).
//!
//! These prove the core engine actually RUNS on wasm32, not just compiles:
//! WKT/WKB round-trips, the OGC validator, repair, and (when the harness
//! has browser globals) the synchronous XHR fetch. The fetch test skips
//! itself in the Node harness, which has no `window`/`XMLHttpRequest`.
//!
//! Run: `cargo test --target wasm32-unknown-unknown --features
//! arrange,structure,simd,validate,wasm --test wasm` with the
//! wasm-bindgen-test-runner installed.

// Native test runs also compile this file (the crate's battery); without
// the gate the `geo_repair::wasm` module is configured out and the file
// fails to link. On non-wasm targets this compiles to an empty test crate.
#![cfg(target_arch = "wasm32")]

use geo::{Coord, LineString, Polygon};
use geo_repair::io::wkb;
use geo_repair::io::wkt;

fn bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![],
    )
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wkt_roundtrip_on_wasm() {
    let p = bowtie();
    let text = wkt::write_wkt(&geo::Geometry::Polygon(p.clone()));
    let back = wkt::read_wkt(&text).expect("WKT parse");
    match back {
        geo::Geometry::Polygon(q) => {
            assert_eq!(p.exterior().0.len(), q.exterior().0.len());
            for (a, b) in p.exterior().0.iter().zip(q.exterior().0.iter()) {
                assert_eq!(a.x, b.x);
                assert_eq!(a.y, b.y);
            }
        }
        other => panic!("expected polygon, got {other:?}"),
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn wkb_roundtrip_on_wasm() {
    let p = bowtie();
    let bytes = wkb::write_wkb(&geo::Geometry::Polygon(p.clone()));
    let back = wkb::read_wkb(&bytes).expect("WKB parse");
    match back {
        geo::Geometry::Polygon(q) => {
            assert_eq!(p.exterior().0.len(), q.exterior().0.len());
            for (a, b) in p.exterior().0.iter().zip(q.exterior().0.iter()) {
                assert_eq!(a.x, b.x);
                assert_eq!(a.y, b.y);
            }
        }
        other => panic!("expected polygon, got {other:?}"),
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn validator_runs_on_wasm() {
    use geo_repair::GeoValidation;
    let p = bowtie();
    assert!(!p.is_valid(), "bowtie must be invalid");
    let ok = Polygon::new(
        LineString::from(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![],
    );
    assert!(ok.is_valid(), "square must be valid");
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn repair_runs_on_wasm() {
    let p = bowtie();
    let g = geo_repair::make_valid::make_valid_owned(p, &Default::default());
    use geo_repair::GeoValidation;
    let valid = match &g {
        geo::Geometry::Polygon(q) => q.is_valid(),
        geo::Geometry::MultiPolygon(mp) => mp.0.iter().all(|q| q.is_valid()),
        other => panic!("unexpected repair output {other:?}"),
    };
    assert!(valid, "repaired bowtie must be valid");
}

/// A 100-vertex winding-2 star (pentagram-like, self-intersecting ring).
/// Exercises the monotone-chain STR sweep, noding, and face walking on
/// wasm - not just the tiny-poly fast path the bowtie covers.
#[wasm_bindgen_test::wasm_bindgen_test]
fn repair_medium_poly_on_wasm() {
    let n = 100u32;
    let mut ring = Vec::with_capacity(n as usize + 1);
    for i in 0..=n {
        let t = i as f64 * 4.0 * std::f64::consts::PI / n as f64; // winding 2
        let r = 10.0 + 3.0 * (i as f64 * 2.0 * std::f64::consts::PI / n as f64).sin();
        ring.push(Coord {
            x: r * t.cos(),
            y: r * t.sin(),
        });
    }
    let p = Polygon::new(LineString::from(ring), vec![]);
    let g = geo_repair::make_valid::make_valid_owned(p, &Default::default());
    use geo_repair::GeoValidation;
    let valid = match &g {
        geo::Geometry::Polygon(q) => q.is_valid(),
        geo::Geometry::MultiPolygon(mp) => mp.0.iter().all(|q| q.is_valid()),
        other => panic!("unexpected repair output {other:?}"),
    };
    assert!(valid, "repaired star must be valid");
}

/// Fetch path: only meaningful where browser globals exist. The Node
/// wasm-bindgen-test harness has no `window`, so `fetch_geometry` returns
/// an Err mentioning it - that is the expected skip signal, not a test
/// failure.
#[wasm_bindgen_test::wasm_bindgen_test]
fn fetch_geometry_available_in_browser() {
    let url = "data:text/plain;base64,UE9MWUdPTiAoKDAgMCwgMiAwLCAyIDIsIDAgMiwgMCAwKSk=";
    match geo_repair::wasm::fetch_geometry(url) {
        Ok(g) => {
            // Browser harness: the data: URL WKT must parse.
            match g {
                geo::Geometry::Polygon(_) => {}
                other => panic!("expected polygon, got {other:?}"),
            }
        }
        Err(e) => {
            // Node harness: no XMLHttpRequest/window - skip.
            assert!(
                e.contains("no window") || e.contains("XHR"),
                "unexpected fetch error: {e}"
            );
        }
    }
}

//! Replay every committed fuzz corpus seed through the repair contract
//! (parse must not panic, repair must not panic, output must validate),
//! for every target's corpus, in both Auto and Structure modes.
//! Mirrors the CI fuzz smoke assertions without libFuzzer (Windows cannot
//! link the libFuzzer engine - LNK2001 on the cdylib crate type).
//!
//! Each corpus is decoded the way its own target decodes it. `make_valid` and `validate`
//! hold raw little-endian f64 pairs, not WKB; reading them as WKB rejected every seed and
//! skipped it, which is why this replay never ran them.
use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::io::wkb::read_wkb;
use geo_repair::io::wkt::read_wkt;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::path::Path;

/// One repaired input, in the shape the `make_valid` and `validate` fuzz targets build.
fn polygon_from_raw_coords(bytes: &[u8], max_coords: usize) -> Option<Geometry<f64>> {
    if bytes.len() < 32 || !bytes.len().is_multiple_of(16) {
        return None;
    }
    let mut coords: Vec<Coord<f64>> = Vec::with_capacity(bytes.len() / 16 + 1);
    let (chunks, _) = bytes.as_chunks::<16>();
    for chunk in chunks {
        let mut xb = [0u8; 8];
        let mut yb = [0u8; 8];
        xb.copy_from_slice(&chunk[0..8]);
        yb.copy_from_slice(&chunk[8..16]);
        coords.push(Coord {
            x: f64::from_le_bytes(xb),
            y: f64::from_le_bytes(yb),
        });
    }
    if coords.len() < 3 || coords.len() > max_coords {
        return None;
    }
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    Some(Geometry::Polygon(Polygon::new(
        LineString::new(coords),
        Vec::new(),
    )))
}

/// The repair contract on one already-parsed geometry: no panic, output valid, and a valid
/// input whose area is not zero comes back with something in it.
fn replay_geometry(geom: &Geometry<f64>, label: &str) {
    let input_valid = geom.validate().valid;
    for method in [PolyMethod::Auto, PolyMethod::Structure] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            geom.make_valid_with_config(&cfg)
        }));
        let out = match out {
            Ok(g) => g,
            Err(_) => panic!("make_valid panicked on {label} in mode {method:?}"),
        };
        assert!(
            out.validate().valid,
            "invalid output on {label} in mode {method:?}: {out:?}"
        );
        if input_valid && !is_empty(geom) {
            assert!(
                !is_empty(&out),
                "valid input emptied on {label} in mode {method:?}"
            );
        }
    }
}

fn is_empty(g: &Geometry<f64>) -> bool {
    match g {
        Geometry::Point(p) => !p.x().is_finite() || !p.y().is_finite(),
        Geometry::LineString(ls) => ls.0.len() < 2,
        Geometry::MultiLineString(mls) => mls.0.iter().all(|ls| ls.0.len() < 2),
        Geometry::Polygon(p) => p.exterior().0.len() < 4,
        Geometry::MultiPolygon(mp) => mp.0.iter().all(|p| p.exterior().0.len() < 4),
        Geometry::MultiPoint(mp) => mp.0.is_empty(),
        Geometry::GeometryCollection(gc) => gc.0.iter().all(is_empty),
        _ => false,
    }
}

fn replay_wkb(bytes: &[u8], label: &str) {
    let geom = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(bytes))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkb panicked on {label} ({} bytes)", bytes.len()),
    };
    let Ok(geom) = geom else { return };
    replay_geometry(&geom, label);
}

fn replay_wkt(bytes: &[u8], label: &str) {
    let text: String = bytes.iter().map(|&b| b as char).collect();
    if text.len() > 4096 {
        return;
    }
    let geom = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(&text))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkt panicked on {label}"),
    };
    let Ok(geom) = geom else { return };
    replay_geometry(&geom, label);
}

#[test]
fn replay_all_corpus_seeds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus");
    let mut n = 0;
    let mut skipped = 0;
    for entry in std::fs::read_dir(&root).expect("corpus dir") {
        let dir = entry.expect("entry").path();
        if !dir.is_dir() {
            continue;
        }
        let target = dir.file_name().unwrap().to_string_lossy().to_string();
        for seed in std::fs::read_dir(&dir).expect("seed dir") {
            let p = seed.expect("seed").path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let label = format!("{target}/{name}");
            let bytes = std::fs::read(&p).expect("read seed");
            let decoded = match target.as_str() {
                "make_valid" => polygon_from_raw_coords(&bytes, 32),
                "validate" => polygon_from_raw_coords(&bytes, 64),
                "wkb_repair" => {
                    replay_wkb(&bytes, &label);
                    n += 1;
                    continue;
                }
                "wkt_repair" => {
                    replay_wkt(&bytes, &label);
                    n += 1;
                    continue;
                }
                _ if name.ends_with(".wkt") => {
                    replay_wkt(&bytes, &label);
                    n += 1;
                    continue;
                }
                _ => {
                    replay_wkb(&bytes, &label);
                    n += 1;
                    continue;
                }
            };
            match decoded {
                Some(geom) => {
                    replay_geometry(&geom, &label);
                    n += 1;
                }
                None => skipped += 1,
            }
        }
    }
    eprintln!("replayed {n} corpus seeds, skipped {skipped} outside a target's input shape");
    assert!(
        n >= 70,
        "replayed only {n} seeds: the corpus was not read as intended"
    );
    assert_eq!(
        skipped, 0,
        "skipped {skipped} seeds: a corpus is being decoded wrongly"
    );
}

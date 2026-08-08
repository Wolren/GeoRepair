//! Replay every committed fuzz corpus seed through the repair contract
//! (parse must not panic, repair must not panic, output must validate),
//! for every target's corpus, in both Auto and Structure modes.
//! Mirrors the CI fuzz smoke assertions without libFuzzer (Windows cannot
//! link the libFuzzer engine - LNK2001 on the cdylib crate type).
use geo_repair::io::wkb::read_wkb;
use geo_repair::io::wkt::read_wkt;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::path::Path;

fn replay_wkb(bytes: &[u8], label: &str) {
    let geom = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(bytes))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkb panicked on {label} ({} bytes)", bytes.len()),
    };
    let Ok(geom) = geom else { return };
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
    }
}

fn replay_wkt(text: &str, label: &str) {
    let geom = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(text))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkt panicked on {label}"),
    };
    let Ok(geom) = geom else { return };
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
    }
}

#[test]
fn replay_all_corpus_seeds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus");
    let mut n = 0;
    for entry in std::fs::read_dir(&root).expect("corpus dir") {
        let dir = entry.expect("entry").path();
        if !dir.is_dir() {
            continue;
        }
        for seed in std::fs::read_dir(&dir).expect("seed dir") {
            let p = seed.expect("seed").path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".bin") {
                let bytes = std::fs::read(&p).expect("read seed");
                replay_wkb(
                    &bytes,
                    &format!("{}/{}", dir.file_name().unwrap().to_string_lossy(), name),
                );
            } else if name.ends_with(".wkt") {
                let text = std::fs::read_to_string(&p).expect("read seed");
                replay_wkt(
                    &text,
                    &format!("{}/{}", dir.file_name().unwrap().to_string_lossy(), name),
                );
            }
            n += 1;
        }
    }
    eprintln!("replayed {n} corpus seeds");
}

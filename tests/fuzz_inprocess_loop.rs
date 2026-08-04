use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

// In-process mini-fuzzer mirroring fuzz/fuzz_targets/make_valid.rs: run the
// committed corpus seeds plus deterministic byte mutations through the
// repair pipeline in one process. The CI libFuzzer run crashed with a
// deadly signal on the mixed-magnitude ring below AFTER other inputs ran
// in the same process; standalone it passes. This test tries to reproduce
// the prior-state crash (heap corruption / stack overflow) in release mode.
#[test]
fn fuzz_inprocess_mutation_loop() {
    let crash_bytes: Vec<u8> = vec![
        255, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 64, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0,
        0, 0, 248, 63, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 128, 0, 0, 0, 248, 63, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 248, 63, 0, 0, 0, 0, 0, 0, 8, 64, 0, 0, 0, 0, 0, 0, 240, 191,
        0, 0, 0, 0, 0, 0, 18, 64, 0, 0, 0, 0, 0, 0, 8, 192, 0, 0, 0, 0, 0, 0, 0, 0, 0, 248,
        63, 0, 0, 0, 0, 255, 0, 8, 64, 0, 0, 0, 0, 0, 0, 240, 191, 0, 0, 0, 0, 0, 0, 18, 64,
        0, 0, 0, 0, 252, 255, 7, 0, 0, 0, 0, 248, 63, 0, 0, 0, 0, 0, 0, 0, 192, 0, 0, 5, 0, 0,
        0, 248, 63, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 20, 64,
    ];
    let mut corpus: Vec<Vec<u8>> = vec![crash_bytes.clone()];
    let dir = std::path::Path::new("fuzz/corpus/make_valid");
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(b) = std::fs::read(e.path()) {
                corpus.push(b);
            }
        }
    }
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut rng = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    let run_one = |data: &[u8]| {
        if data.len() < 32 || data.len() > 16 * 64 || data.len() % 16 != 0 {
            return;
        }
        let mut coords: Vec<Coord<f64>> = Vec::with_capacity(data.len() / 16 + 1);
        for chunk in data.chunks_exact(16) {
            let mut xb = [0u8; 8];
            let mut yb = [0u8; 8];
            xb.copy_from_slice(&chunk[0..8]);
            yb.copy_from_slice(&chunk[8..16]);
            coords.push(Coord { x: f64::from_le_bytes(xb), y: f64::from_le_bytes(yb) });
        }
        if coords.first() != coords.last() {
            coords.push(coords[0]);
        }
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        for method in [PolyMethod::Auto, PolyMethod::Structure] {
            let cfg = MakeValidConfig { poly_method: method, ..Default::default() };
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                poly.make_valid_with_config(&cfg)
            }));
            match out {
                Ok(g) => {
                    assert!(g.validate().valid, "invalid output: {g:?}");
                }
                Err(_) => {}
            }
        }
    };

    for seed in &corpus {
        run_one(seed);
        for _ in 0..64 {
            let mut m = seed.clone();
            let n = m.len();
            for _ in 0..(rng() % 4 + 1) {
                let idx = (rng() % n as u64) as usize;
                m[idx] ^= (rng() & 0xff) as u8;
            }
            run_one(&m);
        }
    }
    // Final pass: the crash input after all the prior state.
    run_one(&crash_bytes);
}

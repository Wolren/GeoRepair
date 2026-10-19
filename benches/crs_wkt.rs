//! CRS WKT benchmark: apples-to-apples WKT1 vs WKT2, parse and serialize.
//!
//! The *same* CRS is encoded both ways, so the only thing under test is the
//! serialization:
//!
//! | encoding | writer |
//! |----------|--------|
//! | WKT2 | [`Crs::to_wkt2`] (ISO 19162:2019 / OGC 18-010r11) |
//! | WKT1 | [`Crs::to_esri_wkt`] (ESRI-flavoured `.prj` WKT1) |
//!
//! Each measurement warms up and then runs many short timed batches,
//! reporting **best (min)** and **median** nanoseconds per operation.
//! Best-of-batches is the statistic that makes regressions visible: machine
//! drift, thermal throttling and noisy neighbours only ever make a batch
//! *slower*, so the minimum across batches approximates the true cost and is
//! reproducible run to run — unlike a single sequential timing.
//!
//! # Running
//!
//! ```text
//! cargo bench --bench crs_wkt
//! ```
//!
//! Write the gate input (JSONL, `label`/`par_us` like the other benches):
//!
//! ```text
//! BENCH_JSON=target/crs_wkt.json cargo bench --bench crs_wkt
//! ```
//!
//! Self-contained regression gate — pass a baseline file and the bench
//! compares `min_ns` per case, failing on a regression past the threshold.
//! When the file does not exist yet it is *recorded* (same convention as
//! `scripts/bench_gate.py`):
//!
//! ```text
//! CRS_WKT_BASELINE=benches/crs_wkt_baseline.txt cargo bench --bench crs_wkt
//! ```
use std::hint::black_box;
use std::time::Instant;

use geo_repair::Crs;

/// Timed batches per measurement (the minimum is taken across these).
const REPS: u32 = 15;
/// Warm-up calls before the first timed batch.
const WARMUP: u32 = 1_000;
/// Smallest batch, so a timed batch dominates `Instant` resolution even for
/// sub-microsecond operations.
const MIN_BATCH: u32 = 20_000;
/// A case may be at most this much slower than its baseline before it fails.
const THRESHOLD: f64 = 0.20;

/// Time `f` with a warm-up and `REPS` timed batches, returning
/// `(min, median)` nanoseconds per call.
fn measure(mut f: impl FnMut()) -> (f64, f64) {
    for _ in 0..WARMUP {
        f();
    }
    let start = Instant::now();
    for _ in 0..MIN_BATCH {
        f();
    }
    let per_op_ns = start.elapsed().as_secs_f64() * 1e9 / f64::from(MIN_BATCH);
    let iters = (2_000_000.0 / per_op_ns.max(1.0)).clamp(1_000.0, 500_000.0) as u32;

    let mut samples: Vec<f64> = Vec::with_capacity(REPS as usize);
    for _ in 0..REPS {
        let start = Instant::now();
        for _ in 0..iters {
            f();
        }
        samples.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(iters));
    }
    samples.sort_by(f64::total_cmp);
    (samples[0], samples[samples.len() / 2])
}

struct Row {
    label: String,
    min_ns: f64,
    med_ns: f64,
    bytes: usize,
}

fn main() {
    let crss = [
        ("EPSG:4326", Crs::from_epsg(4326)),
        ("EPSG:4269", Crs::from_epsg(4269)),
        ("EPSG:3857", Crs::from_epsg(3857)),
    ];

    let mut rows: Vec<Row> = Vec::new();
    for (name, crs) in &crss {
        let wkt2 = crs.to_wkt2().expect("bundled WKT2 definition");
        let wkt1 = crs.to_esri_wkt().expect("WKT1 definition");

        let (min_ns, med_ns) = measure(|| {
            let _ = black_box(Crs::from_wkt(black_box(&wkt2)));
        });
        rows.push(Row {
            label: format!("parse {name} wkt2"),
            min_ns,
            med_ns,
            bytes: wkt2.len(),
        });

        let (min_ns, med_ns) = measure(|| {
            let _ = black_box(Crs::from_wkt(black_box(&wkt1)));
        });
        rows.push(Row {
            label: format!("parse {name} wkt1"),
            min_ns,
            med_ns,
            bytes: wkt1.len(),
        });

        let (min_ns, med_ns) = measure(|| {
            let _ = black_box(crs.to_wkt2());
        });
        rows.push(Row {
            label: format!("write {name} wkt2"),
            min_ns,
            med_ns,
            bytes: wkt2.len(),
        });

        let (min_ns, med_ns) = measure(|| {
            let _ = black_box(crs.to_esri_wkt());
        });
        rows.push(Row {
            label: format!("write {name} wkt1"),
            min_ns,
            med_ns,
            bytes: wkt1.len(),
        });
    }

    println!();
    println!("CRS WKT — best-of-{REPS} (min) and median, nanoseconds per call");
    println!(
        "{:<22}{:>12}{:>12}{:>9}{:>11}",
        "case", "min ns", "med ns", "bytes", "ns/byte"
    );
    println!("{}", "-".repeat(66));
    for r in &rows {
        let per_byte = r.min_ns / r.bytes.max(1) as f64;
        println!(
            "{:<22}{:>12.1}{:>12.1}{:>9}{:>11.3}",
            r.label, r.min_ns, r.med_ns, r.bytes, per_byte
        );
    }

    println!();
    println!("WKT2 vs WKT1 — same CRS, parse only:");
    for (name, _) in &crss {
        let l1 = format!("parse {name} wkt1");
        let l2 = format!("parse {name} wkt2");
        let w1 = rows.iter().find(|r| r.label == l1).expect("wkt1 row");
        let w2 = rows.iter().find(|r| r.label == l2).expect("wkt2 row");
        let pb1 = w1.min_ns / w1.bytes.max(1) as f64;
        let pb2 = w2.min_ns / w2.bytes.max(1) as f64;
        println!(
            "  {name:<10} wkt2 {:9.1} ns ({:>4} B, {:6.3} ns/B)  |  wkt1 {:8.1} ns ({:>3} B, {:6.3} ns/B)  |  {:.2}x total, {:.2}x per byte",
            w2.min_ns,
            w2.bytes,
            pb2,
            w1.min_ns,
            w1.bytes,
            pb1,
            w2.min_ns / w1.min_ns,
            pb2 / pb1,
        );
    }

    if let Ok(path) = std::env::var("BENCH_JSON") {
        let mut out = String::new();
        for r in &rows {
            // is a batch of 1000 calls
            out.push_str(&format!(
                "{{\"label\":\"{}\",\"par_us\":{:.3},\"min_ns\":{:.3},\"med_ns\":{:.3},\"bytes\":{}}}\n",
                r.label, r.min_ns, r.min_ns, r.med_ns, r.bytes
            ));
        }
        std::fs::write(&path, out).expect("write BENCH_JSON");
        println!();
        println!("wrote {path}");
    }

    if let Ok(path) = std::env::var("CRS_WKT_BASELINE") {
        gate(&path, &rows);
    }
}

/// Compare `rows` against a `label<TAB>min_ns` baseline, recording it when the
/// file does not exist yet and exiting non-zero on a regression.
fn gate(path: &str, rows: &[Row]) {
    let baseline_path = std::path::Path::new(path);
    if !baseline_path.exists() {
        let mut text = String::new();
        for r in rows {
            text.push_str(&format!("{}\t{:.3}\n", r.label, r.min_ns));
        }
        std::fs::write(baseline_path, text).expect("record baseline");
        println!();
        println!(
            "CRS-WKT-GATE: recorded {path} ({} cases) — pass",
            rows.len()
        );
        return;
    }

    let text = std::fs::read_to_string(baseline_path).expect("read baseline");
    let mut failed = false;
    for r in rows {
        let mut base = None;
        for line in text.lines() {
            if let Some((label, value)) = line.split_once('\t')
                && label == r.label.as_str()
            {
                base = value.trim().parse::<f64>().ok();
                break;
            }
        }
        match base {
            Some(base) => {
                let limit = base * (1.0 + THRESHOLD);
                if r.min_ns > limit {
                    failed = true;
                    println!(
                        "CRS-WKT-GATE: FAIL {}: {:.1} ns vs baseline {:.1} ns (limit {:.1} ns, +{:.0}%)",
                        r.label,
                        r.min_ns,
                        base,
                        limit,
                        THRESHOLD * 100.0
                    );
                }
            }
            None => println!("CRS-WKT-GATE: new case {} — not in baseline", r.label),
        }
    }
    if failed {
        println!("CRS-WKT-GATE: FAILED — a parsing/serialization regression was detected");
        std::process::exit(1);
    }
    println!(
        "CRS-WKT-GATE: pass ({} cases within +{:.0}%)",
        rows.len(),
        THRESHOLD * 100.0
    );
}

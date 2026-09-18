//! Temp benchmark harness (not committed): generate a large real-data WKT
//! file from data_0.bin and time read_wkt over it.
//!
//! gen:   cargo run --release --example wkt_bench_big -- gen <out> [max_polys]
//! bench: cargo run --release --example wkt_bench_big -- bench <path> [iters]

use std::hint::black_box;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "gen" => {
            let polys = geo_repair::io::load_bin(
                r"D:\Projects\rust\GeoRepair\benches\real_world\data_0.bin",
            )
            .expect("load_bin");
            let max: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40_000);
            let mut text = String::with_capacity(1 << 26);
            for p in polys.into_iter().take(max) {
                text.push_str(&geo_repair::write_wkt(&geo::Geometry::Polygon(p)));
                text.push('\n');
            }
            std::fs::write(&args[2], text.as_bytes()).unwrap();
            eprintln!("GEN {} bytes {} polys", text.len(), max);
        }
        "bench" => {
            let text = std::fs::read_to_string(&args[2]).unwrap();
            let iters: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
            let n_lines = text.lines().count();
            eprintln!(
                "BENCH georepair {} bytes {} lines {} measured-iters",
                text.len(),
                n_lines,
                iters
            );
            let mut times = Vec::new();
            for it in 0..iters + 3 {
                let t0 = Instant::now();
                for line in text.lines() {
                    black_box(geo_repair::read_wkt(line).expect("parse"));
                }
                if it >= 3 {
                    times.push(t0.elapsed());
                }
            }
            report(&mut times);
        }
        _ => panic!("usage: gen | bench"),
    }
}

fn report(times: &mut [std::time::Duration]) {
    times.sort();
    let ms = |d: &std::time::Duration| d.as_secs_f64() * 1e3;
    let rounded: Vec<f64> = times
        .iter()
        .map(|d| (ms(d) * 10.0).round() / 10.0)
        .collect();
    eprintln!("runs(ms): {rounded:?}");
    eprintln!(
        "min {:.1} ms | median {:.1} ms",
        ms(&times[0]),
        ms(&times[times.len() / 2])
    );
}

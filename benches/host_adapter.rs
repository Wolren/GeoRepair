//! Cost of the delivery adapter, against the older two-step chain and a bare clone.
//!
//! Run: cargo bench --bench host_adapter
//! BENCH_FILE selects the corpus, same chain as wkb_cmp.

use std::hint::black_box;
use std::time::Instant;

use geo::{Geometry, Polygon};
use geo_repair::host::to_host;
use geo_repair::output::{OutputPolicy, apply_policy, sanitize};

const DATASET: &str = "benches/real_world/data_0.bin";

fn load() -> Vec<Geometry<f64>> {
    if let Ok(path) = std::env::var("BENCH_FILE") {
        let polys = geo_repair::io::load_bin(&path).expect("load BENCH_FILE");
        return polys.into_iter().map(Geometry::Polygon).collect();
    }
    if let Ok(polys) = geo_repair::io::load_bin(DATASET) {
        return polys.into_iter().map(Geometry::Polygon).collect();
    }
    let polys = geo_repair::io::load_bin("benches/real_world/alaska.bin").expect("load alaska");
    polys.into_iter().map(Geometry::Polygon).collect()
}

fn best_of_three(mut run: impl FnMut()) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let t = Instant::now();
        run();
        best = best.min(t.elapsed().as_secs_f64());
    }
    best
}

fn main() {
    let geoms = load();
    let n = geoms.len();
    let rings: usize = geoms
        .iter()
        .map(|g| match g {
            Geometry::Polygon(p) => p.interiors().len() + 1,
            Geometry::MultiPolygon(mp) => {
                mp.0.iter()
                    .map(|p: &Polygon<f64>| p.interiors().len() + 1)
                    .sum()
            }
            _ => 1,
        })
        .sum();
    println!("[{n} geometries, {rings} rings]");

    let clone_s = best_of_three(|| {
        for g in &geoms {
            black_box(g.clone());
        }
    });

    let mut kept = 0usize;
    let mut dropped_degenerate = 0usize;
    let mut dropped_nonfinite = 0usize;
    let mut flattened = 0usize;
    let adapter_s = best_of_three(|| {
        let mut acc = 0usize;
        let mut deg = 0usize;
        let mut fin = 0usize;
        let mut flat = 0usize;
        for g in &geoms {
            let (out, report) = to_host(black_box(g));
            acc += usize::from(black_box(out).is_some());
            deg += report.dropped_degenerate;
            fin += report.dropped_nonfinite;
            flat += report.flattened_collections;
        }
        kept = acc;
        dropped_degenerate = deg;
        dropped_nonfinite = fin;
        flattened = flat;
        black_box(acc);
    });

    let chain_s = best_of_three(|| {
        let mut acc = 0usize;
        for g in &geoms {
            let (reduced, _) = apply_policy(black_box(g).clone(), OutputPolicy::TopDimension);
            acc += usize::from(black_box(sanitize(&reduced)).is_some());
        }
        black_box(acc);
    });

    let per = |s: f64| s * 1e9 / n as f64;
    println!();
    println!(
        "{:<24}{:>12}{:>12}{:>10}",
        "pass", "total ms", "ns/geom", "vs clone"
    );
    for (name, s) in [
        ("clone (floor)", clone_s),
        ("host::to_host", adapter_s),
        ("apply_policy+sanitize", chain_s),
    ] {
        println!(
            "{:<24}{:>12.2}{:>12.0}{:>9.2}x",
            name,
            s * 1e3,
            per(s),
            s / clone_s
        );
    }
    println!();
    println!(
        "adapter kept {kept}/{n}, dropped {dropped_degenerate} degenerate parts, \
         {dropped_nonfinite} non-finite parts, flattened {flattened} collections"
    );
}

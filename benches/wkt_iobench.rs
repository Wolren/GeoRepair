//! I/O format benchmark: .bin vs .wkb vs .wkt on data_0.
//!
//! Run: cargo bench --bench wkt_iobench
use std::time::Instant;

use geo::Geometry;
use geo_repair::io::load_bin;
use geo_repair::{read_wkb_concat, read_wkt, write_wkb, write_wkt};

const DATASET: &str = "benches/real_world/data_0.bin";

fn main() {
    let t0 = Instant::now();
    let polys = load_bin(DATASET).expect("load bin");
    let t_bin_load = t0.elapsed();
    let n = polys.len();
    eprintln!("[{n} polys]");

    let geoms: Vec<Geometry<f64>> = polys.into_iter().map(Geometry::Polygon).collect();

    let t0 = Instant::now();
    let mut wkb_buf = Vec::with_capacity(geoms.len() * 128);
    let mut wkt_lines = Vec::with_capacity(geoms.len());
    for g in &geoms {
        wkb_buf.extend_from_slice(&write_wkb(g));
        wkt_lines.push(write_wkt(g));
    }
    let wkt_text = wkt_lines.join("\n");
    let t_convert = t0.elapsed();

    let t0 = Instant::now();
    let wkb_geoms = read_wkb_concat(&wkb_buf).expect("read wkb concat");
    let t_wkb_load = t0.elapsed();
    assert_eq!(wkb_geoms.len(), n);

    let t0 = Instant::now();
    let mut wkt_geoms = Vec::with_capacity(n);
    for line in wkt_text.lines() {
        wkt_geoms.push(read_wkt(line).expect("wkt parse"));
    }
    let t_wkt_load = t0.elapsed();
    assert_eq!(wkt_geoms.len(), n);

    let bin_us = t_bin_load.as_secs_f64() * 1e6 / n as f64;
    let wkb_us = t_wkb_load.as_secs_f64() * 1e6 / n as f64;
    let wkt_us = t_wkt_load.as_secs_f64() * 1e6 / n as f64;
    let conv_ns = t_convert.as_nanos() as f64 / n as f64;

    eprintln!();
    eprintln!("╔═══════════════╤═════════════╤══════════════╗");
    eprintln!("║ Format        │ Total       │ Per polygon  ║");
    eprintln!("╠═══════════════╪═════════════╪══════════════╣");
    eprintln!(
        "║ .bin (native) │ {:8.3?}  │ {:8.0} ns  ║",
        t_bin_load,
        t_bin_load.as_nanos() as f64 / n as f64
    );
    eprintln!(
        "║ .wkb (concat) │ {:8.3?}  │ {:8.0} ns  ║",
        t_wkb_load,
        t_wkb_load.as_nanos() as f64 / n as f64
    );
    eprintln!(
        "║ .wkt (1/line) │ {:8.3?}  │ {:8.0} ns  ║",
        t_wkt_load,
        t_wkt_load.as_nanos() as f64 / n as f64
    );
    eprintln!("╠═══════════════╪═════════════╪══════════════╣");
    eprintln!(
        "║ Conversion    │ {:8.3?}  │ {:8.0} ns  ║",
        t_convert, conv_ns
    );
    eprintln!("╚═══════════════╧═════════════╧══════════════╝");
    eprintln!();
    eprintln!("WKT is {:.1}× slower than WKB to load", wkt_us / wkb_us);
    eprintln!("WKB is {:.1}× slower than .bin to load", wkb_us / bin_us);
}

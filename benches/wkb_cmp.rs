//! Performance comparison: our zero-dep WKB/WKT vs georust/wkb + georust/wkt.
//!
//! Features:
//! - Single warmup pass per category (was 3)
//! - Cross-tested reads (each parser on both formats — fair ground)
//! - Summary table with speedup ratios
//!
//! Run: cargo bench --bench wkb_cmp

use std::time::Instant;

use geo::Geometry;
use geo_repair::io::load_bin;
use geo_traits::to_geo::ToGeoGeometry;

const DATASET: &str = "benches/real_world/data_0.bin";
const WKT_RT_SUBSET: usize = 10_000;

fn main() {
    let t_start = Instant::now();

    // ── Load ──
    let t0 = Instant::now();
    let polys: Vec<Geometry<f64>> = load_bin(DATASET)
        .expect("load bin")
        .into_iter()
        .map(Geometry::Polygon)
        .collect();
    let n = polys.len();

    eprintln!("╔═══════════════════════════════════════════════╗");
    eprintln!("║  WKB/WKT: ours vs georust/wkb + georust/wkt ║");
    eprintln!("╚═══════════════════════════════════════════════╝");
    eprintln!();
    eprintln!("  dataset   {DATASET}");
    eprintln!("  polygons  {n}");
    eprintln!("  load      {:?}", t0.elapsed());
    eprintln!();

    // ── Pre-serialize WKB ──
    let t0 = Instant::now();
    let our_wkb: Vec<Vec<u8>> = polys.iter().map(geo_repair::write_wkb).collect();
    let their_wkb: Vec<Vec<u8>> = polys
        .iter()
        .map(|g| {
            use wkb::writer::{WriteOptions, geometry_wkb_size, write_geometry};
            let opts = WriteOptions::default();
            let size = geometry_wkb_size(g);
            let mut buf = Vec::with_capacity(size);
            write_geometry(&mut buf, g, &opts).unwrap();
            buf
        })
        .collect();
    eprintln!("  WKB ser      {:?}", t0.elapsed());

    // Verify roundtrip equivalence
    for (a, b) in our_wkb.iter().zip(their_wkb.iter()) {
        let our_rt = geo_repair::read_wkb(a).unwrap();
        let w = wkb::reader::read_wkb(b).unwrap();
        let their_rt: Geometry<f64> = w.to_geometry();
        assert_eq!(our_rt, their_rt, "WKB roundtrip mismatch");
    }
    eprintln!("  WKB verify   ✓");
    eprintln!();

    // ── Pre-serialize WKT ──
    let t0 = Instant::now();
    let our_wkt: Vec<String> = polys.iter().map(geo_repair::write_wkt).collect();
    let wkt_ser_our = t0.elapsed();

    let t0 = Instant::now();
    let their_wkt: Vec<String> = polys
        .iter()
        .map(|g| {
            use wkt::ToWkt;
            g.wkt_string()
        })
        .collect();
    let wkt_ser_their = t0.elapsed();

    eprintln!(
        "  our  WKT ser {:?}  ({:.0} ns/op)",
        wkt_ser_our,
        wkt_ser_our.as_nanos() as f64 / n as f64
    );
    eprintln!(
        "  their WKT ser {:?}  ({:.0} ns/op)",
        wkt_ser_their,
        wkt_ser_their.as_nanos() as f64 / n as f64
    );
    eprintln!();

    let mut results: Vec<(&str, f64, f64)> = Vec::new();

    // ═══════════════════════════════════════════════
    //  WKB
    // ═══════════════════════════════════════════════
    eprintln!("  ──── WKB ────");
    eprintln!();

    // ── WKB read (cross-tested) ──
    eprintln!("  ── Read (cross) ──");
    let warmup = |b: &[u8]| {
        let _ = geo_repair::read_wkb(b);
    };
    for b in &our_wkb {
        warmup(b);
    }

    let our_ro = bench_ns(&our_wkb, |b| {
        let _ = geo_repair::read_wkb(b).unwrap();
    });
    let our_rt = bench_ns(&their_wkb, |b| {
        let _ = geo_repair::read_wkb(b).unwrap();
    });
    let their_ro = bench_ns(&our_wkb, |b| {
        let w = wkb::reader::read_wkb(b).unwrap();
        let _: Geometry<f64> = w.to_geometry();
    });
    let their_tt = bench_ns(&their_wkb, |b| {
        let w = wkb::reader::read_wkb(b).unwrap();
        let _: Geometry<f64> = w.to_geometry();
    });

    eprintln!("    our  on our   bytes  {our_ro:>8.0} ns/op");
    eprintln!("    our  on their bytes  {our_rt:>8.0} ns/op");
    eprintln!("    their on our   bytes  {their_ro:>8.0} ns/op");
    eprintln!("    their on their bytes  {their_tt:>8.0} ns/op");
    let our_wkb_avg = (our_ro + our_rt) / 2.0;
    let their_wkb_avg = (their_ro + their_tt) / 2.0;
    eprintln!(
        "    → avg: ours {our_wkb_avg:.0} ns  theirs {their_wkb_avg:.0} ns  speedup {:.2}×",
        their_wkb_avg / our_wkb_avg
    );
    eprintln!();
    results.push(("WKB read", our_wkb_avg, their_wkb_avg));

    // ── WKB write ──
    eprintln!("  ── Write ──");
    for g in &polys {
        let _ = geo_repair::write_wkb(g);
    }

    let our_w = bench_ns(&polys, |g| {
        let _ = geo_repair::write_wkb(g);
    });
    let their_w = bench_ns(&polys, |g| {
        use wkb::writer::{WriteOptions, geometry_wkb_size, write_geometry};
        let opts = WriteOptions::default();
        let size = geometry_wkb_size(g);
        let mut buf = Vec::with_capacity(size);
        write_geometry(&mut buf, g, &opts).unwrap();
    });
    let wr_ratio = their_w / our_w;
    eprintln!("    our   {our_w:>8.0} ns/op");
    eprintln!(
        "    their {their_w:>8.0} ns/op{}",
        if wr_ratio < 1.0 { " (faster)" } else { "" }
    );
    eprintln!("    → speedup {wr_ratio:.2}×");
    eprintln!();
    results.push(("WKB write", our_w, their_w));

    // ── WKB roundtrip ──
    eprintln!("  ── Roundtrip ──");
    for g in &polys {
        let bytes = geo_repair::write_wkb(g);
        let _ = geo_repair::read_wkb(&bytes);
    }

    let our_rt = bench_ns(&polys, |g| {
        let bytes = geo_repair::write_wkb(g);
        let _ = geo_repair::read_wkb(&bytes).unwrap();
    });
    let their_rt = bench_ns(&polys, |g| {
        use wkb::writer::{WriteOptions, geometry_wkb_size, write_geometry};
        let opts = WriteOptions::default();
        let size = geometry_wkb_size(g);
        let mut buf = Vec::with_capacity(size);
        write_geometry(&mut buf, g, &opts).unwrap();
        let w = wkb::reader::read_wkb(&buf).unwrap();
        let _: Geometry<f64> = w.to_geometry();
    });
    eprintln!("    our   {our_rt:>8.0} ns/op");
    eprintln!("    their {their_rt:>8.0} ns/op");
    eprintln!("    → speedup {:.2}×", their_rt / our_rt);
    eprintln!();
    results.push(("WKB roundtrip", our_rt, their_rt));

    // ═══════════════════════════════════════════════
    //  WKT
    // ═══════════════════════════════════════════════
    eprintln!("  ──── WKT ────");
    eprintln!();

    // ── WKT read (cross-tested) ──
    eprintln!("  ── Read (cross) ──");
    for s in &our_wkt {
        let _ = geo_repair::read_wkt(s);
    }

    let roo = bench_ns(&our_wkt, |s| {
        let _ = geo_repair::read_wkt(s).unwrap();
    });
    let rot = bench_ns(&their_wkt, |s| {
        let _ = geo_repair::read_wkt(s).unwrap();
    });
    let rto = bench_ns(&our_wkt, |s| {
        let _: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(s).unwrap();
    });
    let rtt = bench_ns(&their_wkt, |s| {
        let _: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(s).unwrap();
    });

    eprintln!("    our  on our   format  {roo:>8.0} ns/op");
    eprintln!("    our  on their format  {rot:>8.0} ns/op");
    eprintln!("    their on our   format  {rto:>8.0} ns/op");
    eprintln!("    their on their format  {rtt:>8.0} ns/op");
    let our_wkt_avg = (roo + rot) / 2.0;
    let their_wkt_avg = (rto + rtt) / 2.0;
    eprintln!(
        "    → avg: ours {our_wkt_avg:.0} ns  theirs {their_wkt_avg:.0} ns  speedup {:.2}×",
        their_wkt_avg / our_wkt_avg
    );
    eprintln!();
    results.push(("WKT read", our_wkt_avg, their_wkt_avg));

    // ── WKT write ──
    eprintln!("  ── Write ──");
    for g in &polys {
        let _ = geo_repair::write_wkt(g);
    }

    let ww_o = bench_ns(&polys, |g| {
        let _ = geo_repair::write_wkt(g);
    });
    let ww_t = bench_ns(&polys, |g| {
        use wkt::ToWkt;
        let _ = g.wkt_string();
    });
    eprintln!("    our   {ww_o:>8.0} ns/op");
    eprintln!("    their {ww_t:>8.0} ns/op");
    eprintln!("    → speedup {:.2}×", ww_t / ww_o);
    eprintln!();
    results.push(("WKT write", ww_o, ww_t));

    // ── WKT roundtrip (subset) ──
    let n_sub = our_wkt.len().min(WKT_RT_SUBSET);
    eprintln!("  ── Roundtrip (n={n_sub}) ──");
    for s in &our_wkt[..n_sub] {
        let g = geo_repair::read_wkt(s).unwrap();
        let _ = geo_repair::write_wkt(&g);
    }

    let rt_o = bench_ns(&our_wkt[..n_sub], |s| {
        let g = geo_repair::read_wkt(s).unwrap();
        let _ = geo_repair::write_wkt(&g);
    });
    let rt_t = bench_ns(&their_wkt[..n_sub], |s| {
        let g: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(s).unwrap();
        use wkt::ToWkt;
        let _ = g.wkt_string();
    });
    eprintln!("    our   {rt_o:>8.0} ns/op");
    eprintln!("    their {rt_t:>8.0} ns/op");
    eprintln!("    → speedup {:.2}×", rt_t / rt_o);
    eprintln!();
    results.push(("WKT roundtrip", rt_o, rt_t));

    // ═══════════════════════════════════════════════
    //  Summary
    // ═══════════════════════════════════════════════
    let total = t_start.elapsed();
    eprintln!("  ═══ Summary ═══");
    eprintln!("  Total wall time: {total:?}");
    eprintln!();
    eprintln!(
        "  {:<20}  {:>8}  {:>8}  {:>7}",
        "Operation", "Ours/op", "Theirs/op", "Speedup"
    );
    eprintln!("  {:->20}  {:->8}  {:->8}  {:->7}", "", "", "", "");
    for (label, ours, theirs) in &results {
        eprintln!(
            "  {label:<20}  {ours:>7.0} ns  {theirs:>7.0} ns  {:>5.2}×",
            theirs / ours
        );
    }
    eprintln!();
}

/// Benchmark `f` on all `items`, return ns/op. No warmup (caller handles it).
fn bench_ns<T>(items: &[T], f: impl Fn(&T)) -> f64 {
    let t0 = Instant::now();
    for item in items {
        f(item);
    }
    let dt = t0.elapsed();
    dt.as_nanos() as f64 / items.len() as f64
}

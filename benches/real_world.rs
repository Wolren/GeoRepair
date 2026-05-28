//! Real-world benchmark: Structure method vs GEOS on real SHP data.
//!
//! Run with:
//!   $env:Path = "C:\Users\Wildbot\miniconda3\Library\bin;$env:Path"
//!   cargo bench --features bench-geos --bench real_world

use geo::{Coord, LineString, Polygon};
use geo_repair::arrange::{self, validate_polygon};
use geo_repair::orient::orient2d;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use geos::Geom;
use std::fs::File;
use std::io::{Read, Write};
use std::time::Instant;
use wkt::ToWkt;

fn read_f64(buf: &[u8], pos: &mut usize) -> f64 {
    let v = f64::from_le_bytes(buf[*pos..*pos + 8].try_into().unwrap());
    *pos += 8;
    v
}
fn read_u32(buf: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    v
}
fn read_ring(buf: &[u8], pos: &mut usize) -> LineString<f64> {
    let n = read_u32(buf, pos) as usize;
    let mut coords = Vec::with_capacity(n);
    for _ in 0..n {
        coords.push(Coord {
            x: read_f64(buf, pos),
            y: read_f64(buf, pos),
        });
    }
    LineString::new(coords)
}
fn read_binary(path: &str) -> Vec<Polygon<f64>> {
    let mut buf = Vec::new();
    File::open(path).unwrap().read_to_end(&mut buf).unwrap();
    let mut pos = 0;
    let n_polys = read_u32(&buf, &mut pos) as usize;
    let mut polys = Vec::with_capacity(n_polys);
    for _ in 0..n_polys {
        let ext = read_ring(&buf, &mut pos);
        let n_holes = read_u32(&buf, &mut pos) as usize;
        let mut holes = Vec::with_capacity(n_holes);
        for _ in 0..n_holes {
            holes.push(read_ring(&buf, &mut pos));
        }
        polys.push(Polygon::new(ext, holes));
    }
    polys
}

fn poly_n_vert(poly: &Polygon<f64>) -> usize {
    let mut n = poly.exterior().0.len();
    for h in poly.interiors() {
        n += h.0.len();
    }
    n
}

/// Lightweight validity breakdown (no O(n²) intersection check).
fn examine_validity(poly: &Polygon<f64>) -> Vec<String> {
    let mut reasons = Vec::new();

    let ext = poly.exterior();
    if ext.0.len() < 4 {
        reasons.push("ext < 4 pts".into());
    } else if ext.0.first() != ext.0.last() {
        reasons.push("ext not closed".into());
    } else {
        for w in ext.0.windows(2) {
            if w[0] == w[1] {
                reasons.push("consecutive dup ext".into());
                break;
            }
        }
    }
    for (_hi, h) in poly.interiors().iter().enumerate() {
        if h.0.len() < 4 {
            reasons.push(format!("hole {_hi} < 4 pts"));
        } else if h.0.first() != h.0.last() {
            reasons.push(format!("hole {_hi} not closed"));
        } else {
            for w in h.0.windows(2) {
                if w[0] == w[1] {
                    reasons.push(format!("consecutive dup hole {_hi}"));
                    break;
                }
            }
        }
    }
    if !reasons.is_empty() {
        return reasons;
    }

    for c in &ext.0 {
        if !c.x.is_finite() || !c.y.is_finite() {
            reasons.push("NaN in ext".into());
            break;
        }
    }
    for h in poly.interiors() {
        for c in &h.0 {
            if !c.x.is_finite() || !c.y.is_finite() {
                reasons.push("NaN in hole".into());
                break;
            }
        }
    }
    if !reasons.is_empty() {
        return reasons;
    }

    for (_hi, h) in poly.interiors().iter().enumerate() {
        if let Some(pt) = h.0.first().copied() {
            if !point_in_ring_exclusive(pt, &ext.0) {
                reasons.push(format!("hole {_hi} outside shell"));
            }
        }
    }
    let holes: Vec<_> = poly.interiors().iter().map(|h| &h.0).collect();
    for (i, h1) in holes.iter().enumerate() {
        for h2 in holes.iter().skip(i + 1) {
            if let Some(pt) = h2.first().copied() {
                if point_in_ring_exclusive(pt, h1) {
                    reasons.push(format!("hole {i} contains hole"));
                }
            }
        }
    }
    reasons
}

fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    let n = ring.len();
    let mut wn = 0i32;
    for i in 0..n - 1 {
        let p1 = ring[i];
        let p2 = ring[i + 1];
        if p1.y <= pt.y {
            if p2.y > pt.y && orient2d(p1, p2, pt) > 0.0 {
                wn += 1;
            }
        } else if p2.y <= pt.y && orient2d(p1, p2, pt) < 0.0 {
            wn -= 1;
        }
    }
    wn != 0
}

/// Estimate average fast-path time by processing N valid polys.
fn sample_fastpath(
    polys: &[Polygon<f64>],
    valid_idx: &[usize],
    config: &MakeValidConfig,
    n: usize,
) -> f64 {
    let sample = valid_idx.len().min(n);
    if sample == 0 {
        return 0.0;
    }
    let t0 = Instant::now();
    for &idx in &valid_idx[..sample] {
        let _ = polys[idx].make_valid_with_config(config);
    }
    t0.elapsed().as_secs_f64() / sample as f64
}

fn main() {
    let path = "benches/real_world/data_0.bin";
    let t0 = Instant::now();
    let polys = read_binary(path);
    let load_time = t0.elapsed().as_secs_f64();
    let n_polys = polys.len();
    eprintln!("[1/5] Loaded {n_polys} polys in {load_time:.3}s");

    // Pre-compute validity and vertex counts
    eprint!("[2/5] Validating {n_polys} polys...");
    let t0 = Instant::now();
    let infos: Vec<_> = polys
        .iter()
        .map(|p| (validate_polygon(p), poly_n_vert(p)))
        .collect();
    let n_valid = infos.iter().filter(|(v, _)| *v).count();
    let n_invalid = n_polys - n_valid;
    let valid_idx: Vec<usize> = infos
        .iter()
        .enumerate()
        .filter(|(_, (v, _))| *v)
        .map(|(i, _)| i)
        .collect();
    let invalid_idx: Vec<usize> = infos
        .iter()
        .enumerate()
        .filter(|(_, (v, _))| !*v)
        .map(|(i, _)| i)
        .collect();
    eprintln!(
        " {:.3}s ({n_valid} valid, {n_invalid} invalid)",
        t0.elapsed().as_secs_f64()
    );

    // =========================================================================
    // Polygon #24823 deep analysis
    // =========================================================================
    let target = 24822;
    let poly = &polys[target];
    eprintln!(
        "\n══ Polygon #{0} ═══════════════════════════════════",
        target + 1
    );
    eprintln!("  n_vert (ext):     {}", poly.exterior().0.len());
    eprintln!("  n_holes:          {}", poly.interiors().len());
    eprintln!("  valid:            {}", infos[target].0);

    let reasons = examine_validity(poly);
    if !reasons.is_empty() {
        eprintln!("  why invalid:");
        for r in reasons {
            eprintln!("    · {r}");
        }
    } else {
        eprintln!("  why invalid:      self-intersections (only remaining check)");
    }

    if let Some(t) = arrange::diagnose_arrange(poly) {
        eprintln!("  CDT prep:         {:.6}s", t.prep_secs);
        eprintln!(
            "  CDT build:        {:.6}s  ({} faces)",
            t.cdt_build_secs, t.cdt_faces
        );
        eprintln!("  CDT label:        {:.6}s", t.label_secs);
        eprintln!("  CDT extract:      {:.6}s", t.extract_secs);
        eprintln!("  CDT total:        {:.6}s", t.total_secs);
    }

    let t0 = Instant::now();
    let wkt = poly.wkt_string();
    match geos::Geometry::new_from_wkt(&wkt) {
        Ok(geom) => {
            let _ = geom.make_valid();
            eprintln!("  GEOS:             {:.6}s", t0.elapsed().as_secs_f64());
        }
        Err(e) => eprintln!("  GEOS err:         {e}"),
    }

    // =========================================================================
    // Sample fast-path time
    // =========================================================================
    eprint!("\n[3/5] Sampling fast-path (100 polys)...");
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    };
    let fp_time = sample_fastpath(&polys, &valid_idx, &cfg, 100);
    eprintln!(" {:.6}s avg per valid poly", fp_time);

    // =========================================================================
    // Method comparison: Structure vs GEOS on invalid polys
    // =========================================================================
    const SAMPLE: usize = 100;
    let sample_n = n_invalid.min(SAMPLE);
    let sample_idx: Vec<usize> = invalid_idx.iter().copied().take(SAMPLE).collect();

    eprintln!("\n[4/5] Structure vs GEOS on first {sample_n} of {n_invalid} invalid polys");
    eprintln!("  First 10 invalid indices & vertex counts:");
    for &idx in sample_idx.iter().take(10) {
        eprintln!("    idx={}, n_vert={}", idx, infos[idx].1);
    }
    std::io::stderr().flush().ok();

    let mut stru_times = Vec::with_capacity(sample_n);
    let mut geos_times = Vec::with_capacity(sample_n);
    let mut geos_errs = 0usize;

    for (pos, &idx) in sample_idx.iter().enumerate() {
        let p = &polys[idx];
        let nv = infos[idx].1;

        eprint!(
            "\r    {}/{} idx={} nv={}: Structure...",
            pos + 1,
            sample_n,
            idx,
            nv
        );
        std::io::stderr().flush().ok();
        let t0 = Instant::now();
        let _ = p.make_valid_with_config(&MakeValidConfig {
            poly_method: PolyMethod::Structure,
            ..Default::default()
        });
        let ts = t0.elapsed().as_secs_f64();

        eprint!(
            "\r    {}/{} idx={} nv={}: GEOS...      ",
            pos + 1,
            sample_n,
            idx,
            nv
        );
        std::io::stderr().flush().ok();
        let t0 = Instant::now();
        match geos::Geometry::new_from_wkt(&p.wkt_string()) {
            Ok(g) => {
                let _ = g.make_valid();
                geos_times.push(t0.elapsed().as_secs_f64());
            }
            Err(_) => {
                geos_times.push(t0.elapsed().as_secs_f64());
                geos_errs += 1;
            }
        }
        stru_times.push(ts);

        eprintln!(
            "\r    {}/{} idx={} nv={}: str={:.4}s, geos={:.4}s  ",
            pos + 1,
            sample_n,
            idx,
            nv,
            ts,
            geos_times[pos]
        );
        std::io::stderr().flush().ok();
    }
    eprintln!();

    // =========================================================================
    // Summary
    // =========================================================================
    let stru_total: f64 = stru_times.iter().sum();
    let geos_total: f64 = geos_times.iter().sum();
    let est_valid_time = n_valid as f64 * fp_time;

    eprintln!("\n\n[5/5] Results (sample: first {sample_n} of {n_invalid} invalid polys, {geos_errs} GEOS errors)");
    eprintln!("═════════════════════════════════════════════════════════════════════");
    eprintln!("  Data: {n_polys} polys ({n_valid} valid, {n_invalid} invalid)");
    eprintln!(
        "  Fast-path: {:.3}µs/valid poly · estimated valid total: {:.4}s",
        fp_time * 1e6,
        est_valid_time
    );
    eprintln!("═════════════════════════════════════════════════════════════════════");
    eprintln!("  Method               │  sample (s) │ per-poly (ms) │  vs GEOS");
    eprintln!("  ─────────────────────┼─────────────┼───────────────┼───────────");
    let stru_per = stru_total * 1000.0 / sample_n as f64;
    let geos_per = geos_total * 1000.0 / sample_n as f64;
    let stru_rat = if geos_total > 0.0 {
        stru_total / geos_total
    } else {
        0.0
    };
    eprintln!(
        "  Structure            │ {stru_total:>9.4}s │ {stru_per:>11.3}    │ {stru_rat:>6.2}x"
    );
    eprintln!("  ─────────────────────┼─────────────┼───────────────┼───────────");
    eprintln!("  GEOS                 │ {geos_total:>9.4}s │ {geos_per:>11.3}    │      —");
    eprintln!("═════════════════════════════════════════════════════════════════════");
}

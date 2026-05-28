//! Real-world benchmark: Structure method vs GEOS on real SHP data.
//!
//! Run with:
//!   $env:Path = "C:\Users\Wildbot\miniconda3\Library\bin;$env:Path"
//!   cargo bench --features bench-geos --bench real_world

use geo::{Coord, LineString, Polygon};
use geo_repair::arrange::{self, validate_polygon};
use geo_repair::orient::orient2d;
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
#[cfg(feature = "bench-geos")]
use geos::Geom;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
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

    // Pre-compute validity and vertex counts in parallel
    eprint!("[2/5] Validating {n_polys} polys...");
    let t0 = Instant::now();
    #[cfg(feature = "parallel")]
    let infos: Vec<_> = polys
        .par_iter()
        .map(|p| (validate_polygon(p), poly_n_vert(p)))
        .collect();
    #[cfg(not(feature = "parallel"))]
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
    const SAMPLE: usize = 1848;
    let sample_n = n_invalid.min(SAMPLE);
    let sample_idx: Vec<usize> = invalid_idx.iter().copied().take(SAMPLE).collect();

    eprintln!("\n[4/5] Structure vs GEOS on first {sample_n} of {n_invalid} invalid polys");
    eprintln!("  First 10 invalid indices & vertex counts:");
    for &idx in sample_idx.iter().take(10) {
        eprintln!("    idx={}, n_vert={}", idx, infos[idx].1);
    }
    std::io::stderr().flush().ok();

    // Collect invalid polys
    let invalid_polys: Vec<&Polygon<f64>> = sample_idx.iter().map(|&idx| &polys[idx]).collect();

    // Parallel Structure batch processing
    let t0 = Instant::now();
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let results = par_fix_polygon_batch(&invalid_polys, &cfg);
    let stru_total = t0.elapsed().as_secs_f64();

    // Validate all Structure outputs through GEOS is_valid()
    let mut stru_invalid_outputs = 0usize;
    for g in &results {
        let wkt = g.wkt_string();
        match geos::Geometry::new_from_wkt(&wkt) {
            Ok(gg) => {
                if !gg.is_valid().unwrap_or(false) {
                    stru_invalid_outputs += 1;
                }
            }
            Err(_) => stru_invalid_outputs += 1,
        }
    }

    // Sequential GEOS processing for comparison
    let t0 = Instant::now();
    for p in &invalid_polys {
        match geos::Geometry::new_from_wkt(&p.wkt_string()) {
            Ok(g) => {
                let _ = g.make_valid();
            }
            Err(_) => {}
        }
    }
    let geos_total = t0.elapsed().as_secs_f64();

    // =========================================================================
    // Full-dataset timing: process N random polys through both methods
    // =========================================================================
    let full_n = n_polys;
    eprintln!("\n[5/5] Full dataset: {full_n} polys (parallel Structure vs sequential GEOS)");

    let t0 = Instant::now();
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let all_polys: Vec<&Polygon<f64>> = (0..full_n).map(|i| &polys[i]).collect();
    let _full_results = par_fix_polygon_batch(&all_polys, &cfg);
    let full_stru = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    for i in 0..full_n {
        match geos::Geometry::new_from_wkt(&polys[i].wkt_string()) {
            Ok(g) => {
                let _ = g.make_valid();
            }
            Err(_) => {}
        }
    }
    let full_geos = t0.elapsed().as_secs_f64();

    // =========================================================================
    // Summary
    // =========================================================================
    let full_ratio = if full_geos > 0.0 {
        full_stru / full_geos
    } else {
        0.0
    };

    eprintln!("\n\n[6/5] Results");
    eprintln!("═════════════════════════════════════════════════════════════════════");
    eprintln!("  Data: {n_polys} polys ({n_valid} valid, {n_invalid} invalid)");
    eprintln!("  Fast-path: {:.3}µs/valid poly", fp_time * 1e6);
    eprintln!(
        "  Structure output GEOS-valid: {}/{} invalid polys",
        stru_invalid_outputs, sample_n
    );
    eprintln!("─────────────────────────────────────────────────────────────────────");
    eprintln!("  Method (invalid polys)│  total (s) │ per-poly (ms) │  vs GEOS");
    eprintln!("  ──────────────────────┼────────────┼───────────────┼───────────");
    let stru_per = stru_total * 1000.0 / sample_n as f64;
    let geos_per = geos_total * 1000.0 / sample_n as f64;
    let stru_rat = if geos_total > 0.0 {
        stru_total / geos_total
    } else {
        0.0
    };
    eprintln!(
        "  Structure             │ {stru_total:>9.4}s │ {stru_per:>11.3}    │ {stru_rat:>6.2}x"
    );
    eprintln!("  GEOS                  │ {geos_total:>9.4}s │ {geos_per:>11.3}    │      —");
    eprintln!("─────────────────────────────────────────────────────────────────────");
    let full_stru_per = full_stru * 1000.0 / full_n as f64;
    let full_geos_per = full_geos * 1000.0 / full_n as f64;
    eprintln!("  Full dataset ({full_n} poly) │ {full_stru:>9.4}s │ {full_stru_per:>9.4}    │ {full_ratio:>6.2}x");
    eprintln!("  GEOS (full)           │ {full_geos:>9.4}s │ {full_geos_per:>9.4}    │      —");
    eprintln!("═════════════════════════════════════════════════════════════════════");
}

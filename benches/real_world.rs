//! Real-world benchmark: Structure method vs GEOS on real SHP data.
//!
//! You can benchmark any dataset by setting `BENCH_FILE` or passing file as first arg:
//!
//!   $env:BENCH_FILE = "benches/real_world/alaska.shp"
//!   cargo bench --features bench-geos --bench real_world
//!
//! Supported: .bin (custom binary), .shp (shapefile)
//!
//! Run with:
//!   scripts/bench-geos.ps1          # Windows — auto-detects conda GEOS
//!   scripts/bench-geos.sh           # Linux/macOS — auto-detects system GEOS
//!
//! Or manually:
//!   cargo bench --features bench-geos --bench real_world "benches/real_world/data_0.bin"
//!
//! Prerequisites: GEOS must be installed on the system.
//!   conda install -c conda-forge geos   # Windows
//!   sudo apt install libgeos-dev        # Debian/Ubuntu
//!   brew install geos                   # macOS

use std::env;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use geo::{Coord, Geometry, Polygon};
use geo_repair::arrange::validate_polygon;
use geo_repair::io::load_bin;
#[cfg(feature = "load-shp")]
use geo_repair::io::load_shp;
use geo_repair::orient::orient2d;
#[cfg(feature = "parallel")]
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
#[cfg(feature = "bench-geos")]
use geos::Geometry as GeosGeometry;
#[cfg(feature = "bench-geos")]
use geos::{CoordSeq, CoordType, Geom};
#[cfg(feature = "bench-geos")]
fn poly_to_geos(poly: &Polygon<f64>) -> Option<GeosGeometry> {
    fn coords_to_ring(coords: &[Coord<f64>]) -> Option<GeosGeometry> {
        let n = coords.len();
        if n < 3 {
            return None;
        }
        let mut cs = CoordSeq::new(n as u32, CoordType::XY).ok()?;
        for (i, c) in coords.iter().enumerate() {
            cs.set_x(i, c.x).ok()?;
            cs.set_y(i, c.y).ok()?;
        }
        GeosGeometry::create_linear_ring(cs).ok()
    }
    let ring = coords_to_ring(&poly.exterior().0)?;
    let holes: Vec<GeosGeometry> = poly
        .interiors()
        .iter()
        .filter_map(|h| coords_to_ring(&h.0))
        .collect();
    GeosGeometry::create_polygon(ring, holes).ok()
}
#[cfg(feature = "bench-geos")]
fn geo_polys_to_geos_batch<'a>(
    polys: impl Iterator<Item = &'a Polygon<f64>>,
) -> Vec<Option<GeosGeometry>> {
    polys.map(|p| poly_to_geos(p)).collect()
}
#[cfg(feature = "bench-geos")]
fn geom_to_geos(geom: &Geometry<f64>) -> Option<GeosGeometry> {
    use geo::Geometry::*;
    match geom {
        Point(p) => {
            let mut cs = CoordSeq::new(1, CoordType::XY).ok()?;
            cs.set_x(0, p.0.x).ok()?;
            cs.set_y(0, p.0.y).ok()?;
            GeosGeometry::create_point(cs).ok()
        }
        LineString(ls) => {
            let n = ls.0.len();
            let mut cs = CoordSeq::new(n as u32, CoordType::XY).ok()?;
            for (i, c) in ls.0.iter().enumerate() {
                cs.set_x(i, c.x).ok()?;
                cs.set_y(i, c.y).ok()?;
            }
            GeosGeometry::create_line_string(cs).ok()
        }
        Polygon(p) => poly_to_geos(p),
        MultiPoint(mp) => {
            let geoms: Vec<GeosGeometry> =
                mp.0.iter()
                    .filter_map(|p| geom_to_geos(&Point(*p)))
                    .collect();
            if geoms.is_empty() {
                return None;
            }
            GeosGeometry::create_multipoint(geoms).ok()
        }
        MultiLineString(mls) => {
            let geoms: Vec<GeosGeometry> = mls
                .0
                .iter()
                .filter_map(|ls| geom_to_geos(&LineString(ls.clone())))
                .collect();
            if geoms.is_empty() {
                return None;
            }
            GeosGeometry::create_multiline_string(geoms).ok()
        }
        MultiPolygon(mp) => {
            let geoms: Vec<GeosGeometry> = mp.0.iter().filter_map(|p| poly_to_geos(p)).collect();
            if geoms.is_empty() {
                return None;
            }
            GeosGeometry::create_multipolygon(geoms).ok()
        }
        GeometryCollection(gc) => {
            let geoms: Vec<GeosGeometry> = gc.0.iter().filter_map(|g| geom_to_geos(g)).collect();
            if geoms.is_empty() {
                return None;
            }
            GeosGeometry::create_geometry_collection(geoms).ok()
        }
        _ => None,
    }
}
#[cfg(feature = "parallel")]
use rayon::prelude::*;

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

fn load_polys(path: &str) -> Vec<Polygon<f64>> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        #[cfg(feature = "load-shp")]
        "shp" => load_shp(path).unwrap(),
        "bin" => load_bin(path).unwrap_or_else(|e| {
            panic!("Failed to load {path}: {e}");
        }),
        #[cfg(not(feature = "load-shp"))]
        "shp" => panic!("load-shp feature not enabled. Re-run with --features load-shp"),
        other => panic!("Unsupported file extension '.{other}'. Use .shp or .bin"),
    }
}

fn main() {
    // Resolve input file: env var BENCH_FILE, or first CLI arg, or default
    let path = env::var("BENCH_FILE")
        .ok()
        .or_else(|| env::args().skip(1).find(|a| !a.starts_with("--")))
        .unwrap_or_else(|| "benches/real_world/data_0.bin".into());
    eprintln!("Dataset: {path}");

    let t0 = Instant::now();
    let polys = load_polys(&path);
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
    // Deep-dive analysis of a specific polygon (ANALYZE_POLY env var)
    // =========================================================================
    if let Ok(target_str) = env::var("ANALYZE_POLY") {
        if let Ok(target) = target_str.parse::<usize>() {
            if target < n_polys {
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

                #[cfg(feature = "bench-geos")]
                if let Some(t) = geo_repair::arrange::diagnose_arrange(poly) {
                    eprintln!("  CDT prep:         {:.6}s", t.prep_secs);
                    eprintln!(
                        "  CDT build:        {:.6}s  ({} faces)",
                        t.cdt_build_secs, t.cdt_faces
                    );
                    eprintln!("  CDT label:        {:.6}s", t.label_secs);
                    eprintln!("  CDT extract:      {:.6}s", t.extract_secs);
                    eprintln!("  CDT total:        {:.6}s", t.total_secs);
                }

                #[cfg(feature = "bench-geos")]
                {
                    let t0 = Instant::now();
                    match poly_to_geos(poly) {
                        Some(geom) => {
                            let _ = geom.make_valid();
                            eprintln!("  GEOS:             {:.6}s", t0.elapsed().as_secs_f64());
                        }
                        None => eprintln!("  GEOS err:         conversion failed"),
                    }
                }
            } else {
                eprintln!(
                    "  ANALYZE_POLY={target} out of range (dataset has {n_polys} polys) — skipping"
                );
            }
        } else {
            eprintln!("  ANALYZE_POLY='{target_str}' is not a valid index — skipping deep-dive");
        }
    }

    // =========================================================================
    // Sample fast-path time
    // =========================================================================
    eprint!("\n[3/5] Sampling fast-path (100 polys)...");
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let fp_time = sample_fastpath(&polys, &valid_idx, &cfg, 100);
    eprintln!(" {:.6}s avg per valid poly", fp_time);

    // =========================================================================
    // Method comparison: Structure vs GEOS on invalid polys
    // =========================================================================
    let sample_n = n_invalid;
    let sample_idx: Vec<usize> = invalid_idx.iter().copied().take(sample_n).collect();

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
    #[cfg(feature = "parallel")]
    let results = par_fix_polygon_batch(&invalid_polys, &cfg);
    #[cfg(not(feature = "parallel"))]
    let results: Vec<Geometry<f64>> = invalid_polys
        .iter()
        .map(|p| p.make_valid_with_config(&cfg))
        .collect();
    let stru_total = t0.elapsed().as_secs_f64();

    // Validate all Structure outputs through GEOS is_valid()
    #[allow(unused_mut)]
    let mut stru_invalid_outputs = 0usize;
    #[cfg(feature = "bench-geos")]
    {
        for g in &results {
            match geom_to_geos(g) {
                Some(gg) => {
                    if !gg.is_valid().unwrap_or(false) {
                        stru_invalid_outputs += 1;
                    }
                }
                None => stru_invalid_outputs += 1,
            }
        }
    }
    #[cfg(not(feature = "bench-geos"))]
    {
        use geo_repair::GeoValidation;
        for g in &results {
            if !g.is_valid() {
                stru_invalid_outputs += 1;
            }
        }
    }

    // Pre-create GEOS geometries outside GEOS timer
    let geos_total: f64;
    #[cfg(feature = "bench-geos")]
    {
        let geos_geoms: Vec<Option<GeosGeometry>> =
            geo_polys_to_geos_batch(invalid_polys.iter().copied());
        let t0 = Instant::now();
        #[cfg(feature = "parallel")]
        geos_geoms.par_iter().for_each(|g| {
            if let Some(g) = g {
                let _ = g.make_valid();
            }
        });
        #[cfg(not(feature = "parallel"))]
        for g in &geos_geoms {
            if let Some(g) = g {
                let _ = g.make_valid();
            }
        }
        geos_total = t0.elapsed().as_secs_f64();
    }
    #[cfg(not(feature = "bench-geos"))]
    {
        geos_total = 0.0;
    }

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
    #[cfg(feature = "parallel")]
    let _full_results = par_fix_polygon_batch(&all_polys, &cfg);
    #[cfg(not(feature = "parallel"))]
    let _full_results: Vec<Geometry<f64>> = all_polys
        .iter()
        .map(|p| p.make_valid_with_config(&cfg))
        .collect();
    let full_stru = t0.elapsed().as_secs_f64();

    let (geos_setup, full_geos, full_geos_total): (f64, f64, f64);
    #[cfg(feature = "bench-geos")]
    {
        eprint!("  Pre-building {} GEOS geometries...", full_n);
        let t0 = Instant::now();
        let geos_geoms: Vec<Option<GeosGeometry>> = geo_polys_to_geos_batch(polys.iter());
        geos_setup = t0.elapsed().as_secs_f64();
        eprintln!(" {:.3}s", geos_setup);

        let t0 = Instant::now();
        #[cfg(feature = "parallel")]
        geos_geoms.par_iter().for_each(|g| {
            if let Some(g) = g {
                let _ = g.make_valid();
            }
        });
        #[cfg(not(feature = "parallel"))]
        for g in &geos_geoms {
            if let Some(g) = g {
                let _ = g.make_valid();
            }
        }
        full_geos = t0.elapsed().as_secs_f64();
        full_geos_total = full_geos;
    }
    #[cfg(not(feature = "bench-geos"))]
    {
        geos_setup = 0.0;
        full_geos = 0.0;
        full_geos_total = 0.0;
    }

    // =========================================================================
    // Summary
    // =========================================================================
    let full_ratio = if full_geos_total > 0.0 {
        full_stru / full_geos_total
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
    eprintln!(
        "  GEOS (full)           │ {full_geos_total:>9.4}s │ {full_geos_per:>9.4}    │      —"
    );
    eprintln!("    (setup: {geos_setup:.3}s, make_valid loop: {full_geos:.3}s)");
    eprintln!("═════════════════════════════════════════════════════════════════════");
}

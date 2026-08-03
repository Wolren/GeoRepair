//! Real-world benchmark: Structure method vs GEOS on real dataset.
//!
//! The default dataset is the original GeoPackage (`data_0.gpkg`), read
//! through the crate's GeoPackage backend (its geometry blobs are WKB of
//! the original data). The custom `.bin` transcription of the same data
//! drops 42 empty parts and is not used for the GEOS comparison.
//!
//! You can benchmark any dataset by setting `BENCH_FILE` or passing file as first arg:
//!
//!   $env:BENCH_FILE = "benches/real_world/data_0.gpkg"
//!   cargo bench --features bench-geos-system --bench real_world   # system (LLVM-optimized)
//!   cargo bench --features bench-geos --bench real_world          # static (MSVC)
//!
//! Run with:
//!   cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp,io-gpkg --bench real_world   # system GEOS (conda LLVM)
//!   cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp,io-gpkg --bench real_world          # static GEOS (MSVC)
//!
//! Prerequisites:
//!   System GEOS (conda): set GEOS_LIB_DIR, GEOS_INCLUDE_DIR, GEOS_VERSION, and PATH.
//!   Static GEOS: GEOS compiled from C source automatically.
//!   conda install -c conda-forge geos   # Windows
//!   sudo apt install libgeos-dev        # Debian/Ubuntu
//!   brew install geos                   # macOS
//!
//! Fast iteration:
//!   --fast       skip ALL GEOS comparison sections (validation head-to-head,
//!                GEOS makeValid on invalid/full sets, GEOS output checks).
//!                Keeps our metrics: validation, fast-path sample, invalid
//!                subset Structure batch, full-dataset pass. ~10s vs ~40s.
//!   BENCH_N=1000 cap the dataset to the first N polys (smoke runs).
//!   Example: BENCH_N=200000 cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world -- --fast

use std::env;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

#[cfg(any(
    feature = "bench-geos",
    feature = "bench-geos-system",
    not(feature = "parallel")
))]
use geo::Geometry;
use geo::{Coord, Polygon};
use geo_repair::arrange::validate_polygon;
use geo_repair::dd::{dd_call_count, reset_dd_count};
use geo_repair::io::load_bin;
use geo_repair::orient::orient2d;
#[cfg(feature = "parallel")]
use geo_repair::parallel::{par_fix_polygon_batch, par_fix_polygon_batch_owned};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
use geos::Geometry as GeosGeometry;
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
use geos::{CoordSeq, CoordType, Geom};
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
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
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn geo_polys_to_geos_batch<'a>(
    polys: impl Iterator<Item = &'a Polygon<f64>>,
) -> Vec<Option<GeosGeometry>> {
    polys.map(|p| poly_to_geos(p)).collect()
}
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
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
        if let Some(pt) = h.0.first().copied()
            && !point_in_ring_exclusive(pt, &ext.0)
        {
            reasons.push(format!("hole {_hi} outside shell"));
        }
    }
    let holes: Vec<_> = poly.interiors().iter().map(|h| &h.0).collect();
    for (i, h1) in holes.iter().enumerate() {
        for h2 in holes.iter().skip(i + 1) {
            if let Some(pt) = h2.first().copied()
                && point_in_ring_exclusive(pt, h1)
            {
                reasons.push(format!("hole {i} contains hole"));
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
        "bin" => load_bin(path).unwrap_or_else(|e| {
            panic!("Failed to load {path}: {e}");
        }),
        other => {
            // Original-data sources (.shp/.gpkg/.wkb) go through the crate's
            // extension-detecting loader instead of the custom .bin
            // transcription, which flattens MultiPolygons and skips empty
            // records. The GEOS comparison should measure the original
            // dataset, not a derived transcription of it.
            let geoms = geo_repair::io::load(path).unwrap_or_else(|e| {
                panic!("Failed to load {path} ('.{other}'): {e}");
            });
            let mut out = Vec::with_capacity(geoms.len());
            for g in geoms {
                match g {
                    Geometry::Polygon(p) => out.push(p),
                    Geometry::MultiPolygon(mp) => out.extend(mp.0),
                    _ => {}
                }
            }
            out
        }
    }
}

fn main() {
    // Resolve input file: env var BENCH_FILE, or first CLI arg, or default
    let path = env::var("BENCH_FILE")
        .ok()
        .or_else(|| env::args().skip(1).find(|a| !a.starts_with("--")))
        .unwrap_or_else(|| "benches/real_world/data_0.gpkg".into());
    // --fast: skip every GEOS comparison section (see header comment).
    let fast = env::args().any(|a| a == "--fast");
    // BENCH_N: cap the dataset to the first N polys for smoke runs.
    let cap_n: Option<usize> = env::var("BENCH_N").ok().and_then(|v| v.parse().ok());
    eprintln!("Dataset: {path}");
    if fast {
        eprintln!("Mode: fast (--fast: GEOS comparison sections skipped)");
    }

    let t0 = Instant::now();
    let polys = load_polys(&path);
    let polys = if let Some(n) = cap_n {
        let capped: Vec<_> = polys.into_iter().take(n).collect();
        eprintln!("BENCH_N={n}: dataset capped to {} polys", capped.len());
        capped
    } else {
        polys
    };
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
    // Validation head-to-head: our validate_polygon vs GEOS isValid
    // =========================================================================
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        if fast {
            eprintln!("[2b/5] Validation head-to-head (skip: --fast)");
        } else {
        eprint!("[2b/5] Validation head-to-head (all {n_polys} polys, parallel)...");
        std::io::stderr().flush().ok();
        use rayon::prelude::*;

        // Time our validator (parallel)
        let t0 = Instant::now();
        let our_valid: u64 = polys
            .par_iter()
            .map(|p| if validate_polygon(p) { 1 } else { 0 })
            .sum();
        let our_time = t0.elapsed().as_secs_f64();

        // Pre-build GEOS geometries in parallel
        let t0 = Instant::now();
        let geos_geoms: Vec<Option<geos::Geometry>> =
            polys.par_iter().map(|p| poly_to_geos(p)).collect();
        let geos_build_time = t0.elapsed().as_secs_f64();

        // Time GEOS isValid (parallel)
        let t0 = Instant::now();
        let geos_valid: u64 = geos_geoms
            .par_iter()
            .map(|g| g.as_ref().and_then(|g| g.is_valid().ok()).unwrap_or(false) as u64)
            .sum();
        let geos_time = t0.elapsed().as_secs_f64();

        // Agreement (parallel)
        let (both_valid, both_invalid, our_valid_geos_invalid, our_invalid_geos_valid): (
            u64,
            u64,
            u64,
            u64,
        ) = polys
            .par_iter()
            .zip(geos_geoms.par_iter())
            .map(|(poly, g)| {
                match (
                    validate_polygon(poly),
                    g.as_ref().and_then(|g| g.is_valid().ok()).unwrap_or(false),
                ) {
                    (true, true) => (1, 0, 0, 0),
                    (false, false) => (0, 1, 0, 0),
                    (true, false) => (0, 0, 1, 0),
                    (false, true) => (0, 0, 0, 1),
                }
            })
            .reduce(
                || (0, 0, 0, 0),
                |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3),
            );

        let our_invalid = n_polys - our_valid as usize;
        let geos_invalid = n_polys - geos_valid as usize;
        let disagree = our_valid_geos_invalid + our_invalid_geos_valid;
        let total = both_valid + both_invalid + disagree;
        let rate = (total - disagree) as f64 / total as f64 * 100.0;
        let our_per = our_time / n_polys as f64 * 1e6;
        let geos_per = geos_time / n_polys as f64 * 1e6;

        eprintln!(" done:");
        eprintln!("  ┌──────────────────────┬──────────────┬─────────────┐");
        eprintln!("  │ Validator            │ total        │ per-poly    │");
        eprintln!("  ├──────────────────────┼──────────────┼─────────────┤");
        eprintln!("  │ Ours                 │ {our_time:>11.4}s │ {our_per:>8.2}µs │");
        eprintln!("  │ GEOS isValid         │ {geos_time:>11.4}s │ {geos_per:>8.2}µs │");
        eprintln!("  │ GEOS build           │ {geos_build_time:>11.4}s │ (one-time)  │");
        eprintln!("  ├──────────────────────┼──────────────┼─────────────┤");
        eprintln!(
            "  │ Our / GEOS           │    {:>7.2}x       │           │",
            our_time / geos_time.max(1e-12)
        );
        eprintln!("  └──────────────────────┴──────────────┴─────────────┘");
        eprintln!("  ┌────────────────────┬──────────┬──────────┐");
        eprintln!("  │                    │  Ours    │  GEOS    │");
        eprintln!("  ├────────────────────┼──────────┼──────────┤");
        eprintln!("  │ Valid              │ {our_valid:>8} │ {geos_valid:>8} │");
        eprintln!("  │ Invalid            │ {our_invalid:>8} │ {geos_invalid:>8} │");
        eprintln!("  ├────────────────────┼──────────┼──────────┤");
        eprintln!("  │ Both agree ✓✓     │ {both_valid:>8}     │           │");
        eprintln!("  │ Both agree ✗✗     │ {both_invalid:>8}     │           │");
        eprintln!("  │ Ours✓ GEOS✗       │ {our_valid_geos_invalid:>8}     │           │");
        eprintln!("  │ Ours✗ GEOS✓       │ {our_invalid_geos_valid:>8}     │           │");
        eprintln!("  │ Agreement          │    {rate:.2}%     │           │");
        eprintln!("  └────────────────────┴──────────┴──────────┘");
        }
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    eprintln!("  (skip: bench-geos feature not enabled)");

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

                #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
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

                #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
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
                    "  ANALYZE_POLY={target} out of range (dataset has {n_polys} polys) - skipping"
                );
            }
        } else {
            eprintln!("  ANALYZE_POLY='{target_str}' is not a valid index - skipping deep-dive");
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
    reset_dd_count();
    #[cfg(feature = "structure")]
    geo_repair::structure::reset_profile();
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
    let dd_calls = dd_call_count();
    eprintln!(
        "  DD calls: {dd_calls} ({:.0} per poly, {:.3}µs per call est = {:.3}s)",
        dd_calls as f64 / sample_n as f64,
        0.2, // estimated 200ns per DD call
        dd_calls as f64 * 0.2e-6
    );
    #[cfg(feature = "structure")]
    geo_repair::structure::print_profile(sample_n);

    // Validate all Structure outputs through GEOS is_valid()
    // (--fast: use our validator instead - no GEOS oracle in fast mode)
    let mut stru_invalid_outputs = 0usize;
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    if !fast {
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
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        use geo_repair::GeoValidation;
        for g in &results {
            if !g.is_valid() {
                stru_invalid_outputs += 1;
            }
        }
    }
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    if fast {
        // Lightweight gate only (arrange::validate_polygon): the full
        // Shewchuk validator on the repaired outputs costs ~35s on the
        // giant invalid polys (187k verts) - defeats the purpose of
        // --fast. The GEOS verdict stays the canonical one in full mode.
        let mut invalid = 0usize;
        for g in &results {
            let mut bad = false;
            let mut check = |p: &geo::Polygon<f64>| {
                if !geo_repair::arrange::validate_polygon(p) {
                    bad = true;
                }
            };
            match g {
                Geometry::Polygon(p) => check(p),
                Geometry::MultiPolygon(mp) => mp.0.iter().for_each(&mut check),
                Geometry::GeometryCollection(gc) => {
                    for c in gc.0.iter() {
                        match c {
                            Geometry::Polygon(p) => check(p),
                            Geometry::MultiPolygon(mp) => mp.0.iter().for_each(&mut check),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            if bad {
                invalid += 1;
            }
        }
        stru_invalid_outputs = invalid;
    }

    // Pre-create GEOS geometries outside GEOS timer
    let geos_total: f64;
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        if fast {
            geos_total = 0.0;
        } else {
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
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        geos_total = 0.0;
    }

    // =========================================================================
    // Full-dataset timing: process N random polys through both methods
    // =========================================================================
    let full_n = n_polys;
    eprintln!("\n[5/5] Full dataset: {full_n} polys (parallel Structure vs parallel GEOS)");

    let (geos_setup, full_geos, full_geos_total): (f64, f64, f64);
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        if fast {
            geos_setup = 0.0;
            full_geos = 0.0;
            full_geos_total = 0.0;
        } else {
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
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        geos_setup = 0.0;
        full_geos = 0.0;
        full_geos_total = 0.0;
    }

    // Structure full pass — runs LAST because it CONSUMES `polys` for the
    // zero-copy owned batch path (valid polygons move into the output
    // instead of being cloned, matching GEOS's shared-geometry return).
    let t0 = Instant::now();
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    #[cfg(feature = "parallel")]
    let _full_results = par_fix_polygon_batch_owned(polys, &cfg);
    #[cfg(not(feature = "parallel"))]
    let _full_results: Vec<Geometry<f64>> = polys
        .into_iter()
        .map(|p| geo_repair::make_valid::make_valid_owned(p, &cfg))
        .collect();
    let full_stru = t0.elapsed().as_secs_f64();

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
    if fast {
        eprintln!(
            "  Structure output invalid (our validator): {}/{} invalid polys",
            stru_invalid_outputs, sample_n
        );
    } else {
        eprintln!(
            "  Structure output GEOS-valid: {}/{} invalid polys",
            stru_invalid_outputs, sample_n
        );
    }
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
    if geos_total > 0.0 {
        eprintln!("  GEOS                  │ {geos_total:>9.4}s │ {geos_per:>11.3}    │      -");
    } else {
        eprintln!("  GEOS                  │        (skip: --fast)        │      -");
    }
    eprintln!("─────────────────────────────────────────────────────────────────────");
    let full_stru_per = full_stru * 1000.0 / full_n as f64;
    let full_geos_per = full_geos * 1000.0 / full_n as f64;
    eprintln!(
        "  Full dataset ({full_n} poly) │ {full_stru:>9.4}s │ {full_stru_per:>9.4}    │ {full_ratio:>6.2}x"
    );
    if full_geos_total > 0.0 {
        eprintln!(
            "  GEOS (full)           │ {full_geos_total:>9.4}s │ {full_geos_per:>9.4}    │      -"
        );
        eprintln!("    (setup: {geos_setup:.3}s, make_valid loop: {full_geos:.3}s)");
    } else {
        eprintln!("  GEOS (full)           │        (skip: --fast)        │      -");
    }
    eprintln!("═════════════════════════════════════════════════════════════════════");
}

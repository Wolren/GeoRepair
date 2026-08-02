//! GEOS-compatible fast-path polygon repair via planar graph extraction.
//!
//! The Structure strategy is the default fast path for polygon repair. It
//! mirrors GEOS's ST_MakeValid algorithm:
//!
//! 1. Build a planar graph from polygon edges
//! 2. Classify edges and extract faces
//! 3. Face walking to find ring boundaries
//! 4. Winding-number assembly into OGC-valid output
//!
//! Strengths:
//! - 10-100x faster than CDT-based approaches on valid/simple inputs
//! - No external dependencies beyond `geo`
//! - Handles the vast majority of real-world invalid polygons
//!
//! Falls back to the Arrange strategy when the topology is too complex
//! (many holes, nested self-intersections).
//!
//! # Submodules
//!
//! - `classify`: Edge classification and planar graph building
//! - `fix_ring`: Ring repair (self-intersection, winding correction)
//! - `fix_ring_graph`: Graph-based ring intersection resolution
//! - `merge`: Face merging after graph extraction
//! - `subtract`: Hole subtraction during face assembly
//! - `sweep`: Plane-sweep intersection detection
/// Edge classification and planar graph building for polygon faces.
pub mod classify;
/// GEOS BuildArea port: polygonize linework → shell/hole classification →
/// even-parent filter. The reference area-building algorithm.
pub mod build_area;
/// Ring repair: self-intersection resolution, winding correction.
pub mod fix_ring;
/// Graph-based ring intersection resolution.
pub mod fix_ring_graph;
/// Face merging after planar graph extraction.
pub mod merge;
/// Topological face extraction from noded edge sets (BuildArea).
pub mod polygonizer;
/// Hole subtraction during polygon face assembly.
pub mod subtract;
/// Edge splitting at intersection points (R-tree / sweep / brute force).
pub(crate) mod edge_split;
/// GEOS MakeValidPoly symdiff loop (BuildArea + XOR accumulation).
pub(crate) mod symdiff;
/// Plane-sweep intersection detection for edge segments.
pub mod sweep;

use geo::{
    Coord, Geometry, Line, LineString, LinesIter, Point, Polygon,
    Winding,
};
use rstar::{AABB, RTree, RTreeObject};
use smallvec::SmallVec;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::core;
use crate::core::MakeValidConfig;
use crate::util;
use log::warn;

// ── Profiling counters (cumulative ns) ──
pub static PROFILE_FP_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_SR_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_HR_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_HN_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_MG_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_FSI_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_CL_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_NEST_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_SUB_NS: AtomicU64 = AtomicU64::new(0);

pub fn reset_profile() {
    PROFILE_FP_NS.store(0, Ordering::Relaxed);
    PROFILE_SR_NS.store(0, Ordering::Relaxed);
    PROFILE_HR_NS.store(0, Ordering::Relaxed);
    PROFILE_HN_NS.store(0, Ordering::Relaxed);
    PROFILE_MG_NS.store(0, Ordering::Relaxed);
    PROFILE_FSI_NS.store(0, Ordering::Relaxed);
    PROFILE_CL_NS.store(0, Ordering::Relaxed);
    PROFILE_NEST_NS.store(0, Ordering::Relaxed);
    PROFILE_SUB_NS.store(0, Ordering::Relaxed);
}

pub fn print_profile(n_polys: usize) {
    let fp = PROFILE_FP_NS.load(Ordering::Relaxed);
    let sr = PROFILE_SR_NS.load(Ordering::Relaxed);
    let hr = PROFILE_HR_NS.load(Ordering::Relaxed);
    let hn = PROFILE_HN_NS.load(Ordering::Relaxed);
    let mg = PROFILE_MG_NS.load(Ordering::Relaxed);
    let fsi = PROFILE_FSI_NS.load(Ordering::Relaxed);
    let cl = PROFILE_CL_NS.load(Ordering::Relaxed);
    let nest = PROFILE_NEST_NS.load(Ordering::Relaxed);
    let sub = PROFILE_SUB_NS.load(Ordering::Relaxed);
    let total_ns = fp + sr + hr + hn + mg;
    let total_ms = total_ns as f64 / 1e6;
    let pct = |v: f64| {
        if total_ms > 0.0 {
            v / total_ms * 100.0
        } else {
            0.0
        }
    };
    let ms = |v: u64| v as f64 / 1e6;
    eprintln!("\n=== Structure profile: {n_polys} polys ===");
    eprintln!("  fast_path     {:>9.3}ms  {:>5.1}%", ms(fp), pct(ms(fp)));
    eprintln!("  shell_repair  {:>9.3}ms  {:>5.1}%", ms(sr), pct(ms(sr)));
    eprintln!("    (self_intx) {:>9.3}ms", ms(fsi));
    eprintln!("  hole_repair   {:>9.3}ms  {:>5.1}%", ms(hr), pct(ms(hr)));
    eprintln!(
        "  hole_nest_sub {:>9.3}ms  {:>5.1}%  break:",
        ms(hn),
        pct(ms(hn))
    );
    eprintln!("    classify    {:>9.3}ms  {:>5.1}%", ms(cl), pct(ms(cl)));
    eprintln!(
        "    nesting     {:>9.3}ms  {:>5.1}%",
        ms(nest),
        pct(ms(nest))
    );
    eprintln!("    subtract    {:>9.3}ms  {:>5.1}%", ms(sub), pct(ms(sub)));
    eprintln!("  merge         {:>9.3}ms  {:>5.1}%", ms(mg), pct(ms(mg)));
    eprintln!("  ─────────────────────────────────");
    eprintln!("  total         {:>9.3}ms", ms(total_ns));
}

/// Borrowed convenience wrapper: clones the polygon and delegates to
/// [`fix_polygon_owned`]. The hot batch paths use the owned variant to avoid
/// the clone (measured: the fast-path clone was ~2 allocs + full ring memcpy
/// per polygon, the dominant per-poly cost on the 1.58M-poly full pass).
pub fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    fix_polygon_owned(poly.clone(), config).ok()
}

/// Owned repair entry: `Ok(geometry)` on success, `Err(polygon)` when the
/// polygon is returned unconsumed (structure repair produced no result and
/// the caller may fall back to precision reduction).
///
/// The fast path MOVES the polygon into the output instead of cloning it —
/// for the ~99.85% of real-world polygons that are already valid this is a
/// zero-copy passthrough, matching GEOS's shared-geometry return.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn fix_polygon_owned(
    poly: Polygon<f64>,
    config: &MakeValidConfig,
) -> Result<Geometry<f64>, Polygon<f64>> {
    // Fast path: valid polygons can return immediately. Use a total-verts limit
    // to avoid the monotone-chain has_no_intersections cost on very large rings.
    let _t_fp = Instant::now();
    #[cfg(feature = "arrange")]
    {
        let total_verts: usize =
            poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
        if total_verts > 0
            && poly.exterior().0.len() >= 4
            && crate::arrange::poly_has_basic_form(&poly)
            // Sub-ULP edge check: an edge shorter than EPSILON * bbox_scale
            // (mixed-magnitude rings, e.g. 1e8 coords with 1e-8 spikes) makes
            // proper-crossing detection blind — collinear overlap is invisible.
            // Such inputs are invalid anyway; route them to the full repair.
            && !crate::arrange::has_sub_ulp_edge(&poly)
            // Collinear ring check: a wide-bbox ring can still be exactly
            // collinear (base=1e10, step=0.09, n=3 — all points on one line).
            // Winding is then numerically ambiguous → WrongOrientation. The
            // fast path must not pass it through; full repair degrades it.
                    {
            if total_verts <= core::FAST_PATH_MAX_VERTS {
                // SmallVec: ~95.6% of the real-world dataset has <= 32
                // vertices, so the line collection stays on the stack and
                // skips the heap allocation entirely; larger rings spill to
                // the heap transparently.
                let lines: SmallVec<[Line<f64>; crate::core::SMALL_RING_LINES]> =
                    poly.lines_iter().collect();
                if !lines.is_empty()
                    && crate::arrange::prep::has_no_intersections(&lines)
                    && crate::arrange::holes_are_valid(&poly)
                {
                    PROFILE_FP_NS.fetch_add(_t_fp.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    return Ok(Geometry::Polygon(poly));
                }
            } else {
                // Very large rings: skip the monotone-chain has_no_intersections
                // (O(n log n) but heavy constant) and use the R-tree proper-
                // crossing sweep instead. A polygon with no proper crossing and
                // valid holes IS the final result — the full repair pipeline
                // (classify → subtract → merge) would only waste time. Measured:
                // 159k-vert shell with 857 holes took 11.3s in subtract_holes
                // for a polygon that was already valid.
                if !crate::structure::has_proper_self_crossing(&poly)
                    && crate::arrange::holes_are_valid_inclusive(&poly)
                {
                    PROFILE_FP_NS.fetch_add(_t_fp.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    return Ok(Geometry::Polygon(poly));
                }
            }
        }
    }
    PROFILE_FP_NS.fetch_add(_t_fp.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Crossing hole: a hole with a vertex strictly OUTSIDE the shell ring.
    // Neither the i_overlay difference (returns a single quantized ring with
    // 1e-9-grid node artifacts; measured: hole vertex split into two nodes
    // 2.4e-7 apart) nor the polygonizer (mis-assigns the crossing pieces as
    // holes - HoleOutsideShell) produces valid output for these. Arrange's
    // pipeline yields the GEOS-identical even-odd decomposition (verified
    // node-identical to geosop makeValid) - delegate the whole polygon.
    // Boundary-touching holes (all vertices on the shell, e.g. CGAL
    // square_hole_rhombus) are NOT crossing and stay on the boolean path.
    // Hot-path discipline: the per-vertex test is O(hole x shell), so it
    // runs only when the hole bbox pokes outside the shell bbox (the
    // common fully-inside case is a pure bbox comparison) and only on
    // small ring pairs (large rings route through the gates downstream).
    #[cfg(feature = "arrange")]
    {
        let shell_bbox = ring_bbox(poly.exterior().0.as_slice());
        let crossing = poly.interiors().iter().any(|h| {
            let hb = ring_bbox(&h.0);
            if hb.0 >= shell_bbox.0
                && hb.1 <= shell_bbox.1
                && hb.2 >= shell_bbox.2
                && hb.3 <= shell_bbox.3
            {
                return false;
            }
            if h.0.len() * poly.exterior().0.len() > 4096 {
                return false;
            }
            hole_vertex_strictly_outside(h, poly.exterior())
        });
        if crossing {
            return Ok(crate::arrange::fallback_polygon_fix(&poly));
        }
    }

    // Compute shell bbox once (needed for hole Type C bypass)
    let shell_bbox = ring_bbox(poly.exterior().0.as_slice());

    // Run shell repair + hole processing concurrently (both are independent).
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    let (valid_shells, hole_rings_cw) = {
        use rayon::prelude::*;
        let (shell_res, holes) = rayon::join(
            || {
                let _t = Instant::now();
                let shell_polys = match fix_ring::repair_ring(poly.exterior()) {
                    Some(polys) => polys,
                    None => return None,
                };
                if shell_polys.is_empty() {
                    return None;
                }
                // Polygons carry structural holes from BuildArea (self-crossing
                // lobes) — they must survive into the subtract step.
                let valid: Vec<Polygon<f64>> =
                    shell_polys.into_iter().filter(|p| p.exterior().0.len() >= 4).collect();
                PROFILE_SR_NS.fetch_add(_t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                if valid.is_empty() { None } else { Some(valid) }
            },
            || {
                let _t = Instant::now();
                let mut hole_results: Vec<Vec<LineString<f64>>> = poly
                    .interiors()
                    .par_iter()
                    .map(|h| {
                        let hole_bbox = ring_bbox(&h.0);
                        if !bboxes_overlap(shell_bbox, hole_bbox) {
                            return vec![h.clone()];
                        }
                        if !fix_ring::has_self_intersections_with_bbox(&h.0, hole_bbox) {
                            return vec![h.clone()];
                        }
                        fix_ring::repair_ring(h).map(|polys| {
                            polys.into_iter().map(|p| p.exterior().clone()).collect()
                        }).unwrap_or_else(|| vec![h.clone()])
                    })
                    .collect();
                PROFILE_HR_NS.fetch_add(_t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                hole_results
                    .iter_mut()
                    .flat_map(|rings| rings.drain(..))
                    .map(ensure_cw)
                    .collect::<Vec<_>>()
            },
        );
        let valid_shells = match shell_res {
            Some(v) => v,
            None => {
                warn!("Structure: shell ring repair failed, falling back to CDT arrange");
                #[cfg(feature = "arrange")]
                if !poly.exterior().0.is_empty() {
                    return Ok(crate::arrange::fallback_polygon_fix(&poly));
                }
                return Ok(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
            }
        };
        (valid_shells, holes)
    };

    // Serial path: shell repair first, then holes sequentially.
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    let (valid_shells, hole_rings_cw) = {
        let _t_sr = Instant::now();
        let shell_polys = match fix_ring::repair_ring(poly.exterior()) {
            Some(polys) => polys,
            None => {
                warn!("Structure: shell ring repair failed, falling back to CDT arrange");
                #[cfg(feature = "arrange")]
                if !poly.exterior().0.is_empty() {
                    return Ok(crate::arrange::fallback_polygon_fix(&poly));

                }
                return Ok(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
            }
        };
        if shell_polys.is_empty() {
            return Ok(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
        }
        let valid_shells: Vec<Polygon<f64>> =
            shell_polys.into_iter().filter(|p| p.exterior().0.len() >= 4).collect();
        if valid_shells.is_empty() {
            return Ok(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
        }
        PROFILE_SR_NS.fetch_add(_t_sr.elapsed().as_nanos() as u64, Ordering::Relaxed);

        let hole_rings_cw: Vec<LineString<f64>> = {
            let _t_hr = Instant::now();
            let mut hole_rings: Vec<LineString<f64>> = Vec::new();
            for h in poly.interiors() {
                let hole_bbox = ring_bbox(&h.0);
                if !bboxes_overlap(shell_bbox, hole_bbox) {
                    hole_rings.push(ensure_cw(h.clone()));
                    continue;
                }
                if !fix_ring::has_self_intersections_with_bbox(&h.0, hole_bbox) {
                    hole_rings.push(ensure_cw(h.clone()));
                    continue;
                }
                if let Some(polys) = fix_ring::repair_ring(h) {
                    hole_rings.extend(polys.into_iter().map(|p| p.exterior().clone()));
                } else {
                    hole_rings.push(ensure_cw(h.clone()));
                }
            }
            PROFILE_HR_NS.fetch_add(_t_hr.elapsed().as_nanos() as u64, Ordering::Relaxed);
            hole_rings.into_iter().map(ensure_cw).collect()
        };
        (valid_shells, hole_rings_cw)
    };

    // For each valid shell polygon, classify and subtract holes.
    // The shell may already carry STRUCTURAL holes from BuildArea (lobes of a
    // self-crossing shell that must remain void); input holes are subtracted
    // on top of them.
    let process_shell = |shell: Polygon<f64>| -> Vec<Polygon<f64>> {
        let mut shell_poly = shell;
        // Structural holes: ensure CW orientation (geo convention) and drop
        // any that are invalid or collinear.
        let struct_holes: Vec<LineString<f64>> = shell_poly
            .interiors()
            .iter()
            .filter(|h| h.0.len() >= 4)
            .map(|h| ensure_cw(h.clone()))
            .collect();
        shell_poly = Polygon::new(ensure_ccw(shell_poly.exterior().clone()), struct_holes);

        let _t_cl = Instant::now();
        let (inner_holes, outer_holes) =
            classify::classify_holes(shell_poly.exterior(), &hole_rings_cw);
        PROFILE_CL_NS.fetch_add(_t_cl.elapsed().as_nanos() as u64, Ordering::Relaxed);

        let _t_nest = Instant::now();
        let (to_subtract, islands) = resolve_nesting(&inner_holes);
        PROFILE_NEST_NS.fetch_add(_t_nest.elapsed().as_nanos() as u64, Ordering::Relaxed);

        let inner_polys: Vec<Polygon<f64>> = to_subtract
            .into_iter()
            .map(|h| Polygon::new(h, Vec::new()))
            .collect();

        let mut local = Vec::new();
        let _t_sub = Instant::now();
        let subtracted = subtract::subtract_holes(&shell_poly, &inner_polys);
        local.extend(subtracted.0);
        PROFILE_SUB_NS.fetch_add(_t_sub.elapsed().as_nanos() as u64, Ordering::Relaxed);

        local.extend(islands);

        // Outer holes: rings classified outside the shell. Normally pushed as
        // separate polygons. BUT when an outer hole fully CONTAINS the shell,
        // GEOS makeValid swaps roles — the larger ring becomes the shell and
        // the original shell becomes its hole (even-odd semantics; measured:
        // shell 10x10 + hole 20x20 → GEOS returns the 20x20 with the 10x10 as
        // hole, area 300 = 400-100; our old output was the 20x20 alone, 400).
        for hole in outer_holes {
            let shell_ring = shell_poly.exterior().clone();
            if shell_ring.0.len() >= 4 && all_vertices_inside_ring(&shell_ring.0, &hole.0) {
                local.push(Polygon::new(hole, vec![ensure_cw(shell_ring)]));
            } else {
                local.push(Polygon::new(hole, Vec::new()));
            }
        }
        local
    };

    let mut result_polys: Vec<Polygon<f64>> = {
        let _t_hn = Instant::now();
        let r = {
            #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
            {
                use rayon::prelude::*;
                valid_shells
                    .into_par_iter()
                    .flat_map(process_shell)
                    .collect()
            }
            #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
            {
                valid_shells.into_iter().flat_map(process_shell).collect()
            }
        };
        PROFILE_HN_NS.fetch_add(_t_hn.elapsed().as_nanos() as u64, Ordering::Relaxed);
        r
    };

    if result_polys.is_empty() {
        warn!("Structure: subtract/merge produced no result polygons");
        return Err(poly);
    }

    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_PP").is_ok() {
        use geo::Area;
        for (i, p) in result_polys.iter().enumerate() {
            eprintln!(
                "DIAG_PP result_poly[{i}]: area={:.4} holes={}",
                p.unsigned_area(),
                p.interiors().len()
            );
        }
    }

    let result = if result_polys.len() == 1 {
        // Safe: len==1 verified above on local Vec
        let p = result_polys.pop().expect("len==1 verified");
        #[cfg(feature = "arrange")]
        {
            // Proper-crossing check only: legitimate hole/shell vertex touches
            // (GEOS makeValid emits them) must survive.
            if has_proper_self_crossing(&p) {
                Geometry::GeometryCollection(geo::GeometryCollection(Vec::new()))
            } else {
                Geometry::Polygon(p)
            }
        }
        #[cfg(not(feature = "arrange"))]
        Geometry::Polygon(p)
    } else {
        let _t_mg = Instant::now();
        // Even-parent filter: when shells are nested (one fully contains another),
        // unary_union produces NestedHoles. The BuildArea even-parent approach
        // keeps only shells with an even number of containing shells.
        let merged = merge::merge_shells(result_polys);
        PROFILE_MG_NS.fetch_add(_t_mg.elapsed().as_nanos() as u64, Ordering::Relaxed);
        // Clean NestedHoles from merge output
        #[cfg(feature = "arrange")]
        {
            let g = crate::make_valid::drop_nested_components(merged);
            #[cfg(any(test, debug_assertions))]
            if std::env::var("DIAG_STR").is_ok() {
                use geo::Area;
                let ga = match &g {
                    Geometry::Polygon(p) => p.unsigned_area(),
                    Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
                    Geometry::GeometryCollection(gc) => gc.0.iter().map(|x| match x {
                        Geometry::Polygon(p) => p.unsigned_area(),
                        _ => 0.0,
                    }).sum(),
                    _ => 0.0,
                };
                eprintln!("DIAG_STR: drop_nested -> {ga:.4}");
            }
            // Discard components with proper self-crossings (floating-point edge
            // cases). Legitimate hole/shell vertex touches survive.
            if let Geometry::MultiPolygon(ref mp) = g {
                let filtered: Vec<Polygon<f64>> = mp
                    .0
                    .iter()
                    .filter(|p| !has_proper_self_crossing(p))
                    .cloned()
                    .collect();
                if filtered.is_empty() {
                    Geometry::GeometryCollection(geo::GeometryCollection(Vec::new()))
                } else if filtered.len() == 1 {
                    Geometry::Polygon(filtered.into_iter().next().unwrap())
                } else {
                    Geometry::MultiPolygon(geo::MultiPolygon::new(filtered))
                }
            } else {
                g
            }
        }
        #[cfg(not(feature = "arrange"))]
        crate::make_valid::drop_nested_components(merged)
    };

    Ok(result)
}

/// True if the polygon's linework has a PROPER self-crossing (interior-interior
/// intersection). Shared endpoints (hole touching shell at a vertex — GEOS
/// makeValid emits them) are legal and do NOT count. Used as a post-fix
/// filter: only genuine floating-point self-crossings are discarded.
/// Uses the R-tree sweep for O(n log n) instead of the brute-force O(n²)
/// pair loop — the quadratic version was fatal on large rings (59k verts →
/// 9.1s, 181k verts → 143s measured on the real-world dataset).
pub fn bbox_test(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    crate::simd::aabb_minmax_simd(coords)
}
pub fn eps_test(coords: &[Coord<f64>]) -> f64 {
    let b = crate::simd::aabb_minmax_simd(coords);
    let scale = (b.1 - b.0).abs().max((b.3 - b.2).abs()).max(1.0);
    crate::core::EPS * scale
}
pub fn has_proper_self_crossing(p: &geo::Polygon<f64>) -> bool {
    // Flatten exterior + holes into one coord slice, remembering ring starts.
    let mut coords: Vec<Coord<f64>> = Vec::with_capacity(
        p.exterior().0.len() + p.interiors().iter().map(|h| h.0.len()).sum::<usize>(),
    );
    let mut ring_offsets: Vec<usize> = Vec::with_capacity(p.interiors().len() + 1);
    ring_offsets.push(0);
    coords.extend_from_slice(&p.exterior().0);
    for h in p.interiors() {
        ring_offsets.push(coords.len());
        coords.extend_from_slice(&h.0);
    }
    if coords.len() < 4 {
        return false;
    }
    let bbox = crate::simd::aabb_minmax_simd(&coords);
    let scale = (bbox.1 - bbox.0).abs().max((bbox.3 - bbox.2).abs()).max(1.0);
    let eps = crate::core::EPS * scale;
    crate::structure::sweep::has_proper_self_crossing_sweep(&coords, &ring_offsets, eps)
}

/// Winding-number point-in-ring test (exclusive of boundary).
/// Delegates to SIMD-accelerated implementation.
fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    crate::simd::point_in_ring_exclusive(pt, ring)
}

/// True if the hole ring has at least one vertex STRICTLY OUTSIDE the shell
/// ring (neither inside nor on the boundary) - i.e. the hole crosses the
/// shell boundary. Boundary-touching holes (all vertices exactly on the
/// shell, e.g. CGAL square_hole_rhombus) return false. Used to route
/// crossing holes to the arrange fallback (see fix_polygon).
fn hole_vertex_strictly_outside(hole: &LineString<f64>, shell: &LineString<f64>) -> bool {
    let ring = shell.0.as_slice();
    if ring.len() < 4 {
        return false;
    }
    for &pt in &hole.0 {
        if point_in_ring_exclusive(pt, ring) {
            continue;
        }
        // On the boundary? Exact-vertex touch: distance to the nearest shell
        // segment within the validation tolerance (1e-12 * L^2 relative).
        let mut on_boundary = false;
        for w in ring.windows(2) {
            if w[0] == w[1] {
                continue;
            }
            let dx = w[1].x - w[0].x;
            let dy = w[1].y - w[0].y;
            let len2 = dx * dx + dy * dy;
            if len2 == 0.0 {
                continue;
            }
            let t = ((pt.x - w[0].x) * dx + (pt.y - w[0].y) * dy) / len2;
            let t = t.clamp(0.0, 1.0);
            let px = w[0].x + t * dx;
            let py = w[0].y + t * dy;
            let d2 = (pt.x - px) * (pt.x - px) + (pt.y - py) * (pt.y - py);
            if d2 <= 1e-12 * len2 {
                on_boundary = true;
                break;
            }
        }
        if !on_boundary {
            return true;
        }
    }
    false
}

/// Compute bounding box of a coordinate ring as (min_x, max_x, min_y, max_y).
fn ring_bbox(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    crate::simd::aabb_minmax_simd(coords)
}

/// True if EVERY vertex of `inner` is strictly inside ring `outer`.
/// Used for hole-role swap: an outer hole that fully contains the shell
/// becomes the shell itself (GEOS even-odd semantics).
fn all_vertices_inside_ring(inner: &[Coord<f64>], outer: &[Coord<f64>]) -> bool {
    if inner.len() < 4 || outer.len() < 4 {
        return false;
    }
    inner
        .iter()
        .all(|pt| point_in_ring_exclusive(*pt, outer))
}

/// Check if two bounding boxes overlap.
#[inline]
fn bboxes_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 <= b.1 && a.1 >= b.0 && a.2 <= b.3 && a.3 >= b.2
}

/// Resolve hole-hole nesting among inner holes of a shell.
///
/// Returns:
/// - `to_subtract`: holes at containment depth 1 (directly inside the shell).
///   These are subtracted from the shell via boolean difference.
/// - `islands`: holes at depth 2+ become separate polygons, with their own
///   sub-holes (depth 3) as interior rings. Depth alternates: even depths are
///   separate polygons (islands/positive space), odd depths are holes (negative space).
fn resolve_nesting(holes: &[LineString<f64>]) -> (Vec<LineString<f64>>, Vec<Polygon<f64>>) {
    if holes.len() <= 1 {
        return (holes.to_vec(), Vec::new());
    }

    // Build parent relationship: hole[j] is inside hole[i] → parent_of[j] = Some(i)
    let n = holes.len();

    // Precompute bbox + area for each hole, then build R-tree for O(log n) lookup
    #[derive(Clone, Copy)]
    struct HoleEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
        area: f64,
    }
    impl RTreeObject for HoleEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }
    let envs: Vec<HoleEnv> = holes
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            let first = h.0.first()?;
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
            for c in &h.0 {
                min_x = min_x.min(c.x);
                max_x = max_x.max(c.x);
                min_y = min_y.min(c.y);
                max_y = max_y.max(c.y);
            }
            Some(HoleEnv {
                idx: i,
                env: AABB::from_corners([min_x, min_y], [max_x, max_y]),
                area: util::shoelace_sum(&h.0).abs() / 2.0,
            })
        })
        .collect();
    let tree = RTree::bulk_load(envs);

    let parent_of: Vec<Option<usize>> = {
        let find_parent = |j: usize| -> Option<usize> {
            let pt = *holes[j].0.first()?;
            let query = AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
            let mut best: Option<usize> = None;
            let mut best_area = f64::MAX;
            let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
                if c.idx == j {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }
                if point_in_ring_exclusive(pt, &holes[c.idx].0) && c.area < best_area {
                    best_area = c.area;
                    best = Some(c.idx);
                }
                std::ops::ControlFlow::<(), ()>::Continue(())
            });
            best
        };
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        if n >= 8 {
            use rayon::prelude::*;
            (0..n).into_par_iter().map(find_parent).collect()
        } else {
            (0..n).map(find_parent).collect()
        }
        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        {
            (0..n).map(find_parent).collect()
        }
    };

    // Compute containment depth for each hole via BFS topological sort
    let mut depth = vec![0usize; n];
    let mut children = vec![Vec::new(); n];
    let mut queue: Vec<usize> = Vec::with_capacity(n);
    for (i, p) in parent_of.iter().enumerate() {
        if let Some(p) = p {
            children[*p].push(i);
        } else {
            depth[i] = 1;
            queue.push(i);
        }
    }
    let mut head = 0;
    while head < queue.len() {
        let p = queue[head];
        head += 1;
        for &child in &children[p] {
            depth[child] = depth[p] + 1;
            queue.push(child);
        }
    }

    // Group holes by depth parity:
    // even depth (2, 4, ...): separate polygons (islands)
    // odd depth (1, 3, ...): subtract-from-parent (holes/voids)
    let mut subtract = Vec::new();
    let mut island_indices = Vec::new();
    for (i, &d) in depth.iter().enumerate() {
        if d == 0 {
            // Unreachable (shouldn't happen), treat as top-level hole
            subtract.push(i);
        } else if d % 2 == 1 {
            subtract.push(i);
        } else {
            island_indices.push(i);
        }
    }

    // For depth-2+ holes (islands), assign depth-3+ children as interior rings
    // Build island polygons with proper sub-hole nesting
    let mut islands: Vec<Polygon<f64>> = Vec::new();
    for &ii in &island_indices {
        let children: Vec<LineString<f64>> = (0..n)
            .filter(|&j| parent_of[j] == Some(ii) && depth[j] > depth[ii] && depth[j] % 2 == 1)
            .map(|j| holes[j].clone())
            .collect();
        islands.push(Polygon::new(holes[ii].clone(), children));
    }

    (
        subtract.into_iter().map(|i| holes[i].clone()).collect(),
        islands,
    )
}

fn ensure_ccw(mut ring: LineString<f64>) -> LineString<f64> {
    #[cfg(feature = "simd")]
    let ccw = crate::simd::is_ring_ccw_simd(&ring.0);
    #[cfg(not(feature = "simd"))]
    let ccw = ring.winding_order() == Some(geo::winding_order::WindingOrder::CounterClockwise);
    if !ccw {
        ring.make_ccw_winding();
    }
    ring
}

fn ensure_cw(mut ring: LineString<f64>) -> LineString<f64> {
    if ring.winding_order() != Some(geo::winding_order::WindingOrder::Clockwise) {
        ring.make_cw_winding();
    }
    ring
}

/// When keep_collapsed is true and the polygon shell collapsed during repair,
/// return a Point or LineString instead of empty.
fn handle_collapse_result(
    exterior: &LineString<f64>,
    _config: &MakeValidConfig,
) -> Option<Geometry<f64>> {
    let coords: Vec<Coord<f64>> = exterior
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    match coords.len() {
        0 => None,
        1 => Some(Geometry::Point(Point(coords[0]))),
        _ => {
            let deduped: Vec<Coord<f64>> = {
                let mut v = Vec::with_capacity(coords.len());
                for c in coords {
                    if v.last() != Some(&c) {
                        v.push(c);
                    }
                }
                v
            };
            if deduped.len() == 1 {
                Some(Geometry::Point(Point(deduped[0])))
            } else {
                Some(Geometry::LineString(LineString::new(deduped)))
            }
        }
    }
}

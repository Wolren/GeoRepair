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
/// Ring repair: self-intersection resolution, winding correction.
pub mod fix_ring;
/// Graph-based ring intersection resolution.
pub mod fix_ring_graph;
/// Face merging after planar graph extraction.
pub mod merge;
/// Hole subtraction during polygon face assembly.
pub mod subtract;
/// Plane-sweep intersection detection for edge segments.
pub mod sweep;

use geo::{
    Coord, Geometry, GeometryCollection, LineString, LinesIter, MultiPolygon, Point, Polygon,
    Winding,
};
use rstar::{AABB, RTree, RTreeObject};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::core;
use crate::core::MakeValidConfig;
use crate::util;
use log::warn;

// ── Profiling counters (cumulative ns) ──
pub(crate) static PROFILE_FP_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_SR_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_HR_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_HN_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_MG_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_FSI_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_CL_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_NEST_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static PROFILE_SUB_NS: AtomicU64 = AtomicU64::new(0);

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

pub(crate) fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    // Fast path: valid polygons can return immediately. Use a total-verts limit
    // to avoid the monotone-chain has_no_intersections cost on very large rings.
    let _t_fp = Instant::now();
    #[cfg(feature = "arrange")]
    {
        let total_verts: usize =
            poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
        if total_verts > 0
            && total_verts <= core::FAST_PATH_MAX_VERTS
            && poly.exterior().0.len() >= 4
            && crate::arrange::poly_has_basic_form(poly)
        {
            let lines: Vec<_> = poly.lines_iter().collect();
            if !lines.is_empty()
                && crate::arrange::prep::has_no_intersections(&lines)
                && crate::arrange::holes_are_valid(poly)
            {
                PROFILE_FP_NS.fetch_add(_t_fp.elapsed().as_nanos() as u64, Ordering::Relaxed);
                return Some(Geometry::Polygon(poly.clone()));
            }
        }
    }
    PROFILE_FP_NS.fetch_add(_t_fp.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Compute shell bbox once (needed for hole Type C bypass)
    let shell_bbox = ring_bbox(poly.exterior().0.as_slice());

    // Run shell repair + hole processing concurrently (both are independent).
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    let (valid_shells, hole_rings_cw) = {
        use rayon::prelude::*;
        let (shell_res, holes) = rayon::join(
            || {
                let _t = Instant::now();
                let shell_rings = match fix_ring::repair_ring(poly.exterior()) {
                    Some(rings) => rings,
                    None => return None,
                };
                if shell_rings.is_empty() {
                    return None;
                }
                let valid: Vec<LineString<f64>> =
                    shell_rings.into_iter().filter(|s| s.0.len() >= 4).collect();
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
                        fix_ring::repair_ring(h).unwrap_or_else(|| vec![h.clone()])
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
                    return Some(crate::arrange::fallback_polygon_fix(poly));
                }
                return handle_collapse_result(poly.exterior(), config);
            }
        };
        (valid_shells, holes)
    };

    // Serial path: shell repair first, then holes sequentially.
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    let (valid_shells, hole_rings_cw) = {
        let _t_sr = Instant::now();
        let shell_rings = match fix_ring::repair_ring(poly.exterior()) {
            Some(rings) => rings,
            None => {
                warn!("Structure: shell ring repair failed, falling back to CDT arrange");
                #[cfg(feature = "arrange")]
                if !poly.exterior().0.is_empty() {
                    return Some(crate::arrange::fallback_polygon_fix(poly));

                }
                return handle_collapse_result(poly.exterior(), config);
            }
        };
        if shell_rings.is_empty() {
            return handle_collapse_result(poly.exterior(), config);
        }
        let valid_shells: Vec<LineString<f64>> =
            shell_rings.into_iter().filter(|s| s.0.len() >= 4).collect();
        if valid_shells.is_empty() {
            return handle_collapse_result(poly.exterior(), config);
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
                if let Some(rings) = fix_ring::repair_ring(h) {
                    hole_rings.extend(rings);
                } else {
                    hole_rings.push(ensure_cw(h.clone()));
                }
            }
            PROFILE_HR_NS.fetch_add(_t_hr.elapsed().as_nanos() as u64, Ordering::Relaxed);
            hole_rings.into_iter().map(ensure_cw).collect()
        };
        (valid_shells, hole_rings_cw)
    };

    // For each valid shell ring, classify and subtract holes
    let process_shell = |shell: LineString<f64>| -> Vec<Polygon<f64>> {
        let shell_poly = Polygon::new(ensure_ccw(shell), Vec::new());

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

        for hole in outer_holes {
            local.push(Polygon::new(hole, Vec::new()));
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
        return None;
    }

    let result = if result_polys.len() == 1 {
        // Safe: len==1 verified above on local Vec
        let p = result_polys.pop().expect("len==1 verified");
        Geometry::Polygon(p)
    } else {
        let _t_mg = Instant::now();
        let mp = Geometry::MultiPolygon(MultiPolygon::new(merge::merge_shells(result_polys).0));
        PROFILE_MG_NS.fetch_add(_t_mg.elapsed().as_nanos() as u64, Ordering::Relaxed);
        mp
    };

    Some(result)
}

/// Winding-number point-in-ring test (exclusive of boundary).
/// Delegates to SIMD-accelerated implementation.
fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    crate::simd::point_in_ring_exclusive(pt, ring)
}

/// Compute bounding box of a coordinate ring as (min_x, max_x, min_y, max_y).
fn ring_bbox(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    crate::simd::aabb_minmax_simd(coords)
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
            let _ = tree.locate_in_envelope_intersecting_int(&query, |c| {
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

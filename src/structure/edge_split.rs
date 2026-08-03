//! Edge splitting at intersection points: R-tree / sweep-line / brute-force
//! strategies chosen by topology, with parametric intersection math.

use geo::{Coord, Line};
use smallvec::SmallVec;

use rustc_hash::FxHashMap;
use rstar::{AABB, RTree, RTreeObject};

use crate::core;
use crate::orient::{orient2d, orient2d_fast};

/// Per-edge split points: parametric position + intersection coordinate.
type SplitPoint = SmallVec<[(f64, Coord<f64>); 2]>;


/// ---------------------------------------------------------------------------
/// Edge splitting at intersection points
/// ---------------------------------------------------------------------------
/// Choose split strategy based on topology.
/// - If many edges share a single endpoint (radial-like, e.g. spoke wheel),
///   sweep-line avoids R-tree degeneracy where all bboxes overlap.
/// - Otherwise R-tree spatial clustering is more efficient.
fn should_use_sweepline(edges: &[Line<f64>], n: usize) -> bool {
    if n < 128 {
        return false;
    }
    let mut freq: FxHashMap<u64, usize> = FxHashMap::default();
    for e in edges {
        let k1 = e.start.x.to_bits() ^ e.start.y.to_bits().wrapping_mul(0x9e3779b97f4a7c15);
        let k2 = e.end.x.to_bits() ^ e.end.y.to_bits().wrapping_mul(0x9e3779b97f4a7c15);
        *freq.entry(k1).or_insert(0) += 1;
        *freq.entry(k2).or_insert(0) += 1;
    }
    freq.into_values().max().unwrap_or(0) > n / 4
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn split_edges(edges: &[Line<f64>]) -> Vec<Line<f64>> {
    let n = edges.len();
    let mut split_points: Vec<SplitPoint> = vec![SmallVec::new(); n];

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for e in edges {
        min_x = min_x.min(e.start.x).min(e.end.x);
        max_x = max_x.max(e.start.x).max(e.end.x);
        min_y = min_y.min(e.start.y).min(e.end.y);
        max_y = max_y.max(e.start.y).max(e.end.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = core::EPS * coord_scale;

    if n > core::GRID_THRESHOLD_N {
        if should_use_sweepline(edges, n) {
            split_edges_sweepline(edges, &mut split_points, eps);
        } else {
            split_edges_rtree(edges, &mut split_points, eps);
        }
    } else {
        split_edges_bruteforce(edges, &mut split_points, eps);
    }

    let eps_param = core::EPS_PARAM;
    // Reconstruction is per-edge independent: sort + dedup the split points,
    // then rebuild the sub-lines. Parallelized for large inputs (the serial
    // rebuild was ~30ms of the 71ms noding on a 260k-edge shell); below the
    // threshold the serial loop's lower dispatch overhead wins.
    if n > 128 {
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        {
            use rayon::prelude::*;
            split_points
                .par_iter_mut()
                .enumerate()
                .flat_map_iter(|(i, pts)| {
                    let e = edges[i];
                    pts.sort_by(|(a, _), (b, _)| {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    pts.dedup_by(|(a, _), (b, _)| (*a - *b).abs() < eps_param);
                    let mut out: Vec<Line<f64>> = Vec::new();
                    let mut prev_pt = e.start;
                    for &(_, pt) in pts.iter() {
                        if dist2(pt, prev_pt) > eps_param {
                            out.push(Line::new(prev_pt, pt));
                        }
                        prev_pt = pt;
                    }
                    if dist2(e.end, prev_pt) > eps_param {
                        out.push(Line::new(prev_pt, e.end));
                    }
                    out
                })
                .collect()
        }
        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        {
            let mut result = Vec::new();
            for i in 0..n {
                let e = edges[i];
                let mut pts = std::mem::take(&mut split_points[i]);
                pts.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                pts.dedup_by(|(a, _), (b, _)| (*a - *b).abs() < eps_param);
                let mut prev_pt = e.start;
                for &(_, pt) in &pts {
                    if dist2(pt, prev_pt) > eps_param {
                        result.push(Line::new(prev_pt, pt));
                    }
                    prev_pt = pt;
                }
                if dist2(e.end, prev_pt) > eps_param {
                    result.push(Line::new(prev_pt, e.end));
                }
            }
            result
        }
    } else {
        let mut result = Vec::new();
        for i in 0..n {
            let e = edges[i];
            let mut pts = std::mem::take(&mut split_points[i]);
            pts.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            pts.dedup_by(|(a, _), (b, _)| (*a - *b).abs() < eps_param);
            let mut prev_pt = e.start;
            for &(_, pt) in &pts {
                if dist2(pt, prev_pt) > eps_param {
                    result.push(Line::new(prev_pt, pt));
                }
                prev_pt = pt;
            }
            if dist2(e.end, prev_pt) > eps_param {
                result.push(Line::new(prev_pt, e.end));
            }
        }
        result
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn split_edges_rtree(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();

    #[derive(Clone, Copy)]
    struct EdgeEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
    }
    impl RTreeObject for EdgeEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }

    let envs: Vec<EdgeEnv> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| EdgeEnv {
            idx: i,
            env: AABB::from_corners(
                [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
                [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
            ),
        })
        .collect();
    let tree = RTree::bulk_load(envs);

    // Per-edge queries are independent — parallelize the query phase with a
    // two-phase hit collection (a pair (i,j) may produce hits for BOTH i and
    // j, so hits are staged flat per thread and appended after, avoiding
    // aliased writes). Measured: noding a 260k-edge giant shell is ~100ms
    // serial; the query phase dominates and parallelizes near-linearly.
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        let hits: Vec<(usize, f64, Coord<f64>)> = (0..n)
            .into_par_iter()
            .flat_map_iter(|i| {
                let e = &edges[i];
                let query = AABB::from_corners(
                    [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
                    [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
                );
                let mut local: Vec<(usize, f64, Coord<f64>)> = Vec::new();
                let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
                    let j = c.idx;
                    if j <= i {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }

                    if i.abs_diff(j) <= 1 || (i == 0 && j == n - 1) {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }

                    if edges[i].start == edges[j].start
                        && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
                    {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }
                    if edges[i].start == edges[j].end
                        && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
                    {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }
                    if edges[i].end == edges[j].start
                        && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
                    {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }
                    if edges[i].end == edges[j].end
                        && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
                    {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }

                    if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
                        && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
                    {
                        let pi = lerp(edges[i], ti);
                        let pj = lerp(edges[j], tj);
                        let pt = Coord {
                            x: (pi.x + pj.x) * 0.5,
                            y: (pi.y + pj.y) * 0.5,
                        };
                        if ti > eps && ti < 1.0 - eps {
                            local.push((i, ti, pt));
                        }
                        if tj > eps && tj < 1.0 - eps {
                            local.push((j, tj, pt));
                        }
                    }
                    std::ops::ControlFlow::<(), ()>::Continue(())
                });
                local
            })
            .collect();
        for (idx, t, pt) in hits {
            split_points[idx].push((t, pt));
        }
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        for i in 0..n {
            let e = &edges[i];
            let query = AABB::from_corners(
                [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
                [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
            );
            let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
                let j = c.idx;
                if j <= i {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }

                if i.abs_diff(j) <= 1 || (i == 0 && j == n - 1) {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }

                if edges[i].start == edges[j].start
                    && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
                {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }
                if edges[i].start == edges[j].end
                    && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
                {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }
                if edges[i].end == edges[j].start
                    && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
                {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }
                if edges[i].end == edges[j].end
                    && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
                {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }

                if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
                    && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
                {
                    let pi = lerp(edges[i], ti);
                    let pj = lerp(edges[j], tj);
                    let pt = Coord {
                        x: (pi.x + pj.x) * 0.5,
                        y: (pi.y + pj.y) * 0.5,
                    };
                    if ti > eps && ti < 1.0 - eps {
                        split_points[i].push((ti, pt));
                    }
                    if tj > eps && tj < 1.0 - eps {
                        split_points[j].push((tj, pt));
                    }
                }
                std::ops::ControlFlow::<(), ()>::Continue(())
            });
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn split_edges_bruteforce(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();
    for i in 0..n {
        for j in (i + 2)..n {
            if i + 1 == j && edges[i].end == edges[j].start {
                continue;
            }
            if i == 0 && j == n - 1 && edges[i].start == edges[j].end {
                continue;
            }
            if edges[i].start == edges[j].start
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
            {
                continue;
            }
            if edges[i].start == edges[j].end
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
            {
                continue;
            }
            if edges[i].end == edges[j].start
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
            {
                continue;
            }
            if edges[i].end == edges[j].end
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
            {
                continue;
            }
            if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
                && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
            {
                let pi = lerp(edges[i], ti);
                let pj = lerp(edges[j], tj);
                let pt = Coord {
                    x: (pi.x + pj.x) * 0.5,
                    y: (pi.y + pj.y) * 0.5,
                };
                if ti > eps && ti < 1.0 - eps {
                    split_points[i].push((ti, pt));
                }
                if tj > eps && tj < 1.0 - eps {
                    split_points[j].push((tj, pt));
                }
            }
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn split_edges_sweepline(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let pairs = crate::noding::sweep_line::find_intersecting_pairs(edges, eps);
    for &(i, j) in &pairs {
        if i.abs_diff(j) <= 1 || (i == 0 && j == edges.len() - 1) {
            continue;
        }
        if edges[i].start == edges[j].start
            && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
        {
            continue;
        }
        if edges[i].start == edges[j].end
            && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
        {
            continue;
        }
        if edges[i].end == edges[j].start
            && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
        {
            continue;
        }
        if edges[i].end == edges[j].end
            && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
        {
            continue;
        }
        if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
            && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
        {
            let pi = lerp(edges[i], ti);
            let pj = lerp(edges[j], tj);
            let pt = Coord {
                x: (pi.x + pj.x) * 0.5,
                y: (pi.y + pj.y) * 0.5,
            };
            if ti > eps && ti < 1.0 - eps {
                split_points[i].push((ti, pt));
            }
            if tj > eps && tj < 1.0 - eps {
                split_points[j].push((tj, pt));
            }
        }
    }
}

#[inline]
pub(crate) fn intersect_param(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    // Phase 1: Detection via robust orient2d (Shewchuk adaptive precision).
    // Fast pre-check rejects obvious non-intersections (both endpoints on
    // the same side of the other segment).
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);

    // Quick rejection: both endpoints on the same side of the other segment
    if o1.signum() == o2.signum() && o1 != 0.0 && o2 != 0.0 {
        return None;
    }
    if o3.signum() == o4.signum() && o3 != 0.0 && o4 != 0.0 {
        return None;
    }

    // Collinear overlap (all four orientations zero)
    if o1 == 0.0 && o2 == 0.0 && o3 == 0.0 && o4 == 0.0 {
        return intersect_param_collinear(e1, e2, eps);
    }

    // Phase 2: Computation via double-double arithmetic (106-bit mantissa).
    // Handles proper crossings AND endpoint-on-segment intersections (both
    // are valid noding events).
    if let Some((_pt, t_dd, u_dd)) =
        crate::dd::segment_intersection_dd(e1.start, e1.end, e2.start, e2.end)
    {
        let t = t_dd.to_f64();
        let u = u_dd.to_f64();
        if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
            return Some((t, u));
        }
    }
    None
}

#[inline]
fn intersect_param_collinear(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);
    if o1.abs() > eps || o2.abs() > eps || o3.abs() > eps || o4.abs() > eps {
        return None;
    }

    let dx = e1.end.x - e1.start.x;
    let dy = e1.end.y - e1.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < eps {
        return None;
    }

    let dot = |c: Coord<f64>| -> f64 { (c.x - e1.start.x) * dx + (c.y - e1.start.y) * dy };

    let s1 = (dot(e1.start) / len2).clamp(0.0, 1.0);
    let s2 = (dot(e1.end) / len2).clamp(0.0, 1.0);
    let p1 = (dot(e2.start) / len2).clamp(0.0, 1.0);
    let p2 = (dot(e2.end) / len2).clamp(0.0, 1.0);

    let e1a = s1.min(s2);
    let e1b = s1.max(s2);
    let e2a = p1.min(p2);
    let e2b = p1.max(p2);

    let lo = e1a.max(e2a);
    let hi = e1b.min(e2b);

    if lo + eps < hi {
        let e2_dot = |c: Coord<f64>| -> f64 {
            let dx2 = e2.end.x - e2.start.x;
            let dy2 = e2.end.y - e2.start.y;
            let len2_2 = dx2 * dx2 + dy2 * dy2;
            if len2_2 < eps {
                return 0.0;
            }
            ((c.x - e2.start.x) * dx2 + (c.y - e2.start.y) * dy2) / len2_2
        };

        let mid_x = e1.start.x + lo * dx;
        let mid_y = e1.start.y + lo * dy;
        let mid = Coord { x: mid_x, y: mid_y };

        let t_param = lo;
        let u_param = e2_dot(mid).clamp(0.0, 1.0);

        let e1_eps = eps / dx.abs().max(dy.abs()).max(1.0);
        let e2_eps = eps
            / (e2.end.x - e2.start.x)
                .abs()
                .max((e2.end.y - e2.start.y).abs())
                .max(1.0);

        let on_e1 = t_param > e1_eps && t_param < 1.0 - e1_eps;
        let on_e2 = u_param > e2_eps && u_param < 1.0 - e2_eps;
        if on_e1 || on_e2 {
            return Some((t_param, u_param));
        }
    }
    None
}

#[inline(always)]
pub(crate) fn lerp(e: Line<f64>, t: f64) -> Coord<f64> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

#[inline(always)]
fn dist2(a: Coord<f64>, b: Coord<f64>) -> f64 {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

use rustc_hash::FxHashSet;
use smallvec::SmallVec;

use geo::{Coord, Line, LineString};
use rstar::{RTree, RTreeObject, AABB};

use crate::core;
use crate::noding;
use crate::orient::orient2d;
use log::warn;
use crate::structure::fix_ring_graph::{
    build_graph, extract_all_faces, label_interior_faces, split_face_at_pinch_points,
};

type SplitPoint = SmallVec<[(f64, Coord<f64>); 2]>;

pub(crate) fn repair_ring(ring: &LineString<f64>) -> Option<Vec<LineString<f64>>> {
    let coords = basic_cleanup(ring)?;
    if coords.len() < 4 {
        return None;
    }
    // Type B: Collinear ring is degenerate, cannot form a valid polygon.
    if is_collinear_ring(&coords) {
        return None;
    }

    if !has_self_intersections(&coords) {
        return Some(vec![LineString::new(coords)]);
    }

    // Fast path: try O(n) split for single self-intersection (70% of invalids).
    if let Some(rings) = try_fast_fix(&coords) {
        let cleaned: Vec<LineString<f64>> = rings
            .into_iter()
            .filter_map(|r| basic_cleanup(&r).map(LineString::new))
            .filter(|r| r.0.len() >= 4)
            .collect();
        if !cleaned.is_empty() {
            // Fast-path output should be valid for simple crossings.
            // Fallback handles any degenerate cases.
            return Some(cleaned);
        }
    }

    if let Some(rings) = fix_self_intersecting(&coords) {
        let cleaned: Vec<LineString<f64>> = rings
            .into_iter()
            .filter_map(|r| basic_cleanup(&r).map(LineString::new))
            .filter(|r| r.0.len() >= 4)
            .collect();
        if cleaned.is_empty() {
            return None;
        }
        return Some(cleaned);
    }
    None
}

pub(crate) fn basic_cleanup(ring: &LineString<f64>) -> Option<Vec<Coord<f64>>> {
    let coords: Vec<_> = ring
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    if coords.is_empty() {
        return None;
    }
    let mut deduped = noding::remove_consecutive_duplicates(&coords);
    if deduped.is_empty() {
        return None;
    }
    if deduped.first() != deduped.last() {
        deduped.push(deduped[0]);
    }
    if deduped.len() < 4 {
        return None;
    }

    Some(deduped)
}

pub(crate) fn is_collinear_ring(coords: &[Coord<f64>]) -> bool {
    if coords.len() < 4 {
        return true;
    }
    let eps = 1e-12;
    for i in 0..coords.len() - 2 {
        let o = orient2d(coords[i], coords[i + 1], coords[i + 2]);
        if o.abs() > eps {
            return false;
        }
    }
    true
}

pub(crate) fn has_self_intersections(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 4 {
        return false;
    }

    let mut seen: FxHashSet<(u64, u64)> =
        FxHashSet::with_capacity_and_hasher(n, Default::default());
    for c in &coords[..n - 1] {
        let key = (c.x.to_bits(), c.y.to_bits());
        if !seen.insert(key) {
            return true;
        }
    }

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = core::EPS * coord_scale;

    if n > core::GRID_THRESHOLD_N {
        return super::sweep::has_self_intersections(coords, eps);
    }

    has_self_intersections_bruteforce(coords, eps)
}

fn has_self_intersections_bruteforce(coords: &[Coord<f64>], eps: f64) -> bool {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in i + 2..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if check_edge_pair(coords, i, j, eps) {
                return true;
            }
        }
    }
    false
}

#[inline(always)]
pub(crate) fn check_edge_pair(coords: &[Coord<f64>], i: usize, j: usize, eps: f64) -> bool {
    assert!(i + 1 < coords.len() && j + 1 < coords.len());
    let a1 = coords[i];
    let a2 = coords[i + 1];
    let b1 = coords[j];
    let b2 = coords[j + 1];

    let o = crate::simd::orient2d_batch_4_robust(
        &[a1, a1, b1, b1],
        &[a2, a2, b2, b2],
        &[b1, b2, a1, a2],
    );

    if o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0 {
        return true;
    }

    if o[2].abs() <= eps
        && a1 != b1
        && a1 != b2
        && ((b1.x - b2.x).abs() > eps && a1.x > b1.x.min(b2.x) + eps && a1.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a1.y > b1.y.min(b2.y) + eps
                && a1.y < b1.y.max(b2.y) - eps)
    {
        return true;
    }
    if o[3].abs() <= eps
        && a2 != b1
        && a2 != b2
        && ((b1.x - b2.x).abs() > eps && a2.x > b1.x.min(b2.x) + eps && a2.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a2.y > b1.y.min(b2.y) + eps
                && a2.y < b1.y.max(b2.y) - eps)
    {
        return true;
    }
    if o[0].abs() <= eps
        && b1 != a1
        && b1 != a2
        && ((a1.x - a2.x).abs() > eps && b1.x > a1.x.min(a2.x) + eps && b1.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b1.y > a1.y.min(a2.y) + eps
                && b1.y < a1.y.max(a2.y) - eps)
    {
        return true;
    }
    if o[1].abs() <= eps
        && b2 != a1
        && b2 != a2
        && ((a1.x - a2.x).abs() > eps && b2.x > a1.x.min(a2.x) + eps && b2.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b2.y > a1.y.min(a2.y) + eps
                && b2.y < a1.y.max(a2.y) - eps)
    {
        return true;
    }

    if o[0].abs() <= eps && o[1].abs() <= eps && o[2].abs() <= eps && o[3].abs() <= eps {
        let lo_x = a1.x.min(a2.x).max(b1.x.min(b2.x));
        let hi_x = a1.x.max(a2.x).min(b1.x.max(b2.x));
        let lo_y = a1.y.min(a2.y).max(b1.y.min(b2.y));
        let hi_y = a1.y.max(a2.y).min(b1.y.max(b2.y));
        if lo_x + eps < hi_x || lo_y + eps < hi_y {
            return true;
        }
    }

    false
}

/// Check if edges i and j have a proper crossing (not just endpoint touch).
/// If so, returns the edge indices and the intersection point.
pub(crate) fn edge_intersection(
    coords: &[Coord<f64>],
    i: usize,
    j: usize,
    eps: f64,
) -> Option<(usize, usize, Coord<f64>)> {
    if !check_edge_pair(coords, i, j, eps) {
        return None;
    }
    let e1 = Line::new(coords[i], coords[i + 1]);
    let e2 = Line::new(coords[j], coords[j + 1]);
    let (ti, tj) = intersect_param(&e1, &e2, eps)?;
    if (ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps) {
        let pi = lerp(e1, ti);
        let pj = lerp(e2, tj);
        let pt = Coord { x: (pi.x + pj.x) * 0.5, y: (pi.y + pj.y) * 0.5 };
        Some((i, j, pt))
    } else {
        None
    }
}

/// Split a self-intersecting ring at the first proper crossing into two
/// simple (non-self-intersecting) rings.  O(n).
///
/// Given edges `(vi, vi+1)` and `(vj, vj+1)` crossing at `pt` (i < j):
///
///   Ring A: `vi+1 → vi+2 → … → vj → pt → vi+1`  (one lobe)
///   Ring B: `v0 → … → vi → pt → vj+1 → … → vn-1 → v0`  (other lobe)
pub(crate) fn split_ring_at_intersection(
    coords: &[Coord<f64>],
    i: usize,
    j: usize,
    pt: Coord<f64>,
) -> (Vec<Coord<f64>>, Vec<Coord<f64>>) {
    let n = coords.len();
    let mut ring1 = Vec::with_capacity(j - i + 2);
    for k in (i + 1)..=j {
        ring1.push(coords[k]);
    }
    ring1.push(pt);
    ring1.push(ring1[0]);

    let mut ring2 = Vec::with_capacity(i + 1 + 1 + (n - 1 - j - 1) + 1);
    for k in 0..=i {
        ring2.push(coords[k]);
    }
    ring2.push(pt);
    for k in (j + 1)..(n - 1) {
        ring2.push(coords[k]);
    }
    ring2.push(coords[0]);

    (ring1, ring2)
}

/// Fast path: try to detect and repair a single self-intersection in O(n).
///
/// Returns `Some(rings)` if the ring had exactly one proper crossing that was
/// split successfully.  Returns `None` if the fix is too complex for the fast
/// path (caller should fall through to the full `fix_self_intersecting`).
pub(crate) fn try_fast_fix(coords: &[Coord<f64>]) -> Option<Vec<LineString<f64>>> {
    let n = coords.len();
    if n < 4 {
        return None;
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = core::EPS * coord_scale;

    let pair = if n > core::GRID_THRESHOLD_N {
        super::sweep::find_first_intersection(coords, eps)?
    } else {
        find_first_intersection_bruteforce(coords, eps)?
    };

    let (i, j, pt) = pair;
    let (ring1, ring2) = split_ring_at_intersection(coords, i, j, pt);
    Some(vec![LineString::new(ring1), LineString::new(ring2)])
}

fn find_first_intersection_bruteforce(
    coords: &[Coord<f64>],
    eps: f64,
) -> Option<(usize, usize, Coord<f64>)> {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in (i + 2)..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if let Some(pair) = edge_intersection(coords, i, j, eps) {
                return Some(pair);
            }
        }
    }
    None
}

/// ---------------------------------------------------------------------------
/// Self-intersecting ring fixer
/// ---------------------------------------------------------------------------
pub(crate) fn fix_self_intersecting(coords: &[Coord<f64>]) -> Option<Vec<LineString<f64>>> {
    let edges = edges_from_coords(coords);
    let mut noded = split_edges(&edges);
    if noded.is_empty() {
        return None;
    }

    // Validate noding with MCIndex-based O(n log n) validator.
    let mut validator = crate::noding::validator::NodingValidator::new(noded.clone());
    validator.validate();
    if validator.has_violations() {
        warn!(
            "fix_self_intersecting: {} noding violation(s) remain, retrying with snap rounding",
            validator.violations().len()
        );
        let snapped = crate::noding::snap_round::snap_round_lines(&edges);
        if !snapped.is_empty() {
            noded = snapped;
        }
    }

    if noded.is_empty() {
        return None;
    }
    let graph = build_graph(&noded);
    if graph.edges.is_empty() {
        return None;
    }
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!("\n=== fix_self_intersecting DIAG ===");
        eprintln!("input coords ({}):", coords.len());
        for (i, c) in coords.iter().enumerate() {
            eprintln!("  c{}: ({:.10}, {:.10})", i, c.x, c.y);
        }
        eprintln!("noded edges ({}):", graph.edges.len());
        for (i, &(fi, ti)) in graph.edges.iter().enumerate() {
            eprintln!(
                "  E{}: v{} ({:.6},{:.6}) -> v{} ({:.6},{:.6})",
                i,
                fi,
                graph.verts[fi].x,
                graph.verts[fi].y,
                ti,
                graph.verts[ti].x,
                graph.verts[ti].y
            );
        }
        eprintln!("verts ({}):", graph.verts.len());
        for (i, v) in graph.verts.iter().enumerate() {
            eprintln!("  v{}: ({:.10}, {:.10})", i, v.x, v.y);
        }
    }
    let faces = extract_all_faces(&graph)?;
    if faces.is_empty() {
        return None;
    }
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!("\nfragments from extract_all_faces ({}):", faces.len());
        for (fi, face) in faces.iter().enumerate() {
            eprintln!("  face {}: {} edges", fi, face.len());
            for (ei, to) in face {
                eprintln!(
                    "    (E{}, to=v{}[{:.4},{:.4}])",
                    ei, to, graph.verts[*to].x, graph.verts[*to].y
                );
            }
        }
    }
    let simple_faces: Vec<Vec<(usize, usize)>> = faces
        .iter()
        .flat_map(|f| split_face_at_pinch_points(f, &graph.edges))
        .filter(|f| f.len() >= 3)
        .collect();
    if simple_faces.is_empty() {
        return None;
    }
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!("\nsimple_faces after pinch split ({}):", simple_faces.len());
        for (fi, face) in simple_faces.iter().enumerate() {
            eprintln!("  face {}: {} edges", fi, face.len());
            let visited_verts: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
            eprintln!("    verts: {:?}", visited_verts);
            eprintln!("    coords:");
            for (j, (ei, to)) in face.iter().enumerate() {
                eprintln!(
                    "      {}: E{} -> v{} ({:.10}, {:.10})",
                    j, ei, to, graph.verts[*to].x, graph.verts[*to].y
                );
            }
        }
    }
    let interior = label_interior_faces(&noded, &graph.verts, coords, &simple_faces, &graph.edges)?;
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!(
            "\ninterior faces: {:?}",
            interior.iter().collect::<Vec<_>>()
        );
    }
    let mut result: Vec<LineString<f64>> = Vec::new();
    for &fi in &interior {
        let face = &simple_faces[fi];
        let mut ring_coords: Vec<Coord<f64>> = face
            .iter()
            .map(|&(_, to_idx)| graph.verts[to_idx])
            .collect();
        if ring_coords.len() >= 3 {
            ring_coords.push(ring_coords[0]);
            #[cfg(any(test, debug_assertions))]
            if std::env::var("DIAG_FIX_RING").is_ok() {
                eprintln!("  interior ring coords ({}):", ring_coords.len());
                let visited: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
                eprintln!("    verts: {:?}", visited);
                for (j, c) in ring_coords.iter().enumerate() {
                    eprintln!("      {}: ({:.10}, {:.10})", j, c.x, c.y);
                }
            }
            result.push(LineString::new(ring_coords));
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

pub(crate) fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

/// ---------------------------------------------------------------------------
/// Edge splitting at intersection points
/// ---------------------------------------------------------------------------
pub(crate) fn split_edges(edges: &[Line<f64>]) -> Vec<Line<f64>> {
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
        split_edges_rtree(edges, &mut split_points, eps);
    } else {
        split_edges_bruteforce(edges, &mut split_points, eps);
    }

    let eps_param = core::EPS_PARAM;
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

fn split_edges_rtree(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();

    struct SegEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
    }
    impl RTreeObject for SegEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }

    let envs: Vec<SegEnv> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let lo_x = e.start.x.min(e.end.x);
            let hi_x = e.start.x.max(e.end.x);
            let lo_y = e.start.y.min(e.end.y);
            let hi_y = e.start.y.max(e.end.y);
            SegEnv {
                idx: i,
                env: AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]),
            }
        })
        .collect();

    let tree = RTree::bulk_load(envs);

    // Collect hits per edge (local Vec avoids double-mutable-borrow on split_points).
    // Each element is (edge_idx, param_t, intersection_point).
    let query_edge = |i: usize, out: &mut Vec<(usize, f64, Coord<f64>)>| {
        let lo_x = edges[i].start.x.min(edges[i].end.x);
        let hi_x = edges[i].start.x.max(edges[i].end.x);
        let lo_y = edges[i].start.y.min(edges[i].end.y);
        let hi_y = edges[i].start.y.max(edges[i].end.y);
        let query_env = AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]);

        let _: std::ops::ControlFlow<()> =
            tree.locate_in_envelope_intersecting_int(&query_env, |candidate| {
                let j = candidate.idx;
                if j <= i {
                    return std::ops::ControlFlow::Continue(());
                }
                if i.abs_diff(j) <= 1 || (i == 0 && j == n - 1) {
                    return std::ops::ControlFlow::Continue(());
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
                        out.push((i, ti, pt));
                    }
                    if tj > eps && tj < 1.0 - eps {
                        out.push((j, tj, pt));
                    }
                }
                std::ops::ControlFlow::Continue(())
            });
    };

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        let all_hits: Vec<Vec<(usize, f64, Coord<f64>)>> = (0..n)
            .into_par_iter()
            .map(|i| {
                let mut local_hits = Vec::new();
                query_edge(i, &mut local_hits);
                local_hits
            })
            .collect();
        for hits in all_hits {
            for (ei, t, pt) in hits {
                split_points[ei].push((t, pt));
            }
        }
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        let mut all_hits: Vec<(usize, f64, Coord<f64>)> = Vec::new();
        for i in 0..n {
            query_edge(i, &mut all_hits);
        }
        for (ei, t, pt) in all_hits {
            split_points[ei].push((t, pt));
        }
    }
}

#[inline]
fn intersect_param(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
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
    if let Some((_pt, t_dd, u_dd)) = crate::dd::segment_intersection_dd(
        e1.start, e1.end, e2.start, e2.end,
    ) {
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
fn lerp(e: Line<f64>, t: f64) -> Coord<f64> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

#[inline(always)]
fn dist2(a: Coord<f64>, b: Coord<f64>) -> f64 {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

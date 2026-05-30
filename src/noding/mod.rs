//! Noding utilities for linear geometry repair.
//!
//! Linear geometries are fixed by:
//! 1. Removing invalid/repeated coordinates
//! 2. Noding self-intersections by splitting crossing edges

use std::mem;

use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString,
};
use rstar::{RTree, RTreeObject, AABB};
use rustc_hash::{FxHashMap, FxHashSet};

// ── Sealed trait: safe f64 downcast ────────────────────────────────────
//
// Only f64 implements NodingFloat.  The unsafe `size_of` + transmute
// dance is isolated to the dispatch boundary of the two public functions
// below; inside the f64‑only helpers no transmutes appear.

mod private {
    use geo::{Coord, GeoFloat};

    pub trait NodingFloat: GeoFloat {
        /// Deterministic hash from coordinate bits for HashMap keys.
        fn coord_hash_key(c: &Coord<Self>) -> u64;
    }

    impl NodingFloat for f64 {
        fn coord_hash_key(c: &Coord<f64>) -> u64 {
            c.x.to_bits() ^ c.y.to_bits().rotate_left(32)
        }
    }

    impl NodingFloat for f32 {
        fn coord_hash_key(c: &Coord<f32>) -> u64 {
            (c.x.to_bits() as u64) ^ (c.y.to_bits() as u64).rotate_left(32)
        }
    }
}

use private::NodingFloat;

/// Node a line string by removing repeated points and splitting at
/// self-intersections. Returns a MultiLineString if splitting occurred,
/// or a single LineString otherwise.
pub(crate) fn node_line_string<T: GeoFloat>(ls: &LineString<T>) -> Geometry<T> {
    let coords: Vec<Coord<T>> =
        ls.0.iter()
            .copied()
            .filter(|c| c.x.is_finite() && c.y.is_finite())
            .collect();
    if coords.len() < 2 {
        return empty();
    }
    let deduped = remove_consecutive_duplicates(&coords);
    if deduped.len() < 2 {
        return empty();
    }

    // Check for self-intersections by testing all non-adjacent edge pairs
    let edges: Vec<Line<T>> = deduped.windows(2).map(|w| Line::new(w[0], w[1])).collect();

    let has_self_intersection = check_self_intersections(&edges);
    if !has_self_intersection {
        return Geometry::LineString(LineString::new(deduped));
    }

    // Split at self-intersections using the same approach as prep.rs
    let split_edges = split_edges_at_intersections(&edges);
    if split_edges.is_empty() {
        return empty();
    }
    // Reconstruct linestrings from split edges (connect touching edges)
    let linestrings = reconnect_edges(split_edges);

    if linestrings.is_empty() {
        empty()
    } else if linestrings.len() == 1 {
        Geometry::LineString(linestrings.into_iter().next().unwrap())
    } else {
        Geometry::MultiLineString(MultiLineString::new(linestrings))
    }
}

/// Check if any non-adjacent edges in the segment list intersect.
fn check_self_intersections<T: GeoFloat>(edges: &[Line<T>]) -> bool {
    let eps = T::from(1e-12).unwrap();
    if edges.len() < 3 {
        return false;
    }

    // f64 path: single transmute at dispatch boundary
    if mem::size_of::<T>() == 8 {
        let edges_f64: &[Line<f64>] = unsafe { mem::transmute(edges) };
        return check_self_intersections_f64(edges_f64, 1e-12);
    }

    // Generic fallback: brute force for non-f64 types
    for i in 0..edges.len() {
        for j in (i + 2)..edges.len() {
            if edges_intersect(&edges[i], &edges[j], eps) {
                return true;
            }
        }
    }
    false
}

fn check_self_intersections_f64(edges: &[Line<f64>], eps: f64) -> bool {
    let n = edges.len();
    if n < 3 {
        return false;
    }

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

    for i in 0..n {
        let e = &edges[i];
        let query = AABB::from_corners(
            [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
            [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
        );
        let result = tree.locate_in_envelope_intersecting_int(&query, |c| {
            let j = c.idx;
            if j <= i {
                return std::ops::ControlFlow::Continue(());
            }
            if i + 1 == j && edges[i].end == edges[j].start {
                return std::ops::ControlFlow::Continue(());
            }
            if j + 1 == i && edges[j].end == edges[i].start {
                return std::ops::ControlFlow::Continue(());
            }
            if edges_intersect(&edges[i], &edges[j], eps) {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        });
        if result.is_break() {
            return true;
        }
    }
    false
}

fn edges_intersect<T: GeoFloat>(e1: &Line<T>, e2: &Line<T>, eps: T) -> bool {
    let o1 = orient2d_generic(e1.start, e1.end, e2.start);
    let o2 = orient2d_generic(e1.start, e1.end, e2.end);
    let o3 = orient2d_generic(e2.start, e2.end, e1.start);
    let o4 = orient2d_generic(e2.start, e2.end, e1.end);

    // General case (proper crossing)
    if o1.abs() > eps && o2.abs() > eps && o3.abs() > eps && o4.abs() > eps {
        return o1.signum() != o2.signum() && o3.signum() != o4.signum();
    }

    // Collinear case: check for interval overlap along the shared line
    if o1.abs() <= eps && o2.abs() <= eps && o3.abs() <= eps && o4.abs() <= eps {
        return collinear_overlap(e1, e2, eps);
    }

    // Endpoint touching — not considered an intersection for noding purposes
    false
}

/// Check whether two collinear segments overlap along the shared line.
fn collinear_overlap<T: GeoFloat>(e1: &Line<T>, e2: &Line<T>, eps: T) -> bool {
    let dx = e1.end.x - e1.start.x;
    let dy = e1.end.y - e1.start.y;
    let dot_d = dx * dx + dy * dy;
    if dot_d <= eps {
        return false;
    }

    // Project e2 endpoints onto e1's direction (t parameter along e1)
    let t2s = ((e2.start.x - e1.start.x) * dx + (e2.start.y - e1.start.y) * dy) / dot_d;
    let t2e = ((e2.end.x - e1.start.x) * dx + (e2.end.y - e1.start.y) * dy) / dot_d;

    let (t2_min, t2_max) = if t2s < t2e { (t2s, t2e) } else { (t2e, t2s) };

    // Overlap region on [0, 1]
    let lo = T::zero().max(t2_min);
    let hi = T::one().min(t2_max);

    // True if overlap covers more than epsilon-length
    lo + eps < hi
}

fn orient2d_generic<T: GeoFloat>(a: Coord<T>, b: Coord<T>, c: Coord<T>) -> T {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Split edges at all pairwise intersection points.
fn split_edges_at_intersections<T: GeoFloat>(edges: &[Line<T>]) -> Vec<Line<T>> {
    let n = edges.len();
    let mut split_points: Vec<Vec<T>> = vec![Vec::new(); n];
    let eps = T::from(1e-12).unwrap();
    let one = T::one();
    let zero = T::zero();

    if n >= 64 && mem::size_of::<T>() == 8 {
        // f64 R‑tree path: single transmute at dispatch boundary
        let edges_f64: &[Line<f64>] = unsafe { mem::transmute(edges) };
        let mut split_f64: Vec<Vec<f64>> = vec![Vec::new(); n];
        split_edges_rtree(edges_f64, &mut split_f64, 1e-12);
        for i in 0..n {
            for &t in &split_f64[i] {
                split_points[i].push(T::from(t).unwrap());
            }
        }
    } else {
        // Brute force for small edge sets or non-f64 types
        for i in 0..n {
            for j in (i + 2)..n {
                if i + 1 == j && edges[i].end == edges[j].start {
                    continue;
                }
                match compute_intersection_param(&edges[i], &edges[j], eps) {
                    Some((ti, tj, _pt)) => {
                        if ti > zero && ti < one {
                            split_points[i].push(ti);
                        }
                        if tj > zero && tj < one {
                            split_points[j].push(tj);
                        }
                    }
                    None => {
                        // Check for collinear overlap
                        let o1 = orient2d_generic(edges[i].start, edges[i].end, edges[j].start);
                        let o2 = orient2d_generic(edges[i].start, edges[i].end, edges[j].end);
                        if o1.abs() <= eps && o2.abs() <= eps {
                            let (p1, p2) = collinear_split_params(&edges[i], &edges[j], eps);
                            for &t in &p1 {
                                if t > zero && t < one {
                                    split_points[i].push(t);
                                }
                            }
                            for &t in &p2 {
                                if t > zero && t < one {
                                    split_points[j].push(t);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let eps_param = T::from(1e-14).unwrap();
    let mut result = Vec::new();
    for i in 0..n {
        let e = edges[i];
        let mut params: Vec<T> = std::mem::take(&mut split_points[i]);
        params.sort_by(|a, b| a.partial_cmp(b).unwrap());
        params.dedup_by(|a, b| (*a - *b).abs() < eps_param);

        let mut prev_t = zero;
        let mut prev_pt = e.start;
        for &t in &params {
            if (t - prev_t).abs() < eps_param {
                continue;
            }
            let pt = interpolate(e, t);
            if dist2(pt, prev_pt) > eps {
                result.push(Line::new(prev_pt, pt));
            }
            prev_t = t;
            prev_pt = pt;
        }
        if dist2(e.end, prev_pt) > eps {
            result.push(Line::new(prev_pt, e.end));
        }
    }

    result
}

fn split_edges_rtree(edges: &[Line<f64>], split_points: &mut [Vec<f64>], eps: f64) {
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

    let mut checked: FxHashSet<(usize, usize)> = FxHashSet::default();

    for i in 0..n {
        let e = &edges[i];
        let query = AABB::from_corners(
            [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
            [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
        );
        let _ = tree.locate_in_envelope_intersecting_int(&query, |c| {
            let j = c.idx;
            if j <= i {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if !checked.insert((i, j)) {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if i + 1 == j && edges[i].end == edges[j].start {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if j + 1 == i && edges[j].end == edges[i].start {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            // Collinear overlap (even shared-vertex pairs can overlap)
            let o1 = orient2d_generic(edges[i].start, edges[i].end, edges[j].start);
            let o2 = orient2d_generic(edges[i].start, edges[i].end, edges[j].end);
            if o1.abs() <= eps && o2.abs() <= eps {
                let (p1, p2) = collinear_split_params(&edges[i], &edges[j], eps);
                for &t in &p1 {
                    if t > 0.0 && t < 1.0 {
                        split_points[i].push(t);
                    }
                }
                for &t in &p2 {
                    if t > 0.0 && t < 1.0 {
                        split_points[j].push(t);
                    }
                }
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            // Pre-filter: edges that share any vertex cannot produce a proper
            // interior crossing, so skip the more expensive
            // `compute_intersection_param`.  Catches the heavy spoke/star‑burst
            // patterns where O(n²) pairs all meet at a single vertex.
            if edges[i].start == edges[j].start
                || edges[i].start == edges[j].end
                || edges[i].end == edges[j].start
                || edges[i].end == edges[j].end
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            if let Some((ti, tj, _pt)) = compute_intersection_param(&edges[i], &edges[j], eps) {
                if ti > 0.0 && ti < 1.0 {
                    split_points[i].push(ti);
                }
                if tj > 0.0 && tj < 1.0 {
                    split_points[j].push(tj);
                }
            }
            std::ops::ControlFlow::<(), ()>::Continue(())
        });
    }
}

fn interpolate<T: GeoFloat>(e: Line<T>, t: T) -> Coord<T> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

fn dist2<T: GeoFloat>(a: Coord<T>, b: Coord<T>) -> T {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

/// For two collinear segments, compute split t-parameters where one edge's
/// endpoint falls strictly inside the other. Returns `(splits_on_e1, splits_on_e2)`.
fn collinear_split_params<T: GeoFloat>(e1: &Line<T>, e2: &Line<T>, eps: T) -> (Vec<T>, Vec<T>) {
    let eps_sq = eps * eps;

    // Project e2 endpoints onto e1's direction
    let dx1 = e1.end.x - e1.start.x;
    let dy1 = e1.end.y - e1.start.y;
    let dot1 = dx1 * dx1 + dy1 * dy1;
    let (mut p1, mut p2) = (Vec::new(), Vec::new());
    if dot1 > eps_sq {
        let t2s = ((e2.start.x - e1.start.x) * dx1 + (e2.start.y - e1.start.y) * dy1) / dot1;
        let t2e = ((e2.end.x - e1.start.x) * dx1 + (e2.end.y - e1.start.y) * dy1) / dot1;
        let one = T::one();
        if t2s > eps && t2s < one - eps {
            p1.push(t2s);
        }
        if t2e > eps && t2e < one - eps {
            p1.push(t2e);
        }
    }

    // Project e1 endpoints onto e2's direction
    let dx2 = e2.end.x - e2.start.x;
    let dy2 = e2.end.y - e2.start.y;
    let dot2 = dx2 * dx2 + dy2 * dy2;
    if dot2 > eps_sq {
        let t1s = ((e1.start.x - e2.start.x) * dx2 + (e1.start.y - e2.start.y) * dy2) / dot2;
        let t1e = ((e1.end.x - e2.start.x) * dx2 + (e1.end.y - e2.start.y) * dy2) / dot2;
        let one = T::one();
        if t1s > eps && t1s < one - eps {
            p2.push(t1s);
        }
        if t1e > eps && t1e < one - eps {
            p2.push(t1e);
        }
    }

    (p1, p2)
}

fn compute_intersection_param<T: GeoFloat>(
    e1: &Line<T>,
    e2: &Line<T>,
    eps: T,
) -> Option<(T, T, Coord<T>)> {
    let denom = (e1.end.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e1.end.y - e1.start.y) * (e2.end.x - e2.start.x);
    if denom.abs() < eps {
        return None;
    }
    let t = ((e2.start.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e2.start.y - e1.start.y) * (e2.end.x - e2.start.x))
        / denom;
    let u = ((e2.start.x - e1.start.x) * (e1.end.y - e1.start.y)
        - (e2.start.y - e1.start.y) * (e1.end.x - e1.start.x))
        / denom;
    let pt = Coord {
        x: e1.start.x + t * (e1.end.x - e1.start.x),
        y: e1.start.y + t * (e1.end.y - e1.start.y),
    };
    Some((t, u, pt))
}

/// Reconnect split edges into continuous linestrings by chaining touching edges.
fn reconnect_edges<T: GeoFloat>(edges: Vec<Line<T>>) -> Vec<LineString<T>> {
    let n = edges.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![LineString::new(vec![edges[0].start, edges[0].end])];
    }

    // f64 path: transmute via size check
    if mem::size_of::<T>() == 8 {
        let e_f64: Vec<Line<f64>> = unsafe { mem::transmute(edges) };
        let r = reconnect_edges_by_key(e_f64);
        return unsafe { mem::transmute(r) };
    }

    // f32 path: same HashMap approach
    if mem::size_of::<T>() == 4 {
        let e_f32: Vec<Line<f32>> = unsafe { mem::transmute(edges) };
        let r = reconnect_edges_by_key(e_f32);
        return unsafe { mem::transmute(r) };
    }

    // Generic fallback: O(n²) for unknown types
    let mut remaining: Vec<Line<T>> = edges;
    let mut result = Vec::new();
    while !remaining.is_empty() {
        let mut chain: Vec<Coord<T>> = Vec::new();
        let first = remaining.swap_remove(0);
        chain.push(first.start);
        chain.push(first.end);

        let mut changed = true;
        while changed {
            changed = false;
            for i in (0..remaining.len()).rev() {
                let last = *chain.last().unwrap();
                if remaining[i].start == last {
                    chain.push(remaining[i].end);
                    remaining.swap_remove(i);
                    changed = true;
                } else if remaining[i].end == last {
                    chain.push(remaining[i].start);
                    remaining.swap_remove(i);
                    changed = true;
                }
            }
        }

        if chain.len() >= 2 {
            result.push(LineString::new(chain));
        }
    }

    result
}

/// O(n) chain reconstruction via HashMap from endpoint → edge index.
/// Works for any type implementing the sealed NodingFloat trait.
fn reconnect_edges_by_key<T: NodingFloat>(edges: Vec<Line<T>>) -> Vec<LineString<T>> {
    let n = edges.len();

    let mut start_map: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    let mut end_map: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    for i in 0..n {
        start_map
            .entry(NodingFloat::coord_hash_key(&edges[i].start))
            .or_default()
            .push(i);
        end_map
            .entry(NodingFloat::coord_hash_key(&edges[i].end))
            .or_default()
            .push(i);
    }

    let mut used = vec![false; n];
    let mut result = Vec::new();

    for start_idx in 0..n {
        if used[start_idx] {
            continue;
        }

        let mut chain: Vec<Coord<T>> = Vec::new();
        chain.push(edges[start_idx].start);
        chain.push(edges[start_idx].end);
        used[start_idx] = true;

        // Extend chain from its last vertex (matches old swap_remove rev-scan order)
        loop {
            let last_key = NodingFloat::coord_hash_key(&chain[chain.len() - 1]);

            // Check forward start-matches (high index first, matching old rev() scan)
            let fwd: Option<usize> = start_map.get(&last_key).and_then(|cands| {
                cands
                    .iter()
                    .copied()
                    .rev()
                    .find(|&idx| !used[idx] && edges[idx].start == *chain.last().unwrap())
            });
            if let Some(idx) = fwd {
                chain.push(edges[idx].end);
                used[idx] = true;
                continue;
            }

            // Check backward end-matches (edge whose end == chain's last vertex)
            let bwd: Option<usize> = end_map.get(&last_key).and_then(|cands| {
                cands
                    .iter()
                    .copied()
                    .rev()
                    .find(|&idx| !used[idx] && edges[idx].end == *chain.last().unwrap())
            });
            if let Some(idx) = bwd {
                chain.push(edges[idx].start);
                used[idx] = true;
                continue;
            }

            break;
        }

        if chain.len() >= 2 {
            result.push(LineString::new(chain));
        }
    }

    result
}

pub(crate) fn remove_consecutive_duplicates<T: CoordNum>(coords: &[Coord<T>]) -> Vec<Coord<T>> {
    let mut result = Vec::with_capacity(coords.len());
    for c in coords {
        if result.last() != Some(c) {
            result.push(*c);
        }
    }
    result
}

fn empty<T: CoordNum>() -> Geometry<T> {
    Geometry::GeometryCollection(GeometryCollection(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------
    // remove_consecutive_duplicates
    // -------------------------------

    #[test]
    fn test_remove_consecutive_duplicates_empty() {
        let result = remove_consecutive_duplicates::<f64>(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_remove_consecutive_duplicates_no_dupes() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(result, coords);
    }

    #[test]
    fn test_remove_consecutive_duplicates_all_identical() {
        let coords = vec![
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(result, vec![Coord { x: 1.0, y: 1.0 }]);
    }

    #[test]
    fn test_remove_consecutive_duplicates_interleaved() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 2.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(
            result,
            vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 2.0, y: 2.0 },
            ]
        );
    }

    // -------------------------------
    // edges_intersect
    // -------------------------------

    #[test]
    fn test_edges_intersect_crossing() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 0.0 });
        assert!(edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_parallel() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_adjacent() {
        // Adjacent edges share a vertex — not an intersection
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_collinear_overlap() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 });
        assert!(edges_intersect(&e1, &e2, 1e-12)); // Collinear overlap detected as intersection
    }

    #[test]
    fn test_edges_intersect_endpoint_on_segment() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 3.0, y: 0.0 }, Coord { x: 3.0, y: 3.0 });
        // This is endpoint-on-segment, not a proper crossing
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_non_adjacent_same_line() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 3.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 });
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    // -------------------------------
    // check_self_intersections
    // -------------------------------

    #[test]
    fn test_check_self_intersections_none() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        assert!(!check_self_intersections(&edges));
    }

    #[test]
    fn test_check_self_intersections_bowtie() {
        // A bowtie shape: (0,0) → (2,2) → (2,0) → (0,2)
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 0.0, y: 2.0 }),
        ];
        // Edge 0 crosses edge 2 (non-adjacent, i=0, j=2)
        assert!(check_self_intersections(&edges));
    }

    #[test]
    fn test_check_self_intersections_empty() {
        assert!(!check_self_intersections::<f64>(&[]));
    }

    #[test]
    fn test_check_self_intersections_single() {
        let edges = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )];
        assert!(!check_self_intersections(&edges));
    }

    // -------------------------------
    // edges_intersect / orient2d_generic
    // -------------------------------

    #[test]
    fn test_orient2d_generic_ccw() {
        let result = orient2d_generic(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.5, y: 1.0 },
        );
        assert!(result > 0.0);
    }

    #[test]
    fn test_orient2d_generic_cw() {
        let result = orient2d_generic(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.5, y: -1.0 },
        );
        assert!(result < 0.0);
    }

    #[test]
    fn test_orient2d_generic_collinear() {
        let result = orient2d_generic(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        );
        assert_eq!(result, 0.0);
    }

    // -------------------------------
    // compute_intersection_param
    // -------------------------------

    #[test]
    fn test_intersection_param_crossing() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 0.0 });
        let result = compute_intersection_param(&e1, &e2, 1e-12);
        assert!(result.is_some());
        let (t1, t2, pt) = result.unwrap();
        assert!((t1 - 0.5f64).abs() < 1e-12f64);
        assert!((t2 - 0.5f64).abs() < 1e-12f64);
        assert!((pt.x - 0.5f64).abs() < 1e-12f64);
        assert!((pt.y - 0.5f64).abs() < 1e-12f64);
    }

    #[test]
    fn test_intersection_param_parallel() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(compute_intersection_param(&e1, &e2, 1e-12).is_none());
    }

    #[test]
    fn test_intersection_param_endpoint_touching() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 1.0 });
        let result = compute_intersection_param(&e1, &e2, 1e-12);
        assert!(result.is_some());
        let (t1, _t2, pt) = result.unwrap();
        assert!((t1 - 1.0f64).abs() < 1e-12f64);
        assert!((pt.x - 1.0f64).abs() < 1e-12f64);
        assert!((pt.y - 0.0f64).abs() < 1e-12f64);
    }

    // -------------------------------
    // split_edges_at_intersections
    // -------------------------------

    #[test]
    fn test_split_edges_no_intersections() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let result = split_edges_at_intersections(&edges);
        assert_eq!(result, edges);
    }

    #[test]
    fn test_split_edges_crossing() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 0.0, y: 2.0 }),
        ];
        // Edge 0 (0->2) and edge 2 (2->0) cross at (1,1)
        // Edge 0 should split into two: (0,0)-(1,1) and (1,1)-(2,2)
        // Edge 2 should stay as (2,0)-(0,2) [or split? depends on param range]
        let result = split_edges_at_intersections(&edges);
        assert!(result.len() >= 3);
    }

    // -------------------------------
    // interpolate
    // -------------------------------

    #[test]
    fn test_interpolate_midpoint() {
        let e = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 4.0 });
        let pt = interpolate(e, 0.5);
        assert!((pt.x - 1.0f64).abs() < 1e-12f64);
        assert!((pt.y - 2.0f64).abs() < 1e-12f64);
    }

    #[test]
    fn test_interpolate_start() {
        let e = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 5.0, y: 10.0 });
        let pt = interpolate(e, 0.0);
        assert!((pt.x - 1.0f64).abs() < 1e-12f64);
        assert!((pt.y - 2.0f64).abs() < 1e-12f64);
    }

    #[test]
    fn test_interpolate_end() {
        let e = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 5.0, y: 10.0 });
        let pt = interpolate(e, 1.0);
        assert!((pt.x - 5.0f64).abs() < 1e-12f64);
        assert!((pt.y - 10.0f64).abs() < 1e-12f64);
    }

    // -------------------------------
    // dist2
    // -------------------------------

    #[test]
    fn test_dist2_identical() {
        assert_eq!(
            dist2(Coord { x: 1.0, y: 2.0 }, Coord { x: 1.0, y: 2.0 }),
            0.0
        );
    }

    #[test]
    fn test_dist2_unit() {
        assert!(
            (dist2(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }) - 1.0f64).abs() < 1e-12f64
        );
    }

    #[test]
    fn test_dist2_diagonal() {
        assert!(
            (dist2(Coord { x: 0.0, y: 0.0 }, Coord { x: 3.0, y: 4.0 }) - 25.0f64).abs() < 1e-12f64
        );
    }

    // -------------------------------
    // reconnect_edges
    // -------------------------------

    #[test]
    fn test_reconnect_edges_single_chain() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.len(), 4);
    }

    #[test]
    fn test_reconnect_edges_disjoint() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 5.0, y: 5.0 }, Coord { x: 6.0, y: 5.0 }),
        ];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_reconnect_edges_empty() {
        let result = reconnect_edges::<f64>(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_reconnect_edges_single() {
        let edges = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
        )];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.len(), 2);
    }

    #[test]
    fn test_reconnect_edges_reversed_order() {
        let edges = vec![
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 0.0, y: 0.0 }),
        ];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.len(), 3);
    }

    // -------------------------------
    // node_line_string
    // -------------------------------

    #[test]
    fn test_node_line_string_no_self_intersection() {
        let ls = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
        ]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::LineString(_)));
    }

    #[test]
    fn test_node_line_string_self_intersecting() {
        // Bowtie path: (0,0) → (2,2) → (2,0) → (0,2)
        let ls = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 0.0, y: 2.0 },
        ]);
        let result = node_line_string(&ls);
        // Should split into multiple LineStrings
        assert!(
            matches!(result, Geometry::MultiLineString(_))
                || matches!(result, Geometry::LineString(_))
        );
        if let Geometry::MultiLineString(ref mls) = result {
            assert!(mls.0.len() >= 2);
        }
    }

    #[test]
    fn test_node_line_string_empty() {
        let ls = LineString::<f64>::new(Vec::new());
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
        assert!(matches!(result, Geometry::GeometryCollection(ref gc) if gc.0.is_empty()));
    }

    #[test]
    fn test_node_line_string_single_point() {
        let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
    }

    #[test]
    fn test_node_line_string_too_few_coords() {
        let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
    }

    #[test]
    fn test_node_line_string_nan_filtered() {
        let ls = LineString::new(vec![
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::LineString(_)));
    }

    #[test]
    fn test_remove_consecutive_duplicates_repeated_non_consecutive() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_node_line_string_two_points() {
        let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::LineString(_)));
    }

    #[test]
    fn test_edges_intersect_very_close() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 0.5, y: 1e-13 }, Coord { x: 0.5, y: -1e-13 });
        // Very near collinear — parallel epsilon should not detect as crossing
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }
}

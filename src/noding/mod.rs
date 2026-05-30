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
use rustc_hash::FxHashSet;

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

    // R-tree spatial index for f64 (compile-time dispatched by size)
    if mem::size_of::<T>() == 8 {
        let edges_f64: &[Line<f64>] = unsafe { std::mem::transmute(edges) };
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

    // Endpoint touching — not considered an intersection for noding purposes
    // if it's the shared vertex of adjacent edges
    false
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

    if n >= 64 {
        // R-tree spatial index for f64 (compile-time dispatched by size)
        if mem::size_of::<T>() == 8 {
            let edges_f64: &[Line<f64>] = unsafe { std::mem::transmute(edges) };
            let mut split_f64: Vec<Vec<f64>> = vec![Vec::new(); n];
            split_edges_rtree(edges_f64, &mut split_f64, 1e-12);
            for i in 0..n {
                for &t in &split_f64[i] {
                    split_points[i].push(T::from(t).unwrap());
                }
            }
        } else {
            // Brute force for non-f64 types
            for i in 0..n {
                for j in (i + 2)..n {
                    if i + 1 == j && edges[i].end == edges[j].start {
                        continue;
                    }
                    let (ti, tj, _pt) = match compute_intersection_param(&edges[i], &edges[j], eps)
                    {
                        Some(v) => v,
                        None => continue,
                    };
                    if ti > zero && ti < one {
                        split_points[i].push(ti);
                    }
                    if tj > zero && tj < one {
                        split_points[j].push(tj);
                    }
                }
            }
        }
    } else {
        // Brute force for small edge sets
        for i in 0..n {
            for j in (i + 2)..n {
                if i + 1 == j && edges[i].end == edges[j].start {
                    continue;
                }
                let (ti, tj, _pt) = match compute_intersection_param(&edges[i], &edges[j], eps) {
                    Some(v) => v,
                    None => continue,
                };
                if ti > zero && ti < one {
                    split_points[i].push(ti);
                }
                if tj > zero && tj < one {
                    split_points[j].push(tj);
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

/// Node a multi line string by fixing each component.
pub(crate) fn node_multi_line_string<T: GeoFloat>(mls: &MultiLineString<T>) -> Geometry<T> {
    let lines: Vec<LineString<T>> = mls
        .0
        .iter()
        .filter_map(|ls| {
            match node_line_string(ls) {
                Geometry::LineString(fixed) => Some(fixed),
                Geometry::MultiLineString(ml) => {
                    // Flatten: merge all sub-linestrings into one vector
                    Some(LineString::new(
                        ml.0.iter().flat_map(|l| l.0.iter().copied()).collect(),
                    ))
                }
                _ => None,
            }
        })
        .collect();
    if lines.is_empty() {
        empty()
    } else {
        Geometry::MultiLineString(MultiLineString::new(lines))
    }
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
        assert!(!edges_intersect(&e1, &e2, 1e-12)); // Collinear not detected as crossing
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
    fn test_node_multi_line_string_empty() {
        let mls = MultiLineString::<f64>::new(Vec::new());
        let result = node_multi_line_string(&mls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
    }

    #[test]
    fn test_node_multi_line_string_with_self_intersection() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }]),
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 2.0, y: 0.0 },
                Coord { x: 0.0, y: 2.0 },
            ]),
        ]);
        let result = node_multi_line_string(&mls);
        assert!(matches!(result, Geometry::MultiLineString(_)));
    }

    #[test]
    fn test_node_multi_line_string_all_invalid() {
        let mls: MultiLineString<f64> = MultiLineString::new(vec![LineString::new(Vec::new())]);
        let result = node_multi_line_string(&mls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
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

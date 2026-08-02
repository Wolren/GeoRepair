#![allow(dead_code)]
//! Noding utilities for linear geometry repair.
//!
//! Linear geometries are fixed by:
//! 1. Removing invalid/repeated coordinates
//! 2. Noding self-intersections by splitting crossing edges

pub(crate) mod intersection;
pub(crate) mod snap_round;
pub(crate) mod sweep_line;
pub(crate) mod validator;

pub(crate) use self::intersection::NodingFloat;

#[cfg_attr(not(test), allow(unused_imports))]
use self::intersection::{
    check_self_intersections, collinear_split_params, compute_intersection_param, dist2,
    edges_intersect, interpolate, orient2d_generic,
};

use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString,
};
use rustc_hash::FxHashMap;
#[cfg(feature = "rstar")]
use rustc_hash::FxHashSet;

#[cfg(feature = "rstar")]
use crate::orient::orient2d as orient2d_robust;
#[cfg(any(feature = "arrange", feature = "structure"))]
use log::warn;

/// Node a line string by removing repeated points and splitting at
/// self-intersections. Returns a MultiLineString if splitting occurred,
/// or a single LineString otherwise.
pub(crate) fn node_line_string<T: NodingFloat>(ls: &LineString<T>) -> Geometry<T> {
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

    // Split at self-intersections (now uses DD arithmetic for f64)
    let split_edges = split_edges_at_intersections(&edges);
    if split_edges.is_empty() {
        return empty();
    }

    // ── Noding validation + snap-rounding fallback (f64 only) ──
    #[cfg(any(feature = "arrange", feature = "structure"))]
    if std::mem::size_of::<T>() == std::mem::size_of::<f64>() {
        let f64_edges: Vec<Line<f64>> = split_edges
            .iter()
            .map(|e| {
                Line::new(
                    Coord {
                        x: e.start.x.to_f64().expect("to_f64"),
                        y: e.start.y.to_f64().expect("to_f64"),
                    },
                    Coord {
                        x: e.end.x.to_f64().expect("to_f64"),
                        y: e.end.y.to_f64().expect("to_f64"),
                    },
                )
            })
            .collect();

        let mut validator = crate::noding::validator::NodingValidator::new(f64_edges);
        validator.validate();

        if validator.has_violations() {
            warn!(
                "node_line_string: {} noding violation(s) remain, falling back to snap rounding",
                validator.violations().len()
            );
            let f64_input: Vec<Line<f64>> = edges
                .iter()
                .map(|e| {
                    Line::new(
                        Coord {
                            x: e.start.x.to_f64().expect("to_f64"),
                            y: e.start.y.to_f64().expect("to_f64"),
                        },
                        Coord {
                            x: e.end.x.to_f64().expect("to_f64"),
                            y: e.end.y.to_f64().expect("to_f64"),
                        },
                    )
                })
                .collect();
            let snapped = crate::noding::snap_round::snap_round_lines(&f64_input);
            if !snapped.is_empty() {
                let converted: Vec<Line<T>> = snapped
                    .iter()
                    .map(|e| {
                        Line::new(
                            Coord {
                                x: T::from(e.start.x).expect("T::from(f64)"),
                                y: T::from(e.start.y).expect("T::from(f64)"),
                            },
                            Coord {
                                x: T::from(e.end.x).expect("T::from(f64)"),
                                y: T::from(e.end.y).expect("T::from(f64)"),
                            },
                        )
                    })
                    .collect();
                let linestrings = reconnect_edges(converted);
                return if linestrings.is_empty() {
                    empty()
                } else if linestrings.len() == 1 {
                    Geometry::LineString(linestrings.into_iter().next().expect("len==1 verified"))
                } else {
                    Geometry::MultiLineString(MultiLineString::new(linestrings))
                };
            }
        }
    }

    // Reconstruct linestrings from split edges (connect touching edges)
    let linestrings = reconnect_edges(split_edges);

    if linestrings.is_empty() {
        empty()
    } else if linestrings.len() == 1 {
        Geometry::LineString(linestrings.into_iter().next().expect("len==1 verified"))
    } else {
        Geometry::MultiLineString(MultiLineString::new(linestrings))
    }
}

/// Split edges at all pairwise intersection points.
fn split_edges_at_intersections<T: GeoFloat>(edges: &[Line<T>]) -> Vec<Line<T>> {
    let n = edges.len();
    let mut split_points: Vec<Vec<T>> = vec![Vec::new(); n];
    let eps = T::from(1e-12).expect("1e-12 fits any GeoFloat");
    let one = T::one();
    let zero = T::zero();

    #[cfg(feature = "rstar")]
    if n >= 64 {
        let edges_f64: Vec<Line<f64>> = edges
            .iter()
            .map(|l| {
                Line::new(
                    Coord {
                        x: l.start.x.to_f64().expect("to_f64"),
                        y: l.start.y.to_f64().expect("to_f64"),
                    },
                    Coord {
                        x: l.end.x.to_f64().expect("to_f64"),
                        y: l.end.y.to_f64().expect("to_f64"),
                    },
                )
            })
            .collect();
        let mut split_f64: Vec<Vec<f64>> = vec![Vec::new(); n];
        split_edges_rtree(&edges_f64, &mut split_f64, 1e-12);
        for i in 0..n {
            for &t in &split_f64[i] {
                split_points[i].push(T::from(t).expect("intersection param in range"));
            }
        }
    }
    #[cfg(not(feature = "rstar"))]
    {}
    if n < 64 || cfg!(not(feature = "rstar")) {
        for i in 0..n {
            for j in (i + 2)..n {
                if i + 1 == j && edges[i].end == edges[j].start {
                    continue;
                }
                if edges[i].start == edges[j].start
                    && orient2d_generic(edges[i].start, edges[i].end, edges[j].end) != T::zero()
                {
                    continue;
                }
                if edges[i].start == edges[j].end
                    && orient2d_generic(edges[i].start, edges[i].end, edges[j].start) != T::zero()
                {
                    continue;
                }
                if edges[i].end == edges[j].start
                    && orient2d_generic(edges[i].end, edges[i].start, edges[j].end) != T::zero()
                {
                    continue;
                }
                if edges[i].end == edges[j].end
                    && orient2d_generic(edges[i].end, edges[i].start, edges[j].start) != T::zero()
                {
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

    let eps_param = T::from(1e-14).expect("1e-14 fits any GeoFloat");
    let mut result = Vec::new();
    for i in 0..n {
        let e = edges[i];
        let mut params: Vec<T> = std::mem::take(&mut split_points[i]);
        params.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
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

#[cfg(feature = "rstar")]
fn split_edges_rtree(edges: &[Line<f64>], split_points: &mut [Vec<f64>], eps: f64) {
    let n = edges.len();

    #[derive(Clone, Copy)]
    struct EdgeEnv {
        idx: usize,
        env: rstar::AABB<[f64; 2]>,
    }
    impl rstar::RTreeObject for EdgeEnv {
        type Envelope = rstar::AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }

    let envs: Vec<EdgeEnv> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| EdgeEnv {
            idx: i,
            env: rstar::AABB::from_corners(
                [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
                [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
            ),
        })
        .collect();
    let tree = rstar::RTree::bulk_load(envs);

    let mut checked: FxHashSet<(usize, usize)> = FxHashSet::default();

    for i in 0..n {
        let e = &edges[i];
        let query = rstar::AABB::from_corners(
            [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
            [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
        );
        let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
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

            if edges[i].start == edges[j].start
                && crate::orient::orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if edges[i].start == edges[j].end
                && crate::orient::orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if edges[i].end == edges[j].start
                && crate::orient::orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if edges[i].end == edges[j].end
                && crate::orient::orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            let o1 = orient2d_robust(edges[i].start, edges[i].end, edges[j].start);
            let o2 = orient2d_robust(edges[i].start, edges[i].end, edges[j].end);
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

/// Reconnect split edges into continuous linestrings by chaining touching edges.
/// Uses the O(n) HashMap-based approach directly (no transmutes).
fn reconnect_edges<T: NodingFloat>(edges: Vec<Line<T>>) -> Vec<LineString<T>> {
    let n = edges.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![LineString::new(vec![edges[0].start, edges[0].end])];
    }
    reconnect_edges_by_key(edges)
}

/// O(n) chain reconstruction via HashMap from endpoint → edge index.
/// Works for any type implementing the sealed NodingFloat trait.
fn reconnect_edges_by_key<T: NodingFloat>(edges: Vec<Line<T>>) -> Vec<LineString<T>> {
    let n = edges.len();

    let mut start_map: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    let mut end_map: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    for (i, edge) in edges.iter().enumerate() {
        start_map
            .entry(NodingFloat::coord_hash_key(&edge.start))
            .or_default()
            .push(i);
        end_map
            .entry(NodingFloat::coord_hash_key(&edge.end))
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
                cands.iter().copied().rev().find(|&idx| {
                    !used[idx] && edges[idx].start == *chain.last().expect("non-empty chain")
                })
            });
            if let Some(idx) = fwd {
                chain.push(edges[idx].end);
                used[idx] = true;
                continue;
            }

            // Check backward end-matches (edge whose end == chain's last vertex)
            let bwd: Option<usize> = end_map.get(&last_key).and_then(|cands| {
                cands.iter().copied().rev().find(|&idx| {
                    !used[idx] && edges[idx].end == *chain.last().expect("non-empty chain")
                })
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
#[path = "mod_tests.rs"]
mod tests;


//! Post-noding validation: verify that no non-adjacent edges cross.
//!
//! Uses MCIndex (monotone chain + R-tree) spatial indexing for
//! O(n log n) validation instead of brute-force O(n²).
//!
//! GEOS's `NodingValidator` checks that after noding, the entire edge set
//! has no remaining intersections. If this check fails, a different noding
//! algorithm or tolerance should be used.

use crate::orient::orient2d;
use geo::{Coord, Line};
#[cfg(feature = "rstar")]
use rustc_hash::FxHashSet;

/// A violation found by [`NodingValidator`].
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NodingViolation {
    /// Index of the first intersecting edge.
    pub edge_a: usize,
    /// Index of the second intersecting edge.
    pub edge_b: usize,
    /// The location where the edges cross or overlap.
    pub at: Coord<f64>,
}

/// Validates that a set of noded line segments has no remaining intersections.
///
/// Only checks non-adjacent, non-consecutive edge pairs. Adjacent edges
/// that share an endpoint are allowed.
///
/// Uses MCIndex spatial indexing internally — O(n log n), safe for large
/// edge sets.
pub struct NodingValidator {
    edges: Vec<Line<f64>>,
    violations: Vec<NodingViolation>,
}

// ── MCIndex: monotone-chain spatial index ──

#[cfg(feature = "rstar")]
fn quadrant(dx: f64, dy: f64) -> u8 {
    if dx > 0.0 {
        if dy >= 0.0 { 0 } else { 1 }
    } else if dx < 0.0 {
        if dy > 0.0 { 3 } else { 2 }
    } else {
        if dy > 0.0 { 0 } else { 2 }
    }
}

#[cfg(feature = "rstar")]
struct ValMonoChain {
    start: usize,
    end: usize,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[cfg(feature = "rstar")]
fn build_chains(edges: &[Line<f64>]) -> Vec<ValMonoChain> {
    let n = edges.len();
    if n == 0 {
        return Vec::new();
    }
    let mut chains = Vec::new();
    let mut start = 0usize;
    let dx = edges[0].end.x - edges[0].start.x;
    let dy = edges[0].end.y - edges[0].start.y;
    let mut prev_quad = quadrant(dx, dy);
    let mut min_x = edges[0].start.x.min(edges[0].end.x);
    let mut max_x = edges[0].start.x.max(edges[0].end.x);
    let mut min_y = edges[0].start.y.min(edges[0].end.y);
    let mut max_y = edges[0].start.y.max(edges[0].end.y);

    for (i, s) in edges.iter().enumerate().skip(1) {
        let dx = s.end.x - s.start.x;
        let dy = s.end.y - s.start.y;
        let cur_quad = quadrant(dx, dy);
        min_x = min_x.min(s.start.x).min(s.end.x);
        max_x = max_x.max(s.start.x).max(s.end.x);
        min_y = min_y.min(s.start.y).min(s.end.y);
        max_y = max_y.max(s.start.y).max(s.end.y);
        if cur_quad != prev_quad {
            chains.push(ValMonoChain {
                start,
                end: i,
                min_x,
                max_x,
                min_y,
                max_y,
            });
            start = i;
            prev_quad = cur_quad;
            min_x = s.start.x.min(s.end.x);
            max_x = s.start.x.max(s.end.x);
            min_y = s.start.y.min(s.end.y);
            max_y = s.start.y.max(s.end.y);
        }
    }
    chains.push(ValMonoChain {
        start,
        end: n,
        min_x,
        max_x,
        min_y,
        max_y,
    });
    chains
}

#[cfg(feature = "rstar")]
struct ChainEnv {
    idx: usize,
    env: rstar::AABB<[f64; 2]>,
}
#[cfg(feature = "rstar")]
impl rstar::RTreeObject for ChainEnv {
    type Envelope = rstar::AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

#[cfg(feature = "rstar")]
fn sub_chain(edges: &[Line<f64>], start: usize, end: usize) -> ValMonoChain {
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for s in &edges[start..end] {
        min_x = min_x.min(s.start.x).min(s.end.x);
        max_x = max_x.max(s.start.x).max(s.end.x);
        min_y = min_y.min(s.start.y).min(s.end.y);
        max_y = max_y.max(s.start.y).max(s.end.y);
    }
    ValMonoChain {
        start,
        end,
        min_x,
        max_x,
        min_y,
        max_y,
    }
}

// ── Core validation logic ──

fn check_crossing_violation(
    edges: &[Line<f64>],
    i: usize,
    j: usize,
    eps: f64,
    violations: &mut Vec<NodingViolation>,
) {
    // Skip consecutive pairs that share an endpoint
    if j == i + 1 && edges[i].end == edges[j].start {
        return;
    }
    // Skip ring wrap: edge[n-1] → edge[0]
    if i == 0 && j == edges.len() - 1 && edges[j].end == edges[i].start {
        return;
    }
    let e1 = &edges[i];
    let e2 = &edges[j];
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);

    // Proper crossing: all four values non-zero with opposite signs
    if o1 != 0.0
        && o2 != 0.0
        && o3 != 0.0
        && o4 != 0.0
        && o1.signum() != o2.signum()
        && o3.signum() != o4.signum()
    {
        let pt = if cfg!(test) {
            compute_intersection(e1, e2, eps)
        } else {
            crate::dd::segment_intersection_dd(e1.start, e1.end, e2.start, e2.end)
                .map(|(pt, _, _)| pt)
        };
        violations.push(NodingViolation {
            edge_a: i,
            edge_b: j,
            at: pt.unwrap_or(Coord {
                x: (e1.start.x + e1.end.x + e2.start.x + e2.end.x) / 4.0,
                y: (e1.start.y + e1.end.y + e2.start.y + e2.end.y) / 4.0,
            }),
        });
        return;
    }

    // Collinear overlap
    if o1 == 0.0 && o2 == 0.0 && o3 == 0.0 && o4 == 0.0 && collinear_overlap_violation(e1, e2, eps)
    {
        let mid = Coord {
            x: (e1.start.x.max(e2.start.x).max(
                e1.end
                    .x
                    .min(e2.end.x)
                    .min(e1.start.x + e1.end.x + e2.start.x + e2.end.x)
                    / 4.0,
            )) / 2.0,
            y: (e1.start.y.max(e2.start.y).max(
                e1.end
                    .y
                    .min(e2.end.y)
                    .min(e1.start.y + e1.end.y + e2.start.y + e2.end.y)
                    / 4.0,
            )) / 2.0,
        };
        violations.push(NodingViolation {
            edge_a: i,
            edge_b: j,
            at: mid,
        });
    }
}

/// Recursive MCIndex divide-and-conquer: check all leaf pairs between two chains.
#[cfg(feature = "rstar")]
fn check_chain_pair(
    edges: &[Line<f64>],
    mc1: &ValMonoChain,
    mc2: &ValMonoChain,
    eps: f64,
    violations: &mut Vec<NodingViolation>,
    checked: &mut FxHashSet<(usize, usize)>,
) {
    // Bounding box overlap test
    if mc1.min_x > mc2.max_x + eps
        || mc1.max_x < mc2.min_x - eps
        || mc1.min_y > mc2.max_y + eps
        || mc1.max_y < mc2.min_y - eps
    {
        return;
    }

    // Leaf: both chains are single segments
    if mc1.end - mc1.start == 1 && mc2.end - mc2.start == 1 {
        let i = mc1.start;
        let j = mc2.start;
        if i >= j || !checked.insert((i, j)) {
            return;
        }
        check_crossing_violation(edges, i, j, eps, violations);
        return;
    }

    // Subdivide the larger chain
    if (mc1.end - mc1.start) >= (mc2.end - mc2.start) {
        let mid = (mc1.start + mc1.end) / 2;
        if mid > mc1.start {
            check_chain_pair(
                edges,
                &sub_chain(edges, mc1.start, mid),
                mc2,
                eps,
                violations,
                checked,
            );
        }
        if mid < mc1.end {
            check_chain_pair(
                edges,
                &sub_chain(edges, mid, mc1.end),
                mc2,
                eps,
                violations,
                checked,
            );
        }
    } else {
        let mid = (mc2.start + mc2.end) / 2;
        if mid > mc2.start {
            check_chain_pair(
                edges,
                mc1,
                &sub_chain(edges, mc2.start, mid),
                eps,
                violations,
                checked,
            );
        }
        if mid < mc2.end {
            check_chain_pair(
                edges,
                mc1,
                &sub_chain(edges, mid, mc2.end),
                eps,
                violations,
                checked,
            );
        }
    }
}

impl NodingValidator {
    /// Create a new validator for the given edge set.
    ///
    /// Validation is not performed until [`validate`](NodingValidator::validate)
    /// is called.
    pub fn new(edges: Vec<Line<f64>>) -> Self {
        Self {
            edges,
            violations: Vec::new(),
        }
    }

    /// Return the edge set being validated.
    #[allow(dead_code)]
    pub fn edges(&self) -> &[Line<f64>] {
        &self.edges
    }

    /// Return the list of violations found after validation.
    pub fn violations(&self) -> &[NodingViolation] {
        &self.violations
    }

    /// Whether any violations were found (shortcut for `!violations().is_empty()`).
    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    /// Validate all non-adjacent edge pairs.
    pub fn validate(&mut self) {
        self.violations.clear();
        let n = self.edges.len();
        if n < 2 {
            return;
        }
        let eps = 1e-12;

        #[cfg(feature = "rstar")]
        {
            let chains = build_chains(&self.edges);
            let envs: Vec<ChainEnv> = chains
                .iter()
                .enumerate()
                .map(|(i, mc)| ChainEnv {
                    idx: i,
                    env: rstar::AABB::from_corners([mc.min_x, mc.min_y], [mc.max_x, mc.max_y]),
                })
                .collect();
            let tree = rstar::RTree::bulk_load(envs);

            let mut checked: FxHashSet<(usize, usize)> = FxHashSet::default();

            for i in 0..chains.len() {
                let mc1 = &chains[i];
                let q = rstar::AABB::from_corners([mc1.min_x, mc1.min_y], [mc1.max_x, mc1.max_y]);
                let _ = tree.locate_in_envelope_intersecting_int(q, |c| {
                    let j = c.idx;
                    if j <= i {
                        return std::ops::ControlFlow::<(), ()>::Continue(());
                    }
                    check_chain_pair(
                        &self.edges,
                        mc1,
                        &chains[j],
                        eps,
                        &mut self.violations,
                        &mut checked,
                    );
                    std::ops::ControlFlow::<(), ()>::Continue(())
                });
                if mc1.end - mc1.start > 1 {
                    let mid = (mc1.start + mc1.end) / 2;
                    if mid > mc1.start {
                        let left = sub_chain(&self.edges, mc1.start, mid);
                        let right = sub_chain(&self.edges, mid, mc1.end);
                        check_chain_pair(
                            &self.edges,
                            &left,
                            &right,
                            eps,
                            &mut self.violations,
                            &mut checked,
                        );
                    }
                }
            }
        }
        #[cfg(not(feature = "rstar"))]
        {
            for i in 0..n {
                for j in (i + 1)..n {
                    check_crossing_violation(&self.edges, i, j, eps, &mut self.violations);
                }
            }
        }
    }
}

fn compute_intersection(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<Coord<f64>> {
    let denom = (e1.end.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e1.end.y - e1.start.y) * (e2.end.x - e2.start.x);
    if denom.abs() < eps {
        return None;
    }
    let t = ((e2.start.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e2.start.y - e1.start.y) * (e2.end.x - e2.start.x))
        / denom;
    Some(Coord {
        x: e1.start.x + t * (e1.end.x - e1.start.x),
        y: e1.start.y + t * (e1.end.y - e1.start.y),
    })
}

fn collinear_overlap_violation(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> bool {
    let dx = e1.end.x - e1.start.x;
    let dy = e1.end.y - e1.start.y;
    let dot = dx * dx + dy * dy;
    if dot <= eps {
        return false;
    }
    let t2s = ((e2.start.x - e1.start.x) * dx + (e2.start.y - e1.start.y) * dy) / dot;
    let t2e = ((e2.end.x - e1.start.x) * dx + (e2.end.y - e1.start.y) * dy) / dot;
    let (lo, hi) = if t2s < t2e { (t2s, t2e) } else { (t2e, t2s) };
    0.0f64.max(lo).min(1.0) < 1.0f64.min(hi).max(0.0) - eps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_no_intersections() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        let mut v = NodingValidator::new(edges);
        v.validate();
        assert!(!v.has_violations());
    }

    #[test]
    fn test_detects_crossing() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }),
            Line::new(Coord { x: 0.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let mut v = NodingValidator::new(edges);
        v.validate();
        assert!(v.has_violations());
        assert_eq!(v.violations().len(), 1);
    }

    #[test]
    fn test_endpoint_touch_valid() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
            Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let mut v = NodingValidator::new(edges);
        v.validate();
        assert!(!v.has_violations());
    }

    #[test]
    fn test_collinear_overlap_detected() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let mut v = NodingValidator::new(edges);
        v.validate();
        assert!(v.has_violations());
    }

    #[test]
    fn test_no_violations_for_disjoint() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: -1.0, y: 0.0 }),
        ];
        let mut v = NodingValidator::new(edges);
        v.validate();
        assert!(!v.has_violations());
    }

    #[test]
    fn test_large_coords() {
        let edges = vec![
            Line::new(
                Coord { x: 1e14, y: 1e14 },
                Coord {
                    x: 1e14 + 1.0,
                    y: 1e14 + 1.0,
                },
            ),
            Line::new(
                Coord {
                    x: 1e14,
                    y: 1e14 + 1.0,
                },
                Coord {
                    x: 1e14 + 1.0,
                    y: 1e14,
                },
            ),
        ];
        let mut v = NodingValidator::new(edges);
        v.validate();
        assert!(v.has_violations());
    }
}

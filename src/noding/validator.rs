//! Post-noding validation: verify that no non-adjacent edges cross.
//!
//! GEOS's `NodingValidator` checks that after noding, the entire edge set
//! has no remaining intersections. If this check fails, a different noding
//! algorithm or tolerance should be used.

use crate::orient::orient2d;
use geo::{Coord, Line};

/// A violation found by [`NodingValidator`].
#[derive(Clone, Debug)]
pub struct NodingViolation {
    pub edge_a: usize,
    pub edge_b: usize,
    pub at: Coord<f64>,
}

/// Validates that a set of noded line segments has no remaining intersections.
///
/// Only checks non-adjacent, non-consecutive edge pairs. Adjacent edges
/// that share an endpoint are allowed.
pub struct NodingValidator {
    edges: Vec<Line<f64>>,
    violations: Vec<NodingViolation>,
}

impl NodingValidator {
    pub fn new(edges: Vec<Line<f64>>) -> Self {
        Self {
            edges,
            violations: Vec::new(),
        }
    }

    pub fn edges(&self) -> &[Line<f64>] {
        &self.edges
    }

    pub fn violations(&self) -> &[NodingViolation] {
        &self.violations
    }

    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    pub fn validate(&mut self) {
        self.violations.clear();
        let n = self.edges.len();
        if n < 2 {
            return;
        }
        let eps = 1e-12;
        for i in 0..n {
            let e1 = &self.edges[i];
            for j in (i + 1)..n {
                // Skip consecutive pairs that share an endpoint
                if j == i + 1 && e1.end == self.edges[j].start {
                    continue;
                }
                // Skip ring wrap: edge[n-1] → edge[0]
                if i == 0 && j == n - 1 && self.edges[j].end == self.edges[i].start {
                    continue;
                }
                let e2 = &self.edges[j];
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
                    let pt = compute_intersection(e1, e2, eps);
                    self.violations.push(NodingViolation {
                        edge_a: i,
                        edge_b: j,
                        at: pt.unwrap_or(Coord {
                            x: (e1.start.x + e1.end.x + e2.start.x + e2.end.x) / 4.0,
                            y: (e1.start.y + e1.end.y + e2.start.y + e2.end.y) / 4.0,
                        }),
                    });
                    continue;
                }

                // Collinear overlap
                if o1 == 0.0 && o2 == 0.0 && o3 == 0.0 && o4 == 0.0 {
                    if collinear_overlap_violation(e1, e2, eps) {
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
                        self.violations.push(NodingViolation {
                            edge_a: i,
                            edge_b: j,
                            at: mid,
                        });
                    }
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
    let overlap = 0.0f64.max(lo).min(1.0) < 1.0f64.min(hi).max(0.0) - eps;
    overlap
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

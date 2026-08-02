//! Shared utility functions used across the crate.

use geo::Coord;

pub(crate) fn shoelace_sum(ring: &[Coord<f64>]) -> f64 {
    let mut sum = 0.0;
    for window in ring.windows(2) {
        sum += window[0].x * window[1].y - window[1].x * window[0].y;
    }
    sum
}

/// Determine CCW winding of a ring using Shewchuk's orient2d on the
/// rightmost-lowest vertex (guaranteed convex for simple rings).
/// Handles extreme fp ratios where shoelace sum flips sign.
pub(crate) fn robust_is_ccw(ring: &[Coord<f64>]) -> bool {
    if ring.len() < 4 {
        return false;
    }
    // Find the vertex with minimum x (rightmost if tie: minimum y too)
    let interior_n = ring.len() - 1;
    let mut min_idx = 0;
    let mut min_x = ring[0].x;
    let mut min_y = ring[0].y;
    for i in 1..interior_n {
        let c = &ring[i];
        if c.x < min_x || (c.x == min_x && c.y < min_y) {
            min_x = c.x;
            min_y = c.y;
            min_idx = i;
        }
    }
    let prev = if min_idx == 0 {
        &ring[interior_n - 1]
    } else {
        &ring[min_idx - 1]
    };
    let curr = &ring[min_idx];
    let next = &ring[(min_idx + 1) % interior_n];
    // orient2d > 0 means CCW for extremal vertex with min x
        let orient = robust::orient2d(
            robust::Coord { x: prev.x, y: prev.y },
            robust::Coord { x: curr.x, y: curr.y },
            robust::Coord { x: next.x, y: next.y },
        );
        // Collinear extremal vertex → shoelace fallback (Shewchuk gives 0)
        if orient.abs() <= 1e-15 {
            shoelace_sum(ring) > 0.0
        } else {
            orient > 0.0
        }
    }

/// A point strictly inside the ring: midpoint of the first non-degenerate
/// edge, nudged toward the ring interior by a relative epsilon (1e-9 of the
/// local magnitude). The interior side follows the ring's signed area
/// (CCW: left of the directed edge; CW: right). Returns None only for
/// degenerate rings (fewer than 3 distinct edges or all-zero-length edges).
///
/// Used for hole-aware nesting tests: vertex probes lie ON a hole ring for
/// islands that touch it, and exclusive point-in-ring semantics then read
/// the vertex as "not in the hole" (misclassifying island-in-hole as
/// nested-in-fill). A strictly-interior probe resolves the ambiguity.
pub(crate) fn ring_interior_probe(ring: &[Coord<f64>]) -> Option<Coord<f64>> {
    let interior_n = ring.len().saturating_sub(1);
    if interior_n < 3 {
        return None;
    }
    let mut sa = 0.0;
    for i in 0..interior_n {
        let a = ring[i];
        let b = ring[(i + 1) % interior_n];
        sa += a.x * b.y - b.x * a.y;
    }
    let sign = if sa >= 0.0 { 1.0 } else { -1.0 };
    for i in 0..interior_n {
        let v0 = ring[i];
        let v1 = ring[(i + 1) % interior_n];
        let dx = v1.x - v0.x;
        let dy = v1.y - v0.y;
        let len2 = dx * dx + dy * dy;
        if len2 <= 1e-24 {
            continue;
        }
        let len = len2.sqrt();
        let edge_mid = Coord {
            x: (v0.x + v1.x) * 0.5,
            y: (v0.y + v1.y) * 0.5,
        };
        let scale = edge_mid.x.abs().max(edge_mid.y.abs()).max(1.0);
        let eps = 1e-9 * scale;
        let (nx, ny) = (-dy / len, dx / len);
        return Some(Coord {
            x: edge_mid.x + nx * eps * sign,
            y: edge_mid.y + ny * eps * sign,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shoelace_sum_ccw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let sum = shoelace_sum(&ring);
        assert!(sum > 0.0);
        assert!((sum - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_shoelace_sum_cw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let sum = shoelace_sum(&ring);
        assert!(sum < 0.0);
        assert!((sum + 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_robust_is_ccw_ccw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(robust_is_ccw(&ring));
    }

    #[test]
    fn test_robust_is_ccw_cw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(!robust_is_ccw(&ring));
    }

    #[test]
    fn test_robust_is_ccw_extreme_fp() {
        // Ring with coords spanning 1e12 to 1e-12 — shoelace can flip
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1e12, y: 0.0 },
            Coord { x: 1e12, y: 1e-12 },
            Coord { x: 0.0, y: 1e-12 },
            Coord { x: 0.0, y: 0.0 },
        ];
        // Should be CCW regardless of fp extremes
        assert!(robust_is_ccw(&ring));
    }
}

//! Adaptive-precision orientation predicates (Shewchuk's algorithm).
//!
//! Wraps the `robust` crate to provide robust `orient2d` with
//! machine-precision fast path and exact arithmetic slow path.

use geo::Coord;

/// Robust 2D orientation test.
/// Returns a positive value if `pa`, `pb`, `pc` are counter-clockwise,
/// negative if clockwise, and zero if collinear.
///
/// Uses Shewchuk's adaptive precision algorithm via the `robust` crate.
#[inline(always)]
pub fn orient2d(pa: Coord<f64>, pb: Coord<f64>, pc: Coord<f64>) -> f64 {
    robust::orient2d(
        robust::Coord { x: pa.x, y: pa.y },
        robust::Coord { x: pb.x, y: pb.y },
        robust::Coord { x: pc.x, y: pc.y },
    )
}

/// Naive double-precision orient2d.
/// Fast but may produce incorrect results near degenerate cases.
#[inline(always)]
pub fn orient2d_fast(pa: Coord<f64>, pb: Coord<f64>, pc: Coord<f64>) -> f64 {
    (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orient2d_ccw() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 1.0, y: 0.0 };
        let pc = Coord { x: 0.5, y: 1.0 };
        assert!(orient2d(pa, pb, pc) > 0.0);
        assert!(orient2d_fast(pa, pb, pc) > 0.0);
    }

    #[test]
    fn test_orient2d_cw() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 0.5, y: 1.0 };
        let pc = Coord { x: 1.0, y: 0.0 };
        assert!(orient2d(pa, pb, pc) < 0.0);
        assert!(orient2d_fast(pa, pb, pc) < 0.0);
    }

    #[test]
    fn test_orient2d_collinear() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 1.0, y: 1.0 };
        let pc = Coord { x: 2.0, y: 2.0 };
        assert_eq!(orient2d(pa, pb, pc), 0.0);
        assert_eq!(orient2d_fast(pa, pb, pc), 0.0);
    }

    #[test]
    fn test_orient2d_near_collinear() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 1e10, y: 1e10 };
        let pc = Coord {
            x: 2e10,
            y: 2e10 + 1e-6,
        };
        let r_fast = orient2d_fast(pa, pb, pc);
        let r_robust = orient2d(pa, pb, pc);
        // Both should have the same sign, but robust may differ in magnitude
        assert_eq!(r_robust.signum(), r_fast.signum());
    }

    #[test]
    fn test_orient2d_large_coords() {
        let pa = Coord { x: 1e15, y: 1e15 };
        let pb = Coord {
            x: 1e15 + 1.0,
            y: 1e15,
        };
        let pc = Coord {
            x: 1e15 + 0.5,
            y: 1e15 + 1.0,
        };
        assert!(orient2d(pa, pb, pc) > 0.0);
    }

    #[test]
    fn test_orient2d_negative_area() {
        let pa = Coord { x: -10.0, y: -10.0 };
        let pb = Coord { x: 10.0, y: -10.0 };
        let pc = Coord { x: 0.0, y: 10.0 };
        assert!(orient2d(pa, pb, pc) > 0.0);
    }

    #[test]
    fn test_orient2d_zero_length_vectors() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 0.0, y: 0.0 };
        let pc = Coord { x: 1.0, y: 1.0 };
        assert_eq!(orient2d(pa, pb, pc), 0.0);
    }

    #[test]
    fn test_orient2d_degenerate_nearly_identical() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 1e-15, y: 1e-15 };
        let pc = Coord { x: 2e-15, y: 2e-15 };
        assert_eq!(orient2d(pa, pb, pc), 0.0);
    }

    #[test]
    fn test_orient2d_symmetry() {
        let pa = Coord { x: 0.0, y: 0.0 };
        let pb = Coord { x: 1.0, y: 0.0 };
        let pc = Coord { x: 0.5, y: 1.0 };
        // Swapping pb and pc should flip the sign
        assert_eq!(orient2d(pa, pb, pc), -orient2d(pa, pc, pb));
    }
}

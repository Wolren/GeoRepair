//! SIMD-accelerated orientation predicates.
//!
//! Provides packed implementations of orient2d (cross product of three coords)
//! using either:
//! - **Portable SIMD** (`std::simd`, nightly) — single code path for all platforms
//! - **AVX2** (x86_64) — 4-wide SIMD via x86 intrinsics
//! - **Scalar fallback** — auto-vectorized by the compiler
//!
//! The robust functions use a hybrid approach: SIMD fast-path with Shewchuk's
//! error bound check, falling back to the `robust` crate's exact adaptive-precision
//! arithmetic only when the fast result is within the error bound.

#[cfg(not(feature = "simd-portable"))]
use geo::{Coord, GeoFloat};
#[cfg(feature = "simd-portable")]
use geo::Coord;

use crate::orient::orient2d as orient2d_robust;

// ============================================================================
// Shared scalar helpers
// ============================================================================

#[cfg(not(feature = "simd-portable"))]
fn scalar_orient2d_batch<T: GeoFloat>(
    pa: &[Coord<T>; 4],
    pb: &[Coord<T>; 4],
    pc: &[Coord<T>; 4],
) -> [T; 4] {
    let mut out = [T::zero(); 4];
    for i in 0..4 {
        out[i] =
            (pb[i].x - pa[i].x) * (pc[i].y - pa[i].y) - (pb[i].y - pa[i].y) * (pc[i].x - pa[i].x);
    }
    out
}

#[cfg(not(feature = "simd-portable"))]
fn is_ring_ccw_scalar(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return true;
    }
    let mut area = 0.0;
    for j in 0..n {
        let next = (j + 1) % n;
        area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
    }
    area > 0.0
}

fn point_in_ring_scalar_loop(
    pt: Coord<f64>,
    coords: &[Coord<f64>],
    range: std::ops::Range<usize>,
) -> i32 {
    let mut wn = 0i32;
    for j in range {
        let p1 = coords[j];
        let p2 = coords[j + 1];
        if p1.y <= pt.y {
            if p2.y > pt.y {
                let o = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
                if o > 0.0 {
                    wn += 1;
                }
            }
        } else if p2.y <= pt.y {
            let o = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
            if o < 0.0 {
                wn -= 1;
            }
        }
    }
    wn
}

/// Point-in-ring test that accepts boundary points as inside (inclusive).
/// Used by the large-valid fast-path gate: GEOS IsValidOp allows a hole to
/// touch the shell at a point (OGC polygon validity), so a hole whose probe
/// vertex lies exactly ON the shell must not disqualify the polygon.
pub fn point_in_ring_inclusive_test
    (pt: Coord<f64>, coords: &[Coord<f64>]) -> bool {
    point_in_ring_inclusive(pt, coords)
}

#[allow(dead_code)]
pub(crate) fn point_in_ring_inclusive(pt: Coord<f64>, coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return false;
    }
    // Boundary check: point on any segment → inside.
    let eps = 1e-12;
    for i in 0..n - 1 {
        let a = coords[i];
        let b = coords[i + 1];
        let o = (b.x - a.x) * (pt.y - a.y) - (b.y - a.y) * (pt.x - a.x);
        if o.abs() <= eps {
            let between_x =
                (a.x - b.x).abs() > eps && pt.x > a.x.min(b.x) + eps && pt.x < a.x.max(b.x) - eps;
            let between_y =
                (a.y - b.y).abs() > eps && pt.y > a.y.min(b.y) + eps && pt.y < a.y.max(b.y) - eps;
            if between_x || between_y || pt == a || pt == b {
                return true;
            }
        }
    }
    // Strict interior via winding number.
    point_in_ring_exclusive(pt, coords)
}
// ============================================================================
// Robust hybrid: SIMD fast path + error-bound check → exact fallback
// ============================================================================

pub(crate) fn orient2d_batch_4_robust(
    pa: &[Coord<f64>; 4],
    pb: &[Coord<f64>; 4],
    pc: &[Coord<f64>; 4],
) -> [f64; 4] {
    let fast = orient2d_batch_4(pa, pb, pc);

    let epsilon = f64::EPSILON;
    let coeff = 3.0 + 16.0 * epsilon;

    let mut out = fast;
    for i in 0..4 {
        let ax = pa[i].x - pc[i].x;
        let ay = pa[i].y - pc[i].y;
        let bx = pb[i].x - pc[i].x;
        let by = pb[i].y - pc[i].y;
        let abs_det = (ax * by).abs() + (bx * ay).abs();
        let error_bound = coeff * epsilon * abs_det;

        if fast[i].abs() <= error_bound {
            out[i] = orient2d_robust(pa[i], pb[i], pc[i]);
        }
    }

    out
}

#[cfg(feature = "simd-portable")]
mod portable;
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
mod x86_64;
#[cfg(all(not(feature = "simd-portable"), not(target_arch = "x86_64")))]
mod fallback;
#[cfg(test)]
mod tests;

#[cfg(feature = "simd-portable")]
pub(crate) use portable::{
    aabb_minmax_simd, is_ring_ccw_simd, orient2d_batch_4, point_in_ring_exclusive,
    point_in_ring_inclusive, snap_coords_simd,
};
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
pub(crate) use x86_64::{
    aabb_minmax_simd, is_ring_ccw_simd, orient2d_batch_4, point_in_ring_exclusive,
    snap_coords_simd,
};
#[cfg(all(not(feature = "simd-portable"), not(target_arch = "x86_64")))]
pub(crate) use fallback::{
    aabb_minmax_simd, is_ring_ccw_simd, orient2d_batch_4, point_in_ring_exclusive,
    snap_coords_simd,
};

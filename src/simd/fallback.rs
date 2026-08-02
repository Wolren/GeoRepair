//! Scalar fallback (non-x86_64, no portable SIMD).

use super::*;


// ============================================================================
// Scalar fallback (non-x86_64, no portable SIMD)
// ============================================================================

#[cfg(not(feature = "simd-portable"))]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn orient2d_batch_4(
    pa: &[Coord<f64>; 4],
    pb: &[Coord<f64>; 4],
    pc: &[Coord<f64>; 4],
) -> [f64; 4] {
    scalar_orient2d_batch(pa, pb, pc)
}

#[cfg(not(feature = "simd-portable"))]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
    is_ring_ccw_scalar(coords)
}

#[cfg(not(feature = "simd-portable"))]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn point_in_ring_exclusive(pt: Coord<f64>, coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return false;
    }
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
                return false;
            }
        }
    }
    let wn = point_in_ring_scalar_loop(pt, coords, 0..n - 1);
    wn != 0
}

#[cfg(not(feature = "simd-portable"))]
#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn snap_coords_simd(coords: &mut [Coord<f64>], scale: f64) {
    for c in coords.iter_mut() {
        c.x = (c.x / scale).round() * scale;
        c.y = (c.y / scale).round() * scale;
    }
}

#[cfg(not(feature = "simd-portable"))]
#[cfg(not(target_arch = "x86_64"))]
#[allow(dead_code)]
pub(crate) fn aabb_minmax_simd(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    let (mut mnx, mut mxx, mut mny, mut mxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in coords {
        mnx = mnx.min(c.x);
        mxx = mxx.max(c.x);
        mny = mny.min(c.y);
        mxy = mxy.max(c.y);
    }
    (mnx, mxx, mny, mxy)
}

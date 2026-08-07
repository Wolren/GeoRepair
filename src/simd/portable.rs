//! Portable SIMD (nightly-only, cross-platform via core::simd).

use super::*;


// ============================================================================
// Portable SIMD (nightly-only, cross-platform via core::simd)
// ============================================================================

#[cfg(feature = "simd-portable")]
pub(crate) fn orient2d_batch_4(
    pa: &[Coord<f64>; 4],
    pb: &[Coord<f64>; 4],
    pc: &[Coord<f64>; 4],
) -> [f64; 4] {
    use core::simd::f64x4;

    let pax = f64x4::from_array([pa[0].x, pa[1].x, pa[2].x, pa[3].x]);
    let pay = f64x4::from_array([pa[0].y, pa[1].y, pa[2].y, pa[3].y]);
    let pbx = f64x4::from_array([pb[0].x, pb[1].x, pb[2].x, pb[3].x]);
    let pby = f64x4::from_array([pb[0].y, pb[1].y, pb[2].y, pb[3].y]);
    let pcx = f64x4::from_array([pc[0].x, pc[1].x, pc[2].x, pc[3].x]);
    let pcy = f64x4::from_array([pc[0].y, pc[1].y, pc[2].y, pc[3].y]);

    let result = (pbx - pax) * (pcy - pay) - (pby - pay) * (pcx - pax);

    result.to_array()
}

#[cfg(feature = "simd-portable")]
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return true;
    }
    let mut area = 0.0f64;
    use core::simd::f64x4;
    use core::simd::num::SimdFloat;
    let mut i = 0usize;
    while i + 4 <= n {
        let xs = f64x4::from_array([
            coords[i].x,
            coords[i + 1].x,
            coords[i + 2].x,
            coords[i + 3].x,
        ]);
        let ys = f64x4::from_array([
            coords[i].y,
            coords[i + 1].y,
            coords[i + 2].y,
            coords[i + 3].y,
        ]);
        let next_xs = f64x4::from_array([
            coords[(i + 1) % n].x,
            coords[(i + 2) % n].x,
            coords[(i + 3) % n].x,
            coords[(i + 4) % n].x,
        ]);
        let next_ys = f64x4::from_array([
            coords[(i + 1) % n].y,
            coords[(i + 2) % n].y,
            coords[(i + 3) % n].y,
            coords[(i + 4) % n].y,
        ]);
        area += (xs * next_ys - next_xs * ys).reduce_sum();
        i += 4;
    }
    for j in i..n {
        let next = (j + 1) % n;
        area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
    }
    area > 0.0
}

/// Snap all coordinates to a uniform grid: `coord = (coord / scale).round() * scale`.
/// Non-finite values pass through unchanged (IEEE 754 semantics).
#[cfg(feature = "simd-portable")]
pub(crate) fn snap_coords_simd(coords: &mut [Coord<f64>], scale: f64) {
    use core::simd::f64x4;
    use core::simd::num::SimdFloat;
    let scale_v = f64x4::splat(scale);
    let inv_scale_v = f64x4::splat(1.0 / scale);
    let n = coords.len();
    let mut i = 0usize;
    while i + 4 <= n {
        let xs = f64x4::from_array([
            coords[i].x,
            coords[i + 1].x,
            coords[i + 2].x,
            coords[i + 3].x,
        ]);
        let ys = f64x4::from_array([
            coords[i].y,
            coords[i + 1].y,
            coords[i + 2].y,
            coords[i + 3].y,
        ]);
        let snapped_xs = (xs * inv_scale_v).round() * scale_v;
        let snapped_ys = (ys * inv_scale_v).round() * scale_v;
        let rx: [f64; 4] = snapped_xs.to_array();
        let ry: [f64; 4] = snapped_ys.to_array();
        for j in 0..4 {
            coords[i + j].x = rx[j];
            coords[i + j].y = ry[j];
        }
        i += 4;
    }
    for c in &mut coords[i..] {
        c.x = (c.x / scale).round() * scale;
        c.y = (c.y / scale).round() * scale;
    }
}

/// Compute the axis-aligned bounding box (min_x, max_x, min_y, max_y) of a
/// coordinate slice using SIMD reductions.
#[cfg(feature = "simd-portable")]
#[allow(dead_code)]
pub(crate) fn aabb_minmax_simd(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    use core::simd::f64x4;
    use core::simd::num::SimdFloat;
    let n = coords.len();
    if n == 0 {
        return (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    }
    let mut min_x = f64x4::splat(f64::MAX);
    let mut max_x = f64x4::splat(f64::MIN);
    let mut min_y = f64x4::splat(f64::MAX);
    let mut max_y = f64x4::splat(f64::MIN);
    let mut i = 0usize;
    while i + 4 <= n {
        let xs = f64x4::from_array([
            coords[i].x,
            coords[i + 1].x,
            coords[i + 2].x,
            coords[i + 3].x,
        ]);
        let ys = f64x4::from_array([
            coords[i].y,
            coords[i + 1].y,
            coords[i + 2].y,
            coords[i + 3].y,
        ]);
        min_x = min_x.simd_min(xs);
        max_x = max_x.simd_max(xs);
        min_y = min_y.simd_min(ys);
        max_y = max_y.simd_max(ys);
        i += 4;
    }
    let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &c in &coords[i..] {
        gmin_x = gmin_x.min(c.x);
        gmax_x = gmax_x.max(c.x);
        gmin_y = gmin_y.min(c.y);
        gmax_y = gmax_y.max(c.y);
    }
    let min_x_arr: [f64; 4] = min_x.to_array();
    let max_x_arr: [f64; 4] = max_x.to_array();
    let min_y_arr: [f64; 4] = min_y.to_array();
    let max_y_arr: [f64; 4] = max_y.to_array();
    for j in 0..4 {
        gmin_x = gmin_x.min(min_x_arr[j]);
        gmax_x = gmax_x.max(max_x_arr[j]);
        gmin_y = gmin_y.min(min_y_arr[j]);
        gmax_y = gmax_y.max(max_y_arr[j]);
    }
    (gmin_x, gmax_x, gmin_y, gmax_y)
}

#[cfg(feature = "simd-portable")]
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
    let mut wn = 0i32;
    use core::simd::f64x4;
    let mut i = 0usize;
    while i + 5 <= n {
        let pax = f64x4::splat(pt.x);
        let pay = f64x4::splat(pt.y);
        let pbx = f64x4::from_array([
            coords[i].x,
            coords[i + 1].x,
            coords[i + 2].x,
            coords[i + 3].x,
        ]);
        let pby = f64x4::from_array([
            coords[i].y,
            coords[i + 1].y,
            coords[i + 2].y,
            coords[i + 3].y,
        ]);
        let pcx = f64x4::from_array([
            coords[i + 1].x,
            coords[i + 2].x,
            coords[i + 3].x,
            coords[i + 4].x,
        ]);
        let pcy = f64x4::from_array([
            coords[i + 1].y,
            coords[i + 2].y,
            coords[i + 3].y,
            coords[i + 4].y,
        ]);
        let orient = (pbx - pax) * (pcy - pay) - (pby - pay) * (pcx - pax);
        let arr: [f64; 4] = orient.to_array();
        for j in 0..4 {
            let p1 = coords[i + j];
            let p2 = coords[i + j + 1];
            if p1.y <= pt.y {
                if p2.y > pt.y && arr[j] > 0.0 {
                    wn += 1;
                }
            } else if p2.y <= pt.y && arr[j] < 0.0 {
                wn -= 1;
            }
        }
        i += 4;
    }
    wn += point_in_ring_scalar_loop(pt, coords, i..n - 1);
    wn != 0
}

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

// ============================================================================
// Portable SIMD (nightly-only, cross-platform via std::simd)
// ============================================================================

#[cfg(feature = "simd-portable")]
pub(crate) fn orient2d_batch_4(
    pa: &[Coord<f64>; 4],
    pb: &[Coord<f64>; 4],
    pc: &[Coord<f64>; 4],
) -> [f64; 4] {
    use std::simd::f64x4;

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
    use std::simd::f64x4;
    use std::simd::num::SimdFloat;
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
    use std::simd::f64x4;
    use std::simd::num::SimdFloat;
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
    use std::simd::f64x4;
    use std::simd::num::SimdFloat;
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
    use std::simd::f64x4;
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

// ============================================================================
// x86_64 AVX2 intrinsics
// ============================================================================

#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
pub(crate) fn orient2d_batch_4(
    pa: &[Coord<f64>; 4],
    pb: &[Coord<f64>; 4],
    pc: &[Coord<f64>; 4],
) -> [f64; 4] {
    #[cfg(target_feature = "avx")]
    // SAFETY: The `avx` target feature is verified at compile time by
    // `#[cfg(target_feature = "avx")]`.  Input arrays `pa`, `pb`, `pc` are
    // on the stack (valid aligned reads).  The output buffer `out` is a
    // stack-allocated array whose pointer is valid for write via
    // `_mm256_storeu_pd` (no alignment required).
    unsafe {
        use std::arch::x86_64::*;
        let pbx = _mm256_setr_pd(pb[0].x, pb[1].x, pb[2].x, pb[3].x);
        let pax = _mm256_setr_pd(pa[0].x, pa[1].x, pa[2].x, pa[3].x);
        let pcy = _mm256_setr_pd(pc[0].y, pc[1].y, pc[2].y, pc[3].y);
        let pay = _mm256_setr_pd(pa[0].y, pa[1].y, pa[2].y, pa[3].y);
        let pby = _mm256_setr_pd(pb[0].y, pb[1].y, pb[2].y, pb[3].y);
        let pcx = _mm256_setr_pd(pc[0].x, pc[1].x, pc[2].x, pc[3].x);

        let dx = _mm256_sub_pd(pbx, pax);
        let dy = _mm256_sub_pd(pcy, pay);
        let term1 = _mm256_mul_pd(dx, dy);

        let dy2 = _mm256_sub_pd(pby, pay);
        let dx2 = _mm256_sub_pd(pcx, pax);
        let term2 = _mm256_mul_pd(dy2, dx2);

        let result = _mm256_sub_pd(term1, term2);

        let mut out = [0.0f64; 4];
        _mm256_storeu_pd(out.as_mut_ptr(), result);
        out
    }
    #[cfg(not(target_feature = "avx"))]
    {
        scalar_orient2d_batch(pa, pb, pc)
    }
}

#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
fn is_ring_ccw_4wide(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return true;
    }
    #[cfg(not(target_feature = "avx"))]
    {
        is_ring_ccw_scalar(coords)
    }
    #[cfg(target_feature = "avx")]
    {
        use std::arch::x86_64::*;
        let mut i = 0usize;
        let mut area = 0.0f64;
        while i + 4 <= n {
            let pb = [
                coords[i],
                coords[(i + 1) % n],
                coords[(i + 2) % n],
                coords[(i + 3) % n],
            ];
            let pc = [
                coords[(i + 1) % n],
                coords[(i + 2) % n],
                coords[(i + 3) % n],
                coords[(i + 4) % n],
            ];
            // SAFETY: Verified avx feature. Stack arrays.
            let batch = unsafe {
                let pbx = _mm256_set_pd(pb[0].x, pb[1].x, pb[2].x, pb[3].x);
                let pby = _mm256_set_pd(pb[0].y, pb[1].y, pb[2].y, pb[3].y);
                let pcx = _mm256_set_pd(pc[0].x, pc[1].x, pc[2].x, pc[3].x);
                let pcy = _mm256_set_pd(pc[0].y, pc[1].y, pc[2].y, pc[3].y);
                let term1 = _mm256_mul_pd(pbx, pcy);
                let term2 = _mm256_mul_pd(pby, pcx);
                let result = _mm256_sub_pd(term1, term2);
                let mut out = [0.0f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), result);
                out
            };
            area += batch[0] + batch[1] + batch[2] + batch[3];
            i += 4;
        }
        for j in i..n {
            let next = (j + 1) % n;
            area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
        }
        area > 0.0
    }
}

#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
fn point_in_ring_4wide(pt: Coord<f64>, coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return false;
    }
    #[cfg(not(target_feature = "avx"))]
    {
        point_in_ring_scalar_loop(pt, coords, 0..n - 1) != 0
    }
    #[cfg(target_feature = "avx")]
    {
        let mut wn = 0i32;
        let mut i = 0usize;
        while i + 5 <= n {
            let pa = [pt; 4];
            let pb = [coords[i], coords[i + 1], coords[i + 2], coords[i + 3]];
            let pc = [coords[i + 1], coords[i + 2], coords[i + 3], coords[i + 4]];
            let orient = orient2d_batch_4_robust(&pa, &pb, &pc);
            for j in 0..4 {
                let p1 = pb[j];
                let p2 = pc[j];
                if p1.y <= pt.y {
                    if p2.y > pt.y && orient[j] > 0.0 {
                        wn += 1;
                    }
                } else if p2.y <= pt.y && orient[j] < 0.0 {
                    wn -= 1;
                }
            }
            i += 4;
        }
        wn += point_in_ring_scalar_loop(pt, coords, i..n - 1);
        wn != 0
    }
}

// ============================================================================
// 8-wide AVX-512 kernels
// ============================================================================

/// Batch-8 orient2d with AVX-512 (8-wide).
/// Uses per-element loads (`setr_pd`) because Coord is AoS layout
/// (x, y interleaved) — contiguous `loadu_pd` would mix x and y values.
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
#[allow(dead_code)]
pub(crate) fn orient2d_batch_8(
    pa: &[Coord<f64>; 8],
    pb: &[Coord<f64>; 8],
    pc: &[Coord<f64>; 8],
) -> [f64; 8] {
    #[cfg(target_feature = "avx512f")]
    // SAFETY: avx512f target feature verified at compile time.
    // Input arrays are stack-local with valid Coord<f64> values.
    // `setr_pd` performs 8 independent scalar loads from the struct fields.
    unsafe {
        use std::arch::x86_64::*;
        let pax = _mm512_setr_pd(
            pa[0].x, pa[1].x, pa[2].x, pa[3].x, pa[4].x, pa[5].x, pa[6].x, pa[7].x,
        );
        let pay = _mm512_setr_pd(
            pa[0].y, pa[1].y, pa[2].y, pa[3].y, pa[4].y, pa[5].y, pa[6].y, pa[7].y,
        );
        let pbx = _mm512_setr_pd(
            pb[0].x, pb[1].x, pb[2].x, pb[3].x, pb[4].x, pb[5].x, pb[6].x, pb[7].x,
        );
        let pby = _mm512_setr_pd(
            pb[0].y, pb[1].y, pb[2].y, pb[3].y, pb[4].y, pb[5].y, pb[6].y, pb[7].y,
        );
        let pcx = _mm512_setr_pd(
            pc[0].x, pc[1].x, pc[2].x, pc[3].x, pc[4].x, pc[5].x, pc[6].x, pc[7].x,
        );
        let pcy = _mm512_setr_pd(
            pc[0].y, pc[1].y, pc[2].y, pc[3].y, pc[4].y, pc[5].y, pc[6].y, pc[7].y,
        );
        let dx1 = _mm512_sub_pd(pbx, pax);
        let dy1 = _mm512_sub_pd(pcy, pay);
        let term1 = _mm512_mul_pd(dx1, dy1);
        let dy2 = _mm512_sub_pd(pby, pay);
        let dx2 = _mm512_sub_pd(pcx, pax);
        let term2 = _mm512_mul_pd(dy2, dx2);
        let result = _mm512_sub_pd(term1, term2);
        let mut out = [0.0f64; 8];
        _mm512_storeu_pd(out.as_mut_ptr(), result);
        out
    }
    #[cfg(not(target_feature = "avx512f"))]
    {
        let lo = orient2d_batch_4(
            &[pa[0], pa[1], pa[2], pa[3]],
            &[pb[0], pb[1], pb[2], pb[3]],
            &[pc[0], pc[1], pc[2], pc[3]],
        );
        let hi = orient2d_batch_4(
            &[pa[4], pa[5], pa[6], pa[7]],
            &[pb[4], pb[5], pb[6], pb[7]],
            &[pc[4], pc[5], pc[6], pc[7]],
        );
        let mut out = [0.0f64; 8];
        out[..4].copy_from_slice(&lo);
        out[4..].copy_from_slice(&hi);
        out
    }
}

/// Robust batch-8 orient2d with Shewchuk fallback.
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
#[allow(dead_code)]
pub(crate) fn orient2d_batch_8_robust(
    pa: &[Coord<f64>; 8],
    pb: &[Coord<f64>; 8],
    pc: &[Coord<f64>; 8],
) -> [f64; 8] {
    let fast = orient2d_batch_8(pa, pb, pc);
    let epsilon = f64::EPSILON;
    let coeff = 3.0 + 16.0 * epsilon;
    let mut out = fast;
    for i in 0..8 {
        let fast_i = fast[i];
        if fast_i == 0.0 {
            out[i] = orient2d_robust(pa[i], pb[i], pc[i]);
        } else {
            let ax = pa[i].x - pc[i].x;
            let ay = pa[i].y - pc[i].y;
            let bx = pb[i].x - pc[i].x;
            let by = pb[i].y - pc[i].y;
            let abs_det = (ax * by).abs() + (bx * ay).abs();
            let error_bound = coeff * epsilon * abs_det;
            if fast_i.abs() <= error_bound {
                out[i] = orient2d_robust(pa[i], pb[i], pc[i]);
            }
        }
    }
    out
}

/// 8-wide ring CCW check using AVX-512.
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 3 {
        return true;
    }
    #[cfg(not(target_feature = "avx512f"))]
    {
        // Delegate to 4-wide scalar/AVX2 version
        is_ring_ccw_4wide(coords)
    }
    #[cfg(target_feature = "avx512f")]
    {
        use std::arch::x86_64::*;
        let mut i = 0usize;
        let mut area = 0.0f64;
        while i + 8 <= n {
            // SAFETY: Stack arrays, verified target feature.
            unsafe {
                let xs = _mm512_setr_pd(
                    coords[i].x,
                    coords[i + 1].x,
                    coords[i + 2].x,
                    coords[i + 3].x,
                    coords[i + 4].x,
                    coords[i + 5].x,
                    coords[i + 6].x,
                    coords[i + 7].x,
                );
                let ys = _mm512_setr_pd(
                    coords[i].y,
                    coords[i + 1].y,
                    coords[i + 2].y,
                    coords[i + 3].y,
                    coords[i + 4].y,
                    coords[i + 5].y,
                    coords[i + 6].y,
                    coords[i + 7].y,
                );
                let next_xs = _mm512_setr_pd(
                    coords[(i + 1) % n].x,
                    coords[(i + 2) % n].x,
                    coords[(i + 3) % n].x,
                    coords[(i + 4) % n].x,
                    coords[(i + 5) % n].x,
                    coords[(i + 6) % n].x,
                    coords[(i + 7) % n].x,
                    coords[(i + 8) % n].x,
                );
                let next_ys = _mm512_setr_pd(
                    coords[(i + 1) % n].y,
                    coords[(i + 2) % n].y,
                    coords[(i + 3) % n].y,
                    coords[(i + 4) % n].y,
                    coords[(i + 5) % n].y,
                    coords[(i + 6) % n].y,
                    coords[(i + 7) % n].y,
                    coords[(i + 8) % n].y,
                );
                let term1 = _mm512_mul_pd(xs, next_ys);
                let term2 = _mm512_mul_pd(next_xs, ys);
                let partial = _mm512_sub_pd(term1, term2);
                area += _mm512_reduce_add_pd(partial);
            }
            i += 8;
        }
        for j in i..n {
            let next = (j + 1) % n;
            area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
        }
        area > 0.0
    }
}

/// 8-wide point-in-ring test using AVX-512.
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
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
    #[cfg(not(target_feature = "avx512f"))]
    {
        point_in_ring_4wide(pt, coords)
    }
    #[cfg(target_feature = "avx512f")]
    {
        let mut wn = 0i32;
        let mut i = 0usize;
        while i + 9 <= n {
            let pa = [pt; 8];
            let mut pb = [coords[0]; 8];
            let mut pc = [coords[0]; 8];
            for j in 0..8 {
                pb[j] = coords[i + j];
                pc[j] = coords[i + j + 1];
            }
            let orient = orient2d_batch_8_robust(&pa, &pb, &pc);
            for j in 0..8 {
                let p1 = pb[j];
                let p2 = pc[j];
                if p1.y <= pt.y {
                    if p2.y > pt.y && orient[j] > 0.0 {
                        wn += 1;
                    }
                } else if p2.y <= pt.y && orient[j] < 0.0 {
                    wn -= 1;
                }
            }
            i += 8;
        }
        wn += point_in_ring_scalar_loop(pt, coords, i..n - 1);
        wn != 0
    }
}

/// Snap all coordinates to a uniform grid using AVX2.
/// `coord = (coord / scale).round() * scale`
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
pub(crate) fn snap_coords_simd(coords: &mut [Coord<f64>], scale: f64) {
    #[cfg(target_feature = "avx")]
    let n = coords.len();
    #[cfg(not(target_feature = "avx"))]
    {
        for c in coords.iter_mut() {
            c.x = (c.x / scale).round() * scale;
            c.y = (c.y / scale).round() * scale;
        }
    }
    #[cfg(target_feature = "avx")]
    {
        use std::arch::x86_64::*;
        const ROUNDING: i32 = _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC;
        let mut i = 0usize;
        while i + 4 <= n {
            // SAFETY: Stack-local arrays, verified avx feature.
            unsafe {
                let scale_v = _mm256_set1_pd(scale);
                let inv_scale_v = _mm256_set1_pd(1.0 / scale);
                let xs = _mm256_setr_pd(
                    coords[i].x,
                    coords[i + 1].x,
                    coords[i + 2].x,
                    coords[i + 3].x,
                );
                let ys = _mm256_setr_pd(
                    coords[i].y,
                    coords[i + 1].y,
                    coords[i + 2].y,
                    coords[i + 3].y,
                );
                let div_xs = _mm256_mul_pd(xs, inv_scale_v);
                let div_ys = _mm256_mul_pd(ys, inv_scale_v);
                let rnd_xs = _mm256_round_pd(div_xs, ROUNDING);
                let rnd_ys = _mm256_round_pd(div_ys, ROUNDING);
                let snapped_xs = _mm256_mul_pd(rnd_xs, scale_v);
                let snapped_ys = _mm256_mul_pd(rnd_ys, scale_v);
                let mut x_arr = [0.0f64; 4];
                let mut y_arr = [0.0f64; 4];
                _mm256_storeu_pd(x_arr.as_mut_ptr(), snapped_xs);
                _mm256_storeu_pd(y_arr.as_mut_ptr(), snapped_ys);
                for j in 0..4 {
                    coords[i + j].x = x_arr[j];
                    coords[i + j].y = y_arr[j];
                }
            }
            i += 4;
        }
        for c in &mut coords[i..] {
            c.x = (c.x / scale).round() * scale;
            c.y = (c.y / scale).round() * scale;
        }
    }
}

/// Compute bounding box of a coordinate slice using AVX2 min/max reductions.
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
pub(crate) fn aabb_minmax_simd(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    let n = coords.len();
    if n == 0 {
        return (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    }
    #[cfg(not(target_feature = "avx"))]
    {
        let (mut mnx, mut mxx, mut mny, mut mxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for c in coords {
            mnx = mnx.min(c.x);
            mxx = mxx.max(c.x);
            mny = mny.min(c.y);
            mxy = mxy.max(c.y);
        }
        (mnx, mxx, mny, mxy)
    }
    #[cfg(target_feature = "avx")]
    {
        use std::arch::x86_64::*;
        // SAFETY: f64 scalars, verified avx feature.
        let mut min_xv = unsafe { _mm256_set1_pd(f64::MAX) };
        let mut max_xv = unsafe { _mm256_set1_pd(f64::MIN) };
        let mut min_yv = unsafe { _mm256_set1_pd(f64::MAX) };
        let mut max_yv = unsafe { _mm256_set1_pd(f64::MIN) };
        let mut i = 0usize;
        while i + 4 <= n {
            // SAFETY: Stack-local arrays, verified avx feature.
            unsafe {
                let xs = _mm256_setr_pd(
                    coords[i].x,
                    coords[i + 1].x,
                    coords[i + 2].x,
                    coords[i + 3].x,
                );
                let ys = _mm256_setr_pd(
                    coords[i].y,
                    coords[i + 1].y,
                    coords[i + 2].y,
                    coords[i + 3].y,
                );
                min_xv = _mm256_min_pd(min_xv, xs);
                max_xv = _mm256_max_pd(max_xv, xs);
                min_yv = _mm256_min_pd(min_yv, ys);
                max_yv = _mm256_max_pd(max_yv, ys);
            }
            i += 4;
        }
        // Horizontal reduction of the 4 lanes
        let mut mnx: [f64; 4] = [0.0; 4];
        let mut mxx: [f64; 4] = [0.0; 4];
        let mut mny: [f64; 4] = [0.0; 4];
        let mut mxy: [f64; 4] = [0.0; 4];
        // SAFETY: Stack arrays, aligned pointers.
        unsafe {
            _mm256_storeu_pd(mnx.as_mut_ptr(), min_xv);
            _mm256_storeu_pd(mxx.as_mut_ptr(), max_xv);
            _mm256_storeu_pd(mny.as_mut_ptr(), min_yv);
            _mm256_storeu_pd(mxy.as_mut_ptr(), max_yv);
        }
        let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) =
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for j in 0..4 {
            gmin_x = gmin_x.min(mnx[j]);
            gmax_x = gmax_x.max(mxx[j]);
            gmin_y = gmin_y.min(mny[j]);
            gmax_y = gmax_y.max(mxy[j]);
        }
        // Remaining coordinates
        for c in &coords[i..] {
            gmin_x = gmin_x.min(c.x);
            gmax_x = gmax_x.max(c.x);
            gmin_y = gmin_y.min(c.y);
            gmax_y = gmax_y.max(c.y);
        }
        (gmin_x, gmax_x, gmin_y, gmax_y)
    }
}

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

// ============================================================================
// Tests (platform-independent)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ring_ccw_simd_ccw() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_cw() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(!is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_collinear() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(!is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_fewer_than_3() {
        assert!(is_ring_ccw_simd(&[]));
        assert!(is_ring_ccw_simd(&[Coord { x: 0.0, y: 0.0 }]));
        assert!(is_ring_ccw_simd(&[
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]));
    }

    #[test]
    fn test_is_ring_ccw_simd_empty() {
        let coords: Vec<Coord<f64>> = Vec::new();
        assert!(is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_single() {
        let coords = vec![Coord { x: 0.0, y: 0.0 }];
        assert!(is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_large_ring() {
        let mut coords = Vec::new();
        for i in 0..10 {
            let angle = 2.0 * std::f64::consts::PI * i as f64 / 10.0;
            coords.push(Coord {
                x: angle.cos(),
                y: angle.sin(),
            });
        }
        coords.push(coords[0]);
        assert!(is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_large_ring_cw() {
        let mut coords = Vec::new();
        for i in 0..10 {
            let angle = -2.0 * std::f64::consts::PI * i as f64 / 10.0;
            coords.push(Coord {
                x: angle.cos(),
                y: angle.sin(),
            });
        }
        coords.push(coords[0]);
        assert!(!is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_point_in_ring_exclusive_square() {
        let pt = Coord { x: 5.0, y: 5.0 };
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(point_in_ring_exclusive(pt, &ring));
    }

    #[test]
    fn test_point_in_ring_exclusive_outside() {
        let pt = Coord { x: 15.0, y: 5.0 };
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(!point_in_ring_exclusive(pt, &ring));
    }

    #[test]
    fn test_point_in_ring_exclusive_on_vertex() {
        let pt = Coord { x: 0.0, y: 0.0 };
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(!point_in_ring_exclusive(pt, &ring));
    }

    #[test]
    fn test_orient2d_batch_4_consistency() {
        let pa = [
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 1.0 },
        ];
        let pb = [
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 1.0, y: 1.0 },
        ];
        let pc = [
            Coord { x: 0.5, y: 1.0 },
            Coord { x: 2.0, y: 1.0 },
            Coord { x: 3.0, y: 2.0 },
            Coord { x: 2.0, y: 0.0 },
        ];
        let batch = orient2d_batch_4(&pa, &pb, &pc);
        assert_eq!(batch.len(), 4);
        let expected: [f64; 4] = [
            (pb[0].x - pa[0].x) * (pc[0].y - pa[0].y) - (pb[0].y - pa[0].y) * (pc[0].x - pa[0].x),
            (pb[1].x - pa[1].x) * (pc[1].y - pa[1].y) - (pb[1].y - pa[1].y) * (pc[1].x - pa[1].x),
            (pb[2].x - pa[2].x) * (pc[2].y - pa[2].y) - (pb[2].y - pa[2].y) * (pc[2].x - pa[2].x),
            (pb[3].x - pa[3].x) * (pc[3].y - pa[3].y) - (pb[3].y - pa[3].y) * (pc[3].x - pa[3].x),
        ];
        for i in 0..4 {
            assert!((batch[i] - expected[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn test_orient2d_batch_4_robust_collinear() {
        let pa = [Coord { x: 0.0, y: 0.0 }; 4];
        let pb = [
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        ];
        let pc = [
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 2.0 },
        ];
        let batch = orient2d_batch_4_robust(&pa, &pb, &pc);
        for (i, &val) in batch.iter().enumerate() {
            assert_eq!(val, 0.0, "collinear triplet {i} should be exactly zero");
        }
    }

    #[test]
    fn test_orient2d_batch_4_robust_near_collinear() {
        let pa = [Coord { x: 0.0, y: 0.0 }; 4];
        let pb = [Coord { x: 1e10, y: 1e10 }; 4];
        let pc = [
            Coord {
                x: 2e10,
                y: 2e10 + 1e-6,
            },
            Coord {
                x: 2e10 + 1e-6,
                y: 2e10,
            },
            Coord {
                x: 2e10,
                y: 2e10 - 1e-6,
            },
            Coord {
                x: 2e10 - 1e-6,
                y: 2e10,
            },
        ];
        let batch = orient2d_batch_4_robust(&pa, &pb, &pc);
        for i in 0..4 {
            let expected = crate::orient::orient2d(pa[i], pb[i], pc[i]);
            assert_eq!(
                batch[i].signum(),
                expected.signum(),
                "triplet {i}: robust sign mismatch"
            );
            assert!(
                batch[i] == expected,
                "triplet {i}: robust value mismatch (batch={}, expected={})",
                batch[i],
                expected
            );
        }
    }

    #[test]
    fn test_orient2d_batch_4_robust_matches_individual() {
        let test_cases: [(f64, f64); 10] = [
            (1e-10, 0.0),
            (1.0, 1.0),
            (1e5, 1e5),
            (1e10, 1e10),
            (1e15, 1e15),
            (1e-5, 1e5),
            (1e10, 1e-10),
            (-1e8, 1e8),
            (0.0, 1e-8),
            (1e8, 1e8),
        ];
        for (dx, dy) in &test_cases {
            let pa = [Coord { x: 0.0, y: 0.0 }; 4];
            let pb = [
                Coord { x: *dx, y: *dy },
                Coord {
                    x: *dx + 1e-10,
                    y: *dy,
                },
                Coord {
                    x: *dx,
                    y: *dy + 1e-10,
                },
                Coord {
                    x: *dx + 1e-10,
                    y: *dy + 1e-10,
                },
            ];
            let pc = [
                Coord {
                    x: *dx * 0.5,
                    y: *dy * 2.0,
                },
                Coord {
                    x: *dx * 2.0,
                    y: *dy * 0.5,
                },
                Coord {
                    x: *dx * 1.5,
                    y: *dy * 1.5,
                },
                Coord {
                    x: *dx * 0.1,
                    y: *dy * 0.1,
                },
            ];
            let batch = orient2d_batch_4_robust(&pa, &pb, &pc);
            for i in 0..4 {
                let expected = crate::orient::orient2d(pa[i], pb[i], pc[i]);
                assert_eq!(
                    batch[i].signum(),
                    expected.signum(),
                    "sign mismatch at case ({},{}) triplet {}",
                    dx,
                    dy,
                    i
                );
            }
        }
    }

    #[test]
    fn test_point_in_ring_robust_near_boundary() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1e6, y: 0.0 },
            Coord { x: 1e6, y: 1e6 },
            Coord { x: 0.0, y: 1e6 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let pt_inside = Coord {
            x: 5e5,
            y: 5e5 + 1e-10,
        };
        assert!(
            point_in_ring_exclusive(pt_inside, &ring),
            "point should be inside"
        );
        let pt_outside = Coord { x: -1.0, y: 5e5 };
        assert!(
            !point_in_ring_exclusive(pt_outside, &ring),
            "point should be outside"
        );
    }
}

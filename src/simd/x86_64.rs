//! x86_64 AVX2 intrinsics (4-wide and 8-wide predicates).

use super::*;


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
    point_in_ring_exclusive_test(pt, coords)
}

/// Test wrapper (x86_64 AVX2/AVX-512 path).
pub fn point_in_ring_exclusive_test(pt: Coord<f64>, coords: &[Coord<f64>]) -> bool {
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

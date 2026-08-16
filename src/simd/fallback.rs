//! Scalar fallback (all stable targets).
//!
//! The x86_64 module was removed after head-to-head measurement showed the
//! hand-written AVX2 kernels lose to LLVM's auto-vectorized scalar loops
//! (see `mod.rs` header). This module serves every stable target; the
//! nightly `simd-portable` path lives in `portable.rs`.

use super::*;

#[cfg(not(feature = "simd-portable"))]
pub(crate) fn orient2d_batch_4(
    pa: &[Coord<f64>; 4],
    pb: &[Coord<f64>; 4],
    pc: &[Coord<f64>; 4],
) -> [f64; 4] {
    // SIMD verdict (measured head-to-head, simd/tests.rs microbench_kernels,
    // 2026-08-16): a shuffle-transpose AVX2 kernel (unpack-based, no gathers)
    // is ~1.17x faster raw, but the runtime dispatch + non-inlinable
    // target_feature boundary costs ~0.8ns/call on a ~3ns kernel, netting a
    // 0.90x LOSS. For a 4-det kernel the auto-vectorized scalar path wins
    // because it inlines into the pair predicates. The 2026-08-02 verdict
    // stands for this kernel; only bulk scans (aabb, snap) justify dispatch.
    scalar_orient2d_batch(pa, pb, pc)
}

#[cfg(not(feature = "simd-portable"))]
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
    is_ring_ccw_scalar(coords)
}

#[cfg(not(feature = "simd-portable"))]
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
pub(crate) fn snap_coords_simd(coords: &mut [Coord<f64>], scale: f64) {
    for c in coords.iter_mut() {
        c.x = (c.x / scale).round() * scale;
        c.y = (c.y / scale).round() * scale;
    }
}

#[cfg(not(feature = "simd-portable"))]
#[inline]
pub fn aabb_minmax_simd(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    let n = coords.len();
    if n == 0 {
        return (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    }
    // Runtime-dispatched AVX2 kernel. The head-to-head microbench (simd
    // audit 2026-08-02, commit fe4c831) measured the hand-written AVX2
    // bbox scan 4.5x faster than the scalar loop — the ONE kernel where
    // intrinsics beat LLVM auto-vectorization (point_in_ring 8.4x slower,
    // is_ring_ccw 2.8x — those stay scalar). Dispatch is runtime
    // (is_x86_feature_detected) so the binary runs on any x86_64; the
    // old compile-time cfg never activated because builds don't set
    // target features.
    #[cfg(all(
        not(feature = "simd-portable"),
        target_arch = "x86_64",
        feature = "std"
    ))]
    {
        if std::arch::is_x86_feature_detected!("avx") {
            // SAFETY: dispatch verified the avx feature.
            return unsafe { aabb_minmax_avx(coords) };
        }
    }
    let (mut mnx, mut mxx, mut mny, mut mxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in coords {
        mnx = mnx.min(c.x);
        mxx = mxx.max(c.x);
        mny = mny.min(c.y);
        mxy = mxy.max(c.y);
    }
    (mnx, mxx, mny, mxy)
}

/// AVX2 min/max reduction over a coordinate slice. Shuffle-transpose
/// variant (unpacklo/unpackhi, no gathers): 2 loads + 2 unpacks per
/// 4 coords. Lane order is irrelevant for min/max so no permute is needed.
/// Restored from the pre-fe4c831 kernel and reworked from setr_pd gathers
/// (measured 4.5x vs scalar on the bbox scan; the shuffle form measures
/// faster still - see simd/tests.rs microbench_kernels).
#[cfg(all(not(feature = "simd-portable"), target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub(crate) unsafe fn aabb_minmax_avx(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    use core::arch::x86_64::*;
    let n = coords.len();
    let mut min_xv = _mm256_set1_pd(f64::MAX);
    let mut max_xv = _mm256_set1_pd(f64::MIN);
    let mut min_yv = _mm256_set1_pd(f64::MAX);
    let mut max_yv = _mm256_set1_pd(f64::MIN);
    let mut i = 0usize;
    while i + 4 <= n {
        // SAFETY: in-bounds wide loads; AoS -> SoA via lane-crossing unpacks.
        let v0 = unsafe { _mm256_loadu_pd(coords.as_ptr().add(i) as *const f64) };
        let v1 = unsafe { _mm256_loadu_pd(coords.as_ptr().add(i + 2) as *const f64) };
        let xs = _mm256_unpacklo_pd(v0, v1);
        let ys = _mm256_unpackhi_pd(v0, v1);
        min_xv = _mm256_min_pd(min_xv, xs);
        max_xv = _mm256_max_pd(max_xv, xs);
        min_yv = _mm256_min_pd(min_yv, ys);
        max_yv = _mm256_max_pd(max_yv, ys);
        i += 4;
    }
    let mut mnx: [f64; 4] = [0.0; 4];
    let mut mxx: [f64; 4] = [0.0; 4];
    let mut mny: [f64; 4] = [0.0; 4];
    let mut mxy: [f64; 4] = [0.0; 4];
    // SAFETY: stack arrays, aligned pointers.
    unsafe {
        _mm256_storeu_pd(mnx.as_mut_ptr(), min_xv);
        _mm256_storeu_pd(mxx.as_mut_ptr(), max_xv);
        _mm256_storeu_pd(mny.as_mut_ptr(), min_yv);
        _mm256_storeu_pd(mxy.as_mut_ptr(), max_yv);
    }
    let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for j in 0..4 {
        gmin_x = gmin_x.min(mnx[j]);
        gmax_x = gmax_x.max(mxx[j]);
        gmin_y = gmin_y.min(mny[j]);
        gmax_y = gmax_y.max(mxy[j]);
    }
    for c in &coords[i..] {
        gmin_x = gmin_x.min(c.x);
        gmax_x = gmax_x.max(c.x);
        gmin_y = gmin_y.min(c.y);
        gmax_y = gmax_y.max(c.y);
    }
    (gmin_x, gmax_x, gmin_y, gmax_y)
}

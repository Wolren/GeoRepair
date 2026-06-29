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

use geo::{Coord, GeoFloat};

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
        i += 3;
    }
    for j in i..n {
        let next = (j + 1) % n;
        area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
    }
    area > 0.0
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
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
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
        let origin = Coord { x: 0.0, y: 0.0 };
        let mut i = 0usize;
        let mut area = 0.0f64;
        while i + 4 <= n {
            let pa = [origin; 4];
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
            let batch = unsafe {
                let pax = _mm256_setzero_pd();
                let pay = _mm256_setzero_pd();
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
            i += 3;
        }
        for j in i..n {
            let next = (j + 1) % n;
            area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
        }
        area > 0.0
    }
}

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

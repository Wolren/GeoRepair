//! SIMD-accelerated orientation predicates.
//!
//! Provides packed implementations of orient2d (cross product of three coords)
//! using 256-bit SIMD registers for 4× throughput on applicable loops.

use geo::{Coord, GeoFloat};

/// Compute orient2d for four pairs of coordinates in parallel.
/// Returns [sign(pa0, pb0, pc0), sign(pa1, pb1, pc1), ...].
#[cfg(target_arch = "x86_64")]
pub(crate) fn orient2d_batch_4<T: GeoFloat>(
    pa: &[Coord<T>; 4],
    pb: &[Coord<T>; 4],
    pc: &[Coord<T>; 4],
) -> [T; 4] {
    // Scalar fallback until simd feature is stabilized for f64
    use std::arch::x86_64::*;

    #[cfg(target_feature = "avx")]
    unsafe {
        // Load coordinates
        let pbx = _mm256_set_pd(pb[0].x, pb[1].x, pb[2].x, pb[3].x);
        let pax = _mm256_set_pd(pa[0].x, pa[1].x, pa[2].x, pa[3].x);
        let pcy = _mm256_set_pd(pc[0].y, pc[1].y, pc[2].y, pc[3].y);
        let pay = _mm256_set_pd(pa[0].y, pa[1].y, pa[2].y, pa[3].y);
        let pby = _mm256_set_pd(pb[0].y, pb[1].y, pb[2].y, pb[3].y);
        let pcx = _mm256_set_pd(pc[0].x, pc[1].x, pc[2].x, pc[3].x);

        // (pb.x - pa.x) * (pc.y - pa.y)
        let dx = _mm256_sub_pd(pbx, pax);
        let dy = _mm256_sub_pd(pcy, pay);
        let term1 = _mm256_mul_pd(dx, dy);

        // (pb.y - pa.y) * (pc.x - pa.x)
        let dy2 = _mm256_sub_pd(pby, pay);
        let dx2 = _mm256_sub_pd(pcx, pax);
        let term2 = _mm256_mul_pd(dy2, dx2);

        // result = term1 - term2
        let result = _mm256_sub_pd(term1, term2);

        let mut out = [T::zero(); 4];
        _mm256_storeu_pd(&mut out as *mut _ as *mut f64, result);
        return out;
    }
    #[cfg(not(target_feature = "avx"))]
    {
        scalar_orient2d_batch(pa, pb, pc)
    }
}

/// Scalar fallback for orient2d batch.
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

#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn orient2d_batch_4<T: GeoFloat>(
    pa: &[Coord<T>; 4],
    pb: &[Coord<T>; 4],
    pc: &[Coord<T>; 4],
) -> [T; 4] {
    scalar_orient2d_batch(pa, pb, pc)
}

/// Check ring winding direction using SIMD where available.
/// Uses f64 specialization since SIMD intrinsics are f64-only.
#[cfg(target_arch = "x86_64")]
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
    use std::arch::x86_64::*;
    let n = coords.len();
    if n < 3 {
        return true;
    }
    let mut area = 0.0f64;
    let origin = Coord { x: 0.0, y: 0.0 };

    #[cfg(target_feature = "avx")]
    {
        let mut i = 0usize;
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
                // (pb.x - 0) * (pc.y - 0) - (pb.y - 0) * (pc.x - 0)
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
        // Scalar remainder
        let start = i;
        for j in start..n {
            let next = (j + 1) % n;
            area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
        }
    }

    #[cfg(not(target_feature = "avx"))]
    {
        for j in 0..n {
            let next = (j + 1) % n;
            area += coords[j].x * coords[next].y - coords[next].x * coords[j].y;
        }
    }

    area > 0.0
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn is_ring_ccw_simd(coords: &[Coord<f64>]) -> bool {
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

#[cfg(test)]
#[cfg(target_arch = "x86_64")]
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
        // Collinear → area = 0 → not CCW
        assert!(!is_ring_ccw_simd(&coords));
    }

    #[test]
    fn test_is_ring_ccw_simd_fewer_than_3() {
        let coords = vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }];
        assert!(is_ring_ccw_simd(&coords));
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
        // 10 points in CCW order
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
        // 10 points in CW order
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
}

#[cfg(test)]
#[cfg(not(target_arch = "x86_64"))]
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
    fn test_is_ring_ccw_simd_fewer_than_3() {
        assert!(is_ring_ccw_simd(&[]));
        assert!(is_ring_ccw_simd(&[Coord { x: 0.0, y: 0.0 }]));
        assert!(is_ring_ccw_simd(&[
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]));
    }

    #[test]
    fn test_orient2d_batch_4_fallback() {
        let pa = [Coord { x: 0.0, y: 0.0 }; 4];
        let pb = [Coord { x: 1.0, y: 0.0 }; 4];
        let pc = [Coord { x: 0.5, y: 1.0 }; 4];
        let batch = orient2d_batch_4(&pa, &pb, &pc);
        assert_eq!(batch.len(), 4);
        for i in 0..4 {
            assert!(batch[i] > 0.0);
        }
    }
}

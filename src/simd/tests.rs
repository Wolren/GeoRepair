//! SIMD predicate test battery.

#[cfg(test)]
use super::*;
use alloc::vec::Vec;

// ============================================================================
// Tests (platform-independent)
// ============================================================================

#[cfg(test)]
mod platform_tests {
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

// ============================================================================
// Shuffle-transpose kernel verification + head-to-head microbench
// ============================================================================

#[cfg(all(
    not(feature = "simd-portable"),
    target_arch = "x86_64",
    feature = "std"
))]
mod shuffle_kernels {
    use super::*;
    use alloc::vec::Vec;
    use std::hint::black_box;
    use std::time::Instant;

    fn pseudo_coords(n: usize) -> Vec<Coord<f64>> {
        (0..n)
            .map(|i| Coord {
                x: ((i * 7919) % 4093) as f64 * 0.05 - 100.0,
                y: ((i * 104729) % 3571) as f64 * 0.07 + 40.0,
            })
            .collect()
    }

    #[test]
    fn aabb_avx_bit_exact_vs_scalar() {
        if !std::arch::is_x86_feature_detected!("avx") {
            return;
        }
        let coords = pseudo_coords(10_000);
        let scalar = {
            let (mut mnx, mut mxx, mut mny, mut mxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for c in &coords {
                mnx = mnx.min(c.x);
                mxx = mxx.max(c.x);
                mny = mny.min(c.y);
                mxy = mxy.max(c.y);
            }
            (mnx, mxx, mny, mxy)
        };
        let simd = unsafe { super::super::fallback::aabb_minmax_avx(&coords) };
        assert_eq!(simd.0.to_bits(), scalar.0.to_bits());
        assert_eq!(simd.1.to_bits(), scalar.1.to_bits());
        assert_eq!(simd.2.to_bits(), scalar.2.to_bits());
        assert_eq!(simd.3.to_bits(), scalar.3.to_bits());
    }

    /// Informational head-to-head timings, no assertions (CI-safe).
    /// Run with: cargo test --release --lib simd:: -- --nocapture
    ///
    /// Verdicts (2026-08-16, i5-12400F, rustc 1.97):
    /// - aabb shuffle-transpose AVX kernel: ~4x vs scalar (wired into
    ///   aabb_minmax_simd, used by the repair paths).
    /// - orient2d batch-of-4: raw shuffle AVX2 kernel ~1.17x faster,
    ///   but runtime dispatch + non-inlinable target_feature boundary
    ///   cost ~0.8ns/call on a ~3ns kernel -> net 0.90x LOSS. Kept
    ///   scalar (inlinable into the pair predicates).
    #[test]
    fn microbench_kernels() {
        let coords = pseudo_coords(100_000);
        let mut rng_state = 0x243f6a8885a308d3u64;
        let mut next = || {
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 7;
            rng_state ^= rng_state << 17;
            ((rng_state >> 11) as f64 / (1u64 << 53) as f64) * 2000.0 - 1000.0
        };
        // aabb: scalar vs shuffle-AVX
        let t = Instant::now();
        let mut acc = 0.0f64;
        for _ in 0..200 {
            let (a, b, c, d) = black_box(aabb_minmax_simd(&coords));
            acc += a + b + c + d;
        }
        let disp_us = t.elapsed().as_micros() as f64 / 200.0;
        let t = Instant::now();
        let mut acc2 = 0.0f64;
        for _ in 0..200 {
            let (mut mnx, mut mxx, mut mny, mut mxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for c in &coords {
                mnx = mnx.min(c.x);
                mxx = mxx.max(c.x);
                mny = mny.min(c.y);
                mxy = mxy.max(c.y);
            }
            acc2 += mnx + mxx + mny + mxy;
        }
        let scalar_us = t.elapsed().as_micros() as f64 / 200.0;
        println!(
            "aabb 100k coords: scalar {scalar_us:.2} us, avx-dispatch {disp_us:.2} us ({:.2}x)",
            scalar_us / disp_us
        );
        black_box((acc, acc2));

        // orient2d batch: scalar vs avx2-dispatch. Inputs rotate through a
        // table (call sites build fresh arrays per pair - loop-invariant
        // inputs would let LLVM collapse the scalar side of the bench).
        type Triad = ([Coord<f64>; 4], [Coord<f64>; 4], [Coord<f64>; 4]);
        let mut table: Vec<Triad> = Vec::new();
        for _ in 0..64 {
            let pa = [
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
            ];
            let pb = [
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
            ];
            let pc = [
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
                Coord {
                    x: next(),
                    y: next(),
                },
            ];
            table.push((pa, pb, pc));
        }
        let t = Instant::now();
        let mut acc = 0.0f64;
        for i in 0..2_000_000 {
            let (pa, pb, pc) = &table[i & 63];
            let o = black_box(orient2d_batch_4(pa, pb, pc));
            acc += o[0] + o[1] + o[2] + o[3];
        }
        let disp_us = t.elapsed().as_micros() as f64;
        let t = Instant::now();
        let mut acc2 = 0.0f64;
        for i in 0..2_000_000 {
            let (pa, pb, pc) = &table[i & 63];
            let o = black_box(scalar_orient2d_batch(pa, pb, pc));
            acc2 += o[0] + o[1] + o[2] + o[3];
        }
        let scalar_us = t.elapsed().as_micros() as f64;
        println!(
            "orient2d x4: scalar {scalar_us:.0} us/2M, avx2-dispatch {disp_us:.0} us/2M ({:.2}x)",
            scalar_us / disp_us
        );
        black_box((acc, acc2));
    }
}

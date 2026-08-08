//! SIMD predicate test battery.

#[cfg(test)]
use super::*;
use alloc::vec::Vec;

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

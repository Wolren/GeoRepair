//! Shared utility functions used across the crate.

use geo::Coord;

pub(crate) fn shoelace_sum(ring: &[Coord<f64>]) -> f64 {
    let mut sum = 0.0;
    for window in ring.windows(2) {
        sum += window[0].x * window[1].y - window[1].x * window[0].y;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shoelace_sum_ccw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let sum = shoelace_sum(&ring);
        assert!(sum > 0.0);
        assert!((sum - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_shoelace_sum_cw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let sum = shoelace_sum(&ring);
        assert!(sum < 0.0);
        assert!((sum + 2.0).abs() < 1e-12);
    }
}

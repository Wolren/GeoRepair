//! Shared utility functions used across the crate.

use geo::{Coord, CoordNum};

#[allow(dead_code)]
pub(crate) fn remove_consecutive_duplicates<T: CoordNum>(coords: &[Coord<T>]) -> Vec<Coord<T>> {
    let mut result = Vec::with_capacity(coords.len());
    for c in coords {
        if result.last() != Some(c) {
            result.push(*c);
        }
    }
    result
}

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
    fn test_remove_consecutive_duplicates_none() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
        ];
        assert_eq!(remove_consecutive_duplicates(&coords), coords);
    }

    #[test]
    fn test_remove_consecutive_duplicates_some() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
        ];
        let expected = vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }];
        assert_eq!(remove_consecutive_duplicates(&coords), expected);
    }

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

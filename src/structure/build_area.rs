//! Even-odd area of a noded edge set.

use alloc::vec::Vec;
use geo::{Line, MultiPolygon};

use crate::structure::polygonizer::even_odd_area;

/// Even-odd interior of the edge set; `None` only when a coordinate is not finite.
pub fn build_area(lines: &[Line<f64>]) -> Option<MultiPolygon<f64>> {
    even_odd_area(lines).map(|(area, _)| area)
}

/// The same area plus its boundary segments, once per boundary ring in build order.
pub(crate) fn build_area_with_boundary(
    lines: &[Line<f64>],
) -> Option<(MultiPolygon<f64>, Vec<Line<f64>>)> {
    even_odd_area(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Area, Coord, LineString, MultiPolygon};
    use rustc_hash::FxHashSet;

    fn square_at(x: f64, y: f64) -> Vec<Line<f64>> {
        [
            (x, y),
            (x + 10.0, y),
            (x + 10.0, y + 10.0),
            (x, y + 10.0),
            (x, y),
        ]
        .windows(2)
        .map(|w| {
            Line::new(
                Coord {
                    x: w[0].0,
                    y: w[0].1,
                },
                Coord {
                    x: w[1].0,
                    y: w[1].1,
                },
            )
        })
        .collect()
    }

    fn square() -> Vec<Line<f64>> {
        square_at(0.0, 0.0)
    }

    fn ring_is_simple(ring: &LineString<f64>) -> bool {
        let coords = &ring.0;
        let n = if coords.len() > 1 && coords[0] == coords[coords.len() - 1] {
            coords.len() - 1
        } else {
            coords.len()
        };
        let mut seen: FxHashSet<(u64, u64)> = FxHashSet::default();
        coords[..n]
            .iter()
            .all(|c| seen.insert((c.x.to_bits(), c.y.to_bits())))
    }

    fn total_area(area: &MultiPolygon<f64>) -> f64 {
        area.0.iter().map(Area::unsigned_area).sum()
    }

    #[test]
    fn empty_input_is_empty_not_none() {
        let area = build_area(&[]).expect("empty input is a valid call");
        assert!(area.0.is_empty());
        assert_eq!(area, MultiPolygon::new(Vec::new()));
    }

    #[test]
    fn boundary_report_lists_each_square_side_once() {
        let (area, boundary) = build_area_with_boundary(&square()).expect("square builds");
        assert_eq!(area.0.len(), 1);
        assert_eq!(boundary.len(), 4);
    }

    #[test]
    fn two_squares_touching_at_a_corner_stay_two_simple_parts() {
        let mut lines = square();
        lines.extend(square_at(10.0, 10.0));
        let area = build_area(&lines).expect("two squares build");
        assert_eq!(area.0.len(), 2);
        assert!((total_area(&area) - 200.0).abs() < 1e-9);
        for part in &area.0 {
            assert!(ring_is_simple(part.exterior()));
            assert_eq!(part.interiors().len(), 0);
        }
    }

    #[test]
    fn hole_touching_the_shell_at_a_corner_still_subtracts() {
        let mut hole: Vec<Line<f64>> = [(0.0, 0.0), (4.0, 1.0), (1.0, 4.0), (0.0, 0.0)]
            .windows(2)
            .map(|w| {
                Line::new(
                    Coord {
                        x: w[0].0,
                        y: w[0].1,
                    },
                    Coord {
                        x: w[1].0,
                        y: w[1].1,
                    },
                )
            })
            .collect();
        hole.extend(square());
        let area = build_area(&hole).expect("pinched ring builds");
        assert_eq!(area.0.len(), 1);
        assert_eq!(area.0[0].interiors().len(), 1);
        assert!((total_area(&area) - 92.5).abs() < 1e-9);
        assert!(ring_is_simple(area.0[0].exterior()));
        assert!(ring_is_simple(&area.0[0].interiors()[0]));
    }
}

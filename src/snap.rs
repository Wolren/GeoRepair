//! Coordinate snap-rounding utilities.
//!
//! Eliminates sliver geometries and near-degenerate artifacts by snapping
//! coordinates to a fixed-precision grid.

use geo::{Coord, Line};

const DEFAULT_GRID: f64 = 1e-10;

/// Snap a coordinate to a grid of the given resolution.
#[inline]
pub fn snap_coord(c: Coord<f64>, resolution: f64) -> Coord<f64> {
    Coord {
        x: (c.x / resolution).round() * resolution,
        y: (c.y / resolution).round() * resolution,
    }
}

/// Snap a coordinate to the default grid.
#[inline]
pub fn snap_coord_default(c: Coord<f64>) -> Coord<f64> {
    snap_coord(c, DEFAULT_GRID)
}

/// Snap a line segment's endpoints to the default grid.
/// Filters out zero-length segments after snapping.
pub fn snap_line(line: Line<f64>) -> Option<Line<f64>> {
    let start = snap_coord_default(line.start);
    let end = snap_coord_default(line.end);
    if start == end {
        return None;
    }
    Some(Line::new(start, end))
}

/// Snap all line segments in a vector, filtering out degenerate ones.
pub fn snap_lines(lines: Vec<Line<f64>>) -> Vec<Line<f64>> {
    lines.into_iter().filter_map(snap_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snap_to_grid() {
        let c = Coord {
            x: 1.2345678912,
            y: 5.00000000000004,
        };
        let snapped = snap_coord_default(c);
        assert!((snapped.x - 1.2345678912).abs() < 1e-9);
        assert!((snapped.y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_snap_filters_short_segments() {
        let line = Line::new(
            Coord {
                x: 1.0 + 1e-20,
                y: 2.0,
            },
            Coord { x: 1.0, y: 2.0 },
        );
        assert!(snap_line(line).is_none());
    }

    #[test]
    fn test_snap_coord_custom_resolution() {
        let c = Coord {
            x: 1.2345678,
            y: 9.8765432,
        };
        let snapped = snap_coord(c, 0.01);
        assert!((snapped.x - 1.23).abs() < 1e-9);
        assert!((snapped.y - 9.88).abs() < 1e-9);
    }

    #[test]
    fn test_snap_coord_negative() {
        let c = Coord {
            x: -1.2345678912e-11,
            y: -5.00000000000004,
        };
        let snapped = snap_coord_default(c);
        assert!((snapped.x - 0.0).abs() < 1e-20);
        assert!((snapped.y - (-5.0)).abs() < 1e-9);
    }

    #[test]
    fn test_snap_coord_zero() {
        let c = Coord { x: 0.0, y: 0.0 };
        let snapped = snap_coord_default(c);
        assert_eq!(snapped.x, 0.0);
        assert_eq!(snapped.y, 0.0);
    }

    #[test]
    fn test_snap_line_valid() {
        let line = Line::new(
            Coord {
                x: 1.2345678912,
                y: 2.3456789123,
            },
            Coord {
                x: 3.4567891234,
                y: 4.5678912345,
            },
        );
        let snapped = snap_line(line);
        assert!(snapped.is_some());
        let snapped = snapped.unwrap();
        assert!((snapped.start.x - 1.2345678912).abs() < 1e-9);
        assert!((snapped.end.y - 4.5678912345).abs() < 1e-9);
        assert!(snapped.start != snapped.end);
    }

    #[test]
    fn test_snap_lines_empty() {
        let result = snap_lines(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_snap_lines_mixed() {
        let lines = vec![
            Line::new(
                Coord {
                    x: 1.0 + 1e-20,
                    y: 2.0,
                },
                Coord { x: 1.0, y: 2.0 },
            ),
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
        ];
        let result = snap_lines(lines);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, Coord { x: 0.0, y: 0.0 });
        assert_eq!(result[0].end, Coord { x: 1.0, y: 1.0 });
    }

    #[test]
    fn test_snap_large_coords() {
        let c = Coord {
            x: 1e15 + 0.5,
            y: -1e15 + 0.5,
        };
        let snapped = snap_coord_default(c);
        assert!((snapped.x - (1e15 + 0.5)).abs() < 1e-4);
        assert!((snapped.y - (-1e15 + 0.5)).abs() < 1e-4);
    }

    #[test]
    fn test_snap_line_zero_length_after_snap() {
        let line = Line::new(
            Coord {
                x: 1.0 + 1e-12,
                y: 2.0,
            },
            Coord { x: 1.0, y: 2.0 },
        );
        let snapped = snap_line(line);
        assert!(snapped.is_none());
    }

    #[test]
    fn test_snap_coord_nan() {
        let c = Coord {
            x: f64::NAN,
            y: 0.0,
        };
        let snapped = snap_coord_default(c);
        assert!(snapped.x.is_nan());
        assert_eq!(snapped.y, 0.0);
    }
}

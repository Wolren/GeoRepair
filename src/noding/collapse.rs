//! Handle collapsed geometries (polygon → line → point).
//!
//! When keep_collapsed is true, degenerate higher-dimensional geometries
//! are preserved as lower-dimensional types instead of becoming empty.

use geo::{CoordNum, GeoFloat, Geometry, LineString, Point};

/// If a geometry has collapsed to a lower dimension, return the
/// appropriate representation when keep_collapsed is true.
pub(crate) fn handle_collapse<T: GeoFloat>(
    input_dim: u8,
    coords: &[Coord<T>],
    keep_collapsed: bool,
) -> Option<Geometry<T>> {
    if !keep_collapsed {
        return None;
    }
    match coords.len() {
        0 => None,
        1 => Some(Geometry::Point(Point(coords[0]))),
        _ => {
            if input_dim == 2 {
                Some(Geometry::LineString(LineString::new(coords.to_vec())))
            } else {
                None
            }
        }
    }
}

use geo::Coord;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_collapse_not_kept() {
        let result = handle_collapse::<f64>(2, &[Coord { x: 0.0, y: 0.0 }], false);
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_collapse_zero_coords() {
        let result = handle_collapse::<f64>(2, &[], true);
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_collapse_single_coord() {
        let result = handle_collapse::<f64>(2, &[Coord { x: 1.0, y: 2.0 }], true);
        assert!(matches!(result, Some(Geometry::Point(_))));
        if let Some(Geometry::Point(p)) = result {
            assert_eq!(p, Point::new(1.0, 2.0));
        }
    }

    #[test]
    fn test_handle_collapse_two_coords_dim2() {
        let result = handle_collapse::<f64>(
            2,
            &[Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }],
            true,
        );
        assert!(matches!(result, Some(Geometry::LineString(_))));
    }

    #[test]
    fn test_handle_collapse_two_coords_dim1() {
        let result = handle_collapse::<f64>(
            1,
            &[Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }],
            true,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_collapse_many_coords_dim2() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        ];
        let result = handle_collapse::<f64>(2, &coords, true);
        assert!(matches!(result, Some(Geometry::LineString(_))));
    }
}

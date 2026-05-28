use geo::{Area, BooleanOps, MultiPolygon, OpType, Polygon};

pub(crate) fn subtract_holes(shell: &Polygon<f64>, holes: &[Polygon<f64>]) -> Option<Polygon<f64>> {
    if holes.is_empty() {
        return Some(shell.clone());
    }
    if holes.len() == 1 {
        let diff = shell.boolean_op(&holes[0], OpType::Difference);
        return largest_polygon(diff);
    }

    // Batch: convert to MultiPolygon, single boolean difference
    // This processes all holes in one overlay operation (GEOS-style)
    let shell_mp = MultiPolygon::new(vec![shell.clone()]);
    let holes_mp = MultiPolygon::new(holes.to_vec());
    let diff = shell_mp.boolean_op(&holes_mp, OpType::Difference);
    if diff.0.is_empty() {
        return None;
    }
    largest_polygon(diff)
}

fn largest_polygon(mp: MultiPolygon<f64>) -> Option<Polygon<f64>> {
    mp.0.into_iter().max_by(|a, b| {
        a.unsigned_area()
            .partial_cmp(&b.unsigned_area())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, LineString};

    fn make_shell() -> Polygon<f64> {
        Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        )
    }

    fn make_hole() -> Polygon<f64> {
        Polygon::new(
            LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ]),
            Vec::new(),
        )
    }

    #[test]
    fn test_subtract_holes_empty() {
        let shell = make_shell();
        let result = subtract_holes(&shell, &[]);
        assert!(result.is_some());
    }

    #[test]
    fn test_subtract_holes_single() {
        let shell = make_shell();
        let hole = make_hole();
        let result = subtract_holes(&shell, &[hole]);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.interiors().len(), 1);
    }

    #[test]
    fn test_subtract_holes_multiple() {
        let shell = make_shell();
        let hole1 = make_hole();
        let hole2 = Polygon::new(
            LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ]),
            Vec::new(),
        );
        let result = subtract_holes(&shell, &[hole1, hole2]);
        assert!(result.is_some());
    }

    #[test]
    fn test_subtract_holes_hole_equals_shell() {
        let shell = make_shell();
        let hole = make_shell();
        let result = subtract_holes(&shell, &[hole]);
        assert!(result.is_none());
    }

    #[test]
    fn test_subtract_holes_hole_outside_shell() {
        let shell = make_shell();
        let hole = Polygon::new(
            LineString::new(vec![
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 25.0, y: 20.0 },
                Coord { x: 25.0, y: 25.0 },
                Coord { x: 20.0, y: 25.0 },
                Coord { x: 20.0, y: 20.0 },
            ]),
            Vec::new(),
        );
        let result = subtract_holes(&shell, &[hole]);
        assert!(result.is_some());
    }
}

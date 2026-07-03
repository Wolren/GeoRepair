use geo::{Area, BooleanOps, MultiPolygon, OpType, Polygon};

pub(crate) fn subtract_holes(shell: &Polygon<f64>, holes: &[Polygon<f64>]) -> MultiPolygon<f64> {
    if holes.is_empty() {
        return MultiPolygon::new(vec![shell.clone()]);
    }

    // Boolean difference preserves ALL result components — a hole that
    // touches the shell in 2+ places splits the shell into multiple
    // polygons (the classic "hourglass" hole pattern). Dropping components
    // via largest_polygon would lose valid geometry.
    let result = if holes.len() == 1 {
        shell.boolean_op(&holes[0], OpType::Difference)
    } else {
        let shell_mp = MultiPolygon::new(vec![shell.clone()]);
        let holes_mp = MultiPolygon::new(holes.to_vec());
        shell_mp.boolean_op(&holes_mp, OpType::Difference)
    };

    // Filter out zero-area artifacts from floating-point imprecision
    let eps = 1e-15;
    let valid: Vec<Polygon<f64>> = result
        .0
        .into_iter()
        .filter(|p| p.unsigned_area() > eps)
        .collect();
    MultiPolygon::new(valid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Area, Coord, LineString};

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
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_subtract_holes_single() {
        let shell = make_shell();
        let hole = make_hole();
        let result = subtract_holes(&shell, &[hole]);
        assert!(!result.0.is_empty());
        assert_eq!(result.0[0].interiors().len(), 1);
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
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_subtract_holes_hole_equals_shell() {
        let shell = make_shell();
        let hole = make_shell();
        let result = subtract_holes(&shell, &[hole]);
        assert!(result.0.is_empty());
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
        assert!(!result.0.is_empty());
    }
}

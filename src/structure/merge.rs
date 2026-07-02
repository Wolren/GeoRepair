use geo::{MultiPolygon, Polygon};

pub(crate) fn merge_shells(shells: Vec<Polygon<f64>>) -> MultiPolygon<f64> {
    if shells.len() <= 1 {
        return MultiPolygon::new(shells);
    }
    // Fast path: if all shell bboxes are disjoint, unary_union is a no-op
    if shells_are_disjoint(&shells) {
        return MultiPolygon::new(shells);
    }
    let mp = MultiPolygon::new(shells);
    geo::algorithm::bool_ops::unary_union(&mp)
}

fn shells_are_disjoint(shells: &[Polygon<f64>]) -> bool {
    let bboxes: Vec<_> = shells
        .iter()
        .map(|p| crate::simd::aabb_minmax_simd(&p.exterior().0))
        .collect();
    for i in 0..shells.len() {
        let (min_x, max_x, min_y, max_y) = bboxes[i];
        for &(m2x, m2x2, m2y, m2y2) in bboxes.iter().skip(i + 1) {
            if min_x <= m2x2 && max_x >= m2x && min_y <= m2y2 && max_y >= m2y {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, LineString};

    #[test]
    fn test_merge_shells_single() {
        let shell = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![shell.clone()]);
        assert_eq!(result.0.len(), 1);
    }

    #[test]
    fn test_merge_shells_disjoint() {
        let s1 = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let s2 = Polygon::new(
            LineString::new(vec![
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 15.0, y: 10.0 },
                Coord { x: 15.0, y: 15.0 },
                Coord { x: 10.0, y: 15.0 },
                Coord { x: 10.0, y: 10.0 },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![s1, s2]);
        assert_eq!(result.0.len(), 2);
    }

    #[test]
    fn test_merge_shells_overlapping() {
        let s1 = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let s2 = Polygon::new(
            LineString::new(vec![
                Coord { x: 3.0, y: 3.0 },
                Coord { x: 8.0, y: 3.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 3.0, y: 8.0 },
                Coord { x: 3.0, y: 3.0 },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![s1, s2]);
        assert_eq!(result.0.len(), 1);
    }

    #[test]
    fn test_merge_shells_empty() {
        let result = merge_shells(Vec::new());
        assert!(result.0.is_empty());
    }
}

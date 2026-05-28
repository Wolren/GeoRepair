use geo::{MultiPolygon, Polygon};

pub(crate) fn merge_shells(shells: Vec<Polygon<f64>>) -> MultiPolygon<f64> {
    let mp = MultiPolygon::new(shells);
    geo::algorithm::bool_ops::unary_union(&mp)
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

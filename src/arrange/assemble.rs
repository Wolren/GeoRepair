use geo::coordinate_position::{coord_pos_relative_to_ring, CoordPos};
use geo::{LineString, MultiPolygon, Polygon, Winding};
use rstar::{RTree, RTreeObject, AABB};

use crate::util;

pub(crate) fn assemble_polygons(rings: Vec<geo::LineString<f64>>) -> geo::MultiPolygon<f64> {
    let mut exteriors: Vec<LineString<f64>> = Vec::new();
    let mut holes: Vec<LineString<f64>> = Vec::new();

    for ring in &rings {
        let sum = util::shoelace_sum(&ring.0);
        if sum > 0.0 {
            exteriors.push(ring.clone());
        } else if sum < 0.0 {
            holes.push(ring.clone());
        }
    }

    if exteriors.is_empty() && holes.is_empty() {
        return MultiPolygon::new(Vec::new());
    }
    if exteriors.is_empty() {
        let mut flipped = holes;
        for r in &mut flipped {
            r.make_ccw_winding();
        }
        return MultiPolygon::new(
            flipped
                .into_iter()
                .map(|r| Polygon::new(r, Vec::new()))
                .collect(),
        );
    }

    let mut polygons: Vec<(LineString<f64>, Vec<LineString<f64>>)> =
        exteriors.into_iter().map(|ext| (ext, Vec::new())).collect();

    struct ExtEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
    }
    impl RTreeObject for ExtEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }
    let ext_tree = {
        let mut envs = Vec::with_capacity(polygons.len());
        for (i, (ext, _)) in polygons.iter().enumerate() {
            let first = ext.0.first().map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.0, first.0, first.1, first.1);
            for c in &ext.0 {
                min_x = min_x.min(c.x);
                max_x = max_x.max(c.x);
                min_y = min_y.min(c.y);
                max_y = max_y.max(c.y);
            }
            envs.push(ExtEnv {
                idx: i,
                env: AABB::from_corners([min_x, min_y], [max_x, max_y]),
            });
        }
        RTree::bulk_load(envs)
    };

    let mut unassigned_holes: Vec<LineString<f64>> = Vec::new();
    'next_hole: for hole in &holes {
        for pt in &hole.0 {
            let query = AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
            let mut found = None;
            let _ = ext_tree.locate_in_envelope_intersecting_int(&query, |c| {
                if coord_pos_relative_to_ring(*pt, &polygons[c.idx].0) == CoordPos::Inside {
                    found = Some(c.idx);
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            });
            if let Some(idx) = found {
                polygons[idx].1.push(hole.clone());
                continue 'next_hole;
            }
        }
        unassigned_holes.push(hole.clone());
    }

    for mut hole in unassigned_holes {
        hole.make_ccw_winding();
        polygons.push((hole, Vec::new()));
    }

    MultiPolygon::new(
        polygons
            .into_iter()
            .map(|(ext, holes)| Polygon::new(ext, holes))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util;
    use geo::Coord;

    #[test]
    fn test_shoelace_sum_ccw_ring() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let sum = util::shoelace_sum(&ring.0);
        assert!(sum > 0.0);
    }

    #[test]
    fn test_shoelace_sum_cw_ring() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let sum = util::shoelace_sum(&ring.0);
        assert!(sum < 0.0);
    }

    #[test]
    fn test_shoelace_sum_zero_area() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let sum = util::shoelace_sum(&ring.0);
        assert_eq!(sum, 0.0);
    }

    #[test]
    fn test_shoelace_sum_single() {
        let ring = LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 0.0, y: 0.0 }]);
        let sum = util::shoelace_sum(&ring.0);
        assert_eq!(sum, 0.0);
    }

    #[test]
    fn test_assemble_polygons_empty() {
        let result = assemble_polygons(Vec::new());
        assert!(result.0.is_empty());
    }

    #[test]
    fn test_assemble_polygons_single_ccw() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let result = assemble_polygons(vec![ring]);
        assert_eq!(result.0.len(), 1);
    }

    #[test]
    fn test_assemble_polygons_single_cw() {
        // CW ring becomes own exterior (flipped)
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let result = assemble_polygons(vec![ring]);
        assert_eq!(result.0.len(), 1);
    }

    #[test]
    fn test_assemble_polygons_ccw_with_cw_hole() {
        let exterior = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let hole = LineString::new(vec![
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 4.0 },
            Coord { x: 4.0, y: 4.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 2.0, y: 2.0 },
        ]);
        let result = assemble_polygons(vec![exterior, hole]);
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.0[0].interiors().len(), 1);
    }

    #[test]
    fn test_assemble_polygons_hole_outside_exterior() {
        let exterior = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let outside_hole = LineString::new(vec![
            Coord { x: 20.0, y: 20.0 },
            Coord { x: 20.0, y: 25.0 },
            Coord { x: 25.0, y: 25.0 },
            Coord { x: 25.0, y: 20.0 },
            Coord { x: 20.0, y: 20.0 },
        ]);
        let result = assemble_polygons(vec![exterior, outside_hole]);
        assert_eq!(result.0.len(), 2);
    }

    #[test]
    fn test_assemble_polygons_two_exteriors() {
        let ext1 = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 0.0, y: 5.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let ext2 = LineString::new(vec![
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 15.0, y: 10.0 },
            Coord { x: 15.0, y: 15.0 },
            Coord { x: 10.0, y: 15.0 },
            Coord { x: 10.0, y: 10.0 },
        ]);
        let result = assemble_polygons(vec![ext1, ext2]);
        assert_eq!(result.0.len(), 2);
    }
}

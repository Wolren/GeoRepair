use geo::{Coord, MultiPolygon, Polygon};
use log::warn;

/// Merge overlapping shells using even-parent filter to prevent NestedHoles.
///
/// When shells are fully nested (one inside another), `unary_union` produces
/// MultiPolygon components where one has a hole that exactly matches the next
/// component — this is the NestedHoles validity error.
///
/// The BuildArea even-parent approach: sort shells by area, count how many
/// larger shells contain each shell, keep only shells with even parent count.
/// Then run unary_union on the kept shells.
pub(crate) fn merge_shells(shells: Vec<Polygon<f64>>) -> MultiPolygon<f64> {
    if shells.is_empty() {
        return MultiPolygon::new(Vec::new());
    }
    if shells.len() == 1 {
        return MultiPolygon::new(shells);
    }
    // Fast path: if bboxes are disjoint, no nesting possible
    if shells_are_disjoint(&shells) {
        return MultiPolygon::new(shells);
    }
    // Even-parent filter: sort by area, count larger containing shells
    let mut with_area: Vec<(Polygon<f64>, f64)> = shells
        .into_iter()
        .map(|p| {
            let area = shoelace_abs_sum(&p.exterior().0);
            (p, area)
        })
        .collect();
    with_area.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let n = with_area.len();
    let mut parent_count: Vec<usize> = vec![0; n];
    for i in 0..n {
        let ext_i = &with_area[i].0.exterior().0;
        if ext_i.len() < 4 { continue; }
        // Try first vertex. If on boundary, try midpoint of first edge.
        let pt_candidates = [ext_i[0], Coord {
            x: (ext_i[0].x + ext_i[1].x) * 0.5,
            y: (ext_i[0].y + ext_i[1].y) * 0.5,
        }];
        for j in 0..i {
            let ext_j = &with_area[j].0.exterior().0;
            if ext_j.len() < 4 { continue; }
            let contained = pt_candidates.iter().any(|&pt| point_in_ring_exclusive(pt, ext_j));
            if contained {
                parent_count[i] += 1;
            }
        }
    }

    let kept: Vec<Polygon<f64>> = with_area
        .into_iter()
        .enumerate()
        .filter_map(|(i, (poly, _))| {
            if parent_count[i] % 2 == 0 { Some(poly) } else { None }
        })
        .collect();

    if kept.is_empty() {
        warn!("merge_shells: even-parent filter removed all shells");
        return MultiPolygon::new(Vec::new());
    }
    if kept.len() == 1 {
        return MultiPolygon::new(kept);
    }

    // Union remaining (possibly overlapping) shells
    let mp = MultiPolygon::new(kept);
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

fn shoelace_abs_sum(coords: &[Coord<f64>]) -> f64 {
    let n = coords.len();
    if n < 3 { return 0.0; }
    let end = if coords.first() == coords.last() { n - 1 } else { n };
    let mut sum = 0.0_f64;
    for i in 0..end - 1 {
        sum += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }
    sum += coords[end - 1].x * coords[0].y - coords[0].x * coords[end - 1].y;
    sum.abs()
}

fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    if ring.len() < 4 { return false; }
    let n = ring.len() - 1;
    let mut inside = false;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        let intersect = ((yi > pt.y) != (yj > pt.y))
            && (pt.x < (xj - xi) * (pt.y - yi) / (yj - yi) + xi);
        if intersect { inside = !inside; }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, LineString};
    use crate::validation::GeoValidation;

    #[test]
    fn test_single() {
        let shell = Polygon::new(
            LineString::new(vec![Coord { x: 0., y: 0. }, Coord { x: 1., y: 0. },
                Coord { x: 1., y: 1. }, Coord { x: 0., y: 1. }, Coord { x: 0., y: 0. }]),
            Vec::new(),
        );
        let result = merge_shells(vec![shell]);
        assert_eq!(result.0.len(), 1);
        assert!(result.is_valid());
    }

    #[test]
    fn test_disjoint() {
        let s1 = Polygon::new(
            LineString::new(vec![Coord { x: 0., y: 0. }, Coord { x: 1., y: 0. },
                Coord { x: 1., y: 1. }, Coord { x: 0., y: 1. }, Coord { x: 0., y: 0. }]),
            Vec::new(),
        );
        let s2 = Polygon::new(
            LineString::new(vec![Coord { x: 2., y: 2. }, Coord { x: 3., y: 2. },
                Coord { x: 3., y: 3. }, Coord { x: 2., y: 3. }, Coord { x: 2., y: 2. }]),
            Vec::new(),
        );
        let result = merge_shells(vec![s1, s2]);
        assert_eq!(result.0.len(), 2);
        assert!(result.is_valid());
    }

    #[test]
    fn test_nested_removes_inner() {
        // Outer fully contains inner → even-parent drops inner
        let outer = Polygon::new(
            LineString::new(vec![Coord { x: 0., y: 0. }, Coord { x: 10., y: 0. },
                Coord { x: 10., y: 10. }, Coord { x: 0., y: 10. }, Coord { x: 0., y: 0. }]),
            Vec::new(),
        );
        let inner = Polygon::new(
            LineString::new(vec![Coord { x: 3., y: 3. }, Coord { x: 7., y: 3. },
                Coord { x: 7., y: 7. }, Coord { x: 3., y: 7. }, Coord { x: 3., y: 3. }]),
            Vec::new(),
        );
        let result = merge_shells(vec![outer, inner]);
        assert!(result.is_valid(), "Even-parent should prevent NestedHoles");
        assert_eq!(result.0.len(), 1, "Inner shell should be filtered out");
    }

    #[test]
    fn test_deep_nesting() {
        let l0 = Polygon::new(
            LineString::new(vec![Coord { x: 0., y: 0. }, Coord { x: 20., y: 0. },
                Coord { x: 20., y: 20. }, Coord { x: 0., y: 20. }, Coord { x: 0., y: 0. }]),
            Vec::new(),
        );
        let l1 = Polygon::new(
            LineString::new(vec![Coord { x: 3., y: 3. }, Coord { x: 17., y: 3. },
                Coord { x: 17., y: 17. }, Coord { x: 3., y: 17. }, Coord { x: 3., y: 3. }]),
            Vec::new(),
        );
        let l2 = Polygon::new(
            LineString::new(vec![Coord { x: 6., y: 6. }, Coord { x: 14., y: 6. },
                Coord { x: 14., y: 14. }, Coord { x: 6., y: 14. }, Coord { x: 6., y: 6. }]),
            Vec::new(),
        );
        let result = merge_shells(vec![l0, l1, l2]);
        assert!(result.is_valid(), "Even-parent should handle deep nesting");
        // L0 (0 parents) kept, L1 (1 parent) dropped, L2 (2 parents) kept but
        // union handles it
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_empty() {
        let result = merge_shells(Vec::new());
        assert!(result.0.is_empty());
    }
}

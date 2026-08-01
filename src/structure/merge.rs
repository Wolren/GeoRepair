use geo::{Area, Coord, MultiPolygon, Polygon, Winding};
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
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn merge_shells(shells: Vec<Polygon<f64>>) -> MultiPolygon<f64> {
    if shells.is_empty() {
        return MultiPolygon::new(Vec::new());
    }
    // Normalize winding FIRST: geo's unary_union silently drops area when
    // input shells are CW (verified: square+rect union = 0.84 CW vs 1.80 CCW).
    // Per-polygon make_valid usually normalizes before calling us, but direct
    // callers (XML fixtures, tests) may not.
    let shells: Vec<Polygon<f64>> = shells
        .into_iter()
        .map(|mut p| {
            if p.exterior().0.len() >= 4
                && !crate::util::robust_is_ccw(&p.exterior().0)
            {
                p.exterior_mut(|r| r.make_ccw_winding());
            }
            for i in 0..p.interiors().len() {
                let hole = p.interiors()[i].clone();
                if crate::util::robust_is_ccw(&hole.0) {
                    p.interiors_mut(|rings| rings[i].make_cw_winding());
                }
            }
            p
        })
        .collect();
    if shells.len() == 1 {
        return MultiPolygon::new(shells);
    }
    // Cancel identical shells pairwise: hole==shell (or duplicate MP
    // components) produce two shells with the same coordinate set — geo's
    // union used to cancel them via opposite winding, but after winding
    // normalization both are CCW and the union keeps the full area (wrong:
    // GEOS/JTS return empty for hole==shell). Remove the pair entirely.
    let shells = cancel_identical_shells(shells);
    if shells.len() <= 1 {
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
            // Full-containment check: EVERY vertex of shell i must be strictly
            // inside shell j (exclusive, holes excluded). A single probe point
            // (or even two) misclassifies PARTIAL overlaps as nesting — e.g.
            // rect (0.8 0.1, 2 0.1, ...) overlapping square (0 0, 1 1): its
            // first vertex is inside the square, but the shell extends to x=2.
            // GEOS's even-parent filter only drops fully-contained shells.
            for j in 0..i {
                if ring_fully_inside(ext_i, &with_area[j].0) {
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

    // Union remaining (possibly overlapping) shells.
    // i_overlay (geo's boolean engine) can panic with an internal
    // `is_fill_top` assertion on degenerate shell sets (measured seed:
    // shell [(54.36,0),(0,0),(18.55,82.91),(-48.92,33.44)] + 6-vert hole).
    // A panic inside a rayon batch kills the whole run, so catch it and
    // return the even-parent-filtered shells without the union — the Auto
    // validator then routes to arrange/reduce; Structure only promises no
    // panic on its output.
    //
    // Area-preservation guard: unary_union must not LOSE area. geo's
    // OverlayNG port drops an island-in-hole component during union
    // (measured: square-with-hole ∪ island-inside-hole → 144, island 64
    // lost; GEOS returns MULTIPOLYGON(square-with-hole, island) = 400-64).
    // Discriminator: when the union shrinks total area AND the
    // even-parent-filtered shells are already valid (winding-insensitive
    // geo validation — island-in-hole is valid, nested-in-fill is not),
    // the union was unnecessary or wrong: keep the filtered shells.
    // Legit nesting merges (deep nesting: l2 absorbed into l0) shrink the
    // pre-union MP's summed area too, but the pre-union MP is INVALID there
    // (nested-in-fill), so the union result stands.
    let mp = MultiPolygon::new(kept);
    let before: f64 = mp.0.iter().map(|p| p.unsigned_area()).sum();
    let eps = 1e-9;
    let unioned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        geo::algorithm::bool_ops::unary_union(&mp)
    }))
    .ok()
    .and_then(|u| {
        let after: f64 = u.0.iter().map(|p| p.unsigned_area()).sum();
        if after >= before - eps {
            Some(u)
        } else if geo::algorithm::Validation::is_valid(&mp) {
            // Union dropped area but filtered shells are valid — keep them.
            None
        } else {
            Some(u)
        }
    });
    unioned.unwrap_or(mp)
}

/// True if every vertex of `ring` lies strictly inside `poly` (exterior
/// exclusive, not inside any hole of `poly`). Used by the even-parent filter:
/// a shell is only "nested" when fully contained — partial overlaps must be
/// kept so unary_union can merge them.
fn ring_fully_inside(ring: &[Coord<f64>], poly: &Polygon<f64>) -> bool {
    if ring.len() < 4 {
        return false;
    }
    for &pt in ring {
        if !point_in_polygon_exclusive(pt, poly) {
            return false;
        }
    }
    true
}

/// Remove shells that appear more than once with the same coordinate set
/// (as unordered sets — winding/reversal/rotation-insensitive). Duplicate
/// shells cancel: hole==shell must yield empty, duplicate MP components
/// must not become DuplicatedRings.
fn cancel_identical_shells(shells: Vec<Polygon<f64>>) -> Vec<Polygon<f64>> {
    let mut fingerprints: Vec<(Vec<(u64, u64)>, Polygon<f64>)> = shells
        .into_iter()
        .map(|p| {
            let mut pts: Vec<(u64, u64)> = p
                .exterior()
                .0
                .iter()
                .map(|c| (c.x.to_bits(), c.y.to_bits()))
                .collect();
            if pts.first() == pts.last() {
                pts.pop();
            }
            pts.sort_unstable();
            (pts, p)
        })
        .collect();
    let mut i = 0;
    while i < fingerprints.len() {
        let mut j = i + 1;
        while j < fingerprints.len() {
            if fingerprints[i].0 == fingerprints[j].0 {
                // Pair cancels — remove both
                fingerprints.remove(j);
                fingerprints.remove(i);
                i = i.saturating_sub(1);
                break;
            }
            j += 1;
        }
        i += 1;
    }
    fingerprints.into_iter().map(|(_, p)| p).collect()
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
    // Boundary check (exclusive: on-edge → outside)
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        let orient = (xi - pt.x) * (yj - pt.y) - (xj - pt.x) * (yi - pt.y);
        if orient.abs() < 1e-15 {
            let min_x = xi.min(xj);
            let max_x = xi.max(xj);
            let min_y = yi.min(yj);
            let max_y = yi.max(yj);
            if pt.x >= min_x - 1e-12 && pt.x <= max_x + 1e-12
                && pt.y >= min_y - 1e-12 && pt.y <= max_y + 1e-12
            {
                return false;
            }
        }
    }
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

/// Point in polygon exclusive of boundary and holes.
fn point_in_polygon_exclusive(pt: Coord<f64>, poly: &Polygon<f64>) -> bool {
    if !point_in_ring_exclusive(pt, &poly.exterior().0) {
        return false;
    }
    for h in poly.interiors() {
        // Interior of a hole → not inside the polygon fill
        if point_in_ring_exclusive(pt, &h.0) {
            return false;
        }
        // On hole boundary → exclusive outside fill
        // (point_in_ring_exclusive already returns false for boundary)
    }
    true
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

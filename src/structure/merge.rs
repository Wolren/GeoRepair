use alloc::vec::Vec;
use geo::{Area, Coord, LineString, MultiPolygon, Polygon, Winding};
use log::warn;

/// Merge overlapping shells with an even-parent filter to prevent NestedHoles. When
/// shells are fully nested, unary_union makes MultiPolygon components with a hole that
/// exactly matches the next component. BuildArea even-parent rule: sort shells by area,
/// count containing larger shells, and keep only shells with an even parent count.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn merge_shells(shells: Vec<Polygon<f64>>) -> MultiPolygon<f64> {
    if shells.is_empty() {
        return MultiPolygon::new(Vec::new());
    }
    // input shells are CW (verified: square+rect union = 0.84 CW vs 1.80 ccw).
    let shells: Vec<Polygon<f64>> = shells
        .into_iter()
        .map(|mut p| {
            if p.exterior().0.len() >= 4 && !crate::util::robust_is_ccw(&p.exterior().0) {
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
    // normalization both are ccw and the union keeps the full area (wrong: GEOS/JTS
    // return empty for hole==shell).
    let shells = cancel_identical_shells(shells);
    if shells.len() <= 1 {
        return MultiPolygon::new(shells);
    }
    if shells_are_disjoint(&shells) {
        return MultiPolygon::new(shells);
    }
    let mut with_area: Vec<(Polygon<f64>, f64)> = shells
        .into_iter()
        .map(|p| {
            let area = crate::util::shoelace_abs_sum(&p.exterior().0);
            (p, area)
        })
        .collect();
    with_area.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));

    let n = with_area.len();
    // not dropped -dropping loses the covered area (measured: nested squares → 144, not
    // 400). parent[i] = immediate parent
    let mut parent_count: Vec<usize> = vec![0; n];
    let mut parent: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let ext_i = &with_area[i].0.exterior().0;
        if ext_i.len() < 4 {
            continue;
        }
        // Full-containment check: every vertex of shell i must be strictly inside shell
        // j (exclusive, holes excluded). rect (0.8 0.1, 2 0.1, .) overlapping square (0
        // 0, 1 1)
        for (j, wa) in with_area.iter().enumerate().take(i) {
            if ring_fully_inside(ext_i, &wa.0) {
                parent_count[i] += 1;
                parent[i] = Some(j);
            }
        }
    }

    let kept: Vec<Polygon<f64>> = with_area
        .iter()
        .enumerate()
        .filter_map(|(i, (poly, _))| {
            if !parent_count[i].is_multiple_of(2) {
                return None; // odd → becomes a hole of its parent
            }
            let mut poly = poly.clone();
            let hole_fps: Vec<Vec<(u64, u64)>> = poly
                .interiors()
                .iter()
                .map(|h| crate::util::ring_fingerprint(&h.0))
                .collect();
            let parent_bbox = crate::simd::aabb_minmax_simd(&poly.exterior().0);
            let safe_to_convert = with_area
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != i && parent_count[*k].is_multiple_of(2))
                .all(|(k, _)| {
                    let (m2x, m2x2, m2y, m2y2) =
                        crate::simd::aabb_minmax_simd(&with_area[k].0.exterior().0);
                    let (mnx, mxx, mny, mxy) = parent_bbox;
                    !(mnx <= m2x2 && mxx >= m2x && mny <= m2y2 && mxy >= m2y)
                });
            for k in 0..n {
                if k == i || parent_count[k].is_multiple_of(2) {
                    continue;
                }
                if safe_to_convert && parent[k] == Some(i) {
                    let fp = crate::util::ring_fingerprint(&with_area[k].0.exterior().0);
                    if !hole_fps.contains(&fp) {
                        let mut hole = LineString::new(with_area[k].0.exterior().0.clone());
                        hole.make_cw_winding();
                        poly.interiors_push(hole);
                    }
                }
            }
            Some(poly)
        })
        .collect();

    if kept.is_empty() {
        warn!("merge_shells: even-parent filter removed all shells");
        return MultiPolygon::new(Vec::new());
    }
    if kept.len() == 1 {
        return MultiPolygon::new(kept);
    }

    // `is_fill_top` assertion on degenerate shell sets (measured seed: shell
    // [(54.36,0),(0,0),(18.55,82.91),(-48.92,33.44)] + 6-vert hole). (measured:
    // square-with-hole ∪ island-inside-hole → 144, island 64 lost; GEOS returns
    // multipolygon(square-with-hole, island) = 400-64).
    let mp = MultiPolygon::new(kept);
    let before: f64 = mp.0.iter().map(|p| p.unsigned_area()).sum();
    let eps = 1e-9;
    #[cfg(feature = "std")]
    let unioned = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        geo::algorithm::bool_ops::unary_union(&mp)
    }))
    .ok()
    .and_then(|u| {
        let after: f64 = u.0.iter().map(|p| p.unsigned_area()).sum();
        if after >= before - eps {
            Some(u)
        } else if shells_look_valid(&mp) {
            None
        } else {
            Some(u)
        }
    });
    #[cfg(not(feature = "std"))]
    let unioned = {
        let u = geo::algorithm::bool_ops::unary_union(&mp);
        let after: f64 = u.0.iter().map(|p| p.unsigned_area()).sum();
        if after >= before - eps {
            Some(u)
        } else if shells_look_valid(&mp) {
            None
        } else {
            Some(u)
        }
    };
    unioned.unwrap_or(mp)
}

/// Winding-insensitive probe: are the filtered shells already valid? It guards area.
/// validation::is_valid cannot be trusted not to panic. The geo relate engine carries a
/// debug_assert false topology conflict at geo 0.33.1 edge_end_bundle_star.rs:116. The
/// assertion escaped as a fuzz crash. make_valid panicked on wkb in mode Auto,
/// fuzz-nightly run 35836311392. It was slow: 30 to 45 s, where mp_looks_valid takes
/// ~100 ms.
#[cfg(feature = "std")]
fn shells_look_valid(mp: &MultiPolygon<f64>) -> bool {
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        crate::make_valid::mp_looks_valid(mp)
    }))
    .unwrap_or(false)
}

#[cfg(not(feature = "std"))]
fn shells_look_valid(mp: &MultiPolygon<f64>) -> bool {
    crate::make_valid::mp_looks_valid(mp)
}

/// True if every vertex of `ring` lies strictly inside `poly` (exterior exclusive, not
/// inside any hole of `poly`). Used by the even-parent filter: a shell is only "nested"
/// when fully contained - partial overlaps must be kept so unary_union can merge them.
fn ring_fully_inside(ring: &[Coord<f64>], poly: &Polygon<f64>) -> bool {
    if ring.len() < 4 {
        return false;
    }
    for &pt in ring {
        if !point_in_polygon_exclusive(pt, poly) {
            return false;
        }
    }
    // hole loses its area (measured: general_TestValid island sharing hole-ring
    // vertices -> -3.75% even-odd area).
    if let Some(probe) = crate::util::ring_interior_probe(ring)
        && poly
            .interiors()
            .iter()
            .any(|h| crate::util::point_in_ring_exclusive_even_odd(probe, &h.0))
        // (measured: 300 -> 400 even-odd area).
        && !poly
            .interiors()
            .iter()
            .any(|h| crate::util::ring_fingerprint(&h.0) == crate::util::ring_fingerprint(ring))
    {
        return false;
    }
    true
}

/// Remove shells that appear more than once with the same coordinate set (as unordered
/// sets - winding/reversal/rotation-insensitive). Duplicate shells cancel: hole==shell
/// must yield empty, duplicate MP components must not become DuplicatedRings.
fn cancel_identical_shells(shells: Vec<Polygon<f64>>) -> Vec<Polygon<f64>> {
    type ShellFingerprint = (Vec<(u64, u64)>, Polygon<f64>);
    let mut fingerprints: Vec<ShellFingerprint> = shells
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

/// Winding/rotation/closure-insensitive ring fingerprint (u64 bits).
fn point_in_polygon_exclusive(pt: Coord<f64>, poly: &Polygon<f64>) -> bool {
    if !crate::util::point_in_ring_exclusive_even_odd(pt, &poly.exterior().0) {
        return false;
    }
    for h in poly.interiors() {
        if crate::util::point_in_ring_exclusive_even_odd(pt, &h.0) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::GeoValidation;
    use geo::{Coord, LineString};

    #[test]
    fn test_single() {
        let shell = Polygon::new(
            LineString::new(vec![
                Coord { x: 0., y: 0. },
                Coord { x: 1., y: 0. },
                Coord { x: 1., y: 1. },
                Coord { x: 0., y: 1. },
                Coord { x: 0., y: 0. },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![shell]);
        assert_eq!(result.0.len(), 1);
        assert!(result.is_valid());
    }

    #[test]
    fn test_disjoint() {
        let s1 = Polygon::new(
            LineString::new(vec![
                Coord { x: 0., y: 0. },
                Coord { x: 1., y: 0. },
                Coord { x: 1., y: 1. },
                Coord { x: 0., y: 1. },
                Coord { x: 0., y: 0. },
            ]),
            Vec::new(),
        );
        let s2 = Polygon::new(
            LineString::new(vec![
                Coord { x: 2., y: 2. },
                Coord { x: 3., y: 2. },
                Coord { x: 3., y: 3. },
                Coord { x: 2., y: 3. },
                Coord { x: 2., y: 2. },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![s1, s2]);
        assert_eq!(result.0.len(), 2);
        assert!(result.is_valid());
    }

    #[test]
    fn test_nested_removes_inner() {
        let outer = Polygon::new(
            LineString::new(vec![
                Coord { x: 0., y: 0. },
                Coord { x: 10., y: 0. },
                Coord { x: 10., y: 10. },
                Coord { x: 0., y: 10. },
                Coord { x: 0., y: 0. },
            ]),
            Vec::new(),
        );
        let inner = Polygon::new(
            LineString::new(vec![
                Coord { x: 3., y: 3. },
                Coord { x: 7., y: 3. },
                Coord { x: 7., y: 7. },
                Coord { x: 3., y: 7. },
                Coord { x: 3., y: 3. },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![outer, inner]);
        assert!(result.is_valid(), "Even-parent should prevent NestedHoles");
        assert_eq!(result.0.len(), 1, "Inner shell should be filtered out");
    }

    #[test]
    fn test_deep_nesting() {
        let l0 = Polygon::new(
            LineString::new(vec![
                Coord { x: 0., y: 0. },
                Coord { x: 20., y: 0. },
                Coord { x: 20., y: 20. },
                Coord { x: 0., y: 20. },
                Coord { x: 0., y: 0. },
            ]),
            Vec::new(),
        );
        let l1 = Polygon::new(
            LineString::new(vec![
                Coord { x: 3., y: 3. },
                Coord { x: 17., y: 3. },
                Coord { x: 17., y: 17. },
                Coord { x: 3., y: 17. },
                Coord { x: 3., y: 3. },
            ]),
            Vec::new(),
        );
        let l2 = Polygon::new(
            LineString::new(vec![
                Coord { x: 6., y: 6. },
                Coord { x: 14., y: 6. },
                Coord { x: 14., y: 14. },
                Coord { x: 6., y: 14. },
                Coord { x: 6., y: 6. },
            ]),
            Vec::new(),
        );
        let result = merge_shells(vec![l0, l1, l2]);
        assert!(result.is_valid(), "Even-parent should handle deep nesting");
        // L0 (0 parents) kept, L1 (1 parent) dropped, L2 (2 parents) kept but union
        // handles it
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_empty() {
        let result = merge_shells(Vec::new());
        assert!(result.0.is_empty());
    }
}

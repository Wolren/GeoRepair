//! Degenerate-strip cleanup: NaN/Inf handling, isSimple checks, and
//! OGC winding helpers.

use super::*;
use crate::noding::remove_consecutive_duplicates;
use alloc::vec::Vec;

pub(super) fn has_nan(g: &Geometry<f64>) -> bool {
    use geo::CoordsIter;
    g.coords_iter()
        .any(|c| !c.x.is_finite() || !c.y.is_finite())
}

/// Candidate demotion line from a ring: drop the closing vertex and
/// consecutive duplicates (a collapsed ring demotes to its open path).
/// None when fewer than two finite, distinct coords remain.
fn ring_demotion_candidate(ring: &[Coord<f64>]) -> Option<LineString<f64>> {
    if ring.len() < 2 || ring.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
        return None;
    }
    let mut coords = ring.to_vec();
    if coords.first() == coords.last() {
        coords.pop();
    }
    let coords = remove_consecutive_duplicates(&coords);
    if coords.len() < 2 {
        None
    } else {
        Some(LineString::new(coords))
    }
}

/// Exact-collinearity test for a ring exterior, anchored on the widest
/// vertex pair (the dominant axis's extremes). The first-edge anchor is
/// unsound: a subnormal-length leading edge underflows every orientation
/// to zero, so a real 1000 x 2e-9 sliver read as collinear and was
/// demoted into a NotSimple line (fuzz-nightly crash 2026-09-14).
fn ring_is_exactly_collinear(ext: &[Coord<f64>], interior_n: usize, ia: usize, ib: usize) -> bool {
    let a = ext[ia];
    let b = ext[ib];
    if a == b {
        return true;
    }
    (0..interior_n).all(|i| crate::orient::orient2d(a, b, ext[i]) == 0.0)
}

/// Contract filter for demoted lines: keep only candidates the validator
/// accepts as simple, then greedily drop any component that intersects an
/// already-kept one, with the same rule and scale-derived eps as the
/// MultiLineString validator (`check_line_components_intersect`).
/// `prep::has_no_intersections` uses a different tolerance set and passed
/// a subnormal sliver the validator flags NotSimple (fuzz-nightly crash
/// 2026-09-14); the validator's own predicate is the contract, so
/// demotions answer to it directly.
fn filter_demoted_lines(candidates: Vec<LineString<f64>>) -> Vec<LineString<f64>> {
    let simple: Vec<LineString<f64>> = candidates
        .into_iter()
        .filter(|ls| !check_linestring_self_intersection(&ls.0))
        .collect();
    if simple.len() < 2 {
        return simple;
    }
    let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for ls in &simple {
        for c in &ls.0 {
            gmin_x = gmin_x.min(c.x);
            gmax_x = gmax_x.max(c.x);
            gmin_y = gmin_y.min(c.y);
            gmax_y = gmax_y.max(c.y);
        }
    }
    let scale = (gmax_x - gmin_x)
        .abs()
        .max((gmax_y - gmin_y).abs())
        .max(1.0);
    let eps = 1e-12 * scale;
    let mut kept: Vec<LineString<f64>> = Vec::new();
    'cand: for ls in simple {
        for k in &kept {
            if check_line_components_intersect(&k.0, &ls.0, eps) {
                continue 'cand;
            }
        }
        kept.push(ls);
    }
    kept
}

/// Emit demoted lines: LineString for one, MultiLineString for several,
/// empty when none survived the contract filter.
fn emit_lines(mut lines: Vec<LineString<f64>>) -> Geometry<f64> {
    match lines.len() {
        0 => empty_geom::<f64>(),
        1 => Geometry::LineString(lines.pop().expect("len==1 verified")),
        _ => Geometry::MultiLineString(MultiLineString::new(lines)),
    }
}

/// Keep/demote decision for one MultiPolygon component. Components at or
/// above the historical area cut stay as they are; smaller ones are kept
/// only when the validator accepts them. The absolute cut demoted VALID
/// micro-polygons, and the raw closed-ring emission turned them into a
/// NotSimple MULTILINESTRING (fuzz-nightly crash 2026-09-18: two valid
/// ~1e-15-area triangles). A small component the validator rejects
/// (self-touching ring, wrong winding) still demotes.
fn polygon_component_is_kept(p: &Polygon<f64>) -> bool {
    let ext = &p.exterior().0;
    if ext.len() < 4 || ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
        return false;
    }
    if shoelace_abs_sum(ext) >= 1e-12 {
        return true;
    }
    p.validate().valid
}

/// Remove degenerate Polygon/MultiPolygon components: exterior rings with
/// <4 coordinates, shoelace area below epsilon, or NaN/Inf coordinates.
/// Returns boundary LineString for degenerate polygons (GEOS-style type degradation).
pub fn strip_degenerate_test(g: Geometry<f64>) -> Geometry<f64> {
    strip_degenerate(g)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn strip_degenerate(g: Geometry<f64>) -> Geometry<f64> {
    match g {
        Geometry::Polygon(p) => {
            let ext = &p.exterior().0;
            // Fast path: valid polygons with ≥4 coords and no NaN pass through
            if ext.len() >= 4 {
                // Single-pass: compute bbox, shoelace, and NaN simultaneously
                let n = ext.len();
                let interior_n = if ext.first() == ext.last() { n - 1 } else { n };
                // Guard: fewer than 3 unique vertices can never form a valid polygon.
                // (interior_n == 3 with positive area is a valid triangle.)
                if interior_n < 3 {
                    // Fall through to boundary output below.
                } else {
                    let (mut min_x, mut max_x, mut min_y, mut max_y) =
                        (ext[0].x, ext[0].x, ext[0].y, ext[0].y);
                    // Extreme indices ride along: the collinearity test below
                    // anchors on the widest vertex pair, never the first edge
                    // (see ring_is_exactly_collinear).
                    let (mut imin_x, mut imax_x, mut imin_y, mut imax_y) =
                        (0usize, 0usize, 0usize, 0usize);
                    let mut has_nan = !ext[0].x.is_finite() || !ext[0].y.is_finite();
                    for i in 0..interior_n - 1 {
                        let c = ext[i + 1];
                        if c.x < min_x {
                            min_x = c.x;
                            imin_x = i + 1;
                        }
                        if c.x > max_x {
                            max_x = c.x;
                            imax_x = i + 1;
                        }
                        if c.y < min_y {
                            min_y = c.y;
                            imin_y = i + 1;
                        }
                        if c.y > max_y {
                            max_y = c.y;
                            imax_y = i + 1;
                        }
                        if !has_nan && (!c.x.is_finite() || !c.y.is_finite()) {
                            has_nan = true;
                        }
                    }
                    // Bbox degeneracy is per-axis LOCAL (same rule as the
                    // make_valid pre-gate): an axis is degenerate when its
                    // extent is at or below the coordinate rounding at that
                    // axis's own magnitude. The old rule compared against the
                    // max spread, so a 4.9e208 spike dominated the 1-unit
                    // x-extent of a valid ring and demoted it to a line.
                    let x_scale = max_x.abs().max(min_x.abs());
                    let y_scale = max_y.abs().max(min_y.abs());
                    let bbox_ok = (max_x - min_x).abs() > f64::EPSILON * x_scale
                        && (max_y - min_y).abs() > f64::EPSILON * y_scale;
                    // Area degeneracy: a ring is degenerate iff its vertices lie
                    // bit-exactly on one line (robust orient == 0). The
                    // historical magnitude-based noise bound demoted real
                    // slivers at large coordinate magnitude (0.14.2 changelog).
                    // Anchor on the dominant axis's extreme vertex pair, never
                    // the first edge: a subnormal-length leading edge (e.g.
                    // (0,0) -> (1.366e-319,0)) underflows every orientation
                    // to zero, so a real 1000 x 2e-9 sliver read as collinear
                    // and was demoted into a NotSimple line (fuzz-nightly
                    // crash 2026-09-14).
                    let (ia, ib) = if (max_x - min_x).abs() >= (max_y - min_y).abs() {
                        (imin_x, imax_x)
                    } else {
                        (imin_y, imax_y)
                    };
                    let area_ok = !ring_is_exactly_collinear(ext, interior_n, ia, ib);
                    if area_ok && bbox_ok && !has_nan {
                        // Non-degenerate polygon - return as-is after hole cleanup
                        let holes: Vec<LineString<f64>> = p
                            .interiors()
                            .iter()
                            .filter(|ring| {
                                ring.0.len() >= 4
                                    && !ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                            })
                            .cloned()
                            .collect();
                        return if holes.len() == p.interiors().len() {
                            Geometry::Polygon(p)
                        } else {
                            Geometry::Polygon(Polygon::new(p.exterior().clone(), holes))
                        };
                    }
                } // end interior_n >= 3
            }
            // Degenerate: demote to open LineString (drop closing vertex). Closed
            // collapsed rings are often NotSimple under OGC; open path is cleaner.
            // If still not simple → empty (keep_collapsed=false default).
            let mut lines: Vec<LineString<f64>> = Vec::new();
            if let Some(l) = ring_demotion_candidate(ext) {
                lines.push(l);
            }
            for ring in p.interiors() {
                if let Some(l) = ring_demotion_candidate(&ring.0) {
                    lines.push(l);
                }
            }
            emit_lines(filter_demoted_lines(lines))
        }
        Geometry::MultiPolygon(mp) => {
            let mut valid_polys: Vec<Polygon<f64>> = Vec::new();
            let mut boundary_lines: Vec<LineString<f64>> = Vec::new();
            for p in mp.0.into_iter() {
                if polygon_component_is_kept(&p) {
                    valid_polys.push(p);
                } else {
                    // Demote a degenerate component's boundary to open lines.
                    // The raw closed rings used to ship unchecked and read
                    // NotSimple the moment two closed components shared a
                    // vertex (fuzz-nightly crash 2026-09-18).
                    if let Some(l) = ring_demotion_candidate(&p.exterior().0) {
                        boundary_lines.push(l);
                    }
                    for ring in p.interiors() {
                        if let Some(l) = ring_demotion_candidate(&ring.0) {
                            boundary_lines.push(l);
                        }
                    }
                }
            }
            let boundary_lines = filter_demoted_lines(boundary_lines);
            match (valid_polys.len(), boundary_lines.is_empty()) {
                (0, true) => empty_geom::<f64>(),
                (0, false) => emit_lines(boundary_lines),
                // Keep MultiPolygon type even for a single component - Geometry
                // dispatch runs strip_degenerate after MultiPolygon::make_valid.
                (1, true) => Geometry::MultiPolygon(MultiPolygon::new(valid_polys)),
                (1, false) => {
                    let geoms: Vec<Geometry<f64>> = vec![
                        Geometry::MultiPolygon(MultiPolygon::new(valid_polys)),
                        emit_lines(boundary_lines),
                    ];
                    Geometry::GeometryCollection(GeometryCollection(geoms))
                }
                (_, true) => Geometry::MultiPolygon(MultiPolygon::new(valid_polys)),
                (_, false) => {
                    let mut geoms: Vec<Geometry<f64>> =
                        valid_polys.into_iter().map(Geometry::Polygon).collect();
                    geoms.push(emit_lines(boundary_lines));
                    Geometry::GeometryCollection(GeometryCollection(geoms))
                }
            }
        }
        other => other,
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn enforce_ccw(mut ring: LineString<f64>) -> (LineString<f64>, usize, bool) {
    // Use Shewchuk's orient2d (adaptive precision) on the extremal vertex
    // to determine winding order. The shoelace sum in geo's winding_order()
    // can flip sign at extreme fp ratios (e.g. 1e12 and 1e-12 in same ring).
    // Returns the extremal index + whether the ring was reversed so the
    // caller can verify the post-winding orientation without re-searching
    // (winding fusion, 2026-08-08).
    let (is_ccw, idx) = crate::util::robust_is_ccw_with_index(&ring.0);
    let reversed = !is_ccw;
    if reversed {
        // Direct reversal, not geo's make_ccw_winding(): that method
        // re-derives the winding from its own shoelace and silently
        // no-ops when the sum is indeterminate (NaN/overflow) - leaving
        // a ring the validator rejects. The robust verdict here is the
        // authority (it is the validator's own check); reversal must
        // follow it unconditionally.
        ring.0.reverse();
    }
    (ring, idx, reversed)
}

/// Variant of [`enforce_ccw`] with the extremal index already located by
/// the fast-path gate's plausibility pass (2026-08-09): the O(n)
/// `min_x_vertex` search is skipped, everything else is identical.
pub(super) fn enforce_ccw_with_idx(
    mut ring: LineString<f64>,
    idx: usize,
) -> (LineString<f64>, usize, bool) {
    let is_ccw = crate::util::robust_is_ccw_at(&ring.0, idx);
    let reversed = !is_ccw;
    if reversed {
        ring.0.reverse();
    }
    (ring, idx, reversed)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn enforce_cw(mut ring: LineString<f64>) -> (LineString<f64>, usize, bool) {
    let (is_ccw, idx) = crate::util::robust_is_ccw_with_index(&ring.0);
    let reversed = is_ccw;
    if reversed {
        ring.0.reverse();
    }
    (ring, idx, reversed)
}

/// Variant of [`enforce_cw`] with the extremal index already located.
pub(super) fn enforce_cw_with_idx(
    mut ring: LineString<f64>,
    idx: usize,
) -> (LineString<f64>, usize, bool) {
    let is_ccw = crate::util::robust_is_ccw_at(&ring.0, idx);
    let reversed = is_ccw;
    if reversed {
        ring.0.reverse();
    }
    (ring, idx, reversed)
}

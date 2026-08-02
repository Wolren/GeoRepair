//! MultiPolygon/Geometry repair: union + even-parent filtering, nested
//! component dropping, and precision-reduction fallbacks.

use super::*;
use super::polygon::{apply_target_crs, enforce_ogc_winding, is_valid_with_geo, shells_have_overlapping_bboxes, shells_have_vertex_inside};
use super::polygon::has_nan_or_infinite;
use super::strip::strip_degenerate;


#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for MultiPolygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        if self.0.is_empty() {
            return empty_geom::<f64>();
        }
        // Bail early on NaN/Inf coordinates
        if self.0.iter().any(has_nan_or_infinite) {
            return empty_geom::<f64>();
        }
        let polys: Vec<Geometry<f64>> = self
            .iter()
            .map(|p| p.make_valid_with_config(config))
            .collect();

        let mut shells = Vec::new();
        for g in polys {
            match g {
                Geometry::Polygon(p) => shells.push(p),
                Geometry::MultiPolygon(mp) => shells.extend(mp.0),
                _ => {}
            }
        }
        if shells.is_empty() {
            return Geometry::MultiPolygon(MultiPolygon::new(Vec::new()));
        }
        if shells.len() == 1 {
            return enforce_ogc_winding(Geometry::Polygon(shells.pop().expect("len==1 verified")));
        }
        let mp = MultiPolygon::new(shells);
        // Fast-path: already valid, return unchanged (idempotency)
        if is_valid_with_geo(&Geometry::MultiPolygon(mp.clone())) {
            return enforce_ogc_winding(Geometry::MultiPolygon(mp));
        }
        // Even-parent filter: prevent NestedHoles from unary_union by removing
        // shells that are fully contained inside larger shells.
        let filtered = crate::structure::merge::merge_shells(mp.0);
        if filtered.0.len() <= 1 {
                    return if filtered.0.is_empty() {
                        empty_geom::<f64>()
                    } else {
                        // Keep MultiPolygon type for multi input
                        enforce_ogc_winding(Geometry::MultiPolygon(filtered))
                    };
                }
        let mp = filtered;
                // Check if shells have overlapping bboxes - if not, unary_union is overkill
                let shells_overlap = shells_have_overlapping_bboxes(&mp);
                let result = if !shells_overlap {
                    enforce_ogc_winding(Geometry::MultiPolygon(mp))
                } else {
                    let unioned = geo::algorithm::bool_ops::unary_union(&mp);
                    // Accept if valid AND no vertex containment (partial overlap w/o edge crossing)
                    if is_valid_with_geo(&Geometry::MultiPolygon(unioned.clone()))
                        && !shells_have_vertex_inside(&unioned)
                    {
                        enforce_ogc_winding(Geometry::MultiPolygon(unioned))
                    } else {
                        warn!("MultiPolygon: unary_union invalid, retrying with precision reduction");
                        let scales = [1e-8, 1e-6, 1e-4, 1e-2];
                        let mut best = None;
                        for &scale in &scales {
                            let snapped = reduce_mp_at_scale(&mp, config, scale);
                            let re_union = geo::algorithm::bool_ops::unary_union(&snapped);
                            let re_valid = is_valid_with_geo(&Geometry::MultiPolygon(re_union.clone()))
                                && !shells_have_vertex_inside(&re_union);
                            if re_valid {
                                best = Some(enforce_ogc_winding(Geometry::MultiPolygon(re_union)));
                                break;
                            }
                            if best.is_none() {
                                best = Some(enforce_ogc_winding(Geometry::MultiPolygon(re_union)));
                            }
                        }
                        // If all retries failed, clean union output with drop_nested_components
                        // Use the best (last) retry result to avoid another union call.
                        let unioned = best.take()
                            .map(|g| match g { Geometry::MultiPolygon(mp) => mp, _ => MultiPolygon::new(Vec::new()) })
                            .unwrap_or_else(|| geo::algorithm::bool_ops::unary_union(&mp));
                        drop_nested_components(unioned)
                    }
                };
                // MultiPolygon input → prefer MultiPolygon output type (GEOS/JTS convention
                // for multi-component repair, even when union collapses to one shell).
                match result {
                    Geometry::Polygon(p) => {
                        Geometry::MultiPolygon(MultiPolygon::new(vec![p]))
                    }
                    other => other,
                }
            }

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        crate::parallel::par_fix_multi_polygon(self, config)
    }
}

// ---------------------------------------------------------------------------
// Geometry + GeometryCollection
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Geometry<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let geom = match self {
            Geometry::Point(g) => g.make_valid_with_config(config),
            Geometry::Line(g) => g.make_valid_with_config(config),
            Geometry::LineString(g) => g.make_valid_with_config(config),
            Geometry::Polygon(g) => g.make_valid_with_config(config),
            Geometry::MultiPoint(g) => g.make_valid_with_config(config),
            Geometry::MultiLineString(g) => g.make_valid_with_config(config),
            Geometry::MultiPolygon(g) => g.make_valid_with_config(config),
            Geometry::GeometryCollection(g) => g.make_valid_with_config(config),
            Geometry::Rect(g) => g.make_valid_with_config(config),
            Geometry::Triangle(g) => g.make_valid_with_config(config),
        };
        let geom = strip_degenerate(geom);
        apply_target_crs(geom, config)
    }

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let geom = match self {
            Geometry::Point(g) => g.par_make_valid_with_config(config),
            Geometry::Line(g) => g.par_make_valid_with_config(config),
            Geometry::LineString(g) => g.par_make_valid_with_config(config),
            Geometry::Polygon(g) => g.par_make_valid_with_config(config),
            Geometry::MultiPoint(g) => g.par_make_valid_with_config(config),
            Geometry::MultiLineString(g) => g.par_make_valid_with_config(config),
            Geometry::MultiPolygon(g) => g.par_make_valid_with_config(config),
            Geometry::GeometryCollection(g) => g.par_make_valid_with_config(config),
            Geometry::Rect(g) => g.par_make_valid_with_config(config),
            Geometry::Triangle(g) => g.par_make_valid_with_config(config),
        };
        let geom = strip_degenerate(geom);
        apply_target_crs(geom, config)
    }
}

/// Post-repair: transform to target CRS if configured.
pub fn drop_nested_components(mp: MultiPolygon<f64>) -> Geometry<f64> {
    if mp.0.len() <= 1 {
        return if mp.0.is_empty() { empty_geom::<f64>() }
               else { enforce_ogc_winding(Geometry::Polygon(mp.0.into_iter().next().unwrap())) };
    }
    let mut with_area: Vec<(Polygon<f64>, f64)> = mp.0.into_iter()
        .map(|p| { let a = shoelace_abs_sum(&p.exterior().0); (p, a) })
        .collect();
    with_area.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let n = with_area.len();
    let mut keep: Vec<bool> = vec![true; n];
    for i in 0..n {
        let ext_i = &with_area[i].0.exterior().0;
        if ext_i.len() < 4 { keep[i] = false; continue; }
        // Interior probes: first vertex, first-edge midpoint nudged toward
        // the interior, and a mid-edge probe. The vertex MEAN is NOT a safe
        // candidate for concave faces (can land outside, inside a neighbor -
        // false "nested" flag). Edge-midpoint probes are always interior.
        let (v0, v1) = (ext_i[0], ext_i[1]);
        let edge_mid = Coord {
            x: (v0.x + v1.x) * 0.5,
            y: (v0.y + v1.y) * 0.5,
        };
        let scale = edge_mid.x.abs().max(edge_mid.y.abs()).max(1.0);
        let eps = 1e-9 * scale;
        let dx = v1.x - v0.x;
        let dy = v1.y - v0.y;
        let len = (dx * dx + dy * dy).sqrt().max(1e-12);
        // Nudge toward the ring interior: for CCW rings that is the LEFT
        // side; compute signed area to pick the correct side.
        let mut sa = 0.0;
        for w in ext_i.windows(2) {
            sa += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        let (nx, ny) = (-dy / len, dx / len);
        let sign = if sa >= 0.0 { 1.0 } else { -1.0 };
        let interior_probe = Coord {
            x: edge_mid.x + nx * eps * sign,
            y: edge_mid.y + ny * eps * sign,
        };
        let pt_candidates = [ext_i[0], edge_mid, interior_probe];
        let is_nested = with_area.iter().enumerate().any(|(j, (p_j, _))| {
            if i == j || !keep[j] { return false; }
            let ext_j = &p_j.exterior().0;
            if ext_j.len() < 4 { return false; }
            // Hole-aware nesting: a component is only nested when it lies in
            // another component's FILL (exterior minus holes). An island
            // inside another component's HOLE is positive space and must be
            // kept - checking only the exterior ring drops it (measured:
            // square-with-hole + island → island 64 lost; GEOS keeps it).
            // The hole test uses the strictly-interior probe: vertex/edge
            // probes lie ON the hole ring for islands that touch it, and
            // exclusive semantics misread that as fill (island converted
            // to a hole, area lost).
            let in_ext = pt_candidates
                .iter()
                .any(|&pt| point_in_ring_exclusive_even_odd(pt, ext_j));
            in_ext
                && !p_j
                    .interiors()
                    .iter()
                    .any(|h| point_in_ring_exclusive_even_odd(interior_probe, &h.0))
        });
        if is_nested { keep[i] = false; }
    }
    let kept: Vec<Polygon<f64>> = with_area
        .iter()
        .enumerate()
        .filter_map(|(i, (p, _))| if keep[i] { Some(p.clone()) } else { None })
        .collect();
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_DN").is_ok() {
        use geo::Area;
        let t: f64 = kept.iter().map(|p| p.unsigned_area()).sum();
        eprintln!("DIAG_DN: kept={} total={t:.4}", kept.len());
    }
    if kept.is_empty() { return empty_geom::<f64>(); }
    let kept_len = kept.len();
    if kept_len == 1 {
        return enforce_ogc_winding(Geometry::Polygon(kept.into_iter().next().unwrap()));
    }
    let mp_kept = MultiPolygon::new(kept);
    // If the components are already valid (disjoint or vertex-touching only -
    // the normal case for BuildArea/symdiff output), return them as-is. The
    // polygonizer fallback is ONLY for genuinely invalid MultiPolygons
    // (edge-sharing components / nested holes); re-polygonizing valid shells
    // re-expands the whole face decomposition (measured: 5 shells → 9 faces).
    //
    // The gate MUST be winding-insensitive: merge_shells emits GEOS walker
    // winding (CW shells), which OUR GeoValidation rejects as WrongOrientation
    // (orientation is normalized later by enforce_ogc_winding). Using our
    // validator here sent valid merged output into the polygonizer fallback,
    // which re-expanded faces into edge-sharing components → SelfIntersection
    // (measured: 3 valid comps → 4 comps with SI on seed a27dfba6).
    if geo::algorithm::Validation::is_valid(&mp_kept) {
        return enforce_ogc_winding(Geometry::MultiPolygon(mp_kept));
    }
    // Edge-sharing case: containment didn't reduce components.
    // Try polygonizer fallback to split edge-sharing components.
    #[cfg(feature = "arrange")]
    {
        if let Some(g) = polygonizer_fallback(&mp_kept) {
            return strip_degenerate(g);
        }
    }
    // Polygonizer failed - filter out components with PinchPoint,
    // RepeatedPoint, or other remaining errors.
    let valid: Vec<Polygon<f64>> = mp_kept.0.into_iter().filter(|p| {
        let v = crate::validation::GeoValidation::validate(p);
        !v.errors.iter().any(|e| matches!(e,
            crate::validation::GeometryValidationError::PinchPoint
            | crate::validation::GeometryValidationError::RepeatedPoint
        )) && v.errors.iter().filter(|e| !matches!(e,
            crate::validation::GeometryValidationError::PinchPoint
            | crate::validation::GeometryValidationError::RepeatedPoint
            | crate::validation::GeometryValidationError::NestedHoles
        )).count() == 0
    }).collect();
    if valid.is_empty() {
        return empty_geom::<f64>();
    }
    if valid.len() == 1 {
        return enforce_ogc_winding(Geometry::Polygon(valid.into_iter().next().unwrap()));
    }
    enforce_ogc_winding(Geometry::MultiPolygon(MultiPolygon::new(valid)))
}

/// Polygonizer fallback for edge-sharing MultiPolygon components
/// that containment-based drop_nested_components can't handle.
pub(super) fn polygonizer_fallback(mp: &MultiPolygon<f64>) -> Option<Geometry<f64>> {
    use geo::LinesIter;
    let lines: Vec<geo::Line<f64>> = mp.0.iter()
        .flat_map(|p| p.lines_iter())
        .collect();
    if lines.is_empty() { return None; }
    // GEOS BuildArea: correct face extraction + shell/hole classification +
    // even-parent. (The legacy polygonizer misclassifies multi-shell inputs:
    // measured 1 poly with 6 holes instead of 5 disjoint shells.)
    let area = crate::structure::build_area::build_area(&lines)?;
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_PF").is_ok() {
        use geo::Area;
        eprintln!("PF: lines={} build_area -> {} polys", lines.len(), area.0.len());
        for (i, p) in area.0.iter().enumerate() {
            eprintln!("PF:   [{i}] area={:.4} holes={}", p.unsigned_area(), p.interiors().len());
        }
    }
    let valid: Vec<Polygon<f64>> = area.0.into_iter()
        .filter(|p| {
            let ext = &p.exterior().0;
            ext.len() >= 4 && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                && shoelace_abs_sum(ext) >= 1e-12
                // Proper-crossing check only: hole/shell vertex touches are
                // legal (GEOS makeValid emits them); only genuine crossings
                // disqualify a component.
                && !crate::structure::has_proper_self_crossing(p)
        })
        .collect();
    if valid.is_empty() { return None; }
    if valid.len() == 1 { return Some(Geometry::Polygon(valid.into_iter().next().unwrap())); }
    Some(Geometry::MultiPolygon(MultiPolygon::new(valid)))
}

/// Snap all coordinates in a MultiPolygon to the default precision grid (1e-8).
#[cfg(any(feature = "arrange", feature = "structure"))]
#[allow(dead_code)]
pub(super) fn reduce_mp(mp: &MultiPolygon<f64>, config: &MakeValidConfig) -> MultiPolygon<f64> {
    reduce_mp_at_scale(mp, config, 1e-8)
}

/// Snap all coordinates in a MultiPolygon to a specific precision grid scale.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn reduce_mp_at_scale(mp: &MultiPolygon<f64>, config: &MakeValidConfig, scale: f64) -> MultiPolygon<f64> {
    use crate::reduce::{GeometryPrecisionReducer, PrecisionModel};
    let model = PrecisionModel::new(scale);
    let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
    let snapped: Vec<Polygon<f64>> = mp
        .0
        .iter()
        .map(|p| {
            let g = reducer.reduce_raw(p);
            match g {
                Geometry::Polygon(poly) => poly,
                Geometry::MultiPolygon(mp) => {
                    mp.0.into_iter().next().unwrap_or_else(|| {
                        Polygon::new(LineString::new(Vec::new()), Vec::new())
                    })
                }
                _ => Polygon::new(LineString::new(Vec::new()), Vec::new()),
            }
        })
        .collect();
    MultiPolygon::new(snapped)
}

// ---------------------------------------------------------------------------
// GeometryCollection
// ---------------------------------------------------------------------------

//! MultiPolygon/Geometry repair: union + even-parent filtering, nested component
//! dropping, and precision-reduction fallbacks.

use super::polygon::has_nan_or_infinite;
use super::polygon::{
    apply_target_crs, enforce_ogc_winding, is_valid_with_geo, shells_have_overlapping_bboxes,
    shells_have_vertex_inside,
};
use super::strip::strip_degenerate;
use super::*;
use alloc::vec::Vec;

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
            // still gated (fuzz wkb seed 1da58cf8 - the component repair shipped a
            // genuinely crossing polygon and this early return skipped) Returned
            // directly instead of through `repair`
            let single = shells.pop().expect("len==1 verified");
            let enforced = enforce_ogc_winding(Geometry::Polygon(single)).0;
            let orig = match &enforced {
                Geometry::Polygon(p) => vec![p.clone()],
                _ => Vec::new(),
            };
            #[cfg(feature = "std")]
            let gated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                validity_gate(enforced, orig)
            }))
            .unwrap_or_else(|_| {
                warn!("MultiPolygon single-survivor gate panicked; returning empty geometry");
                empty_geom::<f64>()
            });
            #[cfg(not(feature = "std"))]
            let gated = validity_gate(enforced, orig);
            return gated;
        }
        // Panic containment: everything from the merge to the final union/gate result
        // runs under one catch_unwind below. unary_union (i_overlay debug_asserts on
        // degenerate merges - fuzz wkt seeds aa4bb7d2 / 455657db, extract_ogc
        // is_fill_top) and the arrange ladder inside
        let repair = || -> Geometry<f64> {
            let mp = MultiPolygon::new(shells);
            if is_valid_with_geo(&Geometry::MultiPolygon(mp.clone())) {
                return enforce_ogc_winding(Geometry::MultiPolygon(mp)).0;
            }
            let orig_shells = mp.0.clone();
            let filtered = crate::structure::merge::merge_shells(mp.0);
            if filtered.0.is_empty() {
                return empty_geom::<f64>();
            }
            let mp = filtered;
            let shells_overlap = shells_have_overlapping_bboxes(&mp);
            let mut certified = false;
            let result = if !shells_overlap {
                enforce_ogc_winding(Geometry::MultiPolygon(mp)).0
            } else {
                let unioned = geo::algorithm::bool_ops::unary_union(&mp);
                if is_valid_with_geo(&Geometry::MultiPolygon(unioned.clone()))
                    && !shells_have_vertex_inside(&unioned)
                {
                    certified = true;
                    enforce_ogc_winding(Geometry::MultiPolygon(unioned)).0
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
                            best = Some(enforce_ogc_winding(Geometry::MultiPolygon(re_union)).0);
                            certified = true;
                            break;
                        }
                        if best.is_none() {
                            best = Some(enforce_ogc_winding(Geometry::MultiPolygon(re_union)).0);
                        }
                    }
                    if certified {
                        best.expect("certified implies a retry result")
                    } else {
                        let unioned = best
                            .take()
                            .map(|g| match g {
                                Geometry::MultiPolygon(mp) => mp,
                                _ => MultiPolygon::new(Vec::new()),
                            })
                            .unwrap_or_else(|| geo::algorithm::bool_ops::unary_union(&mp));
                        drop_nested_components(unioned)
                    }
                }
            };
            if certified {
                result
            } else {
                validity_gate(result, orig_shells)
            }
        };
        #[cfg(feature = "std")]
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(repair)).unwrap_or_else(|_| {
                warn!("MultiPolygon make_valid panicked; returning empty geometry");
                empty_geom::<f64>()
            });
        #[cfg(not(feature = "std"))]
        let result = repair();
        match result {
            Geometry::Polygon(p) => Geometry::MultiPolygon(MultiPolygon::new(vec![p])),
            other => other,
        }
    }

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        crate::parallel::par_fix_multi_polygon(self, config)
    }
}


/// The validator rejects that as `DisconnectedInteriorRing`. Seeds `3111ecaf` and
/// `edf6e65b`.
fn validity_gate(g: Geometry<f64>, orig: Vec<Polygon<f64>>) -> Geometry<f64> {
    use geo::Area;
    if is_valid_with_geo(&g) {
        return g;
    }
    let mp = match g {
        Geometry::MultiPolygon(mp) => mp,
        Geometry::Polygon(p) => MultiPolygon::new(vec![p]),
        other => return other,
    };
    #[cfg(feature = "arrange")]
    {
        for cand in [mp.clone(), MultiPolygon::new(orig.clone())] {
            if let Some(built) = polygonizer_fallback(&cand).filter(is_valid_with_geo) {
                return built;
            }
        }
        for cand in [mp.clone(), MultiPolygon::new(orig.clone())] {
            if let Some(kept) = keep_valid_components(cand).filter(is_valid_with_geo) {
                return kept;
            }
        }
    }
    warn!("MultiPolygon: merged output invalid, degrading to the largest valid component");
    let mut best: Option<Polygon<f64>> = None;
    let mut best_area = 0.0f64;
    for p in orig.into_iter().chain(mp.0) {
        if is_valid_with_geo(&Geometry::Polygon(p.clone())) {
            let a = p.unsigned_area();
            if a > best_area {
                best_area = a;
                best = Some(p);
            }
        }
    }
    match best {
        Some(p) => Geometry::Polygon(p),
        None => empty_geom::<f64>(),
    }
}

/// Drop components that are invalid on their own; `None` when nothing usable is left,
/// so the caller can try the next candidate instead of returning empty.
#[cfg(feature = "arrange")]
fn keep_valid_components(mp: MultiPolygon<f64>) -> Option<Geometry<f64>> {
    let kept: Vec<Polygon<f64>> =
        mp.0.into_iter()
            .filter(|p| is_valid_with_geo(&Geometry::Polygon(p.clone())))
            .collect();
    match kept.len() {
        0 => None,
        1 => Some(Geometry::Polygon(kept.into_iter().next().expect("len==1"))),
        _ => Some(Geometry::MultiPolygon(MultiPolygon::new(kept))),
    }
}


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

/// Winding-insensitive probe for merged MultiPolygons. It replaces
/// `geo::algorithm::Validation::is_valid` at both gates. perf: geo relate takes 30 to
/// 45 s on MultiPolygons. They hold 1100 to 2000 components and 50 to 77k vertices.
/// Seeds aa3d69dc and 251df359 timed out at 1200 s, 97% of it in this call. The
/// Shewchuk validator takes 100 ms. safety: geo relate debug_assert!s panic on
/// adversarial input (edge_end_bundle_star.rs:116, run 35836311392). merge_shells emits
/// GEOS-walker CW shells that read as WrongOrientation. A winding-only rejection sent
/// valid output to the polygonizer, which returned SelfIntersection (a27dfba6).
pub(crate) fn mp_looks_valid(mp: &MultiPolygon<f64>) -> bool {
    is_valid_with_geo(&enforce_ogc_winding(Geometry::MultiPolygon(mp.clone())).0)
}

/// Post-repair: transform to target crs if configured.
pub fn drop_nested_components(mp: MultiPolygon<f64>) -> Geometry<f64> {
    if mp.0.len() <= 1 {
        return if mp.0.is_empty() {
            empty_geom::<f64>()
        } else {
            enforce_ogc_winding(Geometry::Polygon(mp.0.into_iter().next().unwrap())).0
        };
    }
    let mut with_area: Vec<(Polygon<f64>, f64)> =
        mp.0.into_iter()
            .map(|p| {
                let a = shoelace_abs_sum(&p.exterior().0);
                (p, a)
            })
            .collect();
    with_area.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
    let n = with_area.len();
    let mut keep: Vec<bool> = vec![true; n];
    for i in 0..n {
        let ext_i = &with_area[i].0.exterior().0;
        if ext_i.len() < 4 {
            keep[i] = false;
            continue;
        }
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
            if i == j || !keep[j] {
                return false;
            }
            let ext_j = &p_j.exterior().0;
            if ext_j.len() < 4 {
                return false;
            }
            let in_ext = pt_candidates
                .iter()
                .any(|&pt| point_in_ring_exclusive_even_odd(pt, ext_j));
            in_ext
                && !p_j
                    .interiors()
                    .iter()
                    .any(|h| point_in_ring_exclusive_even_odd(interior_probe, &h.0))
        });
        if is_nested {
            keep[i] = false;
        }
    }
    let kept: Vec<Polygon<f64>> = with_area
        .iter()
        .enumerate()
        .filter_map(|(i, (p, _))| if keep[i] { Some(p.clone()) } else { None })
        .collect();
    #[cfg(all(any(test, debug_assertions), feature = "std"))]
    if std::env::var("DIAG_DN").is_ok() {
        use geo::Area;
        let t: f64 = kept.iter().map(|p| p.unsigned_area()).sum();
        eprintln!("DIAG_DN: kept={} total={t:.4}", kept.len());
    }
    if kept.is_empty() {
        return empty_geom::<f64>();
    }
    let kept_len = kept.len();
    if kept_len == 1 {
        return enforce_ogc_winding(Geometry::Polygon(kept.into_iter().next().unwrap())).0;
    }
    let mp_kept = MultiPolygon::new(kept);
    // SelfIntersection (measured: 3 valid comps → 4 comps with SI on seed a27dfba6) -
    // hence `mp_looks_valid`
    let geo_valid = mp_looks_valid(&mp_kept);
    if geo_valid {
        return enforce_ogc_winding(Geometry::MultiPolygon(mp_kept)).0;
    }
    #[cfg(feature = "arrange")]
    {
        if let Some(g) = polygonizer_fallback(&mp_kept) {
            return strip_degenerate(g);
        }
    }
    let valid: Vec<Polygon<f64>> = mp_kept
        .0
        .into_iter()
        .filter(|p| {
            let v = crate::validation::GeoValidation::validate(p);
            !v.errors.iter().any(|e| {
                matches!(
                    e,
                    crate::validation::GeometryValidationError::PinchPoint
                        | crate::validation::GeometryValidationError::RepeatedPoint
                )
            }) && v
                .errors
                .iter()
                .filter(|e| {
                    !matches!(
                        e,
                        crate::validation::GeometryValidationError::PinchPoint
                            | crate::validation::GeometryValidationError::RepeatedPoint
                            | crate::validation::GeometryValidationError::NestedHoles
                    )
                })
                .count()
                == 0
        })
        .collect();
    if valid.is_empty() {
        return empty_geom::<f64>();
    }
    if valid.len() == 1 {
        return enforce_ogc_winding(Geometry::Polygon(valid.into_iter().next().unwrap())).0;
    }
    enforce_ogc_winding(Geometry::MultiPolygon(MultiPolygon::new(valid))).0
}

/// Polygonizer fallback for edge-sharing MultiPolygon components that containment-based
/// drop_nested_components cannot handle.
pub(super) fn polygonizer_fallback(mp: &MultiPolygon<f64>) -> Option<Geometry<f64>> {
    use geo::LinesIter;
    let lines: Vec<geo::Line<f64>> = mp.0.iter().flat_map(|p| p.lines_iter()).collect();
    if lines.is_empty() {
        return None;
    }
    let area = crate::structure::build_area::build_area(&lines)?;
    #[cfg(all(any(test, debug_assertions), feature = "std"))]
    if std::env::var("DIAG_PF").is_ok() {
        use geo::Area;
        eprintln!(
            "PF: lines={} build_area -> {} polys",
            lines.len(),
            area.0.len()
        );
        for (i, p) in area.0.iter().enumerate() {
            eprintln!(
                "PF:   [{i}] area={:.4} holes={}",
                p.unsigned_area(),
                p.interiors().len()
            );
        }
    }
    let valid: Vec<Polygon<f64>> = area
        .0
        .into_iter()
        .filter(|p| {
            let ext = &p.exterior().0;
            ext.len() >= 4 && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                && shoelace_abs_sum(ext) >= 1e-12
                && !crate::structure::has_proper_self_crossing(p)
        })
        .collect();
    if valid.is_empty() {
        return None;
    }
    if valid.len() == 1 {
        return Some(Geometry::Polygon(valid.into_iter().next().unwrap()));
    }
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
pub(super) fn reduce_mp_at_scale(
    mp: &MultiPolygon<f64>,
    config: &MakeValidConfig,
    scale: f64,
) -> MultiPolygon<f64> {
    use crate::reduce::{GeometryPrecisionReducer, PrecisionModel};
    let model = PrecisionModel::new(scale);
    let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
    let snapped: Vec<Polygon<f64>> =
        mp.0.iter()
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


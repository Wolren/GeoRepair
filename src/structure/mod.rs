//! It is 10 to 100x faster than cdt on valid or simple input and needs only geo.

use alloc::vec::Vec;
/// GEOS BuildArea port: polygonize linework → shell/hole classification → even-parent
/// filter.
pub mod build_area;
/// Edge classification and planar graph building for polygon faces.
pub mod classify;
/// Edge splitting at intersection points (R-tree / sweep / brute force).
pub(crate) mod edge_split;
/// Ring repair: self-intersection resolution, winding correction.
pub mod fix_ring;
/// Graph-based ring intersection resolution.
pub mod fix_ring_graph;
/// Face merging after planar graph extraction.
pub mod merge;
/// Topological face extraction from noded edge sets (BuildArea).
pub mod polygonizer;
/// Hole subtraction during polygon face assembly.
pub mod subtract;
/// Plane-sweep intersection detection for edge segments.
pub mod sweep;
/// GEOS MakeValidPoly symdiff loop (BuildArea + xor accumulation).
pub(crate) mod symdiff;

use ::core::sync::atomic::Ordering;
use geo::{Geometry, Line, LineString, LinesIter, Polygon};
use smallvec::SmallVec;

use crate::core::MakeValidConfig;
use crate::util;
use log::warn;

/// The hot batch paths use the owned variant to avoid the clone (measured: the
/// fast-path clone was ~2 allocs + full ring memcpy per polygon, the dominant per-poly
/// cost on the 1.58M-poly full pass).
pub fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    match fix_polygon_owned(poly.clone(), config, None) {
        FixOutcome::Fast(g, _, _, _) | FixOutcome::Repaired(g) => Some(g),
        FixOutcome::Unconsumed(_) => None,
    }
}

/// Distinguishes the zero-copy fast path (passthrough. Provably non-degenerate and
/// NaN-free. The caller can skip the redundant `strip_degenerate`/`has_nan` passes)
/// from rebuilt geometry that still needs those checks.
pub(crate) enum FixOutcome {
    /// * scale (checked by the caller), NaN-free.
    Fast(Geometry<f64>, f64, f64, Vec<usize>),
    /// Rebuilt geometry (repair / fallback paths).
    Repaired(Geometry<f64>),
    /// Structure repair produced nothing; the polygon is returned unconsumed for the
    /// caller's precision-reduction fallback.
    Unconsumed(Polygon<f64>),
}

/// About 99.85% of real-world polygons are already valid. This is a zero-copy
/// passthrough that matches the geos shared-geometry return. ext_scale is the caller
/// precomputed exterior bbox scale, the max spread floored at 1.0.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn fix_polygon_owned(
    poly: Polygon<f64>,
    config: &MakeValidConfig,
    ext_scale: Option<f64>,
) -> FixOutcome {
    let _t_fp = util::ProfileClock::start();
    #[cfg(feature = "arrange")]
    {
        let mut min_abs = f64::INFINITY;
        let mut max_abs = 0.0f64;
        let mut extremal: Vec<usize> = Vec::new();
        if fast_path_check(
            &poly,
            ext_scale,
            &config.tuning,
            &mut min_abs,
            &mut max_abs,
            &mut extremal,
        ) {
            PROFILE_FP_NS.fetch_add(_t_fp.ns(), Ordering::Relaxed);
            return FixOutcome::Fast(Geometry::Polygon(poly), min_abs, max_abs, extremal);
        }
    }
    PROFILE_FP_NS.fetch_add(_t_fp.ns(), Ordering::Relaxed);

    /// It accepts exactly what the exit validator accepts, because the Fast path skips
    /// that validator (2026-08-07).
    #[cfg(feature = "arrange")]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn fast_path_check(
        poly: &Polygon<f64>,
        _ext_scale: Option<f64>,
        tuning: &crate::core::Tuning,
        min_abs: &mut f64,
        max_abs: &mut f64,
        extremal: &mut Vec<usize>,
    ) -> bool {
        let total_verts: usize =
            poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
        if total_verts == 0 || poly.exterior().0.len() < 4 {
            return false;
        }
        // extreme-span gate (crash-65111cd0 guard, 2026-08-09) does not
        let mut bbox = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        let (mut sub_ulp, ext_min_abs) = crate::arrange::ring_guards(&poly.exterior().0);
        *min_abs = (*min_abs).min(ext_min_abs);
        for hole in poly.interiors() {
            let (hole_sub_ulp, hole_min_abs) = crate::arrange::ring_guards(&hole.0);
            sub_ulp |= hole_sub_ulp;
            *min_abs = (*min_abs).min(hole_min_abs);
        }
        if sub_ulp {
            return false;
        }
        let collect_lines = total_verts > tuning.small_ring_lines;
        // Vec::new pays three or four reallocations over a 10000-vertex ring (measured
        // 2026-09-25: 5.7 us of growth alone).
        let mut lines: Vec<Line<f64>> = if collect_lines {
            Vec::with_capacity(total_verts)
        } else {
            Vec::new()
        };
        // walk (one pass instead of walk + build_mono_chains).
        let fused_chains =
            total_verts > tuning.small_ring_lines && total_verts <= tuning.fast_path_max_verts;
        let mut sink = crate::arrange::prep_intersect::ChainSink::default();
        let mut acc = crate::arrange::GateAccum {
            lines: collect_lines.then_some(&mut lines),
            chains: fused_chains.then_some(&mut sink),
            // push_line measured slower (interleaved A/B 2026-09-25: 3/3 rounds, 260.1
            // vs 247.1 us median at 10000v).
            bbox: (!fused_chains).then_some(&mut bbox),
            extremal: Some(extremal),
        };
        if !crate::arrange::ring_is_plausible(poly.exterior(), &mut acc) {
            return false;
        }
        for hole in poly.interiors() {
            if !crate::arrange::ring_is_plausible(hole, &mut acc) {
                return false;
            }
        }
        let fused = if fused_chains {
            Some(sink.finish())
        } else {
            None
        };
        // rule (correct for mixed magnitudes - a 5e-305-thick sliver is valid at its
        // own scale)
        let envelope = fused.as_ref().map_or(bbox, |(_, b)| *b);
        *max_abs = envelope
            .0
            .abs()
            .max(envelope.1.abs())
            .max(envelope.2.abs())
            .max(envelope.3.abs());
        if (envelope.1 - envelope.0).abs() < f64::EPSILON
            || (envelope.3 - envelope.2).abs() < f64::EPSILON
        {
            return false;
        }
        let interiors: Vec<&[geo::Coord<f64>]> =
            poly.interiors().iter().map(|h| &h.0[..]).collect();
        if crate::validation::has_duplicate_rings(&interiors, poly.exterior().0.as_slice()) {
            return false;
        }
        if total_verts <= tuning.fast_path_max_verts {
            // SmallVec: ~95.6% of the real-world dataset has <= 32 vertices
            if total_verts <= tuning.small_ring_lines {
                let lines: SmallVec<[Line<f64>; crate::core::SMALL_RING_LINES]> =
                    poly.lines_iter().collect();
                !lines.is_empty()
                    && crate::arrange::prep::has_no_intersections_nan_ok_tuned(&lines, tuning)
                    && crate::validation::holes::check_holes_valid(
                        poly.exterior().0.as_slice(),
                        poly.interiors(),
                    )
                    .is_empty()
            } else {
                match fused {
                    Some((chains, global_bbox)) => {
                        !lines.is_empty()
                            && crate::arrange::prep::has_no_intersections_from_chains(
                                &lines,
                                &chains,
                                global_bbox,
                                tuning,
                            )
                            && crate::validation::holes::check_holes_valid(
                                poly.exterior().0.as_slice(),
                                poly.interiors(),
                            )
                            .is_empty()
                    }
                    None => false,
                }
            }
        } else {
            // that is ~11ms per giant ring
            use crate::validation::GeometryValidationError;
            if crate::validation::check_ring_validity(poly.exterior().0.as_slice(), true)
                .iter()
                .any(|e| !matches!(e, GeometryValidationError::WrongOrientation))
            {
                return false;
            }
            for hole in poly.interiors() {
                if crate::validation::check_ring_validity(&hole.0, false)
                    .iter()
                    .any(|e| !matches!(e, GeometryValidationError::WrongOrientation))
                {
                    return false;
                }
            }
            crate::arrange::holes_are_valid_inclusive(poly)
        }
    }

    if crate::make_valid::snap_cannot_represent(&poly) {
        if let Some(outcome) = crate::make_valid::precondition::precondition_fix(&poly, config) {
            return outcome;
        }
        return FixOutcome::Unconsumed(poly);
    }
    if let Some(mp) = crate::structure::symdiff::single_pass_fix_tuned(&poly, &config.tuning) {
        let geom = if mp.0.len() == 1 {
            Geometry::Polygon(mp.0.into_iter().next().expect("len==1 verified"))
        } else {
            Geometry::MultiPolygon(mp)
        };
        let g = crate::make_valid::enforce_ogc_winding(geom).0;
        if crate::make_valid::is_valid_with_geo(&g) {
            return FixOutcome::Repaired(g);
        }
    }

    // Neither the i_overlay difference (returns a single quantized ring with 1e-9-grid
    // node artifacts. Measured: hole vertex split into two nodes)
    #[cfg(feature = "arrange")]
    {
        let shell_bbox = ring_bbox(poly.exterior().0.as_slice());
        let crossing = poly.interiors().iter().any(|h| {
            let hb = ring_bbox(&h.0);
            if hb.0 >= shell_bbox.0
                && hb.1 <= shell_bbox.1
                && hb.2 >= shell_bbox.2
                && hb.3 <= shell_bbox.3
            {
                return false;
            }
            if h.0.len() * poly.exterior().0.len() > 4096 {
                return false;
            }
            hole_vertex_strictly_outside(h, poly.exterior())
        });
        if crossing {
            return FixOutcome::Repaired(crate::arrange::fallback_polygon_fix(&poly));
        }
    }

    let shell_bbox = ring_bbox(poly.exterior().0.as_slice());

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    let (valid_shells, hole_rings_cw) = {
        use rayon::prelude::*;
        let (shell_res, holes) = rayon::join(
            || {
                let _t = util::ProfileClock::start();
                let shell_polys = match fix_ring::repair_ring_tuned(poly.exterior(), &config.tuning)
                {
                    Some(polys) => polys,
                    None => return None,
                };
                if shell_polys.is_empty() {
                    return None;
                }
                // Polygons carry structural holes from BuildArea (self-crossing lobes):
                // they must survive into the subtract step.
                let valid: Vec<Polygon<f64>> = shell_polys
                    .into_iter()
                    .filter(|p| p.exterior().0.len() >= 4)
                    .collect();
                PROFILE_SR_NS.fetch_add(_t.ns(), Ordering::Relaxed);
                if valid.is_empty() { None } else { Some(valid) }
            },
            || {
                let _t = util::ProfileClock::start();
                let mut hole_results: Vec<Vec<LineString<f64>>> = poly
                    .interiors()
                    .par_iter()
                    .map(|h| {
                        let hole_bbox = ring_bbox(&h.0);
                        if !bboxes_overlap(shell_bbox, hole_bbox) {
                            return vec![h.clone()];
                        }
                        if !fix_ring::has_self_intersections_with_bbox_tuned(
                            &h.0,
                            hole_bbox,
                            &config.tuning,
                        ) {
                            return vec![h.clone()];
                        }
                        fix_ring::repair_ring_tuned(h, &config.tuning)
                            .map(|polys| polys.into_iter().map(|p| p.exterior().clone()).collect())
                            .unwrap_or_else(|| vec![h.clone()])
                    })
                    .collect();
                PROFILE_HR_NS.fetch_add(_t.ns(), Ordering::Relaxed);
                hole_results
                    .iter_mut()
                    .flat_map(|rings| rings.drain(..))
                    .map(ensure_cw)
                    .collect::<Vec<_>>()
            },
        );
        let valid_shells = match shell_res {
            Some(v) => v,
            None => {
                warn!("Structure: shell ring repair failed, falling back to CDT arrange");
                #[cfg(feature = "arrange")]
                if !poly.exterior().0.is_empty() {
                    return FixOutcome::Repaired(crate::arrange::fallback_polygon_fix(&poly));
                }
                return FixOutcome::Repaired(
                    handle_collapse_result(poly.exterior(), config)
                        .unwrap_or_else(crate::make_valid::empty_geom),
                );
            }
        };
        (valid_shells, holes)
    };

    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    let (valid_shells, hole_rings_cw) = {
        let _t_sr = util::ProfileClock::start();
        let shell_polys = match fix_ring::repair_ring_tuned(poly.exterior(), &config.tuning) {
            Some(polys) => polys,
            None => {
                warn!("Structure: shell ring repair failed, falling back to CDT arrange");
                #[cfg(feature = "arrange")]
                if !poly.exterior().0.is_empty() {
                    return FixOutcome::Repaired(crate::arrange::fallback_polygon_fix(&poly));
                }
                return FixOutcome::Repaired(
                    handle_collapse_result(poly.exterior(), config)
                        .unwrap_or_else(crate::make_valid::empty_geom),
                );
            }
        };
        if shell_polys.is_empty() {
            return FixOutcome::Repaired(
                handle_collapse_result(poly.exterior(), config)
                    .unwrap_or_else(crate::make_valid::empty_geom),
            );
        }
        let valid_shells: Vec<Polygon<f64>> = shell_polys
            .into_iter()
            .filter(|p| p.exterior().0.len() >= 4)
            .collect();
        if valid_shells.is_empty() {
            return FixOutcome::Repaired(
                handle_collapse_result(poly.exterior(), config)
                    .unwrap_or_else(crate::make_valid::empty_geom),
            );
        }
        PROFILE_SR_NS.fetch_add(_t_sr.ns(), Ordering::Relaxed);

        let hole_rings_cw: Vec<LineString<f64>> = {
            let _t_hr = util::ProfileClock::start();
            let mut hole_rings: Vec<LineString<f64>> = Vec::new();
            for h in poly.interiors() {
                let hole_bbox = ring_bbox(&h.0);
                if !bboxes_overlap(shell_bbox, hole_bbox) {
                    hole_rings.push(ensure_cw(h.clone()));
                    continue;
                }
                if !fix_ring::has_self_intersections_with_bbox_tuned(
                    &h.0,
                    hole_bbox,
                    &config.tuning,
                ) {
                    hole_rings.push(ensure_cw(h.clone()));
                    continue;
                }
                if let Some(polys) = fix_ring::repair_ring_tuned(h, &config.tuning) {
                    hole_rings.extend(polys.into_iter().map(|p| p.exterior().clone()));
                } else {
                    hole_rings.push(ensure_cw(h.clone()));
                }
            }
            PROFILE_HR_NS.fetch_add(_t_hr.ns(), Ordering::Relaxed);
            hole_rings.into_iter().map(ensure_cw).collect()
        };
        (valid_shells, hole_rings_cw)
    };

    // The shell may already carry structural holes from BuildArea (lobes of a
    // self-crossing shell that must remain void)
    let process_shell = |shell: Polygon<f64>| -> Vec<Polygon<f64>> {
        let mut shell_poly = shell;
        let struct_holes: Vec<LineString<f64>> = shell_poly
            .interiors()
            .iter()
            .filter(|h| h.0.len() >= 4)
            .map(|h| ensure_cw(h.clone()))
            .collect();
        shell_poly = Polygon::new(ensure_ccw(shell_poly.exterior().clone()), struct_holes);

        let _t_cl = util::ProfileClock::start();
        let (inner_holes, outer_holes) =
            classify::classify_holes(shell_poly.exterior(), &hole_rings_cw);
        PROFILE_CL_NS.fetch_add(_t_cl.ns(), Ordering::Relaxed);

        let _t_nest = util::ProfileClock::start();
        let (to_subtract, islands) = resolve_nesting(&inner_holes);
        PROFILE_NEST_NS.fetch_add(_t_nest.ns(), Ordering::Relaxed);

        let inner_polys: Vec<Polygon<f64>> = to_subtract
            .into_iter()
            .map(|h| Polygon::new(h, Vec::new()))
            .collect();

        let mut local = Vec::new();
        let _t_sub = util::ProfileClock::start();
        let subtracted = subtract::subtract_holes(&shell_poly, &inner_polys);
        local.extend(subtracted.0);
        PROFILE_SUB_NS.fetch_add(_t_sub.ns(), Ordering::Relaxed);

        local.extend(islands);

        for hole in outer_holes {
            let shell_ring = shell_poly.exterior().clone();
            if shell_ring.0.len() >= 4 && all_vertices_inside_ring(&shell_ring.0, &hole.0) {
                local.push(Polygon::new(hole, vec![ensure_cw(shell_ring)]));
            } else {
                local.push(Polygon::new(hole, Vec::new()));
            }
        }
        local
    };

    let mut result_polys: Vec<Polygon<f64>> = {
        let _t_hn = util::ProfileClock::start();
        let r = {
            #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
            {
                use rayon::prelude::*;
                valid_shells
                    .into_par_iter()
                    .flat_map(process_shell)
                    .collect()
            }
            #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
            {
                valid_shells.into_iter().flat_map(process_shell).collect()
            }
        };
        PROFILE_HN_NS.fetch_add(_t_hn.ns(), Ordering::Relaxed);
        r
    };

    if result_polys.is_empty() {
        warn!("Structure: subtract/merge produced no result polygons");
        return FixOutcome::Unconsumed(poly);
    }

    #[cfg(all(any(test, debug_assertions), feature = "std"))]
    if std::env::var("DIAG_PP").is_ok() {
        use geo::Area;
        for (i, p) in result_polys.iter().enumerate() {
            eprintln!(
                "DIAG_PP result_poly[{i}]: area={:.4} holes={}",
                p.unsigned_area(),
                p.interiors().len()
            );
        }
    }

    let result = if result_polys.len() == 1 {
        let p = result_polys.pop().expect("len==1 verified");
        #[cfg(feature = "arrange")]
        {
            // (GEOS makeValid emits them) must survive.
            if has_proper_self_crossing(&p) {
                Geometry::GeometryCollection(geo::GeometryCollection(Vec::new()))
            } else {
                Geometry::Polygon(p)
            }
        }
        #[cfg(not(feature = "arrange"))]
        Geometry::Polygon(p)
    } else {
        let _t_mg = util::ProfileClock::start();
        let merged = merge::merge_shells(result_polys);
        PROFILE_MG_NS.fetch_add(_t_mg.ns(), Ordering::Relaxed);
        #[cfg(feature = "arrange")]
        {
            let g = crate::make_valid::drop_nested_components(merged);
            #[cfg(all(any(test, debug_assertions), feature = "std"))]
            if std::env::var("DIAG_STR").is_ok() {
                use geo::Area;
                let ga = match &g {
                    Geometry::Polygon(p) => p.unsigned_area(),
                    Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
                    Geometry::GeometryCollection(gc) => {
                        gc.0.iter()
                            .map(|x| match x {
                                Geometry::Polygon(p) => p.unsigned_area(),
                                _ => 0.0,
                            })
                            .sum()
                    }
                    _ => 0.0,
                };
                eprintln!("DIAG_STR: drop_nested -> {ga:.4}");
            }
            if let Geometry::MultiPolygon(ref mp) = g {
                let filtered: Vec<Polygon<f64>> =
                    mp.0.iter()
                        .filter(|p| !has_proper_self_crossing(p))
                        .cloned()
                        .collect();
                if filtered.is_empty() {
                    Geometry::GeometryCollection(geo::GeometryCollection(Vec::new()))
                } else if filtered.len() == 1 {
                    Geometry::Polygon(filtered.into_iter().next().unwrap())
                } else {
                    Geometry::MultiPolygon(geo::MultiPolygon::new(filtered))
                }
            } else {
                g
            }
        }
        #[cfg(not(feature = "arrange"))]
        crate::make_valid::drop_nested_components(merged)
    };

    FixOutcome::Repaired(result)
}

/// Hole nesting resolution, ring helpers, collapse handling (extracted 2026-08-07 for
/// file-size governance).
mod nesting;
/// Cumulative per-stage timing counters and the profile printer.
mod profile;

pub use crate::structure::nesting::*;
pub use crate::structure::profile::*;

#[cfg(all(test, feature = "arrange"))]
mod gate_tests;

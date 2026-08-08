//! GEOS-compatible fast-path polygon repair via planar graph extraction.
//!
//! The Structure strategy is the default fast path for polygon repair. It
//! mirrors GEOS's ST_MakeValid algorithm:
//!
//! 1. Build a planar graph from polygon edges
//! 2. Classify edges and extract faces
//! 3. Face walking to find ring boundaries
//! 4. Winding-number assembly into OGC-valid output
//!
//! Strengths:
//! - 10-100x faster than CDT-based approaches on valid/simple inputs
//! - No external dependencies beyond `geo`
//! - Handles the vast majority of real-world invalid polygons
//!
//! Falls back to the Arrange strategy when the topology is too complex
//! (many holes, nested self-intersections).
//!
//! # Submodules
//!
//! - `classify`: Edge classification and planar graph building
//! - `fix_ring`: Ring repair (self-intersection, winding correction)
//! - `fix_ring_graph`: Graph-based ring intersection resolution
//! - `merge`: Face merging after graph extraction
//! - `subtract`: Hole subtraction during face assembly
//! - `sweep`: Plane-sweep intersection detection

use alloc::vec::Vec;
/// Edge classification and planar graph building for polygon faces.
pub mod classify;
/// GEOS BuildArea port: polygonize linework → shell/hole classification →
/// even-parent filter. The reference area-building algorithm.
pub mod build_area;
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
/// Edge splitting at intersection points (R-tree / sweep / brute force).
pub(crate) mod edge_split;
/// GEOS MakeValidPoly symdiff loop (BuildArea + XOR accumulation).
pub(crate) mod symdiff;
/// Plane-sweep intersection detection for edge segments.
pub mod sweep;

use geo::{Geometry, Line, LineString, LinesIter, Polygon};
use smallvec::SmallVec;
use ::core::sync::atomic::Ordering;

use crate::core;
use crate::core::MakeValidConfig;
use crate::util;
use log::warn;

// ── Profiling counters (cumulative ns) ──

/// Borrowed convenience wrapper: clones the polygon and delegates to
/// `fix_polygon_owned`. The hot batch paths use the owned variant to avoid
/// the clone (measured: the fast-path clone was ~2 allocs + full ring memcpy
/// per polygon, the dominant per-poly cost on the 1.58M-poly full pass).
pub fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    match fix_polygon_owned(poly.clone(), config, None) {
        FixOutcome::Fast(g) | FixOutcome::Repaired(g) => Some(g),
        FixOutcome::Unconsumed(_) => None,
    }
}

/// Outcome of [`fix_polygon_owned`]. Distinguishes the zero-copy fast path
/// (passthrough — provably non-degenerate and NaN-free, so the caller can
/// skip the redundant `strip_degenerate`/`has_nan` passes) from rebuilt
/// geometry that still needs those checks.
pub(crate) enum FixOutcome {
    /// Input polygon moved straight into the output — validated by the
    /// fast-path gates: >=4 coords, no sub-ULP edges, no self-intersections,
    /// valid holes, spread >= EPS * scale (checked by the caller), NaN-free.
    Fast(Geometry<f64>),
    /// Rebuilt geometry (repair / fallback paths).
    Repaired(Geometry<f64>),
    /// Structure repair produced nothing; the polygon is returned
    /// unconsumed for the caller's precision-reduction fallback.
    Unconsumed(Polygon<f64>),
}

/// The fast path MOVES the polygon into the output instead of cloning it —
/// for the ~99.85% of real-world polygons that are already valid this is a
/// zero-copy passthrough, matching GEOS's shared-geometry return.
///
/// `ext_scale` is the caller's precomputed exterior bbox scale
/// (max spread, floored at 1.0) — lets `has_sub_ulp_edge` skip its own bbox
/// pass. `None` recomputes it.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn fix_polygon_owned(
    poly: Polygon<f64>,
    config: &MakeValidConfig,
    ext_scale: Option<f64>,
) -> FixOutcome {
    // Fast path: valid polygons can return immediately. Use a total-verts limit
    // to avoid the monotone-chain has_no_intersections cost on very large rings.
    let _t_fp = util::ProfileClock::start();
    #[cfg(feature = "arrange")]
    {
        if fast_path_check(&poly, ext_scale) {
            PROFILE_FP_NS.fetch_add(_t_fp.ns(), Ordering::Relaxed);
            return FixOutcome::Fast(Geometry::Polygon(poly));
        }
    }
    PROFILE_FP_NS.fetch_add(_t_fp.ns(), Ordering::Relaxed);

/// Fast-path gate: valid polygons return immediately, zero-copy. The gate
/// is a COMPLETE certifier: it must accept exactly what the exit validator
/// accepts (the Fast path skips that validator - 2026-08-07), minus
/// orientation, which the caller's re-winding normalizes afterwards. Gates:
/// basic form, sub-ULP edges, per-ring full pair predicate (proper
/// crossing + eps-collinear overlap + vertex-on-edge T-junction, ring-
/// local eps - SmallVec, no heap for <= 32 lines), duplicated rings, and
/// the validator's own hole validation. Large rings swap the chain sweep
/// for the validator itself (one radix pass) - the R-tree proper-crossing
/// sweep was cheaper only in constants and missed the collinear/T-junction
/// classes.
#[cfg(feature = "arrange")]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn fast_path_check(poly: &Polygon<f64>, _ext_scale: Option<f64>) -> bool {
    let total_verts: usize =
        poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
    if total_verts == 0 || poly.exterior().0.len() < 4 {
        return false;
    }
    // Basic form + sub-ULP edges + the global envelope bbox in ONE pass
    // over the coords (measured 2026-08-08: the separate sub-ULP and
    // envelope scans cost ~15-20 us on a 5000-vertex ring).
    let mut bbox = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    let mut sub_ulp = false;
    let mut acc = crate::arrange::GateAccum {
        lines: None,
        bbox: Some(&mut bbox),
        sub_ulp: Some(&mut sub_ulp),
    };
    if !crate::arrange::ring_is_plausible(poly.exterior(), &mut acc) {
        return false;
    }
    for hole in poly.interiors() {
        if !crate::arrange::ring_is_plausible(hole, &mut acc) {
            return false;
        }
    }
    if sub_ulp {
        // Sub-ULP edge check: an edge shorter than EPSILON * bbox_scale
        // (mixed-magnitude rings, e.g. 1e8 coords with 1e-8 spikes) makes
        // proper-crossing detection blind - collinear overlap is invisible.
        // Such inputs are invalid anyway; route them to the full repair.
        return false;
    }
    // Absolute envelope degeneracy - the validator's own rule: a geometry
    // whose bbox is thinner than f64::EPSILON on either axis is rejected
    // as DegenerateExterior. The basic-form gate uses a LOCAL relative
    // rule (correct for mixed magnitudes - a 5e-305-thick sliver is valid
    // at its own scale), so this class needs its own check or the Fast
    // path ships a polygon the validator rejects (fuzz_inprocess_loop
    // micro-sliver, 2026-08-07).
    if (bbox.1 - bbox.0).abs() < f64::EPSILON || (bbox.3 - bbox.2).abs() < f64::EPSILON {
        return false;
    }
    // Duplicated rings (hole == shell, hole == hole): the pair sweeps
    // cannot see identical rings (no proper crossings, no touches that
    // the inclusive hole checks would flag). Validator's own check.
    let interiors: Vec<&[geo::Coord<f64>]> = poly.interiors().iter().map(|h| &h.0[..]).collect();
    if crate::validation::has_duplicate_rings(&interiors, poly.exterior().0.as_slice()) {
        return false;
    }
    if total_verts <= core::FAST_PATH_MAX_VERTS {
        // SmallVec: ~95.6% of the real-world dataset has <= 32 vertices, so
        // the line collection stays on the stack and skips the heap
        // allocation entirely; larger rings spill to the heap transparently.
        let lines: SmallVec<[Line<f64>; crate::core::SMALL_RING_LINES]> = poly.lines_iter().collect();
        !lines.is_empty()
            && crate::arrange::prep::has_no_intersections(&lines)
            && crate::validation::holes::check_holes_valid(
                poly.exterior().0.as_slice(),
                poly.interiors(),
            )
            .is_empty()
    } else {
        // Very large rings: the validator IS the gate - one radix sweep
        // that covers every class the exit validator would re-check
        // (ring simplicity incl. eps-collinear + T-junctions, hole ring
        // validity, hole containment/nesting, duplicates). Orientation
        // errors are filtered: the fast path re-winds after the gate, and
        // winding is the only normalization a simple polygon needs.
        use crate::validation::{GeoValidation, GeometryValidationError};
        let r = poly.validate();
        r.valid
            || r.errors
                .iter()
                .all(|e| matches!(e, GeometryValidationError::WrongOrientation))
    }
}

// Single-pass GEOS MakeValid repair (primary path for invalid input):
    // node shell + holes together in ONE pass, walk even-odd faces. This
    // replaces the multi-stage boolean pipeline (per-ring symdiff +
    // subtract_holes + merge — three noding passes) for the common invalid
    // classes: self-crossings, crossing holes, hole overlaps, holes outside
    // the shell. The result is OGC-wound and validated; on failure we fall
    // through to the boolean pipeline (which remains the safety net).
    //
    // Snap-representability guard: the single-pass snaps to the SNAP_SCALE
    // grid. Inputs whose coordinates span more than the grid can represent
    // (sub-grid small end or > 2^53 large end) must not go through it —
    // the snap destroys micro-features and the boolean fallback can panic
    // in i_overlay. Route them to the caller's arrange/reduce chain, which
    // nodes at full f64 precision. Measured: differential fuzz 2026-08-03.
    if crate::make_valid::snap_cannot_represent(&poly) {
        return FixOutcome::Unconsumed(poly);
    }
    if let Some(mp) = crate::structure::symdiff::single_pass_fix(&poly) {
        // GEOS type semantics: a single-component result keeps the input
        // polygon type; multiple components become MultiPolygon.
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

    // Crossing hole: a hole with a vertex strictly OUTSIDE the shell ring.
    // Neither the i_overlay difference (returns a single quantized ring with
    // 1e-9-grid node artifacts; measured: hole vertex split into two nodes
    // 2.4e-7 apart) nor the polygonizer (mis-assigns the crossing pieces as
    // holes - HoleOutsideShell) produces valid output for these. Arrange's
    // pipeline yields the GEOS-identical even-odd decomposition (verified
    // node-identical to geosop makeValid) - delegate the whole polygon.
    // Boundary-touching holes (all vertices on the shell, e.g. CGAL
    // square_hole_rhombus) are NOT crossing and stay on the boolean path.
    // Hot-path discipline: the per-vertex test is O(hole x shell), so it
    // runs only when the hole bbox pokes outside the shell bbox (the
    // common fully-inside case is a pure bbox comparison) and only on
    // small ring pairs (large rings route through the gates downstream).
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

    // Compute shell bbox once (needed for hole Type C bypass)
    let shell_bbox = ring_bbox(poly.exterior().0.as_slice());

    // Run shell repair + hole processing concurrently (both are independent).
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    let (valid_shells, hole_rings_cw) = {
        use rayon::prelude::*;
        let (shell_res, holes) = rayon::join(
            || {
                let _t = util::ProfileClock::start();
                let shell_polys = match fix_ring::repair_ring(poly.exterior()) {
                    Some(polys) => polys,
                    None => return None,
                };
                if shell_polys.is_empty() {
                    return None;
                }
                // Polygons carry structural holes from BuildArea (self-crossing
                // lobes) — they must survive into the subtract step.
                let valid: Vec<Polygon<f64>> =
                    shell_polys.into_iter().filter(|p| p.exterior().0.len() >= 4).collect();
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
                        if !fix_ring::has_self_intersections_with_bbox(&h.0, hole_bbox) {
                            return vec![h.clone()];
                        }
                        fix_ring::repair_ring(h).map(|polys| {
                            polys.into_iter().map(|p| p.exterior().clone()).collect()
                        }).unwrap_or_else(|| vec![h.clone()])
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
                return FixOutcome::Repaired(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
            }
        };
        (valid_shells, holes)
    };

    // Serial path: shell repair first, then holes sequentially.
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    let (valid_shells, hole_rings_cw) = {
        let _t_sr = util::ProfileClock::start();
        let shell_polys = match fix_ring::repair_ring(poly.exterior()) {
            Some(polys) => polys,
            None => {
                warn!("Structure: shell ring repair failed, falling back to CDT arrange");
                #[cfg(feature = "arrange")]
                if !poly.exterior().0.is_empty() {
                    return FixOutcome::Repaired(crate::arrange::fallback_polygon_fix(&poly));

                }
                return FixOutcome::Repaired(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
            }
        };
        if shell_polys.is_empty() {
            return FixOutcome::Repaired(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
        }
        let valid_shells: Vec<Polygon<f64>> =
            shell_polys.into_iter().filter(|p| p.exterior().0.len() >= 4).collect();
        if valid_shells.is_empty() {
            return FixOutcome::Repaired(handle_collapse_result(poly.exterior(), config).unwrap_or_else(crate::make_valid::empty_geom));
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
                if !fix_ring::has_self_intersections_with_bbox(&h.0, hole_bbox) {
                    hole_rings.push(ensure_cw(h.clone()));
                    continue;
                }
                if let Some(polys) = fix_ring::repair_ring(h) {
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

    // For each valid shell polygon, classify and subtract holes.
    // The shell may already carry STRUCTURAL holes from BuildArea (lobes of a
    // self-crossing shell that must remain void); input holes are subtracted
    // on top of them.
    let process_shell = |shell: Polygon<f64>| -> Vec<Polygon<f64>> {
        let mut shell_poly = shell;
        // Structural holes: ensure CW orientation (geo convention) and drop
        // any that are invalid or collinear.
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

        // Outer holes: rings classified outside the shell. Normally pushed as
        // separate polygons. BUT when an outer hole fully CONTAINS the shell,
        // GEOS makeValid swaps roles — the larger ring becomes the shell and
        // the original shell becomes its hole (even-odd semantics; measured:
        // shell 10x10 + hole 20x20 → GEOS returns the 20x20 with the 10x10 as
        // hole, area 300 = 400-100; our old output was the 20x20 alone, 400).
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
        // Safe: len==1 verified above on local Vec
        let p = result_polys.pop().expect("len==1 verified");
        #[cfg(feature = "arrange")]
        {
            // Proper-crossing check only: legitimate hole/shell vertex touches
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
        // Even-parent filter: when shells are nested (one fully contains another),
        // unary_union produces NestedHoles. The BuildArea even-parent approach
        // keeps only shells with an even number of containing shells.
        let merged = merge::merge_shells(result_polys);
        PROFILE_MG_NS.fetch_add(_t_mg.ns(), Ordering::Relaxed);
        // Clean NestedHoles from merge output
        #[cfg(feature = "arrange")]
        {
            let g = crate::make_valid::drop_nested_components(merged);
            #[cfg(all(any(test, debug_assertions), feature = "std"))]
            if std::env::var("DIAG_STR").is_ok() {
                use geo::Area;
                let ga = match &g {
                    Geometry::Polygon(p) => p.unsigned_area(),
                    Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
                    Geometry::GeometryCollection(gc) => gc.0.iter().map(|x| match x {
                        Geometry::Polygon(p) => p.unsigned_area(),
                        _ => 0.0,
                    }).sum(),
                    _ => 0.0,
                };
                eprintln!("DIAG_STR: drop_nested -> {ga:.4}");
            }
            // Discard components with proper self-crossings (floating-point edge
            // cases). Legitimate hole/shell vertex touches survive.
            if let Geometry::MultiPolygon(ref mp) = g {
                let filtered: Vec<Polygon<f64>> = mp
                    .0
                    .iter()
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

/// Hole nesting resolution, ring helpers, collapse handling (extracted
/// 2026-08-07 for file-size governance).
mod nesting;
/// Cumulative per-stage timing counters and the profile printer.
mod profile;

pub use crate::structure::nesting::*;
pub use crate::structure::profile::*;

#[cfg(all(test, feature = "arrange"))]
mod gate_tests;

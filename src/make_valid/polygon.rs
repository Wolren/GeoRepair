
use super::strip::{enforce_ccw, enforce_cw, has_nan, strip_degenerate};
use super::strip::{enforce_ccw_with_idx, enforce_cw_with_idx};
use super::*;

// Shared polygon-entry steps (2026-09-04) carried identical copies of the pre-scan, the
// NaN-filter rebuild

/// Fused exterior pre-scan: bbox extremes, NaN flag.
fn scan_exterior_bbox_nan(coords: &[Coord<f64>]) -> (f64, f64, f64, f64, bool) {
    let (min_x, max_x, min_y, max_y) = crate::simd::aabb_minmax_simd(coords);
    let mut has_nan = false;
    for c in coords {
        has_nan |= !c.x.is_finite() | !c.y.is_finite();
    }
    if has_nan {
        let (mut mnx, mut mxx, mut mny, mut mxy) =
            (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
        for c in &coords[1..] {
            mnx = mnx.min(c.x);
            mxx = mxx.max(c.x);
            mny = mny.min(c.y);
            mxy = mxy.max(c.y);
        }
        return (mnx, mxx, mny, mxy, true);
    }
    (min_x, max_x, min_y, max_y, false)
}

/// Per-axis local bbox degeneracy: an axis is degenerate when its extent is at or below
/// the coordinate rounding at that axis's own magnitude (eps = epsilon times max
/// |coord| on the axis). A global spread rule lets one distant spike dominate the other
/// axis (fuzz crash-eaab5472, measured 2026-08-04).
fn exterior_is_locally_degenerate(min_x: f64, max_x: f64, min_y: f64, max_y: f64) -> bool {
    let x_scale = max_x.abs().max(min_x.abs());
    let y_scale = max_y.abs().max(min_y.abs());
    (max_x - min_x).abs() <= f64::EPSILON * x_scale
        || (max_y - min_y).abs() <= f64::EPSILON * y_scale
}

/// True when any interior ring holds a non-finite coordinate.
fn interiors_have_nan(poly: &Polygon<f64>) -> bool {
    poly.interiors()
        .iter()
        .any(|ring| ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()))
}

fn short_ring_exit(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    if !config.keep_collapsed && poly.exterior().0.len() < 4 {
        if config.keep_collapsed && !poly.exterior().0.is_empty() {
            return Some(Geometry::Point(Point(poly.exterior().0[0])));
        }
        return Some(empty_geom());
    }
    None
}

/// NaN-filter rebuild: finite-only exterior coords plus finite-only interior rings.
fn filter_finite_rings(poly: &Polygon<f64>) -> (Vec<Coord<f64>>, Vec<LineString<f64>>) {
    let ext_clean: Vec<Coord<f64>> = poly
        .exterior()
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    let int_clean: Vec<LineString<f64>> = poly
        .interiors()
        .iter()
        .map(|ring| {
            LineString::new(
                ring.0
                    .iter()
                    .copied()
                    .filter(|c| c.x.is_finite() && c.y.is_finite())
                    .collect(),
            )
        })
        .collect();
    (ext_clean, int_clean)
}

/// Dedup plus reclose of the NaN-cleaned exterior.
fn dedup_and_close(
    ext_clean: Vec<Coord<f64>>,
) -> Result<(LineString<f64>, Coord<f64>), Geometry<f64>> {
    if ext_clean.is_empty() {
        return Err(empty_geom());
    }
    let first_valid = ext_clean[0];
    let deduped = crate::noding::remove_consecutive_duplicates(&ext_clean);
    if deduped.len() < 3 {
        return Err(match deduped.len() {
            0 => empty_geom(),
            1 => Geometry::Point(Point(deduped[0])),
            _ => Geometry::LineString(LineString::new(deduped)),
        });
    }
    let ext_ring = if deduped.first() == deduped.last() {
        LineString::new(deduped)
    } else {
        let mut c = deduped;
        c.push(c[0]);
        LineString::new(c)
    };
    Ok((ext_ring, first_valid))
}

/// keep_collapsed collapse preservation tail.
fn maybe_collapse_keep(
    result: Geometry<f64>,
    config: &MakeValidConfig,
    orig: &Polygon<f64>,
) -> Geometry<f64> {
    let empty_result = matches!(&result, Geometry::GeometryCollection(gc) if gc.0.is_empty());
    if !config.keep_collapsed || !empty_result {
        return result;
    }
    collapse_degenerate(orig).unwrap_or(result)
}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Triangle<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let coords = [self.v1(), self.v2(), self.v3()];
        for c in coords {
            if !c.x.is_finite() || !c.y.is_finite() {
                warn!("Triangle::make_valid: NaN coordinate ({:?})", c);
                return empty_geom();
            }
        }
        let (a, b, c) = (coords[0], coords[1], coords[2]);
        if a == b || b == c || a == c {
            warn!("Triangle::make_valid: degenerate (duplicate vertices)");
            return empty_geom();
        }
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area == 0.0 {
            warn!("Triangle::make_valid: collinear (zero area)");
            return empty_geom();
        }
        let poly = Polygon::new(LineString::new(vec![a, b, c, a]), Vec::new());
        poly.make_valid_with_config(config)
    }
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
impl<T: GeoFloat> MakeValid for Triangle<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let coords = [self.v1(), self.v2(), self.v3()];
        for c in coords {
            if !c.x.is_finite() || !c.y.is_finite() {
                warn!("Triangle::make_valid: NaN coordinate ({:?})", c);
                return empty_geom();
            }
        }
        let (a, b, c) = (coords[0], coords[1], coords[2]);
        if a == b || b == c || a == c {
            warn!("Triangle::make_valid: degenerate (duplicate vertices)");
            return empty_geom();
        }
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area == T::zero() {
            warn!("Triangle::make_valid: collinear (zero area)");
            return empty_geom();
        }
        let poly = Polygon::new(LineString::new(vec![a, b, c, a]), Vec::new());
        Geometry::Polygon(poly)
    }
}


#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Polygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        // The exterior pre-scan fuses bbox extremes with the NaN check
        if !config.keep_collapsed && self.exterior().0.len() >= 4 {
            let coords = &self.exterior().0;
            let (min_x, max_x, min_y, max_y, mut has_nan) = scan_exterior_bbox_nan(coords);
            if exterior_is_locally_degenerate(min_x, max_x, min_y, max_y) {
                return empty_geom();
            }
            if !has_nan && interiors_have_nan(self) {
                has_nan = true;
            }
            if !has_nan {
                // Panic containment: the boolean overlay path (i_overlay via
                // geo::BooleanOps) can assert on degenerate inputs
                #[cfg(feature = "std")]
                let (repaired, certified) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        make_valid_impl(self, self, config, coords[0])
                    }))
                    .unwrap_or_else(|_| {
                        warn!("make_valid panicked on polygon; returning empty geometry");
                        (empty_geom::<f64>(), false)
                    });
                #[cfg(not(feature = "std"))]
                let (repaired, certified) = make_valid_impl(self, self, config, coords[0]);
                let result = if certified {
                    repaired
                } else {
                    strip_degenerate(repaired)
                };
                return preserve_valid_input(result, config, self);
            }
            // has_nan: fall through to NaN path
        }
        // For valid NaN-free polygons, use make_valid_clean fast-path
        if let Some(g) = short_ring_exit(self, config) {
            return g;
        }

        // NaN path: filter, dedup, rebuild.
        let (ext_clean, int_clean) = filter_finite_rings(self);
        let (ext_ring, first_valid) = match dedup_and_close(ext_clean) {
            Ok(v) => v,
            Err(g) => return g,
        };
        let cleaned = Polygon::new(ext_ring, int_clean);
        // Panic containment (fuzz make_valid seed 0559eed8) A foreign panic degrades to
        // empty.
        #[cfg(feature = "std")]
        let (repaired, _) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            make_valid_impl(self, &cleaned, config, first_valid)
        }))
        .unwrap_or_else(|_| {
            warn!("make_valid panicked on NaN-cleaned polygon; returning empty geometry");
            (empty_geom::<f64>(), false)
        });
        #[cfg(not(feature = "std"))]
        let (repaired, _) = make_valid_impl(self, &cleaned, config, first_valid);
        // The cleaned polygon is not gate-certified (it went through the NaN-filter
        // path).
        let result = strip_degenerate(repaired);
        maybe_collapse_keep(result, config, self)
    }
}

/// Nightly runs 35430371542 and 35703833356 on 2026-09-19 and 2026-09-22 shipped
/// geometrycollection empty for valid inputs.
fn preserve_valid_input(
    result: Geometry<f64>,
    config: &MakeValidConfig,
    orig: &Polygon<f64>,
) -> Geometry<f64> {
    use crate::validation::GeoValidation;
    let out = maybe_collapse_keep(result, config, orig);
    if matches!(&out, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) {
        return out;
    }
    if orig.validate().valid {
        warn!("repair emptied a valid polygon; returning the input unchanged");
        return Geometry::Polygon(orig.clone());
    }
    out
}

/// Check if any coordinate in a polygon is NaN or Infinity.
pub(super) fn has_nan_or_infinite(p: &Polygon<f64>) -> bool {
    p.exterior()
        .0
        .iter()
        .any(|c| !c.x.is_finite() || !c.y.is_finite())
        || p.interiors()
            .iter()
            .any(|ring| ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()))
}

/// True when a snap grid (`SNAP_SCALE` = 1e8) cannot hold the coordinates. Sub-0.5
/// values collapse. A 5.8e-9 coordinate in a 6.2e7 shell is 0.58 units. GEOS fuzz
/// (2026-08-03): snapping mixed-magnitude polygons (1e-9 to 1e7) gave self-intersecting
/// output. Polygon((100 100, 1e15 110, 1e15 100, 100 100)) passes. Seed 5cf953d1 failed
/// at 1.0.
pub(crate) fn snap_cannot_represent(poly: &Polygon<f64>) -> bool {
    let mut min_abs = f64::MAX;
    let mut max_abs = 0.0f64;
    for c in poly
        .exterior()
        .0
        .iter()
        .chain(poly.interiors().iter().flat_map(|h| h.0.iter()))
    {
        let a = c.x.abs().max(c.y.abs());
        if a > 0.0 {
            min_abs = min_abs.min(a);
        }
        max_abs = max_abs.max(a);
    }
    if max_abs == 0.0 || min_abs == f64::MAX {
        return false;
    }
    let scale = crate::core::SNAP_SCALE;
    min_abs * scale < 0.5 || max_abs * scale > (1u64 << 53) as f64
}

pub(crate) fn collapse_degenerate(poly: &Polygon<f64>) -> Option<Geometry<f64>> {
    let coords: Vec<Coord<f64>> = crate::noding::remove_consecutive_duplicates(
        &poly.exterior().0[..poly.exterior().0.len().saturating_sub(1)],
    );
    match coords.len() {
        0 => None,
        1 => Some(Geometry::Point(Point(coords[0]))),
        _ => {
            let area = crate::util::shoelace_abs_sum(&coords);
            if area == 0.0 {
                Some(Geometry::LineString(LineString::new(coords)))
            } else {
                None
            }
        }
    }
}

/// Gated arrange → precision-reduction → empty chain.
#[cfg(any(feature = "arrange", feature = "structure"))]
fn arrange_chain(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    if is_valid_with_geo(&Geometry::Polygon(poly.clone())) {
        return enforce_ogc_winding(Geometry::Polygon(poly.clone())).0;
    }
    let arranged = enforce_ogc_winding(arrange_or_empty(poly, config)).0;
    if is_valid_with_geo(&arranged) {
        arranged
    } else {
        warn!("arrange output invalid, retrying with precision reduction");
        let fb = enforce_ogc_winding(reduce_fallback(poly, config)).0;
        if is_valid_with_geo(&fb) {
            fb
        } else {
            warn!("repair failed all paths, returning empty");
            empty_geom::<f64>()
        }
    }
}

#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn make_valid_impl(
    _self: &Polygon<f64>,
    poly: &Polygon<f64>,
    config: &MakeValidConfig,
    _first_valid: Coord<f64>,
) -> (Geometry<f64>, bool) {
    // the two callers in this module (the clean fast path and the NaN-filtered path in
    // make_valid_with_config) both guarantee
    debug_assert!(
        !has_nan_or_infinite(poly),
        "make_valid_impl requires NaN-free input"
    );
    let (result, certified) = match config.poly_method {
        PolyMethod::Arrange => (arrange_chain(poly, config), false),
        PolyMethod::Structure => {
            let st = structure_fix_owned(poly.clone(), config, None);
            match st {
                crate::structure::FixOutcome::Fast(g, min_abs, max_abs, extrema) => {
                    let (g_norm, ok) = enforce_ogc_winding_with_extrema(g, Some(&extrema));
                    if ok {
                        // at subnormal/huge magnitude mixes (fuzz crash-65111cd0) and
                        // in the f64 orient overflow The extrema come fused from the
                        // fast-path gate's plausibility pass (no re-scan of the coords,
                        // 2026-08-09).
                        if fast_gate_needs_full_validation(min_abs, max_abs)
                            && !is_valid_with_geo(&g_norm)
                        {
                            warn!(
                                "Structure mode: fast-path extreme-magnitude output invalid, \
                                 retrying with CDT arrange"
                            );
                            (arrange_chain(poly, config), false)
                        } else {
                            (g_norm, true)
                        }
                    } else {
                        warn!(
                            "Structure mode: fast-path orientation ambiguous, retrying with CDT arrange"
                        );
                        (arrange_chain(poly, config), false)
                    }
                }
                crate::structure::FixOutcome::Repaired(g) => {
                    // fast path can pass a wrong-wound (CW) input
                    let g_norm = enforce_ogc_winding(g).0;
                    if is_valid_with_geo(&g_norm) {
                        (g_norm, false)
                    } else {
                        warn!("Structure mode: fix output invalid, retrying with CDT arrange");
                        (arrange_chain(poly, config), false)
                    }
                }
                crate::structure::FixOutcome::Unconsumed(p) => {
                    warn!("Structure mode: fix failed, retrying with CDT arrange");
                    (arrange_chain(&p, config), false)
                }
            }
        }
        PolyMethod::Auto => {
            match structure_fix_owned(poly.clone(), config, None) {
                crate::structure::FixOutcome::Fast(g, min_abs, max_abs, extrema) => {
                    let (g_norm, ok) = enforce_ogc_winding_with_extrema(g, Some(&extrema));
                    if ok {
                        // NaN no-cross verdicts - fuzz crash-65111cd0 .
                        if fast_gate_needs_full_validation(min_abs, max_abs)
                            && !is_valid_with_geo(&g_norm)
                        {
                            warn!(
                                "Auto mode: fast-path extreme-magnitude output invalid,                                  falling back to CDT arrange"
                            );
                            (arrange_chain(poly, config), false)
                        } else {
                            (g_norm, true)
                        }
                    } else {
                        warn!(
                            "Auto mode: fast-path orientation ambiguous, falling back to CDT arrange"
                        );
                        (arrange_chain(poly, config), false)
                    }
                }
                crate::structure::FixOutcome::Repaired(r) => {
                    let r_norm = enforce_ogc_winding(r).0;
                    if is_valid_with_geo(&r_norm) {
                        (r_norm, false)
                    } else {
                        warn!("Auto mode: structure_fix invalid, falling back to CDT arrange");
                        (arrange_chain(poly, config), false)
                    }
                }
                crate::structure::FixOutcome::Unconsumed(p) => {
                    warn!("Auto mode: structure_fix failed, falling back to CDT arrange");
                    (arrange_chain(&p, config), false)
                }
            }
        }
    };
    if !certified && has_nan(&result) {
        (empty_geom::<f64>(), false)
    } else {
        (result, certified)
    }
}

/// `verified == true` means the fast-path passthrough, which is provably non-degenerate
/// and NaN-free.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn make_valid_impl_owned(
    poly: Polygon<f64>,
    config: &MakeValidConfig,
    _first_valid: Coord<f64>,
    ext_scale: Option<f64>,
) -> (Geometry<f64>, bool) {
    // Same NaN-free guarantee as make_valid_impl's callers.
    debug_assert!(
        !has_nan_or_infinite(&poly),
        "make_valid_impl_owned requires NaN-free input"
    );
    let (result, verified) = match config.poly_method {
        PolyMethod::Arrange => (arrange_chain(&poly, config), false),
        PolyMethod::Structure => {
            #[cfg(feature = "structure")]
            {
                match structure_fix_owned(poly, config, ext_scale) {
                    crate::structure::FixOutcome::Fast(g, _, _, extrema) => {
                        let (g, ok) = enforce_ogc_winding_with_extrema(g, Some(&extrema));
                        if ok {
                            return (g, true);
                        }
                        warn!(
                            "Structure mode: fast-path orientation ambiguous, retrying with CDT arrange"
                        );
                        if let Geometry::Polygon(p) = g {
                            return (arrange_chain(&p, config), false);
                        }
                        return (g, false);
                    }
                    crate::structure::FixOutcome::Repaired(g) => (g, false),
                    crate::structure::FixOutcome::Unconsumed(p) => {
                        warn!("Structure mode: fix failed, retrying with CDT arrange");
                        (arrange_chain(&p, config), false)
                    }
                }
            }
            #[cfg(not(feature = "structure"))]
            {
                warn!(
                    "PolyMethod::Structure selected but 'structure' feature is not enabled. Enable the 'structure' feature in Cargo.toml to use Structure mode."
                );
                (arrange_chain(&poly, config), false)
            }
        }
        PolyMethod::Auto => {
            let (g, v) = make_valid_impl(&poly, &poly, config, _first_valid);
            (g, v)
        }
    };
    if !verified {
        let result = enforce_ogc_winding(result).0;
        if has_nan(&result) {
            (empty_geom::<f64>(), true)
        } else {
            (result, false)
        }
    } else {
        (result, true)
    }
}

/// Zero-copy for the ~99.85% of real-world polygons that are already valid.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub fn make_valid_owned(poly: Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    if !config.keep_collapsed && poly.exterior().0.len() >= 4 {
        let (min_x, max_x, min_y, max_y, mut has_nan) = scan_exterior_bbox_nan(&poly.exterior().0);
        if exterior_is_locally_degenerate(min_x, max_x, min_y, max_y) {
            return empty_geom();
        }
        if !has_nan && interiors_have_nan(&poly) {
            has_nan = true;
        }
        if !has_nan {
            let first = poly.exterior().0[0];
            // Panic containment (mirrors the borrowed path): i_overlay can assert on
            // degenerate inputs.
            #[cfg(feature = "std")]
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                make_valid_impl_owned(poly, config, first, None)
            }))
            .unwrap_or_else(|_| {
                warn!("make_valid_owned panicked on polygon; returning empty geometry");
                (empty_geom::<f64>(), false)
            });
            #[cfg(not(feature = "std"))]
            let result = make_valid_impl_owned(poly, config, first, None);
            let (g, verified) = result;
            return if verified { g } else { strip_degenerate(g) };
        }
        // has_nan: fall through to the NaN path below (mirrors the borrowed version).
    }
    if let Some(g) = short_ring_exit(&poly, config) {
        return g;
    }
    // keep_collapsed: true with >= 4 verts, or NaN present: rebuild clean.
    let (ext_clean, int_clean) = filter_finite_rings(&poly);
    let (ext_ring, first_valid) = match dedup_and_close(ext_clean) {
        Ok(v) => v,
        Err(g) => return g,
    };
    let cleaned = Polygon::new(ext_ring, int_clean);
    // `cleaned` shares the exterior bbox with the original ring (NaN filtering does not
    // change min/max)
    let (g, verified) = make_valid_impl_owned(cleaned, config, first_valid, None);
    let result = if verified { g } else { strip_degenerate(g) };
    maybe_collapse_keep(result, config, &poly)
}

/// It reverses a ring only when the winding is wrong. Winding fusion, 2026-08-08, 1.58M
/// polygons.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn enforce_ogc_winding(g: Geometry<f64>) -> (Geometry<f64>, bool) {
    enforce_ogc_winding_with_extrema(g, None)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn enforce_ogc_winding_with_extrema(
    g: Geometry<f64>,
    extrema: Option<&[usize]>,
) -> (Geometry<f64>, bool) {
    match g {
        Geometry::Polygon(p) => {
            let (ext, mut holes) = p.into_inner();
            let hole_cnt = holes.len();
            let extrema_ok = extrema.is_some_and(|ex| ex.len() == 1 + hole_cnt);
            let (ext, ext_idx, ext_rev) = if extrema_ok {
                enforce_ccw_with_idx(ext, extrema.unwrap()[0])
            } else {
                enforce_ccw(ext)
            };
            let mut ok = winding_ok(&ext.0, ext_idx, ext_rev, true);
            for (k, h) in holes.iter_mut().enumerate() {
                let owned = core::mem::replace(h, geo::LineString::new(Vec::new()));
                let (hw, h_idx, h_rev) = if extrema_ok {
                    enforce_cw_with_idx(owned, extrema.unwrap()[1 + k])
                } else {
                    enforce_cw(owned)
                };
                ok &= winding_ok(&hw.0, h_idx, h_rev, false);
                *h = hw;
            }
            (Geometry::Polygon(Polygon::new(ext, holes)), ok)
        }
        Geometry::MultiPolygon(mp) => {
            let mut ok = true;
            let polys: Vec<Polygon<f64>> =
                mp.0.into_iter()
                    .map(|p| {
                        let (ext, mut holes) = p.into_inner();
                        let (ext, ext_idx, ext_rev) = enforce_ccw(ext);
                        ok &= winding_ok(&ext.0, ext_idx, ext_rev, true);
                        for h in holes.iter_mut() {
                            let owned = core::mem::replace(h, geo::LineString::new(Vec::new()));
                            let (hw, h_idx, h_rev) = enforce_cw(owned);
                            ok &= winding_ok(&hw.0, h_idx, h_rev, false);
                            *h = hw;
                        }
                        Polygon::new(ext, holes)
                    })
                    .collect();
            (Geometry::MultiPolygon(MultiPolygon::new(polys)), ok)
        }
        other => (other, true),
    }
}

fn winding_ok(ring: &[Coord<f64>], idx: usize, reversed: bool, shell: bool) -> bool {
    let n = ring.len() - 1;
    let mapped = if reversed {
        if idx == 0 { 0 } else { n - idx }
    } else {
        idx
    };
    let ccw = crate::util::robust_is_ccw_at(ring, mapped);
    if shell { ccw } else { !ccw }
}

/// Check if a geometry contains NaN coordinates using CoordsIter.
#[cfg_attr(not(feature = "proj"), allow(unused_variables))]
pub(super) fn apply_target_crs(geom: Geometry<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    #[cfg(feature = "proj")]
    if let (Some(src_crs), Some(dst_crs)) = (&config.crs, &config.target_crs) {
        if src_crs != dst_crs {
            match crate::crs::transform_geometry(&geom, src_crs, dst_crs) {
                Ok(g) => return g,
                Err(e) => log::warn!("CRS transform failed (keeping original): {e}"),
            }
        }
    }
    geom
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
impl<T: NodingFloat> MakeValid for Geometry<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        match self {
            Geometry::Point(g) => g.make_valid_with_config(config),
            Geometry::Line(g) => g.make_valid_with_config(config),
            Geometry::LineString(g) => g.make_valid_with_config(config),
            Geometry::Polygon(_) | Geometry::MultiPolygon(_) => {
                warn!(
                    "Geometry::make_valid: Polygon/MultiPolygon repair requires 'arrange' or 'structure' feature"
                );
                empty_geom()
            }
            Geometry::MultiPoint(g) => g.make_valid_with_config(config),
            Geometry::MultiLineString(g) => g.make_valid_with_config(config),
            Geometry::GeometryCollection(g) => g.make_valid_with_config(config),
            Geometry::Rect(g) => g.make_valid_with_config(config),
            Geometry::Triangle(g) => g.make_valid_with_config(config),
        }
    }
}


#[cfg(feature = "arrange")]
pub(super) fn arrange_or_empty(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    let result = crate::arrange::fix_polygon(poly, config);
    if let Geometry::MultiPolygon(mp) = &result
        && mp.0.len() > 1
    {
        return drop_nested_components(mp.clone());
    }
    result
}

#[cfg(not(feature = "arrange"))]
fn arrange_or_empty(_poly: &Polygon<f64>, _config: &MakeValidConfig) -> Geometry<f64> {
    empty_geom::<f64>()
}

#[cfg(feature = "structure")]
#[cfg(feature = "structure")]
pub(super) fn structure_fix_owned(
    poly: Polygon<f64>,
    config: &MakeValidConfig,
    ext_scale: Option<f64>,
) -> crate::structure::FixOutcome {
    crate::structure::fix_polygon_owned(poly, config, ext_scale)
}

/// Ratio > 1e100: huge and subnormal mixes overlap all bbox pairs. Fuzz crash 65111cd0.
/// max_abs > 1e150: orient products overflow f64 near 1.8e308 to inf. Products overflow
/// at 6.7e153, so 1e150 keeps margin. Fuzz crash 53657: coords 1.5e82 to 1.4e166 span
/// 9.4e83, below the ratio guard.
fn fast_gate_needs_full_validation(min_abs: f64, max_abs: f64) -> bool {
    (min_abs.is_finite() && min_abs > 0.0 && max_abs / min_abs > 1e100)
        || (max_abs.is_finite() && max_abs > 1e150)
}

/// Check OGC validity using our own GeoValidation (Shewchuk-based).
pub fn is_valid_with_geo(g: &Geometry<f64>) -> bool {
    use crate::validation::GeoValidation;
    g.is_valid()
}

/// Last-resort fallback: BuildArea on noded boundary, then precision snap.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn reduce_fallback(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    use crate::reduce::{GeometryPrecisionReducer, PrecisionModel};
    let scales = [1e-10, 1e-8, 1e-6, 1e-4];
    for &scale in &scales {
        let model = PrecisionModel::new(scale);
        let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
        let geom = reducer.reduce_raw(poly);
        if is_valid_with_geo(&geom) {
            return geom;
        }
    }
    let model = PrecisionModel::new(1e-4);
    let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
    reducer.reduce_raw(poly)
}

/// Check if bounding boxes of any two shells in a MultiPolygon overlap.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn shells_have_overlapping_bboxes(mp: &MultiPolygon<f64>) -> bool {
    let bboxes: Vec<(f64, f64, f64, f64)> =
        mp.0.iter()
            .map(|p| {
                let coords = &p.exterior().0;
                if coords.is_empty() {
                    return (0.0, 0.0, 0.0, 0.0);
                }
                let (mut min_x, mut max_x, mut min_y, mut max_y) =
                    (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
                for c in coords.iter().skip(1) {
                    if c.x < min_x {
                        min_x = c.x;
                    }
                    if c.x > max_x {
                        max_x = c.x;
                    }
                    if c.y < min_y {
                        min_y = c.y;
                    }
                    if c.y > max_y {
                        max_y = c.y;
                    }
                }
                (min_x, max_x, min_y, max_y)
            })
            .collect();
    for i in 0..bboxes.len() {
        for j in (i + 1)..bboxes.len() {
            let (min_ix, max_ix, min_iy, max_iy) = bboxes[i];
            let (min_jx, max_jx, min_jy, max_jy) = bboxes[j];
            if min_ix <= max_jx && min_jx <= max_ix && min_iy <= max_jy && min_jy <= max_iy {
                return true;
            }
        }
    }
    false
}

#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn shells_have_vertex_inside(mp: &MultiPolygon<f64>) -> bool {
    for i in 0..mp.0.len() {
        let ext_i = &mp.0[i].exterior().0;
        if ext_i.len() < 4 {
            continue;
        }
        for j in 0..mp.0.len() {
            if i == j {
                continue;
            }
            let ext_j = &mp.0[j].exterior().0;
            if ext_j.len() < 4 {
                continue;
            }
            let max_check = ext_i.len().min(32);
            for pt in ext_i.iter().take(max_check) {
                if point_in_ring_exclusive_even_odd(*pt, ext_j) {
                    return true;
                }
            }
        }
    }
    false
}

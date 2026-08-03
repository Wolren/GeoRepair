//! Polygon repair: Triangle/Polygon MakeValid impls, strategy dispatch,
//! OGC winding enforcement, and reduction fallbacks.

use super::*;
use super::strip::{enforce_ccw, enforce_cw, has_nan, strip_degenerate};

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

// ---------------------------------------------------------------------------
// Polygon - concrete f64 impl
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Polygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        // Fuse the NaN scan into the collapse-check loop to avoid a separate pass.
        // The collapse check already iterates all coords - piggyback the is_finite
        // check there.  make_valid_clean handles the merged logic.
        if !config.keep_collapsed && self.exterior().0.len() >= 4 {
            let coords = &self.exterior().0;
            let (mut min_x, mut max_x, mut min_y, mut max_y) =
                (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
            let mut has_nan = !coords[0].x.is_finite() || !coords[0].y.is_finite();
            for w in coords.windows(2) {
                min_x = min_x.min(w[1].x);
                max_x = max_x.max(w[1].x);
                min_y = min_y.min(w[1].y);
                max_y = max_y.max(w[1].y);
                if !has_nan && (!w[1].x.is_finite() || !w[1].y.is_finite()) {
                    has_nan = true;
                }
            }
            let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
            if (max_x - min_x).abs() < f64::EPSILON * scale
                || (max_y - min_y).abs() < f64::EPSILON * scale
            {
                return empty_geom();
            }
            if !has_nan {
                // Also check interior rings - exterior might be clean but holes can have NaNs
                if !self.interiors().is_empty() {
                    for ring in self.interiors().iter() {
                        if ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
                            has_nan = true;
                            break;
                        }
                    }
                }
            }
            if !has_nan {
                            return strip_degenerate(make_valid_impl(self, self, config, coords[0]));
                        }
            // has_nan: fall through to NaN path
        }
        // For valid NaN-free polygons, use make_valid_clean fast-path
        if !config.keep_collapsed && self.exterior().0.len() < 4 {
            // Degenerate ring (< 4 vertices). If keep_collapsed, save as Point.
            if config.keep_collapsed && !self.exterior().0.is_empty() {
                return Geometry::Point(Point(self.exterior().0[0]));
            }
            return empty_geom();
        }
        // keep_collapsed: true with >= 4 verts: fall through to make_valid_impl

        // NaN path: filter, dedup, rebuild.
        let ext_clean: Vec<Coord<f64>> = self
            .exterior()
            .0
            .iter()
            .copied()
            .filter(|c| c.x.is_finite() && c.y.is_finite())
            .collect();
        if ext_clean.is_empty() {
            return empty_geom();
        }
        let first_valid = ext_clean[0];
        let int_clean: Vec<LineString<f64>> = self
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
        let deduped = crate::noding::remove_consecutive_duplicates(&ext_clean);
        if deduped.len() < 3 {
            return match deduped.len() {
                0 => empty_geom(),
                1 => Geometry::Point(Point(deduped[0])),
                _ => Geometry::LineString(LineString::new(deduped)),
            };
        }
        let ext_ring = if deduped.first() == deduped.last() {
            LineString::new(deduped)
        } else {
            let mut c = deduped;
            c.push(c[0]);
            LineString::new(c)
        };
        let cleaned = Polygon::new(ext_ring, int_clean);
        strip_degenerate(make_valid_impl(self, &cleaned, config, first_valid))
    }
}

/// Check if any coordinate in a polygon is NaN or Infinity.
pub(super) fn has_nan_or_infinite(p: &Polygon<f64>) -> bool {
    p.exterior().0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
        || p.interiors().iter().any(|ring| {
            ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
        })
}

/// Common strategy dispatch after degeneracy checks.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn make_valid_impl(
    _self: &Polygon<f64>,
    poly: &Polygon<f64>,
    config: &MakeValidConfig,
    _first_valid: Coord<f64>,
) -> Geometry<f64> {
    // NaN/Inf bail: the two callers in this module (the clean fast path and
    // the NaN-filtered path in make_valid_with_config) both guarantee
    // NaN-free input before calling, so this is a debug-only guard — the
    // full-scan version cost one extra pass over every polygon on the hot
    // path (1.58M polygons in the real-world benchmark).
    debug_assert!(
        !has_nan_or_infinite(poly),
        "make_valid_impl requires NaN-free input"
    );
    let result = match config.poly_method {
            PolyMethod::Arrange => arrange_or_empty(poly, config),
            PolyMethod::Structure => structure_fix(poly, config).unwrap_or_else(|| {
                warn!("Structure mode: fix failed, retrying with precision reduction");
                reduce_fallback(poly, config)
            }),
            PolyMethod::Auto => {
                if let Some(r) = structure_fix(poly, config) {
                    // The structure path emits GEOS walker winding (CW shells,
                    // CCW holes - GEOS polygonizer convention). OGC validity
                    // requires CCW shells; normalize before the gate.
                    let r_norm = enforce_ogc_winding(r);
                    #[cfg(any(test, debug_assertions))]
                    if std::env::var("DIAG_MV").is_ok() {
                        use geo::Area;
                        let ra = match &r_norm {
                            Geometry::Polygon(p) => p.unsigned_area(),
                            Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
                            Geometry::GeometryCollection(gc) => gc.0.iter().map(|x| match x {
                                Geometry::Polygon(p) => p.unsigned_area(),
                                _ => 0.0,
                            }).sum(),
                            _ => 0.0,
                        };
                        eprintln!("DIAG_MV auto: structure r={ra:.4} valid={}", is_valid_with_geo(&r_norm));
                    }
                    if is_valid_with_geo(&r_norm) { r_norm }
                    else {
                        warn!("Auto mode: structure_fix invalid, falling back to CDT arrange");
                        let arranged = arrange_or_empty(poly, config);
                        if is_valid_with_geo(&arranged) { arranged }
                        else {
                            warn!("Auto mode: arrange also invalid, retrying with precision reduction");
                            reduce_fallback(poly, config)
                        }
                    }
                } else {
                    warn!("Auto mode: structure_fix failed, falling back to CDT arrange");
                    let arranged = arrange_or_empty(poly, config);
                    if is_valid_with_geo(&arranged) { arranged }
                    else {
                        warn!("Auto mode: arrange also invalid, retrying with precision reduction");
                        reduce_fallback(poly, config)
                    }
                }
            }
        };
        let result = enforce_ogc_winding(result);
        if has_nan(&result) { empty_geom::<f64>() } else { result }
        }

/// Owned twin of [`make_valid_impl`]: takes ownership of the working
/// polygon so the Structure fast path can MOVE it into the output instead of
/// cloning (zero-copy passthrough for valid input). Arrange rebuilds anyway
/// and borrows; Auto keeps the borrowed path (its full validation gate
/// dwarfs the clone cost).
///
/// Returns `(geometry, verified)` — `verified == true` means the result is
/// the fast-path passthrough: provably non-degenerate and NaN-free, so the
/// caller can skip `strip_degenerate` (and the result already skipped
/// `has_nan`).
///
/// `ext_scale` is the caller's exterior bbox scale from its earlier scan
/// (see [`fix_polygon_owned`]); `None` recomputes it.
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
    let result = match config.poly_method {
        PolyMethod::Arrange => arrange_or_empty(&poly, config),
        PolyMethod::Structure => match structure_fix_owned(poly, config, ext_scale) {
            // Fast path: input was verified NaN-free by the caller's scan and
            // non-degenerate by the gates — winding is the only normalization
            // needed, and it cannot introduce NaNs. Skip has_nan/strip.
            crate::structure::FixOutcome::Fast(g) => {
                let g = enforce_ogc_winding(g);
                return (g, true);
            }
            crate::structure::FixOutcome::Repaired(g) => g,
            crate::structure::FixOutcome::Unconsumed(p) => {
                warn!("Structure mode: fix failed, retrying with precision reduction");
                reduce_fallback(&p, config)
            }
        },
        PolyMethod::Auto => make_valid_impl(&poly, &poly, config, _first_valid),
    };
    let result = enforce_ogc_winding(result);
    if has_nan(&result) { (empty_geom::<f64>(), true) } else { (result, false) }
}

/// Owned twin of [`Polygon::make_valid_with_config`] for batch pipelines
/// that already own their polygons (e.g. [`crate::parallel::par_fix_polygon_batch_owned`]).
/// Moves the polygon through the Structure fast path — zero-copy for the
/// ~99.85% of real-world polygons that are already valid.
pub fn make_valid_owned(poly: Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    // Mirrors make_valid_with_config with `poly` owned instead of `&self`.
    // Keep the two bodies in sync.
    if !config.keep_collapsed && poly.exterior().0.len() >= 4 {
        let coords = &poly.exterior().0;
        let (mut min_x, mut max_x, mut min_y, mut max_y) =
            (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
        let mut has_nan = !coords[0].x.is_finite() || !coords[0].y.is_finite();
        for w in coords.windows(2) {
            min_x = min_x.min(w[1].x);
            max_x = max_x.max(w[1].x);
            min_y = min_y.min(w[1].y);
            max_y = max_y.max(w[1].y);
            if !has_nan && (!w[1].x.is_finite() || !w[1].y.is_finite()) {
                has_nan = true;
            }
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        if (max_x - min_x).abs() < f64::EPSILON * scale
            || (max_y - min_y).abs() < f64::EPSILON * scale
        {
            return empty_geom();
        }
        if !has_nan && !poly.interiors().is_empty() {
            for ring in poly.interiors().iter() {
                if ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
                    has_nan = true;
                    break;
                }
            }
        }
        if !has_nan {
            let first = coords[0];
            let (g, verified) = make_valid_impl_owned(poly, config, first, Some(scale));
            return if verified { g } else { strip_degenerate(g) };
        }
        // has_nan: fall through to the NaN path below (mirrors the borrowed version).
    }
    if !config.keep_collapsed && poly.exterior().0.len() < 4 {
        if config.keep_collapsed && !poly.exterior().0.is_empty() {
            return Geometry::Point(Point(poly.exterior().0[0]));
        }
        return empty_geom();
    }
    // keep_collapsed: true with >= 4 verts, or NaN present: rebuild clean.
    let ext_clean: Vec<Coord<f64>> = poly
        .exterior()
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    if ext_clean.is_empty() {
        return empty_geom();
    }
    let first_valid = ext_clean[0];
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
    let deduped = crate::noding::remove_consecutive_duplicates(&ext_clean);
    if deduped.len() < 3 {
        return match deduped.len() {
            0 => empty_geom(),
            1 => Geometry::Point(Point(deduped[0])),
            _ => Geometry::LineString(LineString::new(deduped)),
        };
    }
    let ext_ring = if deduped.first() == deduped.last() {
        LineString::new(deduped)
    } else {
        let mut c = deduped;
        c.push(c[0]);
        LineString::new(c)
    };
    let cleaned = Polygon::new(ext_ring, int_clean);
    // `cleaned` shares the exterior bbox with the original ring (NaN filtering
    // does not change min/max), so recompute the scale cheaply from the
    // cleaned exterior — same formula as the scan above.
    let (g, verified) = make_valid_impl_owned(cleaned, config, first_valid, None);
    if verified { g } else { strip_degenerate(g) }
}

/// Enforce OGC winding: CCW exterior, CW interior rings.
/// Consumes the geometry and rebuilds rings in place — no cloning: the
/// exterior and hole `LineString`s are moved out via `into_inner` and only
/// reversed when their winding is wrong. The previous implementation cloned
/// every ring unconditionally, which cost two `Vec` allocations per polygon
/// on the hot path (1.58M polygons in the real-world benchmark).
pub(crate) fn enforce_ogc_winding(g: Geometry<f64>) -> Geometry<f64> {
    match g {
        Geometry::Polygon(p) => {
            let (ext, mut holes) = p.into_inner();
            let ext = enforce_ccw(ext);
            for h in holes.iter_mut() {
                let owned = std::mem::replace(h, geo::LineString::new(Vec::new()));
                *h = enforce_cw(owned);
            }
            Geometry::Polygon(Polygon::new(ext, holes))
        }
        Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(MultiPolygon::new(
            mp.0.into_iter()
                .map(|p| {
                    let (ext, mut holes) = p.into_inner();
                    let ext = enforce_ccw(ext);
                    for h in holes.iter_mut() {
                        let owned = std::mem::replace(h, geo::LineString::new(Vec::new()));
                        *h = enforce_cw(owned);
                    }
                    Polygon::new(ext, holes)
                })
                .collect(),
        )),
        other => other,
    }
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
                warn!("Geometry::make_valid: Polygon/MultiPolygon repair requires 'arrange' or 'structure' feature");
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

// Helper functions for polygon dispatch

#[cfg(feature = "arrange")]
pub(super) fn arrange_or_empty(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    let result = crate::arrange::fix_polygon(poly, config);
    // Clean NestedHoles from Arrange output (edge-sharing components)
    if let Geometry::MultiPolygon(mp) = &result {
        if mp.0.len() > 1 {
            return drop_nested_components(mp.clone());
        }
    }
    result
}

#[cfg(not(feature = "arrange"))]
fn arrange_or_empty(_poly: &Polygon<f64>, _config: &MakeValidConfig) -> Geometry<f64> {
    empty_geom::<f64>()
}

#[cfg(feature = "structure")]
pub(super) fn structure_fix(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    crate::structure::fix_polygon(poly, config)
}

/// Owned structure fix: distinguishes the zero-copy fast-path passthrough
/// (see [`FixOutcome`]) from rebuilt geometry, and returns the polygon
/// unconsumed when repair produced nothing.
#[cfg(feature = "structure")]
pub(super) fn structure_fix_owned(
    poly: Polygon<f64>,
    config: &MakeValidConfig,
    ext_scale: Option<f64>,
) -> crate::structure::FixOutcome {
    crate::structure::fix_polygon_owned(poly, config, ext_scale)
}

#[cfg(not(feature = "structure"))]
fn structure_fix(poly: &Polygon<f64>, _config: &MakeValidConfig) -> Option<Geometry<f64>> {
    if !poly.exterior().0.is_empty() {
        warn!("PolyMethod::Structure selected but 'structure' feature is not enabled. Enable the 'structure' feature in Cargo.toml to use Structure mode.");
    }
    None
}

#[cfg(not(feature = "structure"))]
fn structure_fix_owned(
    poly: Polygon<f64>,
    _config: &MakeValidConfig,
    _ext_scale: Option<f64>,
) -> crate::structure::FixOutcome {
    if !poly.exterior().0.is_empty() {
        warn!("PolyMethod::Structure selected but 'structure' feature is not enabled. Enable the 'structure' feature in Cargo.toml to use Structure mode.");
    }
    crate::structure::FixOutcome::Unconsumed(poly)
}

/// Check OGC validity using our own GeoValidation (Shewchuk-based).
pub fn is_valid_with_geo(g: &Geometry<f64>) -> bool {
    use crate::validation::GeoValidation;
    g.is_valid()
}

/// Last-resort fallback: BuildArea on noded boundary, then precision snap.
/// Uses only `reduce_raw` (snap only, no MakeValid call) to avoid recursion.
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
    // Last resort: coarsest grid, even if invalid
    let model = PrecisionModel::new(1e-4);
    let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
    reducer.reduce_raw(poly)
}

/// Check if bounding boxes of any two shells in a MultiPolygon overlap.
/// Used as a cheap pre-filter - if bboxes don't overlap, there's
/// no chance of shell overlap, so we can safely skip the expensive union.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn shells_have_overlapping_bboxes(mp: &MultiPolygon<f64>) -> bool {
    let bboxes: Vec<(f64, f64, f64, f64)> = mp
        .0
        .iter()
        .map(|p| {
            let coords = &p.exterior().0;
            if coords.is_empty() {
                return (0.0, 0.0, 0.0, 0.0);
            }
            let (mut min_x, mut max_x, mut min_y, mut max_y) =
                (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
            for c in coords.iter().skip(1) {
                if c.x < min_x { min_x = c.x; }
                if c.x > max_x { max_x = c.x; }
                if c.y < min_y { min_y = c.y; }
                if c.y > max_y { max_y = c.y; }
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

/// Check if any vertex of one shell is strictly inside another shell's ring.
/// Catches partial overlaps where is_valid_with_geo misses vertex containment.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(super) fn shells_have_vertex_inside(mp: &MultiPolygon<f64>) -> bool {
    for i in 0..mp.0.len() {
        let ext_i = &mp.0[i].exterior().0;
        if ext_i.len() < 4 { continue; }
        for j in 0..mp.0.len() {
            if i == j { continue; }
            let ext_j = &mp.0[j].exterior().0;
            if ext_j.len() < 4 { continue; }
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

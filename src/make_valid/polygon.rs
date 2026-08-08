//! Polygon repair: Triangle/Polygon MakeValid impls, strategy dispatch,
//! OGC winding enforcement, and reduction fallbacks.


use super::*;
use super::strip::{enforce_ccw, enforce_cw, has_nan, strip_degenerate};

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
            // Bbox degeneracy is per-axis LOCAL: an axis is degenerate when
            // its extent is at or below the coordinate rounding at that
            // axis's own magnitude (eps = EPSILON * max |coord| on the
            // axis). The old rule compared both extents against the max
            // spread, so one distant spike dominated the other axis:
            // measured 2026-08-04, a VALID ring with a 4.9e208 y-spike and
            // a 1-unit x-extent (8 ULPs at 1e15) was emptied here in every
            // repair mode (fuzz crash-eaab5472).
            let x_scale = max_x.abs().max(min_x.abs());
            let y_scale = max_y.abs().max(min_y.abs());
            if (max_x - min_x).abs() <= f64::EPSILON * x_scale
                || (max_y - min_y).abs() <= f64::EPSILON * y_scale
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
                // Panic containment: the boolean overlay path (i_overlay via
                // geo::BooleanOps) can assert on degenerate inputs; a foreign
                // library panic must degrade to empty, never crash the host.
                #[cfg(feature = "std")]
                let repaired =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        make_valid_impl(self, self, config, coords[0])
                    }))
                    .unwrap_or_else(|_| {
                        warn!("make_valid panicked on polygon; returning empty geometry");
                        empty_geom::<f64>()
                    });
                #[cfg(not(feature = "std"))]
                let repaired = make_valid_impl(self, self, config, coords[0]);
                let result = strip_degenerate(repaired);
                if config.keep_collapsed
                    && matches!(&result, Geometry::GeometryCollection(gc) if gc.0.is_empty())
                {
                    // Collapsed geometry with keep_collapsed: preserve it as a
                    // lower dimension (GEOS keepCollapsed semantics) instead
                    // of dropping it. Measured: fully-collinear ring (0 0, 5 0,
                    // 10 0, 0 0) — the closing-edge check flags it as a
                    // self-intersection and every repair path collapses to
                    // empty (test_shell_collapse_keep_collapsed).
                    if let Some(c) = collapse_degenerate(self) {
                        return c;
                    }
                }
                return result;
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
        let result = strip_degenerate(make_valid_impl(self, &cleaned, config, first_valid));
        if config.keep_collapsed
            && matches!(&result, Geometry::GeometryCollection(gc) if gc.0.is_empty())
        {
            // Collapse preservation for keep_collapsed (the !keep_collapsed
            // block above is skipped on this path). Mirrors the panic-
            // containment branch; see test_shell_collapse_keep_collapsed.
            if let Some(c) = collapse_degenerate(self) {
                return c;
            }
        }
        result
    }
}

/// Check if any coordinate in a polygon is NaN or Infinity.
pub(super) fn has_nan_or_infinite(p: &Polygon<f64>) -> bool {
    p.exterior().0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
        || p.interiors().iter().any(|ring| {
            ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
        })
}

/// True when a single snap grid (SNAP_SCALE = 1e8) cannot represent the
/// polygon's coordinates: sub-grid features at the small end are destroyed
/// and integer keys lose precision beyond 2^53 at the large end. Zero
/// coordinates are ignored (a legitimate vertex); NaN is handled earlier.
///
/// "Destroyed" is calibrated to round-half-up: a coordinate c maps to
/// round(c * SNAP_SCALE) / SNAP_SCALE, so values below 0.5 grid units
/// collapse to zero (topology loss) while values at or above 0.5 survive
/// (possibly shifted by one grid unit). The guard therefore fires on
/// `min_abs * SNAP_SCALE < 0.5` — NOT < 1.0 — so borderline cases like a
/// 5.8e-9 coord in a 6.2e7 shell (0.58 grid units) stay on the snapping
/// path, which repairs them correctly (measured: fuzz
/// invariant_mixed_magnitude_polygon seed cc 5cf953d1 — the < 1.0
/// threshold routed them to arrange, which produced a self-intersecting
/// MultiPolygon).
///
/// Measured on differential fuzz against GEOS (2026-08-03): mixed-magnitude
/// polygons (1e-9 .. 1e7) repaired via the snapping single-pass produced
/// self-intersecting output and i_overlay panics on the boolean path. The
/// full-precision CDT Arrange path nodes at native f64 and handles both
/// ends. Only REPAIR inputs reach this guard (the fast path passes valid
/// topology through untouched, so a huge-but-valid polygon like
/// POLYGON((100 100, 1e15 110, 1e15 100, 100 100)) is unaffected).
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

/// Collapse a degenerate ring to lower-dimensional geometry, used only when
/// `keep_collapsed` is set and every repair path came back empty. A
/// fully-collinear ring (shoelace area zero) collapses to the LineString of
/// its deduplicated coords; a single distinct point collapses to a Point.
/// Mirrors GEOS MakeValid keepCollapsed semantics: collapsed geometry is
/// preserved as a lower dimension rather than dropped.
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

/// Gated arrange → precision-reduction → empty chain. Every arm of the
/// strategy dispatch funnels through this so no repair path can ship
/// geometry our validator rejects: measured on fuzz + differential runs,
/// the CDT path handles routed extreme-magnitude inputs correctly but can
/// still emit invalid output for degenerate ones, and the precision ladder
/// can too. The repair contract is "valid or empty", never broken.
#[cfg(any(feature = "arrange", feature = "structure"))]
fn arrange_chain(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    // Valid inputs pass through: the repair chain's collapse gates (sub-ULP
    // demotion, strip degeneracy) can destroy a valid thin polygon at mixed
    // coordinate magnitude. Measured 2026-08-04: a ring with a 4.9e208 spike
    // and a 1-unit base validated under both our validator and geo's, yet
    // every mode collapsed it to GEOMETRYCOLLECTION EMPTY (fuzz target
    // crash-eaab5472 — the structure fast path passed it, but direct
    // Arrange mode needs the same guarantee here). GEOS keeps such
    // polygons; so do we.
    if is_valid_with_geo(&Geometry::Polygon(poly.clone())) {
        return enforce_ogc_winding(Geometry::Polygon(poly.clone())).0;
    }
    // Normalize winding BEFORE the validity gate: repair outputs are
    // OGC-wound at the dispatch exit (enforce_ogc_winding after the match),
    // and CW shells are valid per GEOS but flagged WrongOrientation by our
    // validator — gating pre-winding would empty every CW-valid input
    // (measured: invariant_area_preserved, a valid CW triangle collapsed).
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
            PolyMethod::Arrange => arrange_chain(poly, config),
            PolyMethod::Structure => {
                let st = structure_fix_owned(poly.clone(), config, None);
                match st {
                    // Fast path: the gate is a COMPLETE certifier (2026-08-07)
                    // - winding is the only normalization needed, and the
                    // exit validator would re-run the same sweep. The
                    // winding-invariant checks survive re-winding, and the
                    // re-wound orientation is OGC-correct by construction.
                    crate::structure::FixOutcome::Fast(g) => {
                        // The gate certifies the winding-invariant checks;
                        // orientation is the only property the re-wind can
                        // invalidate (extreme-magnitude rings in the
                        // exact-orient ~0 zone - fuzz
                        // invariant_mixed_fp_in_same_ring). Verify comes
                        // from the enforce pass's extremal indices (no
                        // re-search - winding fusion, 2026-08-08);
                        // ambiguous orientation routes to arrange.
                        let (g_norm, ok) = enforce_ogc_winding(g);
                        if ok {
                            g_norm
                        } else {
                            warn!(
                                "Structure mode: fast-path orientation ambiguous, retrying with CDT arrange"
                            );
                            arrange_chain(poly, config)
                        }
                    }
                    crate::structure::FixOutcome::Repaired(g) => {
                        // Normalize winding BEFORE the validity gate: the
                        // fast path can pass a wrong-wound (CW) input
                        // through and CW shells are valid per GEOS but
                        // flagged WrongOrientation by our validator
                        // (measured: large valid shell + boundary-touching
                        // hole, speed_bug_regressions — gating pre-winding
                        // sent it to arrange, which decomposed the touch
                        // into a MultiPolygon).
                        let g_norm = enforce_ogc_winding(g).0;
                        if is_valid_with_geo(&g_norm) {
                            g_norm
                        } else {
                            warn!("Structure mode: fix output invalid, retrying with CDT arrange");
                            arrange_chain(poly, config)
                        }
                    }
                    crate::structure::FixOutcome::Unconsumed(p) => {
                        warn!("Structure mode: fix failed, retrying with CDT arrange");
                        arrange_chain(&p, config)
                    }
                }
            }
            PolyMethod::Auto => {
                match structure_fix_owned(poly.clone(), config, None) {
                    // Fast: complete-certifier gate (2026-08-07) - same
                    // argument as the Structure arm above.
                    crate::structure::FixOutcome::Fast(g) => {
                        let (g_norm, ok) = enforce_ogc_winding(g);
                        if ok {
                            g_norm
                        } else {
                            warn!(
                                "Auto mode: fast-path orientation ambiguous, falling back to CDT arrange"
                            );
                            arrange_chain(poly, config)
                        }
                    }
                    crate::structure::FixOutcome::Repaired(r) => {
                        // The structure path emits GEOS walker winding (CW
                        // shells, CCW holes - GEOS polygonizer convention).
                        // OGC validity requires CCW shells; normalize before
                        // the gate.
                        let r_norm = enforce_ogc_winding(r).0;
                        #[cfg(all(any(test, debug_assertions), feature = "std"))]
                        if std::env::var("DIAG_MV").is_ok() {
                            use geo::Area;
                            let ra = match &r_norm {
                                Geometry::Polygon(p) => p.unsigned_area(),
                                Geometry::MultiPolygon(mp) => {
                                    mp.0.iter().map(|p| p.unsigned_area()).sum()
                                }
                                Geometry::GeometryCollection(gc) => gc.0.iter().map(|x| match x {
                                    Geometry::Polygon(p) => p.unsigned_area(),
                                    _ => 0.0,
                                }).sum(),
                                _ => 0.0,
                            };
                            eprintln!(
                                "DIAG_MV auto: structure r={ra:.4} valid={}",
                                is_valid_with_geo(&r_norm)
                            );
                        }
                        if is_valid_with_geo(&r_norm) {
                            r_norm
                        } else {
                            warn!("Auto mode: structure_fix invalid, falling back to CDT arrange");
                            arrange_chain(poly, config)
                        }
                    }
                    crate::structure::FixOutcome::Unconsumed(p) => {
                        warn!("Auto mode: structure_fix failed, falling back to CDT arrange");
                        arrange_chain(&p, config)
                    }
                }
            }
        };
        // Every dispatch outcome is already OGC-wound (each arm winds
        // before returning), so the old re-wind here was a full no-op
        // pass over every ring - removed 2026-08-08.
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
    let result = match config.poly_method {
        PolyMethod::Arrange => arrange_chain(&poly, config),
        PolyMethod::Structure => {
            #[cfg(feature = "structure")]
            {
                match structure_fix_owned(poly, config, ext_scale) {
                    // Fast path: input was verified NaN-free by the caller's scan and
                    // non-degenerate by the gates — winding is the only normalization
                    // needed, and it cannot introduce NaNs. Skip has_nan/strip. The
                    // gate certifies the winding-invariant checks; verify the re-wound
                    // orientation O(n) (extreme-magnitude rings can sit in the
                    // exact-orient ~0 zone, fuzz invariant_mixed_fp_in_same_ring).
                    crate::structure::FixOutcome::Fast(g) => {
                        let (g, ok) = enforce_ogc_winding(g);
                        if ok {
                            return (g, true);
                        }
                        // The Fast geometry IS the input polygon (moved into
                        // the Geometry) - re-extract it for the arrange
                        // fallback rather than cloning pre-match.
                        warn!(
                            "Structure mode: fast-path orientation ambiguous, retrying with CDT arrange"
                        );
                        if let Geometry::Polygon(p) = g {
                            return (arrange_chain(&p, config), false);
                        }
                        return (g, false);
                    }
                    crate::structure::FixOutcome::Repaired(g) => g,
                    crate::structure::FixOutcome::Unconsumed(p) => {
                        warn!("Structure mode: fix failed, retrying with CDT arrange");
                        arrange_chain(&p, config)
                    }
                }
            }
            #[cfg(not(feature = "structure"))]
            {
                warn!("PolyMethod::Structure selected but 'structure' feature is not enabled. Enable the 'structure' feature in Cargo.toml to use Structure mode.");
                arrange_chain(&poly, config)
            }
        }
        PolyMethod::Auto => make_valid_impl(&poly, &poly, config, _first_valid),
    };
    let result = enforce_ogc_winding(result).0;
    if has_nan(&result) { (empty_geom::<f64>(), true) } else { (result, false) }
}

/// Owned twin of [`Polygon::make_valid_with_config`] for batch pipelines
/// that already own their polygons (e.g. [`crate::parallel::par_fix_polygon_batch_owned`]).
/// Moves the polygon through the Structure fast path — zero-copy for the
/// ~99.85% of real-world polygons that are already valid.
#[cfg(any(feature = "arrange", feature = "structure"))]
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
        // Bbox degeneracy is per-axis LOCAL: an axis is degenerate when
        // its extent is at or below the coordinate rounding at that
        // axis's own magnitude (eps = EPSILON * max |coord| on the
        // axis). The old rule compared both extents against the max
        // spread, so one distant spike dominated the other axis:
        // measured 2026-08-04, a VALID ring with a 4.9e208 y-spike and
        // a 1-unit x-extent (8 ULPs at 1e15) was emptied here in every
        // repair mode (fuzz crash-eaab5472). A ring whose extent is
        // below its own coordinate rounding has no representable area
        // and is genuinely degenerate.
        let x_scale = max_x.abs().max(min_x.abs());
        let y_scale = max_y.abs().max(min_y.abs());
        if (max_x - min_x).abs() <= f64::EPSILON * x_scale
            || (max_y - min_y).abs() <= f64::EPSILON * y_scale
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
            // Panic containment (mirrors the borrowed path): i_overlay can
            // assert on degenerate inputs; degrade to empty, never crash.
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
    let result = if verified { g } else { strip_degenerate(g) };
    if config.keep_collapsed
        && matches!(&result, Geometry::GeometryCollection(gc) if gc.0.is_empty())
    {
        // Collapse preservation for keep_collapsed — mirrors the borrowed
        // path (see make_valid_with_config).
        if let Some(c) = collapse_degenerate(&poly) {
            return c;
        }
    }
    result
}

/// Enforce OGC winding: CCW exterior, CW interior rings.
/// Consumes the geometry and rebuilds rings in place — no cloning: the
/// exterior and hole `LineString`s are moved out via `into_inner` and only
/// reversed when their winding is wrong. The previous implementation cloned
/// every ring unconditionally, which cost two `Vec` allocations per polygon
/// on the hot path (1.58M polygons in the real-world benchmark).
///
/// Returns `(geometry, orientation_ok)` — the bool is the old
/// `ogc_orientation_ok` verdict computed from the enforce pass's extremal
/// indices: the verify's per-ring extremal SEARCH is not re-run (the
/// orient and the rare ~0-zone shoelace fallback are recomputed fresh, so
/// the verdict is identical - winding fusion, 2026-08-08).
pub(crate) fn enforce_ogc_winding(g: Geometry<f64>) -> (Geometry<f64>, bool) {
    match g {
        Geometry::Polygon(p) => {
            let (ext, mut holes) = p.into_inner();
            let (ext, ext_idx, ext_rev) = enforce_ccw(ext);
            let mut ok = winding_ok(&ext.0, ext_idx, ext_rev, true);
            for h in holes.iter_mut() {
                let owned = core::mem::replace(h, geo::LineString::new(Vec::new()));
                let (hw, h_idx, h_rev) = enforce_cw(owned);
                ok &= winding_ok(&hw.0, h_idx, h_rev, false);
                *h = hw;
            }
            (Geometry::Polygon(Polygon::new(ext, holes)), ok)
        }
        Geometry::MultiPolygon(mp) => {
            let mut ok = true;
            let polys: Vec<Polygon<f64>> = mp
                .0
                .into_iter()
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

/// Post-winding orientation verdict for one ring, computed from the
/// enforce pass's extremal index. A reversed ring maps the index n - idx
/// (the closure point stays at index 0). The orient and the ~0-zone
/// shoelace fallback are recomputed on the current ring - identical to
/// what the old `ogc_orientation_ok`'s fresh search would produce (the
/// min vertex is order-independent; only the O(n) search is saved).
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

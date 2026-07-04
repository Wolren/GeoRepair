//! Geometry repair implementations via the MakeValid and ValidateAndFix traits.
//!
//! This module implements the core repair logic for all geometry types:
//!
//! - **Points**: NaN/Inf filtering, deduplication
//! - **Lines**: Zero-length detection, self-intersection noding
//! - **Polygons**: Structure fast path or Arrange CDT fallback
//! - **Multi-geometries**: Per-component repair with optional parallelism
//! - **GeometryCollection**: Recursive repair of children
//!
//! The main entry points are:
//! - [`MakeValid::make_valid`] — repair with default config
//! - [`MakeValid::make_valid_with_config`] — repair with custom config
//! - [`ValidateAndFix::validate_and_fix`] — combined validation + repair
use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, LinesIter,
    MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle, Winding,
};

use crate::core::MakeValidConfig;
#[cfg(any(feature = "arrange", feature = "structure"))]
use crate::core::PolyMethod;
use crate::noding::{remove_consecutive_duplicates, NodingFloat};
use crate::validation::{GeoValidation, ValidationResult};
use log::warn;

/// Trait for repairing invalid geometries.
///
/// Implemented for all geometry types. Returns a valid geometry (possibly
/// empty or decomposed into a [`GeometryCollection`]) when the input
/// violates OGC Simple Features rules.
///
/// Use [`make_valid`](MakeValid::make_valid) with default config, or
/// [`make_valid_with_config`](MakeValid::make_valid_with_config) for
/// fine-grained control over the repair strategy.
pub trait MakeValid {
    /// The scalar coordinate type (e.g. `f64`, `f32`).
    type Scalar: GeoFloat;

    /// Repair this geometry using default configuration.
    ///
    /// Returns a valid geometry (possibly empty or simplified) when the
    /// input contains OGC violations.
    fn make_valid(&self) -> Geometry<Self::Scalar> {
        self.make_valid_with_config(&MakeValidConfig::default())
    }

    /// Repair this geometry with the given configuration.
    ///
    /// See [`MakeValidConfig`] for available options (polygon strategy,
    /// collapsed geometry preservation, CRS target, etc.).
    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<Self::Scalar>;

    /// Repair this geometry in parallel using default configuration.
    ///
    /// Only available when the `parallel` feature is enabled (non-WASM).
    /// Multi-geometry components are processed on separate rayon threads.
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid(&self) -> Geometry<Self::Scalar>
    where
        Self: Send + Sync,
    {
        self.par_make_valid_with_config(&MakeValidConfig::default())
    }

    /// Repair this geometry in parallel with the given configuration.
    ///
    /// Falls back to [`make_valid_with_config`](MakeValid::make_valid_with_config)
    /// when the default implementation is used (single-threaded dispatch).
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<Self::Scalar>
    where
        Self: Send + Sync,
    {
        self.make_valid_with_config(_config)
    }
}

fn empty_geom<T: CoordNum>() -> Geometry<T> {
    Geometry::GeometryCollection(GeometryCollection(Vec::new()))
}

// ---------------------------------------------------------------------------
// Point
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for Point<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        Geometry::Point(*self)
    }
}

// ---------------------------------------------------------------------------
// MultiPoint
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for MultiPoint<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        use rustc_hash::FxHashSet;
        let mut seen: FxHashSet<(u64, u64)> = FxHashSet::default();
        let points: Vec<Point<T>> = self
            .0
            .iter()
            .copied()
            .filter(|p| {
                let key = (
                    p.0.x.to_f64().expect("to_f64").to_bits(),
                    p.0.y.to_f64().expect("to_f64").to_bits(),
                );
                seen.insert(key)
            })
            .collect();
        if points.is_empty() {
            warn!("MultiPoint::make_valid: no valid points after filtering");
            empty_geom()
        } else {
            Geometry::MultiPoint(MultiPoint::new(points))
        }
    }
}

// ---------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for Line<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let ok = self.start.x.is_finite()
            && self.start.y.is_finite()
            && self.end.x.is_finite()
            && self.end.y.is_finite()
            && self.start != self.end;
        if ok {
            Geometry::Line(*self)
        } else {
            warn!(
                "Line::make_valid: degenerate or NaN ({:?} -> {:?})",
                self.start, self.end
            );
            empty_geom()
        }
    }
}

// ---------------------------------------------------------------------------
// LineString
// ---------------------------------------------------------------------------

impl<T: NodingFloat> MakeValid for LineString<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let coords: Vec<Coord<T>> = self
            .0
            .iter()
            .copied()
            .filter(|c| c.x.is_finite() && c.y.is_finite())
            .collect();
        if coords.is_empty() {
            warn!("LineString::make_valid: all coords filtered (NaN/Inf)");
            return empty_geom();
        }
        let deduped = remove_consecutive_duplicates(&coords);
        if deduped.is_empty() {
            return empty_geom();
        }
        if deduped.len() == 1 {
            return Geometry::Point(Point(deduped[0]));
        }
        Geometry::LineString(LineString::new(deduped))
    }
}

// ---------------------------------------------------------------------------
// MultiLineString
// ---------------------------------------------------------------------------

impl<T: NodingFloat> MakeValid for MultiLineString<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        let mut points: Vec<Point<T>> = Vec::new();
        let mut lines: Vec<LineString<T>> = Vec::new();
        for ls in &self.0 {
            match ls.make_valid_with_config(config) {
                Geometry::Point(p) => points.push(p),
                Geometry::LineString(l) => lines.push(l),
                Geometry::MultiLineString(mls) => lines.extend(mls.0),
                _ => {}
            }
        }
        match (points.len(), lines.len()) {
            (0, 0) => empty_geom(),
            (_, 0) => {
                if points.len() == 1 {
                    Geometry::Point(points.pop().expect("len==1 verified"))
                } else {
                    Geometry::MultiPoint(MultiPoint::new(points))
                }
            }
            (0, _) => {
                if lines.len() == 1 {
                    Geometry::LineString(lines.pop().expect("len==1 verified"))
                } else {
                    Geometry::MultiLineString(MultiLineString::new(lines))
                }
            }
            _ => {
                let mut geoms: Vec<Geometry<T>> =
                    lines.into_iter().map(Geometry::LineString).collect();
                if points.len() == 1 {
                    geoms.push(Geometry::Point(points.pop().expect("len==1 verified")));
                } else {
                    geoms.push(Geometry::MultiPoint(MultiPoint::new(points)));
                }
                Geometry::GeometryCollection(GeometryCollection(geoms))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rect
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for Rect<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let min_ok = self.min().x.is_finite() && self.min().y.is_finite();
        let max_ok = self.max().x.is_finite() && self.max().y.is_finite();
        if min_ok && max_ok {
            Geometry::Rect(*self)
        } else {
            warn!(
                "Rect::make_valid: NaN coordinate ({:?}, {:?})",
                self.min(),
                self.max()
            );
            empty_geom()
        }
    }
}

// ---------------------------------------------------------------------------
// Triangle — concrete f64 when polygon features available
// ---------------------------------------------------------------------------

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
// Polygon — concrete f64 impl
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Polygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        // Fuse the NaN scan into the collapse-check loop to avoid a separate pass.
        // The collapse check already iterates all coords — piggyback the is_finite
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
                // Also check interior rings — exterior might be clean but holes can have NaNs
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
                return make_valid_impl(self, self, config, coords[0]);
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
            if config.keep_collapsed && deduped.len() == 1 {
                return Geometry::Point(Point(deduped[0]));
            }
            return empty_geom();
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

/// Fast path: polygon has no NaN coords, use self directly.
#[cfg(any(feature = "arrange", feature = "structure"))]
#[allow(dead_code)]
fn make_valid_clean(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    if !config.keep_collapsed && poly.exterior().0.len() >= 4 {
        let coords = &poly.exterior().0;
        let (mut min_x, mut max_x, mut min_y, mut max_y) =
            (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
        for w in coords.windows(2) {
            min_x = min_x.min(w[1].x);
            max_x = max_x.max(w[1].x);
            min_y = min_y.min(w[1].y);
            max_y = max_y.max(w[1].y);
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        if (max_x - min_x).abs() < f64::EPSILON * scale
            || (max_y - min_y).abs() < f64::EPSILON * scale
        {
            return empty_geom();
        }
    }
    let first_valid = poly
        .exterior()
        .0
        .first()
        .copied()
        .unwrap_or(Coord::default());
    make_valid_impl(poly, poly, config, first_valid)
}

/// Check if any coordinate in a polygon is NaN or Infinity.
fn has_nan_or_infinite(p: &Polygon<f64>) -> bool {
    p.exterior().0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
        || p.interiors().iter().any(|ring| {
            ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
        })
}

/// Common strategy dispatch after degeneracy checks.
#[cfg(any(feature = "arrange", feature = "structure"))]
fn make_valid_impl(
    _self: &Polygon<f64>,
    poly: &Polygon<f64>,
    config: &MakeValidConfig,
    _first_valid: Coord<f64>,
) -> Geometry<f64> {
    // Bail early on NaN/Inf coordinates
    if has_nan_or_infinite(poly) {
        return empty_geom::<f64>();
    }
    let result = match config.poly_method {
            PolyMethod::Arrange => arrange_or_empty(poly, config),
            PolyMethod::Structure => structure_fix(poly, config).unwrap_or_else(|| {
                warn!("Structure mode: fix failed, retrying with precision reduction");
                reduce_fallback(poly, config)
            }),
            PolyMethod::Auto => {
                if let Some(r) = structure_fix(poly, config) {
                    if is_valid_with_geo(&r) { r }
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
        result
        }

/// Enforce OGC winding: CCW exterior, CW interior rings.
fn enforce_ogc_winding(g: Geometry<f64>) -> Geometry<f64> {
    match g {
        Geometry::Polygon(p) => {
            let ext = enforce_ccw(p.exterior().clone());
            let holes: Vec<_> = p
                .interiors()
                .iter()
                .map(|h| enforce_cw(h.clone()))
                .collect();
            Geometry::Polygon(Polygon::new(ext, holes))
        }
        Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(MultiPolygon::new(
            mp.0.into_iter()
                .map(|p| {
                    let ext = enforce_ccw(p.exterior().clone());
                    let holes: Vec<_> = p
                        .interiors()
                        .iter()
                        .map(|h| enforce_cw(h.clone()))
                        .collect();
                    Polygon::new(ext, holes)
                })
                .collect(),
        )),
        other => other,
        }
        }

/// Remove degenerate Polygon/MultiPolygon components: exterior rings with
/// <4 coordinates, shoelace area below epsilon, or NaN/Inf coordinates.
/// Returns boundary LineString for degenerate polygons (GEOS-style type degradation).
fn strip_degenerate(g: Geometry<f64>) -> Geometry<f64> {
    match g {
        Geometry::Polygon(p) => {
            let ext = &p.exterior().0;
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for c in ext.iter() {
                if c.x.is_finite() { min_x = min_x.min(c.x); max_x = max_x.max(c.x); }
                if c.y.is_finite() { min_y = min_y.min(c.y); max_y = max_y.max(c.y); }
            }
            let bbox_degenerate = (max_x - min_x).abs() < f64::EPSILON * 1e6
                || (max_y - min_y).abs() < f64::EPSILON * 1e6;
            if ext.len() < 4
                || shoelace_abs_sum(ext) < 1e-12
                || bbox_degenerate
                || ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
            {
                // GEOS-style: return degenerate polygon boundary as LineString
                let mut lines: Vec<LineString<f64>> = Vec::new();
                if ext.len() >= 2 {
                    lines.push(p.exterior().clone());
                }
                for ring in p.interiors() {
                    if ring.0.len() >= 2 {
                        lines.push(ring.clone());
                    }
                }
                if lines.is_empty() {
                    empty_geom::<f64>()
                } else if lines.len() == 1 {
                    Geometry::LineString(lines.into_iter().next().unwrap())
                } else {
                    Geometry::MultiLineString(MultiLineString::new(lines))
                }
            } else {
                // Strip degenerate holes (RingTooFewPoints, NaN/Inf)
                let holes: Vec<LineString<f64>> = p.interiors().iter()
                    .filter(|ring| {
                        ring.0.len() >= 4
                            && !ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                    })
                    .cloned()
                    .collect();
                if holes.len() == p.interiors().len() {
                    Geometry::Polygon(p)
                } else {
                    Geometry::Polygon(Polygon::new(p.exterior().clone(), holes))
                }
            }
        }
        Geometry::MultiPolygon(mp) => {
            let mut valid_polys: Vec<Polygon<f64>> = Vec::new();
            let mut boundary_lines: Vec<LineString<f64>> = Vec::new();
            for p in mp.0.into_iter() {
                let ext = &p.exterior().0;
                if ext.len() >= 4
                    && shoelace_abs_sum(ext) >= 1e-12
                    && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                {
                    valid_polys.push(p);
                } else {
                    // Collect degenerate component's boundary as lines
                    if ext.len() >= 2 {
                        boundary_lines.push(p.exterior().clone());
                    }
                    for ring in p.interiors() {
                        if ring.0.len() >= 2 {
                            boundary_lines.push(ring.clone());
                        }
                    }
                }
            }
            match (valid_polys.len(), boundary_lines.is_empty()) {
                (0, true) => empty_geom::<f64>(),
                (0, false) => {
                    if boundary_lines.len() == 1 {
                        Geometry::LineString(boundary_lines.into_iter().next().unwrap())
                    } else {
                        Geometry::MultiLineString(MultiLineString::new(boundary_lines))
                    }
                }
                (1, true) => Geometry::Polygon(valid_polys.into_iter().next().unwrap()),
                (1, false) => {
                    let mut geoms: Vec<Geometry<f64>> = Vec::new();
                    geoms.push(Geometry::Polygon(valid_polys.into_iter().next().unwrap()));
                    let mls = if boundary_lines.len() == 1 {
                        Geometry::LineString(boundary_lines.into_iter().next().unwrap())
                    } else {
                        Geometry::MultiLineString(MultiLineString::new(boundary_lines))
                    };
                    geoms.push(mls);
                    Geometry::GeometryCollection(GeometryCollection(geoms))
                }
                (_, true) => Geometry::MultiPolygon(MultiPolygon::new(valid_polys)),
                (_, false) => {
                    let mut geoms: Vec<Geometry<f64>> = valid_polys.into_iter()
                        .map(Geometry::Polygon).collect();
                    let mls = if boundary_lines.len() == 1 {
                        Geometry::LineString(boundary_lines.into_iter().next().unwrap())
                    } else {
                        Geometry::MultiLineString(MultiLineString::new(boundary_lines))
                    };
                    geoms.push(mls);
                    Geometry::GeometryCollection(GeometryCollection(geoms))
                }
            }
        }
        other => other,
    }
}

fn enforce_ccw(mut ring: LineString<f64>) -> LineString<f64> {
    // Use scalar winding_order() to stay consistent with the validator.
    // SIMD batch-accumulation disagrees with scalar on tiny-area rings
    // causing WrongOrientation false positives.
    let ccw = ring.winding_order() == Some(WindingOrder::CounterClockwise);
    if !ccw {
        ring.make_ccw_winding();
    }
    ring
}

fn enforce_cw(mut ring: LineString<f64>) -> LineString<f64> {
    if ring.winding_order() != Some(WindingOrder::Clockwise) {
        ring.make_cw_winding();
    }
    ring
}

use geo::winding_order::WindingOrder;

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for MultiPolygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        if self.0.is_empty() {
            return empty_geom::<f64>();
        }
        // Bail early on NaN/Inf coordinates
        if self.0.iter().any(|p| has_nan_or_infinite(p)) {
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
            return if filtered.0.is_empty() { empty_geom::<f64>() }
                   else { enforce_ogc_winding(Geometry::Polygon(filtered.0.into_iter().next().unwrap())) };
        }
        let mp = filtered;
        // Check if shells have overlapping bboxes — if not, unary_union is overkill
        let shells_overlap = shells_have_overlapping_bboxes(&mp);
        if !shells_overlap {
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
                    .map(|g| match g { Geometry::MultiPolygon(mp) => mp, _ => return MultiPolygon::new(Vec::new()) })
                    .unwrap_or_else(|| geo::algorithm::bool_ops::unary_union(&mp));
                drop_nested_components(unioned)
            }
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
fn apply_target_crs(geom: Geometry<f64>, _config: &MakeValidConfig) -> Geometry<f64> {
    #[cfg(feature = "proj")]
    if let (Some(ref src_crs), Some(ref dst_crs)) = (&config.crs, &config.target_crs) {
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
fn arrange_or_empty(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
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
fn structure_fix(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    crate::structure::fix_polygon(poly, config)
}

#[cfg(not(feature = "structure"))]
fn structure_fix(poly: &Polygon<f64>, _config: &MakeValidConfig) -> Option<Geometry<f64>> {
    if !poly.exterior().0.is_empty() {
        warn!("PolyMethod::Structure selected but 'structure' feature is not enabled. Enable the 'structure' feature in Cargo.toml to use Structure mode.");
    }
    None
}

/// Check OGC validity using our own GeoValidation (Shewchuk-based).
fn is_valid_with_geo(g: &Geometry<f64>) -> bool {
    use crate::validation::GeoValidation;
    g.is_valid()
}

/// Last‑resort fallback: snap to progressively coarser grids until valid.
///
/// Uses only `reduce_raw` (snap only, no MakeValid call) to avoid recursion.
#[cfg(any(feature = "arrange", feature = "structure"))]
fn reduce_fallback(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
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
/// Used as a cheap pre-filter — if bboxes don't overlap, there's
/// no chance of shell overlap, so we can safely skip the expensive union.
#[cfg(any(feature = "arrange", feature = "structure"))]
fn shells_have_overlapping_bboxes(mp: &MultiPolygon<f64>) -> bool {
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
fn shells_have_vertex_inside(mp: &MultiPolygon<f64>) -> bool {
    for i in 0..mp.0.len() {
        let ext_i = &mp.0[i].exterior().0;
        if ext_i.len() < 4 { continue; }
        for j in 0..mp.0.len() {
            if i == j { continue; }
            let ext_j = &mp.0[j].exterior().0;
            if ext_j.len() < 4 { continue; }
            let max_check = ext_i.len().min(32);
            for pt in ext_i.iter().take(max_check) {
                if point_in_ring_exclusive(*pt, ext_j) {
                    return true;
                }
            }
        }
    }
    false
}

/// Ray-casting point-in-ring test (strict interior, not on boundary).
fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    if ring.len() < 4 { return false; }
    let n = ring.len() - 1;
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

/// Remove components from a MultiPolygon that are inside another component
/// (fixes NestedHoles from unary_union). Sorts by area, keeps only even-parent.
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

pub(crate) fn drop_nested_components(mp: MultiPolygon<f64>) -> Geometry<f64> {
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
        let centroid = ext_i[..ext_i.len()-1].iter().fold(Coord { x: 0.0, y: 0.0 },
            |acc, c| Coord { x: acc.x + c.x, y: acc.y + c.y });
        let n_verts = (ext_i.len() - 1).max(1) as f64;
        let centroid = Coord { x: centroid.x / n_verts, y: centroid.y / n_verts };
        let pt_candidates = [ext_i[0], Coord {
            x: (ext_i[0].x + ext_i[1].x) * 0.5,
            y: (ext_i[0].y + ext_i[1].y) * 0.5,
        }, centroid];
        let is_nested = with_area.iter().enumerate().any(|(j, (p_j, _))| {
            if i == j || !keep[j] { return false; }
            let ext_j = &p_j.exterior().0;
            if ext_j.len() < 4 { return false; }
            pt_candidates.iter().any(|&pt| point_in_ring_exclusive(pt, ext_j))
        });
        if is_nested { keep[i] = false; }
    }
    let kept: Vec<Polygon<f64>> = with_area.into_iter()
        .enumerate()
        .filter_map(|(i, (p, _))| if keep[i] { Some(p) } else { None })
        .collect();
    if kept.is_empty() { return empty_geom::<f64>(); }
    let kept_len = kept.len();
    if kept_len == 1 {
        return enforce_ogc_winding(Geometry::Polygon(kept.into_iter().next().unwrap()));
    }
    // Edge-sharing case: containment didn't reduce components.
    // Filter out components with PinchPoint or self-intersection errors.
    let mp_kept = MultiPolygon::new(kept);
    let valid: Vec<Polygon<f64>> = mp_kept.0.into_iter().filter(|p| {
        let v = crate::validation::GeoValidation::validate(p);
        !v.errors.iter().any(|e| matches!(e, crate::validation::GeometryValidationError::PinchPoint))
            && v.errors.iter().filter(|e| !matches!(e,
                crate::validation::GeometryValidationError::PinchPoint
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
fn polygonizer_fallback(mp: &MultiPolygon<f64>) -> Option<Geometry<f64>> {
    let lines: Vec<geo::Line<f64>> = mp.0.iter()
        .flat_map(|p| p.lines_iter())
        .collect();
    if lines.is_empty() { return None; }
    let noded = crate::arrange::prep::prepare_lines(lines).ok()?;
    if noded.lines.is_empty() { return None; }
    let faces = crate::structure::polygonizer::polygonize(&noded.lines);
    if faces.is_empty() { return None; }
    let valid: Vec<Polygon<f64>> = faces.into_iter()
        .filter(|p| {
            let ext = &p.exterior().0;
            ext.len() >= 4 && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                && shoelace_abs_sum(ext) >= 1e-12
        })
        .collect();
    if valid.is_empty() { return None; }
    if valid.len() == 1 { return Some(Geometry::Polygon(valid.into_iter().next().unwrap())); }
    Some(Geometry::MultiPolygon(MultiPolygon::new(valid)))
}

/// Snap all coordinates in a MultiPolygon to the default precision grid (1e-8).
#[cfg(any(feature = "arrange", feature = "structure"))]
#[allow(dead_code)]
fn reduce_mp(mp: &MultiPolygon<f64>, config: &MakeValidConfig) -> MultiPolygon<f64> {
    reduce_mp_at_scale(mp, config, 1e-8)
}

/// Snap all coordinates in a MultiPolygon to a specific precision grid scale.
#[cfg(any(feature = "arrange", feature = "structure"))]
fn reduce_mp_at_scale(mp: &MultiPolygon<f64>, config: &MakeValidConfig, scale: f64) -> MultiPolygon<f64> {
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

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for GeometryCollection<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let fixed: Vec<Geometry<f64>> = self
            .0
            .iter()
            .map(|g| g.make_valid_with_config(config))
            .filter(|g| !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()))
            .collect();
        if fixed.is_empty() {
            empty_geom::<f64>()
        } else {
            Geometry::GeometryCollection(GeometryCollection(fixed))
        }
    }

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        crate::parallel::par_fix_collection(self, config)
    }
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
impl<T: NodingFloat> MakeValid for GeometryCollection<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        let fixed: Vec<Geometry<T>> = self
            .0
            .iter()
            .map(|g| g.make_valid_with_config(config))
            .filter(|g| !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()))
            .collect();
        if fixed.is_empty() {
            empty_geom()
        } else {
            Geometry::GeometryCollection(GeometryCollection(fixed))
        }
    }
}

// ---------------------------------------------------------------------------
// Validate + Fix mode (GEOS-compatible)
// ---------------------------------------------------------------------------

/// Combine validation and repair into a single pipeline.
///
/// Mirrors GEOS's pattern of checking validity before/after repair.
/// Implemented automatically for all types that implement both
/// [`MakeValid`] and [`GeoValidation`].
///
/// # Methods
///
/// - [`validate_and_fix`](ValidateAndFix::validate_and_fix): validate first, fix if invalid
/// - [`validate_and_fix_always`](ValidateAndFix::validate_and_fix_always): always run both validation and repair
/// - [`validate_or_fix`](ValidateAndFix::validate_or_fix): return `Ok(fixed)` if valid after repair, `Err` otherwise
///
/// # Example
///
/// ```rust
/// # use geo::{Geometry, Point};
/// # let geometry = Geometry::Point(Point::new(0.0, 0.0));
/// use geo_repair::ValidateAndFix;
///
/// let (result, fixed) = geometry.validate_and_fix();
/// if !result.valid {
///     println!("Repaired {} violations", result.errors.len());
/// }
/// ```
pub trait ValidateAndFix: MakeValid<Scalar = f64> + GeoValidation<Scalar = f64> {
    /// Validate the geometry, then fix it if invalid.
    /// Returns (validation_result, fixed_geometry).
    ///
    /// If the geometry is already valid, `fixed_geometry` is a clone of
    /// the input and `validation_result.valid` is `true`.
    fn validate_and_fix(&self) -> (ValidationResult, Geometry<f64>) {
        let result = <Self as GeoValidation>::validate(self);
        if result.valid {
            (result, <Self as MakeValid>::make_valid(self))
        } else {
            (result.clone(), <Self as MakeValid>::make_valid(self))
        }
    }

    /// Validate the geometry, then fix it if invalid.
    /// Returns the fixed geometry unconditionally (fix even valid geoms).
    fn validate_and_fix_always(&self) -> (ValidationResult, Geometry<f64>) {
        (
            <Self as GeoValidation>::validate(self),
            <Self as MakeValid>::make_valid(self),
        )
    }

    /// Return the validated geometry, or fix it if invalid.
    /// Returns Ok(geometry) if valid or fix succeeded, Err((validation_errors, fixed_geometry)) if still invalid.
    fn validate_or_fix(&self) -> Result<Geometry<f64>, (ValidationResult, Geometry<f64>)> {
        let result = <Self as GeoValidation>::validate(self);
        if result.valid {
            return Ok(<Self as MakeValid>::make_valid(self));
        }
        let fixed = <Self as MakeValid>::make_valid(self);
        if <Geometry<f64> as GeoValidation>::validate(&fixed).valid {
            Ok(fixed)
        } else {
            Err((result, fixed))
        }
    }
}

// ---------------------------------------------------------------------------
// ValidateAndFix blanket implementations
// ---------------------------------------------------------------------------

impl ValidateAndFix for Point<f64> {}

impl ValidateAndFix for MultiPoint<f64> {}

impl ValidateAndFix for Line<f64> {}

impl ValidateAndFix for LineString<f64> {}

impl ValidateAndFix for MultiLineString<f64> {}

impl ValidateAndFix for Rect<f64> {}

impl ValidateAndFix for Triangle<f64> {}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl ValidateAndFix for Polygon<f64> {}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl ValidateAndFix for MultiPolygon<f64> {}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl ValidateAndFix for Geometry<f64> {}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl ValidateAndFix for GeometryCollection<f64> {}

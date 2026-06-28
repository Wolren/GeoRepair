use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle,
};

use crate::core::{MakeValidConfig, PolyMethod};
use crate::noding::{node_line_string, remove_consecutive_duplicates, NodingFloat};
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
    type Scalar: GeoFloat;

    fn make_valid(&self) -> Geometry<Self::Scalar> {
        self.make_valid_with_config(&MakeValidConfig::default())
    }

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<Self::Scalar>;

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid(&self) -> Geometry<Self::Scalar>
    where
        Self: Send + Sync,
    {
        self.par_make_valid_with_config(&MakeValidConfig::default())
    }

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
        let c = self.0;
        if c.x.is_finite() && c.y.is_finite() {
            Geometry::Point(*self)
        } else {
            warn!("Point::make_valid: NaN coordinate ({:?})", c);
            empty_geom()
        }
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
                let x_ok = p.0.x.is_finite();
                let y_ok = p.0.y.is_finite();
                if !(x_ok && y_ok) {
                    return false;
                }
                let key = (
                    p.0.x.to_f64().unwrap().to_bits(),
                    p.0.y.to_f64().unwrap().to_bits(),
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
        node_line_string(&LineString::new(deduped))
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
                    Geometry::Point(points.into_iter().next().unwrap())
                } else {
                    Geometry::MultiPoint(MultiPoint::new(points))
                }
            }
            (0, _) => {
                if lines.len() == 1 {
                    Geometry::LineString(lines.into_iter().next().unwrap())
                } else {
                    Geometry::MultiLineString(MultiLineString::new(lines))
                }
            }
            _ => {
                let mut geoms: Vec<Geometry<T>> =
                    lines.into_iter().map(Geometry::LineString).collect();
                if points.len() == 1 {
                    geoms.push(Geometry::Point(points.into_iter().next().unwrap()));
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
        if !config.keep_collapsed && self.exterior().0.len() >= 4 {
            let coords = &self.exterior().0;
            let mut min_x = coords[0].x;
            let mut max_x = coords[0].x;
            let mut min_y = coords[0].y;
            let mut max_y = coords[0].y;
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

        match config.poly_method {
            PolyMethod::Arrange => {
                let result = arrange_or_empty(self, config);
                if is_valid_with_geo(&result) {
                    return result;
                }
                warn!("Arrange mode: result invalid, retrying with precision reduction");
                reduce_fallback(self, config)
            }
            PolyMethod::Structure => structure_fix(self, config).unwrap_or_else(|| {
                warn!("Structure mode: fix failed, retrying with precision reduction");
                reduce_fallback(self, config)
            }),
            PolyMethod::Auto => {
                if let Some(result) = structure_fix(self, config) {
                    if is_valid_with_geo(&result) {
                        return result;
                    }
                    warn!("Auto mode: structure_fix produced invalid output, falling back to CDT arrange");
                }
                let arranged = arrange_or_empty(self, config);
                if is_valid_with_geo(&arranged) {
                    return arranged;
                }
                warn!("Auto mode: arrange also invalid, retrying with precision reduction");
                reduce_fallback(self, config)
            }
        }
    }
}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for MultiPolygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let polys: Vec<Geometry<f64>> = self
            .0
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
            return empty_geom::<f64>();
        }
        if shells.len() == 1 {
            // Safe: len==1 verified above on local Vec
            return Geometry::Polygon(shells.into_iter().next().unwrap());
        }
        let mp = MultiPolygon::new(shells);
        Geometry::MultiPolygon(geo::algorithm::bool_ops::unary_union(&mp))
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
impl<T: GeoFloat> MakeValid for Geometry<T> {
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
    crate::arrange::fix_polygon(poly, config)
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

/// Check OGC validity using the `geo` crate's Validation trait (same as
/// test assertions). This is the authoritative validity check for the pipeline.
fn is_valid_with_geo(g: &Geometry<f64>) -> bool {
    <Geometry<f64> as geo::validation::Validation>::is_valid(g)
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
impl<T: GeoFloat> MakeValid for GeometryCollection<T> {
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
/// Mirrors GEOS's pattern of checking validity before/comparing to repair.
pub trait ValidateAndFix: MakeValid<Scalar = f64> + GeoValidation<Scalar = f64> {
    /// Validate the geometry, then fix it if invalid.
    /// Returns (validation_result, fixed_geometry).
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

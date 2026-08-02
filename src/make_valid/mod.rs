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
//! - [`MakeValid::make_valid`] - repair with default config
//! - [`MakeValid::make_valid_with_config`] - repair with custom config
use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString,
    MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle, Winding,
};
use crate::util::{point_in_ring_exclusive_even_odd, shoelace_abs_sum};

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
        // POINT(NaN NaN) is the canonical empty point - valid, unchanged.
        // A point with a single non-finite ordinate is invalid (GEOS
        // "Invalid Coordinate"); makeValid demotes it to the canonical
        // empty point so the output is always valid.
        if (self.x().is_finite() && self.y().is_finite())
            || (self.x().is_nan() && self.y().is_nan())
        {
            Geometry::Point(*self)
        } else {
            Geometry::Point(Point::new(T::nan(), T::nan()))
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
                // GEOS parity: a point with a single non-finite ordinate is
                // invalid (CoordinateNaN) and makeValid drops it; a true
                // empty point (NaN, NaN) is valid and preserved. Dedup
                // unchanged.
                let finite = p.0.x.is_finite() && p.0.y.is_finite();
                let empty_point = p.0.x.is_nan() && p.0.y.is_nan();
                if !(finite || empty_point) {
                    return false;
                }
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
                // Degenerate (zero-area) rect → empty
                if (self.max().x - self.min().x).abs() < T::epsilon()
                    || (self.max().y - self.min().y).abs() < T::epsilon()
                {
                    empty_geom()
                } else {
                    Geometry::Rect(*self)
                }
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
// Triangle - concrete f64 when polygon features available
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
mod multipolygon;
mod polygon;
mod strip;

pub use multipolygon::drop_nested_components;
pub use polygon::is_valid_with_geo;
pub use strip::strip_degenerate_test;

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

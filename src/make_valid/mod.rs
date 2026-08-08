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

use crate::util::{point_in_ring_exclusive_even_odd, shoelace_abs_sum};
use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle, Winding,
};

use crate::core::MakeValidConfig;
#[cfg(any(feature = "arrange", feature = "structure"))]
use crate::core::PolyMethod;
use crate::noding::{NodingFloat, remove_consecutive_duplicates};
use crate::validation::edges::{edges_intersect_general, edges_vertex_on_edge};
use crate::validation::impls::{
    check_line_components_intersect, check_linestring_self_intersection, segments_collinear_overlap,
};
use crate::validation::{GeoValidation, ValidationResult};
use alloc::vec::Vec;
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

pub(crate) fn empty_geom<T: CoordNum>() -> Geometry<T> {
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
        if deduped.len() >= 3 {
            // Validity contract: the output must be simple. The old path
            // passed any non-simple line through unchanged (GEOS MakeValid
            // does the same for lines, but our repair contract is
            // valid-or-empty - the fuzz smoke caught a denormal-scale
            // self-overlapping LINESTRING shipped as NotSimple). Check with
            // the validator's own predicate, then repair by dropping the
            // conflicting segments (f64 space is lossless for f32 too).
            let fd: Vec<Coord<f64>> = deduped
                .iter()
                .map(|c| Coord {
                    x: c.x.to_f64().unwrap_or(f64::NAN),
                    y: c.y.to_f64().unwrap_or(f64::NAN),
                })
                .collect();
            if check_linestring_self_intersection(&fd) {
                // Noding repair first: split at every self-intersection
                // instead of dropping segments (preserves the full
                // traversal; the lean noder is also an order of magnitude
                // faster than the greedy filter on dense crossings). The
                // noder validates its own output and returns None when it
                // cannot guarantee a valid result - the greedy filter is
                // the fallback.
                let runs = match crate::noding::line::node_line(&fd) {
                    Some(runs) => runs,
                    None => simple_subline(&fd),
                };
                let out: Vec<LineString<T>> = runs
                    .into_iter()
                    .map(|r| {
                        LineString::new(
                            r.into_iter()
                                .map(|c| Coord {
                                    x: <T as num_traits::NumCast>::from(c.x)
                                        .expect("f64 coord converts back to T"),
                                    y: <T as num_traits::NumCast>::from(c.y)
                                        .expect("f64 coord converts back to T"),
                                })
                                .collect(),
                        )
                    })
                    .collect();
                return match out.len() {
                    0 => empty_geom(),
                    1 => Geometry::LineString(out.into_iter().next().expect("len==1 verified")),
                    _ => Geometry::MultiLineString(MultiLineString::new(out)),
                };
            }
        }
        Geometry::LineString(LineString::new(deduped))
    }
}

/// Greedy simplification of a non-simple line: walk the segments, keep a
/// segment unless it conflicts with an already-kept segment, using the same
/// pairwise tests as [`check_linestring_self_intersection`] (adjacent
/// segments may touch only at their shared vertex; collinear overlap beyond
/// it is a conflict; non-adjacent segments conflict on any intersection,
/// including vertex revisits). Returns the maximal kept runs as coordinate
/// chains; the result is simple by construction.
fn simple_subline(coords: &[Coord<f64>]) -> Vec<Vec<Coord<f64>>> {
    let n = coords.len() - 1;
    if n < 2 {
        return vec![coords.to_vec()];
    }
    let closed = coords[0] == coords[n];
    let scale = {
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for c in coords {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0)
    };
    let eps = 1e-12 * scale;
    // Bbox prefilter: cheap reject before the robust predicates (the
    // repair path only runs on non-simple lines; keep it O(1) per pair in
    // the common non-overlapping case).
    let bboxes: Vec<(f64, f64, f64, f64)> = (0..n)
        .map(|i| {
            let a = coords[i];
            let b = coords[i + 1];
            (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y))
        })
        .collect();
    let mut kept: Vec<usize> = Vec::new();
    'seg: for j in 0..n {
        let (bx0, bx1, by0, by1) = bboxes[j];
        for &i in &kept {
            let (ax0, ax1, ay0, ay1) = bboxes[i];
            if ax1 < bx0 || bx1 < ax0 || ay1 < by0 || by1 < ay0 {
                continue; // disjoint bboxes: no intersection possible
            }
            let adjacent = (i as isize - j as isize).abs() == 1
                || (closed && ((i == 0 && j == n - 1) || (i == n - 1 && j == 0)));
            if adjacent {
                if segments_collinear_overlap(
                    coords[i],
                    coords[i + 1],
                    coords[j],
                    coords[j + 1],
                    eps,
                ) {
                    continue 'seg;
                }
            } else {
                let shared = coords[i] == coords[j]
                    || coords[i] == coords[j + 1]
                    || coords[i + 1] == coords[j]
                    || coords[i + 1] == coords[j + 1];
                if shared
                    || edges_intersect_general(
                        coords[i],
                        coords[i + 1],
                        coords[j],
                        coords[j + 1],
                        eps,
                    )
                    || edges_vertex_on_edge(coords[i], coords[i + 1], coords[j], coords[j + 1])
                {
                    continue 'seg;
                }
            }
        }
        kept.push(j);
    }
    // Group consecutive kept segment indices into coordinate runs.
    let mut runs: Vec<Vec<Coord<f64>>> = Vec::new();
    let mut run: Vec<Coord<f64>> = Vec::new();
    for (idx, &s) in kept.iter().enumerate() {
        if idx == 0 {
            run.push(coords[s]);
            run.push(coords[s + 1]);
        } else if s == kept[idx - 1] + 1 {
            run.push(coords[s + 1]);
        } else {
            runs.push(core::mem::take(&mut run));
            run.push(coords[s]);
            run.push(coords[s + 1]);
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
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
        // Validity contract: components must not intersect each other
        // except at boundary points (mirror of the validator's
        // check_line_components_intersect rule). Greedy keep: drop a
        // component that conflicts with an already-kept one.
        if lines.len() > 1 {
            let fd: Vec<Vec<Coord<f64>>> = lines
                .iter()
                .map(|ls| {
                    ls.0.iter()
                        .map(|c| Coord {
                            x: c.x.to_f64().unwrap_or(f64::NAN),
                            y: c.y.to_f64().unwrap_or(f64::NAN),
                        })
                        .collect()
                })
                .collect();
            let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) =
                (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for ls in &fd {
                for c in ls {
                    gmin_x = gmin_x.min(c.x);
                    gmax_x = gmax_x.max(c.x);
                    gmin_y = gmin_y.min(c.y);
                    gmax_y = gmax_y.max(c.y);
                }
            }
            let scale = (gmax_x - gmin_x)
                .abs()
                .max((gmax_y - gmin_y).abs())
                .max(1.0);
            let eps = 1e-12 * scale;
            // Bbox prefilter: most component pairs are disjoint; reject
            // them before the per-edge checks (measured 2026-08-07: mls
            // 50x3v went 1.7 -> 18.5 µs when the cross-component filter
            // ran check_line_components_intersect on every pair).
            let bboxes: Vec<(f64, f64, f64, f64)> = fd
                .iter()
                .map(|ls| {
                    let mut min_x = f64::MAX;
                    let mut max_x = f64::MIN;
                    let mut min_y = f64::MAX;
                    let mut max_y = f64::MIN;
                    for c in ls {
                        min_x = min_x.min(c.x);
                        max_x = max_x.max(c.x);
                        min_y = min_y.min(c.y);
                        max_y = max_y.max(c.y);
                    }
                    (min_x, max_x, min_y, max_y)
                })
                .collect();
            let mut kept: Vec<usize> = Vec::new();
            for j in 0..fd.len() {
                let (bx0, bx1, by0, by1) = bboxes[j];
                let conflict = kept.iter().any(|&i| {
                    let (ax0, ax1, ay0, ay1) = bboxes[i];
                    if ax1 < bx0 || bx1 < ax0 || ay1 < by0 || by1 < ay0 {
                        return false; // disjoint bboxes
                    }
                    check_line_components_intersect(&fd[i], &fd[j], eps)
                });
                if !conflict {
                    kept.push(j);
                }
            }
            if kept.len() != fd.len() {
                lines = kept.into_iter().map(|i| lines[i].clone()).collect();
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

#[cfg(any(feature = "arrange", feature = "structure"))]
pub use multipolygon::drop_nested_components;
pub(crate) use polygon::enforce_ogc_winding;
pub use polygon::is_valid_with_geo;
#[cfg(any(feature = "arrange", feature = "structure"))]
pub use polygon::make_valid_owned;
pub(crate) use polygon::snap_cannot_represent;
pub use strip::strip_degenerate_test;

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

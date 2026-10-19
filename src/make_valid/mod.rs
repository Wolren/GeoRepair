//! Points get NaN and Inf filtering plus deduplication.

use crate::util::{point_in_ring_exclusive_even_odd, shoelace_abs_sum};
use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle,
};

use crate::core::MakeValidConfig;
#[cfg(any(feature = "arrange", feature = "structure"))]
use crate::core::PolyMethod;
use crate::noding::NodingFloat;
use crate::validation::edges::{edges_intersect_general, edges_vertex_on_edge};
use crate::validation::impls::{
    check_line_components_intersect, check_linestring_self_intersection, segments_collinear_overlap,
};
use crate::validation::{GeoValidation, MAX_COLLECTION_DEPTH, ValidationResult};
use alloc::vec::Vec;
use log::warn;

/// Returns a valid geometry (possibly empty or decomposed into a
/// [`GeometryCollection`]) when the input violates OGC Simple Features rules. Use
/// [`make_valid`](MakeValid::make_valid) with default config, or
/// [`make_valid_with_config`](MakeValid::make_valid_with_config) for fine-grained
/// control over the repair strategy.
pub trait MakeValid {
    /// The scalar coordinate type (e.g. `f64`, `f32`).
    type Scalar: GeoFloat;

    fn make_valid(&self) -> Geometry<Self::Scalar> {
        self.make_valid_with_config(&MakeValidConfig::default())
    }

    /// Repair this geometry with the given configuration.
    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<Self::Scalar>;

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    fn par_make_valid(&self) -> Geometry<Self::Scalar>
    where
        Self: Send + Sync,
    {
        self.par_make_valid_with_config(&MakeValidConfig::default())
    }

    /// Falls back to [`make_valid_with_config`](MakeValid::make_valid_with_config) when
    /// the default implementation is used (single-threaded dispatch).
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


impl<T: GeoFloat> MakeValid for Point<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        // point(NaN NaN) is the canonical empty point - valid, unchanged.
        if (self.x().is_finite() && self.y().is_finite())
            || (self.x().is_nan() && self.y().is_nan())
        {
            Geometry::Point(*self)
        } else {
            Geometry::Point(Point::new(T::nan(), T::nan()))
        }
    }
}


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
                // empty point (NaN, NaN) is valid and preserved.
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


impl<T: NodingFloat> MakeValid for LineString<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let mut deduped: Vec<Coord<T>> = Vec::with_capacity(self.0.len());
        let mut prev: Option<Coord<T>> = None;
        for c in &self.0 {
            if !c.x.is_finite() || !c.y.is_finite() {
                continue;
            }
            if prev == Some(*c) {
                continue;
            }
            deduped.push(*c);
            prev = Some(*c);
        }
        if deduped.is_empty() {
            warn!("LineString::make_valid: all coords filtered (NaN/Inf)");
            return empty_geom();
        }
        if deduped.len() == 1 {
            return Geometry::Point(Point(deduped[0]));
        }
        if deduped.len() >= 3 {
            let fd: Vec<Coord<f64>> = deduped
                .iter()
                .map(|c| Coord {
                    x: c.x.to_f64().unwrap_or(f64::NAN),
                    y: c.y.to_f64().unwrap_or(f64::NAN),
                })
                .collect();
            if check_linestring_self_intersection(&fd) {
                let runs = match crate::noding::line::node_line(&fd) {
                    Some(runs) => runs,
                    None => {
                        let runs: Vec<Vec<Coord<f64>>> = simple_subline(&fd)
                            .into_iter()
                            .filter(|r| {
                                r.len() >= 2
                                    && !r.windows(2).any(|w| w[0] == w[1])
                                    && !check_linestring_self_intersection(r)
                            })
                            .collect();
                        match keep_conflicting_runs(&runs) {
                            Some(kept) => kept.into_iter().map(|i| runs[i].clone()).collect(),
                            None => runs,
                        }
                    }
                };
                let out: Vec<LineString<T>> = runs.iter().map(|r| run_to_line::<T>(r)).collect();
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

/// Map a f64 coordinate run (noder / greedy-fallback output) onto the target scalar.
fn run_to_line<T: NodingFloat>(run: &[Coord<f64>]) -> LineString<T> {
    LineString::new(
        run.iter()
            .map(|c| Coord {
                x: <T as num_traits::NumCast>::from(c.x).expect("f64 coord converts back to T"),
                y: <T as num_traits::NumCast>::from(c.y).expect("f64 coord converts back to T"),
            })
            .collect(),
    )
}

fn keep_conflicting_runs(runs: &[Vec<Coord<f64>>]) -> Option<Vec<usize>> {
    if runs.len() < 2 {
        return None;
    }
    let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for r in runs {
        for c in r {
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
    // before the per-edge checks (measured 2026-08-07: mls 50x3v went 1.7 -> 18.5 us
    // when the cross-component filter ran check_line_components_intersect on)
    let bboxes: Vec<(f64, f64, f64, f64)> = runs
        .iter()
        .map(|r| {
            let mut min_x = f64::MAX;
            let mut max_x = f64::MIN;
            let mut min_y = f64::MAX;
            let mut max_y = f64::MIN;
            for c in r {
                min_x = min_x.min(c.x);
                max_x = max_x.max(c.x);
                min_y = min_y.min(c.y);
                max_y = max_y.max(c.y);
            }
            (min_x, max_x, min_y, max_y)
        })
        .collect();
    let mut kept: Vec<usize> = Vec::new();
    // segments_collinear_overlap's `len2 > eps` guard caps L at sqrt(eps)
    let reach = eps.sqrt();
    for j in 0..runs.len() {
        let (bx0, bx1, by0, by1) = bboxes[j];
        let conflict = kept.iter().any(|&i| {
            let (ax0, ax1, ay0, ay1) = bboxes[i];
            if ax1 + reach < bx0 || bx1 + reach < ax0 || ay1 + reach < by0 || by1 + reach < ay0 {
                return false; // bboxes farther apart than the predicate's reach
            }
            check_line_components_intersect(&runs[i], &runs[j], eps)
        });
        if !conflict {
            kept.push(j);
        }
    }
    if kept.len() == runs.len() {
        None
    } else {
        Some(kept)
    }
}


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
            if let Some(kept) = keep_conflicting_runs(&fd) {
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


impl<T: GeoFloat> MakeValid for Rect<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let min_ok = self.min().x.is_finite() && self.min().y.is_finite();
        let max_ok = self.max().x.is_finite() && self.max().y.is_finite();
        if min_ok && max_ok {
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


#[cfg(any(feature = "arrange", feature = "structure"))]
mod multipolygon;
mod polygon;
pub(crate) mod precondition;
mod strip;

#[cfg(any(feature = "arrange", feature = "structure"))]
pub use multipolygon::drop_nested_components;
#[cfg(any(feature = "arrange", feature = "structure"))]
pub(crate) use multipolygon::mp_looks_valid;
pub(crate) use polygon::enforce_ogc_winding;
pub use polygon::is_valid_with_geo;
#[cfg(any(feature = "arrange", feature = "structure"))]
pub use polygon::make_valid_owned;
pub(crate) use polygon::snap_cannot_represent;
pub use strip::strip_degenerate_test;

/// Runs 35973600308 and 36113501091 did so on 2026-09-24 and 2026-09-25.
pub(crate) fn push_repaired_member<T: CoordNum>(
    out: &mut Vec<Geometry<T>>,
    member: Geometry<T>,
    member_depth: usize,
) {
    match member {
        Geometry::GeometryCollection(gc) => {
            if gc.0.is_empty() {
                return;
            }
            if member_depth > MAX_COLLECTION_DEPTH {
                for g in gc.0 {
                    push_repaired_member(out, g, member_depth + 1);
                }
            } else {
                let mut inner = Vec::new();
                for g in gc.0 {
                    push_repaired_member(&mut inner, g, member_depth + 1);
                }
                out.push(Geometry::GeometryCollection(GeometryCollection(inner)));
            }
        }
        other => out.push(other),
    }
}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for GeometryCollection<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let mut fixed: Vec<Geometry<f64>> = Vec::with_capacity(self.0.len());
        for g in &self.0 {
            push_repaired_member(&mut fixed, g.make_valid_with_config(config), 1);
        }
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
        let mut fixed: Vec<Geometry<T>> = Vec::with_capacity(self.0.len());
        for g in &self.0 {
            push_repaired_member(&mut fixed, g.make_valid_with_config(config), 1);
        }
        if fixed.is_empty() {
            empty_geom()
        } else {
            Geometry::GeometryCollection(GeometryCollection(fixed))
        }
    }
}


///
/// # Methods
/// - [`validate_and_fix`](ValidateAndFix::validate_and_fix): validate first, fix if
///   invalid
///
/// # Example
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
    fn validate_and_fix(&self) -> (ValidationResult, Geometry<f64>) {
        let result = <Self as GeoValidation>::validate(self);
        if result.valid {
            (result, <Self as MakeValid>::make_valid(self))
        } else {
            (result.clone(), <Self as MakeValid>::make_valid(self))
        }
    }

    fn validate_and_fix_always(&self) -> (ValidationResult, Geometry<f64>) {
        (
            <Self as GeoValidation>::validate(self),
            <Self as MakeValid>::make_valid(self),
        )
    }

    /// Returns Ok(geometry) if valid or fix succeeded, Err((validation_errors,
    /// fixed_geometry)) if still invalid.
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

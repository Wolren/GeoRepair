//! Output policy: reduce a repaired geometry to a shape the host can store.
//!
//! Repair can return a [`geo::GeometryCollection`], and a host with a closed geometry
//! (point, line, polygon and their multi forms) has nowhere to put one. A policy is an
//! explicit, opt-in post-pass, so the default repair output is unchanged.
//!
//! ```rust
//! # use geo::{LineString, Polygon};
//! # use geo_repair::MakeValid;
//! use geo_repair::output::{apply_policy, OutputPolicy};
//!
//! let bowtie = Polygon::new(
//!     LineString::from(vec![(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0)]),
//!     vec![],
//! );
//! let repaired = bowtie.make_valid();
//! let (out, report) = apply_policy(repaired, OutputPolicy::ArealOnly);
//! assert!(matches!(
//!     out,
//!     geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_)
//! ));
//! assert_eq!(report.dropped_parts, 0);
//! ```

use alloc::vec::Vec;
use geo::{
    GeoFloat, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};

#[cfg(any(feature = "arrange", feature = "structure"))]
use crate::core::MakeValidConfig;
#[cfg(any(feature = "arrange", feature = "structure"))]
use crate::make_valid::MakeValid;

/// How a repair result is reduced before it leaves the crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputPolicy {
    /// Return the engine output unchanged, collection included. This is the default
    /// and it is what [`crate::MakeValid`] returns.
    #[default]
    KeepAll,
    /// Keep only the parts of the highest dimension present: areal over linear over
    /// puntal. A mixed collection comes back as one homogeneous geometry.
    TopDimension,
    /// Keep only areal parts. A linear or puntal input yields an empty MultiPolygon.
    ArealOnly,
}

impl OutputPolicy {
    /// Stable machine key for configuration and logs.
    pub fn code(self) -> &'static str {
        match self {
            OutputPolicy::KeepAll => "keep_all",
            OutputPolicy::TopDimension => "top_dimension",
            OutputPolicy::ArealOnly => "areal_only",
        }
    }
}

/// What the policy kept and what it removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputReport {
    /// Parts of the selected dimension that reached the output.
    pub kept_parts: usize,
    /// Parts removed because a different dimension was selected.
    pub dropped_parts: usize,
}

impl OutputReport {
    /// True when no part was removed.
    pub fn is_lossless(&self) -> bool {
        self.dropped_parts == 0
    }
}

struct Parts<T: GeoFloat> {
    polygons: Vec<Polygon<T>>,
    lines: Vec<LineString<T>>,
    points: Vec<Point<T>>,
}

impl<T: GeoFloat> Default for Parts<T> {
    fn default() -> Self {
        Self {
            polygons: Vec::new(),
            lines: Vec::new(),
            points: Vec::new(),
        }
    }
}

impl<T: GeoFloat> Parts<T> {
    fn len(&self) -> usize {
        self.polygons.len() + self.lines.len() + self.points.len()
    }
}

/// Split a geometry into parts by dimension; collection nesting is flattened.
fn parts_of<T: GeoFloat>(geometry: Geometry<T>) -> Parts<T> {
    let mut parts = Parts::default();
    let mut stack: Vec<Geometry<T>> = Vec::new();
    stack.push(geometry);
    while let Some(g) = stack.pop() {
        match g {
            Geometry::Point(p) => parts.points.push(p),
            Geometry::MultiPoint(mp) => parts.points.extend(mp.0),
            Geometry::Line(l) => parts.lines.push(LineString::from(l)),
            Geometry::LineString(ls) => parts.lines.push(ls),
            Geometry::MultiLineString(mls) => parts.lines.extend(mls.0),
            Geometry::Polygon(p) => parts.polygons.push(p),
            Geometry::MultiPolygon(mp) => parts.polygons.extend(mp.0),
            Geometry::Rect(r) => parts.polygons.push(Polygon::from(r)),
            Geometry::Triangle(t) => parts.polygons.push(Polygon::from(t)),
            // Reverse so the pop order matches the source order.
            Geometry::GeometryCollection(gc) => stack.extend(gc.0.into_iter().rev()),
        }
    }
    parts
}

/// Count parts without moving them, for the `KeepAll` path.
fn count_parts<T: GeoFloat>(geometry: &Geometry<T>) -> usize {
    let mut count = 0usize;
    let mut stack: Vec<&Geometry<T>> = Vec::new();
    stack.push(geometry);
    while let Some(g) = stack.pop() {
        match g {
            Geometry::Point(_) | Geometry::Line(_) | Geometry::Polygon(_) => count += 1,
            Geometry::MultiPoint(mp) => count += mp.0.len(),
            Geometry::MultiLineString(mls) => count += mls.0.len(),
            Geometry::MultiPolygon(mp) => count += mp.0.len(),
            Geometry::LineString(_) | Geometry::Rect(_) | Geometry::Triangle(_) => count += 1,
            Geometry::GeometryCollection(gc) => stack.extend(gc.0.iter()),
        }
    }
    count
}

fn areal_geometry<T: GeoFloat>(mut polygons: Vec<Polygon<T>>) -> Geometry<T> {
    if polygons.len() == 1 {
        Geometry::Polygon(polygons.pop().expect("len==1 verified"))
    } else {
        Geometry::MultiPolygon(MultiPolygon::new(polygons))
    }
}

fn linear_geometry<T: GeoFloat>(mut lines: Vec<LineString<T>>) -> Geometry<T> {
    if lines.len() == 1 {
        Geometry::LineString(lines.pop().expect("len==1 verified"))
    } else {
        Geometry::MultiLineString(MultiLineString::new(lines))
    }
}

fn puntal_geometry<T: GeoFloat>(mut points: Vec<Point<T>>) -> Geometry<T> {
    if points.len() == 1 {
        Geometry::Point(points.pop().expect("len==1 verified"))
    } else {
        Geometry::MultiPoint(MultiPoint::new(points))
    }
}

/// Apply a policy to a geometry. Returns the reduced geometry and what the pass kept.
///
/// `KeepAll` returns the input untouched. The other policies flatten collection nesting
/// and emit a single geometry of the selected dimension. An empty result is an empty
/// MultiPolygon.
pub fn apply_policy<T: GeoFloat>(
    geometry: Geometry<T>,
    policy: OutputPolicy,
) -> (Geometry<T>, OutputReport) {
    if policy == OutputPolicy::KeepAll {
        let kept_parts = count_parts(&geometry);
        return (
            geometry,
            OutputReport {
                kept_parts,
                dropped_parts: 0,
            },
        );
    }

    let parts = parts_of(geometry);
    let total = parts.len();

    let kept = match policy {
        OutputPolicy::ArealOnly => parts.polygons.len(),
        OutputPolicy::TopDimension => {
            if !parts.polygons.is_empty() {
                parts.polygons.len()
            } else if !parts.lines.is_empty() {
                parts.lines.len()
            } else {
                parts.points.len()
            }
        }
        OutputPolicy::KeepAll => unreachable!("handled above"),
    };

    let out = if total == 0 || policy == OutputPolicy::ArealOnly || !parts.polygons.is_empty() {
        areal_geometry(parts.polygons)
    } else if !parts.lines.is_empty() {
        linear_geometry(parts.lines)
    } else {
        puntal_geometry(parts.points)
    };

    (
        out,
        OutputReport {
            kept_parts: kept,
            dropped_parts: total - kept,
        },
    )
}

/// Repair, then apply a policy in one call.
///
/// The policy is a post-pass on the repaired geometry: validity is the engine's, and
/// dropping lower-dimension parts cannot invalidate the areal result.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub fn make_valid_with_policy(
    geometry: &Geometry<f64>,
    config: &MakeValidConfig,
    policy: OutputPolicy,
) -> (Geometry<f64>, OutputReport) {
    apply_policy(geometry.make_valid_with_config(config), policy)
}

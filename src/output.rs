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

/// Drop what a closed host format cannot carry: non-finite coordinates, empty members, open rings.
///
/// Returns `None` when nothing usable survives. Every rule drops, none repairs: a ring is closed
/// only because an open ring cannot be written, and no coordinate is moved.
pub fn sanitize<T: GeoFloat>(geometry: &Geometry<T>) -> Option<Geometry<T>> {
    match geometry {
        Geometry::Point(p) => finite_point(p).map(Geometry::Point),
        Geometry::MultiPoint(mp) => {
            let pts: Vec<Point<T>> = mp.0.iter().filter_map(finite_point).collect();
            if pts.is_empty() {
                None
            } else {
                Some(Geometry::MultiPoint(MultiPoint::new(pts)))
            }
        }
        Geometry::Line(l) => {
            if finite_pair(l.start, l.end) {
                Some(Geometry::Line(*l))
            } else {
                None
            }
        }
        Geometry::LineString(ls) => sanitize_line(ls).map(Geometry::LineString),
        Geometry::MultiLineString(mls) => {
            let lines: Vec<LineString<T>> = mls.0.iter().filter_map(sanitize_line).collect();
            if lines.is_empty() {
                None
            } else {
                Some(Geometry::MultiLineString(MultiLineString::new(lines)))
            }
        }
        Geometry::Polygon(p) => sanitize_polygon(p).map(Geometry::Polygon),
        Geometry::MultiPolygon(mp) => {
            let polys: Vec<Polygon<T>> = mp.0.iter().filter_map(sanitize_polygon).collect();
            if polys.is_empty() {
                None
            } else {
                Some(Geometry::MultiPolygon(MultiPolygon::new(polys)))
            }
        }
        Geometry::Rect(r) => sanitize_polygon(&r.to_polygon()).map(Geometry::Polygon),
        Geometry::Triangle(t) => {
            let rings = sanitize_ring(&t.to_polygon().exterior().0);
            rings.map(|r| Geometry::Polygon(Polygon::new(LineString::new(r), vec![])))
        }
        Geometry::GeometryCollection(gc) => {
            let parts: Vec<Geometry<T>> = gc.0.iter().filter_map(sanitize).collect();
            // The type is preserved: unwrapping a single survivor would make the output shape
            // depend on the data. Flattening is `apply_policy`'s job, not this pass.
            if parts.is_empty() {
                None
            } else {
                Some(Geometry::GeometryCollection(
                    geo::GeometryCollection::new_from(parts),
                ))
            }
        }
    }
}

fn finite_coord<T: GeoFloat>(c: geo::Coord<T>) -> bool {
    c.x.is_finite() && c.y.is_finite()
}

fn finite_point<T: GeoFloat>(p: &Point<T>) -> Option<Point<T>> {
    if finite_coord(p.0) { Some(*p) } else { None }
}

fn finite_pair<T: GeoFloat>(a: geo::Coord<T>, b: geo::Coord<T>) -> bool {
    finite_coord(a) && finite_coord(b)
}

fn sanitize_line<T: GeoFloat>(ls: &LineString<T>) -> Option<LineString<T>> {
    if ls.0.len() < 2 || !ls.0.iter().all(|c| finite_coord(*c)) {
        return None;
    }
    Some(ls.clone())
}

fn sanitize_ring<T: GeoFloat>(ring: &[geo::Coord<T>]) -> Option<Vec<geo::Coord<T>>> {
    if ring.len() < 3 || !ring.iter().all(|c| finite_coord(*c)) {
        return None;
    }
    let mut out = ring.to_vec();
    if out[0] != out[out.len() - 1] {
        out.push(out[0]);
    }
    if out.len() < 4 {
        return None;
    }
    Some(out)
}

fn sanitize_polygon<T: GeoFloat>(p: &Polygon<T>) -> Option<Polygon<T>> {
    let exterior = sanitize_ring(&p.exterior().0)?;
    let mut holes = Vec::with_capacity(p.interiors().len());
    for h in p.interiors() {
        if let Some(r) = sanitize_ring(&h.0) {
            holes.push(LineString::new(r));
        }
    }
    Some(Polygon::new(LineString::new(exterior), holes))
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize;
    use geo::{Geometry, LineString, MultiPoint, Point, Polygon};

    fn ring(coords: &[(f64, f64)]) -> LineString<f64> {
        LineString::from(coords.to_vec())
    }

    #[test]
    fn a_non_finite_point_is_dropped() {
        let g = Geometry::Point(Point::new(f64::NAN, 2.0));
        assert!(sanitize(&g).is_none());
        let g = Geometry::Point(Point::new(f64::INFINITY, 2.0));
        assert!(sanitize(&g).is_none());
    }

    #[test]
    fn a_multipoint_keeps_only_finite_members() {
        let g = Geometry::MultiPoint(MultiPoint::new(vec![
            Point::new(f64::NAN, f64::NAN),
            Point::new(1.0, 2.0),
        ]));
        match sanitize(&g).expect("one member survives") {
            Geometry::MultiPoint(mp) => assert_eq!(mp.0.len(), 1),
            other => panic!("type changed to {other:?}"),
        }
    }

    #[test]
    fn an_open_ring_is_closed_and_not_moved() {
        let poly = Polygon::new(ring(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]), vec![]);
        match sanitize(&Geometry::Polygon(poly)).expect("a triangle ring survives") {
            Geometry::Polygon(p) => {
                let c = &p.exterior().0;
                assert_eq!(c.len(), 4);
                assert_eq!(c[0], c[3]);
            }
            other => panic!("type changed to {other:?}"),
        }
    }

    #[test]
    fn a_ring_with_two_coordinates_is_dropped() {
        let poly = Polygon::new(ring(&[(0.0, 0.0), (1.0, 1.0)]), vec![]);
        assert!(sanitize(&Geometry::Polygon(poly)).is_none());
    }

    #[test]
    fn an_empty_hole_is_dropped() {
        let poly = Polygon::new(
            ring(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)]),
            vec![ring(&[])],
        );
        match sanitize(&Geometry::Polygon(poly)).expect("the shell survives") {
            Geometry::Polygon(p) => assert!(p.interiors().is_empty()),
            other => panic!("type changed to {other:?}"),
        }
    }

    #[test]
    fn a_non_finite_ring_drops_the_polygon() {
        let poly = Polygon::new(
            ring(&[(0.0, 0.0), (f64::NAN, 0.0), (10.0, 10.0), (0.0, 0.0)]),
            vec![],
        );
        assert!(sanitize(&Geometry::Polygon(poly)).is_none());
    }

    #[test]
    fn a_collection_keeps_its_type_and_drops_dead_members() {
        use geo::GeometryCollection;
        let gc = GeometryCollection::new_from(vec![
            Geometry::Point(Point::new(f64::NAN, 0.0)),
            Geometry::Point(Point::new(1.0, 1.0)),
        ]);
        match sanitize(&Geometry::GeometryCollection(gc)).expect("one member survives") {
            Geometry::GeometryCollection(gc) => assert_eq!(gc.0.len(), 1),
            other => panic!("type changed to {other:?}"),
        }
    }

    #[test]
    fn everything_dead_gives_none() {
        use geo::GeometryCollection;
        let gc = GeometryCollection::new_from(vec![Geometry::Point(Point::new(f64::NAN, 0.0))]);
        assert!(sanitize(&Geometry::GeometryCollection(gc)).is_none());
    }
}

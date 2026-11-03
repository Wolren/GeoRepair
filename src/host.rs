//! Adapter for a host whose geometry model is closed: point, line, polygon and their multi forms.
//!
//! `to_host` flattens collections, drops what a host cannot store, and chooses the type from what
//! survives. A host wants the data, not the shape of the input, so a collection whose only live
//! member is a point becomes a point and not a single-member multipoint.

use alloc::vec::Vec;
use geo::{
    GeoFloat, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};

/// What the adapter dropped on the way to the host model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostReport {
    /// Collection levels flattened away.
    pub flattened_collections: usize,
    /// Parts dropped because a coordinate was NaN or infinite.
    pub dropped_nonfinite: usize,
    /// Parts dropped because they cannot form a shape once closed.
    pub dropped_degenerate: usize,
}

/// Reduce a geometry to the closed host model: one of six types, finite, no empties, closed rings.
///
/// Clones once, then works in place. Returns `None` when nothing reachable belongs to the model.
pub fn to_host<T: GeoFloat>(geometry: &Geometry<T>) -> (Option<Geometry<T>>, HostReport) {
    to_host_owned(geometry.clone())
}

/// As [`to_host`], taking ownership so a clean geometry costs one move and no copy.
pub fn to_host_owned<T: GeoFloat>(geometry: Geometry<T>) -> (Option<Geometry<T>>, HostReport) {
    let mut report = HostReport::default();
    let mut parts = Parts {
        polys: Vec::new(),
        lines: Vec::new(),
        points: Vec::new(),
    };
    gather(geometry, &mut parts, &mut report);

    let mut polys: Vec<Polygon<T>> = Vec::with_capacity(parts.polys.len());
    for p in parts.polys {
        if let Some(c) = clean_polygon(p, &mut report) {
            polys.push(c);
        }
    }
    let mut lines: Vec<LineString<T>> = Vec::with_capacity(parts.lines.len());
    for l in parts.lines {
        if let Some(c) = clean_line(l, &mut report) {
            lines.push(c);
        }
    }
    let mut points: Vec<Point<T>> = Vec::with_capacity(parts.points.len());
    for p in parts.points {
        if finite(p.0.x) && finite(p.0.y) {
            points.push(p);
        } else {
            report.dropped_nonfinite += 1;
        }
    }

    let out = if !polys.is_empty() {
        if polys.len() == 1 {
            Geometry::Polygon(polys.pop().expect("length checked"))
        } else {
            Geometry::MultiPolygon(MultiPolygon::new(polys))
        }
    } else if !lines.is_empty() {
        if lines.len() == 1 {
            Geometry::LineString(lines.pop().expect("length checked"))
        } else {
            Geometry::MultiLineString(MultiLineString::new(lines))
        }
    } else if !points.is_empty() {
        if points.len() == 1 {
            Geometry::Point(points.pop().expect("length checked"))
        } else {
            Geometry::MultiPoint(MultiPoint::new(points))
        }
    } else {
        return (None, report);
    };

    debug_assert_eq!(host_clean(&out), Ok(()));
    (Some(out), report)
}

/// The invariants a host may rely on, checkable by the host itself.
pub fn host_clean<T: GeoFloat>(g: &Geometry<T>) -> Result<(), &'static str> {
    match g {
        Geometry::Point(p) => {
            if finite(p.0.x) && finite(p.0.y) {
                Ok(())
            } else {
                Err("point is not finite")
            }
        }
        Geometry::MultiPoint(mp) => {
            if mp.0.is_empty() {
                return Err("empty multipoint");
            }
            if mp.0.iter().any(|p| !finite(p.0.x) || !finite(p.0.y)) {
                return Err("multipoint member is not finite");
            }
            Ok(())
        }
        Geometry::LineString(ls) => check_line(ls),
        Geometry::MultiLineString(mls) => {
            if mls.0.is_empty() {
                return Err("empty multilinestring");
            }
            for l in &mls.0 {
                check_line(l)?;
            }
            Ok(())
        }
        Geometry::Polygon(p) => check_polygon(p),
        Geometry::MultiPolygon(mp) => {
            if mp.0.is_empty() {
                return Err("empty multipolygon");
            }
            for p in &mp.0 {
                check_polygon(p)?;
            }
            Ok(())
        }
        Geometry::GeometryCollection(_) => Err("a collection reached the host"),
        Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::Line(_) => {
            Err("shape outside the host model")
        }
    }
}

fn finite<T: GeoFloat>(v: T) -> bool {
    v.is_finite()
}

struct Parts<T: GeoFloat> {
    polys: Vec<Polygon<T>>,
    lines: Vec<LineString<T>>,
    points: Vec<Point<T>>,
}

fn gather<T: GeoFloat>(g: Geometry<T>, parts: &mut Parts<T>, report: &mut HostReport) {
    match g {
        Geometry::Point(p) => parts.points.push(p),
        Geometry::MultiPoint(mp) => parts.points.extend(mp.0),
        Geometry::Line(l) => parts
            .lines
            .push(LineString::new(alloc::vec![l.start, l.end])),
        Geometry::LineString(ls) => parts.lines.push(ls),
        Geometry::MultiLineString(mls) => parts.lines.extend(mls.0),
        Geometry::Polygon(p) => parts.polys.push(p),
        Geometry::MultiPolygon(mp) => parts.polys.extend(mp.0),
        Geometry::Rect(r) => parts.polys.push(r.to_polygon()),
        Geometry::Triangle(t) => parts.polys.push(t.to_polygon()),
        Geometry::GeometryCollection(gc) => {
            report.flattened_collections += 1;
            for part in gc.0 {
                gather(part, parts, report);
            }
        }
    }
}

fn clean_polygon<T: GeoFloat>(p: Polygon<T>, report: &mut HostReport) -> Option<Polygon<T>> {
    let (shell, holes) = p.into_inner();
    let shell = clean_ring(shell, report)?;
    let mut kept = Vec::with_capacity(holes.len());
    for h in holes {
        if let Some(c) = clean_ring(h, report) {
            kept.push(c);
        }
    }
    Some(Polygon::new(shell, kept))
}

/// A line needs two coordinates and is never closed.
fn clean_line<T: GeoFloat>(line: LineString<T>, report: &mut HostReport) -> Option<LineString<T>> {
    let coords = line.0;
    if coords.iter().any(|c| !finite(c.x) || !finite(c.y)) {
        report.dropped_nonfinite += 1;
        return None;
    }
    if coords.len() < 2 {
        report.dropped_degenerate += 1;
        return None;
    }
    Some(LineString::new(coords))
}

/// A ring needs four coordinates once closed; anything less encloses nothing.
fn clean_ring<T: GeoFloat>(line: LineString<T>, report: &mut HostReport) -> Option<LineString<T>> {
    let mut coords = line.0;
    if coords.iter().any(|c| !finite(c.x) || !finite(c.y)) {
        report.dropped_nonfinite += 1;
        return None;
    }
    if coords.is_empty() {
        report.dropped_degenerate += 1;
        return None;
    }
    if coords[0] != coords[coords.len() - 1] {
        coords.push(coords[0]);
    }
    if coords.len() < 4 {
        report.dropped_degenerate += 1;
        return None;
    }
    Some(LineString::new(coords))
}

fn check_line<T: GeoFloat>(l: &LineString<T>) -> Result<(), &'static str> {
    if l.0.len() < 2 {
        return Err("line has fewer than two coordinates");
    }
    if l.0.iter().any(|c| !finite(c.x) || !finite(c.y)) {
        return Err("line coordinate is not finite");
    }
    Ok(())
}

fn check_polygon<T: GeoFloat>(p: &Polygon<T>) -> Result<(), &'static str> {
    for ring in core::iter::once(p.exterior()).chain(p.interiors().iter()) {
        if ring.0.len() < 4 {
            return Err("ring has fewer than four coordinates");
        }
        if ring.0[0] != ring.0[ring.0.len() - 1] {
            return Err("ring is not closed");
        }
        if ring.0.iter().any(|c| !finite(c.x) || !finite(c.y)) {
            return Err("ring coordinate is not finite");
        }
    }
    Ok(())
}

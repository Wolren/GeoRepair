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
/// Returns `None` when nothing reachable belongs to the host model.
pub fn to_host<T: GeoFloat>(geometry: &Geometry<T>) -> (Option<Geometry<T>>, HostReport) {
    let mut report = HostReport::default();
    let (mut polys, mut lines, mut points) = (Vec::new(), Vec::new(), Vec::new());
    gather(geometry, &mut polys, &mut lines, &mut points, &mut report);

    let mut kept: Vec<Polygon<T>> = Vec::with_capacity(polys.len());
    for p in &polys {
        if let Some(c) = clean_polygon(p, &mut report) {
            kept.push(c);
        }
    }
    polys = kept;
    lines.retain(|l| keep_line(l, &mut report));
    points.retain(|p| keep_point(p, &mut report));

    let out = if !polys.is_empty() {
        if polys.len() == 1 {
            Geometry::Polygon(polys.remove(0))
        } else {
            Geometry::MultiPolygon(MultiPolygon::new(polys))
        }
    } else if !lines.is_empty() {
        if lines.len() == 1 {
            Geometry::LineString(lines.remove(0))
        } else {
            Geometry::MultiLineString(MultiLineString::new(lines))
        }
    } else if !points.is_empty() {
        if points.len() == 1 {
            Geometry::Point(points.remove(0))
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

fn gather<T: GeoFloat>(
    g: &Geometry<T>,
    polys: &mut Vec<Polygon<T>>,
    lines: &mut Vec<LineString<T>>,
    points: &mut Vec<Point<T>>,
    report: &mut HostReport,
) {
    match g {
        Geometry::Point(p) => points.push(*p),
        Geometry::MultiPoint(mp) => points.extend(mp.0.iter().copied()),
        Geometry::Line(l) => lines.push(LineString::from(alloc::vec![l.start, l.end])),
        Geometry::LineString(ls) => lines.push(ls.clone()),
        Geometry::MultiLineString(mls) => lines.extend(mls.0.iter().cloned()),
        Geometry::Polygon(p) => polys.push(p.clone()),
        Geometry::MultiPolygon(mp) => polys.extend(mp.0.iter().cloned()),
        Geometry::Rect(r) => polys.push(r.to_polygon()),
        Geometry::Triangle(t) => polys.push(t.to_polygon()),
        Geometry::GeometryCollection(gc) => {
            report.flattened_collections += 1;
            for part in &gc.0 {
                gather(part, polys, lines, points, report);
            }
        }
    }
}

fn clean_polygon<T: GeoFloat>(p: &Polygon<T>, report: &mut HostReport) -> Option<Polygon<T>> {
    let shell = clean_ring(&p.exterior().0, report)?;
    let mut holes = Vec::with_capacity(p.interiors().len());
    for h in p.interiors() {
        if let Some(r) = clean_ring(&h.0, report) {
            holes.push(LineString::new(r));
        }
    }
    Some(Polygon::new(LineString::new(shell), holes))
}

fn clean_ring<T: GeoFloat>(
    ring: &[geo::Coord<T>],
    report: &mut HostReport,
) -> Option<Vec<geo::Coord<T>>> {
    if ring.iter().any(|c| !finite(c.x) || !finite(c.y)) {
        report.dropped_nonfinite += 1;
        return None;
    }
    let mut out = ring.to_vec();
    if out.is_empty() {
        report.dropped_degenerate += 1;
        return None;
    }
    if out[0] != out[out.len() - 1] {
        out.push(out[0]);
    }
    // A closed ring needs four coordinates; three means it encloses nothing.
    if out.len() < 4 {
        report.dropped_degenerate += 1;
        return None;
    }
    Some(out)
}

fn keep_line<T: GeoFloat>(l: &LineString<T>, report: &mut HostReport) -> bool {
    if l.0.iter().any(|c| !finite(c.x) || !finite(c.y)) {
        report.dropped_nonfinite += 1;
        return false;
    }
    if l.0.len() < 2 {
        report.dropped_degenerate += 1;
        return false;
    }
    true
}

fn keep_point<T: GeoFloat>(p: &Point<T>, report: &mut HostReport) -> bool {
    if finite(p.0.x) && finite(p.0.y) {
        true
    } else {
        report.dropped_nonfinite += 1;
        false
    }
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

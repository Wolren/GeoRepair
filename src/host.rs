//! Adapter for a host whose geometry model is closed: point, line, polygon and their multi forms.
//!
//! `to_host` flattens collections, drops what a host cannot store, and chooses the type from what
//! survives. A host wants the data, not the shape of the input, so a collection whose only live
//! member is a point becomes a point and not a single-member multipoint.
//!
//! `to_host_with` adds repair. A producer such as RustGIS hands over bytes it could not validate;
//! this crate repairs them and returns a shape the host can store, counting what it changed.

use alloc::vec::Vec;
use geo::{
    GeoFloat, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};

use crate::make_valid::MakeValid;
use crate::validation::GeoValidation;

/// What the adapter dropped or rebuilt on the way to the host model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostReport {
    /// Collection levels flattened away.
    pub flattened_collections: usize,
    /// Parts dropped because a coordinate was NaN or infinite.
    pub dropped_nonfinite: usize,
    /// Parts dropped because they cannot form a shape once closed.
    pub dropped_degenerate: usize,
    /// Parts rebuilt by `make_valid` because they failed validity.
    pub repaired: usize,
    /// Parts dropped because repair produced nothing storable.
    pub dropped_unrepairable: usize,
}

/// How far the delivery pass goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostOptions {
    /// Rebuild invalid polygons with `make_valid` before delivery.
    pub repair: bool,
}

impl HostOptions {
    /// Flatten and filter only. No geometry changes shape.
    pub const REDUCE: Self = Self { repair: false };
    /// Flatten, filter, then repair every polygon that fails validity.
    pub const REPAIR: Self = Self { repair: true };
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
    let parts = reduce(geometry, &mut report);
    (assemble(parts), report)
}

/// Deliver to a closed host, optionally repairing what fails validity.
pub fn to_host_with(
    geometry: &Geometry<f64>,
    options: HostOptions,
) -> (Option<Geometry<f64>>, HostReport) {
    to_host_with_owned(geometry.clone(), options)
}

/// As [`to_host_with`], taking ownership of the input.
pub fn to_host_with_owned(
    geometry: Geometry<f64>,
    options: HostOptions,
) -> (Option<Geometry<f64>>, HostReport) {
    let mut report = HostReport::default();
    let mut parts = reduce(geometry, &mut report);
    if options.repair {
        parts.polys = repair_polygons(parts.polys, &mut report);
    }
    (assemble(parts), report)
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
/// Read WKB or EWKB bytes, then deliver to a closed host, repairing when asked.
///
/// The path a producer takes when it hands over bytes: the reader keeps SRID and Z/M beside the
/// geometry, and delivery drops them because the host model is 2D.
pub fn to_host_wkb(
    bytes: &[u8],
    options: HostOptions,
) -> Result<(Option<Geometry<f64>>, HostReport), crate::io::wkb::WkbError> {
    let read = crate::io::wkb::read_ewkb(bytes)?;
    Ok(to_host_with_owned(read.geometry, options))
}

/// Read PostGIS EWKT, then deliver to a closed host, repairing when asked.
pub fn to_host_ewkt(
    text: &str,
    options: HostOptions,
) -> Result<(Option<Geometry<f64>>, HostReport), crate::io::wkt::WktError> {
    let (_, read) = crate::io::wkt::read_ewkt(text)?;
    Ok(to_host_with_owned(read.geometry, options))
}

struct Parts<T: GeoFloat> {
    polys: Vec<Polygon<T>>,
    lines: Vec<LineString<T>>,
    points: Vec<Point<T>>,
}

fn reduce<T: GeoFloat>(geometry: Geometry<T>, report: &mut HostReport) -> Parts<T> {
    let mut raw = Parts {
        polys: Vec::new(),
        lines: Vec::new(),
        points: Vec::new(),
    };
    gather(geometry, &mut raw, report);

    let mut kept = Vec::with_capacity(raw.polys.len());
    for p in raw.polys {
        if let Some(c) = clean_polygon(p, report) {
            kept.push(c);
        }
    }
    let mut lines = Vec::with_capacity(raw.lines.len());
    for l in raw.lines {
        if let Some(c) = clean_line(l, report) {
            lines.push(c);
        }
    }
    let mut points = Vec::with_capacity(raw.points.len());
    for p in raw.points {
        if finite(p.0.x) && finite(p.0.y) {
            points.push(p);
        } else {
            report.dropped_nonfinite += 1;
        }
    }
    Parts {
        polys: kept,
        lines,
        points,
    }
}

fn assemble<T: GeoFloat>(parts: Parts<T>) -> Option<Geometry<T>> {
    let Parts {
        mut polys,
        mut lines,
        mut points,
    } = parts;
    if !polys.is_empty() {
        if polys.len() == 1 {
            return Some(Geometry::Polygon(polys.pop().expect("length checked")));
        }
        return Some(Geometry::MultiPolygon(MultiPolygon::new(polys)));
    }
    if !lines.is_empty() {
        if lines.len() == 1 {
            return Some(Geometry::LineString(lines.pop().expect("length checked")));
        }
        return Some(Geometry::MultiLineString(MultiLineString::new(lines)));
    }
    if !points.is_empty() {
        if points.len() == 1 {
            return Some(Geometry::Point(points.pop().expect("length checked")));
        }
        return Some(Geometry::MultiPoint(MultiPoint::new(points)));
    }
    None
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

fn repair_polygons(polys: Vec<Polygon<f64>>, report: &mut HostReport) -> Vec<Polygon<f64>> {
    let mut out: Vec<Polygon<f64>> = Vec::with_capacity(polys.len());
    for p in polys {
        if p.is_valid() {
            out.push(p);
            continue;
        }
        report.repaired += 1;
        match p.make_valid() {
            Geometry::Polygon(q) => keep_repaired(q, &mut out, report),
            Geometry::MultiPolygon(mp) => {
                for q in mp.0 {
                    keep_repaired(q, &mut out, report);
                }
            }
            Geometry::GeometryCollection(gc) => {
                for g in gc.0 {
                    match g {
                        Geometry::Polygon(q) => keep_repaired(q, &mut out, report),
                        Geometry::MultiPolygon(mp) => {
                            for q in mp.0 {
                                keep_repaired(q, &mut out, report);
                            }
                        }
                        _ => report.dropped_unrepairable += 1,
                    }
                }
            }
            _ => report.dropped_unrepairable += 1,
        }
    }
    out
}

/// Repair can emit a part the host still cannot store, so the finite and ring rules run again.
fn keep_repaired(p: Polygon<f64>, out: &mut Vec<Polygon<f64>>, report: &mut HostReport) {
    match clean_polygon(p, report) {
        Some(c) => out.push(c),
        None => report.dropped_unrepairable += 1,
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

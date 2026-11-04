//! Adapter for a host whose geometry model is closed: point, line, polygon and their multi forms.
//!
//! `to_host` flattens collections, drops what a host cannot store, and chooses the type from what
//! survives. A host wants the data, not the shape of the input, so a collection whose only live
//! member is a point becomes a point and not a single-member multipoint.
//!
//! `to_host_with` adds repair. A producer such as RustGIS hands over geometry it could not
//! validate; this crate repairs it and returns a shape the host can store, counting what changed.
//! Repair runs on the whole multi geometry, not part by part: two valid polygons can still overlap,
//! and an invalid MultiPolygon must become a set the host can accept.

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
    /// Collections rebuilt by `make_valid` because they failed validity.
    pub repaired: usize,
    /// Parts dropped because repair produced nothing storable.
    pub dropped_unrepairable: usize,
}

/// How far the delivery pass goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostOptions {
    /// Rebuild invalid point, line and polygon collections with `make_valid`.
    pub repair: bool,
}

impl HostOptions {
    /// Flatten and filter only. No geometry changes shape.
    pub const REDUCE: Self = Self { repair: false };
    /// Flatten, filter, then repair every collection that fails validity.
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
        // Repair may improve a geometry, never delete one: a family that comes back empty
        // falls back to what the reduce pass kept.
        let points_before = parts.points;
        let restored = repair_points(points_before.clone(), &mut report);
        parts.points = if restored.is_empty() && !points_before.is_empty() {
            report.repaired = report.repaired.saturating_sub(1);
            points_before
        } else {
            restored
        };
        let lines_before = parts.lines;
        let restored = repair_lines(lines_before.clone(), &mut report);
        parts.lines = if restored.is_empty()
            && lines_before
                .iter()
                .any(|l| l.0.windows(2).any(|w| w[0] != w[1]))
        {
            report.repaired = report.repaired.saturating_sub(1);
            lines_before
        } else {
            restored
        };
        let polys_before = parts.polys;
        let restored = repair_polygons(polys_before.clone(), &mut report);
        parts.polys = if restored.is_empty()
            && polys_before
                .iter()
                .any(|p| geo::Area::unsigned_area(p) > 0.0f64)
        {
            report.repaired = report.repaired.saturating_sub(1);
            polys_before
        } else {
            restored
        };
    }
    (assemble(parts), report)
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

fn reduce<T: GeoFloat>(geometry: Geometry<T>, report: &mut HostReport) -> Parts<T> {
    let mut raw = Parts {
        polys: Vec::new(),
        lines: Vec::new(),
        points: Vec::new(),
    };
    gather(geometry, &mut raw, report);

    let mut polys = Vec::with_capacity(raw.polys.len());
    for p in raw.polys {
        if let Some(c) = clean_polygon(p, report) {
            polys.push(c);
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
        polys,
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

/// Duplicate coordinates are the only way finite points fail validity, and make_valid dedupes.
fn repair_points(points: Vec<Point<f64>>, report: &mut HostReport) -> Vec<Point<f64>> {
    if points.len() < 2 {
        return points;
    }
    let geo = Geometry::MultiPoint(MultiPoint::new(points));
    if geo.is_valid() {
        return match geo {
            Geometry::MultiPoint(mp) => mp.0,
            _ => Vec::new(),
        };
    }
    report.repaired += 1;
    let mut out = Vec::new();
    collect_points(geo.make_valid(), &mut out, report);
    out
}

/// Members that cross each other make the delivered MultiLineString invalid, so repair the set.
fn repair_lines(lines: Vec<LineString<f64>>, report: &mut HostReport) -> Vec<LineString<f64>> {
    if lines.is_empty() {
        return lines;
    }
    let geo = if lines.len() == 1 {
        Geometry::LineString(lines.into_iter().next().expect("length checked"))
    } else {
        Geometry::MultiLineString(MultiLineString::new(lines))
    };
    if geo.is_valid() {
        return match geo {
            Geometry::LineString(l) => alloc::vec![l],
            Geometry::MultiLineString(mls) => mls.0,
            _ => Vec::new(),
        };
    }
    let before = match &geo {
        Geometry::LineString(_) => 1,
        Geometry::MultiLineString(mls) => mls.0.len(),
        _ => 0,
    };
    report.repaired += 1;
    let mut out = Vec::new();
    collect_lines(geo.make_valid(), &mut out, report);
    // A split adds parts; only a lost part is a drop.
    if out.len() < before {
        report.dropped_unrepairable += before - out.len();
    }
    out
}

/// Two valid polygons can overlap, so validity belongs to the collection, not to each part.
fn repair_polygons(polys: Vec<Polygon<f64>>, report: &mut HostReport) -> Vec<Polygon<f64>> {
    if polys.is_empty() {
        return polys;
    }
    let geo = if polys.len() == 1 {
        Geometry::Polygon(polys.into_iter().next().expect("length checked"))
    } else {
        Geometry::MultiPolygon(MultiPolygon::new(polys))
    };
    if geo.is_valid() {
        return match geo {
            Geometry::Polygon(p) => alloc::vec![p],
            Geometry::MultiPolygon(mp) => mp.0,
            _ => Vec::new(),
        };
    }
    let before = match &geo {
        Geometry::Polygon(_) => 1,
        Geometry::MultiPolygon(mp) => mp.0.len(),
        _ => 0,
    };
    report.repaired += 1;
    let mut out = Vec::new();
    collect_polys(geo.make_valid(), &mut out, report);
    // A part that make_valid consumed without producing a replacement is a drop.
    if out.len() < before {
        report.dropped_unrepairable += before - out.len();
    }
    out
}

fn collect_points(g: Geometry<f64>, out: &mut Vec<Point<f64>>, report: &mut HostReport) {
    match g {
        Geometry::Point(p) => {
            if finite(p.0.x) && finite(p.0.y) {
                out.push(p);
            } else {
                report.dropped_unrepairable += 1;
            }
        }
        Geometry::MultiPoint(mp) => {
            for p in mp.0 {
                collect_points(Geometry::Point(p), out, report);
            }
        }
        Geometry::GeometryCollection(gc) => {
            for part in gc.0 {
                collect_points(part, out, report);
            }
        }
        _ => report.dropped_unrepairable += 1,
    }
}

fn collect_lines(g: Geometry<f64>, out: &mut Vec<LineString<f64>>, report: &mut HostReport) {
    match g {
        Geometry::LineString(l) => match clean_line(l, report) {
            Some(c) => out.push(c),
            None => report.dropped_unrepairable += 1,
        },
        Geometry::MultiLineString(mls) => {
            for l in mls.0 {
                collect_lines(Geometry::LineString(l), out, report);
            }
        }
        Geometry::GeometryCollection(gc) => {
            for part in gc.0 {
                collect_lines(part, out, report);
            }
        }
        _ => report.dropped_unrepairable += 1,
    }
}

fn collect_polys(g: Geometry<f64>, out: &mut Vec<Polygon<f64>>, report: &mut HostReport) {
    match g {
        Geometry::Polygon(p) => match clean_polygon(p, report) {
            Some(c) => out.push(c),
            None => report.dropped_unrepairable += 1,
        },
        Geometry::MultiPolygon(mp) => {
            for p in mp.0 {
                collect_polys(Geometry::Polygon(p), out, report);
            }
        }
        Geometry::GeometryCollection(gc) => {
            for part in gc.0 {
                collect_polys(part, out, report);
            }
        }
        // A polygon that collapsed to lines or points carries no area; the host layer loses meaning.
        _ => report.dropped_unrepairable += 1,
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

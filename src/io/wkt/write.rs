//! WKT writer: geometry serializer.

use super::*;
use alloc::string::String;
#[cfg(feature = "std")]
use std::io::{self, Write};

/// Serialize a `Geometry<f64>` to WKT and write it to any `io::Write` target.
#[cfg(feature = "std")]
pub fn write_wkt_to(geom: &Geometry<f64>, writer: &mut impl Write) -> io::Result<()> {
    let s = write_wkt(geom);
    writer.write_all(s.as_bytes())
}

/// Serialize a `WktGeometry` and write it to any `io::Write` target.
#[cfg(feature = "std")]
pub fn write_wkt_zm_to(wkt: &WktGeometry, writer: &mut impl Write) -> io::Result<()> {
    let s = write_wkt_zm(wkt);
    writer.write_all(s.as_bytes())
}

/// Serialize a `Geometry<f64>` to 2D WKT string.
pub fn write_wkt(geom: &Geometry<f64>) -> String {
    let mut s = String::new();
    let mut offset = 0;
    write_geom(&mut s, geom, Dims::XY, &[], &mut offset);
    s
}

/// Serialize a geometry with its dimension modifier and extra ordinates.
///
/// `extra_coords` must hold `dims.coord_count() - 2` values per coordinate, in the order
/// [`read_wkt_zm`](super::read_wkt_zm) returns them.
pub fn write_wkt_zm(wkt: &WktGeometry) -> String {
    let mut s = String::new();
    let mut offset = 0;
    write_geom(
        &mut s,
        &wkt.geometry,
        wkt.dims,
        &wkt.extra_coords,
        &mut offset,
    );
    debug_assert_eq!(
        offset,
        wkt.extra_coords.len(),
        "WKT writer left extra coords unused"
    );
    s
}

fn write_f64(s: &mut String, v: f64) {
    let mut buf = ryu::Buffer::new();
    s.push_str(buf.format(v));
}

/// A short `extra_coords` yields NaN instead of panicking.
fn take_extra(extra: &[f64], offset: &mut usize) -> f64 {
    debug_assert!(
        *offset < extra.len(),
        "extra_coords is short of the dimension"
    );
    let value = extra.get(*offset).copied().unwrap_or(f64::NAN);
    *offset += 1;
    value
}

fn write_coord(s: &mut String, c: &Coord<f64>, dims: Dims, extra: &[f64], offset: &mut usize) {
    write_f64(s, c.x);
    s.push(' ');
    write_f64(s, c.y);
    if dims.has_z {
        s.push(' ');
        write_f64(s, take_extra(extra, offset));
    }
    if dims.has_m {
        s.push(' ');
        write_f64(s, take_extra(extra, offset));
    }
}

fn write_coord_list(
    s: &mut String,
    coords: &[Coord<f64>],
    dims: Dims,
    extra: &[f64],
    offset: &mut usize,
) {
    if let Some(first) = coords.first() {
        write_coord(s, first, dims, extra, offset);
        for c in &coords[1..] {
            s.push_str(", ");
            write_coord(s, c, dims, extra, offset);
        }
    }
}

fn write_header(s: &mut String, name: &str, dims: Dims) {
    s.push_str(name);
    if let Some(m) = modifier(dims) {
        s.push(' ');
        s.push_str(m);
    }
}

/// Writes `((exterior), (hole), ...)`. A ring with no coordinates is dropped: OGC WKT has
/// no EMPTY inside a ring list, so `((...), EMPTY)` is unreadable to a strict parser.
fn write_polygon_rings(
    s: &mut String,
    poly: &Polygon<f64>,
    dims: Dims,
    extra: &[f64],
    offset: &mut usize,
) {
    s.push_str("((");
    write_coord_list(s, &poly.exterior().0, dims, extra, offset);
    s.push(')');
    for h in poly.interiors() {
        if h.0.is_empty() {
            continue;
        }
        s.push_str(", (");
        write_coord_list(s, &h.0, dims, extra, offset);
        s.push(')');
    }
    s.push(')');
}

fn write_geom(s: &mut String, geom: &Geometry<f64>, dims: Dims, extra: &[f64], offset: &mut usize) {
    match geom {
        Geometry::Point(p) => {
            write_header(s, "POINT", dims);
            if p.0.x.is_nan() && p.0.y.is_nan() {
                s.push_str(" EMPTY");
            } else {
                s.push_str(" (");
                write_coord(s, &p.0, dims, extra, offset);
                s.push(')');
            }
        }
        Geometry::LineString(ls) => {
            write_header(s, "LINESTRING", dims);
            if ls.0.is_empty() {
                s.push_str(" EMPTY");
            } else {
                s.push_str(" (");
                write_coord_list(s, &ls.0, dims, extra, offset);
                s.push(')');
            }
        }
        Geometry::Polygon(poly) => {
            write_header(s, "POLYGON", dims);
            // An empty shell has no writable ring, whatever the holes hold.
            if poly.exterior().0.is_empty() {
                s.push_str(" EMPTY");
            } else {
                s.push(' ');
                write_polygon_rings(s, poly, dims, extra, offset);
            }
        }
        Geometry::MultiPoint(mp) => {
            write_header(s, "MULTIPOINT", dims);
            if mp.0.is_empty() {
                s.push_str(" EMPTY");
            } else {
                s.push_str(" (");
                for (i, p) in mp.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_coord(s, &p.0, dims, extra, offset);
                }
                s.push(')');
            }
        }
        Geometry::MultiLineString(mls) => {
            write_header(s, "MULTILINESTRING", dims);
            if mls.0.is_empty() {
                s.push_str(" EMPTY");
            } else {
                s.push_str(" (");
                for (i, ls) in mls.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push('(');
                    write_coord_list(s, &ls.0, dims, extra, offset);
                    s.push(')');
                }
                s.push(')');
            }
        }
        Geometry::MultiPolygon(mp) => {
            // A part with no shell cannot be written as a ring list either, so it leaves
            // the collection, exactly as an empty hole leaves its polygon.
            let has_ring = mp.0.iter().any(|p| !p.exterior().0.is_empty());
            write_header(s, "MULTIPOLYGON", dims);
            if !has_ring {
                s.push_str(" EMPTY");
            } else {
                s.push_str(" (");
                let mut first = true;
                for poly in &mp.0 {
                    if poly.exterior().0.is_empty() {
                        continue;
                    }
                    if !first {
                        s.push_str(", ");
                    }
                    first = false;
                    write_polygon_rings(s, poly, dims, extra, offset);
                }
                s.push(')');
            }
        }
        Geometry::GeometryCollection(gc) => {
            write_header(s, "GEOMETRYCOLLECTION", dims);
            if gc.0.is_empty() {
                s.push_str(" EMPTY");
            } else {
                s.push_str(" (");
                for (i, g) in gc.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_geom(s, g, dims, extra, offset);
                }
                s.push(')');
            }
        }
        Geometry::Line(line) => {
            write_header(s, "LINESTRING", dims);
            s.push_str(" (");
            write_coord(s, &line.start, dims, extra, offset);
            s.push_str(", ");
            write_coord(s, &line.end, dims, extra, offset);
            s.push(')');
        }
        Geometry::Rect(rect) => {
            write_header(s, "POLYGON", dims);
            s.push_str(" ((");
            write_coord(
                s,
                &Coord {
                    x: rect.min().x,
                    y: rect.min().y,
                },
                dims,
                extra,
                offset,
            );
            s.push_str(", ");
            write_coord(
                s,
                &Coord {
                    x: rect.max().x,
                    y: rect.min().y,
                },
                dims,
                extra,
                offset,
            );
            s.push_str(", ");
            write_coord(
                s,
                &Coord {
                    x: rect.max().x,
                    y: rect.max().y,
                },
                dims,
                extra,
                offset,
            );
            s.push_str(", ");
            write_coord(
                s,
                &Coord {
                    x: rect.min().x,
                    y: rect.max().y,
                },
                dims,
                extra,
                offset,
            );
            s.push_str(", ");
            write_coord(
                s,
                &Coord {
                    x: rect.min().x,
                    y: rect.min().y,
                },
                dims,
                extra,
                offset,
            );
            s.push_str("))");
        }
        Geometry::Triangle(tri) => {
            write_header(s, "POLYGON", dims);
            s.push_str(" ((");
            write_coord(s, &tri.v1(), dims, extra, offset);
            s.push_str(", ");
            write_coord(s, &tri.v2(), dims, extra, offset);
            s.push_str(", ");
            write_coord(s, &tri.v3(), dims, extra, offset);
            s.push_str(", ");
            write_coord(s, &tri.v1(), dims, extra, offset);
            s.push_str("))");
        }
    }
}

/// Serialize EWKT, the PostGIS form with a SRID prefix. `None` writes plain WKT.
pub fn write_ewkt(wkt: &WktGeometry, srid: Option<u32>) -> String {
    match srid {
        Some(s) => alloc::format!("SRID={s};{}", write_wkt_zm(wkt)),
        None => write_wkt_zm(wkt),
    }
}

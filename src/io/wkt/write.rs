//! WKT writer: geometry serializer + type inference.

use super::*;
use std::io::{self, Write};


/// Serialize a `Geometry<f64>` to WKT and write it to any `io::Write` target.
///
/// Formats the geometry using [`write_wkt`] and writes the resulting
/// string to `writer`. Returns `io::Result<()>` so callers can handle
/// write errors (e.g. broken pipe, disk full).
pub fn write_wkt_to(geom: &Geometry<f64>, writer: &mut impl Write) -> io::Result<()> {
    let s = write_wkt(geom);
    writer.write_all(s.as_bytes())
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

/// Serialize a `Geometry<f64>` to WKT string.
///
/// The output uses standard OGC WKT formatting (space after type keyword,
/// spaces after commas). The same geometry can be round-tripped through
/// [`read_wkt`].
pub fn write_wkt(geom: &Geometry<f64>) -> String {
    let mut s = String::new();
    write_geom(&mut s, geom);
    s
}

fn write_f64(s: &mut String, v: f64) {
    let mut buf = ryu::Buffer::new();
    s.push_str(buf.format(v));
}

fn write_coord(s: &mut String, c: &Coord<f64>) {
    write_f64(s, c.x);
    s.push(' ');
    write_f64(s, c.y);
}

fn write_coord_list(s: &mut String, coords: &[Coord<f64>]) {
    if let Some(first) = coords.first() {
        write_coord(s, first);
        for c in &coords[1..] {
            s.push_str(", ");
            write_coord(s, c);
        }
    }
}

fn write_linestring(s: &mut String, ls: &LineString<f64>) {
    write_coord_list(s, &ls.0);
}

fn write_polygon_rings(s: &mut String, poly: &Polygon<f64>) {
    s.push_str("((");
    write_coord_list(s, &poly.exterior().0);
    s.push(')');
    for h in poly.interiors() {
        s.push_str(", (");
        write_coord_list(s, &h.0);
        s.push(')');
    }
    s.push(')');
}

fn write_geom(s: &mut String, geom: &Geometry<f64>) {
    match geom {
        Geometry::Point(p) => {
            if p.0.x.is_nan() && p.0.y.is_nan() {
                s.push_str("POINT EMPTY");
            } else {
                s.push_str("POINT (");
                write_coord(s, &p.0);
                s.push(')');
            }
        }
        Geometry::LineString(ls) => {
            if ls.0.is_empty() {
                s.push_str("LINESTRING EMPTY");
            } else {
                s.push_str("LINESTRING (");
                write_linestring(s, ls);
                s.push(')');
            }
        }
        Geometry::Polygon(poly) => {
            if poly.exterior().0.is_empty() {
                s.push_str("POLYGON EMPTY");
            } else {
                s.push_str("POLYGON ");
                write_polygon_rings(s, poly);
            }
        }
        Geometry::MultiPoint(mp) => {
            if mp.0.is_empty() {
                s.push_str("MULTIPOINT EMPTY");
            } else {
                s.push_str("MULTIPOINT (");
                for (i, p) in mp.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_coord(s, &p.0);
                }
                s.push(')');
            }
        }
        Geometry::MultiLineString(mls) => {
            if mls.0.is_empty() {
                s.push_str("MULTILINESTRING EMPTY");
            } else {
                s.push_str("MULTILINESTRING (");
                for (i, ls) in mls.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push('(');
                    write_linestring(s, ls);
                    s.push(')');
                }
                s.push(')');
            }
        }
        Geometry::MultiPolygon(mp) => {
            if mp.0.is_empty() {
                s.push_str("MULTIPOLYGON EMPTY");
            } else {
                s.push_str("MULTIPOLYGON (");
                for (i, poly) in mp.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_polygon_rings(s, poly);
                }
                s.push(')');
            }
        }
        Geometry::GeometryCollection(gc) => {
            if gc.0.is_empty() {
                s.push_str("GEOMETRYCOLLECTION EMPTY");
            } else {
                s.push_str("GEOMETRYCOLLECTION (");
                for (i, g) in gc.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_geom(s, g);
                }
                s.push(')');
            }
        }
        // OGC WKT has no Line/Rect/Triangle types; serialize them losslessly
        // as their closest OGC equivalents (coordinate-exact).
        Geometry::Line(line) => {
            s.push_str("LINESTRING (");
            write_coord(s, &line.start);
            s.push_str(", ");
            write_coord(s, &line.end);
            s.push(')');
        }
        Geometry::Rect(rect) => {
            s.push_str("POLYGON ((");
            write_coord(s, &Coord {
                x: rect.min().x,
                y: rect.min().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.max().x,
                y: rect.min().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.max().x,
                y: rect.max().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.min().x,
                y: rect.max().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.min().x,
                y: rect.min().y,
            });
            s.push_str("))");
        }
        Geometry::Triangle(tri) => {
            s.push_str("POLYGON ((");
            write_coord(s, &tri.v1());
            s.push_str(", ");
            write_coord(s, &tri.v2());
            s.push_str(", ");
            write_coord(s, &tri.v3());
            s.push_str(", ");
            write_coord(s, &tri.v1());
            s.push_str("))");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

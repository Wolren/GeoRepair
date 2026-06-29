//! Single-pass WKB parser and encoder for 2D geometries.
//!
//! No dependencies beyond `geo`. Handles both big-endian (NDR) and
//! little-endian (XDR) byte order, including mixed orders inside
//! multi-geometries. Supports EWKB type flag stripping.

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::core::MakeValidError;

// ---------------------------------------------------------------------------
// WKB type codes (ISO standard, 2D only)
// ---------------------------------------------------------------------------

const WKB_POINT: u32 = 1;
const WKB_LINESTRING: u32 = 2;
const WKB_POLYGON: u32 = 3;
const WKB_MULTIPOINT: u32 = 4;
const WKB_MULTILINESTRING: u32 = 5;
const WKB_MULTIPOLYGON: u32 = 6;
const WKB_GEOMETRYCOLLECTION: u32 = 7;

const WKB_Z_FLAG: u32 = 0x80000000;
const WKB_M_FLAG: u32 = 0x40000000;
const WKB_SRID_FLAG: u32 = 0x20000000;

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Parse a single WKB geometry from `buf`. Returns the geometry and the
/// number of bytes consumed.
pub fn read_wkb(buf: &[u8]) -> Result<Geometry<f64>, MakeValidError> {
    let mut pos = 0;
    let geom = read_geometry(buf, &mut pos)?;
    Ok(geom)
}

/// Low-level: read one geometry from `buf` starting at `pos`, advancing
/// `pos` past the geometry.
fn read_geometry(buf: &[u8], pos: &mut usize) -> Result<Geometry<f64>, MakeValidError> {
    let le = read_byte_order(buf, pos)?;
    let raw_type = read_u32(buf, pos, le)?;
    let has_z = (raw_type & WKB_Z_FLAG) != 0;
    let has_m = (raw_type & WKB_M_FLAG) != 0;
    let has_srid = (raw_type & WKB_SRID_FLAG) != 0;
    let type_code = raw_type & 0xff;

    // Skip SRID if present (4 bytes after type)
    if has_srid {
        let _srid = read_u32(buf, pos, le)?;
    }

    let dims = if has_z && has_m {
        4
    } else if has_z || has_m {
        3
    } else {
        2
    };

    match type_code {
        WKB_POINT => read_point_inner(buf, pos, le, dims).map(Geometry::Point),
        WKB_LINESTRING => read_linestring_inner(buf, pos, le, dims).map(Geometry::LineString),
        WKB_POLYGON => read_polygon_inner(buf, pos, le, dims).map(Geometry::Polygon),
        WKB_MULTIPOINT => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut points = Vec::with_capacity(n);
            for _ in 0..n {
                let sub_le = read_byte_order(buf, pos)?;
                let sub_raw = read_u32(buf, pos, sub_le)?;
                let sub_code = sub_raw & 0xff;
                if sub_code != WKB_POINT {
                    return Err(MakeValidError::ParseError(
                        "expected Point inside MultiPoint WKB".into(),
                    ));
                }
                let sub_z = (sub_raw & WKB_Z_FLAG) != 0;
                let sub_m = (sub_raw & WKB_M_FLAG) != 0;
                let sub_dims = if sub_z && sub_m {
                    4
                } else if sub_z || sub_m {
                    3
                } else {
                    2
                };
                points.push(read_point_inner(buf, pos, sub_le, sub_dims)?);
            }
            Ok(Geometry::MultiPoint(MultiPoint(points)))
        }
        WKB_MULTILINESTRING => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut lines = Vec::with_capacity(n);
            for _ in 0..n {
                let sub_le = read_byte_order(buf, pos)?;
                let sub_raw = read_u32(buf, pos, sub_le)?;
                let sub_code = sub_raw & 0xff;
                if sub_code != WKB_LINESTRING {
                    return Err(MakeValidError::ParseError(
                        "expected LineString inside MultiLineString WKB".into(),
                    ));
                }
                let sub_z = (sub_raw & WKB_Z_FLAG) != 0;
                let sub_m = (sub_raw & WKB_M_FLAG) != 0;
                let sub_dims = if sub_z && sub_m {
                    4
                } else if sub_z || sub_m {
                    3
                } else {
                    2
                };
                lines.push(read_linestring_inner(buf, pos, sub_le, sub_dims)?);
            }
            Ok(Geometry::MultiLineString(MultiLineString(lines)))
        }
        WKB_MULTIPOLYGON => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut polys = Vec::with_capacity(n);
            for _ in 0..n {
                let sub_le = read_byte_order(buf, pos)?;
                let sub_raw = read_u32(buf, pos, sub_le)?;
                let sub_code = sub_raw & 0xff;
                if sub_code != WKB_POLYGON {
                    return Err(MakeValidError::ParseError(
                        "expected Polygon inside MultiPolygon WKB".into(),
                    ));
                }
                let sub_z = (sub_raw & WKB_Z_FLAG) != 0;
                let sub_m = (sub_raw & WKB_M_FLAG) != 0;
                let sub_dims = if sub_z && sub_m {
                    4
                } else if sub_z || sub_m {
                    3
                } else {
                    2
                };
                polys.push(read_polygon_inner(buf, pos, sub_le, sub_dims)?);
            }
            Ok(Geometry::MultiPolygon(MultiPolygon(polys)))
        }
        WKB_GEOMETRYCOLLECTION => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut geoms = Vec::with_capacity(n);
            for _ in 0..n {
                geoms.push(read_geometry(buf, pos)?);
            }
            Ok(Geometry::GeometryCollection(GeometryCollection(geoms)))
        }
        _ => Err(MakeValidError::ParseError(format!(
            "unknown WKB type code: {type_code}"
        ))),
    }
}

/// Read byte order byte. Returns `true` for little-endian (NDR),
/// `false` for big-endian (XDR).
fn read_byte_order(buf: &[u8], pos: &mut usize) -> Result<bool, MakeValidError> {
    if *pos >= buf.len() {
        return Err(MakeValidError::ParseError(
            "unexpected EOF in WKB byte order".into(),
        ));
    }
    let b = buf[*pos];
    *pos += 1;
    match b {
        0 => Ok(false), // XDR (big-endian)
        1 => Ok(true),  // NDR (little-endian)
        _ => Err(MakeValidError::ParseError(format!(
            "invalid WKB byte order: {b}"
        ))),
    }
}

#[inline(always)]
fn read_u32(buf: &[u8], pos: &mut usize, le: bool) -> Result<u32, MakeValidError> {
    if *pos + 4 > buf.len() {
        return Err(MakeValidError::ParseError(
            "unexpected EOF reading u32".into(),
        ));
    }
    // SAFETY: bounds checked above. read_unaligned handles misalignment on x86_64/aarch64.
    let v = unsafe { (buf.as_ptr().add(*pos) as *const u32).read_unaligned() };
    *pos += 4;
    // cfg!() is constant-folded. On x86_64 (LE), this simplifies to:
    //   le == true  -> v (direct, no conversion)
    //   le == false -> v.swap_bytes()
    Ok(if cfg!(target_endian = "little") == le {
        v
    } else {
        v.swap_bytes()
    })
}

#[inline(always)]
fn read_f64(buf: &[u8], pos: &mut usize, le: bool) -> Result<f64, MakeValidError> {
    if *pos + 8 > buf.len() {
        return Err(MakeValidError::ParseError(
            "unexpected EOF reading f64".into(),
        ));
    }
    // SAFETY: bounds checked above. read_unaligned handles misalignment on x86_64/aarch64.
    // On x86_64 (LE) with le=true: loads directly into an XMM register — zero copy, zero conversion.
    let v = unsafe { (buf.as_ptr().add(*pos) as *const f64).read_unaligned() };
    *pos += 8;
    Ok(if cfg!(target_endian = "little") == le {
        v
    } else {
        f64::from_bits(v.to_bits().swap_bytes())
    })
}

/// Read a coordinate pair, skipping Z/M values if present.
fn read_coord(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
) -> Result<Coord<f64>, MakeValidError> {
    let x = read_f64(buf, pos, le)?;
    let y = read_f64(buf, pos, le)?;
    // Skip Z and/or M values
    for _ in 2..dims {
        let _ = read_f64(buf, pos, le)?;
    }
    Ok(Coord { x, y })
}

fn read_point_inner(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
) -> Result<Point<f64>, MakeValidError> {
    let c = read_coord(buf, pos, le, dims)?;
    Ok(Point(c))
}

fn read_linestring_inner(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
) -> Result<LineString<f64>, MakeValidError> {
    let n = read_u32(buf, pos, le)? as usize;
    let mut coords = Vec::with_capacity(n);
    for _ in 0..n {
        coords.push(read_coord(buf, pos, le, dims)?);
    }
    Ok(LineString::new(coords))
}

fn read_polygon_inner(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
) -> Result<Polygon<f64>, MakeValidError> {
    let n_rings = read_u32(buf, pos, le)? as usize;
    if n_rings == 0 {
        return Ok(Polygon::new(LineString::new(vec![]), vec![]));
    }
    let exterior = read_linestring_inner(buf, pos, le, dims)?;
    let mut holes = Vec::with_capacity(n_rings.saturating_sub(1));
    for _ in 1..n_rings {
        holes.push(read_linestring_inner(buf, pos, le, dims)?);
    }
    Ok(Polygon::new(exterior, holes))
}

// ---------------------------------------------------------------------------
// Writer (always little-endian, 2D only)
// ---------------------------------------------------------------------------

/// Encode a geometry as little-endian WKB bytes (2D only, no SRID).
pub fn write_wkb(geom: &Geometry<f64>) -> Vec<u8> {
    write_wkb_impl(geom)
}

fn write_wkb_impl(geom: &Geometry<f64>) -> Vec<u8> {
    // Pre-compute size
    let size = wkb_size(geom);
    let mut buf = Vec::with_capacity(size);
    write_geometry(geom, &mut buf);
    debug_assert_eq!(buf.len(), size, "WKB size mismatch");
    buf
}

/// Compute the byte size of a geometry in WKB format.
fn wkb_size(geom: &Geometry<f64>) -> usize {
    use geo::Geometry::*;
    // 1 (byte order) + 4 (type)
    let header = 5;
    match geom {
        Point(_) => header + 16,
        LineString(ls) => header + 4 + ls.0.len() * 16,
        Polygon(poly) => {
            let mut sz = header + 4;
            sz += 4 + poly.exterior().0.len() * 16; // ring count + exterior
            for h in poly.interiors() {
                sz += 4 + h.0.len() * 16;
            }
            sz
        }
        MultiPoint(mp) => {
            let mut sz = header + 4; // header + count
            for _ in &mp.0 {
                sz += 5 + 16; // sub-geom header + point
            }
            sz
        }
        MultiLineString(mls) => {
            let mut sz = header + 4;
            for ls in &mls.0 {
                sz += 5 + 4 + ls.0.len() * 16;
            }
            sz
        }
        MultiPolygon(mp) => {
            let mut sz = header + 4;
            for poly in &mp.0 {
                // sub-header + ring count
                sz += 5 + 4;
                sz += 4 + poly.exterior().0.len() * 16;
                for h in poly.interiors() {
                    sz += 4 + h.0.len() * 16;
                }
            }
            sz
        }
        GeometryCollection(gc) => {
            let mut sz = header + 4;
            for g in &gc.0 {
                sz += wkb_size(g);
            }
            sz
        }
        _ => header + 4, // Triangle, Rect, etc. — encode as empty
    }
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_f64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_coord(buf: &mut Vec<u8>, c: &Coord<f64>) {
    write_f64(buf, c.x);
    write_f64(buf, c.y);
}

fn write_point(buf: &mut Vec<u8>, p: &Point<f64>) {
    write_coord(buf, &p.0);
}

fn write_linestring(buf: &mut Vec<u8>, ls: &LineString<f64>) {
    write_u32(buf, ls.0.len() as u32);
    for c in &ls.0 {
        write_coord(buf, c);
    }
}

fn write_polygon(buf: &mut Vec<u8>, poly: &Polygon<f64>) {
    let n_holes = poly.interiors().len();
    write_u32(buf, (1 + n_holes) as u32);
    write_linestring(buf, poly.exterior());
    for h in poly.interiors() {
        write_linestring(buf, h);
    }
}

fn write_geometry(geom: &Geometry<f64>, buf: &mut Vec<u8>) {
    use geo::Geometry::*;
    // Always little-endian (NDR)
    buf.push(1); // byte order = LE

    match geom {
        Point(p) => {
            write_u32(buf, WKB_POINT);
            write_point(buf, p);
        }
        LineString(ls) => {
            write_u32(buf, WKB_LINESTRING);
            write_linestring(buf, ls);
        }
        Polygon(poly) => {
            write_u32(buf, WKB_POLYGON);
            write_polygon(buf, poly);
        }
        MultiPoint(mp) => {
            write_u32(buf, WKB_MULTIPOINT);
            write_u32(buf, mp.0.len() as u32);
            for p in &mp.0 {
                buf.push(1); // LE
                write_u32(buf, WKB_POINT);
                write_point(buf, p);
            }
        }
        MultiLineString(mls) => {
            write_u32(buf, WKB_MULTILINESTRING);
            write_u32(buf, mls.0.len() as u32);
            for ls in &mls.0 {
                buf.push(1);
                write_u32(buf, WKB_LINESTRING);
                write_linestring(buf, ls);
            }
        }
        MultiPolygon(mp) => {
            write_u32(buf, WKB_MULTIPOLYGON);
            write_u32(buf, mp.0.len() as u32);
            for poly in &mp.0 {
                buf.push(1);
                write_u32(buf, WKB_POLYGON);
                write_polygon(buf, poly);
            }
        }
        GeometryCollection(gc) => {
            write_u32(buf, WKB_GEOMETRYCOLLECTION);
            write_u32(buf, gc.0.len() as u32);
            for g in &gc.0 {
                write_geometry(g, buf);
            }
        }
        _ => {
            // Unsupported types (Triangle, Rect) — encode as empty GC
            write_u32(buf, WKB_GEOMETRYCOLLECTION);
            write_u32(buf, 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-geometry helpers
// ---------------------------------------------------------------------------

/// Read the byte size of the next WKB geometry from `buf` without
/// fully parsing it. Useful for skipping or chunking concatenated WKB.
pub fn estimate_wkb_size(buf: &[u8]) -> Result<usize, MakeValidError> {
    let mut pos = 0;
    let _geom = read_geometry(buf, &mut pos)?;
    Ok(pos)
}

/// Parse a concatenated sequence of WKB geometries.
pub fn read_wkb_concat(buf: &[u8]) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let mut offset = 0;
    let mut geoms = Vec::new();
    while offset < buf.len() {
        let remaining = &buf[offset..];
        let geom = read_wkb(remaining)?;
        let consumed = estimate_wkb_size(remaining)?;
        geoms.push(geom);
        offset += consumed;
    }
    Ok(geoms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_point_le() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_point_be() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        // Manually construct big-endian WKB
        let mut wkb = Vec::new();
        wkb.push(0); // byte order = BE
        wkb.extend_from_slice(&1u32.to_be_bytes()); // Point type
        wkb.extend_from_slice(&1.0f64.to_be_bytes());
        wkb.extend_from_slice(&2.0f64.to_be_bytes());
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_linestring() {
        let g = Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ]));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_polygon() {
        let g = Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ])],
        ));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_multipoint() {
        let g = Geometry::MultiPoint(MultiPoint(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_multilinestring() {
        let g = Geometry::MultiLineString(MultiLineString(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
            LineString::new(vec![Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }]),
        ]));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_multipolygon() {
        let g = Geometry::MultiPolygon(MultiPolygon(vec![
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 },
                    Coord { x: 1.0, y: 0.0 },
                    Coord { x: 1.0, y: 1.0 },
                    Coord { x: 0.0, y: 1.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                vec![],
            ),
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 2.0, y: 2.0 },
                    Coord { x: 3.0, y: 2.0 },
                    Coord { x: 3.0, y: 3.0 },
                    Coord { x: 2.0, y: 3.0 },
                    Coord { x: 2.0, y: 2.0 },
                ]),
                vec![],
            ),
        ]));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn roundtrip_gc() {
        let g = Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ]));
        let wkb = write_wkb(&g);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn estimate_wkb_size_works() {
        let g = Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        ));
        let wkb = write_wkb(&g);
        let est = estimate_wkb_size(&wkb).unwrap();
        assert_eq!(est, wkb.len());
    }

    #[test]
    fn read_wkb_concat_works() {
        let g1 = Geometry::Point(Point::new(1.0, 2.0));
        let g2 = Geometry::Point(Point::new(3.0, 4.0));
        let mut concat = Vec::new();
        concat.extend_from_slice(&write_wkb(&g1));
        concat.extend_from_slice(&write_wkb(&g2));
        let geoms = read_wkb_concat(&concat).unwrap();
        assert_eq!(geoms.len(), 2);
        assert_eq!(geoms[0], g1);
        assert_eq!(geoms[1], g2);
    }

    #[test]
    fn ewkb_srid_stripped() {
        // EWKB point with SRID flag and SRID value
        let mut wkb = Vec::new();
        wkb.push(1); // LE
                     // type = Point (1) | SRID flag (0x20000000)
        let type_with_srid = 1u32 | WKB_SRID_FLAG;
        wkb.extend_from_slice(&type_with_srid.to_le_bytes());
        wkb.extend_from_slice(&4326u32.to_le_bytes()); // SRID
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());

        let geom = read_wkb(&wkb).unwrap();
        assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
    }

    #[test]
    fn mixed_byte_order_multipolygon() {
        // Outer: LE
        // Inner polygon 1: BE
        // Inner polygon 2: LE
        let mut wkb = Vec::new();

        // Header: LE, MultiPolygon
        wkb.push(1);
        wkb.extend_from_slice(&6u32.to_le_bytes()); // MultiPolygon
        wkb.extend_from_slice(&2u32.to_le_bytes()); // 2 polygons

        // Polygon 1: BE
        wkb.push(0); // BE
        wkb.extend_from_slice(&3u32.to_be_bytes()); // Polygon
        wkb.extend_from_slice(&1u32.to_be_bytes()); // 1 ring
        wkb.extend_from_slice(&5u32.to_be_bytes()); // 5 coords
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());

        // Polygon 2: LE
        wkb.push(1); // LE
        wkb.extend_from_slice(&3u32.to_le_bytes()); // Polygon
        wkb.extend_from_slice(&1u32.to_le_bytes()); // 1 ring
        wkb.extend_from_slice(&5u32.to_le_bytes()); // 5 coords
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());

        let geom = read_wkb(&wkb).unwrap();
        if let Geometry::MultiPolygon(mp) = geom {
            assert_eq!(mp.0.len(), 2);
        } else {
            panic!("expected MultiPolygon");
        }
    }
}

//! WKB writer: OGC + EWKB encoders and size estimation.

use super::*;
use std::io::{self, Write};

/// Encode a geometry as little-endian WKB bytes (2D, no SRID).
///
/// Produces standard OGC WKB. Use [`write_ewkb`] to preserve SRID and
/// Z/M dimension data. Output can be decoded with [`read_wkb`].
/// For big-endian output, use [`write_wkb_with_opts`].
pub fn write_wkb(geom: &Geometry<f64>) -> Vec<u8> {
    write_wkb_with_opts(geom, &WriteOptions::default())
}

/// Encode a geometry as WKB bytes with the given options.
///
/// Allows specifying the byte order via [`WriteOptions`]. For the default
/// little-endian output, use [`write_wkb`].
pub fn write_wkb_with_opts(geom: &Geometry<f64>, opts: &WriteOptions) -> Vec<u8> {
    let le = opts.endianness == Endianness::LittleEndian;
    let size = wkb_size(geom);
    let mut buf = Vec::with_capacity(size);
    write_geometry(geom, &mut buf, le);
    debug_assert_eq!(buf.len(), size, "WKB size mismatch");
    buf
}

/// Serialize a geometry as WKB and write it to any `io::Write` target.
///
/// Uses little-endian byte order. For endianness control, use
/// [`write_wkb_with_opts`] and write the bytes yourself.
pub fn write_wkb_to(geom: &Geometry<f64>, writer: &mut impl Write) -> io::Result<()> {
    let bytes = write_wkb(geom);
    writer.write_all(&bytes)
}

/// Encode an EWKB geometry with SRID and Z/M dimension preservation.
///
/// Produces little-endian EWKB bytes that include SRID (if set) and
/// Z/M dimension flags with extra coordinate values. Use [`read_ewkb`]
/// to decode the output.
///
/// # Example
///
/// ```rust
/// use geo_repair::{EwkbDims, EwkbGeometry, read_ewkb, write_ewkb};
///
/// let geom = geo::Geometry::Point(geo::point!(x: 1.0, y: 2.0));
/// let ewkb = EwkbGeometry {
///     geometry: geom,
///     srid: None,
///     dims: EwkbDims::XYZ,
///     extra_coords: vec![3.0],
/// };
/// let bytes = write_ewkb(&ewkb);
/// let roundtrip = read_ewkb(&bytes).unwrap();
/// assert_eq!(roundtrip.extra_coords, vec![3.0]);
/// ```
///
/// For endianness control, construct a byte buffer manually and write
/// with [`write_wkb_with_opts`].
pub fn write_ewkb(geom: &EwkbGeometry) -> Vec<u8> {
    let size = ewkb_size(&geom.geometry, geom.dims, geom.srid.is_some());
    let mut buf = Vec::with_capacity(size);
    let mut extra_offset = 0;
    write_geometry_ewkb(
        &mut buf,
        &geom.geometry,
        geom.dims,
        geom.srid,
        &geom.extra_coords,
        &mut extra_offset,
        true,
    );
    debug_assert_eq!(buf.len(), size, "EWKB size mismatch");
    debug_assert_eq!(
        extra_offset,
        geom.extra_coords.len(),
        "EWKB writer did not consume all extra coords"
    );
    buf
}

/// Compute the byte size of a geometry in 2D WKB format.
fn wkb_size(geom: &Geometry<f64>) -> usize {
    ewkb_size(geom, EwkbDims::XY, false)
}

/// Compute the byte size of a geometry in EWKB format with given dimension and SRID.
fn ewkb_size(geom: &Geometry<f64>, dims: EwkbDims, has_srid: bool) -> usize {
    use geo::Geometry::*;
    let header = 1 + 4 + if has_srid { 4 } else { 0 };
    let coord_bytes = dims.coord_count() as usize * 8;
    match geom {
        Point(_) => header + coord_bytes,
        LineString(ls) => header + 4 + ls.0.len() * coord_bytes,
        Polygon(poly) => {
            let mut sz = header + 4;
            sz += 4 + poly.exterior().0.len() * coord_bytes;
            for h in poly.interiors() {
                sz += 4 + h.0.len() * coord_bytes;
            }
            sz
        }
        MultiPoint(mp) => {
            let mut sz = header + 4;
            for _ in &mp.0 {
                sz += 5 + coord_bytes; // sub-geom header + point
            }
            sz
        }
        MultiLineString(mls) => {
            let mut sz = header + 4;
            for ls in &mls.0 {
                sz += 5 + 4 + ls.0.len() * coord_bytes;
            }
            sz
        }
        MultiPolygon(mp) => {
            let mut sz = header + 4;
            for poly in &mp.0 {
                sz += 5 + 4;
                sz += 4 + poly.exterior().0.len() * coord_bytes;
                for h in poly.interiors() {
                    sz += 4 + h.0.len() * coord_bytes;
                }
            }
            sz
        }
        GeometryCollection(gc) => {
            let mut sz = header + 4;
            for g in &gc.0 {
                sz += ewkb_size(g, dims, false);
            }
            sz
        }
        // OGC WKB has no Line/Rect/Triangle types; encode them losslessly as
        // their closest OGC equivalents, matching write_geometry.
        Line(_) => header + 4 + 2 * coord_bytes,
        Rect(_) => header + 4 + (4 + 5 * coord_bytes),
        Triangle(_) => header + 4 + (4 + 4 * coord_bytes),
    }
}

fn write_u32(buf: &mut Vec<u8>, v: u32, le: bool) {
    if le {
        buf.extend_from_slice(&v.to_le_bytes());
    } else {
        buf.extend_from_slice(&v.to_be_bytes());
    }
}

fn write_f64(buf: &mut Vec<u8>, v: f64, le: bool) {
    if le {
        buf.extend_from_slice(&v.to_le_bytes());
    } else {
        buf.extend_from_slice(&v.to_be_bytes());
    }
}

fn write_coord(buf: &mut Vec<u8>, c: &Coord<f64>, le: bool) {
    write_f64(buf, c.x, le);
    write_f64(buf, c.y, le);
}

fn write_point(buf: &mut Vec<u8>, p: &Point<f64>, le: bool) {
    write_coord(buf, &p.0, le);
}

fn write_linestring(buf: &mut Vec<u8>, ls: &LineString<f64>, le: bool) {
    write_u32(buf, ls.0.len() as u32, le);
    for c in &ls.0 {
        write_coord(buf, c, le);
    }
}

fn write_polygon(buf: &mut Vec<u8>, poly: &Polygon<f64>, le: bool) {
    let n_holes = poly.interiors().len();
    write_u32(buf, (1 + n_holes) as u32, le);
    write_linestring(buf, poly.exterior(), le);
    for h in poly.interiors() {
        write_linestring(buf, h, le);
    }
}

fn write_geometry(geom: &Geometry<f64>, buf: &mut Vec<u8>, le: bool) {
    use geo::Geometry::*;
    buf.push(if le { 1 } else { 0 }); // byte order

    match geom {
        Point(p) => {
            write_u32(buf, WKB_POINT, le);
            write_point(buf, p, le);
        }
        LineString(ls) => {
            write_u32(buf, WKB_LINESTRING, le);
            write_linestring(buf, ls, le);
        }
        Polygon(poly) => {
            write_u32(buf, WKB_POLYGON, le);
            write_polygon(buf, poly, le);
        }
        MultiPoint(mp) => {
            write_u32(buf, WKB_MULTIPOINT, le);
            write_u32(buf, mp.0.len() as u32, le);
            for p in &mp.0 {
                buf.push(if le { 1 } else { 0 });
                write_u32(buf, WKB_POINT, le);
                write_point(buf, p, le);
            }
        }
        MultiLineString(mls) => {
            write_u32(buf, WKB_MULTILINESTRING, le);
            write_u32(buf, mls.0.len() as u32, le);
            for ls in &mls.0 {
                buf.push(if le { 1 } else { 0 });
                write_u32(buf, WKB_LINESTRING, le);
                write_linestring(buf, ls, le);
            }
        }
        MultiPolygon(mp) => {
            write_u32(buf, WKB_MULTIPOLYGON, le);
            write_u32(buf, mp.0.len() as u32, le);
            for poly in &mp.0 {
                buf.push(if le { 1 } else { 0 });
                write_u32(buf, WKB_POLYGON, le);
                write_polygon(buf, poly, le);
            }
        }
        GeometryCollection(gc) => {
            write_u32(buf, WKB_GEOMETRYCOLLECTION, le);
            write_u32(buf, gc.0.len() as u32, le);
            for g in &gc.0 {
                write_geometry(g, buf, le);
            }
        }
        // OGC WKB has no Line/Rect/Triangle types; encode them losslessly as
        // their closest OGC equivalents (coordinate-exact, matching
        // wkb_size). Reading the bytes back yields the OGC type.
        Line(l) => {
            write_u32(buf, WKB_LINESTRING, le);
            write_u32(buf, 2, le);
            write_coord(buf, &l.start, le);
            write_coord(buf, &l.end, le);
        }
        Rect(r) => {
            write_u32(buf, WKB_POLYGON, le);
            write_u32(buf, 1, le);
            write_u32(buf, 5, le);
            write_coord(buf, &Coord { x: r.min().x, y: r.min().y }, le);
            write_coord(buf, &Coord { x: r.max().x, y: r.min().y }, le);
            write_coord(buf, &Coord { x: r.max().x, y: r.max().y }, le);
            write_coord(buf, &Coord { x: r.min().x, y: r.max().y }, le);
            write_coord(buf, &Coord { x: r.min().x, y: r.min().y }, le);
        }
        Triangle(t) => {
            write_u32(buf, WKB_POLYGON, le);
            write_u32(buf, 1, le);
            write_u32(buf, 4, le);
            write_coord(buf, &t.v1(), le);
            write_coord(buf, &t.v2(), le);
            write_coord(buf, &t.v3(), le);
            write_coord(buf, &t.v1(), le);
        }
    }
}

// ---------------------------------------------------------------------------
// EWKB writer
// ---------------------------------------------------------------------------

fn type_code_with_flags(type_code: u32, dims: EwkbDims, has_srid: bool) -> u32 {
    let mut flags = type_code;
    if dims.has_z {
        flags |= WKB_Z_FLAG;
    }
    if dims.has_m {
        flags |= WKB_M_FLAG;
    }
    if has_srid {
        flags |= WKB_SRID_FLAG;
    }
    flags
}

fn write_coord_ewkb(
    buf: &mut Vec<u8>,
    c: &Coord<f64>,
    dims: EwkbDims,
    extra: &[f64],
    offset: &mut usize,
    le: bool,
) {
    write_f64(buf, c.x, le);
    write_f64(buf, c.y, le);
    if dims.has_z {
        write_f64(buf, extra[*offset], le);
        *offset += 1;
    }
    if dims.has_m {
        write_f64(buf, extra[*offset], le);
        *offset += 1;
    }
}

fn write_point_ewkb(
    buf: &mut Vec<u8>,
    p: &Point<f64>,
    dims: EwkbDims,
    extra: &[f64],
    offset: &mut usize,
    le: bool,
) {
    write_coord_ewkb(buf, &p.0, dims, extra, offset, le);
}

fn write_linestring_ewkb(
    buf: &mut Vec<u8>,
    ls: &LineString<f64>,
    dims: EwkbDims,
    extra: &[f64],
    offset: &mut usize,
    le: bool,
) {
    write_u32(buf, ls.0.len() as u32, le);
    for c in &ls.0 {
        write_coord_ewkb(buf, c, dims, extra, offset, le);
    }
}

fn write_polygon_ewkb(
    buf: &mut Vec<u8>,
    poly: &Polygon<f64>,
    dims: EwkbDims,
    extra: &[f64],
    offset: &mut usize,
    le: bool,
) {
    let n_holes = poly.interiors().len();
    write_u32(buf, (1 + n_holes) as u32, le);
    write_linestring_ewkb(buf, poly.exterior(), dims, extra, offset, le);
    for h in poly.interiors() {
        write_linestring_ewkb(buf, h, dims, extra, offset, le);
    }
}

fn write_geometry_ewkb(
    buf: &mut Vec<u8>,
    geom: &Geometry<f64>,
    dims: EwkbDims,
    srid: Option<i32>,
    extra: &[f64],
    offset: &mut usize,
    le: bool,
) {
    use geo::Geometry::*;
    buf.push(if le { 1 } else { 0 }); // byte order

    match geom {
        Point(p) => {
            let tc = type_code_with_flags(WKB_POINT, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_point_ewkb(buf, p, dims, extra, offset, le);
        }
        LineString(ls) => {
            let tc = type_code_with_flags(WKB_LINESTRING, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_linestring_ewkb(buf, ls, dims, extra, offset, le);
        }
        Polygon(poly) => {
            let tc = type_code_with_flags(WKB_POLYGON, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_polygon_ewkb(buf, poly, dims, extra, offset, le);
        }
        MultiPoint(mp) => {
            let tc = type_code_with_flags(WKB_MULTIPOINT, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, mp.0.len() as u32, le);
            for p in &mp.0 {
                // Sub-geometries never carry SRID (inherited from parent)
                write_geometry_ewkb(buf, &Geometry::Point(*p), dims, None, extra, offset, le);
            }
        }
        MultiLineString(mls) => {
            let tc = type_code_with_flags(WKB_MULTILINESTRING, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, mls.0.len() as u32, le);
            for ls in &mls.0 {
                write_geometry_ewkb(
                    buf,
                    &Geometry::LineString(ls.clone()),
                    dims,
                    None,
                    extra,
                    offset,
                    le,
                );
            }
        }
        MultiPolygon(mp) => {
            let tc = type_code_with_flags(WKB_MULTIPOLYGON, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, mp.0.len() as u32, le);
            for poly in &mp.0 {
                write_geometry_ewkb(
                    buf,
                    &Geometry::Polygon(poly.clone()),
                    dims,
                    None,
                    extra,
                    offset,
                    le,
                );
            }
        }
        GeometryCollection(gc) => {
            let tc = type_code_with_flags(WKB_GEOMETRYCOLLECTION, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, gc.0.len() as u32, le);
            for g in &gc.0 {
                write_geometry_ewkb(buf, g, dims, None, extra, offset, le);
            }
        }
        // OGC WKB has no Line/Rect/Triangle types; encode them losslessly as
        // their closest OGC equivalents, matching write_geometry.
        Line(l) => {
            let tc = type_code_with_flags(WKB_LINESTRING, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, 2, le);
            write_coord_ewkb(buf, &l.start, dims, extra, offset, le);
            write_coord_ewkb(buf, &l.end, dims, extra, offset, le);
        }
        Rect(r) => {
            let tc = type_code_with_flags(WKB_POLYGON, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, 1, le);
            write_u32(buf, 5, le);
            let corners = [
                Coord { x: r.min().x, y: r.min().y },
                Coord { x: r.max().x, y: r.min().y },
                Coord { x: r.max().x, y: r.max().y },
                Coord { x: r.min().x, y: r.max().y },
                Coord { x: r.min().x, y: r.min().y },
            ];
            for c in &corners {
                write_coord_ewkb(buf, c, dims, extra, offset, le);
            }
        }
        Triangle(t) => {
            let tc = type_code_with_flags(WKB_POLYGON, dims, srid.is_some());
            write_u32(buf, tc, le);
            if let Some(srid) = srid {
                write_u32(buf, srid as u32, le);
            }
            write_u32(buf, 1, le);
            write_u32(buf, 4, le);
            write_coord_ewkb(buf, &t.v1(), dims, extra, offset, le);
            write_coord_ewkb(buf, &t.v2(), dims, extra, offset, le);
            write_coord_ewkb(buf, &t.v3(), dims, extra, offset, le);
            write_coord_ewkb(buf, &t.v1(), dims, extra, offset, le);
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-geometry helpers
// ---------------------------------------------------------------------------

/// Read the byte size of the next WKB geometry from `buf` without
/// fully parsing it. Useful for skipping or chunking concatenated WKB.
pub fn estimate_wkb_size(buf: &[u8]) -> Result<usize, WkbError> {
    let mut pos = 0;
    let mut dummy_extra = Vec::new();
    let _geom = super::read::read_geometry_inner(buf, &mut pos, &mut dummy_extra)?;
    Ok(pos)
}

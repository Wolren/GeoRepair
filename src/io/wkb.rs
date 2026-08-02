//! Single-pass WKB parser and encoder for 2D geometries.
//!
//! No dependencies beyond `geo`. Handles both big-endian (NDR) and
//! little-endian (XDR) byte order, including mixed orders inside
//! multi-geometries. Supports EWKB type flag stripping.
//!
//! # Writing
//!
//! - [`write_wkb`] — encodes as little-endian WKB
//! - [`write_wkb_with_opts`] — configurable byte order via [`WriteOptions`]
//! - [`write_wkb_to`] — writes WKB to any `io::Write` target
//! - [`write_ewkb`] — EWKB with SRID and Z/M dimension preservation
//!
//! # Reading
//!
//! - [`read_wkb`] — parses WKB/EWKB bytes (returns 2D geometry, discards extras)
//! - [`read_ewkb`] — parses preserving SRID, Z/M into [`EwkbGeometry`]
//! - [`read_wkb_from`] — reads WKB from any `io::Read` source
//! - [`read_wkb_concat`] — parses concatenated WKB sequence

use std::fmt;
use std::io::{self, Read, Write};

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

/// Errors that can occur during WKB parsing.
#[derive(Debug)]
pub enum WkbError {
    UnexpectedEof,
    InvalidByteOrder(u8),
    UnknownTypeCode(u32),
    UnexpectedGeometryType { expected: &'static str, code: u32 },
    UnsupportedDimension { actual_dims: u8 },
    TrailingBytes { consumed: usize, total: usize },
    IoError(std::io::Error),
}

impl fmt::Display for WkbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WkbError::UnexpectedEof => write!(f, "unexpected end of WKB data"),
            WkbError::InvalidByteOrder(b) => write!(f, "invalid WKB byte order: {b}"),
            WkbError::UnknownTypeCode(code) => {
                write!(f, "unknown WKB type code: {code}")
            }
            WkbError::UnexpectedGeometryType { expected, code } => {
                write!(
                    f,
                    "expected {expected} inside WKB container, got type code {code}"
                )
            }
            WkbError::UnsupportedDimension { actual_dims } => {
                write!(
                    f,
                    "unsupported WKB dimension: {actual_dims}D (only 2D is supported)"
                )
            }
            WkbError::TrailingBytes { consumed, total } => {
                write!(
                    f,
                    "trailing bytes after WKB geometry: consumed {consumed} of {total} bytes"
                )
            }
            WkbError::IoError(e) => write!(f, "WKB I/O error: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for WkbError {}

/// Dimension flags for EWKB extended dimensions (Z/M).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EwkbDims {
    pub has_z: bool,
    pub has_m: bool,
}

impl EwkbDims {
    pub const XY: EwkbDims = EwkbDims {
        has_z: false,
        has_m: false,
    };
    pub const XYZ: EwkbDims = EwkbDims {
        has_z: true,
        has_m: false,
    };
    pub const XYM: EwkbDims = EwkbDims {
        has_z: false,
        has_m: true,
    };
    pub const XYZM: EwkbDims = EwkbDims {
        has_z: true,
        has_m: true,
    };

    /// Number of coordinate values per point (2 for XY, 3 for XYZ/XYM, 4 for XYZM).
    pub fn coord_count(self) -> u8 {
        (self.has_z as u8) + (self.has_m as u8) + 2
    }
}

/// An EWKB geometry preserving SRID and Z/M dimension data.
///
/// Returned by [`read_ewkb`] when the input contains extended WKB flags.
/// The 2D geometry is always accessible via `.geometry`, while `.srid`
/// and `.extra_coords` preserve the extended data for roundtrip via
/// [`write_ewkb`].
///
/// For [`read_wkb`], this metadata is discarded and only the 2D geometry
/// is returned.
///
/// # Extra coordinates
///
/// `.extra_coords` stores Z and/or M values in depth-first coordinate
/// traversal order matching the `.geometry` tree. For `XYZ` or `XYM`,
/// one `f64` per coordinate. For `XYZM`, two `f64`s per coordinate
/// (Z first, then M). Extra data is only meaningful when paired with
/// the same geometry structure — use [`read_ewkb`] and [`write_ewkb`]
/// as a matched pair.
#[derive(Debug, Clone)]
pub struct EwkbGeometry {
    pub geometry: Geometry<f64>,
    pub srid: Option<i32>,
    pub dims: EwkbDims,
    pub extra_coords: Vec<f64>,
}

/// Byte order for WKB serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endianness {
    /// Little-endian (NDR) — default, native on x86_64/aarch64
    LittleEndian,
    /// Big-endian (XDR) — network byte order
    BigEndian,
}

/// Options for WKB writing, controlling byte order and format.
#[derive(Debug, Clone, Copy)]
pub struct WriteOptions {
    /// Desired byte order for output.
    pub endianness: Endianness,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            endianness: Endianness::LittleEndian,
        }
    }
}

// ---------------------------------------------------------------------------
// WKB type codes
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

/// Parse a single WKB geometry from `buf`. Returns the 2D geometry.
///
/// If the input contains EWKB Z/M dimension flags or an SRID, the extra
/// data is silently read and discarded — only 2D coordinates are returned.
/// Use [`read_ewkb`] to preserve SRID and Z/M values through a roundtrip.
pub fn read_wkb(buf: &[u8]) -> Result<Geometry<f64>, WkbError> {
    let mut pos = 0;
    let mut dummy_extra = Vec::new();
    let (geom, _, _, _) = read_geometry_inner(buf, &mut pos, &mut dummy_extra)?;
    // Strict single-geometry parse: trailing bytes mean the buffer was
    // truncated, concatenated, or garbage - surface it instead of silently
    // dropping data. Use read_wkb_concat for multi-geometry buffers.
    if pos != buf.len() {
        return Err(WkbError::TrailingBytes {
            consumed: pos,
            total: buf.len(),
        });
    }
    Ok(geom)
}

/// Read a WKB geometry from any `io::Read` source.
///
/// Reads all bytes from the reader, then delegates to [`read_wkb`].
pub fn read_wkb_from(mut reader: impl Read) -> Result<Geometry<f64>, WkbError> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(WkbError::IoError)?;
    read_wkb(&buf)
}

/// Parse a single EWKB geometry from `buf`, preserving SRID and Z/M data.
///
/// Returns an [`EwkbGeometry`] that holds both the 2D geometry and any
/// extended metadata (SRID, Z/M dimension flags, extra coordinate values).
/// Use [`write_ewkb`] to serialize back with full preservation.
///
/// For plain 2D WKB without EWKB flags, `.srid` is `None`, `.dims` is
/// [`EwkbDims::XY`], and `.extra_coords` is empty.
pub fn read_ewkb(buf: &[u8]) -> Result<EwkbGeometry, WkbError> {
    let mut pos = 0;
    let mut extra_coords = Vec::new();
    let (geometry, srid, raw_dims, _) = read_geometry_inner(buf, &mut pos, &mut extra_coords)?;
    let dims = match raw_dims {
        4 => EwkbDims::XYZM,
        3 => EwkbDims::XYZ,
        _ => EwkbDims::XY,
    };
    Ok(EwkbGeometry {
        geometry,
        srid,
        dims,
        extra_coords,
    })
}
/// Low-level: read one geometry from `buf` starting at `pos`, advancing
/// `pos` past the geometry. Optionally accumulates Z/M extra coordinate
/// values into `extra_coords`. Returns the geometry, SRID, dimension
/// count, and the raw WKB type code (for error reporting).
fn read_geometry_inner(
    buf: &[u8],
    pos: &mut usize,
    extra_coords: &mut Vec<f64>,
) -> Result<(Geometry<f64>, Option<i32>, u8, u32), WkbError> {
    let le = read_byte_order(buf, pos)?;
    let raw_type = read_u32(buf, pos, le)?;
    let has_z = (raw_type & WKB_Z_FLAG) != 0;
    let has_m = (raw_type & WKB_M_FLAG) != 0;
    let has_srid = (raw_type & WKB_SRID_FLAG) != 0;
    let type_code = raw_type & 0xff;

    let srid = if has_srid {
        Some(read_u32(buf, pos, le)? as i32)
    } else {
        None
    };

    let dims: u8 = if has_z && has_m {
        4
    } else if has_z || has_m {
        3
    } else {
        2
    };

    let geom = match type_code {
        WKB_POINT => read_point_inner(buf, pos, le, dims as u32, extra_coords).map(Geometry::Point),
        WKB_LINESTRING => {
            read_linestring_inner(buf, pos, le, dims as u32, extra_coords).map(Geometry::LineString)
        }
        WKB_POLYGON => {
            read_polygon_inner(buf, pos, le, dims as u32, extra_coords).map(Geometry::Polygon)
        }
        WKB_MULTIPOINT => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut points = Vec::with_capacity(n);
            for _ in 0..n {
                let (sub, _, _, sub_code) = read_geometry_inner(buf, pos, extra_coords)?;
                match sub {
                    Geometry::Point(p) => points.push(p),
                    _ => {
                        return Err(WkbError::UnexpectedGeometryType {
                            expected: "Point",
                            code: sub_code & 0xff,
                        });
                    }
                }
            }
            Ok(Geometry::MultiPoint(MultiPoint(points)))
        }
        WKB_MULTILINESTRING => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut lines = Vec::with_capacity(n);
            for _ in 0..n {
                let (sub, _, _, sub_code) = read_geometry_inner(buf, pos, extra_coords)?;
                match sub {
                    Geometry::LineString(ls) => lines.push(ls),
                    _ => {
                        return Err(WkbError::UnexpectedGeometryType {
                            expected: "LineString",
                            code: sub_code & 0xff,
                        });
                    }
                }
            }
            Ok(Geometry::MultiLineString(MultiLineString(lines)))
        }
        WKB_MULTIPOLYGON => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut polys = Vec::with_capacity(n);
            for _ in 0..n {
                let (sub, _, _, sub_code) = read_geometry_inner(buf, pos, extra_coords)?;
                match sub {
                    Geometry::Polygon(p) => polys.push(p),
                    _ => {
                        return Err(WkbError::UnexpectedGeometryType {
                            expected: "Polygon",
                            code: sub_code & 0xff,
                        });
                    }
                }
            }
            Ok(Geometry::MultiPolygon(MultiPolygon(polys)))
        }
        WKB_GEOMETRYCOLLECTION => {
            let n = read_u32(buf, pos, le)? as usize;
            let mut geoms = Vec::with_capacity(n);
            for _ in 0..n {
                let (sub, _, _, _) = read_geometry_inner(buf, pos, extra_coords)?;
                geoms.push(sub);
            }
            Ok(Geometry::GeometryCollection(GeometryCollection(geoms)))
        }
        _ => Err(WkbError::UnknownTypeCode(type_code)),
    };
    Ok((geom?, srid, dims, type_code))
}

/// Read byte order byte. Returns `true` for little-endian (NDR),
/// `false` for big-endian (XDR).
fn read_byte_order(buf: &[u8], pos: &mut usize) -> Result<bool, WkbError> {
    if *pos >= buf.len() {
        return Err(WkbError::UnexpectedEof);
    }
    let b = buf[*pos];
    *pos += 1;
    match b {
        0 => Ok(false), // XDR (big-endian)
        1 => Ok(true),  // NDR (little-endian)
        _ => Err(WkbError::InvalidByteOrder(b)),
    }
}

#[inline(always)]
fn read_u32(buf: &[u8], pos: &mut usize, le: bool) -> Result<u32, WkbError> {
    if *pos + 4 > buf.len() {
        return Err(WkbError::UnexpectedEof);
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
#[allow(dead_code)]
fn read_f64(buf: &[u8], pos: &mut usize, le: bool) -> Result<f64, WkbError> {
    if *pos + 8 > buf.len() {
        return Err(WkbError::UnexpectedEof);
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

// ---------------------------------------------------------------------------
// Batch coordinate reader: reads N coordinates with a single bounds check.
// ---------------------------------------------------------------------------

#[inline]
fn read_coords_batch(
    buf: &[u8],
    pos: &mut usize,
    n: usize,
    le: bool,
    dims: u32,
    extra_coords: &mut Vec<f64>,
) -> Result<Vec<Coord<f64>>, WkbError> {
    let stride = (dims as usize) * 8;
    let byte_size = n * stride;
    if *pos + byte_size > buf.len() {
        return Err(WkbError::UnexpectedEof);
    }
    let start = *pos;
    *pos += byte_size;

    // Fast path: native endian, 2D only — bulk-copy raw bytes into Vec
    if le == cfg!(target_endian = "little") && dims == 2 {
        let mut coords = Vec::<Coord<f64>>::with_capacity(n);
        unsafe {
            let dst = coords.spare_capacity_mut().as_mut_ptr() as *mut u8;
            std::ptr::copy_nonoverlapping(buf.as_ptr().add(start), dst, byte_size);
            coords.set_len(n);
        }
        return Ok(coords);
    }

    let mut coords = Vec::with_capacity(n);
    if le == cfg!(target_endian = "little") {
        for i in 0..n {
            let base = start + i * stride;
            let x = unsafe { (buf.as_ptr().add(base) as *const f64).read_unaligned() };
            let y = unsafe { (buf.as_ptr().add(base + 8) as *const f64).read_unaligned() };
            coords.push(Coord { x, y });
            if dims > 2 {
                let z = unsafe { (buf.as_ptr().add(base + 16) as *const f64).read_unaligned() };
                extra_coords.push(z);
                if dims > 3 {
                    let m = unsafe { (buf.as_ptr().add(base + 24) as *const f64).read_unaligned() };
                    extra_coords.push(m);
                }
            }
        }
    } else {
        for i in 0..n {
            let base = start + i * stride;
            let x = f64::from_bits(
                unsafe { (buf.as_ptr().add(base) as *const u64).read_unaligned() }.swap_bytes(),
            );
            let y = f64::from_bits(
                unsafe { (buf.as_ptr().add(base + 8) as *const u64).read_unaligned() }.swap_bytes(),
            );
            coords.push(Coord { x, y });
            if dims > 2 {
                let z = f64::from_bits(
                    unsafe { (buf.as_ptr().add(base + 16) as *const u64).read_unaligned() }
                        .swap_bytes(),
                );
                extra_coords.push(z);
                if dims > 3 {
                    let m = f64::from_bits(
                        unsafe { (buf.as_ptr().add(base + 24) as *const u64).read_unaligned() }
                            .swap_bytes(),
                    );
                    extra_coords.push(m);
                }
            }
        }
    }
    Ok(coords)
}

fn read_point_inner(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
    extra_coords: &mut Vec<f64>,
) -> Result<Point<f64>, WkbError> {
    let mut batch = read_coords_batch(buf, pos, 1, le, dims, extra_coords)?;
    Ok(Point(batch.swap_remove(0)))
}

fn read_linestring_inner(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
    extra_coords: &mut Vec<f64>,
) -> Result<LineString<f64>, WkbError> {
    let n = read_u32(buf, pos, le)? as usize;
    let coords = read_coords_batch(buf, pos, n, le, dims, extra_coords)?;
    Ok(LineString::new(coords))
}

fn read_polygon_inner(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    dims: u32,
    extra_coords: &mut Vec<f64>,
) -> Result<Polygon<f64>, WkbError> {
    let n_rings = read_u32(buf, pos, le)? as usize;
    if n_rings == 0 {
        return Ok(Polygon::new(LineString::new(vec![]), vec![]));
    }
    let exterior = read_linestring_inner(buf, pos, le, dims, extra_coords)?;
    let mut holes = Vec::with_capacity(n_rings.saturating_sub(1));
    for _ in 1..n_rings {
        holes.push(read_linestring_inner(buf, pos, le, dims, extra_coords)?);
    }
    Ok(Polygon::new(exterior, holes))
}

// ---------------------------------------------------------------------------
// Writer (little-endian)
// ---------------------------------------------------------------------------

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
    let _geom = read_geometry_inner(buf, &mut pos, &mut dummy_extra)?;
    Ok(pos)
}

/// Parse a concatenated sequence of WKB geometries from a single buffer.
///
/// Reads back-to-back WKB geometries from the same byte buffer,
/// stopping at the end of the buffer. Supports mixed byte orders
/// and geometry types.
///
/// Useful for batch formats where multiple geometries are written
/// sequentially via [`write_wkb`]. For a single geometry, use
/// [`read_wkb`].
///
/// # Errors
///
/// Returns [`WkbError`] if any geometry has invalid
/// WKB structure. Parsing stops at the first error.
///
/// # Examples
///
/// ```rust
/// use geo_repair::{read_wkb_concat, write_wkb};
///
/// let geom1 = geo::Geometry::Point(geo::point!(x: 1.0, y: 2.0));
/// let geom2 = geo::Geometry::Point(geo::point!(x: 3.0, y: 4.0));
///
/// let mut buf = Vec::new();
/// buf.extend_from_slice(&write_wkb(&geom1));
/// buf.extend_from_slice(&write_wkb(&geom2));
///
/// let geoms = read_wkb_concat(&buf).unwrap();
/// assert_eq!(geoms.len(), 2);
/// ```
pub fn read_wkb_concat(buf: &[u8]) -> Result<Vec<Geometry<f64>>, WkbError> {
    let mut offset = 0;
    let mut geoms = Vec::new();
    while offset < buf.len() {
        let mut extra = Vec::new();
        let (geom, _, _, _) = read_geometry_inner(buf, &mut offset, &mut extra)?;
        geoms.push(geom);
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
    fn ewkb_z_flag_now_returns_2d() {
        // read_wkb no longer rejects Z dimension — silently returns 2D
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_z = 1u32 | WKB_Z_FLAG;
        wkb.extend_from_slice(&type_with_z.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());

        let geom = read_wkb(&wkb).unwrap();
        assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
    }

    #[test]
    fn ewkb_zm_flag_now_returns_2d() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_zm = 1u32 | WKB_Z_FLAG | WKB_M_FLAG;
        wkb.extend_from_slice(&type_with_zm.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());
        wkb.extend_from_slice(&4.0f64.to_le_bytes());

        let geom = read_wkb(&wkb).unwrap();
        assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
    }

    #[test]
    fn ewkb_z_in_multi_sub_geom_now_returns_2d() {
        let mut wkb = Vec::new();
        wkb.push(1);
        wkb.extend_from_slice(&4u32.to_le_bytes()); // MultiPoint
        wkb.extend_from_slice(&2u32.to_le_bytes()); // 2 points

        wkb.push(1);
        wkb.extend_from_slice(&1u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());

        wkb.push(1);
        let type_with_z = 1u32 | WKB_Z_FLAG;
        wkb.extend_from_slice(&type_with_z.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());
        wkb.extend_from_slice(&4.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());

        let geom = read_wkb(&wkb).unwrap();
        if let Geometry::MultiPoint(mp) = geom {
            assert_eq!(mp.0.len(), 2);
            assert_eq!(mp.0[0], Point::new(1.0, 2.0));
            assert_eq!(mp.0[1], Point::new(3.0, 4.0));
        } else {
            panic!("expected MultiPoint");
        }
    }

    // -----------------------------------------------------------------------
    // EWKB roundtrip tests
    // -----------------------------------------------------------------------

    fn make_test_geom() -> Geometry<f64> {
        Geometry::Point(Point::new(1.0, 2.0))
    }

    fn make_test_linestring() -> Geometry<f64> {
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ]))
    }

    fn make_test_polygon() -> Geometry<f64> {
        Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        ))
    }

    fn coord_count(geom: &Geometry<f64>) -> usize {
        use geo::Geometry::*;
        match geom {
            Point(_) => 1,
            LineString(ls) => ls.0.len(),
            Polygon(poly) => {
                let mut n = poly.exterior().0.len();
                for h in poly.interiors() {
                    n += h.0.len();
                }
                n
            }
            MultiPoint(mp) => mp.0.len(),
            MultiLineString(mls) => mls.0.iter().map(|ls| ls.0.len()).sum(),
            MultiPolygon(mp) => {
                mp.0.iter()
                    .map(|p| {
                        let mut n = p.exterior().0.len();
                        for h in p.interiors() {
                            n += h.0.len();
                        }
                        n
                    })
                    .sum()
            }
            GeometryCollection(gc) => gc.0.iter().map(coord_count).sum(),
            _ => 0,
        }
    }

    #[test]
    fn ewkb_read_2d_no_extra() {
        // Plain 2D WKB through read_ewkb yields empty extra_coords
        let bytes = write_wkb(&make_test_geom());
        let ewkb = read_ewkb(&bytes).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, None);
        assert_eq!(ewkb.dims, EwkbDims::XY);
        assert!(ewkb.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_read_with_srid() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_srid = 1u32 | WKB_SRID_FLAG;
        wkb.extend_from_slice(&type_with_srid.to_le_bytes());
        wkb.extend_from_slice(&4326u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, Some(4326));
        assert_eq!(ewkb.dims, EwkbDims::XY);
        assert!(ewkb.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_read_with_z() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_z = 1u32 | WKB_Z_FLAG;
        wkb.extend_from_slice(&type_with_z.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, None);
        assert_eq!(ewkb.dims, EwkbDims::XYZ);
        assert_eq!(ewkb.extra_coords, vec![3.0]);
    }

    #[test]
    fn ewkb_read_with_srid_and_z() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_srid_z = 1u32 | WKB_SRID_FLAG | WKB_Z_FLAG;
        wkb.extend_from_slice(&type_with_srid_z.to_le_bytes());
        wkb.extend_from_slice(&4326u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, Some(4326));
        assert_eq!(ewkb.dims, EwkbDims::XYZ);
        assert_eq!(ewkb.extra_coords, vec![3.0]);
    }

    #[test]
    fn ewkb_read_with_zm() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_zm = 1u32 | WKB_Z_FLAG | WKB_M_FLAG;
        wkb.extend_from_slice(&type_with_zm.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());
        wkb.extend_from_slice(&4.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, None);
        assert_eq!(ewkb.dims, EwkbDims::XYZM);
        assert_eq!(ewkb.extra_coords, vec![3.0, 4.0]);
    }

    #[test]
    fn ewkb_write_read_roundtrip_xy() {
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: None,
            dims: EwkbDims::XY,
            extra_coords: vec![],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, ewkb.geometry);
        assert_eq!(back.srid, None);
        assert_eq!(back.dims, EwkbDims::XY);
        assert!(back.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_write_read_roundtrip_xyz() {
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: vec![99.0],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, ewkb.geometry);
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, vec![99.0]);
    }

    #[test]
    fn ewkb_write_read_roundtrip_xyzm() {
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: Some(4326),
            dims: EwkbDims::XYZM,
            extra_coords: vec![10.0, 20.0],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, ewkb.geometry);
        assert_eq!(back.srid, Some(4326));
        assert_eq!(back.dims, EwkbDims::XYZM);
        assert_eq!(back.extra_coords, vec![10.0, 20.0]);
    }

    #[test]
    fn ewkb_roundtrip_linestring_xyz() {
        let geom = make_test_linestring();
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 1.5).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_polygon_xyz() {
        let geom = make_test_polygon();
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 0.5).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_multipoint_srid() {
        let geom =
            Geometry::MultiPoint(MultiPoint(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]));
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 10.0).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: Some(2154),
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.srid, Some(2154));
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_multipolygon_xyz() {
        let geom = Geometry::MultiPolygon(MultiPolygon(vec![
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
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 0.25).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: Some(4326),
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.srid, Some(4326));
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_gc_xy() {
        let geom = Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ]));
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XY,
            extra_coords: vec![],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.srid, None);
        assert_eq!(back.dims, EwkbDims::XY);
        assert!(back.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_write_then_read_wkb_2d() {
        // write_ewkb → read_wkb returns 2D geo with no metadata
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: Some(4326),
            dims: EwkbDims::XYZ,
            extra_coords: vec![42.0],
        };
        let bytes = write_ewkb(&ewkb);
        let geom = read_wkb(&bytes).unwrap();
        assert_eq!(geom, make_test_geom());
    }

    #[test]
    fn ewkb_roundtrip_polygon_with_hole_xyz() {
        let geom = Geometry::Polygon(Polygon::new(
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
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 0.1).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.extra_coords, extra);
    }

    // -----------------------------------------------------------------------
    // WriteOptions / BE writing tests
    // -----------------------------------------------------------------------

    #[test]
    fn write_wkb_with_opts_be_roundtrip_point() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        // Verify byte order marker is 0 (BE)
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_linestring() {
        let g = Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_polygon() {
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
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_multipolygon() {
        let g = Geometry::MultiPolygon(MultiPolygon(vec![Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        )]));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_gc() {
        let g = Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ]));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_default_is_le() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let wkb = write_wkb(&g);
        assert_eq!(wkb[0], 1); // LE
        let with_opts = write_wkb_with_opts(&g, &WriteOptions::default());
        assert_eq!(wkb, with_opts);
    }

    #[test]
    fn write_wkb_with_opts_mixed_endian_explicit() {
        // Ensure LE and BE produce different byte sequences
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let le_opts = WriteOptions {
            endianness: Endianness::LittleEndian,
        };
        let be_opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let le_wkb = write_wkb_with_opts(&g, &le_opts);
        let be_wkb = write_wkb_with_opts(&g, &be_opts);
        assert_ne!(le_wkb, be_wkb, "LE and BE output should differ");
    }

    // -----------------------------------------------------------------------
    // read_wkb_from / write_wkb_to tests
    // -----------------------------------------------------------------------

    #[test]
    fn read_wkb_from_reader() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let wkb = write_wkb(&g);
        let reader = &wkb[..];
        let back = read_wkb_from(reader).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn read_wkb_from_empty_fails() {
        let err = read_wkb_from(&b""[..]).unwrap_err();
        assert!(matches!(err, WkbError::UnexpectedEof));
    }

    #[test]
    fn read_wkb_from_invalid_fails() {
        let err = read_wkb_from(&b"\xff\xff\xff\xff"[..]).unwrap_err();
        assert!(matches!(err, WkbError::InvalidByteOrder(255)));
    }

    #[test]
    fn write_wkb_to_writer() {
        let g = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkb_to(&g, &mut buf).unwrap();
        let back = read_wkb(&buf).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_to_write_then_read() {
        let g = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkb_to(&g, &mut buf).unwrap();
        let back = read_wkb_from(&buf[..]).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_to_writer_be_via_opts() {
        let g = Geometry::Point(Point::new(1.5, 2.5));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let buf = write_wkb_with_opts(&g, &opts);
        let back = read_wkb_from(&buf[..]).unwrap();
        assert_eq!(g, back);
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

    // -----------------------------------------------------------------------
    // Production-readiness battery: truncation, garbage, trailing bytes,
    // non-OGC variants, empty geometries, precise error codes.
    // -----------------------------------------------------------------------

    /// Every strict prefix of a valid WKB buffer must fail cleanly (no
    /// panics, no silent truncation acceptance).
    #[test]
    fn truncated_wkb_never_panics() {
        let poly = match make_test_polygon() {
            Geometry::Polygon(p) => p,
            other => panic!("expected polygon, got {other:?}"),
        };
        let geoms = vec![
            make_test_geom(),
            make_test_linestring(),
            make_test_polygon(),
            Geometry::MultiPolygon(MultiPolygon(vec![poly.clone(), poly])),
            Geometry::GeometryCollection(GeometryCollection(vec![
                make_test_geom(),
                make_test_linestring(),
            ])),
        ];
        for g in &geoms {
            let wkb = write_wkb(g);
            for cut in 0..wkb.len() {
                let err = read_wkb(&wkb[..cut]).unwrap_err();
                assert!(
                    matches!(&err, WkbError::UnexpectedEof | WkbError::UnknownTypeCode(_)),
                    "prefix {cut} of {} bytes: unexpected success or wrong error {err:?}",
                    wkb.len()
                );
            }
        }
    }

    #[test]
    fn garbage_buffers_rejected() {
        let garbage: Vec<Vec<u8>> = vec![
            vec![],
            vec![0xDE, 0xAD, 0xBE, 0xEF],
            vec![0x01, 0x00, 0x00, 0x00, 0xFF],
            vec![0x02, 0x00, 0x00, 0x00, 0x01], // invalid byte order
            vec![0x01, 0x00, 0x00, 0x00, 0x63], // unknown type code 99
            vec![0x01; 64],
            vec![0x00; 32],
        ];
        for buf in garbage {
            let _ = read_wkb(&buf); // must not panic
            assert!(read_wkb(&buf).is_err(), "garbage accepted: {buf:?}");
        }
    }

    #[test]
    fn trailing_bytes_rejected() {
        let g = make_test_polygon();
        let wkb = write_wkb(&g);

        // One trailing byte.
        let mut buf = wkb.clone();
        buf.push(0x00);
        let err = read_wkb(&buf).unwrap_err();
        assert!(
            matches!(&err, WkbError::TrailingBytes { consumed, total } if *consumed == wkb.len() && *total == wkb.len() + 1),
            "expected TrailingBytes, got {err:?}"
        );

        // A second complete geometry behind the first.
        let mut buf = wkb.clone();
        buf.extend_from_slice(&write_wkb(&make_test_geom()));
        let err = read_wkb(&buf).unwrap_err();
        assert!(
            matches!(err, WkbError::TrailingBytes { .. }),
            "expected TrailingBytes for concatenation, got {err:?}"
        );
        // ...but the concat API reads both.
        assert_eq!(read_wkb_concat(&buf).unwrap().len(), 2);
    }

    #[test]
    fn non_ogc_variants_roundtrip_wkb() {
        // Line/Rect/Triangle serialize to their OGC equivalents
        // (coordinate-exact), never silently to an empty collection.
        let line = Geometry::Line(geo::Line::new(
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 3.0, y: 4.0 },
        ));
        let back = read_wkb(&write_wkb(&line)).unwrap();
        assert_eq!(
            back,
            Geometry::LineString(LineString::new(vec![
                Coord { x: 1.0, y: 2.0 },
                Coord { x: 3.0, y: 4.0 },
            ]))
        );

        let rect = Geometry::Rect(geo::Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 5.0 },
        ));
        let back = read_wkb(&write_wkb(&rect)).unwrap();
        if let Geometry::Polygon(p) = back {
            assert_eq!(p.exterior().0.len(), 5);
            assert_eq!(p.exterior().0[0], Coord { x: 0.0, y: 0.0 });
            assert_eq!(p.exterior().0[2], Coord { x: 10.0, y: 5.0 });
        } else {
            panic!("expected polygon for rect WKB");
        }

        let tri = Geometry::Triangle(geo::Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
        ));
        let back = read_wkb(&write_wkb(&tri)).unwrap();
        if let Geometry::Polygon(p) = back {
            assert_eq!(p.exterior().0.len(), 4);
            assert_eq!(p.exterior().0[3], Coord { x: 0.0, y: 0.0 });
        } else {
            panic!("expected polygon for triangle WKB");
        }

        // Big-endian variants too.
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
            ..Default::default()
        };
        let back = read_wkb(&write_wkb_with_opts(&rect, &opts)).unwrap();
        assert!(matches!(back, Geometry::Polygon(_)));
    }

    #[test]
    fn empty_geometries_roundtrip_wkb() {
        let geoms = vec![
            Geometry::Point(Point::new(f64::NAN, f64::NAN)),
            Geometry::LineString(LineString::new(vec![])),
            Geometry::Polygon(Polygon::new(LineString::new(vec![]), vec![])),
            Geometry::MultiPoint(MultiPoint(vec![])),
            Geometry::MultiLineString(MultiLineString(vec![])),
            Geometry::MultiPolygon(MultiPolygon(vec![])),
            Geometry::GeometryCollection(GeometryCollection(vec![])),
        ];
        for g in &geoms {
            let back = read_wkb(&write_wkb(g)).unwrap();
            match (g, &back) {
                (Geometry::Point(a), Geometry::Point(b)) => {
                    assert!(a.0.x.is_nan() && b.0.x.is_nan(), "empty point round trip");
                }
                _ => assert_eq!(g, &back, "empty geometry round trip"),
            }
        }
    }

    #[test]
    fn wrong_subtype_reports_real_code() {
        // MULTIPOINT containing a LINESTRING sub-geometry: the error must
        // carry the actual sub type code (2 = LINESTRING), not 0.
        let mut buf = Vec::new();
        buf.push(1u8); // NDR
        buf.extend_from_slice(&WKB_MULTIPOINT.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // one element
        buf.extend_from_slice(&write_wkb(&make_test_linestring()));
        let err = read_wkb(&buf).unwrap_err();
        assert!(
            matches!(
                err,
                WkbError::UnexpectedGeometryType {
                    expected: "Point",
                    code: WKB_LINESTRING
                }
            ),
            "expected real sub-type code, got {err:?}"
        );
    }

    #[test]
    fn invalid_byte_order_and_unknown_code() {
        // Byte order byte must be 0 or 1.
        let err = read_wkb(&[0x02, 0x00, 0x00, 0x00, 0x01]).unwrap_err();
        assert!(matches!(err, WkbError::InvalidByteOrder(2)));
        // Unknown type code 99 (0x63).
        let err = read_wkb(&[0x01, 0x63, 0x00, 0x00, 0x00]).unwrap_err();
        assert!(matches!(err, WkbError::UnknownTypeCode(99)));
    }
}

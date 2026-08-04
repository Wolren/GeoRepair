//! WKB reader: single-pass parsing with EWKB flag/SRID/Z-M handling.

use super::*;
use std::io::Read;

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
pub(crate) fn read_geometry_inner(
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
            // Each sub-geometry needs at least a 5-byte header (byte order
            // + type code); the recursion enforces the rest.
            let n = read_bounded_count(buf, pos, le, 5)?;
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
            let n = read_bounded_count(buf, pos, le, 5)?;
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
            let n = read_bounded_count(buf, pos, le, 5)?;
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
            let n = read_bounded_count(buf, pos, le, 5)?;
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

/// Read a count field bounded by the remaining buffer: `n` elements each
/// need at least `min_bytes` to be parseable, so `n > remaining/min_bytes`
/// means the document is truncated relative to its own count fields -
/// same class as a read hitting the buffer end, so it surfaces as
/// [`WkbError::UnexpectedEof`]. This is what keeps a 4-byte count field
/// from driving `Vec::with_capacity` into an OOM abort (measured
/// 2026-08-04: crafted MultiPoint count -> 120 GB allocation). No new
/// public error variant: WkbError is a published enum and adding one
/// would be a semver-breaking change at patch level.
#[inline]
fn read_bounded_count(
    buf: &[u8],
    pos: &mut usize,
    le: bool,
    min_bytes: usize,
) -> Result<usize, WkbError> {
    let n = read_u32(buf, pos, le)? as usize;
    let remaining = buf.len().saturating_sub(*pos);
    if n > remaining / min_bytes.max(1) {
        return Err(WkbError::UnexpectedEof);
    }
    Ok(n)
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
    // Each ring needs at least its 4-byte count field; an EMPTY ring
    // (count 0) is legal and consumes nothing more. The per-ring coord
    // reads enforce the actual coordinate bytes.
    let n_rings = read_bounded_count(buf, pos, le, 4)?;
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


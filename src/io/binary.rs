//! Custom binary bulk polygon format loader and streaming iterator.
//!
//! Provides [`load_bin`] for loading all polygons at once and
//! [`load_bin_stream`] for memory-efficient streaming.
//!
//! # Format
//!
//! Little-endian binary format for polygon batches:
//! - u32 LE: polygon count
//! - Per polygon:
//!   - u32 LE: exterior ring vertex count
//!   - f64 LE × N: (x, y) pairs
//!   - u32 LE: interior ring count
//!   - Per interior ring:
//!     - u32 LE: vertex count
//!     - f64 LE × N: (x, y) pairs
//!
//! This format is designed for bulk transfer from GDAL/QGIS and is not
//! an OGC standard. For portable interchange, use WKB.
use std::fs::File;
use std::io::Read;
use std::io::Write;

use geo::{Coord, LineString, Polygon};

/// Load polygons from a custom binary format.
///
/// Format:
/// - u32 LE: number of polygons
/// - For each polygon:
///   - u32 LE: number of exterior ring coords
///   - f64 LE × N: exterior ring coords (x,y pairs)
///   - u32 LE: number of interior rings
///   - For each interior ring:
///     - u32 LE: number of coords
///     - f64 LE × N: coord pairs
pub fn load_bin(path: &str) -> Result<Vec<Polygon<f64>>, String> {
    let mut buf = Vec::new();
    File::open(path)
        .map_err(|e| format!("cannot open {path}: {e}"))?
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read {path}: {e}"))?;
    let mut pos = 0;

    let read_u32 = |buf: &[u8], pos: &mut usize| -> Result<u32, String> {
        if *pos + 4 > buf.len() {
            return Err("unexpected EOF reading u32".into());
        }
        let v = u32::from_le_bytes(
            buf[*pos..*pos + 4]
                .try_into()
                .map_err(|_| "invalid u32 bytes")?,
        );
        *pos += 4;
        Ok(v)
    };
    let read_f64 = |buf: &[u8], pos: &mut usize| -> Result<f64, String> {
        if *pos + 8 > buf.len() {
            return Err("unexpected EOF reading f64".into());
        }
        let v = f64::from_le_bytes(
            buf[*pos..*pos + 8]
                .try_into()
                .map_err(|_| "invalid f64 bytes")?,
        );
        *pos += 8;
        Ok(v)
    };
    let read_ring = |buf: &[u8], pos: &mut usize| -> Result<LineString<f64>, String> {
        let n = read_u32(buf, pos)? as usize;
        let mut coords = Vec::with_capacity(n);
        for _ in 0..n {
            coords.push(Coord {
                x: read_f64(buf, pos)?,
                y: read_f64(buf, pos)?,
            });
        }
        Ok(LineString::new(coords))
    };

    let n_polys = read_u32(&buf, &mut pos)? as usize;
    let mut polys = Vec::with_capacity(n_polys);
    for _ in 0..n_polys {
        let ext = read_ring(&buf, &mut pos)?;
        let n_holes = read_u32(&buf, &mut pos)? as usize;
        let mut holes = Vec::with_capacity(n_holes);
        for _ in 0..n_holes {
            holes.push(read_ring(&buf, &mut pos)?);
        }
        polys.push(Polygon::new(ext, holes));
    }
    Ok(polys)
}

/// Stream polygons from a binary file without loading everything into memory.
///
/// Returns an iterator that reads polygons one at a time from a custom
/// binary format file. Useful for processing datasets too large to fit
/// in memory.
///
/// # Format
///
/// Same format as [`load_bin`]:
/// - u32 LE: number of polygons
/// - For each polygon:
///   - u32 LE: exterior ring coord count
///   - f64 LE × N: (x, y) pairs
///   - u32 LE: interior ring count
///   - For each interior ring:
///     - u32 LE: coord count
///     - f64 LE × N: (x, y) pairs
///
/// # Panics
///
/// Panics on I/O errors (file not found, read failure).
/// Use [`load_bin`] for fallible access.
///
/// # Example
///
/// ```rust,ignore
/// use geo_repair::load_bin_stream;
///
/// for poly in load_bin_stream("dataset.bin") {
///     process(poly);
/// }
/// ```
pub fn load_bin_stream(path: &str) -> impl Iterator<Item = Polygon<f64>> {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("Cannot open {path}: {e}"));
    let reader = std::io::BufReader::new(file);
    LoadBinStream {
        reader,
        header_read: false,
        total_polys: 0,
        polys_yielded: 0,
    }
}

struct LoadBinStream {
    reader: std::io::BufReader<std::fs::File>,
    header_read: bool,
    total_polys: u32,
    polys_yielded: u32,
}

fn read_u32<R: std::io::Read>(r: &mut R) -> Option<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).ok()?;
    Some(u32::from_le_bytes(buf))
}

#[allow(dead_code)]
fn read_f64<R: std::io::Read>(r: &mut R) -> Option<f64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf).ok()?;
    Some(f64::from_le_bytes(buf))
}

fn read_ring<R: std::io::Read>(r: &mut R) -> Option<LineString<f64>> {
    let n = read_u32(r)? as usize;
    let mut coords = Vec::with_capacity(n);
    for _ in 0..n {
        coords.push(Coord {
            x: read_f64(r)?,
            y: read_f64(r)?,
        });
    }
    Some(LineString::new(coords))
}

impl Iterator for LoadBinStream {
    type Item = Polygon<f64>;

    fn next(&mut self) -> Option<Polygon<f64>> {
        if !self.header_read {
            self.total_polys = read_u32(&mut self.reader)?;
            self.header_read = true;
            if self.total_polys == 0 {
                return None;
            }
        }
        if self.polys_yielded >= self.total_polys {
            return None;
        }
        self.polys_yielded += 1;
        let ext = read_ring(&mut self.reader)?;
        let n_holes = read_u32(&mut self.reader)? as usize;
        let mut holes = Vec::with_capacity(n_holes);
        for _ in 0..n_holes {
            holes.push(read_ring(&mut self.reader)?);
        }
        Some(Polygon::new(ext, holes))
    }
}

/// Write polygons to the custom binary format.
///
/// Same format as [`load_bin`]:
/// - u32 LE: number of polygons
/// - For each polygon:
///   - u32 LE: exterior ring coord count
///   - f64 LE × N: (x, y) pairs
///   - u32 LE: interior ring count
///   - For each interior ring:
///     - u32 LE: coord count
///     - f64 LE × N: (x, y) pairs
///
/// # Example
///
/// ```rust,ignore
/// use geo_repair::{load_bin, write_bin};
///
/// let polys = load_bin("input.bin").unwrap();
/// write_bin("output.bin", &polys).unwrap();
/// ```
pub fn write_bin(path: &str, polys: &[Polygon<f64>]) -> Result<(), String> {
    let mut buf = Vec::with_capacity(polys.len() * 128);
    buf.extend_from_slice(&(polys.len() as u32).to_le_bytes());
    for poly in polys {
        write_ring(&mut buf, poly.exterior());
        buf.extend_from_slice(&(poly.interiors().len() as u32).to_le_bytes());
        for hole in poly.interiors() {
            write_ring(&mut buf, hole);
        }
    }
    let mut file = File::create(path).map_err(|e| format!("cannot create {path}: {e}"))?;
    file.write_all(&buf)
        .map_err(|e| format!("cannot write {path}: {e}"))?;
    Ok(())
}

fn write_ring(buf: &mut Vec<u8>, ring: &LineString<f64>) {
    let coords = &ring.0;
    buf.extend_from_slice(&(coords.len() as u32).to_le_bytes());
    for c in coords {
        buf.extend_from_slice(&c.x.to_le_bytes());
        buf.extend_from_slice(&c.y.to_le_bytes());
    }
}

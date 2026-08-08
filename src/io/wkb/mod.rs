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

use alloc::vec::Vec;
use core::fmt;

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
    UnexpectedGeometryType {
        expected: &'static str,
        code: u32,
    },
    UnsupportedDimension {
        actual_dims: u8,
    },
    TrailingBytes {
        consumed: usize,
        total: usize,
    },
    #[cfg(feature = "std")]
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
            #[cfg(feature = "std")]
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

mod read;
#[cfg(test)]
mod tests;
mod write;

#[cfg(feature = "std")]
#[cfg(feature = "std")]
pub use read::read_wkb_from;
pub use read::{read_ewkb, read_wkb, read_wkb_concat};
#[cfg(feature = "std")]
pub use write::write_wkb_to;
pub use write::{estimate_wkb_size, write_ewkb, write_wkb, write_wkb_with_opts};

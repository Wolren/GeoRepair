
use alloc::vec::Vec;
use core::fmt;

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

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

    pub fn coord_count(self) -> u8 {
        (self.has_z as u8) + (self.has_m as u8) + 2
    }
}

#[derive(Debug, Clone)]
pub struct EwkbGeometry {
    pub geometry: Geometry<f64>,
    pub srid: Option<i32>,
    pub dims: EwkbDims,
    pub extra_coords: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endianness {
    LittleEndian,
    BigEndian,
}

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

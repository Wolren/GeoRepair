//! OGC Well-Known Text (WKT) parsing and serialization.
//!
//! Parses and serializes WKT directly into `geo::Geometry<f64>` without intermediate
//! representations.
//!
//! # Supported types
//!
//! Point, LineString, linearing (as LineString), Polygon, MultiPoint, MultiLineString,
//! MultiPolygon, GeometryCollection.
//!
//! empty geometries and empty elements inside Multi* / GeometryCollection are accepted
//! (empty point components use geo's NaN-coordinate convention). NaN and inf ordinates
//! are accepted (invalid geometry per OGC, but parseable - the corpus carries them).
//!
//! # Dimensions
//!
//! Z, M and ZM modifiers are read on every type. [`read_wkt`] returns x and y only,
//! because `geo::Coord` holds two values. [`read_wkt_zm`] returns the extra ordinates in
//! flat coordinate order, the same layout as
//! [`EwkbGeometry`](crate::io::wkb::EwkbGeometry), and [`write_wkt_zm`] writes the
//! modifier back.
//!
//! One geometry carries one modifier. [`read_wkt`] reads a collection whose members
//! disagree; [`read_wkt_zm`] rejects it with [`WktError::MixedDimensions`], as GEOS does.
//!
//! # Reading
//!
//! - Read WKT from any `io::Read` source
//!
//! # Writing
//!
//! - Write WKT to any `io::Write` target
//!
//! # Example
//!
//! ```rust
//! use geo_repair::{read_wkt, write_wkt};
//!
//! let wkt = "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))";
//! let geom = read_wkt(wkt).unwrap();
//! let roundtrip = write_wkt(&geom);
//! let parsed_again = read_wkt(&roundtrip).unwrap();
//! assert_eq!(geom, parsed_again);
//! ```
//!
//! ```rust
//! use geo_repair::{read_wkt_zm, write_wkt_zm};
//!
//! let zm = read_wkt_zm("POINT Z (1 2 3)").unwrap();
//! assert_eq!(zm.extra_coords, vec![3.0]);
//! assert_eq!(write_wkt_zm(&zm), "POINT Z (1.0 2.0 3.0)");
//! ```

use crate::io::dims::Dims;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

/// WKT geometry with its dimension modifier and extra ordinates.
///
/// `extra_coords` holds `dims.coord_count() - 2` values per coordinate in flat coordinate
/// order: z then m, exterior before holes, members in document order.
#[derive(Debug, Clone)]
pub struct WktGeometry {
    pub geometry: Geometry<f64>,
    pub dims: Dims,
    pub extra_coords: Vec<f64>,
}

/// Errors that can occur during WKT parsing.
#[derive(Debug)]
pub enum WktError {
    ParseError {
        pos: usize,
        message: String,
    },
    InvalidNumber {
        pos: usize,
        value: String,
    },
    UnknownGeometryType {
        pos: usize,
        type_name: String,
    },
    TrailingCharacters {
        pos: usize,
    },
    EmptyInput,
    MixedDimensions {
        pos: usize,
        outer: &'static str,
        found: &'static str,
    },
    #[cfg(feature = "std")]
    IoError(std::io::Error),
}

impl fmt::Display for WktError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WktError::ParseError { pos, message } => {
                write!(f, "WKT parse error at position {pos}: {message}")
            }
            WktError::InvalidNumber { pos, value } => {
                write!(f, "invalid number at position {pos}: '{value}'")
            }
            WktError::UnknownGeometryType { pos, type_name } => {
                write!(f, "unknown geometry type at position {pos}: '{type_name}'")
            }
            WktError::TrailingCharacters { pos } => {
                write!(f, "trailing characters after geometry at position {pos}")
            }
            WktError::EmptyInput => write!(f, "empty WKT input"),
            WktError::MixedDimensions { pos, outer, found } => {
                write!(
                    f,
                    "mixed dimension modifiers at position {pos}: '{found}' in a '{outer}' geometry"
                )
            }
            #[cfg(feature = "std")]
            WktError::IoError(e) => write!(f, "WKT I/O error: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for WktError {}

/// WKT modifier text for a dimension, `None` for XY.
pub(crate) fn modifier(dims: Dims) -> Option<&'static str> {
    match (dims.has_z, dims.has_m) {
        (true, true) => Some("ZM"),
        (true, false) => Some("Z"),
        (false, true) => Some("M"),
        (false, false) => None,
    }
}

mod read;
#[cfg(test)]
mod tests;
mod write;

pub use read::{infer_wkt_type, read_wkt, read_wkt_zm};
#[cfg(feature = "std")]
pub use read::{read_wkt_from, read_wkt_zm_from};
pub use write::{write_wkt, write_wkt_zm};
#[cfg(feature = "std")]
pub use write::{write_wkt_to, write_wkt_zm_to};

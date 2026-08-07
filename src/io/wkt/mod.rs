//! OGC Well-Known Text (WKT) parsing and serialization (2D only).
//!
//! Zero-dependency backend — parses and serializes WKT directly into
//! `geo::Geometry<f64>` without intermediate representations.
//!
//! # Supported types
//!
//! Point, LineString, LINEARING (as LineString), Polygon, MultiPoint,
//! MultiLineString, MultiPolygon, GeometryCollection.
//!
//! EMPTY geometries and EMPTY elements inside Multi* / GeometryCollection
//! are accepted (empty point components use geo's NaN-coordinate
//! convention). NaN and inf ordinates are accepted (invalid geometry per
//! OGC, but parseable - the corpus carries them).
//!
//! Z, M, and ZM dimension modifiers are recognized and rejected with
//! [`WktError::UnsupportedDimension`](crate::WktError).
//!
//! # Reading
//!
//! - [`read_wkt`] — parse a WKT string
//! - [`read_wkt_from`] — read WKT from any `io::Read` source
//! - [`infer_wkt_type`] — peek at the type keyword without full parsing
//!
//! # Writing
//!
//! - [`write_wkt`] — serialize to WKT string
//! - [`write_wkt_to`] — write WKT to any `io::Write` target
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


use alloc::string::String;
use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use core::fmt;

/// Errors that can occur during WKT parsing.
#[derive(Debug)]
pub enum WktError {
    ParseError { pos: usize, message: String },
    InvalidNumber { pos: usize, value: String },
    UnknownGeometryType { pos: usize, type_name: String },
    TrailingCharacters { pos: usize },
    EmptyInput,
    UnsupportedDimension { pos: usize, modifier: String },
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
            WktError::UnsupportedDimension { pos, modifier } => {
                write!(
                    f,
                    "unsupported dimension modifier '{modifier}' at position {pos} (only 2D is supported)"
                )
            }
            #[cfg(feature = "std")]
            WktError::IoError(e) => write!(f, "WKT I/O error: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for WktError {}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

mod read;
mod write;
#[cfg(test)]
mod tests;

pub use read::{infer_wkt_type, read_wkt};
#[cfg(feature = "std")]
pub use read::read_wkt_from;
pub use write::write_wkt;
#[cfg(feature = "std")]
pub use write::write_wkt_to;

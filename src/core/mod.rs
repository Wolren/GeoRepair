use geo::algorithm::bool_ops::FillRule;
use thiserror::Error;

use crate::crs::Crs;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for [`MakeValid`](crate::MakeValid) repair operations.
///
/// Controls polygon method selection, fill rule, CRS handling, and
/// whether collapsed geometries are preserved.
#[derive(Clone, Debug)]
pub struct MakeValidConfig {
    /// If true, keep collapsed/degenerate geometries instead of dropping them.
    pub keep_collapsed: bool,
    /// Polygon repair method selection (Auto, Structure, Arrange).
    pub poly_method: PolyMethod,
    /// Fill rule for polygon repair (EvenOdd or NonZero).
    pub fill_rule: FillRule,
    /// CRS of the input geometry.
    /// When set, used for CRS-aware tolerance and metadata preservation.
    pub crs: Option<Crs>,
    /// Target output CRS.
    /// When set, geometries are transformed to this CRS after repair
    /// (requires the `proj` feature, not yet available).
    pub target_crs: Option<Crs>,
}

impl Default for MakeValidConfig {
    fn default() -> Self {
        Self {
            keep_collapsed: false,
            poly_method: PolyMethod::Auto,
            fill_rule: FillRule::EvenOdd,
            crs: None,
            target_crs: None,
        }
    }
}

/// Polygon repair method selection.
///
/// - [`Auto`](PolyMethod::Auto): tries Structure fast path first; falls back to Arrange CDT.
/// - [`Structure`](PolyMethod::Structure): boolean-operation-based structural repair (fast).
/// - [`Arrange`](PolyMethod::Arrange): Constrained Delaunay Triangulation (robust).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PolyMethod {
    Auto,
    Structure,
    Arrange,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during I/O and repair operations.
///
/// Returned by format-agnostic load/export functions and repair operations
/// that interact with external systems (file I/O, triangulation, CRS).
#[derive(Error, Clone, Debug)]
pub enum MakeValidError {
    #[error("coordinate value is NaN at index {idx}")]
    CoordinateIsNaN { idx: usize },

    #[error("coordinate value is infinite at index {idx}")]
    CoordinateIsInfinite { idx: usize },

    #[error("constraint edge insertion failed in CDT — likely numerical precision issue")]
    ConstraintFailure,

    #[error("triangulation error: {0}")]
    TriangulationError(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("CRS error: {0}")]
    CrsError(String),
}

#[cfg(feature = "arrange")]
impl From<spade::InsertionError> for MakeValidError {
    fn from(e: spade::InsertionError) -> Self {
        match e {
            spade::InsertionError::NAN => Self::CoordinateIsNaN { idx: 0 },
            spade::InsertionError::TooLarge | spade::InsertionError::TooSmall => {
                Self::ConstraintFailure
            }
        }
    }
}

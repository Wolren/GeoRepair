use geo::algorithm::bool_ops::FillRule;
use thiserror::Error;

use crate::crs::Crs;

// ---------------------------------------------------------------------------
// Numeric constants
// ---------------------------------------------------------------------------

/// Default geometric epsilon for robust equality / zero checks.
pub(crate) const EPS: f64 = 1e-12;

/// Epsilon for parametric (t/u) intersection comparisons.
pub(crate) const EPS_PARAM: f64 = 1e-14;

/// Vertex count threshold above which grid-based spatial indexing is used
/// instead of brute-force O(n²) checks for self-intersection detection
/// and edge splitting. At n=2000 the brute-force 2M pair checks completes
/// in ~5ms for most geometries, well below the cost of grid construction.
pub(crate) const GRID_THRESHOLD_N: usize = 2000;

/// Maximum total vertices for the fast-path validity check in fix_polygon.
/// Larger polygons fall through to the full repair pipeline.
pub(crate) const FAST_PATH_MAX_VERTS: usize = 50000;

/// Snap scale factor for integer-keyed graph construction.
pub(crate) const SNAP_SCALE: f64 = 1e8;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct MakeValidConfig {
    pub keep_collapsed: bool,
    pub poly_method: PolyMethod,
    pub fill_rule: FillRule,
    /// CRS of the input geometry.
    /// When set, used for CRS-aware tolerance and metadata preservation.
    pub crs: Option<Crs>,
    /// Target output CRS.
    /// When set, geometries are transformed to this CRS after repair
    /// via PROJ (requires the `proj` feature).
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PolyMethod {
    Auto,
    Structure,
    Arrange,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

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

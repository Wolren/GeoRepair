use geo::algorithm::bool_ops::FillRule;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct MakeValidConfig {
    pub keep_collapsed: bool,
    pub poly_method: PolyMethod,
    pub fill_rule: FillRule,
}

impl Default for MakeValidConfig {
    fn default() -> Self {
        Self {
            keep_collapsed: false,
            poly_method: PolyMethod::Auto,
            fill_rule: FillRule::EvenOdd,
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
}

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

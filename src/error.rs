use geo::GeoFloat;
use thiserror::Error;

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

pub(crate) trait RaiseError {
    type Scalar: GeoFloat;
    fn check_valid_coords(&self) -> Result<(), MakeValidError>;
}

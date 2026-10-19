use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use geo::GeoFloat;

/// Trait for OGC geometry validation.
pub trait GeoValidation {
    /// The scalar coordinate type (e.g. `f64`, `f32`).
    type Scalar: GeoFloat;

    /// Quick validity check - returns `true` if the geometry passes all OGC rules.
    fn is_valid(&self) -> bool {
        self.validate().valid
    }

    /// Full validation - returns a [`ValidationResult`] with all violations found.
    fn validate(&self) -> ValidationResult;

    /// Human-readable validity reason (like GEOS `isValidReason`). Returns `"Valid
    /// Geometry"` when valid, or a semicolon-separated list of violation descriptions
    /// when invalid.
    fn validate_reason(&self) -> String {
        let result = self.validate();
        if result.valid {
            "Valid Geometry".to_string()
        } else {
            result
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

pub(crate) use super::edges::*;
/// Facade: re-export the extracted submodules so existing `crate::validation::core::*`
/// imports keep resolving.
pub use super::errors::*;
pub(crate) use super::holes::*;
pub use super::impls::*;
pub use super::result::*;
pub use super::ring::*;
pub(crate) use super::sweep::*;

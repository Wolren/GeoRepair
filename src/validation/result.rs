//! ValidationResult and its helper methods
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.


use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use super::errors::*;


/// Result of an OGC validity check.
///
/// Contains the overall valid/invalid status and a list of detailed
/// [`GeometryValidationError`] entries describing each violation found.
///
/// # Examples
///
/// ```rust
/// # use geo::{Geometry, Point};
/// # let geometry = Geometry::Point(Point::new(0.0, 0.0));
/// use geo_repair::{validate, ValidationResult};
///
/// let result = validate(&geometry);
/// if result.valid {
///     println!("Geometry is valid");
/// } else {
///     for err in &result.errors {
///         println!("  Violation: {err}");
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationResult {
    /// Whether the geometry passed all OGC validity checks.
    pub valid: bool,
    /// List of validity violations found. Empty when `valid` is true.
    pub errors: Vec<GeometryValidationError>,
}

impl ValidationResult {
    /// Create a result indicating a valid geometry (no errors).
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
        }
    }

    /// Create a result indicating an invalid geometry with the given errors.
    pub fn invalid(errors: Vec<GeometryValidationError>) -> Self {
        Self {
            valid: false,
            errors,
        }
    }

    /// Human-readable validity reason (like GEOS `isValidReason`).
    ///
    /// Returns `"Valid Geometry"` when valid, or a semicolon-separated list of
    /// violations when invalid.
    pub fn reason(&self) -> String {
        if self.valid {
            "Valid Geometry".to_string()
        } else {
            self.errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

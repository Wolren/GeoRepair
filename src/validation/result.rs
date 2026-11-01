//! ValidationResult and its helper methods
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance: core.rs was
//! 2540 lines; the cap is 800). Content is verbatim - no behavior changes; sibling
//! modules resolve shared items through the `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.

use super::errors::*;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

///
/// # Examples
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
    /// List of validity violations found.
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

    /// Human-readable validity reason (like GEOS `isValidReason`). Returns `"Valid
    /// Geometry"` when valid, or a semicolon-separated list of violations when invalid.
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

    /// Error count per class, in [`ValidationClass::ALL`] order, zero included.
    pub fn class_counts(&self) -> [(ValidationClass, usize); 6] {
        let mut counts = [0usize; 6];
        for error in &self.errors {
            counts[error.class().index()] += 1;
        }
        let mut out = [(ValidationClass::Coordinates, 0usize); 6];
        for (slot, class) in out.iter_mut().zip(ValidationClass::ALL) {
            *slot = (class, counts[class.index()]);
        }
        out
    }

    /// Distinct violation codes present, each with its count, in first-seen order.
    pub fn code_counts(&self) -> Vec<(&'static str, usize)> {
        let mut out: Vec<(&'static str, usize)> = Vec::new();
        for error in &self.errors {
            let code = error.code();
            match out.iter_mut().find(|(c, _)| *c == code) {
                Some((_, n)) => *n += 1,
                None => out.push((code, 1)),
            }
        }
        out
    }
}

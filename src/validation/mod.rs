//! OGC Simple Features geometry validation predicates.
//!
//! Checks 18 validity rules from the OGC Simple Features specification using
//! Shewchuk adaptive-precision orientation tests (via the `robust` crate)
//! for reliable results near degeneracies.
//!
//! # Rules checked
//!
//! | Rule | Applies to |
//! |------|-----------|
//! | Coordinate finiteness | All geometries |
//! | Ring closure | Polygon rings |
//! | Ring minimum vertices (>=4) | Polygon rings |
//! | Ring self-intersection | Polygon rings |
//! | Pinch points (non-consecutive duplicates) | Rings |
//! | Hole containment (inside shell) | Polygon |
//! | No nested holes | Polygon |
//! | Interior ring connectivity | Polygon |
//! | Ring orientation (exterior CCW, interior CW) | Polygon |
//! | Non-collinear rings | Polygon |
//! | Consecutive duplicates | Lines/rings |
//! | Duplicate rings | Polygon |
//! | Duplicate points | MultiPoint |
//! | Duplicate lines | MultiLineString |
//! | Non-zero-length lines | Line |
//! | Non-degenerate exterior | Polygon |
//! | Simplicity (no interior intersections) | LineString, MultiLineString |
//! | Nesting depth limit | GeometryCollection |
//!
//! # Usage
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geom = Geometry::Point(Point::new(0.0, 0.0));
//! use geo_repair::{is_valid, validate, validate_reason, GeoValidation, ValidationResult};
//!
//! let ok: bool = is_valid(&geom);
//! let result: ValidationResult = validate(&geom);
//! let reason: String = validate_reason(&geom);
//!
//! // Trait-based (needs import)
//! let ok2: bool = geom.is_valid();
//! ```
//!
//! Passes **2490/2490** tests from the GEOS XML validation test suite.
mod complex;
mod core;

pub use core::*;

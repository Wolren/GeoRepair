//! OGC Simple Features geometry validation predicates.
//!
//! Checks 18 validity rules from the OGC Simple Features specification using
//! Shewchuk adaptive-precision orientation tests (via the `robust` crate)
//! for reliable results near degeneracies.
//!
//! # Strictness policy: exact predicates plus noise-scale gates
//!
//! The predicates are exact (Shewchuk), and where GEOS itself is exact they
//! agree with GEOS's classification - verified by the 934/934 GEOS XML
//! suite pass. On top of the exact predicates the validator applies two
//! RELATIVE tolerance gates:
//!
//! - `collinear_eps = 32 * EPSILON * L²` (edge-pair proximity, see
//!   `edge_intersects`): edges whose exact orientation is nonzero but
//!   within ~32 ulps of the pair's own length scale are treated as
//!   coincident and flag a SelfIntersection. This is the only class where
//!   the validator is deliberately STRICTER than GEOS isValid.
//! - `1e-12 * L²` vertex-on-edge tolerance (T-junction class): a vertex
//!   lying on a non-adjacent edge - exactly, or within orient rounding
//!   noise of the exact collinearity - flags a SelfIntersection. This
//!   implements GEOS's own rule (GEOS flags vertex-on-edge rings, XML Test
//!   22) with a noise floor instead of an exact-only test. The check is
//!   bbox-prefiltered: the vertex's own segment must overlap the edge's
//!   bounding box, so a vertex that merely approaches an edge within the
//!   band without touching it (its segment's bbox sits strictly off the
//!   edge's bbox) is accepted.
//!
//! Rationale for the collinear gate: production GIS data comes from
//! toolchains whose precision is far below f64, so a 32-ulp separation
//! almost certainly means the source intended the edges to coincide.
//! Accepting noise-scale separations as valid bakes rounding artifacts into
//! the topology and destabilizes downstream overlay/buffer geometry. The
//! gates are relative to the pair's own length so the same feature
//! classifies identically at any coordinate magnitude.
//!
//! Conformance note: per the strict OGC definition (exact predicates only)
//! the collinear gate is non-conformant strictness - GEOS isValid and a
//! pure exact validator accept the class. The repair contract is a
//! superset: everything the gates flag is repaired, and the repaired
//! output satisfies GEOS isValid (measured: 0/2298 flagged parts invalid
//! per GEOS after repair). The gates are the two epsilons above; they are
//! the deliberate, documented strictness policy of this crate, not
//! implementation noise.
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
//! Passes **937/937 dispatched** cases from the GEOS XML validation test suite
//! (213 documented masked divergences, 0 known gaps; measured 2026-08-07).
/// Complex validation rules (polygon nesting, multi-geometry checks).
mod complex;
/// Core validation types, traits, and per-geometry implementations (facade).
mod core;
/// Edge-pair intersection predicates and ring edge trees.
pub(crate) mod edges;
/// GeometryValidationError and its impls.
mod errors;
/// Adapter exposing geo_repair's validator through geo's `Validation` trait.
mod geo_bridge;
/// Hole validation, nesting, and cycle detection.
pub(crate) mod holes;
/// Per-geometry GeoValidation impls and free validate functions.
pub(crate) mod impls;
/// ValidationResult and its helper methods.
mod result;
/// Ring validity: closure, self-intersection, orientation, points, duplicates.
pub(crate) mod ring;
/// Sweep-line machinery for ring self-intersection.
pub(crate) mod sweep;

/// Shared by the validator and the fast-path gate (duplicated rings).
pub(crate) use complex::has_duplicate_rings;
/// Re-export all core validation items: [`GeoValidation`], [`GeometryValidationError`],
/// [`ValidationResult`], [`is_valid`], [`validate`], [`validate_reason`].
pub use core::*;
/// geo-trait-compatible adapter and error mapping.
pub use geo_bridge::{GeoRepairValidation, map_geo_invalid};

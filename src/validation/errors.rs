//! GeometryValidationError and its Display/Error impls
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance: core.rs was
//! 2540 lines; the cap is 800). Content is verbatim - no behavior changes; sibling
//! modules resolve shared items through the `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.

use geo::Coord;
use thiserror::Error;

#[derive(Error, Clone, Debug, PartialEq)]
pub enum GeometryValidationError {
    /// One or more coordinates contain NaN or infinite values.
    #[error("Coordinate is NaN")]
    CoordinateNaN,

    #[error("Ring has too few points: found {found}, minimum required {min}")]
    RingTooFewPoints { found: usize, min: usize },

    #[error("Ring is not closed: first {first:?} != last {last:?}")]
    RingNotClosed { first: Coord<f64>, last: Coord<f64> },

    #[error("Ring has self-intersections")]
    SelfIntersection,

    #[error("Ring has repeated non-consecutive vertices (pinch point)")]
    PinchPoint,

    #[error("Hole lies outside shell")]
    HoleOutsideShell,

    #[error("Holes are nested")]
    NestedHoles,

    #[error("Interior ring is disconnected from shell")]
    DisconnectedInteriorRing,

    #[error("Wrong ring orientation: exterior should be CCW, interior CW")]
    WrongOrientation,

    #[error("Collinear ring: all points lie on a line")]
    CollinearRing,

    #[error("Geometry has repeated (duplicate) points")]
    RepeatedPoint,

    #[error("Geometry contains duplicate rings")]
    DuplicatedRings,

    #[error("MultiPoint contains duplicate points")]
    MultiPointDuplicatePoints,

    #[error("MultiLineString contains duplicate linestrings")]
    MultiLineStringDuplicateLines,

    #[error("Line has zero length (start == end at {0:?})")]
    ZeroLengthLine(Coord<f64>),

    #[error("Polygon exterior ring is degenerate (collapsed)")]
    DegenerateExterior,

    #[error("Geometry is not simple: components intersect at interior points")]
    NotSimple,

    #[error("GeometryCollection nesting exceeds maximum depth")]
    ExcessiveNesting,
}

//! GeometryValidationError and its Display/Error impls
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.

use geo::Coord;
use thiserror::Error;


/// Errors reported by OGC geometry validation.
///
/// Each variant corresponds to an OGC Simple Features validity rule.
#[derive(Error, Clone, Debug, PartialEq)]
pub enum GeometryValidationError {
    /// One or more coordinates contain NaN or infinite values.
    #[error("Coordinate is NaN")]
    CoordinateNaN,

    /// A ring does not have enough distinct vertices (min 4 for rings).
    #[error("Ring has too few points: found {found}, minimum required {min}")]
    RingTooFewPoints { found: usize, min: usize },

    /// A ring's first and last coordinates are not equal (not closed).
    #[error("Ring is not closed: first {first:?} != last {last:?}")]
    RingNotClosed { first: Coord<f64>, last: Coord<f64> },

    /// A ring has edges that cross or overlap non-adjacent edges.
    #[error("Ring has self-intersections")]
    SelfIntersection,

    /// A ring has a non-consecutive repeated vertex (pinch point).
    #[error("Ring has repeated non-consecutive vertices (pinch point)")]
    PinchPoint,

    /// A polygon hole lies partially or fully outside its shell.
    #[error("Hole lies outside shell")]
    HoleOutsideShell,

    /// Two or more polygon holes are nested inside each other.
    #[error("Holes are nested")]
    NestedHoles,

    /// An interior ring is disconnected from the shell (touching at ≥ 2 points or edges crossing).
    #[error("Interior ring is disconnected from shell")]
    DisconnectedInteriorRing,

    /// Ring winding direction is incorrect (exterior must be CCW, interior CW).
    #[error("Wrong ring orientation: exterior should be CCW, interior CW")]
    WrongOrientation,

    /// All vertices of a ring are collinear (zero area).
    #[error("Collinear ring: all points lie on a line")]
    CollinearRing,

    /// Consecutive duplicate coordinates found in a geometry.
    #[error("Geometry has repeated (duplicate) points")]
    RepeatedPoint,

    /// A polygon contains two or more identical rings.
    #[error("Geometry contains duplicate rings")]
    DuplicatedRings,

    /// A MultiPoint contains the same point more than once.
    #[error("MultiPoint contains duplicate points")]
    MultiPointDuplicatePoints,

    /// A MultiLineString contains the same linestring more than once.
    #[error("MultiLineString contains duplicate linestrings")]
    MultiLineStringDuplicateLines,

    /// A Line has zero length (start and end coordinates are equal).
    #[error("Line has zero length (start == end at {0:?})")]
    ZeroLengthLine(Coord<f64>),

    /// A polygon's exterior ring has degenerated to a line or point.
    #[error("Polygon exterior ring is degenerate (collapsed)")]
    DegenerateExterior,

    /// A LineString or MultiLineString has components that intersect at interior points.
    #[error("Geometry is not simple: components intersect at interior points")]
    NotSimple,

    /// A GeometryCollection has exceeded the maximum nesting depth.
    #[error("GeometryCollection nesting exceeds maximum depth")]
    ExcessiveNesting,
}

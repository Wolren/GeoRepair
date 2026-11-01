//! GeometryValidationError and its Display/Error impls
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance: core.rs was
//! 2540 lines; the cap is 800). Content is verbatim - no behavior changes; sibling
//! modules resolve shared items through the `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.

use geo::Coord;
use thiserror::Error;

/// Errors reported by OGC geometry validation.
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

    /// An interior ring is disconnected from the shell (touching at ≥ 2 points or edges
    /// crossing).
    #[error("Interior ring is disconnected from shell")]
    DisconnectedInteriorRing,

    /// Ring winding direction is incorrect (exterior must be ccw, interior CW).
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

    /// A LineString or MultiLineString has components that intersect at interior
    /// points.
    #[error("Geometry is not simple: components intersect at interior points")]
    NotSimple,

    /// A GeometryCollection has exceeded the maximum nesting depth.
    #[error("GeometryCollection nesting exceeds maximum depth")]
    ExcessiveNesting,
}

/// Coarse grouping of [`GeometryValidationError`] for reports and check tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ValidationClass {
    /// A coordinate is NaN or infinite.
    Coordinates,
    /// A ring or collection breaks a structural rule: too few points, not closed, too
    /// deep.
    Structure,
    /// Parts touch, cross, or escape where the topology forbids it.
    Topology,
    /// A ring winds the wrong way.
    Orientation,
    /// A ring or line has no area, no length, or no extent.
    Degenerate,
    /// A vertex, ring, or line is present more than once.
    Duplication,
}

impl ValidationClass {
    /// Every class, in report order.
    pub const ALL: [ValidationClass; 6] = [
        ValidationClass::Coordinates,
        ValidationClass::Structure,
        ValidationClass::Topology,
        ValidationClass::Orientation,
        ValidationClass::Degenerate,
        ValidationClass::Duplication,
    ];

    /// Position of this class in [`ValidationClass::ALL`].
    pub fn index(self) -> usize {
        match self {
            ValidationClass::Coordinates => 0,
            ValidationClass::Structure => 1,
            ValidationClass::Topology => 2,
            ValidationClass::Orientation => 3,
            ValidationClass::Degenerate => 4,
            ValidationClass::Duplication => 5,
        }
    }

    /// Stable machine key, safe to store in a report row.
    pub fn code(self) -> &'static str {
        match self {
            ValidationClass::Coordinates => "coordinates",
            ValidationClass::Structure => "structure",
            ValidationClass::Topology => "topology",
            ValidationClass::Orientation => "orientation",
            ValidationClass::Degenerate => "degenerate",
            ValidationClass::Duplication => "duplication",
        }
    }

    /// Short display name for a check tool.
    pub fn label(self) -> &'static str {
        match self {
            ValidationClass::Coordinates => "Coordinates",
            ValidationClass::Structure => "Structure",
            ValidationClass::Topology => "Topology",
            ValidationClass::Orientation => "Orientation",
            ValidationClass::Degenerate => "Degenerate",
            ValidationClass::Duplication => "Duplication",
        }
    }
}

impl GeometryValidationError {
    /// Stable machine key for this violation; codes never change between releases.
    pub fn code(&self) -> &'static str {
        match self {
            GeometryValidationError::CoordinateNaN => "coordinate_nan",
            GeometryValidationError::RingTooFewPoints { .. } => "ring_too_few_points",
            GeometryValidationError::RingNotClosed { .. } => "ring_not_closed",
            GeometryValidationError::SelfIntersection => "self_intersection",
            GeometryValidationError::PinchPoint => "pinch_point",
            GeometryValidationError::HoleOutsideShell => "hole_outside_shell",
            GeometryValidationError::NestedHoles => "nested_holes",
            GeometryValidationError::DisconnectedInteriorRing => "disconnected_interior_ring",
            GeometryValidationError::WrongOrientation => "wrong_orientation",
            GeometryValidationError::CollinearRing => "collinear_ring",
            GeometryValidationError::RepeatedPoint => "repeated_point",
            GeometryValidationError::DuplicatedRings => "duplicated_rings",
            GeometryValidationError::MultiPointDuplicatePoints => "multipoint_duplicate_points",
            GeometryValidationError::MultiLineStringDuplicateLines => {
                "multilinestring_duplicate_lines"
            }
            GeometryValidationError::ZeroLengthLine(_) => "zero_length_line",
            GeometryValidationError::DegenerateExterior => "degenerate_exterior",
            GeometryValidationError::NotSimple => "not_simple",
            GeometryValidationError::ExcessiveNesting => "excessive_nesting",
        }
    }

    /// Short display name without payload values, for a table row or a list item.
    pub fn label(&self) -> &'static str {
        match self {
            GeometryValidationError::CoordinateNaN => "NaN coordinate",
            GeometryValidationError::RingTooFewPoints { .. } => "Ring too few points",
            GeometryValidationError::RingNotClosed { .. } => "Ring not closed",
            GeometryValidationError::SelfIntersection => "Self-intersection",
            GeometryValidationError::PinchPoint => "Pinch point",
            GeometryValidationError::HoleOutsideShell => "Hole outside shell",
            GeometryValidationError::NestedHoles => "Nested holes",
            GeometryValidationError::DisconnectedInteriorRing => "Disconnected interior ring",
            GeometryValidationError::WrongOrientation => "Wrong ring orientation",
            GeometryValidationError::CollinearRing => "Collinear ring",
            GeometryValidationError::RepeatedPoint => "Repeated point",
            GeometryValidationError::DuplicatedRings => "Duplicated rings",
            GeometryValidationError::MultiPointDuplicatePoints => "Duplicate points",
            GeometryValidationError::MultiLineStringDuplicateLines => "Duplicate lines",
            GeometryValidationError::ZeroLengthLine(_) => "Zero-length line",
            GeometryValidationError::DegenerateExterior => "Degenerate exterior",
            GeometryValidationError::NotSimple => "Not simple",
            GeometryValidationError::ExcessiveNesting => "Excessive nesting",
        }
    }

    /// Coarse class this violation belongs to.
    pub fn class(&self) -> ValidationClass {
        match self {
            GeometryValidationError::CoordinateNaN => ValidationClass::Coordinates,
            GeometryValidationError::RingTooFewPoints { .. }
            | GeometryValidationError::RingNotClosed { .. }
            | GeometryValidationError::ExcessiveNesting => ValidationClass::Structure,
            GeometryValidationError::SelfIntersection
            | GeometryValidationError::PinchPoint
            | GeometryValidationError::HoleOutsideShell
            | GeometryValidationError::NestedHoles
            | GeometryValidationError::DisconnectedInteriorRing
            | GeometryValidationError::NotSimple => ValidationClass::Topology,
            GeometryValidationError::WrongOrientation => ValidationClass::Orientation,
            GeometryValidationError::CollinearRing
            | GeometryValidationError::ZeroLengthLine(_)
            | GeometryValidationError::DegenerateExterior => ValidationClass::Degenerate,
            GeometryValidationError::RepeatedPoint
            | GeometryValidationError::DuplicatedRings
            | GeometryValidationError::MultiPointDuplicatePoints
            | GeometryValidationError::MultiLineStringDuplicateLines => {
                ValidationClass::Duplication
            }
        }
    }
}

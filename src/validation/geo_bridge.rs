//! Bridge to geo's [`Validation`]
//! trait taxonomy.
//!
//! geo's `Validation` trait and per-type `Invalid*` error enums are the
//! validation vocabulary of the georust ecosystem. geo_repair's validator is
//! stricter (see the module docs on the collinear gate and T-junction rule)
//! and passes the GEOS XML suite, but its error taxonomy differs from geo's.
//!
//! The orphan rule prevents implementing geo's trait for geo's own types, so
//! this adapter wraps any `Geometry<f64>` in [`GeoRepairValidation`]: code
//! written against geo's trait — generic bounds on `Validation`,
//! `.validation_errors()`, `.check_validation()` — then consumes geo_repair's
//! engine without changing its error handling.
//!
//! The mapping is best-effort. geo_repair checks 18 OGC rules while geo's
//! enums cover a subset, and geo carries ring/index payloads that
//! geo_repair's errors do not record. Error classes with no geo counterpart
//! (ring closure, pinch points, nested holes, orientation, collinearity,
//! duplicates) are omitted from the geo view; they remain visible through
//! the plain [`validate`](crate::validate) API.

use alloc::boxed::Box;
use geo::Geometry;
use geo::algorithm::validation::{
    CoordIndex, InvalidGeometry, InvalidLine, InvalidLineString, InvalidPoint, InvalidPolygon,
    RingRole, Validation,
};

use super::{GeometryValidationError, validate};

/// Adapter exposing geo_repair's validation engine through geo's
/// [`Validation`] trait.
///
/// Wrap any `&Geometry<f64>` and call the trait methods directly:
///
/// ```rust
/// # use geo::{Geometry, Polygon, LineString};
/// # let poly = Polygon::new(LineString::from(vec![
/// #     (0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0),
/// # ]), vec![]);
/// # let geometry = Geometry::Polygon(poly);
/// use geo::algorithm::validation::Validation;
/// use geo_repair::GeoRepairValidation;
///
/// let adapter = GeoRepairValidation(&geometry);
/// assert!(!adapter.is_valid());
/// assert_eq!(adapter.validation_errors().len(), 1);
/// ```
pub struct GeoRepairValidation<'a>(pub &'a Geometry<f64>);

impl Validation for GeoRepairValidation<'_> {
    type Error = InvalidGeometry;

    fn visit_validation<T>(
        &self,
        mut handle: Box<dyn FnMut(Self::Error) -> Result<(), T> + '_>,
    ) -> Result<(), T> {
        for err in validate(self.0).errors {
            if let Some(invalid) = map_geo_invalid(self.0, &err) {
                handle(invalid)?;
            }
        }
        Ok(())
    }
}

/// Best-effort mapping of one geo_repair validation error into geo's
/// [`InvalidGeometry`] taxonomy for the given geometry.
///
/// Returns `None` for error classes with no geo counterpart (geo does not
/// model ring closure, pinch points, nested holes, orientation,
/// collinearity, or duplicates) and for geometry types geo models with a
/// different shape (multi-geometries, collections, rect, triangle).
pub fn map_geo_invalid(
    geometry: &Geometry<f64>,
    err: &GeometryValidationError,
) -> Option<InvalidGeometry> {
    use GeometryValidationError as E;
    match geometry {
        Geometry::Point(_) => match err {
            E::CoordinateNaN => Some(InvalidGeometry::InvalidPoint(InvalidPoint::NonFiniteCoord)),
            _ => None,
        },
        Geometry::Line(_) => match err {
            E::ZeroLengthLine(_) => {
                Some(InvalidGeometry::InvalidLine(InvalidLine::IdenticalCoords))
            }
            E::CoordinateNaN => Some(InvalidGeometry::InvalidLine(InvalidLine::NonFiniteCoord(
                CoordIndex(0),
            ))),
            _ => None,
        },
        Geometry::LineString(_) => match err {
            E::CoordinateNaN => Some(InvalidGeometry::InvalidLineString(
                InvalidLineString::NonFiniteCoord(CoordIndex(0)),
            )),
            _ => None,
        },
        Geometry::Polygon(_) => match err {
            E::CoordinateNaN => Some(InvalidGeometry::InvalidPolygon(
                InvalidPolygon::NonFiniteCoord(RingRole::Exterior, CoordIndex(0)),
            )),
            E::RingTooFewPoints { .. } => Some(InvalidGeometry::InvalidPolygon(
                InvalidPolygon::TooFewPointsInRing(RingRole::Exterior),
            )),
            E::SelfIntersection => Some(InvalidGeometry::InvalidPolygon(
                InvalidPolygon::SelfIntersection(RingRole::Exterior),
            )),
            E::HoleOutsideShell | E::DisconnectedInteriorRing => {
                Some(InvalidGeometry::InvalidPolygon(
                    InvalidPolygon::InteriorRingNotContainedInExteriorRing(RingRole::Interior(0)),
                ))
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{LineString, Polygon};

    fn poly(ring: Vec<(f64, f64)>) -> Geometry<f64> {
        Geometry::Polygon(Polygon::new(LineString::from(ring), vec![]))
    }

    #[test]
    fn valid_polygon_reports_nothing() {
        let geometry = poly(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let adapter = GeoRepairValidation(&geometry);
        assert!(adapter.is_valid());
        assert!(adapter.validation_errors().is_empty());
    }

    #[test]
    fn self_intersection_maps_to_geo_taxonomy() {
        let geometry = poly(vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]);
        let adapter = GeoRepairValidation(&geometry);
        assert!(!adapter.is_valid());

        let errors = adapter.validation_errors();
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            InvalidGeometry::InvalidPolygon(InvalidPolygon::SelfIntersection(RingRole::Exterior))
        ));

        // check_validation mirrors the same single error.
        assert!(adapter.check_validation().is_err());
    }

    #[test]
    fn stricter_than_geo_collinear_sliver() {
        // A 5e-14-tall sliver over a length-10 base: the two long edges lie
        // 5e-14 apart (about 28 ulps at this scale) - genuinely separated,
        // geometrically VALID, and GEOS accepts it. The old
        // `32 * EPSILON * L^2` collinear gate swallowed near-parallel pairs
        // (orient 5e-13 vs gate ~7.1e-13) and flagged a false
        // SelfIntersection; with the adaptive per-orient error bound
        // (2026-08-07) the orient 5e-13 is ~26 orders of magnitude above
        // its rounding bound, so the sliver is correctly accepted - the
        // strictness differential vs geo is the T-junction and exact-
        // collinear classes, not length-inflated margins.
        let poly = Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 5e-14),
                (0.0, 5e-14),
                (0.0, 0.0),
            ]),
            vec![],
        );
        let geometry = Geometry::Polygon(poly);

        // geo's own engine: exact-only, accepts.
        use geo::algorithm::validation::Validation as GeoValidation;
        assert!(
            geometry.is_valid(),
            "geo exact validator accepts this input"
        );

        // geo_repair engine through the bridge: accepts too (adaptive bound).
        let adapter = GeoRepairValidation(&geometry);
        assert!(
            adapter.is_valid(),
            "near-parallel separated edges are valid"
        );
    }

    #[test]
    fn flags_true_t_junction_that_geo_accepts() {
        // The strictness differential: a ring vertex lying EXACTLY on a
        // non-adjacent edge (T-junction). geo's exact validator passes it
        // (no area test), geo_repair flags SelfIntersection (GEOS
        // IsValidOp Test 22 also rejects it).
        let poly = Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (5.0, 0.0), // exactly on edge (0,0)-(10,0)
                (0.0, 10.0),
            ]),
            vec![],
        );
        let geometry = Geometry::Polygon(poly);
        let adapter = GeoRepairValidation(&geometry);
        assert!(
            !adapter.is_valid(),
            "geo_repair must flag the exact T-junction"
        );
    }

    #[test]
    fn unmappable_classes_are_skipped_not_fabricated() {
        // Clockwise exterior ring: geo_repair flags WrongOrientation (its
        // rule table includes ring orientation); geo has no orientation rule
        // in its InvalidPolygon taxonomy, so the adapter must stay silent
        // rather than fabricate a wrong geo error.
        let geometry = poly(vec![
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]);
        let result = validate(&geometry);
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, GeometryValidationError::WrongOrientation))
        );

        use geo::algorithm::validation::Validation as GeoValidation;
        assert!(geometry.is_valid(), "geo has no orientation rule");

        let adapter = GeoRepairValidation(&geometry);
        assert!(adapter.validation_errors().is_empty());
    }
}

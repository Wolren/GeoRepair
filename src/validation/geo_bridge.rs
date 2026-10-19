//! Best-effort: geo_repair checks 18 ogc rules, geo enums cover a subset, and geo
//! carries payloads that geo_repair errors omit.

use alloc::boxed::Box;
use geo::Geometry;
use geo::algorithm::validation::{
    CoordIndex, InvalidGeometry, InvalidLine, InvalidLineString, InvalidPoint, InvalidPolygon,
    RingRole, Validation,
};

use super::{GeometryValidationError, validate};

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

        assert!(adapter.check_validation().is_err());
    }

    #[test]
    fn stricter_than_geo_collinear_sliver() {
        // 5e-14 apart (about 28 ulps at this scale) - genuinely separated,
        // geometrically valid.
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

        use geo::algorithm::validation::Validation as GeoValidation;
        assert!(
            geometry.is_valid(),
            "geo exact validator accepts this input"
        );

        let adapter = GeoRepairValidation(&geometry);
        assert!(
            adapter.is_valid(),
            "near-parallel separated edges are valid"
        );
    }

    #[test]
    fn flags_true_t_junction_that_geo_accepts() {
        // geo_repair flags SelfIntersection (GEOS IsValidOp Test 22 also rejects it).
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

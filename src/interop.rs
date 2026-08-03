//! geo-traits interop surface.
//!
//! Enabled by the `geo-traits` feature. Exposes the validation and repair
//! engines over [`geo_traits::GeometryTrait`] and
//! [`geo_traits::GeometryCollectionTrait`], the trait layer shared by the
//! georust data ecosystem (geo, geoarrow, geozero, the `wkb` crate).
//!
//! Any geometry source that implements those traits can be validated or
//! repaired in one call. The engine materializes a `geo::Geometry<f64>`
//! internally and returns the result as geo geometry; callers that need the
//! result back in their own representation convert with their crate's
//! geo-traits writer (e.g. `wkb::writer::write_wkb`, geoarrow's `ToWKB`).
//!
//! # Example
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geometry = Geometry::Point(Point::new(0.0, 0.0));
//! #[cfg(feature = "geo-traits")]
//! {
//!     use geo_repair::interop::{is_valid_geometry, make_valid_geometry};
//!
//!     // `geometry` is any geo-traits source; here it is a geo type.
//!     let ok = is_valid_geometry(&geometry);
//!     let fixed = make_valid_geometry(&geometry);
//! }
//! ```

use geo::{Geometry, GeometryCollection};
use geo_traits::to_geo::ToGeoGeometry;
use geo_traits::{GeometryCollectionTrait, GeometryTrait};

use crate::make_valid::MakeValid;
use crate::validation::ValidationResult;

/// Repair one geometry from any geo-traits source.
///
/// The result is a valid `geo::Geometry<f64>`. Geometry that cannot be
/// represented as geo geometry (an empty point, or a MultiPoint containing
/// an empty point) is returned as an empty [`GeometryCollection`], matching
/// geo's own representation limits.
pub fn make_valid_geometry<G>(geometry: &G) -> Geometry<f64>
where
    G: GeometryTrait<T = f64>,
{
    match geometry.try_to_geometry() {
        Some(g) => g.make_valid(),
        None => Geometry::GeometryCollection(GeometryCollection::empty()),
    }
}

/// Repair every geometry in a collection, preserving order.
///
/// Each element is repaired independently; an invalid element never aborts
/// the batch.
pub fn make_valid_geometries<G>(collection: &G) -> Vec<Geometry<f64>>
where
    G: GeometryCollectionTrait<T = f64>,
{
    collection
        .geometries()
        .map(|g| make_valid_geometry(&g))
        .collect()
}

/// Repair a whole collection into a single [`GeometryCollection`].
pub fn make_valid_geometry_collection<G>(collection: &G) -> GeometryCollection<f64>
where
    G: GeometryCollectionTrait<T = f64>,
{
    GeometryCollection(make_valid_geometries(collection))
}

/// Quick validity check for any geo-traits geometry.
///
/// An unrepresentable geometry (empty point / MultiPoint with empty points)
/// is treated as valid, matching OGC.
pub fn is_valid_geometry<G>(geometry: &G) -> bool
where
    G: GeometryTrait<T = f64>,
{
    match geometry.try_to_geometry() {
        Some(g) => crate::is_valid(&g),
        None => true,
    }
}

/// Full OGC validation for any geo-traits geometry.
pub fn validate_geometry<G>(geometry: &G) -> ValidationResult
where
    G: GeometryTrait<T = f64>,
{
    match geometry.try_to_geometry() {
        Some(g) => crate::validate(&g),
        None => ValidationResult::valid(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Geometry, LineString, Polygon};
    use geo_traits::GeometryTrait;

    fn bowtie() -> Polygon<f64> {
        // Self-intersecting quad: (0,0) (10,10) (0,10) (10,0).
        Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        )
    }

    #[test]
    fn valid_geometry_passes_through() {
        let poly = Polygon::new(
            LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
            vec![],
        );
        let geom = Geometry::Polygon(poly);
        assert!(is_valid_geometry(&geom));
        let result = validate_geometry(&geom);
        assert!(result.valid);
        // Repair is a no-op on valid input.
        let fixed = make_valid_geometry(&geom);
        assert_eq!(fixed, geom);
    }

    #[test]
    fn invalid_geometry_repaired() {
        let geom = Geometry::Polygon(bowtie());
        assert!(!is_valid_geometry(&geom));
        let result = validate_geometry(&geom);
        assert!(!result.valid);

        let fixed = make_valid_geometry(&geom);
        assert!(crate::is_valid(&fixed), "repaired geometry must be valid");
    }

    #[test]
    fn batch_repair_collection() {
        let valid = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
            vec![],
        ));
        let invalid = Geometry::Polygon(bowtie());
        let collection = geo::GeometryCollection(vec![valid.clone(), invalid.clone()]);

        let repaired = make_valid_geometries(&collection);
        assert_eq!(repaired.len(), 2);
        assert_eq!(repaired[0], valid);
        assert!(crate::is_valid(&repaired[1]));

        let as_collection = make_valid_geometry_collection(&collection);
        assert_eq!(as_collection.0.len(), 2);
    }

    #[test]
    fn wkb_source_is_repairable() {
        // Drive the engine from a non-geo geometry source: the `wkb` crate
        // implements geo-traits, so its geometry works directly.
        use wkb::reader::read_wkb;
        let poly = bowtie();
        let bytes = crate::write_wkb(&Geometry::Polygon(poly));
        let wkb_geom = read_wkb(&bytes).expect("WKB parses");

        assert!(!is_valid_geometry(&wkb_geom));
        let fixed = make_valid_geometry(&wkb_geom);
        assert!(crate::is_valid(&fixed));
    }
}

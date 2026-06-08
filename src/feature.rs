use geo::{Coord, Geometry};
use serde_json::{Map, Value};

use crate::crs::Crs;
use crate::zm::{count_coords, preserve_zm, zm_pairs, ZmValue};

/// A GIS feature combining geometry with optional attributes, CRS, and Z/M values.
///
/// This is the primary data container for I/O operations, ensuring that
/// attribute data, CRS metadata, and Z/M coordinates are preserved through
/// the repair pipeline (only the geometry is modified by repair).
#[derive(Clone, Debug)]
pub struct Feature {
    pub geometry: Geometry<f64>,
    pub properties: Option<Map<String, Value>>,
    pub crs: Option<Crs>,
    pub zm: Vec<ZmValue>,
}

impl Feature {
    /// Create a new feature from a geometry with no attributes.
    pub fn new(geometry: Geometry<f64>) -> Self {
        let count = count_coords(&geometry);
        Self {
            geometry,
            properties: None,
            crs: None,
            zm: vec![ZmValue::NONE; count],
        }
    }

    /// Create a feature with all fields.
    pub fn with_all(
        geometry: Geometry<f64>,
        properties: Option<Map<String, Value>>,
        crs: Option<Crs>,
        zm: Vec<ZmValue>,
    ) -> Self {
        Self {
            geometry,
            properties,
            crs,
            zm,
        }
    }

    /// Create a feature with geometry and properties.
    pub fn with_properties(
        geometry: Geometry<f64>,
        properties: Option<Map<String, Value>>,
    ) -> Self {
        let count = count_coords(&geometry);
        Self {
            geometry,
            properties,
            crs: None,
            zm: vec![ZmValue::NONE; count],
        }
    }

    /// Set the CRS on this feature.
    pub fn with_crs(mut self, crs: Option<Crs>) -> Self {
        self.crs = crs;
        self
    }

    /// Set the Z/M values on this feature.
    pub fn with_zm(mut self, zm: Vec<ZmValue>) -> Self {
        self.zm = zm;
        self
    }

    /// Does this feature have Z (elevation) values?
    pub fn has_z(&self) -> bool {
        self.zm.iter().any(|z| z.z.is_some())
    }

    /// Does this feature have M (measure) values?
    pub fn has_m(&self) -> bool {
        self.zm.iter().any(|z| z.m.is_some())
    }

    /// Replace the geometry, preserving Z/M via nearest-coordinate matching.
    pub fn with_repaired_geometry(&self, repaired: Geometry<f64>) -> Self {
        let zm = if self.zm.iter().any(|z| !z.is_empty()) {
            preserve_zm(&self.geometry, &self.zm, &repaired)
        } else {
            vec![ZmValue::NONE; count_coords(&repaired)]
        };
        Self {
            geometry: repaired,
            properties: self.properties.clone(),
            crs: self.crs.clone(),
            zm,
        }
    }

    /// Iterate over (Coord, ZmValue) pairs.
    pub fn coord_zm_pairs(&self) -> impl Iterator<Item = (Coord<f64>, ZmValue)> + '_ {
        zm_pairs(&self.geometry, &self.zm)
    }

    /// Consume and return the inner geometry.
    pub fn into_geometry(self) -> Geometry<f64> {
        self.geometry
    }
}

impl From<Geometry<f64>> for Feature {
    fn from(geometry: Geometry<f64>) -> Self {
        Feature::new(geometry)
    }
}

//! Z/M coordinate value preservation and querying.
//!
//! Geometry repair can change coordinate counts and positions (e.g.
//! splitting a self-intersecting polygon). This module provides types
//! and functions to preserve Z (elevation) and M (measure) values
//! through the repair pipeline via nearest-coordinate matching.
//!
//! The main types are:
//! - \[`ZmValue`\]: a single Z/M pair for one coordinate
//! - \[`ZmGeometry`\]: a geometry paired with per-coordinate Z/M data
//! - \[`preserve_zm`\]: match Z/M from original to repaired geometry
//! - \[`zm_pairs`\]: iterate coords with their Z/M values
//! - \[`count_coords`\]: count vertices in a geometry
use geo::{Coord, Geometry};

/// A single Z (elevation) and M (measure) value associated with a coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ZmValue {
    /// Elevation (Z) value, or `None` if Z is not present.
    pub z: Option<f64>,
    /// Measure (M) value, or `None` if M is not present.
    pub m: Option<f64>,
}

impl ZmValue {
    /// A ZmValue with neither Z nor M set (all `None`).
    pub const NONE: ZmValue = ZmValue { z: None, m: None };

    /// Create a new ZmValue from optional Z and M values.
    ///
    /// Use `None` for missing components.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use geo_repair::zm::ZmValue;
    /// let zv = ZmValue::new(Some(100.5), Some(1.0));
    /// ```
    pub fn new(z: Option<f64>, m: Option<f64>) -> Self {
        Self { z, m }
    }

    /// Create a ZmValue with only a Z (elevation) component.
    pub fn z_only(z: f64) -> Self {
        Self {
            z: Some(z),
            m: None,
        }
    }

    /// Create a ZmValue with only an M (measure) component.
    pub fn m_only(m: f64) -> Self {
        Self {
            z: None,
            m: Some(m),
        }
    }

    /// Does this value have a Z (elevation) component?
    pub fn has_z(&self) -> bool {
        self.z.is_some()
    }
    /// Does this value have an M (measure) component?
    pub fn has_m(&self) -> bool {
        self.m.is_some()
    }
    /// Is this value completely empty (no Z, no M)?
    pub fn is_empty(&self) -> bool {
        self.z.is_none() && self.m.is_none()
    }
}

/// A geometry with per-coordinate Z/M data stored alongside.
///
/// The `zm` vector has exactly one entry per coordinate in the geometry,
/// in the order defined by `CoordsIter` (depth-first traversal).
#[derive(Clone, Debug)]
pub struct ZmGeometry {
    /// The underlying geometry (repaired or original).
    pub geometry: Geometry<f64>,
    /// Per-coordinate Z/M values. Length must equal the number of
    /// coordinates returned by [`crate::zm::count_coords`].
    pub zm: Vec<ZmValue>,
}

impl ZmGeometry {
    /// Create a new ZmGeometry with no Z/M data (all NONE).
    pub fn new(geometry: Geometry<f64>) -> Self {
        let count = count_coords(&geometry);
        Self {
            geometry,
            zm: vec![ZmValue::NONE; count],
        }
    }

    /// Create a ZmGeometry with explicit Z/M data.
    ///
    /// The `zm` vector must have one entry per coordinate in the
    /// geometry, in depth-first traversal order.
    pub fn with_zm(geometry: Geometry<f64>, zm: Vec<ZmValue>) -> Self {
        Self { geometry, zm }
    }

    /// Consume and return the inner geometry, discarding Z/M.
    pub fn into_geometry(self) -> Geometry<f64> {
        self.geometry
    }

    /// Borrow the inner geometry.
    pub fn geometry(&self) -> &Geometry<f64> {
        &self.geometry
    }

    /// Does any coordinate have a Z (elevation) value?
    pub fn has_z(&self) -> bool {
        self.zm.iter().any(|z| z.z.is_some())
    }

    /// Does any coordinate have an M (measure) value?
    pub fn has_m(&self) -> bool {
        self.zm.iter().any(|z| z.m.is_some())
    }
}

/// Count the total number of coordinates (vertices) in a geometry.
///
/// Returns the count in depth-first traversal order, matching the
/// iteration order of [`geo::CoordsIter`].
pub fn count_coords(geom: &Geometry<f64>) -> usize {
    match geom {
        Geometry::Point(_) => 1,
        Geometry::MultiPoint(mp) => mp.0.len(),
        Geometry::LineString(ls) => ls.0.len(),
        Geometry::MultiLineString(mls) => mls.0.iter().map(|ls| ls.0.len()).sum(),
        Geometry::Polygon(p) => {
            p.exterior().0.len() + p.interiors().iter().map(|h| h.0.len()).sum::<usize>()
        }
        Geometry::MultiPolygon(mp) => mp.0.iter().map(count_polygon_coords).sum(),
        Geometry::Line(_) => 2,
        Geometry::Rect(_) => 5,
        Geometry::Triangle(_) => 4,
        Geometry::GeometryCollection(gc) => gc.0.iter().map(count_coords).sum(),
    }
}

fn count_polygon_coords(p: &geo::Polygon<f64>) -> usize {
    p.exterior().0.len() + p.interiors().iter().map(|h| h.0.len()).sum::<usize>()
}

/// Attempt to preserve Z/M values from an original geometry to a repaired geometry.
///
/// Matches coordinates by position index for same-topology geometries,
/// otherwise falls back to nearest-coordinate matching with tolerance.
pub fn preserve_zm(
    original: &Geometry<f64>,
    original_zm: &[ZmValue],
    repaired: &Geometry<f64>,
) -> Vec<ZmValue> {
    let orig_count = count_coords(original);
    let repaired_count = count_coords(repaired);

    if original_zm.len() != orig_count {
        return vec![ZmValue::NONE; repaired_count];
    }

    if orig_count == repaired_count && topology_unchanged(original, repaired) {
        return original_zm.to_vec();
    }

    match_nearest(original, original_zm, repaired)
}

fn topology_unchanged(a: &Geometry<f64>, b: &Geometry<f64>) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn match_nearest(
    original: &Geometry<f64>,
    original_zm: &[ZmValue],
    repaired: &Geometry<f64>,
) -> Vec<ZmValue> {
    use geo::CoordsIter;
    let orig_coords: Vec<Coord<f64>> = original.coords_iter().collect();
    let tol = 1e-10;
    let mut result = Vec::with_capacity(count_coords(repaired));
    for new_c in repaired.coords_iter() {
        let best = orig_coords
            .iter()
            .zip(original_zm.iter())
            .filter(|(oc, _)| {
                let dx = oc.x - new_c.x;
                let dy = oc.y - new_c.y;
                (dx * dx + dy * dy) < tol * tol
            })
            .min_by(|(a, _), (b, _)| {
                let da = (a.x - new_c.x).hypot(a.y - new_c.y);
                let db = (b.x - new_c.x).hypot(b.y - new_c.y);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, zv)| *zv);
        result.push(best.unwrap_or(ZmValue::NONE));
    }
    result
}

/// Build an iterator over (Coord, ZmValue) pairs in depth-first traversal order.
///
/// Each pair corresponds to one coordinate in the geometry.
pub fn zm_pairs<'a>(
    geom: &'a Geometry<f64>,
    zm: &'a [ZmValue],
) -> impl Iterator<Item = (Coord<f64>, ZmValue)> + 'a {
    use geo::CoordsIter;
    geom.coords_iter().zip(zm.iter().copied())
}

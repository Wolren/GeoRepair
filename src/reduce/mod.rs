//! Geometry precision reduction with topology preservation.
//!
//! Mirrors JTS's `GeometryPrecisionReducer`: reduces all coordinates to a
//! fixed grid, then validates and repairs any topology issues caused by
//! snapping. Supports multiple precision models and automatic retry with
//! progressively coarser grids.

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon, Rect,
};

use crate::core::MakeValidConfig;
use crate::make_valid::MakeValid;
use crate::validation::GeoValidation;

// ---------------------------------------------------------------------------
// Precision model
// ---------------------------------------------------------------------------

/// Controls how coordinates are rounded to a precision grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoundingMode {
    /// Round to nearest grid point (standard).
    ///
    /// `(v / scale).round() * scale` — the usual rounding behaviour.
    Round,
    /// Always floor (for conservative envelope expansions).
    ///
    /// `(v / scale).floor() * scale` — never rounds upward.
    Floor,
    /// Always ceil.
    ///
    /// `(v / scale).ceil() * scale` — never rounds downward.
    Ceil,
}

/// Describes a fixed-precision grid for coordinate reduction.
///
/// `scale` is the grid resolution (e.g., `1e-8` snaps to 8 decimal places).
/// The grid is always origin-centered.
#[derive(Clone, Copy, Debug)]
pub struct PrecisionModel {
    pub scale: f64,
    pub mode: RoundingMode,
}

impl PrecisionModel {
    /// Create a new precision model with the given grid scale.
    ///
    /// Uses [`RoundingMode::Round`] (standard rounding to nearest).
    pub fn new(scale: f64) -> Self {
        Self {
            scale,
            mode: RoundingMode::Round,
        }
    }

    /// Create a precision model with explicit scale and rounding mode.
    pub fn with_mode(scale: f64, mode: RoundingMode) -> Self {
        Self { scale, mode }
    }

    /// Snap a single coordinate to this model's precision grid.
    pub fn reduce_coord(&self, c: Coord<f64>) -> Coord<f64> {
        let round = |v: f64| -> f64 {
            if !v.is_finite() {
                return v;
            }
            match self.mode {
                RoundingMode::Round => (v / self.scale).round() * self.scale,
                RoundingMode::Floor => (v / self.scale).floor() * self.scale,
                RoundingMode::Ceil => (v / self.scale).ceil() * self.scale,
            }
        };
        Coord {
            x: round(c.x),
            y: round(c.y),
        }
    }

    /// Common precision models.
    /// 1e-6 precision (~1 metre at geographic scale).
    pub fn fixed_6() -> Self {
        Self::new(1e-6)
    }
    /// 1e-8 precision (~1 cm at geographic scale). Default for most operations.
    pub fn fixed_8() -> Self {
        Self::new(1e-8)
    }
    /// 1e-10 precision (~0.1 mm at geographic scale).
    pub fn fixed_10() -> Self {
        Self::new(1e-10)
    }
    /// 1e-4 precision (~10 m at geographic scale). Coarsest useful grid.
    pub fn fixed_4() -> Self {
        Self::new(1e-4)
    }
}

// ---------------------------------------------------------------------------
// Geometry precision reducer
// ---------------------------------------------------------------------------

/// Reduces geometry precision with topology preservation.
///
/// Algorithm:
/// 1. Snap all coordinates to the precision grid
/// 2. Validate the result
/// 3. If invalid, run MakeValid to repair topology issues
///
/// The config controls which repair strategy is used for fix-up.
pub struct GeometryPrecisionReducer {
    model: PrecisionModel,
    config: MakeValidConfig,
}

impl GeometryPrecisionReducer {
    /// Create a reducer with the given precision model and default config.
    pub fn new(model: PrecisionModel) -> Self {
        Self {
            model,
            config: MakeValidConfig::default(),
        }
    }

    /// Create a reducer with explicit model and repair config.
    pub fn with_config(model: PrecisionModel, config: MakeValidConfig) -> Self {
        Self { model, config }
    }

    /// Borrow the precision model.
    pub fn model(&self) -> &PrecisionModel {
        &self.model
    }

    /// Reduce precision: snap all coords, validate, fix if needed.
    pub fn reduce(&self, geom: &Geometry<f64>) -> Geometry<f64> {
        let snapped = self.snap_geometry(geom);

        // If result is valid, return it directly
        if snapped.validate().valid {
            return snapped;
        }

        // Repair topology issues caused by snapping
        snapped.make_valid_with_config(&self.config)
    }

    /// Snap all coordinates to the precision grid without calling MakeValid.
    /// Use this inside retry chains to avoid recursive MakeValid calls.
    pub fn reduce_raw(&self, poly: &Polygon<f64>) -> Geometry<f64> {
        let snapped = self.snap_polygon(poly);
        Geometry::Polygon(snapped)
    }

    fn snap_coord(&self, c: Coord<f64>) -> Coord<f64> {
        self.model.reduce_coord(c)
    }

    fn snap_point(&self, p: &Point<f64>) -> Point<f64> {
        Point(self.snap_coord(p.0))
    }

    fn snap_coords(&self, coords: &[Coord<f64>]) -> Vec<Coord<f64>> {
        #[cfg(feature = "simd")]
        if self.model.mode == RoundingMode::Round {
            let mut snapped = coords.to_vec();
            crate::simd::snap_coords_simd(&mut snapped, self.model.scale);
            return snapped;
        }
        coords.iter().map(|&c| self.snap_coord(c)).collect()
    }

    fn snap_linestring(&self, ls: &LineString<f64>) -> LineString<f64> {
        let snapped = self.snap_coords(&ls.0);
        let deduped = crate::noding::remove_consecutive_duplicates(&snapped);
        LineString::new(deduped)
    }

    fn snap_linear_ring(&self, lr: &LineString<f64>) -> LineString<f64> {
        let snapped = self.snap_coords(&lr.0);
        if snapped.len() < 2 {
            return LineString::new(snapped);
        }
        let deduped = crate::noding::remove_consecutive_duplicates(&snapped);
        if deduped.len() < 2 {
            return LineString::new(deduped);
        }
        // Ensure closure
        let mut ring = deduped;
        if ring.len() > 1 && ring.first() != ring.last() {
            ring.push(ring[0]);
        }
        LineString::new(ring)
    }

    fn snap_polygon(&self, poly: &Polygon<f64>) -> Polygon<f64> {
        let exterior = self.snap_linear_ring(poly.exterior());
        let interiors: Vec<LineString<f64>> = poly
            .interiors()
            .iter()
            .map(|h| self.snap_linear_ring(h))
            .collect();
        Polygon::new(exterior, interiors)
    }

    fn snap_geometry(&self, geom: &Geometry<f64>) -> Geometry<f64> {
        match geom {
            Geometry::Point(p) => Geometry::Point(self.snap_point(p)),
            Geometry::Line(l) => {
                let start = self.snap_coord(l.start);
                let end = self.snap_coord(l.end);
                if start == end {
                    return Geometry::LineString(LineString::new(vec![start]));
                }
                Geometry::Line(geo::Line::new(start, end))
            }
            Geometry::LineString(ls) => Geometry::LineString(self.snap_linestring(ls)),
            Geometry::Polygon(p) => Geometry::Polygon(self.snap_polygon(p)),
            Geometry::MultiPoint(mp) => {
                let snapped: Vec<Point<f64>> = mp.0.iter().map(|p| self.snap_point(p)).collect();
                Geometry::MultiPoint(MultiPoint::new(snapped))
            }
            Geometry::MultiLineString(mls) => {
                let snapped: Vec<LineString<f64>> =
                    mls.0.iter().map(|ls| self.snap_linestring(ls)).collect();
                Geometry::MultiLineString(MultiLineString::new(snapped))
            }
            Geometry::MultiPolygon(mp) => {
                let snapped: Vec<Polygon<f64>> =
                    mp.0.iter().map(|p| self.snap_polygon(p)).collect();
                Geometry::MultiPolygon(MultiPolygon::new(snapped))
            }
            Geometry::GeometryCollection(gc) => {
                let snapped: Vec<Geometry<f64>> =
                    gc.0.iter().map(|g| self.snap_geometry(g)).collect();
                Geometry::GeometryCollection(GeometryCollection(snapped))
            }
            Geometry::Rect(r) => {
                let min = self.snap_coord(r.min());
                let max = self.snap_coord(r.max());
                Geometry::Rect(Rect::new(min, max))
            }
            Geometry::Triangle(t) => {
                let snapped = [
                    self.snap_coord(t.v1()),
                    self.snap_coord(t.v2()),
                    self.snap_coord(t.v3()),
                ];
                Geometry::Triangle(geo::Triangle::new(snapped[0], snapped[1], snapped[2]))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Retry chain
// ---------------------------------------------------------------------------

/// Try to reduce precision at progressively coarser grids until success.
///
/// Starts at `1e-10` and degrades by factor 100 each step until `1e-4`.
/// Returns the first valid result, or the last (possibly invalid) result.
pub fn reduce_with_retry(geom: &Geometry<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    let scales = [1e-10, 1e-8, 1e-6, 1e-4];
    for &scale in &scales {
        let model = PrecisionModel::new(scale);
        let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
        let result = reducer.reduce(geom);
        if result.validate().valid {
            return result;
        }
    }
    // Last resort: coarsest grid
    let model = PrecisionModel::new(1e-4);
    let reducer = GeometryPrecisionReducer::with_config(model, config.clone());
    reducer.reduce(geom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(g: &Geometry<f64>) {
        let r = g.validate();
        assert!(r.valid, "expected valid, got: {:?}", r.errors);
    }

    #[test]
    fn test_precision_model_round() {
        let pm = PrecisionModel::new(0.01);
        let c = Coord {
            x: 1.234567,
            y: 9.876543,
        };
        let r = pm.reduce_coord(c);
        assert!((r.x - 1.23).abs() < 1e-10);
        assert!((r.y - 9.88).abs() < 1e-10);
    }

    #[test]
    fn test_precision_model_floor() {
        let pm = PrecisionModel::with_mode(0.01, RoundingMode::Floor);
        let c = Coord {
            x: 1.234567,
            y: 9.876543,
        };
        let r = pm.reduce_coord(c);
        assert!((r.x - 1.23).abs() < 1e-10);
        assert!((r.y - 9.87).abs() < 1e-10);
    }

    #[test]
    fn test_reducer_valid_point() {
        let p = Geometry::Point(geo::point!(x: 1.2345678912, y: 5.00000000004));
        let reducer = GeometryPrecisionReducer::new(PrecisionModel::fixed_8());
        let r = reducer.reduce(&p);
        assert_valid(&r);
    }

    #[test]
    fn test_reducer_valid_polygon() {
        let poly = Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        ));
        let reducer = GeometryPrecisionReducer::new(PrecisionModel::fixed_6());
        let r = reducer.reduce(&poly);
        assert_valid(&r);
    }

    #[test]
    fn test_reducer_fixes_bowtie() {
        let bowtie = Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        ));
        let reducer = GeometryPrecisionReducer::new(PrecisionModel::fixed_10());
        let r = reducer.reduce(&bowtie);
        assert_valid(&r);
    }

    #[test]
    fn test_reduce_with_retry_all_coarsen() {
        let bowtie = Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 1e-9, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1e-9, y: 0.0 },
            ]),
            Vec::new(),
        ));
        let cfg = MakeValidConfig::default();
        let r = reduce_with_retry(&bowtie, &cfg);
        assert_valid(&r);
    }

    #[test]
    fn test_reducer_linestring() {
        let ls = Geometry::LineString(LineString::new(vec![
            Coord {
                x: 1.2345678912,
                y: 2.3456789123,
            },
            Coord {
                x: 3.4567891234,
                y: 4.5678912345,
            },
        ]));
        let reducer = GeometryPrecisionReducer::new(PrecisionModel::fixed_8());
        let r = reducer.reduce(&ls);
        assert_valid(&r);
    }

    #[test]
    fn test_reducer_multipolygon() {
        let p1 = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let mp = Geometry::MultiPolygon(MultiPolygon::new(vec![p1]));
        let reducer = GeometryPrecisionReducer::new(PrecisionModel::fixed_8());
        let r = reducer.reduce(&mp);
        assert_valid(&r);
    }
}

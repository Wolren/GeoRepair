use geo::{
    Coord, CoordNum, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle,
};

use crate::config::{MakeValidConfig, PolyMethod};
use crate::noding::{node_line_string, remove_consecutive_duplicates};

pub trait MakeValid {
    type Scalar: GeoFloat;

    fn make_valid(&self) -> Geometry<Self::Scalar> {
        self.make_valid_with_config(&MakeValidConfig::default())
    }

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<Self::Scalar>;

    #[cfg(feature = "parallel")]
    fn par_make_valid(&self) -> Geometry<Self::Scalar>
    where
        Self: Send + Sync,
    {
        self.par_make_valid_with_config(&MakeValidConfig::default())
    }

    #[cfg(feature = "parallel")]
    fn par_make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<Self::Scalar>
    where
        Self: Send + Sync,
    {
        self.make_valid_with_config(_config)
    }
}

fn empty_geom<T: CoordNum>() -> Geometry<T> {
    Geometry::GeometryCollection(GeometryCollection(Vec::new()))
}

// ---------------------------------------------------------------------------
// Point
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for Point<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let c = self.0;
        if c.x.is_finite() && c.y.is_finite() {
            Geometry::Point(*self)
        } else {
            empty_geom()
        }
    }
}

// ---------------------------------------------------------------------------
// MultiPoint
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for MultiPoint<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let points: Vec<Point<T>> = self
            .0
            .iter()
            .copied()
            .filter(|p| p.0.x.is_finite() && p.0.y.is_finite())
            .collect();
        if points.is_empty() {
            empty_geom()
        } else {
            Geometry::MultiPoint(MultiPoint::new(points))
        }
    }
}

// ---------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for Line<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let ok = self.start.x.is_finite()
            && self.start.y.is_finite()
            && self.end.x.is_finite()
            && self.end.y.is_finite()
            && self.start != self.end;
        if ok {
            Geometry::Line(*self)
        } else {
            empty_geom()
        }
    }
}

// ---------------------------------------------------------------------------
// LineString
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for LineString<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        let coords: Vec<Coord<T>> = self
            .0
            .iter()
            .copied()
            .filter(|c| c.x.is_finite() && c.y.is_finite())
            .collect();
        if coords.is_empty() {
            return empty_geom();
        }
        let deduped = remove_consecutive_duplicates(coords);
        if deduped.len() < 2 {
            if config.keep_collapsed && deduped.len() == 1 {
                return Geometry::Point(Point(deduped[0]));
            }
            return empty_geom();
        }
        node_line_string(&LineString::new(deduped))
    }
}

// ---------------------------------------------------------------------------
// MultiLineString
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for MultiLineString<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        let lines: Vec<LineString<T>> = self
            .0
            .iter()
            .flat_map(|ls| match ls.make_valid_with_config(config) {
                Geometry::LineString(ls) => vec![ls],
                Geometry::MultiLineString(mls) => mls.0,
                _ => Vec::new(),
            })
            .collect();
        if lines.is_empty() {
            empty_geom()
        } else {
            Geometry::MultiLineString(MultiLineString::new(lines))
        }
    }
}

// ---------------------------------------------------------------------------
// Rect
// ---------------------------------------------------------------------------

impl<T: GeoFloat> MakeValid for Rect<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let min_ok = self.min().x.is_finite() && self.min().y.is_finite();
        let max_ok = self.max().x.is_finite() && self.max().y.is_finite();
        if min_ok && max_ok {
            Geometry::Rect(*self)
        } else {
            empty_geom()
        }
    }
}

// ---------------------------------------------------------------------------
// Triangle — concrete f64 when polygon features available
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Triangle<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let coords = [self.v1(), self.v2(), self.v3()];
        for c in coords {
            if !c.x.is_finite() || !c.y.is_finite() {
                return empty_geom();
            }
        }
        let (a, b, c) = (coords[0], coords[1], coords[2]);
        if a == b || b == c || a == c {
            return empty_geom();
        }
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area == 0.0 {
            return empty_geom();
        }
        let poly = Polygon::new(LineString::new(vec![a, b, c, a]), Vec::new());
        poly.make_valid_with_config(config)
    }
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
impl<T: GeoFloat> MakeValid for Triangle<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, _config: &MakeValidConfig) -> Geometry<T> {
        let coords = [self.v1(), self.v2(), self.v3()];
        for c in coords {
            if !c.x.is_finite() || !c.y.is_finite() {
                return empty_geom();
            }
        }
        let (a, b, c) = (coords[0], coords[1], coords[2]);
        if a == b || b == c || a == c {
            return empty_geom();
        }
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area == T::zero() {
            return empty_geom();
        }
        let poly = Polygon::new(LineString::new(vec![a, b, c, a]), Vec::new());
        Geometry::Polygon(poly)
    }
}

// ---------------------------------------------------------------------------
// Polygon — concrete f64 impl
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Polygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        match config.poly_method {
            PolyMethod::Arrange => arrange_or_empty(self, config),
            PolyMethod::Structure => {
                structure_fix(self, config).unwrap_or_else(|| empty_geom::<f64>())
            }
            PolyMethod::Auto => {
                structure_fix(self, config).unwrap_or_else(|| arrange_or_empty(self, config))
            }
        }
    }
}

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for MultiPolygon<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let polys: Vec<Geometry<f64>> = self
            .0
            .iter()
            .map(|p| p.make_valid_with_config(config))
            .collect();

        let mut shells = Vec::new();
        for g in polys {
            match g {
                Geometry::Polygon(p) => shells.push(p),
                Geometry::MultiPolygon(mp) => shells.extend(mp.0),
                _ => {}
            }
        }
        if shells.is_empty() {
            return empty_geom::<f64>();
        }
        if shells.len() == 1 {
            return Geometry::Polygon(shells.into_iter().next().unwrap());
        }
        let mp = MultiPolygon::new(shells);
        Geometry::MultiPolygon(geo::algorithm::bool_ops::unary_union(&mp))
    }
}

// ---------------------------------------------------------------------------
// Geometry + GeometryCollection
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for Geometry<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        match self {
            Geometry::Point(g) => g.make_valid_with_config(config),
            Geometry::Line(g) => g.make_valid_with_config(config),
            Geometry::LineString(g) => g.make_valid_with_config(config),
            Geometry::Polygon(g) => g.make_valid_with_config(config),
            Geometry::MultiPoint(g) => g.make_valid_with_config(config),
            Geometry::MultiLineString(g) => g.make_valid_with_config(config),
            Geometry::MultiPolygon(g) => g.make_valid_with_config(config),
            Geometry::GeometryCollection(g) => g.make_valid_with_config(config),
            Geometry::Rect(g) => g.make_valid_with_config(config),
            Geometry::Triangle(g) => g.make_valid_with_config(config),
        }
    }
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
impl<T: GeoFloat> MakeValid for Geometry<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        match self {
            Geometry::Point(g) => g.make_valid_with_config(config),
            Geometry::Line(g) => g.make_valid_with_config(config),
            Geometry::LineString(g) => g.make_valid_with_config(config),
            Geometry::Polygon(_) | Geometry::MultiPolygon(_) => empty_geom(),
            Geometry::MultiPoint(g) => g.make_valid_with_config(config),
            Geometry::MultiLineString(g) => g.make_valid_with_config(config),
            Geometry::GeometryCollection(g) => g.make_valid_with_config(config),
            Geometry::Rect(g) => g.make_valid_with_config(config),
            Geometry::Triangle(g) => g.make_valid_with_config(config),
        }
    }
}

// Helper functions for polygon dispatch

#[cfg(feature = "arrange")]
fn arrange_or_empty(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    crate::arrange::fix_polygon(poly, config)
}

#[cfg(not(feature = "arrange"))]
fn arrange_or_empty(_poly: &Polygon<f64>, _config: &MakeValidConfig) -> Geometry<f64> {
    empty_geom::<f64>()
}

#[cfg(feature = "structure")]
fn structure_fix(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    crate::structure::fix_polygon(poly, config)
}

#[cfg(not(feature = "structure"))]
fn structure_fix(_poly: &Polygon<f64>, _config: &MakeValidConfig) -> Option<Geometry<f64>> {
    None
}

// ---------------------------------------------------------------------------
// GeometryCollection
// ---------------------------------------------------------------------------

#[cfg(any(feature = "arrange", feature = "structure"))]
impl MakeValid for GeometryCollection<f64> {
    type Scalar = f64;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<f64> {
        let fixed: Vec<Geometry<f64>> = self
            .0
            .iter()
            .map(|g| g.make_valid_with_config(config))
            .filter(|g| !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()))
            .collect();
        if fixed.is_empty() {
            empty_geom::<f64>()
        } else {
            Geometry::GeometryCollection(GeometryCollection(fixed))
        }
    }
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
impl<T: GeoFloat> MakeValid for GeometryCollection<T> {
    type Scalar = T;

    fn make_valid_with_config(&self, config: &MakeValidConfig) -> Geometry<T> {
        let fixed: Vec<Geometry<T>> = self
            .0
            .iter()
            .map(|g| g.make_valid_with_config(config))
            .filter(|g| !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()))
            .collect();
        if fixed.is_empty() {
            empty_geom()
        } else {
            Geometry::GeometryCollection(GeometryCollection(fixed))
        }
    }
}

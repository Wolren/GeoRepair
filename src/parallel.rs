use geo::{GeoFloat, Geometry, GeometryCollection, MultiLineString, MultiPoint, MultiPolygon};
use rayon::prelude::*;

use crate::config::MakeValidConfig;
use crate::make_valid::MakeValid;

pub fn par_fix_multi_point<T: GeoFloat + Send + Sync>(
    mp: &MultiPoint<T>,
    config: &MakeValidConfig,
) -> Geometry<T> {
    let points =
        mp.0.par_iter()
            .copied()
            .map(|p| p.make_valid_with_config(config))
            .filter_map(|g| {
                if let Geometry::Point(p) = g {
                    Some(p)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
    if points.is_empty() {
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    } else {
        Geometry::MultiPoint(MultiPoint::new(points))
    }
}

pub fn par_fix_multi_line_string<T: GeoFloat + Send + Sync>(
    mls: &MultiLineString<T>,
    config: &MakeValidConfig,
) -> Geometry<T> {
    let lines = mls
        .0
        .par_iter()
        .map(|ls| ls.make_valid_with_config(config))
        .filter_map(|g| {
            if let Geometry::LineString(ls) = g {
                Some(ls)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    } else {
        Geometry::MultiLineString(MultiLineString::new(lines))
    }
}

#[cfg(any(feature = "arrange", feature = "structure"))]
pub fn par_fix_multi_polygon(mp: &MultiPolygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    let polys: Vec<Geometry<f64>> =
        mp.0.par_iter()
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
        return Geometry::GeometryCollection(GeometryCollection(Vec::new()));
    }
    if shells.len() == 1 {
        return Geometry::Polygon(shells.into_iter().next().unwrap());
    }
    let mp = MultiPolygon::new(shells);
    Geometry::MultiPolygon(geo::algorithm::bool_ops::unary_union(&mp))
}

#[cfg(not(any(feature = "arrange", feature = "structure")))]
pub fn par_fix_collection<T: GeoFloat + Send + Sync>(
    gc: &GeometryCollection<T>,
    config: &MakeValidConfig,
) -> Geometry<T> {
    let fixed =
        gc.0.par_iter()
            .map(|g| g.make_valid_with_config(config))
            .filter(|g| !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()))
            .collect::<Vec<_>>();
    if fixed.is_empty() {
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    } else {
        Geometry::GeometryCollection(GeometryCollection(fixed))
    }
}

#[cfg(any(feature = "arrange", feature = "structure"))]
pub fn par_fix_collection(gc: &GeometryCollection<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    let fixed =
        gc.0.par_iter()
            .map(|g| g.make_valid_with_config(config))
            .filter(|g| !matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()))
            .collect::<Vec<_>>();
    if fixed.is_empty() {
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    } else {
        Geometry::GeometryCollection(GeometryCollection(fixed))
    }
}

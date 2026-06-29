use geo::{
    GeoFloat, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use rayon::prelude::*;

use crate::core::MakeValidConfig;
use crate::make_valid::MakeValid;
use crate::noding::NodingFloat;

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

pub fn par_fix_multi_line_string<T: NodingFloat + Send + Sync>(
    mls: &MultiLineString<T>,
    config: &MakeValidConfig,
) -> Geometry<T> {
    let mut points: Vec<Point<T>> = Vec::new();
    let mut lines: Vec<LineString<T>> = Vec::new();
    for g in mls
        .0
        .par_iter()
        .map(|ls| ls.make_valid_with_config(config))
        .collect::<Vec<_>>()
    {
        match g {
            Geometry::Point(p) => points.push(p),
            Geometry::LineString(l) => lines.push(l),
            Geometry::MultiLineString(mls) => lines.extend(mls.0),
            _ => {}
        }
    }
    match (points.len(), lines.len()) {
        (0, 0) => Geometry::GeometryCollection(GeometryCollection(Vec::new())),
        (_, 0) => {
            if points.len() == 1 {
                Geometry::Point(points.pop().expect("len==1 verified"))
            } else {
                Geometry::MultiPoint(MultiPoint::new(points))
            }
        }
        (0, _) => {
            if lines.len() == 1 {
                Geometry::LineString(lines.pop().expect("len==1 verified"))
            } else {
                Geometry::MultiLineString(MultiLineString::new(lines))
            }
        }
        _ => {
            let mut geoms: Vec<Geometry<T>> = lines.into_iter().map(Geometry::LineString).collect();
            if points.len() == 1 {
                geoms.push(Geometry::Point(points.pop().expect("len==1 verified")));
            } else {
                geoms.push(Geometry::MultiPoint(MultiPoint::new(points)));
            }
            Geometry::GeometryCollection(GeometryCollection(geoms))
        }
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
        // Safe: len==1 verified above on local Vec
        return Geometry::Polygon(shells.pop().expect("len==1 verified"));
    }
    let mp = MultiPolygon::new(shells);
    Geometry::MultiPolygon(geo::algorithm::bool_ops::unary_union(&mp))
}

/// Process a batch of independent polygons in parallel.
/// Each polygon is fixed independently with no shared state — near-linear scaling.
/// Returns the results in the same order as input.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub fn par_fix_polygon_batch(
    polys: &[&Polygon<f64>],
    config: &MakeValidConfig,
) -> Vec<Geometry<f64>> {
    polys
        .par_iter()
        .map(|p| (*p).make_valid_with_config(config))
        .collect()
}

/// Process polygons from an iterator in chunks — peak memory is bounded by
/// `chunk_size` polygons. Useful for streaming large datasets without loading
/// everything into memory at once. Results preserve input order.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub fn par_fix_polygon_batch_chunked<I>(
    iter: I,
    chunk_size: usize,
    config: &MakeValidConfig,
) -> Vec<Geometry<f64>>
where
    I: Iterator<Item = Polygon<f64>>,
{
    let chunk_size = chunk_size.max(1);
    let mut results = Vec::new();
    let mut chunk: Vec<Polygon<f64>> = Vec::with_capacity(chunk_size);
    for poly in iter {
        chunk.push(poly);
        if chunk.len() == chunk_size {
            let refs: Vec<&Polygon<f64>> = chunk.iter().collect();
            results.extend(par_fix_polygon_batch(&refs, config));
            chunk.clear();
        }
    }
    if !chunk.is_empty() {
        let refs: Vec<&Polygon<f64>> = chunk.iter().collect();
        results.extend(par_fix_polygon_batch(&refs, config));
    }
    results
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

#[cfg(feature = "load-shp")]
use geo::LineString;
use geo::{Coord, Geometry, Polygon};

#[cfg(feature = "load-shp")]
use crate::feature::Feature;
#[cfg(feature = "load-shp")]
use crate::Crs;
#[cfg(feature = "load-shp")]
#[cfg(feature = "load-shp")]
use geo::MultiPolygon;
#[cfg(feature = "load-shp")]
use serde_json::Value;
#[cfg(feature = "load-shp")]
use shapefile::dbase;
#[cfg(feature = "load-shp")]
use std::path::Path;

pub fn signed_area(ring: &[Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}

pub fn polygon_area(p: &Polygon<f64>) -> f64 {
    signed_area(&p.exterior().0).abs()
}

pub fn geo_area(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => polygon_area(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon_area).sum(),
        _ => 0.0,
    }
}

pub fn count_sub_polys(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::Polygon(_) => 1,
        Geometry::MultiPolygon(mp) => mp.0.len(),
        _ => 0,
    }
}

/// Load polygons from a shapefile.
///
/// Reads only Polygon shapes; Point and PolyLine shapes are skipped.
/// Returns polygons constructed from the shapefile rings using
/// signed-area winding direction to distinguish exterior vs holes.
#[cfg(feature = "load-shp")]
pub fn load_shp(path: &str) -> Result<Vec<Polygon<f64>>, shapefile::Error> {
    let mut reader = shapefile::Reader::from_path(path)?;
    let mut all_rings: Vec<Vec<Coord<f64>>> = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, _) = match result {
            Ok(v) => v,
            Err(shapefile::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::WriteZero =>
            {
                break;
            }
            Err(_) => continue,
        };
        if let shapefile::Shape::Polygon(poly) = shape {
            for r in poly.rings() {
                let coords: Vec<Coord<f64>> = r
                    .clone()
                    .into_inner()
                    .into_iter()
                    .map(|p| Coord { x: p.x, y: p.y })
                    .collect();
                all_rings.push(coords);
            }
        }
    }

    Ok(assemble_polygons(all_rings))
}

/// Load all geometry types from a shapefile.
///
/// Returns points, polylines, and polygons as appropriate Geometry variants.
#[cfg(feature = "load-shp")]
pub fn load_shp_geometries(path: &str) -> Result<Vec<Geometry<f64>>, shapefile::Error> {
    let mut reader = shapefile::Reader::from_path(path)?;
    let mut geoms = Vec::new();

    for result in reader.iter_shapes_and_records() {
        let (shape, _) = match result {
            Ok(v) => v,
            Err(shapefile::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::WriteZero =>
            {
                break;
            }
            Err(_) => continue,
        };
        match shape {
            shapefile::Shape::Point(p) => {
                geoms.push(Geometry::Point(geo::Point::new(p.x, p.y)));
            }
            shapefile::Shape::PointM(p) => {
                geoms.push(Geometry::Point(geo::Point::new(p.x, p.y)));
            }
            shapefile::Shape::PointZ(p) => {
                geoms.push(Geometry::Point(geo::Point::new(p.x, p.y)));
            }
            shapefile::Shape::Polyline(pl) => {
                let parts: Vec<LineString<f64>> = pl
                    .parts()
                    .iter()
                    .map(|part| {
                        let coords: Vec<Coord<f64>> =
                            part.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
                        LineString::new(coords)
                    })
                    .collect();
                if parts.len() == 1 {
                    geoms.push(Geometry::LineString(parts.into_iter().next().expect("len==1 verified")));
                } else {
                    geoms.push(Geometry::MultiLineString(geo::MultiLineString::new(parts)));
                }
            }
            shapefile::Shape::Polygon(poly) => {
                let rings: Vec<Vec<Coord<f64>>> = poly
                    .rings()
                    .iter()
                    .map(|r| {
                        r.clone()
                            .into_inner()
                            .into_iter()
                            .map(|p| Coord { x: p.x, y: p.y })
                            .collect()
                    })
                    .collect();
                for poly in assemble_polygons(rings) {
                    geoms.push(Geometry::Polygon(poly));
                }
            }
            shapefile::Shape::NullShape => {}
            shapefile::Shape::Multipoint(mp) => {
                let points: Vec<geo::Point<f64>> = mp
                    .points()
                    .iter()
                    .map(|p| geo::Point::new(p.x, p.y))
                    .collect();
                geoms.push(Geometry::MultiPoint(geo::MultiPoint::new(points)));
            }
            _ => {} // skip other types (MultiPatch, etc.)
        }
    }

    Ok(geoms)
}

/// Load features (geometry + attributes) from a shapefile.
///
/// Reads all shape types, `.dbf` attributes, and `.prj` CRS sidecar.
#[cfg(feature = "load-shp")]
fn dbf_record_count(shp_path: &Path) -> Option<usize> {
    use std::io::Read;
    let dbf_path = shp_path.with_extension("dbf");
    let mut f = std::fs::File::open(&dbf_path).ok()?;
    let mut header = [0u8; 8];
    f.read_exact(&mut header).ok()?;
    let count = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    Some(count as usize)
}

#[cfg(feature = "load-shp")]
pub fn load_shp_features(
    path: &str,
    progress: Option<&dyn Fn(f64)>,
) -> Result<Vec<Feature>, shapefile::Error> {
    use serde_json::Map;

    let path = Path::new(path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");

    let crs = load_shp_crs(&dir.join(format!("{stem}.prj")));

    let mut reader = match shapefile::Reader::from_path(path) {
        Ok(r) => r,
        Err(e) => {
            return Err(e);
        }
    };
    let mut features = Vec::new();
    let estimated_count = dbf_record_count(path);

    for (i, result) in reader.iter_shapes_and_records().enumerate() {
        let (shape, record) = match result {
            Ok(v) => v,
            Err(shapefile::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::WriteZero =>
            {
                // Truncated / partial file — return what we have
                break;
            }
            Err(e) => return Err(e),
        };
        let geometry = convert_shape_to_geometry(shape);
        if let Some(geom) = geometry {
            let properties: Map<String, Value> = record
                .into_iter()
                .map(|(name, val)| (name, field_value_to_json(val)))
                .filter(|(_, v)| !v.is_null())
                .collect();
            let props = if properties.is_empty() {
                None
            } else {
                Some(properties)
            };
            features.push(Feature::with_all(geom, props, crs.clone(), Vec::new()));
        }
        if let Some(ref cb) = progress
            && let Some(total) = estimated_count
                && total > 0 && i % 100 == 0 {
                    cb((i as f64 / total as f64) * 100.0);
                }
    }

    Ok(features)
}

#[cfg(feature = "load-shp")]
fn convert_shape_to_geometry(shape: shapefile::Shape) -> Option<Geometry<f64>> {
    Some(match shape {
        shapefile::Shape::Point(p) => Geometry::Point(geo::Point::new(p.x, p.y)),
        shapefile::Shape::PointM(p) => Geometry::Point(geo::Point::new(p.x, p.y)),
        shapefile::Shape::PointZ(p) => Geometry::Point(geo::Point::new(p.x, p.y)),
        shapefile::Shape::Polyline(pl) => {
            let parts: Vec<LineString<f64>> = pl
                .parts()
                .iter()
                .map(|part| {
                    let coords: Vec<Coord<f64>> =
                        part.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
                    LineString::new(coords)
                })
                .collect();
            if parts.len() == 1 {
                Geometry::LineString(parts.into_iter().next().expect("len==1 verified"))
            } else {
                Geometry::MultiLineString(geo::MultiLineString::new(parts))
            }
        }
        shapefile::Shape::Polygon(poly) => {
            let rings: Vec<Vec<Coord<f64>>> = poly
                .rings()
                .iter()
                .map(|r| {
                    r.clone()
                        .into_inner()
                        .into_iter()
                        .map(|p| Coord { x: p.x, y: p.y })
                        .collect()
                })
                .collect();
            let assembled = assemble_polygons(rings);
            if assembled.len() == 1 {
                Geometry::Polygon(assembled.into_iter().next().expect("len==1 verified"))
            } else {
                Geometry::MultiPolygon(geo::MultiPolygon::new(assembled))
            }
        }
        shapefile::Shape::Multipoint(mp) => {
            let pts: Vec<geo::Point<f64>> = mp
                .points()
                .iter()
                .map(|p| geo::Point::new(p.x, p.y))
                .collect();
            Geometry::MultiPoint(geo::MultiPoint::new(pts))
        }
        shapefile::Shape::NullShape => return None,
        _ => return None,
    })
}

#[cfg(feature = "load-shp")]
fn field_value_to_json(val: dbase::FieldValue) -> serde_json::Value {
    use serde_json::{Number, Value};
    match val {
        dbase::FieldValue::Character(Some(s)) => Value::String(s),
        dbase::FieldValue::Numeric(Some(f)) => {
            Number::from_f64(f).map_or(Value::Null, Value::Number)
        }
        dbase::FieldValue::Float(Some(f)) => {
            Number::from_f64(f as f64).map_or(Value::Null, Value::Number)
        }
        dbase::FieldValue::Integer(i) => Value::Number(Number::from(i)),
        dbase::FieldValue::Double(f) => Number::from_f64(f).map_or(Value::Null, Value::Number),
        dbase::FieldValue::Logical(Some(b)) => Value::Bool(b),
        dbase::FieldValue::Date(Some(d)) => Value::String(format!("{}", d)),
        dbase::FieldValue::Currency(f) => Number::from_f64(f).map_or(Value::Null, Value::Number),
        dbase::FieldValue::Memo(s) => Value::String(s),
        _ => Value::Null,
    }
}

#[cfg(feature = "load-shp")]
fn load_shp_crs(prj_path: &Path) -> Option<Crs> {
    let content = std::fs::read_to_string(prj_path).ok()?;
    Crs::from_prj_wkt(&content)
}

#[cfg(feature = "load-shp")]
fn assemble_polygons(all_rings: Vec<Vec<Coord<f64>>>) -> Vec<Polygon<f64>> {
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    let first_idx = all_rings.iter().position(|r| signed_area(r).abs() > 1e-12);
    if let Some(first) = first_idx {
        let ref_area = signed_area(&all_rings[first]);
        let mut cur_ext: Option<Vec<Coord<f64>>> = None;
        let mut cur_holes: Vec<Vec<Coord<f64>>> = Vec::new();
        for (i, ring) in all_rings.into_iter().enumerate() {
            if signed_area(&ring).abs() < 1e-12 {
                continue;
            }
            if i == first || cur_ext.is_none() {
                if let Some(ext) = cur_ext.take() {
                    polys.push(Polygon::new(
                        LineString::new(ext),
                        cur_holes.drain(..).map(LineString::new).collect(),
                    ));
                }
                cur_ext = Some(ring);
            } else {
                if signed_area(&ring) * ref_area > 0.0 {
                    if let Some(ext) = cur_ext.take() {
                        polys.push(Polygon::new(
                            LineString::new(ext),
                            cur_holes.drain(..).map(LineString::new).collect(),
                        ));
                    }
                    cur_ext = Some(ring);
                } else {
                    cur_holes.push(ring);
                }
            }
        }
        if let Some(ext) = cur_ext.take() {
            polys.push(Polygon::new(
                LineString::new(ext),
                cur_holes.drain(..).map(LineString::new).collect(),
            ));
        }
    }
    // Normalize winding: exteriors CCW
    for p in &mut polys {
        if signed_area(&p.exterior().0) < 0.0 {
            p.exterior_mut(|ls| ls.0.reverse());
        }
    }
    polys
}

/// Streaming shapefile polygon reader.
#[cfg(feature = "load-shp")]
#[allow(dead_code)]
pub fn load_shp_stream(path: &str) -> Result<impl Iterator<Item = Polygon<f64>>, shapefile::Error> {
    let reader = shapefile::Reader::from_path(path)?;
    Ok(LoadShpStream {
        reader,
        buf: Vec::new(),
    })
}

#[cfg(feature = "load-shp")]
#[allow(dead_code)]
struct LoadShpStream {
    reader: shapefile::Reader<std::io::BufReader<std::fs::File>, std::io::BufReader<std::fs::File>>,
    buf: Vec<Polygon<f64>>,
}

#[cfg(feature = "load-shp")]
impl Iterator for LoadShpStream {
    type Item = Polygon<f64>;

    fn next(&mut self) -> Option<Polygon<f64>> {
        if let Some(p) = self.buf.pop() {
            return Some(p);
        }
        loop {
            match self.reader.iter_shapes_and_records().next() {
                None => return None,
                Some(Ok((shape, _))) => {
                    if let shapefile::Shape::Polygon(poly) = shape {
                        let mp: Result<MultiPolygon<f64>, _> = poly.try_into();
                        if let Ok(mp) = mp {
                            let mut members = mp.0;
                            if let Some(first) = members.pop() {
                                self.buf = members;
                                return Some(first);
                            }
                        }
                    }
                }
                Some(Err(_)) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_area_square() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!((signed_area(&ring) - 100.0).abs() < 1e-12);
    }

    #[test]
    fn test_polygon_area() {
        let ext = geo::LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let p = Polygon::new(ext, vec![]);
        assert!((polygon_area(&p) - 100.0).abs() < 1e-12);
    }
}

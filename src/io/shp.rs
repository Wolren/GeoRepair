use std::fs::File;
use std::io::{BufWriter, Write};

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
use shapefile::dbase::FieldValue;
#[cfg(feature = "load-shp")]
use std::path::Path;

/// Signed area of a closed ring (positive = CCW).
pub fn signed_area(ring: &[Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}

/// Area of a polygon (absolute value).
pub fn polygon_area(p: &Polygon<f64>) -> f64 {
    signed_area(&p.exterior().0).abs()
}

/// Area of a geometry (Polygon or MultiPolygon sum).
pub fn geo_area(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => polygon_area(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon_area).sum(),
        _ => 0.0,
    }
}

/// Count sub-polygons in a geometry (1 for Polygon, N for MultiPolygon).
pub fn count_sub_polys(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::Polygon(_) => 1,
        Geometry::MultiPolygon(mp) => mp.0.len(),
        _ => 0,
    }
}

/// Load polygons from a shapefile (Polygon shapes only).
#[cfg(feature = "load-shp")]
pub fn load_shp(path: &str) -> Result<Vec<Polygon<f64>>, shapefile::Error> {
    let mut reader = shapefile::Reader::from_path(path)?;
    let mut all_rings: Vec<Vec<Coord<f64>>> = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let Ok((shape, _)) = result else { continue };
        match shape {
            shapefile::Shape::Polygon(poly) => {
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
            _ => {}
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
        let Ok((shape, _)) = result else { continue };
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
                    geoms.push(Geometry::LineString(parts.into_iter().next().unwrap()));
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
pub fn load_shp_features(path: &str) -> Result<Vec<Feature>, shapefile::Error> {
    use serde_json::Map;

    let path = Path::new(path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");

    let crs = load_shp_crs(&dir.join(format!("{stem}.prj")));

    let mut reader = shapefile::Reader::from_path(path)?;
    let mut features = Vec::new();

    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result?;
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
                Geometry::LineString(parts.into_iter().next().unwrap())
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
                Geometry::Polygon(assembled.into_iter().next().unwrap())
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
pub fn load_shp_stream(path: &str) -> Result<impl Iterator<Item = Polygon<f64>>, shapefile::Error> {
    let reader = shapefile::Reader::from_path(path)?;
    Ok(LoadShpStream {
        reader,
        buf: Vec::new(),
    })
}

#[cfg(feature = "load-shp")]
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

/// Export geometries to a shapefile.
///
/// Writes a `.shp` file with corresponding `.dbf` and (optionally) `.prj`.
#[cfg(feature = "load-shp")]
pub fn export_shp(geoms: &[Geometry<f64>], path: &str, crs: Option<&Crs>) -> std::io::Result<()> {
    use shapefile::{Point, PolygonRing};

    let path = Path::new(path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let shp_path = dir.join(format!("{stem}.shp"));

    let mut writer = shapefile::Writer::from_path(&shp_path, dbase::TableWriterBuilder::new())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for geom in geoms {
        match geom {
            Geometry::Point(p) => {
                writer
                    .write_shape_and_record(&Point::new(p.x(), p.y()), &dbase::Record::default())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::MultiPoint(mp) => {
                for p in &mp.0 {
                    writer
                        .write_shape_and_record(
                            &Point::new(p.x(), p.y()),
                            &dbase::Record::default(),
                        )
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
            }
            Geometry::Line(ln) => {
                let shape = shapefile::Polyline::new(vec![
                    Point::new(ln.start.x, ln.start.y),
                    Point::new(ln.end.x, ln.end.y),
                ]);
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::LineString(ls) => {
                let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                let shape = shapefile::Polyline::new(pts);
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::MultiLineString(mls) => {
                for ls in &mls.0 {
                    let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                    let shape = shapefile::Polyline::new(pts);
                    writer
                        .write_shape_and_record(&shape, &dbase::Record::default())
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
            }
            Geometry::Polygon(poly) => {
                let mut rings: Vec<PolygonRing<Point>> = Vec::new();
                let ext: Vec<Point> = poly
                    .exterior()
                    .0
                    .iter()
                    .map(|c| Point::new(c.x, c.y))
                    .collect();
                rings.push(PolygonRing::Outer(ext));
                for h in poly.interiors() {
                    let pts: Vec<Point> = h.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                    rings.push(PolygonRing::Inner(pts));
                }
                let shape = shapefile::Polygon::with_rings(rings);
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::MultiPolygon(mp) => {
                for poly in &mp.0 {
                    let mut rings: Vec<PolygonRing<Point>> = Vec::new();
                    let ext: Vec<Point> = poly
                        .exterior()
                        .0
                        .iter()
                        .map(|c| Point::new(c.x, c.y))
                        .collect();
                    rings.push(PolygonRing::Outer(ext));
                    for h in poly.interiors() {
                        let pts: Vec<Point> = h.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                        rings.push(PolygonRing::Inner(pts));
                    }
                    let shape = shapefile::Polygon::with_rings(rings);
                    writer
                        .write_shape_and_record(&shape, &dbase::Record::default())
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
            }
            Geometry::Rect(r) => {
                let pts = vec![
                    Point::new(r.min().x, r.min().y),
                    Point::new(r.max().x, r.min().y),
                    Point::new(r.max().x, r.max().y),
                    Point::new(r.min().x, r.max().y),
                    Point::new(r.min().x, r.min().y),
                ];
                let shape = shapefile::Polygon::new(PolygonRing::Outer(pts));
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::Triangle(t) => {
                let pts = vec![
                    Point::new(t.v1().x, t.v1().y),
                    Point::new(t.v2().x, t.v2().y),
                    Point::new(t.v3().x, t.v3().y),
                    Point::new(t.v1().x, t.v1().y),
                ];
                let shape = shapefile::Polygon::new(PolygonRing::Outer(pts));
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::GeometryCollection(gc) => {
                let sub: Vec<Geometry<f64>> = gc.0.clone();
                export_shp(&sub, path.to_str().unwrap_or("export.shp"), crs)?;
            }
        }
    }

    drop(writer);

    // Write .prj sidecar file if CRS is available
    if let Some(crs) = crs {
        if let Some(prj_wkt) = crs.to_esri_wkt() {
            let prj_path = dir.join(format!("{stem}.prj"));
            std::fs::write(&prj_path, prj_wkt.as_bytes())?;
        }
    }

    Ok(())
}

/// Export features (geometry + attributes) to a shapefile.
///
/// Writes `.shp`, `.dbf` with attribute columns, and optionally `.prj`.
#[cfg(feature = "load-shp")]
pub fn export_shp_features(
    features: &[Feature],
    path: &str,
    crs: Option<&Crs>,
) -> std::io::Result<()> {
    use std::collections::BTreeMap;
    use std::convert::TryFrom;

    use dbase::{FieldName, Record, TableWriterBuilder};
    use shapefile::{Point, PolygonRing};

    // Collect all unique property keys and infer types
    // DBF field names are limited to 10 chars — longer names get truncated
    let mut schema: Vec<(String, DbfFieldType)> = Vec::new();
    let mut seen = BTreeMap::<String, DbfFieldType>::new();
    for f in features {
        if let Some(ref props) = f.properties {
            for (key, val) in props {
                let dbf_name = key.chars().take(10).collect::<String>();
                if !seen.contains_key(&dbf_name) {
                    seen.insert(dbf_name, infer_dbf_type(val));
                }
            }
        }
    }
    for (name, ft) in &seen {
        schema.push((name.clone(), ft.clone()));
    }

    let path = Path::new(path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let shp_path = dir.join(format!("{stem}.shp"));

    let mut builder = TableWriterBuilder::new();
    for (name, ft) in &schema {
        let fname = FieldName::try_from(name.as_str())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        match ft {
            DbfFieldType::String => {
                builder = builder.add_character_field(fname, 254);
            }
            DbfFieldType::Numeric => {
                builder = builder.add_numeric_field(fname, 20, 10);
            }
            DbfFieldType::Integer => {
                builder = builder.add_numeric_field(fname, 10, 0);
            }
            DbfFieldType::Logical => {
                builder = builder.add_logical_field(fname);
            }
        }
    }

    let mut writer = shapefile::Writer::from_path(&shp_path, builder)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for feature in features {
        let mut record = Record::default();
        if let Some(ref props) = feature.properties {
            for (name, _) in &schema {
                let val = props.get(name);
                record.insert(name.clone(), to_dbf_field(val));
            }
        }

        match &feature.geometry {
            Geometry::Point(p) => {
                writer
                    .write_shape_and_record(&Point::new(p.x(), p.y()), &record)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::MultiPoint(mp) => {
                for p in &mp.0 {
                    writer
                        .write_shape_and_record(&Point::new(p.x(), p.y()), &record)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
            }
            Geometry::Line(ln) => {
                let pl = shapefile::Polyline::new(vec![
                    Point::new(ln.start.x, ln.start.y),
                    Point::new(ln.end.x, ln.end.y),
                ]);
                writer
                    .write_shape_and_record(&pl, &record)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::LineString(ls) => {
                let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                let pl = shapefile::Polyline::new(pts);
                writer
                    .write_shape_and_record(&pl, &record)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::MultiLineString(mls) => {
                for ls in &mls.0 {
                    let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                    let pl = shapefile::Polyline::new(pts);
                    writer
                        .write_shape_and_record(&pl, &record)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
            }
            Geometry::Polygon(poly) => {
                let mut rings: Vec<PolygonRing<Point>> = Vec::new();
                rings.push(PolygonRing::Outer(
                    poly.exterior()
                        .0
                        .iter()
                        .map(|c| Point::new(c.x, c.y))
                        .collect(),
                ));
                for h in poly.interiors() {
                    rings.push(PolygonRing::Inner(
                        h.0.iter().map(|c| Point::new(c.x, c.y)).collect(),
                    ));
                }
                let shp = shapefile::Polygon::with_rings(rings);
                writer
                    .write_shape_and_record(&shp, &record)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::MultiPolygon(mp) => {
                for poly in &mp.0 {
                    let mut rings: Vec<PolygonRing<Point>> = Vec::new();
                    rings.push(PolygonRing::Outer(
                        poly.exterior()
                            .0
                            .iter()
                            .map(|c| Point::new(c.x, c.y))
                            .collect(),
                    ));
                    for h in poly.interiors() {
                        rings.push(PolygonRing::Inner(
                            h.0.iter().map(|c| Point::new(c.x, c.y)).collect(),
                        ));
                    }
                    let shp = shapefile::Polygon::with_rings(rings);
                    writer
                        .write_shape_and_record(&shp, &record)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
            }
            Geometry::Rect(r) => {
                let pts = vec![
                    Point::new(r.min().x, r.min().y),
                    Point::new(r.max().x, r.min().y),
                    Point::new(r.max().x, r.max().y),
                    Point::new(r.min().x, r.max().y),
                    Point::new(r.min().x, r.min().y),
                ];
                let shp = shapefile::Polygon::new(PolygonRing::Outer(pts));
                writer
                    .write_shape_and_record(&shp, &record)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::Triangle(t) => {
                let pts = vec![
                    Point::new(t.v1().x, t.v1().y),
                    Point::new(t.v2().x, t.v2().y),
                    Point::new(t.v3().x, t.v3().y),
                    Point::new(t.v1().x, t.v1().y),
                ];
                let shp = shapefile::Polygon::new(PolygonRing::Outer(pts));
                writer
                    .write_shape_and_record(&shp, &record)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            }
            Geometry::GeometryCollection(_) => {}
        }
    }

    drop(writer);

    if let Some(crs) = crs {
        if let Some(prj_wkt) = crs.to_esri_wkt() {
            let prj_path = dir.join(format!("{stem}.prj"));
            std::fs::write(&prj_path, prj_wkt.as_bytes())?;
        }
    }

    Ok(())
}

#[cfg(feature = "load-shp")]
#[derive(Clone)]
enum DbfFieldType {
    String,
    Numeric,
    Integer,
    Logical,
}

#[cfg(feature = "load-shp")]
fn infer_dbf_type(val: &serde_json::Value) -> DbfFieldType {
    match val {
        Value::String(_) => DbfFieldType::String,
        Value::Number(n) => {
            if n.is_f64() || n.as_f64().map_or(false, |f| f.fract() != 0.0) {
                DbfFieldType::Numeric
            } else {
                DbfFieldType::Integer
            }
        }
        Value::Bool(_) => DbfFieldType::Logical,
        _ => DbfFieldType::String,
    }
}

#[cfg(feature = "load-shp")]
fn to_dbf_field(val: Option<&serde_json::Value>) -> FieldValue {
    match val {
        Some(Value::String(s)) => FieldValue::Character(Some(s.clone())),
        Some(Value::Number(n)) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() {
                    FieldValue::Numeric(Some(f))
                } else {
                    FieldValue::Float(Some(f as f32))
                }
            } else {
                FieldValue::Character(None)
            }
        }
        Some(Value::Bool(b)) => FieldValue::Logical(Some(*b)),
        _ => FieldValue::Character(None),
    }
}

// ---------------------------------------------------------------------------
// GeoJSON export helper (benchmark-specific, simplified)
// ---------------------------------------------------------------------------
// GeoJSON export helper (benchmark-specific, simplified)
// ---------------------------------------------------------------------------

fn write_ring(f: &mut dyn Write, ring: &[Coord<f64>]) -> std::io::Result<()> {
    write!(f, "[")?;
    for (i, c) in ring.iter().enumerate() {
        if i > 0 {
            write!(f, ",")?;
        }
        write!(f, "[{},{}]", c.x, c.y)?;
    }
    write!(f, "]")
}

fn write_geometry_json(f: &mut dyn Write, g: &Geometry<f64>) -> std::io::Result<()> {
    match g {
        Geometry::Polygon(p) => {
            write!(f, "{{\"type\":\"Polygon\",\"coordinates\":[")?;
            write_ring(f, &p.exterior().0)?;
            for h in p.interiors() {
                write!(f, ",")?;
                write_ring(f, &h.0)?;
            }
            write!(f, "]}}")?;
        }
        Geometry::MultiPolygon(mp) => {
            write!(f, "{{\"type\":\"MultiPolygon\",\"coordinates\":[")?;
            for (pi, p) in mp.0.iter().enumerate() {
                if pi > 0 {
                    write!(f, ",")?;
                }
                write!(f, "[")?;
                write_ring(f, &p.exterior().0)?;
                for h in p.interiors() {
                    write!(f, ",")?;
                    write_ring(f, &h.0)?;
                }
                write!(f, "]")?;
            }
            write!(f, "]}}")?;
        }
        _ => write!(f, "null")?,
    }
    Ok(())
}

pub fn export_geojson(
    polys: &[Polygon<f64>],
    results: &[Geometry<f64>],
    geos_valid: &[bool],
    path: &str,
    crs_name: Option<&str>,
) -> std::io::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);

    write!(f, "{{")?;
    if let Some(crs) = crs_name {
        write!(
            f,
            "\"crs\":{{\"type\":\"name\",\"properties\":{{\"name\":\"{crs}\"}}}},"
        )?;
    }
    write!(f, "\"type\":\"FeatureCollection\",\"features\":[")?;

    for (i, (p, g)) in polys.iter().zip(results.iter()).enumerate() {
        let input_area = polygon_area(p);
        let output_area = geo_area(g);
        let ratio = if input_area > 0.0 {
            output_area / input_area
        } else {
            0.0
        };
        let out_polys = count_sub_polys(g);

        if i > 0 {
            write!(f, ",")?;
        }
        write!(
            f,
            "{{\"type\":\"Feature\",\"properties\":{{\"id\":{i},\"geos_valid\":{},\"input_area\":{input_area:.0},\"output_area\":{output_area:.0},\"area_ratio\":{ratio:.4},\"output_polys\":{out_polys}}},\"geometry\":",
            geos_valid[i]
        )?;
        write_geometry_json(&mut f, g)?;
        write!(f, "}}")?;
    }

    writeln!(f, "]}}")
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

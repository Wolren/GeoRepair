use super::shp_load::{count_sub_polys, geo_area, polygon_area};
use geo::{Coord, Geometry, Polygon};
use std::fs::File;
use std::io::{BufWriter, Write};

#[cfg(feature = "load-shp")]
use crate::feature::Feature;
use crate::Crs;
use serde_json::Value;
use shapefile::dbase;
use std::path::Path;

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
        .map_err(std::io::Error::other)?;

    for geom in geoms {
        match geom {
            Geometry::Point(p) => {
                writer
                    .write_shape_and_record(&Point::new(p.x(), p.y()), &dbase::Record::default())
                    .map_err(std::io::Error::other)?;
            }
            Geometry::MultiPoint(mp) => {
                for p in &mp.0 {
                    writer
                        .write_shape_and_record(
                            &Point::new(p.x(), p.y()),
                            &dbase::Record::default(),
                        )
                        .map_err(std::io::Error::other)?;
                }
            }
            Geometry::Line(ln) => {
                let shape = shapefile::Polyline::new(vec![
                    Point::new(ln.start.x, ln.start.y),
                    Point::new(ln.end.x, ln.end.y),
                ]);
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(std::io::Error::other)?;
            }
            Geometry::LineString(ls) => {
                let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                let shape = shapefile::Polyline::new(pts);
                writer
                    .write_shape_and_record(&shape, &dbase::Record::default())
                    .map_err(std::io::Error::other)?;
            }
            Geometry::MultiLineString(mls) => {
                for ls in &mls.0 {
                    let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                    let shape = shapefile::Polyline::new(pts);
                    writer
                        .write_shape_and_record(&shape, &dbase::Record::default())
                        .map_err(std::io::Error::other)?;
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
                    .map_err(std::io::Error::other)?;
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
                        .map_err(std::io::Error::other)?;
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
                    .map_err(std::io::Error::other)?;
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
                    .map_err(std::io::Error::other)?;
            }
            Geometry::GeometryCollection(gc) => {
                let sub: Vec<Geometry<f64>> = gc.0.clone();
                export_shp(&sub, path.to_str().unwrap_or("export.shp"), crs)?;
            }
        }
    }

    drop(writer);

    // Write .prj sidecar file if CRS is available
    if let Some(crs) = crs && let Some(prj_wkt) = crs.to_esri_wkt() {
        let prj_path = dir.join(format!("{stem}.prj"));
        std::fs::write(&prj_path, prj_wkt.as_bytes())?;
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
    progress: Option<&dyn Fn(f64)>,
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
                seen.entry(dbf_name).or_insert_with(|| infer_dbf_type(val));
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

    let mut writer =
        shapefile::Writer::from_path(&shp_path, builder).map_err(std::io::Error::other)?;

    let total = features.len();
    for (i, feature) in features.iter().enumerate() {
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
                    .map_err(std::io::Error::other)?;
            }
            Geometry::MultiPoint(mp) => {
                for p in &mp.0 {
                    writer
                        .write_shape_and_record(&Point::new(p.x(), p.y()), &record)
                        .map_err(std::io::Error::other)?;
                }
            }
            Geometry::Line(ln) => {
                let pl = shapefile::Polyline::new(vec![
                    Point::new(ln.start.x, ln.start.y),
                    Point::new(ln.end.x, ln.end.y),
                ]);
                writer
                    .write_shape_and_record(&pl, &record)
                    .map_err(std::io::Error::other)?;
            }
            Geometry::LineString(ls) => {
                let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                let pl = shapefile::Polyline::new(pts);
                writer
                    .write_shape_and_record(&pl, &record)
                    .map_err(std::io::Error::other)?;
            }
            Geometry::MultiLineString(mls) => {
                for ls in &mls.0 {
                    let pts: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                    let pl = shapefile::Polyline::new(pts);
                    writer
                        .write_shape_and_record(&pl, &record)
                        .map_err(std::io::Error::other)?;
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
                    .map_err(std::io::Error::other)?;
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
                        .map_err(std::io::Error::other)?;
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
                    .map_err(std::io::Error::other)?;
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
                    .map_err(std::io::Error::other)?;
            }
            Geometry::GeometryCollection(_) => {}
        }

        if let Some(ref cb) = progress && total > 0 && i % 100 == 0 {
            cb((i as f64 / total as f64) * 100.0);
        }
    }

    drop(writer);

    if let Some(crs) = crs && let Some(prj_wkt) = crs.to_esri_wkt() {
        let prj_path = dir.join(format!("{stem}.prj"));
        std::fs::write(&prj_path, prj_wkt.as_bytes())?;
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
            if n.is_f64() || n.as_f64().is_some_and(|f| f.fract() != 0.0) {
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
fn to_dbf_field(val: Option<&serde_json::Value>) -> dbase::FieldValue {
    match val {
        Some(Value::String(s)) => dbase::FieldValue::Character(Some(s.clone())),
        Some(Value::Number(n)) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() {
                    dbase::FieldValue::Numeric(Some(f))
                } else {
                    dbase::FieldValue::Float(Some(f as f32))
                }
            } else {
                dbase::FieldValue::Character(None)
            }
        }
        Some(Value::Bool(b)) => dbase::FieldValue::Logical(Some(*b)),
        _ => dbase::FieldValue::Character(None),
    }
}

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

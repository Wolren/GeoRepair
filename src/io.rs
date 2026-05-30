use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use geo::{Coord, Geometry, LineString, Polygon};

use crate::error::MakeValidError;

#[cfg(feature = "load-shp")]
pub use crate::load::load_shp;
pub use crate::load::{export_geojson, load_bin, polygon_area, signed_area};

/// Detect format from file extension and load geometries.
/// Supports: .shp, .bin, .geojson/.json, .wkt, .wkb, .csv
pub fn load_geometries(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                let polys = load_shp(path);
                Ok(polys.into_iter().map(Geometry::Polygon).collect())
            }
            #[cfg(not(feature = "load-shp"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile support requires 'load-shp' feature".into(),
                ))
            }
        }
        "bin" => {
            let polys = load_bin(path);
            Ok(polys.into_iter().map(Geometry::Polygon).collect())
        }
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                load_geojson(path)
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON support requires 'io-geojson' feature".into(),
                ))
            }
        }
        "wkt" => {
            #[cfg(feature = "io-wkt")]
            {
                load_wkt(path)
            }
            #[cfg(not(feature = "io-wkt"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKT support requires 'io-wkt' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                load_wkb(path)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKB support requires 'io-wkb' feature".into(),
                ))
            }
        }
        "csv" => {
            #[cfg(feature = "io-csv")]
            {
                load_csv_wkt(path)
            }
            #[cfg(not(feature = "io-csv"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "CSV WKT support requires 'io-csv' feature".into(),
                ))
            }
        }
        _ => Err(MakeValidError::UnsupportedFormat(format!(
            "unsupported format: .{ext} (supported: .shp, .bin, .geojson, .json, .wkt, .wkb, .csv)"
        ))),
    }
}

/// Export geometries to the specified format (geojson, wkt, wkb).
pub fn export_geometries(geoms: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                export_geojson_rfc7946(geoms, path)
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON export requires 'io-geojson' feature".into(),
                ))
            }
        }
        "wkt" => {
            #[cfg(feature = "io-wkt")]
            {
                export_wkt(geoms, path)
            }
            #[cfg(not(feature = "io-wkt"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKT export requires 'io-wkt' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                export_wkb(geoms, path)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKB export requires 'io-wkb' feature".into(),
                ))
            }
        }
        _ => Err(MakeValidError::UnsupportedFormat(format!(
            "unsupported export format: .{ext} (supported: .geojson, .json, .wkt, .wkb)"
        ))),
    }
}

// ---------------------------------------------------------------------------
// GeoJSON loader
// ---------------------------------------------------------------------------

#[cfg(feature = "io-geojson")]
pub fn load_geojson(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    use geojson::{Feature, GeoJson};
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let reader = BufReader::new(file);
    let geojson: GeoJson =
        serde_json::from_reader(reader).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let mut geometries = Vec::new();

    match geojson {
        GeoJson::FeatureCollection(collection) => {
            for mut feature in collection.features {
                if let Some(geom) = feature.geometry.take() {
                    let geo_geom = convert_geojson_geometry(geom, &feature);
                    if let Ok(geo_geom) = geo_geom {
                        geometries.push(geo_geom);
                    }
                }
            }
        }
        GeoJson::Feature(mut feature) => {
            if let Some(geom) = feature.geometry.take() {
                let geo_geom = convert_geojson_geometry(geom, &feature);
                if let Ok(geo_geom) = geo_geom {
                    geometries.push(geo_geom);
                }
            }
        }
        GeoJson::Geometry(geom) => {
            if let Ok(geo_geom) = convert_geojson_geometry(geom, &Feature::default()) {
                geometries.push(geo_geom);
            }
        }
    }

    Ok(geometries)
}

#[cfg(feature = "io-geojson")]
fn convert_geojson_geometry(
    geom: geojson::Geometry,
    _feature: &Feature,
) -> Result<Geometry<f64>, MakeValidError> {
    use geojson::GeometryValue;

    fn coords_to_coord(pos: &geojson::Position) -> Coord<f64> {
        Coord {
            x: pos[0],
            y: pos[1],
        }
    }

    fn parse_ring(coords: &[geojson::Position]) -> LineString<f64> {
        LineString::new(coords.iter().map(|c| coords_to_coord(c)).collect())
    }

    fn parse_polygon_coords(coords: &[Vec<geojson::Position>]) -> Polygon<f64> {
        let mut rings = coords
            .iter()
            .map(|r| LineString::new(r.iter().map(|c| coords_to_coord(c)).collect()));
        let exterior = rings.next().unwrap_or_else(|| LineString::new(Vec::new()));
        let interiors: Vec<LineString<f64>> = rings.collect();
        Polygon::new(exterior, interiors)
    }

    match geom.value {
        GeometryValue::Point {
            coordinates: ref coords,
        } => {
            let c = coords_to_coord(coords);
            Ok(Geometry::Point(geo::Point(c)))
        }
        GeometryValue::MultiPoint {
            coordinates: ref points,
        } => {
            let pts: Vec<geo::Point<f64>> = points
                .iter()
                .map(|c| geo::Point(coords_to_coord(c)))
                .collect();
            Ok(Geometry::MultiPoint(geo::MultiPoint::new(pts)))
        }
        GeometryValue::LineString {
            coordinates: ref coords,
        } => {
            let ls = parse_ring(coords);
            Ok(Geometry::LineString(ls))
        }
        GeometryValue::MultiLineString {
            coordinates: ref lines,
        } => {
            let mls: Vec<LineString<f64>> = lines.iter().map(|l| parse_ring(l)).collect();
            Ok(Geometry::MultiLineString(geo::MultiLineString::new(mls)))
        }
        GeometryValue::Polygon {
            coordinates: ref coords,
        } => {
            let poly = parse_polygon_coords(coords);
            Ok(Geometry::Polygon(poly))
        }
        GeometryValue::MultiPolygon {
            coordinates: ref polygons,
        } => {
            let polys: Vec<Polygon<f64>> =
                polygons.iter().map(|p| parse_polygon_coords(p)).collect();
            Ok(Geometry::MultiPolygon(geo::MultiPolygon::new(polys)))
        }
        GeometryValue::GeometryCollection {
            geometries: ref geoms,
        } => {
            let mut collected = Vec::new();
            for g in geoms {
                if let Ok(converted) = convert_geojson_geometry(g.clone(), &Feature::default()) {
                    collected.push(converted);
                }
            }
            Ok(Geometry::GeometryCollection(geo::GeometryCollection(
                collected,
            )))
        }
    }
}

#[cfg(feature = "io-geojson")]
use geojson::Feature;

// ---------------------------------------------------------------------------
// RFC 7946 compliant GeoJSON export
// ---------------------------------------------------------------------------

#[cfg(feature = "io-geojson")]
pub fn export_geojson_rfc7946(
    geometries: &[Geometry<f64>],
    path: &str,
) -> Result<(), MakeValidError> {
    use geojson::{Feature, FeatureCollection, GeoJson};
    use std::fs::File;
    use std::io::BufWriter;

    let features: Vec<Feature> = geometries
        .iter()
        .map(|geom| {
            let gj_geom = geo_geom_to_geojson(geom.clone());
            Feature {
                geometry: Some(gj_geom),
                properties: None,
                id: None,
                bbox: None,
                foreign_members: None,
            }
        })
        .collect();

    let collection = FeatureCollection {
        features,
        bbox: None,
        foreign_members: None,
    };

    let gj = GeoJson::FeatureCollection(collection);
    let json =
        serde_json::to_string_pretty(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    write!(writer, "{json}").map_err(|e| MakeValidError::IoError(e.to_string()))?;

    Ok(())
}

#[cfg(feature = "io-geojson")]
fn geo_geom_to_geojson(geom: Geometry<f64>) -> geojson::Geometry {
    use geojson::GeometryValue;

    fn coord_to_pos(c: Coord<f64>) -> geojson::Position {
        [c.x, c.y].into()
    }

    fn ring_to_pos(ring: &LineString<f64>) -> Vec<geojson::Position> {
        ring.0.iter().map(|c| coord_to_pos(*c)).collect()
    }

    fn polygon_to_coords(poly: &Polygon<f64>) -> Vec<Vec<geojson::Position>> {
        let mut coords = vec![ring_to_pos(poly.exterior())];
        for h in poly.interiors() {
            coords.push(ring_to_pos(h));
        }
        coords
    }

    let value = match geom {
        Geometry::Point(p) => GeometryValue::Point {
            coordinates: coord_to_pos(p.0),
        },
        Geometry::MultiPoint(mp) => GeometryValue::MultiPoint {
            coordinates: mp.0.iter().map(|p| coord_to_pos(p.0)).collect(),
        },
        Geometry::LineString(ls) => GeometryValue::LineString {
            coordinates: ring_to_pos(&ls),
        },
        Geometry::MultiLineString(mls) => GeometryValue::MultiLineString {
            coordinates: mls.0.iter().map(|ls| ring_to_pos(ls)).collect(),
        },
        Geometry::Polygon(p) => GeometryValue::Polygon {
            coordinates: polygon_to_coords(&p),
        },
        Geometry::Line(l) => GeometryValue::LineString {
            coordinates: vec![[l.start.x, l.start.y].into(), [l.end.x, l.end.y].into()],
        },
        Geometry::MultiPolygon(mp) => GeometryValue::MultiPolygon {
            coordinates: mp.0.iter().map(|p| polygon_to_coords(&p)).collect(),
        },
        Geometry::GeometryCollection(gc) => {
            let geoms: Vec<geojson::Geometry> = gc.0.into_iter().map(geo_geom_to_geojson).collect();
            GeometryValue::GeometryCollection { geometries: geoms }
        }
        Geometry::Rect(r) => {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord {
                        x: r.min().x,
                        y: r.min().y,
                    },
                    Coord {
                        x: r.max().x,
                        y: r.min().y,
                    },
                    Coord {
                        x: r.max().x,
                        y: r.max().y,
                    },
                    Coord {
                        x: r.min().x,
                        y: r.max().y,
                    },
                    Coord {
                        x: r.min().x,
                        y: r.min().y,
                    },
                ]),
                Vec::new(),
            );
            GeometryValue::Polygon {
                coordinates: polygon_to_coords(&poly),
            }
        }
        Geometry::Triangle(t) => {
            let poly = Polygon::new(
                LineString::new(vec![t.v1(), t.v2(), t.v3(), t.v1()]),
                Vec::new(),
            );
            GeometryValue::Polygon {
                coordinates: polygon_to_coords(&poly),
            }
        }
    };

    geojson::Geometry::new(value)
}

// ---------------------------------------------------------------------------
// WKT loader
// ---------------------------------------------------------------------------

#[cfg(feature = "io-wkt")]
pub fn load_wkt(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let content =
        std::fs::read_to_string(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let content = content.trim();

    let geom: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(content)
        .map_err(|e| MakeValidError::ParseError(format!("WKT parse error: {e}")))?;

    Ok(extract_geometries(geom))
}

#[cfg(feature = "io-wkt")]
pub fn export_wkt(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    use wkt::ToWkt;
    let mut out = String::new();
    for geom in geometries {
        out.push_str(&geom.wkt_string());
        out.push('\n');
    }
    std::fs::write(path, &out).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    Ok(())
}

fn extract_geometries(geom: Geometry<f64>) -> Vec<Geometry<f64>> {
    match geom {
        Geometry::GeometryCollection(gc) => {
            let mut result = Vec::new();
            for g in gc.0 {
                result.extend(extract_geometries(g));
            }
            result
        }
        other => vec![other],
    }
}

// ---------------------------------------------------------------------------
// WKB loader
// ---------------------------------------------------------------------------

#[cfg(feature = "io-wkb")]
pub fn load_wkb(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    use geo_traits::to_geo::ToGeoGeometry;
    use wkb::reader::read_wkb;

    let mut buf = Vec::new();
    File::open(path)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?
        .read_to_end(&mut buf)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;

    let wkb_geom =
        read_wkb(&buf).map_err(|e| MakeValidError::ParseError(format!("WKB parse error: {e}")))?;
    let geo_geom: Geometry<f64> = wkb_geom.to_geometry();

    Ok(extract_geometries(geo_geom))
}

#[cfg(feature = "io-wkb")]
pub fn export_wkb(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    use std::io::Cursor;
    use wkb::writer::{geometry_wkb_size, write_geometry, WriteOptions};

    let opts = WriteOptions::default();
    let mut buf = Vec::new();
    for geom in geometries {
        let size = geometry_wkb_size(geom);
        let start = buf.len();
        buf.resize(start + size, 0u8);
        write_geometry(&mut Cursor::new(&mut buf[start..]), geom, &opts)
            .map_err(|e| MakeValidError::ParseError(format!("WKB write error: {e}")))?;
    }
    std::fs::write(path, &buf).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CSV with WKT geometry column
// ---------------------------------------------------------------------------

#[cfg(feature = "io-csv")]
pub fn load_csv_wkt(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let reader = BufReader::new(file);
    let mut csv_reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(reader);

    let headers: Vec<String> = csv_reader
        .headers()
        .map_err(|e| MakeValidError::ParseError(format!("CSV header error: {e}")))?
        .iter()
        .map(|h| h.to_string().to_lowercase())
        .collect();

    // Find "geometry", "wkt", or "geom" column
    let geom_col = headers
        .iter()
        .position(|h| matches!(h.as_str(), "geometry" | "wkt" | "geom" | "geography"));

    let geom_col = match geom_col {
        Some(idx) => idx,
        None => {
            // Assume last column is WKT if no header match
            headers.len().saturating_sub(1)
        }
    };

    let mut geometries = Vec::new();
    for result in csv_reader.records() {
        let record =
            result.map_err(|e| MakeValidError::ParseError(format!("CSV record error: {e}")))?;
        if let Some(wkt_str) = record.get(geom_col) {
            if !wkt_str.trim().is_empty() {
                let geom: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(wkt_str.trim())
                    .map_err(|e| MakeValidError::ParseError(format!("CSV WKT parse error: {e}")))?;
                geometries.extend(extract_geometries(geom));
            }
        }
    }

    Ok(geometries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_geometries_point() {
        let g = Geometry::Point(geo::Point::new(1.0, 2.0));
        let result = extract_geometries(g);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_extract_geometries_collection() {
        let gc = geo::GeometryCollection(vec![
            Geometry::Point(geo::Point::new(1.0, 2.0)),
            Geometry::Point(geo::Point::new(3.0, 4.0)),
        ]);
        let result = extract_geometries(Geometry::GeometryCollection(gc));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_load_geometries_unsupported_format() {
        let result = load_geometries("test.xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_polygon_area_util() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!((polygon_area(&poly) - 100.0).abs() < 1e-12);
    }

    #[test]
    #[cfg(feature = "io-wkt")]
    fn test_wkt_roundtrip() {
        let geom = Geometry::Point(geo::Point::new(1.0, 2.0));
        let path = std::env::temp_dir().join("test_roundtrip.wkt");
        let path_str = path.to_str().unwrap().to_string();

        let result = export_geometries(&[geom.clone()], &path_str);
        assert!(result.is_ok());

        let loaded = load_geometries(&path_str);
        assert!(loaded.is_ok());
        let loaded_geoms = loaded.unwrap();
        assert_eq!(loaded_geoms.len(), 1);
        assert_eq!(loaded_geoms[0], geom);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_detect_format_geojson() {
        let ext = Path::new("test.geojson")
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        assert_eq!(ext, Some("geojson".to_string()));
    }

    #[test]
    fn test_detect_format_json() {
        let ext = Path::new("test.json")
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        assert_eq!(ext, Some("json".to_string()));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_geometries("nonexistent_file_for_testing.shp");
        assert!(result.is_err());
    }
}

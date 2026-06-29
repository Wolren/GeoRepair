use std::path::Path;

use geo::{Coord, Geometry, Polygon};

use crate::core::MakeValidError;
use crate::feature::Feature;
use crate::zm::{count_coords, ZmValue};
use crate::Crs;

pub mod binary;
pub mod gml;
pub mod shp_export;
pub mod shp_load;
pub mod stream;

pub use binary::{load_bin, load_bin_stream};
pub use shp_export::export_geojson;
#[cfg(feature = "load-shp")]
pub use shp_export::{export_shp, export_shp_features};

pub use stream::{open_reader, open_writer, stream_repair, FeatureReader, FeatureWriter};

// ---------------------------------------------------------------------------
// Error helper
// ---------------------------------------------------------------------------

pub(crate) fn io_err(msg: impl Into<String>) -> MakeValidError {
    MakeValidError::UnsupportedFormat(msg.into())
}

// ---------------------------------------------------------------------------
// Geometry area utilities
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Geometry loading — per format
// ---------------------------------------------------------------------------

fn load_geojson_content(content: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    let gj: geojson::GeoJson =
        serde_json::from_str(content).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let mut geoms = Vec::new();
    let mut crs = None;

    match gj {
        geojson::GeoJson::FeatureCollection(fc) => {
            // CRS from foreign members
            if let Some(ref fm) = fc.foreign_members {
                if let Some(crs_val) = fm.get("crs") {
                    if let Some(name) = crs_val
                        .get("properties")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        if name.starts_with("EPSG:") || name.starts_with("epsg:") {
                            let code = name[5..].parse::<u32>().ok();
                            if let Some(c) = code {
                                crs = Some(Crs::from_epsg(c));
                            }
                        }
                    }
                }
            }
            for f in fc.features {
                if let Some(gj_geom) = f.geometry {
                    let geo_geom = geo::Geometry::<f64>::try_from(&gj_geom)
                        .map_err(|e: geojson::Error| {
                            MakeValidError::ParseError(e.to_string())
                        })?;
                    geoms.extend(extract_geometries(geo_geom));
                }
            }
        }
        geojson::GeoJson::Feature(f) => {
            if let Some(gj_geom) = f.geometry {
                let geo_geom = geo::Geometry::<f64>::try_from(&gj_geom)
                    .map_err(|e: geojson::Error| {
                        MakeValidError::ParseError(e.to_string())
                    })?;
                geoms.push(geo_geom);
            }
        }
        geojson::GeoJson::Geometry(gj_geom) => {
            let geo_geom = geo::Geometry::<f64>::try_from(&gj_geom)
                .map_err(|e: geojson::Error| {
                    MakeValidError::ParseError(e.to_string())
                })?;
            geoms.push(geo_geom);
        }
    }

    Ok((geoms, crs))
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

fn load_wkt_content(content: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    use std::str::FromStr;

    fn parse_ewkt(line: &str) -> Result<(Geometry<f64>, Option<Crs>), MakeValidError> {
        let trimmed = line.trim();
        // Check for EWKT SRID prefix: SRID=4326;POINT(...)
        let (wkt_str, crs) = if let Some(rest) = trimmed.strip_prefix("SRID=") {
            if let Some(semi_pos) = rest.find(';') {
                let code: u32 = rest[..semi_pos]
                    .parse()
                    .map_err(|_| MakeValidError::ParseError("invalid SRID".into()))?;
                (rest[semi_pos + 1..].trim(), Some(Crs::from_epsg(code)))
            } else {
                (trimmed, None)
            }
        } else {
            (trimmed, None)
        };
        let wkt: wkt::Wkt<f64> =
            wkt::Wkt::from_str(wkt_str)
                .map_err(|e| MakeValidError::ParseError(format!("WKT error: {e}")))?;
        let geom: Geometry<f64> = wkt
            .try_into()
            .map_err(|e| MakeValidError::ParseError(format!("WKT convert: {e}")))?;
        Ok((geom, crs))
    }

    let mut geoms = Vec::new();
    let mut crs = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (geom, line_crs) = parse_ewkt(trimmed)?;
        if crs.is_none() {
            crs = line_crs;
        }
        geoms.extend(extract_geometries(geom));
    }
    Ok((geoms, crs))
}

fn load_wkb_content(data: &[u8]) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    use geo_traits::to_geo::ToGeoGeometry;

    // Try EWKB with SRID
    let (srid, clean_data) = extract_ewkb_srid(data);
    let crs = srid.map(|s| Crs::from_epsg(s as u32));

    let mut offset = 0;
    let mut geoms = Vec::new();
    while offset < clean_data.len() {
        let remaining = &clean_data[offset..];
        match wkb::reader::read_wkb(remaining) {
            Ok(wkb_geom) => {
                let consumed = estimate_wkb_size(remaining).unwrap_or(remaining.len());
                let geo_geom: Geometry<f64> = wkb_geom.to_geometry();
                geoms.extend(extract_geometries(geo_geom));
                offset += consumed;
            }
            Err(e) => {
                if geoms.is_empty() {
                    return Err(MakeValidError::ParseError(format!("WKB error: {e}")));
                }
                break;
            }
        }
    }
    Ok((geoms, crs))
}

fn extract_ewkb_srid(buf: &[u8]) -> (Option<i32>, Vec<u8>) {
    if buf.len() < 5 {
        return (None, buf.to_vec());
    }
    let endianness = buf[0];
    let geom_type = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    const EWKB_SRID_FLAG: u32 = 0x20000000;
    if geom_type & EWKB_SRID_FLAG != 0 && buf.len() >= 9 {
        let srid = i32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
        let mut clean = Vec::with_capacity(buf.len() - 4);
        clean.push(endianness);
        clean.extend_from_slice(&(geom_type & !EWKB_SRID_FLAG).to_le_bytes());
        clean.extend_from_slice(&buf[9..]);
        (Some(srid), clean)
    } else {
        (None, buf.to_vec())
    }
}

fn estimate_wkb_size(buf: &[u8]) -> Option<usize> {
    if buf.len() < 5 {
        return None;
    }
    let is_be = buf[0] == 0;
    let geom_type = if is_be {
        u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]])
    } else {
        u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]])
    };
    const WKB_POINT: u32 = 1;
    const WKB_LINESTRING: u32 = 2;
    const WKB_POLYGON: u32 = 3;
    const WKB_MULTIPOINT: u32 = 4;
    const WKB_MULTILINESTRING: u32 = 5;
    const WKB_MULTIPOLYGON: u32 = 6;
    const WKB_GEOMETRYCOLLECTION: u32 = 7;

    let base_type = geom_type & 0xFFFF;
    let has_z = (geom_type & 0x80000000) != 0 || (geom_type & 0x00001000) != 0;
    let has_m = (geom_type & 0x40000000) != 0 || (geom_type & 0x00002000) != 0;
    let coord_size = 8 * (2 + if has_z { 1 } else { 0 } + if has_m { 1 } else { 0 });

    Some(match base_type {
        WKB_POINT => 1 + 4 + coord_size,
        WKB_LINESTRING => {
            if buf.len() < 9 {
                return None;
            }
            let num_points = if is_be {
                u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]])
            } else {
                u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]])
            };
            1 + 4 + 4 + (num_points as usize) * coord_size
        }
        WKB_POLYGON => {
            if buf.len() < 9 {
                return None;
            }
            let num_rings = if is_be {
                u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]])
            } else {
                u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]])
            };
            let mut offset = 9;
            for _ in 0..num_rings {
                if buf.len() < offset + 4 {
                    return None;
                }
                let num_points = if is_be {
                    u32::from_be_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
                } else {
                    u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
                };
                offset += 4 + (num_points as usize) * coord_size;
            }
            offset
        }
        WKB_MULTIPOINT | WKB_MULTILINESTRING | WKB_MULTIPOLYGON | WKB_GEOMETRYCOLLECTION => {
            if buf.len() < 9 {
                return None;
            }
            let num_items = if is_be {
                u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]])
            } else {
                u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]])
            };
            let mut offset = 9;
            for _ in 0..num_items {
                if offset >= buf.len() {
                    return None;
                }
                let item_size = estimate_wkb_size(&buf[offset..])?;
                offset += item_size;
            }
            offset
        }
        _ => return None,
    })
}

#[cfg(feature = "io-csv")]
fn load_csv_content(content: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    use std::str::FromStr;

    let mut rdr = csv::Reader::from_reader(content.as_bytes());
    let mut geoms = Vec::new();

    for result in rdr.records() {
        let record = result.map_err(|e| io_err(e.to_string()))?;
        // Find "geometry" column
        let wkt_val = if let Some(v) = record.get(0) {
            v.to_string()
        } else {
            continue;
        };
        let trimmed = wkt_val.trim().trim_matches('"');
        if trimmed.is_empty() {
            continue;
        }
        let wkt: wkt::Wkt<f64> =
            wkt::Wkt::from_str(trimmed).map_err(|e| MakeValidError::ParseError(format!("CSV WKT: {e}")))?;
        let geom: Geometry<f64> = wkt
            .try_into()
            .map_err(|e| MakeValidError::ParseError(format!("CSV WKT convert: {e}")))?;
        geoms.push(geom);
    }

    Ok((geoms, None))
}

#[cfg(not(feature = "io-csv"))]
fn load_csv_content(_content: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    Err(MakeValidError::UnsupportedFormat(
        "CSV loading requires 'io-csv' feature".into(),
    ))
}

fn load_shp_file(path: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    #[cfg(feature = "load-shp")]
    {
        let geoms = crate::io::shp_load::load_shp_geometries(path)
            .map_err(|e| io_err(e.to_string()))?;
        let crs = load_prj_crs(path);
        Ok((geoms, crs))
    }
    #[cfg(not(feature = "load-shp"))]
    {
        let _ = path;
        Err(MakeValidError::UnsupportedFormat(
            "shapefile loading requires 'load-shp' feature".into(),
        ))
    }
}

#[cfg(feature = "load-shp")]
fn load_prj_crs(shp_path: &str) -> Option<Crs> {
    let prj_path = std::path::Path::new(shp_path).with_extension("prj");
    let wkt = std::fs::read_to_string(&prj_path).ok()?;
    let trimmed = wkt.trim();
    if trimmed.is_empty() {
        return None;
    }
    Crs::from_prj_wkt(trimmed)
}

fn load_gpkg_file(path: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    #[cfg(feature = "io-gpkg")]
    {
        load_gpkg_file_inner(path)
    }
    #[cfg(not(feature = "io-gpkg"))]
    {
        let _ = path;
        Err(MakeValidError::UnsupportedFormat(
            "GeoPackage support requires 'io-gpkg' feature".into(),
        ))
    }
}

#[cfg(feature = "io-gpkg")]
fn load_gpkg_file_inner(path: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    use geo_traits::to_geo::ToGeoGeometry;
    use rusqlite::Connection;

    let conn = Connection::open(path).map_err(|e| io_err(format!("GPKG open: {e}")))?;

    let srid: Option<i32> = conn
        .prepare("SELECT srid FROM gpkg_geometry_columns LIMIT 1")
        .and_then(|mut stmt| stmt.query_row([], |row| row.get(0)))
        .ok();
    let crs = srid.and_then(|s| Some(Crs::from_epsg(s as u32)));

    let (table_name, geom_col): (String, String) = conn
        .prepare(
            "SELECT table_name, column_name FROM gpkg_geometry_columns LIMIT 1",
        )
        .and_then(|mut stmt| stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?))))
        .map_err(|e| io_err(format!("GPKG metadata: {e}")))?;

    let query = format!("SELECT \"{geom_col}\" FROM \"{table_name}\"");
    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| io_err(format!("GPKG query: {e}")))?;

    let mut geoms = Vec::new();
    let rows = stmt
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|e| io_err(format!("GPKG read: {e}")))?;

    for row in rows {
        let blob = row.map_err(|e| io_err(format!("GPKG row: {e}")))?;
        if blob.is_empty() || blob.len() < 5 {
            continue;
        }
        if blob.len() > 15 && &blob[..16] == b"SQLite format 3\x00" {
            continue;
        }

        // GPKG WKB has optional 4-byte envelope prefix
        let wkb_data = if blob.len() > 4 && blob[0] != 0x01 && blob[0] != 0x00 {
            &blob[4..]
        } else {
            &blob[..]
        };

        match wkb::reader::read_wkb(wkb_data) {
            Ok(wkb_geom) => {
                let geo_geom: Geometry<f64> = wkb_geom.to_geometry();
                geoms.push(geo_geom);
            }
            Err(e) => {
                log::warn!("GPKG: skipping unparseable geometry blob: {e}");
            }
        }
    }

    Ok((geoms, crs))
}

// ---------------------------------------------------------------------------
// Format-agnostic loading (geometry only)
// ---------------------------------------------------------------------------

pub fn load_geometries(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    load_geometries_with_crs(path).map(|(geoms, _)| geoms)
}

pub fn load_geometries_with_crs(
    path: &str,
) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let content = std::fs::read_to_string(path);
    let data = std::fs::read(path);

    match ext.as_str() {
        "dbf" | "shx" => {
            let shp_path = Path::new(path).with_extension("shp");
            let shp_str = shp_path.to_str().ok_or_else(|| {
                MakeValidError::UnsupportedFormat("Invalid path encoding".into())
            })?;
            load_geometries_with_crs(shp_str)
        }
        "shp" => load_shp_file(path),
        "bin" => {
            let polys = load_bin(path).map_err(MakeValidError::UnsupportedFormat)?;
            Ok((polys.into_iter().map(Geometry::Polygon).collect(), None))
        }
        "geojson" | "json" => {
            let c = content.map_err(|e| io_err(e.to_string()))?;
            load_geojson_content(&c)
        }
        "wkt" => {
            let c = content.map_err(|e| io_err(e.to_string()))?;
            load_wkt_content(&c)
        }
        "wkb" => {
            let d = data.map_err(|e| io_err(e.to_string()))?;
            load_wkb_content(&d)
        }
        "csv" => {
            let c = content.map_err(|e| io_err(e.to_string()))?;
            load_csv_content(&c)
        }
        "gpkg" => load_gpkg_file(path),
        "gml" | "xml" => {
            let c = content.map_err(|e| io_err(e.to_string()))?;
            gml::load_gml_content(&c)
        }
        _ => Err(MakeValidError::UnsupportedFormat(format!(
            "unsupported format: .{ext} (supported: .shp, .bin, .geojson, .json, .wkt, .wkb, .csv, .gpkg, .gml, .xml)"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Format-agnostic loading (features with attributes)
// ---------------------------------------------------------------------------

pub fn load_features(path: &str) -> Result<Vec<Feature>, MakeValidError> {
    load_features_with_progress(path, None)
}

pub fn load_features_with_progress(
    path: &str,
    progress: Option<&dyn Fn(f64)>,
) -> Result<Vec<Feature>, MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                let content =
                    std::fs::read_to_string(path).map_err(|e| io_err(e.to_string()))?;
                let gj: geojson::GeoJson =
                    serde_json::from_str(&content)
                        .map_err(|e| MakeValidError::ParseError(e.to_string()))?;
                match gj {
                    geojson::GeoJson::FeatureCollection(fc) => {
                        let crs = extract_geojson_crs(&fc.foreign_members);
                        let mut features = Vec::with_capacity(fc.features.len());
                        for f in fc.features {
                            features.push(convert_geojson_feature_inner(f, crs.clone()));
                        }
                        Ok(features)
                    }
                    geojson::GeoJson::Feature(f) => {
                        Ok(vec![convert_geojson_feature_inner(f, None)])
                    }
                    geojson::GeoJson::Geometry(gj_geom) => {
                        let zm = convert_geojson_zm_inner(gj_geom);
                        match zm {
                            Ok(zm) => {
                                Ok(vec![Feature::with_all(zm.geometry, None, None, zm.zm)])
                            }
                            Err(e) => Err(e),
                        }
                    }
                }
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                let _ = progress;
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON feature loading requires 'io-geojson' feature".into(),
                ))
            }
        }
        "dbf" | "shx" => {
            let shp_path = Path::new(path).with_extension("shp");
            let shp_str = shp_path.to_str().ok_or_else(|| {
                MakeValidError::UnsupportedFormat("Invalid path encoding".into())
            })?;
            load_features_with_progress(shp_str, progress)
        }
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                let features = shp_load::load_shp_features(path, progress)
                    .map_err(|e| io_err(e.to_string()))?;
                Ok(features)
            }
            #[cfg(not(feature = "load-shp"))]
            {
                let _ = progress;
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile feature loading requires 'load-shp' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                let d = std::fs::read(path).map_err(|e| io_err(e.to_string()))?;
                let (geoms, _crs) = load_wkb_content(&d)?;
                let features: Vec<Feature> =
                    geoms.into_iter().map(|g| Feature::new(g)).collect();
                Ok(features)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                let _ = progress;
                Err(MakeValidError::UnsupportedFormat(
                    "WKB feature loading requires 'io-wkb' feature".into(),
                ))
            }
        }
        _ => {
            let (geoms, crs) = load_geometries_with_crs(path)?;
            Ok(geoms
                .into_iter()
                .map(|g| Feature::new(g).with_crs(crs.clone()))
                .collect())
        }
    }
}

#[cfg(feature = "io-geojson")]
fn extract_geojson_crs(foreign: &Option<serde_json::Map<String, serde_json::Value>>) -> Option<Crs> {
    let members = foreign.as_ref()?;
    let crs_val = members.get("crs")?;
    let props = crs_val.as_object()?;
    if props.get("type")?.as_str()? != "name" {
        return None;
    }
    let props_obj = props.get("properties")?.as_object()?;
    let name = props_obj.get("name")?.as_str()?;
    Some(Crs::from_authority(name))
}

#[cfg(feature = "io-geojson")]
fn convert_geojson_feature_inner(
    mut f: geojson::Feature,
    crs: Option<Crs>,
) -> Feature {
    if let Some(gj_geom) = f.geometry.take() {
        match convert_geojson_zm_inner(gj_geom) {
            Ok(zm) => Feature::with_all(zm.geometry, f.properties, crs, zm.zm),
            Err(_) => Feature::with_all(
                Geometry::GeometryCollection(geo::GeometryCollection(Vec::new())),
                f.properties,
                crs,
                Vec::new(),
            ),
        }
    } else {
        Feature::with_all(
            Geometry::GeometryCollection(geo::GeometryCollection(Vec::new())),
            f.properties,
            crs,
            Vec::new(),
        )
    }
}

#[cfg(feature = "io-geojson")]
fn convert_geojson_zm_inner(
    gj_geom: geojson::Geometry,
) -> Result<crate::zm::ZmGeometry, MakeValidError> {
    use crate::zm::ZmGeometry;

    let geo_geom: geo::Geometry<f64> = (&gj_geom.value)
        .try_into()
        .map_err(|e: geojson::Error| MakeValidError::ParseError(e.to_string()))?;
    let n = count_coords(&geo_geom);
    let mut zm = Vec::with_capacity(n);
    extract_geojson_zm(&gj_geom, &mut zm);
    Ok(ZmGeometry::with_zm(geo_geom, zm))
}

#[cfg(feature = "io-geojson")]
fn extract_geojson_zm(gj_geom: &geojson::Geometry, zm: &mut Vec<ZmValue>) {
    use geojson::GeometryValue;
    fn pos_z(c: &geojson::Position) -> Option<f64> {
        if c.len() > 2 {
            Some(c[2])
        } else {
            None
        }
    }
    fn pos_m(c: &geojson::Position) -> Option<f64> {
        if c.len() > 3 {
            Some(c[3])
        } else {
            None
        }
    }
    match &gj_geom.value {
        GeometryValue::Point { coordinates: c } => {
            zm.push(ZmValue::new(pos_z(c), pos_m(c)));
        }
        GeometryValue::MultiPoint { coordinates: points } => {
            for c in points {
                zm.push(ZmValue::new(pos_z(c), pos_m(c)));
            }
        }
        GeometryValue::LineString { coordinates: coords } => {
            for c in coords {
                zm.push(ZmValue::new(pos_z(c), pos_m(c)));
            }
        }
        GeometryValue::MultiLineString { coordinates: lines } => {
            for line in lines {
                for c in line {
                    zm.push(ZmValue::new(pos_z(c), pos_m(c)));
                }
            }
        }
        GeometryValue::Polygon { coordinates: rings } => {
            for ring in rings {
                for c in ring {
                    zm.push(ZmValue::new(pos_z(c), pos_m(c)));
                }
            }
        }
        GeometryValue::MultiPolygon { coordinates: polys } => {
            for poly in polys {
                for ring in poly {
                    for c in ring {
                        zm.push(ZmValue::new(pos_z(c), pos_m(c)));
                    }
                }
            }
        }
        GeometryValue::GeometryCollection { geometries: geoms } => {
            for g in geoms {
                extract_geojson_zm(g, zm);
            }
        }
    }
}

#[cfg(feature = "io-geojson")]
pub fn convert_geojson_zm(
    gj_geom: geojson::Geometry,
) -> Result<crate::zm::ZmGeometry, MakeValidError> {
    convert_geojson_zm_inner(gj_geom)
}

#[cfg(feature = "io-geojson")]
pub fn geo_geom_to_geojson(geom: &Geometry<f64>) -> geojson::Geometry {
    geojson::Geometry::from(geom)
}

#[cfg(feature = "io-geojson")]
fn geom_to_geojson_zm(geom: &Geometry<f64>, zm: &mut dyn Iterator<Item = ZmValue>) -> geojson::Geometry {
    use geojson::GeometryValue;
    fn pos(c: Coord<f64>, zv: ZmValue) -> geojson::Position {
        let mut p = vec![c.x, c.y];
        if zv.z.is_some() || zv.m.is_some() {
            p.push(zv.z.unwrap_or(0.0));
        }
        if let Some(m) = zv.m {
            p.push(m);
        }
        geojson::Position::from(p)
    }
    fn ring(r: &geo::LineString<f64>, zm: &mut dyn Iterator<Item = ZmValue>) -> Vec<geojson::Position> {
        r.0.iter().map(|c| pos(*c, zm.next().unwrap_or(ZmValue::NONE))).collect()
    }
    let value = match geom {
        Geometry::Point(p) => {
            GeometryValue::Point { coordinates: pos(p.0, zm.next().unwrap_or(ZmValue::NONE)) }
        }
        Geometry::MultiPoint(mp) => {
            GeometryValue::MultiPoint {
                coordinates: mp.0.iter().map(|pt| pos(pt.0, zm.next().unwrap_or(ZmValue::NONE))).collect(),
            }
        }
        Geometry::LineString(ls) => GeometryValue::LineString { coordinates: ring(ls, zm) },
        Geometry::MultiLineString(mls) => {
            GeometryValue::MultiLineString {
                coordinates: mls.0.iter().map(|ls| ring(ls, zm)).collect(),
            }
        }
        Geometry::Polygon(p) => {
            let mut coords = vec![ring(p.exterior(), zm)];
            for h in p.interiors() { coords.push(ring(h, zm)); }
            GeometryValue::Polygon { coordinates: coords }
        }
        Geometry::Line(l) => {
            GeometryValue::LineString {
                coordinates: vec![
                    pos(l.start, zm.next().unwrap_or(ZmValue::NONE)),
                    pos(l.end, zm.next().unwrap_or(ZmValue::NONE)),
                ],
            }
        }
        Geometry::MultiPolygon(mp) => {
            GeometryValue::MultiPolygon {
                coordinates: mp.0.iter().map(|p| {
                    let mut coords = vec![ring(p.exterior(), zm)];
                    for h in p.interiors() { coords.push(ring(h, zm)); }
                    coords
                }).collect(),
            }
        }
        Geometry::GeometryCollection(gc) => {
            GeometryValue::GeometryCollection {
                geometries: gc.0.iter().map(|g| geom_to_geojson_zm(g, zm)).collect(),
            }
        }
        Geometry::Rect(r) => {
            let poly = geo::Polygon::new(
                geo::LineString::new(vec![
                    Coord { x: r.min().x, y: r.min().y },
                    Coord { x: r.max().x, y: r.min().y },
                    Coord { x: r.max().x, y: r.max().y },
                    Coord { x: r.min().x, y: r.max().y },
                    Coord { x: r.min().x, y: r.min().y },
                ]),
                Vec::new(),
            );
            GeometryValue::Polygon { coordinates: vec![ring(poly.exterior(), zm)] }
        }
        Geometry::Triangle(t) => {
            GeometryValue::Polygon {
                coordinates: vec![vec![
                    pos(t.v1(), zm.next().unwrap_or(ZmValue::NONE)),
                    pos(t.v2(), zm.next().unwrap_or(ZmValue::NONE)),
                    pos(t.v3(), zm.next().unwrap_or(ZmValue::NONE)),
                    pos(t.v1(), zm.next().unwrap_or(ZmValue::NONE)),
                ]],
            }
        }
    };
    geojson::Geometry::new(value)
}

#[cfg(feature = "io-geojson")]
fn geo_geom_to_geojson_feature(feat: &Feature) -> geojson::Feature {
    let has_zm = feat.zm.iter().any(|z| z.z.is_some() || z.m.is_some());
    let gj_geom = if has_zm && count_coords(&feat.geometry) == feat.zm.len() {
        geom_to_geojson_zm(&feat.geometry, &mut feat.zm.iter().copied())
    } else {
        geojson::Geometry::from(&feat.geometry)
    };
    geojson::Feature {
        geometry: Some(gj_geom),
        properties: feat.properties.clone(),
        id: None,
        bbox: None,
        foreign_members: None,
    }
}

// ---------------------------------------------------------------------------
// Format-agnostic export
// ---------------------------------------------------------------------------

pub fn export_geometries(geoms: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    export_geometries_with_crs(geoms, path, None)
}

pub fn export_geometries_with_crs(
    geoms: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => export_geojson_geometry(geoms, path, crs),
        "wkt" => export_wkt_geometry(geoms, path, crs),
        "wkb" => export_wkb_geometry(geoms, path, crs),
        "csv" => export_csv_geometry(geoms, path),
        "dbf" | "shx" => {
            let shp_path = Path::new(path).with_extension("shp");
            let shp_str = shp_path.to_str().ok_or_else(|| {
                MakeValidError::UnsupportedFormat("Invalid path encoding".into())
            })?;
            export_geometries_with_crs(geoms, shp_str, crs)
        }
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                shp_export::export_shp(geoms, path, crs).map_err(|e| io_err(e.to_string()))
            }
            #[cfg(not(feature = "load-shp"))]
            {
                let _ = crs;
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile export requires 'load-shp' feature".into(),
                ))
            }
        }
        "gpkg" => export_gpkg_geometry(geoms, path, crs),
        "gml" | "xml" => gml::export_gml_geometry(geoms, path, crs),
        _ => Err(MakeValidError::UnsupportedFormat(format!(
            "unsupported export format: .{ext} (supported: .geojson, .json, .wkt, .wkb, .csv, .shp, .gpkg, .gml, .xml)"
        ))),
    }
}

fn export_geojson_geometry(
    geoms: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    if geoms.len() == 1 && crs.is_none() {
        let gj = geojson::Geometry::from(&geoms[0]);
        let json =
            serde_json::to_string(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;
        std::fs::write(path, json.as_bytes()).map_err(|e| io_err(e.to_string()))?;
    } else {
        let foreign_members = crs.and_then(|c| {
            let auth = c.authority()?;
            let mut fm = serde_json::Map::new();
            let mut props = serde_json::Map::new();
            props.insert("name".to_string(), serde_json::Value::String(auth.to_string()));
            let mut crs_obj = serde_json::Map::new();
            crs_obj.insert("type".to_string(), serde_json::Value::String("name".to_string()));
            crs_obj.insert("properties".to_string(), serde_json::Value::Object(props));
            fm.insert("crs".to_string(), serde_json::Value::Object(crs_obj));
            Some(fm)
        });

        let features: Vec<geojson::Feature> = geoms
            .iter()
            .map(|geom| {
                let gj_geom = geojson::Geometry::from(geom);
                geojson::Feature {
                    geometry: Some(gj_geom),
                    properties: Some(serde_json::Map::new()),
                    id: None,
                    bbox: None,
                    foreign_members: None,
                }
            })
            .collect();

        let collection = geojson::FeatureCollection {
            features,
            bbox: None,
            foreign_members,
        };
        let gj = geojson::GeoJson::FeatureCollection(collection);
        let json =
            serde_json::to_string(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;
        std::fs::write(path, json.as_bytes()).map_err(|e| io_err(e.to_string()))?;
    }

    Ok(())
}

fn export_wkt_geometry(geoms: &[Geometry<f64>], path: &str, crs: Option<&Crs>) -> Result<(), MakeValidError> {
    use std::fs::File;
    use std::io::Write;
    use wkt::ToWkt;

    let mut file = File::create(path).map_err(|e| io_err(e.to_string()))?;
    let srid_prefix = crs.and_then(|c| c.srid()).map(|s| format!("SRID={s};"));
    for geom in geoms {
        let wkt = geom.to_wkt();
        if let Some(ref prefix) = srid_prefix {
            writeln!(file, "{prefix}{wkt}").map_err(|e| io_err(e.to_string()))?;
        } else {
            writeln!(file, "{wkt}").map_err(|e| io_err(e.to_string()))?;
        }
    }
    Ok(())
}

fn export_wkb_geometry(
    geoms: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    use std::io::Cursor;
    use wkb::writer::{geometry_wkb_size, write_geometry, WriteOptions};

    let mut data = Vec::new();
    let opts = WriteOptions::default();
    for geom in geoms {
        // Convert Rect/Triangle to Polygon for WKB (wkb crate may not handle them)
        let converted = match geom {
            Geometry::Rect(r) => {
                let coords = vec![
                    r.min(),
                    geo::Coord { x: r.max().x, y: r.min().y },
                    r.max(),
                    geo::Coord { x: r.min().x, y: r.max().y },
                    r.min(),
                ];
                Geometry::Polygon(Polygon::new(geo::LineString::new(coords), vec![]))
            }
            Geometry::Triangle(t) => {
                Geometry::Polygon(Polygon::new(
                    geo::LineString::new(vec![t.v1(), t.v2(), t.v3(), t.v1()]),
                    vec![],
                ))
            }
            other => other.clone(),
        };
        let size = geometry_wkb_size(&converted);
        let start = data.len();
        data.resize(start + size, 0);
        write_geometry(&mut Cursor::new(&mut data[start..]), &converted, &opts)
            .map_err(|e| MakeValidError::ParseError(format!("WKB write: {e}")))?;
    }

    // Wrap in EWKB with SRID if available
    let out_data = if let Some(c) = crs
        && let Some(srid) = c.srid() {
        wrap_ewkb(&data, srid)
    } else {
        data
    };

    std::fs::write(path, &out_data).map_err(|e| io_err(e.to_string()))?;
    Ok(())
}

fn wrap_ewkb(wkb: &[u8], srid: i32) -> Vec<u8> {
    if wkb.len() < 5 {
        return wkb.to_vec();
    }
    let geom_type = u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]);
    const EWKB_SRID_FLAG: u32 = 0x20000000;
    let ewkb_type = geom_type | EWKB_SRID_FLAG;
    let mut result = Vec::with_capacity(wkb.len() + 4);
    result.push(wkb[0]);
    result.extend_from_slice(&ewkb_type.to_le_bytes());
    result.extend_from_slice(&srid.to_le_bytes());
    result.extend_from_slice(&wkb[5..]);
    result
}

fn export_csv_geometry(geoms: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    use std::fs::File;
    use std::io::Write;
    use wkt::ToWkt;

    let mut file = File::create(path).map_err(|e| io_err(e.to_string()))?;
    writeln!(file, "geometry").map_err(|e| io_err(e.to_string()))?;
    for geom in geoms {
        let wkt = geom.to_wkt();
        writeln!(file, "\"{wkt}\"").map_err(|e| io_err(e.to_string()))?;
    }
    Ok(())
}

#[cfg(feature = "io-gpkg")]
fn export_gpkg_geometry(geoms: &[Geometry<f64>], path: &str, crs: Option<&Crs>) -> Result<(), MakeValidError> {
    use std::io::Cursor;
    use rusqlite::{params, Connection};
    use wkb::writer::{geometry_wkb_size, write_geometry, WriteOptions};

    let conn = Connection::open(path).map_err(|e| io_err(format!("GPKG create: {e}")))?;

    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS gpkg_spatial_ref_sys (
            srs_id INTEGER PRIMARY KEY,
            organization TEXT NOT NULL,
            organization_coordsys_id INTEGER NOT NULL,
            definition TEXT NOT NULL,
            description TEXT
        );
        CREATE TABLE IF NOT EXISTS gpkg_contents (
            table_name TEXT NOT NULL PRIMARY KEY,
            data_type TEXT NOT NULL DEFAULT 'features',
            identifier TEXT,
            description TEXT,
            last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            min_x REAL, min_y REAL, max_x REAL, max_y REAL,
            srs_id INTEGER,
            FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
        );
        CREATE TABLE IF NOT EXISTS gpkg_geometry_columns (
            table_name TEXT NOT NULL,
            column_name TEXT NOT NULL,
            geometry_type_name TEXT NOT NULL,
            srs_id INTEGER NOT NULL,
            z TINYINT NOT NULL DEFAULT 0,
            m TINYINT NOT NULL DEFAULT 0,
            PRIMARY KEY (table_name, column_name),
            FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
            FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
        );
    ").map_err(|e| io_err(format!("GPKG create tables: {e}")))?;

    let srs_id = match crs.and_then(|c| c.srid()) {
        Some(srid) => {
            conn.execute(
                "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_id, organization, organization_coordsys_id, definition, description) VALUES (?1, ?2, ?3, '', '')",
                params![srid, "EPSG", srid],
            ).map_err(|e| io_err(format!("GPKG insert SRS: {e}")))?;
            srid as i32
        }
        None => {
            conn.execute(
                "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_id, organization, organization_coordsys_id, definition, description) VALUES (4326, 'EPSG', 4326, '', '')",
                [],
            ).map_err(|e| io_err(format!("GPKG insert SRS: {e}")))?;
            4326
        }
    };

    let geom_type_name = if geoms.len() == 1 {
        gpkg_geom_type_name(&geoms[0])
    } else {
        "GEOMETRY"
    };

    conn.execute(
        "INSERT OR IGNORE INTO gpkg_contents (table_name, data_type, srs_id) VALUES ('geometries', 'features', ?1)",
        params![srs_id],
    ).map_err(|e| io_err(format!("GPKG insert contents: {e}")))?;

    conn.execute(
        "INSERT OR IGNORE INTO gpkg_geometry_columns (table_name, column_name, geometry_type_name, srs_id, z, m) VALUES ('geometries', 'geom', ?1, ?2, 0, 0)",
        params![geom_type_name, srs_id],
    ).map_err(|e| io_err(format!("GPKG insert geom columns: {e}")))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS geometries (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom BLOB)",
        [],
    ).map_err(|e| io_err(format!("GPKG create data table: {e}")))?;

    let opts = WriteOptions::default();
    for geom in geoms {
        let converted = match geom {
            Geometry::Rect(r) => Geometry::Polygon(Polygon::new(
                geo::LineString::new(vec![
                    r.min(),
                    geo::Coord { x: r.max().x, y: r.min().y },
                    r.max(),
                    geo::Coord { x: r.min().x, y: r.max().y },
                    r.min(),
                ]),
                vec![],
            )),
            Geometry::Triangle(t) => Geometry::Polygon(Polygon::new(
                geo::LineString::new(vec![t.v1(), t.v2(), t.v3(), t.v1()]),
                vec![],
            )),
            other => other.clone(),
        };
        let size = geometry_wkb_size(&converted);
        let mut buf = vec![0u8; size];
        write_geometry(&mut Cursor::new(&mut buf[..]), &converted, &opts)
            .map_err(|e| MakeValidError::ParseError(format!("WKB write: {e}")))?;
        conn.execute("INSERT INTO geometries (geom) VALUES (?1)", params![buf])
            .map_err(|e| io_err(format!("GPKG insert geometry: {e}")))?;
    }

    Ok(())
}

#[cfg(not(feature = "io-gpkg"))]
fn export_gpkg_geometry(_geoms: &[Geometry<f64>], _path: &str, _crs: Option<&Crs>) -> Result<(), MakeValidError> {
    Err(MakeValidError::UnsupportedFormat(
        "GeoPackage export requires 'io-gpkg' feature".into(),
    ))
}

fn gpkg_geom_type_name(geom: &Geometry<f64>) -> &'static str {
    match geom {
        Geometry::Point(_) => "POINT",
        Geometry::LineString(_) => "LINESTRING",
        Geometry::Polygon(_) => "POLYGON",
        Geometry::MultiPoint(_) => "MULTIPOINT",
        Geometry::MultiLineString(_) => "MULTILINESTRING",
        Geometry::MultiPolygon(_) => "MULTIPOLYGON",
        Geometry::GeometryCollection(_) => "GEOMETRYCOLLECTION",
        Geometry::Line(_) => "LINESTRING",
        Geometry::Rect(_) => "POLYGON",
        Geometry::Triangle(_) => "POLYGON",
    }
}

pub fn export_features(features: &[Feature], path: &str) -> Result<(), MakeValidError> {
    export_features_with_progress(features, path, None)
}

pub fn export_features_with_progress(
    features: &[Feature],
    path: &str,
    progress: Option<&dyn Fn(f64)>,
) -> Result<(), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => export_features_geojson(features, path, progress),
        "dbf" | "shx" => {
            let shp_path = Path::new(path).with_extension("shp");
            let shp_str = shp_path.to_str().ok_or_else(|| {
                MakeValidError::UnsupportedFormat("Invalid path encoding".into())
            })?;
            export_features_with_progress(features, shp_str, progress)
        }
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                let crs = features.first().and_then(|f| f.crs.as_ref());
                shp_export::export_shp_features(features, path, crs, progress)
                    .map_err(|e| io_err(e.to_string()))
            }
            #[cfg(not(feature = "load-shp"))]
            {
                let _ = progress;
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile export requires 'load-shp' feature".into(),
                ))
            }
        }
        "wkb" => {
            let geoms: Vec<Geometry<f64>> = features.iter().map(|f| f.geometry.clone()).collect();
            let crs = features.first().and_then(|f| f.crs.as_ref());
            export_wkb_geometry(&geoms, path, crs)
        }
        _ => {
            let geoms: Vec<Geometry<f64>> = features.iter().map(|f| f.geometry.clone()).collect();
            let crs = features.first().and_then(|f| f.crs.as_ref());
            export_geometries_with_crs(&geoms, path, crs)
        }
    }
}

fn export_features_geojson(
    features: &[Feature],
    path: &str,
    _progress: Option<&dyn Fn(f64)>,
) -> Result<(), MakeValidError> {
    #[cfg(feature = "io-geojson")]
    {
        let foreign_members = features.first().and_then(|f| {
            f.crs.as_ref().and_then(|c| {
                let auth = c.authority()?;
                let mut fm = serde_json::Map::new();
                let mut props = serde_json::Map::new();
                props.insert("name".to_string(), serde_json::Value::String(auth.to_string()));
                let mut crs_obj = serde_json::Map::new();
                crs_obj.insert("type".to_string(), serde_json::Value::String("name".to_string()));
                crs_obj.insert("properties".to_string(), serde_json::Value::Object(props));
                fm.insert("crs".to_string(), serde_json::Value::Object(crs_obj));
                Some(fm)
            })
        });

        let collection = geojson::FeatureCollection {
            features: features
                .iter()
                .map(|feat| geo_geom_to_geojson_feature(feat))
                .collect(),
            bbox: None,
            foreign_members,
        };

        let gj = geojson::GeoJson::FeatureCollection(collection);
        let json =
            serde_json::to_string(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;
        std::fs::write(path, json.as_bytes()).map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(feature = "io-geojson"))]
    {
        let _ = (features, path);
        Err(MakeValidError::UnsupportedFormat(
            "GeoJSON feature export requires 'io-geojson' feature".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_load_geometries_unsupported_format() {
        let result = load_geometries("test.xyz");
        assert!(result.is_err());
    }

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
    fn test_polygon_area_util() {
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

    #[test]
    fn test_wkt_roundtrip() {
        let geom = Geometry::Point(geo::Point::new(1.0, 2.0));
        let path = std::env::temp_dir().join("test_roundtrip.wkt");
        let path_str = path.to_str().unwrap().to_string();

        let result = export_geometries(std::slice::from_ref(&geom), &path_str);
        assert!(result.is_ok());

        let loaded = load_geometries(&path_str);
        assert!(loaded.is_ok());
        let loaded_geoms = loaded.unwrap();
        assert_eq!(loaded_geoms.len(), 1);
        assert_eq!(loaded_geoms[0], geom);

        let _ = std::fs::remove_file(&path);
    }
}

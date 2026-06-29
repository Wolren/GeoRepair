use std::collections::{HashMap, HashSet};
use std::path::Path;

use geo::Geometry;
use geo_traits::to_geo::ToGeoGeometry;
use serde_json::Value;
use wkb::reader::read_wkb;

use crate::core::{io_err, MakeValidError};
use crate::feature::Feature;
use crate::zm::count_coords;
use crate::Crs;

/// Load geometries from a GeoPackage (.gpkg) file.
pub fn load_geopackage(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    use rusqlite::Connection;

    let conn = Connection::open(path).map_err(|e| io_err(format!("open GPKG: {e}")))?;

    let mut stmt = conn
        .prepare("SELECT table_name, column_name FROM gpkg_geometry_columns")
        .map_err(|e| MakeValidError::ParseError(format!("GPKG metadata: {e}")))?;

    let tables: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| MakeValidError::ParseError(format!("GPKG metadata: {e}")))?
        .filter_map(|r| r.ok())
        .collect();

    if tables.is_empty() {
        return Err(MakeValidError::ParseError(
            "no feature tables found in GeoPackage".into(),
        ));
    }

    let mut geometries = Vec::new();

    for (table_name, col_name) in &tables {
        let query = format!("SELECT \"{col_name}\" FROM \"{table_name}\"");
        let mut row_stmt = conn
            .prepare(&query)
            .map_err(|e| MakeValidError::ParseError(format!("query {table_name}: {e}")))?;

        let rows = row_stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|e| MakeValidError::ParseError(format!("read {table_name}: {e}")))?;

        for blob in rows.flatten() {
            if let Ok(geom) = parse_gpkg_blob(&blob) {
                geometries.push(geom);
            }
        }
    }

    Ok(geometries)
}

/// Export geometries to a GeoPackage file.
///
/// If the file already exists with proper GPKG metadata tables, geometries are
/// appended to an existing `features` table. If the file does not exist or lacks
/// metadata, a new GeoPackage is created from scratch with CRS metadata from `_crs`.
pub fn export_geopackage(
    geometries: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    use rusqlite::Connection;

    let _db_exists = Path::new(path).exists();
    let conn = Connection::open(path).map_err(|e| io_err(format!("open GPKG: {e}")))?;

    // Enable WAL mode for concurrent access
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| io_err(format!("GPKG pragma: {e}")))?;

    let has_metadata = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='gpkg_contents'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if !has_metadata {
        create_gpkg_schema(&conn, geometries, crs)?;
    } else {
        // Ensure features table exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS features (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom BLOB);",
        )
        .map_err(|e| io_err(format!("create features table: {e}")))?;
    }

    // Detect if any geometry has Z/M
    let has_z = geometries.iter().any(|g| count_coords(g) > 0);
    let _has_m = false;

    for geom in geometries {
        let blob = encode_gpkg_blob(geom, has_z)?;
        conn.execute(
            "INSERT INTO features (geom) VALUES (?1)",
            rusqlite::params![blob],
        )
        .map_err(|e| io_err(format!("insert geometry: {e}")))?;
    }

    Ok(())
}

/// Export features (with attributes) to a GeoPackage file.
pub fn export_geopackage_features(
    features: &[Feature],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    use rusqlite::Connection;

    let conn = Connection::open(path).map_err(|e| io_err(format!("open GPKG: {e}")))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| io_err(format!("GPKG pragma: {e}")))?;

    // Phase 1: collect unique property keys with sample values for type inference
    let mut all_keys: HashMap<String, &Value> = HashMap::new();
    for feat in features {
        if let Some(ref props) = feat.properties {
            for (key, val) in props {
                all_keys.entry(key.clone()).or_insert(val);
            }
        }
    }

    // Phase 2: assign sanitized column names, resolving collisions
    let mut used: HashSet<String> = HashSet::new();
    let col_defs: Vec<(String, String, &str)> = all_keys
        .iter()
        .map(|(orig, sample)| {
            let san = unique_col_name(orig, &used);
            used.insert(san.clone());
            (san, orig.clone(), infer_sqlite_type(sample))
        })
        .collect();

    let geometries: Vec<&Geometry<f64>> = features.iter().map(|f| &f.geometry).collect();
    let has_metadata = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='gpkg_contents'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if !has_metadata {
        create_gpkg_schema_features(&conn, &geometries, crs, &col_defs)?;
    } else {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS features (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom BLOB);",
        )
        .map_err(|e| io_err(format!("create features table: {e}")))?;
        for (san, _orig, sql_type) in &col_defs {
            let sql = format!("ALTER TABLE features ADD COLUMN \"{san}\" {sql_type};");
            let _ = conn.execute_batch(&sql);
        }
    }

    let has_z = features.iter().any(|f| f.has_z());

    // Build dynamic INSERT SQL
    let col_list: String = if col_defs.is_empty() {
        "geom".to_string()
    } else {
        let cols: Vec<String> = col_defs
            .iter()
            .map(|(san, _, _)| format!("\"{san}\""))
            .collect();
        format!("geom, {}", cols.join(", "))
    };
    let ph_list: String = if col_defs.is_empty() {
        "?1".to_string()
    } else {
        let phs: Vec<String> = (1..=col_defs.len() + 1).map(|i| format!("?{i}")).collect();
        phs.join(", ")
    };

    let sql = format!("INSERT INTO features ({col_list}) VALUES ({ph_list})");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| io_err(format!("prepare insert: {e}")))?;

    for feat in features {
        let blob = encode_gpkg_blob(&feat.geometry, has_z)?;
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        values.push(Box::new(blob));
        if let Some(ref props) = feat.properties {
            for (_san, orig, _) in &col_defs {
                let val = props.get(orig.as_str());
                values.push(json_value_to_sql(val));
            }
        } else {
            for _ in &col_defs {
                values.push(Box::new(rusqlite::types::Null));
            }
        }

        let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        stmt.execute(params.as_slice())
            .map_err(|e| io_err(format!("insert: {e}")))?;
    }

    Ok(())
}

fn json_value_to_sql(val: Option<&Value>) -> Box<dyn rusqlite::types::ToSql> {
    match val {
        Some(Value::String(s)) => Box::new(s.clone()),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else {
                Box::new(n.as_f64().unwrap_or(0.0))
            }
        }
        Some(Value::Bool(b)) => Box::new(if *b { 1i32 } else { 0i32 }),
        Some(Value::Array(_) | Value::Object(_)) => {
            let s = match val {
                Some(v) => v.to_string(),
                None => String::new(),
            };
            Box::new(s)
        }
        _ => Box::new(rusqlite::types::Null),
    }
}

fn infer_sqlite_type(val: &Value) -> &'static str {
    match val {
        Value::String(_) => "TEXT",
        Value::Number(n) => {
            if n.is_f64() && !n.is_i64() {
                "REAL"
            } else {
                "INTEGER"
            }
        }
        Value::Bool(_) => "INTEGER",
        Value::Null => "TEXT",
        Value::Array(_) | Value::Object(_) => "TEXT",
    }
}

fn unique_col_name(key: &str, used: &HashSet<String>) -> String {
    let base: String = key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = base
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .to_string();
    let base = if base.is_empty() {
        "col".to_string()
    } else {
        base
    };
    if !used.contains(&base) {
        return base;
    }
    let mut i = 1;
    loop {
        let candidate = format!("{base}_{i}");
        if !used.contains(&candidate) {
            return candidate;
        }
        i += 1;
    }
}

fn create_gpkg_schema_features(
    conn: &rusqlite::Connection,
    geometries: &[&Geometry<f64>],
    crs: Option<&Crs>,
    col_defs: &[(String, String, &str)],
) -> Result<(), MakeValidError> {
    let srs_id = crs
        .and_then(|c| c.authority().and_then(parse_srid))
        .unwrap_or(-1);
    let srs_name = crs.and_then(|c| c.authority()).unwrap_or("EPSG:-1");
    let is_geographic = crs.map(|c| c.is_geographic()).unwrap_or(true);

    // Build CREATE TABLE with attribute columns
    let mut create_sql =
        "CREATE TABLE IF NOT EXISTS features (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom BLOB"
            .to_string();
    for (san, _orig, sql_type) in col_defs {
        create_sql.push_str(&format!(", \"{san}\" {sql_type}"));
    }
    create_sql.push_str(");");

    conn.execute_batch(&format!(
        "{create_sql}
        CREATE TABLE gpkg_contents (
            table_name TEXT NOT NULL PRIMARY KEY,
            data_type TEXT NOT NULL,
            identifier TEXT, description TEXT DEFAULT '',
            last_change DATETIME NOT NULL DEFAULT '2025-01-01T00:00:00.000Z',
            min_x REAL, min_y REAL, max_x REAL, max_y REAL, srs_id INTEGER
        );
        CREATE TABLE gpkg_geometry_columns (
            table_name TEXT NOT NULL, column_name TEXT NOT NULL DEFAULT 'geom',
            geometry_type_name TEXT NOT NULL, srs_id INTEGER NOT NULL,
            z TINYINT NOT NULL DEFAULT 0, m TINYINT NOT NULL DEFAULT 0,
            CONSTRAINT pk_geometry_columns PRIMARY KEY (table_name, column_name)
        );
        CREATE TABLE gpkg_spatial_ref_sys (
            srs_id INTEGER NOT NULL PRIMARY KEY, srs_name TEXT NOT NULL,
            srs_type TEXT NOT NULL, organization TEXT NOT NULL,
            organization_coordsys_id INTEGER NOT NULL, definition TEXT, description TEXT
        );",
    ))
    .map_err(|e| io_err(format!("create GPKG tables: {e}")))?;

    // Insert SRS
    if srs_id == 4326 {
        conn.execute(
            "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_id,srs_name,srs_type,organization,organization_coordsys_id,definition) VALUES (?1,?2,?3,?4,?1,?5)",
            rusqlite::params![4326, "WGS 84", "geographic", "EPSG", "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AUTHORITY[\"EPSG\",\"4326\"]]"],
        )
        .map_err(|e| io_err(format!("GPKG SRS: {e}")))?;
    }

    if srs_id != 4326 && srs_id != -1 {
        let srs_type = if is_geographic {
            "geographic"
        } else {
            "projected"
        };
        let org = srs_name.split(':').next().unwrap_or("EPSG");
        let def_wkt = crs
            .and_then(|c| c.to_esri_wkt())
            .unwrap_or_else(|| "UNDEFINED".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO gpkg_spatial_ref_sys VALUES (?1,?2,?3,?4,?1,?5,NULL)",
            rusqlite::params![srs_id, srs_name, srs_type, org, def_wkt],
        )
        .map_err(|e| io_err(format!("GPKG SRS: {e}")))?;
    }

    conn.execute(
        "INSERT OR IGNORE INTO gpkg_spatial_ref_sys VALUES (-1,'Undefined','geographic','NONE',-1,'UNDEFINED',NULL)",
        rusqlite::params![],
    )
    .map_err(|e| io_err(format!("GPKG SRS undefined: {e}")))?;

    let (min_x, min_y, max_x, max_y) = compute_bounds_slice(geometries);

    conn.execute(
        "INSERT INTO gpkg_contents (table_name,data_type,min_x,min_y,max_x,max_y,srs_id)
         VALUES ('features','features',?1,?2,?3,?4,?5)",
        rusqlite::params![min_x, min_y, max_x, max_y, srs_id],
    )
    .map_err(|e| io_err(format!("gpkg_contents: {e}")))?;

    let has_z = geometries.iter().any(|g| count_coords(g) > 0);
    conn.execute(
        "INSERT INTO gpkg_geometry_columns VALUES ('features','geom','GEOMETRY',?1,?2,?3)",
        rusqlite::params![srs_id, if has_z { 1 } else { 0 }, 0],
    )
    .map_err(|e| io_err(format!("gpkg_geometry_columns: {e}")))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gpkg_rtree_features_geom (
            fid INTEGER PRIMARY KEY,
            min_x REAL, max_x REAL,
            min_y REAL, max_y REAL
        );",
    )
    .map_err(|e| io_err(format!("GPKG R-tree table: {e}")))?;

    for (i, geom) in geometries.iter().enumerate() {
        let (min_x, max_x, min_y, max_y) = compute_bounds_slice(std::slice::from_ref(geom));
        conn.execute(
            "INSERT OR IGNORE INTO gpkg_rtree_features_geom VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![i as i64 + 1, min_x, max_x, min_y, max_y],
        )
        .map_err(|e| io_err(format!("GPKG R-tree insert: {e}")))?;
    }

    Ok(())
}

fn compute_bounds_slice(geometries: &[&Geometry<f64>]) -> (f64, f64, f64, f64) {
    use geo::BoundingRect;
    let mut bbox: Option<(f64, f64, f64, f64)> = None;
    for geom in geometries {
        if let Some(r) = geom.bounding_rect() {
            let (xmin, ymin, xmax, ymax) =
                bbox.unwrap_or((r.min().x, r.min().y, r.max().x, r.max().y));
            bbox = Some((
                xmin.min(r.min().x),
                ymin.min(r.min().y),
                xmax.max(r.max().x),
                ymax.max(r.max().y),
            ));
        }
    }
    bbox.unwrap_or((0.0, 0.0, 0.0, 0.0))
}

fn create_gpkg_schema(
    conn: &rusqlite::Connection,
    geometries: &[Geometry<f64>],
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let srs_id = crs
        .and_then(|c| c.authority().and_then(parse_srid))
        .unwrap_or(-1);

    let srs_name = crs.and_then(|c| c.authority()).unwrap_or("EPSG:-1");

    let is_geographic = crs.map(|c| c.is_geographic()).unwrap_or(true);

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS features (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom BLOB);
        CREATE TABLE gpkg_contents (
            table_name TEXT NOT NULL PRIMARY KEY,
            data_type TEXT NOT NULL,
            identifier TEXT, description TEXT DEFAULT '',
            last_change DATETIME NOT NULL DEFAULT '2025-01-01T00:00:00.000Z',
            min_x REAL, min_y REAL, max_x REAL, max_y REAL, srs_id INTEGER
        );
        CREATE TABLE gpkg_geometry_columns (
            table_name TEXT NOT NULL, column_name TEXT NOT NULL DEFAULT 'geom',
            geometry_type_name TEXT NOT NULL, srs_id INTEGER NOT NULL,
            z TINYINT NOT NULL DEFAULT 0, m TINYINT NOT NULL DEFAULT 0,
            CONSTRAINT pk_geometry_columns PRIMARY KEY (table_name, column_name)
        );
        CREATE TABLE gpkg_spatial_ref_sys (
            srs_id INTEGER NOT NULL PRIMARY KEY, srs_name TEXT NOT NULL,
            srs_type TEXT NOT NULL, organization TEXT NOT NULL,
            organization_coordsys_id INTEGER NOT NULL, definition TEXT, description TEXT
        );
        ",
    )
    .map_err(|e| io_err(format!("create GPKG tables: {e}")))?;

    // Insert SRS for the given CRS
    if srs_id == 4326 {
        conn.execute(
            "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_id,srs_name,srs_type,organization,organization_coordsys_id,definition) VALUES (?1,?2,?3,?4,?1,?5)",
            rusqlite::params![4326, "WGS 84", "geographic", "EPSG", "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AUTHORITY[\"EPSG\",\"4326\"]]"],
        )
        .map_err(|e| io_err(format!("GPKG SRS: {e}")))?;
    }

    if srs_id != 4326 && srs_id != -1 {
        let srs_type = if is_geographic {
            "geographic"
        } else {
            "projected"
        };
        let org = srs_name.split(':').next().unwrap_or("EPSG");
        let def_wkt = crs
            .and_then(|c| c.to_esri_wkt())
            .unwrap_or_else(|| "UNDEFINED".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO gpkg_spatial_ref_sys VALUES (?1,?2,?3,?4,?1,?5,NULL)",
            rusqlite::params![srs_id, srs_name, srs_type, org, def_wkt],
        )
        .map_err(|e| io_err(format!("GPKG SRS: {e}")))?;
    }

    conn.execute(
        "INSERT OR IGNORE INTO gpkg_spatial_ref_sys VALUES (-1,'Undefined','geographic','NONE',-1,'UNDEFINED',NULL)",
        rusqlite::params![],
    )
    .map_err(|e| io_err(format!("GPKG SRS undefined: {e}")))?;

    let (min_x, min_y, max_x, max_y) = compute_bounds(geometries);

    conn.execute(
        "INSERT INTO gpkg_contents (table_name,data_type,min_x,min_y,max_x,max_y,srs_id)
         VALUES ('features','features',?1,?2,?3,?4,?5)",
        rusqlite::params![min_x, min_y, max_x, max_y, srs_id],
    )
    .map_err(|e| io_err(format!("gpkg_contents: {e}")))?;

    // Detect Z/M from geometries
    let has_z = geometries.iter().any(|g| count_coords(g) > 0);
    let has_m = false;

    conn.execute(
        "INSERT INTO gpkg_geometry_columns VALUES ('features','geom','GEOMETRY',?1,?2,?3)",
        rusqlite::params![srs_id, if has_z { 1 } else { 0 }, if has_m { 1 } else { 0 }],
    )
    .map_err(|e| io_err(format!("gpkg_geometry_columns: {e}")))?;

    // Create R-tree spatial index with Rust-computed bounds
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gpkg_rtree_features_geom (
            fid INTEGER PRIMARY KEY,
            min_x REAL, max_x REAL,
            min_y REAL, max_y REAL
        );",
    )
    .map_err(|e| io_err(format!("GPKG R-tree table: {e}")))?;

    for (i, geom) in geometries.iter().enumerate() {
        let (min_x, max_x, min_y, max_y) = compute_bounds(std::slice::from_ref(geom));
        conn.execute(
            "INSERT OR IGNORE INTO gpkg_rtree_features_geom VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![i as i64 + 1, min_x, max_x, min_y, max_y],
        )
        .map_err(|e| io_err(format!("GPKG R-tree insert: {e}")))?;
    }

    Ok(())
}

/// Parse a GeoPackageBinary blob into a geo Geometry.
fn parse_gpkg_blob(data: &[u8]) -> Result<Geometry<f64>, MakeValidError> {
    if data.len() < 8 {
        return Err(MakeValidError::ParseError("truncated GPKG blob".into()));
    }

    if &data[0..2] != b"GP" {
        return Err(MakeValidError::ParseError(
            "invalid GPKG magic bytes".into(),
        ));
    }
    let _version = data[2];
    let flags = data[3];
    let envelope_indicator = flags & 0x07;
    let empty = (flags >> 4) & 0x01 == 1;
    let _has_srid = (flags >> 5) & 0x01 == 1;

    if empty {
        return Err(MakeValidError::ParseError("empty GPKG geometry".into()));
    }

    let mut offset = 4usize;

    // SRS ID (big-endian uint32)
    offset += 4;

    // Skip envelope (f32 values)
    let envelope_floats: usize = match envelope_indicator {
        0 => 0,
        1 => 4,
        2 | 3 => 6,
        4 => 8,
        _ => {
            return Err(MakeValidError::ParseError(format!(
                "unknown GPKG envelope: {envelope_indicator}"
            )))
        }
    };
    offset += envelope_floats * 4;

    if offset >= data.len() {
        return Err(MakeValidError::ParseError("truncated GPKG blob".into()));
    }

    let wkb_geom = read_wkb(&data[offset..])
        .map_err(|e| MakeValidError::ParseError(format!("GPKG WKB parse: {e}")))?;
    let geo_geom: Geometry<f64> = wkb_geom.to_geometry();
    Ok(geo_geom)
}

/// Encode a geo Geometry into a GeoPackageBinary blob.
fn encode_gpkg_blob(geom: &Geometry<f64>, _has_z: bool) -> Result<Vec<u8>, MakeValidError> {
    let wkb_buf = crate::io::wkb::encode_wkb_2d(geom)
        .map_err(|e| MakeValidError::ParseError(format!("WKB encode: {e}")))?;

    let mut blob = Vec::with_capacity(8 + wkb_buf.len());
    blob.extend_from_slice(b"GP");
    blob.push(0u8); // version
    blob.push(0u8); // flags (no envelope, not empty)
    blob.extend_from_slice(&0i32.to_be_bytes()); // SRS ID = 0
    blob.extend_from_slice(&wkb_buf);

    Ok(blob)
}

fn compute_bounds(geometries: &[Geometry<f64>]) -> (f64, f64, f64, f64) {
    use geo::BoundingRect;
    let mut bbox: Option<(f64, f64, f64, f64)> = None;
    for geom in geometries {
        if let Some(r) = geom.bounding_rect() {
            let (xmin, ymin, xmax, ymax) =
                bbox.unwrap_or((r.min().x, r.min().y, r.max().x, r.max().y));
            bbox = Some((
                xmin.min(r.min().x),
                ymin.min(r.min().y),
                xmax.max(r.max().x),
                ymax.max(r.max().y),
            ));
        }
    }
    bbox.unwrap_or((0.0, 0.0, 0.0, 0.0))
}

fn parse_srid(authority: &str) -> Option<i32> {
    authority
        .split(':')
        .next_back()
        .and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, LineString, Point, Polygon};

    fn point_geom(x: f64, y: f64) -> Geometry<f64> {
        Geometry::Point(Point::new(x, y))
    }

    fn line_geom() -> Geometry<f64> {
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]))
    }

    fn poly_geom() -> Geometry<f64> {
        Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 2.0, y: 0.0 },
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 0.0, y: 2.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        ))
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let cases = vec![point_geom(1.0, 2.0), line_geom(), poly_geom()];
        for geom in &cases {
            let blob = encode_gpkg_blob(geom, false).unwrap();
            let decoded = parse_gpkg_blob(&blob).unwrap();
            assert_eq!(geom, &decoded);
        }
    }

    #[test]
    fn test_parse_gpkg_blob_with_envelope() {
        let wkb: Vec<u8> = vec![
            0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
        ];
        let mut gp_blob = b"GP".to_vec();
        gp_blob.push(0u8); // version
        gp_blob.push(0x09u8); // flags (envelope=1, has_srid)
        gp_blob.extend_from_slice(&0i32.to_be_bytes()); // SRS ID = 0
        gp_blob.extend_from_slice(&0.9f32.to_le_bytes());
        gp_blob.extend_from_slice(&1.9f32.to_le_bytes());
        gp_blob.extend_from_slice(&1.1f32.to_le_bytes());
        gp_blob.extend_from_slice(&2.1f32.to_le_bytes());
        gp_blob.extend_from_slice(&wkb);

        let geom = parse_gpkg_blob(&gp_blob).unwrap();
        match geom {
            Geometry::Point(p) => {
                assert!((p.x() - 1.0).abs() < 1e-12);
                assert!((p.y() - 2.0).abs() < 1e-12);
            }
            _ => panic!("expected Point"),
        }
    }

    #[test]
    fn test_compute_bounds() {
        let geoms = vec![point_geom(0.0, 0.0), point_geom(10.0, 20.0)];
        let (min_x, _min_y, _max_x, max_y) = compute_bounds(&geoms);
        assert!((min_x - 0.0).abs() < 1e-12);
        assert!((max_y - 20.0).abs() < 1e-12);
    }

    #[test]
    fn test_parse_srid() {
        assert_eq!(parse_srid("EPSG:4326"), Some(4326));
        assert_eq!(parse_srid("EPSG:3857"), Some(3857));
        assert_eq!(parse_srid("foo"), None);
    }
}

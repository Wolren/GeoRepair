use std::path::Path;

use geo::Geometry;
use geo_traits::to_geo::ToGeoGeometry;
use wkb::reader::read_wkb;

use crate::core::MakeValidError;
use crate::zm::count_coords;
use crate::Crs;

/// Load geometries from a GeoPackage (.gpkg) file.
pub fn load_geopackage(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    use rusqlite::Connection;

    let conn =
        Connection::open(path).map_err(|e| MakeValidError::IoError(format!("open GPKG: {e}")))?;

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
    let conn =
        Connection::open(path).map_err(|e| MakeValidError::IoError(format!("open GPKG: {e}")))?;

    // Enable WAL mode for concurrent access
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| MakeValidError::IoError(format!("GPKG pragma: {e}")))?;

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
        .map_err(|e| MakeValidError::IoError(format!("create features table: {e}")))?;
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
        .map_err(|e| MakeValidError::IoError(format!("insert geometry: {e}")))?;
    }

    Ok(())
}

fn create_gpkg_schema(
    conn: &rusqlite::Connection,
    geometries: &[Geometry<f64>],
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let srs_id = crs
        .and_then(|c| c.authority().and_then(|a| parse_srid(a)))
        .unwrap_or(4326);

    let srs_name = crs.and_then(|c| c.authority()).unwrap_or("EPSG:4326");

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
    .map_err(|e| MakeValidError::IoError(format!("create GPKG tables: {e}")))?;

    // Insert SRS for the given CRS
    if srs_id == 4326 || srs_id == -1 {
        conn.execute(
            "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_id,srs_name,srs_type,organization,organization_coordsys_id,definition) VALUES (?1,?2,?3,?4,?1,?5)",
            rusqlite::params![4326, "WGS 84", "geographic", "EPSG", "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AUTHORITY[\"EPSG\",\"4326\"]]"],
        )
        .map_err(|e| MakeValidError::IoError(format!("GPKG SRS: {e}")))?;
    }

    if srs_id != 4326 {
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
        .map_err(|e| MakeValidError::IoError(format!("GPKG SRS: {e}")))?;
    }

    conn.execute(
        "INSERT OR IGNORE INTO gpkg_spatial_ref_sys VALUES (-1,'Undefined','geographic','NONE',-1,'UNDEFINED',NULL)",
        rusqlite::params![],
    )
    .map_err(|e| MakeValidError::IoError(format!("GPKG SRS undefined: {e}")))?;

    let (min_x, min_y, max_x, max_y) = compute_bounds(geometries);

    conn.execute(
        "INSERT INTO gpkg_contents (table_name,data_type,min_x,min_y,max_x,max_y,srs_id)
         VALUES ('features','features',?1,?2,?3,?4,?5)",
        rusqlite::params![min_x, min_y, max_x, max_y, srs_id],
    )
    .map_err(|e| MakeValidError::IoError(format!("gpkg_contents: {e}")))?;

    // Detect Z/M from geometries
    let has_z = geometries.iter().any(|g| count_coords(g) > 0);
    let has_m = false;

    conn.execute(
        "INSERT INTO gpkg_geometry_columns VALUES ('features','geom','GEOMETRY',?1,?2,?3)",
        rusqlite::params![srs_id, if has_z { 1 } else { 0 }, if has_m { 1 } else { 0 }],
    )
    .map_err(|e| MakeValidError::IoError(format!("gpkg_geometry_columns: {e}")))?;

    // Create R-tree spatial index with Rust-computed bounds
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gpkg_rtree_features_geom (
            fid INTEGER PRIMARY KEY,
            min_x REAL, max_x REAL,
            min_y REAL, max_y REAL
        );",
    )
    .map_err(|e| MakeValidError::IoError(format!("GPKG R-tree table: {e}")))?;

    for (i, geom) in geometries.iter().enumerate() {
        let (min_x, max_x, min_y, max_y) = compute_bounds(std::slice::from_ref(geom));
        conn.execute(
            "INSERT OR IGNORE INTO gpkg_rtree_features_geom VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![i as i64 + 1, min_x, max_x, min_y, max_y],
        )
        .map_err(|e| MakeValidError::IoError(format!("GPKG R-tree insert: {e}")))?;
    }

    Ok(())
}

/// Parse a GeoPackageBinary blob into a geo Geometry.
fn parse_gpkg_blob(data: &[u8]) -> Result<Geometry<f64>, MakeValidError> {
    if data.is_empty() {
        return Err(MakeValidError::ParseError("empty GPKG blob".into()));
    }

    let flags = data[0];
    let envelope_indicator = flags & 0x07;
    let empty = (flags >> 4) & 0x01 == 1;
    let _has_srid = (flags >> 5) & 0x01 == 1;

    if empty {
        return Err(MakeValidError::ParseError("empty GPKG geometry".into()));
    }

    let mut offset = 1usize;

    // Skip SRID (big-endian uint32)
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

    let mut blob = Vec::with_capacity(5 + wkb_buf.len());
    blob.push(0u8);
    blob.extend_from_slice(&0i32.to_be_bytes());
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
    authority.split(':').last().and_then(|s| s.parse().ok())
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
        let mut gp_blob = vec![0x09u8];
        gp_blob.extend_from_slice(&0i32.to_be_bytes());
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

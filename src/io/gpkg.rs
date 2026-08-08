//! GeoPackage (`.gpkg`) backend: OGC 12-128r17 subset via `rusqlite`.
//!
//! Reads every table registered in `gpkg_geometry_columns` and parses the
//! geometry column blobs with the crate's own WKB reader. Writes a minimal
//! conforming GeoPackage (WGS 84 / EPSG:4326) with a single `georepair`
//! feature table. All SQLite identifiers are double-quoted and escaped.

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use std::fs;

use geo::{Coord, Geometry};

use super::wkb::{read_wkb, write_wkb};

const APPLICATION_ID: i32 = 0x4750_4B47; // "GPKG"

/// Load every geometry from a GeoPackage file.
pub fn load_gpkg(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    let conn = open_readonly(path)?;
    let mut out = Vec::new();
    {
        // Which tables carry geometry, and which column in each.
        let mut stmt = conn
            .prepare("SELECT table_name, column_name FROM gpkg_geometry_columns")
            .map_err(|e| format!("{path}: cannot read gpkg_geometry_columns: {e}"))?;
        let tables: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| format!("{path}: {e}"))?
            .filter_map(Result::ok)
            .collect();
        for (table, col) in tables {
            let sql = format!(
                "SELECT {} FROM {} WHERE {} IS NOT NULL",
                quote(&col),
                quote(&table),
                quote(&col)
            );
            let mut q = conn.prepare(&sql).map_err(|e| format!("{path}: {e}"))?;
            let blobs = q
                .query_map([], |r| r.get::<_, Vec<u8>>(0))
                .map_err(|e| format!("{path}: {e}"))?;
            for blob in blobs {
                let blob = blob.map_err(|e| format!("{path}: {e}"))?;
                let wkb =
                    strip_gpkg_header(&blob).map_err(|e| format!("{path}: {table}.{col}: {e}"))?;
                match read_wkb(wkb) {
                    Ok(g) => out.push(g),
                    Err(e) => return Err(format!("{path}: WKB parse error in {table}.{col}: {e}")),
                }
            }
        }
    }
    Ok(out)
}

/// Strip the GeoPackage binary header (OGC 12-128r15, section 6.1) from a
/// geometry blob, returning the embedded WKB. The header is `GP`, version,
/// flags (bit 1 = envelope present, bits 2-3 = envelope type), and a 4-byte
/// SRS id, followed by an optional envelope and then the WKB. Blobs without
/// the magic (older headerless writers) pass through unchanged.
fn strip_gpkg_header(blob: &[u8]) -> Result<&[u8], String> {
    if blob.len() < 8 || blob[0] != b'G' || blob[1] != b'P' {
        return Ok(blob);
    }
    let flags = blob[3];
    let mut off = 8usize;
    if flags & 0x02 != 0 {
        let env_type = (flags >> 2) & 0x03;
        off += match env_type {
            0 => 32,     // XY envelope
            1 | 2 => 48, // XYZ / XYM
            _ => 64,     // XYZM
        };
    }
    if off > blob.len() {
        return Err("invalid GeoPackage header: envelope overruns blob".to_string());
    }
    Ok(&blob[off..])
}

/// Write geometries to a new GeoPackage file (EPSG:4326, table `georepair`).
pub fn save_gpkg(path: &str, geoms: &[Geometry<f64>]) -> Result<(), String> {
    if geoms.is_empty() {
        return Err(format!("{path}: no geometries to write"));
    }
    let _ = fs::remove_file(path);
    let mut conn = rusqlite::Connection::open(path).map_err(|e| format!("{path}: {e}"))?;
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(|e| format!("{path}: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE gpkg_spatial_ref_sys (
             srs_name TEXT NOT NULL,
             srs_id INTEGER PRIMARY KEY,
             organization TEXT NOT NULL,
             organization_coordsys_id INTEGER NOT NULL,
             definition TEXT NOT NULL,
             description TEXT
         );
         INSERT INTO gpkg_spatial_ref_sys VALUES
             ('WGS 84 geodetic', 4326, 'EPSG', 4326, 'GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AUTHORITY[\"EPSG\",\"4326\"]]', NULL);
         CREATE TABLE gpkg_contents (
             table_name TEXT NOT NULL PRIMARY KEY,
             data_type TEXT NOT NULL,
             identifier TEXT UNIQUE,
             description TEXT DEFAULT '',
             last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
             min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE,
             srs_id INTEGER
         );
         CREATE TABLE gpkg_geometry_columns (
             table_name TEXT NOT NULL,
             column_name TEXT NOT NULL,
             geometry_type_name TEXT NOT NULL,
             srs_id INTEGER NOT NULL,
             z INTEGER NOT NULL,
             m INTEGER NOT NULL,
             CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name)
         );
         CREATE TABLE \"georepair\" (
             fid INTEGER PRIMARY KEY AUTOINCREMENT,
             geom BLOB NOT NULL
         );",
    )
    .map_err(|e| format!("{path}: schema: {e}"))?;

    // Bounding box over all geometries (only polygons contribute).
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for g in geoms {
        for c in coords_of(g) {
            min_x = min_x.min(c.x);
            min_y = min_y.min(c.y);
            max_x = max_x.max(c.x);
            max_y = max_y.max(c.y);
        }
    }
    let (min_x, min_y, max_x, max_y) = if min_x <= max_x {
        (min_x, min_y, max_x, max_y)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    conn.execute(
        "INSERT INTO gpkg_contents (table_name, data_type, identifier, min_x, min_y, max_x, max_y, srs_id)
         VALUES ('georepair', 'features', 'georepair', ?1, ?2, ?3, ?4, 4326)",
        rusqlite::params![min_x, min_y, max_x, max_y],
    )
    .map_err(|e| format!("{path}: contents: {e}"))?;
    conn.execute(
        "INSERT INTO gpkg_geometry_columns (table_name, column_name, geometry_type_name, srs_id, z, m)
         VALUES ('georepair', 'geom', 'GEOMETRY', 4326, 0, 0)",
        [],
    )
    .map_err(|e| format!("{path}: geom cols: {e}"))?;

    {
        // One transaction for the whole batch: autocommit per insert does a
        // journal commit + fsync PER FEATURE - 1.58M rows measured at
        // minutes instead of seconds (2026-08-06, QGIS-export run).
        let tx = conn
            .transaction()
            .map_err(|e| format!("{path}: begin tx: {e}"))?;
        {
            let mut ins = tx
                .prepare("INSERT INTO \"georepair\" (geom) VALUES (?1)")
                .map_err(|e| format!("{path}: {e}"))?;
            for g in geoms {
                // GeoPackage binary header (OGC 12-128r15 6.1): 'GP',
                // version 0, flags 0x01 (little-endian, no envelope),
                // SRS id 4326 LE. GDAL/QGIS reject headerless blobs.
                let mut blob = Vec::with_capacity(8 + 64);
                blob.extend_from_slice(&[0x47, 0x50, 0x00, 0x01]);
                blob.extend_from_slice(&4326i32.to_le_bytes());
                blob.extend_from_slice(&write_wkb(g));
                ins.execute(rusqlite::params![blob])
                    .map_err(|e| format!("{path}: insert: {e}"))?;
            }
        }
        tx.commit().map_err(|e| format!("{path}: commit: {e}"))?;
    }
    Ok(())
}

fn open_readonly(path: &str) -> Result<rusqlite::Connection, String> {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{path}: cannot open GeoPackage: {e}"))
}

/// Double-quote an SQLite identifier, escaping embedded quotes.
fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Collect all coordinates from a geometry (polygons and lines only;
/// points are included as single coordinates).
fn coords_of(g: &Geometry<f64>) -> Vec<Coord<f64>> {
    let mut out = Vec::new();
    match g {
        Geometry::Point(p) => out.push(p.0),
        Geometry::LineString(ls) => out.extend(ls.0.iter().copied()),
        Geometry::Polygon(p) => {
            out.extend(p.exterior().0.iter().copied());
            for h in p.interiors() {
                out.extend(h.0.iter().copied());
            }
        }
        Geometry::MultiPoint(mp) => out.extend(mp.0.iter().map(|p| p.0)),
        Geometry::MultiLineString(ml) => {
            for ls in &ml.0 {
                out.extend(ls.0.iter().copied());
            }
        }
        Geometry::MultiPolygon(mp) => {
            for p in &mp.0 {
                out.extend(p.exterior().0.iter().copied());
                for h in p.interiors() {
                    out.extend(h.0.iter().copied());
                }
            }
        }
        Geometry::GeometryCollection(gc) => {
            for g in &gc.0 {
                out.extend(coords_of(g));
            }
        }
        Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::Line(_) => {}
    }
    out
}

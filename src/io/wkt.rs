use std::fs::File;
use std::io::{BufWriter, Write};
use std::str::FromStr;

use geo::Geometry;
use wkt::{ToWkt, Wkt};

use crate::core::MakeValidError;
use crate::Crs;

fn strip_srid_prefix(content: &str) -> (&str, Option<i32>) {
    let trimmed = content.trim();
    if let Some(rest) = trimmed.strip_prefix("SRID=") {
        if let Some(semi_pos) = rest.find(';') {
            let srid_str = &rest[..semi_pos];
            if let Ok(srid) = srid_str.parse::<i32>() {
                return (&rest[semi_pos + 1..], Some(srid));
            }
        }
    }
    (trimmed, None)
}

/// Load WKT geometry file. Supports optional `SRID=...;` prefix.
pub fn load_wkt(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    load_wkt_with_crs(path).map(|(geoms, _)| geoms)
}

/// Load WKT geometry file and extract CRS from `SRID=...;` prefix.
pub fn load_wkt_with_crs(path: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    let content =
        std::fs::read_to_string(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let (body, srid) = strip_srid_prefix(&content);
    let crs = srid.map(|s| Crs::from_epsg(s as u32));

    let wkt = Wkt::from_str(body)
        .map_err(|e| MakeValidError::ParseError(format!("WKT parse error: {e}")))?;
    let geom: Geometry<f64> = wkt
        .try_into()
        .map_err(|e| MakeValidError::ParseError(format!("WKT parse error: {e}")))?;

    Ok((extract_geometries(geom), crs))
}

/// Write geometries as WKT (one per line). Optionally prepended with `SRID=...;`.
pub fn export_wkt(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    export_wkt_with_crs(geometries, path, None)
}

/// Write geometries as WKT with optional SRID prefix from CRS.
pub fn export_wkt_with_crs(
    geometries: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let srid_prefix = crs
        .and_then(|c| c.srid())
        .map(|s| format!("SRID={s};"))
        .unwrap_or_default();
    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    for geom in geometries {
        write!(writer, "{srid_prefix}").map_err(|e| MakeValidError::IoError(e.to_string()))?;
        let wkt = geom.to_wkt();
        write!(writer, "{wkt}").map_err(|e| MakeValidError::IoError(e.to_string()))?;
        writeln!(writer).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    }
    Ok(())
}

fn extract_geometries(geom: Geometry<f64>) -> Vec<Geometry<f64>> {
    match geom {
        Geometry::GeometryCollection(gc) => gc.0,
        other => vec![other],
    }
}

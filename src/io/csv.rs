use std::fs::File;
use std::io::{BufWriter, Write};
use std::str::FromStr;

use geo::Geometry;
use wkt::{ToWkt, Wkt};

use crate::core::MakeValidError;

/// Load geometries from a CSV file with a `geometry` column containing WKT.
pub fn load_csv_wkt(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let file = File::open(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut reader = csv::Reader::from_reader(file);
    let headers = reader
        .headers()
        .map_err(|e| MakeValidError::ParseError(e.to_string()))?;
    let geom_idx = headers
        .iter()
        .position(|h| h == "geometry")
        .ok_or_else(|| MakeValidError::ParseError("missing 'geometry' column".to_string()))?;
    let mut geometries = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| MakeValidError::ParseError(e.to_string()))?;
        let wkt_str = record
            .get(geom_idx)
            .ok_or_else(|| MakeValidError::ParseError("missing geometry field".to_string()))?;
        let wkt = Wkt::from_str(wkt_str)
            .map_err(|e| MakeValidError::ParseError(format!("WKT parse error: {e}")))?;
        let geom: Geometry<f64> = wkt
            .try_into()
            .map_err(|e| MakeValidError::ParseError(format!("WKT convert error: {e}")))?;
        geometries.extend(extract_geometries(geom));
    }
    Ok(geometries)
}

/// Export geometries as CSV with a `geometry` column (WKT).
pub fn export_csv_wkt(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "geometry").map_err(|e| MakeValidError::IoError(e.to_string()))?;
    for geom in geometries {
        let wkt = geom.to_wkt();
        writeln!(writer, "\"{wkt}\"").map_err(|e| MakeValidError::IoError(e.to_string()))?;
    }
    Ok(())
}

fn extract_geometries(geom: Geometry<f64>) -> Vec<Geometry<f64>> {
    match geom {
        Geometry::GeometryCollection(gc) => gc.0,
        other => vec![other],
    }
}

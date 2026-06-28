use std::fs::File;
use std::io::{BufWriter, Write};

use geo::Geometry;
use geozero::csv::CsvString;
use geozero::ToGeo;
use geozero::wkt::WktWriter;

use crate::core::MakeValidError;

pub fn load_csv_wkt(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let content =
        std::fs::read_to_string(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let csv = CsvString::new("geometry", content);
    let geom = csv
        .to_geo()
        .map_err(|e| MakeValidError::ParseError(format!("CSV parse error: {e}")))?;
    Ok(extract_geometries(geom))
}

pub fn export_csv_wkt(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "geometry").map_err(|e| MakeValidError::IoError(e.to_string()))?;
    for geom in geometries {
        let mut wkt_buf = Vec::new();
        {
            let mut wkt_writer = WktWriter::new(&mut wkt_buf);
            wkt_writer.write_geometry(geom).map_err(|e| MakeValidError::ParseError(format!("WKT error: {e}")))?;
        }
        let wkt_str = String::from_utf8(wkt_buf)
            .map_err(|e| MakeValidError::ParseError(format!("UTF-8 error: {e}")))?;
        writeln!(writer, "\"{wkt_str}\"").map_err(|e| MakeValidError::IoError(e.to_string()))?;
    }
    Ok(())
}

fn extract_geometries(geom: Geometry<f64>) -> Vec<Geometry<f64>> {
    match geom {
        Geometry::GeometryCollection(gc) => gc.0,
        other => vec![other],
    }
}

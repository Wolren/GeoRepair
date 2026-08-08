//! CSV backend (`.csv`): one geometry per row, first column = WKT.
//!
//! The writer emits a single column of WKT. The reader accepts any CSV with
//! a WKT geometry in the first column and ignores additional attribute
//! columns (the `Vec<Geometry>` return type cannot carry attributes).

use alloc::string::String;
use alloc::vec::Vec;
use std::fs;

use geo::Geometry;

use super::wkt::{read_wkt, write_wkt};

/// Load geometries from a CSV file (WKT in the first column).
pub fn load_csv(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(data.as_bytes());
    let mut out = Vec::new();
    for (i, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| format!("{path}: row {}: {e}", i + 1))?;
        let Some(wkt) = rec.get(0) else {
            return Err(format!("{path}: row {}: empty row", i + 1));
        };
        let wkt = wkt.trim();
        if wkt.is_empty() {
            continue;
        }
        let g = read_wkt(wkt).map_err(|e| format!("{path}: row {}: {e}", i + 1))?;
        out.push(g);
    }
    Ok(out)
}

/// Save geometries to a CSV file, one WKT geometry per row.
pub fn save_csv(path: &str, geoms: &[Geometry<f64>]) -> Result<(), String> {
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    for g in geoms {
        wtr.write_record([write_wkt(g)])
            .map_err(|e| format!("{path}: {e}"))?;
    }
    let bytes = wtr.into_inner().map_err(|e| format!("{path}: {e}"))?;
    fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

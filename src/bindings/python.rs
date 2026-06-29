use std::str::FromStr;

use geo::Geometry;
use geojson::GeoJson;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use wkt::{ToWkt, Wkt};

use crate::validation::GeoValidation;
use crate::{MakeValid, MakeValidConfig, PolyMethod};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn encode_wkb_2d(geom: &Geometry<f64>) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    use wkb::writer::{geometry_wkb_size, write_geometry, WriteOptions};

    let opts = WriteOptions::default();
    let size = geometry_wkb_size(geom);
    let mut buf = vec![0u8; size];
    write_geometry(&mut Cursor::new(&mut buf[..]), geom, &opts)
        .map_err(|e| format!("WKB write error: {e}"))?;
    Ok(buf)
}

#[pymodule]
#[pyo3(name = "geo_repair")]
fn geo_repair_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", VERSION)?;

    m.add_function(wrap_pyfunction!(repair_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(validate_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb_batch, m)?)?;
    #[cfg(feature = "parallel")]
    {
        m.add_function(wrap_pyfunction!(par_repair_wkt_batch, m)?)?;
        m.add_function(wrap_pyfunction!(par_repair_wkb_batch, m)?)?;
    }
    m.add_function(wrap_pyfunction!(repair_file, m)?)?;
    m.add_function(wrap_pyfunction!(repair_file_to_file, m)?)?;
    Ok(())
}

fn make_config(method: Option<&str>) -> MakeValidConfig {
    let pm = match method.unwrap_or("auto").to_lowercase().as_str() {
        "arrange" => PolyMethod::Arrange,
        "structure" => PolyMethod::Structure,
        _ => PolyMethod::Auto,
    };
    MakeValidConfig {
        poly_method: pm,
        ..Default::default()
    }
}

fn parse_wkt_geom(wkt_str: &str) -> PyResult<Geometry<f64>> {
    let wkt = Wkt::from_str(wkt_str)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("WKT parse error: {e}")))?;
    wkt.try_into()
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("{e}")))
}

fn validation_errors(geom: &Geometry<f64>) -> Vec<String> {
    let result = geom.validate();
    result.errors.iter().map(|e| format!("{e}")).collect()
}

fn repair_one(geom: Geometry<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    geom.make_valid_with_config(config)
}

// ---------------------------------------------------------------------------
// WKT repair
// ---------------------------------------------------------------------------

/// Repair a single WKT geometry string.
///
/// Args:
///     wkt: WKT string of the geometry to repair.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///
/// Returns:
///     WKT string of the repaired geometry.
#[pyfunction]
#[pyo3(signature = (wkt, method = None))]
fn repair_wkt(wkt: &str, method: Option<&str>) -> PyResult<String> {
    let config = make_config(method);
    let geom = parse_wkt_geom(wkt)?;
    let fixed = repair_one(geom, &config);
    Ok(fixed.wkt_string())
}

// ---------------------------------------------------------------------------
// WKB repair
// ---------------------------------------------------------------------------

/// Repair a single WKB geometry (bytes).
///
/// Args:
///     wkb: Raw WKB bytes of the geometry to repair.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///
/// Returns:
///     WKB bytes of the repaired geometry.
#[pyfunction]
#[pyo3(signature = (wkb, method = None))]
fn repair_wkb(wkb: Vec<u8>, method: Option<&str>) -> PyResult<Vec<u8>> {
    let config = make_config(method);
    let wkb_geom = wkb::reader::read_wkb(&wkb)
        .map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))?;
    let geom = geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom);
    let fixed = repair_one(geom, &config);
    encode_wkb_2d(&fixed).map_err(|e| PyValueError::new_err(format!("WKB write error: {e}")))
}

// ---------------------------------------------------------------------------
// GeoJSON repair
// ---------------------------------------------------------------------------

/// Repair geometries from a GeoJSON string.
///
/// Accepts a Geometry, Feature, or FeatureCollection.  The return type
/// matches the input type so that properties and structure are preserved.
///
/// Args:
///     geojson: GeoJSON string to repair.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///
/// Returns:
///     GeoJSON string with all geometries repaired.
#[pyfunction]
#[pyo3(signature = (geojson, method = None))]
fn repair_geojson(geojson: &str, method: Option<&str>) -> PyResult<String> {
    let config = make_config(method);
    let gj: GeoJson = serde_json::from_str(geojson)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("GeoJSON parse error: {e}")))?;

    let result = match gj {
        GeoJson::Geometry(g) => {
            if let Ok(geo) = g.try_into() {
                let fixed = repair_one(geo, &config);
                GeoJson::Geometry(geojson::Geometry::from(&fixed))
            } else {
                GeoJson::Geometry(g)
            }
        }
        GeoJson::Feature(mut f) => {
            if let Some(g) = f.geometry.take() {
                if let Ok(geo) = g.try_into() {
                    let fixed = repair_one(geo, &config);
                    f.geometry = Some(geojson::Geometry::from(&fixed));
                } else {
                    f.geometry = Some(g);
                }
            }
            GeoJson::Feature(f)
        }
        GeoJson::FeatureCollection(fc) => {
            let features: Vec<geojson::Feature> = fc
                .features
                .into_iter()
                .map(|mut f| {
                    if let Some(g) = f.geometry.take() {
                        if let Ok(geo) = g.try_into() {
                            let fixed = repair_one(geo, &config);
                            f.geometry = Some(geojson::Geometry::from(&fixed));
                        } else {
                            f.geometry = Some(g);
                        }
                    }
                    f
                })
                .collect();
            GeoJson::FeatureCollection(geojson::FeatureCollection {
                features,
                bbox: fc.bbox,
                foreign_members: fc.foreign_members,
            })
        }
    };

    serde_json::to_string(&result)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("JSON error: {e}")))
}

// ---------------------------------------------------------------------------
// Validation — WKT, GeoJSON
// ---------------------------------------------------------------------------

/// Validate a WKT geometry, returning a list of error descriptions.
///
/// Returns an empty list when the geometry is valid.
#[pyfunction]
#[pyo3(signature = (wkt))]
fn validate_wkt(wkt: &str) -> PyResult<Vec<String>> {
    let geom = parse_wkt_geom(wkt)?;
    Ok(validation_errors(&geom))
}

/// Validate a GeoJSON geometry/feature/collection, returning a list of error descriptions.
///
/// Returns an empty list when all geometries are valid.
#[pyfunction]
#[pyo3(signature = (geojson))]
fn validate_geojson(geojson: &str) -> PyResult<Vec<String>> {
    let gj: GeoJson = serde_json::from_str(geojson)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("GeoJSON parse error: {e}")))?;

    let geoms = extract_geometries(gj);
    let mut errors = Vec::new();
    for (i, g) in geoms.iter().enumerate() {
        for e in validation_errors(g) {
            errors.push(format!("[geom {i}] {e}"));
        }
    }
    Ok(errors)
}

/// Quick validity check for a WKT geometry.
///
/// Returns ``True`` if the geometry is OGC-valid.
#[pyfunction]
#[pyo3(signature = (wkt))]
fn is_valid_wkt(wkt: &str) -> PyResult<bool> {
    let geom = parse_wkt_geom(wkt)?;
    Ok(geom.is_valid())
}

/// Quick validity check for a GeoJSON geometry/feature/collection.
///
/// Returns ``True`` when all geometries are OGC-valid.
#[pyfunction]
#[pyo3(signature = (geojson))]
fn is_valid_geojson(geojson: &str) -> PyResult<bool> {
    let gj: GeoJson = serde_json::from_str(geojson)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("GeoJSON parse error: {e}")))?;
    let geoms = extract_geometries(gj);
    Ok(geoms.iter().all(|g| g.is_valid()))
}

// ---------------------------------------------------------------------------
// Combined repair + validate
// ---------------------------------------------------------------------------

/// Repair and validate a WKT geometry in one call.
///
/// Returns ``(wkt_string, is_valid_before, [errors])``.
#[pyfunction]
#[pyo3(signature = (wkt, method = None))]
fn repair_validate_wkt(wkt: &str, method: Option<&str>) -> PyResult<(String, bool, Vec<String>)> {
    let config = make_config(method);
    let geom = parse_wkt_geom(wkt)?;
    let valid = geom.is_valid();
    let errors = validation_errors(&geom);
    let fixed = repair_one(geom, &config);
    Ok((fixed.wkt_string(), valid, errors))
}

/// Repair and validate a GeoJSON geometry/feature/collection in one call.
///
/// Returns ``(geojson_string, is_valid_before, [errors])``.
#[pyfunction]
#[pyo3(signature = (geojson, method = None))]
fn repair_validate_geojson(
    geojson: &str,
    method: Option<&str>,
) -> PyResult<(String, bool, Vec<String>)> {
    let config = make_config(method);
    let gj: GeoJson = serde_json::from_str(geojson)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("GeoJSON parse error: {e}")))?;
    let geoms = extract_geometries(gj);
    let valid = geoms.iter().all(|g| g.is_valid());
    let all_errors: Vec<String> = geoms.iter().flat_map(|g| validation_errors(g)).collect();
    let fixed: Vec<Geometry<f64>> = geoms.into_iter().map(|g| repair_one(g, &config)).collect();
    let fc = geojson::FeatureCollection {
        features: fixed
            .iter()
            .map(|g| geojson::Feature {
                bbox: None,
                geometry: Some(geojson::Geometry::from(g)),
                id: None,
                properties: Some(serde_json::Map::new()),
                foreign_members: None,
            })
            .collect(),
        bbox: None,
        foreign_members: None,
    };
    let out = serde_json::to_string(&fc)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("JSON error: {e}")))?;
    Ok((out, valid, all_errors))
}

// ---------------------------------------------------------------------------
// Batch processing — WKT
// ---------------------------------------------------------------------------

/// Repair a list of WKT strings.
///
/// Invalid inputs are returned unchanged (error string is silently kept).
#[pyfunction]
#[pyo3(signature = (wkts, method = None))]
fn repair_wkt_batch(wkts: Vec<String>, method: Option<&str>) -> PyResult<Vec<String>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkts.len());
    for w in wkts {
        let r = match parse_wkt_geom(&w) {
            Ok(geom) => repair_one(geom, &config).wkt_string(),
            Err(_) => w.to_string(),
        };
        results.push(r);
    }
    Ok(results)
}

/// Parallel repair of a list of WKT strings (requires ``parallel`` feature).
#[cfg(feature = "parallel")]
#[pyfunction]
#[pyo3(signature = (wkts, method = None))]
fn par_repair_wkt_batch(wkts: Vec<String>, method: Option<&str>) -> PyResult<Vec<String>> {
    use rayon::prelude::*;
    let config = make_config(method);
    let results: Vec<String> = wkts
        .par_iter()
        .map(|w| match parse_wkt_geom(w) {
            Ok(geom) => repair_one(geom, &config).wkt_string(),
            Err(_) => w.to_string(),
        })
        .collect();
    Ok(results)
}

/// Validate a list of WKT strings.
#[pyfunction]
#[pyo3(signature = (wkts))]
fn validate_wkt_batch(wkts: Vec<String>) -> PyResult<Vec<(bool, Vec<String>)>> {
    let mut results = Vec::with_capacity(wkts.len());
    for w in wkts {
        let r = match parse_wkt_geom(&w) {
            Ok(geom) => (geom.is_valid(), validation_errors(&geom)),
            Err(_) => (false, vec!["Parse error".into()]),
        };
        results.push(r);
    }
    Ok(results)
}

/// Repair + validate a list of WKT strings.
///
/// Returns ``[(wkt_string, is_valid_before, [errors]), ...]``.
#[pyfunction]
#[pyo3(signature = (wkts, method = None))]
fn repair_validate_wkt_batch(
    wkts: Vec<String>,
    method: Option<&str>,
) -> PyResult<Vec<(String, bool, Vec<String>)>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkts.len());
    for w in wkts {
        let r = match parse_wkt_geom(&w) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = validation_errors(&geom);
                let fixed = repair_one(geom, &config);
                (fixed.wkt_string(), valid, errors)
            }
            Err(e) => (w.to_string(), false, vec![format!("{e}")]),
        };
        results.push(r);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Batch processing — WKB
// ---------------------------------------------------------------------------

fn parse_wkb_geom(wkb: &[u8]) -> PyResult<Geometry<f64>> {
    let wkb_geom = wkb::reader::read_wkb(wkb)
        .map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))?;
    Ok(geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom))
}

/// Repair a list of WKB byte buffers.
///
/// Invalid inputs are returned unchanged.
#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn repair_wkb_batch(wkbs: Vec<Vec<u8>>, method: Option<&str>) -> PyResult<Vec<Vec<u8>>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        let r = match parse_wkb_geom(&wkb) {
            Ok(geom) => {
                let fixed = repair_one(geom, &config);
                encode_wkb_2d(&fixed).unwrap_or(wkb.to_vec())
            }
            Err(_) => wkb.to_vec(),
        };
        results.push(r);
    }
    Ok(results)
}

/// Parallel repair of a list of WKB buffers (requires ``parallel`` feature).
#[cfg(feature = "parallel")]
#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn par_repair_wkb_batch(wkbs: Vec<Vec<u8>>, method: Option<&str>) -> PyResult<Vec<Vec<u8>>> {
    use rayon::prelude::*;
    let config = make_config(method);
    let results: Vec<Vec<u8>> = wkbs
        .par_iter()
        .map(|wkb| match parse_wkb_geom(wkb) {
            Ok(geom) => {
                let fixed = repair_one(geom, &config);
                encode_wkb_2d(&fixed).unwrap_or(wkb.to_vec())
            }
            Err(_) => wkb.to_vec(),
        })
        .collect();
    Ok(results)
}

/// Repair + validate a list of WKB buffers.
#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn repair_validate_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
) -> PyResult<Vec<(Vec<u8>, bool, Vec<String>)>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        let r = match parse_wkb_geom(&wkb) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = validation_errors(&geom);
                let fixed = repair_one(geom, &config);
                let out = encode_wkb_2d(&fixed).unwrap_or(wkb.to_vec());
                (out, valid, errors)
            }
            Err(e) => (wkb.to_vec(), false, vec![format!("{e}")]),
        };
        results.push(r);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// File-level operations
// ---------------------------------------------------------------------------

/// Load a file, repair all geometries, and return results as (WKB, valid, [errors]).
///
/// Args:
///     input_path: Path to a supported file (.shp, .geojson, .wkt, .wkb, etc.).
///     method: Optional repair method.
#[pyfunction]
#[pyo3(signature = (input_path, method = None))]
fn repair_file(
    input_path: &str,
    method: Option<&str>,
) -> PyResult<Vec<(Vec<u8>, bool, Vec<String>)>> {
    let config = make_config(method);
    let geoms = crate::io::load_geometries(input_path)
        .map_err(|e| PyValueError::new_err(format!("Failed to load {}: {e}", input_path)))?;
    let mut results = Vec::with_capacity(geoms.len());
    for geom in geoms {
        let valid = geom.is_valid();
        let errors = validation_errors(&geom);
        let fixed = repair_one(geom, &config);
        let out = match encode_wkb_2d(&fixed) {
            Ok(b) => b,
            Err(e) => {
                results.push((Vec::new(), false, vec![format!("WKB write error: {e}")]));
                continue;
            }
        };
        results.push((out, valid, errors));
    }
    Ok(results)
}

/// Load a file, repair, and write the output — all in native Rust.
///
/// Args:
///     input_path: Source file path.
///     output_path: Destination file path.
///     method: Optional repair method.
///     mode: ``"both"`` (default, repair + return diagnostics), ``"validate"``
///           (diagnose only), ``"repair"`` (silent repair, no diagnostics).
///     progress: Optional callback ``fn(pct: float)`` called with 0–100 progress.
///
/// Returns:
///     ``(total_count, [(is_valid, [errors]), ...])``.
///     In ``"repair"`` mode the diagnostics list is empty.
#[pyfunction]
#[pyo3(signature = (input_path, output_path, method = None, mode = "both", progress = None))]
fn repair_file_to_file(
    input_path: &str,
    output_path: &str,
    method: Option<&str>,
    mode: &str,
    progress: Option<PyObject>,
) -> PyResult<(usize, Vec<(bool, Vec<String>)>)> {
    let config = make_config(method);
    let report = |pct: f64, py: Python<'_>| {
        if let Some(ref cb) = progress {
            let _ = cb.call1(py, (pct,));
        }
    };

    let load_report = |pct: f64, py: Python<'_>| report(pct * 0.05, py);
    let mut features = crate::io::load_features_with_progress(
        input_path,
        Some(&|p| {
            Python::with_gil(|py| load_report(p, py));
        }),
    )
    .map_err(|e| PyValueError::new_err(format!("Failed to load {}: {e}", input_path)))?;
    let count = features.len();
    let mut diags = Vec::with_capacity(count);

    // Process (5-75%)
    if mode == "validate" {
        for (i, feat) in features.iter().enumerate() {
            let valid = feat.geometry.is_valid();
            let errors = if valid {
                vec![]
            } else {
                validation_errors(&feat.geometry)
            };
            diags.push((valid, errors));
            if i % 100 == 0 {
                Python::with_gil(|py| report(5.0 + (i as f64 / count as f64) * 70.0, py));
            }
        }
    } else {
        for (i, feat) in features.iter_mut().enumerate() {
            let valid = feat.geometry.is_valid();
            let errors = if valid {
                vec![]
            } else {
                validation_errors(&feat.geometry)
            };
            let fixed = feat.geometry.make_valid_with_config(&config);
            feat.geometry = fixed;
            diags.push((valid, errors));
            if i % 100 == 0 {
                Python::with_gil(|py| report(5.0 + (i as f64 / count as f64) * 70.0, py));
            }
        }
    }

    // Export (80-100%)
    Python::with_gil(|py| report(80.0, py));
    let export_report = |pct: f64, py: Python<'_>| report(80.0 + pct * 0.2, py);
    crate::io::export_features_with_progress(
        &features,
        output_path,
        Some(&|p| {
            Python::with_gil(|py| export_report(p, py));
        }),
    )
    .map_err(|e| PyValueError::new_err(format!("Failed to write {}: {e}", output_path)))?;

    Python::with_gil(|py| report(100.0, py));

    if mode == "repair" {
        Ok((count, vec![]))
    } else {
        Ok((count, diags))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_geometries(gj: GeoJson) -> Vec<Geometry<f64>> {
    let mut geoms = Vec::new();
    match gj {
        GeoJson::FeatureCollection(fc) => {
            for f in fc.features {
                if let Some(g) = f.geometry {
                    if let Ok(geo) = g.try_into() {
                        geoms.push(geo);
                    }
                }
            }
        }
        GeoJson::Feature(f) => {
            if let Some(g) = f.geometry {
                if let Ok(geo) = g.try_into() {
                    geoms.push(geo);
                }
            }
        }
        GeoJson::Geometry(g) => {
            if let Ok(geo) = g.try_into() {
                geoms.push(geo);
            }
        }
    }
    geoms
}

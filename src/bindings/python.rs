use std::str::FromStr;

use geo::Geometry;
use geojson::GeoJson;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use wkt::{ToWkt, Wkt};

use crate::validation::GeoValidation;
use crate::{MakeValid, MakeValidConfig, PolyMethod};

#[pymodule]
#[pyo3(name = "geo_repair")]
fn geo_repair_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(repair_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(validate_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_file, m)?)?;
    m.add_function(wrap_pyfunction!(repair_file_to_file, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb, m)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (wkt_str, method = None))]
fn repair_wkt(wkt_str: &str, method: Option<&str>) -> PyResult<String> {
    let config = make_config(method);
    let wkt = Wkt::from_str(wkt_str).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("WKT parse error: {e}"))
    })?;
    let geom: Geometry<f64> = wkt
        .try_into()
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
    let fixed = geom.make_valid_with_config(&config);
    Ok(fixed.wkt_string())
}

#[pyfunction]
#[pyo3(signature = (geojson_str, method = None))]
fn repair_geojson(geojson_str: &str, method: Option<&str>) -> PyResult<String> {
    let config = make_config(method);
    let gj: GeoJson = serde_json::from_str(geojson_str).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("GeoJSON parse error: {e}"))
    })?;

    let geoms = extract_geometries(gj);
    let fixed: Vec<Geometry<f64>> = geoms
        .into_iter()
        .map(|g| g.make_valid_with_config(&config))
        .collect();

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

    serde_json::to_string(&fc)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("JSON error: {e}")))
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

fn extract_geometries(gj: GeoJson) -> Vec<Geometry<f64>> {
    let mut geoms = Vec::new();
    match gj {
        GeoJson::FeatureCollection(fc) => {
            for mut f in fc.features {
                if let Some(g) = f.geometry.take() && let Ok(geo) = g.try_into() {
                    geoms.push(geo);
                }
            }
        }
        GeoJson::Feature(mut f) => {
            if let Some(g) = f.geometry.take() && let Ok(geo) = g.try_into() {
                geoms.push(geo);
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

// ---------------------------------------------------------------------------
// Batch functions — process many WKT strings in a single Rust call
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (wkts, method = None))]
fn repair_wkt_batch(wkts: Vec<String>, method: Option<&str>) -> PyResult<Vec<String>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkts.len());
    for wkt_str in wkts {
        match (|| -> PyResult<String> {
            let wkt = Wkt::from_str(&wkt_str)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            let geom: Geometry<f64> = wkt
                .try_into()
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
            let fixed = geom.make_valid_with_config(&config);
            Ok(fixed.wkt_string())
        })() {
            Ok(r) => results.push(r),
            Err(_) => results.push(wkt_str.to_string()),
        }
    }
    Ok(results)
}

#[pyfunction]
#[pyo3(signature = (wkts))]
fn validate_wkt_batch(wkts: Vec<String>) -> PyResult<Vec<(bool, Vec<String>)>> {
    let mut results = Vec::with_capacity(wkts.len());
    for wkt_str in wkts {
        match (|| -> PyResult<(bool, Vec<String>)> {
            let wkt = Wkt::from_str(&wkt_str)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            let geom: Geometry<f64> = wkt
                .try_into()
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
            let valid = geom.is_valid();
            let errors = validation_errors(&geom);
            Ok((valid, errors))
        })() {
            Ok(r) => results.push(r),
            Err(_) => results.push((false, vec!["Parse error".into()])),
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Validation helpers (shared by WKT and GeoJSON)
// ---------------------------------------------------------------------------

fn parse_wkt_geom(wkt_str: &str) -> PyResult<Geometry<f64>> {
    let wkt = Wkt::from_str(wkt_str).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("WKT parse error: {e}"))
    })?;
    wkt.try_into()
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))
}

fn validation_errors(geom: &Geometry<f64>) -> Vec<String> {
    let result = geom.validate();
    result.errors.iter().map(|e| format!("{e}")).collect()
}

// ---------------------------------------------------------------------------
// validate_wkt / validate_geojson — full validation error list
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (wkt_str))]
fn validate_wkt(wkt_str: &str) -> PyResult<Vec<String>> {
    let geom = parse_wkt_geom(wkt_str)?;
    Ok(validation_errors(&geom))
}

#[pyfunction]
#[pyo3(signature = (geojson_str))]
fn validate_geojson(geojson_str: &str) -> PyResult<Vec<String>> {
    let gj: GeoJson = serde_json::from_str(geojson_str).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("GeoJSON parse error: {e}"))
    })?;
    let geoms = extract_geometries(gj);
    let mut errors = Vec::new();
    for (i, g) in geoms.iter().enumerate() {
        for e in validation_errors(g) {
            errors.push(format!("[geom {i}] {e}"));
        }
    }
    Ok(errors)
}

// ---------------------------------------------------------------------------
// is_valid_wkt / is_valid_geojson — boolean check
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (wkt_str))]
fn is_valid_wkt(wkt_str: &str) -> PyResult<bool> {
    let geom = parse_wkt_geom(wkt_str)?;
    Ok(geom.is_valid())
}

#[pyfunction]
#[pyo3(signature = (geojson_str))]
fn is_valid_geojson(geojson_str: &str) -> PyResult<bool> {
    let gj: GeoJson = serde_json::from_str(geojson_str).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("GeoJSON parse error: {e}"))
    })?;
    let geoms = extract_geometries(gj);
    Ok(geoms.iter().all(|g| g.is_valid()))
}

// ---------------------------------------------------------------------------
// repair_validate_wkt_batch — single Rust call for both repair + validate
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (wkts, method = None))]
fn repair_validate_wkt_batch(
    wkts: Vec<String>,
    method: Option<&str>,
) -> PyResult<Vec<(String, bool, Vec<String>)>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkts.len());
    for wkt_str in wkts {
        match (|| -> PyResult<(String, bool, Vec<String>)> {
            let wkt = Wkt::from_str(&wkt_str)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            let geom: Geometry<f64> = wkt
                .try_into()
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
            let is_valid = geom.is_valid();
            let errors = validation_errors(&geom);
            let fixed = geom.make_valid_with_config(&config);
            Ok((fixed.wkt_string(), is_valid, errors))
        })() {
            Ok(r) => results.push(r),
            Err(e) => results.push((wkt_str.to_string(), false, vec![format!("{e}")])),
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// WKB batch functions — binary WKB I/O, faster than WKT
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn repair_wkb_batch(wkbs: Vec<Vec<u8>>, method: Option<&str>) -> PyResult<Vec<Vec<u8>>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb_bytes in wkbs {
        match (|| -> PyResult<Vec<u8>> {
            let wkb_geom = wkb::reader::read_wkb(&wkb_bytes)
                .map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))?;
            let geom = geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom);
            let fixed = geom.make_valid_with_config(&config);
            crate::io::wkb::encode_wkb_2d(&fixed)
                .map_err(|e| PyValueError::new_err(format!("WKB write error: {e}")))
        })() {
            Ok(r) => results.push(r),
            Err(_) => results.push(wkb_bytes),
        }
    }
    Ok(results)
}

#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
#[allow(clippy::type_complexity)]
fn repair_validate_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
) -> PyResult<Vec<(Vec<u8>, bool, Vec<String>)>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb_bytes in wkbs {
        match (|| -> PyResult<(Vec<u8>, bool, Vec<String>)> {
            let wkb_geom = wkb::reader::read_wkb(&wkb_bytes)
                .map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))?;
            let geom = geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom);
            let is_valid = geom.is_valid();
            let errors = validation_errors(&geom);
            let fixed = geom.make_valid_with_config(&config);
            let out_bytes = crate::io::wkb::encode_wkb_2d(&fixed)
                .map_err(|e| PyValueError::new_err(format!("WKB write error: {e}")))?;
            Ok((out_bytes, is_valid, errors))
        })() {
            Ok(r) => results.push(r),
            Err(e) => results.push((wkb_bytes, false, vec![format!("{e}")])),
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// repair_file — read from a file in Rust, repair, return WKB results
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (input_path, method = None))]
#[allow(clippy::type_complexity)]
fn repair_file(
    input_path: &str,
    method: Option<&str>,
) -> PyResult<Vec<(Vec<u8>, bool, Vec<String>)>> {
    let config = make_config(method);
    let geoms = crate::io::load_geometries(input_path)
        .map_err(|e| PyValueError::new_err(format!("Failed to load {}: {e}", input_path)))?;
    let mut results = Vec::with_capacity(geoms.len());
    for geom in geoms {
        let is_valid = geom.is_valid();
        let errors = validation_errors(&geom);
        let fixed = geom.make_valid_with_config(&config);
        match crate::io::wkb::encode_wkb_2d(&fixed) {
            Ok(out_bytes) => results.push((out_bytes, is_valid, errors)),
            Err(e) => results.push((Vec::new(), false, vec![format!("WKB write error: {e}")])),
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// repair_file_to_file — read file, repair, write file (everything in Rust)
//   mode: "both" (default) — repair + validate, return diagnostics
//         "validate"      — validate only, write original geometry
//         "repair"        — repair silently, return empty diagnostics
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (input_path, output_path, method = None, mode = "both", progress = None))]
#[allow(clippy::type_complexity)]
fn repair_file_to_file(
    input_path: &str,
    output_path: &str,
    method: Option<&str>,
    mode: &str,
    #[allow(deprecated)] progress: Option<Py<PyAny>>,
) -> PyResult<(usize, Vec<(bool, Vec<String>)>)> {
    let config = make_config(method);
    let report = |pct: f64| {
        if let Some(ref cb) = progress {
            Python::attach(|py| {
                let _ = cb.call1(py, (pct,));
            });
        }
    };

    // Load with progress mapped to 0-5% of overall
    let load_report = |pct: f64| report(pct * 0.05);
    let mut features = crate::io::load_features_with_progress(input_path, Some(&load_report))
        .map_err(|e| PyValueError::new_err(format!("Failed to load {}: {e}", input_path)))?;
    let count = features.len();
    let mut diags = Vec::with_capacity(count);

    // Process (5-75%)
    if mode == "validate" {
        for (i, feat) in features.iter().enumerate() {
            let is_valid = feat.geometry.is_valid();
            let errors = if is_valid {
                vec![]
            } else {
                validation_errors(&feat.geometry)
            };
            diags.push((is_valid, errors));
            if i % 100 == 0 {
                report(5.0 + (i as f64 / count as f64) * 70.0);
            }
        }
    } else {
        for (i, feat) in features.iter_mut().enumerate() {
            let is_valid = feat.geometry.is_valid();
            let errors = if is_valid {
                vec![]
            } else {
                validation_errors(&feat.geometry)
            };
            let fixed = feat.geometry.make_valid_with_config(&config);
            feat.geometry = fixed;
            diags.push((is_valid, errors));
            if i % 100 == 0 {
                report(5.0 + (i as f64 / count as f64) * 70.0);
            }
        }
    }

    // Export with progress mapped to 80-100% of overall
    report(80.0);
    let export_report = |pct: f64| report(80.0 + pct * 0.2);
    crate::io::export_features_with_progress(&features, output_path, Some(&export_report))
        .map_err(|e| PyValueError::new_err(format!("Failed to write {}: {e}", output_path)))?;

    report(100.0);

    if mode == "repair" {
        Ok((count, vec![]))
    } else {
        Ok((count, diags))
    }
}

// ---------------------------------------------------------------------------
// repair_validate_wkb — single-geometry WKB repair + validate (streaming)
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (wkb, method = None))]
fn repair_validate_wkb(
    wkb: Vec<u8>,
    method: Option<&str>,
) -> PyResult<(Vec<u8>, bool, Vec<String>)> {
    let config = make_config(method);
    let wkb_geom = wkb::reader::read_wkb(&wkb)
        .map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))?;
    let geom = geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom);
    let is_valid = geom.is_valid();
    let errors = validation_errors(&geom);
    let fixed = geom.make_valid_with_config(&config);
    let out_bytes = crate::io::wkb::encode_wkb_2d(&fixed)
        .map_err(|e| PyValueError::new_err(format!("WKB write error: {e}")))?;
    Ok((out_bytes, is_valid, errors))
}

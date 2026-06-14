use std::str::FromStr;

use geo::Geometry;
use geojson::GeoJson;
use pyo3::prelude::*;
use pyo3::types::PyList;
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
                if let Some(g) = f.geometry.take() {
                    if let Ok(geo) = g.try_into() {
                        geoms.push(geo);
                    }
                }
            }
        }
        GeoJson::Feature(mut f) => {
            if let Some(g) = f.geometry.take() {
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
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

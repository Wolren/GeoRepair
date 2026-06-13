use std::str::FromStr;

use geo::Geometry;
use geojson::GeoJson;
use pyo3::prelude::*;
use wkt::{ToWkt, Wkt};

use crate::{MakeValid, MakeValidConfig, PolyMethod};

#[pymodule]
#[pyo3(name = "geo_repair")]
fn geo_repair_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(repair_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_geojson, m)?)?;
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
                properties: None,
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

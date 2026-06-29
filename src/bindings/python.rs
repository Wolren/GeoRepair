use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::io::wkb::{read_wkb, write_wkb};
use crate::validation::GeoValidation;
use crate::{MakeValid, MakeValidConfig, PolyMethod};

const VERSION: &str = env!("CARGO_PKG_VERSION");

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

fn validation_errors(geom: &geo::Geometry<f64>) -> Vec<String> {
    let result = geom.validate();
    result.errors.iter().map(|e| format!("{e}")).collect()
}

fn repair_one(geom: geo::Geometry<f64>, config: &MakeValidConfig) -> geo::Geometry<f64> {
    geom.make_valid_with_config(config)
}

// ---------------------------------------------------------------------------
// Module definition
// ---------------------------------------------------------------------------

#[pymodule]
#[pyo3(name = "geo_repair")]
fn geo_repair_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", VERSION)?;

    m.add_function(wrap_pyfunction!(repair_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb_batch, m)?)?;
    #[cfg(feature = "parallel")]
    {
        m.add_function(wrap_pyfunction!(par_repair_wkb_batch, m)?)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Single WKB repair
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
    let geom =
        read_wkb(&wkb).map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))?;
    let fixed = repair_one(geom, &config);
    Ok(write_wkb(&fixed))
}

// ---------------------------------------------------------------------------
// Batch WKB repair (sequential)
// ---------------------------------------------------------------------------

/// Repair a list of WKB byte buffers.
///
/// Invalid inputs are returned unchanged.
#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn repair_wkb_batch(wkbs: Vec<Vec<u8>>, method: Option<&str>) -> PyResult<Vec<Vec<u8>>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        let r = match read_wkb(&wkb) {
            Ok(geom) => {
                let fixed = repair_one(geom, &config);
                write_wkb(&fixed)
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
        .map(|wkb| match read_wkb(wkb) {
            Ok(geom) => {
                let fixed = repair_one(geom, &config);
                write_wkb(&fixed)
            }
            Err(_) => wkb.to_vec(),
        })
        .collect();
    Ok(results)
}

// ---------------------------------------------------------------------------
// Combined repair + validate (batch)
// ---------------------------------------------------------------------------

/// Repair + validate a list of WKB buffers.
///
/// Returns ``[(wkb_bytes, is_valid_before, [errors]), ...]``.
#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn repair_validate_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
) -> PyResult<Vec<(Vec<u8>, bool, Vec<String>)>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        let r = match read_wkb(&wkb) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = validation_errors(&geom);
                let fixed = repair_one(geom, &config);
                let out = write_wkb(&fixed);
                (out, valid, errors)
            }
            Err(e) => (wkb.to_vec(), false, vec![format!("{e}")]),
        };
        results.push(r);
    }
    Ok(results)
}

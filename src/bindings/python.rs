//! Python bindings via PyO3.
//!
//! The Python package exposes WKB, WKT, and GeoJSON processing — ideal for
//! use with QGIS or GDAL where WKB is the native geometry format, or for
//! scripting pipelines that work with text geometries.
//!
//! # Build and install
//!
//! ```bash
//! pip install maturin
//! python -m maturin build --features python
//! pip install target/wheels/geo_repair-*.whl
//! ```
//!
//! # Python usage
//!
//! ```python
//! import geo_repair
//!
//! # --- WKB ---
//! wkb_out = geo_repair.repair_wkb(wkb_in, method="auto")
//! results = geo_repair.par_repair_wkb_batch(wkb_batch, method="auto")
//! is_valid, errors = geo_repair.validate_wkb(wkb_in)
//!
//! # --- WKT ---
//! fixed = geo_repair.repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
//! assert geo_repair.is_valid_wkt(fixed)
//!
//! # --- GeoJSON ---
//! fixed_gj = geo_repair.repair_geojson(geojson_str, method="structure")
//! was_valid, errors = geo_repair.validate_geojson(geojson_str)
//! ```
//!
//! **QGIS integration:** See `qgis/qgis_geo_repair.py` for a complete
//! processing script.
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::io::wkb::{read_wkb, write_wkb};
use crate::io::wkt::{read_wkt, write_wkt};
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

fn parse_wkb(wkb: &[u8]) -> PyResult<geo::Geometry<f64>> {
    read_wkb(wkb).map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))
}

fn repair_one(geom: geo::Geometry<f64>, config: &MakeValidConfig) -> geo::Geometry<f64> {
    geom.make_valid_with_config(config)
}

/// Collect validation errors as strings; for GeometryCollection inputs each
/// child's errors are prefixed with `[geom N]` so callers can tell which
/// component failed.
fn error_strings(geom: &geo::Geometry<f64>) -> Vec<String> {
    if let geo::Geometry::GeometryCollection(gc) = geom {
        let mut out = Vec::new();
        for (i, child) in gc.0.iter().enumerate() {
            for e in &child.validate().errors {
                out.push(format!("[geom {i}] {e}"));
            }
        }
        out
    } else {
        geom.validate()
            .errors
            .iter()
            .map(|e| format!("{e}"))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Module definition
// ---------------------------------------------------------------------------

#[pymodule]
#[pyo3(name = "geo_repair")]
fn geo_repair_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", VERSION)?;

    // WKB
    m.add_function(wrap_pyfunction!(repair_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb_batch, m)?)?;
    #[cfg(feature = "parallel")]
    {
        m.add_function(wrap_pyfunction!(par_repair_wkb_batch, m)?)?;
    }
    m.add_function(wrap_pyfunction!(is_valid_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkb_batch, m)?)?;

    // WKT
    m.add_function(wrap_pyfunction!(repair_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkt, m)?)?;

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
    let geom = parse_wkb(&wkb)?;
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
        let r = match parse_wkb(&wkb) {
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
        .map(|wkb| match parse_wkb(wkb) {
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
// Single WKB validation
// ---------------------------------------------------------------------------

/// Check whether a WKB geometry is OGC-valid.
///
/// Args:
///     wkb: Raw WKB bytes.
///
/// Returns:
///     ``True`` if the geometry is valid, ``False`` otherwise.
#[pyfunction]
fn is_valid_wkb(wkb: Vec<u8>) -> PyResult<bool> {
    let geom = parse_wkb(&wkb)?;
    Ok(geom.is_valid())
}

/// Validate a WKB geometry and return (is_valid, [errors]).
///
/// Args:
///     wkb: Raw WKB bytes.
///
/// Returns:
///     A tuple ``(is_valid, error_list)``.
#[pyfunction]
fn validate_wkb(wkb: Vec<u8>) -> PyResult<(bool, Vec<String>)> {
    let geom = parse_wkb(&wkb)?;
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    Ok((valid, errors))
}

/// Validate a WKB geometry, then fix it if invalid.
///
/// Args:
///     wkb: Raw WKB bytes.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///
/// Returns:
///     ``(was_valid, errors_before_repair, fixed_wkb_bytes)``
///     If the geometry was already valid, `errors_before_repair` is empty
///     and `fixed_wkb_bytes` equals the original input.
#[pyfunction]
#[pyo3(signature = (wkb, method = None))]
fn validate_and_fix_wkb(
    wkb: Vec<u8>,
    method: Option<&str>,
) -> PyResult<(bool, Vec<String>, Vec<u8>)> {
    let config = make_config(method);
    let geom = parse_wkb(&wkb)?;
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    let fixed = repair_one(geom, &config);
    Ok((valid, errors, write_wkb(&fixed)))
}

// ---------------------------------------------------------------------------
// Batch WKB validation
// ---------------------------------------------------------------------------

/// Check whether each WKB geometry is OGC-valid.
#[pyfunction]
fn is_valid_wkb_batch(wkbs: Vec<Vec<u8>>) -> PyResult<Vec<bool>> {
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        results.push(match parse_wkb(&wkb) {
            Ok(geom) => geom.is_valid(),
            Err(_) => false,
        });
    }
    Ok(results)
}

/// Validate each WKB geometry, returning ``[(is_valid, [errors]), ...]``.
#[pyfunction]
fn validate_wkb_batch(wkbs: Vec<Vec<u8>>) -> PyResult<Vec<(bool, Vec<String>)>> {
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        let r = match parse_wkb(&wkb) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = error_strings(&geom);
                (valid, errors)
            }
            Err(e) => (false, vec![format!("{e}")]),
        };
        results.push(r);
    }
    Ok(results)
}

/// Validate then fix each WKB geometry.
///
/// Returns ``[(was_valid, errors_before_repair, fixed_wkb_bytes), ...]``.
#[pyfunction]
#[pyo3(signature = (wkbs, method = None))]
fn validate_and_fix_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
) -> PyResult<Vec<(bool, Vec<String>, Vec<u8>)>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkbs.len());
    for wkb in wkbs {
        let r = match parse_wkb(&wkb) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = error_strings(&geom);
                let fixed = repair_one(geom, &config);
                (valid, errors, write_wkb(&fixed))
            }
            Err(e) => (false, vec![format!("{e}")], wkb.to_vec()),
        };
        results.push(r);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Combined repair + validate
// ---------------------------------------------------------------------------

/// Repair + validate a single WKB buffer.
///
/// Returns ``(wkb_bytes, is_valid_before, [errors])``.
#[pyfunction]
#[pyo3(signature = (wkb, method = None))]
fn repair_validate_wkb(wkb: Vec<u8>, method: Option<&str>) -> PyResult<(Vec<u8>, bool, Vec<String>)> {
    let config = make_config(method);
    let geom = match parse_wkb(&wkb) {
        Ok(geom) => geom,
        Err(e) => return Ok((wkb.to_vec(), false, vec![format!("{e}")])),
    };
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    let fixed = repair_one(geom, &config);
    Ok((write_wkb(&fixed), valid, errors))
}

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
        let r = match parse_wkb(&wkb) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = error_strings(&geom);
                let fixed = repair_one(geom, &config);
                (write_wkb(&fixed), valid, errors)
            }
            Err(e) => (wkb.to_vec(), false, vec![format!("{e}")]),
        };
        results.push(r);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// WKT
// ---------------------------------------------------------------------------

fn parse_wkt(wkt: &str) -> PyResult<geo::Geometry<f64>> {
    read_wkt(wkt).map_err(|e| PyValueError::new_err(format!("WKT parse error: {e}")))
}

/// Repair a WKT geometry string.
///
/// Args:
///     wkt: WKT text of the geometry to repair.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///
/// Returns:
///     WKT text of the repaired geometry.
#[pyfunction]
#[pyo3(signature = (wkt, method = None))]
fn repair_wkt(wkt: &str, method: Option<&str>) -> PyResult<String> {
    let config = make_config(method);
    let geom = parse_wkt(wkt)?;
    let fixed = repair_one(geom, &config);
    Ok(write_wkt(&fixed))
}

/// Repair a list of WKT strings. Unparseable inputs are returned unchanged.
#[pyfunction]
#[pyo3(signature = (wkts, method = None))]
fn repair_wkt_batch(wkts: Vec<String>, method: Option<&str>) -> PyResult<Vec<String>> {
    let config = make_config(method);
    let mut results = Vec::with_capacity(wkts.len());
    for wkt in wkts {
        let r = match parse_wkt(&wkt) {
            Ok(geom) => write_wkt(&repair_one(geom, &config)),
            Err(_) => wkt,
        };
        results.push(r);
    }
    Ok(results)
}

/// Check whether a WKT geometry is OGC-valid.
#[pyfunction]
fn is_valid_wkt(wkt: &str) -> PyResult<bool> {
    let geom = parse_wkt(wkt)?;
    Ok(geom.is_valid())
}

/// Validate a list of WKT strings, returning ``[bool, ...]``.
#[pyfunction]
fn is_valid_wkt_batch(wkts: Vec<String>) -> PyResult<Vec<bool>> {
    let mut results = Vec::with_capacity(wkts.len());
    for wkt in wkts {
        results.push(match parse_wkt(&wkt) {
            Ok(geom) => geom.is_valid(),
            Err(_) => false,
        });
    }
    Ok(results)
}

/// Validate a WKT geometry, returning the list of violations.
/// Collection components are prefixed with ``[geom N]``.
#[pyfunction]
fn validate_wkt(wkt: &str) -> PyResult<Vec<String>> {
    let geom = parse_wkt(wkt)?;
    Ok(error_strings(&geom))
}

/// Validate a list of WKT strings, returning ``[[errors], ...]``.
#[pyfunction]
fn validate_wkt_batch(wkts: Vec<String>) -> PyResult<Vec<Vec<String>>> {
    let mut results = Vec::with_capacity(wkts.len());
    for wkt in wkts {
        results.push(match parse_wkt(&wkt) {
            Ok(geom) => error_strings(&geom),
            Err(e) => vec![format!("{e}")],
        });
    }
    Ok(results)
}

/// Validate a WKT geometry, then fix it if invalid.
///
/// Returns ``(was_valid, errors_before_repair, fixed_wkt)``.
#[pyfunction]
#[pyo3(signature = (wkt, method = None))]
fn validate_and_fix_wkt(
    wkt: &str,
    method: Option<&str>,
) -> PyResult<(bool, Vec<String>, String)> {
    let config = make_config(method);
    let geom = parse_wkt(wkt)?;
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    let fixed = repair_one(geom, &config);
    Ok((valid, errors, write_wkt(&fixed)))
}

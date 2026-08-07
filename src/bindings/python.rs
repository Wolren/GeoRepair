//! Python bindings via PyO3.
//!
//! The `geo_repair` package exposes the full validation and repair
//! surface over both WKB (bytes, the GIS-native format used by QGIS and
//! GDAL) and WKT (text). Every repair/validate-and-fix function accepts
//! `method` ("auto" | "arrange" | "structure") and `keep_collapsed`;
//! batch functions mirror the single-geometry semantics, and
//! `par_repair_*_batch` uses the rayon batch when the wheel was built
//! with the `parallel` feature.
//!
//! # Build and install
//!
//! ```bash
//! pip install maturin
//! python -m maturin build --release --features python
//! pip install target/wheels/geo_repair-*.whl
//! ```
//!
//! # Python usage
//!
//! ```python
//! import geo_repair
//!
//! # --- WKB ---
//! wkb_out = geo_repair.repair_wkb(wkb_in, method="auto", keep_collapsed=False)
//! results = geo_repair.par_repair_wkb_batch(wkb_batch)
//! is_valid, errors = geo_repair.validate_wkb(wkb_in)
//!
//! # --- WKT ---
//! fixed = geo_repair.repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
//! assert geo_repair.is_valid_wkt(fixed)
//!
//! print(geo_repair.version())
//! ```
//!
//! **QGIS integration:** See `qgis/qgis_geo_repair.py` for a complete
//! processing script (batched WKB streaming, memory O(1)).

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::io::wkb::{read_wkb, write_wkb};
use crate::io::wkt::{read_wkt, write_wkt};
use crate::validation::GeoValidation;
use crate::{MakeValid, MakeValidConfig, PolyMethod};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `(was_valid, errors_before_repair, fixed_bytes)` for one WKB geometry.
type WkbValidateAndFix = (bool, Vec<String>, Vec<u8>);
/// `(fixed_bytes, was_valid_before, errors)` for one WKB geometry.
type WkbRepairValidate = (Vec<u8>, bool, Vec<String>);
/// `(was_valid, errors_before_repair, fixed_wkt)` for one WKT geometry.
type WktValidateAndFix = (bool, Vec<String>, String);
/// `(fixed_wkt, was_valid_before, errors)` for one WKT geometry.
type WktRepairValidate = (String, bool, Vec<String>);

/// Library version string (identical to the Rust crate version).
#[pyfunction]
fn version() -> &'static str {
    VERSION
}

fn make_config(method: Option<&str>, keep_collapsed: Option<bool>) -> MakeValidConfig {
    let pm = match method.unwrap_or("auto").to_lowercase().as_str() {
        "arrange" => PolyMethod::Arrange,
        "structure" => PolyMethod::Structure,
        _ => PolyMethod::Auto,
    };
    MakeValidConfig {
        poly_method: pm,
        keep_collapsed: keep_collapsed.unwrap_or(false),
        ..Default::default()
    }
}

fn parse_wkb(wkb: &[u8]) -> PyResult<geo::Geometry<f64>> {
    read_wkb(wkb).map_err(|e| PyValueError::new_err(format!("WKB parse error: {e}")))
}

fn parse_wkt(wkt: &str) -> PyResult<geo::Geometry<f64>> {
    read_wkt(wkt).map_err(|e| PyValueError::new_err(format!("WKT parse error: {e}")))
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
    m.add_function(wrap_pyfunction!(version, m)?)?;

    // WKB
    m.add_function(wrap_pyfunction!(repair_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkb_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkb, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkb_batch, m)?)?;
    #[cfg(feature = "parallel")]
    {
        m.add_function(wrap_pyfunction!(par_repair_wkb_batch, m)?)?;
    }

    // WKT
    m.add_function(wrap_pyfunction!(repair_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(repair_validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(validate_wkt_batch, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkt, m)?)?;
    m.add_function(wrap_pyfunction!(validate_and_fix_wkt_batch, m)?)?;
    #[cfg(feature = "parallel")]
    {
        m.add_function(wrap_pyfunction!(par_repair_wkt_batch, m)?)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// WKB
// ---------------------------------------------------------------------------

/// Repair a single WKB geometry (bytes).
///
/// Args:
///     wkb: Raw WKB bytes of the geometry to repair.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///     keep_collapsed: Optional; when True, collapsed (zero-area)
///         components are kept instead of dropped.
///
/// Returns:
///     WKB bytes of the repaired geometry.
#[pyfunction]
#[pyo3(signature = (wkb, method = None, keep_collapsed = None))]
fn repair_wkb(wkb: Vec<u8>, method: Option<&str>, keep_collapsed: Option<bool>) -> PyResult<Vec<u8>> {
    let config = make_config(method, keep_collapsed);
    let geom = parse_wkb(&wkb)?;
    let fixed = repair_one(geom, &config);
    Ok(write_wkb(&fixed))
}

/// Repair a list of WKB byte buffers.
///
/// Unparseable inputs are returned unchanged (the batch never fails as a
/// whole).
#[pyfunction]
#[pyo3(signature = (wkbs, method = None, keep_collapsed = None))]
fn repair_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<Vec<u8>>> {
    let config = make_config(method, keep_collapsed);
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
#[pyo3(signature = (wkbs, method = None, keep_collapsed = None))]
fn par_repair_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<Vec<u8>>> {
    use rayon::prelude::*;
    let config = make_config(method, keep_collapsed);
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
#[pyfunction]
fn validate_wkb(wkb: Vec<u8>) -> PyResult<(bool, Vec<String>)> {
    let geom = parse_wkb(&wkb)?;
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    Ok((valid, errors))
}

/// Validate a WKB geometry, then fix it if invalid.
///
/// Returns:
///     ``(was_valid, errors_before_repair, fixed_wkb_bytes)``
///     If the geometry was already valid, `errors_before_repair` is empty
///     and `fixed_wkb_bytes` equals the original input.
#[pyfunction]
#[pyo3(signature = (wkb, method = None, keep_collapsed = None))]
fn validate_and_fix_wkb(
    wkb: Vec<u8>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<(bool, Vec<String>, Vec<u8>)> {
    let config = make_config(method, keep_collapsed);
    let geom = parse_wkb(&wkb)?;
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    let fixed = repair_one(geom, &config);
    Ok((valid, errors, write_wkb(&fixed)))
}

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
#[pyo3(signature = (wkbs, method = None, keep_collapsed = None))]
fn validate_and_fix_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<WkbValidateAndFix>> {
    let config = make_config(method, keep_collapsed);
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

/// Repair + validate a single WKB buffer.
///
/// Returns ``(wkb_bytes, is_valid_before, [errors])``.
#[pyfunction]
#[pyo3(signature = (wkb, method = None, keep_collapsed = None))]
fn repair_validate_wkb(
    wkb: Vec<u8>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<(Vec<u8>, bool, Vec<String>)> {
    let config = make_config(method, keep_collapsed);
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
#[pyo3(signature = (wkbs, method = None, keep_collapsed = None))]
fn repair_validate_wkb_batch(
    wkbs: Vec<Vec<u8>>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<WkbRepairValidate>> {
    let config = make_config(method, keep_collapsed);
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

/// Repair a WKT geometry string.
///
/// Args:
///     wkt: WKT text of the geometry to repair.
///     method: Optional repair method (``"auto"``, ``"arrange"``, ``"structure"``).
///     keep_collapsed: Optional; when True, collapsed (zero-area)
///         components are kept instead of dropped.
///
/// Returns:
///     WKT text of the repaired geometry.
#[pyfunction]
#[pyo3(signature = (wkt, method = None, keep_collapsed = None))]
fn repair_wkt(wkt: &str, method: Option<&str>, keep_collapsed: Option<bool>) -> PyResult<String> {
    let config = make_config(method, keep_collapsed);
    let geom = parse_wkt(wkt)?;
    let fixed = repair_one(geom, &config);
    Ok(write_wkt(&fixed))
}

/// Repair a list of WKT strings. Unparseable inputs are returned unchanged.
#[pyfunction]
#[pyo3(signature = (wkts, method = None, keep_collapsed = None))]
fn repair_wkt_batch(
    wkts: Vec<String>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<String>> {
    let config = make_config(method, keep_collapsed);
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

/// Parallel repair of a list of WKT strings (requires ``parallel`` feature).
#[cfg(feature = "parallel")]
#[pyfunction]
#[pyo3(signature = (wkts, method = None, keep_collapsed = None))]
fn par_repair_wkt_batch(
    wkts: Vec<String>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<String>> {
    use rayon::prelude::*;
    let config = make_config(method, keep_collapsed);
    let results: Vec<String> = wkts
        .par_iter()
        .map(|wkt| match parse_wkt(wkt) {
            Ok(geom) => write_wkt(&repair_one(geom, &config)),
            Err(_) => wkt.clone(),
        })
        .collect();
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
#[pyo3(signature = (wkt, method = None, keep_collapsed = None))]
fn validate_and_fix_wkt(
    wkt: &str,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<(bool, Vec<String>, String)> {
    let config = make_config(method, keep_collapsed);
    let geom = parse_wkt(wkt)?;
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    let fixed = repair_one(geom, &config);
    Ok((valid, errors, write_wkt(&fixed)))
}

/// Validate then fix each WKT geometry.
///
/// Returns ``[(was_valid, errors_before_repair, fixed_wkt), ...]``.
#[pyfunction]
#[pyo3(signature = (wkts, method = None, keep_collapsed = None))]
fn validate_and_fix_wkt_batch(
    wkts: Vec<String>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<WktValidateAndFix>> {
    let config = make_config(method, keep_collapsed);
    let mut results = Vec::with_capacity(wkts.len());
    for wkt in wkts {
        let r = match parse_wkt(&wkt) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = error_strings(&geom);
                let fixed = repair_one(geom, &config);
                (valid, errors, write_wkt(&fixed))
            }
            Err(e) => (false, vec![format!("{e}")], wkt),
        };
        results.push(r);
    }
    Ok(results)
}

/// Repair + validate a single WKT string.
///
/// Returns ``(fixed_wkt, is_valid_before, [errors])``.
#[pyfunction]
#[pyo3(signature = (wkt, method = None, keep_collapsed = None))]
fn repair_validate_wkt(
    wkt: &str,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<(String, bool, Vec<String>)> {
    let config = make_config(method, keep_collapsed);
    let geom = match parse_wkt(wkt) {
        Ok(geom) => geom,
        Err(e) => return Ok((wkt.to_string(), false, vec![format!("{e}")])),
    };
    let valid = geom.is_valid();
    let errors = error_strings(&geom);
    let fixed = repair_one(geom, &config);
    Ok((write_wkt(&fixed), valid, errors))
}

/// Repair + validate a list of WKT strings.
///
/// Returns ``[(fixed_wkt, is_valid_before, [errors]), ...]``.
#[pyfunction]
#[pyo3(signature = (wkts, method = None, keep_collapsed = None))]
fn repair_validate_wkt_batch(
    wkts: Vec<String>,
    method: Option<&str>,
    keep_collapsed: Option<bool>,
) -> PyResult<Vec<WktRepairValidate>> {
    let config = make_config(method, keep_collapsed);
    let mut results = Vec::with_capacity(wkts.len());
    for wkt in wkts {
        let r = match parse_wkt(&wkt) {
            Ok(geom) => {
                let valid = geom.is_valid();
                let errors = error_strings(&geom);
                let fixed = repair_one(geom, &config);
                (write_wkt(&fixed), valid, errors)
            }
            Err(e) => (wkt.to_string(), false, vec![format!("{e}")]),
        };
        results.push(r);
    }
    Ok(results)
}

//! WKT single-geometry API (extracted from bindings/ffi.rs 2026-08-07; verbatim).



use alloc::string::String;
use super::types::*;
use super::util::*;
use std::ffi::c_char;
use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use geo::Geometry;
use crate::core::MakeValidConfig;
use crate::io::wkt::write_wkt;
use crate::make_valid::MakeValid;
use crate::validation::GeoValidation;


fn wkt_result_from(wkt: *const c_char, f: impl FnOnce(Geometry<f64>) -> GeoRepairStringResult) -> GeoRepairStringResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match wkt_from_cstr(wkt) {
            Ok(g) => g,
            Err(code) => return GeoRepairStringResult::error(code, "WKT parse error"),
        };
        f(geom)
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairStringResult::error(GeoRepairErrorCode::Panic, "internal error: operation panicked"),
    }
}

fn repair_to_wkt(geom: Geometry<f64>, config: &MakeValidConfig) -> GeoRepairStringResult {
    let fixed = geom.make_valid_with_config(config);
    GeoRepairStringResult::success(write_wkt(&fixed))
}

/// Repair a WKT geometry using default configuration.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string. The returned
/// [`GeoRepairStringResult`] must be freed with
/// [`geo_repair_free_string_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid_wkt(wkt: *const c_char) -> GeoRepairStringResult {
    wkt_result_from(wkt, |geom| {
        let config = make_config(false, 0, 0, 0);
        repair_to_wkt(geom, &config)
    })
}

/// Repair a WKT geometry with configuration.
///
/// `poly_method`: 0 = Auto, 1 = Arrange, 2 = Structure.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string. The returned
/// [`GeoRepairStringResult`] must be freed with
/// [`geo_repair_free_string_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid_wkt_with_config(
    wkt: *const c_char,
    keep_collapsed: bool,
    poly_method: u8,
) -> GeoRepairStringResult {
    wkt_result_from(wkt, |geom| {
        let config = make_config(keep_collapsed, poly_method, 0, 0);
        repair_to_wkt(geom, &config)
    })
}

/// Repair a WKT geometry with full configuration.
///
/// `poly_method`: 0 = Auto, 1 = Arrange, 2 = Structure.
/// `fill_rule`: 0 = EvenOdd, 1 = NonZero. `epsg_code` <= 0 means unknown CRS.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string. The returned
/// [`GeoRepairStringResult`] must be freed with
/// [`geo_repair_free_string_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid_wkt_with_config_full(
    wkt: *const c_char,
    keep_collapsed: bool,
    poly_method: u8,
    fill_rule: u8,
    epsg_code: i32,
) -> GeoRepairStringResult {
    wkt_result_from(wkt, |geom| {
        let config = make_config(keep_collapsed, poly_method, fill_rule, epsg_code);
        repair_to_wkt(geom, &config)
    })
}

/// Check whether a WKT geometry is OGC-valid.
///
/// Returns 1 if valid, 0 if invalid, 0 on parse failure.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_is_valid_wkt(wkt: *const c_char) -> u8 {
    catch_unwind(AssertUnwindSafe(|| {
        let geom = match wkt_from_cstr(wkt) {
            Ok(g) => g,
            Err(_) => return 0,
        };
        if geom.is_valid() { 1 } else { 0 }
    }))
    .unwrap_or_default()
}

/// Validate a WKT geometry.
///
/// `success == true` when the geometry is valid; `success == false`,
/// `error_code == InvalidGeometry` and `error_msg` set to the joined
/// violation reasons when invalid.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string. The returned
/// [`GeoRepairStringResult`] must be freed with
/// [`geo_repair_free_string_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_wkt(wkt: *const c_char) -> GeoRepairStringResult {
    wkt_result_from(wkt, |geom| {
        if geom.is_valid() {
            GeoRepairStringResult::success(String::new())
        } else {
            GeoRepairStringResult::invalid_geometry(&all_error_reasons(&geom))
        }
    })
}

/// Validate a WKT geometry, then repair it if invalid.
///
/// On success (`result.success == true`):
/// - `data` / `len` contain the (possibly repaired) WKT string.
/// - `error_msg` is `NULL` and `error_code == None` when the input was
///   already valid; when the input was invalid, `error_code ==
///   InvalidGeometry` and `error_msg` contains the validation reasons.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string. The returned
/// [`GeoRepairStringResult`] must be freed with
/// [`geo_repair_free_string_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_and_fix_wkt(wkt: *const c_char) -> GeoRepairStringResult {
    wkt_result_from(wkt, |geom| {
        let reasons = if geom.is_valid() {
            None
        } else {
            Some(all_error_reasons(&geom))
        };
        let config = make_config(false, 0, 0, 0);
        let mut res = repair_to_wkt(geom, &config);
        if let Some(reason) = reasons {
            res.error_code = GeoRepairErrorCode::InvalidGeometry;
            res.error_msg = CString::new(reason).unwrap_or_default().into_raw();
        }
        res
    })
}

/// Validate a WKT geometry, then repair it with configuration.
///
/// `poly_method`: 0 = Auto, 1 = Arrange, 2 = Structure.
///
/// On success (`result.success == true`):
/// - `data` / `len` contain the (possibly repaired) WKT string.
/// - `error_msg` is `NULL` and `error_code == None` when the input was
///   already valid; when the input was invalid, `error_code ==
///   InvalidGeometry` and `error_msg` contains the validation reasons.
///
/// # Safety
///
/// `wkt` must be a valid NUL-terminated string. The returned
/// [`GeoRepairStringResult`] must be freed with
/// [`geo_repair_free_string_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_and_fix_wkt_with_config(
    wkt: *const c_char,
    keep_collapsed: bool,
    poly_method: u8,
) -> GeoRepairStringResult {
    wkt_result_from(wkt, |geom| {
        let reasons = if geom.is_valid() {
            None
        } else {
            Some(all_error_reasons(&geom))
        };
        let config = make_config(keep_collapsed, poly_method, 0, 0);
        let mut res = repair_to_wkt(geom, &config);
        if let Some(reason) = reasons {
            res.error_code = GeoRepairErrorCode::InvalidGeometry;
            res.error_msg = CString::new(reason).unwrap_or_default().into_raw();
        }
        res
    })
}

// ---------------------------------------------------------------------------
// Memory management
// ---------------------------------------------------------------------------


use super::types::*;
use super::util::*;
use crate::core::MakeValidConfig;
use crate::make_valid::MakeValid;
use crate::validation::GeoValidation;
use alloc::vec::Vec;
use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};

///
/// # Safety
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(code) => return GeoRepairResult::error(code, "WKB parse error"),
        };
        let fixed = geom.make_valid();
        match geometry_to_wkb(&fixed) {
            Ok(wkb) => GeoRepairResult::success(wkb),
            Err(code) => GeoRepairResult::error(code, "WKB write error"),
        }
    })) {
        Ok(r) => r,
        Err(_) => {
            GeoRepairResult::error(GeoRepairErrorCode::Panic, "internal error: repair panicked")
        }
    }
}

///
/// # Safety
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid_with_config(
    wkb_data: *const u8,
    wkb_len: usize,
    keep_collapsed: bool,
    poly_method: u8,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(code) => return GeoRepairResult::error(code, "WKB parse error"),
        };
        let config = make_config(keep_collapsed, poly_method, 0, 0);
        let fixed = geom.make_valid_with_config(&config);
        match geometry_to_wkb(&fixed) {
            Ok(wkb) => GeoRepairResult::success(wkb),
            Err(code) => GeoRepairResult::error(code, "WKB write error"),
        }
    })) {
        Ok(r) => r,
        Err(_) => {
            GeoRepairResult::error(GeoRepairErrorCode::Panic, "internal error: repair panicked")
        }
    }
}

///
/// # Safety
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid_with_config_full(
    wkb_data: *const u8,
    wkb_len: usize,
    keep_collapsed: bool,
    poly_method: u8,
    fill_rule: u8,
    epsg_code: i32,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(code) => return GeoRepairResult::error(code, "WKB parse error"),
        };
        let config = make_config(keep_collapsed, poly_method, fill_rule, epsg_code);
        let fixed = geom.make_valid_with_config(&config);
        match geometry_to_wkb(&fixed) {
            Ok(wkb) => GeoRepairResult::success(wkb),
            Err(code) => GeoRepairResult::error(code, "WKB write error"),
        }
    })) {
        Ok(r) => r,
        Err(_) => {
            GeoRepairResult::error(GeoRepairErrorCode::Panic, "internal error: repair panicked")
        }
    }
}


///
/// # Safety
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_is_valid(wkb_data: *const u8, wkb_len: usize) -> u8 {
    catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(_) => return 0,
        };
        if geom.is_valid() { 1 } else { 0 }
    }))
    .unwrap_or_default()
}

///
/// # Safety
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(code) => return GeoRepairResult::error(code, "WKB parse error"),
        };
        if geom.is_valid() {
            GeoRepairResult::success(Vec::new())
        } else {
            GeoRepairResult::invalid_geometry(&all_error_reasons(&geom))
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error(
            GeoRepairErrorCode::Panic,
            "internal error: validation panicked",
        ),
    }
}

///
/// # Safety
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_reason(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    unsafe { geo_repair_validate(wkb_data, wkb_len) }
}


#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_and_fix(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(code) => return GeoRepairResult::error(code, "WKB parse error"),
        };
        let reasons = if geom.is_valid() {
            None
        } else {
            Some(all_error_reasons(&geom))
        };
        let fixed = geom.make_valid();
        let wkb = match geometry_to_wkb(&fixed) {
            Ok(w) => w,
            Err(code) => return GeoRepairResult::error(code, "WKB write error"),
        };
        match reasons {
            Some(reason) => {
                let mut res = GeoRepairResult::success(wkb);
                res.error_code = GeoRepairErrorCode::InvalidGeometry;
                res.error_msg = CString::new(reason).unwrap_or_default().into_raw();
                res
            }
            None => GeoRepairResult::success(wkb),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error(
            GeoRepairErrorCode::Panic,
            "internal error: validate_and_fix panicked",
        ),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_and_fix_with_config(
    wkb_data: *const u8,
    wkb_len: usize,
    keep_collapsed: bool,
    poly_method: u8,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(code) => return GeoRepairResult::error(code, "WKB parse error"),
        };
        let reasons = if geom.is_valid() {
            None
        } else {
            Some(all_error_reasons(&geom))
        };
        let config = make_config(keep_collapsed, poly_method, 0, 0);
        let fixed = geom.make_valid_with_config(&config);
        let wkb = match geometry_to_wkb(&fixed) {
            Ok(w) => w,
            Err(code) => return GeoRepairResult::error(code, "WKB write error"),
        };
        match reasons {
            Some(reason) => {
                let mut res = GeoRepairResult::success(wkb);
                res.error_code = GeoRepairErrorCode::InvalidGeometry;
                res.error_msg = CString::new(reason).unwrap_or_default().into_raw();
                res
            }
            None => GeoRepairResult::success(wkb),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error(
            GeoRepairErrorCode::Panic,
            "internal error: validate_and_fix panicked",
        ),
    }
}


pub(crate) fn repair_batch_sequential(
    inputs: &[GeoRepairWkbBuffer],
    config: &MakeValidConfig,
) -> Vec<GeoRepairResult> {
    inputs
        .iter()
        .map(|buf| {
            let geom = match geometry_from_wkb(buf.data, buf.len) {
                Ok(g) => g,
                Err(code) => {
                    return GeoRepairResult::error(code, "WKB parse error");
                }
            };
            let fixed = geom.make_valid_with_config(config);
            match geometry_to_wkb(&fixed) {
                Ok(wkb) => GeoRepairResult::success(wkb),
                Err(code) => GeoRepairResult::error(code, "WKB write error"),
            }
        })
        .collect()
}

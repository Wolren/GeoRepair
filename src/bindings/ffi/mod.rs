//! C-compatible ffi bindings for geo-repair, behind the ffi feature. The API covers WKB
//! (the GIS-native binary) and WKT, single and batch. # C header The header is
//! include/geo_repair.h, shipped with each release. # Error model Every result carries
//! a GeoRepairErrorCode: None, Parse, InvalidInput, Encode, GeoRepairErrorCode::Panic,
//! InvalidGeometry on validate paths only. Free results with geo_repair_free_*. # Panic
//! safety and build profile Panic containment uses catch_unwind and needs panic =
//! "unwind", as in the Cargo.toml release profile.

use alloc::vec::Vec;
use std::ffi::{CStr, CString, c_char};
use std::ptr;

/// Static nul-terminated version string, derived from Cargo.toml at compile time
/// (`CARGO_PKG_VERSION`) so the C API and the crate can never drift.
static VERSION_CSTR: &CStr = unsafe {
    CStr::from_bytes_with_nul_unchecked(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes())
};

/// Return the geo-repair library version as a nul-terminated string.
///
/// The caller must not free it.
#[unsafe(no_mangle)]
pub extern "C" fn geo_repair_version() -> *const c_char {
    VERSION_CSTR.as_ptr()
}

/// Free a [`GeoRepairResult`] returned by any WKB `geo_repair_*` function.
///
///
/// # Safety
///
/// `result` must not be null and must point to a valid result that has not been freed
/// before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_free_result(result: *mut GeoRepairResult) {
    if result.is_null() {
        return;
    }
    let r = unsafe { &mut *result };
    if !r.wkb_data.is_null() {
        unsafe { drop(Vec::from_raw_parts(r.wkb_data, r.wkb_len, r.wkb_len)) };
    }
    if !r.error_msg.is_null() {
        // safety: the error string was allocated by CString.
        unsafe { drop(CString::from_raw(r.error_msg)) };
    }
    r.success = false;
    r.error_code = GeoRepairErrorCode::None;
    r.wkb_data = ptr::null_mut();
    r.wkb_len = 0;
    r.error_msg = ptr::null_mut();
}

/// Free a [`GeoRepairStringResult`] returned by any WKT `geo_repair_*` function.
///
///
/// # Safety
///
/// `result` must not be null and must point to a valid result that has not been freed
/// before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_free_string_result(result: *mut GeoRepairStringResult) {
    if result.is_null() {
        return;
    }
    let r = unsafe { &mut *result };
    if !r.data.is_null() {
        unsafe { drop(CString::from_raw(r.data)) };
    }
    if !r.error_msg.is_null() {
        // safety: the error string was allocated by CString.
        unsafe { drop(CString::from_raw(r.error_msg)) };
    }
    r.success = false;
    r.error_code = GeoRepairErrorCode::None;
    r.data = ptr::null_mut();
    r.len = 0;
    r.error_msg = ptr::null_mut();
}

/// Free a [`GeoRepairBatchResult`] returned by [`geo_repair_make_valid_batch`].
///
///
/// # Safety
///
/// `result` must not be null and must point to a valid result that has not been freed
/// before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_free_batch_result(result: *mut GeoRepairBatchResult) {
    if result.is_null() {
        return;
    }
    let r = unsafe { &mut *result };
    if !r.items.is_null() && r.count > 0 {
        let items = unsafe { std::slice::from_raw_parts_mut(r.items, r.count) };
        for item in items.iter_mut() {
            if !item.wkb_data.is_null() {
                unsafe {
                    drop(Vec::from_raw_parts(
                        item.wkb_data,
                        item.wkb_len,
                        item.wkb_len,
                    ))
                };
            }
            if !item.error_msg.is_null() {
                unsafe { drop(CString::from_raw(item.error_msg)) };
            }
        }
        unsafe { drop(Vec::from_raw_parts(r.items, r.count, r.count)) };
    }
    if !r.error_msg.is_null() {
        // safety: the error string was allocated by CString.
        unsafe { drop(CString::from_raw(r.error_msg)) };
    }
    r.success = false;
    r.error_code = GeoRepairErrorCode::None;
    r.items = ptr::null_mut();
    r.count = 0;
    r.error_msg = ptr::null_mut();
}

mod batch;
mod types;
mod util;
mod wkb;
mod wkt;

pub use crate::bindings::ffi::batch::*;
pub use crate::bindings::ffi::types::*;
pub use crate::bindings::ffi::wkb::*;
pub use crate::bindings::ffi::wkt::*;

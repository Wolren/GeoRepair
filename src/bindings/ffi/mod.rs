//! C-compatible FFI bindings for geo-repair.
//!
//! Enable the `ffi` feature for a C-compatible API. The C surface covers
//! WKB (the GIS-native binary format) and WKT (text), single geometries
//! and parallel batches.
//!
//! # C header
//!
//! The canonical header lives at `include/geo_repair.h` and is shipped
//! with every release. The API is panic-safe: a Rust panic inside the
//! library is caught and surfaced as `GeoRepairErrorCode::Panic` in the
//! result. Every result must be released with the matching
//! `geo_repair_free_*` function when no longer needed.
//!
//! # Error model
//!
//! Every result carries a `GeoRepairErrorCode` so C callers can branch
//! programmatically without parsing `error_msg`:
//!
//! - `None` — operation succeeded.
//! - `Parse` — input WKB/WKT could not be parsed.
//! - `InvalidInput` — null pointer or invalid length argument.
//! - `InvalidGeometry` — validation found violations (only on
//!   `geo_repair_validate*` / `geo_repair_validate_and_fix*` paths, where
//!   the geometry being invalid is the *result*, not a failure).
//! - `Encode` — the output geometry could not be encoded to WKB/WKT.
//! - `Panic` — an internal panic was caught. This should never happen;
//!   report it as a bug.
//!
//! # Panic safety and build profile
//!
//! Panic containment relies on `catch_unwind`, which requires the crate
//! to be built with `panic = "unwind"`. The library release profile uses
//! unwind (see `[profile.release]` in Cargo.toml); building the FFI with
//! `panic = "abort"` disables containment and a panic would abort the
//! host process.

use alloc::vec::Vec;
use std::ffi::{CStr, CString, c_char};
use std::ptr;

/// Static NUL-terminated version string, derived from Cargo.toml at compile
/// time (`CARGO_PKG_VERSION`) so the C API and the crate can never drift.
static VERSION_CSTR: &CStr = unsafe {
    CStr::from_bytes_with_nul_unchecked(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes())
};

/// Return the geo-repair library version as a NUL-terminated string.
///
/// The returned pointer is static — the caller must NOT free it.
#[unsafe(no_mangle)]
pub extern "C" fn geo_repair_version() -> *const c_char {
    VERSION_CSTR.as_ptr()
}

/// Free a [`GeoRepairResult`] returned by any WKB `geo_repair_*` function.
///
/// After calling this function the result struct is zeroed so that
/// double-free is harmless (the inner pointers will be null).
///
/// # Safety
///
/// `result` must not be null and must point to a valid result that has
/// not been freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_free_result(result: *mut GeoRepairResult) {
    if result.is_null() {
        return;
    }
    // SAFETY: caller guarantees result is a valid, non-null pointer.
    let r = unsafe { &mut *result };
    if !r.wkb_data.is_null() {
        // SAFETY: the WKB buffer was allocated by Vec in success().
        unsafe { drop(Vec::from_raw_parts(r.wkb_data, r.wkb_len, r.wkb_len)) };
    }
    if !r.error_msg.is_null() {
        // SAFETY: the error string was allocated by CString.
        unsafe { drop(CString::from_raw(r.error_msg)) };
    }
    r.success = false;
    r.error_code = GeoRepairErrorCode::None;
    r.wkb_data = ptr::null_mut();
    r.wkb_len = 0;
    r.error_msg = ptr::null_mut();
}

/// Free a [`GeoRepairStringResult`] returned by any WKT `geo_repair_*`
/// function.
///
/// After calling this function the result struct is zeroed so that
/// double-free is harmless.
///
/// # Safety
///
/// `result` must not be null and must point to a valid result that has
/// not been freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_free_string_result(result: *mut GeoRepairStringResult) {
    if result.is_null() {
        return;
    }
    // SAFETY: caller guarantees result is a valid, non-null pointer.
    let r = unsafe { &mut *result };
    if !r.data.is_null() {
        // SAFETY: the string was allocated by CString.
        unsafe { drop(CString::from_raw(r.data)) };
    }
    if !r.error_msg.is_null() {
        // SAFETY: the error string was allocated by CString.
        unsafe { drop(CString::from_raw(r.error_msg)) };
    }
    r.success = false;
    r.error_code = GeoRepairErrorCode::None;
    r.data = ptr::null_mut();
    r.len = 0;
    r.error_msg = ptr::null_mut();
}

/// Free a [`GeoRepairBatchResult`] returned by
/// [`geo_repair_make_valid_batch`].
///
/// Frees every per-item result, the item array, and the batch error
/// string, then zeroes the struct so double-free is harmless.
///
/// # Safety
///
/// `result` must not be null and must point to a valid result that has
/// not been freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_free_batch_result(result: *mut GeoRepairBatchResult) {
    if result.is_null() {
        return;
    }
    // SAFETY: caller guarantees result is a valid, non-null pointer.
    let r = unsafe { &mut *result };
    if !r.items.is_null() && r.count > 0 {
        // SAFETY: the item array was allocated by Vec in success(). Each
        // item owns its own buffers; free them before the array itself.
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
        // SAFETY: the error string was allocated by CString.
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

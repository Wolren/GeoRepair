//! C-compatible FFI bindings for geo-repair.
//!
//! Enable the `ffi` feature for a C-compatible API using WKB.
//!
//! # C header
//!
//! ```c
//! #include <stdint.h>
//! #include <stdbool.h>
//!
//! typedef struct {
//!     bool      success;
//!     uint8_t*  wkb_data;
//!     size_t    wkb_len;
//!     char*     error_msg;
//! } GeoRepairResult;
//!
//! // --- Repair ---
//! GeoRepairResult geo_repair_make_valid(const uint8_t* wkb_data, size_t wkb_len);
//! GeoRepairResult geo_repair_make_valid_with_config(
//!     const uint8_t* wkb_data, size_t wkb_len,
//!     bool keep_collapsed, uint8_t poly_method);
//! GeoRepairResult geo_repair_make_valid_with_config_full(
//!     const uint8_t* wkb_data, size_t wkb_len,
//!     bool keep_collapsed, uint8_t poly_method,
//!     uint8_t fill_rule, int32_t epsg_code);
//!
//! // --- Validation ---
//! uint8_t         geo_repair_is_valid(const uint8_t* wkb_data, size_t wkb_len);
//! GeoRepairResult geo_repair_validate(const uint8_t* wkb_data, size_t wkb_len);
//! GeoRepairResult geo_repair_validate_reason(const uint8_t* wkb_data, size_t wkb_len);
//!
//! // --- Combined validate + fix ---
//! // Returns fixed WKB on success.  error_msg is null when input was valid,
//! // or contains validation errors when input was invalid.
//! GeoRepairResult geo_repair_validate_and_fix(const uint8_t* wkb_data, size_t wkb_len);
//! GeoRepairResult geo_repair_validate_and_fix_with_config(
//!     const uint8_t* wkb_data, size_t wkb_len,
//!     bool keep_collapsed, uint8_t poly_method);
//!
//! // --- Memory management ---
//! void            geo_repair_free_result(GeoRepairResult* result);
//! ```
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::core::MakeValidConfig;
use crate::make_valid::MakeValid;
use crate::validation::GeoValidation;
use geo::Geometry;

/// Result returned by geo-repair C FFI functions.
///
/// On success, `wkb_data` / `wkb_len` contain the output WKB geometry
/// and `error_msg` is null.  On failure, `wkb_data` is null and
/// `error_msg` points to a NUL-terminated error string.
///
/// # Safety
///
/// The caller must call [`geo_repair_free_result`] to release the
/// allocated memory when the result is no longer needed.
#[repr(C)]
#[derive(Debug)]
pub struct GeoRepairResult {
    /// Whether the operation succeeded. When `true`, `wkb_data`/`wkb_len` are valid.
    pub success: bool,
    /// Pointer to the output WKB byte buffer (valid when `success` is true).
    pub wkb_data: *mut u8,
    /// Length of the output WKB buffer in bytes.
    pub wkb_len: usize,
    /// NUL-terminated error message string (non-null when `success` is false).
    pub error_msg: *mut c_char,
}

impl GeoRepairResult {
    fn success(wkb: Vec<u8>) -> Self {
        let mut wkb = wkb;
        wkb.shrink_to_fit();
        let len = wkb.len();
        let ptr = wkb.as_mut_ptr();
        std::mem::forget(wkb);
        Self {
            success: true,
            wkb_data: ptr,
            wkb_len: len,
            error_msg: ptr::null_mut(),
        }
    }

    fn error(msg: &str) -> Self {
        let c_msg = CString::new(msg).unwrap_or_default();
        Self {
            success: false,
            wkb_data: ptr::null_mut(),
            wkb_len: 0,
            error_msg: c_msg.into_raw(),
        }
    }
}

fn geometry_from_wkb(data: *const u8, len: usize) -> Result<Geometry<f64>, String> {
    if data.is_null() {
        return Err("null pointer".to_string());
    }
    if len == 0 || len > isize::MAX as usize {
        return Err("invalid length".to_string());
    }
    let buf = unsafe { std::slice::from_raw_parts(data, len) };
    let wkb_geom = wkb::reader::read_wkb(buf).map_err(|e| format!("WKB parse error: {e}"))?;
    let geo_geom = geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom);
    Ok(geo_geom)
}

fn geometry_to_wkb(geom: &Geometry<f64>) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    use wkb::writer::{WriteOptions, geometry_wkb_size, write_geometry};

    let opts = WriteOptions::default();
    let size = geometry_wkb_size(geom);
    let mut buf = vec![0u8; size];
    write_geometry(&mut Cursor::new(&mut buf[..]), geom, &opts)
        .map_err(|e| format!("WKB write error: {e}"))?;
    Ok(buf)
}

/// Repair a geometry from WKB using default configuration.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(e) => return GeoRepairResult::error(&e),
        };
        let fixed = geom.make_valid();
        match geometry_to_wkb(&fixed) {
            Ok(wkb) => GeoRepairResult::success(wkb),
            Err(e) => GeoRepairResult::error(&e),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: repair panicked"),
    }
}

/// Repair a geometry from WKB with configuration.
///
/// `poly_method`: 0 = Auto, 1 = Arrange, 2 = Structure.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
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
            Err(e) => return GeoRepairResult::error(&e),
        };
        let config = MakeValidConfig {
            keep_collapsed,
            poly_method: match poly_method {
                0 => crate::PolyMethod::Auto,
                1 => crate::PolyMethod::Arrange,
                2 => crate::PolyMethod::Structure,
                _ => crate::PolyMethod::Auto,
            },
            fill_rule: Default::default(),
            crs: None,
            target_crs: None,
        };
        let fixed = geom.make_valid_with_config(&config);
        match geometry_to_wkb(&fixed) {
            Ok(wkb) => GeoRepairResult::success(wkb),
            Err(e) => GeoRepairResult::error(&e),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: repair panicked"),
    }
}

/// Repair a geometry from WKB with full configuration.
///
/// `poly_method`: 0 = Auto, 1 = Arrange, 2 = Structure.
/// `fill_rule`: 0 = EvenOdd, 1 = NonZero.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
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
            Err(e) => return GeoRepairResult::error(&e),
        };
        let config = MakeValidConfig {
            keep_collapsed,
            poly_method: match poly_method {
                0 => crate::PolyMethod::Auto,
                1 => crate::PolyMethod::Arrange,
                2 => crate::PolyMethod::Structure,
                _ => crate::PolyMethod::Auto,
            },
            fill_rule: match fill_rule {
                0 => geo::algorithm::bool_ops::FillRule::EvenOdd,
                _ => geo::algorithm::bool_ops::FillRule::NonZero,
            },
            crs: if epsg_code > 0 {
                Some(crate::Crs::from_epsg(epsg_code as u32))
            } else {
                None
            },
            target_crs: None,
        };
        let fixed = geom.make_valid_with_config(&config);
        match geometry_to_wkb(&fixed) {
            Ok(wkb) => GeoRepairResult::success(wkb),
            Err(e) => GeoRepairResult::error(&e),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: repair panicked"),
    }
}

/// Check whether a WKB-encoded geometry is OGC-valid.
///
/// Returns 1 if valid, 0 if invalid, and sets `error_msg` on the result
/// when invalid to describe the violation.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
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

/// Validate a WKB-encoded geometry and return a human-readable reason.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned string must be freed with [`geo_repair_free_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_reason(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(e) => return GeoRepairResult::error(&e),
        };
        if geom.is_valid() {
            GeoRepairResult::success(Vec::new())
        } else {
            let reason = geom.validate_reason();
            GeoRepairResult::error(&reason)
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: validation panicked"),
    }
}

/// Validate a WKB-encoded geometry.
///
/// Returns a result with:
/// - `success = true` and `wkb_len == 0` when the geometry is valid.
/// - `success = false` and `error_msg` set to the violation reasons when invalid.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(e) => return GeoRepairResult::error(&e),
        };
        if geom.is_valid() {
            GeoRepairResult::success(Vec::new())
        } else {
            let reason = geom.validate_reason();
            GeoRepairResult::error(&reason)
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: validation panicked"),
    }
}

/// Validate a WKB geometry, then repair it if invalid.
///
/// On success (`result.success == true`):
/// - `wkb_data` / `wkb_len` contain the (possibly repaired) WKB geometry.
/// - `error_msg` is `NULL` when the input was already valid, or contains
///   the validation error reasons when the input was repaired.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_validate_and_fix(
    wkb_data: *const u8,
    wkb_len: usize,
) -> GeoRepairResult {
    match catch_unwind(AssertUnwindSafe(|| {
        let geom = match geometry_from_wkb(wkb_data, wkb_len) {
            Ok(g) => g,
            Err(e) => return GeoRepairResult::error(&e),
        };
        let errors = if geom.is_valid() {
            None
        } else {
            Some(geom.validate_reason())
        };
        let fixed = geom.make_valid();
        let wkb = match geometry_to_wkb(&fixed) {
            Ok(w) => w,
            Err(e) => return GeoRepairResult::error(&e),
        };
        match errors {
            Some(reason) => {
                let mut res = GeoRepairResult::success(wkb);
                res.error_msg = CString::new(reason).unwrap_or_default().into_raw();
                res
            }
            None => GeoRepairResult::success(wkb),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: validate_and_fix panicked"),
    }
}

/// Validate a WKB geometry, then repair it with configuration.
///
/// `poly_method`: 0 = Auto, 1 = Arrange, 2 = Structure.
///
/// On success (`result.success == true`):
/// - `wkb_data` / `wkb_len` contain the (possibly repaired) WKB geometry.
/// - `error_msg` is `NULL` when the input was already valid, or contains
///   the validation error reasons when the input was repaired.
///
/// # Safety
///
/// `wkb_data` must point to a valid WKB buffer of `wkb_len` bytes.
/// The returned [`GeoRepairResult`] must be freed with
/// [`geo_repair_free_result`].
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
            Err(e) => return GeoRepairResult::error(&e),
        };
        let errors = if geom.is_valid() {
            None
        } else {
            Some(geom.validate_reason())
        };
        let config = MakeValidConfig {
            keep_collapsed,
            poly_method: match poly_method {
                0 => crate::PolyMethod::Auto,
                1 => crate::PolyMethod::Arrange,
                2 => crate::PolyMethod::Structure,
                _ => crate::PolyMethod::Auto,
            },
            fill_rule: Default::default(),
            crs: None,
            target_crs: None,
        };
        let fixed = geom.make_valid_with_config(&config);
        let wkb = match geometry_to_wkb(&fixed) {
            Ok(w) => w,
            Err(e) => return GeoRepairResult::error(&e),
        };
        match errors {
            Some(reason) => {
                let mut res = GeoRepairResult::success(wkb);
                res.error_msg = CString::new(reason).unwrap_or_default().into_raw();
                res
            }
            None => GeoRepairResult::success(wkb),
        }
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairResult::error("internal error: validate_and_fix panicked"),
    }
}

/// Free a [`GeoRepairResult`] returned by any `geo_repair_*` function.
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
        // SAFETY: the error string was allocated by CString in error().
        unsafe { drop(CString::from_raw(r.error_msg)) };
    }
    r.success = false;
    r.wkb_data = ptr::null_mut();
    r.wkb_len = 0;
    r.error_msg = ptr::null_mut();
}

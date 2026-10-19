
use alloc::string::String;
use alloc::vec::Vec;
use std::ffi::{CString, c_char};
use std::ptr;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoRepairErrorCode {
    None = 0,
    Parse = 1,
    InvalidInput = 2,
    InvalidGeometry = 3,
    Encode = 4,
    /// An internal panic was caught.
    Panic = 5,
}

/// This happens when the input was invalid and had to be repaired. # Safety Call
/// super::geo_repair_free_result to release the memory.
#[repr(C)]
#[derive(Debug)]
pub struct GeoRepairResult {
    /// Whether the operation succeeded.
    pub success: bool,
    /// Programmatic error classification (see [`GeoRepairErrorCode`]).
    pub error_code: GeoRepairErrorCode,
    /// Pointer to the output WKB byte buffer (valid when `success` is true).
    pub wkb_data: *mut u8,
    /// Length of the output WKB buffer in bytes.
    pub wkb_len: usize,
    pub error_msg: *mut c_char,
}

impl GeoRepairResult {
    pub(crate) fn success(wkb: Vec<u8>) -> Self {
        if wkb.is_empty() {
            return Self {
                success: true,
                error_code: GeoRepairErrorCode::None,
                wkb_data: ptr::null_mut(),
                wkb_len: 0,
                error_msg: ptr::null_mut(),
            };
        }
        let mut wkb = wkb;
        wkb.shrink_to_fit();
        let len = wkb.len();
        let ptr = wkb.as_mut_ptr();
        std::mem::forget(wkb);
        Self {
            success: true,
            error_code: GeoRepairErrorCode::None,
            wkb_data: ptr,
            wkb_len: len,
            error_msg: ptr::null_mut(),
        }
    }

    pub(crate) fn error(code: GeoRepairErrorCode, msg: &str) -> Self {
        let c_msg = CString::new(msg).unwrap_or_default();
        Self {
            success: false,
            error_code: code,
            wkb_data: ptr::null_mut(),
            wkb_len: 0,
            error_msg: c_msg.into_raw(),
        }
    }

    pub(crate) fn invalid_geometry(msg: &str) -> Self {
        Self::error(GeoRepairErrorCode::InvalidGeometry, msg)
    }
}

///
/// # Safety
#[repr(C)]
#[derive(Debug)]
pub struct GeoRepairStringResult {
    /// Whether the operation succeeded.
    pub success: bool,
    /// Programmatic error classification (see [`GeoRepairErrorCode`]).
    pub error_code: GeoRepairErrorCode,
    /// Pointer to the output WKT string (nul-terminated, valid when `success` is true).
    pub data: *mut c_char,
    /// Length of the output WKT string in bytes (excluding the nul).
    pub len: usize,
    pub error_msg: *mut c_char,
}

impl GeoRepairStringResult {
    pub(crate) fn success(wkt: String) -> Self {
        let c = CString::new(wkt).unwrap_or_default();
        let len = c.as_bytes().len();
        Self {
            success: true,
            error_code: GeoRepairErrorCode::None,
            data: c.into_raw(),
            len,
            error_msg: ptr::null_mut(),
        }
    }

    pub(crate) fn error(code: GeoRepairErrorCode, msg: &str) -> Self {
        let c_msg = CString::new(msg).unwrap_or_default();
        Self {
            success: false,
            error_code: code,
            data: ptr::null_mut(),
            len: 0,
            error_msg: c_msg.into_raw(),
        }
    }

    pub(crate) fn invalid_geometry(msg: &str) -> Self {
        Self::error(GeoRepairErrorCode::InvalidGeometry, msg)
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct GeoRepairWkbBuffer {
    /// Pointer to a WKB byte buffer.
    pub data: *const u8,
    /// Length of the buffer in bytes.
    pub len: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct GeoRepairBatchResult {
    /// Whether the batch call itself succeeded.
    pub success: bool,
    /// Programmatic error classification for the batch call itself.
    pub error_code: GeoRepairErrorCode,
    /// Array of `count` per-item results (valid when `success` is true).
    pub items: *mut GeoRepairResult,
    /// Number of items in the batch.
    pub count: usize,
    /// nul-terminated error string for batch-level failures.
    pub error_msg: *mut c_char,
}

impl GeoRepairBatchResult {
    pub(crate) fn success(items: Vec<GeoRepairResult>) -> Self {
        let count = items.len();
        if count == 0 {
            return Self {
                success: true,
                error_code: GeoRepairErrorCode::None,
                items: ptr::null_mut(),
                count: 0,
                error_msg: ptr::null_mut(),
            };
        }
        let mut items = items;
        let ptr = items.as_mut_ptr();
        std::mem::forget(items);
        Self {
            success: true,
            error_code: GeoRepairErrorCode::None,
            items: ptr,
            count,
            error_msg: ptr::null_mut(),
        }
    }

    pub(crate) fn error(code: GeoRepairErrorCode, msg: &str) -> Self {
        let c_msg = CString::new(msg).unwrap_or_default();
        Self {
            success: false,
            error_code: code,
            items: ptr::null_mut(),
            count: 0,
            error_msg: c_msg.into_raw(),
        }
    }
}


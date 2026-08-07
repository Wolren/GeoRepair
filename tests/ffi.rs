//! Runtime tests for the C FFI surface (`ffi` feature).
//!
//! The extern "C" entry points are exercised directly from Rust (they are
//! plain functions), covering: version, validity, repair (all config
//! depths), validation (error codes + reasons), validate-and-fix, WKT
//! entry points, batch semantics (parallel and sequential), null/garbage
//! input handling, and free/double-free safety. The C compiler harness
//! (tests/c/test_geo_repair.c) compiles and links the same surface from C;
//! this suite is the cross-platform runtime check that runs everywhere.
#![cfg(feature = "ffi")]

use geo::Geometry;
use geo_repair::bindings::ffi::{
    GeoRepairErrorCode, GeoRepairResult, GeoRepairStringResult, GeoRepairWkbBuffer,
    geo_repair_free_batch_result, geo_repair_free_result, geo_repair_free_string_result,
    geo_repair_is_valid, geo_repair_is_valid_wkt, geo_repair_make_valid,
    geo_repair_make_valid_batch, geo_repair_make_valid_wkt, geo_repair_make_valid_wkt_with_config,
    geo_repair_make_valid_with_config, geo_repair_validate, geo_repair_validate_and_fix,
    geo_repair_validate_and_fix_with_config, geo_repair_validate_and_fix_wkt,
    geo_repair_validate_reason, geo_repair_validate_wkt, geo_repair_version,
};
use geo_repair::io::wkb::{read_wkb, write_wkb};
use geo_repair::io::wkt::{read_wkt, write_wkt};
use geo_repair::validation::GeoValidation;
use std::ffi::{CStr, CString};
use std::ptr;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn square() -> Geometry<f64> {
    read_wkt("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))").unwrap()
}

fn bowtie() -> Geometry<f64> {
    read_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap()
}

fn square_wkb() -> Vec<u8> {
    write_wkb(&square())
}

fn bowtie_wkb() -> Vec<u8> {
    write_wkb(&bowtie())
}

/// SAFETY: the result must own a valid WKB buffer (success == true).
unsafe fn result_wkb(res: &GeoRepairResult) -> Vec<u8> {
    std::slice::from_raw_parts(res.wkb_data, res.wkb_len).to_vec()
}

/// SAFETY: the string result must own a valid data string (success == true).
unsafe fn string_result_data(res: &GeoRepairStringResult) -> String {
    CStr::from_ptr(res.data).to_str().unwrap().to_string()
}

fn is_valid_wkb_bytes(wkb: &[u8]) -> bool {
    read_wkb(wkb).map(|g| g.is_valid()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

#[test]
fn version_matches_crate() {
    let v = unsafe { CStr::from_ptr(geo_repair_version()) }
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(v, env!("CARGO_PKG_VERSION"));
    assert!(v.split('.').count() == 3, "semver shape: {v}");
}

// ---------------------------------------------------------------------------
// is_valid
// ---------------------------------------------------------------------------

#[test]
fn is_valid_ok_and_invalid() {
    let sq = square_wkb();
    let bt = bowtie_wkb();
    unsafe {
        assert_eq!(geo_repair_is_valid(sq.as_ptr(), sq.len()), 1);
        assert_eq!(geo_repair_is_valid(bt.as_ptr(), bt.len()), 0);
    }
}

#[test]
fn is_valid_garbage_returns_zero_without_panic() {
    let garbage = [0x01u8, 0x02, 0x03, 0x04];
    unsafe {
        assert_eq!(geo_repair_is_valid(garbage.as_ptr(), garbage.len()), 0);
        assert_eq!(geo_repair_is_valid(ptr::null(), 0), 0);
    }
}

// ---------------------------------------------------------------------------
// make_valid
// ---------------------------------------------------------------------------

#[test]
fn make_valid_repairs_bowtie() {
    let bt = bowtie_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_make_valid(bt.as_ptr(), bt.len()) };
    assert!(res.success, "repair failed: {:?}", res.error_code);
    assert_eq!(res.error_code, GeoRepairErrorCode::None);
    assert!(res.error_msg.is_null());
    let out = unsafe { result_wkb(&res) };
    assert!(!out.is_empty());
    assert!(is_valid_wkb_bytes(&out), "repaired output still invalid");
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn make_valid_preserves_valid_input() {
    let sq = square_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_make_valid(sq.as_ptr(), sq.len()) };
    assert!(res.success);
    let out = unsafe { result_wkb(&res) };
    assert!(is_valid_wkb_bytes(&out));
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn make_valid_with_config_methods() {
    let bt = bowtie_wkb();
    for (method, expected_code) in [(1u8, 0u8), (2u8, 0u8)] {
        let mut res: GeoRepairResult = unsafe {
            geo_repair_make_valid_with_config(bt.as_ptr(), bt.len(), false, method)
        };
        assert!(res.success, "method {method} failed: {:?}", res.error_code);
        let _ = expected_code;
        let out = unsafe { result_wkb(&res) };
        assert!(is_valid_wkb_bytes(&out));
        unsafe { geo_repair_free_result(&mut res) };
    }
}

#[test]
fn make_valid_null_and_garbage_are_errors() {
    let mut res: GeoRepairResult = unsafe { geo_repair_make_valid(ptr::null(), 0) };
    assert!(!res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::InvalidInput);
    assert!(!res.error_msg.is_null());
    unsafe { geo_repair_free_result(&mut res) };

    let garbage = [0x01u8, 0x02, 0x03];
    let mut res: GeoRepairResult = unsafe { geo_repair_make_valid(garbage.as_ptr(), garbage.len()) };
    assert!(!res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::Parse);
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn free_result_is_double_free_safe() {
    let bt = bowtie_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_make_valid(bt.as_ptr(), bt.len()) };
    assert!(res.success);
    unsafe {
        geo_repair_free_result(&mut res);
        geo_repair_free_result(&mut res); // second free must be a no-op
        geo_repair_free_result(ptr::null_mut()); // null must be a no-op
    }
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

#[test]
fn validate_valid_geometry() {
    let sq = square_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_validate(sq.as_ptr(), sq.len()) };
    assert!(res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::None);
    assert_eq!(res.wkb_len, 0);
    assert!(res.wkb_data.is_null());
    assert!(res.error_msg.is_null());
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn validate_invalid_geometry_reports_code_and_reasons() {
    let bt = bowtie_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_validate(bt.as_ptr(), bt.len()) };
    assert!(!res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::InvalidGeometry);
    assert!(!res.error_msg.is_null());
    let msg = unsafe { CStr::from_ptr(res.error_msg) }.to_str().unwrap();
    assert!(!msg.is_empty(), "reasons string must not be empty");
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn validate_reason_is_validate() {
    let bt = bowtie_wkb();
    let mut a: GeoRepairResult = unsafe { geo_repair_validate(bt.as_ptr(), bt.len()) };
    let mut b: GeoRepairResult = unsafe { geo_repair_validate_reason(bt.as_ptr(), bt.len()) };
    assert_eq!(a.success, b.success);
    assert_eq!(a.error_code, b.error_code);
    let msg_a = unsafe { CStr::from_ptr(a.error_msg) }.to_str().unwrap().to_string();
    let msg_b = unsafe { CStr::from_ptr(b.error_msg) }.to_str().unwrap().to_string();
    assert_eq!(msg_a, msg_b);
    unsafe {
        geo_repair_free_result(&mut a);
        geo_repair_free_result(&mut b);
    }
}

// ---------------------------------------------------------------------------
// validate_and_fix
// ---------------------------------------------------------------------------

#[test]
fn validate_and_fix_invalid_reports_reasons_and_repairs() {
    let bt = bowtie_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_validate_and_fix(bt.as_ptr(), bt.len()) };
    assert!(res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::InvalidGeometry);
    assert!(!res.error_msg.is_null(), "reasons expected after repair");
    let out = unsafe { result_wkb(&res) };
    assert!(is_valid_wkb_bytes(&out));
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn validate_and_fix_valid_passes_through() {
    let sq = square_wkb();
    let mut res: GeoRepairResult = unsafe { geo_repair_validate_and_fix(sq.as_ptr(), sq.len()) };
    assert!(res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::None);
    assert!(res.error_msg.is_null());
    let out = unsafe { result_wkb(&res) };
    assert!(is_valid_wkb_bytes(&out));
    unsafe { geo_repair_free_result(&mut res) };
}

#[test]
fn validate_and_fix_with_config() {
    let bt = bowtie_wkb();
    let mut res: GeoRepairResult = unsafe {
        geo_repair_validate_and_fix_with_config(bt.as_ptr(), bt.len(), false, 1)
    };
    assert!(res.success);
    let out = unsafe { result_wkb(&res) };
    assert!(is_valid_wkb_bytes(&out));
    unsafe { geo_repair_free_result(&mut res) };
}

// ---------------------------------------------------------------------------
// WKT surface
// ---------------------------------------------------------------------------

#[test]
fn wkt_make_valid_roundtrip() {
    let bt = CString::new("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap();
    let mut res: GeoRepairStringResult =
        unsafe { geo_repair_make_valid_wkt(bt.as_ptr()) };
    assert!(res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::None);
    let out = unsafe { string_result_data(&res) };
    assert!(!out.is_empty());
    let geom = read_wkt(&out).expect("output must be parseable WKT");
    assert!(geom.is_valid());
    unsafe { geo_repair_free_string_result(&mut res) };
}

#[test]
fn wkt_make_valid_with_config() {
    let bt = CString::new("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap();
    let mut res: GeoRepairStringResult =
        unsafe { geo_repair_make_valid_wkt_with_config(bt.as_ptr(), false, 2) };
    assert!(res.success);
    let out = unsafe { string_result_data(&res) };
    assert!(read_wkt(&out).unwrap().is_valid());
    unsafe { geo_repair_free_string_result(&mut res) };
}

#[test]
fn wkt_is_valid() {
    let sq = CString::new("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))").unwrap();
    let bt = CString::new("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap();
    unsafe {
        assert_eq!(geo_repair_is_valid_wkt(sq.as_ptr()), 1);
        assert_eq!(geo_repair_is_valid_wkt(bt.as_ptr()), 0);
        assert_eq!(geo_repair_is_valid_wkt(ptr::null()), 0);
    }
}

#[test]
fn wkt_validate() {
    let sq = CString::new("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))").unwrap();
    let bt = CString::new("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap();
    let mut valid: GeoRepairStringResult = unsafe { geo_repair_validate_wkt(sq.as_ptr()) };
    assert!(valid.success);
    assert_eq!(valid.error_code, GeoRepairErrorCode::None);
    assert!(valid.error_msg.is_null());
    unsafe { geo_repair_free_string_result(&mut valid) };

    let mut invalid: GeoRepairStringResult = unsafe { geo_repair_validate_wkt(bt.as_ptr()) };
    assert!(!invalid.success);
    assert_eq!(invalid.error_code, GeoRepairErrorCode::InvalidGeometry);
    assert!(!invalid.error_msg.is_null());
    unsafe { geo_repair_free_string_result(&mut invalid) };
}

#[test]
fn wkt_validate_and_fix() {
    let bt = CString::new("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap();
    let mut res: GeoRepairStringResult =
        unsafe { geo_repair_validate_and_fix_wkt(bt.as_ptr()) };
    assert!(res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::InvalidGeometry);
    let out = unsafe { string_result_data(&res) };
    assert!(read_wkt(&out).unwrap().is_valid());
    unsafe { geo_repair_free_string_result(&mut res) };
}

#[test]
fn wkt_parse_error_is_reported() {
    let bad = CString::new("NOTVALID").unwrap();
    let mut res: GeoRepairStringResult = unsafe { geo_repair_make_valid_wkt(bad.as_ptr()) };
    assert!(!res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::Parse);
    unsafe { geo_repair_free_string_result(&mut res) };
}

#[test]
fn free_string_result_is_double_free_safe() {
    let bt = CString::new("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))").unwrap();
    let mut res: GeoRepairStringResult = unsafe { geo_repair_make_valid_wkt(bt.as_ptr()) };
    assert!(res.success);
    unsafe {
        geo_repair_free_string_result(&mut res);
        geo_repair_free_string_result(&mut res);
        geo_repair_free_string_result(ptr::null_mut());
    }
}

// ---------------------------------------------------------------------------
// Batch
// ---------------------------------------------------------------------------

#[test]
fn batch_sequential_and_parallel_agree() {
    let sq = square_wkb();
    let bt = bowtie_wkb();
    let garbage = vec![0x01u8, 0x02, 0x03];
    let inputs = [
        GeoRepairWkbBuffer { data: sq.as_ptr(), len: sq.len() },
        GeoRepairWkbBuffer { data: bt.as_ptr(), len: bt.len() },
        GeoRepairWkbBuffer { data: garbage.as_ptr(), len: garbage.len() },
    ];
    for parallel in [0, 1] {
        let mut res = unsafe { geo_repair_make_valid_batch(inputs.as_ptr(), inputs.len(), parallel) };
        assert!(res.success, "batch failed: {:?}", res.error_code);
        assert_eq!(res.count, 3);
        assert!(!res.items.is_null());
        let items = unsafe { std::slice::from_raw_parts(res.items, res.count) };
        // item 0: valid square -> success, still valid
        assert!(items[0].success);
        let out0 = unsafe { result_wkb(&items[0]) };
        assert!(is_valid_wkb_bytes(&out0));
        // item 1: bowtie -> success, repaired valid
        assert!(items[1].success);
        let out1 = unsafe { result_wkb(&items[1]) };
        assert!(is_valid_wkb_bytes(&out1));
        // item 2: garbage -> per-item parse error, batch still succeeds
        assert!(!items[2].success);
        assert_eq!(items[2].error_code, GeoRepairErrorCode::Parse);
        unsafe { geo_repair_free_batch_result(&mut res) };
    }
}

#[test]
fn batch_null_inputs_rejected() {
    let mut res = unsafe { geo_repair_make_valid_batch(ptr::null(), 3, 0) };
    assert!(!res.success);
    assert_eq!(res.error_code, GeoRepairErrorCode::InvalidInput);
    unsafe { geo_repair_free_batch_result(&mut res) };
}

#[test]
fn batch_empty_is_ok() {
    let mut res = unsafe { geo_repair_make_valid_batch(ptr::null(), 0, 0) };
    assert!(res.success);
    assert_eq!(res.count, 0);
    assert!(res.items.is_null());
    unsafe { geo_repair_free_batch_result(&mut res) };
}

#[test]
fn free_batch_result_is_double_free_safe() {
    let sq = square_wkb();
    let inputs = [GeoRepairWkbBuffer { data: sq.as_ptr(), len: sq.len() }];
    let mut res = unsafe { geo_repair_make_valid_batch(inputs.as_ptr(), inputs.len(), 0) };
    assert!(res.success);
    unsafe {
        geo_repair_free_batch_result(&mut res);
        geo_repair_free_batch_result(&mut res);
    }
}

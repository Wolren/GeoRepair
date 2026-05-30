use std::ffi::{c_char, CString};
use std::ptr;

use crate::core::MakeValidConfig;
use crate::make_valid::MakeValid;
use crate::validation::GeoValidation;
use geo::Geometry;

#[repr(C)]
pub struct GeoRepairResult {
    pub success: bool,
    pub wkb_data: *mut u8,
    pub wkb_len: usize,
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
    let buf = unsafe { std::slice::from_raw_parts(data, len) };
    let wkb_geom = wkb::reader::read_wkb(buf).map_err(|e| format!("WKB parse error: {e}"))?;
    let geo_geom = geo_traits::to_geo::ToGeoGeometry::to_geometry(&wkb_geom);
    Ok(geo_geom)
}

fn geometry_to_wkb(geom: &Geometry<f64>) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    use wkb::writer::{geometry_wkb_size, write_geometry, WriteOptions};

    let opts = WriteOptions::default();
    let size = geometry_wkb_size(geom);
    let mut buf = vec![0u8; size];
    write_geometry(&mut Cursor::new(&mut buf[..]), geom, &opts)
        .map_err(|e| format!("WKB write error: {e}"))?;
    Ok(buf)
}

#[unsafe(no_mangle)]
pub extern "C" fn geo_repair_make_valid(wkb_data: *const u8, wkb_len: usize) -> GeoRepairResult {
    let geom = match geometry_from_wkb(wkb_data, wkb_len) {
        Ok(g) => g,
        Err(e) => return GeoRepairResult::error(&e),
    };
    let fixed = geom.make_valid();
    match geometry_to_wkb(&fixed) {
        Ok(wkb) => GeoRepairResult::success(wkb),
        Err(e) => GeoRepairResult::error(&e),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn geo_repair_make_valid_with_config(
    wkb_data: *const u8,
    wkb_len: usize,
    keep_collapsed: bool,
    poly_method: u8,
) -> GeoRepairResult {
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
    };
    let fixed = geom.make_valid_with_config(&config);
    match geometry_to_wkb(&fixed) {
        Ok(wkb) => GeoRepairResult::success(wkb),
        Err(e) => GeoRepairResult::error(&e),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn geo_repair_validate(wkb_data: *const u8, wkb_len: usize) -> GeoRepairResult {
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
}

#[unsafe(no_mangle)]
pub extern "C" fn geo_repair_free_result(result: GeoRepairResult) {
    if !result.wkb_data.is_null() {
        let _ = unsafe { Vec::from_raw_parts(result.wkb_data, result.wkb_len, result.wkb_len) };
    }
    if !result.error_msg.is_null() {
        let _ = unsafe { CString::from_raw(result.error_msg) };
    }
}

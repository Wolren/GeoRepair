//! conversion helpers: WKB/WKT to Geometry, config, error strings (extracted from bindings/ffi.rs 2026-08-07; verbatim).



use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use super::types::*;
use std::ffi::c_char;
use std::ffi::CStr;
use geo::Geometry;
use crate::core::MakeValidConfig;
use crate::io::wkt::read_wkt;
use crate::validation::GeoValidation;


pub(crate) fn geometry_from_wkb(data: *const u8, len: usize) -> Result<Geometry<f64>, GeoRepairErrorCode> {
    if data.is_null() {
        return Err(GeoRepairErrorCode::InvalidInput);
    }
    if len == 0 || len > isize::MAX as usize {
        return Err(GeoRepairErrorCode::InvalidInput);
    }
    let buf = unsafe { std::slice::from_raw_parts(data, len) };
    crate::io::wkb::read_wkb(buf).map_err(|_| GeoRepairErrorCode::Parse)
}

pub(crate) fn wkt_from_cstr(wkt: *const c_char) -> Result<Geometry<f64>, GeoRepairErrorCode> {
    if wkt.is_null() {
        return Err(GeoRepairErrorCode::InvalidInput);
    }
    // SAFETY: caller guarantees a valid NUL-terminated string.
    let text = match unsafe { CStr::from_ptr(wkt) }.to_str() {
        Ok(t) => t,
        Err(_) => return Err(GeoRepairErrorCode::Parse),
    };
    read_wkt(text).map_err(|_| GeoRepairErrorCode::Parse)
}

pub(crate) fn geometry_to_wkb(geom: &Geometry<f64>) -> Result<Vec<u8>, GeoRepairErrorCode> {
    Ok(crate::io::wkb::write_wkb(geom))
}

pub(crate) fn make_config(
    keep_collapsed: bool,
    poly_method: u8,
    fill_rule: u8,
    epsg_code: i32,
) -> MakeValidConfig {
    MakeValidConfig {
        keep_collapsed,
        poly_method: match poly_method {
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
    }
}

/// Collect all validation error messages as one `; `-joined string.
pub(crate) fn all_error_reasons(geom: &Geometry<f64>) -> String {
    geom.validate()
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

// ---------------------------------------------------------------------------
// WKB: repair
// ---------------------------------------------------------------------------

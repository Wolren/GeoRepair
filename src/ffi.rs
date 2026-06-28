//! FFI bindings for geo-repair.
//!
//! This module exposes a C-compatible API for calling geo-repair
//! functions from other languages (Python, C, Java, etc.).

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use crate::core::MakeValidError;
use geo::Geometry;

/// Parse WKB hex string and repair the geometry.
/// Returns a WKB hex string of the repaired geometry.
/// Caller must free the returned string with `geo_repair_free_string`.
#[no_mangle]
pub extern "C" fn geo_repair_make_valid(wkb_hex: *const c_char) -> *mut c_char {
    let result = (|| -> Result<String, MakeValidError> {
        let input = unsafe {
            if wkb_hex.is_null() {
                return Err(MakeValidError::ParseError("null pointer".into()));
            }
            CStr::from_ptr(wkb_hex).to_str()?.to_string()
        };

        let geom = wkbe::decode_hex(&input)?;
        let valid = crate::make_valid::MakeValid::make_valid(&geom);

        let wkb_bytes = wkbe::encode(&valid)?;
        Ok(hex::encode(wkb_bytes))
    })();

    match result {
        Ok(s) => CString::new(s).unwrap_or_default().into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string returned by `geo_repair_make_valid`.
#[no_mangle]
pub extern "C" fn geo_repair_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)); }
    }
}

mod wkbe {
    use wkb::geom_type::GeometryType;
    use wkb::reader::WkbReader;
    use wkb::writer::WkbWriter;
    use wkb::Endianness;

    pub fn decode_hex(hex: &str) -> Result<geo::Geometry<f64>, super::MakeValidError> {
        let bytes = hex::decode(hex)
            .map_err(|e| super::MakeValidError::ParseError(format!("hex decode: {e}")))?;
        let mut reader = WkbReader::new(&bytes);
        let geom = reader.read()?;
        Ok(geom)
    }

    pub fn encode(geom: &geo::Geometry<f64>) -> Result<Vec<u8>, super::MakeValidError> {
        let mut buf = Vec::new();
        let mut writer = WkbWriter::new(&mut buf, Endianness::LittleEndian);
        writer.write_geometry(geom)?;
        drop(writer);
        Ok(buf)
    }
}

/// Helper: convert CStr to_str error
impl From<std::str::Utf8Error> for MakeValidError {
    fn from(e: std::str::Utf8Error) -> Self {
        MakeValidError::ParseError(format!("UTF-8 error: {e}"))
    }
}

/// Helper: WKB reading error
impl From<wkb::Error> for MakeValidError {
    fn from(e: wkb::Error) -> Self {
        MakeValidError::ParseError(format!("WKB error: {e}"))
    }
}

use std::fmt;
// hex encoding/decoding for FFI (avoids pulling in hex crate)
mod hex {
    const CHARS: &[u8] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push(CHARS[(b >> 4) as usize] as char);
            out.push(CHARS[(b & 0x0f) as usize] as char);
        }
        out
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, super::HexError> {
        if s.len() % 2 != 0 {
            return Err(super::HexError("odd length"));
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                let hi = from_hex(s.as_bytes()[i])?;
                let lo = from_hex(s.as_bytes()[i + 1])?;
                Ok((hi << 4) | lo)
            })
            .collect()
    }

    fn from_hex(c: u8) -> Result<u8, super::HexError> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => Err(super::HexError("invalid hex digit")),
        }
    }
}

#[derive(Debug)]
pub struct HexError(pub &'static str);

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hex error: {}", self.0)
    }
}

impl std::error::Error for HexError {}

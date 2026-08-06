//! WebAssembly browser support (`wasm` feature, wasm32 targets only).
//!
//! The core library is pure Rust and works in any wasm runtime as-is
//! (validation, repair, WKT/WKB I/O, no_std). This module adds the one
//! thing only a browser provides: fetching geometry data over HTTP.
//!
//! ```ignore
//! // browser (wasm32-unknown-unknown)
//! let g = geo_repair::wasm::fetch_geometry("https://example.com/polygon.wkb")?;
//! ```
//!
//! Fetching is SYNCHRONOUS (blocking XMLHttpRequest), matching the
//! documented `wasm` feature contract. The function returns `Err` when
//! `XMLHttpRequest` is unavailable (e.g. the Node wasm-bindgen-test
//! harness, which has no browser globals).

use geo::Geometry;

/// Fetch a URL as raw bytes with a synchronous XMLHttpRequest.
///
/// Binary-safe: the response is read with `responseType = "arraybuffer"`,
/// so both text (UTF-8 bytes) and binary (WKB/GPKG) payloads arrive
/// unmodified.
pub fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let _window = web_sys::window().ok_or("fetch_bytes: no window (not in a browser)")?;
    let xhr =
        web_sys::XmlHttpRequest::new().map_err(|e| format!("fetch_bytes: XHR create: {e:?}"))?;
    xhr.set_response_type(web_sys::XmlHttpRequestResponseType::Arraybuffer);
    xhr.open_with_async("GET", url, false)
        .map_err(|e| format!("fetch_bytes: XHR open: {e:?}"))?;
    xhr.send().map_err(|e| format!("fetch_bytes: XHR send: {e:?}"))?;
    let status = xhr
        .status()
        .map_err(|e| format!("fetch_bytes: XHR status: {e:?}"))?;
    if status != 200 {
        return Err(format!("fetch_bytes: HTTP {status} fetching {url}"));
    }
    let body = xhr
        .response()
        .map_err(|e| format!("fetch_bytes: XHR response: {e:?}"))?;
    if body.is_array() {
        Ok(js_sys::Uint8Array::new(&body).to_vec())
    } else {
        // Response is not an ArrayBuffer (e.g. empty body) - try text.
        xhr.response_text()
            .ok()
            .flatten()
            .map(|t| t.into_bytes())
            .ok_or_else(|| "fetch_bytes: empty response".to_string())
    }
}

/// Fetch a geometry from a URL, sniffing the wire format.
///
/// A leading WKB endian byte (`0x00` big- or `0x01` little-endian) parses
/// as WKB via `io::wkb::read_wkb`; anything else is parsed as WKT via
/// `io::wkt::read_wkt`. This covers the two text/binary formats the
/// crate writes natively, so any file produced by `io::save` round-trips
/// through this function.
pub fn fetch_geometry(url: &str) -> Result<Geometry<f64>, String> {
    let bytes = fetch_bytes(url)?;
    if bytes.len() >= 1 && (bytes[0] == 0x00 || bytes[0] == 0x01) {
        crate::io::wkb::read_wkb(&bytes).map_err(|e| format!("fetch_geometry: WKB parse: {e}"))
    } else {
        let text = std::str::from_utf8(&bytes).map_err(|e| format!("fetch_geometry: not UTF-8: {e}"))?;
        crate::io::wkt::read_wkt(text).map_err(|e| format!("fetch_geometry: WKT parse: {e}"))
    }
}

//! ```ignore
//! // browser (wasm32-unknown-unknown)
//! let g = geo_repair::wasm::fetch_geometry("https://example.com/polygon.wkb")?;
//! ```
//! The function returns `Err` when `XMLHttpRequest` is unavailable (e.g. the Node
//! wasm-bindgen-test harness, which has no browser globals).

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use geo::Geometry;

/// Fetch a URL as raw bytes with a synchronous XMLHttpRequest.
pub fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let _window = web_sys::window().ok_or("fetch_bytes: no window (not in a browser)")?;
    let xhr =
        web_sys::XmlHttpRequest::new().map_err(|e| format!("fetch_bytes: XHR create: {e:?}"))?;
    xhr.set_response_type(web_sys::XmlHttpRequestResponseType::Arraybuffer);
    xhr.open_with_async("GET", url, false)
        .map_err(|e| format!("fetch_bytes: XHR open: {e:?}"))?;
    xhr.send()
        .map_err(|e| format!("fetch_bytes: XHR send: {e:?}"))?;
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
        xhr.response_text()
            .ok()
            .flatten()
            .map(|t| t.into_bytes())
            .ok_or_else(|| "fetch_bytes: empty response".to_string())
    }
}

pub fn fetch_geometry(url: &str) -> Result<Geometry<f64>, String> {
    let bytes = fetch_bytes(url)?;
    if bytes.len() >= 1 && (bytes[0] == 0x00 || bytes[0] == 0x01) {
        crate::io::wkb::read_wkb(&bytes).map_err(|e| format!("fetch_geometry: WKB parse: {e}"))
    } else {
        let text =
            std::str::from_utf8(&bytes).map_err(|e| format!("fetch_geometry: not UTF-8: {e}"))?;
        crate::io::wkt::read_wkt(text).map_err(|e| format!("fetch_geometry: WKT parse: {e}"))
    }
}

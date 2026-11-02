//! Emits this crate's WKT and WKB results as JSON lines, for `conformance/run.py`.
//!
//! Usage: `conformance_dump <corpus.tsv>` for WKT cases, `conformance_dump --wkb <cases.tsv>`
//! for hex WKB cases. The corpus is `id<TAB>text` per line, blank lines and `#` are skipped.
//!
//! A panic in one case is recorded as `"panic":"..."` and the run continues, because a
//! library panic on one geometry must not hide the other cases.

use std::io::{self, BufRead, Write};
use std::panic;
use std::sync::Mutex;

use geo_repair::io::wkb::{
    EwkbDims, EwkbGeometry, read_ewkb, write_ewkb, write_wkb, write_wkb_iso,
};
use geo_repair::io::wkt::{read_wkt, read_wkt_zm, write_wkt_zm};

static LAST_PANIC: Mutex<String> = Mutex::new(String::new());

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn dims_tag(d: EwkbDims) -> String {
    let mut s = String::new();
    if d.has_z {
        s.push('Z');
    }
    if d.has_m {
        s.push('M');
    }
    s
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

fn wkt_case(text: &str) -> String {
    let (plain_ok, plain_err, wkb_hex) = match read_wkt(text) {
        Ok(g) => (true, String::new(), hex(&write_wkb(&g))),
        Err(e) => (false, e.to_string(), String::new()),
    };

    let (zm_ok, zm_err, dims, wkt_out, ewkb_hex, iso_hex, ewkb_err) = match read_wkt_zm(text) {
        Ok(w) => {
            let d = w.dims;
            let text_out = write_wkt_zm(&w);
            let ewkb_dims = EwkbDims {
                has_z: d.has_z,
                has_m: d.has_m,
            };
            {
                let g = EwkbGeometry {
                    geometry: w.geometry,
                    srid: None,
                    dims: ewkb_dims,
                    extra_coords: w.extra_coords,
                };
                (
                    true,
                    String::new(),
                    dims_tag(ewkb_dims),
                    text_out,
                    hex(&write_ewkb(&g)),
                    hex(&write_wkb_iso(&g)),
                    String::new(),
                )
            }
        }
        Err(e) => (
            false,
            e.to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
    };

    format!(
        "\"plain_ok\":{plain_ok},\"plain_err\":\"{}\",\"zm_ok\":{zm_ok},\"zm_err\":\"{}\",\"dims\":\"{dims}\",\"wkt_out\":\"{}\",\"wkb\":\"{wkb_hex}\",\"ewkb\":\"{ewkb_hex}\",\"isowkb\":\"{iso_hex}\",\"ewkb_err\":\"{}\"",
        esc(&plain_err),
        esc(&zm_err),
        esc(&wkt_out),
        esc(&ewkb_err)
    )
}

fn wkb_case(text: &str) -> String {
    let (ok, err, dims, recoded, srid) = match from_hex(text).map(|b| read_ewkb(&b)) {
        Some(Ok(g)) => {
            let tag = dims_tag(g.dims);
            let srid = g.srid;
            let g = EwkbGeometry {
                geometry: g.geometry,
                srid: g.srid,
                dims: g.dims,
                extra_coords: g.extra_coords,
            };
            let back = write_wkb_iso(&g);
            (
                true,
                String::new(),
                tag,
                hex(&back),
                srid.map(|v| v.to_string()).unwrap_or_default(),
            )
        }
        Some(Err(e)) => (
            false,
            e.to_string(),
            String::new(),
            String::new(),
            String::new(),
        ),
        None => (
            false,
            "bad hex".to_string(),
            String::new(),
            String::new(),
            String::new(),
        ),
    };
    format!(
        "\"ok\":{ok},\"err\":\"{}\",\"dims\":\"{dims}\",\"recoded\":\"{recoded}\",\"srid\":\"{srid}\"",
        esc(&err)
    )
}

fn main() {
    panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        if let Ok(mut slot) = LAST_PANIC.lock() {
            *slot = format!("{msg} at {loc}");
        }
    }));

    let mut args = std::env::args().skip(1);
    let first = args.next().unwrap_or_default();
    let wkb_mode = first == "--wkb";
    let path = if wkb_mode {
        args.next().unwrap_or_default()
    } else {
        first
    };
    if path.is_empty() {
        eprintln!("usage: conformance_dump [--wkb] <corpus.tsv>");
        std::process::exit(2);
    }
    let file = std::fs::File::open(&path).expect("open corpus");
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for line in io::BufReader::new(file).lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (id, raw) = match line.split_once('\t') {
            Some(pair) => pair,
            None => continue,
        };
        // The corpus is line based and tab separated, so whitespace inside a case arrives
        // as a marker.
        let text = raw
            .replace("|NL|", "\n")
            .replace("|TAB|", "\t")
            .replace("|CR|", "\r");
        let text = text.as_str();

        if let Ok(mut slot) = LAST_PANIC.lock() {
            slot.clear();
        }
        let body = panic::catch_unwind(|| {
            if wkb_mode {
                wkb_case(text)
            } else {
                wkt_case(text)
            }
        });
        match body {
            Ok(body) => {
                writeln!(out, "{{\"id\":\"{}\",{body}}}", esc(id)).unwrap();
            }
            Err(_) => {
                let msg = LAST_PANIC
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|_| "panic".to_string());
                writeln!(
                    out,
                    "{{\"id\":\"{}\",\"panic\":\"{}\"}}",
                    esc(id),
                    esc(&msg)
                )
                .unwrap();
            }
        }
    }
}

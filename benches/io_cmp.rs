//! IO head-to-head benchmark: GeoRepair WKT/WKB vs GEOS (geos_c) and GDAL (OGR).
//!
//! Measures read+write for both formats on the same polygon dataset.
//!
//! Run (system GEOS + conda GDAL on Windows):
//!   export PATH="/d/Miniconda/Library/bin:/d/Miniconda/envs/bench-gdal/Library/bin:$PATH"
//!   export GEOS_LIB_DIR='D:\Miniconda\Library\lib'
//!   export GEOS_INCLUDE_DIR='D:\Miniconda\Library\include'
//!   export GEOS_VERSION=3.14.1
//!   export GDAL_LIB_DIR='D:\Miniconda\envs\bench-gdal\Library\lib'
//!   export GDAL_INCLUDE_DIR='D:\Miniconda\envs\bench-gdal\Library\include'
//!   cargo bench --features bench-geos-system,bench-gdal-system --bench io_cmp
//!
//! BENCH_FILE=<path.bin> overrides the dataset (full 748MB transcode);
//! default is benches/real_world/data_0.bin, then alaska.bin, then synthetic.

#[cfg(any(feature = "bench-geos-system", feature = "bench-gdal-system"))]
use std::ffi::CString;
use std::hint::black_box;
use std::time::Instant;

use geo::Geometry;
use geo_repair::io::load_bin;
use geo_repair::{read_wkb_concat, read_wkt, write_wkb, write_wkt};

const DATASET: &str = "benches/real_world/data_0.bin";

// Kept dead on purpose: its presence pins the geo-types resolution for this
// bench target (see benches/io_cmp.rs header). Do not remove without
// re-verifying that `cargo bench` still compiles under both features.
#[allow(dead_code)]
fn load_geoms() -> Vec<Geometry<f64>> {
    if let Ok(path) = std::env::var("BENCH_FILE") {
        let polys = load_bin(&path).expect("load BENCH_FILE");
        let geoms: Vec<Geometry<f64>> = polys.into_iter().map(Geometry::Polygon).collect();
        return geoms;
    }
    if let Ok(polys) = load_bin(DATASET) {
        let geoms: Vec<Geometry<f64>> = polys.into_iter().map(Geometry::Polygon).collect();
        return geoms;
    }
    if let Ok(polys) = load_bin("benches/real_world/alaska.bin") {
        let geoms: Vec<Geometry<f64>> = polys.into_iter().map(Geometry::Polygon).collect();
        return geoms;
    }
    eprintln!("io_cmp: no dataset found (BENCH_FILE, data_0.bin, alaska.bin)");
    std::process::exit(1);
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed().as_secs_f64())
}

fn fmt_us(secs: f64, n: usize) -> String {
    let us = secs * 1e6 / n as f64;
    if us >= 1000.0 {
        format!("{:.2} ms", us / 1000.0)
    } else {
        format!("{:.2} us", us)
    }
}

fn main() {
    let src = std::env::var("BENCH_FILE").unwrap_or_else(|_| DATASET.into());
    let cap: usize = std::env::var("BENCH_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40_000);
    let mut polys = load_bin(&src).expect("load dataset");
    polys.truncate(cap);
    let geoms: Vec<Geometry<f64>> = polys.into_iter().map(Geometry::Polygon).collect();
    let n = geoms.len();
    eprintln!("[io_cmp] {n} geometries");

    // ---- Ours: writes (timed), then shared inputs ----
    let (wkt, t_wktw) = timed(|| geoms.iter().map(write_wkt).collect::<Vec<_>>());
    let (wkb_blobs, t_wkbw) = timed(|| geoms.iter().map(write_wkb).collect::<Vec<_>>());
    let wkt_bytes: usize = wkt.iter().map(|s| s.len()).sum();
    let wkb_bytes: usize = wkb_blobs.iter().map(|b| b.len()).sum();

    // Ours: reads
    let (ok_a, t_wktr) = timed(|| {
        let mut acc = 0usize;
        for s in &wkt {
            black_box(read_wkt(s).unwrap());
            acc += 1;
        }
        acc
    });
    let mut concat = Vec::new();
    for b in &wkb_blobs {
        concat.extend_from_slice(b);
    }
    let (ok_b, t_wkbr) = timed(|| black_box(read_wkb_concat(&concat).unwrap().len()));
    let (_, t_wkbr2) = timed(|| {
        let mut acc = 0usize;
        for b in &wkb_blobs {
            black_box(read_wkb_concat(b).unwrap());
            acc += 1;
        }
        acc
    });

    // Shared C-string lines for GEOS/GDAL (outside timed regions)
    #[cfg(any(feature = "bench-geos-system", feature = "bench-gdal-system"))]
    let wkt_lines: Vec<CString> = wkt
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();

    eprintln!("-- ours ------------------------------------------");
    eprintln!("write WKT   : {}  ({wkt_bytes} B)", fmt_us(t_wktw, n));
    eprintln!("write WKB   : {}  ({wkb_bytes} B)", fmt_us(t_wkbw, n));
    eprintln!("read  WKT   : {}  (coords {ok_a})", fmt_us(t_wktr, n));
    eprintln!("read  WKB   : {}  (n {ok_b} concat)", fmt_us(t_wkbr, n));
    eprintln!("read  WKB   : {}  (per blob)", fmt_us(t_wkbr2, n));

    // ---- GEOS (geos_c, system) ----
    #[cfg(feature = "bench-geos-system")]
    {
        use geos_ffi::*;
        use std::ffi::c_void;
        unsafe {
            let h = GEOS_init_r();
            let rd = GEOSWKBReader_create_r(h);
            let mut gg: Vec<*mut c_void> = Vec::with_capacity(n);
            let (_, t_r) = timed(|| {
                for b in &wkb_blobs {
                    let g = GEOSWKBReader_read_r(h, rd, b.as_ptr(), b.len());
                    assert!(!g.is_null());
                    gg.push(g);
                }
            });
            let ver = std::env::var("GEOS_VERSION").unwrap_or_else(|_| "?".into());
            eprintln!("-- geos (system {ver}) --");
            eprintln!("read  WKB   : {}  (per blob)", fmt_us(t_r, n));
            let wr = GEOSWKTWriter_create_r(h);
            let (_, t_r) = timed(|| {
                for g in &gg {
                    let s = GEOSWKTWriter_write_r(h, wr, *g);
                    assert!(!s.is_null());
                    GEOSFree_r(h, s.cast());
                }
            });
            eprintln!("write WKT   : {}", fmt_us(t_r, n));
            let rd2 = GEOSWKTReader_create_r(h);
            let (_, t_r) = timed(|| {
                for line in &wkt_lines {
                    let g = GEOSWKTReader_read_r(h, rd2, line.as_ptr());
                    assert!(!g.is_null());
                    GEOSGeom_destroy_r(h, g);
                }
            });
            eprintln!("read  WKT   : {}", fmt_us(t_r, n));
            let wb = GEOSWKBWriter_create_r(h);
            GEOSWKBWriter_setByteOrder_r(h, wb, 1); // GEOS_WKB_NDR = 1
            let (_, t_r) = timed(|| {
                for g in &gg {
                    let mut sz: usize = 0;
                    let buf = GEOSWKBWriter_write_r(h, wb, *g, &mut sz);
                    assert!(!buf.is_null());
                    GEOSFree_r(h, buf.cast());
                }
            });
            eprintln!("write WKB   : {}", fmt_us(t_r, n));
            GEOSWKBWriter_destroy_r(h, wb);
            GEOSWKTWriter_destroy_r(h, wr);
            GEOSWKTReader_destroy_r(h, rd2);
            GEOSWKBReader_destroy_r(h, rd);
            for g in gg {
                GEOSGeom_destroy_r(h, g);
            }
            GEOS_finish_r(h);
        }
    }

    // ---- GDAL (OGR) ----
    #[cfg(feature = "bench-gdal-system")]
    {
        use gdal_ffi::*;
        use std::ffi::{c_char, c_int, c_void};
        unsafe {
            let mut gr: Vec<*mut c_void> = Vec::with_capacity(n);
            let (_, t_r) = timed(|| {
                for b in &wkb_blobs {
                    let mut g: *mut c_void = std::ptr::null_mut();
                    let r = OGR_G_CreateFromWkb(
                        b.as_ptr(),
                        std::ptr::null_mut(),
                        &mut g,
                        b.len() as c_int,
                    );
                    assert_eq!(r, 0);
                    gr.push(g);
                }
            });
            eprintln!("-- gdal (ogr) -------------");
            eprintln!("read  WKB   : {}  (per blob)", fmt_us(t_r, n));
            let mut outs: Vec<*mut c_char> = Vec::with_capacity(n);
            let (_, t_r) = timed(|| {
                for g in &gr {
                    let mut out: *mut c_char = std::ptr::null_mut();
                    let r = OGR_G_ExportToWkt(*g, &mut out);
                    assert_eq!(r, 0);
                    outs.push(out);
                }
            });
            let bytes: usize = outs
                .iter()
                .map(|o| {
                    let mut len = 0usize;
                    unsafe {
                        while *o.add(len) != 0 {
                            len += 1;
                        }
                    }
                    len
                })
                .sum();
            for o in outs {
                unsafe { VSIFree(o.cast()) };
            }
            eprintln!("write WKT   : {}  ({bytes} B)", fmt_us(t_r, n));
            let (_, t_r) = timed(|| {
                for line in &wkt_lines {
                    let mut s: *mut c_char = line.as_ptr() as *mut c_char;
                    let mut g: *mut c_void = std::ptr::null_mut();
                    let r = OGR_G_CreateFromWkt(&mut s, std::ptr::null_mut(), &mut g);
                    assert_eq!(r, 0);
                    OGR_G_DestroyGeometry(g);
                }
            });
            eprintln!("read  WKT   : {}", fmt_us(t_r, n));
            let mut sizes: Vec<usize> = Vec::with_capacity(n);
            let (_, t_r) = timed(|| {
                for g in &gr {
                    let sz = OGR_G_WkbSize(*g) as usize;
                    let mut buf = vec![0u8; sz];
                    let r = OGR_G_ExportToWkb(*g, 1, buf.as_mut_ptr()); // OGR_WKB_NDR
                    assert_eq!(r, 0); // OGRErr: 0 = success
                    sizes.push(sz);
                }
            });
            eprintln!(
                "write WKB   : {}  ({} B)",
                fmt_us(t_r, n),
                sizes.iter().sum::<usize>()
            );
            for g in gr {
                OGR_G_DestroyGeometry(g);
            }
        }
    }
}

// ---- Raw geos_c bindings (C API, same DLL the geos crate links) ----
#[cfg(feature = "bench-geos-system")]
mod geos_ffi {
    use std::ffi::{c_char, c_int, c_void};
    #[link(name = "geos_c")]
    unsafe extern "C" {
        pub fn GEOS_init_r() -> *mut c_void;
        pub fn GEOS_finish_r(h: *mut c_void);
        pub fn GEOSWKTReader_create_r(h: *mut c_void) -> *mut c_void;
        pub fn GEOSWKTReader_destroy_r(h: *mut c_void, r: *mut c_void);
        pub fn GEOSWKTReader_read_r(
            h: *mut c_void,
            r: *mut c_void,
            w: *const c_char,
        ) -> *mut c_void;
        pub fn GEOSWKBReader_create_r(h: *mut c_void) -> *mut c_void;
        pub fn GEOSWKBReader_destroy_r(h: *mut c_void, r: *mut c_void);
        pub fn GEOSWKBReader_read_r(
            h: *mut c_void,
            r: *mut c_void,
            w: *const u8,
            s: usize,
        ) -> *mut c_void;
        pub fn GEOSWKTWriter_create_r(h: *mut c_void) -> *mut c_void;
        pub fn GEOSWKTWriter_destroy_r(h: *mut c_void, w: *mut c_void);
        pub fn GEOSWKTWriter_write_r(h: *mut c_void, w: *mut c_void, g: *mut c_void)
        -> *mut c_char;
        pub fn GEOSWKBWriter_create_r(h: *mut c_void) -> *mut c_void;
        pub fn GEOSWKBWriter_destroy_r(h: *mut c_void, w: *mut c_void);
        pub fn GEOSWKBWriter_setByteOrder_r(h: *mut c_void, w: *mut c_void, o: c_int);
        pub fn GEOSWKBWriter_write_r(
            h: *mut c_void,
            w: *mut c_void,
            g: *mut c_void,
            size: *mut usize,
        ) -> *mut u8;
        pub fn GEOSGeom_destroy_r(h: *mut c_void, g: *mut c_void);
        pub fn GEOSFree_r(h: *mut c_void, p: *mut c_void);
    }
}

// ---- Raw OGR C API bindings (gdal.dll) ----
#[cfg(feature = "bench-gdal-system")]
mod gdal_ffi {
    use std::ffi::{c_char, c_int, c_void};
    #[link(name = "gdal")]
    unsafe extern "C" {
        pub fn OGR_G_CreateFromWkt(
            pp: *mut *mut c_char,
            srs: *mut c_void,
            g: *mut *mut c_void,
        ) -> c_int;
        pub fn OGR_G_CreateFromWkb(
            d: *const u8,
            srs: *mut c_void,
            g: *mut *mut c_void,
            size: c_int,
        ) -> c_int;
        pub fn OGR_G_ExportToWkt(g: *mut c_void, out: *mut *mut c_char) -> c_int;
        pub fn OGR_G_WkbSize(g: *mut c_void) -> c_int;
        pub fn OGR_G_ExportToWkb(g: *mut c_void, order: c_int, buf: *mut u8) -> c_int;
        pub fn OGR_G_DestroyGeometry(g: *mut c_void);
        pub fn VSIFree(p: *mut c_void);
    }
}

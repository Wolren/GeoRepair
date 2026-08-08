//! Regression: count fields are bounded against the remaining buffer
//! before any allocation (read_bounded_count, src/io/wkb/read.rs).
//! A crafted document whose count claims more elements than the buffer
//! can possibly hold must return UnexpectedEof - never panic, and never
//! drive Vec::with_capacity into a multi-GB allocation (measured
//! 2026-08-04: crafted MultiPoint count -> 120 GB allocation attempt).
//!
//! These documents are 9-13 bytes: header + one huge u32 count, nothing
//! else. Before the guard, `n` flowed straight into with_capacity(n).

use geo_repair::io::wkb::{WkbError, read_wkb};

fn expect_eof(buf: &[u8], what: &str) {
    match read_wkb(buf) {
        Err(WkbError::UnexpectedEof) => {}
        other => panic!("{what}: expected UnexpectedEof, got {other:?}"),
    }
}

#[test]
fn huge_counts_are_bounded() {
    let huge = 0x7FFF_FFFFu32.to_le_bytes();

    // MultiPoint: count = 0x7FFFFFFF, zero coordinate bytes follow.
    let mut mp = vec![0x01, 0x01, 0x00, 0x00, 0x00];
    mp.extend_from_slice(&huge);
    expect_eof(&mp, "MultiPoint count");

    // LineString: count = huge.
    let mut ls = vec![0x01, 0x02, 0x00, 0x00, 0x00];
    ls.extend_from_slice(&huge);
    expect_eof(&ls, "LineString count");

    // Polygon: ring count = huge.
    let mut poly_rings = vec![0x01, 0x03, 0x00, 0x00, 0x00];
    poly_rings.extend_from_slice(&huge);
    expect_eof(&poly_rings, "Polygon ring count");

    // Polygon: 1 ring, coord count = huge.
    let mut poly_coords = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    poly_coords.extend_from_slice(&huge);
    expect_eof(&poly_coords, "Polygon ring coord count");

    // MultiLineString: component count = huge.
    let mut mls = vec![0x01, 0x05, 0x00, 0x00, 0x00];
    mls.extend_from_slice(&huge);
    expect_eof(&mls, "MultiLineString count");

    // MultiPolygon: component count = huge.
    let mut mpoly = vec![0x01, 0x06, 0x00, 0x00, 0x00];
    mpoly.extend_from_slice(&huge);
    expect_eof(&mpoly, "MultiPolygon count");

    // GeometryCollection: member count = huge.
    let mut gc = vec![0x01, 0x07, 0x00, 0x00, 0x00];
    gc.extend_from_slice(&huge);
    expect_eof(&gc, "GeometryCollection count");

    // LineString count = exactly remaining/16 (the bound limit): must
    // PARSE. The bound must not reject documents that fit: 2 coords
    // need 32 bytes, remaining after the count IS 32.
    let mut fits: Vec<u8> = vec![0x01, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    for v in [0u64, 1, 2, 3] {
        fits.extend_from_slice(&v.to_le_bytes());
    }
    assert!(
        read_wkb(&fits).is_ok(),
        "a count that exactly fits the buffer must parse"
    );
}

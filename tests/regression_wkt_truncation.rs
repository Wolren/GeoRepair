use geo_repair::io::wkb::read_wkb;
use geo_repair::io::wkt::{read_wkt, write_wkt};

// Fuzz-discovered (wkt_repair crash-5909cb82, CI 2026-08-04): truncated
// documents ending mid-polygon panicked with index-out-of-bounds at
// src/io/wkt/read.rs:286 (unchecked `self.s[self.i]` after the guard
// failed to break). The parser must return Err on truncation, never
// panic. All four unchecked-index sites are covered here.
#[test]
fn truncated_wkt_never_panics() {
    let cases = [
        // Rings loop: input ends after the first ring, no closing paren.
        "POLYGON ((0 0, 1 1, 1 0",
        "POLYGON ((0 0, 1 1",
        "POLYGON ((0 0",
        // MultiPolygon rings loop (second unchecked site).
        "MULTIPOLYGON (((0 0, 1 1, 1 0",
        "MULTIPOLYGON (((0 0, 1 1, 1 0)), ((0 0, 1 1",
        // parse_point peek on empty input / mid-token.
        "POINT",
        "POINT ",
        "POLYGON (EMPTY",
        "POLYGON (",
        "MULTIPOLYGON (EMPTY",
        "MULTIPOLYGON (((0 0, 1 1, 1 0)),",
        // Bare token endings.
        "GEOMETRYCOLLECTION (",
        "LINESTRING (",
        "",
        "POLYGON",
        // Comma-then-EOF inside element loops (i advances past len, then
        // the EMPTY check indexes): fixed 2026-08-04.
        "GEOMETRYCOLLECTION (POINT (0 0),",
        "MULTILINESTRING ((0 0, 1 1),",
        "MULTILINESTRING ((0 0, 1 1, 0 0), (1 1, 2 2, 1 1),",
        "MULTIPOLYGON (((0 0, 1 1, 0 0)),",
        "MULTIPOLYGON (((0 0, 1 1, 0 0), (1 1, 2 2, 1 1),",
    ];
    for text in cases {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(text)));
        assert!(r.is_ok(), "read_wkt panicked on {text:?}");
        if let Ok(Ok(g)) = r {
            // Whatever parsed must not be a NaN-free polygon that then
            // crashes downstream; the parse result itself is what matters.
            let _ = g;
        }
    }
    // The fuzz crash input itself (artifact crash-5909cb82): a polygon
    // with a NaN coordinate and a trailing comma. Must parse or reject
    // cleanly, never panic.
    let crash_text = "POLYGON((0 0,nan 0,10 10,0 10,0 0,)";
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(crash_text)));
    assert!(r.is_ok(), "read_wkt panicked on the fuzz crash input");
}

// Ring order must survive parse -> write -> parse: two distinct holes
// keep their order (swap_remove(0) in the polygon assembly previously
// swapped the LAST ring into the first hole slot, scrambling any 2+ hole
// polygon - found 2026-08-04 by the roundtrip probe), and empty rings
// are preserved (GEOS keeps empty shells).
#[test]
fn wkt_ring_order_and_empty_rings() {
    // Two distinct holes: their order is meaningful data.
    let text = "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (1 1, 1 2, 2 2, 2 1, 1 1), (5 5, 5 7, 7 7, 7 5, 5 5))";
    {
        let g = read_wkt(text).expect("parse");
        let g2 = read_wkt(&write_wkt(&g)).expect("reparse");
        assert_eq!(
            format!("{:?}", g),
            format!("{:?}", g2),
            "ring order lost for {text}"
        );
    }
    // Empty shell + holes: structure must survive (exterior empty, holes
    // in order, empty holes preserved).
    let empty_shell = "POLYGON ((EMPTY), EMPTY, (4.243991582e-314 6.3659874475e-314), EMPTY)";
    let g = read_wkt(empty_shell).expect("parse empty-shell");
    let g2 = read_wkt(&write_wkt(&g)).expect("reparse empty-shell");
    assert_eq!(
        format!("{:?}", g),
        format!("{:?}", g2),
        "empty-shell structure lost"
    );
}

// Recursion depth: both readers recurse per container nesting level. A
// crafted document nesting beyond the limit must return Err, not overflow
// the stack (an uncatchable abort). Build 2000-level nesting, which is
// ~10x the cap.
#[test]
fn readers_bound_nesting_depth() {
    let wkt_depth = 2000;
    let mut wkt = String::from("GEOMETRYCOLLECTION (");
    for _ in 0..wkt_depth {
        wkt.push_str("GEOMETRYCOLLECTION (");
    }
    wkt.push_str("POINT (0 0)");
    for _ in 0..wkt_depth {
        wkt.push(')');
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(&wkt)));
    assert!(r.is_ok(), "read_wkt panicked on deep nesting");
    let res = r.unwrap();
    assert!(
        res.is_err(),
        "deep WKT nesting must be rejected, got {res:?}"
    );

    // WKB: nested GeometryCollection, each level = byte order + type
    // (GC = 7) + count 1 = 9 bytes.
    let wkb_depth = 2000;
    let mut wkb = Vec::with_capacity(9 * wkb_depth + 21);
    for _ in 0..wkb_depth {
        wkb.extend_from_slice(&[1u8, 7, 0, 0, 0, 1, 0, 0, 0]);
    }
    wkb.extend_from_slice(&[
        1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(&wkb)));
    assert!(r.is_ok(), "read_wkb panicked on deep nesting");
    let res = r.unwrap();
    assert!(
        res.is_err(),
        "deep WKB nesting must be rejected, got {res:?}"
    );
}

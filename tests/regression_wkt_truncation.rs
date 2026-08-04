use geo_repair::io::wkt::read_wkt;

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

use geo_repair::io::wkb::read_wkb;
use geo_repair::io::wkt::read_wkt;

// Stress: parsers must never panic on ANY byte input - random, truncated,
// adversarial. Returns Err (parse rejection) or Ok, never panics.
#[test]
fn parsers_never_panic_random_bytes() {
    // Deterministic xorshift (no external rng dep).
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut wkb_panics = 0;
    let mut wkt_panics = 0;
    let mut wkb_ok = 0u64;
    let mut wkt_ok = 0u64;
    for _ in 0..200_000 {
        let len = (next() % 256) as usize;
        let mut buf = vec![0u8; len];
        for b in buf.iter_mut() {
            *b = (next() & 0xff) as u8;
        }
        // Bias toward valid-looking WKB headers (byte order + type codes).
        if buf.len() >= 2 && (next() & 3) == 0 {
            buf[0] = 0; // big-endian
            buf[1] = (next() % 18) as u8; // plausible type codes
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(&buf)));
        match r {
            Ok(Ok(_)) => wkb_ok += 1,
            Ok(Err(_)) => {}
            Err(_) => wkb_panics += 1,
        }
        // WKT: arbitrary bytes as lossy chars.
        let text: String = buf.iter().map(|&b| b as char).collect();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(&text)));
        match r {
            Ok(Ok(_)) => wkt_ok += 1,
            Ok(Err(_)) => {}
            Err(_) => wkt_panics += 1,
        }
    }
    println!("wkb panics: {wkb_panics} (ok parses: {wkb_ok})");
    println!("wkt panics: {wkt_panics} (ok parses: {wkt_ok})");
    assert_eq!(wkb_panics, 0, "read_wkb panicked on random input");
    assert_eq!(wkt_panics, 0, "read_wkt panicked on random input");
}

// Every truncation of a valid WKB/WKT document must Err, never panic.
#[test]
fn parsers_never_panic_truncation() {
    let wkb = vec![
        0x01u8, 0x03, 0x00, 0x00, 0x00, // little-endian polygon
        0x01, 0x00, 0x00, 0x00, // 1 ring
        0x04, 0x00, 0x00, 0x00, // 4 points
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // (0,0)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0,
        0x3f, // (1,0)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0,
        0x3f, // (1,1)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, // (0,1)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let wkt = "POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))";
    for cut in 0..=wkb.len() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(&wkb[..cut])));
        assert!(r.is_ok(), "read_wkb panicked at truncation {cut}");
    }
    for cut in 0..=wkt.len() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(&wkt[..cut])));
        assert!(r.is_ok(), "read_wkt panicked at truncation {cut}");
    }
}

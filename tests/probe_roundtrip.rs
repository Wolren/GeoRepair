use geo::{Coord, Geometry, LineString, Point, Polygon};
use geo_repair::io::wkb::{read_wkb, write_wkb};
use geo_repair::io::wkt::{read_wkt, write_wkt};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

// Idempotence comparison: a single-member MultiPolygon and the plain
// Polygon with identical coordinates are the same value in the repair
// pipeline - pass 1 may emit a single-member MP (union result kept in
// MultiPolygon type) where pass 2 emits the unwrapped Polygon (or vice
// versa). The union-based repair cannot reproduce GEOS's BuildArea face
// split (point-touching faces merge to one component), so wrapper type
// is deliberately outside the idempotence contract; coordinates and
// structure must match exactly.
fn geom_norm_eq(a: &Geometry<f64>, b: &Geometry<f64>) -> bool {
    let av: Geometry<f64> = match a {
        Geometry::MultiPolygon(mp) if mp.0.len() == 1 => Geometry::Polygon(mp.0[0].clone()),
        other => other.clone(),
    };
    let bv: Geometry<f64> = match b {
        Geometry::MultiPolygon(mp) if mp.0.len() == 1 => Geometry::Polygon(mp.0[0].clone()),
        other => other.clone(),
    };
    geom_eq(&av, &bv)
}

fn coord_eq(a: &Coord<f64>, b: &Coord<f64>) -> bool {
    // NaN payloads are not meaningful in serialized form: WKB preserves
    // the raw payload bits, WKT writes "NaN" and reparses to the
    // canonical payload (same as strtod/GEOS). Compare as values.
    let x = a.x.to_bits() == b.x.to_bits() || (a.x.is_nan() && b.x.is_nan());
    let y = a.y.to_bits() == b.y.to_bits() || (a.y.is_nan() && b.y.is_nan());
    x && y
}
fn ls_eq(a: &LineString<f64>, b: &LineString<f64>) -> bool {
    a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| coord_eq(x, y))
}
fn poly_eq(a: &Polygon<f64>, b: &Polygon<f64>) -> bool {
    ls_eq(a.exterior(), b.exterior())
        && a.interiors().len() == b.interiors().len()
        && a.interiors()
            .iter()
            .zip(b.interiors())
            .all(|(x, y)| ls_eq(x, y))
}
fn geom_eq(a: &Geometry<f64>, b: &Geometry<f64>) -> bool {
    match (a, b) {
        (Geometry::Point(Point(a)), Geometry::Point(Point(b))) => coord_eq(a, b),
        (Geometry::LineString(a), Geometry::LineString(b)) => ls_eq(a, b),
        (Geometry::Polygon(a), Geometry::Polygon(b)) => poly_eq(a, b),
        (Geometry::MultiPoint(a), Geometry::MultiPoint(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| coord_eq(&x.0, &y.0))
        }
        (Geometry::MultiLineString(a), Geometry::MultiLineString(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| ls_eq(x, y))
        }
        (Geometry::MultiPolygon(a), Geometry::MultiPolygon(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| poly_eq(x, y))
        }
        (Geometry::GeometryCollection(a), Geometry::GeometryCollection(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| geom_eq(x, y))
        }
        _ => false,
    }
}

fn check(g: &Geometry<f64>, via: &str) -> Vec<String> {
    let mut problems = Vec::new();
    // WKB roundtrip
    let back = read_wkb(&write_wkb(g));
    if let Ok(b) = back {
        if !geom_eq(g, &b) {
            problems.push(format!("{via} WKB roundtrip changed geometry"));
        }
    } else {
        problems.push(format!("{via} WKB roundtrip failed: {back:?}"));
    }
    // WKT roundtrip
    let back = read_wkt(&write_wkt(g));
    if let Ok(b) = back {
        if !geom_eq(g, &b) {
            problems.push(format!("{via} WKT roundtrip changed geometry"));
        }
    } else {
        problems.push(format!("{via} WKT roundtrip failed: {back:?}"));
    }
    // Idempotence
    for method in [PolyMethod::Auto, PolyMethod::Structure] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let o1 = g.make_valid_with_config(&cfg);
        let o2 = o1.make_valid_with_config(&cfg);
        if !geom_norm_eq(&o1, &o2) {
            problems.push(format!("{via} not idempotent in mode {method:?}"));
        }
        if !o2.validate().valid {
            problems.push(format!("{via} second-pass invalid in mode {method:?}"));
        }
    }
    problems
}

#[test]
fn roundtrip_and_idempotence_probe() {
    let mut state: u64 = 0x6a09e667f3bcc909;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // Real seeds: valid WKB/WKT documents. Mutations of these parse
    // frequently, which is what actually exercises the invariants.
    let wkb_seeds: Vec<Vec<u8>> = vec![
        // polygon
        vec![
            1u8, 3, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 240, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 240, 63, 0, 0, 0, 0, 0,
            0, 240, 63, 0, 0, 0, 0, 0, 0, 240, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        // multipolygon with a hole (two polygons)
        vec![
            1u8, 6, 0, 0, 0, 2, 0, 0, 0, 1, 3, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0,
            0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 1, 3, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 30, 0, 0, 0, 0, 0, 0, 0, 30, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0,
        ],
        // geometry collection: point + linestring
        vec![
            1u8, 7, 0, 0, 0, 2, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
        ],
        // degenerate: collinear ring
        vec![
            1u8, 3, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            0, 2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ],
    ];
    let wkt_seeds: Vec<String> = vec![
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 2 4, 4 4, 4 2, 2 2))".into(),
        "GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1, 2 2), POLYGON ((0 0, 1 0, 1 1, 0 0)))".into(),
        "MULTIPOLYGON (((0 0, 10 0, 10 10, 0 10, 0 0)), ((20 20, 30 20, 30 30, 20 30, 20 20)))".into(),
        "LINESTRING (0 0, 1e15 1e15, 1e15 4.919094327364069e208)".into(),
    ];

    let mut failures = 0;
    let mut parsed = 0;
    for _ in 0..1_000_000 {
        // Mutate a seed: flip bytes, truncate, or extend.
        let mut buf: Vec<u8> = wkb_seeds[(next() as usize) % wkb_seeds.len()].clone();
        let n_mut = 1 + (next() % 4) as usize;
        for _ in 0..n_mut {
            if buf.is_empty() {
                break;
            }
            let idx = (next() as usize) % buf.len();
            buf[idx] ^= (next() & 0xff) as u8;
        }
        if (next() & 3) == 0 && buf.len() > 4 {
            buf.truncate((next() as usize) % buf.len());
        }
        if let Ok(g) = read_wkb(&buf) {
            parsed += 1;
            for p in check(&g, "wkb") {
                failures += 1;
                if failures <= 4 {
                    let w = write_wkt(&g);
                    let g2 = read_wkt(&w).ok();
                    println!("FAIL(wkb): {p} len={} buf={buf:02x?}", buf.len());
                    println!("  wkt={w}");
                    if let Some(g2) = g2 {
                        println!("  g={g:?}");
                        println!("  g2={g2:?}");
                    }
                }
            }
        }
        let mut text: String = wkt_seeds[(next() as usize) % wkt_seeds.len()].clone();
        let chars: Vec<char> = text.chars().collect();
        let n_mut = 1 + (next() % 4) as usize;
        for _ in 0..n_mut {
            if chars.is_empty() {
                break;
            }
            let idx = (next() as usize) % chars.len();
            let c = chars[idx];
            text = text.replacen(c, "9", 1);
        }
        if (next() & 3) == 0 && text.len() > 4 {
            text.truncate((next() as usize) % text.len());
        }
        if text.len() <= 4096
            && let Ok(g) = read_wkt(&text)
        {
            parsed += 1;
            for p in check(&g, "wkt") {
                failures += 1;
                if failures <= 8 {
                    println!("FAIL(wkt): {p} text={text:?}");
                }
            }
        }
    }
    println!("parsed: {parsed}, failures: {failures}");
    assert_eq!(
        failures, 0,
        "roundtrip/idempotence violations found: {failures}"
    );
}

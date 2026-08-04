//! Fuzz the serialize/repair invariants:
//!   1. WKB read -> write -> read is bit-exact (the writer must not lose
//!      or alter coordinates, including NaN payloads of empty points).
//!   2. WKT read -> write -> read is bit-exact (ryu shortest round-trip).
//!   3. make_valid is idempotent: make_valid(make_valid(x)) ==
//!      make_valid(x), and the second pass output is valid or empty.
//!
//! The existing targets assert valid-or-empty on a SINGLE pass; this one
//! covers the invariant class GEOS documents for repair pipelines:
//! stable output across repeated passes (a second pass that changes
//! geometry is a regression signal, not a feature).
#![no_main]
use libfuzzer_sys::fuzz_target;

use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
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
        && a.interiors().iter().zip(b.interiors()).all(|(x, y)| ls_eq(x, y))
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

fuzz_target!(|data: &[u8]| {
    // WKB roundtrip.
    let parsed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkb(data))) {
        Ok(p) => p,
        Err(_) => panic!("read_wkb panicked on {} bytes", data.len()),
    };
    if let Ok(g) = parsed {
        let back = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_wkb(&write_wkb(&g))
        }));
        let back = match back {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => panic!("WKB roundtrip failed to parse: {e:?}"),
            Err(_) => panic!("read_wkb panicked on its own write output"),
        };
        assert!(geom_eq(&g, &back), "WKB roundtrip changed the geometry");
        check_idempotent(&g, "WKB");
    }

    // WKT roundtrip (lossy chars, same convention as wkt_repair).
    let text: String = data.iter().map(|&b| b as char).collect();
    if text.len() <= 4096 {
        let parsed =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_wkt(&text))) {
                Ok(p) => p,
                Err(_) => panic!("read_wkt panicked on {text:?}"),
            };
        if let Ok(g) = parsed {
            let back = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                read_wkt(&write_wkt(&g))
            }));
            let back = match back {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => panic!("WKT roundtrip failed to parse: {e:?}"),
                Err(_) => panic!("read_wkt panicked on its own write output"),
            };
            assert!(geom_eq(&g, &back), "WKT roundtrip changed the geometry: {text:?}");
            check_idempotent(&g, "WKT");
        }
    }
});

fn check_idempotent(g: &Geometry<f64>, via: &str) {
    for method in [PolyMethod::Auto, PolyMethod::Structure] {
        let cfg = MakeValidConfig { poly_method: method, ..Default::default() };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            g.make_valid_with_config(&cfg)
        }));
        let out = match out {
            Ok(o) => o,
            Err(_) => panic!("make_valid panicked on {via} input in mode {method:?}"),
        };
        let out2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            out.make_valid_with_config(&cfg)
        }));
        let out2 = match out2 {
            Ok(o) => o,
            Err(_) => panic!("make_valid panicked on its own output in mode {method:?}"),
        };
        assert!(
            geom_norm_eq(&out, &out2),
            "make_valid not idempotent in mode {method:?} ({via} input)"
        );
        assert!(out2.validate().valid, "second-pass output invalid in mode {method:?}");
    }
}

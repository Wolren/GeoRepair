//! wkt test battery (split from tests.rs 2026-08-07 for
//! file-size governance; content verbatim).

#[cfg(test)]
use super::*;

use super::*;

use super::*;
use crate::io::{load, save};
use geo::{Coord, Geometry, LineString, Polygon};
use std::time::Instant;

#[test]
fn roundtrip_point() {
    let wkt = "POINT (1.5 2.5)";
    let geom = read_wkt(wkt).unwrap();
    assert_eq!(write_wkt(&geom), wkt);
}

#[test]
fn roundtrip_point_no_space() {
    let wkt_compact = "POINT(1.5 2.5)";
    let geom = read_wkt(wkt_compact).unwrap();
    assert_eq!(write_wkt(&geom), "POINT (1.5 2.5)");
}

#[test]
fn roundtrip_linestring() {
    let geom = read_wkt("LINESTRING (0 0, 1 1, 2 0)").unwrap();
    assert_eq!(write_wkt(&geom), "LINESTRING (0.0 0.0, 1.0 1.0, 2.0 0.0)");
}

#[test]
fn roundtrip_linestring_compact() {
    let geom = read_wkt("LINESTRING(0 0,1 1,2 0)").unwrap();
    assert_eq!(write_wkt(&geom), "LINESTRING (0.0 0.0, 1.0 1.0, 2.0 0.0)");
}

#[test]
fn roundtrip_polygon() {
    let geom = read_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))").unwrap();
    assert_eq!(
        write_wkt(&geom),
        "POLYGON ((0.0 0.0, 10.0 0.0, 10.0 10.0, 0.0 10.0, 0.0 0.0))"
    );
}

#[test]
fn roundtrip_polygon_with_hole() {
    let geom =
        read_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 8 2, 8 8, 2 8, 2 2))").unwrap();
    assert_eq!(
        write_wkt(&geom),
        "POLYGON ((0.0 0.0, 10.0 0.0, 10.0 10.0, 0.0 10.0, 0.0 0.0), (2.0 2.0, 8.0 2.0, 8.0 8.0, 2.0 8.0, 2.0 2.0))"
    );
}

#[test]
fn roundtrip_multipoint_parenthesized() {
    let geom = read_wkt("MULTIPOINT (1.5 2.5, 3 4)").unwrap();
    assert_eq!(write_wkt(&geom), "MULTIPOINT (1.5 2.5, 3.0 4.0)");
}

#[test]
fn roundtrip_multipoint_double_parens() {
    let geom = read_wkt("MULTIPOINT ((1.5 2.5), (3 4))").unwrap();
    assert_eq!(write_wkt(&geom), "MULTIPOINT (1.5 2.5, 3.0 4.0)");
}

#[test]
fn roundtrip_multilinestring() {
    let geom = read_wkt("MULTILINESTRING ((0 0, 1 1), (2 2, 3 3))").unwrap();
    assert_eq!(
        write_wkt(&geom),
        "MULTILINESTRING ((0.0 0.0, 1.0 1.0), (2.0 2.0, 3.0 3.0))"
    );
}

#[test]
fn roundtrip_multipolygon() {
    let geom = read_wkt("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)), ((2 2, 3 2, 3 3, 2 3, 2 2)))")
        .unwrap();
    assert_eq!(
        write_wkt(&geom),
        "MULTIPOLYGON (((0.0 0.0, 1.0 0.0, 1.0 1.0, 0.0 1.0, 0.0 0.0)), ((2.0 2.0, 3.0 2.0, 3.0 3.0, 2.0 3.0, 2.0 2.0)))"
    );
}

#[test]
fn roundtrip_geometrycollection() {
    let geom = read_wkt("GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))").unwrap();
    assert_eq!(
        write_wkt(&geom),
        "GEOMETRYCOLLECTION (POINT (1.0 2.0), LINESTRING (0.0 0.0, 1.0 1.0))"
    );
}

#[test]
fn read_invalid_wkt() {
    let err = read_wkt("NOT A GEOMETRY").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown geometry type"), "{msg}");
}

#[test]
fn point_empty() {
    let geom = read_wkt("POINT EMPTY").unwrap();
    assert!(matches!(geom, Geometry::Point(_)));
}

#[test]
fn linestring_empty() {
    let geom = read_wkt("LINESTRING EMPTY").unwrap();
    assert!(matches!(geom, Geometry::LineString(_)));
}

#[test]
fn polygon_empty() {
    let geom = read_wkt("POLYGON EMPTY").unwrap();
    assert!(matches!(geom, Geometry::Polygon(_)));
}

#[test]
fn multipoint_empty() {
    let geom = read_wkt("MULTIPOINT EMPTY").unwrap();
    assert!(matches!(geom, Geometry::MultiPoint(_)));
}

#[test]
fn geometrycollection_empty() {
    let geom = read_wkt("GEOMETRYCOLLECTION EMPTY").unwrap();
    assert!(matches!(geom, Geometry::GeometryCollection(_)));
}

#[test]
fn z_modifier_rejected() {
    let err = read_wkt("POINT Z (1 2 3)").unwrap_err();
    assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    let msg = err.to_string();
    assert!(msg.contains("Z"), "{msg}");
}

#[test]
fn zm_modifier_rejected() {
    let err = read_wkt("POINT ZM (1 2 3 4)").unwrap_err();
    assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    let msg = err.to_string();
    assert!(msg.contains("ZM"), "{msg}");
}

#[test]
fn m_modifier_rejected() {
    let err = read_wkt("POINT M (1 2 3)").unwrap_err();
    assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    let msg = err.to_string();
    assert!(msg.contains("M"), "{msg}");
}

#[test]
fn roundtrip_via_file() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let geom = Geometry::Polygon(poly);

    let dir = std::env::temp_dir().join("geo_repair_wkt_test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.wkt");
    let path_str = path.to_str().unwrap();

    save(path_str, &geom).unwrap();
    let loaded = load(path_str).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], geom);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn iops_wkt_vs_wkb() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1000.0, y: 0.0 },
            Coord {
                x: 1000.0,
                y: 1000.0,
            },
            Coord { x: 0.0, y: 1000.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let geom = Geometry::Polygon(poly);
    let n = 10000;

    let t0 = Instant::now();
    for _ in 0..n {
        let wkt = write_wkt(&geom);
        let _ = read_wkt(&wkt).unwrap();
    }
    let dt_wkt = t0.elapsed();

    let t0 = Instant::now();
    for _ in 0..n {
        let wkb = crate::io::wkb::write_wkb(&geom);
        let _ = crate::io::wkb::read_wkb(&wkb).unwrap();
    }
    let dt_wkb = t0.elapsed();

    eprintln!(
        "WKT roundtrip ({n}×):  {dt_wkt:.3?}  ({:7.0} ns/op)",
        dt_wkt.as_nanos() as f64 / n as f64
    );
    eprintln!(
        "WKB roundtrip ({n}×):  {dt_wkb:.3?}  ({:7.0} ns/op)",
        dt_wkb.as_nanos() as f64 / n as f64
    );
    eprintln!(
        "WKT is {:.1}× slower than WKB",
        dt_wkt.as_nanos() as f64 / dt_wkb.as_nanos().max(1) as f64
    );
}

#[test]
fn trailiing_garbage_rejected() {
    assert!(read_wkt("POINT (1 2) extra").is_err());
}

#[test]
fn empty_input_rejected() {
    assert!(read_wkt("").is_err());
    assert!(read_wkt("   ").is_err());
}

/// Roundtrip all geometry types against the wkt crate to verify equivalence.
#[test]
fn roundtrip_all_types_vs_wkt_crate() {
    use wkt::ToWkt;

    let cases = [
        "POINT (1 2)",
        "LINESTRING (0 0, 1 1, 2 0)",
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))",
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 8 2, 8 8, 2 8, 2 2))",
        "MULTIPOINT (1 2, 3 4)",
        "MULTILINESTRING ((0 0, 1 1), (2 2, 3 3))",
        "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)))",
        "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)), ((2 2, 3 2, 3 3, 2 3, 2 2)))",
        "GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))",
    ];

    for wkt in &cases {
        let ours = read_wkt(wkt).unwrap();
        let theirs: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(wkt).unwrap();
        assert_eq!(ours, theirs, "mismatch for {wkt}");

        let our_out = write_wkt(&ours);
        let their_out = theirs.to_wkt().to_string();
        // Our output format may differ in whitespace — re-parse both to compare
        let ours_reparsed = read_wkt(&our_out).unwrap();
        let theirs_reparsed: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(&their_out).unwrap();
        assert_eq!(ours_reparsed, theirs_reparsed, "output mismatch for {wkt}");
    }
}

// ---------------------------------------------------------------------------
// Comprehensive double-roundtrip tests
// ---------------------------------------------------------------------------

mod double;

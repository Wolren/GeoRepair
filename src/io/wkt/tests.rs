//! WKT parser/writer test battery.

#[cfg(test)]
use super::*;

mod tests {
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
        let geom =
            read_wkt("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)), ((2 2, 3 2, 3 3, 2 3, 2 2)))")
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
            let theirs_reparsed: Geometry<f64> =
                wkt::TryFromWkt::try_from_wkt_str(&their_out).unwrap();
            assert_eq!(ours_reparsed, theirs_reparsed, "output mismatch for {wkt}");
        }
    }

    // ---------------------------------------------------------------------------
    // Comprehensive double-roundtrip tests
    // ---------------------------------------------------------------------------

    fn check_double_roundtrip(geom: &Geometry<f64>) {
        let wkt = write_wkt(geom);
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, &parsed, "double roundtrip failed for {wkt}");

        let wkt2 = write_wkt(&parsed);
        let parsed2 = read_wkt(&wkt2).unwrap();
        assert_eq!(geom, &parsed2, "triple roundtrip failed for {wkt}");
    }

    #[test]
    fn double_roundtrip_point() {
        check_double_roundtrip(&Geometry::Point(Point::new(1.5, 2.5)));
    }

    #[test]
    fn double_roundtrip_point_zero() {
        check_double_roundtrip(&Geometry::Point(Point::new(0.0, 0.0)));
    }

    #[test]
    fn double_roundtrip_point_negative() {
        check_double_roundtrip(&Geometry::Point(Point::new(-1.5, -2.5)));
    }

    #[test]
    fn double_roundtrip_point_precision() {
        check_double_roundtrip(&Geometry::Point(Point::new(
            1.2345678901234567,
            9.876543210987654,
        )));
    }

    #[test]
    fn double_roundtrip_point_high_values() {
        check_double_roundtrip(&Geometry::Point(Point::new(1e12, -3.14e8)));
    }

    #[test]
    fn double_roundtrip_point_tiny() {
        check_double_roundtrip(&Geometry::Point(Point::new(1e-12, -5e-10)));
    }

    #[test]
    fn double_roundtrip_linestring() {
        check_double_roundtrip(&Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ])));
    }

    #[test]
    fn double_roundtrip_polygon() {
        check_double_roundtrip(&Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        )));
    }

    #[test]
    fn double_roundtrip_polygon_with_hole() {
        check_double_roundtrip(&Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ])],
        )));
    }

    #[test]
    fn double_roundtrip_multipoint() {
        check_double_roundtrip(&Geometry::MultiPoint(MultiPoint(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])));
    }

    #[test]
    fn double_roundtrip_multilinestring() {
        check_double_roundtrip(&Geometry::MultiLineString(MultiLineString(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
            LineString::new(vec![Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }]),
        ])));
    }

    #[test]
    fn double_roundtrip_multipolygon() {
        check_double_roundtrip(&Geometry::MultiPolygon(MultiPolygon(vec![Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        )])));
    }

    #[test]
    fn double_roundtrip_geometrycollection() {
        check_double_roundtrip(&Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ])));
    }

    // EMPTY geometry roundtrip
    #[test]
    fn double_roundtrip_point_empty() {
        let geom = read_wkt("POINT EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "POINT EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        // NaN != NaN in IEEE 754, so check both are NaN coords
        if let (Geometry::Point(a), Geometry::Point(b)) = (&geom, &parsed) {
            assert!(a.x().is_nan() && a.y().is_nan());
            assert!(b.x().is_nan() && b.y().is_nan());
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn double_roundtrip_linestring_empty() {
        let geom = read_wkt("LINESTRING EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "LINESTRING EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_polygon_empty() {
        let geom = read_wkt("POLYGON EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "POLYGON EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_multipoint_empty() {
        let geom = read_wkt("MULTIPOINT EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "MULTIPOINT EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_multilinestring_empty() {
        let geom = read_wkt("MULTILINESTRING EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "MULTILINESTRING EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_multipolygon_empty() {
        let geom = read_wkt("MULTIPOLYGON EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "MULTIPOLYGON EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_gc_empty() {
        let geom = read_wkt("GEOMETRYCOLLECTION EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "GEOMETRYCOLLECTION EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    // read_f64 edge cases
    #[test]
    fn parse_nan() {
        let geom = read_wkt("POINT (NaN NaN)").unwrap();
        if let Geometry::Point(p) = geom {
            assert!(p.x().is_nan());
            assert!(p.y().is_nan());
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn parse_inf() {
        let geom = read_wkt("POINT (inf -inf)").unwrap();
        if let Geometry::Point(p) = geom {
            assert!(p.x().is_infinite());
            assert!(p.x().is_sign_positive());
            assert!(p.y().is_infinite());
            assert!(p.y().is_sign_negative());
        } else {
            panic!("expected Point");
        }
    }

    // Minimal POLYGON (()) — previously would fail
    #[test]
    fn double_roundtrip_empty_ring() {
        let geom = read_wkt("POLYGON EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        // Should write as "POLYGON EMPTY", not "POLYGON (())"
        assert_eq!(wkt, "POLYGON EMPTY");
    }

    // Construct empty geometries programmatically and roundtrip
    #[test]
    fn double_roundtrip_empty_point_constructed() {
        let geom = Geometry::Point(Point(Coord {
            x: f64::NAN,
            y: f64::NAN,
        }));
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "POINT EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        if let Geometry::Point(p) = parsed {
            assert!(p.x().is_nan() && p.y().is_nan());
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn double_roundtrip_empty_linestring_constructed() {
        check_double_roundtrip(&Geometry::LineString(LineString::new(vec![])));
    }

    #[test]
    fn double_roundtrip_empty_polygon_constructed() {
        check_double_roundtrip(&Geometry::Polygon(Polygon::new(
            LineString::new(vec![]),
            vec![],
        )));
    }

    // read_wkt_from / write_wkt_to tests
    #[test]
    fn read_wkt_from_reader() {
        let input = "POINT (1.5 2.5)";
        let reader = input.as_bytes();
        let geom = read_wkt_from(reader).unwrap();
        assert_eq!(write_wkt(&geom), "POINT (1.5 2.5)");
    }

    #[test]
    fn read_wkt_from_empty_fails() {
        let err = read_wkt_from(&b""[..]).unwrap_err();
        assert!(matches!(err, WktError::EmptyInput));
    }

    #[test]
    fn read_wkt_from_invalid_fails() {
        let err = read_wkt_from(&b"NOT WKT"[..]).unwrap_err();
        assert!(matches!(err, WktError::ParseError { .. }));
    }

    #[test]
    fn read_wkt_from_z_rejected() {
        let err = read_wkt_from(&b"POINT Z (1 2 3)"[..]).unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    }

    #[test]
    fn write_wkt_to_writer() {
        let geom = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkt_to(&geom, &mut buf).unwrap();
        assert_eq!(buf, b"POINT (1.5 2.5)");
    }

    #[test]
    fn write_wkt_to_write_then_read() {
        let geom = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkt_to(&geom, &mut buf).unwrap();
        let back = read_wkt_from(&buf[..]).unwrap();
        assert_eq!(write_wkt(&geom), write_wkt(&back));
    }

    // infer_wkt_type tests
    #[test]
    fn infer_type_point() {
        let (name, dims) = infer_wkt_type("POINT (1 2)").unwrap();
        assert_eq!(name, "POINT");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_linestring() {
        let (name, dims) = infer_wkt_type("LINESTRING (0 0, 1 1)").unwrap();
        assert_eq!(name, "LINESTRING");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_polygon() {
        let (name, dims) = infer_wkt_type("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))").unwrap();
        assert_eq!(name, "POLYGON");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_multipoint() {
        let (name, dims) = infer_wkt_type("MULTIPOINT (1 2, 3 4)").unwrap();
        assert_eq!(name, "MULTIPOINT");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_multilinestring() {
        let (name, dims) = infer_wkt_type("MULTILINESTRING ((0 0, 1 1))").unwrap();
        assert_eq!(name, "MULTILINESTRING");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_multipolygon() {
        let (name, dims) = infer_wkt_type("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)))").unwrap();
        assert_eq!(name, "MULTIPOLYGON");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_gc() {
        let (name, dims) = infer_wkt_type("GEOMETRYCOLLECTION (POINT (1 2))").unwrap();
        assert_eq!(name, "GEOMETRYCOLLECTION");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_empty_fails() {
        let err = infer_wkt_type("").unwrap_err();
        assert!(matches!(err, WktError::EmptyInput));
    }

    #[test]
    fn infer_type_whitespace_fails() {
        let err = infer_wkt_type("   ").unwrap_err();
        assert!(matches!(err, WktError::EmptyInput));
    }

    #[test]
    fn infer_type_z_rejected() {
        let err = infer_wkt_type("POINT Z (1 2 3)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    }

    #[test]
    fn infer_type_zm_rejected() {
        let err = infer_wkt_type("POINT ZM (1 2 3 4)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    }

    #[test]
    fn infer_type_unknown_fails() {
        let err = infer_wkt_type("CIRCULARSTRING (1 2, 3 4)").unwrap_err();
        assert!(matches!(err, WktError::ParseError { .. }));
    }

    // -----------------------------------------------------------------------
    // Production-readiness battery: strtod specials, case-insensitive
    // keywords, EMPTY elements, malformed inputs, non-OGC variants.
    // -----------------------------------------------------------------------

    #[test]
    fn special_values_strtod_semantics() {
        // GEOS parses numbers via strtod: nan/inf/infinity, any case, signed.
        for (wkt, x, y) in [
            ("POINT (NaN NaN)", f64::NAN, f64::NAN),
            ("POINT (nan 5)", f64::NAN, 5.0),
            ("POINT (NAN 5)", f64::NAN, 5.0),
            ("POINT (-nan 5)", f64::NAN, 5.0),
            ("POINT (Inf 5)", f64::INFINITY, 5.0),
            ("POINT (inf 5)", f64::INFINITY, 5.0),
            ("POINT (INF 5)", f64::INFINITY, 5.0),
            ("POINT (-Inf 5)", f64::NEG_INFINITY, 5.0),
            ("POINT (+Inf 5)", f64::INFINITY, 5.0),
            ("POINT (Infinity 5)", f64::INFINITY, 5.0),
            ("POINT (-Infinity 5)", f64::NEG_INFINITY, 5.0),
        ] {
            let g = read_wkt(wkt).unwrap();
            if let Geometry::Point(p) = g {
                assert_eq!(p.0.y.is_nan(), y.is_nan(), "y nan-ness mismatch for {wkt}");
                if !y.is_nan() {
                    assert_eq!(p.0.y, y, "y mismatch for {wkt}");
                }
                assert_eq!(p.0.x.is_nan(), x.is_nan(), "nan-ness mismatch for {wkt}");
                if !x.is_nan() {
                    assert_eq!(p.0.x, x, "x mismatch for {wkt}");
                }
            } else {
                panic!("expected point for {wkt}");
            }
        }
    }

    #[test]
    fn keywords_case_insensitive() {
        for (wkt, expect) in [
            ("point (1 2)", "POINT (1.0 2.0)"),
            ("linestring (0 0, 1 1)", "LINESTRING (0.0 0.0, 1.0 1.0)"),
            ("Polygon ((0 0, 1 0, 1 1, 0 1, 0 0))", "POLYGON ((0.0 0.0, 1.0 0.0, 1.0 1.0, 0.0 1.0, 0.0 0.0))"),
            ("multipoint (1 2, 3 4)", "MULTIPOINT (1.0 2.0, 3.0 4.0)"),
            ("MultiPolygon (((0 0, 1 0, 1 1, 0 1, 0 0)))", "MULTIPOLYGON (((0.0 0.0, 1.0 0.0, 1.0 1.0, 0.0 1.0, 0.0 0.0)))"),
            ("geometrycollection (point (1 2))", "GEOMETRYCOLLECTION (POINT (1.0 2.0))"),
        ] {
            let g = read_wkt(wkt).unwrap();
            assert_eq!(write_wkt(&g), expect, "mismatch for {wkt}");
        }
    }

    #[test]
    fn linearring_parsed_as_linestring() {
        let g = read_wkt("LINEARRING (0 0, 0 10, 10 10, 10 0, 0 0)").unwrap();
        assert!(matches!(g, Geometry::LineString(_)));
    }

    #[test]
    fn empty_elements_in_collections() {
        let g = read_wkt("MULTIPOINT (EMPTY, (1 2), empty)").unwrap();
        if let Geometry::MultiPoint(mp) = g {
            assert_eq!(mp.0.len(), 3);
            assert!(mp.0[0].0.x.is_nan() && mp.0[0].0.y.is_nan());
            assert_eq!(mp.0[1], Point::new(1.0, 2.0));
            assert!(mp.0[2].0.x.is_nan());
        } else {
            panic!("expected multipoint");
        }

        let g = read_wkt("MULTILINESTRING (EMPTY, (1 1, 2 2))").unwrap();
        if let Geometry::MultiLineString(mls) = g {
            assert_eq!(mls.0.len(), 2);
            assert!(mls.0[0].0.is_empty());
        } else {
            panic!("expected multilinestring");
        }

        let g = read_wkt("MULTIPOLYGON (EMPTY, ((0 0, 1 0, 1 1, 0 1, 0 0)))").unwrap();
        if let Geometry::MultiPolygon(mp) = g {
            assert_eq!(mp.0.len(), 2);
            assert!(mp.0[0].exterior().0.is_empty());
        } else {
            panic!("expected multipolygon");
        }

        let g = read_wkt("GEOMETRYCOLLECTION (EMPTY, POINT (1 2))").unwrap();
        if let Geometry::GeometryCollection(gc) = g {
            assert_eq!(gc.0.len(), 2);
            assert!(matches!(gc.0[0], Geometry::Point(p) if p.0.x.is_nan()));
        } else {
            panic!("expected geometrycollection");
        }

        // All-EMPTY polygon rings parse to an empty polygon.
        let g = read_wkt("POLYGON (EMPTY, EMPTY, EMPTY)").unwrap();
        if let Geometry::Polygon(p) = g {
            assert!(p.exterior().0.is_empty());
        } else {
            panic!("expected polygon");
        }
    }

    #[test]
    fn malformed_wkt_rejected() {
        let bad = [
            "POINT (EMPTY)",      // EMPTY is a token, not a coordinate
            "LINESTRING ()",      // empty coordinate list without EMPTY keyword
            "POINT (1)",          // missing y
            "POINT (1 2 3)",      // undeclared Z is rejected (2D only)
            "POINT (1 2, 3 4)",   // extra coordinate
            "POINT 1 2",          // missing parens
            "POINT (1 2))",       // unbalanced
            "LINESTRING (1 2,, 3 4)", // double comma
            "POINT (1 2) POINT (3 4)", // trailing geometry
            "",                   // empty input
            "   ",                // whitespace only
            "POINT Z (1 2 3)",    // Z modifier
            "POINT (1 2",         // truncated
            "GEOMETRYCOLLECTION (POINT (1 2)", // unbalanced collection
            "MULTIPOINT ()",      // empty parens without EMPTY keyword
        ];
        for wkt in bad {
            assert!(
                read_wkt(wkt).is_err(),
                "expected rejection of {wkt:?}"
            );
        }
    }

    #[test]
    fn degenerate_ring_parses_but_flagged_by_validator() {
        // A 2-coordinate ring is structurally parsed (Polygon::new
        // auto-closes), then the validator flags it - the reader is
        // deliberately structural; validity is the validator's gate.
        let g = read_wkt("POLYGON ((0 0, 1 1))").unwrap();
        assert!(!crate::validation::validate(&g).valid);
    }

    #[test]
    fn non_ogc_variants_serialized_losslessly() {
        // OGC WKT has no Line/Rect/Triangle; they must serialize to their
        // closest OGC equivalents (coordinate-exact), never silently drop
        // to GEOMETRYCOLLECTION EMPTY.
        let line = Geometry::Line(geo::Line::new(
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 3.0, y: 4.0 },
        ));
        assert_eq!(write_wkt(&line), "LINESTRING (1.0 2.0, 3.0 4.0)");
        assert_eq!(read_wkt(&write_wkt(&line)).unwrap(), Geometry::LineString(LineString::new(vec![
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 3.0, y: 4.0 },
        ])));

        let rect = Geometry::Rect(geo::Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 5.0 },
        ));
        assert_eq!(
            write_wkt(&rect),
            "POLYGON ((0.0 0.0, 10.0 0.0, 10.0 5.0, 0.0 5.0, 0.0 0.0))"
        );

        let tri = Geometry::Triangle(geo::Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
        ));
        assert_eq!(
            write_wkt(&tri),
            "POLYGON ((0.0 0.0, 2.0 0.0, 1.0 2.0, 0.0 0.0))"
        );
        // And they round-trip through the reader.
        assert!(matches!(
            read_wkt(&write_wkt(&tri)).unwrap(),
            Geometry::Polygon(_)
        ));
    }

    #[test]
    fn collection_empties_roundtrip() {
        for wkt in [
            "MULTIPOINT EMPTY",
            "MULTILINESTRING EMPTY",
            "MULTIPOLYGON EMPTY",
            "GEOMETRYCOLLECTION EMPTY",
            "POINT EMPTY",
            "LINESTRING EMPTY",
            "POLYGON EMPTY",
        ] {
            let g = read_wkt(wkt).unwrap();
            assert_eq!(write_wkt(&g), wkt, "writer must canonicalize {wkt}");
            assert_eq!(write_wkt(&read_wkt(&write_wkt(&g)).unwrap()), wkt);
        }
    }

    #[test]
    fn read_wkt_from_and_write_wkt_to() {
        let mut rdr = std::io::Cursor::new(b"POINT (1 2)".to_vec());
        let g = read_wkt_from(&mut rdr).unwrap();
        let mut out = Vec::new();
        write_wkt_to(&g, &mut out).unwrap();
        assert_eq!(out, b"POINT (1.0 2.0)");
    }
}

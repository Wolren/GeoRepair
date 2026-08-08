//! Differential WKB parity vs the georust/wkb crate (independent
//! implementation, geo_traits-based zero-copy reader).
//!
//! Every corpus geometry is written by BOTH writers and parsed by BOTH
//! readers; all four parse results must agree bit-exactly (NaN payloads
//! excepted - neither implementation preserves them through a roundtrip).
//! This is a pure test-oracle dependency: the crate's own readers stay
//! canonical (the external wkb crate is banned from the production path).

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use geo_repair::io::wkb::{read_ewkb, read_wkb, write_wkb};
use geo_traits::to_geo::ToGeoGeometry;
use wkb::writer::{WriteOptions, write_geometry};

fn coord_eq(a: &Coord<f64>, b: &Coord<f64>) -> bool {
    let x = a.x.to_bits() == b.x.to_bits() || (a.x.is_nan() && b.x.is_nan());
    let y = a.y.to_bits() == b.y.to_bits() || (a.y.is_nan() && b.y.is_nan());
    x && y
}

fn ls_eq(a: &LineString<f64>, b: &LineString<f64>) -> bool {
    a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| coord_eq(x, y))
}

fn poly_eq(a: &Polygon<f64>, b: &Polygon<f64>) -> bool {
    ls_eq(&a.exterior(), &b.exterior())
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

fn our_parse(bytes: &[u8]) -> Geometry<f64> {
    read_wkb(bytes).expect("our reader")
}

fn their_parse(bytes: &[u8]) -> Geometry<f64> {
    let wkb = wkb::reader::read_wkb(bytes).expect("georust/wkb reader");
    wkb.to_geometry()
}

fn their_write(g: &Geometry<f64>) -> Vec<u8> {
    let mut buf = Vec::new();
    write_geometry(&mut buf, g, &WriteOptions::default()).expect("georust/wkb writer");
    buf
}

fn corpus() -> Vec<Geometry<f64>> {
    let poly = |coords: &[(f64, f64)], holes: &[&[(f64, f64)]]| {
        let ext: Vec<Coord<f64>> = coords.iter().map(|&(x, y)| Coord { x, y }).collect();
        let ints: Vec<LineString<f64>> = holes
            .iter()
            .map(|h| LineString::new(h.iter().map(|&(x, y)| Coord { x, y }).collect()))
            .collect();
        Polygon::new(LineString::new(ext), ints)
    };
    vec![
        Geometry::Point(Point::new(0.0, 0.0)),
        Geometry::Point(Point::new(1e15, -4.919094327364069e208)),
        Geometry::Point(Point::new(f64::NAN, 5.0)),
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1e-310,
                y: 1e-310,
            },
            Coord { x: 3.0, y: 3.0 },
            Coord { x: 3.0, y: 3.0 },
            Coord { x: 0.0, y: 0.0 },
        ])),
        Geometry::LineString(LineString::new(vec![])),
        Geometry::Polygon(poly(
            &[
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ],
            &[],
        )),
        Geometry::Polygon(poly(
            &[
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ],
            &[
                &[(2.0, 2.0), (2.0, 3.0), (3.0, 3.0), (3.0, 2.0), (2.0, 2.0)],
                &[(6.0, 6.0), (6.0, 7.0), (7.0, 7.0), (7.0, 6.0), (6.0, 6.0)],
            ],
        )),
        Geometry::Polygon(Polygon::new(LineString::new(vec![]), vec![])),
        Geometry::MultiPoint(MultiPoint::new(vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 0.0),
        ])),
        Geometry::MultiPoint(MultiPoint::new(vec![])),
        Geometry::MultiLineString(MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
            LineString::new(vec![]),
        ])),
        Geometry::MultiPolygon(MultiPolygon::new(vec![
            poly(
                &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0), (0.0, 0.0)],
                &[],
            ),
            poly(
                &[
                    (20.0, 20.0),
                    (25.0, 20.0),
                    (25.0, 25.0),
                    (20.0, 25.0),
                    (20.0, 20.0),
                ],
                &[],
            ),
        ])),
        Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::Polygon(poly(
                &[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0), (0.0, 0.0)],
                &[],
            )),
            Geometry::LineString(LineString::new(vec![])),
        ])),
        Geometry::GeometryCollection(GeometryCollection(vec![])),
    ]
}

/// Hand-crafted EWKB: byte order 1 (NDR), type with Z flag (0x80000003 for
/// Polygon+Z), optional SRID flag (0x20000000). Both readers must drop the
/// extra Z ordinates and agree on the 2D projection.
#[test]
fn ewkb_z_and_srid_parity() {
    // POINT Z: 01 01 00 00 80 | x y z (3 f64 LE)
    let mut pt_z = vec![0x01, 0x01, 0x00, 0x00, 0x80];
    for v in [1.0_f64, 2.0, 99.0] {
        pt_z.extend_from_slice(&v.to_le_bytes());
    }
    let ours = our_parse(&pt_z);
    let theirs = their_parse(&pt_z);
    assert!(
        geom_eq(&ours, &theirs),
        "POINT Z parity: ours {ours:?} theirs {theirs:?}"
    );
    assert!(
        matches!(ours, Geometry::Point(_)),
        "POINT Z must project to 2D"
    );

    // POLYGON Z with SRID: 01 03 00 00 a0 (0x20000003 | 0x80000000 = 0xa0000003)
    // | srid u32 | ring count u32 | coord count u32 | x y z x y z ...
    let mut poly_z = vec![0x01, 0x03, 0x00, 0x00, 0xa0];
    poly_z.extend_from_slice(&4326u32.to_le_bytes());
    poly_z.extend_from_slice(&1u32.to_le_bytes());
    poly_z.extend_from_slice(&4u32.to_le_bytes());
    let pts: [(f64, f64); 4] = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 0.0)];
    for (x, y) in pts {
        poly_z.extend_from_slice(&x.to_le_bytes());
        poly_z.extend_from_slice(&y.to_le_bytes());
        poly_z.extend_from_slice(&7.5f64.to_le_bytes());
    }
    let ours = our_parse(&poly_z);
    let theirs = their_parse(&poly_z);
    assert!(
        geom_eq(&ours, &theirs),
        "POLYGON Z SRID parity: ours {ours:?} theirs {theirs:?}"
    );
    let ewkb = read_ewkb(&poly_z).expect("our EWKB reader keeps SRID");
    assert_eq!(ewkb.srid, Some(4326), "SRID must survive read_ewkb");
}

#[test]
fn wkb_parity_both_writers() {
    for (i, g) in corpus().iter().enumerate() {
        // Our writer -> both readers.
        let our_bytes = write_wkb(g);
        let ours = our_parse(&our_bytes);
        let theirs = their_parse(&our_bytes);
        assert!(
            geom_eq(&ours, &theirs),
            "case {i}: our writer output diverges: ours {ours:?} theirs {theirs:?}"
        );
        assert!(geom_eq(&ours, g), "case {i}: our write->read lost data");

        // Their writer -> both readers.
        let their_bytes = their_write(g);
        let theirs = their_parse(&their_bytes);
        let ours2 = our_parse(&their_bytes);
        assert!(
            geom_eq(&ours2, &theirs),
            "case {i}: their writer output diverges: ours {ours2:?} theirs {theirs:?}"
        );
        assert!(
            geom_eq(&ours2, g),
            "case {i}: their write->read lost data: {ours2:?}"
        );
    }
}

#[test]
fn wkb_cross_parse_byte_equality() {
    // For the canonical cases the two writers must produce byte-identical
    // output (both write 2D NDR WKB, same structure). This pins the wire
    // format: any drift in either writer is a compatibility break.
    for (i, g) in corpus().iter().enumerate() {
        // Skip NaN payload cases: NaN bit patterns are implementation
        // specific and not comparable across writers.
        let has_nan = || {
            fn geom_has_nan(g: &Geometry<f64>) -> bool {
                match g {
                    Geometry::Point(p) => p.0.x.is_nan() || p.0.y.is_nan(),
                    _ => false,
                }
            }
            geom_has_nan(g)
        };
        if has_nan() {
            continue;
        }
        let ours = write_wkb(g);
        let theirs = their_write(g);
        assert_eq!(
            ours,
            theirs,
            "case {i}: writers disagree on the wire format (ours len {}, theirs len {})",
            ours.len(),
            theirs.len()
        );
    }
}

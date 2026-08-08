//! wkb test battery (split from tests.rs 2026-08-07 for
//! file-size governance; content verbatim).

#[cfg(test)]
use super::*;

use super::*;

use super::*;

#[test]
fn roundtrip_point_le() {
    let g = Geometry::Point(Point::new(1.0, 2.0));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_point_be() {
    let g = Geometry::Point(Point::new(1.0, 2.0));
    // Manually construct big-endian WKB
    let mut wkb = Vec::new();
    wkb.push(0); // byte order = BE
    wkb.extend_from_slice(&1u32.to_be_bytes()); // Point type
    wkb.extend_from_slice(&1.0f64.to_be_bytes());
    wkb.extend_from_slice(&2.0f64.to_be_bytes());
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_linestring() {
    let g = Geometry::LineString(LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 0.0 },
    ]));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_polygon() {
    let g = Geometry::Polygon(Polygon::new(
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
    ));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_multipoint() {
    let g = Geometry::MultiPoint(MultiPoint(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_multilinestring() {
    let g = Geometry::MultiLineString(MultiLineString(vec![
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        LineString::new(vec![Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }]),
    ]));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_multipolygon() {
    let g = Geometry::MultiPolygon(MultiPolygon(vec![
        Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        ),
        Polygon::new(
            LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 3.0, y: 2.0 },
                Coord { x: 3.0, y: 3.0 },
                Coord { x: 2.0, y: 3.0 },
                Coord { x: 2.0, y: 2.0 },
            ]),
            vec![],
        ),
    ]));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn roundtrip_gc() {
    let g = Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ])),
    ]));
    let wkb = write_wkb(&g);
    let back = read_wkb(&wkb).unwrap();
    assert_eq!(g, back);
}

#[test]
fn estimate_wkb_size_works() {
    let g = Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![],
    ));
    let wkb = write_wkb(&g);
    let est = estimate_wkb_size(&wkb).unwrap();
    assert_eq!(est, wkb.len());
}

#[test]
fn read_wkb_concat_works() {
    let g1 = Geometry::Point(Point::new(1.0, 2.0));
    let g2 = Geometry::Point(Point::new(3.0, 4.0));
    let mut concat = Vec::new();
    concat.extend_from_slice(&write_wkb(&g1));
    concat.extend_from_slice(&write_wkb(&g2));
    let geoms = read_wkb_concat(&concat).unwrap();
    assert_eq!(geoms.len(), 2);
    assert_eq!(geoms[0], g1);
    assert_eq!(geoms[1], g2);
}

#[test]
fn ewkb_srid_stripped() {
    // EWKB point with SRID flag and SRID value
    let mut wkb = Vec::new();
    wkb.push(1); // LE
    // type = Point (1) | SRID flag (0x20000000)
    let type_with_srid = 1u32 | WKB_SRID_FLAG;
    wkb.extend_from_slice(&type_with_srid.to_le_bytes());
    wkb.extend_from_slice(&4326u32.to_le_bytes()); // SRID
    wkb.extend_from_slice(&1.0f64.to_le_bytes());
    wkb.extend_from_slice(&2.0f64.to_le_bytes());

    let geom = read_wkb(&wkb).unwrap();
    assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
}

#[test]
fn ewkb_z_flag_now_returns_2d() {
    // read_wkb no longer rejects Z dimension — silently returns 2D
    let mut wkb = Vec::new();
    wkb.push(1);
    let type_with_z = 1u32 | WKB_Z_FLAG;
    wkb.extend_from_slice(&type_with_z.to_le_bytes());
    wkb.extend_from_slice(&1.0f64.to_le_bytes());
    wkb.extend_from_slice(&2.0f64.to_le_bytes());
    wkb.extend_from_slice(&3.0f64.to_le_bytes());

    let geom = read_wkb(&wkb).unwrap();
    assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
}

#[test]
fn ewkb_zm_flag_now_returns_2d() {
    let mut wkb = Vec::new();
    wkb.push(1);
    let type_with_zm = 1u32 | WKB_Z_FLAG | WKB_M_FLAG;
    wkb.extend_from_slice(&type_with_zm.to_le_bytes());
    wkb.extend_from_slice(&1.0f64.to_le_bytes());
    wkb.extend_from_slice(&2.0f64.to_le_bytes());
    wkb.extend_from_slice(&3.0f64.to_le_bytes());
    wkb.extend_from_slice(&4.0f64.to_le_bytes());

    let geom = read_wkb(&wkb).unwrap();
    assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
}

#[test]
fn ewkb_z_in_multi_sub_geom_now_returns_2d() {
    let mut wkb = Vec::new();
    wkb.push(1);
    wkb.extend_from_slice(&4u32.to_le_bytes()); // MultiPoint
    wkb.extend_from_slice(&2u32.to_le_bytes()); // 2 points

    wkb.push(1);
    wkb.extend_from_slice(&1u32.to_le_bytes());
    wkb.extend_from_slice(&1.0f64.to_le_bytes());
    wkb.extend_from_slice(&2.0f64.to_le_bytes());

    wkb.push(1);
    let type_with_z = 1u32 | WKB_Z_FLAG;
    wkb.extend_from_slice(&type_with_z.to_le_bytes());
    wkb.extend_from_slice(&3.0f64.to_le_bytes());
    wkb.extend_from_slice(&4.0f64.to_le_bytes());
    wkb.extend_from_slice(&5.0f64.to_le_bytes());

    let geom = read_wkb(&wkb).unwrap();
    if let Geometry::MultiPoint(mp) = geom {
        assert_eq!(mp.0.len(), 2);
        assert_eq!(mp.0[0], Point::new(1.0, 2.0));
        assert_eq!(mp.0[1], Point::new(3.0, 4.0));
    } else {
        panic!("expected MultiPoint");
    }
}

// -----------------------------------------------------------------------
// EWKB roundtrip tests
// -----------------------------------------------------------------------

mod ewkb;

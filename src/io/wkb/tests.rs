//! WKB/WKT round-trip and parser test battery.

#[cfg(test)]
use super::*;

mod tests {
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

    fn make_test_geom() -> Geometry<f64> {
        Geometry::Point(Point::new(1.0, 2.0))
    }

    fn make_test_linestring() -> Geometry<f64> {
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ]))
    }

    fn make_test_polygon() -> Geometry<f64> {
        Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        ))
    }

    fn coord_count(geom: &Geometry<f64>) -> usize {
        use geo::Geometry::*;
        match geom {
            Point(_) => 1,
            LineString(ls) => ls.0.len(),
            Polygon(poly) => {
                let mut n = poly.exterior().0.len();
                for h in poly.interiors() {
                    n += h.0.len();
                }
                n
            }
            MultiPoint(mp) => mp.0.len(),
            MultiLineString(mls) => mls.0.iter().map(|ls| ls.0.len()).sum(),
            MultiPolygon(mp) => {
                mp.0.iter()
                    .map(|p| {
                        let mut n = p.exterior().0.len();
                        for h in p.interiors() {
                            n += h.0.len();
                        }
                        n
                    })
                    .sum()
            }
            GeometryCollection(gc) => gc.0.iter().map(coord_count).sum(),
            _ => 0,
        }
    }

    #[test]
    fn ewkb_read_2d_no_extra() {
        // Plain 2D WKB through read_ewkb yields empty extra_coords
        let bytes = write_wkb(&make_test_geom());
        let ewkb = read_ewkb(&bytes).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, None);
        assert_eq!(ewkb.dims, EwkbDims::XY);
        assert!(ewkb.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_read_with_srid() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_srid = 1u32 | WKB_SRID_FLAG;
        wkb.extend_from_slice(&type_with_srid.to_le_bytes());
        wkb.extend_from_slice(&4326u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, Some(4326));
        assert_eq!(ewkb.dims, EwkbDims::XY);
        assert!(ewkb.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_read_with_z() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_z = 1u32 | WKB_Z_FLAG;
        wkb.extend_from_slice(&type_with_z.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, None);
        assert_eq!(ewkb.dims, EwkbDims::XYZ);
        assert_eq!(ewkb.extra_coords, vec![3.0]);
    }

    #[test]
    fn ewkb_read_with_srid_and_z() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_srid_z = 1u32 | WKB_SRID_FLAG | WKB_Z_FLAG;
        wkb.extend_from_slice(&type_with_srid_z.to_le_bytes());
        wkb.extend_from_slice(&4326u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, Some(4326));
        assert_eq!(ewkb.dims, EwkbDims::XYZ);
        assert_eq!(ewkb.extra_coords, vec![3.0]);
    }

    #[test]
    fn ewkb_read_with_zm() {
        let mut wkb = Vec::new();
        wkb.push(1);
        let type_with_zm = 1u32 | WKB_Z_FLAG | WKB_M_FLAG;
        wkb.extend_from_slice(&type_with_zm.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());
        wkb.extend_from_slice(&4.0f64.to_le_bytes());

        let ewkb = read_ewkb(&wkb).unwrap();
        assert_eq!(ewkb.geometry, make_test_geom());
        assert_eq!(ewkb.srid, None);
        assert_eq!(ewkb.dims, EwkbDims::XYZM);
        assert_eq!(ewkb.extra_coords, vec![3.0, 4.0]);
    }

    #[test]
    fn ewkb_write_read_roundtrip_xy() {
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: None,
            dims: EwkbDims::XY,
            extra_coords: vec![],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, ewkb.geometry);
        assert_eq!(back.srid, None);
        assert_eq!(back.dims, EwkbDims::XY);
        assert!(back.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_write_read_roundtrip_xyz() {
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: vec![99.0],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, ewkb.geometry);
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, vec![99.0]);
    }

    #[test]
    fn ewkb_write_read_roundtrip_xyzm() {
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: Some(4326),
            dims: EwkbDims::XYZM,
            extra_coords: vec![10.0, 20.0],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, ewkb.geometry);
        assert_eq!(back.srid, Some(4326));
        assert_eq!(back.dims, EwkbDims::XYZM);
        assert_eq!(back.extra_coords, vec![10.0, 20.0]);
    }

    #[test]
    fn ewkb_roundtrip_linestring_xyz() {
        let geom = make_test_linestring();
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 1.5).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_polygon_xyz() {
        let geom = make_test_polygon();
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 0.5).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_multipoint_srid() {
        let geom =
            Geometry::MultiPoint(MultiPoint(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]));
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 10.0).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: Some(2154),
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.srid, Some(2154));
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_multipolygon_xyz() {
        let geom = Geometry::MultiPolygon(MultiPolygon(vec![
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
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 0.25).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: Some(4326),
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.srid, Some(4326));
        assert_eq!(back.dims, EwkbDims::XYZ);
        assert_eq!(back.extra_coords, extra);
    }

    #[test]
    fn ewkb_roundtrip_gc_xy() {
        let geom = Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ]));
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XY,
            extra_coords: vec![],
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.srid, None);
        assert_eq!(back.dims, EwkbDims::XY);
        assert!(back.extra_coords.is_empty());
    }

    #[test]
    fn ewkb_write_then_read_wkb_2d() {
        // write_ewkb → read_wkb returns 2D geo with no metadata
        let ewkb = EwkbGeometry {
            geometry: make_test_geom(),
            srid: Some(4326),
            dims: EwkbDims::XYZ,
            extra_coords: vec![42.0],
        };
        let bytes = write_ewkb(&ewkb);
        let geom = read_wkb(&bytes).unwrap();
        assert_eq!(geom, make_test_geom());
    }

    #[test]
    fn ewkb_roundtrip_polygon_with_hole_xyz() {
        let geom = Geometry::Polygon(Polygon::new(
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
        let n = coord_count(&geom);
        let extra: Vec<f64> = (0..n).map(|i| i as f64 * 0.1).collect();
        let ewkb = EwkbGeometry {
            geometry: geom.clone(),
            srid: None,
            dims: EwkbDims::XYZ,
            extra_coords: extra.clone(),
        };
        let bytes = write_ewkb(&ewkb);
        let back = read_ewkb(&bytes).unwrap();
        assert_eq!(back.geometry, geom);
        assert_eq!(back.extra_coords, extra);
    }

    // -----------------------------------------------------------------------
    // WriteOptions / BE writing tests
    // -----------------------------------------------------------------------

    #[test]
    fn write_wkb_with_opts_be_roundtrip_point() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        // Verify byte order marker is 0 (BE)
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_linestring() {
        let g = Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_polygon() {
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
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_multipolygon() {
        let g = Geometry::MultiPolygon(MultiPolygon(vec![Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        )]));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_with_opts_be_roundtrip_gc() {
        let g = Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ]));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let wkb = write_wkb_with_opts(&g, &opts);
        assert_eq!(wkb[0], 0);
        let back = read_wkb(&wkb).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_default_is_le() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let wkb = write_wkb(&g);
        assert_eq!(wkb[0], 1); // LE
        let with_opts = write_wkb_with_opts(&g, &WriteOptions::default());
        assert_eq!(wkb, with_opts);
    }

    #[test]
    fn write_wkb_with_opts_mixed_endian_explicit() {
        // Ensure LE and BE produce different byte sequences
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let le_opts = WriteOptions {
            endianness: Endianness::LittleEndian,
        };
        let be_opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let le_wkb = write_wkb_with_opts(&g, &le_opts);
        let be_wkb = write_wkb_with_opts(&g, &be_opts);
        assert_ne!(le_wkb, be_wkb, "LE and BE output should differ");
    }

    // -----------------------------------------------------------------------
    // read_wkb_from / write_wkb_to tests
    // -----------------------------------------------------------------------

    #[test]
    fn read_wkb_from_reader() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        let wkb = write_wkb(&g);
        let reader = &wkb[..];
        let back = read_wkb_from(reader).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn read_wkb_from_empty_fails() {
        let err = read_wkb_from(&b""[..]).unwrap_err();
        assert!(matches!(err, WkbError::UnexpectedEof));
    }

    #[test]
    fn read_wkb_from_invalid_fails() {
        let err = read_wkb_from(&b"\xff\xff\xff\xff"[..]).unwrap_err();
        assert!(matches!(err, WkbError::InvalidByteOrder(255)));
    }

    #[test]
    fn write_wkb_to_writer() {
        let g = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkb_to(&g, &mut buf).unwrap();
        let back = read_wkb(&buf).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_to_write_then_read() {
        let g = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkb_to(&g, &mut buf).unwrap();
        let back = read_wkb_from(&buf[..]).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn write_wkb_to_writer_be_via_opts() {
        let g = Geometry::Point(Point::new(1.5, 2.5));
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
        };
        let buf = write_wkb_with_opts(&g, &opts);
        let back = read_wkb_from(&buf[..]).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn mixed_byte_order_multipolygon() {
        // Outer: LE
        // Inner polygon 1: BE
        // Inner polygon 2: LE
        let mut wkb = Vec::new();

        // Header: LE, MultiPolygon
        wkb.push(1);
        wkb.extend_from_slice(&6u32.to_le_bytes()); // MultiPolygon
        wkb.extend_from_slice(&2u32.to_le_bytes()); // 2 polygons

        // Polygon 1: BE
        wkb.push(0); // BE
        wkb.extend_from_slice(&3u32.to_be_bytes()); // Polygon
        wkb.extend_from_slice(&1u32.to_be_bytes()); // 1 ring
        wkb.extend_from_slice(&5u32.to_be_bytes()); // 5 coords
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&10.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());
        wkb.extend_from_slice(&0.0f64.to_be_bytes());

        // Polygon 2: LE
        wkb.push(1); // LE
        wkb.extend_from_slice(&3u32.to_le_bytes()); // Polygon
        wkb.extend_from_slice(&1u32.to_le_bytes()); // 1 ring
        wkb.extend_from_slice(&5u32.to_le_bytes()); // 5 coords
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&15.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());
        wkb.extend_from_slice(&5.0f64.to_le_bytes());

        let geom = read_wkb(&wkb).unwrap();
        if let Geometry::MultiPolygon(mp) = geom {
            assert_eq!(mp.0.len(), 2);
        } else {
            panic!("expected MultiPolygon");
        }
    }

    // -----------------------------------------------------------------------
    // Production-readiness battery: truncation, garbage, trailing bytes,
    // non-OGC variants, empty geometries, precise error codes.
    // -----------------------------------------------------------------------

    /// Every strict prefix of a valid WKB buffer must fail cleanly (no
    /// panics, no silent truncation acceptance).
    #[test]
    fn truncated_wkb_never_panics() {
        let poly = match make_test_polygon() {
            Geometry::Polygon(p) => p,
            other => panic!("expected polygon, got {other:?}"),
        };
        let geoms = vec![
            make_test_geom(),
            make_test_linestring(),
            make_test_polygon(),
            Geometry::MultiPolygon(MultiPolygon(vec![poly.clone(), poly])),
            Geometry::GeometryCollection(GeometryCollection(vec![
                make_test_geom(),
                make_test_linestring(),
            ])),
        ];
        for g in &geoms {
            let wkb = write_wkb(g);
            for cut in 0..wkb.len() {
                let err = read_wkb(&wkb[..cut]).unwrap_err();
                assert!(
                    matches!(
                        &err,
                        WkbError::UnexpectedEof
                            | WkbError::UnknownTypeCode(_)
                            | WkbError::InconsistentCount { .. }
                    ),
                    "prefix {cut} of {} bytes: unexpected success or wrong error {err:?}",
                    wkb.len()
                );
            }
        }
    }

    #[test]
    fn garbage_buffers_rejected() {
        let garbage: Vec<Vec<u8>> = vec![
            vec![],
            vec![0xDE, 0xAD, 0xBE, 0xEF],
            vec![0x01, 0x00, 0x00, 0x00, 0xFF],
            vec![0x02, 0x00, 0x00, 0x00, 0x01], // invalid byte order
            vec![0x01, 0x00, 0x00, 0x00, 0x63], // unknown type code 99
            vec![0x01; 64],
            vec![0x00; 32],
        ];
        for buf in garbage {
            let _ = read_wkb(&buf); // must not panic
            assert!(read_wkb(&buf).is_err(), "garbage accepted: {buf:?}");
        }
    }

    #[test]
    fn trailing_bytes_rejected() {
        let g = make_test_polygon();
        let wkb = write_wkb(&g);

        // One trailing byte.
        let mut buf = wkb.clone();
        buf.push(0x00);
        let err = read_wkb(&buf).unwrap_err();
        assert!(
            matches!(&err, WkbError::TrailingBytes { consumed, total } if *consumed == wkb.len() && *total == wkb.len() + 1),
            "expected TrailingBytes, got {err:?}"
        );

        // A second complete geometry behind the first.
        let mut buf = wkb.clone();
        buf.extend_from_slice(&write_wkb(&make_test_geom()));
        let err = read_wkb(&buf).unwrap_err();
        assert!(
            matches!(err, WkbError::TrailingBytes { .. }),
            "expected TrailingBytes for concatenation, got {err:?}"
        );
        // ...but the concat API reads both.
        assert_eq!(read_wkb_concat(&buf).unwrap().len(), 2);
    }

    #[test]
    fn non_ogc_variants_roundtrip_wkb() {
        // Line/Rect/Triangle serialize to their OGC equivalents
        // (coordinate-exact), never silently to an empty collection.
        let line = Geometry::Line(geo::Line::new(
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 3.0, y: 4.0 },
        ));
        let back = read_wkb(&write_wkb(&line)).unwrap();
        assert_eq!(
            back,
            Geometry::LineString(LineString::new(vec![
                Coord { x: 1.0, y: 2.0 },
                Coord { x: 3.0, y: 4.0 },
            ]))
        );

        let rect = Geometry::Rect(geo::Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 5.0 },
        ));
        let back = read_wkb(&write_wkb(&rect)).unwrap();
        if let Geometry::Polygon(p) = back {
            assert_eq!(p.exterior().0.len(), 5);
            assert_eq!(p.exterior().0[0], Coord { x: 0.0, y: 0.0 });
            assert_eq!(p.exterior().0[2], Coord { x: 10.0, y: 5.0 });
        } else {
            panic!("expected polygon for rect WKB");
        }

        let tri = Geometry::Triangle(geo::Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
        ));
        let back = read_wkb(&write_wkb(&tri)).unwrap();
        if let Geometry::Polygon(p) = back {
            assert_eq!(p.exterior().0.len(), 4);
            assert_eq!(p.exterior().0[3], Coord { x: 0.0, y: 0.0 });
        } else {
            panic!("expected polygon for triangle WKB");
        }

        // Big-endian variants too.
        let opts = WriteOptions {
            endianness: Endianness::BigEndian,
            ..Default::default()
        };
        let back = read_wkb(&write_wkb_with_opts(&rect, &opts)).unwrap();
        assert!(matches!(back, Geometry::Polygon(_)));
    }

    #[test]
    fn empty_geometries_roundtrip_wkb() {
        let geoms = vec![
            Geometry::Point(Point::new(f64::NAN, f64::NAN)),
            Geometry::LineString(LineString::new(vec![])),
            Geometry::Polygon(Polygon::new(LineString::new(vec![]), vec![])),
            Geometry::MultiPoint(MultiPoint(vec![])),
            Geometry::MultiLineString(MultiLineString(vec![])),
            Geometry::MultiPolygon(MultiPolygon(vec![])),
            Geometry::GeometryCollection(GeometryCollection(vec![])),
        ];
        for g in &geoms {
            let back = read_wkb(&write_wkb(g)).unwrap();
            match (g, &back) {
                (Geometry::Point(a), Geometry::Point(b)) => {
                    assert!(a.0.x.is_nan() && b.0.x.is_nan(), "empty point round trip");
                }
                _ => assert_eq!(g, &back, "empty geometry round trip"),
            }
        }
    }

    #[test]
    fn wrong_subtype_reports_real_code() {
        // MULTIPOINT containing a LINESTRING sub-geometry: the error must
        // carry the actual sub type code (2 = LINESTRING), not 0.
        let mut buf = Vec::new();
        buf.push(1u8); // NDR
        buf.extend_from_slice(&WKB_MULTIPOINT.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // one element
        buf.extend_from_slice(&write_wkb(&make_test_linestring()));
        let err = read_wkb(&buf).unwrap_err();
        assert!(
            matches!(
                err,
                WkbError::UnexpectedGeometryType {
                    expected: "Point",
                    code: WKB_LINESTRING
                }
            ),
            "expected real sub-type code, got {err:?}"
        );
    }

    #[test]
    fn invalid_byte_order_and_unknown_code() {
        // Byte order byte must be 0 or 1.
        let err = read_wkb(&[0x02, 0x00, 0x00, 0x00, 0x01]).unwrap_err();
        assert!(matches!(err, WkbError::InvalidByteOrder(2)));
        // Unknown type code 99 (0x63).
        let err = read_wkb(&[0x01, 0x63, 0x00, 0x00, 0x00]).unwrap_err();
        assert!(matches!(err, WkbError::UnknownTypeCode(99)));
    }
}

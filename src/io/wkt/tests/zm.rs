//! Z/M dimension tests: reading, writing, round trip, and the EWKB convention.

use super::*;
use crate::io::Dims;
use crate::io::wkb::{EwkbDims, EwkbGeometry, read_ewkb, write_ewkb};
use geo::{Geometry, Point};

fn xy(x: f64, y: f64) -> Geometry<f64> {
    Geometry::Point(Point::new(x, y))
}

#[test]
fn point_z_keeps_the_extra_ordinate() {
    let zm = read_wkt_zm("POINT Z (1 2 3)").unwrap();
    assert_eq!(zm.geometry, xy(1.0, 2.0));
    assert_eq!(zm.dims, Dims::XYZ);
    assert_eq!(zm.extra_coords, vec![3.0]);
    assert_eq!(write_wkt_zm(&zm), "POINT Z (1.0 2.0 3.0)");
}

#[test]
fn point_m_keeps_the_extra_ordinate() {
    let zm = read_wkt_zm("POINT M (1 2 3)").unwrap();
    assert_eq!(zm.geometry, xy(1.0, 2.0));
    assert_eq!(zm.dims, Dims::XYM);
    assert_eq!(zm.extra_coords, vec![3.0]);
    assert_eq!(write_wkt_zm(&zm), "POINT M (1.0 2.0 3.0)");
}

#[test]
fn point_zm_keeps_both_ordinates() {
    let zm = read_wkt_zm("POINT ZM (1 2 3 4)").unwrap();
    assert_eq!(zm.geometry, xy(1.0, 2.0));
    assert_eq!(zm.dims, Dims::XYZM);
    assert_eq!(zm.extra_coords, vec![3.0, 4.0]);
    assert_eq!(write_wkt_zm(&zm), "POINT ZM (1.0 2.0 3.0 4.0)");
}

#[test]
fn read_wkt_drops_the_extra_ordinates() {
    for (text, expected) in [
        ("POINT Z (1 2 3)", xy(1.0, 2.0)),
        ("POINT M (1 2 3)", xy(1.0, 2.0)),
        ("POINT ZM (1 2 3 4)", xy(1.0, 2.0)),
    ] {
        assert_eq!(read_wkt(text).unwrap(), expected, "{text}");
    }
}

#[test]
fn the_2d_writer_never_emits_a_modifier() {
    let zm = read_wkt_zm("LINESTRING Z (0 0 1, 1 1 2)").unwrap();
    assert_eq!(write_wkt(&zm.geometry), "LINESTRING (0.0 0.0, 1.0 1.0)");
}

#[test]
fn linestring_z_keeps_every_z() {
    let zm = read_wkt_zm("LINESTRING Z (0 0 1, 1 1 2, 2 0 3)").unwrap();
    assert_eq!(zm.dims, Dims::XYZ);
    assert_eq!(zm.extra_coords, vec![1.0, 2.0, 3.0]);
    assert_eq!(
        write_wkt_zm(&zm),
        "LINESTRING Z (0.0 0.0 1.0, 1.0 1.0 2.0, 2.0 0.0 3.0)"
    );
}

#[test]
fn polygon_z_keeps_shell_then_hole_order() {
    let text = "POLYGON Z ((0 0 1, 10 0 2, 10 10 3, 0 10 4, 0 0 1), \
                (2 2 5, 8 2 6, 8 8 7, 2 8 8, 2 2 5))";
    let zm = read_wkt_zm(text).unwrap();
    assert_eq!(
        zm.extra_coords,
        vec![1.0, 2.0, 3.0, 4.0, 1.0, 5.0, 6.0, 7.0, 8.0, 5.0]
    );
    let back = read_wkt_zm(&write_wkt_zm(&zm)).unwrap();
    assert_eq!(back.geometry, zm.geometry);
    assert_eq!(back.extra_coords, zm.extra_coords);
}

#[test]
fn multipoint_z_accepts_both_forms() {
    let flat = read_wkt_zm("MULTIPOINT Z (1 2 3, 4 5 6)").unwrap();
    let paren = read_wkt_zm("MULTIPOINT Z ((1 2 3), (4 5 6))").unwrap();
    assert_eq!(flat.extra_coords, vec![3.0, 6.0]);
    assert_eq!(flat.geometry, paren.geometry);
    assert_eq!(flat.extra_coords, paren.extra_coords);
    assert_eq!(
        write_wkt_zm(&flat),
        "MULTIPOINT Z (1.0 2.0 3.0, 4.0 5.0 6.0)"
    );
}

#[test]
fn multilinestring_z_keeps_line_order() {
    let zm = read_wkt_zm("MULTILINESTRING Z ((0 0 1, 1 1 2), (2 2 3, 3 3 4))").unwrap();
    assert_eq!(zm.extra_coords, vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        write_wkt_zm(&zm),
        "MULTILINESTRING Z ((0.0 0.0 1.0, 1.0 1.0 2.0), (2.0 2.0 3.0, 3.0 3.0 4.0))"
    );
}

#[test]
fn multipolygon_z_keeps_part_order() {
    let text = "MULTIPOLYGON Z (((0 0 1, 1 0 2, 1 1 3, 0 0 1)), \
                ((2 2 4, 3 2 5, 3 3 6, 2 2 4)))";
    let zm = read_wkt_zm(text).unwrap();
    assert_eq!(
        zm.extra_coords,
        vec![1.0, 2.0, 3.0, 1.0, 4.0, 5.0, 6.0, 4.0]
    );
    let back = read_wkt_zm(&write_wkt_zm(&zm)).unwrap();
    assert_eq!(back.geometry, zm.geometry);
    assert_eq!(back.extra_coords, zm.extra_coords);
}

#[test]
fn geometrycollection_z_keeps_member_order() {
    let text = "GEOMETRYCOLLECTION Z (POINT Z (1 2 3), LINESTRING Z (0 0 4, 1 1 5))";
    let zm = read_wkt_zm(text).unwrap();
    assert_eq!(zm.extra_coords, vec![3.0, 4.0, 5.0]);
    assert_eq!(
        write_wkt_zm(&zm),
        "GEOMETRYCOLLECTION Z (POINT Z (1.0 2.0 3.0), LINESTRING Z (0.0 0.0 4.0, 1.0 1.0 5.0))"
    );
}

#[test]
fn a_member_inherits_the_collection_modifier() {
    // GEOS reads this and reprints the member as POINT Z.
    let zm = read_wkt_zm("GEOMETRYCOLLECTION Z (POINT (1 2 3))").unwrap();
    assert_eq!(zm.dims, Dims::XYZ);
    assert_eq!(zm.extra_coords, vec![3.0]);
    assert_eq!(
        write_wkt_zm(&zm),
        "GEOMETRYCOLLECTION Z (POINT Z (1.0 2.0 3.0))"
    );
}

#[test]
fn empty_geometries_keep_the_modifier() {
    for text in [
        "POINT Z EMPTY",
        "POINT ZM EMPTY",
        "LINESTRING Z EMPTY",
        "POLYGON ZM EMPTY",
        "MULTIPOINT Z EMPTY",
        "MULTILINESTRING ZM EMPTY",
        "MULTIPOLYGON Z EMPTY",
        "GEOMETRYCOLLECTION ZM EMPTY",
    ] {
        let zm = read_wkt_zm(text).unwrap();
        assert!(zm.extra_coords.is_empty(), "{text}");
        assert_eq!(write_wkt_zm(&zm), text, "{text}");
    }
}

#[test]
fn mixed_dimensions_are_rejected_by_the_zm_entry_only() {
    let text = "GEOMETRYCOLLECTION (POINT Z (1 2 3), POINT (4 5))";
    let err = read_wkt_zm(text).unwrap_err();
    assert!(matches!(err, WktError::MixedDimensions { .. }), "{err}");
    assert!(read_wkt(text).is_ok());
}

#[test]
fn a_narrow_member_under_a_wide_header_is_a_parse_error() {
    // GEOS: "Cannot mix dimensionality in a geometry."
    let err = read_wkt("GEOMETRYCOLLECTION Z (POINT (4 5))").unwrap_err();
    assert!(matches!(err, WktError::ParseError { .. }), "{err}");
}

#[test]
fn a_missing_ordinate_is_a_parse_error() {
    for text in [
        "POINT Z (1 2)",
        "POINT ZM (1 2 3)",
        "POINT Z ()",
        "LINESTRING Z ()",
    ] {
        assert!(
            matches!(read_wkt(text), Err(WktError::ParseError { .. })),
            "{text}"
        );
    }
}

#[test]
fn lowercase_and_tight_modifiers_are_read() {
    let zm = read_wkt_zm("point zm(1 2 3 4)").unwrap();
    assert_eq!(zm.dims, Dims::XYZM);
    assert_eq!(zm.extra_coords, vec![3.0, 4.0]);
}

#[test]
fn roundtrip_over_every_coordinate_count() {
    for (dims, label) in [(Dims::XYZ, "Z"), (Dims::XYM, "M"), (Dims::XYZM, "ZM")] {
        let extra_per_coord = (dims.coord_count() - 2) as usize;
        for n in 1..24usize {
            let mut coords = String::new();
            let mut extras = Vec::new();
            for i in 0..n {
                if i > 0 {
                    coords.push_str(", ");
                }
                let x = i as f64;
                let y = 2.0 * i as f64;
                coords.push_str(&format!("{x} {y}"));
                for k in 0..extra_per_coord {
                    let v = 10.0 + i as f64 + k as f64 * 0.5;
                    coords.push_str(&format!(" {v}"));
                    extras.push(v);
                }
            }
            let text = format!("LINESTRING {label} ({coords})");
            let zm = read_wkt_zm(&text).unwrap();
            assert_eq!(zm.dims, dims, "{text}");
            assert_eq!(zm.extra_coords, extras, "{text}");
            let back = read_wkt_zm(&write_wkt_zm(&zm)).unwrap();
            assert_eq!(back.geometry, zm.geometry, "{text}");
            assert_eq!(back.dims, dims, "{text}");
            assert_eq!(back.extra_coords, extras, "{text}");
        }
    }
}

#[test]
fn wkt_extras_follow_the_ewkb_convention() {
    for (text, dims) in [
        ("POINT Z (1 2 3)", EwkbDims::XYZ),
        ("LINESTRING ZM (0 0 1 2, 1 1 3 4)", EwkbDims::XYZM),
        ("POLYGON M ((0 0 1, 1 0 2, 1 1 3, 0 0 1))", EwkbDims::XYM),
    ] {
        let wkt = read_wkt_zm(text).unwrap();
        assert_eq!(wkt.dims, dims, "{text}");
        let ewkb = EwkbGeometry {
            geometry: wkt.geometry.clone(),
            srid: None,
            dims: wkt.dims,
            extra_coords: wkt.extra_coords.clone(),
        };
        let back = read_ewkb(&write_ewkb(&ewkb)).unwrap();
        assert_eq!(back.geometry, wkt.geometry, "{text}");
        assert_eq!(back.dims, dims, "{text}");
        assert_eq!(back.extra_coords, wkt.extra_coords, "{text}");
    }
}

#[test]
fn ewkb_extras_reach_wkt_text() {
    let ewkb = EwkbGeometry {
        geometry: Geometry::Point(Point::new(1.0, 2.0)),
        srid: None,
        dims: EwkbDims::XYZ,
        extra_coords: vec![7.5],
    };
    let back = read_ewkb(&write_ewkb(&ewkb)).unwrap();
    let wkt = WktGeometry {
        geometry: back.geometry,
        dims: back.dims,
        extra_coords: back.extra_coords,
    };
    assert_eq!(write_wkt_zm(&wkt), "POINT Z (1.0 2.0 7.5)");
}

#[test]
fn wkt_and_ewkb_name_the_same_dimension_type() {
    let dims: Dims = EwkbDims::XYZM;
    assert_eq!(dims.coord_count(), 4);
}

#[test]
fn xy_output_matches_the_2d_writer() {
    let geom = read_wkt("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))").unwrap();
    let flat = WktGeometry {
        geometry: geom.clone(),
        dims: Dims::XY,
        extra_coords: vec![],
    };
    assert_eq!(write_wkt_zm(&flat), write_wkt(&geom));
}

#[test]
fn reads_ogr_normalised_text() {
    // The spacing OGR prints: no space after a comma.
    let zm = read_wkt_zm("LINESTRING ZM (0 0 1 2,1 1 3 4)").unwrap();
    assert_eq!(zm.extra_coords, vec![1.0, 2.0, 3.0, 4.0]);
    let z = read_wkt_zm("GEOMETRYCOLLECTION Z (POINT Z (1 2 3),POINT Z (4 5 6))").unwrap();
    assert_eq!(z.extra_coords, vec![3.0, 6.0]);
}

#[test]
fn read_wkt_zm_from_reads_a_reader() {
    let zm = read_wkt_zm_from(&b"LINESTRING M (0 0 1, 1 1 2)"[..]).unwrap();
    assert_eq!(zm.dims, Dims::XYM);
    assert_eq!(zm.extra_coords, vec![1.0, 2.0]);
}

#[test]
fn write_wkt_zm_to_writes_a_writer() {
    let zm = read_wkt_zm("POINT Z (1 2 3)").unwrap();
    let mut buf = Vec::new();
    write_wkt_zm_to(&zm, &mut buf).unwrap();
    assert_eq!(buf, b"POINT Z (1.0 2.0 3.0)");
}

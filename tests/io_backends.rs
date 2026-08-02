//! Round-trip and load tests for the optional I/O backends
//! (CSV, GeoPackage, Shapefile, GML). Each module is feature-gated to match
//! the backend it exercises.

use geo::{Coord, Geometry, LineString, Point, Polygon};

fn sample_polygon() -> Geometry<f64> {
    Geometry::Polygon(Polygon::new(
        LineString::from(vec![
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 3.0),
            Point::new(2.0, 5.0),
            Point::new(0.0, 3.0),
            Point::new(0.0, 0.0),
        ]),
        vec![LineString::from(vec![
            Point::new(1.0, 1.0),
            Point::new(2.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(1.0, 2.0),
            Point::new(1.0, 1.0),
        ])],
    ))
}

fn sample_point() -> Geometry<f64> {
    Geometry::Point(Point::new(1.5, 2.5))
}

#[cfg(feature = "io-csv")]
mod csv_tests {
    use super::*;
    use geo_repair::io::csv_io::{load_csv, save_csv};

    #[test]
    fn csv_roundtrip_polygon_and_point() {
        let dir = std::env::temp_dir().join("geo_repair_csv_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.csv");
        let path = path.to_str().unwrap().to_string();
        save_csv(&path, &[sample_polygon(), sample_point()]).unwrap();
        let loaded = load_csv(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(matches!(loaded[0], Geometry::Polygon(_)));
        assert!(matches!(loaded[1], Geometry::Point(_)));
        if let Geometry::Polygon(p) = &loaded[0] {
            assert_eq!(p.exterior().0.len(), 6);
            assert_eq!(p.interiors().len(), 1);
        }
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(feature = "io-gpkg")]
mod gpkg_tests {
    use super::*;
    use geo_repair::io::gpkg::{load_gpkg, save_gpkg};

    #[test]
    fn gpkg_roundtrip() {
        let dir = std::env::temp_dir().join("geo_repair_gpkg_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.gpkg");
        let path = path.to_str().unwrap().to_string();
        save_gpkg(&path, &[sample_polygon()]).unwrap();
        let loaded = load_gpkg(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        if let Geometry::Polygon(p) = &loaded[0] {
            assert_eq!(p.exterior().0.len(), 6);
            assert_eq!(p.interiors().len(), 1);
            // Hole vertex must survive the WKB round trip.
            let hole_first = p.interiors()[0].0[0];
            assert!((hole_first.x - 1.0).abs() < 1e-12);
        } else {
            panic!("expected polygon");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn gpkg_empty_is_error() {
        let dir = std::env::temp_dir().join("geo_repair_gpkg_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.gpkg");
        let path = path.to_str().unwrap().to_string();
        assert!(save_gpkg(&path, &[]).is_err());
    }
}

#[cfg(feature = "io-shp")]
mod shp_tests {
    use super::*;
    use geo_repair::io::shp::{load_shp, save_shp};

    #[test]
    fn shp_roundtrip_polygon_with_hole() {
        let dir = std::env::temp_dir().join("geo_repair_shp_test");
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("out");
        let base = base.to_str().unwrap().to_string();
        save_shp(&base, &[sample_polygon()]).unwrap();
        let loaded = load_shp(&base).unwrap();
        assert_eq!(loaded.len(), 1);
        if let Geometry::Polygon(p) = &loaded[0] {
            assert_eq!(p.exterior().0.len(), 6);
            // The hole ring is preserved as an inner ring by the writer.
            assert!(p.interiors().len() >= 1);
        } else {
            panic!("expected polygon");
        }
        std::fs::remove_file(format!("{base}.shp")).ok();
        std::fs::remove_file(format!("{base}.shx")).ok();
    }

    #[test]
    fn shp_multipolygon_roundtrip() {
        let dir = std::env::temp_dir().join("geo_repair_shp_test");
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("multi");
        let base = base.to_str().unwrap().to_string();
        let mp = Geometry::MultiPolygon(geo::MultiPolygon(vec![
            Polygon::new(
                LineString::from(vec![
                    Point::new(0.0, 0.0),
                    Point::new(1.0, 0.0),
                    Point::new(1.0, 1.0),
                    Point::new(0.0, 0.0),
                ]),
                Vec::new(),
            ),
            Polygon::new(
                LineString::from(vec![
                    Point::new(5.0, 5.0),
                    Point::new(6.0, 5.0),
                    Point::new(6.0, 6.0),
                    Point::new(5.0, 5.0),
                ]),
                Vec::new(),
            ),
        ]));
        save_shp(&base, &[mp]).unwrap();
        let loaded = load_shp(&base).unwrap();
        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            Geometry::MultiPolygon(mp) => assert_eq!(mp.0.len(), 2),
            Geometry::Polygon(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
        std::fs::remove_file(format!("{base}.shp")).ok();
        std::fs::remove_file(format!("{base}.shx")).ok();
    }
}

#[cfg(feature = "io-gml")]
mod gml_tests {
    use super::*;
    use geo_repair::io::gml::{load_gml, save_gml};

    #[test]
    fn gml_roundtrip_polygon_with_hole() {
        let dir = std::env::temp_dir().join("geo_repair_gml_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.gml");
        let path = path.to_str().unwrap().to_string();
        save_gml(&path, &[sample_polygon()]).unwrap();
        let loaded = load_gml(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        if let Geometry::Polygon(p) = &loaded[0] {
            assert_eq!(p.exterior().0.len(), 6);
            assert_eq!(p.interiors().len(), 1);
            let hole_first = p.interiors()[0].0[0];
            assert!((hole_first.x - 1.0).abs() < 1e-12);
        } else {
            panic!("expected polygon");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn gml_load_point_and_multipolygon() {
        let gml = r#"<?xml version="1.0" encoding="UTF-8"?>
<gml:FeatureCollection xmlns:gml="http://www.opengis.net/gml/3.2">
  <gml:featureMember>
    <gml:Point gml:id="p1"><gml:pos>1.0 2.0</gml:pos></gml:Point>
  </gml:featureMember>
  <gml:featureMember>
    <gml:MultiPolygon gml:id="mp1">
      <gml:polygonMember>
        <gml:Polygon>
          <gml:exterior><gml:LinearRing><gml:posList>0 0 2 0 2 2 0 0</gml:posList></gml:LinearRing></gml:exterior>
        </gml:Polygon>
      </gml:polygonMember>
    </gml:MultiPolygon>
  </gml:featureMember>
</gml:FeatureCollection>"#;
        let dir = std::env::temp_dir().join("geo_repair_gml_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("in.gml");
        std::fs::write(&path, gml).unwrap();
        let path = path.to_str().unwrap().to_string();
        let loaded = load_gml(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(matches!(loaded[0], Geometry::Point(_)));
        if let Geometry::MultiPolygon(mp) = &loaded[1] {
            assert_eq!(mp.0.len(), 1, "member polygon must be parsed, not skipped");
            assert_eq!(mp.0[0].exterior().0.len(), 4);
        } else {
            panic!("expected multipolygon");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn gml_surface_and_3d_pos_list() {
        // srsDimension="3" posList: third ordinate must be ignored.
        let gml = r#"<gml:MultiSurface xmlns:gml="http://www.opengis.net/gml/3.2">
  <gml:surfaceMember>
    <gml:Polygon>
      <gml:exterior><gml:LinearRing><gml:posList srsDimension="3">0 0 9 3 0 9 3 3 9 0 0 9</gml:posList></gml:LinearRing></gml:exterior>
    </gml:Polygon>
  </gml:surfaceMember>
</gml:MultiSurface>"#;
        let dir = std::env::temp_dir().join("geo_repair_gml_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("surface.gml");
        std::fs::write(&path, gml).unwrap();
        let path = path.to_str().unwrap().to_string();
        let loaded = load_gml(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        if let Geometry::MultiPolygon(mp) = &loaded[0] {
            let ext = &mp.0[0].exterior().0;
            assert_eq!(ext.len(), 4);
            assert_eq!(ext[0], Coord { x: 0.0, y: 0.0 });
        } else {
            panic!("expected multipolygon");
        }
        std::fs::remove_file(&path).ok();
    }
}

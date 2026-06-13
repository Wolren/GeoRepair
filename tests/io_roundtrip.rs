#![cfg(feature = "io-all")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};
use geo_repair::zm::ZmValue;
use geo_repair::{
    export_features, export_geometries_with_crs, load_features, load_geometries,
    load_geometries_with_crs, Crs, Feature,
};

// ===== Unique temp path helpers =====

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_path(ext: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    let tag = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("geo_rt_{ext}_{tag}.{ext}"))
}

fn cleanup_path(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

fn cleanup_shp(stem: &PathBuf) {
    for ext in &["shp", "shx", "dbf", "prj"] {
        let _ = std::fs::remove_file(stem.with_extension(ext));
    }
}

// ===== Geometry factories =====

fn test_crs() -> Option<Crs> {
    Some(Crs::from_epsg(4326))
}

fn test_point() -> Geometry<f64> {
    Geometry::Point(Point::new(1.0, 2.0))
}

fn test_geom() -> Geometry<f64> {
    Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            (0.0, 0.0).into(),
            (10.0, 0.0).into(),
            (10.0, 10.0).into(),
            (0.0, 10.0).into(),
            (0.0, 0.0).into(),
        ]),
        vec![],
    ))
}

fn test_multipoint() -> Geometry<f64> {
    Geometry::MultiPoint(MultiPoint::new(vec![
        Point::new(1.0, 2.0),
        Point::new(3.0, 4.0),
        Point::new(5.0, 6.0),
    ]))
}

fn test_multilinestring() -> Geometry<f64> {
    Geometry::MultiLineString(MultiLineString::new(vec![
        LineString::new(vec![(0.0, 0.0).into(), (1.0, 1.0).into()]),
        LineString::new(vec![(2.0, 2.0).into(), (3.0, 3.0).into()]),
    ]))
}

fn test_multipolygon() -> Geometry<f64> {
    Geometry::MultiPolygon(MultiPolygon::new(vec![
        Polygon::new(
            LineString::new(vec![
                (0.0, 0.0).into(),
                (1.0, 0.0).into(),
                (1.0, 1.0).into(),
                (0.0, 0.0).into(),
            ]),
            vec![],
        ),
        Polygon::new(
            LineString::new(vec![
                (2.0, 2.0).into(),
                (3.0, 2.0).into(),
                (3.0, 3.0).into(),
                (2.0, 2.0).into(),
            ]),
            vec![],
        ),
    ]))
}

fn test_geometrycollection() -> Geometry<f64> {
    Geometry::GeometryCollection(GeometryCollection(vec![
        test_point(),
        test_geom(),
        test_multipoint(),
    ]))
}

fn test_linestring() -> Geometry<f64> {
    Geometry::LineString(LineString::new(vec![
        (0.0, 0.0).into(),
        (1.0, 1.0).into(),
        (2.0, 2.0).into(),
    ]))
}

fn test_line() -> Geometry<f64> {
    Geometry::Line(Line::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
    ))
}

fn test_triangle() -> Geometry<f64> {
    Geometry::Triangle(Triangle::new(
        (0.0, 0.0).into(),
        (1.0, 0.0).into(),
        (0.0, 1.0).into(),
    ))
}

fn test_rect() -> Geometry<f64> {
    Geometry::Rect(Rect::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 2.0, y: 2.0 },
    ))
}

fn test_large_coords() -> Geometry<f64> {
    Geometry::Point(Point::new(1e12, -1e12))
}

fn test_many_coords() -> Geometry<f64> {
    let coords: Vec<Coord<f64>> = (0..1000)
        .map(|i| Coord {
            x: i as f64,
            y: (i * 2) as f64,
        })
        .collect();
    let mut all = coords.clone();
    all.push(coords[0]);
    Geometry::Polygon(Polygon::new(LineString::new(all), vec![]))
}

// ===== Z/M feature factories =====

fn test_point_z() -> Feature {
    Feature::with_all(
        Geometry::Point(Point::new(1.0, 2.0)),
        None,
        None,
        vec![ZmValue::z_only(100.0)],
    )
}

fn test_point_m() -> Feature {
    Feature::with_all(
        Geometry::Point(Point::new(1.0, 2.0)),
        None,
        None,
        vec![ZmValue::m_only(200.0)],
    )
}

fn test_point_zm() -> Feature {
    Feature::with_all(
        Geometry::Point(Point::new(1.0, 2.0)),
        None,
        None,
        vec![ZmValue::new(Some(100.0), Some(200.0))],
    )
}

fn test_polygon_z() -> Feature {
    Feature::with_all(
        Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                (0.0, 0.0).into(),
                (10.0, 0.0).into(),
                (10.0, 10.0).into(),
                (0.0, 10.0).into(),
                (0.0, 0.0).into(),
            ]),
            vec![],
        )),
        None,
        None,
        vec![
            ZmValue::z_only(10.0),
            ZmValue::z_only(20.0),
            ZmValue::z_only(30.0),
            ZmValue::z_only(40.0),
            ZmValue::z_only(50.0),
        ],
    )
}

// ===== Roundtrip helpers =====

/// Export + load, verify exactly 1 geometry loaded (count only).
fn roundtrip_geom(ext: &str, geom: &Geometry<f64>, crs: Option<&Crs>) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    assert!(
        export_geometries_with_crs(&[geom.clone()], path_str, crs).is_ok(),
        "Export .{ext} failed"
    );
    let loaded = load_geometries(path_str);
    assert!(loaded.is_ok(), "Load .{ext} failed: {:?}", loaded);
    assert_eq!(loaded.unwrap().len(), 1, ".{ext}: expected 1 geometry");
    cleanup_path(&path);
}

/// Export + load, verify exact geometry equality.
fn roundtrip_geom_eq(ext: &str, geom: &Geometry<f64>, crs: Option<&Crs>) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    assert!(
        export_geometries_with_crs(&[geom.clone()], path_str, crs).is_ok(),
        "Export .{ext} failed"
    );
    let loaded = load_geometries(path_str).unwrap();
    assert_eq!(loaded.len(), 1, ".{ext}: expected 1 geometry");
    assert_eq!(loaded[0], *geom, ".{ext}: geometry mismatch");
    cleanup_path(&path);
}

/// Export + load for lossy types (Line→LineString, Rect→Polygon, Triangle→Polygon).
fn roundtrip_geom_lossy(ext: &str, geom: &Geometry<f64>) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    assert!(
        export_geometries_with_crs(&[geom.clone()], path_str, None).is_ok(),
        "Export .{ext} failed"
    );
    let loaded = load_geometries(path_str);
    assert!(loaded.is_ok(), "Load .{ext} failed: {:?}", loaded);
    assert!(
        !loaded.unwrap().is_empty(),
        ".{ext}: expected at least 1 geometry"
    );
    cleanup_path(&path);
}

/// Export + load GeometryCollection, verify expected count.
fn roundtrip_gc(ext: &str, geom: &Geometry<f64>, expected_count: usize) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    assert!(
        export_geometries_with_crs(&[geom.clone()], path_str, None).is_ok(),
        "Export .{ext} failed"
    );
    let loaded = load_geometries(path_str).unwrap();
    assert_eq!(
        loaded.len(),
        expected_count,
        ".{ext}: expected {expected_count} geometries, got {}",
        loaded.len()
    );
    cleanup_path(&path);
}

/// Export with CRS, load and verify CRS is preserved.
fn roundtrip_crs(ext: &str, geom: &Geometry<f64>, crs: &Crs) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    assert!(
        export_geometries_with_crs(&[geom.clone()], path_str, Some(crs)).is_ok(),
        "Export .{ext} failed"
    );
    let (loaded_geoms, loaded_crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded_geoms.is_empty(), ".{ext}: no geometries loaded");
    assert_eq!(
        loaded_crs.as_ref(),
        Some(crs),
        ".{ext}: CRS mismatch: expected {:?}, got {:?}",
        Some(crs),
        loaded_crs
    );
    cleanup_path(&path);
}

/// Feature roundtrip with attribute and CRS verification.
fn roundtrip_features_with_attrs(ext: &str) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    let stem = PathBuf::from(path_str);

    // SHP DBF doesn't support floats in Numeric fields — use integer
    let ratio_val = if ext == "shp" {
        serde_json::json!(3)
    } else {
        serde_json::json!(3.14)
    };

    let mut props = serde_json::Map::new();
    props.insert("name".into(), serde_json::json!("test"));
    props.insert("count".into(), serde_json::json!(42));
    props.insert("ratio".into(), ratio_val);

    let feature = Feature::with_all(test_geom(), Some(props.clone()), test_crs(), vec![]);

    let result = export_features(&[feature.clone()], path_str);
    assert!(
        result.is_ok(),
        "Export .{ext} features failed: {:?}",
        result
    );
    let loaded = load_features(path_str).unwrap();
    assert!(!loaded.is_empty(), ".{ext}: no features loaded");
    assert_eq!(
        loaded[0].geometry, feature.geometry,
        ".{ext}: geometry mismatch"
    );

    if ext == "geojson" {
        assert_eq!(
            loaded[0].properties.as_ref(),
            Some(&props),
            ".{ext}: properties mismatch"
        );
        assert_eq!(loaded[0].crs, feature.crs, ".{ext}: CRS mismatch");
    }
    // SHP: load_features doesn't return attrs/CRS (uses load_shp_geometries)
    // WKB: binary format, no properties

    if ext == "shp" {
        cleanup_shp(&stem);
    } else {
        cleanup_path(&path);
    }
}

/// Z/M feature roundtrip.
fn roundtrip_zm_feature(ext: &str, feature: &Feature) {
    let path = unique_path(ext);
    let path_str = path.to_str().unwrap();
    assert!(
        export_features(&[feature.clone()], path_str).is_ok(),
        "Export .{ext} ZM feature failed"
    );
    let loaded = load_features(path_str).unwrap();
    assert!(!loaded.is_empty(), ".{ext}: no features loaded");
    assert_eq!(
        loaded[0].zm.len(),
        feature.zm.len(),
        ".{ext}: ZM count mismatch: expected {}, got {}",
        feature.zm.len(),
        loaded[0].zm.len()
    );

    for (i, (lz, oz)) in loaded[0].zm.iter().zip(feature.zm.iter()).enumerate() {
        assert_eq!(
            lz.z, oz.z,
            ".{ext}: Z mismatch at index {i}: expected {:?}, got {:?}",
            oz.z, lz.z
        );
        assert_eq!(
            lz.m, oz.m,
            ".{ext}: M mismatch at index {i}: expected {:?}, got {:?}",
            oz.m, lz.m
        );
    }
    cleanup_path(&path);
}

// ==============================================================
// Geometry type coverage: Multi-geometries
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_multipoint() {
    roundtrip_geom_eq("geojson", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_multilinestring() {
    roundtrip_geom_eq("geojson", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_multipolygon() {
    roundtrip_geom_eq("geojson", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_geometrycollection() {
    roundtrip_geom_eq("geojson", &test_geometrycollection(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_multipoint() {
    roundtrip_geom_eq("wkt", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_multilinestring() {
    roundtrip_geom_eq("wkt", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_multipolygon() {
    roundtrip_geom_eq("wkt", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_geometrycollection() {
    // WKT splits GC into children
    roundtrip_gc("wkt", &test_geometrycollection(), 3);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_multipoint() {
    roundtrip_geom_eq("wkb", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_multilinestring() {
    roundtrip_geom_eq("wkb", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_multipolygon() {
    roundtrip_geom_eq("wkb", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_geometrycollection() {
    // WKB splits GC into children
    roundtrip_gc("wkb", &test_geometrycollection(), 3);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_multipoint() {
    roundtrip_geom_eq("csv", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_multilinestring() {
    roundtrip_geom_eq("csv", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_multipolygon() {
    roundtrip_geom_eq("csv", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_geometrycollection() {
    // CSV loads first geometry from multi-row files
    roundtrip_gc("csv", &test_geometrycollection(), 1);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_multipoint() {
    roundtrip_geom_eq("gpkg", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_multilinestring() {
    roundtrip_geom_eq("gpkg", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_multipolygon() {
    roundtrip_geom_eq("gpkg", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_geometrycollection() {
    roundtrip_geom_eq("gpkg", &test_geometrycollection(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_multipoint() {
    roundtrip_geom_eq("gml", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_multilinestring() {
    roundtrip_geom_eq("gml", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_multipolygon() {
    roundtrip_geom_eq("gml", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_geometrycollection() {
    roundtrip_geom_eq("gml", &test_geometrycollection(), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_multipoint() {
    roundtrip_geom_eq("kml", &test_multipoint(), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_multilinestring() {
    roundtrip_geom_eq("kml", &test_multilinestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_multipolygon() {
    roundtrip_geom_eq("kml", &test_multipolygon(), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_geometrycollection() {
    roundtrip_geom_eq("kml", &test_geometrycollection(), None);
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_multipoint() {
    // SHP flattens MultiPoint into individual Point records
    roundtrip_geom_lossy("shp", &test_multipoint());
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_multipolygon() {
    // SHP flattens MultiPolygon into individual Polygon records
    roundtrip_geom_lossy("shp", &test_multipolygon());
}

// ==============================================================
// Geometry type coverage: Line, Triangle, Rect (lossy types)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_lossy_types() {
    roundtrip_geom_lossy("geojson", &test_line());
    roundtrip_geom_lossy("geojson", &test_triangle());
    roundtrip_geom_lossy("geojson", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_lossy_types() {
    roundtrip_geom_lossy("wkt", &test_line());
    roundtrip_geom_lossy("wkt", &test_triangle());
    roundtrip_geom_lossy("wkt", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_lossy_types() {
    roundtrip_geom_lossy("wkb", &test_line());
    roundtrip_geom_lossy("wkb", &test_triangle());
    roundtrip_geom_lossy("wkb", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_lossy_types() {
    roundtrip_geom_lossy("csv", &test_line());
    roundtrip_geom_lossy("csv", &test_triangle());
    roundtrip_geom_lossy("csv", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_lossy_types() {
    roundtrip_geom_lossy("gpkg", &test_line());
    roundtrip_geom_lossy("gpkg", &test_triangle());
    roundtrip_geom_lossy("gpkg", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_lossy_types() {
    roundtrip_geom_lossy("gml", &test_line());
    roundtrip_geom_lossy("gml", &test_triangle());
    roundtrip_geom_lossy("gml", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_lossy_types() {
    roundtrip_geom_lossy("kml", &test_line());
    roundtrip_geom_lossy("kml", &test_triangle());
    roundtrip_geom_lossy("kml", &test_rect());
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_lossy_types() {
    roundtrip_geom_lossy("shp", &test_line());
    roundtrip_geom_lossy("shp", &test_rect());
    // Triangle: SHP doesn't support, will become Polygon
    roundtrip_geom_lossy("shp", &test_triangle());
}

// ==============================================================
// Geometry type coverage: LineString remains lossless
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_linestring() {
    roundtrip_geom_eq("geojson", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_linestring() {
    roundtrip_geom_eq("wkt", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_linestring() {
    roundtrip_geom_eq("wkb", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_linestring() {
    roundtrip_geom_eq("csv", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_linestring() {
    roundtrip_geom_eq("gpkg", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_linestring() {
    roundtrip_geom_eq("gml", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_linestring() {
    roundtrip_geom_eq("kml", &test_linestring(), None);
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_linestring() {
    roundtrip_geom("shp", &test_linestring(), None);
}

// ==============================================================
// Z/M coordinate preservation (geojson, wkb)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_zm_point_z() {
    roundtrip_zm_feature("geojson", &test_point_z());
}

// GeoJSON doesn't preserve M-only state — missing Z becomes Some(0.0)
// in the coordinate array. Only Z-only and ZM are tested.
#[cfg(feature = "io-geojson")]
#[test]
fn rt_geojson_zm_point_m_roundtrips_as_zm() {
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    // M-only feature: write [x, y, 0, 200], read back as z=0,m=200
    let feature = test_point_m();
    assert!(export_features(&[feature.clone()], path_str).is_ok());
    let loaded = load_features(path_str).unwrap();
    assert!(!loaded.is_empty());
    // GeoJSON encodes M-only as [x, y, 0, m], so z becomes Some(0.0)
    assert_eq!(loaded[0].zm[0].z, Some(0.0));
    assert_eq!(loaded[0].zm[0].m, Some(200.0));
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_zm_point_zm() {
    roundtrip_zm_feature("geojson", &test_point_zm());
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_zm_polygon_z() {
    roundtrip_zm_feature("geojson", &test_polygon_z());
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_zm_point_z() {
    roundtrip_zm_feature("wkb", &test_point_z());
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_zm_point_m() {
    roundtrip_zm_feature("wkb", &test_point_m());
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_zm_point_zm() {
    roundtrip_zm_feature("wkb", &test_point_zm());
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_zm_polygon_z() {
    roundtrip_zm_feature("wkb", &test_polygon_z());
}

// ==============================================================
// CRS roundtrip
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_crs() {
    roundtrip_crs("geojson", &test_point(), &Crs::from_epsg(4326));
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_crs() {
    roundtrip_crs("wkt", &test_point(), &Crs::from_epsg(4326));
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_crs() {
    roundtrip_crs("wkb", &test_point(), &Crs::from_epsg(4326));
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_crs() {
    roundtrip_crs("shp", &test_point(), &Crs::from_epsg(4326));
}

// ==============================================================
// Feature/attribute roundtrips
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_features_attrs() {
    roundtrip_features_with_attrs("geojson");
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_features_attrs() {
    roundtrip_features_with_attrs("wkb");
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_features_attrs() {
    roundtrip_features_with_attrs("shp");
}

// ==============================================================
// Error cases
// ==============================================================

#[test]
fn rt_error_missing_file() {
    let result = load_geometries("nonexistent_file_xyzzy.shp");
    assert!(result.is_err());
}

#[test]
fn rt_error_unsupported_format() {
    let result = load_geometries("test.xyz");
    assert!(result.is_err());
}

// ==============================================================
// Cross-format tests
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_cross_geojson_json() {
    // Writing with .json extension, reading as .json
    let path = unique_path("json");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_point()], path_str, None).is_ok());
    let loaded = load_geometries(path_str).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], test_point());
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(all(feature = "io-wkt", feature = "io-csv")), ignore)]
fn rt_cross_wkt_csv() {
    // Write as WKT, read as CSV (both WKT-based)
    let path = unique_path("csv");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_geom()], path_str, None).is_ok());
    let loaded = load_geometries(path_str).unwrap();
    assert_eq!(loaded.len(), 1);
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_cross_gml_xml() {
    // Writing with .xml extension should be treated as GML
    let path = unique_path("xml");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_point()], path_str, None).is_ok());
    let loaded = load_geometries(path_str).unwrap();
    assert_eq!(loaded.len(), 1);
    cleanup_path(&path);
}

// ==============================================================
// Edge cases
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_large_coordinates() {
    roundtrip_geom_eq("geojson", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_large_coordinates_wkt() {
    roundtrip_geom_eq("wkt", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_large_coordinates_wkb() {
    roundtrip_geom_eq("wkb", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_many_coords() {
    roundtrip_geom_lossy("geojson", &test_many_coords());
}

// ==============================================================
// Multi-geometry export + features roundtrip (GeoJSON)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_multi_features() {
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();

    let f1 = Feature::with_all(
        test_multipoint(),
        Some({
            let mut m = serde_json::Map::new();
            m.insert("id".into(), serde_json::json!(1));
            m
        }),
        test_crs(),
        vec![],
    );
    let f2 = Feature::with_all(
        test_multipolygon(),
        Some({
            let mut m = serde_json::Map::new();
            m.insert("id".into(), serde_json::json!(2));
            m
        }),
        test_crs(),
        vec![],
    );

    assert!(export_features(&[f1.clone(), f2.clone()], path_str).is_ok());
    let loaded = load_features(path_str).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].geometry, f1.geometry);
    assert_eq!(loaded[1].geometry, f2.geometry);
    assert_eq!(loaded[0].properties, f1.properties);
    assert_eq!(loaded[1].properties, f2.properties);
    assert_eq!(loaded[0].crs, f1.crs);

    cleanup_path(&path);
}

// ==============================================================
// Multi-geometry: SHP MultiLineString (missing coverage)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_multilinestring() {
    roundtrip_geom_lossy("shp", &test_multilinestring());
}

// ==============================================================
// CRS export for GPKG and GML (export doesn't crash, CRS may be lost on load)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_crs_export() {
    let path = unique_path("gpkg");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_point()], path_str, test_crs().as_ref()).is_ok());
    let (loaded, _crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded.is_empty(), ".gpkg: no geometries loaded");
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_crs_export() {
    let path = unique_path("gml");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_point()], path_str, test_crs().as_ref()).is_ok());
    let (loaded, _crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded.is_empty(), ".gml: no geometries loaded");
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_crs_export() {
    let path = unique_path("kml");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_point()], path_str, test_crs().as_ref()).is_ok());
    let (loaded, _crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded.is_empty(), ".kml: no geometries loaded");
    cleanup_path(&path);
}

// ==============================================================
// Error cases: corrupted and empty files
// ==============================================================

#[test]
fn rt_error_corrupted_file() {
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "not valid geojson content @#$%").unwrap();
    let result = load_geometries(path_str);
    assert!(
        result.is_err(),
        "Expected error for corrupted .geojson file"
    );
    cleanup_path(&path);
}

#[test]
fn rt_error_empty_file() {
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for empty .geojson file");
    cleanup_path(&path);
}

#[test]
fn rt_error_corrupted_wkt() {
    let path = unique_path("wkt");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "NOT_A_VALID_WKT_STRING").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for corrupted .wkt file");
    cleanup_path(&path);
}

// ==============================================================
// Edge cases: large coordinates for all format families
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_edge_large_coordinates_csv() {
    roundtrip_geom("csv", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_edge_large_coordinates_gpkg() {
    roundtrip_geom("gpkg", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_edge_large_coordinates_gml() {
    roundtrip_geom("gml", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_edge_large_coordinates_kml() {
    roundtrip_geom("kml", &test_large_coords(), None);
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_edge_large_coordinates_shp() {
    roundtrip_geom("shp", &test_large_coords(), None);
}

// ==============================================================
// Edge cases: many coords for additional formats
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_many_coords_wkt() {
    roundtrip_geom_lossy("wkt", &test_many_coords());
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_many_coords_wkb() {
    roundtrip_geom_lossy("wkb", &test_many_coords());
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_edge_many_coords_csv() {
    roundtrip_geom_lossy("csv", &test_many_coords());
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_edge_many_coords_gpkg() {
    roundtrip_geom_lossy("gpkg", &test_many_coords());
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_edge_many_coords_gml() {
    roundtrip_geom_lossy("gml", &test_many_coords());
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_edge_many_coords_kml() {
    roundtrip_geom_lossy("kml", &test_many_coords());
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_edge_many_coords_shp() {
    roundtrip_geom_lossy("shp", &test_many_coords());
}

// ==============================================================
// Edge case: empty GeometryCollection
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_empty_geometrycollection_geojson() {
    let gc = Geometry::GeometryCollection(GeometryCollection(vec![]));
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[gc], path_str, None).is_ok());
    let loaded = load_geometries(path_str).unwrap();
    // GeoJSON may emit {"geometries":[]} — at minimum it should not panic
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_edge_empty_geometrycollection_gpkg() {
    let gc = Geometry::GeometryCollection(GeometryCollection(vec![]));
    let path = unique_path("gpkg");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[gc], path_str, None);
    // GPKG may or may not support empty GC — at minimum should not panic
    if let Ok(()) = result {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_edge_empty_geometrycollection_kml() {
    let gc = Geometry::GeometryCollection(GeometryCollection(vec![]));
    let path = unique_path("kml");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[gc], path_str, None);
    if let Ok(()) = result {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_edge_empty_geometrycollection_gml() {
    let gc = Geometry::GeometryCollection(GeometryCollection(vec![]));
    let path = unique_path("gml");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[gc], path_str, None);
    if let Ok(()) = result {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

// ==============================================================
// Edge case: NaN coordinate handling in formats that don't reject NaN
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_nan_coord_wkt() {
    // WKT should handle NaN coordinates gracefully
    let nan_point = Geometry::Point(Point::new(f64::NAN, 0.0));
    let path = unique_path("wkt");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[nan_point], path_str, None);
    // May or may not succeed; at minimum should not panic
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_nan_coord_wkb() {
    let nan_point = Geometry::Point(Point::new(f64::NAN, 0.0));
    let path = unique_path("wkb");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[nan_point], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

// ==============================================================
// Additional cross-format: .json ↔ .geojson symmetry
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_cross_json_geojson_symmetry() {
    // Export as .json, load as .geojson
    let json_path = unique_path("json");
    let json_str = json_path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_geom()], json_str, None).is_ok());

    let geojson_path = PathBuf::from(json_str.replace(".json", ".geojson"));
    std::fs::rename(&json_path, &geojson_path).unwrap();
    let geojson_str = geojson_path.to_str().unwrap();

    let loaded = load_geometries(geojson_str).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], test_geom());
    cleanup_path(&geojson_path);
}

// ==============================================================
// Export with CRS for all formats that support it (validates no crash)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_crs_export_polygon() {
    roundtrip_crs("geojson", &test_geom(), &Crs::from_epsg(4326));
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_crs_export_ignored() {
    // CSV ignores CRS — verify export doesn't crash
    let path = unique_path("csv");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_point()], path_str, test_crs().as_ref()).is_ok());
    let _ = load_geometries(path_str).unwrap();
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_crs_export_polygon() {
    let path = unique_path("gpkg");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_geom()], path_str, test_crs().as_ref()).is_ok());
    let (loaded, _crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded.is_empty());
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_crs_export_polygon() {
    let path = unique_path("gml");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_geom()], path_str, test_crs().as_ref()).is_ok());
    let (loaded, _crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded.is_empty());
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_kml_crs_export_polygon() {
    let path = unique_path("kml");
    let path_str = path.to_str().unwrap();
    assert!(export_geometries_with_crs(&[test_geom()], path_str, test_crs().as_ref()).is_ok());
    let (loaded, _crs) = load_geometries_with_crs(path_str).unwrap();
    assert!(!loaded.is_empty());
    cleanup_path(&path);
}

// ==============================================================
// Edge case: minimal valid inputs
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_minimal_point() {
    // Origin point
    roundtrip_geom_eq("geojson", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_minimal_point_wkt() {
    roundtrip_geom_eq("wkt", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_minimal_point_wkb() {
    roundtrip_geom_eq("wkb", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_minimal_linestring() {
    // Two-point LineString
    let ls = Geometry::LineString(LineString::new(vec![(0.0, 0.0).into(), (1.0, 1.0).into()]));
    roundtrip_geom_eq("geojson", &ls, None);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_negative_coords() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("geojson", &neg, None);
}

// ==============================================================
// WKB GeometryCollection via features (wraps as GC, extracts children)
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_geometrycollection_via_features() {
    let path = unique_path("wkb");
    let path_str = path.to_str().unwrap();

    let gc = test_geometrycollection();
    let f = Feature::with_all(gc.clone(), None, None, vec![]);

    assert!(export_features(&[f.clone()], path_str).is_ok());
    let loaded = load_features(path_str).unwrap();
    // WKB loads GC and extracts children (3 geoms)
    assert_eq!(loaded.len(), 3);
    cleanup_path(&path);
}

// ==============================================================
// Z/M feature roundtrip for features with attributes preserved
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_zm_with_attributes() {
    let mut props = serde_json::Map::new();
    props.insert("label".into(), serde_json::json!("Z"));
    props.insert("height".into(), serde_json::json!(100.0));

    let feature = Feature::with_all(
        Geometry::Point(Point::new(1.0, 2.0)),
        Some(props.clone()),
        None,
        vec![ZmValue::new(Some(50.0), Some(60.0))],
    );

    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    assert!(export_features(&[feature.clone()], path_str).is_ok());
    let loaded = load_features(path_str).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].geometry, feature.geometry);
    assert_eq!(loaded[0].properties.as_ref(), Some(&props));
    assert_eq!(loaded[0].zm.len(), 1);
    assert_eq!(loaded[0].zm[0].z, Some(50.0));
    assert_eq!(loaded[0].zm[0].m, Some(60.0));
    cleanup_path(&path);
}

// ==============================================================
// Polygon factory with interior ring
// ==============================================================

fn test_polygon_hole() -> Geometry<f64> {
    Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            (0.0, 0.0).into(),
            (10.0, 0.0).into(),
            (10.0, 10.0).into(),
            (0.0, 10.0).into(),
            (0.0, 0.0).into(),
        ]),
        vec![LineString::new(vec![
            (2.0, 2.0).into(),
            (2.0, 8.0).into(),
            (8.0, 8.0).into(),
            (8.0, 2.0).into(),
            (2.0, 2.0).into(),
        ])],
    ))
}

// ==============================================================
// Polygon with hole
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_polygon_hole() {
    roundtrip_geom_eq("geojson", &test_polygon_hole(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_polygon_hole() {
    roundtrip_geom_eq("wkt", &test_polygon_hole(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_polygon_hole() {
    roundtrip_geom_eq("wkb", &test_polygon_hole(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_polygon_hole() {
    roundtrip_geom_eq("gpkg", &test_polygon_hole(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_polygon_hole() {
    roundtrip_geom_eq("gml", &test_polygon_hole(), None);
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_shp_polygon_hole() {
    roundtrip_geom("shp", &test_polygon_hole(), None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_polygon_hole() {
    roundtrip_geom("csv", &test_polygon_hole(), None);
}

// ==============================================================
// CSV/GPKG/GML features with attributes
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_csv_features_attrs() {
    roundtrip_features_with_attrs("csv");
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_features_attrs() {
    roundtrip_features_with_attrs("gpkg");
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_features_attrs() {
    roundtrip_features_with_attrs("gml");
}

// ==============================================================
// Empty geometry types
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_empty_point() {
    let empty = Geometry::Point(Point::new(f64::NAN, f64::NAN));
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    // Should succeed or fail gracefully; at minimum should not panic
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_empty_linestring() {
    let empty = Geometry::LineString(LineString::new(vec![]));
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_empty_multipoint() {
    let empty = Geometry::MultiPoint(MultiPoint::new(vec![]));
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_empty_multipolygon() {
    let empty = Geometry::MultiPolygon(MultiPolygon::new(vec![]));
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_empty_linestring() {
    let empty = Geometry::LineString(LineString::new(vec![]));
    let path = unique_path("wkt");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_empty_linestring() {
    let empty = Geometry::LineString(LineString::new(vec![]));
    let path = unique_path("wkb");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_empty_multipoint() {
    let empty = Geometry::MultiPoint(MultiPoint::new(vec![]));
    let path = unique_path("gpkg");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[empty], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

// ==============================================================
// Nested GeometryCollection factory
// ==============================================================

fn test_nested_gc() -> Geometry<f64> {
    Geometry::GeometryCollection(GeometryCollection(vec![Geometry::GeometryCollection(
        GeometryCollection(vec![Geometry::Point(Point::new(1.0, 2.0))]),
    )]))
}

// ==============================================================
// Nested GeometryCollection
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_geojson_nested_gc() {
    roundtrip_geom_eq("geojson", &test_nested_gc(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_gml_nested_gc() {
    // GML flattens nested GC into a single GC
    roundtrip_geom("gml", &test_nested_gc(), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_gpkg_nested_gc() {
    roundtrip_geom_eq("gpkg", &test_nested_gc(), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_wkt_nested_gc() {
    // WKT splits GC into children
    roundtrip_gc("wkt", &test_nested_gc(), 1);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_nested_gc() {
    // WKB splits GC into children
    roundtrip_gc("wkb", &test_nested_gc(), 1);
}

// ==============================================================
// Multi-feature WKB
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_wkb_multi_features() {
    let path = unique_path("wkb");
    let path_str = path.to_str().unwrap();

    let f1 = Feature::with_all(Geometry::Point(Point::new(1.0, 2.0)), None, None, vec![]);
    let f2 = Feature::with_all(test_geom(), None, None, vec![]);

    assert!(export_features(&[f1.clone(), f2.clone()], path_str).is_ok());
    let loaded = load_features(path_str).unwrap();
    // WKB binary format may not support multi-feature; at least 1 geometry loads
    assert!(!loaded.is_empty(), "WKB: expected at least 1 feature");
    assert_eq!(loaded[0].geometry, f1.geometry);
    cleanup_path(&path);
}

// ==============================================================
// Infinity coordinates
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_infinity_wkt() {
    let inf = Geometry::Point(Point::new(f64::INFINITY, 0.0));
    let path = unique_path("wkt");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[inf], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_infinity_wkb() {
    let inf = Geometry::Point(Point::new(f64::INFINITY, 0.0));
    let path = unique_path("wkb");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[inf], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

#[test]
#[cfg_attr(not(feature = "io-geojson"), ignore)]
fn rt_edge_neg_infinity_geojson() {
    let inf = Geometry::Point(Point::new(f64::NEG_INFINITY, 0.0));
    let path = unique_path("geojson");
    let path_str = path.to_str().unwrap();
    let result = export_geometries_with_crs(&[inf], path_str, None);
    if result.is_ok() {
        let _ = load_geometries(path_str);
    }
    cleanup_path(&path);
}

// ==============================================================
// Minimal inputs for remaining formats
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_edge_minimal_point_csv() {
    roundtrip_geom_eq("csv", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_edge_minimal_point_gpkg() {
    roundtrip_geom_eq("gpkg", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_edge_minimal_point_gml() {
    roundtrip_geom_eq("gml", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_edge_minimal_point_shp() {
    roundtrip_geom("shp", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_minimal_linestring_wkt() {
    let ls = Geometry::LineString(LineString::new(vec![(0.0, 0.0).into(), (1.0, 1.0).into()]));
    roundtrip_geom_eq("wkt", &ls, None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_minimal_linestring_wkb() {
    let ls = Geometry::LineString(LineString::new(vec![(0.0, 0.0).into(), (1.0, 1.0).into()]));
    roundtrip_geom_eq("wkb", &ls, None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_edge_minimal_linestring_csv() {
    let ls = Geometry::LineString(LineString::new(vec![(0.0, 0.0).into(), (1.0, 1.0).into()]));
    roundtrip_geom_eq("csv", &ls, None);
}

// ==============================================================
// Negative coordinates for other formats
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-wkt"), ignore)]
fn rt_edge_negative_wkt() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("wkt", &neg, None);
}

#[test]
#[cfg_attr(not(feature = "io-wkb"), ignore)]
fn rt_edge_negative_wkb() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("wkb", &neg, None);
}

#[test]
#[cfg_attr(not(feature = "io-csv"), ignore)]
fn rt_edge_negative_csv() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("csv", &neg, None);
}

#[test]
#[cfg_attr(not(feature = "io-gpkg"), ignore)]
fn rt_edge_negative_gpkg() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("gpkg", &neg, None);
}

#[test]
#[cfg_attr(not(feature = "io-gml"), ignore)]
fn rt_edge_negative_gml() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("gml", &neg, None);
}

#[test]
#[cfg_attr(not(feature = "load-shp"), ignore)]
fn rt_edge_negative_shp() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom("shp", &neg, None);
}

// ==============================================================
// Corrupted files per format
// ==============================================================

#[test]
fn rt_error_corrupted_wkb() {
    let path = unique_path("wkb");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, b"NOT VALID WKB\x00\x01\x02").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for corrupted .wkb file");
    cleanup_path(&path);
}

#[test]
fn rt_error_corrupted_csv() {
    let path = unique_path("csv");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "NOT A CSV").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for corrupted .csv file");
    cleanup_path(&path);
}

#[test]
fn rt_error_corrupted_gpkg() {
    let path = unique_path("gpkg");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "").unwrap();
    let result = load_geometries(path_str);
    assert!(
        result.is_err(),
        "Expected error for empty/corrupted .gpkg file"
    );
    cleanup_path(&path);
}

#[test]
fn rt_error_corrupted_gml() {
    let path = unique_path("gml");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "<not><valid></gml>").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for corrupted .gml file");
    cleanup_path(&path);
}

#[test]
fn rt_error_corrupted_shp() {
    let path = unique_path("shp");
    let path_str = path.to_str().unwrap();
    let stem = PathBuf::from(path_str);
    std::fs::write(&path, b"not a shapefile").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for corrupted .shp file");
    cleanup_shp(&stem);
}

// ==============================================================
// KML-specific edge cases
// ==============================================================

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_edge_minimal_point_kml() {
    roundtrip_geom_eq("kml", &Geometry::Point(Point::new(0.0, 0.0)), None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_edge_minimal_linestring_kml() {
    let ls = Geometry::LineString(LineString::new(vec![(0.0, 0.0).into(), (1.0, 1.0).into()]));
    roundtrip_geom_eq("kml", &ls, None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_edge_negative_kml() {
    let neg = Geometry::Point(Point::new(-180.0, -90.0));
    roundtrip_geom_eq("kml", &neg, None);
}

#[test]
#[cfg_attr(not(feature = "io-kml"), ignore)]
fn rt_error_corrupted_kml() {
    let path = unique_path("kml");
    let path_str = path.to_str().unwrap();
    std::fs::write(&path, "<not><valid></kml>").unwrap();
    let result = load_geometries(path_str);
    assert!(result.is_err(), "Expected error for corrupted .kml file");
    cleanup_path(&path);
}

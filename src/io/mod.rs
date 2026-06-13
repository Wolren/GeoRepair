use std::path::Path;

use geo::Geometry;

use crate::core::MakeValidError;
use crate::feature::Feature;
use crate::Crs;

pub mod binary;
#[cfg(feature = "io-csv")]
pub mod csv;
#[cfg(feature = "io-geojson")]
pub mod geojson;
#[cfg(feature = "io-gpkg")]
pub mod geopackage;
#[cfg(feature = "io-gml")]
pub mod gml;
#[cfg(feature = "io-kml")]
pub mod kml;
pub mod shp;
#[cfg(feature = "io-wkb")]
pub mod wkb;
#[cfg(feature = "io-wkt")]
pub mod wkt;

pub use self::binary::load_bin;
pub use self::binary::load_bin_stream;
#[cfg(feature = "io-csv")]
pub use self::csv::{export_csv_wkt, load_csv_wkt};
#[cfg(feature = "io-geojson")]
pub use self::geojson::{
    export_geojson_rfc7946, export_geojson_with_crs, export_geojson_with_crs_zm, load_geojson,
    load_geojson_features, load_geojson_with_crs, load_geojson_zm,
};
#[cfg(feature = "io-kml")]
pub use self::kml::{export_kml, export_kml_with_crs, load_kml};
pub use self::shp::count_sub_polys;
pub use self::shp::export_geojson;
pub use self::shp::geo_area;
pub use self::shp::polygon_area;
pub use self::shp::signed_area;
#[cfg(feature = "load-shp")]
pub use self::shp::{
    export_shp, export_shp_features, load_shp, load_shp_features, load_shp_geometries,
};
#[cfg(feature = "io-wkb")]
pub use self::wkb::{
    export_wkb, export_wkb_with_crs, export_wkb_zm_with_crs, load_wkb, load_wkb_with_crs,
    load_wkb_zm,
};
#[cfg(feature = "io-wkt")]
pub use self::wkt::{export_wkt, export_wkt_with_crs, load_wkt, load_wkt_with_crs};

// ---------------------------------------------------------------------------
// Format-agnostic loading (geometry only)
// ---------------------------------------------------------------------------

/// Detect format from file extension and load geometries.
/// Supports: .shp, .bin, .geojson/.json, .wkt, .wkb, .csv, .gpkg, .gml/.xml
pub fn load_geometries(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    load_geometries_with_crs(path).map(|(geoms, _)| geoms)
}

/// Like `load_geometries` but also extracts CRS metadata when the
/// format supports it.
pub fn load_geometries_with_crs(
    path: &str,
) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                load_shp_features(path)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))
                    .map(|features| {
                        let crs = features.first().and_then(|f| f.crs.clone());
                        let geoms = features.into_iter().map(|f| f.geometry).collect();
                        (geoms, crs)
                    })
            }
            #[cfg(not(feature = "load-shp"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile support requires 'load-shp' feature".into(),
                ))
            }
        }
        "bin" => {
            let polys = load_bin(path).map_err(|e| MakeValidError::UnsupportedFormat(e))?;
            Ok((polys.into_iter().map(Geometry::Polygon).collect(), None))
        }
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                load_geojson_with_crs(path)
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON support requires 'io-geojson' feature".into(),
                ))
            }
        }
        "wkt" => {
            #[cfg(feature = "io-wkt")]
            {
                load_wkt_with_crs(path)
            }
            #[cfg(not(feature = "io-wkt"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKT support requires 'io-wkt' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                load_wkb_with_crs(path)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKB support requires 'io-wkb' feature".into(),
                ))
            }
        }
        "csv" => {
            #[cfg(feature = "io-csv")]
            {
                let geoms = load_csv_wkt(path)?;
                Ok((geoms, None))
            }
            #[cfg(not(feature = "io-csv"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "CSV WKT support requires 'io-csv' feature".into(),
                ))
            }
        }
        "gpkg" => {
            #[cfg(feature = "io-gpkg")]
            {
                let geoms = geopackage::load_geopackage(path)?;
                Ok((geoms, None))
            }
            #[cfg(not(feature = "io-gpkg"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoPackage support requires 'io-gpkg' feature".into(),
                ))
            }
        }
        "gml" | "xml" => {
            #[cfg(feature = "io-gml")]
            {
                let geoms = gml::load_gml(path)?;
                Ok((geoms, None))
            }
            #[cfg(not(feature = "io-gml"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GML support requires 'io-gml' feature".into(),
                ))
            }
        }
        "kml" => {
            #[cfg(feature = "io-kml")]
            {
                let geoms = kml::load_kml(path)?;
                Ok((geoms, None))
            }
            #[cfg(not(feature = "io-kml"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "KML support requires 'io-kml' feature".into(),
                ))
            }
        }
        _ => Err(MakeValidError::UnsupportedFormat(format!(
            "unsupported format: .{ext} (supported: .shp, .bin, .geojson, .json, .wkt, .wkb, .csv, .gpkg, .gml, .xml, .kml)"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Format-agnostic loading (features with attributes)
// ---------------------------------------------------------------------------

/// Load features (geometry + attributes + CRS + Z/M) from a file.
///
/// Currently supported: .geojson/.json (attributes and Z/M preserved),
/// .wkb (Z/M preserved where available).
pub fn load_features(path: &str) -> Result<Vec<Feature>, MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                geojson::load_geojson_features(path)
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON feature loading requires 'io-geojson' feature".into(),
                ))
            }
        }
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                let geoms = load_shp_geometries(path)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))?;
                Ok(geoms.into_iter().map(|g| Feature::new(g)).collect())
            }
            #[cfg(not(feature = "load-shp"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile feature loading requires 'load-shp' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                let (zm_geoms, crs) = load_wkb_zm(path)?;
                let features: Vec<Feature> = zm_geoms
                    .into_iter()
                    .map(|z| Feature::with_all(z.geometry, None, crs.clone(), z.zm))
                    .collect();
                Ok(features)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKB feature loading requires 'io-wkb' feature".into(),
                ))
            }
        }
        _ => {
            // fallback: load geometries without attributes
            let (geoms, crs) = load_geometries_with_crs(path)?;
            Ok(geoms
                .into_iter()
                .map(|g| Feature::new(g).with_crs(crs.clone()))
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Format-agnostic export
// ---------------------------------------------------------------------------

/// Export geometries to the specified format (geojson, wkt, wkb, shp, gpkg, gml).
pub fn export_geometries(geoms: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    export_geometries_with_crs(geoms, path, None)
}

/// Export geometries with optional CRS metadata.
pub fn export_geometries_with_crs(
    geoms: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                geojson::export_geojson_with_crs(geoms, path, crs)
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON export requires 'io-geojson' feature".into(),
                ))
            }
        }
        "wkt" => {
            #[cfg(feature = "io-wkt")]
            {
                export_wkt_with_crs(geoms, path, crs)
            }
            #[cfg(not(feature = "io-wkt"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKT export requires 'io-wkt' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                export_wkb_with_crs(geoms, path, crs)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKB export requires 'io-wkb' feature".into(),
                ))
            }
        }
        "gpkg" => {
            #[cfg(feature = "io-gpkg")]
            {
                geopackage::export_geopackage(geoms, path, crs)
            }
            #[cfg(not(feature = "io-gpkg"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoPackage export requires 'io-gpkg' feature".into(),
                ))
            }
        }
        "gml" | "xml" => {
            #[cfg(feature = "io-gml")]
            {
                gml::export_gml_with_crs(geoms, path, crs)
            }
            #[cfg(not(feature = "io-gml"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GML export requires 'io-gml' feature".into(),
                ))
            }
        }
        "kml" => {
            #[cfg(feature = "io-kml")]
            {
                kml::export_kml_with_crs(geoms, path, crs)
            }
            #[cfg(not(feature = "io-kml"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "KML export requires 'io-kml' feature".into(),
                ))
            }
        }
        "csv" => {
            #[cfg(feature = "io-csv")]
            {
                export_csv_wkt(geoms, path)
            }
            #[cfg(not(feature = "io-csv"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "CSV export requires 'io-csv' feature".into(),
                ))
            }
        }
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                shp::export_shp(geoms, path, crs)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))
            }
            #[cfg(not(feature = "load-shp"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile export requires 'load-shp' feature".into(),
                ))
            }
        }
        _ => Err(MakeValidError::UnsupportedFormat(format!(
            "unsupported export format: .{ext} (supported: .geojson, .json, .wkt, .wkb, .csv, .shp, .gpkg, .gml, .xml, .kml)"
        ))),
    }
}

/// Export features (with attributes and Z/M) to the specified format.
pub fn export_features(features: &[Feature], path: &str) -> Result<(), MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => {
            #[cfg(feature = "io-geojson")]
            {
                geojson::export_features(path, features)
            }
            #[cfg(not(feature = "io-geojson"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "GeoJSON export requires 'io-geojson' feature".into(),
                ))
            }
        }
        "shp" => {
            #[cfg(feature = "load-shp")]
            {
                let crs = features.first().and_then(|f| f.crs.as_ref());
                shp::export_shp_features(features, path, crs)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))
            }
            #[cfg(not(feature = "load-shp"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "shapefile export requires 'load-shp' feature".into(),
                ))
            }
        }
        "wkb" => {
            #[cfg(feature = "io-wkb")]
            {
                let zm_geoms: Vec<_> = features
                    .iter()
                    .map(|f| crate::zm::ZmGeometry::with_zm(f.geometry.clone(), f.zm.clone()))
                    .collect();
                let crs = features.first().and_then(|f| f.crs.as_ref());
                export_wkb_zm_with_crs(&zm_geoms, path, crs)
            }
            #[cfg(not(feature = "io-wkb"))]
            {
                Err(MakeValidError::UnsupportedFormat(
                    "WKB export requires 'io-wkb' feature".into(),
                ))
            }
        }
        _ => {
            // Fallback: strip attributes and use geometry-only export
            let geoms: Vec<Geometry<f64>> = features.iter().map(|f| f.geometry.clone()).collect();
            let crs = features.first().and_then(|f| f.crs.as_ref());
            export_geometries_with_crs(&geoms, path, crs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_format_geojson() {
        let ext = Path::new("test.geojson")
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        assert_eq!(ext, Some("geojson".to_string()));
    }

    #[test]
    fn test_detect_format_json() {
        let ext = Path::new("test.json")
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        assert_eq!(ext, Some("json".to_string()));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_geometries("nonexistent_file_for_testing.shp");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_geometries_unsupported_format() {
        let result = load_geometries("test.xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_polygon_area_util() {
        let poly = geo::Polygon::new(
            geo::LineString::new(vec![
                geo::Coord { x: 0.0, y: 0.0 },
                geo::Coord { x: 10.0, y: 0.0 },
                geo::Coord { x: 10.0, y: 10.0 },
                geo::Coord { x: 0.0, y: 10.0 },
                geo::Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!((polygon_area(&poly) - 100.0).abs() < 1e-12);
    }

    #[test]
    #[cfg(feature = "io-wkt")]
    fn test_wkt_roundtrip() {
        let geom = Geometry::Point(geo::Point::new(1.0, 2.0));
        let path = std::env::temp_dir().join("test_roundtrip.wkt");
        let path_str = path.to_str().unwrap().to_string();

        let result = export_geometries(&[geom.clone()], &path_str);
        assert!(result.is_ok());

        let loaded = load_geometries(&path_str);
        assert!(loaded.is_ok());
        let loaded_geoms = loaded.unwrap();
        assert_eq!(loaded_geoms.len(), 1);
        assert_eq!(loaded_geoms[0], geom);

        let _ = std::fs::remove_file(&path);
    }
}

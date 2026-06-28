use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

use geo::{Coord, Geometry, LineString, Polygon};
use geojson::GeometryValue;
use geojson::{Feature, FeatureCollection, GeoJson};
use serde_json::{Map, Value};

use crate::core::MakeValidError;
use crate::feature::Feature as GeoRepairFeature;
use crate::zm::{count_coords, ZmGeometry, ZmValue};

pub fn load_geojson(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    load_geojson_with_crs(path).map(|(geoms, _)| geoms)
}

pub fn load_geojson_with_crs(
    path: &str,
) -> Result<(Vec<Geometry<f64>>, Option<crate::Crs>), MakeValidError> {
    use crate::Crs;

    let file = File::open(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let reader = BufReader::new(file);
    let geojson: GeoJson =
        serde_json::from_reader(reader).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let mut geometries = Vec::new();
    let mut crs: Option<Crs> = None;

    match geojson {
        GeoJson::FeatureCollection(collection) => {
            crs = extract_crs_from_foreign(&collection.foreign_members);
            for mut feature in collection.features {
                if let Some(geom) = feature.geometry.take() {
                    let geo_geom = convert_geojson_geometry(geom, &feature);
                    if let Ok(geo_geom) = geo_geom {
                        geometries.push(geo_geom);
                    }
                }
            }
        }
        GeoJson::Feature(mut feature) => {
            if let Some(geom) = feature.geometry.take() {
                let geo_geom = convert_geojson_geometry(geom, &feature);
                if let Ok(geo_geom) = geo_geom {
                    geometries.push(geo_geom);
                }
            }
        }
        GeoJson::Geometry(geom) => {
            if let Ok(geo_geom) = convert_geojson_geometry(geom, &Feature::default()) {
                geometries.push(geo_geom);
            }
        }
    }

    Ok((geometries, crs))
}

pub fn load_geojson_features(path: &str) -> Result<Vec<GeoRepairFeature>, MakeValidError> {
    let file = File::open(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let reader = BufReader::new(file);
    let geojson: GeoJson =
        serde_json::from_reader(reader).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let mut features = Vec::new();
    let fc_crs: Option<crate::Crs>;

    match geojson {
        GeoJson::FeatureCollection(collection) => {
            fc_crs = extract_crs_from_foreign(&collection.foreign_members);
            for mut feature in collection.features {
                if let Some(gj_geom) = feature.geometry.take() && let Ok(zm) = convert_geojson_zm(gj_geom) {
                    let f = GeoRepairFeature::with_all(
                        zm.geometry,
                        feature.properties.take(),
                        fc_crs.clone(),
                        zm.zm,
                    );
                    features.push(f);
                }
            }
        }
        GeoJson::Feature(mut feature) => {
            if let Some(gj_geom) = feature.geometry.take() && let Ok(zm) = convert_geojson_zm(gj_geom) {
                let f = GeoRepairFeature::with_all(
                    zm.geometry,
                    feature.properties.take(),
                    None,
                    zm.zm,
                );
                features.push(f);
            }
        }
        GeoJson::Geometry(geom) => {
            if let Ok(zm) = convert_geojson_zm(geom) {
                let f = GeoRepairFeature::with_all(zm.geometry, None, None, zm.zm);
                features.push(f);
            }
        }
    }

    Ok(features)
}

pub fn load_geojson_zm(
    path: &str,
) -> Result<(Vec<ZmGeometry>, Option<crate::Crs>), MakeValidError> {
    let file = File::open(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let reader = BufReader::new(file);
    let geojson: GeoJson =
        serde_json::from_reader(reader).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let mut zm_geoms = Vec::new();
    let mut crs: Option<crate::Crs> = None;

    match geojson {
        GeoJson::FeatureCollection(collection) => {
            crs = extract_crs_from_foreign(&collection.foreign_members);
            for mut feature in collection.features {
                if let Some(geom) = feature.geometry.take() && let Ok(zm) = convert_geojson_zm(geom) {
                    zm_geoms.push(zm);
                }
            }
        }
        GeoJson::Feature(mut feature) => {
            if let Some(geom) = feature.geometry.take() && let Ok(zm) = convert_geojson_zm(geom) {
                zm_geoms.push(zm);
            }
        }
        GeoJson::Geometry(geom) => {
            if let Ok(zm) = convert_geojson_zm(geom) {
                zm_geoms.push(zm);
            }
        }
    }

    Ok((zm_geoms, crs))
}

fn extract_crs_from_foreign(foreign: &Option<Map<String, Value>>) -> Option<crate::Crs> {
    let members = foreign.as_ref()?;
    if let Some(crs_val) = members.get("crs")
        && let Some(props) = crs_val.as_object()
        && let Some(Value::String(typ)) = props.get("type")
        && typ == "name"
        && let Some(props_obj) = props.get("properties").and_then(|v| v.as_object())
        && let Some(Value::String(name)) = props_obj.get("name")
    {
        return Some(crate::Crs::from_authority(name));
    }
    None
}

pub(crate) fn convert_geojson_zm(geom: geojson::Geometry) -> Result<ZmGeometry, MakeValidError> {
    fn coord_zm(pos: &geojson::Position) -> (Coord<f64>, ZmValue) {
        (
            Coord {
                x: pos[0],
                y: pos[1],
            },
            ZmValue::new(
                pos.as_slice().get(2).copied(),
                pos.as_slice().get(3).copied(),
            ),
        )
    }

    fn ring_zm(coords: &[geojson::Position]) -> (LineString<f64>, Vec<ZmValue>) {
        let mut zms = Vec::with_capacity(coords.len());
        let ls = LineString::new(
            coords
                .iter()
                .map(|c| {
                    let (coord, zv) = coord_zm(c);
                    zms.push(zv);
                    coord
                })
                .collect(),
        );
        (ls, zms)
    }

    fn polygon_zm(coords: &[Vec<geojson::Position>]) -> (Polygon<f64>, Vec<ZmValue>) {
        let mut all_zm = Vec::new();
        let mut rings: Vec<LineString<f64>> = Vec::with_capacity(coords.len());
        for r in coords {
            let (ls, zm) = ring_zm(r);
            all_zm.extend(zm);
            rings.push(ls);
        }
        let mut iter = rings.into_iter();
        let exterior = iter.next().unwrap_or_else(|| LineString::new(Vec::new()));
        let interiors: Vec<_> = iter.collect();
        (Polygon::new(exterior, interiors), all_zm)
    }

    match geom.value {
        GeometryValue::Point { coordinates, .. } => {
            let (c, zv) = coord_zm(&coordinates);
            Ok(ZmGeometry::with_zm(
                Geometry::Point(geo::Point(c)),
                vec![zv],
            ))
        }
        GeometryValue::MultiPoint { coordinates, .. } => {
            let mut zms = Vec::with_capacity(coordinates.len());
            let pts: Vec<geo::Point<f64>> = coordinates
                .iter()
                .map(|c| {
                    let (c, zv) = coord_zm(c);
                    zms.push(zv);
                    geo::Point(c)
                })
                .collect();
            Ok(ZmGeometry::with_zm(
                Geometry::MultiPoint(geo::MultiPoint::new(pts)),
                zms,
            ))
        }
        GeometryValue::LineString { coordinates, .. } => {
            let (ls, zms) = ring_zm(&coordinates);
            Ok(ZmGeometry::with_zm(Geometry::LineString(ls), zms))
        }
        GeometryValue::MultiLineString { coordinates, .. } => {
            let mut all_zm = Vec::new();
            let mls: Vec<LineString<f64>> = coordinates
                .iter()
                .map(|l| {
                    let (ls, zm) = ring_zm(l);
                    all_zm.extend(zm);
                    ls
                })
                .collect();
            Ok(ZmGeometry::with_zm(
                Geometry::MultiLineString(geo::MultiLineString::new(mls)),
                all_zm,
            ))
        }
        GeometryValue::Polygon { coordinates, .. } => {
            let (poly, zms) = polygon_zm(&coordinates);
            Ok(ZmGeometry::with_zm(Geometry::Polygon(poly), zms))
        }
        GeometryValue::MultiPolygon { coordinates, .. } => {
            let mut all_zm = Vec::new();
            let polys: Vec<Polygon<f64>> = coordinates
                .iter()
                .map(|p| {
                    let (poly, zm) = polygon_zm(p);
                    all_zm.extend(zm);
                    poly
                })
                .collect();
            Ok(ZmGeometry::with_zm(
                Geometry::MultiPolygon(geo::MultiPolygon::new(polys)),
                all_zm,
            ))
        }
        GeometryValue::GeometryCollection { geometries, .. } => {
            let mut all_geoms = Vec::new();
            let mut all_zm = Vec::new();
            for g in geometries {
                if let Ok(zm_geom) = convert_geojson_zm(g.clone()) {
                    let cnt = count_coords(&zm_geom.geometry);
                    all_zm.extend(zm_geom.zm.into_iter().take(cnt));
                    all_geoms.push(zm_geom.geometry);
                }
            }
            Ok(ZmGeometry::with_zm(
                Geometry::GeometryCollection(geo::GeometryCollection(all_geoms)),
                all_zm,
            ))
        }
    }
}

fn convert_geojson_geometry(
    geom: geojson::Geometry,
    _feature: &Feature,
) -> Result<Geometry<f64>, MakeValidError> {
    convert_geojson_zm(geom).map(|z| z.geometry)
}

pub fn export_geojson_rfc7946(
    geometries: &[Geometry<f64>],
    path: &str,
) -> Result<(), MakeValidError> {
    export_geojson_with_crs(geometries, path, None)
}

pub fn export_geojson_with_crs(
    geometries: &[Geometry<f64>],
    path: &str,
    crs: Option<&crate::Crs>,
) -> Result<(), MakeValidError> {
    use serde_json::{Map, Value};

    let features: Vec<Feature> = geometries
        .iter()
        .map(|geom| {
            let gj_geom = geo_geom_to_geojson(geom.clone());
            Feature {
                geometry: Some(gj_geom),
                properties: None,
                id: None,
                bbox: None,
                foreign_members: None,
            }
        })
        .collect();

    let foreign_members = crs.and_then(|c| {
        let auth = c.authority()?;
        let mut map = Map::new();
        let mut props = Map::new();
        props.insert("name".to_string(), Value::String(auth.to_string()));
        let mut crs_obj = Map::new();
        crs_obj.insert("type".to_string(), Value::String("name".to_string()));
        crs_obj.insert("properties".to_string(), Value::Object(props));
        map.insert("crs".to_string(), Value::Object(crs_obj));
        Some(map)
    });

    let collection = FeatureCollection {
        features,
        bbox: None,
        foreign_members,
    };

    let gj = GeoJson::FeatureCollection(collection);
    let json =
        serde_json::to_string_pretty(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    write!(writer, "{json}").map_err(|e| MakeValidError::IoError(e.to_string()))?;

    Ok(())
}

/// Export features (geometry + properties + Z/M) to GeoJSON.
pub fn export_features(path: &str, features: &[GeoRepairFeature]) -> Result<(), MakeValidError> {
    use serde_json::{Map, Value};

    let gj_features: Vec<Feature> = features
        .iter()
        .map(|f| {
            let gj_geom =
                geo_geom_to_geojson_zm(&ZmGeometry::with_zm(f.geometry.clone(), f.zm.clone()));
            Feature {
                geometry: Some(gj_geom),
                properties: f.properties.clone(),
                id: None,
                bbox: None,
                foreign_members: None,
            }
        })
        .collect();

    let foreign_members: Option<Map<String, Value>> =
        features.first().and_then(|f| f.crs.as_ref()).and_then(|c| {
            let auth = c.authority()?;
            let mut map = Map::new();
            let mut props = Map::new();
            props.insert("name".to_string(), Value::String(auth.to_string()));
            let mut crs_obj = Map::new();
            crs_obj.insert("type".to_string(), Value::String("name".to_string()));
            crs_obj.insert("properties".to_string(), Value::Object(props));
            map.insert("crs".to_string(), Value::Object(crs_obj));
            Some(map)
        });

    let collection = FeatureCollection {
        features: gj_features,
        bbox: None,
        foreign_members,
    };

    let gj = GeoJson::FeatureCollection(collection);
    let json =
        serde_json::to_string_pretty(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    write!(writer, "{json}").map_err(|e| MakeValidError::IoError(e.to_string()))?;

    Ok(())
}

pub fn export_geojson_with_crs_zm(
    geometries: &[ZmGeometry],
    path: &str,
    crs: Option<&crate::Crs>,
) -> Result<(), MakeValidError> {
    use serde_json::{Map, Value};

    let features: Vec<Feature> = geometries
        .iter()
        .map(|zm_geom| {
            let gj_geom = geo_geom_to_geojson_zm(zm_geom);
            Feature {
                geometry: Some(gj_geom),
                properties: None,
                id: None,
                bbox: None,
                foreign_members: None,
            }
        })
        .collect();

    let foreign_members = crs.and_then(|c| {
        let auth = c.authority()?;
        let mut map = Map::new();
        let mut props = Map::new();
        props.insert("name".to_string(), Value::String(auth.to_string()));
        let mut crs_obj = Map::new();
        crs_obj.insert("type".to_string(), Value::String("name".to_string()));
        crs_obj.insert("properties".to_string(), Value::Object(props));
        map.insert("crs".to_string(), Value::Object(crs_obj));
        Some(map)
    });

    let collection = FeatureCollection {
        features,
        bbox: None,
        foreign_members,
    };

    let gj = GeoJson::FeatureCollection(collection);
    let json =
        serde_json::to_string_pretty(&gj).map_err(|e| MakeValidError::ParseError(e.to_string()))?;

    let file = File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    write!(writer, "{json}").map_err(|e| MakeValidError::IoError(e.to_string()))?;

    Ok(())
}

pub(crate) fn geo_geom_to_geojson(geom: Geometry<f64>) -> geojson::Geometry {
    geo_geom_to_geojson_inner(geom, &mut std::iter::empty())
}

fn geo_geom_to_geojson_zm(geom: &ZmGeometry) -> geojson::Geometry {
    geo_geom_to_geojson_inner(geom.geometry.clone(), &mut geom.zm.iter().copied())
}

fn geo_geom_to_geojson_inner(
    geom: Geometry<f64>,
    zm: &mut impl Iterator<Item = ZmValue>,
) -> geojson::Geometry {
    fn coord_to_pos(c: Coord<f64>, zv: ZmValue) -> geojson::Position {
        let mut pos = vec![c.x, c.y];
        if zv.z.is_some() || zv.m.is_some() {
            pos.push(zv.z.unwrap_or(0.0));
        }
        if let Some(m) = zv.m {
            pos.push(m);
        }
        geojson::Position::from(pos)
    }

    fn ring_to_pos(
        ring: &LineString<f64>,
        zm: &mut impl Iterator<Item = ZmValue>,
    ) -> Vec<geojson::Position> {
        ring.0
            .iter()
            .map(|c| coord_to_pos(*c, zm.next().unwrap_or(ZmValue::NONE)))
            .collect()
    }

    fn polygon_to_coords(
        poly: &Polygon<f64>,
        zm: &mut impl Iterator<Item = ZmValue>,
    ) -> Vec<Vec<geojson::Position>> {
        let mut coords = vec![ring_to_pos(poly.exterior(), zm)];
        for h in poly.interiors() {
            coords.push(ring_to_pos(h, zm));
        }
        coords
    }

    let value = match geom {
        Geometry::Point(p) => GeometryValue::Point {
            coordinates: coord_to_pos(p.0, zm.next().unwrap_or(ZmValue::NONE)),
        },
        Geometry::MultiPoint(mp) => GeometryValue::MultiPoint {
            coordinates: mp
                .0
                .iter()
                .map(|p| coord_to_pos(p.0, zm.next().unwrap_or(ZmValue::NONE)))
                .collect(),
        },
        Geometry::LineString(ls) => GeometryValue::LineString {
            coordinates: ring_to_pos(&ls, zm),
        },
        Geometry::MultiLineString(mls) => GeometryValue::MultiLineString {
            coordinates: mls.0.iter().map(|ls| ring_to_pos(ls, zm)).collect(),
        },
        Geometry::Polygon(p) => GeometryValue::Polygon {
            coordinates: polygon_to_coords(&p, zm),
        },
        Geometry::Line(l) => GeometryValue::LineString {
            coordinates: vec![
                coord_to_pos(l.start, zm.next().unwrap_or(ZmValue::NONE)),
                coord_to_pos(l.end, zm.next().unwrap_or(ZmValue::NONE)),
            ],
        },
        Geometry::MultiPolygon(mp) => GeometryValue::MultiPolygon {
            coordinates: mp.0.iter().map(|p| polygon_to_coords(p, zm)).collect(),
        },
        Geometry::GeometryCollection(gc) => {
            let geoms: Vec<geojson::Geometry> =
                gc.0.into_iter()
                    .map(|g| geo_geom_to_geojson_inner(g, zm))
                    .collect();
            GeometryValue::GeometryCollection { geometries: geoms }
        }
        Geometry::Rect(r) => {
            let poly = Polygon::new(
                LineString::new(vec![
                    Coord {
                        x: r.min().x,
                        y: r.min().y,
                    },
                    Coord {
                        x: r.max().x,
                        y: r.min().y,
                    },
                    Coord {
                        x: r.max().x,
                        y: r.max().y,
                    },
                    Coord {
                        x: r.min().x,
                        y: r.max().y,
                    },
                    Coord {
                        x: r.min().x,
                        y: r.min().y,
                    },
                ]),
                Vec::new(),
            );
            GeometryValue::Polygon {
                coordinates: polygon_to_coords(&poly, zm),
            }
        }
        Geometry::Triangle(t) => {
            let poly = Polygon::new(
                LineString::new(vec![t.v1(), t.v2(), t.v3(), t.v1()]),
                Vec::new(),
            );
            GeometryValue::Polygon {
                coordinates: polygon_to_coords(&poly, zm),
            }
        }
    };

    geojson::Geometry::new(value)
}

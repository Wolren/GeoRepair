#![cfg(feature = "io-wkb")]

use std::fs::File;
use std::io::Read;

use geo::{Coord, Geometry, LineString};
use geo_traits::to_geo::ToGeoGeometry;
use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};
use wkb::reader::read_wkb;

use crate::core::MakeValidError;
use crate::crs::Crs;
use crate::zm::{count_coords, ZmGeometry, ZmValue};

const EWKB_SRID_FLAG: u32 = 0x20000000;

const WKB_POINT: u32 = 1;
const WKB_LINESTRING: u32 = 2;
const WKB_POLYGON: u32 = 3;
const WKB_MULTI_POINT: u32 = 4;
const WKB_MULTI_LINESTRING: u32 = 5;
const WKB_MULTI_POLYGON: u32 = 6;
const WKB_GEOMETRY_COLLECTION: u32 = 7;
const WKB_Z_FLAG: u32 = 0x80000000;
const WKB_M_FLAG: u32 = 0x40000000;

/// Load WKB geometry file.
pub fn load_wkb(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    load_wkb_with_crs(path).map(|(geoms, _)| geoms)
}

/// Load WKB geometry file and extract SRID as CRS (EWKB format).
pub fn load_wkb_with_crs(path: &str) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    let mut buf = Vec::new();
    File::open(path)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?
        .read_to_end(&mut buf)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;

    let (srid, clean_buf) = extract_ewkb_srid(&buf);
    let crs = srid.map(|s| Crs::from_epsg(s as u32));

    let wkb_geom = read_wkb(&clean_buf)
        .map_err(|e| MakeValidError::ParseError(format!("WKB parse error: {e}")))?;
    let geo_geom: Geometry<f64> = wkb_geom.to_geometry();

    Ok((extract_geometries(geo_geom), crs))
}

/// Load WKB with Z/M coordinate preservation.
pub fn load_wkb_zm(path: &str) -> Result<(Vec<ZmGeometry>, Option<Crs>), MakeValidError> {
    let mut buf = Vec::new();
    File::open(path)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?
        .read_to_end(&mut buf)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;

    let (srid, clean_buf) = extract_ewkb_srid(&buf);
    let crs = srid.map(|s| Crs::from_epsg(s as u32));

    let (geo_geom, zm) = read_wkb_zm(&clean_buf)
        .map_err(|e| MakeValidError::ParseError(format!("WKB Z/M parse error: {e}")))?;

    Ok((extract_geometries_zm(geo_geom, zm), crs))
}

/// Export geometries as WKB (one per file, concatenated).
pub fn export_wkb(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    export_wkb_with_crs(geometries, path, None)
}

/// Export geometries as WKB with optional EWKB SRID prefix.
pub fn export_wkb_with_crs(
    geometries: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    export_wkb_zm_with_crs(
        &geometries
            .iter()
            .map(|g| ZmGeometry::new(g.clone()))
            .collect::<Vec<_>>(),
        path,
        crs,
    )
}

/// Export geometries as WKB with Z/M values and optional EWKB SRID.
pub fn export_wkb_zm_with_crs(
    geometries: &[ZmGeometry],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let mut wkb_buf = Vec::new();
    for zm_geom in geometries {
        write_wkb_geometry(&mut wkb_buf, zm_geom)
            .map_err(|e| MakeValidError::ParseError(format!("WKB Z/M write: {e}")))?;
    }

    let out_buf = match crs.and_then(|c| c.srid()) {
        Some(srid) => wrap_ewkb(&wkb_buf, srid),
        None => wkb_buf,
    };

    std::fs::write(path, &out_buf).map_err(|e| MakeValidError::IoError(e.to_string()))?;
    Ok(())
}

fn extract_ewkb_srid(buf: &[u8]) -> (Option<i32>, Vec<u8>) {
    if buf.len() < 5 {
        return (None, buf.to_vec());
    }
    let endianness = buf[0];
    let geom_type = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    if geom_type & EWKB_SRID_FLAG != 0 {
        if buf.len() < 9 {
            return (None, buf.to_vec());
        }
        let srid = i32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
        let mut clean = Vec::with_capacity(buf.len() - 4);
        clean.push(endianness);
        clean.extend_from_slice(&(geom_type & !EWKB_SRID_FLAG).to_le_bytes());
        clean.extend_from_slice(&buf[9..]);
        (Some(srid), clean)
    } else {
        (None, buf.to_vec())
    }
}

fn wrap_ewkb(wkb: &[u8], srid: i32) -> Vec<u8> {
    if wkb.len() < 5 {
        return wkb.to_vec();
    }
    let geom_type = u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]);
    let ewkb_type = geom_type | EWKB_SRID_FLAG;
    let mut result = Vec::with_capacity(wkb.len() + 4);
    result.push(wkb[0]);
    result.extend_from_slice(&ewkb_type.to_le_bytes());
    result.extend_from_slice(&srid.to_le_bytes());
    result.extend_from_slice(&wkb[5..]);
    result
}

fn read_wkb_zm(buf: &[u8]) -> Result<(Geometry<f64>, Vec<ZmValue>), String> {
    let wkb_geom = read_wkb(buf).map_err(|e| format!("{e}"))?;
    let zm = extract_zm_from_wkb(&wkb_geom);
    let geo_geom: Geometry<f64> = wkb_geom.to_geometry();
    Ok((geo_geom, zm))
}

fn extract_zm_from_wkb(wkb: &wkb::reader::Wkb<'_>) -> Vec<ZmValue> {
    fn coord_zm<C: CoordTrait<T = f64>>(c: &C) -> ZmValue {
        match c.dim() {
            Dimensions::Xy => ZmValue::NONE,
            Dimensions::Xyz => ZmValue::z_only(c.nth_or_panic(2)),
            Dimensions::Xym => ZmValue::m_only(c.nth_or_panic(2)),
            Dimensions::Xyzm => ZmValue::new(Some(c.nth_or_panic(2)), Some(c.nth_or_panic(3))),
            _ => ZmValue::NONE,
        }
    }

    fn ls_zm<LS: LineStringTrait<T = f64>>(ls: &LS) -> Vec<ZmValue> {
        (0..ls.num_coords())
            .filter_map(|i| ls.coord(i).map(|c| coord_zm(&c)))
            .collect()
    }

    match wkb.as_type() {
        GeometryType::Point(pt) => {
            vec![pt.coord().map(|c| coord_zm(&c)).unwrap_or(ZmValue::NONE)]
        }
        GeometryType::LineString(ls) => ls_zm(&ls),
        GeometryType::Polygon(poly) => {
            let mut result = Vec::new();
            if let Some(ext) = poly.exterior() {
                result.extend(ls_zm(ext));
            }
            for i in 0..poly.num_interiors() {
                if let Some(int) = poly.interior(i) {
                    result.extend(ls_zm(int));
                }
            }
            result
        }
        GeometryType::MultiPoint(mp) => {
            let mut result = Vec::with_capacity(mp.num_points());
            for i in 0..mp.num_points() {
                if let Some(pt) = mp.point(i) {
                    result.push(pt.coord().map(|c| coord_zm(&c)).unwrap_or(ZmValue::NONE));
                }
            }
            result
        }
        GeometryType::MultiLineString(mls) => {
            let mut result = Vec::new();
            for i in 0..mls.num_line_strings() {
                if let Some(ls) = mls.line_string(i) {
                    result.extend(ls_zm(ls));
                }
            }
            result
        }
        GeometryType::MultiPolygon(mp) => {
            let mut result = Vec::new();
            for i in 0..mp.num_polygons() {
                if let Some(poly) = mp.polygon(i) {
                    if let Some(ext) = poly.exterior() {
                        result.extend(ls_zm(ext));
                    }
                    for j in 0..poly.num_interiors() {
                        if let Some(int) = poly.interior(j) {
                            result.extend(ls_zm(int));
                        }
                    }
                }
            }
            result
        }
        GeometryType::GeometryCollection(gc) => {
            let mut result = Vec::new();
            for i in 0..gc.num_geometries() {
                if let Some(sub) = gc.geometry(i) {
                    result.extend(extract_zm_from_wkb(sub));
                }
            }
            result
        }
        _ => Vec::new(),
    }
}

fn write_wkb_geometry(buf: &mut Vec<u8>, zm_geom: &ZmGeometry) -> Result<usize, String> {
    let has_z = zm_geom.has_z();
    let has_m = zm_geom.has_m();
    let start = buf.len();

    buf.push(1u8);

    let type_code = wkb_type_code(&zm_geom.geometry, has_z, has_m);
    buf.extend_from_slice(&type_code.to_le_bytes());

    let zm_iter = &mut zm_geom.zm.iter().copied();

    match &zm_geom.geometry {
        Geometry::Point(p) => {
            write_wkb_coord(buf, p.0, zm_iter.next().unwrap_or_default(), has_z, has_m);
        }
        Geometry::MultiPoint(mp) => {
            buf.extend_from_slice(&(mp.0.len() as u32).to_le_bytes());
            for pt in &mp.0 {
                let _sub_start = buf.len();
                buf.push(1u8);
                let sub_type = wkb_type_code(&Geometry::Point(*pt), has_z, has_m);
                buf.extend_from_slice(&sub_type.to_le_bytes());
                write_wkb_coord(buf, pt.0, zm_iter.next().unwrap_or_default(), has_z, has_m);
            }
        }
        Geometry::LineString(ls) => {
            write_wkb_points(buf, &ls.0, zm_iter, has_z, has_m);
        }
        Geometry::MultiLineString(mls) => {
            buf.extend_from_slice(&(mls.0.len() as u32).to_le_bytes());
            for ls in &mls.0 {
                let _sub_start = buf.len();
                buf.push(1u8);
                let sub_type = wkb_type_code(&Geometry::LineString(ls.clone()), has_z, has_m);
                buf.extend_from_slice(&sub_type.to_le_bytes());
                write_wkb_points(buf, &ls.0, zm_iter, has_z, has_m);
            }
        }
        Geometry::Polygon(poly) => {
            write_wkb_polygon(buf, poly, zm_iter, has_z, has_m);
        }
        Geometry::MultiPolygon(mp) => {
            buf.extend_from_slice(&(mp.0.len() as u32).to_le_bytes());
            for poly in &mp.0 {
                let _sub_start = buf.len();
                buf.push(1u8);
                let sub_type = wkb_type_code(&Geometry::Polygon(poly.clone()), has_z, has_m);
                buf.extend_from_slice(&sub_type.to_le_bytes());
                write_wkb_polygon(buf, poly, zm_iter, has_z, has_m);
            }
        }
        Geometry::Line(l) => {
            buf.extend_from_slice(&(2u32).to_le_bytes());
            write_wkb_coord(
                buf,
                l.start,
                zm_iter.next().unwrap_or_default(),
                has_z,
                has_m,
            );
            write_wkb_coord(buf, l.end, zm_iter.next().unwrap_or_default(), has_z, has_m);
        }
        Geometry::Rect(r) => {
            let coords = vec![
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
            ];
            write_wkb_polygon_coords(buf, &coords, &[], zm_iter, has_z, has_m);
        }
        Geometry::Triangle(t) => {
            write_wkb_polygon_coords(
                buf,
                &[t.v1(), t.v2(), t.v3(), t.v1()],
                &[],
                zm_iter,
                has_z,
                has_m,
            );
        }
        Geometry::GeometryCollection(gc) => {
            buf.extend_from_slice(&(gc.0.len() as u32).to_le_bytes());
            for child in &gc.0 {
                let cnt = count_coords(child);
                let mut sub_zm = Vec::with_capacity(cnt);
                for _ in 0..cnt {
                    sub_zm.push(zm_iter.next().unwrap_or_default());
                }
                write_wkb_geometry(buf, &ZmGeometry::with_zm(child.clone(), sub_zm))?;
            }
        }
    }

    Ok(buf.len() - start)
}

/// Encode a 2D geometry to standard little-endian WKB bytes.
/// Used by the GeoPackage module for GPKG blob encoding.
pub(crate) fn encode_wkb_2d(geom: &Geometry<f64>) -> Result<Vec<u8>, String> {
    let zm_geom = ZmGeometry::with_zm(geom.clone(), vec![]);
    let mut buf = Vec::new();
    write_wkb_geometry(&mut buf, &zm_geom)?;
    Ok(buf)
}

fn wkb_type_code(geom: &Geometry<f64>, has_z: bool, has_m: bool) -> u32 {
    let base = match geom {
        Geometry::Point(_) => WKB_POINT,
        Geometry::Line(_) => WKB_LINESTRING,
        Geometry::MultiPoint(_) => WKB_MULTI_POINT,
        Geometry::LineString(_) => WKB_LINESTRING,
        Geometry::MultiLineString(_) => WKB_MULTI_LINESTRING,
        Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => WKB_POLYGON,
        Geometry::MultiPolygon(_) => WKB_MULTI_POLYGON,
        Geometry::GeometryCollection(_) => WKB_GEOMETRY_COLLECTION,
    };
    let mut code = base;
    if has_z {
        code |= WKB_Z_FLAG;
    }
    if has_m {
        code |= WKB_M_FLAG;
    }
    code
}

fn write_wkb_coord(buf: &mut Vec<u8>, c: Coord<f64>, zv: ZmValue, has_z: bool, has_m: bool) {
    buf.extend_from_slice(&c.x.to_le_bytes());
    buf.extend_from_slice(&c.y.to_le_bytes());
    if has_z {
        buf.extend_from_slice(&zv.z.unwrap_or(0.0).to_le_bytes());
    }
    if has_m {
        buf.extend_from_slice(&zv.m.unwrap_or(0.0).to_le_bytes());
    }
}

fn write_wkb_points(
    buf: &mut Vec<u8>,
    coords: &[Coord<f64>],
    zm: &mut impl Iterator<Item = ZmValue>,
    has_z: bool,
    has_m: bool,
) {
    buf.extend_from_slice(&(coords.len() as u32).to_le_bytes());
    for c in coords {
        write_wkb_coord(buf, *c, zm.next().unwrap_or_default(), has_z, has_m);
    }
}

fn write_wkb_polygon(
    buf: &mut Vec<u8>,
    poly: &geo::Polygon<f64>,
    zm: &mut impl Iterator<Item = ZmValue>,
    has_z: bool,
    has_m: bool,
) {
    let ext = poly.exterior();
    let interiors = poly.interiors();
    write_wkb_polygon_coords(buf, &ext.0, interiors, zm, has_z, has_m);
}

fn write_wkb_polygon_coords(
    buf: &mut Vec<u8>,
    exterior: &[Coord<f64>],
    interiors: &[LineString<f64>],
    zm: &mut impl Iterator<Item = ZmValue>,
    has_z: bool,
    has_m: bool,
) {
    let num_rings = 1 + interiors.len();
    buf.extend_from_slice(&(num_rings as u32).to_le_bytes());
    write_wkb_points(buf, exterior, zm, has_z, has_m);
    for h in interiors {
        write_wkb_points(buf, &h.0, zm, has_z, has_m);
    }
}

fn extract_geometries_zm(geom: Geometry<f64>, zm: Vec<ZmValue>) -> Vec<ZmGeometry> {
    match geom {
        Geometry::GeometryCollection(gc) => {
            let mut result = Vec::new();
            let mut idx = 0;
            for g in gc.0 {
                let cnt = count_coords(&g);
                let sub_zm = zm[idx..idx + cnt].to_vec();
                idx += cnt;
                result.push(ZmGeometry::with_zm(g, sub_zm));
            }
            result
        }
        other => vec![ZmGeometry::with_zm(other, zm)],
    }
}

fn extract_geometries(geom: Geometry<f64>) -> Vec<Geometry<f64>> {
    match geom {
        Geometry::GeometryCollection(gc) => {
            let mut result = Vec::new();
            for g in gc.0 {
                result.extend(extract_geometries(g));
            }
            result
        }
        other => vec![other],
    }
}

pub fn extract_geometries_re(geom: Geometry<f64>) -> Vec<Geometry<f64>> {
    extract_geometries(geom)
}

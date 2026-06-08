use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::core::MakeValidError;
use crate::Crs;

const GML_NS: &str = "http://www.opengis.net/gml/3.2";
const GML_PREFIX: &str = "gml";

/// Convert a `Crs` to a GML 3.2 `srsName` URN.
fn crs_to_srs_name(crs: &Crs) -> Option<String> {
    let auth = crs.authority()?;
    let parts: Vec<&str> = auth.split(':').collect();
    if parts.len() == 2 {
        Some(format!("urn:ogc:def:crs:{}::{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// Load geometries from a GML file (.gml or .xml).
pub fn load_gml(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| MakeValidError::IoError(format!("read GML: {e}")))?;

    let mut reader = Reader::from_str(&content);
    let mut buf = Vec::new();
    let mut geometries = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = local_name(e);
                if let Some(geom) = try_parse_geometry(&mut reader, &name) {
                    geometries.push(geom);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!("GML parse: {e}")));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(geometries)
}

/// Export geometries to GML 3.2 (ISO 19136).
pub fn export_gml(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    export_gml_with_crs(geometries, path, None)
}

/// Export geometries to GML 3.2 with optional CRS metadata (`srsName`).
pub fn export_gml_with_crs(
    geometries: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    use std::io::BufWriter;
    let file = std::fs::File::create(path)
        .map_err(|e| MakeValidError::IoError(format!("create GML: {e}")))?;
    let mut writer = Writer::new_with_indent(BufWriter::new(file), b' ', 2);

    let srs_name = crs.and_then(crs_to_srs_name);

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| MakeValidError::IoError(format!("GML decl: {e}")))?;

    let root = BytesStart::new(format!("{GML_PREFIX}:FeatureCollection"))
        .with_attributes(vec![("xmlns:gml", GML_NS)]);
    writer
        .write_event(Event::Start(root))
        .map_err(|e| MakeValidError::IoError(format!("GML root: {e}")))?;

    for geom in geometries {
        writer
            .write_event(Event::Start(BytesStart::new(format!(
                "{GML_PREFIX}:featureMember"
            ))))
            .map_err(|e| MakeValidError::IoError(format!("member: {e}")))?;
        write_geometry_gml(&mut writer, geom, srs_name.as_deref())
            .map_err(|e| MakeValidError::IoError(format!("geom: {e}")))?;
        writer
            .write_event(Event::End(BytesEnd::new(format!(
                "{GML_PREFIX}:featureMember"
            ))))
            .map_err(|e| MakeValidError::IoError(format!("/member: {e}")))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new(format!(
            "{GML_PREFIX}:FeatureCollection"
        ))))
        .map_err(|e| MakeValidError::IoError(format!("/root: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn write_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    tag: &str,
    text: &str,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new(format!("{GML_PREFIX}:{tag}"))))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:{tag}"))))?;
    Ok(())
}

fn write_element_tag<W: std::io::Write>(
    writer: &mut Writer<W>,
    tag: &str,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new(format!("{GML_PREFIX}:{tag}"))))?;
    Ok(())
}

fn write_end_tag<W: std::io::Write>(
    writer: &mut Writer<W>,
    tag: &str,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:{tag}"))))?;
    Ok(())
}

fn write_geometry_gml<W: std::io::Write>(
    writer: &mut Writer<W>,
    geom: &Geometry<f64>,
    srs_name: Option<&str>,
) -> Result<(), quick_xml::Error> {
    match geom {
        Geometry::Point(p) => write_point_gml(writer, &p.0, srs_name),
        Geometry::MultiPoint(mp) => write_multi(writer, "MultiPoint", srs_name, |w| {
            for pt in &mp.0 {
                write_element_tag(w, "pointMember")?;
                write_point_gml(w, &pt.0, None)?;
                write_end_tag(w, "pointMember")?;
            }
            Ok(())
        }),
        Geometry::LineString(ls) => write_linestring_gml(writer, &ls.0, srs_name),
        Geometry::MultiLineString(mls) => write_multi(writer, "MultiCurve", srs_name, |w| {
            for ls in &mls.0 {
                write_element_tag(w, "curveMember")?;
                write_linestring_gml(w, &ls.0, None)?;
                write_end_tag(w, "curveMember")?;
            }
            Ok(())
        }),
        Geometry::Polygon(p) => write_polygon_gml(writer, p, srs_name),
        Geometry::MultiPolygon(mp) => write_multi(writer, "MultiSurface", srs_name, |w| {
            for p in &mp.0 {
                write_element_tag(w, "surfaceMember")?;
                write_polygon_gml(w, p, None)?;
                write_end_tag(w, "surfaceMember")?;
            }
            Ok(())
        }),
        Geometry::GeometryCollection(gc) => write_multi(writer, "MultiGeometry", srs_name, |w| {
            for child in &gc.0 {
                write_element_tag(w, "geometryMember")?;
                write_geometry_gml(w, child, None)?;
                write_end_tag(w, "geometryMember")?;
            }
            Ok(())
        }),
        Geometry::Line(l) => write_linestring_gml(writer, &[l.start, l.end], srs_name),
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
            write_polygon_rings(writer, &coords, &[], srs_name)
        }
        Geometry::Triangle(t) => {
            write_polygon_rings(writer, &[t.v1(), t.v2(), t.v3(), t.v1()], &[], srs_name)
        }
    }
}

fn write_multi<W: std::io::Write, F: Fn(&mut Writer<W>) -> Result<(), quick_xml::Error>>(
    writer: &mut Writer<W>,
    tag: &str,
    srs_name: Option<&str>,
    f: F,
) -> Result<(), quick_xml::Error> {
    let mut start = BytesStart::new(format!("{GML_PREFIX}:{tag}"));
    if let Some(srs) = srs_name {
        start.push_attribute(("srsName", srs));
    }
    writer.write_event(Event::Start(start))?;
    f(writer)?;
    writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:{tag}"))))?;
    Ok(())
}

fn write_point_gml<W: std::io::Write>(
    writer: &mut Writer<W>,
    coord: &Coord<f64>,
    srs_name: Option<&str>,
) -> Result<(), quick_xml::Error> {
    let mut start = BytesStart::new(format!("{GML_PREFIX}:Point"));
    if let Some(srs) = srs_name {
        start.push_attribute(("srsName", srs));
    }
    writer.write_event(Event::Start(start))?;
    write_element(writer, "pos", &format!("{} {}", coord.x, coord.y))?;
    writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:Point"))))?;
    Ok(())
}

fn write_linestring_gml<W: std::io::Write>(
    writer: &mut Writer<W>,
    coords: &[Coord<f64>],
    srs_name: Option<&str>,
) -> Result<(), quick_xml::Error> {
    let txt = coords
        .iter()
        .map(|c| format!("{} {}", c.x, c.y))
        .collect::<Vec<_>>()
        .join(" ");
    let mut start = BytesStart::new(format!("{GML_PREFIX}:LineString"));
    if let Some(srs) = srs_name {
        start.push_attribute(("srsName", srs));
    }
    writer.write_event(Event::Start(start))?;
    write_element(writer, "posList", &txt)?;
    writer.write_event(Event::End(BytesEnd::new(format!(
        "{GML_PREFIX}:LineString"
    ))))?;
    Ok(())
}

fn write_polygon_gml<W: std::io::Write>(
    writer: &mut Writer<W>,
    poly: &Polygon<f64>,
    srs_name: Option<&str>,
) -> Result<(), quick_xml::Error> {
    write_polygon_rings(
        writer,
        poly.exterior().0.as_slice(),
        poly.interiors(),
        srs_name,
    )
}

fn write_polygon_rings<W: std::io::Write>(
    writer: &mut Writer<W>,
    exterior: &[Coord<f64>],
    interiors: &[LineString<f64>],
    srs_name: Option<&str>,
) -> Result<(), quick_xml::Error> {
    let mut start = BytesStart::new(format!("{GML_PREFIX}:Polygon"));
    if let Some(srs) = srs_name {
        start.push_attribute(("srsName", srs));
    }
    writer.write_event(Event::Start(start))?;

    // exterior
    writer.write_event(Event::Start(BytesStart::new(format!(
        "{GML_PREFIX}:exterior"
    ))))?;
    writer.write_event(Event::Start(BytesStart::new(format!(
        "{GML_PREFIX}:LinearRing"
    ))))?;
    let ext_txt = exterior
        .iter()
        .map(|c| format!("{} {}", c.x, c.y))
        .collect::<Vec<_>>()
        .join(" ");
    write_element(writer, "posList", &ext_txt)?;
    writer.write_event(Event::End(BytesEnd::new(format!(
        "{GML_PREFIX}:LinearRing"
    ))))?;
    writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:exterior"))))?;

    // interiors
    for ring in interiors {
        writer.write_event(Event::Start(BytesStart::new(format!(
            "{GML_PREFIX}:interior"
        ))))?;
        writer.write_event(Event::Start(BytesStart::new(format!(
            "{GML_PREFIX}:LinearRing"
        ))))?;
        let int_txt = ring
            .0
            .iter()
            .map(|c| format!("{} {}", c.x, c.y))
            .collect::<Vec<_>>()
            .join(" ");
        write_element(writer, "posList", &int_txt)?;
        writer.write_event(Event::End(BytesEnd::new(format!(
            "{GML_PREFIX}:LinearRing"
        ))))?;
        writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:interior"))))?;
    }

    writer.write_event(Event::End(BytesEnd::new(format!("{GML_PREFIX}:Polygon"))))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Get the local (unprefixed) name of an element.
fn local_name(e: &quick_xml::events::BytesStart) -> String {
    String::from_utf8_lossy(e.name().as_ref())
        .split(':')
        .last()
        .unwrap_or_default()
        .to_string()
}

fn local_name_from_end(e: &quick_xml::events::BytesEnd) -> String {
    String::from_utf8_lossy(e.name().as_ref())
        .split(':')
        .last()
        .unwrap_or_default()
        .to_string()
}

/// Try to read a geometry element starting at `reader`.
fn try_parse_geometry(reader: &mut Reader<&[u8]>, name: &str) -> Option<Geometry<f64>> {
    match name {
        "Point" => parse_point(reader).map(Geometry::Point),
        "LineString" => parse_linestring(reader).map(Geometry::LineString),
        "Polygon" => parse_polygon(reader).map(Geometry::Polygon),
        "MultiPoint" | "MultiCurve" | "MultiSurface" | "MultiGeometry" | "MultiLineString"
        | "MultiPolygon" => {
            let members = parse_multi_members(reader, name);
            if members.is_empty() {
                return None;
            }
            let all_points = members.iter().all(|g| matches!(g, Geometry::Point(_)));
            let all_lines = members.iter().all(|g| matches!(g, Geometry::LineString(_)));
            let all_polys = members.iter().all(|g| matches!(g, Geometry::Polygon(_)));
            Some(if all_points {
                Geometry::MultiPoint(MultiPoint::new(
                    members
                        .into_iter()
                        .filter_map(|g| {
                            if let Geometry::Point(p) = g {
                                Some(p)
                            } else {
                                None
                            }
                        })
                        .collect(),
                ))
            } else if all_lines {
                Geometry::MultiLineString(MultiLineString::new(
                    members
                        .into_iter()
                        .filter_map(|g| {
                            if let Geometry::LineString(ls) = g {
                                Some(ls)
                            } else {
                                None
                            }
                        })
                        .collect(),
                ))
            } else if all_polys {
                Geometry::MultiPolygon(MultiPolygon::new(
                    members
                        .into_iter()
                        .filter_map(|g| {
                            if let Geometry::Polygon(p) = g {
                                Some(p)
                            } else {
                                None
                            }
                        })
                        .collect(),
                ))
            } else {
                Geometry::GeometryCollection(GeometryCollection(members))
            })
        }
        _ => None,
    }
}

fn parse_point(reader: &mut Reader<&[u8]>) -> Option<Point<f64>> {
    let coords = parse_text_coords(reader, &["Point"])?;
    if coords.len() == 1 {
        Some(Point::new(coords[0].x, coords[0].y))
    } else {
        None
    }
}

fn parse_linestring(reader: &mut Reader<&[u8]>) -> Option<LineString<f64>> {
    let coords = parse_text_coords(reader, &["LineString"])?;
    Some(LineString::new(coords))
}

fn parse_polygon(reader: &mut Reader<&[u8]>) -> Option<Polygon<f64>> {
    let buf = &mut Vec::new();
    let mut exterior: Option<LineString<f64>> = None;
    let mut interiors: Vec<LineString<f64>> = Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e);
                match name.as_str() {
                    "exterior" => {
                        exterior = parse_linear_ring(reader);
                    }
                    "interior" => {
                        if let Some(ring) = parse_linear_ring(reader) {
                            interiors.push(ring);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name_from_end(e) == "Polygon" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    exterior.map(|ext| Polygon::new(ext, interiors))
}

/// Parse a `<gml:LinearRing>` element, consuming its contents.
fn parse_linear_ring(reader: &mut Reader<&[u8]>) -> Option<LineString<f64>> {
    let coords = parse_text_coords(reader, &["LinearRing"])?;
    Some(LineString::new(coords))
}

/// Collect text content from posList/pos/coordinates elements within
/// a geometry container, stopping when one of `end_tags` is encountered.
fn parse_text_coords(reader: &mut Reader<&[u8]>, end_tags: &[&str]) -> Option<Vec<Coord<f64>>> {
    let buf = &mut Vec::new();
    let mut all_text = String::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e);
                if name == "pos" || name == "posList" {
                    if let Ok(Event::Text(t)) = reader.read_event_into(buf) {
                        all_text.push_str(&String::from_utf8_lossy(t.as_ref()));
                        all_text.push(' ');
                    }
                } else if name == "coordinates" {
                    if let Ok(Event::Text(t)) = reader.read_event_into(buf) {
                        all_text.push_str(&String::from_utf8_lossy(t.as_ref()));
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = local_name_from_end(e);
                if end_tags.contains(&name.as_str()) {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    parse_coords(&all_text)
}

/// Parse coordinate text into a Vec<Coord>.
///
/// Handles space-separated ("1 2 3 4") and legacy comma-separated ("1,2 3,4").
fn parse_coords(text: &str) -> Option<Vec<Coord<f64>>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    // comma-separated pairs: "1.0,2.0 3.0,4.0"
    if text.contains(',') {
        let pairs: Vec<&str> = text.split_whitespace().collect();
        let mut coords = Vec::with_capacity(pairs.len());
        for pair in pairs {
            let xy: Vec<&str> = pair.split(',').collect();
            if xy.len() >= 2 {
                coords.push(Coord {
                    x: xy[0].parse().ok()?,
                    y: xy[1].parse().ok()?,
                });
            } else {
                return None;
            }
        }
        return Some(coords);
    }

    // space-separated: "1.0 2.0 3.0 4.0"
    let values: Vec<f64> = text
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();

    if values.len() < 2 || values.len() % 2 != 0 {
        return None;
    }

    Some(
        values
            .chunks(2)
            .map(|c| Coord { x: c[0], y: c[1] })
            .collect(),
    )
}

/// Parse member elements (pointMember, curveMember, surfaceMember, etc.)
fn parse_multi_members(reader: &mut Reader<&[u8]>, start_name: &str) -> Vec<Geometry<f64>> {
    let buf = &mut Vec::new();
    let mut members = Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e);
                if matches!(
                    name.as_str(),
                    "pointMember" | "curveMember" | "surfaceMember" | "geometryMember" | "member"
                ) {
                    if let Some(child) = parse_member_child(reader) {
                        members.push(child);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name_from_end(e) == start_name {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    members
}

/// Parse the geometry inside a member element.
fn parse_member_child(reader: &mut Reader<&[u8]>) -> Option<Geometry<f64>> {
    let buf = &mut Vec::new();
    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e);
                if let Some(geom) = try_parse_geometry(reader, &name) {
                    return Some(geom);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = local_name_from_end(e);
                if matches!(
                    name.as_str(),
                    "pointMember"
                        | "curveMember"
                        | "surfaceMember"
                        | "geometryMember"
                        | "member"
                        | "featureMember"
                ) {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_coords_space() {
        let coords = parse_coords("1.0 2.0").unwrap();
        assert_eq!(coords.len(), 1);
        assert!((coords[0].x - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_parse_coords_poslist() {
        let coords = parse_coords("0 0 1 1 2 2").unwrap();
        assert_eq!(coords.len(), 3);
        assert_eq!(coords[1].x, 1.0);
    }

    #[test]
    fn test_parse_coords_legacy() {
        let coords = parse_coords("1.0,2.0 3.0,4.0").unwrap();
        assert_eq!(coords.len(), 2);
        assert!((coords[1].y - 4.0).abs() < 1e-12);
    }

    fn write_and_read(geom: Geometry<f64>, name: &str) -> Geometry<f64> {
        let path = std::env::temp_dir().join(format!("test_gml_{name}.gml"));
        let ps = path.to_str().unwrap().to_string();
        export_gml(&[geom], &ps).unwrap();
        let loaded = load_gml(&ps).unwrap();
        let _ = std::fs::remove_file(&path);
        loaded.into_iter().next().unwrap()
    }

    #[test]
    fn test_gml_point_roundtrip() {
        let geom = Geometry::Point(Point::new(1.5, 2.5));
        let loaded = write_and_read(geom, "point");
        match loaded {
            Geometry::Point(p) => {
                assert!((p.x() - 1.5).abs() < 1e-10);
                assert!((p.y() - 2.5).abs() < 1e-10);
            }
            _ => panic!("expected Point"),
        }
    }

    #[test]
    fn test_gml_linestring_roundtrip() {
        let geom = Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ]));
        let loaded = write_and_read(geom.clone(), "linestring");
        assert_eq!(loaded, geom);
    }

    #[test]
    fn test_gml_polygon_roundtrip() {
        let geom = Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        ));
        let loaded = write_and_read(geom.clone(), "polygon");
        assert_eq!(loaded, geom);
    }

    #[test]
    fn test_gml_multipoint_roundtrip() {
        let geom = Geometry::MultiPoint(MultiPoint::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ]));
        let loaded = write_and_read(geom.clone(), "multipoint");
        assert_eq!(loaded, geom);
    }
}

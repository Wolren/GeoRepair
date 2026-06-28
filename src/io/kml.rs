use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::core::{io_err, MakeValidError};
use crate::Crs;

const KML_NS: &str = "http://www.opengis.net/kml/2.2";

/// Load geometries from a KML file (.kml).
pub fn load_kml(path: &str) -> Result<Vec<Geometry<f64>>, MakeValidError> {
    let content = std::fs::read_to_string(path).map_err(|e| io_err(format!("read KML: {e}")))?;

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
                return Err(MakeValidError::ParseError(format!("KML parse: {e}")));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(geometries)
}

/// Export geometries to KML 2.2.
pub fn export_kml(geometries: &[Geometry<f64>], path: &str) -> Result<(), MakeValidError> {
    export_kml_with_crs(geometries, path, None)
}

/// Export geometries to KML 2.2.
///
/// KML always uses EPSG:4326 (WGS84), so CRS is informational only
/// and does not affect the output.
pub fn export_kml_with_crs(
    geometries: &[Geometry<f64>],
    path: &str,
    _crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    use std::io::BufWriter;
    let file = std::fs::File::create(path).map_err(|e| io_err(format!("create KML: {e}")))?;
    let mut writer = Writer::new_with_indent(BufWriter::new(file), b' ', 2);

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| io_err(format!("KML decl: {e}")))?;

    let root = BytesStart::new("kml").with_attributes(vec![("xmlns", KML_NS)]);
    writer
        .write_event(Event::Start(root))
        .map_err(|e| io_err(format!("KML root: {e}")))?;

    writer
        .write_event(Event::Start(BytesStart::new("Document")))
        .map_err(|e| io_err(format!("Document: {e}")))?;

    for geom in geometries {
        writer
            .write_event(Event::Start(BytesStart::new("Placemark")))
            .map_err(|e| io_err(format!("Placemark: {e}")))?;
        write_geometry_kml(&mut writer, geom).map_err(|e| io_err(format!("geom: {e}")))?;
        writer
            .write_event(Event::End(BytesEnd::new("Placemark")))
            .map_err(|e| io_err(format!("/Placemark: {e}")))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("Document")))
        .map_err(|e| io_err(format!("/Document: {e}")))?;
    writer
        .write_event(Event::End(BytesEnd::new("kml")))
        .map_err(|e| io_err(format!("/kml: {e}")))?;

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
    writer.write_event(Event::Start(BytesStart::new(tag)))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

fn write_coordinates<W: std::io::Write>(
    writer: &mut Writer<W>,
    coords: &[Coord<f64>],
) -> Result<(), quick_xml::Error> {
    let txt = coords
        .iter()
        .map(|c| format!("{},{}", c.x, c.y))
        .collect::<Vec<_>>()
        .join(" ");
    write_element(writer, "coordinates", &txt)
}

fn write_geometry_kml<W: std::io::Write>(
    writer: &mut Writer<W>,
    geom: &Geometry<f64>,
) -> Result<(), quick_xml::Error> {
    match geom {
        Geometry::Point(p) => {
            writer.write_event(Event::Start(BytesStart::new("Point")))?;
            write_coordinates(writer, &[p.0])?;
            writer.write_event(Event::End(BytesEnd::new("Point")))?;
        }
        Geometry::MultiPoint(mp) => {
            writer.write_event(Event::Start(BytesStart::new("MultiGeometry")))?;
            for pt in &mp.0 {
                writer.write_event(Event::Start(BytesStart::new("Point")))?;
                write_coordinates(writer, &[pt.0])?;
                writer.write_event(Event::End(BytesEnd::new("Point")))?;
            }
            writer.write_event(Event::End(BytesEnd::new("MultiGeometry")))?;
        }
        Geometry::LineString(ls) => {
            writer.write_event(Event::Start(BytesStart::new("LineString")))?;
            write_coordinates(writer, &ls.0)?;
            writer.write_event(Event::End(BytesEnd::new("LineString")))?;
        }
        Geometry::MultiLineString(mls) => {
            writer.write_event(Event::Start(BytesStart::new("MultiGeometry")))?;
            for ls in &mls.0 {
                writer.write_event(Event::Start(BytesStart::new("LineString")))?;
                write_coordinates(writer, &ls.0)?;
                writer.write_event(Event::End(BytesEnd::new("LineString")))?;
            }
            writer.write_event(Event::End(BytesEnd::new("MultiGeometry")))?;
        }
        Geometry::Polygon(p) => {
            write_polygon_kml(writer, p.exterior().0.as_slice(), p.interiors())?;
        }
        Geometry::MultiPolygon(mp) => {
            writer.write_event(Event::Start(BytesStart::new("MultiGeometry")))?;
            for p in &mp.0 {
                write_polygon_kml(writer, p.exterior().0.as_slice(), p.interiors())?;
            }
            writer.write_event(Event::End(BytesEnd::new("MultiGeometry")))?;
        }
        Geometry::GeometryCollection(gc) => {
            writer.write_event(Event::Start(BytesStart::new("MultiGeometry")))?;
            for child in &gc.0 {
                write_geometry_kml(writer, child)?;
            }
            writer.write_event(Event::End(BytesEnd::new("MultiGeometry")))?;
        }
        Geometry::Line(l) => {
            writer.write_event(Event::Start(BytesStart::new("LineString")))?;
            write_coordinates(writer, &[l.start, l.end])?;
            writer.write_event(Event::End(BytesEnd::new("LineString")))?;
        }
        Geometry::Rect(r) => {
            let ring = vec![
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
            write_polygon_kml(writer, &ring, &[])?;
        }
        Geometry::Triangle(t) => {
            let ring = vec![t.v1(), t.v2(), t.v3(), t.v1()];
            write_polygon_kml(writer, &ring, &[])?;
        }
    }
    Ok(())
}

fn write_polygon_kml<W: std::io::Write>(
    writer: &mut Writer<W>,
    exterior: &[Coord<f64>],
    interiors: &[LineString<f64>],
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new("Polygon")))?;

    writer.write_event(Event::Start(BytesStart::new("outerBoundaryIs")))?;
    writer.write_event(Event::Start(BytesStart::new("LinearRing")))?;
    write_coordinates(writer, exterior)?;
    writer.write_event(Event::End(BytesEnd::new("LinearRing")))?;
    writer.write_event(Event::End(BytesEnd::new("outerBoundaryIs")))?;

    for ring in interiors {
        writer.write_event(Event::Start(BytesStart::new("innerBoundaryIs")))?;
        writer.write_event(Event::Start(BytesStart::new("LinearRing")))?;
        write_coordinates(writer, &ring.0)?;
        writer.write_event(Event::End(BytesEnd::new("LinearRing")))?;
        writer.write_event(Event::End(BytesEnd::new("innerBoundaryIs")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("Polygon")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

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

fn try_parse_geometry(reader: &mut Reader<&[u8]>, name: &str) -> Option<Geometry<f64>> {
    match name {
        "Point" => parse_point(reader).map(Geometry::Point),
        "LineString" => parse_linestring(reader).map(Geometry::LineString),
        "Polygon" => parse_polygon(reader).map(Geometry::Polygon),
        "MultiGeometry" => {
            let members = parse_multi_members(reader, "MultiGeometry");
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
    let coords = parse_coordinates(reader, &["Point"])?;
    coords.first().map(|c| Point::new(c.x, c.y))
}

fn parse_linestring(reader: &mut Reader<&[u8]>) -> Option<LineString<f64>> {
    let coords = parse_coordinates(reader, &["LineString"])?;
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
                    "outerBoundaryIs" => {
                        exterior = parse_linear_ring(reader);
                    }
                    "innerBoundaryIs" => {
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

fn parse_linear_ring(reader: &mut Reader<&[u8]>) -> Option<LineString<f64>> {
    let coords = parse_coordinates(reader, &["LinearRing"])?;
    Some(LineString::new(coords))
}

/// Gather coordinates from a `<coordinates>` element, stopping when
/// one of `end_tags` is encountered.
fn parse_coordinates(reader: &mut Reader<&[u8]>, end_tags: &[&str]) -> Option<Vec<Coord<f64>>> {
    let buf = &mut Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e);
                if name == "coordinates" {
                    if let Ok(Event::Text(t)) = reader.read_event_into(buf) {
                        let text = String::from_utf8_lossy(t.as_ref());
                        return Some(parse_kml_coords(&text));
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
    None
}

/// Parse KML `<coordinates>` text into `Vec<Coord>`.
///
/// KML coordinates format: `lon,lat[,alt]` tuples separated by whitespace.
/// Altitude is ignored (we use 2D coordinates only).
fn parse_kml_coords(text: &str) -> Vec<Coord<f64>> {
    let mut coords = Vec::new();
    for token in text.split_whitespace() {
        let parts: Vec<&str> = token.split(',').collect();
        if parts.len() >= 2 {
            if let (Ok(lon), Ok(lat)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                coords.push(Coord { x: lon, y: lat });
            }
        }
    }
    coords
}

/// Parse member geometries inside a `MultiGeometry` container.
fn parse_multi_members(reader: &mut Reader<&[u8]>, start_name: &str) -> Vec<Geometry<f64>> {
    let buf = &mut Vec::new();
    let mut members = Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e);
                if matches!(
                    name.as_str(),
                    "Point" | "LineString" | "Polygon" | "MultiGeometry"
                ) {
                    if let Some(geom) = try_parse_geometry(reader, &name) {
                        members.push(geom);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kml_coords_simple() {
        let coords = parse_kml_coords("1.0,2.0 3.0,4.0");
        assert_eq!(coords.len(), 2);
        assert!((coords[0].x - 1.0).abs() < 1e-12);
        assert!((coords[0].y - 2.0).abs() < 1e-12);
        assert!((coords[1].x - 3.0).abs() < 1e-12);
        assert!((coords[1].y - 4.0).abs() < 1e-12);
    }

    #[test]
    fn test_parse_kml_coords_with_altitude() {
        let coords = parse_kml_coords("1.0,2.0,100.0 3.0,4.0,200.0");
        assert_eq!(coords.len(), 2);
        assert!((coords[0].x - 1.0).abs() < 1e-12);
        assert!((coords[0].y - 2.0).abs() < 1e-12);
        // altitude should be stripped
    }

    #[test]
    fn test_parse_kml_coords_empty() {
        let coords = parse_kml_coords("");
        assert!(coords.is_empty());
    }

    fn write_and_read(geom: Geometry<f64>, name: &str) -> Geometry<f64> {
        let path = std::env::temp_dir().join(format!("test_kml_{name}.kml"));
        let ps = path.to_str().unwrap().to_string();
        export_kml(&[geom], &ps).unwrap();
        let loaded = load_kml(&ps).unwrap();
        let _ = std::fs::remove_file(&path);
        loaded.into_iter().next().unwrap()
    }

    #[test]
    fn test_kml_point_roundtrip() {
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
    fn test_kml_linestring_roundtrip() {
        let geom = Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ]));
        let loaded = write_and_read(geom.clone(), "linestring");
        assert_eq!(loaded, geom);
    }

    #[test]
    fn test_kml_polygon_roundtrip() {
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
    fn test_kml_multipoint_roundtrip() {
        let geom = Geometry::MultiPoint(MultiPoint::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ]));
        let loaded = write_and_read(geom.clone(), "multipoint");
        assert_eq!(loaded, geom);
    }
}

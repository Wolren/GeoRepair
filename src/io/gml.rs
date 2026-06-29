#[cfg(feature = "io-gml")]
use crate::core::MakeValidError;
#[cfg(feature = "io-gml")]
use crate::Crs;
#[cfg(feature = "io-gml")]
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
#[cfg(feature = "io-gml")]
use quick_xml::events::Event;
#[cfg(feature = "io-gml")]
use quick_xml::Reader;
#[cfg(feature = "io-gml")]
use std::io::Write;

#[cfg(feature = "io-gml")]
const GML_NS: &str = "http://www.opengis.net/gml/3.2";

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

#[cfg(feature = "io-gml")]
pub fn export_gml_geometry(
    geoms: &[Geometry<f64>],
    path: &str,
    crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    let mut file =
        std::fs::File::create(path).map_err(|e| MakeValidError::IoError(e.to_string()))?;

    let srid = crs.and_then(|c| c.srid());

    writeln!(file, r#"<?xml version="1.0" encoding="UTF-8"?>"#)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;
    writeln!(file, r#"<gml:FeatureCollection xmlns:gml="{GML_NS}">"#)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;
    writeln!(file, r#"  <gml:featureMembers>"#)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;

    for geom in geoms {
        write_geom(&mut file, geom, srid, 4)?;
    }

    writeln!(file, r#"  </gml:featureMembers>"#)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;
    writeln!(file, r#"</gml:FeatureCollection>"#)
        .map_err(|e| MakeValidError::IoError(e.to_string()))?;

    Ok(())
}

#[cfg(feature = "io-gml")]
fn write_geom(
    file: &mut impl Write,
    geom: &Geometry<f64>,
    srid: Option<i32>,
    indent: usize,
) -> Result<(), MakeValidError> {
    let ind = " ".repeat(indent);
    match geom {
        Geometry::Point(p) => {
            let srs = srid
                .map(|s| format!(" srsName=\"urn:ogc:def:crs:EPSG::{s}\""))
                .unwrap_or_default();
            writeln!(
                file,
                "{ind}<gml:Point{srs}><gml:pos>{} {}</gml:pos></gml:Point>",
                p.x(),
                p.y()
            )
            .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::LineString(ls) => {
            let coords = coord_list_str(&ls.0);
            writeln!(
                file,
                "{ind}<gml:LineString><gml:posList>{coords}</gml:posList></gml:LineString>"
            )
            .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::Polygon(poly) => {
            writeln!(file, "{ind}<gml:Polygon>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            write_ring(file, poly.exterior(), "exterior", indent + 2)?;
            for interior in poly.interiors() {
                write_ring(file, interior, "interior", indent + 2)?;
            }
            writeln!(file, "{ind}</gml:Polygon>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::MultiPoint(mp) => {
            writeln!(file, "{ind}<gml:MultiPoint>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            for pt in &mp.0 {
                let srs = srid
                    .map(|s| format!(" srsName=\"urn:ogc:def:crs:EPSG::{s}\""))
                    .unwrap_or_default();
                writeln!(
                    file,
                    "{}  <gml:pointMember><gml:Point{srs}><gml:pos>{} {}</gml:pos></gml:Point></gml:pointMember>",
                    ind, pt.x(), pt.y()
                )
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            }
            writeln!(file, "{ind}</gml:MultiPoint>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::MultiLineString(mls) => {
            writeln!(file, "{ind}<gml:MultiLineString>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            for ls in &mls.0 {
                let coords = coord_list_str(&ls.0);
                writeln!(
                    file,
                    "{ind}  <gml:lineStringMember><gml:LineString><gml:posList>{coords}</gml:posList></gml:LineString></gml:lineStringMember>"
                )
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            }
            writeln!(file, "{ind}</gml:MultiLineString>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::MultiPolygon(mp) => {
            writeln!(file, "{ind}<gml:MultiPolygon>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            for poly in &mp.0 {
                writeln!(file, "{}  <gml:polygonMember>", ind)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))?;
                write_polygon_inner(file, poly, indent + 4)?;
                writeln!(file, "{}  </gml:polygonMember>", ind)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            }
            writeln!(file, "{ind}</gml:MultiPolygon>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::GeometryCollection(gc) => {
            writeln!(file, "{ind}<gml:MultiGeometry>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            for g in &gc.0 {
                writeln!(file, "{}  <gml:geometryMember>", ind)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))?;
                write_geom(file, g, srid, indent + 4)?;
                writeln!(file, "{}  </gml:geometryMember>", ind)
                    .map_err(|e| MakeValidError::IoError(e.to_string()))?;
            }
            writeln!(file, "{ind}</gml:MultiGeometry>")
                .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::Line(l) => {
            let coords = coord_list_str(&[l.start, l.end]);
            writeln!(
                file,
                "{ind}<gml:LineString><gml:posList>{coords}</gml:posList></gml:LineString>"
            )
            .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::Rect(r) => {
            let ring = vec![
                r.min(),
                Coord {
                    x: r.max().x,
                    y: r.min().y,
                },
                r.max(),
                Coord {
                    x: r.min().x,
                    y: r.max().y,
                },
                r.min(),
            ];
            let coords = coord_list_str(&ring);
            writeln!(
                file,
                "{ind}<gml:Polygon><gml:exterior><gml:LinearRing><gml:posList>{coords}</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon>"
            )
            .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
        Geometry::Triangle(t) => {
            let ring = vec![t.v1(), t.v2(), t.v3(), t.v1()];
            let coords = coord_list_str(&ring);
            writeln!(
                file,
                "{ind}<gml:Polygon><gml:exterior><gml:LinearRing><gml:posList>{coords}</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon>"
            )
            .map_err(|e| MakeValidError::IoError(e.to_string()))
        }
    }
}

#[cfg(feature = "io-gml")]
fn write_ring(
    file: &mut impl Write,
    ring: &LineString<f64>,
    role: &str,
    indent: usize,
) -> Result<(), MakeValidError> {
    let ind = " ".repeat(indent);
    let coords = coord_list_str(&ring.0);
    writeln!(
        file,
        "{ind}<gml:{role}><gml:LinearRing><gml:posList>{coords}</gml:posList></gml:LinearRing></gml:{role}>"
    )
    .map_err(|e| MakeValidError::IoError(e.to_string()))
}

#[cfg(feature = "io-gml")]
fn write_polygon_inner(
    file: &mut impl Write,
    poly: &Polygon<f64>,
    indent: usize,
) -> Result<(), MakeValidError> {
    let ind = " ".repeat(indent);
    writeln!(file, "{ind}<gml:Polygon>").map_err(|e| MakeValidError::IoError(e.to_string()))?;
    write_ring(file, poly.exterior(), "exterior", indent + 2)?;
    for interior in poly.interiors() {
        write_ring(file, interior, "interior", indent + 2)?;
    }
    writeln!(file, "{ind}</gml:Polygon>").map_err(|e| MakeValidError::IoError(e.to_string()))
}

#[cfg(feature = "io-gml")]
fn coord_list_str(coords: &[Coord<f64>]) -> String {
    coords
        .iter()
        .map(|c| format!("{} {}", c.x, c.y))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

#[cfg(feature = "io-gml")]
enum ReadOutcome {
    Tag(Vec<u8>),
    Done,
    Skip,
}

#[cfg(feature = "io-gml")]
pub fn load_gml_content(
    content: &str,
) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    let mut reader = Reader::from_str(content);
    let mut buf = Vec::new();
    let mut geoms = Vec::new();
    let mut crs = None;

    loop {
        buf.clear();
        let outcome = match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                if crs.is_none() {
                    crs = extract_srs_crs(e);
                }
                ReadOutcome::Tag(e.local_name().as_ref().to_vec())
            }
            Ok(Event::Empty(ref e)) => {
                if crs.is_none() {
                    crs = extract_srs_crs(e);
                }
                ReadOutcome::Skip
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!("GML parse error: {e}")));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"Point" => {
                    if let Ok(pt) = parse_point(&mut reader, &mut buf) {
                        geoms.push(Geometry::Point(pt));
                    }
                }
                b"LineString" => {
                    if let Ok(ls) = parse_linestring(&mut reader, &mut buf) {
                        geoms.push(Geometry::LineString(ls));
                    }
                }
                b"Polygon" => {
                    if let Ok(poly) = parse_polygon(&mut reader, &mut buf) {
                        geoms.push(Geometry::Polygon(poly));
                    }
                }
                b"MultiPoint" => {
                    if let Ok(mp) = parse_multipoint(&mut reader, &mut buf) {
                        geoms.push(Geometry::MultiPoint(mp));
                    }
                }
                b"MultiLineString" => {
                    if let Ok(mls) = parse_multilinestring(&mut reader, &mut buf) {
                        geoms.push(Geometry::MultiLineString(mls));
                    }
                }
                b"MultiPolygon" => {
                    if let Ok(mp) = parse_multipolygon(&mut reader, &mut buf) {
                        geoms.push(Geometry::MultiPolygon(mp));
                    }
                }
                b"MultiGeometry" => {
                    if let Ok(gc) = parse_multigeometry(&mut reader, &mut buf) {
                        geoms.push(Geometry::GeometryCollection(gc));
                    }
                }
                _ => {}
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }

    Ok((geoms, crs))
}

#[cfg(feature = "io-gml")]
fn extract_srs_crs(e: &quick_xml::events::BytesStart) -> Option<Crs> {
    if let Ok(Some(attr)) = e.try_get_attribute("srsName") {
        if let Ok(val) = attr.unescape_value() {
            if let Some(code) = parse_srs_name(&val) {
                return Some(Crs::from_epsg(code));
            }
        }
    }
    None
}

#[cfg(feature = "io-gml")]
fn parse_srs_name(srs: &str) -> Option<u32> {
    if let Some(pos) = srs.rfind(':') {
        srs[pos + 1..].parse::<u32>().ok()
    } else {
        srs.parse::<u32>().ok()
    }
}

#[cfg(feature = "io-gml")]
fn read_text(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>, end_local: &[u8]) -> String {
    loop {
        buf.clear();
        match reader.read_event_into(buf) {
            Ok(Event::Text(t)) => {
                return t.unescape().unwrap_or_default().to_string();
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == end_local => {
                return String::new();
            }
            Ok(Event::Eof) => return String::new(),
            Err(_) => return String::new(),
            _ => {}
        }
    }
}

#[cfg(feature = "io-gml")]
fn skip_element(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>, name: &[u8]) {
    let mut depth = 1;
    loop {
        buf.clear();
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == name => depth += 1,
            Ok(Event::End(ref e)) if e.local_name().as_ref() == name => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            Ok(Event::Eof) => return,
            Err(_) => return,
            _ => {}
        }
    }
}

#[cfg(feature = "io-gml")]
fn parse_pos_list(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Vec<Coord<f64>> {
    let text = read_text(reader, buf, b"posList");
    let parts: Vec<f64> = text
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    let mut coords = Vec::with_capacity(parts.len() / 2);
    for chunk in parts.chunks(2) {
        if chunk.len() == 2 {
            coords.push(Coord {
                x: chunk[0],
                y: chunk[1],
            });
        }
    }
    coords
}

#[cfg(feature = "io-gml")]
fn parse_pos(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Option<Coord<f64>> {
    let text = read_text(reader, buf, b"pos");
    let parts: Vec<f64> = text
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() >= 2 {
        Some(Coord {
            x: parts[0],
            y: parts[1],
        })
    } else {
        None
    }
}

#[cfg(feature = "io-gml")]
fn parse_linear_ring(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<LineString<f64>, MakeValidError> {
    let mut coords = Vec::new();
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"LinearRing" => ReadOutcome::Done,
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!("GML ring parse: {e}")));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"posList" => coords = parse_pos_list(reader, buf),
                b"pos" => {
                    if let Some(c) = parse_pos(reader, buf) {
                        coords.push(c);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(LineString::new(coords))
}

#[cfg(feature = "io-gml")]
fn parse_point(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Point<f64>, MakeValidError> {
    let mut coord: Option<Coord<f64>> = None;
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"Point" => ReadOutcome::Done,
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!("GML point parse: {e}")));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"pos" => {
                    coord = parse_pos(reader, buf);
                }
                b"posList" => {
                    let coords = parse_pos_list(reader, buf);
                    if let Some(c) = coords.into_iter().next() {
                        coord = Some(c);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    coord
        .map(|c| Point::new(c.x, c.y))
        .ok_or_else(|| MakeValidError::ParseError("GML Point missing pos".into()))
}

#[cfg(feature = "io-gml")]
fn parse_linestring(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<LineString<f64>, MakeValidError> {
    let mut coords = Vec::new();
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"LineString" => ReadOutcome::Done,
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML linestring parse: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"posList" => coords = parse_pos_list(reader, buf),
                b"pos" => {
                    if let Some(c) = parse_pos(reader, buf) {
                        coords.push(c);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(LineString::new(coords))
}

#[cfg(feature = "io-gml")]
fn parse_polygon(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Polygon<f64>, MakeValidError> {
    let mut exterior: Option<LineString<f64>> = None;
    let mut interiors: Vec<LineString<f64>> = Vec::new();

    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"Polygon" => ReadOutcome::Done,
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML polygon parse: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"exterior" => {
                    parse_exterior(reader, buf, &mut exterior)?;
                }
                b"interior" => {
                    let mut ring = None;
                    parse_exterior(reader, buf, &mut ring)?;
                    if let Some(r) = ring {
                        interiors.push(r);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }

    Ok(Polygon::new(
        exterior.unwrap_or(LineString::new(vec![])),
        interiors,
    ))
}

#[cfg(feature = "io-gml")]
fn parse_exterior(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    out: &mut Option<LineString<f64>>,
) -> Result<(), MakeValidError> {
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e))
                if e.local_name().as_ref() == b"exterior"
                    || e.local_name().as_ref() == b"interior" =>
            {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!("GML ring reader: {e}")));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"LinearRing" => {
                    *out = Some(parse_linear_ring(reader, buf)?);
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(())
}

#[cfg(feature = "io-gml")]
fn parse_multipoint(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<MultiPoint<f64>, MakeValidError> {
    let mut points = Vec::new();
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"MultiPoint" => ReadOutcome::Done,
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multipoint parse: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"pointMember" => {
                    parse_point_members(reader, buf, &mut points)?;
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(MultiPoint::new(points))
}

#[cfg(feature = "io-gml")]
fn parse_point_members(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    out: &mut Vec<Point<f64>>,
) -> Result<(), MakeValidError> {
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"pointMember" => ReadOutcome::Done,
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multipoint member: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"Point" => {
                    if let Ok(pt) = parse_point(reader, buf) {
                        out.push(pt);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(())
}

#[cfg(feature = "io-gml")]
fn parse_multilinestring(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<MultiLineString<f64>, MakeValidError> {
    let mut linestrings = Vec::new();
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"MultiLineString" => {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multilinestring parse: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"lineStringMember" => {
                    parse_ls_members(reader, buf, &mut linestrings)?;
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(MultiLineString::new(linestrings))
}

#[cfg(feature = "io-gml")]
fn parse_ls_members(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    out: &mut Vec<LineString<f64>>,
) -> Result<(), MakeValidError> {
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"lineStringMember" => {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multilinestring member: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"LineString" => {
                    if let Ok(ls) = parse_linestring(reader, buf) {
                        out.push(ls);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(())
}

#[cfg(feature = "io-gml")]
fn parse_multipolygon(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<MultiPolygon<f64>, MakeValidError> {
    let mut polygons = Vec::new();
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"MultiPolygon" => {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multipolygon parse: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"polygonMember" => {
                    parse_poly_members(reader, buf, &mut polygons)?;
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(MultiPolygon::new(polygons))
}

#[cfg(feature = "io-gml")]
fn parse_poly_members(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    out: &mut Vec<Polygon<f64>>,
) -> Result<(), MakeValidError> {
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"polygonMember" => {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multipolygon member: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"Polygon" => {
                    if let Ok(poly) = parse_polygon(reader, buf) {
                        out.push(poly);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(())
}

#[cfg(feature = "io-gml")]
fn parse_multigeometry(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<geo::GeometryCollection<f64>, MakeValidError> {
    let mut geoms = Vec::new();
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"MultiGeometry" => {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multigeometry parse: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"geometryMember" => {
                    parse_geom_members(reader, buf, &mut geoms)?;
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(geo::GeometryCollection(geoms))
}

#[cfg(feature = "io-gml")]
fn parse_geom_members(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    out: &mut Vec<Geometry<f64>>,
) -> Result<(), MakeValidError> {
    loop {
        buf.clear();
        let outcome = match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => ReadOutcome::Tag(e.local_name().as_ref().to_vec()),
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"geometryMember" => {
                ReadOutcome::Done
            }
            Ok(Event::Eof) => ReadOutcome::Done,
            Err(e) => {
                return Err(MakeValidError::ParseError(format!(
                    "GML multigeometry member: {e}"
                )));
            }
            _ => ReadOutcome::Skip,
        };
        match outcome {
            ReadOutcome::Tag(tag) => match tag.as_slice() {
                b"Point" => {
                    if let Ok(pt) = parse_point(reader, buf) {
                        out.push(Geometry::Point(pt));
                    }
                }
                b"LineString" => {
                    if let Ok(ls) = parse_linestring(reader, buf) {
                        out.push(Geometry::LineString(ls));
                    }
                }
                b"Polygon" => {
                    if let Ok(poly) = parse_polygon(reader, buf) {
                        out.push(Geometry::Polygon(poly));
                    }
                }
                b"MultiPoint" => {
                    if let Ok(mp) = parse_multipoint(reader, buf) {
                        out.push(Geometry::MultiPoint(mp));
                    }
                }
                b"MultiLineString" => {
                    if let Ok(mls) = parse_multilinestring(reader, buf) {
                        out.push(Geometry::MultiLineString(mls));
                    }
                }
                b"MultiPolygon" => {
                    if let Ok(mp) = parse_multipolygon(reader, buf) {
                        out.push(Geometry::MultiPolygon(mp));
                    }
                }
                b"MultiGeometry" => {
                    if let Ok(gc) = parse_multigeometry(reader, buf) {
                        out.extend(gc.0);
                    }
                }
                _ => {
                    skip_element(reader, buf, &tag);
                }
            },
            ReadOutcome::Done => break,
            ReadOutcome::Skip => {}
        }
    }
    Ok(())
}

#[cfg(not(feature = "io-gml"))]
pub fn load_gml_content(
    _content: &str,
) -> Result<(Vec<Geometry<f64>>, Option<Crs>), MakeValidError> {
    Err(MakeValidError::UnsupportedFormat(
        "GML loading requires 'io-gml' feature".into(),
    ))
}

#[cfg(not(feature = "io-gml"))]
pub fn export_gml_geometry(
    _geoms: &[Geometry<f64>],
    _path: &str,
    _crs: Option<&Crs>,
) -> Result<(), MakeValidError> {
    Err(MakeValidError::UnsupportedFormat(
        "GML export requires 'io-gml' feature".into(),
    ))
}

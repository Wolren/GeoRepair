//! GML (`.gml`) backend: OGC GML 3.2 geometry subset via `quick-xml`.
//!
//! Reads namespace-agnostic `Point`, `LineString`, `Polygon`, `MultiPoint`,
//! `MultiLineString`, `MultiPolygon`, and `MultiSurface` elements with
//! `pos`/`posList` coordinates (2D; extra ordinates are ignored). Writes a
//! minimal GML 3.2 feature collection of the same element set. The
//! `srsDimension`/`srsName` attributes are accepted and ignored on read.

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use std::fs;
use std::io::Write;

use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, Point, Polygon};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// Load every GML geometry from a file (any element in a `gml` namespace).
pub fn load_gml(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    let data = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let mut reader = Reader::from_reader(data.as_slice());
    let mut out = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("{path}: {e}"))?
        {
            Event::Start(e) if is_geometry(local(e.name())) => {
                out.push(parse_geometry(&mut reader, local(e.name()))?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Save geometries as a GML 3.2 feature collection.
pub fn save_gml(path: &str, geoms: &[Geometry<f64>]) -> Result<(), String> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <gml:FeatureCollection xmlns:gml=\"http://www.opengis.net/gml/3.2\">\n",
    );
    for (i, g) in geoms.iter().enumerate() {
        out.push_str("  <gml:featureMember>\n");
        write_geometry(&mut out, g, i, "    ")?;
        out.push_str("  </gml:featureMember>\n");
    }
    out.push_str("</gml:FeatureCollection>\n");
    let mut f = fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    f.write_all(out.as_bytes())
        .map_err(|e| format!("{path}: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Local (namespace-stripped) element name from a QName.
fn local<'a>(n: quick_xml::name::QName<'a>) -> &'a [u8] {
    let b: &'a [u8] = n.0;
    match b.iter().rposition(|&c| c == b':') {
        Some(i) => &b[i + 1..],
        None => b,
    }
}

fn is_geometry(local: &[u8]) -> bool {
    matches!(
        local,
        b"Point"
            | b"LineString"
            | b"Polygon"
            | b"MultiPoint"
            | b"MultiLineString"
            | b"MultiPolygon"
            | b"MultiSurface"
    )
}

/// Parse one geometry element; `local` is the element's local name. Consumes
/// events through the geometry's matching end tag.
fn parse_geometry(
    reader: &mut Reader<&[u8]>,
    local: &[u8],
) -> Result<Geometry<f64>, String> {
    match local {
        b"Point" => {
            let coords = read_pos(reader, b"Point")?;
            Ok(Geometry::Point(Point::new(coords[0], coords[1])))
        }
        b"LineString" => {
            let coords = read_pos_list(reader, b"LineString")?;
            Ok(Geometry::LineString(LineString(coords)))
        }
        b"Polygon" => {
            let (ext, holes) = read_rings(reader, b"Polygon")?;
            let exterior = ext.ok_or_else(|| "Polygon without exterior ring".to_string())?;
            Ok(Geometry::Polygon(Polygon::new(exterior, holes)))
        }
        b"MultiPoint" => {
            let pts = read_members(reader, b"MultiPoint", b"Point")?;
            Ok(Geometry::MultiPoint(MultiPoint(
                pts.into_iter()
                    .filter_map(|g| match g {
                        Geometry::Point(p) => Some(p),
                        _ => None,
                    })
                    .collect(),
            )))
        }
        b"MultiLineString" => {
            let ls = read_members(reader, b"MultiLineString", b"LineString")?;
            Ok(Geometry::MultiLineString(MultiLineString(
                ls.into_iter()
                    .filter_map(|g| match g {
                        Geometry::LineString(l) => Some(l),
                        _ => None,
                    })
                    .collect(),
            )))
        }
        b"MultiPolygon" | b"MultiSurface" => {
            let member = if local == b"MultiSurface" {
                b"Surface"
            } else {
                b"Polygon"
            };
            let polys = read_members(reader, local, member)?;
            Ok(Geometry::MultiPolygon(geo::MultiPolygon(
                polys.into_iter()
                    .filter_map(|g| match g {
                        Geometry::Polygon(p) => Some(p),
                        _ => None,
                    })
                    .collect(),
            )))
        }
        _ => Err(format!(
            "unsupported GML geometry element: {}",
            String::from_utf8_lossy(local)
        )),
    }
}

/// Read a `<pos>` element: exactly one coordinate tuple (honors
/// `srsDimension` on the element; extra ordinates are dropped).
fn read_pos(reader: &mut Reader<&[u8]>, container: &[u8]) -> Result<Vec<f64>, String> {
    let mut nums = Vec::new();
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if local(e.name()) == b"pos" => {
                nums.extend(collect_numbers(reader, b"pos")?);
            }
            Event::Empty(e) if local(e.name()) == b"pos" => {}
            Event::Start(_) => skip_element(reader)?,
            Event::End(e) if local(e.name()) == container => break,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    if nums.len() < 2 {
        return Err("GML Point without coordinates".to_string());
    }
    Ok(nums)
}

/// Read a `<posList>` element: a flat list of coordinate tuples (honors
/// `srsDimension` on the element; extra ordinates are dropped).
fn read_pos_list(
    reader: &mut Reader<&[u8]>,
    container: &[u8],
) -> Result<Vec<Coord<f64>>, String> {
    let mut nums = Vec::new();
    let mut dim = 2usize;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if local(e.name()) == b"posList" => {
                dim = dimension_of(&e).max(2);
                nums.extend(collect_numbers(reader, b"posList")?);
            }
            Event::Start(_) => skip_element(reader)?,
            Event::End(e) if local(e.name()) == container => break,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    coords_from_flat(&nums, container, dim)
}

/// Collect all whitespace-separated numbers from the text of one element.
fn collect_numbers(reader: &mut Reader<&[u8]>, elem: &[u8]) -> Result<Vec<f64>, String> {
    let mut nums = Vec::new();
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Text(t) => {
                let text = t.decode().map_err(|e| e.to_string())?;
                for tok in text.split_whitespace() {
                    let v = parse_gml_number(tok)
                        .map_err(|_| format!("bad coordinate `{tok}` in GML"))?;
                    nums.push(v);
                }
            }
            Event::End(e) if local(e.name()) == elem => break,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    Ok(nums)
}

/// A polygon's rings: optional exterior plus holes.
type RingSet = (Option<LineString<f64>>, Vec<LineString<f64>>);

/// Read a Polygon's `<exterior>`/`<interior>` rings.
fn read_rings(
    reader: &mut Reader<&[u8]>,
    container: &[u8],
) -> Result<RingSet, String> {
    let mut exterior = None;
    let mut holes = Vec::new();
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if local(e.name()) == b"exterior" => {
                exterior = Some(read_linear_ring(reader)?);
            }
            Event::Start(e) if local(e.name()) == b"interior" => {
                holes.push(read_linear_ring(reader)?);
            }
            Event::Start(_) => skip_element(reader)?,
            Event::End(e) if local(e.name()) == container => break,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    Ok((exterior, holes))
}

/// Read one `<LinearRing>` (a posList inside).
fn read_linear_ring(reader: &mut Reader<&[u8]>) -> Result<LineString<f64>, String> {
    let mut nums = Vec::new();
    let mut in_ring = false;
    let mut dim = 2usize;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if local(e.name()) == b"LinearRing" => in_ring = true,
            Event::Start(e) if local(e.name()) == b"posList" && in_ring => {
                dim = dimension_of(&e).max(2);
                nums.extend(collect_numbers(reader, b"posList")?);
            }
            Event::Start(_) => skip_element(reader)?,
            Event::End(e) if local(e.name()) == b"LinearRing" => break,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    Ok(LineString(coords_from_flat(&nums, b"LinearRing", dim)?))
}

/// Read the `srsDimension` attribute of a start tag (default 2).
fn dimension_of(e: &BytesStart) -> usize {
    e.attributes().flatten().find_map(|a| {
        if a.key.0 == b"srsDimension" {
            std::str::from_utf8(&a.value)
                .ok()
                .and_then(|s| s.trim().parse::<usize>().ok())
        } else {
            None
        }
    }).unwrap_or(2)
}

/// Read a multi-geometry: every member element (e.g. `polygonMember`)
/// wrapping a geometry element (`Polygon`, `Surface`, ...).
fn read_members(
    reader: &mut Reader<&[u8]>,
    container: &[u8],
    _member_geom: &[u8],
) -> Result<Vec<Geometry<f64>>, String> {
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let l = local(e.name());
                if is_geometry(l) {
                    out.push(parse_geometry(reader, l)?);
                } else {
                    // Member wrapper (or unknown container): descend one
                    // level, parse the first geometry found inside, then
                    // consume the wrapper's end tag.
                    loop {
                        match reader.read_event().map_err(|e| e.to_string())? {
                            Event::Start(inner) if is_geometry(local(inner.name())) => {
                                let gl = local(inner.name());
                                out.push(parse_geometry(reader, gl)?);
                            }
                            Event::Start(_) => skip_element(reader)?,
                            Event::End(inner) if local(inner.name()) == l => break,
                            Event::Eof => return Err("unexpected EOF in GML".to_string()),
                            _ => {}
                        }
                    }
                }
            }
            Event::End(e) if local(e.name()) == container => break,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    Ok(out)
}

/// Skip an element and everything nested inside it (depth-based; correct
/// even when element names repeat at different nesting levels).
fn skip_element(reader: &mut Reader<&[u8]>) -> Result<(), String> {
    let mut depth = 1usize;
    while depth > 0 {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            Event::Eof => return Err("unexpected EOF in GML".to_string()),
            _ => {}
        }
    }
    Ok(())
}

/// Turn a flat number list into coordinates, validating the count against
/// the declared dimension (3D ordinates are dropped).
fn coords_from_flat(
    nums: &[f64],
    ctx: &[u8],
    dim: usize,
) -> Result<Vec<Coord<f64>>, String> {
    if nums.is_empty() {
        return Ok(Vec::new());
    }
    if !nums.len().is_multiple_of(dim) {
        return Err(format!(
            "GML {}: coordinate count {} not divisible by dimension {dim}",
            String::from_utf8_lossy(ctx),
            nums.len()
        ));
    }
    Ok(nums
        .chunks_exact(dim)
        .map(|c| Coord { x: c[0], y: c[1] })
        .collect())
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

fn write_geometry(
    out: &mut String,
    g: &Geometry<f64>,
    id: usize,
    indent: &str,
) -> Result<(), String> {
    match g {
        Geometry::Point(p) => {
            out.push_str(&format!(
                "{indent}<gml:Point gml:id=\"p{id}\"><gml:pos>{} {}</gml:pos></gml:Point>\n",
                fmt(p.x()),
                fmt(p.y())
            ));
        }
        Geometry::LineString(ls) => {
            out.push_str(&format!(
                "{indent}<gml:LineString gml:id=\"l{id}\"><gml:posList>{}</gml:posList></gml:LineString>\n",
                fmt_list(&ls.0)
            ));
        }
        Geometry::Polygon(p) => write_polygon(out, p, id, indent)?,
        Geometry::MultiPoint(mp) => {
            out.push_str(&format!(
                "{indent}<gml:MultiPoint gml:id=\"mp{id}\">\n"
            ));
            for p in &mp.0 {
                out.push_str(&format!(
                    "{indent}  <gml:pointMember><gml:Point><gml:pos>{} {}</gml:pos></gml:Point></gml:pointMember>\n",
                    fmt(p.x()),
                    fmt(p.y())
                ));
            }
            out.push_str(&format!("{indent}</gml:MultiPoint>\n"));
        }
        Geometry::MultiLineString(ml) => {
            out.push_str(&format!(
                "{indent}<gml:MultiLineString gml:id=\"ml{id}\">\n"
            ));
            for ls in &ml.0 {
                out.push_str(&format!(
                    "{indent}  <gml:lineStringMember><gml:LineString><gml:posList>{}</gml:posList></gml:LineString></gml:lineStringMember>\n",
                    fmt_list(&ls.0)
                ));
            }
            out.push_str(&format!("{indent}</gml:MultiLineString>\n"));
        }
        Geometry::MultiPolygon(mp) => {
            out.push_str(&format!(
                "{indent}<gml:MultiPolygon gml:id=\"mpoly{id}\">\n"
            ));
            for (j, p) in mp.0.iter().enumerate() {
                let sub = format!("{indent}  ");
                out.push_str(&format!("{sub}<gml:polygonMember>\n"));
                write_polygon(out, p, id * 100 + j, &format!("{sub}  "))?;
                out.push_str(&format!("{sub}</gml:polygonMember>\n"));
            }
            out.push_str(&format!("{indent}</gml:MultiPolygon>\n"));
        }
        Geometry::GeometryCollection(gc) => {
            for (j, sub) in gc.0.iter().enumerate() {
                write_geometry(out, sub, id * 100 + j, indent)?;
            }
        }
        Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::Line(_) => {
            return Err("GML writer: unsupported geometry type".to_string())
        }
    }
    Ok(())
}

fn write_polygon(
    out: &mut String,
    p: &Polygon<f64>,
    id: usize,
    indent: &str,
) -> Result<(), String> {
    out.push_str(&format!("{indent}<gml:Polygon gml:id=\"poly{id}\">\n"));
    out.push_str(&format!(
        "{indent}  <gml:exterior><gml:LinearRing><gml:posList>{}</gml:posList></gml:LinearRing></gml:exterior>\n",
        fmt_list(&p.exterior().0)
    ));
    for h in p.interiors() {
        out.push_str(&format!(
            "{indent}  <gml:interior><gml:LinearRing><gml:posList>{}</gml:posList></gml:LinearRing></gml:interior>\n",
            fmt_list(&h.0)
        ));
    }
    out.push_str(&format!("{indent}</gml:Polygon>\n"));
    Ok(())
}

fn fmt(v: f64) -> String {
    // ryu: shortest round-trip formatting (same as the WKT writer);
    // std's Display float path is 5-10x slower per number.
    let mut buf = ryu::Buffer::new();
    buf.format(v).to_string()
}

/// GML coordinate tokens: decimal numbers via fast_float (correctly
/// rounded, strtod-equivalent - same as the WKT reader), with the
/// NaN/inf keyword forms still routed through std's parser for full
/// strtod parity (fast_float has no keyword form).
fn parse_gml_number(tok: &str) -> Result<f64, ()> {
    let b = tok.as_bytes();
    let body = if let Some(rest) = b.strip_prefix(b"+").or_else(|| b.strip_prefix(b"-")) {
        rest
    } else {
        b
    };
    let kw = |k: &[u8]| body.len() == k.len() && body.eq_ignore_ascii_case(k);
    if kw(b"nan") || kw(b"inf") || kw(b"infinity") {
        return tok.parse::<f64>().map_err(|_| ());
    }
    fast_float2::parse::<f64, _>(b).map_err(|_| ())
}

fn fmt_list(coords: &[Coord<f64>]) -> String {
    coords
        .iter()
        .map(|c| format!("{} {}", fmt(c.x), fmt(c.y)))
        .collect::<Vec<_>>()
        .join(" ")
}

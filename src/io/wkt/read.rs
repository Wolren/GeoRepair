//! WKT parser: single-pass tokenizer + reader.

use alloc::string::String;
use alloc::vec::Vec;
/// Each level consumes at least ~14 bytes of input. The cap never rejects real data
/// while bounding recursion (stack overflow is an uncatchable abort).
const MAX_WKT_NESTING: usize = 256;

use super::*;
#[cfg(feature = "std")]
use std::io::Read;

pub(crate) struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    /// Extra ordinates in flat coordinate order, kept only when the caller asks.
    extra: Vec<f64>,
    keep_extra: bool,
    /// Dimension of the outer geometry; nested members compare against it.
    root_dims: Option<Dims>,
    /// Position and dimension of the first member that disagrees with `root_dims`.
    mixed: Option<(usize, Dims)>,
}

impl<'a> Parser<'a> {
    fn new(s: &'a [u8], keep_extra: bool) -> Self {
        Parser {
            s,
            i: 0,
            extra: Vec::new(),
            keep_extra,
            root_dims: None,
            mixed: None,
        }
    }

    fn peek(&self) -> u8 {
        self.s[self.i]
    }

    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn err(&self, msg: &str) -> WktError {
        let ctx_start = self.i.saturating_sub(20);
        let ctx = String::from_utf8_lossy(&self.s[ctx_start..self.s.len().min(self.i + 20)]);
        WktError::ParseError {
            pos: self.i,
            message: format!("{msg}\n  near: {ctx}"),
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), WktError> {
        self.skip_ws();
        if self.i >= self.s.len() || self.s[self.i] != c {
            return Err(self.err(&format!("expected '{}'", c as char)));
        }
        self.i += 1;
        Ok(())
    }

    fn peek_keyword(&mut self) -> Result<(Keyword, Dims, bool), WktError> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        let kw = &self.s[start..self.i];
        let (mut dims, mut explicit) = self.read_dims();
        let kw_upper: Vec<u8> = kw.iter().map(|b| b.to_ascii_uppercase()).collect();
        let full = kw_upper.as_slice();
        let (kw, compact) = match split_compact_modifier(full) {
            Some((base, d)) => (base, Some(d)),
            None => (full, None),
        };
        if let Some(d) = compact {
            if explicit {
                return Err(self.err("a type takes one dimension modifier, not two"));
            }
            dims = d;
            explicit = true;
        }
        let kw = match kw {
            b"POINT" => Keyword::Point,
            b"LINESTRING" => Keyword::LineString,
            b"LINEARRING" => Keyword::LineString,
            b"POLYGON" => Keyword::Polygon,
            b"MULTIPOINT" => Keyword::MultiPoint,
            b"MULTILINESTRING" => Keyword::MultiLineString,
            b"MULTIPOLYGON" => Keyword::MultiPolygon,
            b"GEOMETRYCOLLECTION" => Keyword::GeometryCollection,
            _ => {
                return Err(self.err(&format!(
                    "unknown geometry type '{}'",
                    String::from_utf8_lossy(kw)
                )));
            }
        };
        Ok((kw, dims, explicit))
    }

    /// Reads the optional Z, M or ZM modifier that follows a geometry keyword.
    fn read_dims(&mut self) -> (Dims, bool) {
        self.skip_ws();
        let s = self.s;
        let rest = &s[self.i..];
        if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case(b"ZM") {
            self.i += 2;
            (Dims::XYZM, true)
        } else if !rest.is_empty() && rest[..1].eq_ignore_ascii_case(b"Z") {
            self.i += 1;
            (Dims::XYZ, true)
        } else if !rest.is_empty() && rest[..1].eq_ignore_ascii_case(b"M") {
            self.i += 1;
            (Dims::XYM, true)
        } else {
            (Dims::XY, false)
        }
    }

    fn read_f64(&mut self) -> Result<f64, WktError> {
        self.skip_ws();
        let start = self.i;
        if self.i >= self.s.len() {
            return Err(self.err("expected number"));
        }

        let bytes = &self.s;
        let n = bytes.len();
        if self.i < n && (bytes[self.i] == b'-' || bytes[self.i] == b'+') {
            self.i += 1;
        }

        // NaN / inf / Infinity, strtod-style: optional sign, case-insensitive.
        if self.i < n && bytes[self.i].is_ascii_alphabetic() {
            let rest = &bytes[self.i..];
            let kw_len = if rest.len() >= 8 && rest[..8].eq_ignore_ascii_case(b"infinity") {
                8
            } else if (rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"inf"))
                || (rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"nan"))
            {
                3
            } else {
                return Err(self.err("expected number"));
            };
            self.i += kw_len;
            return core::str::from_utf8(&self.s[start..self.i])
                .map_err(|_| self.err("expected number"))?
                .parse::<f64>()
                .map_err(|_| self.err("expected number"));
        }

        let mut parsed_any = false;
        while self.i < n && bytes[self.i].is_ascii_digit() {
            parsed_any = true;
            self.i += 1;
        }

        if self.i < n && bytes[self.i] == b'.' {
            self.i += 1;
            while self.i < n && bytes[self.i].is_ascii_digit() {
                parsed_any = true;
                self.i += 1;
            }
        }

        if !parsed_any {
            return Err(self.err("expected digit"));
        }

        if self.i < n && (bytes[self.i] == b'e' || bytes[self.i] == b'E') {
            self.i += 1;
            if self.i < n && (bytes[self.i] == b'-' || bytes[self.i] == b'+') {
                self.i += 1;
            }
            if self.i >= n || !bytes[self.i].is_ascii_digit() {
                return Err(self.err("expected exponent digit"));
            }
            while self.i < n && bytes[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }

        // an Eisel-Lemire implementation (correctly rounded, ~2x faster than std's
        // float parsing - measured 2026-08-04: float parsing is ~88% of WKT) Note: hex
        // floats ("0x1p3") are rejected - WKT has no hex form and GEOS never emits one.
        fast_float2::parse::<f64, _>(&self.s[start..self.i])
            .map_err(|_| self.err("expected number"))
    }

    fn read_coord(&mut self, dims: Dims) -> Result<Coord<f64>, WktError> {
        let x = self.read_f64()?;
        let y = self.read_f64()?;
        if dims.has_z {
            let z = self.read_f64()?;
            if self.keep_extra {
                self.extra.push(z);
            }
        }
        if dims.has_m {
            let m = self.read_f64()?;
            if self.keep_extra {
                self.extra.push(m);
            }
        }
        Ok(Coord { x, y })
    }

    fn read_coord_list(&mut self, dims: Dims) -> Result<Vec<Coord<f64>>, WktError> {
        self.skip_ws();
        let mut coords = Vec::new();
        coords.push(self.read_coord(dims)?);
        loop {
            self.skip_ws();
            if self.i >= self.s.len() || (self.s[self.i] != b',' && self.s[self.i] != b' ') {
                break;
            }
            if self.s[self.i] == b',' {
                self.i += 1;
            }
            self.skip_ws();
            if self.i >= self.s.len() || self.s[self.i] == b')' {
                break;
            }
            coords.push(self.read_coord(dims)?);
        }
        Ok(coords)
    }

    fn parse_point(&mut self, dims: Dims) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.peek() == b'E' || self.peek() == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::Point(Point(Coord {
                    x: f64::NAN,
                    y: f64::NAN,
                })));
            }
        }
        self.expect(b'(')?;
        let c = self.read_coord(dims)?;
        self.expect(b')')?;
        Ok(Geometry::Point(Point(c)))
    }

    fn parse_linestring(&mut self, dims: Dims) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::LineString(LineString::new(vec![])));
            }
        }
        self.expect(b'(')?;
        let coords = self.read_coord_list(dims)?;
        self.expect(b')')?;
        Ok(Geometry::LineString(LineString::new(coords)))
    }

    fn parse_polygon(&mut self, dims: Dims) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::Polygon(Polygon::new(
                    LineString::new(vec![]),
                    vec![],
                )));
            }
        }
        self.expect(b'(')?;
        let mut rings = Vec::new();
        let mut saw_any = false;
        loop {
            self.skip_ws();
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            if !rings.is_empty() {
                if self.i < self.s.len() && self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    rings.push(LineString::new(vec![]));
                    saw_any = true;
                    self.skip_ws();
                    if self.i < self.s.len() && self.s[self.i] == b',' {
                        self.i += 1;
                    }
                    continue;
                }
            }
            if self.i < self.s.len() && self.s[self.i] == b'(' {
                let rest = &self.s[self.i + 1..];
                if rest.len() >= 5 && rest[..5].eq_ignore_ascii_case(b"EMPTY") {
                    let mut j = self.i + 6;
                    while j < self.s.len() && self.s[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < self.s.len() && self.s[j] == b')' {
                        self.i = j + 1;
                        rings.push(LineString::new(vec![]));
                        saw_any = true;
                        continue;
                    }
                }
            }
            self.expect(b'(')?;
            let coords = self.read_coord_list(dims)?;
            self.expect(b')')?;
            rings.push(LineString::new(coords));
            saw_any = true;
        }
        self.expect(b')')?;
        if !saw_any {
            return Err(self.err("expected at least one ring"));
        }
        if rings.is_empty() {
            return Ok(Geometry::Polygon(Polygon::new(
                LineString::new(vec![]),
                vec![],
            )));
        }
        let exterior = rings.remove(0);
        Ok(Geometry::Polygon(Polygon::new(exterior, rings)))
    }

    fn parse_multipoint(&mut self, dims: Dims) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::MultiPoint(MultiPoint(vec![])));
            }
        }
        self.expect(b'(')?;
        let mut points = Vec::new();
        let mut saw_any = false;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() || self.s[self.i] == b')' {
                break;
            }
            // Geo's empty-point convention is NaN coords.
            if self.s[self.i] == b'E' || self.s[self.i] == b'e' {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    points.push(Point::new(f64::NAN, f64::NAN));
                    saw_any = true;
                    self.skip_ws();
                    if self.i < self.s.len() && self.s[self.i] == b',' {
                        self.i += 1;
                    }
                    continue;
                }
            }
            let c = if self.s[self.i] == b'(' {
                self.i += 1;
                let c = self.read_coord(dims)?;
                self.expect(b')')?;
                c
            } else {
                self.read_coord(dims)?
            };
            points.push(Point(c));
            saw_any = true;
            self.skip_ws();
            if self.i < self.s.len() && self.s[self.i] == b',' {
                self.i += 1;
            }
        }
        self.expect(b')')?;
        if !saw_any {
            return Err(self.err("expected at least one point"));
        }
        Ok(Geometry::MultiPoint(MultiPoint(points)))
    }

    fn parse_multilinestring(&mut self, dims: Dims) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::MultiLineString(MultiLineString(vec![])));
            }
        }
        self.expect(b'(')?;
        let mut lines = Vec::new();
        let mut saw_any = false;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() || self.s[self.i] == b')' {
                break;
            }
            if !lines.is_empty() {
                if self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    lines.push(LineString::new(vec![]));
                    saw_any = true;
                    continue;
                }
            }
            self.expect(b'(')?;
            let coords = self.read_coord_list(dims)?;
            self.expect(b')')?;
            lines.push(LineString::new(coords));
            saw_any = true;
        }
        self.expect(b')')?;
        if !saw_any {
            return Err(self.err("expected at least one line"));
        }
        Ok(Geometry::MultiLineString(MultiLineString(lines)))
    }

    fn parse_multipolygon(&mut self, dims: Dims) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::MultiPolygon(MultiPolygon(vec![])));
            }
        }
        self.expect(b'(')?;
        let mut polys = Vec::new();
        let mut saw_any = false;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() || self.s[self.i] == b')' {
                break;
            }
            if !polys.is_empty() {
                if self.i < self.s.len() && self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    polys.push(Polygon::new(LineString::new(vec![]), vec![]));
                    saw_any = true;
                    continue;
                }
            }
            self.expect(b'(')?;
            let mut rings = Vec::new();
            let mut saw_ring = false;
            loop {
                self.skip_ws();
                if self.i < self.s.len() && self.s[self.i] == b')' {
                    break;
                }
                if !rings.is_empty() {
                    if self.i < self.s.len() && self.s[self.i] == b',' {
                        self.i += 1;
                    }
                    self.skip_ws();
                }
                if self.i < self.s.len() && self.s[self.i] == b')' {
                    break;
                }
                if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                    let rest = &self.s[self.i..];
                    if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                        self.i += 5;
                        rings.push(LineString::new(vec![]));
                        saw_ring = true;
                        continue;
                    }
                }
                if self.i < self.s.len() && self.s[self.i] == b'(' {
                    let rest = &self.s[self.i + 1..];
                    if rest.len() >= 5 && rest[..5].eq_ignore_ascii_case(b"EMPTY") {
                        let mut j = self.i + 6;
                        while j < self.s.len() && self.s[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < self.s.len() && self.s[j] == b')' {
                            self.i = j + 1;
                            rings.push(LineString::new(vec![]));
                            saw_ring = true;
                            continue;
                        }
                    }
                }
                self.expect(b'(')?;
                let coords = self.read_coord_list(dims)?;
                self.expect(b')')?;
                rings.push(LineString::new(coords));
                saw_ring = true;
            }
            self.expect(b')')?;
            if !saw_ring {
                return Err(self.err("expected at least one ring"));
            }
            let exterior = if rings.is_empty() {
                LineString::new(vec![])
            } else {
                rings.remove(0)
            };
            polys.push(Polygon::new(exterior, rings));
            saw_any = true;
        }
        self.expect(b')')?;
        if !saw_any {
            return Err(self.err("expected at least one polygon"));
        }
        Ok(Geometry::MultiPolygon(MultiPolygon(polys)))
    }

    fn parse_geometrycollection(
        &mut self,
        depth: usize,
        dims: Dims,
    ) -> Result<Geometry<f64>, WktError> {
        self.skip_ws();
        if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
            let rest = &self.s[self.i..];
            if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                self.i += 5;
                return Ok(Geometry::GeometryCollection(GeometryCollection(vec![])));
            }
        }
        self.expect(b'(')?;
        let mut geoms = Vec::new();
        let mut saw_any = false;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() || self.s[self.i] == b')' {
                break;
            }
            if !geoms.is_empty() {
                if self.i < self.s.len() && self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            // empty element (geometrycollection (empty, point (1 2))) -an empty point
            // component (geo convention: NaN coords).
            if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    geoms.push(Geometry::Point(Point::new(f64::NAN, f64::NAN)));
                    saw_any = true;
                    continue;
                }
            }
            geoms.push(self.parse_any(depth + 1, dims)?);
            saw_any = true;
        }
        self.expect(b')')?;
        if !saw_any {
            return Err(self.err("expected at least one element"));
        }
        Ok(Geometry::GeometryCollection(GeometryCollection(geoms)))
    }

    fn parse_any(&mut self, depth: usize, inherited: Dims) -> Result<Geometry<f64>, WktError> {
        // A crafted document can nest arbitrarily deep and overflow the stack
        // (uncatchable abort).
        if depth > MAX_WKT_NESTING {
            return Err(self.err("geometry nesting exceeds limit"));
        }
        self.skip_ws();
        let header_pos = self.i;
        let (kw, header_dims, explicit) = self.peek_keyword()?;
        // A member without a modifier inherits the collection dimension, as GEOS does.
        let dims = if explicit { header_dims } else { inherited };
        if self.keep_extra {
            match self.root_dims {
                None => self.root_dims = Some(dims),
                Some(root) if root != dims && self.mixed.is_none() => {
                    self.mixed = Some((header_pos, dims));
                }
                Some(_) => {}
            }
        }
        match kw {
            Keyword::Point => self.parse_point(dims),
            Keyword::LineString => self.parse_linestring(dims),
            Keyword::Polygon => self.parse_polygon(dims),
            Keyword::MultiPoint => self.parse_multipoint(dims),
            Keyword::MultiLineString => self.parse_multilinestring(dims),
            Keyword::MultiPolygon => self.parse_multipolygon(dims),
            Keyword::GeometryCollection => self.parse_geometrycollection(depth, dims),
        }
    }
}

pub(crate) enum Keyword {
    Point,
    LineString,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
    GeometryCollection,
}

fn parse_document(s: &[u8], keep_extra: bool) -> Result<(Parser<'_>, Geometry<f64>), WktError> {
    let mut p = Parser::new(s, keep_extra);
    p.skip_ws();
    if p.i >= p.s.len() {
        return Err(WktError::EmptyInput);
    }
    let geometry = p.parse_any(0, Dims::XY)?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(WktError::TrailingCharacters { pos: p.i });
    }
    Ok((p, geometry))
}

/// Parse a WKT string into a `Geometry<f64>`.
///
/// Z, M and ZM modifiers are read and their ordinates dropped, because `geo::Coord`
/// holds x and y only. Use [`read_wkt_zm`] to keep them.
pub fn read_wkt(input: &str) -> Result<Geometry<f64>, WktError> {
    parse_document(input.as_bytes(), false).map(|(_, geometry)| geometry)
}

/// Read a WKT geometry from any `io::Read` source.
#[cfg(feature = "std")]
pub fn read_wkt_from(mut reader: impl Read) -> Result<Geometry<f64>, WktError> {
    let mut s = String::new();
    reader.read_to_string(&mut s).map_err(WktError::IoError)?;
    read_wkt(&s)
}

/// Parse WKT and keep its dimension modifier and extra ordinates.
///
/// A collection whose members carry different modifiers returns
/// [`WktError::MixedDimensions`], because one `dims` value describes the whole geometry.
pub fn read_wkt_zm(input: &str) -> Result<WktGeometry, WktError> {
    let (p, geometry) = parse_document(input.as_bytes(), true)?;
    if let Some((pos, found)) = p.mixed {
        return Err(WktError::MixedDimensions {
            pos,
            outer: modifier(p.root_dims.unwrap_or(Dims::XY)).unwrap_or("XY"),
            found: modifier(found).unwrap_or("XY"),
        });
    }
    Ok(WktGeometry {
        geometry,
        dims: p.root_dims.unwrap_or(Dims::XY),
        extra_coords: p.extra,
    })
}

/// Read a WKT geometry with its modifier from any `io::Read` source.
#[cfg(feature = "std")]
pub fn read_wkt_zm_from(mut reader: impl Read) -> Result<WktGeometry, WktError> {
    let mut s = String::new();
    reader.read_to_string(&mut s).map_err(WktError::IoError)?;
    read_wkt_zm(&s)
}

/// Peek at the beginning of a WKT string to determine the geometry type name and the
/// number of ordinates per coordinate (2 for XY, 3 for Z or M, 4 for ZM).
///
/// # Errors
/// Returns [`WktError::ParseError`] if the keyword is not recognized, and
/// [`WktError::EmptyInput`] for empty/whitespace-only strings.
///
/// # Example
/// ```rust
/// use geo_repair::infer_wkt_type;
///
/// let (type_name, dims) = infer_wkt_type("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))").unwrap();
/// assert_eq!(type_name, "POLYGON");
/// assert_eq!(dims, 2);
/// ```
pub fn infer_wkt_type(input: &str) -> Result<(&'static str, u32), WktError> {
    use super::read::{Keyword::*, Parser};
    let mut p = Parser::new(input.as_bytes(), false);
    p.skip_ws();
    if p.i >= p.s.len() {
        return Err(WktError::EmptyInput);
    }
    let (kw, dims, _) = p.peek_keyword()?;
    let name = match kw {
        Point => "POINT",
        LineString => "LINESTRING",
        Polygon => "POLYGON",
        MultiPoint => "MULTIPOINT",
        MultiLineString => "MULTILINESTRING",
        MultiPolygon => "MULTIPOLYGON",
        GeometryCollection => "GEOMETRYCOLLECTION",
    };
    Ok((name, dims.coord_count() as u32))
}

/// PostGIS and GDAL weld the modifier to the type name ("POINTZ"); the spaced form stays valid.
fn split_compact_modifier(kw: &[u8]) -> Option<(&[u8], Dims)> {
    const NAMES: [&[u8]; 8] = [
        b"POINT",
        b"LINESTRING",
        b"LINEARRING",
        b"POLYGON",
        b"MULTIPOINT",
        b"MULTILINESTRING",
        b"MULTIPOLYGON",
        b"GEOMETRYCOLLECTION",
    ];
    for name in NAMES {
        if kw.len() > name.len() && kw.starts_with(name) {
            let dims = match &kw[name.len()..] {
                b"ZM" => Dims::XYZM,
                b"Z" => Dims::XYZ,
                b"M" => Dims::XYM,
                _ => continue,
            };
            return Some((name, dims));
        }
    }
    None
}

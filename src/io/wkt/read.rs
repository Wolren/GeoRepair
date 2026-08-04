//! WKT parser: single-pass tokenizer + reader.

/// Maximum GEOMETRYCOLLECTION nesting depth. Each level consumes at least
/// ~14 bytes of input, so the cap never rejects real data while bounding
/// recursion (stack overflow is an uncatchable abort).
const MAX_WKT_NESTING: usize = 256;

use super::*;
use std::io::Read;

pub(crate) struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
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

    fn peek_keyword(&mut self) -> Result<(Keyword, u32), WktError> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        let kw = &self.s[start..self.i];
        let dims = self.read_dims()?;
        // Case-insensitive keywords (the corpus carries "MultiPolygon").
        let kw_upper: Vec<u8> = kw.iter().map(|b| b.to_ascii_uppercase()).collect();
        let kw = kw_upper.as_slice();
        let kw = match kw {
            b"POINT" => Keyword::Point,
            b"LINESTRING" => Keyword::LineString,
            // LINEARING is OGC WKT for a ring - parsed as a LineString;
            // ring validity semantics are the caller's concern.
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
        Ok((kw, dims))
    }

    fn read_dims(&mut self) -> Result<u32, WktError> {
        self.skip_ws();
        if self.i + 1 < self.s.len() && &self.s[self.i..self.i + 2] == b"ZM" {
            let modif = String::from_utf8_lossy(&self.s[self.i..self.i + 2]).to_string();
            return Err(WktError::UnsupportedDimension {
                pos: self.i,
                modifier: modif,
            });
        } else if self.i < self.s.len() && (self.s[self.i] == b'Z' || self.s[self.i] == b'M') {
            let modif = String::from_utf8_lossy(&self.s[self.i..self.i + 1]).to_string();
            return Err(WktError::UnsupportedDimension {
                pos: self.i,
                modifier: modif,
            });
        }
        Ok(2)
    }
    fn read_f64(&mut self) -> Result<f64, WktError> {
        self.skip_ws();
        if self.i >= self.s.len() {
            return Err(self.err("expected number"));
        }

        // NaN / inf / Infinity, strtod-style: optional sign, case-insensitive
        // (GEOS StringTokenizer parses numbers via strtod, which accepts
        // "nan", "inf", "infinity" and their signed variants in any case).
        let negative = match self.s[self.i] {
            b'-' => {
                self.i += 1;
                true
            }
            b'+' => {
                self.i += 1;
                false
            }
            _ => false,
        };

        if self.i < self.s.len() {
            let rest = &self.s[self.i..];
            if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"nan") {
                self.i += 3;
                return Ok(f64::NAN);
            }
            if rest.len() >= 8 && rest[..8].eq_ignore_ascii_case(b"infinity") {
                self.i += 8;
                return Ok(if negative {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                });
            }
            if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"inf") {
                self.i += 3;
                return Ok(if negative {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                });
            }
        }

        // Parse integer part digit-by-digit into u64
        let mut int_val: u64 = 0;
        let mut parsed_any = false;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            parsed_any = true;
            int_val = int_val
                .saturating_mul(10)
                .saturating_add((self.s[self.i] - b'0') as u64);
            self.i += 1;
        }

        // Parse fractional part
        let mut frac_val: u64 = 0;
        let mut frac_digits: u32 = 0;
        if self.i < self.s.len() && self.s[self.i] == b'.' {
            self.i += 1;
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                parsed_any = true;
                frac_val = frac_val
                    .saturating_mul(10)
                    .saturating_add((self.s[self.i] - b'0') as u64);
                frac_digits += 1;
                self.i += 1;
            }
        }

        if !parsed_any {
            return Err(self.err("expected digit"));
        }

        // Parse exponent
        let mut exp: i32 = 0;
        if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
            self.i += 1;
            let exp_negative = if self.i < self.s.len() && self.s[self.i] == b'-' {
                self.i += 1;
                true
            } else if self.i < self.s.len() && self.s[self.i] == b'+' {
                self.i += 1;
                false
            } else {
                false
            };
            if self.i >= self.s.len() || !self.s[self.i].is_ascii_digit() {
                return Err(self.err("expected exponent digit"));
            }
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                exp = exp
                    .saturating_mul(10)
                    .saturating_add((self.s[self.i] - b'0') as i32);
                self.i += 1;
            }
            if exp_negative {
                exp = -exp;
            }
        }

        // Combine using f64 arithmetic (avoids str::parse<f64>() entirely)
        let value = if frac_digits > 0 {
            (int_val as f64 + frac_val as f64 / 10f64.powi(frac_digits as i32)) * 10f64.powi(exp)
        } else {
            int_val as f64 * 10f64.powi(exp)
        };

        Ok(if negative { -value } else { value })
    }
    fn read_coord(&mut self, _dims: u32) -> Result<Coord<f64>, WktError> {
        let x = self.read_f64()?;
        let y = self.read_f64()?;
        Ok(Coord { x, y })
    }

    fn read_coord_list(&mut self, dims: u32) -> Result<Vec<Coord<f64>>, WktError> {
        self.skip_ws();
        // A bare '(' ')' coordinate list is rejected (GEOS rejects it too);
        // emptiness is expressed with the EMPTY keyword. POLYGON (EMPTY)
        // is handled by the ring parsers before reaching this point.
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

    fn parse_point(&mut self, dims: u32) -> Result<Geometry<f64>, WktError> {
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

    fn parse_linestring(&mut self, dims: u32) -> Result<Geometry<f64>, WktError> {
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

    fn parse_polygon(&mut self, dims: u32) -> Result<Geometry<f64>, WktError> {
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
            // EMPTY ring element (POLYGON ((...), EMPTY) or POLYGON
            // (EMPTY, EMPTY)) - an empty ring contributes nothing.
            if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    saw_any = true;
                    self.skip_ws();
                    if self.i < self.s.len() && self.s[self.i] == b',' {
                        self.i += 1;
                    }
                    continue;
                }
            }
            self.expect(b'(')?;
            let coords = self.read_coord_list(dims)?;
            self.expect(b')')?;
            rings.push(LineString::new(coords));
            saw_any = true;
        }
        self.expect(b')')?;
        // POLYGON () is rejected (GEOS rejects it too); POLYGON (EMPTY)
        // is a valid empty polygon.
        if !saw_any {
            return Err(self.err("expected at least one ring"));
        }
        if rings.is_empty() {
            return Ok(Geometry::Polygon(Polygon::new(
                LineString::new(vec![]),
                vec![],
            )));
        }
        let exterior = rings.swap_remove(0);
        Ok(Geometry::Polygon(Polygon::new(exterior, rings)))
    }

    fn parse_multipoint(&mut self, dims: u32) -> Result<Geometry<f64>, WktError> {
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
            // EMPTY element (MULTIPOINT (EMPTY, (1 2), ...)) - an empty
            // point component; geo's empty-point convention is NaN coords.
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
        // MULTIPOINT () is rejected (GEOS rejects it too).
        if !saw_any {
            return Err(self.err("expected at least one point"));
        }
        Ok(Geometry::MultiPoint(MultiPoint(points)))
    }

    fn parse_multilinestring(&mut self, dims: u32) -> Result<Geometry<f64>, WktError> {
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
            // EMPTY element (MULTILINESTRING (EMPTY, (1 1, 2 2))) - an
            // empty line component.
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
        // MULTILINESTRING () is rejected (GEOS rejects it too).
        if !saw_any {
            return Err(self.err("expected at least one line"));
        }
        Ok(Geometry::MultiLineString(MultiLineString(lines)))
    }

    fn parse_multipolygon(&mut self, dims: u32) -> Result<Geometry<f64>, WktError> {
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
            // EMPTY element (MULTIPOLYGON (EMPTY, ((0 0, ...)))) - an
            // empty polygon component.
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
                // EMPTY ring element (MULTIPOLYGON ((...), (EMPTY))) - an
                // empty ring contributes nothing; the polygon stays empty.
                if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                    let rest = &self.s[self.i..];
                    if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                        self.i += 5;
                        saw_ring = true;
                        continue;
                    }
                }
                self.expect(b'(')?;
                let coords = self.read_coord_list(dims)?;
                self.expect(b')')?;
                rings.push(LineString::new(coords));
                saw_ring = true;
            }
            self.expect(b')')?;
            // An empty-paren polygon inside MULTIPOLYGON is rejected
            // (GEOS rejects it too); (EMPTY) is a valid empty component.
            if !saw_ring {
                return Err(self.err("expected at least one ring"));
            }
            let exterior = if rings.is_empty() {
                LineString::new(vec![])
            } else {
                rings.swap_remove(0)
            };
            polys.push(Polygon::new(exterior, rings));
            saw_any = true;
        }
        self.expect(b')')?;
        // MULTIPOLYGON () is rejected (GEOS rejects it too).
        if !saw_any {
            return Err(self.err("expected at least one polygon"));
        }
        Ok(Geometry::MultiPolygon(MultiPolygon(polys)))
    }

    fn parse_geometrycollection(&mut self, depth: usize) -> Result<Geometry<f64>, WktError> {
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
            // EMPTY element (GEOMETRYCOLLECTION (EMPTY, POINT (1 2))) -
            // an empty point component (geo convention: NaN coords).
            if self.i < self.s.len() && (self.s[self.i] == b'E' || self.s[self.i] == b'e') {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    geoms.push(Geometry::Point(Point::new(f64::NAN, f64::NAN)));
                    saw_any = true;
                    continue;
                }
            }
            geoms.push(self.parse_any(depth + 1)?);
            saw_any = true;
        }
        self.expect(b')')?;
        // GEOMETRYCOLLECTION () is rejected (GEOS rejects it too).
        if !saw_any {
            return Err(self.err("expected at least one element"));
        }
        Ok(Geometry::GeometryCollection(GeometryCollection(geoms)))
    }

    fn parse_any(&mut self, depth: usize) -> Result<Geometry<f64>, WktError> {
        // GEOMETRYCOLLECTION nests recursively; a crafted document can
        // nest arbitrarily deep and overflow the stack (uncatchable
        // abort). GEOS bounds its reader similarly. A flat collection's
        // members each parse at depth+1, so the cap is on nesting, not
        // element count.
        if depth > MAX_WKT_NESTING {
            return Err(self.err("geometry nesting exceeds limit"));
        }
        let (kw, dims) = self.peek_keyword()?;
        match kw {
            Keyword::Point => self.parse_point(dims),
            Keyword::LineString => self.parse_linestring(dims),
            Keyword::Polygon => self.parse_polygon(dims),
            Keyword::MultiPoint => self.parse_multipoint(dims),
            Keyword::MultiLineString => self.parse_multilinestring(dims),
            Keyword::MultiPolygon => self.parse_multipolygon(dims),
            Keyword::GeometryCollection => self.parse_geometrycollection(depth),
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

/// Parse a WKT string into a `Geometry<f64>`.
///
/// Supports all OGC geometry types: Point, LineString, Polygon,
/// MultiPoint, MultiLineString, MultiPolygon, GeometryCollection.
/// Z, M, and ZM dimension modifiers are rejected with
/// [`WktError::UnsupportedDimension`].
pub fn read_wkt(input: &str) -> Result<Geometry<f64>, WktError> {
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    if p.i >= p.s.len() {
        return Err(WktError::EmptyInput);
    }
    let geom = p.parse_any(0)?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(WktError::TrailingCharacters { pos: p.i });
    }
    Ok(geom)
}

/// Read a WKT geometry from any `io::Read` source.
///
/// Reads the entire input to a string, then delegates to [`read_wkt`].
/// Supports the same geometry types and rejects Z/M/ZM modifiers.
pub fn read_wkt_from(mut reader: impl Read) -> Result<Geometry<f64>, WktError> {
    let mut s = String::new();
    reader.read_to_string(&mut s).map_err(WktError::IoError)?;
    read_wkt(&s)
}



/// Peek at the beginning of a WKT string to determine the geometry type name
/// and dimension (always 2 for GeoRepair, since Z/M/ZM are rejected).
///
/// This function reads only the type keyword and optional dimension modifier,
/// without parsing the full geometry. Useful for routing or preview.
///
/// # Errors
///
/// Returns [`WktError::UnknownGeometryType`] if the keyword is not recognized,
/// [`WktError::UnsupportedDimension`] for Z/M/ZM modifiers, and
/// [`WktError::EmptyInput`] for empty/whitespace-only strings.
///
/// # Example
///
/// ```rust
/// use geo_repair::infer_wkt_type;
///
/// let (type_name, dims) = infer_wkt_type("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))").unwrap();
/// assert_eq!(type_name, "POLYGON");
/// assert_eq!(dims, 2);
/// ```
pub fn infer_wkt_type(input: &str) -> Result<(&'static str, u32), WktError> {
    use super::read::{Keyword::*, Parser};
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    if p.i >= p.s.len() {
        return Err(WktError::EmptyInput);
    }
    let (kw, dims) = p.peek_keyword()?;
    let name = match kw {
        Point => "POINT",
        LineString => "LINESTRING",
        Polygon => "POLYGON",
        MultiPoint => "MULTIPOINT",
        MultiLineString => "MULTILINESTRING",
        MultiPolygon => "MULTIPOLYGON",
        GeometryCollection => "GEOMETRYCOLLECTION",
    };
    Ok((name, dims))
}

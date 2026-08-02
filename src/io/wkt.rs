//! OGC Well-Known Text (WKT) parsing and serialization (2D only).
//!
//! Zero-dependency backend — parses and serializes WKT directly into
//! `geo::Geometry<f64>` without intermediate representations.
//!
//! # Supported types
//!
//! Point, LineString, LINEARING (as LineString), Polygon, MultiPoint,
//! MultiLineString, MultiPolygon, GeometryCollection.
//!
//! EMPTY geometries and EMPTY elements inside Multi* / GeometryCollection
//! are accepted (empty point components use geo's NaN-coordinate
//! convention). NaN and inf ordinates are accepted (invalid geometry per
//! OGC, but parseable - the corpus carries them).
//!
//! Z, M, and ZM dimension modifiers are recognized and rejected with
//! [`WktError::UnsupportedDimension`](crate::WktError).
//!
//! # Reading
//!
//! - [`read_wkt`] — parse a WKT string
//! - [`read_wkt_from`] — read WKT from any `io::Read` source
//! - [`infer_wkt_type`] — peek at the type keyword without full parsing
//!
//! # Writing
//!
//! - [`write_wkt`] — serialize to WKT string
//! - [`write_wkt_to`] — write WKT to any `io::Write` target
//!
//! # Example
//!
//! ```rust
//! use geo_repair::{read_wkt, write_wkt};
//!
//! let wkt = "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))";
//! let geom = read_wkt(wkt).unwrap();
//! let roundtrip = write_wkt(&geom);
//! let parsed_again = read_wkt(&roundtrip).unwrap();
//! assert_eq!(geom, parsed_again);
//! ```

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use std::fmt;
use std::io::{self, Read, Write};

/// Errors that can occur during WKT parsing.
#[derive(Debug)]
pub enum WktError {
    ParseError { pos: usize, message: String },
    InvalidNumber { pos: usize, value: String },
    UnknownGeometryType { pos: usize, type_name: String },
    TrailingCharacters { pos: usize },
    EmptyInput,
    UnsupportedDimension { pos: usize, modifier: String },
    IoError(std::io::Error),
}

impl fmt::Display for WktError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WktError::ParseError { pos, message } => {
                write!(f, "WKT parse error at position {pos}: {message}")
            }
            WktError::InvalidNumber { pos, value } => {
                write!(f, "invalid number at position {pos}: '{value}'")
            }
            WktError::UnknownGeometryType { pos, type_name } => {
                write!(f, "unknown geometry type at position {pos}: '{type_name}'")
            }
            WktError::TrailingCharacters { pos } => {
                write!(f, "trailing characters after geometry at position {pos}")
            }
            WktError::EmptyInput => write!(f, "empty WKT input"),
            WktError::UnsupportedDimension { pos, modifier } => {
                write!(
                    f,
                    "unsupported dimension modifier '{modifier}' at position {pos} (only 2D is supported)"
                )
            }
            WktError::IoError(e) => write!(f, "WKT I/O error: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for WktError {}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
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
        let mut kw_upper: Vec<u8> = kw.iter().map(|b| b.to_ascii_uppercase()).collect();
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
        if self.peek() == b'E' || self.peek() == b'e' {
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
                if self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            // EMPTY ring element (POLYGON ((...), EMPTY) or POLYGON
            // (EMPTY, EMPTY)) - an empty ring contributes nothing.
            if self.s[self.i] == b'E' || self.s[self.i] == b'e' {
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
            if self.s[self.i] == b'E' || self.s[self.i] == b'e' {
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
                if self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            // EMPTY element (MULTIPOLYGON (EMPTY, ((0 0, ...)))) - an
            // empty polygon component.
            if self.s[self.i] == b'E' || self.s[self.i] == b'e' {
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
                    if self.s[self.i] == b',' {
                        self.i += 1;
                    }
                    self.skip_ws();
                }
                if self.i < self.s.len() && self.s[self.i] == b')' {
                    break;
                }
                // EMPTY ring element (MULTIPOLYGON ((...), (EMPTY))) - an
                // empty ring contributes nothing; the polygon stays empty.
                if self.s[self.i] == b'E' || self.s[self.i] == b'e' {
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

    fn parse_geometrycollection(&mut self) -> Result<Geometry<f64>, WktError> {
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
                if self.s[self.i] == b',' {
                    self.i += 1;
                }
                self.skip_ws();
            }
            if self.i < self.s.len() && self.s[self.i] == b')' {
                break;
            }
            // EMPTY element (GEOMETRYCOLLECTION (EMPTY, POINT (1 2))) -
            // an empty point component (geo convention: NaN coords).
            if self.s[self.i] == b'E' || self.s[self.i] == b'e' {
                let rest = &self.s[self.i..];
                if rest.starts_with(b"EMPTY") || rest.starts_with(b"empty") {
                    self.i += 5;
                    geoms.push(Geometry::Point(Point::new(f64::NAN, f64::NAN)));
                    saw_any = true;
                    continue;
                }
            }
            geoms.push(self.parse_any()?);
            saw_any = true;
        }
        self.expect(b')')?;
        // GEOMETRYCOLLECTION () is rejected (GEOS rejects it too).
        if !saw_any {
            return Err(self.err("expected at least one element"));
        }
        Ok(Geometry::GeometryCollection(GeometryCollection(geoms)))
    }

    fn parse_any(&mut self) -> Result<Geometry<f64>, WktError> {
        let (kw, dims) = self.peek_keyword()?;
        match kw {
            Keyword::Point => self.parse_point(dims),
            Keyword::LineString => self.parse_linestring(dims),
            Keyword::Polygon => self.parse_polygon(dims),
            Keyword::MultiPoint => self.parse_multipoint(dims),
            Keyword::MultiLineString => self.parse_multilinestring(dims),
            Keyword::MultiPolygon => self.parse_multipolygon(dims),
            Keyword::GeometryCollection => self.parse_geometrycollection(),
        }
    }
}

enum Keyword {
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
    let geom = p.parse_any()?;
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

/// Serialize a `Geometry<f64>` to WKT and write it to any `io::Write` target.
///
/// Formats the geometry using [`write_wkt`] and writes the resulting
/// string to `writer`. Returns `io::Result<()>` so callers can handle
/// write errors (e.g. broken pipe, disk full).
pub fn write_wkt_to(geom: &Geometry<f64>, writer: &mut impl Write) -> io::Result<()> {
    let s = write_wkt(geom);
    writer.write_all(s.as_bytes())
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
    use Keyword::*;
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

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

/// Serialize a `Geometry<f64>` to WKT string.
///
/// The output uses standard OGC WKT formatting (space after type keyword,
/// spaces after commas). The same geometry can be round-tripped through
/// [`read_wkt`].
pub fn write_wkt(geom: &Geometry<f64>) -> String {
    let mut s = String::new();
    write_geom(&mut s, geom);
    s
}

fn write_f64(s: &mut String, v: f64) {
    let mut buf = ryu::Buffer::new();
    s.push_str(buf.format(v));
}

fn write_coord(s: &mut String, c: &Coord<f64>) {
    write_f64(s, c.x);
    s.push(' ');
    write_f64(s, c.y);
}

fn write_coord_list(s: &mut String, coords: &[Coord<f64>]) {
    if let Some(first) = coords.first() {
        write_coord(s, first);
        for c in &coords[1..] {
            s.push_str(", ");
            write_coord(s, c);
        }
    }
}

fn write_linestring(s: &mut String, ls: &LineString<f64>) {
    write_coord_list(s, &ls.0);
}

fn write_polygon_rings(s: &mut String, poly: &Polygon<f64>) {
    s.push_str("((");
    write_coord_list(s, &poly.exterior().0);
    s.push(')');
    for h in poly.interiors() {
        s.push_str(", (");
        write_coord_list(s, &h.0);
        s.push(')');
    }
    s.push(')');
}

fn write_geom(s: &mut String, geom: &Geometry<f64>) {
    match geom {
        Geometry::Point(p) => {
            if p.0.x.is_nan() && p.0.y.is_nan() {
                s.push_str("POINT EMPTY");
            } else {
                s.push_str("POINT (");
                write_coord(s, &p.0);
                s.push(')');
            }
        }
        Geometry::LineString(ls) => {
            if ls.0.is_empty() {
                s.push_str("LINESTRING EMPTY");
            } else {
                s.push_str("LINESTRING (");
                write_linestring(s, ls);
                s.push(')');
            }
        }
        Geometry::Polygon(poly) => {
            if poly.exterior().0.is_empty() {
                s.push_str("POLYGON EMPTY");
            } else {
                s.push_str("POLYGON ");
                write_polygon_rings(s, poly);
            }
        }
        Geometry::MultiPoint(mp) => {
            if mp.0.is_empty() {
                s.push_str("MULTIPOINT EMPTY");
            } else {
                s.push_str("MULTIPOINT (");
                for (i, p) in mp.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_coord(s, &p.0);
                }
                s.push(')');
            }
        }
        Geometry::MultiLineString(mls) => {
            if mls.0.is_empty() {
                s.push_str("MULTILINESTRING EMPTY");
            } else {
                s.push_str("MULTILINESTRING (");
                for (i, ls) in mls.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push('(');
                    write_linestring(s, ls);
                    s.push(')');
                }
                s.push(')');
            }
        }
        Geometry::MultiPolygon(mp) => {
            if mp.0.is_empty() {
                s.push_str("MULTIPOLYGON EMPTY");
            } else {
                s.push_str("MULTIPOLYGON (");
                for (i, poly) in mp.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_polygon_rings(s, poly);
                }
                s.push(')');
            }
        }
        Geometry::GeometryCollection(gc) => {
            if gc.0.is_empty() {
                s.push_str("GEOMETRYCOLLECTION EMPTY");
            } else {
                s.push_str("GEOMETRYCOLLECTION (");
                for (i, g) in gc.0.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    write_geom(s, g);
                }
                s.push(')');
            }
        }
        // OGC WKT has no Line/Rect/Triangle types; serialize them losslessly
        // as their closest OGC equivalents (coordinate-exact).
        Geometry::Line(line) => {
            s.push_str("LINESTRING (");
            write_coord(s, &line.start);
            s.push_str(", ");
            write_coord(s, &line.end);
            s.push(')');
        }
        Geometry::Rect(rect) => {
            s.push_str("POLYGON ((");
            write_coord(s, &Coord {
                x: rect.min().x,
                y: rect.min().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.max().x,
                y: rect.min().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.max().x,
                y: rect.max().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.min().x,
                y: rect.max().y,
            });
            s.push_str(", ");
            write_coord(s, &Coord {
                x: rect.min().x,
                y: rect.min().y,
            });
            s.push_str("))");
        }
        Geometry::Triangle(tri) => {
            s.push_str("POLYGON ((");
            write_coord(s, &tri.v1());
            s.push_str(", ");
            write_coord(s, &tri.v2());
            s.push_str(", ");
            write_coord(s, &tri.v3());
            s.push_str(", ");
            write_coord(s, &tri.v1());
            s.push_str("))");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{load, save};
    use geo::{Coord, Geometry, LineString, Polygon};
    use std::time::Instant;

    #[test]
    fn roundtrip_point() {
        let wkt = "POINT (1.5 2.5)";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_point_no_space() {
        let wkt_compact = "POINT(1.5 2.5)";
        let geom = read_wkt(wkt_compact).unwrap();
        assert_eq!(write_wkt(&geom), "POINT (1.5 2.5)");
    }

    #[test]
    fn roundtrip_linestring() {
        let geom = read_wkt("LINESTRING (0 0, 1 1, 2 0)").unwrap();
        assert_eq!(write_wkt(&geom), "LINESTRING (0.0 0.0, 1.0 1.0, 2.0 0.0)");
    }

    #[test]
    fn roundtrip_linestring_compact() {
        let geom = read_wkt("LINESTRING(0 0,1 1,2 0)").unwrap();
        assert_eq!(write_wkt(&geom), "LINESTRING (0.0 0.0, 1.0 1.0, 2.0 0.0)");
    }

    #[test]
    fn roundtrip_polygon() {
        let geom = read_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))").unwrap();
        assert_eq!(
            write_wkt(&geom),
            "POLYGON ((0.0 0.0, 10.0 0.0, 10.0 10.0, 0.0 10.0, 0.0 0.0))"
        );
    }

    #[test]
    fn roundtrip_polygon_with_hole() {
        let geom =
            read_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 8 2, 8 8, 2 8, 2 2))").unwrap();
        assert_eq!(
            write_wkt(&geom),
            "POLYGON ((0.0 0.0, 10.0 0.0, 10.0 10.0, 0.0 10.0, 0.0 0.0), (2.0 2.0, 8.0 2.0, 8.0 8.0, 2.0 8.0, 2.0 2.0))"
        );
    }

    #[test]
    fn roundtrip_multipoint_parenthesized() {
        let geom = read_wkt("MULTIPOINT (1.5 2.5, 3 4)").unwrap();
        assert_eq!(write_wkt(&geom), "MULTIPOINT (1.5 2.5, 3.0 4.0)");
    }

    #[test]
    fn roundtrip_multipoint_double_parens() {
        let geom = read_wkt("MULTIPOINT ((1.5 2.5), (3 4))").unwrap();
        assert_eq!(write_wkt(&geom), "MULTIPOINT (1.5 2.5, 3.0 4.0)");
    }

    #[test]
    fn roundtrip_multilinestring() {
        let geom = read_wkt("MULTILINESTRING ((0 0, 1 1), (2 2, 3 3))").unwrap();
        assert_eq!(
            write_wkt(&geom),
            "MULTILINESTRING ((0.0 0.0, 1.0 1.0), (2.0 2.0, 3.0 3.0))"
        );
    }

    #[test]
    fn roundtrip_multipolygon() {
        let geom =
            read_wkt("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)), ((2 2, 3 2, 3 3, 2 3, 2 2)))")
                .unwrap();
        assert_eq!(
            write_wkt(&geom),
            "MULTIPOLYGON (((0.0 0.0, 1.0 0.0, 1.0 1.0, 0.0 1.0, 0.0 0.0)), ((2.0 2.0, 3.0 2.0, 3.0 3.0, 2.0 3.0, 2.0 2.0)))"
        );
    }

    #[test]
    fn roundtrip_geometrycollection() {
        let geom = read_wkt("GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))").unwrap();
        assert_eq!(
            write_wkt(&geom),
            "GEOMETRYCOLLECTION (POINT (1.0 2.0), LINESTRING (0.0 0.0, 1.0 1.0))"
        );
    }

    #[test]
    fn read_invalid_wkt() {
        let err = read_wkt("NOT A GEOMETRY").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown geometry type"), "{msg}");
    }

    #[test]
    fn point_empty() {
        let geom = read_wkt("POINT EMPTY").unwrap();
        assert!(matches!(geom, Geometry::Point(_)));
    }

    #[test]
    fn linestring_empty() {
        let geom = read_wkt("LINESTRING EMPTY").unwrap();
        assert!(matches!(geom, Geometry::LineString(_)));
    }

    #[test]
    fn polygon_empty() {
        let geom = read_wkt("POLYGON EMPTY").unwrap();
        assert!(matches!(geom, Geometry::Polygon(_)));
    }

    #[test]
    fn multipoint_empty() {
        let geom = read_wkt("MULTIPOINT EMPTY").unwrap();
        assert!(matches!(geom, Geometry::MultiPoint(_)));
    }

    #[test]
    fn geometrycollection_empty() {
        let geom = read_wkt("GEOMETRYCOLLECTION EMPTY").unwrap();
        assert!(matches!(geom, Geometry::GeometryCollection(_)));
    }

    #[test]
    fn z_modifier_rejected() {
        let err = read_wkt("POINT Z (1 2 3)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
        let msg = err.to_string();
        assert!(msg.contains("Z"), "{msg}");
    }

    #[test]
    fn zm_modifier_rejected() {
        let err = read_wkt("POINT ZM (1 2 3 4)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
        let msg = err.to_string();
        assert!(msg.contains("ZM"), "{msg}");
    }

    #[test]
    fn m_modifier_rejected() {
        let err = read_wkt("POINT M (1 2 3)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
        let msg = err.to_string();
        assert!(msg.contains("M"), "{msg}");
    }

    #[test]
    fn roundtrip_via_file() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let geom = Geometry::Polygon(poly);

        let dir = std::env::temp_dir().join("geo_repair_wkt_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.wkt");
        let path_str = path.to_str().unwrap();

        save(path_str, &geom).unwrap();
        let loaded = load(path_str).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], geom);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn iops_wkt_vs_wkb() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1000.0, y: 0.0 },
                Coord {
                    x: 1000.0,
                    y: 1000.0,
                },
                Coord { x: 0.0, y: 1000.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let geom = Geometry::Polygon(poly);
        let n = 10000;

        let t0 = Instant::now();
        for _ in 0..n {
            let wkt = write_wkt(&geom);
            let _ = read_wkt(&wkt).unwrap();
        }
        let dt_wkt = t0.elapsed();

        let t0 = Instant::now();
        for _ in 0..n {
            let wkb = crate::io::wkb::write_wkb(&geom);
            let _ = crate::io::wkb::read_wkb(&wkb).unwrap();
        }
        let dt_wkb = t0.elapsed();

        eprintln!(
            "WKT roundtrip ({n}×):  {dt_wkt:.3?}  ({:7.0} ns/op)",
            dt_wkt.as_nanos() as f64 / n as f64
        );
        eprintln!(
            "WKB roundtrip ({n}×):  {dt_wkb:.3?}  ({:7.0} ns/op)",
            dt_wkb.as_nanos() as f64 / n as f64
        );
        eprintln!(
            "WKT is {:.1}× slower than WKB",
            dt_wkt.as_nanos() as f64 / dt_wkb.as_nanos().max(1) as f64
        );
    }

    #[test]
    fn trailiing_garbage_rejected() {
        assert!(read_wkt("POINT (1 2) extra").is_err());
    }

    #[test]
    fn empty_input_rejected() {
        assert!(read_wkt("").is_err());
        assert!(read_wkt("   ").is_err());
    }

    /// Roundtrip all geometry types against the wkt crate to verify equivalence.
    #[test]
    fn roundtrip_all_types_vs_wkt_crate() {
        use wkt::ToWkt;

        let cases = [
            "POINT (1 2)",
            "LINESTRING (0 0, 1 1, 2 0)",
            "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))",
            "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 8 2, 8 8, 2 8, 2 2))",
            "MULTIPOINT (1 2, 3 4)",
            "MULTILINESTRING ((0 0, 1 1), (2 2, 3 3))",
            "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)))",
            "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)), ((2 2, 3 2, 3 3, 2 3, 2 2)))",
            "GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))",
        ];

        for wkt in &cases {
            let ours = read_wkt(wkt).unwrap();
            let theirs: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(wkt).unwrap();
            assert_eq!(ours, theirs, "mismatch for {wkt}");

            let our_out = write_wkt(&ours);
            let their_out = theirs.to_wkt().to_string();
            // Our output format may differ in whitespace — re-parse both to compare
            let ours_reparsed = read_wkt(&our_out).unwrap();
            let theirs_reparsed: Geometry<f64> =
                wkt::TryFromWkt::try_from_wkt_str(&their_out).unwrap();
            assert_eq!(ours_reparsed, theirs_reparsed, "output mismatch for {wkt}");
        }
    }

    // ---------------------------------------------------------------------------
    // Comprehensive double-roundtrip tests
    // ---------------------------------------------------------------------------

    fn check_double_roundtrip(geom: &Geometry<f64>) {
        let wkt = write_wkt(geom);
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, &parsed, "double roundtrip failed for {wkt}");

        let wkt2 = write_wkt(&parsed);
        let parsed2 = read_wkt(&wkt2).unwrap();
        assert_eq!(geom, &parsed2, "triple roundtrip failed for {wkt}");
    }

    #[test]
    fn double_roundtrip_point() {
        check_double_roundtrip(&Geometry::Point(Point::new(1.5, 2.5)));
    }

    #[test]
    fn double_roundtrip_point_zero() {
        check_double_roundtrip(&Geometry::Point(Point::new(0.0, 0.0)));
    }

    #[test]
    fn double_roundtrip_point_negative() {
        check_double_roundtrip(&Geometry::Point(Point::new(-1.5, -2.5)));
    }

    #[test]
    fn double_roundtrip_point_precision() {
        check_double_roundtrip(&Geometry::Point(Point::new(
            1.2345678901234567,
            9.876543210987654,
        )));
    }

    #[test]
    fn double_roundtrip_point_high_values() {
        check_double_roundtrip(&Geometry::Point(Point::new(1e12, -3.14e8)));
    }

    #[test]
    fn double_roundtrip_point_tiny() {
        check_double_roundtrip(&Geometry::Point(Point::new(1e-12, -5e-10)));
    }

    #[test]
    fn double_roundtrip_linestring() {
        check_double_roundtrip(&Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 0.0 },
        ])));
    }

    #[test]
    fn double_roundtrip_polygon() {
        check_double_roundtrip(&Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        )));
    }

    #[test]
    fn double_roundtrip_polygon_with_hole() {
        check_double_roundtrip(&Geometry::Polygon(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ])],
        )));
    }

    #[test]
    fn double_roundtrip_multipoint() {
        check_double_roundtrip(&Geometry::MultiPoint(MultiPoint(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])));
    }

    #[test]
    fn double_roundtrip_multilinestring() {
        check_double_roundtrip(&Geometry::MultiLineString(MultiLineString(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
            LineString::new(vec![Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }]),
        ])));
    }

    #[test]
    fn double_roundtrip_multipolygon() {
        check_double_roundtrip(&Geometry::MultiPolygon(MultiPolygon(vec![Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        )])));
    }

    #[test]
    fn double_roundtrip_geometrycollection() {
        check_double_roundtrip(&Geometry::GeometryCollection(GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
            ])),
        ])));
    }

    // EMPTY geometry roundtrip
    #[test]
    fn double_roundtrip_point_empty() {
        let geom = read_wkt("POINT EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "POINT EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        // NaN != NaN in IEEE 754, so check both are NaN coords
        if let (Geometry::Point(a), Geometry::Point(b)) = (&geom, &parsed) {
            assert!(a.x().is_nan() && a.y().is_nan());
            assert!(b.x().is_nan() && b.y().is_nan());
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn double_roundtrip_linestring_empty() {
        let geom = read_wkt("LINESTRING EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "LINESTRING EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_polygon_empty() {
        let geom = read_wkt("POLYGON EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "POLYGON EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_multipoint_empty() {
        let geom = read_wkt("MULTIPOINT EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "MULTIPOINT EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_multilinestring_empty() {
        let geom = read_wkt("MULTILINESTRING EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "MULTILINESTRING EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_multipolygon_empty() {
        let geom = read_wkt("MULTIPOLYGON EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "MULTIPOLYGON EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn double_roundtrip_gc_empty() {
        let geom = read_wkt("GEOMETRYCOLLECTION EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "GEOMETRYCOLLECTION EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        assert_eq!(geom, parsed);
    }

    // read_f64 edge cases
    #[test]
    fn parse_nan() {
        let geom = read_wkt("POINT (NaN NaN)").unwrap();
        if let Geometry::Point(p) = geom {
            assert!(p.x().is_nan());
            assert!(p.y().is_nan());
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn parse_inf() {
        let geom = read_wkt("POINT (inf -inf)").unwrap();
        if let Geometry::Point(p) = geom {
            assert!(p.x().is_infinite());
            assert!(p.x().is_sign_positive());
            assert!(p.y().is_infinite());
            assert!(p.y().is_sign_negative());
        } else {
            panic!("expected Point");
        }
    }

    // Minimal POLYGON (()) — previously would fail
    #[test]
    fn double_roundtrip_empty_ring() {
        let geom = read_wkt("POLYGON EMPTY").unwrap();
        let wkt = write_wkt(&geom);
        // Should write as "POLYGON EMPTY", not "POLYGON (())"
        assert_eq!(wkt, "POLYGON EMPTY");
    }

    // Construct empty geometries programmatically and roundtrip
    #[test]
    fn double_roundtrip_empty_point_constructed() {
        let geom = Geometry::Point(Point(Coord {
            x: f64::NAN,
            y: f64::NAN,
        }));
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "POINT EMPTY");
        let parsed = read_wkt(&wkt).unwrap();
        if let Geometry::Point(p) = parsed {
            assert!(p.x().is_nan() && p.y().is_nan());
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn double_roundtrip_empty_linestring_constructed() {
        check_double_roundtrip(&Geometry::LineString(LineString::new(vec![])));
    }

    #[test]
    fn double_roundtrip_empty_polygon_constructed() {
        check_double_roundtrip(&Geometry::Polygon(Polygon::new(
            LineString::new(vec![]),
            vec![],
        )));
    }

    // read_wkt_from / write_wkt_to tests
    #[test]
    fn read_wkt_from_reader() {
        let input = "POINT (1.5 2.5)";
        let reader = input.as_bytes();
        let geom = read_wkt_from(reader).unwrap();
        assert_eq!(write_wkt(&geom), "POINT (1.5 2.5)");
    }

    #[test]
    fn read_wkt_from_empty_fails() {
        let err = read_wkt_from(&b""[..]).unwrap_err();
        assert!(matches!(err, WktError::EmptyInput));
    }

    #[test]
    fn read_wkt_from_invalid_fails() {
        let err = read_wkt_from(&b"NOT WKT"[..]).unwrap_err();
        assert!(matches!(err, WktError::ParseError { .. }));
    }

    #[test]
    fn read_wkt_from_z_rejected() {
        let err = read_wkt_from(&b"POINT Z (1 2 3)"[..]).unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    }

    #[test]
    fn write_wkt_to_writer() {
        let geom = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkt_to(&geom, &mut buf).unwrap();
        assert_eq!(buf, b"POINT (1.5 2.5)");
    }

    #[test]
    fn write_wkt_to_write_then_read() {
        let geom = Geometry::Point(Point::new(1.5, 2.5));
        let mut buf = Vec::new();
        write_wkt_to(&geom, &mut buf).unwrap();
        let back = read_wkt_from(&buf[..]).unwrap();
        assert_eq!(write_wkt(&geom), write_wkt(&back));
    }

    // infer_wkt_type tests
    #[test]
    fn infer_type_point() {
        let (name, dims) = infer_wkt_type("POINT (1 2)").unwrap();
        assert_eq!(name, "POINT");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_linestring() {
        let (name, dims) = infer_wkt_type("LINESTRING (0 0, 1 1)").unwrap();
        assert_eq!(name, "LINESTRING");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_polygon() {
        let (name, dims) = infer_wkt_type("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))").unwrap();
        assert_eq!(name, "POLYGON");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_multipoint() {
        let (name, dims) = infer_wkt_type("MULTIPOINT (1 2, 3 4)").unwrap();
        assert_eq!(name, "MULTIPOINT");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_multilinestring() {
        let (name, dims) = infer_wkt_type("MULTILINESTRING ((0 0, 1 1))").unwrap();
        assert_eq!(name, "MULTILINESTRING");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_multipolygon() {
        let (name, dims) = infer_wkt_type("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)))").unwrap();
        assert_eq!(name, "MULTIPOLYGON");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_gc() {
        let (name, dims) = infer_wkt_type("GEOMETRYCOLLECTION (POINT (1 2))").unwrap();
        assert_eq!(name, "GEOMETRYCOLLECTION");
        assert_eq!(dims, 2);
    }

    #[test]
    fn infer_type_empty_fails() {
        let err = infer_wkt_type("").unwrap_err();
        assert!(matches!(err, WktError::EmptyInput));
    }

    #[test]
    fn infer_type_whitespace_fails() {
        let err = infer_wkt_type("   ").unwrap_err();
        assert!(matches!(err, WktError::EmptyInput));
    }

    #[test]
    fn infer_type_z_rejected() {
        let err = infer_wkt_type("POINT Z (1 2 3)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    }

    #[test]
    fn infer_type_zm_rejected() {
        let err = infer_wkt_type("POINT ZM (1 2 3 4)").unwrap_err();
        assert!(matches!(err, WktError::UnsupportedDimension { .. }));
    }

    #[test]
    fn infer_type_unknown_fails() {
        let err = infer_wkt_type("CIRCULARSTRING (1 2, 3 4)").unwrap_err();
        assert!(matches!(err, WktError::ParseError { .. }));
    }

    // -----------------------------------------------------------------------
    // Production-readiness battery: strtod specials, case-insensitive
    // keywords, EMPTY elements, malformed inputs, non-OGC variants.
    // -----------------------------------------------------------------------

    #[test]
    fn special_values_strtod_semantics() {
        // GEOS parses numbers via strtod: nan/inf/infinity, any case, signed.
        for (wkt, x, y) in [
            ("POINT (NaN NaN)", f64::NAN, f64::NAN),
            ("POINT (nan 5)", f64::NAN, 5.0),
            ("POINT (NAN 5)", f64::NAN, 5.0),
            ("POINT (-nan 5)", f64::NAN, 5.0),
            ("POINT (Inf 5)", f64::INFINITY, 5.0),
            ("POINT (inf 5)", f64::INFINITY, 5.0),
            ("POINT (INF 5)", f64::INFINITY, 5.0),
            ("POINT (-Inf 5)", f64::NEG_INFINITY, 5.0),
            ("POINT (+Inf 5)", f64::INFINITY, 5.0),
            ("POINT (Infinity 5)", f64::INFINITY, 5.0),
            ("POINT (-Infinity 5)", f64::NEG_INFINITY, 5.0),
        ] {
            let g = read_wkt(wkt).unwrap();
            if let Geometry::Point(p) = g {
                assert_eq!(p.0.y.is_nan(), y.is_nan(), "y nan-ness mismatch for {wkt}");
                if !y.is_nan() {
                    assert_eq!(p.0.y, y, "y mismatch for {wkt}");
                }
                assert_eq!(p.0.x.is_nan(), x.is_nan(), "nan-ness mismatch for {wkt}");
                if !x.is_nan() {
                    assert_eq!(p.0.x, x, "x mismatch for {wkt}");
                }
            } else {
                panic!("expected point for {wkt}");
            }
        }
    }

    #[test]
    fn keywords_case_insensitive() {
        for (wkt, expect) in [
            ("point (1 2)", "POINT (1.0 2.0)"),
            ("linestring (0 0, 1 1)", "LINESTRING (0.0 0.0, 1.0 1.0)"),
            ("Polygon ((0 0, 1 0, 1 1, 0 1, 0 0))", "POLYGON ((0.0 0.0, 1.0 0.0, 1.0 1.0, 0.0 1.0, 0.0 0.0))"),
            ("multipoint (1 2, 3 4)", "MULTIPOINT (1.0 2.0, 3.0 4.0)"),
            ("MultiPolygon (((0 0, 1 0, 1 1, 0 1, 0 0)))", "MULTIPOLYGON (((0.0 0.0, 1.0 0.0, 1.0 1.0, 0.0 1.0, 0.0 0.0)))"),
            ("geometrycollection (point (1 2))", "GEOMETRYCOLLECTION (POINT (1.0 2.0))"),
        ] {
            let g = read_wkt(wkt).unwrap();
            assert_eq!(write_wkt(&g), expect, "mismatch for {wkt}");
        }
    }

    #[test]
    fn linearring_parsed_as_linestring() {
        let g = read_wkt("LINEARRING (0 0, 0 10, 10 10, 10 0, 0 0)").unwrap();
        assert!(matches!(g, Geometry::LineString(_)));
    }

    #[test]
    fn empty_elements_in_collections() {
        let g = read_wkt("MULTIPOINT (EMPTY, (1 2), empty)").unwrap();
        if let Geometry::MultiPoint(mp) = g {
            assert_eq!(mp.0.len(), 3);
            assert!(mp.0[0].0.x.is_nan() && mp.0[0].0.y.is_nan());
            assert_eq!(mp.0[1], Point::new(1.0, 2.0));
            assert!(mp.0[2].0.x.is_nan());
        } else {
            panic!("expected multipoint");
        }

        let g = read_wkt("MULTILINESTRING (EMPTY, (1 1, 2 2))").unwrap();
        if let Geometry::MultiLineString(mls) = g {
            assert_eq!(mls.0.len(), 2);
            assert!(mls.0[0].0.is_empty());
        } else {
            panic!("expected multilinestring");
        }

        let g = read_wkt("MULTIPOLYGON (EMPTY, ((0 0, 1 0, 1 1, 0 1, 0 0)))").unwrap();
        if let Geometry::MultiPolygon(mp) = g {
            assert_eq!(mp.0.len(), 2);
            assert!(mp.0[0].exterior().0.is_empty());
        } else {
            panic!("expected multipolygon");
        }

        let g = read_wkt("GEOMETRYCOLLECTION (EMPTY, POINT (1 2))").unwrap();
        if let Geometry::GeometryCollection(gc) = g {
            assert_eq!(gc.0.len(), 2);
            assert!(matches!(gc.0[0], Geometry::Point(p) if p.0.x.is_nan()));
        } else {
            panic!("expected geometrycollection");
        }

        // All-EMPTY polygon rings parse to an empty polygon.
        let g = read_wkt("POLYGON (EMPTY, EMPTY, EMPTY)").unwrap();
        if let Geometry::Polygon(p) = g {
            assert!(p.exterior().0.is_empty());
        } else {
            panic!("expected polygon");
        }
    }

    #[test]
    fn malformed_wkt_rejected() {
        let bad = [
            "POINT (EMPTY)",      // EMPTY is a token, not a coordinate
            "LINESTRING ()",      // empty coordinate list without EMPTY keyword
            "POINT (1)",          // missing y
            "POINT (1 2 3)",      // undeclared Z is rejected (2D only)
            "POINT (1 2, 3 4)",   // extra coordinate
            "POINT 1 2",          // missing parens
            "POINT (1 2))",       // unbalanced
            "LINESTRING (1 2,, 3 4)", // double comma
            "POINT (1 2) POINT (3 4)", // trailing geometry
            "",                   // empty input
            "   ",                // whitespace only
            "POINT Z (1 2 3)",    // Z modifier
            "POINT (1 2",         // truncated
            "GEOMETRYCOLLECTION (POINT (1 2)", // unbalanced collection
            "MULTIPOINT ()",      // empty parens without EMPTY keyword
        ];
        for wkt in bad {
            assert!(
                read_wkt(wkt).is_err(),
                "expected rejection of {wkt:?}"
            );
        }
    }

    #[test]
    fn degenerate_ring_parses_but_flagged_by_validator() {
        // A 2-coordinate ring is structurally parsed (Polygon::new
        // auto-closes), then the validator flags it - the reader is
        // deliberately structural; validity is the validator's gate.
        let g = read_wkt("POLYGON ((0 0, 1 1))").unwrap();
        assert!(!crate::validation::validate(&g).valid);
    }

    #[test]
    fn non_ogc_variants_serialized_losslessly() {
        // OGC WKT has no Line/Rect/Triangle; they must serialize to their
        // closest OGC equivalents (coordinate-exact), never silently drop
        // to GEOMETRYCOLLECTION EMPTY.
        let line = Geometry::Line(geo::Line::new(
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 3.0, y: 4.0 },
        ));
        assert_eq!(write_wkt(&line), "LINESTRING (1.0 2.0, 3.0 4.0)");
        assert_eq!(read_wkt(&write_wkt(&line)).unwrap(), Geometry::LineString(LineString::new(vec![
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 3.0, y: 4.0 },
        ])));

        let rect = Geometry::Rect(geo::Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 5.0 },
        ));
        assert_eq!(
            write_wkt(&rect),
            "POLYGON ((0.0 0.0, 10.0 0.0, 10.0 5.0, 0.0 5.0, 0.0 0.0))"
        );

        let tri = Geometry::Triangle(geo::Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
        ));
        assert_eq!(
            write_wkt(&tri),
            "POLYGON ((0.0 0.0, 2.0 0.0, 1.0 2.0, 0.0 0.0))"
        );
        // And they round-trip through the reader.
        assert!(matches!(
            read_wkt(&write_wkt(&tri)).unwrap(),
            Geometry::Polygon(_)
        ));
    }

    #[test]
    fn collection_empties_roundtrip() {
        for wkt in [
            "MULTIPOINT EMPTY",
            "MULTILINESTRING EMPTY",
            "MULTIPOLYGON EMPTY",
            "GEOMETRYCOLLECTION EMPTY",
            "POINT EMPTY",
            "LINESTRING EMPTY",
            "POLYGON EMPTY",
        ] {
            let g = read_wkt(wkt).unwrap();
            assert_eq!(write_wkt(&g), wkt, "writer must canonicalize {wkt}");
            assert_eq!(write_wkt(&read_wkt(&write_wkt(&g)).unwrap()), wkt);
        }
    }

    #[test]
    fn read_wkt_from_and_write_wkt_to() {
        let mut rdr = std::io::Cursor::new(b"POINT (1 2)".to_vec());
        let g = read_wkt_from(&mut rdr).unwrap();
        let mut out = Vec::new();
        write_wkt_to(&g, &mut out).unwrap();
        assert_eq!(out, b"POINT (1.0 2.0)");
    }
}

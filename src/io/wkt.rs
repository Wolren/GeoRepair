//! OGC Well-Known Text (WKT) parsing and serialization.
//!
//! Zero-dependency backend — parses and serializes WKT directly into
//! `geo::Geometry<f64>` without intermediate representations.
//!
//! # Supported types
//!
//! Point, LineString, Polygon, MultiPoint, MultiLineString,
//! MultiPolygon, GeometryCollection — with optional Z/M/ZM modifiers.
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

    fn err(&self, msg: &str) -> String {
        let ctx_start = if self.i > 20 { self.i - 20 } else { 0 };
        let ctx = String::from_utf8_lossy(&self.s[ctx_start..self.s.len().min(self.i + 20)]);
        format!(
            "WKT parse error at position {}: {msg}\n  near: {ctx}",
            self.i
        )
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        self.skip_ws();
        if self.i >= self.s.len() || self.s[self.i] != c {
            return Err(self.err(&format!("expected '{}'", c as char)));
        }
        self.i += 1;
        Ok(())
    }

    fn peek_keyword(&mut self) -> Result<(Keyword, u32), String> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        let kw = &self.s[start..self.i];
        let dims = self.read_dims()?;
        let kw = match kw {
            b"POINT" => Keyword::Point,
            b"LINESTRING" => Keyword::LineString,
            b"POLYGON" => Keyword::Polygon,
            b"MULTIPOINT" => Keyword::MultiPoint,
            b"MULTILINESTRING" => Keyword::MultiLineString,
            b"MULTIPOLYGON" => Keyword::MultiPolygon,
            b"GEOMETRYCOLLECTION" => Keyword::GeometryCollection,
            _ => {
                return Err(self.err(&format!(
                    "unknown geometry type '{}'",
                    String::from_utf8_lossy(kw)
                )))
            }
        };
        Ok((kw, dims))
    }

    fn read_dims(&mut self) -> Result<u32, String> {
        self.skip_ws();
        if self.i + 1 < self.s.len() && &self.s[self.i..self.i + 2] == b"ZM" {
            self.i += 2;
            Ok(4)
        } else if self.i < self.s.len() && self.s[self.i] == b'Z' {
            self.i += 1;
            Ok(3)
        } else if self.i < self.s.len() && self.s[self.i] == b'M' {
            self.i += 1;
            Ok(3)
        } else {
            Ok(2)
        }
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        self.skip_ws();
        let start = self.i;
        if self.i >= self.s.len() {
            return Err(self.err("expected number"));
        }
        if self.s[self.i] == b'-' || self.s[self.i] == b'+' {
            self.i += 1;
        }
        if self.i >= self.s.len() || !self.s[self.i].is_ascii_digit() {
            return Err(self.err("expected digit"));
        }
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_digit()
                || self.s[self.i] == b'.'
                || self.s[self.i] == b'e'
                || self.s[self.i] == b'E'
                || self.s[self.i] == b'+'
                || self.s[self.i] == b'-')
        {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.s[start..self.i])
            .map_err(|_| self.err("invalid utf-8 in number"))?;
        s.parse::<f64>()
            .map_err(|_| self.err(&format!("invalid number '{s}'")))
    }

    fn read_coord(&mut self, dims: u32) -> Result<Coord<f64>, String> {
        let x = self.read_f64()?;
        let y = self.read_f64()?;
        for _ in 2..dims {
            let _ = self.read_f64()?;
        }
        Ok(Coord { x, y })
    }

    fn read_coord_list(&mut self, dims: u32) -> Result<Vec<Coord<f64>>, String> {
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

    fn parse_point(&mut self, dims: u32) -> Result<Geometry<f64>, String> {
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

    fn parse_linestring(&mut self, dims: u32) -> Result<Geometry<f64>, String> {
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

    fn parse_polygon(&mut self, dims: u32) -> Result<Geometry<f64>, String> {
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
            self.expect(b'(')?;
            let coords = self.read_coord_list(dims)?;
            self.expect(b')')?;
            rings.push(LineString::new(coords));
        }
        self.expect(b')')?;
        if rings.is_empty() {
            return Ok(Geometry::Polygon(Polygon::new(
                LineString::new(vec![]),
                vec![],
            )));
        }
        let exterior = rings.swap_remove(0);
        Ok(Geometry::Polygon(Polygon::new(exterior, rings)))
    }

    fn parse_multipoint(&mut self, dims: u32) -> Result<Geometry<f64>, String> {
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
        loop {
            self.skip_ws();
            if self.i >= self.s.len() || self.s[self.i] == b')' {
                break;
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
            self.skip_ws();
            if self.i < self.s.len() && self.s[self.i] == b',' {
                self.i += 1;
            }
        }
        self.expect(b')')?;
        Ok(Geometry::MultiPoint(MultiPoint(points)))
    }

    fn parse_multilinestring(&mut self, dims: u32) -> Result<Geometry<f64>, String> {
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
            self.expect(b'(')?;
            let coords = self.read_coord_list(dims)?;
            self.expect(b')')?;
            lines.push(LineString::new(coords));
        }
        self.expect(b')')?;
        Ok(Geometry::MultiLineString(MultiLineString(lines)))
    }

    fn parse_multipolygon(&mut self, dims: u32) -> Result<Geometry<f64>, String> {
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
            self.expect(b'(')?;
            let mut rings = Vec::new();
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
                self.expect(b'(')?;
                let coords = self.read_coord_list(dims)?;
                self.expect(b')')?;
                rings.push(LineString::new(coords));
            }
            self.expect(b')')?;
            let exterior = if rings.is_empty() {
                LineString::new(vec![])
            } else {
                rings.swap_remove(0)
            };
            polys.push(Polygon::new(exterior, rings));
        }
        self.expect(b')')?;
        Ok(Geometry::MultiPolygon(MultiPolygon(polys)))
    }

    fn parse_geometrycollection(&mut self) -> Result<Geometry<f64>, String> {
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
            geoms.push(self.parse_any()?);
        }
        self.expect(b')')?;
        Ok(Geometry::GeometryCollection(GeometryCollection(geoms)))
    }

    fn parse_any(&mut self) -> Result<Geometry<f64>, String> {
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
/// Handles Z, M, and ZM modifiers (extra dimensions are read and
/// discarded — only X and Y are returned).
pub fn read_wkt(input: &str) -> Result<Geometry<f64>, String> {
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    if p.i >= p.s.len() {
        return Err("WKT parse error: empty input".into());
    }
    let geom = p.parse_any()?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(p.err("trailing characters after geometry"));
    }
    Ok(geom)
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
    use std::fmt::Write as _;
    write!(s, "{v}").unwrap();
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
            s.push_str("POINT (");
            write_coord(s, &p.0);
            s.push(')');
        }
        Geometry::LineString(ls) => {
            s.push_str("LINESTRING (");
            write_linestring(s, ls);
            s.push(')');
        }
        Geometry::Polygon(poly) => {
            s.push_str("POLYGON ");
            write_polygon_rings(s, poly);
        }
        Geometry::MultiPoint(mp) => {
            s.push_str("MULTIPOINT (");
            for (i, p) in mp.0.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                write_coord(s, &p.0);
            }
            s.push(')');
        }
        Geometry::MultiLineString(mls) => {
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
        Geometry::MultiPolygon(mp) => {
            s.push_str("MULTIPOLYGON (");
            for (i, poly) in mp.0.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                write_polygon_rings(s, poly);
            }
            s.push(')');
        }
        Geometry::GeometryCollection(gc) => {
            s.push_str("GEOMETRYCOLLECTION (");
            for (i, g) in gc.0.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                write_geom(s, g);
            }
            s.push(')');
        }
        _ => {
            // Triangle, Rect, etc. — encode as empty GC
            s.push_str("GEOMETRYCOLLECTION EMPTY");
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
        let wkt = "LINESTRING (0 0, 1 1, 2 0)";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_linestring_compact() {
        let geom = read_wkt("LINESTRING(0 0,1 1,2 0)").unwrap();
        let wkt = write_wkt(&geom);
        assert_eq!(wkt, "LINESTRING (0 0, 1 1, 2 0)");
    }

    #[test]
    fn roundtrip_polygon() {
        let wkt = "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_polygon_with_hole() {
        let wkt = "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 8 2, 8 8, 2 8, 2 2))";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_multipoint_parenthesized() {
        let wkt = "MULTIPOINT (1.5 2.5, 3 4)";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_multipoint_double_parens() {
        let wkt = "MULTIPOINT ((1.5 2.5), (3 4))";
        let geom = read_wkt(wkt).unwrap();
        // Our serializer uses the un-parenthesized form
        assert_eq!(write_wkt(&geom), "MULTIPOINT (1.5 2.5, 3 4)");
    }

    #[test]
    fn roundtrip_multilinestring() {
        let wkt = "MULTILINESTRING ((0 0, 1 1), (2 2, 3 3))";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_multipolygon() {
        let wkt = "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)), ((2 2, 3 2, 3 3, 2 3, 2 2)))";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn roundtrip_geometrycollection() {
        let wkt = "GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))";
        let geom = read_wkt(wkt).unwrap();
        assert_eq!(write_wkt(&geom), wkt);
    }

    #[test]
    fn read_invalid_wkt() {
        let err = read_wkt("NOT A GEOMETRY").unwrap_err();
        assert!(err.contains("WKT parse error"), "{err}");
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
    fn z_modifier() {
        let geom = read_wkt("POINT Z (1 2 3)").unwrap();
        if let Geometry::Point(p) = geom {
            assert_eq!(p.x(), 1.0);
            assert_eq!(p.y(), 2.0);
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn zm_modifier() {
        let geom = read_wkt("POINT ZM (1 2 3 4)").unwrap();
        if let Geometry::Point(p) = geom {
            assert_eq!(p.x(), 1.0);
            assert_eq!(p.y(), 2.0);
        } else {
            panic!("expected Point");
        }
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
}

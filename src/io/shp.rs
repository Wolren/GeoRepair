//! ESRI Shapefile (`.shp`) backend via the `shapefile` crate.
//!
//! Reads all shape types (2D, M, and Z variants; M/Z ordinates are dropped).
//! Polygon records use the crate's `PolygonRing::Outer`/`Inner` ring
//! classification: multiple `Outer` rings become a `MultiPolygon` (the
//! shapefile representation of multi-part polygons), `Inner` rings become
//! holes attached to the exterior whose bounding box contains them.
//! Multi-part polylines become `MultiLineString`.
//!
//! Writes polygons/multipolygons as single `Polygon` records with multiple
//! rings, producing `.shp` + `.shx` (no `.dbf` — geometry only; add an
//! attribute table with a GIS tool if attributes are needed).

use alloc::string::String;
use alloc::vec::Vec;
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, Point, Polygon};

use shapefile::{PolygonRing, Shape, ShapeWriter};

/// Load every shape from a `.shp` file as geometries.
pub fn load_shp(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    let mut reader = shapefile::ShapeReader::from_path(path).map_err(|e| format!("{path}: {e}"))?;
    let mut out = Vec::new();
    for (i, shape) in reader.iter_shapes().enumerate() {
        let shape = shape.map_err(|e| format!("{path}: record {}: {e}", i + 1))?;
        out.push(
            shape_to_geo(shape)
                .ok_or_else(|| format!("{path}: record {}: unsupported shape type", i + 1))?,
        );
    }
    Ok(out)
}

/// Save polygons (and multipolygons) to a `.shp` file.
///
/// Non-polygon geometries are skipped; an empty polygon set is an error.
pub fn save_shp(path: &str, geoms: &[Geometry<f64>]) -> Result<(), String> {
    let polys: Vec<Geometry<f64>> = geoms
        .iter()
        .filter(|g| matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)))
        .cloned()
        .collect();
    if polys.is_empty() {
        return Err(format!("{path}: no polygon geometries to write"));
    }
    let mut writer = ShapeWriter::from_path(path).map_err(|e| format!("{path}: {e}"))?;
    for g in &polys {
        // Shape::try_from maps Polygon -> Polygon record and MultiPolygon ->
        // one Polygon record with several outer rings (shapefile semantics).
        let shape =
            Shape::try_from(g.clone()).map_err(|e| format!("{path}: shape conversion: {e}"))?;
        // Each polygon variant carries a different point type but all
        // implement EsriShape, so write through a match on the record.
        match shape {
            Shape::Polygon(p) => writer.write_shape(&p),
            Shape::PolygonM(p) => writer.write_shape(&p),
            Shape::PolygonZ(p) => writer.write_shape(&p),
            _ => return Err(format!("{path}: conversion produced non-polygon shape")),
        }
        .map_err(|e| format!("{path}: {e}"))?;
    }
    // Headers are finalized on drop (flush returns nothing usable here).
    Ok(())
}

/// Convert a shapefile `Shape` to a geo `Geometry`; `None` for types with no
/// geo equivalent (null shapes, multipatch).
fn shape_to_geo(shape: Shape) -> Option<Geometry<f64>> {
    Some(match shape {
        Shape::NullShape | Shape::Multipatch(_) => return None,
        Shape::Point(p) => Geometry::Point(Point::new(p.x, p.y)),
        Shape::PointM(p) => Geometry::Point(Point::new(p.x, p.y)),
        Shape::PointZ(p) => Geometry::Point(Point::new(p.x, p.y)),
        Shape::Polyline(l) => parts_to_line_geometry(l.parts()),
        Shape::PolylineM(l) => parts_to_line_geometry(l.parts()),
        Shape::PolylineZ(l) => parts_to_line_geometry(l.parts()),
        Shape::Polygon(p) => rings_to_geometry(p.rings()),
        Shape::PolygonM(p) => rings_to_geometry(p.rings()),
        Shape::PolygonZ(p) => rings_to_geometry(p.rings()),
        Shape::Multipoint(mp) => Geometry::MultiPoint(MultiPoint(
            mp.points().iter().map(|p| Point::new(p.x, p.y)).collect(),
        )),
        Shape::MultipointM(mp) => Geometry::MultiPoint(MultiPoint(
            mp.points().iter().map(|p| Point::new(p.x, p.y)).collect(),
        )),
        Shape::MultipointZ(mp) => Geometry::MultiPoint(MultiPoint(
            mp.points().iter().map(|p| Point::new(p.x, p.y)).collect(),
        )),
    })
}

/// One part -> LineString, several parts -> MultiLineString.
fn parts_to_line_geometry<P: CoordLike>(parts: &[Vec<P>]) -> Geometry<f64> {
    let lines: Vec<LineString<f64>> = parts.iter().map(|p| pts_to_ls(p)).collect();
    if lines.len() == 1 {
        Geometry::LineString(lines.into_iter().next().unwrap())
    } else {
        Geometry::MultiLineString(MultiLineString(lines))
    }
}

/// Convert a record's ring list to a Polygon or MultiPolygon.
///
/// Outer rings are exteriors (multiple outers = multi-part polygon); inner
/// rings are holes attached to the smallest exterior whose bounding box
/// contains the hole's first vertex.
fn rings_to_geometry<P: CoordLike>(rings: &[PolygonRing<P>]) -> Geometry<f64> {
    let mut exteriors: Vec<LineString<f64>> = Vec::new();
    let mut holes: Vec<LineString<f64>> = Vec::new();
    for ring in rings {
        match ring {
            PolygonRing::Outer(pts) => exteriors.push(pts_to_ls(pts)),
            PolygonRing::Inner(pts) => holes.push(pts_to_ls(pts)),
        }
    }
    if exteriors.is_empty() {
        return Geometry::Polygon(Polygon::new(LineString(Vec::new()), Vec::new()));
    }
    if exteriors.len() == 1 {
        return Geometry::Polygon(Polygon::new(exteriors.pop().unwrap(), holes));
    }
    let ext_bbox: Vec<(LineString<f64>, [f64; 4])> = exteriors
        .into_iter()
        .map(|ls| {
            let mut bb = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for c in &ls.0 {
                bb[0] = bb[0].min(c.x);
                bb[1] = bb[1].min(c.y);
                bb[2] = bb[2].max(c.x);
                bb[3] = bb[3].max(c.y);
            }
            (ls, bb)
        })
        .collect();
    let mut hole_assign: Vec<Vec<LineString<f64>>> = vec![Vec::new(); ext_bbox.len()];
    for h in holes {
        let anchor = h.0.first().copied().unwrap_or(Coord { x: 0.0, y: 0.0 });
        let mut best: Option<usize> = None;
        let mut best_area = f64::MAX;
        for (i, (_, bb)) in ext_bbox.iter().enumerate() {
            if anchor.x >= bb[0] && anchor.x <= bb[2] && anchor.y >= bb[1] && anchor.y <= bb[3] {
                let area = (bb[2] - bb[0]) * (bb[3] - bb[1]);
                if area < best_area {
                    best_area = area;
                    best = Some(i);
                }
            }
        }
        hole_assign[best.unwrap_or(0)].push(h);
    }
    let mut parts: Vec<Polygon<f64>> = Vec::new();
    for (i, (ls, _)) in ext_bbox.into_iter().enumerate() {
        parts.push(Polygon::new(ls, std::mem::take(&mut hole_assign[i])));
    }
    Geometry::MultiPolygon(geo::MultiPolygon(parts))
}

fn pts_to_ls<P: CoordLike>(pts: &[P]) -> LineString<f64> {
    LineString(pts.iter().map(|p| p.as_coord()).collect())
}

/// Minimal abstraction over the shapefile point types (`Point`, `PointZ`,
/// `PointM`), which all expose `x`/`y`.
trait CoordLike {
    fn as_coord(&self) -> Coord<f64>;
}

impl CoordLike for shapefile::Point {
    fn as_coord(&self) -> Coord<f64> {
        Coord {
            x: self.x,
            y: self.y,
        }
    }
}

impl CoordLike for shapefile::PointZ {
    fn as_coord(&self) -> Coord<f64> {
        Coord {
            x: self.x,
            y: self.y,
        }
    }
}

impl CoordLike for shapefile::PointM {
    fn as_coord(&self) -> Coord<f64> {
        Coord {
            x: self.x,
            y: self.y,
        }
    }
}

use std::fs::File;
use std::io::{BufWriter, Read, Write};

use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};

/// Compute signed area of a closed ring (positive = CCW).
/// The ring should be closed (first == last).
pub fn signed_area(ring: &[Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}

/// Area of a Polygon exterior (absolute).
pub fn polygon_area(p: &Polygon<f64>) -> f64 {
    signed_area(&p.exterior().0).abs()
}

/// Area of a Geometry (Polygon or MultiPolygon — sum of exteriors).
pub fn geo_area(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => polygon_area(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon_area).sum(),
        _ => 0.0,
    }
}

/// Count sub-polygons in a Geometry result.
pub fn count_sub_polys(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::Polygon(_) => 1,
        Geometry::MultiPolygon(mp) => mp.0.len(),
        _ => 0,
    }
}

/// Load polygons from a shapefile at `path`. Each shapefile Polygon ring
/// is grouped into geo::Polygon using signed-area winding direction to
/// distinguish exterior vs holes.
#[cfg(feature = "load-shp")]
pub fn load_shp(path: &str) -> Vec<Polygon<f64>> {
    let mut reader = shapefile::Reader::from_path(path).unwrap();
    let mut all_rings: Vec<Vec<Coord<f64>>> = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, _) = result.unwrap();
        if let shapefile::Shape::Polygon(poly) = shape {
            for r in poly.rings() {
                let coords: Vec<Coord<f64>> = r
                    .clone()
                    .into_inner()
                    .into_iter()
                    .map(|p| Coord { x: p.x, y: p.y })
                    .collect();
                all_rings.push(coords);
            }
        }
    }

    let mut polys: Vec<Polygon<f64>> = Vec::new();
    let first_idx = all_rings.iter().position(|r| signed_area(r).abs() > 1e-12);
    if let Some(first) = first_idx {
        let ref_area = signed_area(&all_rings[first]);
        let mut cur_ext: Option<Vec<Coord<f64>>> = None;
        let mut cur_holes: Vec<Vec<Coord<f64>>> = Vec::new();
        for (i, ring) in all_rings.into_iter().enumerate() {
            if signed_area(&ring).abs() < 1e-12 {
                continue;
            }
            if i == first || cur_ext.is_none() {
                if let Some(ext) = cur_ext.take() {
                    polys.push(Polygon::new(
                        LineString::new(ext),
                        cur_holes.drain(..).map(LineString::new).collect(),
                    ));
                }
                cur_ext = Some(ring);
            } else {
                if signed_area(&ring) * ref_area > 0.0 {
                    if let Some(ext) = cur_ext.take() {
                        polys.push(Polygon::new(
                            LineString::new(ext),
                            cur_holes.drain(..).map(LineString::new).collect(),
                        ));
                    }
                    cur_ext = Some(ring);
                } else {
                    cur_holes.push(ring);
                }
            }
        }
        if let Some(ext) = cur_ext.take() {
            polys.push(Polygon::new(
                LineString::new(ext),
                cur_holes.drain(..).map(LineString::new).collect(),
            ));
        }
    }
    polys
}

/// Load polygons from custom binary format:
///   [u32: n_polys]
///   for each: [ring_data] [u32: n_holes] [hole_rings...]
///   each ring: [u32: n_coords] [f64: x, f64: y] × n_coords
pub fn load_bin(path: &str) -> Vec<Polygon<f64>> {
    let mut buf = Vec::new();
    File::open(path).unwrap().read_to_end(&mut buf).unwrap();
    let mut pos = 0;

    let read_u32 = |buf: &[u8], pos: &mut usize| -> u32 {
        let v = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().unwrap());
        *pos += 4;
        v
    };
    let read_f64 = |buf: &[u8], pos: &mut usize| -> f64 {
        let v = f64::from_le_bytes(buf[*pos..*pos + 8].try_into().unwrap());
        *pos += 8;
        v
    };
    let read_ring = |buf: &[u8], pos: &mut usize| -> LineString<f64> {
        let n = read_u32(buf, pos) as usize;
        let mut coords = Vec::with_capacity(n);
        for _ in 0..n {
            coords.push(Coord {
                x: read_f64(buf, pos),
                y: read_f64(buf, pos),
            });
        }
        LineString::new(coords)
    };

    let n_polys = read_u32(&buf, &mut pos) as usize;
    let mut polys = Vec::with_capacity(n_polys);
    for _ in 0..n_polys {
        let ext = read_ring(&buf, &mut pos);
        let n_holes = read_u32(&buf, &mut pos) as usize;
        let mut holes = Vec::with_capacity(n_holes);
        for _ in 0..n_holes {
            holes.push(read_ring(&buf, &mut pos));
        }
        polys.push(Polygon::new(ext, holes));
    }
    polys
}

/// Streaming shapefile reader — yields polygons one at a time without loading
/// the entire file into memory. Uses the shapefile crate's built-in geo-types
/// conversion for ring-to-polygon grouping.
#[cfg(feature = "load-shp")]
pub fn load_shp_stream(path: &str) -> impl Iterator<Item = Polygon<f64>> {
    let reader = shapefile::Reader::from_path(path).unwrap();
    LoadShpStream {
        reader,
        buf: Vec::new(),
    }
}

#[cfg(feature = "load-shp")]
struct LoadShpStream {
    reader: shapefile::Reader<std::io::BufReader<std::fs::File>, std::io::BufReader<std::fs::File>>,
    buf: Vec<Polygon<f64>>,
}

#[cfg(feature = "load-shp")]
impl Iterator for LoadShpStream {
    type Item = Polygon<f64>;

    fn next(&mut self) -> Option<Polygon<f64>> {
        if let Some(p) = self.buf.pop() {
            return Some(p);
        }
        loop {
            match self.reader.iter_shapes_and_records().next() {
                None => return None,
                Some(Ok((shape, _))) => {
                    if let shapefile::Shape::Polygon(poly) = shape {
                        let mp: Result<MultiPolygon<f64>, _> = poly.try_into();
                        if let Ok(mp) = mp {
                            let mut members = mp.0;
                            if let Some(first) = members.pop() {
                                self.buf = members;
                                return Some(first);
                            }
                        }
                    }
                }
                Some(Err(_)) => continue,
            }
        }
    }
}

/// Streaming binary reader — yields polygons one at a time without loading
/// the entire file into memory. Matches the `load_bin` wire format.
pub fn load_bin_stream(path: &str) -> impl Iterator<Item = Polygon<f64>> {
    let file = std::fs::File::open(path).unwrap();
    let reader = std::io::BufReader::new(file);
    LoadBinStream {
        reader,
        header_read: false,
        total_polys: 0,
        polys_yielded: 0,
    }
}

struct LoadBinStream {
    reader: std::io::BufReader<std::fs::File>,
    header_read: bool,
    total_polys: u32,
    polys_yielded: u32,
}

fn read_u32<R: std::io::Read>(r: &mut R) -> u32 {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).unwrap();
    u32::from_le_bytes(buf)
}

fn read_f64<R: std::io::Read>(r: &mut R) -> f64 {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf).unwrap();
    f64::from_le_bytes(buf)
}

fn read_ring<R: std::io::Read>(r: &mut R) -> LineString<f64> {
    let n = read_u32(r) as usize;
    let mut coords = Vec::with_capacity(n);
    for _ in 0..n {
        coords.push(Coord {
            x: read_f64(r),
            y: read_f64(r),
        });
    }
    LineString::new(coords)
}

impl Iterator for LoadBinStream {
    type Item = Polygon<f64>;

    fn next(&mut self) -> Option<Polygon<f64>> {
        if !self.header_read {
            self.total_polys = read_u32(&mut self.reader);
            self.header_read = true;
            if self.total_polys == 0 {
                return None;
            }
        }
        if self.polys_yielded >= self.total_polys {
            return None;
        }
        self.polys_yielded += 1;
        let ext = read_ring(&mut self.reader);
        let n_holes = read_u32(&mut self.reader) as usize;
        let mut holes = Vec::with_capacity(n_holes);
        for _ in 0..n_holes {
            holes.push(read_ring(&mut self.reader));
        }
        Some(Polygon::new(ext, holes))
    }
}

// ---------------------------------------------------------------------------
// GeoJSON export helpers
// ---------------------------------------------------------------------------

fn write_ring(f: &mut dyn Write, ring: &[Coord<f64>]) -> std::io::Result<()> {
    write!(f, "[")?;
    for (i, c) in ring.iter().enumerate() {
        if i > 0 {
            write!(f, ",")?;
        }
        write!(f, "[{},{}]", c.x, c.y)?;
    }
    write!(f, "]")
}

fn write_geometry_json(f: &mut dyn Write, g: &Geometry<f64>) -> std::io::Result<()> {
    match g {
        Geometry::Polygon(p) => {
            write!(f, "{{\"type\":\"Polygon\",\"coordinates\":[")?;
            write_ring(f, &p.exterior().0)?;
            for h in p.interiors() {
                write!(f, ",")?;
                write_ring(f, &h.0)?;
            }
            write!(f, "]}}")?;
        }
        Geometry::MultiPolygon(mp) => {
            write!(f, "{{\"type\":\"MultiPolygon\",\"coordinates\":[")?;
            for (pi, p) in mp.0.iter().enumerate() {
                if pi > 0 {
                    write!(f, ",")?;
                }
                write!(f, "[")?;
                write_ring(f, &p.exterior().0)?;
                for h in p.interiors() {
                    write!(f, ",")?;
                    write_ring(f, &h.0)?;
                }
                write!(f, "]")?;
            }
            write!(f, "]}}")?;
        }
        _ => write!(f, "null")?,
    }
    Ok(())
}

/// Write a FeatureCollection to `path` with one feature per fixed polygon result.
/// Each feature includes index, validity, and area-ratio properties.
/// If `crs_name` is provided, a `crs` member is written to the collection.
pub fn export_geojson(
    polys: &[Polygon<f64>],
    results: &[Geometry<f64>],
    geos_valid: &[bool],
    path: &str,
    crs_name: Option<&str>,
) -> std::io::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);

    write!(f, "{{")?;
    if let Some(crs) = crs_name {
        write!(
            f,
            "\"crs\":{{\"type\":\"name\",\"properties\":{{\"name\":\"{crs}\"}}}},"
        )?;
    }
    write!(f, "\"type\":\"FeatureCollection\",\"features\":[")?;

    for (i, (p, g)) in polys.iter().zip(results.iter()).enumerate() {
        let input_area = polygon_area(p);
        let output_area = geo_area(g);
        let ratio = if input_area > 0.0 {
            output_area / input_area
        } else {
            0.0
        };
        let out_polys = count_sub_polys(g);

        if i > 0 {
            write!(f, ",")?;
        }
        write!(
            f,
            "{{\"type\":\"Feature\",\"properties\":{{\"id\":{i},\"geos_valid\":{},\"input_area\":{input_area:.0},\"output_area\":{output_area:.0},\"area_ratio\":{ratio:.4},\"output_polys\":{out_polys}}},\"geometry\":",
            geos_valid[i]
        )?;
        write_geometry_json(&mut f, g)?;
        write!(f, "}}")?;
    }

    writeln!(f, "]}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_area_square() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!((signed_area(&ring) - 100.0).abs() < 1e-12);
    }

    #[test]
    fn test_polygon_area() {
        let ext = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let p = Polygon::new(ext, vec![]);
        assert!((polygon_area(&p) - 100.0).abs() < 1e-12);
    }
}

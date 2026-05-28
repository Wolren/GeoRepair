//! Convert a shapefile to a flat binary format for fast loading.
//!
//! Properly handles multi-part polygons: splits by ring winding direction.
//!
//! Usage:
//!   cargo run --release --example convert_shp [input.shp] [output.bin]
//!
//! Defaults:
//!   input:  benches/real_world/data_0.shp
//!   output: benches/real_world/data_0.bin
//!
//! Binary format (little-endian):
//!   [u32: n_polygons]
//!   for each polygon:
//!     [u32: n_ext_pts]    // exterior ring point count
//!     [f64; n_ext_pts*2]  // interleaved x,y
//!     [u32: n_holes]
//!     for each hole:
//!       [u32: n_pts]
//!       [f64; n_pts*2]

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use geo::{Coord, LineString, Polygon};

fn ring_to_coords(ring: shapefile::PolygonRing<shapefile::Point>) -> Vec<Coord<f64>> {
    ring.into_inner()
        .into_iter()
        .map(|p| Coord { x: p.x, y: p.y })
        .collect()
}

fn ring_z_to_coords(ring: shapefile::PolygonRing<shapefile::PointZ>) -> Vec<Coord<f64>> {
    ring.into_inner()
        .into_iter()
        .map(|p| Coord { x: p.x, y: p.y })
        .collect()
}

fn signed_area(ring: &[Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}

/// Split rings into proper polygons by detecting winding direction.
/// First ring is the reference outer. Subsequent rings with same
/// winding start new polygons; opposite winding are holes.
fn rings_to_polygons(rings: Vec<Vec<Coord<f64>>>) -> Vec<Polygon<f64>> {
    if rings.is_empty() {
        return vec![];
    }

    // Find first non-degenerate ring for reference winding
    let first_idx = rings.iter().position(|r| signed_area(r).abs() > 1e-12);
    let first_idx = match first_idx {
        Some(i) => i,
        None => return vec![],
    };

    let ref_area = signed_area(&rings[first_idx]);
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    let mut cur_ext: Option<Vec<Coord<f64>>> = None;
    let mut cur_holes: Vec<Vec<Coord<f64>>> = Vec::new();

    for (i, ring) in rings.into_iter().enumerate() {
        // Skip degenerate rings (zero area)
        if signed_area(&ring).abs() < 1e-12 {
            continue;
        }

        if i == first_idx || cur_ext.is_none() {
            // Start or continue with outer ring
            if let Some(ext) = cur_ext.take() {
                polys.push(Polygon::new(
                    LineString::new(ext),
                    cur_holes.drain(..).map(LineString::new).collect(),
                ));
            }
            cur_ext = Some(ring);
        } else {
            let area = signed_area(&ring);
            if area * ref_area > 0.0 {
                // Same winding → new polygon part
                if let Some(ext) = cur_ext.take() {
                    polys.push(Polygon::new(
                        LineString::new(ext),
                        cur_holes.drain(..).map(LineString::new).collect(),
                    ));
                }
                cur_ext = Some(ring);
            } else {
                // Opposite winding → hole
                cur_holes.push(ring);
            }
        }
    }

    if let Some(ext) = cur_ext {
        polys.push(Polygon::new(
            LineString::new(ext),
            cur_holes.drain(..).map(LineString::new).collect(),
        ));
    }

    polys
}

fn write_polygon<W: Write>(w: &mut W, poly: &Polygon<f64>) -> std::io::Result<()> {
    let ext = poly.exterior();
    w.write_all(&(ext.0.len() as u32).to_le_bytes())?;
    for coord in &ext.0 {
        w.write_all(&coord.x.to_le_bytes())?;
        w.write_all(&coord.y.to_le_bytes())?;
    }
    let holes = poly.interiors();
    w.write_all(&(holes.len() as u32).to_le_bytes())?;
    for hole in holes {
        w.write_all(&(hole.0.len() as u32).to_le_bytes())?;
        for coord in &hole.0 {
            w.write_all(&coord.x.to_le_bytes())?;
            w.write_all(&coord.y.to_le_bytes())?;
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "benches/real_world/data_0.shp".to_string());
    let output = args.get(2).cloned().unwrap_or_else(|| {
        let stem = Path::new(&input).with_extension("");
        format!("{}.bin", stem.display())
    });

    let mut reader = shapefile::ShapeReader::from_path(&input).unwrap();
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    let mut shape_count = 0usize;
    let mut part_count = 0usize;

    eprintln!("Reading SHP: {input}...");
    let t0 = Instant::now();
    for result in reader.iter_shapes() {
        shape_count += 1;
        if shape_count % 100_000 == 0 {
            eprintln!("  read {shape_count} shapes...");
        }
        let shape = match result {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  warning (shape {shape_count}): {e}");
                continue;
            }
        };
        let rings: Vec<Vec<Coord<f64>>> = match shape {
            shapefile::Shape::Polygon(p) => {
                p.into_inner().into_iter().map(ring_to_coords).collect()
            }
            shapefile::Shape::PolygonZ(p) => {
                p.into_inner().into_iter().map(ring_z_to_coords).collect()
            }
            _ => continue,
        };
        let split = rings_to_polygons(rings);
        part_count += split.len();
        polys.extend(split);
    }
    let elapsed = t0.elapsed();
    eprintln!(
        "Read {poly} polygons from {shp} shapes ({parts} parts) in {t:.3}s",
        poly = polys.len(),
        shp = shape_count,
        parts = part_count,
        t = elapsed.as_secs_f64()
    );

    eprintln!("Writing binary: {output}...");
    let t0 = Instant::now();
    let f = File::create(&output).unwrap();
    let mut w = BufWriter::new(f);
    w.write_all(&(polys.len() as u32).to_le_bytes()).unwrap();
    for poly in &polys {
        write_polygon(&mut w, poly).unwrap();
    }
    w.flush().unwrap();
    let elapsed = t0.elapsed();
    eprintln!(
        "Wrote {poly} polygons in {t:.3}s",
        poly = polys.len(),
        t = elapsed.as_secs_f64()
    );
}

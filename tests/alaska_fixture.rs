//! Alaska SHP regression tests.
//! Loads the real Alaska SHP, runs Structure fix on all GEOS-invalid polys,
//! and verifies every fixed output is GEOS-valid.
//! Requires `bench-geos` feature and the Alaska SHP at the expected path.

#![cfg(feature = "bench-geos")]

use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use geos::Geom;
use wkt::ToWkt;

fn load_alaska_shp() -> Vec<Polygon<f64>> {
    let path = "benches/real_world/alaska.shp";
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

    // Convert rings to polygons via signed-area winding
    fn signed_area(ring: &[Coord<f64>]) -> f64 {
        let mut s = 0.0;
        for w in ring.windows(2) {
            s += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        s / 2.0
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

#[test]
fn alaska_structure_output_all_geos_valid() {
    let polys = load_alaska_shp();

    // Find GEOS-invalid input polys
    let mut invalid_indices = Vec::new();
    for (i, p) in polys.iter().enumerate() {
        if let Ok(g) = geos::Geometry::new_from_wkt(&p.wkt_string()) {
            if !g.is_valid().unwrap_or(false) {
                invalid_indices.push(i);
            }
        }
    }

    // Skip if no invalid polys found
    if invalid_indices.is_empty() {
        eprintln!("No GEOS-invalid Alaska polys found — skipping test");
        return;
    }

    eprintln!(
        "Testing {} GEOS-invalid Alaska polys...",
        invalid_indices.len()
    );

    // Fix with Structure
    let invalid: Vec<&Polygon<f64>> = invalid_indices.iter().map(|&i| &polys[i]).collect();
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let results = par_fix_polygon_batch(&invalid, &cfg);

    // Debug: try buffer(0) on failing polys to see panic reason
    for &idx in &invalid_indices {
        let p = &polys[idx];
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            use geo::Buffer;
            let _ = p.buffer(0.0);
        }));
        if let Err(e) = r {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", e));
            eprintln!("  buffer(0) FAILED for poly #{idx}: {msg}");
        } else {
            eprintln!("  buffer(0) SUCCEEDED for poly #{idx}");
        }
    }

    // Verify all outputs are GEOS-valid
    let mut failures = Vec::new();
    for (ri, g) in results.iter().enumerate() {
        let wkt = g.wkt_string();
        match geos::Geometry::new_from_wkt(&wkt) {
            Ok(gg) => {
                if !gg.is_valid().unwrap_or(false) {
                    failures.push((invalid_indices[ri], "GEOS is_valid returned false".into()));
                }
            }
            Err(e) => {
                failures.push((invalid_indices[ri], format!("WKT error: {e}")));
            }
        }
    }

    if !failures.is_empty() {
        for (idx, reason) in &failures {
            eprintln!("  Alaska poly #{idx}: output STILL GEOS-invalid: {reason}");
        }
        panic!(
            "{} Alaska polys still GEOS-invalid after Structure fix",
            failures.len()
        );
    }
}

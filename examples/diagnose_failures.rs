/// Diagnose the 3 Alaska polys that remain GEOS-invalid after our Structure fix.
/// Loads from the fixed_invalid.geojson output and compares with GEOS's own fix.
fn main() {
    use geo::Polygon;
    use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
    use geos::Geom;
    use wkt::ToWkt;

    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let known_bad = [590usize, 630, 638];

    // Load SHP directly and test the known-bad indices
    let path = "benches/real_world/alaska.shp";
    let mut reader = shapefile::Reader::from_path(path).unwrap();

    let mut shapes = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, _) = result.unwrap();
        if let shapefile::Shape::Polygon(poly) = shape {
            // Collect rings into coordinates, skip huge ones
            let mut all_rings = Vec::new();
            for r in poly.rings() {
                let coords: Vec<geo::Coord<f64>> = r
                    .clone()
                    .into_inner()
                    .into_iter()
                    .map(|p| geo::Coord { x: p.x, y: p.y })
                    .collect();
                if coords.len() > 100_000 {
                    all_rings.clear();
                    break;
                }
                all_rings.push(coords);
            }
            if !all_rings.is_empty() {
                shapes.push(all_rings);
            }
        }
    }

    // Convert to geo Polygons via signed area
    fn signed_area(ring: &[geo::Coord<f64>]) -> f64 {
        let mut s = 0.0;
        for w in ring.windows(2) {
            s += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        s / 2.0
    }

    let mut index = 0usize;
    for rings in &shapes {
        let mut polys = Vec::new();
        let first_idx = rings.iter().position(|r| signed_area(r).abs() > 1e-12);
        if let Some(first) = first_idx {
            let ref_area = signed_area(&rings[first]);
            let mut cur_ext: Option<Vec<geo::Coord<f64>>> = None;
            let mut cur_holes: Vec<Vec<geo::Coord<f64>>> = Vec::new();
            for (i, ring) in rings.iter().enumerate() {
                if signed_area(ring).abs() < 1e-12 {
                    continue;
                }
                if i == first || cur_ext.is_none() {
                    if let Some(ext) = cur_ext.take() {
                        polys.push(Polygon::new(
                            geo::LineString::new(ext),
                            cur_holes.drain(..).map(geo::LineString::new).collect(),
                        ));
                    }
                    cur_ext = Some(ring.clone());
                } else {
                    if signed_area(ring) * ref_area > 0.0 {
                        if let Some(ext) = cur_ext.take() {
                            polys.push(Polygon::new(
                                geo::LineString::new(ext),
                                cur_holes.drain(..).map(geo::LineString::new).collect(),
                            ));
                        }
                        cur_ext = Some(ring.clone());
                    } else {
                        cur_holes.push(ring.clone());
                    }
                }
            }
            if let Some(ext) = cur_ext.take() {
                polys.push(Polygon::new(
                    geo::LineString::new(ext),
                    cur_holes.drain(..).map(geo::LineString::new).collect(),
                ));
            }
        }

        // Find our polys by index
        for p in &polys {
            if known_bad.contains(&index) {
                let nv =
                    p.exterior().0.len() + p.interiors().iter().map(|h| h.0.len()).sum::<usize>();
                eprintln!("\n=== SHP poly #{index} ({nv} verts) ===");

                // Our Structure fix
                let our = p.make_valid_with_config(&cfg);
                let our_wkt = our.wkt_string();

                // GEOS fix
                let geos_fixed = geos::Geometry::new_from_wkt(&p.wkt_string())
                    .unwrap()
                    .make_valid()
                    .unwrap();
                let geos_wkt = geos_fixed.to_wkt().unwrap_or_default();

                eprintln!(
                    "  Our parts: ~{}  GEOS parts: ~{}",
                    our_wkt.matches("POLYGON").count(),
                    geos_wkt.matches("POLYGON").count()
                );
                eprintln!(
                    "  Our WKT len: {}  GEOS WKT len: {}",
                    our_wkt.len(),
                    geos_wkt.len()
                );

                // Check OGC validity of our output
                use geo::validation::Validation;
                eprintln!("  Our OGC valid: {:?}", our.check_validation().is_ok());
                if let Err(e) = our.check_validation() {
                    eprintln!("  OGC error: {e:?}");
                }

                // Buffer comparison
                use geo::Buffer;
                let mp = p.buffer(0.0);
                let buf_wkt = mp.wkt_string();
                if let Ok(gg) = geos::Geometry::new_from_wkt(&buf_wkt) {
                    eprintln!(
                        "  geo::Buffer({} parts) GEOS-valid: {}",
                        mp.0.len(),
                        gg.is_valid().unwrap_or(false)
                    );
                }

                // Compare first 50 chars of WKT
                let o = our_wkt.chars().take(60).collect::<String>();
                let g = geos_wkt.chars().take(60).collect::<String>();
                eprintln!("  Our WKT start:  {o}");
                eprintln!("  GEOS WKT start: {g}");
            }
            index += 1;
        }
    }
}

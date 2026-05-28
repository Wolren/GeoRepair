fn write_ring(f: &mut dyn std::io::Write, ring: &[geo::Coord<f64>]) -> std::io::Result<()> {
    write!(f, "[")?;
    for (i, c) in ring.iter().enumerate() {
        if i > 0 {
            write!(f, ",")?;
        }
        write!(f, "[{},{}]", c.x, c.y)?;
    }
    write!(f, "]")
}
fn ring_to_coords(ring: shapefile::PolygonRing<shapefile::Point>) -> Vec<geo::Coord<f64>> {
    ring.into_inner()
        .into_iter()
        .map(|p| geo::Coord { x: p.x, y: p.y })
        .collect()
}
fn signed_area(ring: &[geo::Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}
fn main() {
    use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};
    use geo_repair::{parallel::par_fix_polygon_batch, MakeValid, MakeValidConfig, PolyMethod};
    use geos::Geom;
    use std::env;
    use std::fs::File;
    use std::io::{BufWriter, Write};
    use wkt::ToWkt;
    let args: Vec<String> = env::args().collect();
    let in_path = args
        .get(1)
        .cloned()
        .unwrap_or("benches/real_world/data_0.shp".into());
    let out_path = args.get(2).cloned().unwrap_or_else(|| {
        let t = in_path.trim_end_matches(".shp");
        format!("{t}_fixed.geojson")
    });
    let mut reader = shapefile::Reader::from_path(&in_path).unwrap();
    let mut all_rings: Vec<Vec<Coord<f64>>> = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, _) = result.unwrap();
        if let shapefile::Shape::Polygon(poly) = shape {
            for r in poly.rings() {
                all_rings.push(ring_to_coords(r.clone()));
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
    eprintln!("Loaded {} polys", polys.len());

    let mut invalid_idx = Vec::new();
    for (i, p) in polys.iter().enumerate() {
        let wkt = p.wkt_string();
        let geos_valid = geos::Geometry::new_from_wkt(&wkt)
            .ok()
            .and_then(|g| g.is_valid().ok())
            .unwrap_or(false);
        if !geos_valid {
            invalid_idx.push(i);
        }
    }
    eprintln!("{}/{} GEOS-invalid", invalid_idx.len(), polys.len());

    let invalid: Vec<&Polygon<f64>> = invalid_idx.iter().map(|&i| &polys[i]).collect();
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let results = par_fix_polygon_batch(&invalid, &cfg);

    // Check with GEOS
    let mut geos_bad = 0;
    for (ri, g) in results.iter().enumerate() {
        if let Ok(gg) = geos::Geometry::new_from_wkt(&g.wkt_string()) {
            if !gg.is_valid().unwrap_or(false) {
                geos_bad += 1;
                eprintln!("  GEOS invalid: poly #{}", invalid_idx[ri]);
            }
        }
    }
    eprintln!("GEOS-invalid after fix: {geos_bad}");

    let mut f = BufWriter::new(File::create(&out_path).unwrap());
    write!(f, "{{\"type\":\"FeatureCollection\",\"features\":[").unwrap();
    for (ri, g) in results.iter().enumerate() {
        if ri > 0 {
            write!(f, ",").unwrap();
        }
        write!(
            f,
            "{{\"type\":\"Feature\",\"properties\":{{\"id\":{}}},\"geometry\":",
            invalid_idx[ri]
        )
        .unwrap();
        match g {
            Geometry::Polygon(p) => {
                write!(f, "{{\"type\":\"Polygon\",\"coordinates\":[").unwrap();
                write_ring(&mut f, &p.exterior().0).unwrap();
                for h in p.interiors() {
                    write!(f, ",").unwrap();
                    write_ring(&mut f, &h.0).unwrap();
                }
                write!(f, "]}}").unwrap();
            }
            Geometry::MultiPolygon(mp) => {
                write!(f, "{{\"type\":\"MultiPolygon\",\"coordinates\":[").unwrap();
                for (pi, p) in mp.0.iter().enumerate() {
                    if pi > 0 {
                        write!(f, ",").unwrap();
                    }
                    write!(f, "[").unwrap();
                    write_ring(&mut f, &p.exterior().0).unwrap();
                    for h in p.interiors() {
                        write!(f, ",").unwrap();
                        write_ring(&mut f, &h.0).unwrap();
                    }
                    write!(f, "]").unwrap();
                }
                write!(f, "]}}").unwrap();
            }
            _ => write!(f, "null").unwrap(),
        }
        write!(f, "}}").unwrap();
    }
    writeln!(f, "]}}").unwrap();
    eprintln!("Wrote {} fixed polys to {out_path}", invalid_idx.len());
}

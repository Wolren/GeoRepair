//! TEMP diagnostic: single_pass_fix acceptance + timing buckets by size.
use geo::Polygon;
use geo_repair::structure::fix_ring::single_pass_fix;

fn main() {
    let polys = geo_repair::io::load_bin("benches/real_world/data_0.bin").expect("load");
    let invalid: Vec<&Polygon<f64>> = polys
        .iter()
        .filter(|p| !geo_repair::arrange::validate_polygon(p))
        .collect();
    println!("invalid: {}", invalid.len());

    let (mut some_cnt, mut none_cnt, mut empty_mp, mut total_components) =
        (0usize, 0usize, 0usize, 0usize);
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut sp_time = std::time::Duration::ZERO;
    let mut val_time = std::time::Duration::ZERO;
    for p in &invalid {
        let t0 = std::time::Instant::now();
        let sp = single_pass_fix(p);
        sp_time += t0.elapsed();
        match sp {
            Some(mp) => {
                some_cnt += 1;
                total_components += mp.0.len();
                if mp.0.is_empty() {
                    empty_mp += 1;
                }
                // Simulate the acceptance gate: enforce OGC winding
                // (shoelace, reverse CW shells / CCW holes) then our
                // validator (is_valid_with_geo is public).
                let geom = if mp.0.len() == 1 {
                    geo::Geometry::Polygon(mp.0.into_iter().next().unwrap())
                } else {
                    geo::Geometry::MultiPolygon(mp)
                };
                let t1 = std::time::Instant::now();
                let g = enforce_winding_inline(geom);
                let ok = geo_repair::make_valid::is_valid_with_geo(&g);
                val_time += t1.elapsed();
                if ok {
                    accepted += 1;
                } else {
                    rejected += 1;
                }
            }
            None => none_cnt += 1,
        }
    }
    println!(
        "single_pass: Some={} None={} empty_mp={} components={} ACCEPTED={} REJECTED={}",
        some_cnt, none_cnt, empty_mp, total_components, accepted, rejected
    );
    println!(
        "sp_time={:.2}s val_time={:.2}s",
        sp_time.as_secs_f64(),
        val_time.as_secs_f64()
    );

    // Stage timing on the BIGGEST invalid poly (the boolean path's worst case).
    let biggest = invalid
        .iter()
        .max_by_key(|p| {
            p.exterior().0.len() + p.interiors().iter().map(|h| h.0.len()).sum::<usize>()
        })
        .unwrap();
    let n_holes = biggest.interiors().len();
    let n_edges = biggest.exterior().0.len() + biggest.interiors().iter().map(|h| h.0.len()).sum::<usize>();
    println!("\nbiggest: {} verts total, {} holes", n_edges, n_holes);
    let t0 = std::time::Instant::now();
    let shell_edges = geo_repair::structure::fix_ring::edges_from_coords(&biggest.exterior().0);
    let t1 = std::time::Instant::now();
    let noded = geo_repair::structure::fix_ring::split_edges(&shell_edges);
    let t2 = std::time::Instant::now();
    let _ba = geo_repair::structure::build_area::build_area(&noded);
    let t3 = std::time::Instant::now();
    let _rr = geo_repair::structure::fix_ring::repair_ring(biggest.exterior());
    let t4 = std::time::Instant::now();
    println!(
        "edges_from_coords: {:.1}ms  split_edges({}): {:.1}ms  build_area: {:.1}ms  repair_ring: {:.1}ms",
        (t1 - t0).as_secs_f64() * 1e3,
        shell_edges.len(),
        (t2 - t1).as_secs_f64() * 1e3,
        (t3 - t2).as_secs_f64() * 1e3,
        (t4 - t3).as_secs_f64() * 1e3,
    );
}

/// OGC winding enforcement for the diag (mirrors make_valid::enforce_ogc_winding).
fn enforce_winding_inline(g: geo::Geometry<f64>) -> geo::Geometry<f64> {
    fn shoelace(ring: &[geo::Coord<f64>]) -> f64 {
        let mut s = 0.0;
        for w in ring.windows(2) {
            s += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        s
    }
    match g {
        geo::Geometry::Polygon(p) => {
            let (ext, mut holes) = p.into_inner();
            let ext = if shoelace(&ext.0) > 0.0 {
                ext
            } else {
                let mut c: Vec<_> = ext.0.iter().copied().rev().collect();
                if c.first() != c.last() {
                    c.push(c[0]);
                }
                geo::LineString::new(c)
            };
            for h in holes.iter_mut() {
                if shoelace(&h.0) > 0.0 {
                    let mut c: Vec<_> = h.0.iter().copied().rev().collect();
                    if c.first() != c.last() {
                        c.push(c[0]);
                    }
                    *h = geo::LineString::new(c);
                }
            }
            geo::Geometry::Polygon(geo::Polygon::new(ext, holes))
        }
        other => other,
    }
}

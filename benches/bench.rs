//! Sweep structure parallel vs GEOS across all geometry shapes.
//! Usage: cargo bench --bench bench (no GEOS — serial + parallel columns)
//!        cargo bench --features bench-geos --bench bench (GEOS from source)
//!        cargo bench --features bench-geos-system --bench bench (system GEOS)
use std::time::Instant;

use geo::{Coord, Geometry, Line, LineString, MultiLineString, MultiPolygon, Polygon};
#[cfg(feature = "parallel")]
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
use geos::Geom;
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
use rayon::prelude::*;

/// Convert a geo Geometry to a GEOS Geometry via CoordSeq direct construction
/// (no WKT round-trip).
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn geometry_to_geos(geom: &Geometry<f64>) -> Option<geos::Geometry> {
    use geos::{CoordSeq, CoordType};
    match geom {
        Geometry::Point(p) => {
            let mut cs = CoordSeq::new(1, CoordType::XY).ok()?;
            cs.set_x(0, p.0.x).ok()?;
            cs.set_y(0, p.0.y).ok()?;
            geos::Geometry::create_point(cs).ok()
        }
        Geometry::Line(l) => {
            let mut cs = CoordSeq::new(2, CoordType::XY).ok()?;
            cs.set_x(0, l.start.x).ok()?;
            cs.set_y(0, l.start.y).ok()?;
            cs.set_x(1, l.end.x).ok()?;
            cs.set_y(1, l.end.y).ok()?;
            geos::Geometry::create_line_string(cs).ok()
        }
        Geometry::LineString(ls) => {
            let n = ls.0.len() as u32;
            let mut cs = CoordSeq::new(n, CoordType::XY).ok()?;
            for (i, c) in ls.0.iter().enumerate() {
                cs.set_x(i, c.x).ok()?;
                cs.set_y(i, c.y).ok()?;
            }
            geos::Geometry::create_line_string(cs).ok()
        }
        Geometry::Polygon(p) => polygon_to_geos(p),
        Geometry::MultiPoint(mp) => {
            let geoms: Vec<_> =
                mp.0.iter()
                    .filter_map(|p| geometry_to_geos(&Geometry::Point(*p)))
                    .collect();
            if geoms.is_empty() {
                None
            } else {
                geos::Geometry::create_multipoint(geoms).ok()
            }
        }
        Geometry::MultiLineString(mls) => {
            let geoms: Vec<_> = mls
                .0
                .iter()
                .filter_map(|ls| geometry_to_geos(&Geometry::LineString(ls.clone())))
                .collect();
            if geoms.is_empty() {
                None
            } else {
                geos::Geometry::create_multiline_string(geoms).ok()
            }
        }
        Geometry::MultiPolygon(mp) => {
            let geoms: Vec<_> = mp.0.iter().filter_map(|p| polygon_to_geos(p)).collect();
            if geoms.is_empty() {
                None
            } else {
                geos::Geometry::create_multipolygon(geoms).ok()
            }
        }
        _ => None,
    }
}

/// Convert a geo Polygon to a GEOS Polygon via CoordSeq direct construction.
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn polygon_to_geos(poly: &Polygon<f64>) -> Option<geos::Geometry> {
    use geos::{CoordSeq, CoordType};
    fn coords_to_ring(coords: &[Coord<f64>]) -> Option<geos::Geometry> {
        let n = coords.len() as u32;
        let mut cs = CoordSeq::new(n, CoordType::XY).ok()?;
        for (i, c) in coords.iter().enumerate() {
            cs.set_x(i, c.x).ok()?;
            cs.set_y(i, c.y).ok()?;
        }
        geos::Geometry::create_linear_ring(cs).ok()
    }
    let ring = coords_to_ring(&poly.exterior().0)?;
    let holes: Vec<geos::Geometry> = poly
        .interiors()
        .iter()
        .filter_map(|h| coords_to_ring(&h.0))
        .collect();
    geos::Geometry::create_polygon(ring, holes).ok()
}

#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
fn run_geos_batch(geoms: &[geos::Geometry]) -> f64 {
    let t0 = Instant::now();
    geoms.par_iter().for_each(|gg| {
        let _ = gg.make_valid();
    });
    t0.elapsed().as_secs_f64()
}

fn make_valid_ring(n: usize, r: f64) -> Polygon<f64> {
    let mut coords = Vec::with_capacity(n);
    for i in 0..n - 1 {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
        coords.push(Coord {
            x: r * angle.cos() + 500.0,
            y: r * angle.sin() + 500.0,
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn make_bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
        ]),
        Vec::new(),
    )
}

fn run_ser(polys: &[Polygon<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    for p in polys {
        let _ = p.make_valid_with_config(cfg);
    }
    t0.elapsed().as_secs_f64()
}

#[cfg(feature = "parallel")]
fn run_par(polys: &[&Polygon<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    let _ = par_fix_polygon_batch(polys, cfg);
    t0.elapsed().as_secs_f64()
}

#[cfg(not(feature = "parallel"))]
fn run_par(_polys: &[&Polygon<f64>], _cfg: &MakeValidConfig) -> f64 {
    0.0
}

fn run_line_ser(items: &[Geometry<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    for g in items {
        let _ = g.make_valid_with_config(cfg);
    }
    t0.elapsed().as_secs_f64()
}

#[cfg(feature = "parallel")]
fn run_line_par(items: &[Geometry<f64>], cfg: &MakeValidConfig) -> f64 {
    use rayon::prelude::*;
    let t0 = Instant::now();
    let _: Vec<_> = items
        .par_iter()
        .map(|g| g.make_valid_with_config(cfg))
        .collect();
    t0.elapsed().as_secs_f64()
}

#[cfg(not(feature = "parallel"))]
fn run_line_par(_items: &[Geometry<f64>], _cfg: &MakeValidConfig) -> f64 {
    0.0
}

fn make_line(coords: [(f64, f64); 2]) -> Geometry<f64> {
    Geometry::Line(Line::new(
        Coord {
            x: coords[0].0,
            y: coords[0].1,
        },
        Coord {
            x: coords[1].0,
            y: coords[1].1,
        },
    ))
}

fn make_linestring(coords: &[(f64, f64)]) -> Geometry<f64> {
    let ls: Vec<Coord<f64>> = coords.iter().map(|&(x, y)| Coord { x, y }).collect();
    Geometry::LineString(LineString::new(ls))
}

fn make_multilinestring(parts: &[Vec<(f64, f64)>]) -> Geometry<f64> {
    let lines: Vec<LineString<f64>> = parts
        .iter()
        .map(|coords| {
            let cs: Vec<Coord<f64>> = coords.iter().map(|&(x, y)| Coord { x, y }).collect();
            LineString::new(cs)
        })
        .collect();
    Geometry::MultiLineString(MultiLineString::new(lines))
}

// ─── Specialized shape generators ─────────────────────────────────

fn make_starburst(spikes: usize, r: f64) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(spikes * 2 + 1);
    for i in 0..spikes {
        let a = 2.0 * std::f64::consts::PI * i as f64 / spikes as f64;
        coords.push((0.0, 0.0));
        coords.push((r * a.cos(), r * a.sin()));
    }
    coords.push((0.0, 0.0));
    make_linestring(&coords)
}

fn make_collinear_overlap(segments: usize) -> Geometry<f64> {
    let mut coords = Vec::new();
    for i in 0..segments {
        let x = i as f64 * 10.0;
        coords.push((x, 0.0));
        // overlap back by 5 units
        coords.push((x + 10.0, 0.0));
        if i < segments - 1 {
            coords.push((x + 5.0, 0.0));
        }
    }
    make_linestring(&coords)
}

fn make_extreme_mixed_scale(n: usize) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let x = if i % 2 == 0 { 1e12 } else { 1e-12 };
        let y = if i % 3 == 0 { -1e12 } else { 1e-12 };
        coords.push((x + i as f64, y + i as f64));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn make_tight_ringing(n: usize, amplitude: f64) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let x = i as f64 * 0.1;
        let y = amplitude * ((i as f64 * 3.0).sin() + (i as f64 * 5.0).sin());
        coords.push((x, y));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn hilbert_coords(order: u32) -> Vec<(f64, f64)> {
    fn d2xy(n: u32, d: u32) -> (f64, f64) {
        let mut x = 0i32;
        let mut y = 0i32;
        let mut t = d;
        let mut s = 1i32;
        while s < n as i32 {
            let rx = (t >> 1) & s as u32;
            let ry = (t ^ rx) & s as u32;
            if ry == 0 {
                if rx != 0 {
                    x = s - 1 - x;
                    y = s - 1 - y;
                }
                std::mem::swap(&mut x, &mut y);
            }
            x += rx as i32;
            y += ry as i32;
            t >>= 2;
            s <<= 1;
        }
        (x as f64 * 10.0, y as f64 * 10.0)
    }
    let n = 1u32 << order;
    let total = n * n;
    let mut coords = Vec::with_capacity(total as usize + 1);
    for i in 0..total {
        coords.push(d2xy(n, i));
    }
    coords.push(coords[0]);
    coords
}

fn make_lissajous(n: usize, a: f64, b: f64, scale: f64) -> Geometry<f64> {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        coords.push((scale * (a * t).sin(), scale * (b * t).sin()));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn make_spoke_wheel(spokes: usize, r: f64) -> Geometry<f64> {
    let mut coords = Vec::new();
    for i in 0..spokes {
        let a = 2.0 * std::f64::consts::PI * i as f64 / spokes as f64;
        coords.push((r * a.cos(), r * a.sin()));
        coords.push((0.0, 0.0));
    }
    coords.push((r, 0.0));
    make_linestring(&coords)
}

fn make_star_comb(spikes: usize) -> Geometry<f64> {
    // Boost Geometry classic worst-case: alternating long/short radii
    // Non-adjacent spikes have near-intersections — NO shared endpoints
    // (Unlike star-burst where all edges share origin)
    let mut coords = Vec::with_capacity(spikes + 1);
    for i in 0..spikes {
        let a = 2.0 * std::f64::consts::PI * i as f64 / spikes as f64;
        let r = if i % 2 == 0 { 1000.0 } else { 50.0 };
        coords.push((r * a.cos(), r * a.sin()));
    }
    coords.push(coords[0]);
    make_linestring(&coords)
}

fn make_self_touching_polygon() -> Polygon<f64> {
    // Shell that touches itself at (150,0), forming a hole ("banana polygon")
    Polygon::new(
        LineString::new(vec![
            Coord { x: 100.0, y: 0.0 },
            Coord { x: 100.0, y: 100.0 },
            Coord { x: 200.0, y: 100.0 },
            Coord { x: 200.0, y: 0.0 },
            Coord { x: 150.0, y: 0.0 },
            Coord { x: 170.0, y: 40.0 },
            Coord { x: 130.0, y: 40.0 },
            Coord { x: 150.0, y: 0.0 },
            Coord { x: 100.0, y: 0.0 },
        ]),
        Vec::new(),
    )
}

fn make_nearly_collinear_polygon() -> Polygon<f64> {
    // Shewchuk classic stress case: nearly-collinear vertices
    // small perturbation from collinear stresses orient2d adaptive precision
    Polygon::new(
        LineString::new(vec![
            Coord { x: -0.01, y: -0.59 },
            Coord { x: 0.01, y: 0.57 },
            Coord { x: 5000.0, y: 0.0 }, // long edge to create large bounding box
            Coord { x: 0.0, y: -0.01 },
            Coord { x: -0.01, y: -0.59 },
        ]),
        Vec::new(),
    )
}

fn make_collapsed_polygon() -> Polygon<f64> {
    // Shell with a backtrack edge — partial zero area, tests collapsed output
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 }, // backtrack — zero-area spike
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )
}

fn make_dense_overlap_grid(per_side: usize) -> Geometry<f64> {
    // Grid of small overlapping squares as MultiPolygon
    // Each square overlaps neighbors by ~30% — tests batch overlap performance
    let size = 10.0;
    let stride = size * 0.7;
    let polys: Vec<Polygon<f64>> = (0..per_side)
        .flat_map(|i| {
            (0..per_side).map(move |j| {
                let x = i as f64 * stride;
                let y = j as f64 * stride;
                Polygon::new(
                    LineString::new(vec![
                        Coord { x, y },
                        Coord { x: x + size, y },
                        Coord {
                            x: x + size,
                            y: y + size,
                        },
                        Coord { x, y: y + size },
                        Coord { x, y },
                    ]),
                    Vec::new(),
                )
            })
        })
        .collect();
    Geometry::MultiPolygon(MultiPolygon::new(polys))
}

fn make_large_coord_polygon() -> Polygon<f64> {
    // Polygon with coordinates at ±1e12 — tests numerical stability
    Polygon::new(
        LineString::new(vec![
            Coord { x: -1e12, y: -1e12 },
            Coord { x: 1e12, y: -1e12 },
            Coord { x: 1e12, y: 1e12 },
            Coord { x: -1e12, y: 1e12 },
            Coord { x: -1e12, y: -1e12 },
        ]),
        Vec::new(),
    )
}

fn make_multipoly_overlap(n: usize, size: f64) -> Geometry<f64> {
    let polys: Vec<Polygon<f64>> = (0..n)
        .map(|i| {
            let off = i as f64 * size * 0.3;
            Polygon::new(
                LineString::new(vec![
                    Coord { x: off, y: off },
                    Coord {
                        x: off + size,
                        y: off,
                    },
                    Coord {
                        x: off + size,
                        y: off + size,
                    },
                    Coord {
                        x: off,
                        y: off + size,
                    },
                    Coord { x: off, y: off },
                ]),
                Vec::new(),
            )
        })
        .collect();
    Geometry::MultiPolygon(MultiPolygon::new(polys))
}

fn make_hole_hierarchy(n_holes: usize, size: f64) -> Polygon<f64> {
    let shell = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: size, y: 0.0 },
        Coord { x: size, y: size },
        Coord { x: 0.0, y: size },
        Coord { x: 0.0, y: 0.0 },
    ]);
    let grid = (n_holes as f64).sqrt().ceil() as usize;
    let cell = size / (grid + 1) as f64;
    let mut holes = Vec::new();
    let mut idx = 0usize;
    for gi in 0..grid {
        for gj in 0..grid {
            if idx >= n_holes {
                break;
            }
            let cx = (gi + 1) as f64 * cell;
            let cy = (gj + 1) as f64 * cell;
            let hs = cell * 0.3;
            holes.push(LineString::new(vec![
                Coord {
                    x: cx - hs,
                    y: cy - hs,
                },
                Coord {
                    x: cx + hs,
                    y: cy - hs,
                },
                Coord {
                    x: cx + hs,
                    y: cy + hs,
                },
                Coord {
                    x: cx - hs,
                    y: cy + hs,
                },
                Coord {
                    x: cx - hs,
                    y: cy - hs,
                },
            ]));
            idx += 1;
        }
        if idx >= n_holes {
            break;
        }
    }
    Polygon::new(shell, holes)
}

fn make_sliver_polygon(segments: usize, gap: f64) -> Polygon<f64> {
    let n = segments;
    let mut coords = Vec::with_capacity(n * 2 + 1);
    for i in 0..n {
        let x = i as f64 * 10.0;
        let y = (i as f64 * 0.3).sin() * gap;
        coords.push(Coord { x, y });
    }
    for i in (0..n).rev() {
        let x = i as f64 * 10.0;
        let y = (i as f64 * 0.3).sin() * gap - gap;
        coords.push(Coord { x, y });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn bench_line(label: &str, g: &Geometry<f64>, batch: usize, cfg: &MakeValidConfig) {
    let items: Vec<Geometry<f64>> = (0..batch).map(|_| g.clone()).collect();
    let par = run_line_par(&items, cfg);
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        // CoordSeq direct construction — no WKT overhead
        let geos_geoms: Vec<geos::Geometry> = items.iter().filter_map(geometry_to_geos).collect();
        let geos = run_geos_batch(&geos_geoms);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            par * 1_000_000.0 / batch as f64,
            geos * 1_000_000.0 / batch as f64,
        );
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        let ser = run_line_ser(&items, cfg);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            ser * 1_000_000.0 / batch as f64,
            par * 1_000_000.0 / batch as f64,
        );
    }
}

fn bench_polygons(label: &str, polys: &[Polygon<f64>], batch: usize, cfg: &MakeValidConfig) {
    let refs: Vec<&Polygon<f64>> = polys.iter().collect();
    let par = run_par(&refs, cfg);
    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    {
        // CoordSeq direct construction — no WKT overhead
        let geos_geoms: Vec<geos::Geometry> = polys.iter().filter_map(polygon_to_geos).collect();
        let geos = run_geos_batch(&geos_geoms);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            par * 1_000_000.0 / batch as f64,
            geos * 1_000_000.0 / batch as f64,
        );
    }
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    {
        let ser = run_ser(polys, cfg);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            ser * 1_000_000.0 / batch as f64,
            par * 1_000_000.0 / batch as f64,
        );
    }
}

fn main() {
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };

    // Warmup
    let poly4 = make_valid_ring(4, 100.0);
    let mut warm: Vec<Polygon<f64>> = Vec::new();
    for _ in 0..10000 {
        warm.push(poly4.clone());
    }
    let wrefs: Vec<&Polygon<f64>> = warm.iter().collect();
    run_ser(&warm, &cfg);
    run_par(&wrefs, &cfg);

    #[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
    let header = ("geo-repair", "geos");
    #[cfg(not(any(feature = "bench-geos", feature = "bench-geos-system")))]
    let header = ("serial", "parallel");

    eprintln!(
        "  {:<20} {:>10} {:>10} {:>10}",
        "test", header.0, header.1, "unit"
    );
    eprintln!("{}", "-".repeat(55));

    // Valid polygons: small batch (50K) for cheap tests, 1000 for expensive
    for &(n, batch) in &[
        (4, 50000usize),
        (10, 50000),
        (50, 10000),
        (100, 5000),
        (500, 1000),
        (1000, 1000),
        (5000, 1000),
        (10000, 1000),
    ] {
        let poly = make_valid_ring(n, 100.0);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("valid polygon {:>5}v", n), &polys, batch, &cfg);
    }

    // Invalid bowtie 4v
    {
        let poly = make_bowtie();
        let polys: Vec<Polygon<f64>> = (0..50000)
            .map(|i| {
                let mut p = poly.clone();
                p.exterior_mut(|ext| {
                    for c in &mut ext.0 {
                        c.x += i as f64 * 20.0;
                        c.y += i as f64 * 20.0;
                    }
                });
                p
            })
            .collect();
        bench_polygons("invalid bowtie 4v", &polys, 50000, &cfg);
    }

    // Large invalid star 100v
    {
        let mut coords = Vec::with_capacity(100);
        for i in 0..99 {
            let angle = 2.0 * std::f64::consts::PI * i as f64 / 99.0;
            let r = if i % 3 == 0 { 100.0 } else { 50.0 };
            coords.push(Coord {
                x: r * angle.cos(),
                y: r * angle.sin(),
            });
        }
        coords.push(coords[0]);
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        let polys: Vec<Polygon<f64>> = (0..1000)
            .map(|i| {
                let mut p = poly.clone();
                p.exterior_mut(|ext| {
                    for c in &mut ext.0 {
                        c.x += i as f64 * 300.0;
                        c.y += i as f64 * 300.0;
                    }
                });
                p
            })
            .collect();
        bench_polygons("invalid star 100v", &polys, 1000, &cfg);
    }

    // Self-touching (banana) polygon — tests self-touch forming hole
    {
        let poly = make_self_touching_polygon();
        let polys: Vec<Polygon<f64>> = (0..50000).map(|_| poly.clone()).collect();
        bench_polygons("self-touch poly", &polys, 50000, &cfg);
    }

    // Collapsed polygon (zero-area spike) — tests collapsed output handling
    {
        let poly = make_collapsed_polygon();
        let polys: Vec<Polygon<f64>> = (0..50000).map(|_| poly.clone()).collect();
        bench_polygons("collapsed poly", &polys, 50000, &cfg);
    }

    // Nearly-collinear polygon — Shewchuk orient2d stress case
    {
        let poly = make_nearly_collinear_polygon();
        let polys: Vec<Polygon<f64>> = (0..50000).map(|_| poly.clone()).collect();
        bench_polygons("near-collinear", &polys, 50000, &cfg);
    }

    // Large coordinate polygon (±1e12) — tests numerical stability vs GEOS
    {
        let poly = make_large_coord_polygon();
        let polys: Vec<Polygon<f64>> = (0..5000).map(|_| poly.clone()).collect();
        bench_polygons("large coord 1e12", &polys, 5000, &cfg);
    }

    // ─── Line benchmarks ─────────────────────────────────────────────
    eprintln!("{}", "-".repeat(55));

    // Single Line tests
    bench_line(
        "valid line",
        &make_line([(0.0, 0.0), (1000.0, 500.0)]),
        100000,
        &cfg,
    );
    bench_line(
        "zero-length line",
        &make_line([(5.0, 5.0), (5.0, 5.0)]),
        100000,
        &cfg,
    );

    // Valid LineString vertex sweep
    for &(n, batch) in &[
        (4, 50000usize),
        (10, 50000),
        (50, 10000),
        (100, 5000),
        (500, 1000),
    ] {
        let coords: Vec<(f64, f64)> = (0..n).map(|i| (i as f64, (i as f64).sin())).collect();
        let g = make_linestring(&coords);
        bench_line(&format!("valid ls {:>4}v", n), &g, batch, &cfg);
    }

    // Collinear LineString vertex sweep
    for &(n, batch) in &[
        (4, 50000usize),
        (10, 50000),
        (50, 10000),
        (100, 5000),
        (500, 1000),
    ] {
        let coords: Vec<(f64, f64)> = (0..n).map(|i| (i as f64, 0.0)).collect();
        let g = make_linestring(&coords);
        bench_line(&format!("collinear ls {:>4}v", n), &g, batch, &cfg);
    }

    // Convoluted: zigzag (alternating y)
    for &(n, batch) in &[(10, 50000usize), (50, 10000), (100, 5000), (500, 1000)] {
        let coords: Vec<(f64, f64)> = (0..n)
            .map(|i| (i as f64, if i % 2 == 0 { 0.0 } else { 1000.0 }))
            .collect();
        let g = make_linestring(&coords);
        bench_line(&format!("zigzag ls {:>4}v", n), &g, batch, &cfg);
    }

    // Convoluted: spiral (tightly wound)
    for &(n, batch) in &[(10, 50000usize), (50, 10000), (100, 5000)] {
        let mut coords = Vec::new();
        for i in 0..n {
            let t = i as f64 * 0.5;
            let r = 100.0 + t * 2.0;
            coords.push((r * t.cos(), r * t.sin()));
        }
        let g = make_linestring(&coords);
        bench_line(&format!("spiral ls {:>4}v", n), &g, batch, &cfg);
    }

    // Self-intersecting: figure-8
    {
        let coords = vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ];
        let g = make_linestring(&coords);
        bench_line("self-int ls 5v", &g, 50000, &cfg);
    }

    // Self-intersecting: many crossing edges (dense bowtie chain)
    for &(n, batch) in &[(10, 50000usize), (50, 10000), (100, 5000)] {
        let mut coords = Vec::new();
        for i in 0..n {
            let x = i as f64 * 10.0;
            let y = if i % 2 == 0 { 0.0 } else { 1000.0 } + (i as f64).sin() * 50.0;
            coords.push((x, y));
        }
        let g = make_linestring(&coords);
        bench_line(&format!("dense self ls {:>4}v", n), &g, batch, &cfg);
    }

    // LineString with consecutive duplicates
    {
        let mut coords = Vec::new();
        for i in 0..50 {
            coords.push((i as f64, 0.0));
            coords.push((i as f64, 0.0));
        }
        let g = make_linestring(&coords);
        bench_line("duped ls 100v", &g, 50000, &cfg);
    }

    // MultiLineString: many short parts
    {
        let parts: Vec<Vec<(f64, f64)>> = (0..50)
            .map(|i| {
                let base = i as f64 * 200.0;
                vec![(base, 0.0), (base + 100.0, 50.0), (base + 200.0, 0.0)]
            })
            .collect();
        let g = make_multilinestring(&parts);
        bench_line("mls 50x3v", &g, 10000, &cfg);
    }

    // MultiLineString with many self-intersecting components
    {
        let parts: Vec<Vec<(f64, f64)>> = (0..50)
            .map(|i| {
                let base = i as f64 * 30.0;
                vec![
                    (base, 0.0),
                    (base + 10.0, 10.0),
                    (base + 10.0, 0.0),
                    (base, 10.0),
                ]
            })
            .collect();
        let g = make_multilinestring(&parts);
        bench_line("self-int mls 50x4v", &g, 10000, &cfg);
    }

    // ─── Special shapes ────────────────────────────────────────────
    eprintln!("{}", "-".repeat(55));

    // Star-burst: all edges from/to center — stresses duplicate-vertex detection
    for &(spikes, batch) in &[(10usize, 50000usize), (50, 10000), (100, 5000), (500, 100)] {
        let g = make_starburst(spikes, 1000.0);
        bench_line(&format!("star-burst {}sp", spikes), &g, batch, &cfg);
    }

    // Collinear overlap: segments on same line with partial overlap (regression test)
    for &(segments, batch) in &[(10usize, 50000usize), (50, 10000), (100, 5000), (500, 500)] {
        let g = make_collinear_overlap(segments);
        bench_line(&format!("collinear ov {}seg", segments), &g, batch, &cfg);
    }

    // Extreme mixed scale: alternates 1e12 and 1e-12 coords — tests epsilon robustness
    for &(n, batch) in &[(10usize, 50000usize), (50, 1000), (100, 100)] {
        let g = make_extreme_mixed_scale(n);
        bench_line(&format!("x-scale {}v", n), &g, batch, &cfg);
    }

    // Tight ringing: dense near-miss oscillations — stresses orient2d near-boundary
    for &(n, batch) in &[(100usize, 10000usize), (500, 50)] {
        let g = make_tight_ringing(n, 1e-8);
        bench_line(&format!("ringing {}v", n), &g, batch, &cfg);
    }

    // Hilbert curve (order 4 = 256v, order 5 = 1024v) — space-filling, grid stress
    {
        let g = make_linestring(&hilbert_coords(4));
        bench_line("hilbert 256v", &g, 2000, &cfg);
    }
    {
        let g = make_linestring(&hilbert_coords(5));
        bench_line("hilbert 1024v", &g, 200, &cfg);
    }

    // Lissajous curve: complex self-intersection pattern (5:3 ratio)
    for &(n, batch) in &[(200usize, 5000usize), (500, 1000), (1000, 100)] {
        let g = make_lissajous(n, 5.0, 3.0, 1000.0);
        bench_line(&format!("lissajous {}v", n), &g, batch, &cfg);
    }
    // Lissajous 7:4 ratio (different crossing pattern)
    {
        let g = make_lissajous(500, 7.0, 4.0, 1000.0);
        bench_line("lissajous 7:4 500v", &g, 1000, &cfg);
    }

    // Spoke wheel: all edges converge at origin — stresses noding at common point
    for &(spokes, batch) in &[(10usize, 50000usize), (50, 5000), (100, 500), (500, 50)] {
        let g = make_spoke_wheel(spokes, 1000.0);
        bench_line(&format!("spoke {}sp", spokes), &g, batch, &cfg);
    }

    // Star comb: alternating long/short spikes — NO shared endpoints (differs from star-burst)
    for &(spikes, batch) in &[(20usize, 50000usize), (100, 5000), (500, 100)] {
        let g = make_star_comb(spikes);
        bench_line(&format!("star-comb {}sp", spikes), &g, batch, &cfg);
    }

    // ─── MultiPolygon / hole hierarchy / sliver ────────────────────
    eprintln!("{}", "-".repeat(55));

    // Hole hierarchy: shell with many nested holes
    for &(nh, batch) in &[(5usize, 10000usize), (20, 1000), (50, 200)] {
        let poly = make_hole_hierarchy(nh, 500.0);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("hole hier {}h", nh), &polys, batch, &cfg);
    }

    // MultiPolygon with overlapping shells
    for &(ns, batch) in &[(5usize, 1000usize), (20, 200), (50, 50)] {
        let g = make_multipoly_overlap(ns, 100.0);
        bench_line(&format!("overlap mp {}sh", ns), &g, batch, &cfg);
    }

    // Dense grid of overlapping small polygons
    {
        let g = make_dense_overlap_grid(5);
        bench_line("dense grid 5x5=25", &g, 1000, &cfg);
    }
    {
        let g = make_dense_overlap_grid(10);
        bench_line("dense grid 10x10=100", &g, 200, &cfg);
    }
    {
        let g = make_dense_overlap_grid(20);
        bench_line("dense grid 20x20=400", &g, 50, &cfg);
    }

    // Sliver edges: near-collinear, very thin polygon
    for &(n, batch) in &[(100usize, 1000usize), (500, 100)] {
        let poly = make_sliver_polygon(n, 0.001);
        let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
        bench_polygons(&format!("sliver {}v", n), &polys, batch, &cfg);
    }

    // ─── Arrange pipeline (CDT fallback) ───────────────────────────
    #[cfg(feature = "arrange")]
    {
        eprintln!("{}", "-".repeat(55));
        let acfg = MakeValidConfig {
            poly_method: PolyMethod::Arrange,
            ..Default::default()
        };

        for &(n, batch) in &[(4usize, 10000usize), (10, 5000), (50, 1000)] {
            let poly = make_valid_ring(n, 100.0);
            let polys: Vec<Polygon<f64>> = (0..batch).map(|_| poly.clone()).collect();
            bench_polygons(&format!("arrange valid {}v", n), &polys, batch, &acfg);
        }

        {
            let poly = make_bowtie();
            let polys: Vec<Polygon<f64>> = (0..5000)
                .map(|i| {
                    let mut p = poly.clone();
                    p.exterior_mut(|ext| {
                        for c in &mut ext.0 {
                            c.x += i as f64 * 20.0;
                            c.y += i as f64 * 20.0;
                        }
                    });
                    p
                })
                .collect();
            bench_polygons("arrange bowtie 4v", &polys, 5000, &acfg);
        }

        // Star polygon through Arrange (challenging for CDT)
        for &(spikes, batch) in &[(10usize, 5000usize), (50, 500)] {
            let g = make_starburst(spikes, 1000.0);
            bench_line(&format!("arrange star {}sp", spikes), &g, batch, &acfg);
        }
    }
}

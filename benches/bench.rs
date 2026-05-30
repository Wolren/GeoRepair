//! Sweep structure parallel vs GEOS across all geometry shapes.
//! Usage: cargo bench --bench quick_bench (no GEOS)
//!        cargo bench --features bench-geos --bench quick_bench (with GEOS)
use std::time::Instant;

use geo::{Coord, Geometry, Line, LineString, MultiLineString, Polygon};
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

#[cfg(feature = "bench-geos")]
use geos::Geom;
#[cfg(feature = "bench-geos")]
use wkt::ToWkt;

#[cfg(feature = "bench-geos")]
fn run_geos_batch(wkts: &[String]) -> f64 {
    let t0 = Instant::now();
    for wkt in wkts {
        if let Ok(gg) = geos::Geometry::new_from_wkt(wkt) {
            let _ = gg.make_valid();
        }
    }
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

fn run_par(polys: &[&Polygon<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    let _ = par_fix_polygon_batch(polys, cfg);
    t0.elapsed().as_secs_f64()
}

fn run_line_ser(items: &[Geometry<f64>], cfg: &MakeValidConfig) -> f64 {
    let t0 = Instant::now();
    for g in items {
        let _ = g.make_valid_with_config(cfg);
    }
    t0.elapsed().as_secs_f64()
}

fn run_line_par(items: &[Geometry<f64>], cfg: &MakeValidConfig) -> f64 {
    use rayon::prelude::*;
    let t0 = Instant::now();
    let _: Vec<_> = items
        .par_iter()
        .map(|g| g.make_valid_with_config(cfg))
        .collect();
    t0.elapsed().as_secs_f64()
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

fn bench_line(label: &str, g: &Geometry<f64>, batch: usize, cfg: &MakeValidConfig) {
    let items: Vec<Geometry<f64>> = (0..batch).map(|_| g.clone()).collect();
    let par = run_line_par(&items, cfg);
    #[cfg(feature = "bench-geos")]
    {
        let wkts: Vec<String> = items.iter().map(|g| g.to_wkt().to_string()).collect();
        let geos = run_geos_batch(&wkts);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            par * 1_000_000.0 / batch as f64,
            geos * 1_000_000.0 / batch as f64,
        );
    }
    #[cfg(not(feature = "bench-geos"))]
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
    #[cfg(feature = "bench-geos")]
    {
        let wkts: Vec<String> = polys.iter().map(|p| p.to_wkt().to_string()).collect();
        let geos = run_geos_batch(&wkts);
        eprintln!(
            "  {:<20} {:>10.3} {:>10.3} µs",
            label,
            par * 1_000_000.0 / batch as f64,
            geos * 1_000_000.0 / batch as f64,
        );
    }
    #[cfg(not(feature = "bench-geos"))]
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

    #[cfg(feature = "bench-geos")]
    let header = ("parallel", "geos");
    #[cfg(not(feature = "bench-geos"))]
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
}

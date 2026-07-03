//! Cross-comparison benchmarks: geo-repair vs GEOS vs CGAL.
//!
//! Run with GEOS comparison:
//!   cargo bench --features bench-geos
//!
//! Future: Add CGAL comparison when CGAL C++ executables are compiled.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use geo::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
    Triangle,
};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
#[cfg(feature = "bench-geos")]
use geos::Geom;
#[cfg(feature = "bench-geos")]
use wkt::ToWkt;

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

fn make_valid_square() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
        ]),
        Vec::new(),
    )
}

fn make_large_polygon(n: usize, r: f64) -> Polygon<f64> {
    let mut seed = 0xDEADBEEF_DEADBEEFu64;
    let mut coords = Vec::with_capacity(n);
    for i in 0..n {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let dt = (seed as f64 / u64::MAX as f64 - 0.5) * (2.0 * std::f64::consts::PI / n as f64);
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let dr = (seed as f64 / u64::MAX as f64 - 0.5) * r * 0.35;
        let rr = r + dr;
        let aa = angle + dt;
        coords.push(Coord {
            x: rr * aa.cos(),
            y: rr * aa.sin(),
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn make_complex_bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 3.0, y: 3.0 },
            Coord { x: 2.0, y: 5.0 },
            Coord { x: 5.0, y: 2.0 },
            Coord { x: 7.0, y: 4.0 },
            Coord { x: 4.0, y: 7.0 },
            Coord { x: 1.0, y: 9.0 },
            Coord { x: 8.0, y: 1.0 },
            Coord { x: 6.0, y: 8.0 },
            Coord { x: 9.0, y: 5.0 },
        ]),
        Vec::new(),
    )
}

fn make_overlapping_polygons() -> MultiPolygon<f64> {
    MultiPolygon::new(vec![
        Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
            ]),
            Vec::new(),
        ),
        Polygon::new(
            LineString::new(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 15.0, y: 5.0 },
                Coord { x: 15.0, y: 15.0 },
                Coord { x: 5.0, y: 15.0 },
            ]),
            Vec::new(),
        ),
    ])
}

// =========================================================================
// Orientation benchmark (orient2d)
// =========================================================================

fn bench_orient2d(c: &mut Criterion) {
    let pts: Vec<(f64, f64)> = (0..1000)
        .map(|i| {
            let x = (i as f64 * 7.3).fract();
            let y = (i as f64 * 13.7).fract();
            (x, y)
        })
        .collect();

    let mut group = c.benchmark_group("orient2d");
    group.bench_function("geo_repair", |b| {
        b.iter(|| {
            for i in 0..pts.len() - 2 {
                black_box(geo_repair::orient::orient2d(
                    Coord {
                        x: pts[i].0,
                        y: pts[i].1,
                    },
                    Coord {
                        x: pts[i + 1].0,
                        y: pts[i + 1].1,
                    },
                    Coord {
                        x: pts[i + 2].0,
                        y: pts[i + 2].1,
                    },
                ));
            }
        })
    });

    #[cfg(feature = "bench-geos")]
    group.bench_function("GEOS", |b| {
        let geos_pts: Vec<(f64, f64)> = pts.clone();
        b.iter(|| {
            for i in 0..geos_pts.len() - 2 {
                let result = geos::orientation_index(
                    geos_pts[i].0,
                    geos_pts[i].1,
                    geos_pts[i + 1].0,
                    geos_pts[i + 1].1,
                    geos_pts[i + 2].0,
                    geos_pts[i + 2].1,
                )
                .unwrap();
                black_box(result);
            }
        })
    });
    group.finish();
}

// =========================================================================
// MakeValid benchmarks
// =========================================================================

fn bench_make_valid(c: &mut Criterion) {
    let square = make_valid_square();
    let bowtie = make_bowtie();
    let complex = make_complex_bowtie();
    let large_100 = make_large_polygon(100, 100.0);
    let large_500 = make_large_polygon(500, 100.0);
    let large_2000 = make_large_polygon(2000, 100.0);
    let large_5000 = make_large_polygon(5000, 100.0);
    let large_10000 = make_large_polygon(10000, 100.0);
    let overlapping = make_overlapping_polygons();
    let config_arrange = MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    };
    let config_structure = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };

    #[cfg(feature = "bench-geos")]
    let (
        geos_square,
        geos_bowtie,
        geos_complex,
        geos_large_100,
        geos_large_500,
        geos_large_2000,
        geos_large_5000,
        geos_large_10000,
        geos_overlapping,
    ) = {
        let gs = geos::Geometry::new_from_wkt(&square.wkt_string()).unwrap();
        let gb = geos::Geometry::new_from_wkt(&bowtie.wkt_string()).unwrap();
        let gc = geos::Geometry::new_from_wkt(&complex.wkt_string()).unwrap();
        let gl100 = geos::Geometry::new_from_wkt(&large_100.wkt_string()).unwrap();
        let gl500 = geos::Geometry::new_from_wkt(&large_500.wkt_string()).unwrap();
        let gl2000 = geos::Geometry::new_from_wkt(&large_2000.wkt_string()).unwrap();
        let gl5000 = geos::Geometry::new_from_wkt(&large_5000.wkt_string()).unwrap();
        let gl10000 = geos::Geometry::new_from_wkt(&large_10000.wkt_string()).unwrap();
        let go = geos::Geometry::new_from_wkt(&overlapping.wkt_string()).unwrap();
        (gs, gb, gc, gl100, gl500, gl2000, gl5000, gl10000, go)
    };

    let mut group = c.benchmark_group("make_valid_square");
    group.bench_function("geo_repair_arrange", |b| {
        b.iter(|| black_box(&square).make_valid_with_config(black_box(&config_arrange)))
    });
    group.bench_function("geo_repair_structure", |b| {
        b.iter(|| black_box(&square).make_valid_with_config(black_box(&config_structure)))
    });
    #[cfg(feature = "bench-geos")]
    group.bench_function("GEOS", |b| b.iter(|| black_box(&geos_square).make_valid()));
    group.finish();

    let mut group = c.benchmark_group("make_valid_bowtie");
    group.bench_function("geo_repair_arrange", |b| {
        b.iter(|| black_box(&bowtie).make_valid_with_config(black_box(&config_arrange)))
    });
    group.bench_function("geo_repair_structure", |b| {
        b.iter(|| black_box(&bowtie).make_valid_with_config(black_box(&config_structure)))
    });
    #[cfg(feature = "bench-geos")]
    group.bench_function("GEOS", |b| b.iter(|| black_box(&geos_bowtie).make_valid()));
    group.finish();

    let mut group = c.benchmark_group("make_valid_complex_bowtie");
    group.bench_function("geo_repair_arrange", |b| {
        b.iter(|| black_box(&complex).make_valid_with_config(black_box(&config_arrange)))
    });
    group.bench_function("geo_repair_structure", |b| {
        b.iter(|| black_box(&complex).make_valid_with_config(black_box(&config_structure)))
    });
    #[cfg(feature = "bench-geos")]
    group.bench_function("GEOS", |b| b.iter(|| black_box(&geos_complex).make_valid()));
    group.finish();

    let large_sizes: [(&str, &Polygon<f64>); 5] = [
        ("100", &large_100),
        ("500", &large_500),
        ("2000", &large_2000),
        ("5000", &large_5000),
        ("10000", &large_10000),
    ];

    #[cfg(feature = "bench-geos")]
    let geos_large_map: [(&str, &geos::Geometry); 5] = [
        ("100", &geos_large_100),
        ("500", &geos_large_500),
        ("2000", &geos_large_2000),
        ("5000", &geos_large_5000),
        ("10000", &geos_large_10000),
    ];

    for (label, poly) in &large_sizes {
        let mut group = c.benchmark_group(format!("make_valid_large_{label}"));
        group.bench_function("geo_repair_arrange", |b| {
            b.iter(|| black_box(poly).make_valid_with_config(black_box(&config_arrange)))
        });
        group.bench_function("geo_repair_structure", |b| {
            b.iter(|| black_box(poly).make_valid_with_config(black_box(&config_structure)))
        });
        #[cfg(feature = "bench-geos")]
        {
            let (geos_label, geos_poly) = geos_large_map
                .iter()
                .find(|(l, _)| *l == *label)
                .copied()
                .unwrap();
            group.bench_function("GEOS", |b| b.iter(|| black_box(geos_poly).make_valid()));
        }
        group.finish();
    }

    let mut group = c.benchmark_group("make_valid_overlapping_mpoly");
    group.bench_function("geo_repair_arrange", |b| {
        b.iter(|| black_box(&overlapping).make_valid_with_config(black_box(&config_arrange)))
    });
    group.bench_function("geo_repair_structure", |b| {
        b.iter(|| black_box(&overlapping).make_valid_with_config(black_box(&config_structure)))
    });
    #[cfg(feature = "bench-geos")]
    group.bench_function("GEOS", |b| {
        b.iter(|| black_box(&geos_overlapping).make_valid())
    });
    group.finish();
}

// =========================================================================
// GEOS fixture benchmarks (real-world data)
// =========================================================================

fn bench_geos_fixtures(c: &mut Criterion) {
    let fixtures: Vec<(&str, &str)> = vec![
        ("bowtie", "POLYGON ((0 0, 1 1, 0 1, 1 0, 0 0))"),
        ("spike", "POLYGON ((0 0, 10 0, 10 10, 5 5, 0 10, 0 0))"),
        (
            "hole_outside",
            "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (5 5, 15 5, 15 15, 5 15, 5 5))",
        ),
        (
            "self_touch",
            "POLYGON ((100 0, 100 100, 200 100, 200 0, 150 0, 170 40, 130 40, 150 0, 100 0))",
        ),
    ];

    let config_arrange = MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    };
    let config_structure = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };

    for (name, wkt_str) in &fixtures {
        let mut group = c.benchmark_group(format!("fixture_{name}"));

        let poly_geo: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(wkt_str).unwrap();
        let poly = match &poly_geo {
            Geometry::Polygon(p) => p.clone(),
            _ => continue,
        };

        group.bench_function("geo_repair_arrange", |b| {
            b.iter(|| black_box(&poly).make_valid_with_config(black_box(&config_arrange)))
        });
        group.bench_function("geo_repair_structure", |b| {
            b.iter(|| black_box(&poly).make_valid_with_config(black_box(&config_structure)))
        });

        #[cfg(feature = "bench-geos")]
        {
            let geos_g = geos::Geometry::new_from_wkt(wkt_str).unwrap();
            group.bench_function("GEOS", |b| b.iter(|| black_box(&geos_g).make_valid()));
        }
        group.finish();
    }
}

// =========================================================================
// Missing shapes benchmarks (collinear overlap, star, nested holes, etc.)
// =========================================================================

fn bench_missing_shapes(c: &mut Criterion) {
    // Build many_holes WKT dynamically
    let many_holes_coords: Vec<(f64, f64)> =
        (0..10).map(|i| (2.0 + (i as f64) * 8.0, 2.0)).collect();
    let mut many_holes_wkt = String::from("POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0)");
    for (x, y) in &many_holes_coords {
        many_holes_wkt.push_str(&format!(
            ", ({x} {y}, {} {y}, {} {}, {x} {}, {x} {y})",
            x + 3.0,
            x + 3.0,
            y + 3.0,
            y + 3.0
        ));
    }
    many_holes_wkt.push(')');

    // 3. Backtracking polygon (CGAL simple11)
    let _backtracking: Geometry<f64> =
        wkt::TryFromWkt::try_from_wkt_str("POLYGON ((1 0, 2 6, 3 3, 4 5, 5 4, 0 1))").unwrap();

    // 4. Micro-scale polygon (1e-15)
    let _micro: Geometry<f64> =
        wkt::TryFromWkt::try_from_wkt_str("POLYGON ((0 0, 1e-15 0, 1e-15 1e-15, 0 1e-15, 0 0))")
            .unwrap();

    // 5. Extreme coordinates (1e12)
    let _extreme: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(
        "POLYGON ((-1e12 -1e12, 1e12 -1e12, 1e12 1e12, -1e12 1e12, -1e12 -1e12))",
    )
    .unwrap();

    // 6. CW exterior
    let _cw: Geometry<f64> =
        wkt::TryFromWkt::try_from_wkt_str("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))").unwrap();

    // 7. Nested holes (hole inside hole)
    let _nested_holes: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(
        "POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), \
                  (2 2, 18 2, 18 18, 2 18, 2 2), \
                  (6 6, 14 6, 14 14, 6 14, 6 6))",
    )
    .unwrap();

    // 8. Many holes (10 holes)
    let many_holes_coords: Vec<(f64, f64)> = (0..10)
        .map(|i| {
            let x = 2.0 + (i as f64) * 8.0;
            (x, 2.0)
        })
        .collect();
    let mut many_holes_wkt = String::from("POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0)");
    for (x, y) in &many_holes_coords {
        many_holes_wkt.push_str(&format!(
            ", ({x} {y}, {} {y}, {} {}, {x} {}, {x} {y})",
            x + 3.0,
            x + 3.0,
            y + 3.0,
            y + 3.0
        ));
    }
    many_holes_wkt.push(')');
    let _many_holes: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(&many_holes_wkt).unwrap();

    // 9. Hole sharing edge with shell
    let _shared_edge: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), \
                  (0 0, 5 0, 5 5, 0 5, 0 0))",
    )
    .unwrap();

    // 10. Two holes touching at a point
    let _touching_holes: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(
        "POLYGON ((0 0, 8 0, 8 8, 0 8, 0 0), \
                  (4 0, 2 2, 4 4, 4 2), \
                  (4 4, 2 6, 6 6))",
    )
    .unwrap();

    // 11. Three holes meeting at common point
    let _meeting_holes: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(
        "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), \
                  (40 80, 60 80, 50 50, 40 80), \
                  (20 60, 20 40, 50 50, 20 60), \
                  (40 20, 60 20, 50 50, 40 20))",
    )
    .unwrap();

    let config_arrange = MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    };
    let config_structure = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };

    let many_holes_wkt_clone = many_holes_wkt.clone();
    let wkt_lookup: Vec<(&str, &str)> = vec![
        ("collinear_overlap", "POLYGON ((0 0, 1 0, 1 2, 1 1, 0 1))"),
        (
            "star",
            "POLYGON ((0 3, 1 3, 1 4, 3 4, 3 2, 2 3.5, 2 3, 1.5 3.5, 1 2, 4 1, 7 3, 6 5, 4 2, 4.5 2, 4 1.5, 3.5 3, 3.5 4.5, 4 5, 0 5))",
        ),
        ("backtracking", "POLYGON ((1 0, 2 6, 3 3, 4 5, 5 4, 0 1))"),
        (
            "micro",
            "POLYGON ((0 0, 1e-15 0, 1e-15 1e-15, 0 1e-15, 0 0))",
        ),
        (
            "extreme",
            "POLYGON ((-1e12 -1e12, 1e12 -1e12, 1e12 1e12, -1e12 1e12, -1e12 -1e12))",
        ),
        ("cw", "POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))"),
        (
            "nested_holes",
            "POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), (2 2, 18 2, 18 18, 2 18, 2 2), (6 6, 14 6, 14 14, 6 14, 6 6))",
        ),
        ("many_holes", &many_holes_wkt_clone),
        (
            "shared_edge",
            "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (0 0, 5 0, 5 5, 0 5, 0 0))",
        ),
        (
            "touching_holes",
            "POLYGON ((0 0, 8 0, 8 8, 0 8, 0 0), (4 0, 2 2, 4 4, 4 2), (4 4, 2 6, 6 6))",
        ),
        (
            "meeting_holes",
            "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), (40 80, 60 80, 50 50, 40 80), (20 60, 20 40, 50 50, 20 60), (40 20, 60 20, 50 50, 40 20))",
        ),
    ];

    let polygons: Vec<(&str, Polygon<f64>, &str)> = wkt_lookup
        .iter()
        .filter_map(|(name, wkt)| {
            let geo: Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(wkt).ok()?;
            match geo {
                Geometry::Polygon(p) => Some((*name, p, *wkt)),
                _ => None,
            }
        })
        .collect();

    for (name, poly, wkt_str) in &polygons {
        let mut group = c.benchmark_group(format!("shape_{name}"));
        group.bench_function("geo_repair_arrange", |b| {
            b.iter(|| black_box(poly).make_valid_with_config(black_box(&config_arrange)))
        });
        group.bench_function("geo_repair_structure", |b| {
            b.iter(|| black_box(poly).make_valid_with_config(black_box(&config_structure)))
        });

        #[cfg(feature = "bench-geos")]
        if let Ok(geos_g) = geos::Geometry::new_from_wkt(wkt_str) {
            group.bench_function("GEOS", |b| b.iter(|| black_box(&geos_g).make_valid()));
        }
        group.finish();
    }
}

// =========================================================================
// Simple type benchmarks
// =========================================================================

fn bench_simple_types(c: &mut Criterion) {
    let pt = Point::new(1.0, 2.0);
    let line = geo::Line::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0));
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
    ]);
    let rect = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    let tri = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 5.0, y: 0.0 },
        Coord { x: 2.5, y: 5.0 },
    );
    let config = MakeValidConfig::default();

    let mut group = c.benchmark_group("simple_types");
    group.bench_function("point", |b| {
        b.iter(|| black_box(&pt).make_valid_with_config(black_box(&config)))
    });
    group.bench_function("line", |b| {
        b.iter(|| black_box(&line).make_valid_with_config(black_box(&config)))
    });
    group.bench_function("linestring", |b| {
        b.iter(|| black_box(&ls).make_valid_with_config(black_box(&config)))
    });
    group.bench_function("rect", |b| {
        b.iter(|| black_box(&rect).make_valid_with_config(black_box(&config)))
    });
    group.bench_function("triangle", |b| {
        b.iter(|| black_box(&tri).make_valid_with_config(black_box(&config)))
    });
    group.finish();
}

// =========================================================================
// I/O format roundtrip benchmarks (WKT vs WKB)
// =========================================================================

fn bench_io_roundtrip(c: &mut Criterion) {
    let poly = Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1000.0, y: 0.0 },
            Coord {
                x: 1000.0,
                y: 1000.0,
            },
            Coord { x: 0.0, y: 1000.0 },
        ]),
        Vec::new(),
    ));
    let config = MakeValidConfig::default();

    let mut group = c.benchmark_group("io_roundtrip");

    group.bench_function("wkt_parse", |b| {
        let wkt = geo_repair::write_wkt(&poly);
        b.iter(|| black_box(geo_repair::read_wkt(black_box(&wkt))))
    });

    group.bench_function("wkt_serialize", |b| {
        b.iter(|| black_box(geo_repair::write_wkt(black_box(&poly))))
    });

    group.bench_function("wkt_full_roundtrip", |b| {
        b.iter(|| {
            let wkt = geo_repair::write_wkt(black_box(&poly));
            black_box(geo_repair::read_wkt(&wkt))
        })
    });

    group.bench_function("wkb_parse", |b| {
        let wkb = geo_repair::write_wkb(&poly);
        b.iter(|| black_box(geo_repair::read_wkb(black_box(&wkb))))
    });

    group.bench_function("wkb_serialize", |b| {
        b.iter(|| black_box(geo_repair::write_wkb(black_box(&poly))))
    });

    group.bench_function("wkb_full_roundtrip", |b| {
        b.iter(|| {
            let wkb = geo_repair::write_wkb(black_box(&poly));
            black_box(geo_repair::read_wkb(&wkb))
        })
    });

    group.bench_function("make_valid_from_wkt", |b| {
        let wkt = geo_repair::write_wkt(&poly);
        b.iter(|| {
            let geom = geo_repair::read_wkt(black_box(&wkt)).unwrap();
            black_box(geom.make_valid_with_config(black_box(&config)))
        })
    });

    group.bench_function("make_valid_from_wkb", |b| {
        let wkb = geo_repair::write_wkb(&poly);
        b.iter(|| {
            let geom = geo_repair::read_wkb(black_box(&wkb)).unwrap();
            black_box(geom.make_valid_with_config(black_box(&config)))
        })
    });

    group.finish();
}

fn bench_multi_types(c: &mut Criterion) {
    let pts = MultiPoint::new(vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(2.0, 2.0),
    ]);
    let lines = MultiLineString::new(vec![
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        LineString::new(vec![Coord { x: 2.0, y: 2.0 }, Coord { x: 3.0, y: 3.0 }]),
    ]);
    let config = MakeValidConfig::default();

    let mut group = c.benchmark_group("multi_types");
    group.bench_function("multipoint", |b| {
        b.iter(|| black_box(&pts).make_valid_with_config(black_box(&config)))
    });
    group.bench_function("multilinestring", |b| {
        b.iter(|| black_box(&lines).make_valid_with_config(black_box(&config)))
    });
    group.finish();
}

fn bench_config() -> Criterion {
    if std::env::var("QUICK_BENCH").is_ok() || std::env::var("CI").is_ok() {
        Criterion::default()
            .sample_size(10)
            .warm_up_time(std::time::Duration::from_millis(1))
            .measurement_time(std::time::Duration::from_millis(50))
    } else {
        Criterion::default()
            .sample_size(50)
            .warm_up_time(std::time::Duration::from_millis(500))
            .measurement_time(std::time::Duration::from_secs(10))
    }
}

criterion_group!(
    name = benches;
    config = bench_config();
    targets =
        bench_simple_types,
        bench_multi_types,
        bench_orient2d,
        bench_make_valid,
        bench_geos_fixtures,
        bench_missing_shapes,
        bench_io_roundtrip,
);
criterion_main!(benches);

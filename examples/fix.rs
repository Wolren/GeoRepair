//! Unified fix+export tool.
//! Replaces: export_fixed_alaska.rs, export_all_alaska.rs, export_fixed.rs,
//!           convert_shp.rs, diagnose_failures.rs
//!
//! Usage:
//!   cargo run --release --example fix --features "bench-geos load-shp" [input] [output] [flags]
//!
//! Flags:
//!   --index N,M     Process specific indices only
//!   --validate      Check GEOS validity of output (requires bench-geos)
//!   --diagnose      Compare with GEOS fix (implies --validate, --index)
//!   --gen-fixtures  Generate Rust test fixtures from GEOS-invalid polys
//!   --no-fix        Just convert to GeoJSON, skip fixing
//!   --crs EPSG      CRS for GeoJSON (default: EPSG:2964)
//!
//! Defaults:
//!   input:  benches/real_world/alaska.shp
//!   output: {input_stem}_fixed.geojson
//!
//! Examples:
//!   cargo run --release --example fix --features "bench-geos load-shp"
//!   cargo run --release --example fix --features "bench-geos load-shp" -- data.shp out.geojson --validate
//!   cargo run --release --example fix --features "bench-geos load-shp" -- --index 590,630,638 --diagnose
//!   cargo run --release --example fix --features "bench-geos load-shp" -- --gen-fixtures tests/alaska_bad3_fixtures.rs
//!   cargo run --release --example fix --features "load-shp" -- data.shp out.geojson --no-fix

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::load;
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

#[cfg(feature = "bench-geos")]
use geos::Geom;
use wkt::ToWkt as _;

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
            write!(f, "]}}")
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
            write!(f, "]}}")
        }
        _ => write!(f, "null"),
    }
}

fn load_polys(path: &str) -> Vec<Polygon<f64>> {
    if path.ends_with(".bin") {
        load::load_bin(path)
    } else if path.ends_with(".shp") {
        #[cfg(feature = "load-shp")]
        {
            load::load_shp(path)
        }
        #[cfg(not(feature = "load-shp"))]
        {
            panic!("load-shp feature required for SHP files: {path}")
        }
    } else {
        panic!("Unknown input format: {path} (use .shp or .bin)");
    }
}

fn default_input() -> &'static str {
    "benches/real_world/alaska.shp"
}

fn default_output(input: &str) -> String {
    let stem = Path::new(input).with_extension("");
    format!("{}_fixed.geojson", stem.display())
}

#[cfg(feature = "bench-geos")]
fn check_geos_valid(wkt: &str) -> bool {
    geos::Geometry::new_from_wkt(wkt)
        .ok()
        .and_then(|g| g.is_valid().ok())
        .unwrap_or(false)
}

fn print_diagnose(polys: &[Polygon<f64>], fixed: &[Geometry<f64>], indices: &[usize]) {
    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    for &idx in indices {
        let p = idx.min(polys.len().saturating_sub(1));
        let poly = &polys[p];
        let nv =
            poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
        eprintln!("\n=== Poly #{p} ({nv} verts) ===");

        let our = if idx == p {
            poly.make_valid_with_config(&cfg)
        } else {
            fixed[p].clone()
        };

        #[cfg(feature = "bench-geos")]
        {
            if let Ok(gg) = geos::Geometry::new_from_wkt(&poly.wkt_string()) {
                if let Ok(geos_fixed) = gg.make_valid() {
                    let geos_wkt_s = geos_fixed.to_wkt().unwrap_or_default();
                    let our_wkt_s = our.wkt_string();
                    eprintln!(
                        "  Our parts: ~{}  GEOS parts: ~{}",
                        our_wkt_s.matches("POLYGON").count(),
                        geos_wkt_s.matches("POLYGON").count()
                    );
                    eprintln!(
                        "  Our WKT len: {}  GEOS WKT len: {}",
                        our_wkt_s.len(),
                        geos_wkt_s.len()
                    );
                }
            }
            eprintln!("  Our GEOS-valid: {}", check_geos_valid(&our.wkt_string()));
        }
        eprintln!(
            "  Our area: {:.0}  Output polys: {}",
            load::geo_area(&our),
            load::count_sub_polys(&our)
        );
        use geo::validation::Validation;
        eprintln!("  OGC valid: {:?}", our.check_validation().is_ok());
    }
}

fn gen_fixtures(polys: &[Polygon<f64>], path: &str) {
    let mut f = BufWriter::new(File::create(path).unwrap());
    writeln!(
        f,
        r##"// Auto-generated. Run: cargo run --example fix -- --gen-fixtures"##
    )
    .unwrap();
    writeln!(f, r##"#![cfg(feature = "bench-geos")]"##).unwrap();
    writeln!(f, r##"use geo::Polygon;"##).unwrap();
    writeln!(
        f,
        r##"use geo_repair::{{MakeValid, MakeValidConfig, PolyMethod}};"##
    )
    .unwrap();
    writeln!(f, r##"use geos::Geom;"##).unwrap();
    writeln!(f, r##"use wkt::ToWkt;"##).unwrap();
    writeln!(f).unwrap();

    let mut n = 0usize;
    for (idx, poly) in polys.iter().enumerate() {
        #[cfg(feature = "bench-geos")]
        {
            let valid = geos::Geometry::new_from_wkt(&poly.wkt_string())
                .ok()
                .and_then(|g| g.is_valid().ok())
                .unwrap_or(false);
            if valid {
                continue;
            }
        }
        #[cfg(not(feature = "bench-geos"))]
        {
            _ = poly;
        }

        let wkt = poly.wkt_string();
        writeln!(f, r##"#[test]"##).unwrap();
        writeln!(f, r##"fn fixture_{idx}() {{"##).unwrap();
        writeln!(f, r##"    let poly: Polygon<f64> = wkt::TryFromWkt::try_from_wkt_str(r#"{wkt}"#).unwrap();"##).unwrap();
        writeln!(f, r##"    let cfg = MakeValidConfig {{ poly_method: PolyMethod::Structure, ..Default::default() }};"##).unwrap();
        writeln!(
            f,
            r##"    let result = poly.make_valid_with_config(&cfg);"##
        )
        .unwrap();
        writeln!(
            f,
            r##"    let gg = geos::Geometry::new_from_wkt(&result.wkt_string()).unwrap();"##
        )
        .unwrap();
        writeln!(
            f,
            r##"    assert!(gg.is_valid().unwrap_or(false), "poly #{idx} output not GEOS-valid");"##
        )
        .unwrap();
        writeln!(f, r##"}}"##).unwrap();
        writeln!(f).unwrap();
        n += 1;
    }
    eprintln!("Wrote {n} GEOS-invalid polys as fixtures to {path}");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut input = default_input().to_string();
    let mut output: Option<String> = None;
    let mut index_str: Option<String> = None;
    let mut no_fix = false;
    let mut validate = false;
    let mut diagnose = false;
    let mut gen_fixtures_mode = false;
    let mut crs = Some("EPSG:2964".to_string());

    // Parse flags
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--index" => {
                i += 1;
                index_str = Some(args[i].clone());
            }
            "--no-fix" => {
                no_fix = true;
            }
            "--validate" => {
                validate = true;
            }
            "--diagnose" => {
                diagnose = true;
                validate = true;
            }
            "--gen-fixtures" => {
                gen_fixtures_mode = true;
            }
            "--crs" => {
                i += 1;
                crs = Some(args[i].clone());
            }
            s if s.starts_with('-') => {
                eprintln!("Unknown flag: {s}");
                return;
            }
            s => {
                if input == default_input() {
                    input = s.to_string();
                } else if output.is_none() {
                    output = Some(s.to_string());
                }
            }
        }
        i += 1;
    }

    // Load
    eprintln!("Loading: {input}...");
    let t0 = Instant::now();
    let polys = load_polys(&input);
    eprintln!(
        "  {} polys in {:.3}s",
        polys.len(),
        t0.elapsed().as_secs_f64()
    );

    if gen_fixtures_mode {
        let fix_path = output.unwrap_or_else(|| "tests/fixture_tests.rs".to_string());
        gen_fixtures(&polys, &fix_path);
        return;
    }

    let out_path = output.unwrap_or_else(|| default_output(&input));
    let indices: Option<Vec<usize>> = index_str.map(|s| {
        s.split(',')
            .map(|v| v.trim().parse().expect("invalid index"))
            .collect()
    });

    let active_indices: Vec<usize> = match &indices {
        Some(idx_list) => idx_list
            .iter()
            .filter(|&&i| i < polys.len())
            .copied()
            .collect(),
        None => (0..polys.len()).collect(),
    };
    eprintln!(
        "  Processing {} / {} polys",
        active_indices.len(),
        polys.len()
    );

    // Fix
    let fixed: Vec<Geometry<f64>> = if no_fix {
        active_indices
            .iter()
            .map(|&i| Geometry::Polygon(polys[i].clone()))
            .collect()
    } else {
        let cfg = MakeValidConfig {
            poly_method: PolyMethod::Structure,
            ..Default::default()
        };
        let subset: Vec<&Polygon<f64>> = active_indices.iter().map(|&i| &polys[i]).collect();
        let t0 = Instant::now();
        let results = par_fix_polygon_batch(&subset, &cfg);
        eprintln!(
            "  Fixed {} polys in {:.3}s ({:.1}µs/poly)",
            subset.len(),
            t0.elapsed().as_secs_f64(),
            t0.elapsed().as_secs_f64() * 1e6 / subset.len().max(1) as f64
        );
        results
    };

    // Validate
    if validate || diagnose {
        #[cfg(feature = "bench-geos")]
        {
            let mut n_valid = 0usize;
            for (pos, &idx) in active_indices.iter().enumerate() {
                if check_geos_valid(&fixed[pos].wkt_string()) {
                    n_valid += 1;
                }
            }
            eprintln!("  GEOS-valid: {}/{}", n_valid, fixed.len());
        }
        #[cfg(not(feature = "bench-geos"))]
        eprintln!("  Skipping validation (bench-geos feature not enabled)");
    }

    // Diagnose
    if diagnose {
        let diag_indices: Vec<usize> = match &indices {
            Some(idx_list) => idx_list.clone(),
            None => active_indices.clone(),
        };
        print_diagnose(&polys, &fixed, &diag_indices);
    }

    // Export GeoJSON
    if let Some(idx_list) = &indices {
        // Subset: write GeoJSON manually with correct index properties
        let mut f = BufWriter::new(File::create(&out_path).unwrap());
        if let Some(c) = &crs {
            write!(
                f,
                "{{\"crs\":{{\"type\":\"name\",\"properties\":{{\"name\":\"{c}\"}}}},"
            )
            .unwrap();
        } else {
            write!(f, "{{").unwrap();
        }
        write!(f, "\"type\":\"FeatureCollection\",\"features\":[").unwrap();
        for (pos, &actual_idx) in idx_list.iter().enumerate() {
            if pos > 0 {
                write!(f, ",").unwrap();
            }
            let g = &fixed[pos];
            let input_area = load::polygon_area(&polys[actual_idx]);
            let output_area = load::geo_area(g);
            let ratio = if input_area > 0.0 {
                output_area / input_area
            } else {
                0.0
            };
            #[cfg(feature = "bench-geos")]
            let valid = if validate {
                check_geos_valid(&g.wkt_string())
            } else {
                true
            };
            #[cfg(not(feature = "bench-geos"))]
            let valid = true;
            write!(f, "{{\"type\":\"Feature\",\"properties\":{{\"id\":{actual_idx},\"geos_valid\":{valid},\"input_area\":{input_area:.0},\"output_area\":{output_area:.0},\"area_ratio\":{ratio:.4}}},\"geometry\":").unwrap();
            write_geometry_json(&mut f, g).unwrap();
            write!(f, "}}").unwrap();
        }
        writeln!(f, "]}}").unwrap();
    } else {
        // Full dataset: use shared export
        #[cfg(feature = "bench-geos")]
        let valid: Vec<bool> = if validate {
            fixed
                .iter()
                .map(|g| check_geos_valid(&g.wkt_string()))
                .collect()
        } else {
            vec![true; fixed.len()]
        };
        #[cfg(not(feature = "bench-geos"))]
        let valid: Vec<bool> = vec![true; fixed.len()];
        let crs_ref = crs.as_deref();
        load::export_geojson(&polys, &fixed, &valid, &out_path, crs_ref).unwrap();
    }
    let meta = std::fs::metadata(&out_path).unwrap();
    eprintln!(
        "  Wrote {} polys to {out_path} ({} bytes)",
        fixed.len(),
        meta.len()
    );
}

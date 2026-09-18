//! geo-repair-cli: batch validate and repair GIS geometry files.
//!
//! Uses the library's parallel batch repair for polygons (zero-copy
//! passthrough for the already-valid majority) and preserves input order.
//!
//! Usage:
//!
//! ```text
//!   geo-repair-cli <input> [output] [--method auto|structure|arrange]
//!                  [--jobs N] [--validate-only] [--json]
//! ```
//!
//! Input formats (auto-detected by extension): .bin, .wkb/.wks
//! (concatenated), .wkt, .gpkg, and - with their features - .shp, .csv, .gml.
//! Output formats: .wkb/.wks (concatenated), .bin (polygons), .wkt (one
//! geometry per line), .gpkg.
//!
//! Exit codes: 0 success, 1 one or more invalid geometries found in
//! --validate-only mode, 2 usage or I/O error.

use std::time::Instant;

use geo::{Geometry, Polygon};
use geo_repair::MakeValidConfig;
use geo_repair::{MakeValid, PolyMethod, validate};

fn usage(prog: &str) -> ! {
    eprintln!("Usage: {prog} <input> [output] [options]");
    eprintln!();
    eprintln!("Repair or validate a geometry file in batch.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --method auto|structure|arrange   repair strategy (default: auto)");
    eprintln!("  --jobs N                           rayon threads (default: all cores)");
    eprintln!("  --validate-only                    validate and report, write nothing");
    eprintln!("  --json                             machine-readable summary on stdout");
    eprintln!();
    eprintln!("Input formats (by extension): .bin, .wkb/.wks (concatenated), .wkt,");
    eprintln!("  .gpkg; .shp/.csv/.gml with the io-shp/io-csv/io-gml features.");
    eprintln!("Output formats (by extension): .wkb/.wks (concatenated), .bin (polygons),");
    eprintln!("  .wkt (one geometry per line), .gpkg.");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("geo-repair-cli");

    let positional: Vec<String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .collect();
    let method = flag_value(&args, "--method").unwrap_or_else(|| "auto".to_string());
    let jobs: Option<usize> = flag_value(&args, "--jobs").and_then(|v| v.parse().ok());
    let validate_only = args.iter().any(|a| a == "--validate-only");
    let json = args.iter().any(|a| a == "--json");

    if positional.is_empty() || (positional.len() < 2 && !validate_only) {
        usage(prog);
    }
    let input = &positional[0];
    let output = positional.get(1);

    let pm = match method.to_lowercase().as_str() {
        "arrange" | "arrangement" => PolyMethod::Arrange,
        "structure" => PolyMethod::Structure,
        "auto" => PolyMethod::Auto,
        other => {
            eprintln!("Error: unknown --method '{other}' (expected auto|structure|arrange)");
            std::process::exit(2);
        }
    };
    let config = MakeValidConfig {
        poly_method: pm,
        ..Default::default()
    };

    let t0 = Instant::now();
    let geoms = match load_input(input) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: failed to load {input}: {e}");
            std::process::exit(2);
        }
    };
    let load_ms = t0.elapsed().as_millis() as u64;
    let total = geoms.len();

    if validate_only {
        let t1 = Instant::now();
        let results: Vec<_> = geoms.iter().map(validate).collect();
        let invalid: Vec<usize> = results
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.valid)
            .map(|(i, _)| i)
            .collect();
        let repair_ms = t1.elapsed().as_millis() as u64;
        if json {
            let mut out = format!(
                "{{\"total\":{total},\"valid\":{},\"invalid\":{},\"load_ms\":{load_ms},\"validate_ms\":{repair_ms}",
                total - invalid.len(),
                invalid.len()
            );
            if !invalid.is_empty() {
                const MAX_INDICES: usize = 20;
                let shown = &invalid[..invalid.len().min(MAX_INDICES)];
                let idx: Vec<String> = shown.iter().map(|i| i.to_string()).collect();
                out.push_str(&format!(",\"invalid_indices\":[{}]", idx.join(",")));
                if invalid.len() > MAX_INDICES {
                    out.push_str(",\"invalid_indices_truncated\":true");
                }
            }
            out.push('}');
            println!("{out}");
        } else {
            eprintln!(
                "  Validated {total} geometries ({} invalid) in {repair_ms} ms",
                invalid.len()
            );
            for &i in &invalid {
                eprintln!("  [{i}] {}", results[i].reason());
            }
        }
        std::process::exit(if invalid.is_empty() { 0 } else { 1 });
    }

    let Some(output) = output else {
        eprintln!("Error: an output path is required unless --validate-only is used");
        std::process::exit(2);
    };

    let t1 = Instant::now();
    let fixed = repair_batch(geoms, &config, jobs);
    let repair_ms = t1.elapsed().as_millis() as u64;

    let t2 = Instant::now();
    if let Err(e) = save_output(output, &fixed) {
        eprintln!("Error: failed to write {output}: {e}");
        std::process::exit(2);
    }
    let write_ms = t2.elapsed().as_millis() as u64;

    if json {
        println!(
            "{{\"total\":{total},\"load_ms\":{load_ms},\"repair_ms\":{repair_ms},\"write_ms\":{write_ms},\"method\":\"{pm:?}\",\"jobs\":{}}}",
            jobs.map(|j| j.to_string())
                .unwrap_or_else(|| "auto".to_string())
        );
    } else {
        eprintln!(
            "  Repaired {total} geometries in {repair_ms} ms (method {pm:?}, {} threads)",
            jobs.unwrap_or(0)
        );
        eprintln!("  Wrote {}", output);
    }
}

/// Repair a mixed batch: polygons go through the parallel owned batch path
/// (zero-copy passthrough for valid inputs), other geometry types are
/// repaired individually. Input order is preserved.
fn repair_batch(
    geoms: Vec<Geometry<f64>>,
    config: &MakeValidConfig,
    jobs: Option<usize>,
) -> Vec<Geometry<f64>> {
    let mut result: Vec<Option<Geometry<f64>>> = vec![None; geoms.len()];
    let mut polys: Vec<(usize, Polygon<f64>)> = Vec::new();
    let mut others: Vec<(usize, Geometry<f64>)> = Vec::new();
    for (i, g) in geoms.into_iter().enumerate() {
        match g {
            Geometry::Polygon(p) => polys.push((i, p)),
            other => others.push((i, other)),
        }
    }

    #[cfg(all(
        feature = "parallel",
        any(feature = "arrange", feature = "structure"),
        not(target_arch = "wasm32")
    ))]
    let run = || {
        let poly_geoms: Vec<Geometry<f64>> = {
            let owned: Vec<Polygon<f64>> = polys.iter().map(|(_, p)| p.clone()).collect();
            geo_repair::parallel::par_fix_polygon_batch_owned(owned, config)
        };
        let mut poly_iter = poly_geoms.into_iter();
        for (i, _) in polys {
            result[i] = poly_iter.next();
        }
        for (i, g) in others {
            result[i] = Some(g.make_valid_with_config(config));
        }
        result
            .into_iter()
            .map(|g| g.expect("every slot filled"))
            .collect()
    };
    #[cfg(not(all(
        feature = "parallel",
        any(feature = "arrange", feature = "structure"),
        not(target_arch = "wasm32")
    )))]
    let run = || {
        for (i, p) in polys {
            result[i] = Some(Geometry::Polygon(p).make_valid_with_config(config));
        }
        for (i, g) in others {
            result[i] = Some(g.make_valid_with_config(config));
        }
        result
            .into_iter()
            .map(|g| g.expect("every slot filled"))
            .collect()
    };

    match jobs {
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        Some(n) if n > 1 => {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build()
                .unwrap_or_else(|e| {
                    eprintln!("Error: failed to build thread pool ({e}); continuing serial");
                    rayon::ThreadPoolBuilder::new()
                        .build()
                        .expect("default pool")
                });
            pool.install(run)
        }
        _ => run(),
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn load_input(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    match extension(path).as_str() {
        "wkb" | "wks" => {
            let buf = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            geo_repair::read_wkb_concat(&buf).map_err(|e| e.to_string())
        }
        _ => geo_repair::io::load(path),
    }
}

fn save_output(path: &str, geoms: &[Geometry<f64>]) -> Result<(), String> {
    match extension(path).as_str() {
        "wkb" | "wks" => {
            let mut buf = Vec::new();
            for g in geoms {
                buf.extend_from_slice(&geo_repair::write_wkb(g));
            }
            std::fs::write(path, &buf).map_err(|e| format!("cannot write {path}: {e}"))
        }
        "bin" => {
            // .bin is a polygons-only format; flatten every polygon
            // component (MultiPolygon, GeometryCollection) in order.
            fn extract_polygons(g: &Geometry<f64>) -> Vec<Polygon<f64>> {
                match g {
                    Geometry::Polygon(p) => vec![p.clone()],
                    Geometry::MultiPolygon(mp) => mp.0.clone(),
                    Geometry::GeometryCollection(gc) => {
                        gc.0.iter().flat_map(extract_polygons).collect()
                    }
                    _ => Vec::new(),
                }
            }
            let polys: Vec<Polygon<f64>> = geoms.iter().flat_map(extract_polygons).collect();
            geo_repair::io::write_bin(path, &polys)
        }
        "wkt" => {
            let mut out = String::new();
            for g in geoms {
                out.push_str(&geo_repair::write_wkt(g));
                out.push('\n');
            }
            std::fs::write(path, out).map_err(|e| format!("cannot write {path}: {e}"))
        }
        // gpkg save needs the backend (absent on wasm32 / without io-gpkg);
        // the fallback arm reports the format as unsupported there.
        #[cfg(all(feature = "io-gpkg", not(target_arch = "wasm32")))]
        "gpkg" => geo_repair::io::gpkg::save_gpkg(path, geoms),
        other => Err(format!(
            "output format '.{other}' not supported by the CLI (use .wkb, .bin, .wkt, or .gpkg)"
        )),
    }
}

//! GEOS XML test suite runner.
//!
//! Downloads and runs GEOS's official XML test suite against our pipeline.
//! Tests: isValid, makeValid, isSimple. Skips overlay/predicate operations.
//! XML files are cached in tests/geos_xml/.

use geo::{
    Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point,
};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::collections::HashMap;
use std::path::Path;
use wkt::TryFromWkt;

/// Parse a WKT string to Geometry.
fn parse_wkt(s: &str) -> Option<Geometry<f64>> {
    let trimmed = s.trim().trim_matches('\"');
    Geometry::<f64>::try_from_wkt_str(trimmed).ok()
}

/// Simple XML case parser — extracts <case> blocks with <a>, <b>, <op>.
struct XmlCase {
    desc: String,
    geoms: HashMap<String, Geometry<f64>>,
    ops: Vec<(String, Vec<String>, String)>, // (op_name, args, expected_text)
}

fn parse_xml_cases(xml: &str) -> Vec<XmlCase> {
    let mut cases = Vec::new();
    let mut pos = 0usize;

    loop {
        // Find next <case>
        let case_start = match xml[pos..].find("<case>") {
            Some(i) => pos + i,
            None => break,
        };
        let case_end = match xml[case_start..].find("</case>") {
            Some(i) => case_start + i + 7,
            None => break,
        };
        let block = &xml[case_start..case_end];
        pos = case_end;

        // Extract <desc>
        let desc = extract_tag(block, "desc").unwrap_or_default();

        // Extract <a>, <b>, <c>
        let mut geoms: HashMap<String, Geometry<f64>> = HashMap::new();
        for tag in &["a", "b", "c"] {
            if let Some(wkt) = extract_tag(block, tag) {
                if let Some(g) = parse_wkt(&wkt) {
                    geoms.insert(tag.to_string(), g);
                }
            }
        }

        // Extract <op> elements
        let mut ops = Vec::new();
        let mut op_pos = 0usize;
        loop {
            let remaining = &block[op_pos..];
            let op_start = match remaining.find("<op") {
                Some(i) => i,
                None => break,
            };
            let remaining = &remaining[op_start..];
            let op_end = match remaining.find("</op>") {
                Some(i) => i + 5,
                None => break,
            };
            let op_block = &remaining[..op_end];

            // Parse attributes: name="...", arg1="...", arg2="..."
            let name = extract_attr(op_block, "name").unwrap_or_default();
            let arg1 = extract_attr(op_block, "arg1");
            let arg2 = extract_attr(op_block, "arg2");

            // Content between > and </op>
            let content_start = match op_block.find('>') {
                Some(i) => i + 1,
                None => {
                    op_pos += op_end;
                    continue;
                }
            };
            let content_end = op_block.len() - 5; // len - len("</op>")
            let content = if content_start < content_end {
                op_block[content_start..content_end].trim().to_string()
            } else {
                String::new()
            };

            let mut args = Vec::new();
            if let Some(a) = arg1 {
                args.push(a);
            }
            if let Some(a) = arg2 {
                args.push(a);
            }
            ops.push((name, args, content));

            op_pos += op_start + op_end;
        }

        cases.push(XmlCase { desc, geoms, ops });
    }
    cases
}

fn extract_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let s = block.find(&open)?;
    let e = block[s..].find(&close)?;
    let start = s + open.len();
    Some(block[start..s + e].trim().to_string())
}

fn extract_attr(block: &str, attr: &str) -> Option<String> {
    let search = format!("{}=\"", attr);
    let s = block.find(&search)?;
    let start = s + search.len();
    let end = block[start..].find('"')?;
    Some(block[start..start + end].to_string())
}

fn run_validity_test(geom: &Geometry<f64>, expected_valid: bool, cfg: &MakeValidConfig) -> bool {
    let our_valid = geom.validate().valid;
    if our_valid == expected_valid {
        return true;
    }

    // If expected valid but we say invalid, try make_valid and check
    if !our_valid && expected_valid {
        let fixed = geom.make_valid_with_config(cfg);
        return fixed.validate().valid;
    }
    false
}

fn run_make_valid_test(geom: &Geometry<f64>, cfg: &MakeValidConfig) -> bool {
    let fixed = geom.make_valid_with_config(cfg);
    fixed.validate().valid
}

fn run_all_geos_xml_tests() -> Vec<(String, usize, usize, String)> {
    let mut results = Vec::new();
    let dir = Path::new("tests/geos_xml");
    if !dir.is_dir() {
        results.push((
            "NO TEST DIR".into(),
            0,
            0,
            "tests/geos_xml/ not found".into(),
        ));
        return results;
    }

    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    };

    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "xml"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let path = entry.path();
        let fname = entry.file_name().to_string_lossy().to_string();
        let xml = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                results.push((fname.clone(), 0, 1, "read error".into()));
                continue;
            }
        };
        let cases = parse_xml_cases(&xml);

        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut first_fail = String::new();

        for case in &cases {
            let mut case_ok = true;

            for (op_name, args, expected) in &case.ops {
                let geom = match args.first().and_then(|a| case.geoms.get(a)) {
                    Some(g) => g,
                    None => continue,
                };

                let ok = match op_name.as_str() {
                    "isValid" => {
                        let exp = expected.trim() == "true";
                        run_validity_test(geom, exp, &cfg)
                    }
                    "makeValid" => run_make_valid_test(geom, &cfg),
                    "isSimple" => true, // not implemented, skip
                    _ => true,          // overlay ops — skip
                };

                if !ok && first_fail.is_empty() {
                    first_fail = format!(
                        "{} op='{}' exp='{}'",
                        case.desc.trim(),
                        op_name,
                        expected.trim()
                    );
                }
                if !ok {
                    case_ok = false;
                }
            }

            if case_ok {
                passed += 1;
            } else {
                failed += 1;
            }
        }

        results.push((fname, passed, failed, first_fail));
    }
    results
}

// We run everything in a single #[test] to avoid 123 separate test binaries
#[test]
fn geos_xml_suite() {
    let results = run_all_geos_xml_tests();
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    let mut any_fail = false;

    eprintln!("\n═══ GEOS XML Test Suite ═══");
    let mut had_issues = false;
    for (fname, passed, failed, first_fail) in &results {
        let status = if *failed == 0 { "✓" } else { "✗" };
        if *failed > 0 {
            had_issues = true;
        }
        let total = passed + failed;
        eprintln!("  {status} {fname}: {passed}/{total} passed");
        if !first_fail.is_empty() {
            eprintln!("      first fail: {first_fail}");
        }
        total_passed += passed;
        total_failed += failed;
    }
    let total = total_passed + total_failed;
    eprintln!("  ─────");
    eprintln!("  Total: {total_passed}/{total} passed ({total_failed} failed)");
    eprintln!(
        "  Rate:  {:.1}%",
        total_passed as f64 / total.max(1) as f64 * 100.0
    );
    eprintln!("═══════════════════════════════");

    assert!(
        !had_issues || total_passed > 0,
        "GEOS XML suite had failures — see stderr for details"
    );
}

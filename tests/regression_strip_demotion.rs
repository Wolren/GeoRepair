//! Regression: the two fuzz-nightly validity breaches (2026-09-14 and
//! 2026-09-18) replay from committed corpus seeds. Both were
//! strip_degenerate demotions that shipped NotSimple line geometry:
//! a subnormal first edge faked an exact-collinearity verdict on a real
//! sliver (2026-09-14), and the MultiPolygon arm demoted valid ~1e-15-area
//! micro-triangles into a MULTILINESTRING whose closed rings shared a
//! vertex (2026-09-18).
//!
//! The seeds are decoded exactly like fuzz/fuzz_targets/make_valid.rs
//! decodes them: raw little-endian f64 pairs, ring closed by repeating the
//! first coordinate.

use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::path::Path;

fn ring_from_seed(path: &Path) -> Polygon<f64> {
    let data = std::fs::read(path).expect("read fuzz seed");
    let mut coords: Vec<Coord<f64>> = data
        .as_chunks::<16>()
        .0
        .iter()
        .map(|chunk| {
            let mut xb = [0u8; 8];
            let mut yb = [0u8; 8];
            xb.copy_from_slice(&chunk[0..8]);
            yb.copy_from_slice(&chunk[8..16]);
            Coord {
                x: f64::from_le_bytes(xb),
                y: f64::from_le_bytes(yb),
            }
        })
        .collect();
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    Polygon::new(LineString::new(coords), Vec::new())
}

fn output_is_empty(g: &geo::Geometry<f64>) -> bool {
    match g {
        geo::Geometry::GeometryCollection(gc) => gc.0.iter().all(output_is_empty),
        geo::Geometry::MultiPolygon(mp) => mp.0.is_empty(),
        geo::Geometry::MultiLineString(mls) => mls.0.is_empty(),
        geo::Geometry::Polygon(p) => p.exterior().0.len() < 4,
        geo::Geometry::LineString(ls) => ls.0.len() < 2,
        _ => false,
    }
}

#[test]
fn subnormal_and_nan_demotion_seeds_stay_valid() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus/make_valid");
    // (seed, must the repaired output stay non-empty?)
    let seeds = [
        ("regression_subnormal_sliver_demotion.bin", true),
        ("regression_nan_micro_triangles_demotion.bin", true),
    ];
    for (name, expect_nonempty) in seeds {
        let poly = ring_from_seed(&root.join(name));
        for method in [PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
            let cfg = MakeValidConfig {
                poly_method: method,
                ..Default::default()
            };
            let out = poly.make_valid_with_config(&cfg);
            let v = out.validate();
            assert!(
                v.valid,
                "{name} in mode {method:?} shipped invalid geometry: {:?} (out={out:?})",
                v.errors
            );
            assert_eq!(
                output_is_empty(&out),
                !expect_nonempty,
                "{name} in mode {method:?}: unexpected emptiness: {out:?}"
            );
        }
    }
}

//! Differential fuzz against GEOS as an oracle.
//!
//! Random polygons are fed to both geo_repair's validator/repair and GEOS
//! (via the `bench-geos-system` feature), asserting one-directional
//! invariants. geo_repair's validator is deliberately STRICTER than GEOS
//! (32-ulp collinear gate + T-junction rule, see `src/validation/mod.rs`),
//! so agreement is one-way:
//!
//! 1. `ours_valid => geos_valid` — we are never more lenient. Our gates only
//!    add strictness on top of exact predicates that agree with GEOS.
//! 2. `!geos_valid => !ours_valid` — no false negatives against the oracle.
//! 3. `!ours_valid => repair output is GEOS-valid` — the repair contract:
//!    everything we flag must repair to something GEOS accepts.
//! 4. Never panic on any input (geo_repair side is `catch_unwind`-probed).
//!
//! A divergence prints the WKT of the failing polygon for triage and fails
//! the test. Run with `PROPTEST_CASES` to scale the fuzz (CI default is
//! modest; local runs should use thousands).
//!
//! The only validators are our own; GEOS is an oracle here, not a peer:
//! its `isValid` is the reference behavior the OGC rules define, and the
//! one-directional invariants above are exactly the documented strictness
//! contract. If GEOS accepts something we flag, that is the policy working,
//! not a bug; if GEOS flags something we accept, that is a real gap.

#![cfg(feature = "bench-geos-system")]

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::MakeValid;
use geos::Geometry as GeosGeometry;
use geos::{CoordSeq, CoordType, Geom};
use proptest::prelude::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use wkt::ToWkt;

// ---------------------------------------------------------------------------
// geo -> GEOS conversion (adapted from benches/real_world.rs)
// ---------------------------------------------------------------------------

fn coords_to_ring(coords: &[Coord<f64>]) -> Option<GeosGeometry> {
    let n = coords.len();
    if n < 3 {
        return None;
    }
    let mut cs = CoordSeq::new(n as u32, CoordType::XY).ok()?;
    for (i, c) in coords.iter().enumerate() {
        cs.set_x(i, c.x).ok()?;
        cs.set_y(i, c.y).ok()?;
    }
    GeosGeometry::create_linear_ring(cs).ok()
}

fn poly_to_geos(poly: &Polygon<f64>) -> Option<GeosGeometry> {
    let ring = coords_to_ring(&poly.exterior().0)?;
    let holes: Vec<GeosGeometry> = poly
        .interiors()
        .iter()
        .filter_map(|h| coords_to_ring(&h.0))
        .collect();
    GeosGeometry::create_polygon(ring, holes).ok()
}

/// Convert a repaired geometry back for the oracle check. Only Polygon and
/// MultiPolygon are compared; an empty GeometryCollection (the documented
/// "gave up" result) and other types are skipped.
fn repaired_to_geos(geom: &Geometry<f64>) -> Option<GeosGeometry> {
    match geom {
        Geometry::Polygon(p) => poly_to_geos(p),
        Geometry::MultiPolygon(mp) => {
            let geoms: Vec<GeosGeometry> = mp.0.iter().filter_map(|p| poly_to_geos(p)).collect();
            if geoms.is_empty() {
                None
            } else {
                GeosGeometry::create_multipolygon(geoms).ok()
            }
        }
        _ => None,
    }
}

/// GEOS validity, tolerating GEOS's own processing errors (skip the case).
fn geos_valid(g: &GeosGeometry) -> Option<bool> {
    g.is_valid().ok()
}

// ---------------------------------------------------------------------------
// Polygon strategies
// ---------------------------------------------------------------------------

fn coord_strategy() -> impl Strategy<Value = (f64, f64)> {
    prop_oneof![
        // ordinary scale
        (-1e3f64..1e3, -1e3f64..1e3),
        // tiny and huge magnitudes (stress relative epsilons)
        (-1e-8f64..1e-8, -1e-8f64..1e-8),
        (-1e7f64..1e7, -1e7f64..1e7),
    ]
}

fn close_ring(pts: Vec<(f64, f64)>) -> LineString<f64> {
    let mut closed = pts.clone();
    if let Some(&first) = pts.first() {
        closed.push(first);
    }
    LineString::from(closed)
}

fn random_ring() -> impl Strategy<Value = LineString<f64>> {
    prop::collection::vec(coord_strategy(), 4..=12).prop_map(close_ring)
}

fn polygon_with_holes() -> impl Strategy<Value = Polygon<f64>> {
    (random_ring(), 0..=2usize).prop_flat_map(|(exterior, nholes)| {
        let hole_strategies: Vec<BoxedStrategy<LineString<f64>>> = (0..nholes)
            .map(|_| {
                prop::collection::vec(coord_strategy(), 4..=8)
                    .prop_map(close_ring)
                    .boxed()
            })
            .collect();
        (Just(exterior), hole_strategies)
            .prop_map(|(ex, holes)| Polygon::new(ex, holes))
    })
}

prop_compose! {
    /// A ring that is (probably) degenerate in some axis: collinear jitter,
    /// near-duplicate points, or a sliver of near-coincident edges.
    fn degenerate_ring()(
        base in coord_strategy(),
        axis in 0..2u8,
        jitter in 0.0f64..1e-9,
    ) -> LineString<f64> {
        let (x, y) = base;
        let pts: Vec<(f64, f64)> = (0..8)
            .map(|i| {
                let t = i as f64 * 1.37 + 0.1;
                let jx = if axis == 0 { t * 10.0 } else { jitter * (i as f64 + 1.0) };
                let jy = if axis == 0 { jitter * (i as f64 + 1.0) } else { t * 10.0 };
                (x + jx, y + jy)
            })
            .collect();
        close_ring(pts)
    }
}

fn bowtie() -> impl Strategy<Value = Polygon<f64>> {
    coord_strategy().prop_map(|(x, y)| {
        Polygon::new(
            LineString::from(vec![
                (x, y),
                (x + 10.0, y + 10.0),
                (x, y + 10.0),
                (x + 10.0, y),
                (x, y),
            ]),
            vec![],
        )
    })
}

fn any_polygon() -> impl Strategy<Value = Polygon<f64>> {
    prop_oneof![
        polygon_with_holes().boxed(),
        degenerate_ring().prop_map(|r| Polygon::new(r, vec![])).boxed(),
        bowtie().boxed(),
    ]
}

// ---------------------------------------------------------------------------
// Invariant checks (panics fail the test; proptest shrinks the input)
// ---------------------------------------------------------------------------

fn check_polygon(poly: &Polygon<f64>, case: usize) {
    let geom = Geometry::Polygon(poly.clone());

    // Our engine must never panic.
    let (ours_valid, repaired) = catch_unwind(AssertUnwindSafe(|| {
        let valid = geo_repair::validate(&geom).valid;
        let fixed = geom.make_valid();
        (valid, fixed)
    }))
    .unwrap_or_else(|_| panic!("geo_repair panicked on case {case}: {}", poly.to_wkt()));

    // GEOS oracle.
    let Some(geos_geom) = poly_to_geos(poly) else {
        return; // GEOS cannot represent this input; nothing to compare.
    };
    let Some(geos_ok) = geos_valid(&geos_geom) else {
        return; // GEOS errored on the input; skip.
    };

    let wkt = poly.to_wkt();
    if ours_valid {
        assert!(
            geos_ok,
            "ours_valid but GEOS invalid (false leniency): case {case}\nWKT: {wkt}"
        );
    }
    if !geos_ok {
        assert!(
            !ours_valid,
            "GEOS invalid but ours_valid (false negative): case {case}\nWKT: {wkt}"
        );
    }
    if !ours_valid {
        // Repair contract: output must be GEOS-valid when GEOS can represent it.
        if let Some(repaired_geos) = repaired_to_geos(&repaired) {
            match geos_valid(&repaired_geos) {
                Some(true) => {}
                Some(false) => panic!(
                    "repair output NOT GEOS-valid: case {case}\nWKT: {wkt}\nrepaired: {}",
                    repaired.to_wkt()
                ),
                None => {}
            }
        }
    }
}

fn run_proptest(seed: Option<u64>) {
    let cases: u32 = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let config = ProptestConfig {
        cases,
        max_shrink_iters: 4000,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let mut runner = if let Some(s) = seed {
        let mut seed_bytes = [0u8; 32];
        seed_bytes[..8].copy_from_slice(&s.to_le_bytes());
        proptest::test_runner::TestRunner::new_with_rng(
            config,
            proptest::test_runner::TestRng::from_seed(
                proptest::test_runner::RngAlgorithm::ChaCha,
                &seed_bytes,
            ),
        )
    } else {
        proptest::test_runner::TestRunner::new(config)
    };
    let strategy = any_polygon();
    runner
        .run(&strategy, |poly| {
            check_polygon(&poly, 0);
            Ok(())
        })
        .expect("differential invariants must hold");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 500,
        max_shrink_iters: 2000,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn differential_random(poly in any_polygon()) {
        check_polygon(&poly, 0);
    }
}

#[test]
fn differential_deterministic_seeds() {
    // Deterministic seeds first (the house rule): a fixed set that has
    // historically stressed degenerate paths.
    for seed in [1u64, 42, 12345, 999_999, 0xDEADBEEF] {
        run_proptest(Some(seed));
    }
    // Then an unseeded run at the PROPTEST_CASES count.
    run_proptest(None);
}

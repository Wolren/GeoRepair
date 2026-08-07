//! Unit tests for the lean line noder.
//!
//! The core assertion on every case: the noded chains must each pass the
//! validator's own simplicity predicate (`check_linestring_self_intersection`),
//! because that is the repair contract (valid-or-empty). Cross-component
//! validity is asserted through `GeoValidation::validate` on the assembled
//! MultiLineString where relevant.

use alloc::vec::Vec;

use geo::{Coord, Geometry, LineString, MultiLineString};

use crate::make_valid::MakeValid;
use crate::noding::line::node_line;
use crate::validation::impls::check_linestring_self_intersection;
use crate::validation::GeoValidation;

fn c(x: f64, y: f64) -> Coord<f64> {
    Coord { x, y }
}

fn coords(pairs: &[(f64, f64)]) -> Vec<Coord<f64>> {
    pairs.iter().map(|&(x, y)| c(x, y)).collect()
}

fn assert_chains_valid(chains: &[Vec<Coord<f64>>]) {
    assert!(!chains.is_empty(), "noder produced no chains");
    for ch in chains {
        assert!(
            !check_linestring_self_intersection(ch),
            "chain {:?} is not simple",
            ch
        );
    }
}

fn assert_mls_valid(chains: &[Vec<Coord<f64>>]) {
    let mls: MultiLineString<f64> = MultiLineString::new(
        chains
            .iter()
            .map(|ch| LineString::new(ch.clone()))
            .collect(),
    );
    let g = Geometry::MultiLineString(mls);
    assert!(g.is_valid(), "assembled MultiLineString is not valid");
}

fn node_ok(pairs: &[(f64, f64)]) -> Vec<Vec<Coord<f64>>> {
    let cs = coords(pairs);
    let chains = node_line(&cs).unwrap_or_else(|| {
        panic!("noder returned None (fell back) for input {pairs:?}");
    });
    assert_chains_valid(&chains);
    chains
}

fn node_ok_closed(pairs: &[(f64, f64)]) -> Vec<Vec<Coord<f64>>> {
    let mut p = pairs.to_vec();
    p.push(p[0]);
    let cs = coords(&p);
    let chains = node_line(&cs).unwrap_or_else(|| {
        panic!("noder returned None (fell back) for input {pairs:?}");
    });
    assert_chains_valid(&chains);
    chains
}

/// Figure-8: a closed line crossing itself once. The four arcs must share
/// the crossing node bit-exactly and be simple.
#[test]
fn figure8_crossing() {
    let chains = node_ok_closed(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]);
    assert!(chains.len() >= 4, "expected >= 4 arcs, got {}", chains.len());
    // The crossing node (5,5) is a degree-4 junction: every arc ends or
    // starts there (one arc per piece around the junction).
    let mut touch = 0;
    for ch in &chains {
        let f = ch[0];
        let l = ch[ch.len() - 1];
        if (f.x - 5.0).abs() < 1e-9 && (f.y - 5.0).abs() < 1e-9
            || (l.x - 5.0).abs() < 1e-9 && (l.y - 5.0).abs() < 1e-9
        {
            touch += 1;
        }
    }
    assert_eq!(touch, 4, "junction (5,5) must have degree 4");
    assert_mls_valid(&chains);
}

/// Adjacent out-and-back: LINESTRING(0 0, 10 0, 5 0). The doubled-back
/// segment is a collinear overlap of its neighbour; the noder must merge
/// the traversal into a single chain 0 -> 5 -> 10.
#[test]
fn out_and_back_adjacent() {
    let chains = node_ok(&[(0.0, 0.0), (10.0, 0.0), (5.0, 0.0)]);
    assert_eq!(chains.len(), 1, "expected one chain, got {chains:?}");
    let ch = &chains[0];
    assert_eq!(ch.len(), 3, "expected [0,5,10], got {ch:?}");
    assert!((ch[0].x - 0.0).abs() < 1e-12);
    assert!((ch[1].x - 5.0).abs() < 1e-12);
    assert!((ch[2].x - 10.0).abs() < 1e-12);
}

/// Non-adjacent collinear overlap: 0-10-5-15. Segments [0,10] and [5,15]
/// overlap on [5,10]; the noder must split both and dedup the doubled
/// traversal into one chain 0 -> 5 -> 10 -> 15.
#[test]
fn collinear_overlap_non_adjacent() {
    let chains = node_ok(&[(0.0, 0.0), (10.0, 0.0), (5.0, 0.0), (15.0, 0.0)]);
    assert_eq!(chains.len(), 1, "expected one chain, got {chains:?}");
    let ch = &chains[0];
    assert_eq!(ch.len(), 4, "expected [0,5,10,15], got {ch:?}");
    for (k, x) in [0.0, 5.0, 10.0, 15.0].iter().enumerate() {
        assert!((ch[k].x - x).abs() < 1e-12, "vertex {k} wrong: {ch:?}");
    }
}

/// T-junction: a vertex of one segment lies on the interior of another.
/// The horizontal must split at the junction; the vertical stays whole.
#[test]
fn t_junction_vertex_on_edge() {
    let chains = node_ok(&[(0.0, 0.0), (10.0, 0.0), (5.0, 0.0), (5.0, 10.0)]);
    // The vertical [(5,0),(5,10)] stays whole; the horizontal splits into
    // [0,5] and [5,10], each forming its own chain (the junction has
    // degree 3).
    assert_eq!(chains.len(), 3, "expected 3 chains, got {chains:?}");
    for ch in &chains {
        if (ch[0].x - 5.0).abs() < 1e-12 && (ch[0].y - 0.0).abs() < 1e-12
            && (ch[1].x - 5.0).abs() < 1e-12 && (ch[1].y - 10.0).abs() < 1e-12
        {
            assert_eq!(ch.len(), 2, "vertical must be whole: {ch:?}");
        }
    }
    assert_mls_valid(&chains);
}

/// Vertex revisit: the line passes through an existing vertex. The noder
/// may fall back (the greedy filter is the safe path), but the final
/// make_valid output must be valid.
#[test]
fn vertex_revisit_make_valid_ok() {
    let cs = coords(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 0.0), (20.0, 0.0)]);
    let g = Geometry::LineString(LineString::new(cs));
    let out = g.make_valid();
    assert!(out.is_valid(), "make_valid output must be valid");
}

/// Star-burst: five spokes meeting at the centre. Every spoke is its own
/// chain; the centre is a degree-5 junction.
#[test]
fn star_burst() {
    let chains = node_ok_closed(&[
        (10.0, 0.0),
        (0.0, 0.0),
        (7.07, 7.07),
        (0.0, 0.0),
        (-7.07, 7.07),
        (0.0, 0.0),
        (-7.07, -7.07),
        (0.0, 0.0),
        (7.07, -7.07),
    ]);
    assert_eq!(chains.len(), 5, "expected 5 spokes, got {chains:?}");
    assert_mls_valid(&chains);
}

/// Three segments crossing at nearly the same point (within the noding
/// eps of each other). All three must split at ONE canonical node, and
/// every chain must be simple.
#[test]
fn near_coincident_crossing_cluster() {
    let chains = node_ok(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (5.0, -5.0),
        (5.000000001, 5.0),
        (5.000000001, -5.0),
        (5.0, 5.0),
    ]);
    assert!(chains.len() >= 6, "expected >= 6 chains, got {chains:?}");
    assert_mls_valid(&chains);
}

/// The bench collinear-overlap shape (50 segments on y = 0 with doubled
/// coverage): the noder must reduce it to one simple chain covering the
/// full span.
#[test]
fn collinear_overlap_bench_shape() {
    let mut pairs = Vec::new();
    for i in 0..50 {
        pairs.push((i as f64 * 10.0, 0.0));
        pairs.push((i as f64 * 10.0 + 10.0, 0.0));
        if i < 49 {
            pairs.push((i as f64 * 10.0 + 5.0, 0.0));
        }
    }
    let chains = node_ok(&pairs);
    assert_eq!(chains.len(), 1, "expected one chain, got {chains:?}");
    let ch = &chains[0];
    assert!((ch[0].x - 0.0).abs() < 1e-9, "chain must start at 0: {ch:?}");
    let last = ch[ch.len() - 1].x;
    assert!((last - 500.0).abs() < 1e-9, "chain must end at 500: {ch:?}");
    // The chain must cover the span without gaps: every consecutive pair
    // is a forward step.
    for w in ch.windows(2) {
        assert!(w[1].x > w[0].x + 1e-9, "backtracking chain: {ch:?}");
    }
}

/// A lissajous 5:3 polyline. The parametrization retraces every segment
/// (t and pi - t give the same point), so the noded output is the figure
/// covered once: about half the input length, all simple.
#[test]
fn lissajous_retrace_resolved() {
    let n = 400usize;
    let mut pairs = Vec::with_capacity(n + 1);
    for i in 0..n {
        let t = 2.0 * core::f64::consts::PI * i as f64 / n as f64;
        pairs.push((100.0 * (5.0 * t).sin(), 100.0 * (3.0 * t).sin()));
    }
    let cs = coords(&pairs);
    let input_len: f64 = pairs
        .windows(2)
        .map(|w| {
            let dx = w[1].0 - w[0].0;
            let dy = w[1].1 - w[0].1;
            (dx * dx + dy * dy).sqrt()
        })
        .sum();
    let chains = node_ok(&pairs);
    let out_len: f64 = chains
        .iter()
        .flat_map(|ch| ch.windows(2))
        .map(|w| {
            let dx = w[1].x - w[0].x;
            let dy = w[1].y - w[0].y;
            (dx * dx + dy * dy).sqrt()
        })
        .sum();
    // The 5:3 lissajous on [0, 2pi] covers every line twice (t and pi-t),
    // so the valid single coverage is half the input length.
    let ratio = out_len / input_len;
    assert!(
        (ratio - 0.5).abs() < 0.05,
        "expected ~half length (retrace), got ratio {ratio} (in {input_len}, out {out_len})"
    );
    let _ = cs;
    assert_mls_valid(&chains);
}

/// make_valid on a non-simple line: the result must be valid and simple
/// (never NotSimple).
#[test]
fn make_valid_line_contract() {
    let cs = coords(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0), (0.0, 0.0)]);
    let g = Geometry::LineString(LineString::new(cs));
    let out = g.make_valid();
    assert!(out.is_valid(), "make_valid output must be valid");
}

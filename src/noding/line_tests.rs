
use alloc::vec::Vec;

use geo::{Coord, Geometry, LineString, MultiLineString};

use crate::make_valid::MakeValid;
use crate::noding::line::LineNoder;
use crate::noding::line::node_line;
use crate::validation::GeoValidation;
use crate::validation::impls::check_linestring_self_intersection;

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

#[test]
fn figure8_crossing() {
    let chains = node_ok_closed(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]);
    assert!(
        chains.len() >= 4,
        "expected >= 4 arcs, got {}",
        chains.len()
    );
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

#[test]
fn t_junction_vertex_on_edge() {
    let chains = node_ok(&[(0.0, 0.0), (10.0, 0.0), (5.0, 0.0), (5.0, 10.0)]);
    assert_eq!(chains.len(), 3, "expected 3 chains, got {chains:?}");
    for ch in &chains {
        if (ch[0].x - 5.0).abs() < 1e-12
            && (ch[0].y - 0.0).abs() < 1e-12
            && (ch[1].x - 5.0).abs() < 1e-12
            && (ch[1].y - 10.0).abs() < 1e-12
        {
            assert_eq!(ch.len(), 2, "vertical must be whole: {ch:?}");
        }
    }
    assert_mls_valid(&chains);
}

#[test]
fn vertex_revisit_make_valid_ok() {
    let cs = coords(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (0.0, 10.0),
        (10.0, 0.0),
        (20.0, 0.0),
    ]);
    let g = Geometry::LineString(LineString::new(cs));
    let out = g.make_valid();
    assert!(out.is_valid(), "make_valid output must be valid");
}

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
    assert!(
        (ch[0].x - 0.0).abs() < 1e-9,
        "chain must start at 0: {ch:?}"
    );
    let last = ch[ch.len() - 1].x;
    assert!((last - 500.0).abs() < 1e-9, "chain must end at 500: {ch:?}");
    for w in ch.windows(2) {
        assert!(w[1].x > w[0].x + 1e-9, "backtracking chain: {ch:?}");
    }
}

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
    let ratio = out_len / input_len;
    assert!(
        (ratio - 0.5).abs() < 0.05,
        "expected ~half length (retrace), got ratio {ratio} (in {input_len}, out {out_len})"
    );
    let _ = cs;
    assert_mls_valid(&chains);
}

#[test]
fn make_valid_line_contract() {
    let cs = coords(&[
        (0.0, 0.0),
        (10.0, 10.0),
        (10.0, 0.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ]);
    let g = Geometry::LineString(LineString::new(cs));
    let out = g.make_valid();
    assert!(out.is_valid(), "make_valid output must be valid");
}


fn subdivide(pairs: &[(f64, f64)], n: usize) -> Vec<Coord<f64>> {
    let edges = pairs.len() - 1;
    let lens: Vec<f64> = (0..edges)
        .map(|e| {
            let (ax, ay) = pairs[e];
            let (bx, by) = pairs[e + 1];
            ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt()
        })
        .collect();
    let total: f64 = lens.iter().sum();
    let mut out = Vec::with_capacity(n);
    let mut e = 0usize;
    let mut edge_off = 0.0f64;
    for j in 0..n {
        while e < edges && lens[e] <= 0.0 {
            e += 1;
        }
        if e >= edges {
            break;
        }
        let target = (j as f64 / n as f64) * total;
        while e + 1 < edges && edge_off + lens[e] < target {
            edge_off += lens[e];
            e += 1;
        }
        let (ax, ay) = pairs[e];
        let (bx, by) = pairs[e + 1];
        let t = if lens[e] > 0.0 {
            ((target - edge_off) / lens[e]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(c(ax + (bx - ax) * t, ay + (by - ay) * t));
    }
    out
}

fn figure8(n: usize) -> Vec<Coord<f64>> {
    subdivide(
        &[
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ],
        n,
    )
}

fn lissajous(n: usize) -> Vec<Coord<f64>> {
    let mut cs = Vec::with_capacity(n + 1);
    for i in 0..n {
        let t = 2.0 * core::f64::consts::PI * i as f64 / n as f64;
        cs.push(c(1000.0 * (5.0 * t).sin(), 1000.0 * (3.0 * t).sin()));
    }
    cs.push(cs[0]);
    cs
}

fn general_chains(cs: &[Coord<f64>]) -> Option<Vec<Vec<Coord<f64>>>> {
    let mut ln = LineNoder::new(cs);
    let n = cs.len() - 1;
    if n >= 32 && ln.revisit_dominated() {
        return None;
    }
    ln.run_full()
}

#[test]
fn crossing_only_figure8_parity() {
    for n in [100usize, 500, 1000] {
        let cs = figure8(n);
        let fast = LineNoder::new(&cs).crossing_only();
        assert!(fast.is_some(), "figure-8 {n}v must take the fast path");
        assert_eq!(
            fast,
            general_chains(&cs),
            "figure-8 {n}v fast/general outputs differ"
        );
        assert_chains_valid(fast.as_ref().expect("checked some"));
    }
}

#[test]
fn crossing_only_bowtie_parity() {
    let cs = coords(&[
        (0.0, 0.0),
        (10.0, 10.0),
        (10.0, 0.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ]);
    let fast = LineNoder::new(&cs).crossing_only();
    assert!(fast.is_some(), "bowtie must take the fast path");
    assert_eq!(
        fast,
        general_chains(&cs),
        "bowtie fast/general outputs differ"
    );
    assert_chains_valid(fast.as_ref().expect("checked some"));
}

#[test]
fn crossing_only_bails_on_lissajous_eps_classes() {
    let cs = lissajous(500);
    assert!(
        LineNoder::new(&cs).crossing_only().is_none(),
        "lissajous has 1-ulp anchor pairs; the fast path must bail"
    );
    let general = general_chains(&cs);
    assert!(general.is_some());
    assert_chains_valid(general.as_ref().expect("checked some"));
}

#[test]
fn crossing_only_bails_on_out_and_back() {
    let cs = coords(&[(0.0, 0.0), (10.0, 0.0), (2.0, 0.0), (2.0, 10.0)]);
    assert!(
        LineNoder::new(&cs).crossing_only().is_none(),
        "collinear backtrack must bail to the general path"
    );
}

#[test]
fn crossing_only_splits_vertex_on_edge() {
    let cs = coords(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (5.0, 0.0),
        (5.0, 10.0),
    ]);
    // (5.0, 0.0) rests strictly inside segment (0,0)-(10,0). 1000v +60% locally / +93%
    // CI: every wave-vertex-on-chord contact tripped it (fuzz crash-9c50dad6 fix,
    // 64c2b58).
    let chains = LineNoder::new(&cs)
        .crossing_only()
        .expect("vertex-on-edge now splits the host segment instead of bailing");
    assert_chains_valid(&chains);
    let v = c(5.0, 0.0);
    assert!(
        chains
            .iter()
            .any(|ch| ch.first() == Some(&v) || ch.last() == Some(&v)),
        "split point (5,0) must terminate a chain: {chains:?}"
    );
}

#[test]
fn crossing_only_simple_line_parity() {
    let mut cs = Vec::new();
    for i in 0..200 {
        let t = i as f64 * 0.3;
        let r = 50.0 + t * 30.0;
        cs.push(c(r * t.cos(), r * t.sin()));
    }
    let fast = LineNoder::new(&cs).crossing_only();
    assert!(fast.is_some(), "simple line must take the fast path");
    assert_eq!(fast, general_chains(&cs));
}

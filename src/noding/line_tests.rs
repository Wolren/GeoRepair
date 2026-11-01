//! The core assertion on every case: the noded chains must each pass the validator's
//! own simplicity predicate (`check_linestring_self_intersection`).

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

/// Figure-8: a closed line crossing itself once.
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

/// Adjacent out-and-back: linestring(0 0, 10 0, 5 0).
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

/// Non-adjacent collinear overlap: 0-10-5-15.
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

/// Vertex revisit: the line passes through an existing vertex.
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

/// Star-burst: five spokes meeting at the centre.
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

/// Three segments crossing at nearly the same point (within the noding eps of each
/// other).
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

/// The bench collinear-overlap shape (50 segments on y = 0 with doubled coverage): the
/// noder must reduce it to one simple chain covering the full span.
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

/// A lissajous 5:3 polyline.
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

/// make_valid on a non-simple line: the result must be valid and simple (never
/// NotSimple).
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

/// Subdivide a closed coordinate loop to exactly n vertices by arc length (the bench's
/// figure-8 construction).
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

/// The general path on a fresh noder, reached exactly as `run` reaches it when the fast
/// path bails (revisit pre-scan, then `run_full`).
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

// --- node_fast differential -------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn dedup_consecutive(cs: &[Coord<f64>]) -> Vec<Coord<f64>> {
    let mut out: Vec<Coord<f64>> = Vec::with_capacity(cs.len());
    for &c in cs {
        if out.last() != Some(&c) {
            out.push(c);
        }
    }
    out
}

/// Random line shapes that exercise every branch of the lean path.
fn random_line(rng: &mut Rng) -> Vec<Coord<f64>> {
    let n = 3 + rng.below(48);
    let kind = rng.below(7);
    let mut cs: Vec<Coord<f64>> = Vec::with_capacity(n + 1);
    match kind {
        0 => {
            // integer-lattice walk: exact revisits and shared endpoints
            let mut x = rng.below(8) as f64;
            let mut y = rng.below(8) as f64;
            for _ in 0..n {
                cs.push(c(x, y));
                x += rng.below(5) as f64 - 2.0;
                y += rng.below(5) as f64 - 2.0;
            }
            cs.push(c(x, y));
        }
        1 => {
            // continuous random walk
            let mut x = 0.0;
            let mut y = 0.0;
            for _ in 0..n {
                cs.push(c(x, y));
                x += rng.f(-3.0, 3.0);
                y += rng.f(-3.0, 3.0);
            }
            cs.push(c(x, y));
        }
        2 => {
            // closed ring
            let cx = rng.f(-10.0, 10.0);
            let cy = rng.f(-10.0, 10.0);
            for i in 0..n {
                let t = i as f64 / n as f64 * core::f64::consts::TAU;
                let r = rng.f(1.0, 20.0);
                cs.push(c(cx + r * t.cos(), cy + r * t.sin()));
            }
            cs.push(cs[0]);
        }
        3 => {
            // star: every segment from one anchor
            let cx = rng.f(-5.0, 5.0);
            let cy = rng.f(-5.0, 5.0);
            for _ in 0..n {
                cs.push(c(cx, cy));
                cs.push(c(cx + rng.f(-10.0, 10.0), cy + rng.f(-10.0, 10.0)));
            }
        }
        4 => {
            // collinear run with a kink
            let y = rng.f(-2.0, 2.0);
            for i in 0..n {
                cs.push(c(i as f64, y));
            }
            cs.push(c(rng.f(-5.0, 5.0), rng.f(-5.0, 5.0)));
        }
        5 => {
            // monotone on one axis with sub-eps steps on the other: the eps gates see an
            // overlap that an exact monotone comparison misses (wkb smoke seed 1299f6ac)
            let jitter = 1e-15;
            for i in 0..n {
                cs.push(c(i as f64, rng.f(-jitter, jitter)));
            }
            if rng.below(2) == 0 {
                cs.push(c(rng.f(-1.0, 1.0), rng.f(-jitter, jitter)));
            }
        }
        _ => {
            // near-coincident perturbations
            let mut x = 0.0;
            let mut y = 0.0;
            for _ in 0..n {
                cs.push(c(x, y));
                x += rng.f(-2.0, 2.0);
                y += rng.f(-2.0, 2.0);
            }
            cs.push(c(x, y));
            if cs.len() > 4 {
                let k = rng.below(cs.len() - 2);
                let j = k + 2 + rng.below(cs.len() - k - 2);
                cs[j] = c(cs[k].x + 1e-13, cs[k].y);
            }
        }
    }
    if kind == 4 && rng.below(2) == 0 {
        for v in cs.iter_mut() {
            v.x *= 1e9;
            v.y *= 1e9;
        }
    }
    dedup_consecutive(&cs)
}

#[test]
fn node_fast_matches_general_noder() {
    use crate::noding::line::{Fast, node_fast};
    let mut rng = Rng(0x2026_1031_5eed_1234);
    let mut counts = [0usize; 3];
    for _ in 0..20000 {
        let cs = random_line(&mut rng);
        if cs.len() < 3 {
            continue;
        }
        let old = node_line(&cs);
        match node_fast(&cs) {
            Fast::Simple => {
                counts[0] += 1;
                assert!(
                    !check_linestring_self_intersection(&cs),
                    "node_fast called a non-simple line simple: {cs:?}"
                );
            }
            Fast::Noded(chains) => {
                counts[1] += 1;
                assert!(!chains.is_empty(), "no chains: {cs:?}");
                assert_chains_valid(&chains);
                if let Some(old) = &old {
                    assert_eq!(&chains, old, "lean/general outputs differ: {cs:?}");
                }
            }
            Fast::Unsupported => counts[2] += 1,
        }
    }
    assert!(counts[0] > 1000, "too few simple verdicts: {counts:?}");
    assert!(counts[1] > 1000, "too few noded verdicts: {counts:?}");
}

/// The monotone exit must not fire on sub-eps steps: y here rises by denormal amounts, so
/// the line is monotone to an exact comparison while its last vertex returns within eps of
/// the first (wkb smoke seed crash-1299f6ac).
#[test]
fn monotone_exit_skips_sub_eps_steps() {
    use crate::noding::line::{Fast, node_fast};
    let denormal = [
        (0.0, 0.0),
        (2.000000005529728, 1e-322),
        (3.48092189549943e-310, 3.484317088765e-310),
    ];
    let cs = coords(&denormal);
    assert!(
        !matches!(node_fast(&cs), Fast::Simple),
        "sub-eps monotone line reported simple: {cs:?}"
    );
    assert_mls_valid(&node_ok(&denormal));

    // Same class at a normal scale: every y step is under eps while x moves 20 units.
    let swing_pairs = [(0.0, 0.0), (10.0, 1e-13), (20.0, 2e-13), (0.0, 3e-13)];
    let swing = coords(&swing_pairs);
    assert!(
        !matches!(node_fast(&swing), Fast::Simple),
        "sub-eps swing reported simple: {swing:?}"
    );
    assert_mls_valid(&node_ok(&swing_pairs));
}

/// The shortcut itself must survive: a line that moves more than eps per step on one axis
/// takes the fast exit, which is the whole cost of a valid line.
#[test]
fn monotone_exit_fires_above_eps() {
    use crate::noding::line::{Fast, node_fast};
    let cs = coords(&[(0.0, 0.0), (10.0, 1.0), (20.0, 2.0)]);
    assert!(
        matches!(node_fast(&cs), Fast::Simple),
        "monotone line must take the fast exit"
    );
}

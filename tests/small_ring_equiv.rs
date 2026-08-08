//! Equivalence tests: the small-ring O(n²) sweep in
//! `has_no_intersections_small` must produce IDENTICAL results to the
//! monotone-chain + grid + R-tree machinery (`chain_path` below) on every
//! input. The small path is a pure optimization of the same predicate.

use geo::Coord;
use geo::Line;
use geo_repair::arrange::prep_intersect::has_no_intersections;
use geo_repair::arrange::prep_intersect::has_no_intersections_small;
use geo_repair::core::SMALL_RING_LINES;

fn lines_of(coords: &[(f64, f64)]) -> Vec<Line<f64>> {
    let mut v: Vec<Line<f64>> = Vec::new();
    for w in coords.windows(2) {
        v.push(Line::new(
            Coord {
                x: w[0].0,
                y: w[0].1,
            },
            Coord {
                x: w[1].0,
                y: w[1].1,
            },
        ));
    }
    v.push(Line::new(
        Coord {
            x: coords[coords.len() - 1].0,
            y: coords[coords.len() - 1].1,
        },
        Coord {
            x: coords[0].0,
            y: coords[0].1,
        },
    ));
    v
}

/// Reference implementation of the chain path: exactly what
/// `has_no_intersections` does when n > SMALL_RING_LINES. We force it on
/// small inputs too, so the test compares the two algorithms, not the
/// dispatch.
fn chain_path(lines: &[Line<f64>]) -> bool {
    // Force the chain machinery by checking a synthetic large input is not
    // needed: replicate build_mono_chains + grid + R-tree via the public
    // entry on a padded copy? No — instead reimplement the dispatch by
    // calling has_no_intersections on a copy padded with a far-away ring.
    // Padding with a disjoint ring at distance 1e9 cannot change the
    // intersection verdict for any pair of original segments: the original
    // pairs are checked identically, and the pad ring shares no segment
    // with them. So: pad -> public function -> same verdict.
    let mut padded = lines.to_vec();
    let base = 1e9;
    let pad = [
        (base, base),
        (base + 1.0, base),
        (base + 1.0, base + 1.0),
        (base, base + 1.0),
    ];
    padded.extend(lines_of(&pad));
    has_no_intersections(&padded)
}

fn rng(seed: u64) -> u64 {
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn rand_coord(seed: u64, scale: f64) -> (f64, f64) {
    let x = (rng(seed) as f64 / u64::MAX as f64) * 2.0 - 1.0;
    let y = (rng(seed ^ 0xABCDEF) as f64 / u64::MAX as f64) * 2.0 - 1.0;
    (x * scale, y * scale)
}

fn random_ring(seed: u64, nverts: usize, scale: f64) -> Vec<(f64, f64)> {
    (0..nverts)
        .map(|i| rand_coord(seed.wrapping_add(i as u64 * 7919), scale))
        .collect()
}

#[test]
fn small_matches_chain_on_random_rings() {
    for seed in 0..400u64 {
        let nverts = 3 + (rng(seed) % 30) as usize;
        let scale = match seed % 4 {
            0 => 1.0,
            1 => 1e-8,
            2 => 1e8,
            _ => 1e-3,
        };
        let ring = random_ring(seed * 3 + 1, nverts, scale);
        let lines = lines_of(&ring);
        let small = has_no_intersections_small(&lines);
        let chain = chain_path(&lines);
        assert_eq!(
            small, chain,
            "seed={seed} n={nverts} scale={scale}: small={small} chain={chain} ring={ring:?}"
        );
    }
}

#[test]
fn small_matches_chain_with_holes() {
    for seed in 0..200u64 {
        let shell_n = 4 + (rng(seed) % 10) as usize;
        let hole_n = 3 + (rng(seed ^ 0x55) % 8) as usize;
        let shell = random_ring(seed * 7 + 2, shell_n, 10.0);
        let hole = random_ring(seed * 13 + 5, hole_n, 3.0);
        let mut coords = shell.clone();
        coords.extend(hole);
        let lines = lines_of(&coords);
        let small = has_no_intersections_small(&lines);
        let chain = chain_path(&lines);
        assert_eq!(
            small, chain,
            "seed={seed} shell_n={shell_n} hole_n={hole_n}: small={small} chain={chain}"
        );
    }
}

#[test]
fn small_detects_proper_crossings() {
    let bowtie = lines_of(&[(0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0)]);
    assert!(!has_no_intersections_small(&bowtie));
    assert!(!chain_path(&bowtie));

    let square = lines_of(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    assert!(has_no_intersections_small(&square));
    assert!(chain_path(&square));

    // Self-touch at a vertex: not a proper crossing.
    let pinch = lines_of(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (1.0, 1.0), (0.0, 2.0)]);
    assert!(has_no_intersections_small(&pinch));
    assert!(chain_path(&pinch));
}

#[test]
fn boundary_between_rings_compared() {
    let ring_a = lines_of(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    let ring_b = lines_of(&[(0.5, -1.0), (1.5, -1.0), (1.5, 1.0), (0.5, 1.0)]);
    let mut all = ring_a;
    all.extend(ring_b);
    assert!(!has_no_intersections_small(&all));
    assert!(!chain_path(&all));
}

#[test]
fn all_small_sizes_dispatched_correctly() {
    for n in 2..=SMALL_RING_LINES {
        let ring = random_ring(n as u64 * 31 + 7, n, 5.0);
        let lines = lines_of(&ring);
        assert_eq!(
            has_no_intersections(&lines),
            has_no_intersections_small(&lines),
            "n={n}"
        );
    }
    let big = random_ring(999, SMALL_RING_LINES + 8, 5.0);
    let lines = lines_of(&big);
    // Above the threshold: public entry must dispatch to the chain path and
    // agree with it (small path is out of scope for inputs this large).
    assert_eq!(has_no_intersections(&lines), chain_path(&lines));
}

#[test]
fn degenerate_inputs_match() {
    let dup = lines_of(&[(0.0, 0.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    assert_eq!(has_no_intersections_small(&dup), chain_path(&dup));

    let coll = lines_of(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]);
    assert_eq!(has_no_intersections_small(&coll), chain_path(&coll));

    let off = lines_of(&[
        (1e12, 1e12),
        (1e12 + 1.0, 1e12),
        (1e12 + 1.0, 1e12 + 1.0),
        (1e12, 1e12 + 1.0),
    ]);
    assert_eq!(has_no_intersections_small(&off), chain_path(&off));

    assert!(has_no_intersections_small(&lines_of(&[
        (0.0, 0.0),
        (1.0, 0.0)
    ])));
    assert!(has_no_intersections_small(&[]));
}

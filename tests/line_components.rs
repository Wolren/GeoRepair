//! MultiLineString repair keeps every component: point contacts are noded, duplicates are
//! deduplicated, and no part is dropped for touching another.
use geo::{Coord, Geometry, LineString, MultiLineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig};

fn ls(points: &[(f64, f64)]) -> LineString<f64> {
    LineString::new(points.iter().map(|&(x, y)| Coord { x, y }).collect())
}

fn mls(parts: &[&[(f64, f64)]]) -> MultiLineString<f64> {
    MultiLineString::new(parts.iter().map(|p| ls(p)).collect())
}

fn length(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::LineString(l) => {
            l.0.windows(2)
                .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
                .sum()
        }
        Geometry::MultiLineString(m) => {
            m.0.iter()
                .map(|l| length(&Geometry::LineString(l.clone())))
                .sum()
        }
        Geometry::GeometryCollection(gc) => gc.0.iter().map(length).sum(),
        _ => 0.0,
    }
}

fn components(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::LineString(_) => 1,
        Geometry::MultiLineString(m) => m.0.len(),
        Geometry::GeometryCollection(gc) => gc.0.len(),
        _ => 1,
    }
}

fn repair(g: &Geometry<f64>) -> Geometry<f64> {
    g.make_valid_with_config(&MakeValidConfig::default())
}

#[test]
fn crossing_parts_are_noded_not_dropped() {
    // Two parts cross at (50, 0): each is split there and all four pieces stay.
    let g = Geometry::MultiLineString(mls(&[
        &[(0.0, 0.0), (100.0, 0.0)],
        &[(50.0, -50.0), (50.0, 50.0)],
    ]));
    let out = repair(&g);
    assert!(out.validate().valid, "noded output must be valid: {out:?}");
    assert_eq!(
        components(&out),
        4,
        "both parts split at the crossing: {out:?}"
    );
    assert!(
        (length(&out) - 200.0).abs() < 1e-9,
        "length must survive: {out:?}"
    );
}

#[test]
fn endpoint_on_part_is_noded() {
    // Part 2 ends in the middle of part 1: part 1 is split there and the contact becomes
    // an endpoint of every piece.
    let g = Geometry::MultiLineString(mls(&[
        &[(0.0, 0.0), (100.0, 0.0)],
        &[(50.0, 50.0), (50.0, 0.0)],
    ]));
    let want = length(&g);
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(components(&out), 3, "part 1 split at (50, 0): {out:?}");
    assert!(
        (length(&out) - want).abs() < 1e-9,
        "length must survive: {out:?}"
    );
}

#[test]
fn identical_parts_are_deduplicated() {
    let g = Geometry::MultiLineString(mls(&[
        &[(0.0, 0.0), (100.0, 0.0)],
        &[(0.0, 0.0), (100.0, 0.0)],
    ]));
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(
        components(&out),
        1,
        "the duplicate is dropped, not the geometry: {out:?}"
    );
    assert!((length(&out) - 100.0).abs() < 1e-9, "length: {out:?}");
}

#[test]
fn partial_collinear_overlap_keeps_the_union_length() {
    // 0..100 and 50..200 share 50..100 once.
    let g = Geometry::MultiLineString(mls(&[
        &[(0.0, 0.0), (100.0, 0.0)],
        &[(50.0, 0.0), (200.0, 0.0)],
    ]));
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert!(
        (length(&out) - 200.0).abs() < 1e-9,
        "union length 200: {out:?}"
    );
}

#[test]
fn parts_meeting_at_endpoints_are_unchanged() {
    let g = Geometry::MultiLineString(mls(&[
        &[(0.0, 0.0), (100.0, 0.0)],
        &[(0.0, 0.0), (50.0, 50.0)],
    ]));
    assert!(g.validate().valid, "endpoint contact is valid");
    let out = repair(&g);
    assert_eq!(components(&out), 2, "nothing to do: {out:?}");
    assert!((length(&out) - (100.0 + 50.0f64.hypot(50.0))).abs() < 1e-9);
}

#[test]
fn near_duplicate_chains_are_kept() {
    // 1e-11 apart along a diagonal: twice a segment area is 2e-14, below eps, but the
    // distance is 7x eps. Distance decides, so both chains stay.
    let chain = |dy: f64| -> Vec<(f64, f64)> {
        (0..=4)
            .map(|i| {
                (
                    146.28 + 0.002 * f64::from(i),
                    -36.95 + 0.002 * f64::from(i) + dy,
                )
            })
            .collect()
    };
    let g = Geometry::MultiLineString(mls(&[&chain(0.0), &chain(1e-11)]));
    assert!(g.validate().valid, "two disjoint chains are valid");
    let want = length(&g);
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(
        components(&out),
        2,
        "the near-duplicate chain is kept: {out:?}"
    );
    assert!(
        (length(&out) - want).abs() < 1e-9,
        "length must survive: {out:?}"
    );
}

#[test]
fn interior_vertex_on_a_part_is_noded_on_both_sides() {
    // (1e-9, 0) is a vertex of part 2 that sits inside part 1, and part 2 retraces
    // 1e-9 of part 1. Both sides must split there: nothing may be dropped.
    let g = Geometry::MultiLineString(mls(&[
        &[(0.0, 0.0), (10.0, 0.0)],
        &[(10.0, 0.0), (5.0, 2.0), (1e-9, 0.0), (0.0, 0.0)],
    ]));
    assert!(!g.validate().valid, "the retrace makes the input invalid");
    let want = length(&g) - 1e-9;
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(
        components(&out),
        3,
        "both parts split, one duplicate piece dropped: {out:?}"
    );
    assert!(
        (length(&out) - want).abs() < 1e-9,
        "only the retraced 1e-9 may go: {out:?}"
    );
}

#[test]
fn closed_part_touching_an_open_part_keeps_coverage() {
    // A closed component has no boundary, so it cannot touch anything; it is cut into two
    // open paths at the contact point, and no segment is lost.
    let g = Geometry::MultiLineString(mls(&[
        &[(10.0, 10.0), (20.0, 20.0)],
        &[
            (20.0, 20.0),
            (20.0, 30.0),
            (30.0, 30.0),
            (30.0, 20.0),
            (20.0, 20.0),
        ],
    ]));
    let want = length(&g);
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(
        components(&out),
        3,
        "the closed square becomes two open paths: {out:?}"
    );
    assert!(
        (length(&out) - want).abs() < 1e-9,
        "length must survive: {out:?}"
    );
}

#[test]
fn retrace_ring_is_invalid_and_repaired() {
    // A ring that runs back along part of its own previous edge: invalid, and the repair
    // must drop the retrace, not keep it.
    let ring = ls(&[
        (0.0, 5.0),
        (999901360038144.0, 1.5),
        (0.0, 589338232487936.0),
        (0.0, -5.500000000232603),
        (0.0, 5.0),
    ]);
    let g = Geometry::Polygon(Polygon::new(ring, Vec::new()));
    assert!(!g.validate().valid, "the retrace must be rejected");
    let out = repair(&g);
    assert!(
        out.validate().valid,
        "repair must produce a valid ring: {out:?}"
    );
    assert!(
        length_polygon_ring(&out) > 0.0,
        "the thin sliver survives: {out:?}"
    );
}

fn length_polygon_ring(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => p.exterior().0.len() as f64,
        Geometry::MultiPolygon(m) => m.0.iter().map(|p| p.exterior().0.len() as f64).sum(),
        _ => 0.0,
    }
}

/// Out-and-back spikes from the origin, the bench shape. Every spike retraces its own
/// outbound edge, so the distinct linework is one radius per spike.
fn star_line(spikes: usize, r: f64) -> LineString<f64> {
    let mut cs = Vec::with_capacity(2 * spikes + 1);
    for i in 0..spikes {
        let a = 2.0 * core::f64::consts::PI * i as f64 / spikes as f64;
        cs.push(Coord { x: 0.0, y: 0.0 });
        cs.push(Coord {
            x: r * a.cos(),
            y: r * a.sin(),
        });
    }
    cs.push(Coord { x: 0.0, y: 0.0 });
    LineString::new(cs)
}

/// Measured 2026-11-17: at 100 spikes the repair returned 2 parts and 2,000 of the
/// 100,000 distinct units, a 98% loss with a GEOS-valid output.
fn segments_of(g: &Geometry<f64>) -> Vec<(Coord<f64>, Coord<f64>)> {
    let mut out = Vec::new();
    match g {
        Geometry::LineString(l) => {
            for w in l.0.windows(2) {
                out.push((w[0], w[1]));
            }
        }
        Geometry::MultiLineString(m) => {
            for l in &m.0 {
                for w in l.0.windows(2) {
                    out.push((w[0], w[1]));
                }
            }
        }
        Geometry::GeometryCollection(gc) => {
            for p in &gc.0 {
                out.extend(segments_of(p));
            }
        }
        _ => {}
    }
    out
}

fn point_segment_distance(p: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// Every input segment midpoint lies on the output within the repair eps.
fn assert_covered(g_in: &Geometry<f64>, g_out: &Geometry<f64>) {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (a, b) in segments_of(g_in) {
        min_x = min_x.min(a.x).min(b.x);
        max_x = max_x.max(a.x).max(b.x);
        min_y = min_y.min(a.y).min(b.y);
        max_y = max_y.max(a.y).max(b.y);
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * scale;
    let out = segments_of(g_out);
    for (a, b) in segments_of(g_in) {
        if a == b {
            continue;
        }
        let mid = Coord {
            x: (a.x + b.x) * 0.5,
            y: (a.y + b.y) * 0.5,
        };
        let d = out
            .iter()
            .map(|&(c, d)| point_segment_distance(mid, c, d))
            .fold(f64::INFINITY, f64::min);
        assert!(
            d <= eps,
            "input midpoint at ({}, {}) is {d} from the output, eps {eps}: {g_out:?}",
            mid.x,
            mid.y
        );
    }
}

#[test]
fn star_burst_keeps_every_spike() {
    for spikes in [16usize, 100, 1000] {
        let g = Geometry::LineString(star_line(spikes, 1000.0));
        assert!(
            !g.validate().valid,
            "a self-intersecting line is invalid here"
        );
        let out = repair(&g);
        assert!(
            out.validate().valid,
            "spikes={spikes}: output must be valid: {out:?}"
        );
        assert_covered(&g, &out);
        let want = spikes as f64 * 1000.0;
        let got = length(&out);
        assert!(
            (got - want).abs() <= 1e-9 * want,
            "spikes={spikes}: distinct input length {want}, output {got}: {out:?}"
        );
        assert_eq!(
            components(&out),
            spikes,
            "spikes={spikes}: one component per spike: {out:?}"
        );
    }
}

/// Measured 2026-11-17: the repair dropped the b->c->d loop and kept 99.947% of the
/// line, a valid output with 5.8e81 of linework missing.
#[test]
fn extreme_span_line_keeps_the_loop() {
    let g = Geometry::LineString(ls(&[
        (3.91211143701096e-310, 2.924099431164646e-225),
        (5.256633473081384e83, 3.8e-321),
        (5.370934160209324e83, 5.256633473081384e83),
        (5.256633479043941e83, 1.0612611946e-313),
        (5.368649357466e-312, 9.946459985351758e86),
        (5.183966989559614e83, 1.5181983059899793e82),
    ]));
    let want = length(&g);
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_covered(&g, &out);
    let got = length(&out);
    assert!(
        got >= 0.999 * want,
        "the whole loop must survive: input {want}, output {got}: {out:?}"
    );
}

pub mod assemble;
pub mod cdt;
pub mod extract;
pub mod label;
pub mod prep;

use crate::config::MakeValidConfig;
use geo::{Geometry, GeometryCollection, LinesIter, MultiPolygon, Polygon};
use rustc_hash::FxHashSet;
use spade::Triangulation;

pub(crate) fn fix_polygon(poly: &Polygon<f64>, _config: &MakeValidConfig) -> Geometry<f64> {
    if !poly_has_basic_form(poly) {
        let lines: Vec<_> = poly.lines_iter().collect();
        if lines.is_empty() {
            return empty();
        }
        return match fix_from_lines(lines) {
            Some(mp) => Geometry::MultiPolygon(mp),
            None => empty(),
        };
    }
    let lines: Vec<_> = poly.lines_iter().collect();
    if lines.is_empty() {
        return empty();
    }
    if prep::has_no_intersections(&lines) && holes_are_valid(poly) {
        return Geometry::Polygon(poly.clone());
    }
    match fix_from_lines(lines) {
        Some(mp) => Geometry::MultiPolygon(mp),
        None => empty(),
    }
}

/// Validate a polygon against all OGC validity rules relevant to our pipeline.
///
/// Checks: ring closure & min points, non-finite coords, no self-intersections,
/// hole containment, and no nested/overlapping holes.
pub fn validate_polygon(poly: &Polygon<f64>) -> bool {
    if !poly_has_basic_form(poly) {
        return false;
    }
    for ring in std::iter::once(poly.exterior()).chain(poly.interiors().iter()) {
        if ring_has_non_finite(ring) {
            return false;
        }
    }
    let lines: Vec<_> = poly.lines_iter().collect();
    if !prep::has_no_intersections(&lines) {
        return false;
    }
    if poly.interiors().is_empty() {
        return true;
    }
    holes_are_valid(poly)
}

fn ring_has_non_finite(ring: &geo::LineString<f64>) -> bool {
    ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
}

/// Lightweight check: hole containment + nesting.
/// Used after [`has_no_intersections`] for the fast path.
pub(crate) fn holes_are_valid(poly: &Polygon<f64>) -> bool {
    let shell = poly.exterior();
    let shell_coords = &shell.0;
    for hole in poly.interiors() {
        let Some(pt) = hole.0.first().copied() else {
            return false;
        };
        if !point_in_ring_exclusive(pt, shell_coords) {
            return false;
        }
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for c in &hole.0 {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        if (max_x - min_x).abs() < f64::EPSILON * scale
            || (max_y - min_y).abs() < f64::EPSILON * scale
        {
            return false;
        }
    }
    let holes: Vec<_> = poly.interiors().iter().map(|h| &h.0).collect();
    for (i, h1) in holes.iter().enumerate() {
        for h2 in holes.iter().skip(i + 1) {
            let Some(pt) = h2.first().copied() else {
                continue;
            };
            if point_in_ring_exclusive(pt, h1) {
                return false;
            }
        }
    }
    true
}

/// Winding-number point-in-ring test (strict interior, boundary counts as outside).
fn point_in_ring_exclusive(pt: geo::Coord<f64>, ring: &[geo::Coord<f64>]) -> bool {
    let n = ring.len();
    let mut wn = 0i32;
    for i in 0..n - 1 {
        let p1 = ring[i];
        let p2 = ring[i + 1];
        if p1.y <= pt.y {
            if p2.y > pt.y && orient2d(p1, p2, pt) > 0.0 {
                wn += 1;
            }
        } else if p2.y <= pt.y && orient2d(p1, p2, pt) < 0.0 {
            wn -= 1;
        }
    }
    wn != 0
}

fn orient2d(p1: geo::Coord<f64>, p2: geo::Coord<f64>, p3: geo::Coord<f64>) -> f64 {
    (p2.x - p1.x) * (p3.y - p1.y) - (p3.x - p1.x) * (p2.y - p1.y)
}

/// Timing breakdown for the CDT arrange pipeline.
/// Only available with `--features bench-geos`.
#[cfg(feature = "bench-geos")]
#[derive(Default, Debug)]
pub struct ArrangeTiming {
    pub prep_secs: f64,
    pub cdt_build_secs: f64,
    pub cdt_faces: usize,
    pub label_secs: f64,
    pub extract_secs: f64,
    pub total_secs: f64,
}

#[cfg(feature = "bench-geos")]
pub fn diagnose_arrange(poly: &Polygon<f64>) -> Option<ArrangeTiming> {
    use std::time::Instant;
    let mut t = ArrangeTiming::default();

    let lines: Vec<_> = poly.lines_iter().collect();
    if lines.is_empty() {
        return None;
    }

    let start = Instant::now();
    let prepared = prep::prepare_lines(lines).ok()?;
    t.prep_secs = start.elapsed().as_secs_f64();

    let start = Instant::now();
    let cdt = cdt::build(&prepared).ok()?;
    t.cdt_build_secs = start.elapsed().as_secs_f64();
    t.cdt_faces = cdt.num_inner_faces();

    let start = Instant::now();
    let interior = label::label_faces(&cdt);
    t.label_secs = start.elapsed().as_secs_f64();

    let start = Instant::now();
    let _raw_rings = extract::trace_rings(&cdt, &interior);
    t.extract_secs = start.elapsed().as_secs_f64();

    t.total_secs = t.prep_secs + t.cdt_build_secs + t.label_secs + t.extract_secs;
    Some(t)
}

pub(crate) fn poly_has_basic_form(poly: &Polygon<f64>) -> bool {
    fn ring_is_plausible(ring: &geo::LineString<f64>) -> bool {
        let coords = &ring.0;
        if coords.len() < 4 || coords.first() != coords.last() {
            return false;
        }
        for w in coords.windows(2) {
            if w[0] == w[1] {
                return false;
            }
        }
        let n = coords.len() - 1;
        let mut seen = FxHashSet::with_capacity_and_hasher(n, Default::default());
        for c in &coords[..n] {
            if !seen.insert((c.x.to_bits(), c.y.to_bits())) {
                return false;
            }
        }
        true
    }
    if !ring_is_plausible(poly.exterior()) {
        return false;
    }
    poly.interiors().iter().all(|h| ring_is_plausible(h))
}

pub(crate) fn fix_multi_polygon(
    mp: &MultiPolygon<f64>,
    _config: &MakeValidConfig,
) -> Geometry<f64> {
    let lines: Vec<_> = mp.iter().flat_map(|p| p.lines_iter()).collect();
    if lines.is_empty() {
        return empty();
    }
    match fix_from_lines(lines) {
        Some(mp) => Geometry::MultiPolygon(mp),
        None => empty(),
    }
}

pub(crate) fn fix_from_lines(lines: Vec<geo::Line<f64>>) -> Option<MultiPolygon<f64>> {
    let prepared = prep::prepare_lines(lines).ok()?;
    let cdt = cdt::build(&prepared).ok()?;
    if cdt.num_inner_faces() == 0 {
        return Some(MultiPolygon::new(Vec::new()));
    }
    let interior = label::label_faces(&cdt);
    if interior.is_empty() {
        return Some(MultiPolygon::new(Vec::new()));
    }
    let raw_rings = extract::trace_rings(&cdt, &interior);
    let rings: Vec<_> = raw_rings
        .into_iter()
        .flat_map(|r| extract::split_ring_at_pinch_points(r))
        .map(geo::LineString::new)
        .collect();
    Some(assemble::assemble_polygons(rings))
}

fn empty() -> Geometry<f64> {
    Geometry::GeometryCollection(GeometryCollection(Vec::new()))
}

//! Even-odd area composition behind polygon repair.

use alloc::vec::Vec;
use geo::{Coord, Line, LinesIter, MultiPolygon, Polygon};

use crate::core::Tuning;
use crate::structure::build_area::build_area_with_boundary;

/// Ring segments in walk order; zero-length steps are skipped.
pub fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords
        .windows(2)
        .filter(|w| w[0] != w[1])
        .map(|w| Line::new(w[0], w[1]))
        .collect()
}

/// Even-odd interior of an edge set, as disjoint polygons.
pub fn make_valid_poly_symdiff(cut_edges: &[Line<f64>]) -> Vec<Polygon<f64>> {
    build_area_with_boundary(cut_edges)
        .map(|(area, _)| area.0)
        .unwrap_or_default()
}

/// Even-odd repair of one polygon boundary; `None` when it bounds no area.
pub fn single_pass_fix(poly: &Polygon<f64>) -> Option<MultiPolygon<f64>> {
    single_pass_fix_tuned(poly, &Tuning::default())
}

/// Tuned variant of [`single_pass_fix`]; splits crossings with the robust noder.
pub(crate) fn single_pass_fix_tuned(
    poly: &Polygon<f64>,
    tuning: &Tuning,
) -> Option<MultiPolygon<f64>> {
    let mut lines: Vec<Line<f64>> = Vec::new();
    lines.extend(edges_from_coords(&poly.exterior().0));
    for hole in poly.interiors() {
        lines.extend(edges_from_coords(&hole.0));
    }
    if lines.is_empty() {
        return None;
    }
    let snapped = crate::arrange::prep::snap_endpoints(lines);
    if snapped.is_empty() {
        return None;
    }
    let noded = crate::structure::edge_split::split_edges_tuned(&snapped, tuning);
    if noded.is_empty() {
        return None;
    }
    let area = crate::structure::polygonizer::even_odd_area_noded(&noded)?.0;
    if area.0.is_empty() { None } else { Some(area) }
}

/// Symmetric difference of two polygon sets, as disjoint polygons.
pub fn symdiff_test(a: &[Polygon<f64>], b: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    let mut lines: Vec<Line<f64>> = Vec::new();
    for poly in a.iter().chain(b) {
        lines.extend(poly.lines_iter());
    }
    make_valid_poly_symdiff(&lines)
}

/// Undirected segment key: -0.0 folds to 0.0 and the smaller endpoint comes first.
pub(crate) fn segment_key(a: Coord<f64>, b: Coord<f64>) -> (u64, u64, u64, u64) {
    let a = fold(a);
    let b = fold(b);
    let (lo, hi) = match a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y)) {
        ::core::cmp::Ordering::Greater => (b, a),
        _ => (a, b),
    };
    (
        lo.x.to_bits(),
        lo.y.to_bits(),
        hi.x.to_bits(),
        hi.y.to_bits(),
    )
}

#[inline]
fn fold(c: Coord<f64>) -> Coord<f64> {
    Coord {
        x: if c.x == 0.0 { 0.0 } else { c.x },
        y: if c.y == 0.0 { 0.0 } else { c.y },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Area, LineString};

    fn ring(coords: &[(f64, f64)]) -> Vec<Line<f64>> {
        coords
            .windows(2)
            .map(|w| {
                Line::new(
                    Coord {
                        x: w[0].0,
                        y: w[0].1,
                    },
                    Coord {
                        x: w[1].0,
                        y: w[1].1,
                    },
                )
            })
            .collect()
    }

    fn area_of(lines: &[Line<f64>]) -> f64 {
        make_valid_poly_symdiff(lines)
            .iter()
            .map(Area::unsigned_area)
            .sum()
    }

    fn square(x: f64, y: f64, width: f64) -> Polygon<f64> {
        Polygon::new(
            LineString::new(
                [
                    (x, y),
                    (x + width, y),
                    (x + width, y + width),
                    (x, y + width),
                    (x, y),
                ]
                .iter()
                .map(|&(x, y)| Coord { x, y })
                .collect(),
            ),
            Vec::new(),
        )
    }

    #[test]
    fn pentagram_centre_is_empty() {
        let lines = ring(&[
            (0.0, 0.0),
            (2.0, 3.0),
            (4.0, 0.0),
            (0.0, 2.0),
            (4.0, 2.0),
            (0.0, 0.0),
        ]);
        assert!((area_of(&lines) - 3.333_333_333).abs() < 1e-6);
    }

    #[test]
    fn two_overlapping_squares_give_six() {
        let out = symdiff_test(&[square(0.0, 0.0, 2.0)], &[square(1.0, 1.0, 2.0)]);
        let area: f64 = out.iter().map(Area::unsigned_area).sum();
        assert!((area - 6.0).abs() < 1e-9, "even-odd area {area}");
    }

    #[test]
    fn doubled_ring_bounds_nothing() {
        let lines = ring(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        assert!(area_of(&lines) < 1e-9);
    }
}

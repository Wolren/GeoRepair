//! Tolerance-based repeated-point removal.
//!
//! Our own implementation of the GEOS `RepeatedPointRemover` semantics
//! (source: `geos/src/operation/valid/RepeatedPointRemover.cpp`, commit
//! `cad26ad98` "Return EMPTY components when repeated point removal renders
//! the underlying parts invalid"). 2D only.
//!
//! Behavior contract (source-verified):
//! - Sequence level: keep the first coordinate, then every coordinate that
//!   is not exactly equal to and not within `tolerance` of the last kept
//!   coordinate. Tolerance 0 removes exact consecutive duplicates.
//! - Geometry level: invalid (non-finite) coordinates are skipped, including
//!   leading ones. Lines keep a minimum of 2 coordinates, rings 3.
//! - If filtering leaves a single coordinate the component collapses to
//!   empty (line, polygon, or collection part removed).
//! - The original end coordinate is re-attached when the filtered output is
//!   too small (fluff) or when it was stripped (last-point repair), but
//!   never when the original end was invalid.

use geo::{Coord, Geometry, HasDimensions, LineString, MultiLineString, MultiPolygon, Polygon};

/// Minimum ring size GEOS accepts (2 distinct + closure).
const RING_MIN: usize = 3;
/// Minimum line size.
const LINE_MIN: usize = 2;

/// Sequence-level removal: consecutive duplicates and within-tolerance
/// points are dropped (GEOS `removeRepeatedPoints(seq, tolerance)`).
pub fn remove_repeated_coords(coords: &[Coord<f64>], tolerance: f64) -> Vec<Coord<f64>> {
    if coords.is_empty() {
        return Vec::new();
    }
    if tolerance == 0.0 {
        return crate::noding::remove_consecutive_duplicates(coords);
    }
    let sq = tolerance * tolerance;
    let mut out: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
    for &c in coords {
        if let Some(&p) = out.last()
            && (c == p || (c.x - p.x).powi(2) + (c.y - p.y).powi(2) <= sq)
        {
            continue;
        }
        out.push(c);
    }
    out
}

/// Geometry-level removal (GEOS `removeRepeatedPoints(geometry, tolerance)`).
/// Collapsed components become empty geometries.
pub fn remove_repeated_points(geom: &Geometry<f64>, tolerance: f64) -> Geometry<f64> {
    match geom {
        Geometry::Point(_) => geom.clone(),
        Geometry::LineString(ls) => match edit_sequence(&ls.0, LINE_MIN, tolerance) {
            None => empty_line(),
            Some(coords) => Geometry::LineString(LineString::new(coords)),
        },
        Geometry::MultiLineString(mls) => {
            let parts: Vec<LineString<f64>> = mls
                .0
                .iter()
                .filter_map(|ls| edit_sequence(&ls.0, LINE_MIN, tolerance).map(LineString::new))
                .collect();
            if parts.is_empty() {
                empty_line()
            } else {
                Geometry::MultiLineString(MultiLineString::new(parts))
            }
        }
        Geometry::Polygon(p) => {
            let Some(shell) = edit_sequence(&p.exterior().0, RING_MIN, tolerance) else {
                return empty_polygon();
            };
            let holes: Vec<LineString<f64>> = p
                .interiors()
                .iter()
                .filter_map(|h| edit_sequence(&h.0, RING_MIN, tolerance).map(LineString::new))
                .collect();
            Geometry::Polygon(Polygon::new(LineString::new(shell), holes))
        }
        Geometry::MultiPolygon(mp) => {
            let parts: Vec<Polygon<f64>> = mp
                .0
                .iter()
                .filter_map(|p| match remove_repeated_points(&Geometry::Polygon(p.clone()), tolerance) {
                    Geometry::Polygon(pp) if !pp.is_empty() => Some(pp),
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                empty_polygon()
            } else {
                Geometry::MultiPolygon(MultiPolygon::new(parts))
            }
        }
        Geometry::GeometryCollection(gc) => {
            let parts: Vec<Geometry<f64>> = gc
                .0
                .iter()
                .map(|g| remove_repeated_points(g, tolerance))
                .collect();
            Geometry::GeometryCollection(geo::GeometryCollection(parts))
        }
        _ => geom.clone(),
    }
}

/// GEOS `RepeatedPointCoordinateOperation::edit` (with the invalid-coordinate
/// filter and end repair). Returns `None` when the component collapses.
fn edit_sequence(coords: &[Coord<f64>], min_len: usize, tolerance: f64) -> Option<Vec<Coord<f64>>> {
    // No way to filter short sequences.
    if coords.len() <= min_len {
        return Some(coords.to_vec());
    }

    // RepeatedInvalidPointFilter: skip invalid coords (leading ones too),
    // exact duplicates, and points within tolerance of the last kept.
    let sq = tolerance * tolerance;
    let mut filt: Vec<Coord<f64>> = Vec::new();
    for &c in coords {
        let invalid = !c.x.is_finite() || !c.y.is_finite();
        if filt.is_empty() && invalid {
            continue;
        }
        let skip = match filt.last() {
            None => false,
            Some(&p) => invalid || c == p || (c.x - p.x).powi(2) + (c.y - p.y).powi(2) <= sq,
        };
        if skip {
            continue;
        }
        filt.push(c);
    }
    if filt.is_empty() || filt.len() == 1 {
        return None;
    }

    // End points for comparison and sequence repair.
    let orig_end = *coords.last().unwrap();
    let orig_end_valid = orig_end.x.is_finite() && orig_end.y.is_finite();

    // Fluff up overly small filtered outputs.
    if filt.len() < min_len && orig_end_valid {
        filt.push(orig_end);
    }

    let filt_end = *filt.last().unwrap();

    // We stripped the last point, let's put it back on.
    if orig_end_valid && orig_end != filt_end {
        // If the end of the filtered coordinates is within tolerance of
        // the original end, drop the last filtered coordinate so the
        // output still follows the tolerance rule.
        if (orig_end.x - filt_end.x).powi(2) + (orig_end.y - filt_end.y).powi(2) <= sq {
            filt.pop();
        }
        filt.push(orig_end);
    }

    if filt.len() <= 1 {
        None
    } else {
        Some(filt)
    }
}

fn empty_line() -> Geometry<f64> {
    Geometry::LineString(LineString::new(Vec::new()))
}

fn empty_polygon() -> Geometry<f64> {
    Geometry::Polygon(Polygon::new(LineString::new(Vec::new()), Vec::new()))
}

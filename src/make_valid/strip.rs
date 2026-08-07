//! Degenerate-strip cleanup: NaN/Inf handling, isSimple checks, and
//! OGC winding helpers.


use alloc::vec::Vec;
use super::*;

pub(super) fn has_nan(g: &Geometry<f64>) -> bool {
    use geo::CoordsIter;
    g.coords_iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
}

/// Cheap simplicity check for 2-pt and short linestrings demoted from collapsed polys.
pub(super) fn linestring_is_simple(ls: &LineString<f64>) -> bool {
    let coords = &ls.0;
    if coords.len() < 2 {
        return false;
    }
    // Open path for intersection check (drop closing duplicate if present)
    let end = if coords.len() >= 2 && coords.first() == coords.last() {
        coords.len() - 1
    } else {
        coords.len()
    };
    if end < 2 {
        return false;
    }
    if end == 2 {
        return coords[0] != coords[1];
    }
    let lines: Vec<Line<f64>> = (0..end - 1)
        .map(|i| Line::new(coords[i], coords[i + 1]))
        .filter(|l| l.start != l.end)
        .collect();
    if lines.len() < 2 {
        return !lines.is_empty();
    }
    #[cfg(feature = "arrange")]
    {
        crate::arrange::prep::has_no_intersections(&lines)
    }
    #[cfg(not(feature = "arrange"))]
    {
        true
    }
}

/// Remove degenerate Polygon/MultiPolygon components: exterior rings with
/// <4 coordinates, shoelace area below epsilon, or NaN/Inf coordinates.
/// Returns boundary LineString for degenerate polygons (GEOS-style type degradation).
pub fn strip_degenerate_test(
    g: Geometry<f64>, 
) -> Geometry<f64> {
    strip_degenerate(g)
}

pub(super) fn strip_degenerate(g: Geometry<f64>) -> Geometry<f64> {
    match g {
        Geometry::Polygon(p) => {
            let ext = &p.exterior().0;
            // Fast path: valid polygons with ≥4 coords and no NaN pass through
            if ext.len() >= 4 {
                            // Single-pass: compute bbox, shoelace, and NaN simultaneously
                            let n = ext.len();
                            let interior_n = if ext.first() == ext.last() { n - 1 } else { n };
                            // Guard: fewer than 3 unique vertices can never form a valid polygon.
                            // (interior_n == 3 with positive area is a valid triangle.)
                            if interior_n < 3 {
                                // Fall through to boundary output below.
                            } else {
                            let (mut min_x, mut max_x, mut min_y, mut max_y) = (ext[0].x, ext[0].x, ext[0].y, ext[0].y);
                let mut has_nan = !ext[0].x.is_finite() || !ext[0].y.is_finite();
                for i in 0..interior_n - 1 {
                    let c = ext[i + 1];
                    min_x = min_x.min(c.x);
                    max_x = max_x.max(c.x);
                    min_y = min_y.min(c.y);
                    max_y = max_y.max(c.y);
                    if !has_nan && (!c.x.is_finite() || !c.y.is_finite()) {
                        has_nan = true;
                    }
                }
                // Bbox degeneracy is per-axis LOCAL (same rule as the
                // make_valid pre-gate): an axis is degenerate when its
                // extent is at or below the coordinate rounding at that
                // axis's own magnitude. The old rule compared against the
                // max spread, so a 4.9e208 spike dominated the 1-unit
                // x-extent of a valid ring and demoted it to a line.
                let x_scale = max_x.abs().max(min_x.abs());
                let y_scale = max_y.abs().max(min_y.abs());
                let bbox_ok = (max_x - min_x).abs() > f64::EPSILON * x_scale
                    && (max_y - min_y).abs() > f64::EPSILON * y_scale;
                // Area degeneracy: a ring is degenerate iff its vertices lie
                // bit-exactly on one line (robust orient == 0). The
                // historical magnitude-based noise bound demoted real
                // slivers at large coordinate magnitude (0.14.2 changelog).
                let p0 = ext[0];
                let p1 = ext[1];
                let exactly_collinear =
                    (2..interior_n).all(|i| crate::orient::orient2d(p0, p1, ext[i]) == 0.0);
                let area_ok = !exactly_collinear;
                if area_ok && bbox_ok && !has_nan {
                    // Non-degenerate polygon - return as-is after hole cleanup
                    let holes: Vec<LineString<f64>> = p.interiors().iter()
                        .filter(|ring| {
                            ring.0.len() >= 4
                                && !ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                        })
                        .cloned()
                        .collect();
                    return if holes.len() == p.interiors().len() {
                                            Geometry::Polygon(p)
                                        } else {
                                            Geometry::Polygon(Polygon::new(p.exterior().clone(), holes))
                                        };
                                    }
                                    } // end interior_n >= 3
                                }
                                // Degenerate: demote to open LineString (drop closing vertex). Closed
                                            // collapsed rings are often NotSimple under OGC; open path is cleaner.
                                            // If still not simple → empty (keep_collapsed=false default).
                                            let mut lines: Vec<LineString<f64>> = Vec::new();
                                            if ext.len() >= 2 && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
                                                let mut coords = ext.clone();
                                                if coords.len() >= 2 && coords.first() == coords.last() {
                                                    coords.pop();
                                                }
                                                let coords = remove_consecutive_duplicates(&coords);
                                                if coords.len() >= 2 {
                                                    lines.push(LineString::new(coords));
                                                }
                                            }
                                            for ring in p.interiors() {
                                                if ring.0.len() >= 2
                                                    && !ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                                                {
                                                    let mut coords = ring.0.clone();
                                                    if coords.len() >= 2 && coords.first() == coords.last() {
                                                        coords.pop();
                                                    }
                                                    let coords = remove_consecutive_duplicates(&coords);
                                                    if coords.len() >= 2 {
                                                        lines.push(LineString::new(coords));
                                                    }
                                                }
                                            }
                                            let simple: Vec<LineString<f64>> = lines
                                                .into_iter()
                                                .filter(linestring_is_simple)
                                                .collect();
                                            if simple.is_empty() {
                                                empty_geom::<f64>()
                                            } else if simple.len() == 1 {
                                                Geometry::LineString(simple.into_iter().next().unwrap())
                                            } else {
                                                Geometry::MultiLineString(MultiLineString::new(simple))
                                            }
                                        }
        Geometry::MultiPolygon(mp) => {
            let mut valid_polys: Vec<Polygon<f64>> = Vec::new();
            let mut boundary_lines: Vec<LineString<f64>> = Vec::new();
            for p in mp.0.into_iter() {
                let ext = &p.exterior().0;
                if ext.len() >= 4
                    && shoelace_abs_sum(ext) >= 1e-12
                    && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                {
                    valid_polys.push(p);
                } else {
                    // Collect degenerate component's boundary as lines
                    if ext.len() >= 2 && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
                        boundary_lines.push(p.exterior().clone());
                    }
                    for ring in p.interiors() {
                        if ring.0.len() >= 2
                            && !ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                        {
                            boundary_lines.push(ring.clone());
                        }
                    }
                }
            }
            match (valid_polys.len(), boundary_lines.is_empty()) {
                            (0, true) => empty_geom::<f64>(),
                            (0, false) => {
                                if boundary_lines.len() == 1 {
                                    Geometry::LineString(boundary_lines.into_iter().next().unwrap())
                                } else {
                                    Geometry::MultiLineString(MultiLineString::new(boundary_lines))
                                }
                            }
                            // Keep MultiPolygon type even for a single component - Geometry
                            // dispatch runs strip_degenerate after MultiPolygon::make_valid.
                            (1, true) => Geometry::MultiPolygon(MultiPolygon::new(valid_polys)),
                            (1, false) => {
                                let mut geoms: Vec<Geometry<f64>> = Vec::new();
                                geoms.push(Geometry::MultiPolygon(MultiPolygon::new(valid_polys)));
                                let mls = if boundary_lines.len() == 1 {
                                    Geometry::LineString(boundary_lines.into_iter().next().unwrap())
                                } else {
                                    Geometry::MultiLineString(MultiLineString::new(boundary_lines))
                                };
                                geoms.push(mls);
                                Geometry::GeometryCollection(GeometryCollection(geoms))
                            }
                            (_, true) => Geometry::MultiPolygon(MultiPolygon::new(valid_polys)),
                            (_, false) => {
                                let mut geoms: Vec<Geometry<f64>> = valid_polys.into_iter()
                                    .map(Geometry::Polygon).collect();
                                let mls = if boundary_lines.len() == 1 {
                                    Geometry::LineString(boundary_lines.into_iter().next().unwrap())
                                } else {
                                    Geometry::MultiLineString(MultiLineString::new(boundary_lines))
                                };
                                geoms.push(mls);
                                Geometry::GeometryCollection(GeometryCollection(geoms))
                            }
                        }
        }
        other => other,
    }
}

pub(super) fn enforce_ccw(mut ring: LineString<f64>) -> LineString<f64> {
    // Use Shewchuk's orient2d (adaptive precision) on the extremal vertex
    // to determine winding order. The shoelace sum in geo's winding_order()
    // can flip sign at extreme fp ratios (e.g. 1e12 and 1e-12 in same ring).
    let is_ccw = crate::util::robust_is_ccw(&ring.0);
    if !is_ccw {
        ring.make_ccw_winding();
    }
    ring
}

pub(super) fn enforce_cw(mut ring: LineString<f64>) -> LineString<f64> {
    let is_ccw = crate::util::robust_is_ccw(&ring.0);
    if is_ccw {
        ring.make_cw_winding();
    }
    ring
}



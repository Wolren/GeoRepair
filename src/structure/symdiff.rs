//! GEOS MakeValidPoly symdiff loop: BuildArea the remaining cut edges, xor the new
//! faces into the accumulated area, remove the built boundary, and repeat until nothing
//! remains (even-odd rule, GEOS MakeValidPoly.cpp).

use alloc::vec::Vec;
use geo::algorithm::bool_ops::BooleanOps;
use geo::{Coord, Line, MultiPolygon, Polygon};

use log::warn;

use super::fix_ring::{basic_cleanup, collapse_sub_ulp_vertices, is_collinear_ring};

/// geos MakeValid-style repair: BuildArea levels XORed in even-odd order. The public
/// name is historical; the loop may run more than one pass.
pub fn single_pass_fix(poly: &Polygon<f64>) -> Option<MultiPolygon<f64>> {
    single_pass_fix_tuned(poly, &crate::core::Tuning::default())
}

/// Tuned variant of [`single_pass_fix`]: the repair pipeline threads the caller's
/// [`crate::core::Tuning`] through the dispatch thresholds.
pub(crate) fn single_pass_fix_tuned(
    poly: &Polygon<f64>,
    tuning: &crate::core::Tuning,
) -> Option<MultiPolygon<f64>> {
    // pipeline (measured 168 ms/poly on 200k-edge monsters vs ~36 ms)
    let n_edges: usize = poly.exterior().0.len().saturating_sub(1)
        + poly
            .interiors()
            .iter()
            .map(|h| h.0.len().saturating_sub(1))
            .sum::<usize>();
    if n_edges > tuning.sp_max_edges {
        return None;
    }

    let mut edges: Vec<Line<f64>> = Vec::new();
    for ring in core::iter::once(poly.exterior()).chain(poly.interiors()) {
        let Some(coords) = basic_cleanup(ring) else {
            continue;
        };
        if coords.len() < 4 {
            continue;
        }
        if is_collinear_ring(&coords) {
            continue;
        }
        let coords = collapse_sub_ulp_vertices(&coords, false);
        if coords.len() < 4 {
            continue;
        }
        edges.extend(edges_from_coords(&coords));
    }
    if edges.is_empty() {
        return None;
    }

    let mut noded = crate::structure::edge_split::split_edges_tuned(&edges, tuning);
    if noded.is_empty() {
        return None;
    }

    let mut validator = crate::noding::validator::NodingValidator::new(noded.clone());
    validator.validate();
    if validator.has_violations() {
        warn!(
            "single_pass_fix: {} noding violation(s) remain, retrying with snap rounding",
            validator.violations().len()
        );
        let snapped = crate::noding::snap_round::snap_round_lines(&edges);
        if !snapped.is_empty() {
            noded = snapped;
        }
    }
    if noded.is_empty() {
        return None;
    }

    // The GEOS loop, not one BuildArea pass: a self-crossing ring's inner level has to
    // be subtracted by the next pass (pentagram 4.667 as a union, 3.333 after the xor).
    let area = make_valid_poly_symdiff(&noded);
    if area.is_empty() {
        return None;
    }
    Some(MultiPolygon::new(area))
}

/// GEOS MakeValidPoly::buildArea loop: repeatedly BuildArea the remaining cut edges,
/// xor into the accumulated area.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn make_valid_poly_symdiff(cut_edges: &[Line<f64>]) -> Vec<Polygon<f64>> {
    let snap = |c: Coord<f64>| Coord {
        x: (c.x * crate::core::SNAP_SCALE).round() / crate::core::SNAP_SCALE,
        y: (c.y * crate::core::SNAP_SCALE).round() / crate::core::SNAP_SCALE,
    };
    let mut remaining: Vec<Line<f64>> = cut_edges
        .iter()
        .map(|l| Line::new(snap(l.start), snap(l.end)))
        .collect();
    let mut area: Vec<Polygon<f64>> = Vec::new(); // accumulated (XOR) area
    let mut guard = 0usize;
    while !remaining.is_empty() {
        guard += 1;
        if guard > 64 {
            warn!("make_valid_poly_symdiff: iteration guard exceeded");
            #[cfg(test)]
            panic!("make_valid_poly_symdiff hit the iteration guard (non-convergence)");
            #[cfg(not(test))]
            break;
        }
        let Some((new_area, boundary)) =
            crate::structure::build_area::build_area_with_boundary(&remaining)
        else {
            break;
        };
        if new_area.0.is_empty() {
            break;
        }
        area = symdiff_polygons(&area, &new_area.0);
        #[cfg(all(any(test, debug_assertions), feature = "std"))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            use geo::Area;
            eprintln!(
                "symdiff iter {guard}: remaining edges = {}, new_area = {} polys, boundary = {} segs",
                remaining.len(),
                new_area.0.len(),
                boundary.len()
            );
            let mut total = 0.0;
            for (i, p) in new_area.0.iter().enumerate() {
                let a = p.unsigned_area();
                total += a;
                eprintln!(
                    "   new_area[{i}]: area={a:.4} holes={}",
                    p.interiors().len()
                );
            }
            eprintln!("   new_area total = {total:.4}");
            let t: f64 = area.iter().map(|p| p.unsigned_area()).sum();
            eprintln!("   area after XOR = {t:.4} ({} polys)", area.len());
        }
        let boundary_keys: rustc_hash::FxHashSet<(u64, u64, u64, u64)> = boundary
            .iter()
            .map(|l| segment_key(l.start, l.end))
            .collect();
        remaining.retain(|l| !boundary_keys.contains(&segment_key(l.start, l.end)));
    }
    area
}

/// Orientation-insensitive key for a segment (bit-exact after snapping).
fn segment_key(a: Coord<f64>, b: Coord<f64>) -> (u64, u64, u64, u64) {
    let (ax, ay, bx, by) = (a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits());
    if (ax, ay) <= (bx, by) {
        (ax, ay, bx, by)
    } else {
        (bx, by, ax, ay)
    }
}

pub fn symdiff_test(a: &[Polygon<f64>], b: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    symdiff_polygons(a, b)
}

/// Symmetric difference of two polygon sets, accumulated xor, for the MakeValidPoly
/// loop. BuildArea dissolves its faces, so iteration N+1 can carry a hole that
/// iteration N does not, and only a real overlay cancels it (overlapping parts: 7
/// from union, 6 from xor, matching GEOS).
fn symdiff_polygons(a: &[Polygon<f64>], b: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let out = MultiPolygon::new(a.to_vec()).xor(&MultiPolygon::new(b.to_vec()));
    out.0
}

pub fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MakeValid, MakeValidConfig, PolyMethod};
    use geo::{Area, LineString, Polygon};

    /// Verbatim replication of benches/bench.rs::make_spaghetti_ring.
    fn spaghetti_ring(n: usize) -> Polygon<f64> {
        let span = (n as f64).sqrt().ceil() as i64;
        let mut x = 0i64;
        let mut y = 0i64;
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        let mut coords = Vec::with_capacity(n + 1);
        coords.push(Coord {
            x: x as f64,
            y: y as f64,
        });
        for _ in 1..n {
            match next() % 4 {
                0 => x += 1,
                1 => x -= 1,
                2 => y += 1,
                _ => y -= 1,
            }
            x = x.rem_euclid(span);
            y = y.rem_euclid(span);
            coords.push(Coord {
                x: x as f64,
                y: y as f64,
            });
        }
        coords.push(coords[0]);
        Polygon::new(LineString::new(coords), Vec::new())
    }

    /// The symdiff loop used to cycle on this class: the same faces were rebuilt every
    /// pass, so the area turned on the 64-iteration guard's parity. The dissolved
    /// BuildArea and the boundary-segment subtraction make it converge.
    #[test]
    fn symdiff_converges_on_self_crossing_walk() {
        let cfg = MakeValidConfig {
            poly_method: PolyMethod::Structure,
            ..Default::default()
        };
        // GEOS gives 144.244318 and 790.486264 on these rings. These pins guard
        // convergence, not GEOS parity.
        for (n, expected) in [(500usize, 193.681818f64), (2000, 212.741758)] {
            let out = spaghetti_ring(n).make_valid_with_config(&cfg);
            assert!(crate::is_valid(&out), "n={n}: repair must stay valid");
            let area = out.unsigned_area();
            assert!(
                (area - expected).abs() < 1e-4,
                "n={n}: area {area} drifted from the pinned {expected}"
            );
        }
    }

    /// Even-odd is the assembly rule: the pentagram centre has winding 2 and stays
    /// empty. GEOS makeValid returns the same 3.333 on this ring, in both strategies.
    #[test]
    fn pentagram_keeps_the_centre_empty() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 3.0 },
            Coord { x: 4.0, y: 0.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        for method in [PolyMethod::Structure, PolyMethod::Arrange] {
            let cfg = MakeValidConfig {
                poly_method: method,
                ..Default::default()
            };
            let out = Polygon::new(LineString::new(coords.clone()), Vec::new())
                .make_valid_with_config(&cfg);
            assert!(
                crate::is_valid(&out),
                "{method:?} pentagram repair must stay valid"
            );
            let area = out.unsigned_area();
            assert!(
                (area - 3.333_333_333_333_333_5).abs() < 1e-6,
                "{method:?} pentagram area {area} is not the GEOS even-odd 3.333"
            );
        }
    }
}

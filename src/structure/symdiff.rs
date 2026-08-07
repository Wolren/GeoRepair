//! GEOS MakeValidPoly symdiff loop: BuildArea the remaining cut edges,
//! XOR the new faces into the accumulated area, remove the built boundary,
//! and repeat until nothing remains (even-odd rule, GEOS MakeValidPoly.cpp).


use alloc::vec::Vec;
use geo::{Coord, Line, MultiPolygon, Polygon};

use log::warn;

use super::fix_ring::{basic_cleanup, collapse_sub_ulp_vertices, is_collinear_ring};

/// Single-pass GEOS MakeValid-style repair: node ALL ring linework (shell +
/// holes together) in ONE pass, then BuildArea the result with even-odd face
/// labeling. Holes become holes via the even-parent filter; self-crossings,
/// crossing holes, hole overlaps, and holes outside the shell are all
/// resolved by the noding + face walk — no boolean subtraction, no per-ring
/// symdiff loop.
///
/// Returns `None` when the linework cannot be closed into faces (the caller
/// falls back to the multi-stage boolean pipeline).
pub fn single_pass_fix(poly: &Polygon<f64>) -> Option<MultiPolygon<f64>> {
    // Routing gate: count total ring edges first (cheap O(n) scan). Above
    // SP_MAX_EDGES the all-rings R-tree noding outweighs the boolean
    // pipeline (measured 168 ms/poly on 200k-edge monsters vs ~36 ms) —
    // return None so the caller's boolean pipeline handles the giants.
    let n_edges: usize = poly
        .exterior()
        .0
        .len()
        .saturating_sub(1)
        + poly.interiors().iter().map(|h| h.0.len().saturating_sub(1)).sum::<usize>();
    if n_edges > crate::core::SP_MAX_EDGES {
        return None;
    }

    // 1. Collect + clean all ring linework (shell first, then holes).
    let mut edges: Vec<Line<f64>> = Vec::new();
    for ring in core::iter::once(poly.exterior()).chain(poly.interiors()) {
        let Some(coords) = basic_cleanup(ring) else { continue };
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

    // 2. Node everything together (R-tree/sweep-line, parametric splits).
    let mut noded = crate::structure::edge_split::split_edges(&edges);
    if noded.is_empty() {
        return None;
    }

    // 3. Noding validation with snap-round retry (same as fix_self_intersecting).
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

    // 4. Even-odd face extraction (GEOS BuildArea port).
    let mp = crate::structure::build_area::build_area(&noded)?;
    if mp.0.is_empty() {
        return None;
    }
    Some(mp)
}


/// GEOS MakeValidPoly::buildArea loop: repeatedly BuildArea the remaining
/// cut edges, XOR into the accumulated area, and remove the built boundary
/// from the cut edges, until nothing remains. Returns the odd-winding faces
/// (even-odd rule), exactly like GEOS.
pub fn make_valid_poly_symdiff(cut_edges: &[Line<f64>]) -> Vec<Polygon<f64>> {
    // build_area snaps vertices to the SNAP_SCALE grid; snap the input edges
    // the same way so boundary removal matches exactly.
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
            break;
        }
        let Some(new_area) = crate::structure::build_area::build_area(&remaining) else {
            break;
        };
        if new_area.0.is_empty() {
            break;
        }
        // XOR the new faces into the accumulated area (symdiff).
        area = symdiff_polygons(&area, &new_area.0);
        // Remove the BOUNDARY of the built area from the cut edges. The
        // boundary = segments appearing in exactly ONE face ring (internal
        // edges appear in two adjacent faces, opposite directions — they
        // stay in `remaining` for the next iteration). This mirrors GEOS's
        // CascadedPolygonUnion dissolve before the boundary difference.
        let mut seg_counts: rustc_hash::FxHashMap<(u64, u64, u64, u64), usize> =
            rustc_hash::FxHashMap::default();
        for p in &new_area.0 {
            for ring in core::iter::once(p.exterior()).chain(p.interiors()) {
                for w in ring.0.windows(2) {
                    if w[0] != w[1] {
                        let key = segment_key(w[0], w[1]);
                        *seg_counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }
        #[cfg(all(any(test, debug_assertions), feature = "std"))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            use geo::Area;
            eprintln!(
                "symdiff iter {guard}: remaining edges = {}, new_area = {} polys",
                remaining.len(),
                new_area.0.len()
            );
            let mut total = 0.0;
            for (i, p) in new_area.0.iter().enumerate() {
                let a = p.unsigned_area();
                total += a;
                eprintln!("   new_area[{i}]: area={a:.4} holes={}", p.interiors().len());
            }
            eprintln!("   new_area total = {total:.4}");
        }
        #[cfg(all(any(test, debug_assertions), feature = "std"))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            use geo::Area;
            let t: f64 = area.iter().map(|p| p.unsigned_area()).sum();
            eprintln!("   area after XOR = {t:.4} ({} polys)", area.len());
        }
        remaining.retain(|l| {
            let key = segment_key(snap(l.start), snap(l.end));
            // Keep edges NOT on the built boundary: count==0 (unused) or
            // count==2 (internal edge shared by two faces → next iter).
            seg_counts.get(&key).copied().unwrap_or(0) != 1
        });
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

/// Symmetric difference of two polygon sets (accumulated XOR) for the
/// MakeValidPoly loop.
///
/// Our build_area returns ATOMIC subdivision cells (no CascadedPolygonUnion
/// dissolve), so the faces of iteration N are exactly a SUBSET of iteration 1
/// faces — the XOR is a set difference by ring fingerprint, no boolean ops.
/// (geo's boolean ops are winding-sensitive and would fragment valid faces,
/// losing area downstream in unary_union.)
fn symdiff_polygons(a: &[Polygon<f64>], b: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let fp = |p: &Polygon<f64>| -> Vec<(u64, u64)> {
        let mut pts: Vec<(u64, u64)> = p
            .exterior()
            .0
            .iter()
            .map(|c| (c.x.to_bits(), c.y.to_bits()))
            .collect();
        if pts.first() == pts.last() {
            pts.pop();
        }
        pts.sort_unstable();
        pts
    };
    #[cfg(all(any(test, debug_assertions), feature = "std"))]
    if std::env::var("DIAG_SYMDIFF").is_ok() {
        use geo::Area;
        eprintln!("   XOR: a={} polys, b={} polys", a.len(), b.len());
        for (i, q) in a.iter().enumerate() {
            eprintln!(
                "     a[{i}]: area={:.4} fp={:?}",
                q.unsigned_area(),
                fp(q)
            );
        }
        for (i, p) in b.iter().enumerate() {
            eprintln!(
                "     b[{i}]: area={:.4} fp={:?}",
                p.unsigned_area(),
                fp(p)
            );
        }
    }
    let b_set: Vec<Vec<(u64, u64)>> = b.iter().map(fp).collect();
    let mut out: Vec<Polygon<f64>> = Vec::new();
    for q in a {
        let qf = fp(q);
        let matched = b_set.contains(&qf);
        #[cfg(all(any(test, debug_assertions), feature = "std"))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            eprintln!("     match a fp {:?} -> {}", qf, matched);
        }
        if !matched {
            out.push(q.clone());
        }
    }
    // Faces of b not present in ORIGINAL a survive the XOR (defensive;
    // normally none at iteration > 1 since b faces are a subset of a faces).
    let a_set: Vec<Vec<(u64, u64)>> = a.iter().map(fp).collect();
    for p in b {
        let pf = fp(p);
        if !a_set.contains(&pf) {
            out.push(p.clone());
        }
    }
    out
}

pub fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

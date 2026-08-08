use alloc::vec::Vec;
#[cfg_attr(not(test), allow(unused_imports))]
use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};

use crate::validation::core::*;

impl GeoValidation for Polygon<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();

        // An empty exterior is an empty polygon - valid OGC (same as
        // POINT EMPTY / LINESTRING EMPTY; GEOS isValid=true on
        // POLYGON (EMPTY, EMPTY, EMPTY)).
        if self.exterior().0.is_empty() {
            return ValidationResult::valid();
        }

        // Giant shells: the shell's own checks and the hole checks are
        // independent validity conditions. NOTE (2026-08-06): running them
        // in parallel (rayon::join + per-hole par_iter) was MEASURED as a
        // regression (3.83s vs 3.17s on the 1.58M dataset) - the batch's
        // pool is already saturated, so nested parallelism only adds join
        // and split overhead. The batch-level size partition (bench) is the
        // parallelism lever that works; per-poly stays serial.
        let ext_errors = check_ring_validity(&self.exterior().0, true);
        if !ext_errors.is_empty() {
            // The fused ring check folds the shell orientation into the
            // same pass; the pre-fusion code early-returned on structural
            // ring errors but continued to the holes when ONLY the
            // orientation was wrong. Preserve the error-set parity
            // (IsValidOp t2 expects HoleOutsideShell alongside the
            // shell's WrongOrientation).
            let only_orientation = ext_errors
                .iter()
                .all(|e| matches!(e, GeometryValidationError::WrongOrientation));
            errors.extend(ext_errors);
            if !only_orientation {
                return ValidationResult::invalid(errors);
            }
        }

        // Ring orientation is part of check_ring_validity's verdict (the
        // OGC winding contract) - no separate extremal-search pass
        // (2026-08-08).

        if self.interiors().is_empty() {
            if errors.is_empty() {
                return ValidationResult::valid();
            }
            return ValidationResult::invalid(errors);
        }

        let interiors: Vec<&[Coord<f64>]> = self.interiors().iter().map(|h| &h.0[..]).collect();

        if has_duplicate_rings(&interiors, &self.exterior().0) {
            errors.push(GeometryValidationError::DuplicatedRings);
            return ValidationResult::invalid(errors);
        }

        for hole in self.interiors() {
            let hole_errors = check_ring_validity(&hole.0, false);
            if !hole_errors.is_empty() {
                errors.extend(hole_errors);
                continue;
            }
        }

        let hole_containment_errors = check_holes_valid(&self.exterior().0, self.interiors());
        errors.extend(hole_containment_errors);

        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

/// True when any hole duplicates another hole or the shell (rotated-start
/// duplicates included). Fingerprint grouping avoids the O(h²) pairwise
/// scan. Shared by the validator and the fast-path gate (the gate must
/// certify exactly what the validator accepts before it may skip the exit
/// validator - 2026-08-07).
pub(crate) fn has_duplicate_rings(interiors: &[&[Coord<f64>]], shell: &[Coord<f64>]) -> bool {
    if interiors.len() > 1 {
        let mut groups: rustc_hash::FxHashMap<(usize, u64), Vec<usize>> =
            rustc_hash::FxHashMap::with_capacity_and_hasher(interiors.len(), Default::default());
        for (i, h) in interiors.iter().enumerate() {
            groups.entry(ring_dup_fingerprint(h)).or_default().push(i);
        }
        for (_, indices) in groups {
            for (ii, &a) in indices.iter().enumerate() {
                for &b in indices.iter().skip(ii + 1) {
                    if is_rotated_duplicate(interiors[a], interiors[b]) {
                        return true;
                    }
                }
            }
        }
    }
    // Also check each hole against the shell
    for h in interiors {
        if ring_dup_fingerprint(h) == ring_dup_fingerprint(shell) && is_rotated_duplicate(h, shell)
        {
            return true;
        }
    }
    false
}

impl GeoValidation for MultiPolygon<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        for p in &self.0 {
            let r = p.validate();
            if !r.valid {
                errors.extend(r.errors);
            }
        }

        let shells: Vec<&[Coord<f64>]> = self.0.iter().map(|p| &p.exterior().0[..]).collect();

        // Check for duplicate shells (including rotated-start duplicates)
        if shells.len() > 1 {
            let mut groups: rustc_hash::FxHashMap<(usize, u64), Vec<usize>> =
                rustc_hash::FxHashMap::with_capacity_and_hasher(shells.len(), Default::default());
            for (i, s) in shells.iter().enumerate() {
                groups.entry(ring_dup_fingerprint(s)).or_default().push(i);
            }
            for (_, indices) in groups {
                for (ii, &a) in indices.iter().enumerate() {
                    for &b in indices.iter().skip(ii + 1) {
                        if is_rotated_duplicate(shells[a], shells[b]) {
                            errors.push(GeometryValidationError::DuplicatedRings);
                            return ValidationResult::invalid(errors);
                        }
                    }
                }
            }
        }

        if shells.len() > 1 {
            // Compute global scale for intersection epsilon
            let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) =
                (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for s in &shells {
                for c in *s {
                    gmin_x = gmin_x.min(c.x);
                    gmax_x = gmax_x.max(c.x);
                    gmin_y = gmin_y.min(c.y);
                    gmax_y = gmax_y.max(c.y);
                }
            }
            let scale = (gmax_x - gmin_x)
                .abs()
                .max((gmax_y - gmin_y).abs())
                .max(1.0);
            let eps = 1e-12 * scale;

            // Cross-component ring intersection check (GEOS
            // checkAreaIntersections covers ALL ring pairs across
            // components: shell-shell, shell-hole, hole-hole - a proper
            // crossing between any two rings of different components is
            // invalid, eSelfIntersection). Pure vertex touches are NOT
            // crossings (edges_intersect_general semantics), so MP
            // touch-at-vertices cases stay valid. Candidate pairs are
            // bounding-box filtered (R-tree when many rings) - never an
            // unfiltered O(m^2) pair loop.
            let mut all_rings: Vec<(usize, bool, &[Coord<f64>])> = Vec::new();
            let mut comp_holes: Vec<Vec<&[Coord<f64>]>> = Vec::with_capacity(self.0.len());
            for (ci, p) in self.0.iter().enumerate() {
                all_rings.push((ci, false, &p.exterior().0[..]));
                let mut holes: Vec<&[Coord<f64>]> = Vec::new();
                for h in p.interiors() {
                    all_rings.push((ci, true, &h.0[..]));
                    holes.push(&h.0[..]);
                }
                comp_holes.push(holes);
            }
            let rb: Vec<[f64; 4]> = all_rings.iter().map(|(_, _, r)| ring_bbox(r)).collect();
            for (a, b) in overlap_pairs(&rb, 0) {
                if all_rings[a].0 == all_rings[b].0 {
                    continue;
                }
                let ra = all_rings[a].2;
                let rb2 = all_rings[b].2;
                if check_rings_intersect(ra, rb2, eps) {
                    errors.push(GeometryValidationError::SelfIntersection);
                    return ValidationResult::invalid(errors);
                }
                // Interior-overlap probe (GEOS checkAreaIntersections also
                // flags containment without an edge crossing, e.g. a
                // component whose shell shares vertices with another
                // component's hole ring - t21). Only (shell, hole) pairs are
                // probed, and only the HOLE side's vertices inside the
                // shell's FILL (holes of the shell's own component are
                // excised and do not count): a shell entirely inside another
                // shell's hole is a valid island (Test 616/641 in
                // general_TestValid2.xml) whose own hole may also sit in the
                // outer hole - both are valid. Hole-hole edge crossings are
                // already caught above; nested shells are the nesting
                // check's job.
                if all_rings[a].1 != all_rings[b].1 {
                    if all_rings[a].1
                        && ring_has_vertex_inside(ra, rb2, rb[b], &comp_holes[all_rings[b].0])
                    {
                        errors.push(GeometryValidationError::SelfIntersection);
                        return ValidationResult::invalid(errors);
                    }
                    if all_rings[b].1
                        && ring_has_vertex_inside(rb2, ra, rb[a], &comp_holes[all_rings[a].0])
                    {
                        errors.push(GeometryValidationError::SelfIntersection);
                        return ValidationResult::invalid(errors);
                    }
                }
            }

            // Nesting check: one shell fully inside another
            #[cfg(feature = "rstar")]
            {
                struct ShellEnv {
                    idx: usize,
                    env: rstar::AABB<[f64; 2]>,
                }
                impl rstar::RTreeObject for ShellEnv {
                    type Envelope = rstar::AABB<[f64; 2]>;
                    fn envelope(&self) -> Self::Envelope {
                        self.env
                    }
                }
                let mut envs = Vec::with_capacity(shells.len());
                for (i, s) in shells.iter().enumerate() {
                    let first = s.first().map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
                    let (mut min_x, mut max_x, mut min_y, mut max_y) =
                        (first.0, first.0, first.1, first.1);
                    for c in *s {
                        min_x = min_x.min(c.x);
                        max_x = max_x.max(c.x);
                        min_y = min_y.min(c.y);
                        max_y = max_y.max(c.y);
                    }
                    envs.push(ShellEnv {
                        idx: i,
                        env: rstar::AABB::from_corners([min_x, min_y], [max_x, max_y]),
                    });
                }
                let tree = rstar::RTree::bulk_load(envs);
                for (i, s2) in shells.iter().enumerate() {
                    let Some(pt) = s2.first().copied() else {
                        continue;
                    };
                    // Strictly-interior probe of shell j for the hole test:
                    // the first vertex lies ON a hole ring for islands that
                    // touch it, and exclusive semantics misread that as fill
                    // (island-in-hole falsely flagged NestedHoles).
                    let probe = crate::util::ring_interior_probe(s2).unwrap_or(pt);
                    let query = rstar::AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
                    let mut overlaps = false;
                    let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
                        if c.idx != i && point_in_ring_exclusive(pt, shells[c.idx]) {
                            // Hole-aware: a shell inside ANOTHER SHELL'S HOLE is
                            // an island — valid positive space, NOT nesting
                            // (GEOS isValid=true on square-with-hole ∪ island).
                            // Only a probe in the other shell's FILL (exterior
                            // minus holes) counts as nested.
                            let p_other = &self.0[c.idx];
                            let in_hole = p_other
                                .interiors()
                                .iter()
                                .any(|h| point_in_ring_exclusive(probe, &h.0));
                            if !in_hole {
                                overlaps = true;
                                return core::ops::ControlFlow::Break(());
                            }
                        }
                        core::ops::ControlFlow::<(), ()>::Continue(())
                    });
                    if overlaps {
                        errors.push(GeometryValidationError::NestedHoles);
                        return ValidationResult::invalid(errors);
                    }
                }
            }
            #[cfg(not(feature = "rstar"))]
            {
                for i in 0..shells.len() {
                    for j in 0..shells.len() {
                        if i == j {
                            continue;
                        }
                        if let Some(pt) = shells[j].first().copied()
                            && point_in_ring_exclusive(pt, shells[i])
                        {
                            // Hole-aware: island inside another shell's hole is
                            // valid positive space (GEOS isValid=true). The
                            // hole test uses a strictly-interior probe of
                            // shell j: its first vertex lies ON a hole ring
                            // for islands touching the hole, and exclusive
                            // semantics misread that as fill.
                            let probe = crate::util::ring_interior_probe(shells[j]).unwrap_or(pt);
                            let in_hole = self.0[i]
                                .interiors()
                                .iter()
                                .any(|h| point_in_ring_exclusive(probe, &h.0));
                            if !in_hole {
                                errors.push(GeometryValidationError::NestedHoles);
                                return ValidationResult::invalid(errors);
                            }
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

trait ValidateDepth {
    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> ValidationResult;
}

/// Any vertex of `probe` strictly inside `target`'s FILL (bbox-prefiltered;
/// vertices inside `exclusions` - the target component's own holes - are
/// excised and do not count). Used by the cross-component interior-overlap
/// probe: a hole ring with a vertex in another shell's fill means the two
/// interiors overlap without necessarily crossing edges (t21). Pure boundary
/// touches (point_in_ring_exclusive semantics) do not count - MP touch cases
/// stay valid.
fn ring_has_vertex_inside(
    probe: &[Coord<f64>],
    target: &[Coord<f64>],
    target_bbox: [f64; 4],
    exclusions: &[&[Coord<f64>]],
) -> bool {
    'outer: for &v in probe {
        if v.x < target_bbox[0]
            || v.x > target_bbox[2]
            || v.y < target_bbox[1]
            || v.y > target_bbox[3]
        {
            continue;
        }
        for ex in exclusions {
            if point_in_ring_exclusive(v, ex) {
                continue 'outer;
            }
        }
        if point_in_ring_exclusive(v, target) {
            return true;
        }
    }
    false
}

impl ValidateDepth for Geometry<f64> {
    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> ValidationResult {
        match self {
            Geometry::GeometryCollection(gc) => gc.validate_at_depth(depth, max_depth),
            _ => self.validate(),
        }
    }
}

impl ValidateDepth for GeometryCollection<f64> {
    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> ValidationResult {
        if depth > max_depth {
            return ValidationResult::invalid(vec![GeometryValidationError::ExcessiveNesting]);
        }
        let mut errors = Vec::new();
        for g in &self.0 {
            let r = g.validate_at_depth(depth + 1, max_depth);
            if !r.valid {
                // OGC: GeometryCollection doesn't enforce simplicity.
                // Filter out NotSimple from sub-geometry validation.
                for e in r.errors {
                    if !matches!(e, GeometryValidationError::NotSimple) {
                        errors.push(e);
                    }
                }
            }
        }
        for i in 0..self.0.len() {
            for j in (i + 1)..self.0.len() {
                if self.0[i] == self.0[j] {
                    errors.push(GeometryValidationError::DuplicatedRings);
                }
            }
        }
        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

impl GeoValidation for Geometry<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        match self {
            Geometry::Point(g) => g.validate(),
            Geometry::Line(g) => g.validate(),
            Geometry::LineString(g) => g.validate(),
            Geometry::Polygon(g) => g.validate(),
            Geometry::MultiPoint(g) => g.validate(),
            Geometry::MultiLineString(g) => g.validate(),
            Geometry::MultiPolygon(g) => g.validate(),
            Geometry::GeometryCollection(g) => g.validate(),
            Geometry::Rect(g) => g.validate(),
            Geometry::Triangle(g) => g.validate(),
        }
    }
}

impl GeoValidation for GeometryCollection<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        self.validate_at_depth(0, 100)
    }
}

#[cfg(test)]
#[path = "complex_tests.rs"]
mod tests;

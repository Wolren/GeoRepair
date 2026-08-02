//! Topological face extraction from fully-noded edge sets (Polygonizer).
//!
//! Implements the GEOS BuildArea pipeline:
//! 1. Extract all face cycles from planar graph using smallest-CCW-turn walker
//! 2. Classify as shell (CCW) or hole (CW) by winding
//! 3. Assign holes to containing shells using GEOS-style point walking
//! 4. Build face polygons (shell + holes)
//! 5. Even-parent filter (BuildArea findFaceHoles + collectWithEvenAncestors)
//! 6. Union kept faces to dissolve shared edges

use geo::{Coord, Line, LineString, MultiPolygon, Polygon, Winding};

use super::fix_ring_graph;

pub fn polygonize(lines: &[Line<f64>]) -> Vec<Polygon<f64>> {
    if lines.is_empty() { return Vec::new(); }

    let graph = fix_ring_graph::build_graph(lines);
    if graph.edges.is_empty() { return Vec::new(); }

    let face_edges = match fix_ring_graph::extract_all_faces_geos(&graph) {
        Some(faces) => faces,
        None => return Vec::new(),
    };

    // Split faces at repeated vertices (figure-8 pinch points)
    let face_edges: Vec<Vec<(usize, usize)>> = face_edges
        .into_iter()
        .flat_map(|face| fix_ring_graph::split_face_at_pinch_points(&face, &graph.edges))
        .filter(|face| face.len() >= 3)
        .collect();

    let mut rings: Vec<Vec<Coord<f64>>> = Vec::new();
    for face in &face_edges {
        let ring = face_to_ring(&graph, face);
        if ring.len() >= 4 && signed_area_x2(&ring).abs() > 0.0 {
            // Split rings that touch at a single vertex (pinch points)
            let split = split_at_pinch_point(&ring);
            rings.extend(split);
        }
    }
    if rings.is_empty() { return Vec::new(); }

    let mut shell_rings: Vec<(usize, Vec<Coord<f64>>, f64)> = Vec::new();
    let mut hole_rings: Vec<(usize, Vec<Coord<f64>>, f64)> = Vec::new();
    for (i, coords) in rings.iter().enumerate() {
        let area = signed_area_x2(coords);
        if area > 0.0 {
            shell_rings.push((i, coords.clone(), area));
        } else {
            hole_rings.push((i, coords.clone(), -area));
        }
    }
    shell_rings.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    hole_rings.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Discard universe (largest CW by abs area)
    let holes: Vec<(usize, Vec<Coord<f64>>, f64)> = if hole_rings.len() > 1 {
        hole_rings.into_iter().skip(1).collect()
    } else { Vec::new() };

    // Assign each hole to smallest containing shell (GEOS HoleAssigner)
    let mut shell_holes: Vec<Vec<LineString<f64>>> = vec![Vec::new(); shell_rings.len()];
    for (_, hole_coords, _) in &holes {
        let mut best_shell: Option<usize> = None;
        for (si, (_, shell_coords, _)) in shell_rings.iter().enumerate() {
            if hole_in_shell(hole_coords, shell_coords) {
                best_shell = Some(si);
            }
        }
        if let Some(si) = best_shell {
            let mut ls = LineString::new(hole_coords.clone());
            ls.make_cw_winding();
            shell_holes[si].push(ls);
        }
    }

    let mut face_polys: Vec<Polygon<f64>> = Vec::new();
    for (si, (_, shell_coords, _)) in shell_rings.iter().enumerate() {
        let mut shell_ls = LineString::new(shell_coords.clone());
        shell_ls.make_ccw_winding();
        let holes = std::mem::take(&mut shell_holes[si]);
        face_polys.push(Polygon::new(shell_ls, holes));
    }
    if face_polys.len() <= 1 { return face_polys; }

    build_area_filter(face_polys)
}

/// GEOS-style containment: check points one at a time for interior/exterior.
fn hole_in_shell(hole: &[Coord<f64>], shell: &[Coord<f64>]) -> bool {
    if hole.len() < 3 || shell.len() < 4 { return false; }
    for pt in hole {
        match point_class(*pt, shell) {
            PtClass::Inside => return true,
            PtClass::Outside => return false,
            PtClass::Boundary => continue,
        }
    }
    false
}

#[derive(Debug, PartialEq)]
enum PtClass { Inside, Outside, Boundary }

/// Classify a point relative to a ring (handles boundary correctly).
fn point_class(pt: Coord<f64>, ring: &[Coord<f64>]) -> PtClass {
    if ring.len() < 4 { return PtClass::Outside; }
    let n = ring.len() - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        // Check collinearity + bounding box for boundary detection
        let cross = (xj - xi) * (pt.y - yi) - (yj - yi) * (pt.x - xi);
        if cross.abs() < 1e-12
            && pt.x >= xi.min(xj) - 1e-10
            && pt.x <= xi.max(xj) + 1e-10
            && pt.y >= yi.min(yj) - 1e-10
            && pt.y <= yi.max(yj) + 1e-10
        {
            return PtClass::Boundary;
        }
    }
    // Not on boundary — do standard ray crossing
    let mut inside = false;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        let intersect = ((yi > pt.y) != (yj > pt.y))
            && (pt.x < (xj - xi) * (pt.y - yi) / (yj - yi) + xi);
        if intersect { inside = !inside; }
    }
    if inside { PtClass::Inside } else { PtClass::Outside }
}

/// BuildArea even-parent filter.
fn build_area_filter(polys: Vec<Polygon<f64>>) -> Vec<Polygon<f64>> {
    struct FaceInfo { poly: Polygon<f64>, env_area: f64, parent: Option<usize> }

    let mut faces: Vec<FaceInfo> = polys.into_iter().map(|p| {
        let env = envelope_area(&p);
        FaceInfo { poly: p, env_area: env, parent: None }
    }).collect();

    faces.sort_by(|a, b| b.env_area.partial_cmp(&a.env_area).unwrap_or(std::cmp::Ordering::Equal));

    // findFaceHoles: for each face's hole ring, match to LATER face's exterior
    let n = faces.len();
    for i in 0..n {
        let nholes = faces[i].poly.interiors().len();
        if nholes == 0 { continue; }
        for h in 0..nholes {
            let hole_ring = faces[i].poly.interiors()[h].clone();
            for f in faces.iter_mut().skip(i + 1) {
                if f.parent.is_some() { continue; }
                let ext_ring = f.poly.exterior().clone();
                if rings_equal_any_direction(&hole_ring.0, &ext_ring.0) {
                    f.parent = Some(i);
                    break;
                }
            }
        }
    }

    // collectFacesWithEvenAncestors
    let mut result: Vec<Polygon<f64>> = Vec::new();
    for fi in &faces {
        let mut count = 0usize;
        let mut cur = fi.parent;
        while let Some(pidx) = cur {
            count += 1;
            cur = faces[pidx].parent;
        }
        if count % 2 == 0 {
            result.push(fi.poly.clone());
        }
    }

    if result.len() <= 1 {
        return result;
    }
    // Dissolve shared edges via unary_union. Do NOT run merge_shells even-parent
    // again: that counts containment by exterior only and drops islands that sit
    // inside a hole of a kept shell (deep nesting L0+hole + L2 island).
    let mp = MultiPolygon::new(result);
    geo::algorithm::bool_ops::unary_union(&mp).0
}

/// Check if two rings are equal in either direction.
fn rings_equal_any_direction(r1: &[Coord<f64>], r2: &[Coord<f64>]) -> bool {
    if r1.len() < 4 || r2.len() < 4 || r1.len() != r2.len() { return false; }
    let n = r1.len() - 1;
    let first = r1[0];
    let offset = match r2.iter().position(|p| (p.x-first.x).abs()<1e-10 && (p.y-first.y).abs()<1e-10) {
        Some(o) => o, None => return false
    };
    // Forward
    let mut eq = true;
    for (k, &p1) in r1[1..n].iter().enumerate() {
        let j = (offset + k + 1) % n;
        if (p1.x - r2[j].x).abs() > 1e-10 || (p1.y - r2[j].y).abs() > 1e-10 {
            eq = false; break;
        }
    }
    if eq { return true; }
    // Backward
    eq = true;
    for (k, &p1) in r1[1..n].iter().enumerate() {
        let j = (offset + n - (k + 1)) % n;
        if (p1.x - r2[j].x).abs() > 1e-10 || (p1.y - r2[j].y).abs() > 1e-10 {
            eq = false; break;
        }
    }
    eq
}

fn envelope_area(p: &Polygon<f64>) -> f64 {
    let coords = &p.exterior().0;
    if coords.is_empty() { return 0.0; }
    let (mut min_x, mut max_x, mut min_y, mut max_y) =
        (coords[0].x, coords[0].x, coords[0].y, coords[0].y);
    for c in coords.iter().skip(1) {
        if c.x < min_x { min_x = c.x; }
        if c.x > max_x { max_x = c.x; }
        if c.y < min_y { min_y = c.y; }
        if c.y > max_y { max_y = c.y; }
    }
    (max_x - min_x) * (max_y - min_y)
}

fn face_to_ring(graph: &fix_ring_graph::Graph, face: &[(usize, usize)]) -> Vec<Coord<f64>> {
    let mut ring: Vec<Coord<f64>> = Vec::with_capacity(face.len() + 1);
    for &(_, v_idx) in face { ring.push(graph.verts[v_idx]); }
    if ring.len() >= 2 && ring.first() != ring.last() { ring.push(ring[0]); }
    ring
}

fn signed_area_x2(coords: &[Coord<f64>]) -> f64 {
    let n = coords.len();
    if n < 3 { return 0.0; }
    let end = if coords.first() == coords.last() { n - 1 } else { n };
    let mut sum = 0.0_f64;
    for i in 0..end - 1 {
        sum += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }
    sum += coords[end - 1].x * coords[0].y - coords[0].x * coords[end - 1].y;
    sum
}

/// Split a ring at pinch points (vertices that appear more than once),
/// producing separate rings. A pinch point is a vertex where the ring
/// touches itself — the same coordinate appears at two different indices.
fn split_at_pinch_point(ring: &[Coord<f64>]) -> Vec<Vec<Coord<f64>>> {
    if ring.len() < 4 {
        return vec![ring.to_vec()];
    }
    let interior_end = if ring.first() == ring.last() { ring.len() - 1 } else { ring.len() };
    let mut dup: Option<usize> = None;
    for i in 1..interior_end {
        for j in 0..i {
            if (ring[i].x - ring[j].x).abs() < 1e-12 && (ring[i].y - ring[j].y).abs() < 1e-12 {
                dup = Some(j);
                break;
            }
        }
        if dup.is_some() { break; }
    }
    if let Some(dup_idx) = dup {
        let mut positions: Vec<usize> = Vec::new();
        for i in 0..interior_end {
            if (ring[i].x - ring[dup_idx].x).abs() < 1e-12 && (ring[i].y - ring[dup_idx].y).abs() < 1e-12 {
                positions.push(i);
            }
        }
        if positions.len() >= 2 {
            let mut result: Vec<Vec<Coord<f64>>> = Vec::new();
            for w in positions.windows(2) {
                let mut new_ring: Vec<Coord<f64>> = ring[w[0]..=w[1]].to_vec();
                if new_ring.first() != new_ring.last() { new_ring.push(new_ring[0]); }
                if new_ring.len() >= 4 && signed_area_x2(&new_ring).abs() > 0.0 { result.push(new_ring); }
            }
            let last_pos = positions[positions.len() - 1];
            if last_pos + 1 < interior_end {
                let mut close_ring: Vec<Coord<f64>> = ring[last_pos..].to_vec();
                close_ring.extend_from_slice(&ring[0..=positions[0]]);
                if close_ring.first() != close_ring.last() { close_ring.push(close_ring[0]); }
                if close_ring.len() >= 4 && signed_area_x2(&close_ring).abs() > 0.0 { result.push(close_ring); }
            }
            if !result.is_empty() { return result; }
        }
    }
    vec![ring.to_vec()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Line;
    use crate::validation::GeoValidation;

    #[test]
    fn test_simple_square() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 }),
            Line::new(Coord { x: 5.0, y: 0.0 }, Coord { x: 5.0, y: 5.0 }),
            Line::new(Coord { x: 5.0, y: 5.0 }, Coord { x: 0.0, y: 5.0 }),
            Line::new(Coord { x: 0.0, y: 5.0 }, Coord { x: 0.0, y: 0.0 }),
        ];
        let polys = polygonize(&lines);
        assert_eq!(polys.len(), 1);
        assert!(polys[0].is_valid());
    }

    #[test]
    fn test_two_squares() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 }),
            Line::new(Coord { x: 5.0, y: 0.0 }, Coord { x: 5.0, y: 5.0 }),
            Line::new(Coord { x: 5.0, y: 5.0 }, Coord { x: 0.0, y: 5.0 }),
            Line::new(Coord { x: 0.0, y: 5.0 }, Coord { x: 0.0, y: 0.0 }),
            Line::new(Coord { x: 10.0, y: 10.0 }, Coord { x: 15.0, y: 10.0 }),
            Line::new(Coord { x: 15.0, y: 10.0 }, Coord { x: 15.0, y: 15.0 }),
            Line::new(Coord { x: 15.0, y: 15.0 }, Coord { x: 10.0, y: 15.0 }),
            Line::new(Coord { x: 10.0, y: 15.0 }, Coord { x: 10.0, y: 10.0 }),
        ];
        let polys = polygonize(&lines);
        assert_eq!(polys.len(), 2);
    }

    #[test]
    fn test_square_with_hole() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 }),
            Line::new(Coord { x: 10.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }),
            Line::new(Coord { x: 10.0, y: 10.0 }, Coord { x: 0.0, y: 10.0 }),
            Line::new(Coord { x: 0.0, y: 10.0 }, Coord { x: 0.0, y: 0.0 }),
            Line::new(Coord { x: 3.0, y: 3.0 }, Coord { x: 3.0, y: 7.0 }),
            Line::new(Coord { x: 3.0, y: 7.0 }, Coord { x: 7.0, y: 7.0 }),
            Line::new(Coord { x: 7.0, y: 7.0 }, Coord { x: 7.0, y: 3.0 }),
            Line::new(Coord { x: 7.0, y: 3.0 }, Coord { x: 3.0, y: 3.0 }),
        ];
        let polys = polygonize(&lines);
        assert!(!polys.is_empty());
        assert!(polys[0].is_valid());
        let has_hole = polys.iter().any(|p| p.interiors().len() >= 1);
        assert!(has_hole);
    }

    #[test]
    fn test_deep_nesting() {
        // L0 outer, L1 hole (CW), L2 island (CCW inside hole)
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 20.0, y: 0.0 }),
            Line::new(Coord { x: 20.0, y: 0.0 }, Coord { x: 20.0, y: 20.0 }),
            Line::new(Coord { x: 20.0, y: 20.0 }, Coord { x: 0.0, y: 20.0 }),
            Line::new(Coord { x: 0.0, y: 20.0 }, Coord { x: 0.0, y: 0.0 }),
            // L1 CW hole: E, S, W, N
            Line::new(Coord { x: 3.0, y: 3.0 }, Coord { x: 17.0, y: 3.0 }),
            Line::new(Coord { x: 17.0, y: 3.0 }, Coord { x: 17.0, y: 17.0 }),
            Line::new(Coord { x: 17.0, y: 17.0 }, Coord { x: 3.0, y: 17.0 }),
            Line::new(Coord { x: 3.0, y: 17.0 }, Coord { x: 3.0, y: 3.0 }),
            // L2 CCW island: E, N, W, S
            Line::new(Coord { x: 6.0, y: 6.0 }, Coord { x: 14.0, y: 6.0 }),
            Line::new(Coord { x: 14.0, y: 6.0 }, Coord { x: 14.0, y: 14.0 }),
            Line::new(Coord { x: 14.0, y: 14.0 }, Coord { x: 6.0, y: 14.0 }),
            Line::new(Coord { x: 6.0, y: 14.0 }, Coord { x: 6.0, y: 6.0 }),
        ];
        let polys = polygonize(&lines);
        for p in &polys {
            assert!(p.is_valid(), "Each polygon should be valid");
        }
        assert_eq!(polys.len(), 2, "Should have 2 polygons (L0+hole + L2 island)");
        let l0 = polys.iter().find(|p| p.interiors().len() == 1);
        let l2 = polys.iter().find(|p| p.interiors().is_empty());
        assert!(l0.is_some(), "Should have a polygon with a hole (L0)");
        assert!(l2.is_some(), "Should have a polygon with no holes (L2 island)");
    }
}

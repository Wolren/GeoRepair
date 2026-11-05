use alloc::vec::Vec;
use geo::{Coord, Line, LineString, MultiPolygon, Polygon, Winding};
use rustc_hash::{FxHashMap, FxHashSet};

use super::fix_ring_graph::{build_graph, extract_all_faces_geos};

/// geos BuildArea port.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn build_area(lines: &[geo::Line<f64>]) -> Option<MultiPolygon<f64>> {
    build_area_impl(lines, false).map(|(area, _)| area)
}

/// The dissolved area plus its boundary segments. The MakeValid loop subtracts those
/// segments from the remaining cut edges; dissolve merges collinear runs, so ring
/// windows cannot be matched back to the noded input.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn build_area_with_boundary(
    lines: &[geo::Line<f64>],
) -> Option<(MultiPolygon<f64>, Vec<Line<f64>>)> {
    build_area_impl(lines, true)
}

fn build_area_impl(
    lines: &[geo::Line<f64>],
    dissolve: bool,
) -> Option<(MultiPolygon<f64>, Vec<Line<f64>>)> {
    if lines.is_empty() {
        return Some((MultiPolygon::new(Vec::new()), Vec::new()));
    }
    let graph = build_graph(lines);
    if graph.edges.is_empty() {
        return Some((MultiPolygon::new(Vec::new()), Vec::new()));
    }
    // None as a fallback signal (make_valid.rs polygonizer_fallback, fix_ring.rs) -
    // keep the semantics
    let faces = extract_all_faces_geos(&graph)?;
    if faces.is_empty() {
        return Some((MultiPolygon::new(Vec::new()), Vec::new()));
    }

    struct Face {
        ring: Vec<Coord<f64>>, // as walked (shells CW, holes CCW)
        envelope_area: f64,
        is_shell: bool,
        parent: Option<usize>,
    }
    let mut faces_out: Vec<Face> = Vec::new();
    for face in &faces {
        let mut ring: Vec<Coord<f64>> = face.iter().map(|&(_, to)| graph.verts[to]).collect();
        if ring.len() < 3 {
            continue;
        }
        if ring.first() != ring.last() {
            ring.push(ring[0]);
        }
        let mut sa = 0.0;
        for w in ring.windows(2) {
            sa += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        sa *= 0.5;
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for c in &ring {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        faces_out.push(Face {
            envelope_area: (max_x - min_x) * (max_y - min_y),
            ring,
            is_shell: sa < 0.0, // GEOS: CW = shell
            parent: None,
        });
    }
    if faces_out.is_empty() {
        return Some((MultiPolygon::new(Vec::new()), Vec::new()));
    }

    // Step 2: sort by envelope area desc (GEOS CompareByEnvarea)
    faces_out.sort_by(|a, b| {
        b.envelope_area
            .partial_cmp(&a.envelope_area)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    let shell_idx: Vec<usize> = faces_out
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_shell)
        .map(|(i, _)| i)
        .collect();
    for h in 0..faces_out.len() {
        if faces_out[h].is_shell {
            continue;
        }
        let Some(probe) = ring_probe(&faces_out[h].ring) else {
            continue;
        };
        let mut best: Option<(usize, f64)> = None;
        for &s in &shell_idx {
            if s == h {
                continue;
            }
            if !point_in_ring(&faces_out[s].ring, probe) {
                continue;
            }
            let ea = faces_out[s].envelope_area;
            if best.is_none_or(|(_, b)| ea < b) {
                best = Some((s, ea));
            }
        }
        if let Some((s, _)) = best {
            faces_out[h].parent = Some(s);
        }
    }

    let mut owner_of_ring: FxHashMap<Vec<(u64, u64)>, usize> = FxHashMap::default();
    for f in &faces_out {
        if let Some(s) = f.parent {
            owner_of_ring.insert(crate::util::ring_fingerprint(&f.ring), s);
        }
    }
    let ancestor_count = |idx: usize| -> usize {
        let mut count = 0usize;
        let mut cur = idx;
        let mut guard = 0usize;
        loop {
            let fp = crate::util::ring_fingerprint(&faces_out[cur].ring);
            match owner_of_ring.get(&fp) {
                Some(&p) if p != cur => {
                    count += 1;
                    cur = p;
                }
                _ => break,
            }
            guard += 1;
            if guard > faces_out.len() {
                break;
            }
        }
        count
    };

    let mut shell_holes: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for (h, f) in faces_out.iter().enumerate() {
        if let Some(s) = f.parent {
            shell_holes.entry(s).or_default().push(h);
        }
    }
    let mut kept: Vec<Polygon<f64>> = Vec::new();
    for &s in &shell_idx {
        if ancestor_count(s) % 2 != 0 {
            continue;
        }
        let mut shell = LineString::new(faces_out[s].ring.clone());
        if dissolve {
            shell.make_ccw_winding();
        }
        let mut holes: Vec<LineString<f64>> = Vec::new();
        if let Some(hs) = shell_holes.get(&s) {
            for &h in hs {
                let mut hole = LineString::new(faces_out[h].ring.clone());
                if dissolve {
                    hole.make_cw_winding();
                }
                holes.push(hole);
            }
        }
        kept.push(Polygon::new(shell, holes));
    }

    if !dissolve {
        return Some((MultiPolygon::new(kept), Vec::new()));
    }

    let mut seg_faces: FxHashMap<(u64, u64, u64, u64), usize> = FxHashMap::default();
    for p in &kept {
        let mut seen: FxHashSet<(u64, u64, u64, u64)> = FxHashSet::default();
        for ring in core::iter::once(p.exterior()).chain(p.interiors()) {
            for w in ring.0.windows(2) {
                if w[0] == w[1] {
                    continue;
                }
                let k = seg_key(w[0], w[1]);
                if seen.insert(k) {
                    *seg_faces.entry(k).or_insert(0) += 1;
                }
            }
        }
    }
    let mut boundary: Vec<Line<f64>> = Vec::new();
    let mut emitted: FxHashSet<(u64, u64, u64, u64)> = FxHashSet::default();
    for p in &kept {
        for ring in core::iter::once(p.exterior()).chain(p.interiors()) {
            for w in ring.0.windows(2) {
                if w[0] == w[1] {
                    continue;
                }
                let k = seg_key(w[0], w[1]);
                if seg_faces.get(&k) == Some(&1) && emitted.insert(k) {
                    boundary.push(Line::new(w[0], w[1]));
                }
            }
        }
    }

    if kept.is_empty() {
        return Some((MultiPolygon::new(Vec::new()), Vec::new()));
    }
    if kept.len() == 1 {
        return Some((MultiPolygon::new(kept), boundary));
    }
    // GEOS dissolves the kept faces with CascadedPolygonUnion; without this a
    // self-crossing ring returns its atomic faces (pentagram: 6 parts)
    let mp = MultiPolygon::new(kept);
    #[cfg(feature = "std")]
    let dissolved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        geo::algorithm::bool_ops::unary_union(&mp)
    }))
    .unwrap_or(mp);
    #[cfg(not(feature = "std"))]
    let dissolved = geo::algorithm::bool_ops::unary_union(&mp);
    Some((dissolved, boundary))
}

/// Orientation-insensitive bit key for a segment.
fn seg_key(a: Coord<f64>, b: Coord<f64>) -> (u64, u64, u64, u64) {
    let (ax, ay, bx, by) = (a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits());
    if (ax, ay) <= (bx, by) {
        (ax, ay, bx, by)
    } else {
        (bx, by, ax, ay)
    }
}

/// Fingerprint of a ring: sorted coordinate bit pairs, closure removed.
fn ring_probe(ring: &[Coord<f64>]) -> Option<Coord<f64>> {
    let (v0, v1) = (*ring.first()?, *ring.get(1)?);
    let mid = Coord {
        x: (v0.x + v1.x) * 0.5,
        y: (v0.y + v1.y) * 0.5,
    };
    let scale = (mid.x.abs().max(mid.y.abs())).max(1.0);
    let eps = 1e-9 * scale;
    let dx = v1.x - v0.x;
    let dy = v1.y - v0.y;
    let len = (dx * dx + dy * dy).sqrt().max(1e-12);
    let (nx, ny) = (-dy / len, dx / len);
    Some(Coord {
        x: mid.x - nx * eps,
        y: mid.y - ny * eps,
    })
}

/// Even-odd point-in-ring test (boundary counted as inside).
fn point_in_ring(ring: &[Coord<f64>], pt: Coord<f64>) -> bool {
    let mut inside = false;
    let n = ring.len();
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if (a.y > pt.y) != (b.y > pt.y) {
            let xint = (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x;
            if pt.x < xint {
                inside = !inside;
            }
        }
    }
    inside
}

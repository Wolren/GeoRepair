use geo::{Coord, LineString, MultiPolygon, Polygon};
use rustc_hash::FxHashMap;

use super::fix_ring_graph::{build_graph, extract_all_faces_geos};

/// ---------------------------------------------------------------------------
/// GEOS BuildArea port (BuildArea.cpp + Polygonizer.cpp):
///
/// 1. Polygonize noded linework into face rings (extract_all_faces_geos).
/// 2. Classify by ring orientation (EdgeRing::computeHole):
///    CW (negative signed area) = SHELL, CCW (positive) = HOLE.
/// 3. Assign each hole to the SMALLEST containing shell (findEdgeRing-
///    Containing by envelope area).
/// 4. Build polygons: shell ring + assigned holes.
/// 5. Even-parent filter (collectFacesWithEvenAncestors): a face F is a
///    parent of G if one of F's hole rings equals G's exterior ring
///    (direction/rotation-insensitive). Keep faces with an EVEN ancestor
///    count; drop odd (they sit in hole position).
///
/// Verified bit-identical to GEOS on multi-crossing rings.
/// ---------------------------------------------------------------------------
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn build_area(lines: &[geo::Line<f64>]) -> Option<MultiPolygon<f64>> {
    if lines.is_empty() {
        return Some(MultiPolygon::new(Vec::new()));
    }
    let graph = build_graph(lines);
    if graph.edges.is_empty() {
        return Some(MultiPolygon::new(Vec::new()));
    }
    // None = no walkable faces (open linework without cycles). Callers use
    // None as a fallback signal (make_valid.rs polygonizer_fallback,
    // fix_ring.rs) - keep the semantics: un-closable linework is a "try
    // something else" signal, NOT an empty result. The GEOS buildarea
    // oracle in the XML suite maps None to empty itself.
    let faces = extract_all_faces_geos(&graph)?;
    if faces.is_empty() {
        return Some(MultiPolygon::new(Vec::new()));
    }

    // ---- Step 1: rings + orientation classification -------------------------
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
        return Some(MultiPolygon::new(Vec::new()));
    }

    // ---- Step 2: sort by envelope area DESC (GEOS CompareByEnvarea) --------
    faces_out.sort_by(|a, b| {
        b.envelope_area
            .partial_cmp(&a.envelope_area)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ---- Step 3: assign holes to smallest containing shell -----------------
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

    // ---- Step 4: even-parent filter (ring equality, direction-agnostic) ----
    // owner_of_ring: fingerprint of a hole ring -> the shell that owns it.
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
        let mut holes: Vec<LineString<f64>> = Vec::new();
        if let Some(hs) = shell_holes.get(&s) {
            for &h in hs {
                holes.push(LineString::new(faces_out[h].ring.clone()));
            }
        }
        kept.push(Polygon::new(
            LineString::new(faces_out[s].ring.clone()),
            holes,
        ));
    }

    Some(MultiPolygon::new(kept))
}

/// Fingerprint of a ring: sorted coordinate bit pairs, closure removed.
/// Direction/rotation-insensitive - GEOS ringsEqualAnyDirection equivalent.
/// Interior probe point for a ring: midpoint of first edge nudged toward the
/// ring's interior (right of the directed edge - walker convention).
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

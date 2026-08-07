
use alloc::vec::Vec;
use geo::Coord;
use rstar::{RTree, RTreeObject, AABB};

use crate::orient::orient2d_fast;
#[cfg(feature = "std")]
use crate::util::ProfileClock;

/// Edge with 2D bounding envelope for R-tree spatial indexing.
struct EdgeEnvelope {
    index: u32,
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for EdgeEnvelope {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

/// Build bounding envelope for edge i, expanded so that nearly-vertical edges
/// and near-touching intervals are still detected.
fn edge_envelope(coords: &[Coord<f64>], i: usize) -> AABB<[f64; 2]> {
    let lo_x = coords[i].x.min(coords[i + 1].x);
    let hi_x = coords[i].x.max(coords[i + 1].x);
    let lo_y = coords[i].y.min(coords[i + 1].y);
    let hi_y = coords[i].y.max(coords[i + 1].y);
    let ext = f64::EPSILON * 100.0 * (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0);
    AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext])
}

/// PROPER self-crossing detection over a whole polygon (exterior + holes)
/// using an R-tree over edge bounding boxes. Interior-interior crossings
/// only - shared endpoints (hole touching shell at a vertex, which GEOS
/// makeValid legitimately emits) do NOT count.
///
/// This is the sweep variant of `has_proper_self_crossing`: O(n log n)
/// average instead of the brute-force O(n²) pair loop, which is fatal on
/// large rings (measured: 59k verts → 9.1s brute force, 181k verts → 143s).
pub(crate) fn has_proper_self_crossing_sweep(
    coords: &[Coord<f64>],
    ring_offsets: &[usize],
    _eps: f64,
) -> bool {
    let n_edges = coords.len().saturating_sub(1);
    if n_edges < 4 {
        return false;
    }
    let edges: Vec<EdgeEnvelope> = (0..n_edges)
        .filter(|&i| !is_phantom_segment(i, ring_offsets))
        .map(|i| EdgeEnvelope {
            index: i as u32,
            envelope: edge_envelope(coords, i),
        })
        .collect();

    let tree = RTree::bulk_load(edges);

    for i in 0..n_edges {
        // Skip phantom segments (closure-vertex boundary lines that don't
        // exist in the geometry) when QUERYING too.
        if is_phantom_segment(i, ring_offsets) {
            continue;
        }
        let query_env = edge_envelope(coords, i);
        let result = tree.locate_in_envelope_intersecting_int(query_env, |candidate| {
            let j = candidate.index as usize;
            if j <= i {
                return ::core::ops::ControlFlow::Continue(());
            }
            // Same-ring adjacent segments share an endpoint by construction.
            if segments_adjacent_in_ring(i, j, ring_offsets) {
                return ::core::ops::ControlFlow::Continue(());
            }
            if super::fix_ring::segments_properly_cross_seg(
                coords[i],
                coords[i + 1],
                coords[j],
                coords[j + 1],
            ) {
                #[cfg(feature = "std")]
                if std::env::var("DIAG_SWEEP_CROSS").is_ok() {
                    eprintln!(
                        "SWEEP CROSS i={i} j={j} a=({:.6},{:.6})-({:.6},{:.6}) b=({:.6},{:.6})-({:.6},{:.6})",
                        coords[i].x, coords[i].y, coords[i + 1].x, coords[i + 1].y,
                        coords[j].x, coords[j].y, coords[j + 1].x, coords[j + 1].y
                    );
                }
                ::core::ops::ControlFlow::Break(())
            } else if ring_of_segment(i, ring_offsets) == ring_of_segment(j, ring_offsets)
                && crate::validation::edges_vertex_on_edge(
                    coords[i],
                    coords[i + 1],
                    coords[j],
                    coords[j + 1],
                )
            {
                // Same-ring vertex-on-edge self-touch (T-junction): GEOS
                // rejects a ring vertex on a non-adjacent edge (Test 22).
                // Cross-ring pairs stay untouched (hole vertex on shell
                // edge is a VALID OGC touch).
                ::core::ops::ControlFlow::Break(())
            } else {
                ::core::ops::ControlFlow::Continue(())
            }
        });
        if result.is_break() {
            return true;
        }
    }
    false
}

/// True if segment index `s` is a PHANTOM - the last index before a ring
/// boundary, which spans (closure vertex of ring r) → (first vertex of ring
/// r+1). That line does not exist in the geometry (the flat array just
/// concatenates rings) and must never be tested. Note: the closure vertex of
/// the LAST ring is covered by the ring's final real segment (end-2 → end-1),
/// so no phantom exists after the last ring.
fn is_phantom_segment(s: usize, ring_offsets: &[usize]) -> bool {
    ring_offsets.iter().skip(1).any(|&off| s + 1 == off)
}

/// Ring index (0 = exterior) of segment `s` in the flat coord slice.
fn ring_of_segment(s: usize, ring_offsets: &[usize]) -> usize {
    let mut r = 0;
    for (k, &off) in ring_offsets.iter().enumerate() {
        if off > s {
            break;
        }
        r = k;
    }
    r
}

/// True if segment indices `i` and `j` are adjacent within the SAME ring
/// (consecutive segments, or first/last of a closed ring). Ring offsets are
/// the start index of each ring in the flat coord slice (offset 0 = exterior).
fn segments_adjacent_in_ring(i: usize, j: usize, ring_offsets: &[usize]) -> bool {
    if is_phantom_segment(j, ring_offsets) {
        return true;
    }
    // Find the ring containing i (offsets are sorted; last ring runs to end).
    let mut r = 0;
    for (k, &off) in ring_offsets.iter().enumerate() {
        if off > i {
            break;
        }
        r = k;
    }
    let start = ring_offsets[r];
    let end = ring_offsets.get(r + 1).copied().unwrap_or(usize::MAX);
    // Segment k spans [k, k+1); the ring's real segments are [start, end-2]
    // (end-1 is the phantom spanning into the next ring).
    let seg_end = end.saturating_sub(1);
    let in_ring = |s: usize| s >= start && s < seg_end;
    if !in_ring(j) {
        return false;
    }
    if j == i + 1 {
        return true;
    }
    // First and last real segment of a closed ring share the closure vertex.
    j == start && i == seg_end - 1
}


/// Edge envelope as a flat [min_x, max_x, min_y, max_y] with the same
/// epsilon expansion as [`edge_envelope`].
fn edge_env_array(coords: &[Coord<f64>], i: usize) -> [f64; 4] {
    let lo_x = coords[i].x.min(coords[i + 1].x);
    let hi_x = coords[i].x.max(coords[i + 1].x);
    let lo_y = coords[i].y.min(coords[i + 1].y);
    let hi_y = coords[i].y.max(coords[i + 1].y);
    let ext = f64::EPSILON * 100.0 * (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0);
    [lo_x - ext, hi_x + ext, lo_y - ext, hi_y + ext]
}

#[inline]
fn env_intersect(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[1] && a[1] >= b[0] && a[2] <= b[3] && a[3] >= b[2]
}

/// Node of the parallel-STR index.
#[derive(Clone, Copy)]
struct StrNode {
    env: [f64; 4],
    start: u32,
    len: u32,
    is_leaf: bool,
}

/// Multi-level STR index built with rayon-parallel sorts.
///
/// Replaces rstar's serial STR bulk_load (~20ms on a 260k-edge giant shell)
/// with a parallel sort + pack (~8ms). The structure mirrors rstar's
/// bulk_load: at each level the ranges are sorted by the level's axis
/// center (x, then y, alternating) and split into sqrt(leaves-below)
/// children, recursing until ranges hold at most [`STR_LEAF_CAP`] edges.
/// A 2-level STR is NOT enough here — y-band leaves of a ring span the
/// whole slab in x, so every query visits ~sqrt(n) leaves (measured 8x
/// worse than rstar); the multi-level tree keeps leaf unions tight.
struct StrIndex {
    /// levels[0] = root, last level = leaves. A node's children are
    /// `levels[l+1][start .. start+len]`; a leaf's members are
    /// `order[start .. start+len]`.
    levels: Vec<Vec<StrNode>>,
    order: Vec<u32>,
}

const STR_LEAF_CAP: usize = 32;

impl StrIndex {
    fn build(envs: &[[f64; 4]]) -> Self {
        let n = envs.len();
        let mut order: Vec<u32> = (0..n as u32).collect();
        let cx: Vec<f64> = envs.iter().map(|e| (e[0] + e[1]) * 0.5).collect();
        let cy: Vec<f64> = envs.iter().map(|e| (e[2] + e[3]) * 0.5).collect();

        let by_axis = |axis: usize, a: &u32, b: &u32| {
            let (ca, cb) = if axis == 0 {
                (cx[*a as usize], cx[*b as usize])
            } else {
                (cy[*a as usize], cy[*b as usize])
            };
            ca.partial_cmp(&cb).unwrap_or(::core::cmp::Ordering::Equal)
        };

        // Pass 1 (top-down): sort each level's ranges by the level's axis,
        // then split every range into sqrt(leaves-below) children. Record
        // each range's parent node index for the child-pointer pass.
        let mut level_ranges: Vec<Vec<::core::ops::Range<usize>>> = Vec::new();
        let mut level_parents: Vec<Vec<u32>> = Vec::new();
        let mut ranges: Vec<::core::ops::Range<usize>> = ::core::iter::once(0..n).collect();
        let mut parents: Vec<u32> = vec![u32::MAX];
        let mut axis = 0usize;
        while ranges.iter().any(|r| r.len() > STR_LEAF_CAP) {
            #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
            {
                use rayon::prelude::*;
                // Ranges partition `order` without overlap (consecutive
                // splits of a partition), so each worker may sort its own
                // disjoint slice. The base pointer travels as usize (Sync).
                let order_base = order.as_mut_ptr() as usize;
                ranges.par_iter().for_each(|r| {
                    // SAFETY: disjoint ranges, one worker per range.
                    let slice: &mut [u32] = unsafe {
                        std::slice::from_raw_parts_mut(
                            (order_base as *mut u32).add(r.start),
                            r.len(),
                        )
                    };
                    slice.sort_unstable_by(|a, b| by_axis(axis, a, b));
                });
            }
            #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
            {
                for r in &ranges {
                    order[r.clone()].sort_unstable_by(|a, b| by_axis(axis, a, b));
                }
            }
            let mut next: Vec<::core::ops::Range<usize>> = Vec::new();
            let mut next_parents: Vec<u32> = Vec::new();
            for (pi, r) in ranges.iter().enumerate() {
                let leaves_below = r.len().div_ceil(STR_LEAF_CAP).max(1);
                let s = (leaves_below as f64).sqrt().ceil().max(2.0) as usize;
                let child_len = r.len().div_ceil(s).max(1);
                let mut c = r.start;
                while c < r.end {
                    let e = (c + child_len).min(r.end);
                    next.push(c..e);
                    next_parents.push(pi as u32);
                    c = e;
                }
            }
            level_ranges.push(::core::mem::take(&mut ranges));
            level_parents.push(::core::mem::take(&mut parents));
            ranges = next;
            parents = next_parents;
            axis ^= 1;
        }
        level_ranges.push(ranges);
        level_parents.push(parents);

        // Pass 2 (bottom-up): leaves first, then internal levels. Node k's
        // children are the contiguous block in the level below whose parent
        // indices equal k (ranges are created in parent order).
        let n_levels = level_ranges.len();
        let mut levels: Vec<Vec<StrNode>> = Vec::with_capacity(n_levels);

        let leaf_ranges = level_ranges.pop().unwrap();
        let mut next_parents = level_parents.pop().unwrap();
        let leaves: Vec<StrNode> = leaf_ranges
            .iter()
            .map(|r| {
                let mut env = [f64::MAX, f64::MIN, f64::MAX, f64::MIN];
                for &idx in &order[r.clone()] {
                    let ee = envs[idx as usize];
                    env[0] = env[0].min(ee[0]);
                    env[1] = env[1].max(ee[1]);
                    env[2] = env[2].min(ee[2]);
                    env[3] = env[3].max(ee[3]);
                }
                StrNode {
                    env,
                    start: r.start as u32,
                    len: r.len() as u32,
                    is_leaf: true,
                }
            })
            .collect();
        levels.push(leaves);

        while let Some(ranges) = level_ranges.pop() {
            let node_count = ranges.len();
            // Child blocks into the already-built level below.
            let mut counts = vec![0u32; node_count];
            for &p in &next_parents {
                counts[p as usize] += 1;
            }
            let mut starts = Vec::with_capacity(node_count);
            let mut cum = 0u32;
            for &c in &counts {
                starts.push(cum);
                cum += c;
            }
            let nodes: Vec<StrNode> = ranges
                .iter()
                .enumerate()
                .map(|(k, _r)| {
                    let cs = starts[k];
                    let cl = counts[k];
                    let mut env = [f64::MAX, f64::MIN, f64::MAX, f64::MIN];
                    for child in &levels.last().unwrap()[cs as usize..(cs + cl) as usize] {
                        let ce = child.env;
                        env[0] = env[0].min(ce[0]);
                        env[1] = env[1].max(ce[1]);
                        env[2] = env[2].min(ce[2]);
                        env[3] = env[3].max(ce[3]);
                    }
                    StrNode {
                        env,
                        start: cs,
                        len: cl,
                        is_leaf: false,
                    }
                })
                .collect();
            levels.push(nodes);
            next_parents = level_parents.pop().unwrap();
        }
        levels.reverse();

        StrIndex { levels, order }
    }

    /// Visit every candidate edge j whose leaf envelope intersects `q`,
    /// calling `visit(j)` until it returns `true`. Fixed-array stack
    /// (depth ~5, siblings < 256) — no allocation per query.
    #[inline]
    fn query(&self, q: [f64; 4], mut visit: impl FnMut(u32) -> bool) -> bool {
        let mut stack = [(0u32, 0u32); 512]; // (level, node idx)
        let mut sp = 0usize;
        stack[sp] = (0, 0);
        sp += 1;
        while sp > 0 {
            sp -= 1;
            let (l, ni) = stack[sp];
            let node = self.levels[l as usize][ni as usize];
            if !env_intersect(node.env, q) {
                continue;
            }
            if node.is_leaf {
                for k in node.start..node.start + node.len {
                    if visit(self.order[k as usize]) {
                        return true;
                    }
                }
            } else {
                for c in (node.start..node.start + node.len).rev() {
                    stack[sp] = (l + 1, c);
                    sp += 1;
                }
            }
        }
        false
    }
}

/// Self-intersection detection using a parallel-STR index over edge
/// bounding boxes.
///
/// Builds the index (rayon-parallel STR sort + pack, ~8ms for 260k edges vs
/// ~20ms rstar bulk_load), then queries each edge to find candidates whose
/// bounding boxes overlap. Only checks each pair once (j > i). Early-exits
/// on first intersection. The 2D envelope prunes on both x and y, which is
/// strictly more selective than a 1D interval tree — especially for radial
/// geometries like star-bursts where edges in different quadrants have
/// disjoint y-ranges.
pub(crate) fn has_self_intersections(coords: &[Coord<f64>], eps: f64) -> bool {
    let n = coords.len();
    let n_edges = n.saturating_sub(1);

    let envs: Vec<[f64; 4]> = (0..n_edges).map(|i| edge_env_array(coords, i)).collect();
    #[cfg(feature = "std")]
    let t_build = ProfileClock::start();
    let tree = StrIndex::build(&envs);
    #[cfg(feature = "std")]
    let dt_build = core::time::Duration::from_nanos(t_build.ns());
    #[cfg(feature = "std")]
    if std::env::var("DIAG_SI").is_ok() {
        eprintln!(
            "SI n={n_edges} build={:.1}ms levels={} leaves={}",
            dt_build.as_secs_f64() * 1e3,
            tree.levels.len(),
            tree.levels.last().map_or(0, |l| l.len())
        );
    }

    // Per-edge queries are independent — parallelize with find_any, which
    // short-circuits the whole search on the first crossing while still
    // splitting the 260k-edge giant shells across workers. Same pair
    // semantics as the serial loop below (identical closure body).
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        let t_q = std::time::Instant::now();
        let found = (0..n_edges).into_par_iter().find_any(|&i| {
            tree.query(envs[i], |j| {
                let j = j as usize;
                if j <= i {
                    return false;
                }
                if i.abs_diff(j) <= 1 || (i == 0 && j == n_edges - 1) {
                    return false;
                }
                if coords[i] == coords[j]
                    && orient2d_fast(coords[i], coords[i + 1], coords[j + 1]) != 0.0
                {
                    return false;
                }
                if coords[i] == coords[j + 1]
                    && orient2d_fast(coords[i], coords[i + 1], coords[j]) != 0.0
                {
                    return false;
                }
                if coords[i + 1] == coords[j]
                    && orient2d_fast(coords[i + 1], coords[i], coords[j + 1]) != 0.0
                {
                    return false;
                }
                if coords[i + 1] == coords[j + 1]
                    && orient2d_fast(coords[i + 1], coords[i], coords[j]) != 0.0
                {
                    return false;
                }
                super::fix_ring::check_edge_pair(coords, i, j, eps)
            })
        })
        .is_some();
        #[cfg(feature = "std")]
        if std::env::var("DIAG_SI").is_ok() {
            eprintln!(
                "SI query={:.1}ms found={found}",
                t_q.elapsed().as_secs_f64() * 1e3
            );
        }
        found
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        for i in 0..n_edges {
            if tree.query(envs[i], |j| {
                let j = j as usize;
                if j <= i {
                    return false;
                }
                if i.abs_diff(j) <= 1 || (i == 0 && j == n_edges - 1) {
                    return false;
                }
                if coords[i] == coords[j]
                    && orient2d_fast(coords[i], coords[i + 1], coords[j + 1]) != 0.0
                {
                    return false;
                }
                if coords[i] == coords[j + 1]
                    && orient2d_fast(coords[i], coords[i + 1], coords[j]) != 0.0
                {
                    return false;
                }
                if coords[i + 1] == coords[j]
                    && orient2d_fast(coords[i + 1], coords[i], coords[j + 1]) != 0.0
                {
                    return false;
                }
                if coords[i + 1] == coords[j + 1]
                    && orient2d_fast(coords[i + 1], coords[i], coords[j]) != 0.0
                {
                    return false;
                }
                super::fix_ring::check_edge_pair(coords, i, j, eps)
            }) {
                return true;
            }
        }
        false
    }
}

/// Find any proper crossing OTHER than the pair (i0, j0), using the
/// parallel-STR index. O(n log n) — the brute-force variant is O(n^2) and
/// costs ~90ms on a 260k-edge giant shell (the sweep branch of try_fast_fix
/// has no GRID_THRESHOLD_N bound, so the small-ring comment on the brute
/// force does not apply there).
pub(crate) fn find_second_intersection(
    coords: &[Coord<f64>],
    eps: f64,
    i0: usize,
    j0: usize,
) -> Option<(usize, usize, Coord<f64>)> {
    let n_edges = coords.len().saturating_sub(1);
    let envs: Vec<[f64; 4]> = (0..n_edges).map(|i| edge_env_array(coords, i)).collect();
    let tree = StrIndex::build(&envs);
    for i in 0..n_edges {
        let mut out: Option<(usize, usize, Coord<f64>)> = None;
        tree.query(envs[i], |j| {
            let j = j as usize;
            if j <= i {
                return false;
            }
            // The pair try_fast_fix already split at is not a "second"
            // crossing (i0 < j0 by construction of find_first_intersection).
            if i == i0 && j == j0 {
                return false;
            }
            if i.abs_diff(j) <= 1 || (i == 0 && j == n_edges - 1) {
                return false;
            }
            if coords[i] == coords[j]
                && orient2d_fast(coords[i], coords[i + 1], coords[j + 1]) != 0.0
            {
                return false;
            }
            if coords[i] == coords[j + 1]
                && orient2d_fast(coords[i], coords[i + 1], coords[j]) != 0.0
            {
                return false;
            }
            if coords[i + 1] == coords[j]
                && orient2d_fast(coords[i + 1], coords[i], coords[j + 1]) != 0.0
            {
                return false;
            }
            if coords[i + 1] == coords[j + 1]
                && orient2d_fast(coords[i + 1], coords[i], coords[j]) != 0.0
            {
                return false;
            }
            match super::fix_ring::edge_intersection(coords, i, j, eps) {
                Some(pair) => {
                    out = Some(pair);
                    true
                }
                None => false,
            }
        });
        if let Some(p) = out {
            return Some(p);
        }
    }
    None
}

/// Find ANY proper edge intersection using the parallel-STR index.
/// Returns `(edge_i, edge_j, intersection_point)` with `i < j`.
///
/// Early-exit variant of `has_self_intersections` that also returns the
/// intersection geometry - used by the ATR fast-path ring splitter. O(n
/// log n) average. `find_any` semantics: any crossing is acceptable to the
/// splitter (correctness is enforced by find_second_intersection, which is
/// order-independent); the serial rstar variant scanned to the lowest-index
/// crossing, which cost ~90ms on 260k-edge giants when the first crossing
/// sat at a high edge index.
pub(crate) fn find_first_intersection(
    coords: &[Coord<f64>],
    eps: f64,
) -> Option<(usize, usize, Coord<f64>)> {
    let n_edges = coords.len().saturating_sub(1);
    let envs: Vec<[f64; 4]> = (0..n_edges).map(|i| edge_env_array(coords, i)).collect();
    let tree = StrIndex::build(&envs);

    let visit = |i: usize, out: &mut Option<(usize, usize, Coord<f64>)>| {
        tree.query(envs[i], |j| {
            let j = j as usize;
            if j <= i {
                return false;
            }
            if i.abs_diff(j) <= 1 || (i == 0 && j == n_edges - 1) {
                return false;
            }
            if coords[i] == coords[j]
                && orient2d_fast(coords[i], coords[i + 1], coords[j + 1]) != 0.0
            {
                return false;
            }
            if coords[i] == coords[j + 1]
                && orient2d_fast(coords[i], coords[i + 1], coords[j]) != 0.0
            {
                return false;
            }
            if coords[i + 1] == coords[j]
                && orient2d_fast(coords[i + 1], coords[i], coords[j + 1]) != 0.0
            {
                return false;
            }
            if coords[i + 1] == coords[j + 1]
                && orient2d_fast(coords[i + 1], coords[i], coords[j]) != 0.0
            {
                return false;
            }
            match super::fix_ring::edge_intersection(coords, i, j, eps) {
                Some(pair) => {
                    *out = Some(pair);
                    true
                }
                None => false,
            }
        });
    };

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        (0..n_edges).into_par_iter().find_map_any(|i| {
            let mut out: Option<(usize, usize, Coord<f64>)> = None;
            visit(i, &mut out);
            out
        })
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        for i in 0..n_edges {
            let mut out: Option<(usize, usize, Coord<f64>)> = None;
            visit(i, &mut out);
            if let Some(p) = out {
                return Some(p);
            }
        }
        None
    }
}

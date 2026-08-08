//! Piece construction, dedup and reconnection for the lean line noder.
//! Segments split at their node points; pieces are deduplicated (exact
//! canonical endpoint key, plus a tolerant validator-predicate sweep when
//! near-collinear noding happened) and reconnected into simple chains.

use alloc::vec;
use alloc::vec::Vec;

use geo::Coord;

use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use crate::validation::sweep::{radix_sort_keys_tls, sortable_u64};

use super::Hit;
use super::NO_FAMILY;
use super::NodeEnt;
use super::classify::classify;

impl<'a> super::LineNoder<'a> {
    pub(super) fn test_pair(&mut self, i: usize, j: usize) {
        let a1 = self.a[i];
        let a2 = self.b[i];
        let b1 = self.a[j];
        let b2 = self.b[j];
        match classify(a1, a2, b1, b2, self.eps) {
            Hit::None | Hit::Shared => {}
            Hit::Cross(pt) => {
                self.nodes.push(NodeEnt { seg: i as u32, pt });
                self.nodes.push(NodeEnt { seg: j as u32, pt });
            }
            Hit::Collinear => {
                self.near_collinear = true;
                self.nodes.push(NodeEnt {
                    seg: i as u32,
                    pt: b1,
                });
                self.nodes.push(NodeEnt {
                    seg: i as u32,
                    pt: b2,
                });
                self.nodes.push(NodeEnt {
                    seg: j as u32,
                    pt: a1,
                });
                self.nodes.push(NodeEnt {
                    seg: j as u32,
                    pt: a2,
                });
            }
            Hit::VertexOnEdge(v) => {
                if v == a1 || v == a2 {
                    self.nodes.push(NodeEnt {
                        seg: j as u32,
                        pt: v,
                    });
                } else {
                    self.nodes.push(NodeEnt {
                        seg: i as u32,
                        pt: v,
                    });
                }
            }
        }
    }

    /// Endpoint snap through the cluster canonicals: every piece endpoint
    /// that lies within eps of a cluster resolves to the same bit-exact
    /// canonical point, so sub-tolerance topology disappears instead of
    /// leaving micro-pieces the validator would flag.
    pub(super) fn snap(&self, p: Coord<f64>) -> Coord<f64> {
        self.canon_map
            .get(&(p.x.to_bits(), p.y.to_bits()))
            .copied()
            .unwrap_or(p)
    }

    // ---------------------------------------------------------------------
    // Phase 4: clustering at the validator's eps + per-segment splits
    // ---------------------------------------------------------------------

    pub(super) fn cluster_and_split(&mut self) {
        // Input points: every original vertex (anchor) plus every node
        // point. Sorted by x; union-find within eps (Chebyshev).
        let n_anchors = self.coords.len();
        let total = n_anchors + self.nodes.len();
        let mut pts: Vec<(Coord<f64>, bool, u32)> = Vec::with_capacity(total);
        for c in self.coords {
            pts.push((*c, true, 0));
        }
        for (k, e) in self.nodes.iter().enumerate() {
            pts.push((e.pt, false, (k + 1) as u32));
        }
        for (k, _) in self.nodes.iter().enumerate() {
            pts[n_anchors + k].2 = (k + 1) as u32;
        }
        pts.sort_by(|p, q| {
            p.0.x
                .partial_cmp(&q.0.x)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then(
                    p.0.y
                        .partial_cmp(&q.0.y)
                        .unwrap_or(core::cmp::Ordering::Equal),
                )
        });
        let mut parent: Vec<u32> = (0..total as u32).collect();
        fn find(parent: &mut [u32], x: u32) -> u32 {
            let mut r = x;
            while parent[r as usize] != r {
                r = parent[r as usize];
            }
            let mut c = x;
            while parent[c as usize] != r {
                let next = parent[c as usize];
                parent[c as usize] = r;
                c = next;
            }
            r
        }
        for (i, &(pt_, _, _)) in pts.iter().enumerate() {
            let (px, py) = (pt_.x, pt_.y);
            let mut j = i + 1;
            while j < total && pts[j].0.x - px <= self.eps {
                if (pts[j].0.y - py).abs() <= self.eps {
                    let ri = find(&mut parent, i as u32);
                    let rj = find(&mut parent, j as u32);
                    if ri != rj {
                        parent[ri as usize] = rj;
                    }
                }
                j += 1;
            }
        }
        // Canonical per cluster: the first point in the sort order
        // (deterministic). When a cluster contains an original vertex the
        // vertex sorts first among equal-x points and wins the canonical.
        let mut canon: Vec<Coord<f64>> = vec![
            Coord {
                x: f64::NAN,
                y: f64::NAN
            };
            total
        ];
        let mut has: Vec<bool> = vec![false; total];
        for (i, &(pt, _, _)) in pts.iter().enumerate() {
            let r = find(&mut parent, i as u32) as usize;
            if !has[r] {
                canon[r] = pt;
                has[r] = true;
            }
        }
        // Node -> sorted position: the pre-sort index n_anchors + k is
        // invalid once the sort reorders the points.
        let mut node_pos: Vec<usize> = vec![0; self.nodes.len()];
        for (i, &(_, _, tag)) in pts.iter().enumerate() {
            if tag != 0 {
                node_pos[tag as usize - 1] = i;
            }
        }
        self.splits = vec![Vec::new(); self.n];
        for (k, e) in self.nodes.iter().enumerate() {
            let idx = node_pos[k];
            let r = find(&mut parent, idx as u32) as usize;
            self.splits[e.seg as usize].push(canon[r]);
        }
        self.canon_map.clear();
        for (i, &(pt_, _, _)) in pts.iter().enumerate() {
            let r = find(&mut parent, i as u32) as usize;
            self.canon_map
                .insert((pt_.x.to_bits(), pt_.y.to_bits()), canon[r]);
        }
    }

    // ---------------------------------------------------------------------
    // Phase 5: pieces
    // ---------------------------------------------------------------------

    /// Piece construction for the pure-collinear fast path: identical to
    /// `build_pieces` but endpoints are used as-is (the 1-D split points
    /// are the other members' original vertices - already bit-exact).
    pub(super) fn build_pieces_direct(&mut self) -> Vec<(Coord<f64>, Coord<f64>, u32)> {
        let mut out: Vec<(Coord<f64>, Coord<f64>, u32)> = Vec::new();
        for i in 0..self.n {
            let a = self.a[i];
            let b = self.b[i];
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len == 0.0 {
                continue;
            }
            let mut pts = core::mem::take(&mut self.splits[i]);
            pts.sort_by(|p, q| {
                let tp = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
                let tq = ((q.x - a.x) * dx + (q.y - a.y) * dy) / len;
                tp.partial_cmp(&tq).unwrap_or(core::cmp::Ordering::Equal)
            });
            let mut prev = a;
            let mut prev_dist = 0.0f64;
            for &p in &pts {
                let d = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
                // Out-of-span guard: a node whose projection lies outside
                // the segment (adjacent-pass endpoints of a long neighbour)
                // must not produce a phantom piece.
                if d < -self.eps || d > len + self.eps {
                    continue;
                }
                if (d - prev_dist).abs() <= self.eps || (len - d).abs() <= self.eps {
                    continue;
                }
                out.push((prev, p, self.family[i]));
                prev = p;
                prev_dist = d;
            }
            if (len - prev_dist).abs() > self.eps {
                out.push((prev, b, self.family[i]));
            }
        }
        out
    }

    pub(super) fn build_pieces(&mut self) -> Vec<(Coord<f64>, Coord<f64>, u32)> {
        let mut out: Vec<(Coord<f64>, Coord<f64>, u32)> = Vec::new();
        for i in 0..self.n {
            // Piece endpoints snap through the cluster canonical map so
            // both segments of a near-coincident pair split at the same
            // bit-exact point; a segment that collapses onto its twin is
            // dropped (the twin carries the traversal).
            let a = self.snap(self.a[i]);
            let b = self.snap(self.b[i]);
            if a == b {
                continue;
            }
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len == 0.0 {
                continue;
            }
            let mut pts = core::mem::take(&mut self.splits[i]);
            pts.sort_by(|p, q| {
                let tp = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
                let tq = ((q.x - a.x) * dx + (q.y - a.y) * dy) / len;
                tp.partial_cmp(&tq).unwrap_or(core::cmp::Ordering::Equal)
            });
            let mut prev = a;
            let mut prev_dist = 0.0f64;
            for &p in &pts {
                let d = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
                // Out-of-span guard: a node whose projection lies outside
                // the segment (adjacent-pass endpoints of a long neighbour)
                // must not produce a phantom piece.
                if d < -self.eps || d > len + self.eps {
                    continue;
                }
                // Snap: drop split points within eps of the current emitted
                // point or of the end (the cluster canonical is a vertex in
                // those cases anyway - the other segments split at it).
                if (d - prev_dist).abs() <= self.eps || (len - d).abs() <= self.eps {
                    continue;
                }
                out.push((prev, p, self.family[i]));
                prev = p;
                prev_dist = d;
            }
            if (len - prev_dist).abs() > self.eps {
                out.push((prev, b, self.family[i]));
            }
        }
        out
    }

    pub(super) fn dedup_pieces(
        &self,
        pieces: Vec<(Coord<f64>, Coord<f64>, u32)>,
    ) -> Vec<(Coord<f64>, Coord<f64>, u32)> {
        let mut seen: FxHashSet<(u64, u64, u64, u64)> = FxHashSet::default();
        let mut out: Vec<(Coord<f64>, Coord<f64>, u32)> = Vec::with_capacity(pieces.len());
        for (s, e, f) in pieces {
            let sk = (s.x.to_bits(), s.y.to_bits());
            let ek = (e.x.to_bits(), e.y.to_bits());
            let key = if sk <= ek {
                (sk.0, sk.1, ek.0, ek.1)
            } else {
                (ek.0, ek.1, sk.0, sk.1)
            };
            if seen.insert(key) {
                out.push((s, e, f));
            }
        }
        out
    }

    /// Tolerant piece dedup: runs only when near-collinear noding happened.
    /// Drops a piece that the validator's own cross-component predicate
    /// flags against an earlier piece (coincident or overlapping residuals
    /// that exact-key dedup cannot see).
    pub(super) fn dedup_pieces_tolerant(&self, pieces: &mut Vec<(Coord<f64>, Coord<f64>, u32)>) {
        let m = pieces.len();
        if m < 2 {
            return;
        }
        let lo_x: Vec<f64> = pieces.iter().map(|p| p.0.x.min(p.1.x)).collect();
        let hi_x: Vec<f64> = pieces.iter().map(|p| p.0.x.max(p.1.x)).collect();
        let lo_y: Vec<f64> = pieces.iter().map(|p| p.0.y.min(p.1.y)).collect();
        let hi_y: Vec<f64> = pieces.iter().map(|p| p.0.y.max(p.1.y)).collect();
        let mut keys: Vec<u64> = lo_x.iter().map(|&x| sortable_u64(x)).collect();
        let mut order: Vec<u32> = (0..m as u32).collect();
        radix_sort_keys_tls(&mut keys, &mut order);
        let mut dead = vec![false; m];
        let mut active: Vec<u32> = Vec::new();
        for &ord in &order {
            let j = ord as usize;
            active.retain(|&p| hi_x[p as usize] + self.eps >= lo_x[j]);
            for &p in &active {
                let i = p as usize;
                if dead[i] {
                    continue;
                }
                if hi_y[i] < lo_y[j] - self.eps || lo_y[i] > hi_y[j] + self.eps {
                    continue;
                }
                let (a1, a2) = (pieces[i].0, pieces[i].1);
                let (b1, b2) = (pieces[j].0, pieces[j].1);
                if a1 == b1 || a1 == b2 || a2 == b1 || a2 == b2 {
                    continue;
                }
                if pieces[i].2 != NO_FAMILY && pieces[i].2 == pieces[j].2 {
                    continue;
                }
                if crate::validation::impls::segments_intersect_any(a1, a2, b1, b2, self.eps, false)
                {
                    dead[j] = true;
                    break;
                }
            }
            active.push(ord);
        }
        let mut k = 0;
        for i in 0..m {
            if !dead[i] {
                pieces[k] = pieces[i];
                k += 1;
            }
        }
        pieces.truncate(k);
    }

    // ---------------------------------------------------------------------
    // Phase 6: reconnection through degree-2 vertices
    // ---------------------------------------------------------------------

    pub(super) fn reconnect(
        &self,
        pieces: Vec<(Coord<f64>, Coord<f64>, u32)>,
    ) -> Vec<Vec<Coord<f64>>> {
        let m = pieces.len();
        let mut ends: FxHashMap<(u64, u64), Vec<(u32, bool)>> = FxHashMap::default();
        for (i, &(s, e, _)) in pieces.iter().enumerate() {
            ends.entry((s.x.to_bits(), s.y.to_bits()))
                .or_default()
                .push((i as u32, true));
            ends.entry((e.x.to_bits(), e.y.to_bits()))
                .or_default()
                .push((i as u32, false));
        }
        let mut used = vec![false; m];
        let mut chains: Vec<Vec<Coord<f64>>> = Vec::new();
        for start in 0..m {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut chain = vec![pieces[start].0, pieces[start].1];
            let mut visited: FxHashSet<(u64, u64)> = FxHashSet::default();
            visited.insert((pieces[start].0.x.to_bits(), pieces[start].0.y.to_bits()));
            visited.insert((pieces[start].1.x.to_bits(), pieces[start].1.y.to_bits()));
            let mut cur = start;
            // Forward extension.
            loop {
                let v = *chain.last().expect("non-empty chain");
                let k = (v.x.to_bits(), v.y.to_bits());
                let Some(cands) = ends.get(&k) else { break };
                if cands.len() != 2 {
                    break;
                }
                let other = cands
                    .iter()
                    .find(|(q, _)| *q as usize != cur && !used[*q as usize]);
                let Some(&(q, is_start)) = other else { break };
                let fv = if is_start {
                    pieces[q as usize].1
                } else {
                    pieces[q as usize].0
                };
                // Loop guard: a closed chain (the far vertex was already
                // visited) must not extend through itself.
                if !visited.insert((fv.x.to_bits(), fv.y.to_bits())) {
                    break;
                }
                used[q as usize] = true;
                chain.push(fv);
                cur = q as usize;
            }
            // Backward extension (prepend, collected reversed).
            let mut pre: Vec<Coord<f64>> = Vec::new();
            let mut cur_b = start;
            loop {
                let v = pieces[cur_b].0;
                let k = (v.x.to_bits(), v.y.to_bits());
                let Some(cands) = ends.get(&k) else { break };
                if cands.len() != 2 {
                    break;
                }
                let other = cands
                    .iter()
                    .find(|(q, _)| *q as usize != cur_b && !used[*q as usize]);
                let Some(&(q, is_start)) = other else { break };
                let fv = if is_start {
                    pieces[q as usize].1
                } else {
                    pieces[q as usize].0
                };
                if !visited.insert((fv.x.to_bits(), fv.y.to_bits())) {
                    break;
                }
                used[q as usize] = true;
                pre.push(fv);
                cur_b = q as usize;
            }
            pre.reverse();
            pre.extend(chain);
            if pre.len() >= 2 {
                chains.push(pre);
            }
        }
        chains
    }
}

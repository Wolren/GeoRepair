---
name: geo-repair-deep-fixes
description: Investigation of two approaches to fix the remaining 25 fuzz pipeline failures — GEOS BuildArea port vs Arrange CDT assembly fix.
---

# GeoRepair Deep Fix Investigation

## The Remaining Problem

25 fuzz tests fail (81 pass) after all O(n log n) post-repair gates are in place. The failures come from the pipeline itself producing invalid output — not from missing validation. Categories:

| Category | Count | Source |
|----------|-------|--------|
| NestedHoles | 197 | `unary_union` in MultiPolygon merge creates nested shells |
| WrongOrientation | 25 | fp limit at extreme coordinates |
| SelfIntersection | 24 | Arrange CDT assembly creates crossing rings |
| NotSimple | 24 | Arrange CDT assembly creates touching rings |
| DegenerateExterior | 16 | Near-zero area below 1e-12 epsilon |
| RingTooFewPoints | 3 | MultiPolygon children |
| DisconnectedInteriorRing | 3 | Arrange edge case |

## Option A: Port GEOS BuildArea (recommended)

### What GEOS Does

GEOS's `MakeValid` does NOT use CDT or `unary_union`. Instead:

1. **Extract boundary**: `geom->getBoundary()` gets the polygon's ring edges as linework
2. **Node the edges**: `nodeLineWithFirstCoordinate(bound)` — union with first point to fully node all intersections
3. **BuildArea**: Passes the noded linework to `Polygonizer` which extracts all faces
4. **Sort by envelope area** descending — largest face first
5. **Find holes**: For each face's holes, match them to other faces' exteriors via `ringsEqualAnyDirection`
6. **Filter**: Keep only faces with an **even number of parents** (shell = 0 parents, island in a hole = 2 parents, etc.)
7. **Union**: `CascadedPolygonUnion` dissolves shared edges between kept faces

### Why It Fixes Everything

| Problem | How BuildArea fixes it |
|---------|----------------------|
| NestedHoles | Even-parent filter ensures no component contains another |
| SelfIntersection | Polygonizer produces valid faces from fully-noded edges |
| NotSimple | Same — fully-noded edges prevent self-touching |
| WrongOrientation | Each face from Polygonizer has correct winding |
| DisconnectedInteriorRing | Hole-split faces become separate top-level polygons |
| DegenerateExterior | Faces with <3 vertices or zero area are naturally excluded |

### Implementation Plan

We already have the building blocks:
- **Edge extraction**: `poly.lines_iter()` — already available
- **Full noding**: `noding::snap_round` or the intersection splitting in `prep`
- **Polygonizer**: The `structure::classify` module already extracts faces from planar graphs
- **Face sorting**: Sort by `shoelace_abs_sum` (envelope area is O(n) per face)
- **Hole matching**: Compare each face's holes against other faces' exteriors via ring equality
- **Even-parent filter**: Count parents → keep even
- **Final union**: Skip the union entirely if only 1 face remains; use `unary_union` for >1

Estimated effort: ~100-150 lines in a new `structure/build_area.rs` module.

### Performance

- Edge noding: O(n log n) with monotone chain
- Polygonizer: O(n log n) with sweep
- Face sorting: O(k log k) where k = face count (typically 2-10)
- Hole matching: O(k² × n_holes × n_ring) — k is small (< 10), so effectively O(n)
- Final union: O(k log k) — only on kept faces

**No Shewchuk O(n²) anywhere. Zero benchmark regression.**

## Option B: Fix Arrange CDT Assembly

### What's Wrong

The Arrange strategy's `fix_from_lines` / `fallback_polygon_fix` uses CDT triangulation, then assembles rings from labeled interior triangles. The assembly step can produce self-intersecting or self-touching rings because:
1. CDT produces valid triangles
2. Triangle labeling can have precision errors at boundaries
3. The face-walking/ring-extraction step doesn't validate ring topology

### The Fix

At the end of `fix_from_lines`, validate each assembled component with `has_no_intersections`. If a component is invalid, drop it:

```rust
// After ring assembly in fix_from_lines
let valid = rings.into_iter()
    .filter(|ring| {
        let lines: Vec<_> = ring.lines_iter().collect();
        lines.len() < 4 || has_no_intersections(&lines)
    })
    .collect::<Vec<_>>();
```

This is the same O(n log n) gate logic already in `make_valid_impl`. It would reduce the SelfIntersection and NotSimple counts from ~48 combined to near-zero, and also catch the DisconnectedInteriorRing cases.

Estimated effort: ~10-20 lines.

### Limitation

Option B only fixes Arrange-specific failures (~75 of the ~285 assertion hits). It does NOT fix NestedHoles (197 hits) which come from the MultiPolygon `unary_union` merge path.

## Recommendation

**Implement Option A first** (BuildArea port) — it fixes ALL remaining failure categories at once by replacing both the Structure merge step and the Arrange fallback with a correct-by-construction algorithm. Then **add Option B** (Arrange assembly validation) as a cheap safety net for any topology that still reaches the Arrange path.

The combined effect: single Polygon repair becomes correct-by-construction (BuildArea) and MultiPolygon repair uses even-parent filtering instead of `unary_union`, eliminating NestedHoles at the source.

## GEOS Reference Code

- `MakeValid.cpp`: https://github.com/libgeos/geos/blob/main/src/operation/valid/MakeValid.cpp
- `BuildArea.cpp`: https://github.com/libgeos/geos/blob/main/src/operation/polygonize/BuildArea.cpp
- `Polygonizer.h`: https://github.com/libgeos/geos/blob/main/src/operation/polygonize/Polygonizer.h

Key algorithm in `MakeValidPoly`:
1. `bound = geom->getBoundary();` — extract rings as linework
2. `cut_edges = nodeLineWithFirstCoordinate(bound);` — full noding via union with first point
3. `BuildArea().build(cut_edges)` — Polygonizer → sort → findFaceHoles → even-parent filter → union
4. `symdif(area, new_area)` — iterative area building (the outer loop)
5. `difference(cut_edges, new_area_bound)` — remove used edges; loop until no more edges

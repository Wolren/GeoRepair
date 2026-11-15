# Repair core specification: even-odd area and the MakeValid loop

Functional specification for the two building blocks behind polygon repair:
`build_area` and the MakeValid loop. It describes required behaviour, not an
implementation. A clean-room implementation can work from this file alone.

## 1. Terms

- **Edge set**: a list of straight 2D segments.
- **Noded**: every pair of segments meets only at shared endpoints. No segment
  crosses another in its interior.
- **Face**: a connected region of the plane bounded by a cycle of edges, with no
  edge inside it.
- **Depth of a face**: the number of faces that contain it, counted outwards.
- **Even-odd interior**: the union of faces whose depth is even, equivalently the
  set of points crossed an odd number of times by a ray from the boundary. A
  segment walked an even number of times bounds nothing.
- **Boundary of an area**: the segments that separate the area from its complement,
  each listed once.

## 2. `build_area(edges)`

### Input contract

- Segments are finite 2D segments. The caller removes NaN, infinite and
  zero-length segments.
- The edge set is fully noded. If it is not, the implementation may node it first.
- Endpoints are exact `f64` values. Intersection points must already exist as
  vertices when the input is noded; the implementation may add a vertex at an
  intersection it finds.

### Output contract

- The result is the even-odd interior of the edge set, as a `MultiPolygon`.
- Shells are counter-clockwise, holes clockwise, every ring closed.
- Parts do not overlap and do not share an edge.
- A face whose depth is even is included; a face of odd depth is excluded.
- A run of collinear boundary segments may be merged into one segment.
- Coordinates come from the input. The implementation must not invent a
  coordinate that is not an input vertex or an intersection of input segments.
- Empty interior means an empty `MultiPolygon`, never `None` and never a panic.
- The same input gives the same set of parts and the same ring order.

### Degenerate cases

- Empty input: empty result.
- No closed cycle: empty result.
- Dangling chains: ignored.
- Zero-area faces: dropped.
- A segment that appears an even number of times, in any direction, cancels. An
  odd count leaves one copy.
- A vertex where two rings touch is a pinch point: split the face there, so the
  output rings stay simple.

### Boundary report

The implementation also returns the segments on the boundary of the built area,
each once, in the order the rings were built. The MakeValid entry points use this
list; it is not part of the public API.

### Cost

Expected `O((n + k) log n)` time and `O(n + k)` memory, where `n` is the edge
count and `k` the number of intersections. No unbounded loop, no panic.

## 3. MakeValid entry points

### Input

A polygon boundary: the shell and hole rings of one polygon, as segments.

### Steps

The XOR loop below is one valid method. A direct parity calculation over the
planar faces gives the same result. The output contract is what matters.

1. Node the boundary. Split segments at every crossing and drop zero-length
   segments. Duplicate segments keep their multiplicity.
2. Set the accumulated area to empty and the remaining edge set to the noded
   boundary.
3. While the remaining edge set is not empty:
   - Build the area of the remaining edges. If it is empty, stop.
   - XOR it into the accumulated area: points covered an odd number of times stay.
   - Subtract the built area's boundary from the remaining edges, one copy per
     boundary segment.
   - Count iterations. After 64, log a warning and stop.
4. Return the accumulated area.

### Output contract

- The result is the even-odd interior of the input boundary.
- Every output is valid or empty. No panic on any finite input.
- The result is idempotent: repairing the output again returns it unchanged.
- Area matches the even-odd interior within `1e-6` relative, which covers the
  snap grid used by the implementation.
- Linework that bounds no area is dropped. The caller delivers polygons.
- A self-crossing ring whose interior is empty returns empty.

### Cost and termination

Expected cost is `O((n + k) log n)` for the noded boundary. The XOR method adds
a pass per level, and the guard bounds the worst case. The direct parity method
uses one pass.

## 4. Contract tests

| Property | Test |
|---|---|
| Even-odd area of two crossing walks | `tests/repair_area_oracle.rs` |
| Pentagram centre empty (3.333) | `src/structure/symdiff.rs` |
| Two overlapping 2x2 squares give 6, not 7 | `src/structure/symdiff.rs` |
| Structure and Arrange agree on random walks | `tests/repair_area_oracle.rs` |
| Corpus repair matches the even-odd area | `conformance/run.py`, check O |
| Validity, idempotence, no panic | `tests/fuzz.rs`, `tests/host_adapter.rs` |
| Square with hole and island | `tests/repair_core_spec.rs` |
| Duplicate edges cancel | `tests/repair_core_spec.rs` |

## 5. Provenance

The current implementation was written from this specification. It computes the
even-odd interior with one dual walk over the planar faces, so it carries no
GEOS or rttopo lineage. This specification describes behaviour only. A future
implementation written from it, by a person who has not read the GEOS or rttopo
source, is not a derivative of that source and can be licensed under
Apache-2.0. Keep the tests in section 4 as the acceptance gate.

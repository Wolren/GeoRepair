# Roadmap

GeoRepair repairs invalid OGC geometries and answers whether a geometry is valid. This file lists
the known gaps, each with the measurement that found it and the test that will close it.

## How the gaps were found

A differential harness ran Repair Geometry (RustGIS `046bd3c2`) and GeoRepair over 1,282 cases:
the 120-case OGC conformance corpus, the GEOS and JTS XML suites, 6 random crossing walks, and
200 fuzz seeds and variants. Both engines were scored against an independent oracle: even-odd area
from polygonize plus crossing parity, with GEOS `make_valid` as a second opinion. GeoRepair missed
the oracle on 45 cases before `a0848de` and on 6 after it.

## Done

- `a0848de` - MultiLineString components are noded at their contacts instead of dropped. A closed
  component that touches another is cut into two open paths, so coverage stays complete. 33 cases.
- `a0848de` - A ring that retraces its previous edge is rejected, and the fast path declines it.
  Six cases.
- `a0848de` - `tests/corpus_replay.rs` decodes each corpus the way its fuzz target does. 79 seeds
  run, 0 are skipped. Before, all 41 raw f64 seeds were read as WKB and dropped silently.

## Now

### 1. Collinearity tolerance in the component rule

`segments_collinear_overlap` compares an orientation value (twice an area) with `eps` (a length).
A pair of near-parallel segments then reads as overlapping at any gap. The `geos-392` geometry
(123 parts) touches only at endpoints, but the rule reports an overlap, and the repair drops 29
parts, 30% of the length.

Work: compare `|f| / len` with `eps`. Run the XML suites and re-baseline the masked divergence
count if it moves.

Done when: the three `geos-392` cases keep all 123 parts, and no suite regresses.

### 2. Policy below the snap grid

A polygon with coordinates at 1e208 and a sliver 3.7e4 wide is thinner than one grid step at that
extent (`SNAP_SCALE = 1e8`). The repair returns an empty GeometryCollection. The even-odd area of
that input is not representable in f64 either, so the loss cannot be measured.

Work: choose one policy and write it in `Limitations`: return empty with a warning, or return the
input unchanged when the area is below the grid.

Done when: the policy is implemented, tested on the three corpus mutants, and documented.

## Next

### 3. Noding cost on large MultiLineStrings

`node_components` checks every pair of pieces. 123 parts cost about 0.3 ms. The cost is quadratic
and unbounded.

Work: benchmark 1k and 10k parts, then gate the pairwise loop behind the R-tree if it dominates.

Done when: a bench row exists and the 1k-part case stays under one millisecond.

### 4. Report deduplicated line pieces

Identical or overlapping components are deduplicated silently. `OutputReport` counts polygon parts
and drops; line components have no counter.

Work: add a line-component counter and a test that the report matches the output.

Done when: a caller can tell that a repair dropped or merged a line component.

## Later

### 5. Exact replay assert

`tests/corpus_replay.rs` allows up to 8 skipped seeds. The count is 0 since the format fix.

Work: assert `skipped == 0`.

### 6. Line validity: component rule or GEOS parity

The XML suite decides line validity by GEOS parity, but the repair enforces the OGC component
rule. The two disagree on linework that GEOS calls valid and whose parts meet. Decide which one
the public contract follows, and make the suite and the repair agree. Item 1 removes the false
positives; this item removes the ambiguity.

## Not planned

- Polygon repair semantics. GeoRepair matches the even-odd oracle on the measured polygon cases
  outside the two items above, so there is no work item here.

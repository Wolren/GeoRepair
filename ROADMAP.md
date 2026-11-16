# Roadmap

GeoRepair repairs invalid OGC geometries and answers whether a geometry is valid. This file lists
the known gaps, each with the measurement that found it and the test that will close it.

## How the gaps were found

A differential harness ran Repair Geometry (RustGIS `046bd3c2`) and GeoRepair over 1,282 cases:
the 120-case OGC conformance corpus, the GEOS and JTS XML suites, 6 random crossing walks, and
200 fuzz seeds and variants. Both engines were scored against an independent oracle: even-odd area
from polygonize plus crossing parity, with GEOS `make_valid` as a second opinion. GeoRepair missed
the oracle on 45 cases before `a0848de`, on 6 after it, and on 0 after the two fixes below.

## Done

- `a0848de` - MultiLineString components are noded at their contacts instead of dropped. A closed
  component that touches another is cut into two open paths, so coverage stays complete. 33 cases.
- `a0848de` - A ring that retraces its previous edge is rejected, and the fast path declines it.
  Six cases.
- `a0848de` - `tests/corpus_replay.rs` decodes each corpus the way its fuzz target does. 79 seeds
  run, 0 are skipped. Before, all 41 raw f64 seeds were read as WKB and dropped silently.
- `57de5dd` - Item 1: collinearity is a distance, not an area. `segments_collinear_overlap` and
  `segments_intersect_any` compare `|f| / len` with `eps`, the longer segment supplies the reference
  line, and the degenerate guard is `len2 <= eps * eps` (it was `len2 <= eps`, which exempted every
  segment shorter than 1e-6, so the verdict depended on part order). `node_components` cuts both
  parts at each end of an overlap, and `on_segment` uses the same convention. `geos-392`: 123 parts
  in, 125 pieces out, one 1.2e-10 duplicate deduplicated, 1.2e-10 of length lost.
- `abff4b9` - Item 2: below the snap grid the repair keeps the input. When `|coord| * 1e8` overflows
  the 2^53 key space and the repair would return nothing, `preserve_valid_input` returns the input
  with consecutive duplicate vertices collapsed, and only when that result validates. `mut26`,
  `mut34` and `mut99` keep their polygon.

## Now

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
the public contract follows, and make the suite and the repair agree. The distance-based rule in
`57de5dd` removed the false positives; this item removes the ambiguity.

## Not planned

- Polygon repair semantics. GeoRepair matches the even-odd oracle on every measured polygon case,
  so there is no work item here.

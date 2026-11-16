# Roadmap

GeoRepair repairs invalid OGC geometries and answers whether a geometry is valid. This file lists
the known gaps, each with the measurement that found it and the test that will close it.

## How the gaps were found

A differential harness ran Repair Geometry (RustGIS `046bd3c2`) and GeoRepair over 1,282 cases:
the 120-case OGC conformance corpus, the GEOS and JTS XML suites, 6 random crossing walks, and
200 fuzz seeds and variants. Both engines were scored against an independent oracle: even-odd area
from polygonize plus crossing parity, with GEOS `make_valid` as a second opinion. GeoRepair missed
the oracle on 45 cases before `a0848de`, on 6 after it, and on 0 after the component-rule and
snap-grid fixes (`57de5dd`, `abff4b9`).

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
- `b28c6db` - Item 3: `node_components` scans only nearby pieces. A sweep over the piece boxes,
  sorted by min x and stopping past `max_x + eps`, replaces the pairwise loops in `connect`,
  `split_closed_parts`, `dedupe_covered` and `any_conflict`: a pair further apart than `eps` cannot
  touch. 1,000 parts went from 137 ms to 0.66 ms serial and 0.11 ms parallel; 10,000 parts from
  11.8 s to 6.2 ms. Bench rows `mls 1000x3v` and `mls 10000x3v`. A set that spreads in y still pays
  only the four-compare box test per candidate.
- `5443b37` - Item 4: `OutputReport` now reports line components. `line_parts_in` and
  `line_parts_out` count the caller's input and the emitted geometry, and `is_lossless()` is false
  when a deduplicated component disappears: two identical parts report 2 in, 1 out.
- `476e9b2` - Item 5: `tests/corpus_replay.rs` requires `skipped == 0`. All 79 seeds run.

## Now

### 6. Line validity: component rule or GEOS parity

The XML suite decides line validity by GEOS parity, but the repair enforces the OGC component
rule. The two disagree on linework that GEOS calls valid and whose parts meet. Decide which one
the public contract follows, and make the suite and the repair agree. The distance-based rule in
`57de5dd` removed the false positives; this item removes the ambiguity.

## Not planned

- Polygon repair semantics. GeoRepair matches the even-odd oracle on every measured polygon case,
  so there is no work item here.

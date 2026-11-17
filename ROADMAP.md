# Roadmap

GeoRepair repairs invalid OGC geometries and answers whether a geometry is valid. This file lists
the known gaps, each with the measurement that found it and the test that will close it.

## How the gaps were found

A differential harness ran Repair Geometry (RustGIS `046bd3c2`) and GeoRepair over 1,301 cases:
the 120-case OGC conformance corpus, the GEOS and JTS XML suites, 6 random crossing walks, 19
synthetic bench-shaped families, and 200 fuzz seeds and variants. Both engines were scored against
an independent oracle: the input's even-odd interior as a geometry (symmetric difference, not only
area), GEOS validity, input boundary/part coverage, and the distinct linework length for lines.
GeoRepair missed the oracle on 45 cases before `a0848de`, on 6 after it, and on 0 after the
component-rule, snap-grid, line-coverage and area-gate fixes (`57de5dd`, `abff4b9`, items 7-14).

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
- Item 7: line repair keeps every spike. `simple_subline` treated a shared endpoint as a conflict
  and dropped every later spike, and `filter_demoted_lines` dropped a whole hairpin instead of
  splitting it. A star-burst now keeps one component per spike: 100 spikes 2 parts / 2,000 units
  -> 100 parts / 100,000; 1,000 spikes 2 / 2,000 -> 1,000 / 1,000,000. The extreme-span seed
  `regression_line_not_simple_1e83.bin` keeps 100% of the distinct linework; it lost 5.8e81 of
  midpoint coverage before. `tests/line_components.rs` pins both. Repeated-vertex splitting runs
  before the greedy walk, so 1,000 spikes cost 83 us on CI (the old lossy output cost 48 us); the
  gate baseline records the correct output.
- Item 8: nested holes are detected in either input order. `check_holes_valid` tested only the
  later ring for nesting, so JTS "Holes - Nested" (the inner hole listed first) was called valid
  and shipped a nested-holes polygon. The pair check now tests both directions:
  `test_nested_holes_reversed_order`, and the repaired area is 4,400 (was 1,200 in point set,
  +16.72% wrong in the report).
- Item 9: a MultiPolygon component nested inside another is detected when every vertex lies on the
  parent boundary. The validator probed the first vertex only; it now uses an interior probe, so
  JTS "mA - shell inside shell, all vertices touch" is rejected, and the "shell inside hole" island
  (GEOS-valid) is accepted instead of losing 18.75% of its area.
- Item 10: a valid input comes back unchanged. `preserve_valid_input` accepted the snapped repair
  result before checking the input; at 1e15 the 1e8 snap grid moves a coordinate by 0.1 and changed
  the area of `mut8`, `mut26`, `mut34`, `mut99` by 12.5% to 25%. A valid ring now returns bit-exact;
  an invalid sub-grid ring returns its deduplicated input. `sub_grid_mutants_keep_the_input`.
- Item 11: a valid output with the wrong interior is caught and repaired. Auto compares the repaired
  area with the even-odd boundary area (`build_area`), then tries the arrangement path and the
  even-odd faces. `mut13` -99.44% -> 0, `geos-590` +16.89% -> 0, AX_Buffer-1 case 25 -92.51% -> 0,
  and the two overlapping bowtie holes +16.72% -> 0. `tests/repair_area_oracle.rs` names each.
- Item 12: a MultiPolygon whose only defect is ring winding is re-wound whole, islands included.
  "shell inside hole, all vertices touch" returned 125,600 units (-1.875%) before; it now returns
  128,000.
- Item 13: a CW thin shell of 45,799 units that returned empty is re-wound, and a vertex within eps
  of a non-adjacent edge is nudged 3 eps away from it. AX_Buffer-1 part 2: empty -> 45,799.48,
  relative area error 4e-9. `clockwise_thin_shell_keeps_its_area`.
- Item 14: `MULTIPOINT (EMPTY, (1 2))` writes `MULTIPOINT (EMPTY, (1.0 2.0))`. The old writer emitted
  `MULTIPOINT (NaN NaN, 1.0 2.0)`, which GEOS rejects. `multipoint_with_an_empty_member_writes_geos_valid_wkt`.
- Item 17: the full 813-feature GeoPackage run finished: 1,579,028 polygon parts, every input
  GEOS-valid. GeoRepair writes 0 invalid outputs and 4 area misses; the misses are four copies of one
  1,451-part feature, +0.0122% each (0.0033 area units on 27.1208). RustGIS writes 61 invalid
  outputs and 0 area misses. The four misses sit above the 1e-6 harness tolerance and are the
  tolerant near-touch class (item 18), not a lost part.
- Item 19: a valid MultiPolygon returns unchanged, minus empty components. The old path repaired
  every part and re-noded all components even when the feature was valid. An 80,000-part valid
  checkerboard went from 298 ms to 119 ms. `valid_multipolygon_comes_back_byte_for_byte`.

## Now

### 6. Line validity: component rule or GEOS parity

The XML suite decides line validity by GEOS parity, but the repair enforces the OGC component
rule. The two disagree on linework that GEOS calls valid and whose parts meet. Decide which one
the public contract follows, and make the suite and the repair agree. The distance-based rule in
`57de5dd` removed the false positives; this item removes the ambiguity.

### 15. Explicit Structure mode can still return the wrong area

Auto is correct on `mut13` and `geos-590` after the area gate, but the explicitly selected
Structure method still returns 7.0 instead of 1,269.28 and 24,923.70 instead of 21,322.02. The gate
is applied to `FixOutcome::Repaired`; these results come through a branch it does not see. Decide
whether Structure mode should carry the same even-odd area contract as Auto.

### 16. The one remaining both_diverge case

`geos_xml/general_TestValid.xml/mA - multipolygon with component with too few points#50`: both
engines drop a degenerate component and keep the even-odd area (delta 0). The harness flags the
lost linework as a divergence, not a wrong area. This is a documented divergence; no repair bug is
claimed.

### 18. Four real-world features move by 0.0122%

The 813-feature run has four copies of a 1,451-part valid MultiPolygon whose area moves from
27.1208 to 27.1241 (+0.0033 units, +0.0122%). GEOS calls the input valid; the tolerant validator
sees a near-touch, and the repair changes the sliver. This is above the harness 1e-6 tolerance.
Decide whether the tolerant near-touch rule should return the input unchanged when the exact
predicates find no crossing.

## Not planned

- Polygon repair semantics on the measured corpus. The differential harness now has 0 `our_bug`
  cases over 1,301 cases: 895 match, 86 their_bug, 179 different decomposition, 137 unsupported,
  3 unmeasured, 1 both_diverge (item 16).

# Benchmarks

Full benchmark tables, methodology, and diagnosis notes for geo-repair.
The README carries the summary; this file is the detail.

## Methodology

- Machine: i5-12400F (6C/12T), Windows 10, release profile (LTO, mimalloc).
- GEOS: conda-forge `libgeos` 3.14.1 (MSVC, serial per-call, no LTO),
  driven concurrently via Rayon, geometries built via CoordSeq direct
  construction (no WKT round-trip). Both columns measured in the same
  process; the GeoRepair column is the Rayon parallel batch.
- GEOS reference per case: `makeValid` for valid inputs, `UnaryUnion` for
  invalid line inputs (GEOS `makeValid` on lines is a repeated-point
  strip + clone - a passthrough, not noding; verified against
  `GeometryFixer.cpp`). Polygon invalid cases run GEOS `makeValid`.
- Ratio convention everywhere: **GEOS / GeoRepair** - `>1` means
  geo-repair is faster, `<1` means GEOS is faster.
- Measurement rules: always take the settled second run (first-run-after-
  build is inflated ~18% by Windows Defender + cold LTO code); never trust
  a bench binary you cannot trace to a source file (stale
  `target/release/examples/*.exe` have produced phantom numbers before).
- Sub-µs rows are Rayon dispatch noise; read the larger rows and the
  real-world batch numbers.
- Historical note: pre-2026-08-07 GEOS columns went through a WKT
  round-trip per geometry, which inflated the GEOS side of large
  MultiPolygon cases 30-500x (e.g. "dense grid ~200x", "overlap ~175x" in
  earlier READMEs). The current harness constructs CoordSeq directly; the
  wins on those rows are ~4-14x, still wins, but the old magnitudes were
  an artifact.

## Real-world dataset (1,579,030 polygons)

813 features from a production GeoPackage (geometry blobs are WKB),
flattened to 1,579,030 polygon parts. Measured 2026-08-07.

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | 2.47 s (1.57 µs/poly) | 3.46 s (2.19 µs/poly) | 1.40x |
| Full pass (1.58M) | 4.05 s (2.56 µs/poly) | 3.49 s (2.21 µs/poly) | 0.86x |

Validity agreement with GEOS: 100% (0/0 disagreements). GEOS setup cost
(one-time pre-build of 1.58M geometries, 1.35 s) excluded.

The 2,298 "invalid" figure that appeared in earlier notes and READMEs is
a bug-inflated historical artifact, not current behavior: the
pre-2026-08-03 product-form proper-crossing test (`o1*o2 < 0`) treated a
-0.0 orient (an exact collinear touch, common on snapped vertices) as a
crossing. The zero-safe strict opposite-sign fix dropped the
winding-agnostic count to 1, and the 2026-08-06 validator-gap fixes
(ring-touch graph cycles, incident-segment hole nesting, cross-component
hole probes) closed the last real-world case. Measured 2026-08-07
evening: **0 invalid both sides**. The winding-sensitive validator still
flags the ~1.58M clockwise rings as orientation violations - those are
GEOS-valid; that is the OGC winding contract (see the tolerance section),
not a validity gap.

## Synthetic benchmarks (2026-08-07, lean-predicate + gate-fusion pass)

### Polygons

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| valid polygon 4v | 0.088 | 0.244 | 2.78x |
| valid polygon 10v | 0.193 | 0.358 | 1.85x |
| valid polygon 50v | 0.463 | 0.418 | 0.90x |
| valid polygon 100v | 0.590 | 0.483 | 0.82x |
| valid polygon 500v | 2.177 | 1.379 | 0.63x |
| valid polygon 1000v | 3.916 | 2.725 | 0.70x |
| valid polygon 5000v | 25.67 | 9.267 | 0.36x |
| valid polygon 10000v | 55.56 | 17.46 | 0.31x |
| invalid bowtie 4v | 0.867 | 18.88 | 21.78x |
| invalid bowtie 50v | 8.022 | 68.95 | 8.59x |
| invalid bowtie 100v | 29.39 | 135.3 | 4.60x |
| invalid bowtie 500v | 62.07 | 506.9 | 8.17x |
| invalid star 100v | 8.654 | 7.674 | 0.89x |
| spaghetti 500v | 4631.1 | 3001.1 | 0.65x |
| spaghetti 2000v | 34068.8 | 21303.9 | 0.63x |
| self-touch poly | 0.995 | 17.03 | 17.12x |
| collapsed poly | 0.609 | 10.46 | 17.16x |
| near-collinear | 1.066 | 15.80 | 14.82x |
| large coord 1e12 | 0.111 | 0.265 | 2.38x |

Bowtie rows are subdivided-edge bowties (same crossing, n/4 collinear
segments per edge) - the genuine invalid-at-scale coverage. The 500v row
was 474 us before the 2026-08-07 edge-split dispatch fix (below): the old
2000-edge brute-force gate made the noding O(n^2) with a ~6.6 us/edge
constant; with the 128-edge gate the same row is 62 us.

The "invalid star" rows are NOT invalid: geosop isValid returns true for
the star generator at every size (star99/500/1000 verified 2026-08-07).
The row measures a valid spiky polygon - keep it for the valid-spiky
constant gap, but it is not an invalid-repair benchmark.

The spaghetti rows (torus-wrapped random walks, dozens of proper
crossings) are the real dense-invalid class and remain the open repair
gap: single_pass noding splits cleanly (500 edges -> 925 noded in ~560 us)
but the NodingValidator reports ~2232 violations on the result, forcing
snap-round + a boolean/arrange fallback chain (~30 ms serial). The
violations are predominantly collinear overlaps along the walk's
revisited grid lines - the parametric splitter handles proper crossings
but not the overlap class. GEOS: 2.5 ms serial on the same input.

### LineStrings & MultiLineStrings

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| valid line | 0.007 | 0.040 | 5.49x |
| zero-length line | 0.007 | 0.259 | 38.82x |
| valid ls 4v | 0.026 | 0.039 | 1.47x |
| valid ls 10v | 0.068 | 0.040 | 0.58x |
| valid ls 50v | 0.163 | 0.082 | 0.50x |
| valid ls 100v | 0.316 | 0.153 | 0.48x |
| valid ls 500v | 1.326 | 0.562 | 0.42x |
| collinear ls 4v | 0.030 | 0.574 | 19.00x |
| collinear ls 10v | 0.078 | 0.595 | 7.61x |
| collinear ls 50v | 0.228 | 0.681 | 2.99x |
| collinear ls 100v | 0.485 | 0.710 | 1.46x |
| collinear ls 500v | 1.957 | 1.653 | 0.84x |
| zigzag ls 10v | 0.066 | 1.286 | 19.49x |
| zigzag ls 50v | 0.172 | 4.652 | 27.03x |
| zigzag ls 100v | 0.357 | 11.11 | 31.10x |
| zigzag ls 500v | 1.569 | 60.81 | 38.74x |
| spiral ls 10v | 0.078 | 0.899 | 11.51x |
| spiral ls 50v | 0.568 | 4.520 | 7.95x |
| spiral ls 100v | 1.404 | 15.20 | 10.83x |
| self-int ls 5v | 0.511 | 1.722 | 3.37x |
| dense self ls 10v | 0.089 | 1.179 | 13.19x |
| dense self ls 50v | 0.234 | 4.864 | 20.77x |
| dense self ls 100v | 0.356 | 12.22 | 34.31x |
| dense self ls 500v | 1.459 | 119.9 | 82.16x |
| duped ls 100v | 0.371 | 0.698 | 1.88x |
| mls 50x3v | 2.990 | 31.32 | 10.48x |
| self-int mls 50x4v | 30.17 | 76.31 | 2.53x |

The GEOS reference for the invalid-line rows is `UnaryUnion` (the
operation GEOS users actually call to fix linework). GEOS `makeValid` on
lines is a passthrough: `fixLineStringElement` strips repeated points and
clones, with no noding branch anywhere in `GeometryFixer.cpp` - verified
three ways (geosop oracle, shapely C-API, source). The reference matters:
against the passthrough the line rows would read 5-5,000x "slower" than
real GEOS noding; against `UnaryUnion` they are honest wins. Do not
revert these rows to `makeValid`.

### Special shapes

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| star-burst 10sp | 2.510 | 13.58 | 5.41x |
| star-burst 50sp | 0.760 | 252.1 | 331.82x |
| star-burst 100sp | 1.546 | 1094.3 | 707.69x |
| star-burst 500sp | 11.06 | 36413.1 | 3292.92x |
| collinear ov 10seg | 1.012 | 12.00 | 11.86x |
| collinear ov 50seg | 7.102 | 61.44 | 8.65x |
| collinear ov 100seg | 12.53 | 114.0 | 9.09x |
| collinear ov 500seg | 60.06 | 535.4 | 8.91x |
| x-scale 10v | 2.014 | 19.82 | 9.84x |
| x-scale 50v | 51.51 | 378.6 | 7.35x |
| x-scale 100v | 228.8 | 1528.8 | 6.68x |
| ringing 100v | 10.63 | 61.03 | 5.74x |
| ringing 500v | 48.58 | 312.3 | 6.43x |
| hilbert 256v | 32.90 | 159.5 | 4.85x |
| hilbert 1024v | 297.9 | 1161.7 | 3.90x |
| lissajous 200v | 19.05 | 140.0 | 7.35x |
| lissajous 500v | 49.55 | 452.0 | 9.12x |
| lissajous 1000v | 141.3 | 782.6 | 5.54x |
| lissajous 2000v | 191.5 | 1590.0 | 8.30x |
| lissajous 5000v | 596.8 | 5391.6 | 9.03x |
| lissajous 7:4 500v | 33.83 | 50.04 | 1.48x |
| spoke 10sp | 2.259 | 14.28 | 6.32x |
| spoke 50sp | 0.815 | 250.9 | 307.90x |
| spoke 100sp | 1.618 | 1053.1 | 650.97x |
| spoke 500sp | 10.16 | 38840.6 | 3821.39x |
| star-comb 20sp | 0.273 | 2.662 | 9.76x |
| star-comb 100sp | 3.060 | 32.48 | 10.61x |
| star-comb 500sp | 75.56 | 687.9 | 9.10x |

### Holes, overlaps & grids

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| hole hier 5h | 1.811 | 2.128 | 1.17x |
| hole hier 20h | 7.744 | 9.939 | 1.28x |
| hole hier 50h | 22.23 | 27.72 | 1.25x |
| overlap mp 5sh | 2.080 | 14.24 | 6.85x |
| overlap mp 20sh | 8.220 | 78.48 | 9.55x |
| overlap mp 50sh | 22.26 | 230.2 | 10.34x |
| dense grid 5x5=25 | 9.022 | 122.9 | 13.63x |
| dense grid 10x10=100 | 46.03 | 485.3 | 10.54x |
| dense grid 20x20=400 | 500.1 | 2114.2 | 4.23x |
| sliver 100v | 1.748 | 2.118 | 1.21x |
| sliver 500v | 9.201 | 11.80 | 1.28x |

### Arrange pipeline (CDT fallback)

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| arrange valid 4v | 0.055 | 0.224 | 4.08x |
| arrange valid 10v | 0.132 | 0.324 | 2.46x |
| arrange valid 50v | 0.696 | 0.428 | 0.61x |
| arrange bowtie 4v | 1.274 | 16.20 | 12.72x |
| arrange star 10sp | 2.252 | 13.16 | 5.84x |
| arrange star 50sp | 0.894 | 261.2 | 292.25x |

### Summary of the losses (2026-08-07)

GeoRepair wins 73 of 91 cases; the losses are four known, measured
classes, in order of size:

1. **Valid polygons >= 50v (0.33-0.90x).** Serial make_valid on a
   5000-vertex valid polygon is ~148 us vs GEOS's ~78 us; the parallel
   rows are memory-bandwidth-bound and the ratio widens with size (our
   path makes ~5-6 passes over the coords, GEOS's IsValidOp ~2). The
   pass count is structural (fused basic-form+sub-ULP+envelope scan,
   line collect, chains, grid, winding); see the stage breakdown below.
2. **Valid ls >= 10v (0.42-0.58x).** The eps-class contract price:
   tolerance predicates (eps computation, padded bboxes, margins) vs
   GEOS isSimple's exact-zero pipeline. Serial gap is ~1.5-1.9x, not the
   throughput gap.
3. **Spaghetti (0.63-0.65x).** The one genuine repair loss: dense invalid
   rings force a snap-round + boolean fallback chain (2232 violations on
   a 500-edge walk; see the Polygons note).
4. **Noise-level:** invalid star 100v (0.89x - a valid spiky polygon, not
   invalid repair), collinear ls 500v (0.84x).

Everything else wins, most by 5-3,800x. See the tolerance section below
for why classes 1-2 are a deliberate contract, not inefficiency.

## Tolerance class vs GEOS exact-zero (design position, 2026-08-07)

GEOS validates at exact-zero tolerance: a pair intersects iff the exact
(robust) orientation is zero. GeoRepair validates at the eps-class:
1e-12 x ring scale, adaptive 32-ulp collinear margins, vertex-on-edge at
1e-12 x tested-segment len2. These are different contracts, not one being
"better":

- **GEOS exact-zero answers "does an exact intersection exist".** A pure
  computational-geometry question. It is the ecosystem oracle - PostGIS,
  Shapely, and QGIS all use it - so "valid" in the field means GEOS-valid.
  Its failure mode is silence: real-world coordinates carry quantization
  noise, segments that should touch are often 1e-13 apart, and GEOS
  reports valid (no exact intersection found) while the topology is
  genuinely incoherent at the data's own precision.
- **The eps-class answers "is this topologically coherent at the
  precision the data has".** A data-quality question. Its failure mode is
  the opposite: genuinely clean features closer than 1e-12 x scale are
  treated as touching and may be merged (at that separation, below any
  real-world precision, the call is deliberate).

Why the eps-class exists - repair coherence: the noder clusters at the
validator's eps, the gate uses the validator's predicates, and the Fast
path can only ship what the exit validator accepts (the 2026-08-07 gate
bug class: a gate with looser predicates shipped polygons the exit
validator rejected). GEOS's own pipeline is not coherent at the tolerance
boundary: its `makeValid` on lines ships non-simple output its own
`isSimple` flags (passthrough, verified against GeometryFixer.cpp), and
on polygons it nods at noding tolerance while `isValid` checks exact-zero
- so GEOS can reject its own repaired output. GeoRepair refuses that:
repair output is certifiable by construction (valid-or-empty).

Measured consequences on the real-world dataset: the winding-agnostic
validity agreement with GEOS is 0/0 on 1,579,030 polygons - the eps-class
does not over-flag the production data. The strictness shows on
borderline synthetic cases: the XML suite baselines 213 GEOS-valid
verdicts we reject under the eps-class (the historical 2,298 count was
bug-inflated; see the real-world section). The repaired-output residual
("29 still flagged") belongs to the same pre-correction measurement and
is not reproduced by the current validator.

Costs of the eps-class, beyond performance:

1. **Interop divergence.** 213 XML-suite cases are baselined as expected
   divergence (194 WrongOrientation, 8 RepeatedPoint, 8
   MultiPointDuplicatePoints, 2 RingTooFewPoints, 1 PinchPoint) -
   GEOS-valid verdicts we reject under the eps-class. Triage:
   `docs/MASKED-DIVERGENCE-TRIAGE-2026-08-04.md`.
2. **Scale-dependence in mixed-magnitude rings.** The extremal-vertex
   orientation ~0 zone (fuzz invariant_mixed_fp_in_same_ring) is the one
   place a verdict can depend on representation noise.
3. **Repair artifacts.** The tolerance class can create its own edge
   cases when repairing borderline inputs (a historical "29 of 2,298
   repaired outputs still flagged" measurement, since corrected with the
   validator fixes - the residual is not reproduced today). The class of
   failure remains: a repair at the tolerance boundary can produce
   output the same tolerance flags.

The architecture is deliberately both: the eps-class is the internal
repair contract (validator = gate = noder), GEOS parity is the external
check where interop demands it (geosop oracle, the XML suite, real-world
agreement). Divergences are baselined, not hidden. The valid-input
performance gap is the price of the internal contract; closing it to
GEOS parity would mean abandoning the eps-class, which would break the
repair pipeline's central guarantee.

## Stage breakdown (valid 5000-vertex polygon, serial)

Measured 2026-08-07 (evening, lean-predicate + gate-fusion pass) with a
throwaway stage probe (deleted after the run).

| Stage | Time | Share |
|-------|-----:|------:|
| Fast-path gate (arrange validate_polygon) | 112 us | 76% of make_valid |
| - duplicate scan + basic form | ~45 us | |
| - has_no_intersections (chains + grid + pairs) | 31 us | |
| - line collect + NaN + scans | ~35 us | |
| Winding (enforce_ogc_winding + ogc_orientation_ok) + overhead | ~35 us | 24% |
| **make_valid total** | **147.5 us** | |
| Full validator certification (check_ring_validity) | 230 us | not on the make_valid fast path |

The make_valid fast path is gate + winding: the full certification runs
only when the gate fails. The gate's own cost is dominated by the
duplicate scan (bit-exact open-addressing table; the FxHashSet version
was ~60 us, the table ~45 us - the scan is memory-bound, both are) and
the chain/grid pair machinery. The throughput rows are worse than these
serial numbers suggest because the parallel batch is
memory-bandwidth-bound: our path reads the coords ~5-6 times, GEOS
IsValidOp ~2, and the ratio widens with size (valid polygon 5000v
throughput 0.36x, serial ~1.9x).

The validator certification (230 us) dominates the full-validate path
but is off the make_valid fast path. The validator is a radix-sorted
sweep, O(n log n) - the same asymptotics as GEOS. The constant gap is
the predicate cost: GEOS runs fast-FP orientation with robust escalation
at exact-zero; geo-repair runs fast-FP-first with escalation only within
the 32-ulp collinear gate (decision-identical: 937/937 GEOS XML suite,
100% real-world agreement), plus the eps-class margins. The
fast-FP-first predicate design landed 2026-08-07 and cut real-world
validation from 3.0-3.2 s to 2.5 s.

## The valid-line cost and its history (2026-08-07)

The valid-or-empty line contract (commit 918eabb) requires make_valid to
never ship a non-simple line - deliberately stricter than GEOS, which
passes non-simple lines through unchanged (confirmed via geosop). Every
valid line pays an O(n) simplicity check.

The check's FIRST implementation regressed valid lines 20-35x (100v:
0.235 -> 8.264 us; 500v: 0.76 -> 28.5 us) and shipped in a release with
fully green CI - nothing measured the bench, so nothing caught it. Root
causes, in order of impact:

1. **The check was an O(n^2) naive pair loop for n <= 128, then an rstar
   bulk_load tree above it.** rstar 0.13 `bulk_load` costs ~80 ns/item
   in release (40 us to build a 500-item tree) plus query overhead, and
   the naive loop paid 4 Shewchuk orient2d calls per pair with no bbox
   prefilter - a 500-vertex line was 28.5 us before the fix. (An early
   hypothesis blamed rstar's `_int` queries as "lossy"; verified against
   rstar-0.13.0 source: `_int` is internal iteration with EXACT envelope
   intersection - the wrap theory was wrong, the tree was merely slow.)
2. The `check_edge_pair_intersection`/sweep ring indexing
   (`(i + 1) % n`) silently corrupted OPEN chains: the last segment's
   endpoint wrapped to coords[0], fabricating a phantom crossing chord
   (false "non-simple" on valid sine waves).
3. `edges_intersect_general` had no bbox prefilter - every naive-loop
   pair paid 4 Shewchuk orient2d calls.

Fix (this tree): the line check now runs the radix-sort sweep (rstar
demoted to dense-overlap fallback), the ring indexing is
`(i + 1).min(n)` (identical for closed rings, correct for open chains),
and the predicates gained the eps-padded prefilter + fast-FP-first.

History of the valid ls 500v row, same day: 28.5 us (first check) ->
4.57 us (sweep + fast-FP-first) -> 1.60 us (adjacent single-orient fast
reject, x-sortedness skip, flat-capacity revisit hash) -> 1.33 us (lean
pair predicate with vertex-on-edge-gated escalation; the flat hash
removed, revisit detection moved to escalated pairs only). The residual
~2.4x vs GEOS isSimple is the eps-class contract price, not
inefficiency.

## Edge-split dispatch fix (2026-08-07)

`split_edges` (the noding pair-finder used by single-pass repair) ran an
O(n^2) brute-force loop for every input up to 2000 edges. A 500-edge
bowtie paid 250k exact pair tests (~3.3 ms); the same shape through the
R-tree path is ~450 us. The dispatch gate is now `SPLIT_BRUTEFORCE_MAX_N
= 128` (measured crossover: 100 edges is faster bruteforce, 500 edges is
~7x faster indexed). Result: invalid bowtie 500v 474 -> 62 us
(0.98x -> 8.2x vs GEOS).

## CI regression gate

`scripts/bench_gate.py` runs the synthetic bench on a curated subset
(`BENCH_SUBSET`), compares the parallel column against
`benches/bench_baseline.json`, and fails on >30% regression (cases below
the 1 us floor are exempt as dispatch noise). The first run on a new
environment records the baseline instead of failing; commit the uploaded
artifact. This gate exists because CI was green during the valid-line
regression above - tests alone do not measure performance.

# Benchmarks

Full benchmark tables, methodology, and diagnosis notes for geo-repair.
The README carries the summary; this file is the detail.

## Methodology

- Machine: i5-12400F (6C/12T), Windows 10, release profile (LTO, mimalloc).
- GEOS: conda-forge `libgeos` 3.14.1 (MSVC, serial per-call, no LTO),
  driven concurrently via Rayon, geometries built via CoordSeq direct
  construction (no WKT round-trip). Both columns measured in the same
  process; the GeoRepair column is the Rayon parallel batch.
- Ratio convention everywhere: **GEOS / GeoRepair** - `>1` means
  geo-repair is faster, `<1` means GEOS is faster.
- Measurement rules: always take the settled second run (first-run-after-
  build is inflated ~18% by Windows Defender + cold LTO code); never trust
  a bench binary you cannot trace to a source file (stale
  `target/release/examples/*.exe` have produced phantom numbers before).
- Sub-µs rows are Rayon dispatch noise; read the larger rows and the
  real-world batch numbers.

## Real-world dataset (1,579,030 polygons)

813 features from a production GeoPackage (geometry blobs are WKB),
flattened to 1,579,030 polygon parts. Measured 2026-08-07.

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | 2.47 s (1.57 µs/poly) | 3.46 s (2.19 µs/poly) | 1.40x |
| Full pass (1.58M) | 4.05 s (2.56 µs/poly) | 3.49 s (2.21 µs/poly) | 0.86x |

Validity agreement with GEOS: 100% (0/0 disagreements). GEOS setup cost
(one-time pre-build of 1.58M geometries, 1.35 s) excluded.

## Synthetic benchmarks (2026-08-07)

### Polygons

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| valid polygon 4v | 0.081 | 0.282 | 3.5x |
| valid polygon 10v | 0.217 | 0.365 | 1.7x |
| valid polygon 50v | 1.174 | 0.556 | 0.47x |
| valid polygon 100v | 2.119 | 0.595 | 0.28x |
| valid polygon 500v | 5.079 | 1.167 | 0.23x |
| valid polygon 1000v | 11.442 | 2.371 | 0.21x |
| valid polygon 5000v | 57.822 | 8.554 | 0.15x |
| valid polygon 10000v | 124.039 | 17.863 | 0.14x |
| invalid bowtie 4v | 1.557 | 20.615 | 13x |
| invalid bowtie 50v | 9.206 | 72.438 | 7.9x |
| invalid bowtie 100v | 31.224 | 132.864 | 4.3x |
| invalid bowtie 500v | 66.511 | 547.832 | 8.2x |
| spaghetti 500v | 4991.781 | 2921.201 | 0.59x |
| spaghetti 2000v | 28862.765 | 18333.920 | 0.64x |
| invalid star 100v | 9.070 | 7.340 | 0.81x |
| self-touch poly | 1.187 | 20.841 | 18x |
| collapsed poly | 1.502 | 11.949 | 8.0x |
| near-collinear | 1.395 | 17.684 | 13x |
| large coord 1e12 | 0.558 | 0.236 | 0.42x |

Bowtie rows are subdivided-edge bowties (same crossing, n/4 collinear
segments per edge) - the genuine invalid-at-scale coverage. The 500v row
was 474 us before the 2026-08-07 edge-split dispatch fix (below): the old
2000-edge brute-force gate made the noding O(n^2) with a ~6.6 us/edge
constant; with the 128-edge gate the same row is 66.5 us.

The "invalid star" rows are NOT invalid: geosop isValid returns true for
the star generator at every size (star99/500/1000 verified 2026-08-07).
The row measures a valid spiky polygon - keep it for the valid-spiky
constant gap, but it is not an invalid-repair benchmark.

The spaghetti rows (torus-wrapped random walks, dozens of proper
crossings) are the real dense-invalid class and remain the open gap:
single_pass noding splits cleanly (500 edges -> 925 noded in ~560 us)
but the NodingValidator reports ~2232 violations on the result, forcing
snap-round + a boolean/arrange fallback chain (~30 ms serial). The
violations are predominantly collinear overlaps along the walk's
revisited grid lines - the parametric splitter handles proper crossings
but not the overlap class. GEOS: 2.5 ms serial on the same input.

### LineStrings & MultiLineStrings

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| valid line | 0.009 | 0.039 | 4.3x |
| zero-length line | 0.009 | 0.352 | 39x |
| valid ls 4v | 0.047 | 0.041 | 0.87x |
| valid ls 10v | 0.077 | 0.043 | 0.56x |
| valid ls 50v | 0.494 | 0.073 | 0.15x |
| valid ls 100v | 0.896 | 0.134 | 0.15x |
| valid ls 500v | 4.566 | 0.536 | 0.12x |
| collinear ls 4v | 0.030 | 0.041 | 1.4x |
| collinear ls 10v | 0.096 | 0.043 | 0.45x |
| collinear ls 50v | 0.963 | 0.075 | 0.08x |
| collinear ls 100v | 2.625 | 0.272 | 0.10x |
| collinear ls 500v | 14.024 | 0.495 | 0.035x |
| zigzag ls 10v | 0.100 | 0.044 | 0.44x |
| zigzag ls 50v | 0.956 | 0.099 | 0.10x |
| zigzag ls 100v | 1.949 | 0.196 | 0.10x |
| zigzag ls 500v | 10.691 | 0.661 | 0.062x |
| spiral ls 10v | 0.089 | 0.061 | 0.69x |
| spiral ls 50v | 1.040 | 0.087 | 0.08x |
| spiral ls 100v | 2.265 | 0.176 | 0.08x |
| self-int ls 5v | 0.128 | 0.067 | 0.52x |
| dense self ls 10v | 0.100 | 0.048 | 0.48x |
| dense self ls 50v | 0.669 | 0.090 | 0.13x |
| dense self ls 100v | 1.196 | 0.167 | 0.14x |
| duped ls 100v | 1.111 | 0.130 | 0.12x |
| mls 50x3v | 2.853 | 2.644 | 0.93x |
| self-int mls 50x4v | 3.329 | 3.067 | 0.92x |

### Special shapes

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| star-burst 10sp | 0.157 | 0.069 | 0.44x |
| star-burst 50sp | 0.301 | 0.131 | 0.44x |
| star-burst 100sp | 0.512 | 0.293 | 0.57x |
| star-burst 500sp | 2.909 | 1.839 | 0.63x |
| collinear ov 10seg | 0.382 | 0.075 | 0.20x |
| collinear ov 50seg | 2.417 | 0.189 | 0.08x |
| collinear ov 100seg | 6.552 | 0.420 | 0.06x |
| collinear ov 500seg | 98.148 | 2.303 | 0.023x |
| x-scale 10v | 0.205 | 0.058 | 0.28x |
| x-scale 50v | 1.210 | 0.388 | 0.32x |
| x-scale 100v | 6.557 | 0.609 | 0.09x |
| ringing 100v | 2.367 | 0.136 | 0.06x |
| ringing 500v | 32.080 | 2.630 | 0.08x |
| hilbert 256v | 2.637 | 0.375 | 0.14x |
| hilbert 1024v | 14.338 | 1.898 | 0.13x |
| lissajous 200v | 3.180 | 0.208 | 0.07x |
| lissajous 500v | 21.290 | 0.544 | 0.026x |
| lissajous 1000v | 83.487 | 2.052 | 0.025x |
| lissajous 7:4 500v | 30.340 | 0.552 | 0.018x |
| spoke 10sp | 0.096 | 0.053 | 0.55x |
| spoke 50sp | 0.263 | 0.128 | 0.49x |
| spoke 100sp | 0.897 | 0.266 | 0.30x |
| spoke 500sp | 4.966 | 2.006 | 0.40x |
| star-comb 20sp | 0.263 | 0.056 | 0.21x |
| star-comb 100sp | 6.020 | 0.151 | 0.025x |
| star-comb 500sp | 139.273 | 1.221 | 0.009x |

### Holes, overlaps & grids

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| hole hier 5h | 2.072 | 1.878 | 0.91x |
| hole hier 20h | 11.178 | 9.631 | 0.86x |
| hole hier 50h | 33.155 | 22.809 | 0.69x |
| overlap mp 5sh | 2.379 | 493.573 | 207x |
| overlap mp 20sh | 7.892 | 2674.487 | 339x |
| overlap mp 50sh | 27.450 | 7468.206 | 272x |
| dense grid 5x5=25 | 9.325 | 1834.135 | 197x |
| dense grid 10x10=100 | 50.567 | 14327.071 | 283x |
| dense grid 20x20=400 | 565.974 | 110474.150 | 195x |
| sliver 100v | 3.325 | 2.932 | 0.88x |
| sliver 500v | 18.091 | 15.169 | 0.84x |

### Arrange pipeline (CDT fallback)

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio |
|-----------|---------------:|----------:|------:|
| arrange valid 4v | 0.056 | 0.228 | 4.1x |
| arrange valid 10v | 0.166 | 0.299 | 1.8x |
| arrange valid 50v | 0.688 | 0.452 | 0.66x |
| arrange bowtie 4v | 1.255 | 18.917 | 15x |
| arrange star 10sp | 0.110 | 0.066 | 0.60x |
| arrange star 50sp | 0.352 | 0.245 | 0.70x |

## Stage breakdown (valid 5000-vertex polygon, serial)

Measured 2026-08-07 with a throwaway stage probe (deleted after the run).

| Stage | Time | Share |
|-------|-----:|------:|
| Fast-path gate sweep | 90.6 µs | 24% |
| Full validator certification | 230.6 µs | 60% |
| Winding + overhead | 6.5 µs | 2% |
| **Total** | 382 µs | |

The validator certification dominates the valid-polygon cost: make_valid
on a valid input still runs the full OGC validator before returning the
input unchanged. The validator is a radix-sorted sweep, O(n log n) - the
same asymptotics as GEOS. The constant gap is the predicate cost: GEOS
runs fast-FP orientation with robust escalation; geo-repair ran Shewchuk
exact predicates on every candidate pair until 2026-08-07, when
`edges_intersect_general` and `segments_collinear_overlap` gained the
fast-FP-first design (escalate only when an orientation sits within the
32-ulp collinear gate, plus an eps-padded bbox prefilter). The change is
decision-identical by construction (the fast error is ~4 ulps of L2,
below both gates) and was verified: 937/937 GEOS XML suite, 100%
real-world validity agreement, geo_bridge strictness tests.

## The valid-line cost and its history (2026-08-07)

The valid-or-empty line contract (commit 918eabb) requires make_valid to
never ship a non-simple line - deliberately stricter than GEOS, which
passes non-simple lines through unchanged (confirmed via geosop). Every
valid line pays an O(n) simplicity check.

The check's FIRST implementation regressed valid lines 20-35x (100v:
0.235 -> 8.264 µs; 500v: 0.76 -> 28.5 µs) and shipped in a release with
fully green CI - nothing measured the bench, so nothing caught it. Root
causes, in order of impact:

1. **The check was an O(n^2) naive pair loop for n <= 128, then an rstar
   bulk_load tree above it.** rstar 0.13 `bulk_load` costs ~80 ns/item
   in release (40 µs to build a 500-item tree) plus query overhead, and
   the naive loop paid 4 Shewchuk orient2d calls per pair with no bbox
   prefilter - a 500-vertex line was 28.5 µs before the fix. (An early
   hypothesis blamed rstar's `_int` queries as "lossy"; verified against
   rstar-0.13.0 source: `_int` is internal iteration with EXACT envelope
   intersection - the wrap theory was wrong, the tree was merely slow.)
2. The `check_edge_pair_intersection`/sweep ring indexing
   (`(i + 1) % n`) silently corrupted OPEN chains: the last segment's
   endpoint wrapped to coords[0], fabricating a phantom crossing chord
   (false "non-simple" on valid sine waves).
3. `edges_intersect_general` had no bbox prefilter - every naive-loop
   pair paid 4 Shewchuk orient2d calls.

Fix (this tree): the line check now runs an O(n) vertex-revisit hash +
the radix-sort sweep (rstar demoted to dense-overlap fallback), the ring
indexing is `(i + 1).min(n)` (identical for closed rings, correct for
open chains), and the predicates gained the eps-padded prefilter +
fast-FP-first. Result: valid ls 500v 28.5 -> 4.57 µs; the residual
4-6x vs the pre-contract baseline is the simplicity check itself.

Known remaining gap (2026-08-07): the valid-polygon rows (0.14-0.5x vs
GEOS) are dominated by the exit certification: make_valid runs the fast-
path gate sweep AND then the full OGC validator's own sweep before
returning a valid input unchanged - two O(n log n) passes where GEOS
runs one IsValidOp. The gate only proves proper-crossing freedom + a
first-vertex hole probe, so the validator cannot be skipped without
extending the gate to the validator's full pair predicate (collinear
overlap + vertex-on-edge), the O(n) rules (repeated points, closure,
pinch), and the full hole checks.

## Edge-split dispatch fix (2026-08-07)

`split_edges` (the noding pair-finder used by single-pass repair) ran an
O(n^2) brute-force loop for every input up to 2000 edges. A 500-edge
bowtie paid 250k exact pair tests (~3.3 ms); the same shape through the
R-tree path is ~450 us. The dispatch gate is now `SPLIT_BRUTEFORCE_MAX_N
= 128` (measured crossover: 100 edges is faster bruteforce, 500 edges is
~7x faster indexed). Result: invalid bowtie 500v 474 -> 66.5 us
(0.98x -> 8.2x vs GEOS).

## CI regression gate

`scripts/bench_gate.py` runs the synthetic bench on a curated subset
(`BENCH_SUBSET`), compares the parallel column against
`benches/bench_baseline.json`, and fails on >30% regression (cases below
the 1 µs floor are exempt as dispatch noise). The first run on a new
environment records the baseline instead of failing; commit the uploaded
artifact. This gate exists because CI was green during the valid-line
regression above - tests alone do not measure performance.

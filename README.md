# GeoRepair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.85+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)
[![Status](https://img.shields.io/badge/status-experimental-orange.svg)]()

> **This crate is experimental.**  The API is actively evolving — expect
> breaking changes between 0.x releases.  Core algorithms, I/O backends,
> and feature flags are all subject to change as we improve correctness
> and performance.

OGC geometry repair and validation for Rust. Built-in I/O for WKB, WKT,
and a custom binary batch format — no extra dependencies required.
Detects and fixes invalid
GIS geometries (self-intersections, unclosed rings, degenerate shapes, NaN
coordinates, and more) using algorithms selected by geometry type.

The **Structure** strategy (default) mirrors GEOS's ST_MakeValid
algorithm: planar graph extraction, face walking, and winding-number
assembly.  The **Arrange** strategy uses CDT-based repair as a robust
fallback for complex topologies.  Passes 2490/2490 GEOS XML validation
tests, with parallel batch performance roughly **on par with GEOS** on
1.58M data set polygons (full dataset 0.92×, validation 3.3×).

See the [full documentation](https://docs.rs/geo-repair) for quick-start
examples, validation rules, CRS support, I/O backends, Python bindings,
C FFI, and known limitations.

## Performance

### Real-world dataset (1,578,988 polygons)

Structure batch on a production GIS dataset.  GEOS linked via
conda-forge (MSVC, serial internally, no LTO).  i5-12400F (6C/12T),
mimalloc.  All columns are parallel batch (Rayon 12 threads); serial
included for reference.

| Dataset | GeoRepair (ser) | GeoRepair (par) | GEOS (par batch) | vs GEOS |
|---------|----------------:|----------------:|-----------------:|:-------:|
| Validation (1.58M) | 3.14 s | **1.13 s** | **3.78 s** | **3.3×** |
| Invalid subset (1855 polys) | 5.78 s | **1.96 s** | **1.88 s** | **1.04×** |
| Full dataset (1.58M polys) | 9.40 s | **3.34 s** | **3.61 s** | **0.92×** |

### Synthetic benchmarks (CoordSeq direct — no WKT overhead)

Structure strategy, i5-12400F (6C/12T).  **GEOS benchmark uses CoordSeq
direct construction, NOT WKT — this is a fair comparison.**  Serial column
is single-threaded; parallel column uses Rayon 12-thread batch.

Stars indicate improvement: `***` ≥100× · `**` 10–100× · `*` 1–10× · (blank) <1× (GEOS faster)

<table>
<thead><tr><th>Benchmark</th><th align=right>Ser (µs)</th><th align=right>Par (µs)</th><th align=right>GEOS (µs)</th><th align=right>Ratio (ser)</th><th align=center>★</th><th align=right>Ratio (par)</th><th align=center>★</th></tr></thead>
<tbody>
<tr><td>Valid polygon 4v</td><td align=right>0.21</td><td align=right>0.10</td><td align=right>1.38</td><td align=right>6.6×</td><td align=center>*</td><td align=right>13.8×</td><td align=center>**</td></tr>
<tr><td>Valid polygon 1000v</td><td align=right>8.39</td><td align=right>7.22</td><td align=right>10.5</td><td align=right>1.3×</td><td align=center>*</td><td align=right>1.5×</td><td align=center>*</td></tr>
<tr><td>Valid polygon 10000v</td><td align=right>49.5</td><td align=right>37.6</td><td align=right>90.9</td><td align=right>1.8×</td><td align=center>*</td><td align=right>2.4×</td><td align=center>*</td></tr>
<tr><td>Invalid bowtie 4v</td><td align=right>2.09</td><td align=right>0.33</td><td align=right>15.2</td><td align=right>7.3×</td><td align=center>*</td><td align=right>46×</td><td align=center>**</td></tr>
<tr><td>Collapsed poly</td><td align=right>0.76</td><td align=right>0.17</td><td align=right>10.0</td><td align=right>13×</td><td align=center>**</td><td align=right>59×</td><td align=center>**</td></tr>
<tr><td>Self-touching poly</td><td align=right>4.13</td><td align=right>1.08</td><td align=right>16.2</td><td align=right>3.9×</td><td align=center>*</td><td align=right>15×</td><td align=center>**</td></tr>
<tr><td>Near-collinear poly</td><td align=right>1.40</td><td align=right>0.33</td><td align=right>16.9</td><td align=right>12×</td><td align=center>**</td><td align=right>51×</td><td align=center>**</td></tr>
<tr><td>Hilbert curve 256v</td><td align=right>0.58</td><td align=right>0.56</td><td align=right>0.84</td><td align=right>1.4×</td><td align=center>*</td><td align=right>1.5×</td><td align=center>*</td></tr>
<tr><td>Hilbert curve 1024v</td><td align=right>2.38</td><td align=right>1.93</td><td align=right>3.29</td><td align=right>1.4×</td><td align=center>*</td><td align=right>1.7×</td><td align=center>*</td></tr>
<tr><td>Lissajous 200v</td><td align=right>0.39</td><td align=right>0.46</td><td align=right>0.83</td><td align=right>2.1×</td><td align=center>*</td><td align=right>1.8×</td><td align=center>*</td></tr>
<tr><td>Lissajous 1000v</td><td align=right>3.50</td><td align=right>4.28</td><td align=right>6.00</td><td align=right>1.7×</td><td align=center>*</td><td align=right>1.4×</td><td align=center>*</td></tr>
<tr><td>Star-burst 10sp</td><td align=right>0.27</td><td align=right>0.07</td><td align=right>0.31</td><td align=right>1.1×</td><td align=center>*</td><td align=right>4.4×</td><td align=center>*</td></tr>
<tr><td>Star-burst 500sp</td><td align=right>8.99</td><td align=right>1.14</td><td align=right>2.15</td><td align=right>0.2×</td><td align=center></td><td align=right>1.9×</td><td align=center>*</td></tr>
<tr><td>Spoke wheel 10sp</td><td align=right>0.22</td><td align=right>0.06</td><td align=right>0.26</td><td align=right>1.2×</td><td align=center>*</td><td align=right>4.3×</td><td align=center>*</td></tr>
<tr><td>Spoke wheel 100sp</td><td align=right>2.10</td><td align=right>0.43</td><td align=right>0.98</td><td align=right>0.5×</td><td align=center></td><td align=right>2.3×</td><td align=center>*</td></tr>
<tr><td>Spoke wheel 500sp</td><td align=right>8.71</td><td align=right>2.08</td><td align=right>2.79</td><td align=right>0.3×</td><td align=center></td><td align=right>1.3×</td><td align=center>*</td></tr>
<tr><td>Star-comb 20sp</td><td align=right>0.23</td><td align=right>0.10</td><td align=right>0.41</td><td align=right>1.8×</td><td align=center>*</td><td align=right>4.1×</td><td align=center>*</td></tr>
<tr><td>Star-comb 100sp</td><td align=right>0.81</td><td align=right>0.20</td><td align=right>0.87</td><td align=right>1.1×</td><td align=center>*</td><td align=right>4.4×</td><td align=center>*</td></tr>
<tr><td>Star-comb 500sp</td><td align=right>4.02</td><td align=right>0.83</td><td align=right>2.66</td><td align=right>0.7×</td><td align=center></td><td align=right>3.2×</td><td align=center>*</td></tr>
<tr><td>Collinear overlap 10seg</td><td align=right>0.29</td><td align=right>0.08</td><td align=right>0.48</td><td align=right>1.7×</td><td align=center>*</td><td align=right>6.0×</td><td align=center>*</td></tr>
<tr><td>Collinear overlap 500seg</td><td align=right>10.8</td><td align=right>1.95</td><td align=right>3.64</td><td align=right>0.3×</td><td align=center></td><td align=right>1.9×</td><td align=center>*</td></tr>
<tr><td>Hole hierarchy 5h</td><td align=right>1.85</td><td align=right>1.17</td><td align=right>2.31</td><td align=right>1.2×</td><td align=center>*</td><td align=right>2.0×</td><td align=center>*</td></tr>
<tr><td>Hole hierarchy 50h</td><td align=right>17.5</td><td align=right>12.8</td><td align=right>30.3</td><td align=right>1.7×</td><td align=center>*</td><td align=right>2.4×</td><td align=center>*</td></tr>
<tr><td>Overlapping MP 5sh</td><td align=right>3.98</td><td align=right>1.41</td><td align=right>346</td><td align=right>87×</td><td align=center>**</td><td align=right>245×</td><td align=center>***</td></tr>
<tr><td>Overlapping MP 20sh</td><td align=right>19.3</td><td align=right>6.72</td><td align=right>2787</td><td align=right>144×</td><td align=center>***</td><td align=right>415×</td><td align=center>***</td></tr>
<tr><td>Overlapping MP 50sh</td><td align=right>44.8</td><td align=right>15.2</td><td align=right>7066</td><td align=right>158×</td><td align=center>***</td><td align=right>465×</td><td align=center>***</td></tr>
<tr><td>Dense grid 5×5=25</td><td align=right>13.8</td><td align=right>5.19</td><td align=right>1538</td><td align=right>111×</td><td align=center>***</td><td align=right>296×</td><td align=center>***</td></tr>
<tr><td>Dense grid 10×10=100</td><td align=right>63.8</td><td align=right>28.0</td><td align=right>16563</td><td align=right>260×</td><td align=center>***</td><td align=right>592×</td><td align=center>***</td></tr>
<tr><td>Dense grid 20×20=400</td><td align=right>283</td><td align=right>136</td><td align=right>101193</td><td align=right>358×</td><td align=center>***</td><td align=right>744×</td><td align=center>***</td></tr>
</tbody>
</table>

**Arrange pipeline (CDT fallback):**

| Benchmark | GeoRepair (par) | GEOS (par batch) | Ratio |
|-----------|----------------:|-----------------:|:-----:|
| Valid polygon 4v | 0.09 µs | 1.49 µs | 17× |
| Valid polygon 50v | 1.65 µs | 3.84 µs | 2.3× |
| Invalid bowtie 4v | 0.65 µs | 17.4 µs | 27× |
| Star-burst 10sp | 0.09 µs | 0.35 µs | 3.9× |
| Star-burst 50sp | 0.51 µs | 0.87 µs | 1.7× |

**Notes:**
- conda-forge `libgeos` on Windows is MSVC, serial per-call, no LTO.
  "par batch" = many GEOS C calls run concurrently via Rayon (12 threads).
- **Synthetic benchmarks use CoordSeq direct GEOS construction (no WKT).**
  WKT round-trip inflated earlier GEOS numbers by 10-120×.  Real-world
  benchmark always used CoordSeq (fair).
- GeoRepair parallel speedup is limited on sub-5µs geometries (Rayon
  overhead dominates).  Real-world batches of 1.58M polygons show better
  throughput scaling (2-3× vs serial).
- GeoRepair wins big on **invalid polygon repair** and **MultiPolygon
  unification** (C++ quadratic algorithms hit their worst case).  GEOS wins
  on large valid polygons where C++ optimization and mature JTS codebase
  dominate.

### Run benchmarks

```shell
# Real-world dataset benchmark (system GEOS — conda-forge)
cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world

# Real-world dataset benchmark (static GEOS — built from source)
cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp --bench real_world

# Synthetic benchmarks with serial + parallel columns (no GEOS)
cargo bench --features arrange,structure,parallel,simd --bench bench

# Synthetic benchmarks with GEOS comparison
cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench bench

# Criterion microbenchmarks
cargo bench --features bench-criterion --bench criterion
```

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `arrange` | CDT-based polygon repair (requires `spade`) | yes |
| `structure` | Structure-based fast path repair | yes |
| `parallel` | Rayon parallel processing (non-WASM) | yes |
| `simd` | Retained for compatibility; stable builds use auto-vectorized scalar kernels (hand-written AVX2 measured slower) | yes |
| `validate` | OGC validation predicates | yes |
| `mimalloc` | Use mimalloc global allocator | yes |
| `std` | Standard library + file I/O. Disable for no_std builds. | yes |
| `simd-portable` | Portable SIMD via `core::simd` (nightly only) | no |
| `memmap` | Memory-mapped binary file loading | no |
| `wasm` | WASM browser fetch (synchronous XHR) | no |
| `proj` | CRS transformation (placeholder) | no |
| `serde` | Geometry serde support (`geo/serde`) | no |
| `ffi` | C-compatible FFI bindings | no |
| `python` | Python bindings via PyO3 | no |
| `io-shp` | Shapefile format backend | no |
| `io-wkt` | No-op (WKT is built-in, kept for CI compatibility) | no |
| `io-csv` | CSV format backend | no |
| `io-gml` | GML/XML format backend | no |
| `io-gpkg` | GeoPackage format backend (not WASM) | no |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `bench-geos` | GEOS comparison benchmarks (build from source — MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS — conda-forge MSVC) | no |
| `bench-criterion` | Criterion benchmark harness | no |

## License

Apache-2.0

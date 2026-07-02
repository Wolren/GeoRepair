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
tests, with parallel batch performance **on par with GEOS** on
1.58M data set polygons (full dataset 0.92×, validation 3.4×).

See the [full documentation](https://docs.rs/geo-repair) for quick-start
examples, validation rules, CRS support, I/O backends, Python bindings,
C FFI, and known limitations.

## What it does

Real-world GIS data often contains geometry defects.  GeoRepair detects
these via OGC-style validation and repairs them:

| Geometry | Repair approach |
|----------|----------------|
| `Polygon` / `MultiPolygon` | Two strategies (Structure fast path / Arrange CDT fallback) |
| `LineString` / `MultiLineString` | NaN filtering, duplicate removal, self-intersection noding |
| `Line` | Zero-length and NaN detection |
| `Point` / `MultiPoint` | NaN/Inf filtering, deduplication |
| `Rect` / `Triangle` | Basic degeneracy checks |
| `GeometryCollection` | Recursive repair of children |

## Performance

### Real-world dataset (1,578,988 polygons)

Structure parallel batch on a production GIS dataset.  GEOS linked via
conda-forge (MSVC, serial internally, no LTO).  i5-12400F (6C/12T),
mimalloc.  Both benchmarks run in parallel via Rayon batch over all
polygons (GEOS per-poly serial, but many polys run concurrently).

| Dataset | GeoRepair | Per-poly | GEOS | Per-poly | vs GEOS |
|---------|-----------|----------|------|----------|---------|
| Invalid subset (1855 polys) | **1.96 s** | 1.06 ms | **1.88 s** | 1.02 ms | *1.04×* |
| Full dataset (1.58M polys) | **3.34 s** | 2.1 µs | **3.61 s** | 2.3 µs | *0.92×* |

Validation comparison on full dataset — our validator is **3.4× faster**:

| Validator | total | per-poly |
|-----------|-------|----------|
| Geo-repair | 1.13 s | 0.71 µs |
| GEOS isValid | 3.78 s | 2.40 µs |

### Synthetic benchmarks

Structure strategy, i5-12400F (6C/12T).  GEOS linked via conda-forge
(MSVC, serial, no LTO).  GEOS call includes WKT round-trip (encode to
WKT, decode by GEOS).  GeoRepair serial column is apples-to-apples
(single-threaded); parallel column shows the Rayon batch speedup.

Ratio color key:
<span style="background:#1a6b1a;color:white;padding:0 6px;font-weight:bold">≥100×</span> massive ·
<span style="background:#c6efce;padding:0 6px">10–100×</span> great ·
<span style="background:#ffeb9c;padding:0 6px">1–10×</span> modest ·
<span style="background:#ffc7ce;padding:0 6px">&lt;1×</span> GEOS faster

<table>
<thead><tr><th>Benchmark</th><th align=right>Ser (µs)</th><th align=right>Par (µs)</th><th align=right>GEOS (µs)</th><th align=right>Ratio (ser)</th><th align=right>Ratio (par)</th></tr></thead>
<tbody>
<tr><td>Valid polygon 4v</td><td align=right>0.21</td><td align=right>0.10</td><td align=right>3.79</td><td align=right style='background:#c6efce'>18×</td><td align=right style='background:#c6efce'>38×</td></tr>
<tr><td>Valid polygon 50v</td><td align=right>0.43</td><td align=right>0.32</td><td align=right>5.13</td><td align=right style='background:#c6efce'>12×</td><td align=right style='background:#c6efce'>16×</td></tr>
<tr><td>Valid polygon 500v</td><td align=right>3.06</td><td align=right>1.77</td><td align=right>34.1</td><td align=right style='background:#c6efce'>11×</td><td align=right style='background:#c6efce'>19×</td></tr>
<tr><td>Valid polygon 10000v</td><td align=right>49.5</td><td align=right>37.6</td><td align=right>647</td><td align=right style='background:#c6efce'>13×</td><td align=right style='background:#c6efce'>17×</td></tr>
<tr><td>Invalid bowtie 4v</td><td align=right>2.09</td><td align=right>0.33</td><td align=right>17.8</td><td align=right style='background:#ffeb9c'>8.5×</td><td align=right style='background:#c6efce'>54×</td></tr>
<tr><td>Invalid star 100v</td><td align=right>25.6</td><td align=right>4.46</td><td align=right>22.5</td><td align=right style='background:#ffc7ce'>0.9×</td><td align=right style='background:#ffeb9c'>5.0×</td></tr>
<tr><td>Self-touching poly</td><td align=right>4.13</td><td align=right>1.10</td><td align=right>20.8</td><td align=right style='background:#ffeb9c'>5.0×</td><td align=right style='background:#c6efce'>19×</td></tr>
<tr><td>Collapsed poly</td><td align=right>0.76</td><td align=right>0.19</td><td align=right>27.9</td><td align=right style='background:#c6efce'>37×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>147×</td></tr>
<tr><td>Near-collinear poly</td><td align=right>1.40</td><td align=right>0.44</td><td align=right>42.5</td><td align=right style='background:#c6efce'>30×</td><td align=right style='background:#c6efce'>97×</td></tr>
<tr><td>Hilbert curve 256v</td><td align=right>0.58</td><td align=right>0.56</td><td align=right>14.3</td><td align=right style='background:#c6efce'>25×</td><td align=right style='background:#c6efce'>26×</td></tr>
<tr><td>Hilbert curve 1024v</td><td align=right>2.38</td><td align=right>1.93</td><td align=right>34.7</td><td align=right style='background:#c6efce'>15×</td><td align=right style='background:#c6efce'>18×</td></tr>
<tr><td>Lissajous 200v</td><td align=right>0.39</td><td align=right>0.46</td><td align=right>20.5</td><td align=right style='background:#c6efce'>53×</td><td align=right style='background:#c6efce'>45×</td></tr>
<tr><td>Lissajous 1000v</td><td align=right>3.50</td><td align=right>4.28</td><td align=right>98.7</td><td align=right style='background:#c6efce'>28×</td><td align=right style='background:#c6efce'>23×</td></tr>
<tr><td>Star-burst 10sp</td><td align=right>0.27</td><td align=right>0.07</td><td align=right>7.26</td><td align=right style='background:#c6efce'>27×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>104×</td></tr>
<tr><td>Star-burst 50sp</td><td align=right>0.95</td><td align=right>0.21</td><td align=right>12.9</td><td align=right style='background:#c6efce'>14×</td><td align=right style='background:#c6efce'>61×</td></tr>
<tr><td>Star-burst 100sp</td><td align=right>1.97</td><td align=right>0.32</td><td align=right>24.7</td><td align=right style='background:#c6efce'>13×</td><td align=right style='background:#c6efce'>77×</td></tr>
<tr><td>Star-burst 500sp</td><td align=right>8.99</td><td align=right>1.26</td><td align=right>122</td><td align=right style='background:#c6efce'>14×</td><td align=right style='background:#c6efce'>97×</td></tr>
<tr><td>Spoke wheel 10sp</td><td align=right>0.22</td><td align=right>0.06</td><td align=right>5.91</td><td align=right style='background:#c6efce'>27×</td><td align=right style='background:#c6efce'>99×</td></tr>
<tr><td>Spoke wheel 50sp</td><td align=right>0.73</td><td align=right>0.15</td><td align=right>8.06</td><td align=right style='background:#c6efce'>11×</td><td align=right style='background:#c6efce'>54×</td></tr>
<tr><td>Spoke wheel 100sp</td><td align=right>2.10</td><td align=right>0.43</td><td align=right>14.0</td><td align=right style='background:#ffeb9c'>6.7×</td><td align=right style='background:#c6efce'>33×</td></tr>
<tr><td>Spoke wheel 500sp</td><td align=right>8.71</td><td align=right>1.32</td><td align=right>71.6</td><td align=right style='background:#ffeb9c'>8.2×</td><td align=right style='background:#c6efce'>54×</td></tr>
<tr><td>Star-comb 20sp</td><td align=right>0.23</td><td align=right>0.10</td><td align=right>6.73</td><td align=right style='background:#c6efce'>29×</td><td align=right style='background:#c6efce'>67×</td></tr>
<tr><td>Star-comb 100sp</td><td align=right>0.81</td><td align=right>0.20</td><td align=right>12.1</td><td align=right style='background:#c6efce'>15×</td><td align=right style='background:#c6efce'>61×</td></tr>
<tr><td>Star-comb 500sp</td><td align=right>4.02</td><td align=right>0.83</td><td align=right>48.3</td><td align=right style='background:#c6efce'>12×</td><td align=right style='background:#c6efce'>58×</td></tr>
<tr><td>Collinear overlap 10seg</td><td align=right>0.29</td><td align=right>0.08</td><td align=right>5.11</td><td align=right style='background:#c6efce'>18×</td><td align=right style='background:#c6efce'>64×</td></tr>
<tr><td>Collinear overlap 50seg</td><td align=right>1.19</td><td align=right>0.24</td><td align=right>5.21</td><td align=right style='background:#ffeb9c'>4.4×</td><td align=right style='background:#c6efce'>22×</td></tr>
<tr><td>Collinear overlap 100seg</td><td align=right>2.50</td><td align=right>0.49</td><td align=right>8.15</td><td align=right style='background:#ffeb9c'>3.3×</td><td align=right style='background:#c6efce'>17×</td></tr>
<tr><td>Collinear overlap 500seg</td><td align=right>10.8</td><td align=right>1.95</td><td align=right>41.3</td><td align=right style='background:#ffeb9c'>3.8×</td><td align=right style='background:#c6efce'>21×</td></tr>
<tr><td>Hole hierarchy 5h</td><td align=right>1.85</td><td align=right>1.17</td><td align=right>8.24</td><td align=right style='background:#ffeb9c'>4.5×</td><td align=right style='background:#ffeb9c'>7.0×</td></tr>
<tr><td>Hole hierarchy 20h</td><td align=right>5.56</td><td align=right>3.65</td><td align=right>14.2</td><td align=right style='background:#ffeb9c'>2.6×</td><td align=right style='background:#ffeb9c'>3.9×</td></tr>
<tr><td>Hole hierarchy 50h</td><td align=right>17.5</td><td align=right>12.8</td><td align=right>59.7</td><td align=right style='background:#ffeb9c'>3.4×</td><td align=right style='background:#ffeb9c'>4.7×</td></tr>
<tr><td>Overlapping MP 5sh</td><td align=right>3.98</td><td align=right>1.41</td><td align=right>421</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>106×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>299×</td></tr>
<tr><td>Overlapping MP 20sh</td><td align=right>19.3</td><td align=right>6.72</td><td align=right>2581</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>134×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>384×</td></tr>
<tr><td>Overlapping MP 50sh</td><td align=right>44.8</td><td align=right>15.2</td><td align=right>6649</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>148×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>437×</td></tr>
<tr><td>Dense grid 5×5=25</td><td align=right>13.8</td><td align=right>5.19</td><td align=right>1631</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>118×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>314×</td></tr>
<tr><td>Dense grid 10×10=100</td><td align=right>63.8</td><td align=right>28.0</td><td align=right>14005</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>220×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>500×</td></tr>
<tr><td>Dense grid 20×20=400</td><td align=right>283</td><td align=right>136</td><td align=right>100012</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>353×</td><td align=right style='background:#1a6b1a;color:white;font-weight:bold'>735×</td></tr>
<tr><td>Sliver polygon 100v</td><td align=right>3.38</td><td align=right>2.02</td><td align=right>18.2</td><td align=right style='background:#ffeb9c'>5.4×</td><td align=right style='background:#ffeb9c'>9.0×</td></tr>
<tr><td>Sliver polygon 500v</td><td align=right>16.6</td><td align=right>7.60</td><td align=right>72.4</td><td align=right style='background:#ffeb9c'>4.4×</td><td align=right style='background:#ffeb9c'>9.5×</td></tr>
</tbody>
</table>

**Arrange pipeline (CDT fallback):**

| Benchmark | GeoRepair (par) | GEOS (par batch) | Ratio |
|-----------|----------------:|-----------------:|------:|
| Valid polygon 4v | 0.09 µs | 3.93 µs | 42× |
| Valid polygon 50v | 1.65 µs | 5.62 µs | 3.4× |
| Invalid bowtie 4v | 0.65 µs | 18.1 µs | 28× |
| Star-burst 10sp | 0.09 µs | 7.25 µs | 79× |
| Star-burst 50sp | 0.51 µs | 12.8 µs | 25× |

**Notes on GEOS comparison:**
- conda-forge `libgeos` on Windows is compiled with MSVC, runs single-threaded internally, and does not use LTO. The "par batch" columns run many GEOS calls concurrently via Rayon (throughput parallelism), since GEOS per-call is serial.
- Synthetic GEOS numbers include WKT serialization/deserialization overhead. The real-world benchmark uses GEOS CoordSeq direct construction (no WKT) for fair comparison.
- GeoRepair parallel speedup is typically 1-3× on 12 cores for synthetic shapes (sub-5µs per call — Rayon overhead dominates). For real-world batches of 1.58M polygons, the throughput advantage is larger due to better amortization.

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
| `simd` | AVX2-accelerated orientation tests (x86_64) | yes |
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

//! Core configuration, error types, and numeric constants for geometry repair.
//!
//! Provides [`MakeValidConfig`] for fine-grained control over the repair
//! strategy, [`PolyMethod`] to select the polygon repair algorithm, and
//! [`MakeValidError`] as the crate's unified error type for repair,
//! validation, WKB parsing, and I/O operations.
//!
//! Also defines internal numeric constants used across the crate for
//! epsilon comparisons, grid thresholds, and snap scaling.

use alloc::string::String;
use geo::algorithm::bool_ops::FillRule;
use thiserror::Error;

use crate::crs::Crs;

// ---------------------------------------------------------------------------
// Numeric constants
// ---------------------------------------------------------------------------

/// Default geometric epsilon for robust equality / zero checks.
pub(crate) const EPS: f64 = 1e-12;

/// Epsilon for parametric (t/u) intersection comparisons.
pub(crate) const EPS_PARAM: f64 = 1e-14;

/// Compile-time stack capacity for the small-ring fast paths: the
/// `SmallVec` inline buffers in the structure gate and the fixed-size
/// `ring_of` array in `arrange::prep_intersect`. `Tuning::small_ring_lines`
/// defaults to this value; runtime values above it only widen the
/// line-collection dispatch (SmallVec spills to the heap), while the
/// fixed-array paths clamp at this constant.
pub const SMALL_RING_LINES: usize = 32;

/// Snap scale factor for integer-keyed graph construction.
pub(crate) const SNAP_SCALE: f64 = 1e8;

/// Performance dispatch thresholds for the repair pipeline.
///
/// Every field is a measured routing decision (see the per-field docs).
/// Defaults match the constants the crate shipped before they became
/// tunable; the values are coupled to the shape distribution of the input
/// dataset, so change them only when you have benchmarked your workload.
///
/// Set per-call on [`MakeValidConfig::tuning`]; the public leaf helpers
/// that take no config (e.g. `arrange::validate_polygon`) use the
/// defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    /// Vertex count threshold above which grid-based spatial indexing is
    /// used instead of brute-force O(n²) checks for self-intersection
    /// detection and edge splitting. At n=2000 the brute-force 2M pair
    /// checks completes in ~5ms for most geometries, well below the cost
    /// of grid construction.
    pub grid_threshold_n: usize,
    /// Edge-split dispatch: below this, the O(n^2) pair loop is the fastest
    /// option; at or above it the R-tree / sweep-line noding wins. Measured
    /// 2026-08-07: a 500-edge bowtie took 3.29 ms through the bruteforce
    /// (250k exact pair tests) vs ~260 us via the indexed paths; a 100-edge
    /// bowtie was faster bruteforce (173 us vs 337 us rtree bulk_load) - the
    /// crossover sits near 128, so the old 2000-edge gate was a 5-60x
    /// latency tax on mid-size repairs.
    pub split_bruteforce_max_n: usize,
    /// Edge-split rebuild dispatch: above this edge count the per-edge
    /// split-point reconstruction runs in parallel; below it the serial
    /// loop's lower dispatch overhead wins (the serial rebuild was ~30ms
    /// of the 71ms noding on a 260k-edge shell).
    pub split_rebuild_parallel_min: usize,
    /// Maximum total vertices for the fast-path validity check in
    /// `structure::fix_polygon`. Larger polygons fall through to the full
    /// repair pipeline.
    pub fast_path_max_verts: usize,
    /// Small-ring dispatch threshold: inputs at or below this vertex count
    /// use the allocation-free pairwise sweep and stack buffers. Values
    /// above the compile-time [`SMALL_RING_LINES`] constant only widen the
    /// line-collection dispatch; the fixed-array paths clamp at the
    /// constant.
    pub small_ring_lines: usize,
    /// Single-pass repair routing threshold: total ring edges above this
    /// skip the single-pass noding + BuildArea path and fall through to
    /// the boolean pipeline. Measured on the real-world dataset
    /// (2026-08-02): single-pass costs 0.03 ms/poly (<64 edges),
    /// 1.7 ms/poly (64-4096 edges) but 168 ms per poly on the 418 giants
    /// (>=4096 edges, up to 200k) - the R-tree noding of all rings
    /// together outweighs the boolean pipeline there (~36 ms). Below the
    /// threshold single-pass is both faster and simpler.
    pub sp_max_edges: usize,
    /// Fraction of chains-of-length-1 above which a ring is "spiky" and
    /// routes to the edge-level radix sweep instead of the monotone-chain
    /// grid. Spiky rings (star poly: r alternating every 3rd vertex)
    /// shatter into ~n single-edge chains; the grid then co-locates each
    /// long radial edge in many cells and pays O(cells^2/2) pair tests -
    /// measured 63,603 cell-pair tests on star poly 500v vs 5,272 for the
    /// x-sweep over the same edges (12x). Smooth rings (circle: 4 long
    /// chains) keep the chain machinery.
    pub spiky_chain_frac: f64,
    /// Upper chain-count bound for the spiky sweep. Past it the ring is big
    /// enough that the grid goes dense (>64 per cell) and falls through to
    /// the R-tree, whose O(n log n) bulk load beats the sweep's O(n^1.5)
    /// active-set scan (measured: star poly 1000v, 992 chains - grid dense,
    /// R-tree row 247 us vs sweep 494 us). The sweep's window is the middle
    /// band: too many chains for the small-grid floor, too few for the
    /// density fallback.
    pub spiky_max_chains: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            grid_threshold_n: 2000,
            split_bruteforce_max_n: 128,
            split_rebuild_parallel_min: 128,
            fast_path_max_verts: 50_000,
            small_ring_lines: SMALL_RING_LINES,
            sp_max_edges: 4096,
            spiky_chain_frac: 0.8,
            spiky_max_chains: 640,
        }
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for geometry repair operations.
///
/// Controls which polygon repair strategy to use, whether collapsed
/// geometries should be kept, and CRS-aware tolerance settings.
///
/// # Default
///
/// ```rust
/// use geo_repair::MakeValidConfig;
///
/// let config = MakeValidConfig::default();
/// // Equivalent to:
/// //   poly_method: PolyMethod::Auto,
/// //   keep_collapsed: false,
/// //   fill_rule: FillRule::EvenOdd,
/// //   crs: None,
/// //   target_crs: None,
/// //   tuning: Tuning::default(),
/// ```
#[derive(Clone, Debug)]
pub struct MakeValidConfig {
    /// When true, geometries that collapse to empty during repair
    /// (e.g. zero-area after self-intersection resolution) are kept
    /// rather than discarded.
    pub keep_collapsed: bool,

    /// Which polygon repair strategy to use.
    ///
    /// Default: [`PolyMethod::Auto`] (fast path with fallback).
    pub poly_method: PolyMethod,

    /// Fill rule for polygon assembly.
    ///
    /// - `EvenOdd`: standard OGC winding rule (default)
    /// - `NonZero`: holes treated as positive space
    pub fill_rule: FillRule,

    /// CRS of the input geometry.
    /// When set, used for CRS-aware tolerance and metadata preservation.
    pub crs: Option<Crs>,

    /// Target output CRS.
    /// When set, geometries are transformed to this CRS after repair
    /// via PROJ (requires the `proj` feature).
    pub target_crs: Option<Crs>,

    /// Performance dispatch thresholds for the repair pipeline.
    ///
    /// Default: [`Tuning::default()`] (the crate's measured routing
    /// decisions). See [`Tuning`] for the per-threshold rationale.
    pub tuning: Tuning,
}

impl Default for MakeValidConfig {
    fn default() -> Self {
        Self {
            keep_collapsed: false,
            poly_method: PolyMethod::Auto,
            fill_rule: FillRule::EvenOdd,
            crs: None,
            target_crs: None,
            tuning: Tuning::default(),
        }
    }
}

/// Polygon repair strategy selector.
///
/// Controls which algorithm is used for polygon repair.
///
/// - [`Auto`](PolyMethod::Auto): Try Structure first (fast path), fall back
///   to Arrange for complex topology
/// - [`Structure`](PolyMethod::Structure): Planar graph fast path only
/// - [`Arrange`](PolyMethod::Arrange): CDT triangulation only
///
/// See the [`structure`](crate::structure) and [`arrange`](crate::arrange)
/// module docs for algorithm details.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PolyMethod {
    /// Automatic selection: try Structure first, fall back to Arrange.
    Auto,
    /// Structure fast path (planar graph extraction).
    /// 10-100x faster on valid/simple inputs.
    Structure,
    /// CDT-based repair (handles any topology, slower).
    Arrange,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during geometry repair, validation, or I/O.
///
/// This is the crate's unified error type, used by repair, validation,
/// WKB parsing, and all I/O backends.
///
/// # Feature gating
///
/// The `std::error::Error` impl is only available when the `std` feature
/// is enabled. In no_std mode, only `Display` is available.
#[derive(Error, Clone, Debug)]
pub enum MakeValidError {
    /// A coordinate value was NaN at the given index.
    #[error("coordinate value is NaN at index {idx}")]
    CoordinateIsNaN { idx: usize },

    /// A coordinate value was infinite at the given index.
    #[error("coordinate value is infinite at index {idx}")]
    CoordinateIsInfinite { idx: usize },

    /// CDT constraint edge insertion failed — likely a numerical precision
    /// issue with near-collinear or degenerate input.
    #[error("constraint edge insertion failed in CDT — likely numerical precision issue")]
    ConstraintFailure,

    /// The constrained Delaunay triangulation failed.
    #[error("triangulation error: {0}")]
    TriangulationError(String),

    /// An I/O error occurred (file not found, permission denied, etc.).
    /// Only available with the `std` feature.
    #[error("I/O error: {0}")]
    IoError(String),

    /// A parsing error occurred (invalid WKB, malformed GeoJSON, etc.).
    #[error("parse error: {0}")]
    ParseError(String),

    /// The requested format is not supported.
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    /// A CRS-related error occurred.
    #[error("CRS error: {0}")]
    CrsError(String),
}

#[cfg(feature = "arrange")]
impl From<spade::InsertionError> for MakeValidError {
    fn from(e: spade::InsertionError) -> Self {
        match e {
            spade::InsertionError::NAN => Self::CoordinateIsNaN { idx: 0 },
            spade::InsertionError::TooLarge | spade::InsertionError::TooSmall => {
                Self::ConstraintFailure
            }
        }
    }
}

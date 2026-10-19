
use alloc::string::String;
use geo::algorithm::bool_ops::FillRule;
use thiserror::Error;

use crate::crs::Crs;


/// Default geometric epsilon for robust equality / zero checks.
pub(crate) const EPS: f64 = 1e-12;

/// Epsilon for parametric (t/u) intersection comparisons.
pub(crate) const EPS_PARAM: f64 = 1e-14;

pub const SMALL_RING_LINES: usize = 32;

/// Snap scale factor for integer-keyed graph construction.
pub(crate) const SNAP_SCALE: f64 = 1e8;

#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    /// At n=2000 the brute-force 2M pair checks completes in ~5ms for most geometries,
    /// well below the cost of grid construction.
    pub grid_threshold_n: usize,
    /// Measured 2026-08-07: a 500-edge bowtie took 3.29 ms through the bruteforce (250k
    /// exact pair tests) vs ~260 us via the indexed paths.
    pub split_bruteforce_max_n: usize,
    /// Below it the serial loop's lower dispatch overhead wins (the serial rebuild was
    /// ~30ms of the 71ms noding on a 260k-edge shell).
    pub split_rebuild_parallel_min: usize,
    /// Maximum total vertices for the fast-path validity check in
    /// `structure::fix_polygon`.
    pub fast_path_max_verts: usize,
    pub small_ring_lines: usize,
    /// Single-pass repair routing threshold. Measurement (2026-08-02): single-pass
    /// costs 0.03 ms/poly below 64 edges, 1.7 ms/poly for 64 to 4096 edges. It costs
    /// 168 ms/poly on the 418 giants (4096 edges or more, up to 200k). There the R-tree
    /// noding of all rings outweighs the boolean pipeline (36 ms).
    pub sp_max_edges: usize,
    /// Measured: 63,603 on star poly 500v against 5,272 for the x-sweep (12x).
    pub spiky_chain_frac: f64,
    /// Past it the ring is big enough that the grid goes dense (>64 per cell) and falls
    /// through to the R-tree, whose O(n log n) bulk load beats the sweep's O(n^1.5)
    /// active-set scan (measured: star poly 1000v, 992 chains - grid dense, R-tree row
    /// 247 us vs sweep 494 us).
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


/// Controls which polygon repair strategy to use, whether collapsed geometries should
/// be kept, and crs-aware tolerance settings.
///
/// # Default
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
    pub keep_collapsed: bool,

    pub poly_method: PolyMethod,

    pub fill_rule: FillRule,

    /// When set, used for crs-aware tolerance and metadata preservation.
    pub crs: Option<Crs>,

    pub target_crs: Option<Crs>,

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PolyMethod {
    Auto,
    /// Structure fast path (planar graph extraction). 10-100x faster on valid/simple
    /// inputs.
    Structure,
    Arrange,
}


///
/// # Feature gating
/// The `std::error::Error` impl is only available when the `std` feature is enabled.
#[derive(Error, Clone, Debug)]
pub enum MakeValidError {
    /// A coordinate value was NaN at the given index.
    #[error("coordinate value is NaN at index {idx}")]
    CoordinateIsNaN { idx: usize },

    #[error("coordinate value is infinite at index {idx}")]
    CoordinateIsInfinite { idx: usize },

    /// Likely a numerical precision issue with near-collinear or degenerate input.
    #[error("constraint edge insertion failed in CDT — likely numerical precision issue")]
    ConstraintFailure,

    #[error("triangulation error: {0}")]
    TriangulationError(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

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

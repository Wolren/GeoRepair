use geo::algorithm::bool_ops::FillRule;

#[derive(Clone, Debug)]
pub struct MakeValidConfig {
    /// If true, collapsed geometries (e.g. a polygon that shrinks to a line)
    /// are preserved as lower-dimensional types. Default: false (GEOS-style:
    /// collapsed → empty).
    pub keep_collapsed: bool,

    /// Which algorithm to use for polygonal geometries.
    pub poly_method: PolyMethod,

    /// Fill rule for the arrangement-based algorithm.
    pub fill_rule: FillRule,
}

impl Default for MakeValidConfig {
    fn default() -> Self {
        Self {
            keep_collapsed: false,
            poly_method: PolyMethod::Auto,
            fill_rule: FillRule::EvenOdd,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PolyMethod {
    /// Try structure fast-path first; if result is still invalid,
    /// fall back to the full CDT arrangement.
    Auto,
    /// JTS GeometryFixer-style: fix rings, subtract intersecting holes
    /// from shell, convert outside holes to separate shells, union
    /// overlapping shells.
    Structure,
    /// CDT-based even-odd arrangement: build constrained triangulation,
    /// flood-fill label faces, reconstruct boundaries.
    Arrange,
}

use super::prep::PreparedLines;
use crate::core::MakeValidError;
use geo::Coord;
use rustc_hash::FxHashMap;
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Triangulation};

pub(crate) fn build(
    prepared: &PreparedLines,
) -> Result<ConstrainedDelaunayTriangulation<Coord<f64>>, MakeValidError> {
    let mut cdt = ConstrainedDelaunayTriangulation::<Coord<f64>>::new();
    let mut pos_to_handle: FxHashMap<(u64, u64), FixedVertexHandle> = FxHashMap::default();

    for &line in &prepared.lines {
        let start_key = (line.start.x.to_bits(), line.start.y.to_bits());
        let start = if let Some(&h) = pos_to_handle.get(&start_key) {
            h
        } else {
            // spade rejects coordinates whose magnitude exceeds its
            // internal grid (InsertionError::TooLarge). That is a routing
            // decision, not a panic: return the error so the arrange chain
            // falls back to the boolean path. The previous .expect()
            // panicked, and with panic = "abort" release profiles that
            // panic killed the process (measured 2026-08-04: mixed-
            // magnitude corpus seeds aborted the cargo-fuzz smoke).
            let h = cdt
                .insert(line.start)
                .map_err(|_| MakeValidError::ConstraintFailure)?;
            pos_to_handle.insert(start_key, h);
            h
        };

        let end_key = (line.end.x.to_bits(), line.end.y.to_bits());
        let end = if let Some(&h) = pos_to_handle.get(&end_key) {
            h
        } else {
            let h = cdt
                .insert(line.end)
                .map_err(|_| MakeValidError::ConstraintFailure)?;
            pos_to_handle.insert(end_key, h);
            h
        };

        if start != end && cdt.can_add_constraint(start, end) {
            // add_constraint returns bool (false = infeasible); the
            // can_add_constraint gate above already covers that.
            let _ = cdt.add_constraint(start, end);
        }
    }

    Ok(cdt)
}

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
        let start = *pos_to_handle
            .entry(start_key)
            .or_insert_with(|| cdt.insert(line.start).expect("vertex insertion failed"));

        let end_key = (line.end.x.to_bits(), line.end.y.to_bits());
        let end = *pos_to_handle
            .entry(end_key)
            .or_insert_with(|| cdt.insert(line.end).expect("vertex insertion failed"));

        if start != end && cdt.can_add_constraint(start, end) {
            cdt.add_constraint(start, end);
        }
    }

    Ok(cdt)
}

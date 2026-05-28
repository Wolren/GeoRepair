use super::prep::PreparedLines;
use crate::error::MakeValidError;
use geo::Coord;
use spade::{ConstrainedDelaunayTriangulation, Triangulation};

pub(crate) fn build(
    prepared: &PreparedLines,
) -> Result<ConstrainedDelaunayTriangulation<Coord<f64>>, MakeValidError> {
    let mut cdt = ConstrainedDelaunayTriangulation::<Coord<f64>>::new();
    for &line in &prepared.lines {
        let start = cdt.insert(line.start).map_err(MakeValidError::from)?;
        let end = cdt.insert(line.end).map_err(MakeValidError::from)?;
        if start != end && cdt.can_add_constraint(start, end) {
            cdt.add_constraint(start, end);
        }
    }
    Ok(cdt)
}

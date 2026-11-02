//! Coordinate dimension flags shared by the WKT and WKB codecs.

/// Ordinates that follow x and y in every coordinate of a geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dims {
    pub has_z: bool,
    pub has_m: bool,
}

impl Dims {
    pub const XY: Dims = Dims {
        has_z: false,
        has_m: false,
    };
    pub const XYZ: Dims = Dims {
        has_z: true,
        has_m: false,
    };
    pub const XYM: Dims = Dims {
        has_z: false,
        has_m: true,
    };
    pub const XYZM: Dims = Dims {
        has_z: true,
        has_m: true,
    };

    /// Coordinate values per point: 2 for XY, 3 for XYZ/XYM, 4 for XYZM.
    pub fn coord_count(self) -> u8 {
        (self.has_z as u8) + (self.has_m as u8) + 2
    }
}

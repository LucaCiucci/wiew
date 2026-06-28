use crate::{mesh::{Color, Normal, Position}, provided::PcLodBuildError};



/// A point in a [`PcLod`](super::PcLod) point cloud.
///
/// The most important property of a point is its [`position`](PcLodPoint::position):
/// the position is used to build the LOD tree while the other attributes are
/// simply carried along and averaged when building the LOD tree.
#[derive(Debug, Clone, Copy)]
pub struct PcLodPoint {
    pub position: Position,
    pub normal: Normal,
    pub color: Color,
}

impl PcLodPoint {
    /// Create an iterator of [`PcLodPoint`]s from separate streams of positions, normals, and colors.
    ///
    /// This is just a convenience function to avoid having to [`zip`](std::iter::zip) the streams manually for feeding into [`PcLod::from_points`](super::PcLod::from_points).
    ///
    /// The lengths of the streams must match, otherwise a [`PcLodBuildError::LengthMismatch`] is returned.
    ///
    /// See also [`PcLod::from_streams`](super::PcLod::from_streams).
    pub fn from_streams(
        positions: Vec<Position>,
        normals: Vec<Normal>,
        colors: Vec<Color>,
    ) -> Result<impl Iterator<Item = PcLodPoint>, PcLodBuildError> {
        if positions.len() != normals.len() || positions.len() != colors.len() {
            return Err(PcLodBuildError::LengthMismatch {
                positions: positions.len(),
                normals: normals.len(),
                colors: colors.len(),
            });
        }

        let points = positions
            .into_iter()
            .zip(normals)
            .zip(colors)
            .map(|((position, normal), color)| PcLodPoint {
                position,
                normal,
                color,
            });

        Ok(points)
    }
}

// TODO normal and color should not be first class citizens. Instead, they should
// be optional attributes that can be added to a point cloud. This would allow
// for more flexibility in the types of point clouds that can be represented.
// At this level, the attributes can simply be a generic argument. For example
// `PcLodPoint<Attr>` where `Attr` could be:
// ```ignore
// struct Attr {
//     normal: Normal,
//     color: Color,
//     intensity: f32,
//     // etc...
// }
// ```
// and then `Attr` should implement `Add` and `Div` so that we can average the
// attributes when building the LOD tree.
//
// After this is done, we will be able to decompose the core logic from the
// rendering so that we are not forced to use `LitMaterial`/shader and, instead,
// the user can supply their own material/shader for rendering the point cloud.

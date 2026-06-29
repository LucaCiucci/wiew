use super::*;

/// Axis-aligned bounding box (AABB) for a set of points.
///
/// The AABB is defined by its minimum and maximum corners, which are computed
/// from the points.
#[derive(Debug, Clone, Copy)]
pub(super) struct Aabb {
    pub min: Point3<f32>,
    pub max: Point3<f32>,
}

impl Aabb {
    /// Create an AABB from a set of points.
    pub fn from_points(points: &[PcLodPoint]) -> Self {
        let mut min = Point3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Point3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for point in points {
            for axis in 0..3 {
                min[axis] = min[axis].min(point.position[axis]);
                max[axis] = max[axis].max(point.position[axis]);
            }
        }
        Self { min, max }
    }

    /// Compute the center of the AABB.
    pub fn center(self) -> Point3<f32> {
        Point3::from_vec((self.min.to_vec() + self.max.to_vec()) * 0.5)
    }

    /// Compute the radius of the AABB.
    pub fn radius(self) -> f32 {
        (self.max - self.center()).magnitude()
    }

    /// Get the eight corners of the AABB.
    pub fn corners(self) -> [Point3<f32>; 8] {
        [
            Point3::new(self.min.x, self.min.y, self.min.z),
            Point3::new(self.max.x, self.min.y, self.min.z),
            Point3::new(self.min.x, self.max.y, self.min.z),
            Point3::new(self.max.x, self.max.y, self.min.z),
            Point3::new(self.min.x, self.min.y, self.max.z),
            Point3::new(self.max.x, self.min.y, self.max.z),
            Point3::new(self.min.x, self.max.y, self.max.z),
            Point3::new(self.max.x, self.max.y, self.max.z),
        ]
    }
}

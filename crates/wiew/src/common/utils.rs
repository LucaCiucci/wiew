use cgmath::{Matrix4, Point3, Vector4};

pub fn project(
    view_proj: &Matrix4<f32>,
    point: Point3<f32>,
) -> Vector4<f32> {
    // Augment point to homogeneous coordinates
    let point = Vector4::new(point.x, point.y, point.z, 1.0);

    view_proj * point
}

// TODO un-project

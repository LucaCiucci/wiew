use crate::common::utils::project;

use super::*;

pub(super) struct PcLodNode {
    pub bounds: Aabb,
    pub representative: PcLodPoint,
    pub point_count: usize,
    pub node_lods: Option<usize>,
    pub children: [Option<usize>; 8],
    pub leaf_mesh: Option<usize>,
}

impl PcLodNode {
    /// Initialize a new [`PcLodNode`] from a set of points.
    ///
    /// This computes the bounding box, representative point, and node LODs. It
    /// does not split into children or create leaf meshes; that is done by the
    /// [`PcLodBuilder`].
    pub fn init(
        config: &PcLodConfig,
        points: &[PcLodPoint],
        node_lods: &mut Vec<Vec<Mesh>>,
    ) -> Self {
        let bounds = Aabb::from_points(points);
        let representative = representative_point(points);
        let point_count = points.len();
        let node_lods = PcLodNode::node_lods_from_points(&config, points, node_lods);

        PcLodNode {
            bounds,
            representative,
            point_count,
            node_lods,
            children: [None; 8],
            leaf_mesh: None,
        }
    }

    pub fn projected_diameter_px(
        &self,
        view: Matrix4<f32>,
        viewport_height: f32,
        fov_y_deg: f32,
    ) -> f32 {
        let center = self.bounds.center();
        let view_center = view * Vector4::new(center.x, center.y, center.z, 1.0);
        let radius = self.bounds.radius().max(0.0001);
        let depth = (-view_center.z - radius).max(0.0001);
        let focal_px = viewport_height * 0.5 / (fov_y_deg.to_radians() * 0.5).tan();
        radius * 2.0 * focal_px / depth
    }

    pub fn projected_screen_rect(
        &self,
        view_proj: Matrix4<f32>,
        viewport: [f32; 2],
    ) -> Option<ScreenRect> {
        let mut all_left = true;
        let mut all_right = true;
        let mut all_below = true;
        let mut all_above = true;
        let mut all_before_near = true;
        let mut all_after_far = true;
        let mut crosses_eye = false;
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        let mut visible_corner_count = 0usize;

        for corner in self.bounds.corners() {
            let clip = project(&view_proj, corner);

            all_left &= clip.x < -clip.w;
            all_right &= clip.x > clip.w;
            all_below &= clip.y < -clip.w;
            all_above &= clip.y > clip.w;
            all_before_near &= clip.z < 0.0;
            all_after_far &= clip.z > clip.w;

            if clip.w <= 0.0 {
                crosses_eye = true;
                continue;
            }

            let ndc = [clip.x / clip.w, clip.y / clip.w];
            let screen = [
                ((ndc[0] * 0.5 + 0.5) * viewport[0]).clamp(0.0, viewport[0]),
                ((1.0 - (ndc[1] * 0.5 + 0.5)) * viewport[1]).clamp(0.0, viewport[1]),
            ];
            for axis in 0..2 {
                min[axis] = min[axis].min(screen[axis]);
                max[axis] = max[axis].max(screen[axis]);
            }
            visible_corner_count += 1;
        }

        if all_left || all_right || all_below || all_above || all_before_near || all_after_far {
            return None;
        }

        if visible_corner_count == 0 || crosses_eye {
            return Some(ScreenRect::full(viewport));
        }

        Some(ScreenRect { min, max })
    }

    fn node_lods_from_points(
        config: &PcLodConfig,
        points: &[PcLodPoint],
        node_lods: &mut Vec<Vec<Mesh>>,
    ) -> Option<usize> {
        let min_source_points = config.leaf_point_count.saturating_mul(4).max(1);
        if points.len() < min_source_points || config.node_lod_point_count == 0 {
            return None;
        }

        let max_target = config.node_lod_point_count.min(points.len() / 2);
        let lods = point_lods_from_targets(points, &NODE_LOD_TARGETS, max_target);
        if lods.is_empty() {
            return None;
        }

        let index = node_lods.len();
        node_lods.push(lods);
        Some(index)
    }
}

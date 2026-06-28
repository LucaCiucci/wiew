pub(super) const LEAF_LOD_TARGETS: [usize; 5] = [512, 2_048, 8_192, 32_768, 131_072];
pub(super) const NODE_LOD_TARGETS: [usize; 3] = [512, 2_048, 8_192];

/// Configuration for building [`PcLod`](super::PcLod) from a point cloud.
#[derive(Debug, Clone)]
pub struct PcLodConfig {
    /// Stop splitting once a node reaches this depth.
    ///
    /// This is a hard limit on the depth of the tree to mitigate pathological
    /// cases. The actual depth may be lower if the point cloud is small or if
    /// the leaf point count is reached first.
    pub max_depth: u32,

    /// Stop splitting once a node contains at most this many points.
    ///
    /// This is a soft limit on the number of points in a leaf node. The actual
    /// number of points may be lower if the point cloud, or if the max depth is
    /// reached first, it may be higher if the point cloud is not evenly
    /// distributed.
    pub leaf_point_count: usize,

    /// Render a node representative when its projected diameter is below this
    /// threshold.
    ///
    /// Larger nodes descend into children or leaf chunks.
    pub proxy_diameter_px: f32,

    /// Desired point density for selected leaf chunks.
    ///
    /// Higher values draw more points per screen pixel; lower values favor
    /// coarser leaf payloads.
    pub points_per_pixel: f32,

    /// Maximum point count for internal multi-point proxies.
    pub node_lod_point_count: usize,
}

impl Default for PcLodConfig {
    fn default() -> Self {
        Self {
            max_depth: 14,
            leaf_point_count: 32_768,
            proxy_diameter_px: 2.5,
            points_per_pixel: 1.0,
            node_lod_point_count: 8_192,
        }
    }
}

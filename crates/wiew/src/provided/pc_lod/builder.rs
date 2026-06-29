use crate::mesh::Mesh;
use super::*;


pub(super) struct PcLodBuilder {
    pub config: PcLodConfig,
    pub nodes: Vec<PcLodNode>,
    pub leaf_meshes: Vec<Mesh>,
    pub leaf_lods: Vec<Vec<Mesh>>,
    pub node_lods: Vec<Vec<Mesh>>,
}

impl PcLodBuilder {
    /// Build a point cloud LOD tree from a set of points.
    ///
    /// This is the main entry point for building a [`PcLod`] from a set of points.
    /// It creates a new [`PcLodBuilder`], builds the tree, and returns the resulting [`PcLod`].
    pub fn build_node(&mut self, points: Vec<PcLodPoint>, depth: u32) -> usize {
        let (node_id, bounds) = self.init_node(&points);

        if self.should_not_split_further(&points, depth) {
            self.construct_leaf(node_id, points);
            return node_id;
        }

        let children = Self::partition_points_for_children(&bounds, points);
        let any_child_split = self.compute_children(children, node_id, depth);

        // If no children were created, make this node a leaf with no points.
        if !any_child_split {
            self.construct_leaf(node_id, Vec::new());
        }

        node_id
    }

    /// Create a new node and add it to the nodes vector.
    fn init_node(&mut self, points: &[PcLodPoint]) -> (usize, Aabb) {
        // Next node ID is the current length of the nodes vector.
        let node_id = self.nodes.len();

        // Precompute the bounding box and representative point for the node, and create the node.
        let node = PcLodNode::init(&self.config, points, &mut self.node_lods);

        // Store the node
        let bounds = node.bounds;
        self.nodes.push(node);

        (node_id, bounds)
    }

    /// Check if we should stop splitting into children and create a leaf instead.
    ///
    /// This takes the limits into account when deciding whether to split into
    /// children or create a leaf:
    /// - if we reached the maximum depth, we create a leaf
    /// - if we have fewer points than the leaf point count, we create a leaf
    ///
    /// If this is not the case, we will continue to split into children.
    fn should_not_split_further(&self, points: &[PcLodPoint], depth: u32) -> bool {
        depth >= self.config.max_depth || points.len() <= self.config.leaf_point_count
    }

    fn construct_leaf(&mut self, node_id: usize, points: Vec<PcLodPoint>) {
        let leaf_mesh = self.leaf_meshes.len();
        self.leaf_lods.push(leaf_lods_from_points(&points));
        self.leaf_meshes.push(mesh_from_points(points));
        self.nodes[node_id].leaf_mesh = Some(leaf_mesh);
    }

    fn partition_points_for_children(bounds: &Aabb, points: Vec<PcLodPoint>) -> [Vec<PcLodPoint>; 8] {
        let center = bounds.center();
        let mut children: [_; 8] = std::array::from_fn(|_| Vec::new());
        for point in points {
            let child = octant(point.position, center);
            children[child as usize].push(point);
        }
        children
    }

    fn compute_children(&mut self, children: [Vec<PcLodPoint>; 8], node_id: usize, depth: u32) -> bool {
        let mut any_child_split = false;
        for (child_index, child_points) in children.into_iter().enumerate() {
            if child_points.is_empty() {
                continue;
            }
            any_child_split = true;
            let child_id = self.build_node(child_points, depth + 1);
            self.nodes[node_id].children[child_index] = Some(child_id);
        }

        any_child_split
    }
}

fn leaf_lods_from_points(points: &[PcLodPoint]) -> Vec<Mesh> {
    point_lods_from_targets(points, &LEAF_LOD_TARGETS, points.len().saturating_sub(1))
}

fn octant(position: Position, center: Point3<f32>) -> u8 {
    let mut index = 0;
    if position[0] >= center.x {
        index |= 1;
    }
    if position[1] >= center.y {
        index |= 2;
    }
    if position[2] >= center.z {
        index |= 4;
    }
    index
}

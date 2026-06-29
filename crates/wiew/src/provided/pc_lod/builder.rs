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
    pub fn build_node(&mut self, points: Vec<PcLodPoint>, depth: u32) -> usize {
        // Create a new node and add it to the nodes vector.
        let node_id = self.nodes.len();
        let node = PcLodNode::init(&self.config, &points, &mut self.node_lods);
        let bounds = node.bounds;
        self.nodes.push(node);

        if depth >= self.config.max_depth || points.len() <= self.config.leaf_point_count {
            let leaf_mesh = self.leaf_meshes.len();
            self.leaf_lods.push(leaf_lods_from_points(&points));
            self.leaf_meshes.push(mesh_from_points(points));
            self.nodes[node_id].leaf_mesh = Some(leaf_mesh);
            return node_id;
        }

        let center = bounds.center();
        let mut children: [Vec<PcLodPoint>; 8] = std::array::from_fn(|_| Vec::new());
        for point in points {
            let child = octant(point.position, center);
            children[child].push(point);
        }

        let mut any_child_split = false;
        for (child_index, child_points) in children.into_iter().enumerate() {
            if child_points.is_empty() {
                continue;
            }
            any_child_split = true;
            self.nodes[node_id].children[child_index] =
                Some(self.build_node(child_points, depth + 1));
        }

        if !any_child_split {
            self.nodes[node_id].leaf_mesh = Some(self.leaf_meshes.len());
            self.leaf_lods.push(Vec::new());
            self.leaf_meshes.push(Mesh::new(Vec::new()));
        }

        node_id
    }
}

fn leaf_lods_from_points(points: &[PcLodPoint]) -> Vec<Mesh> {
    point_lods_from_targets(points, &LEAF_LOD_TARGETS, points.len().saturating_sub(1))
}

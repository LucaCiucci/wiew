//! Experimental hierarchical point-cloud LOD.
//!
//! This is a CPU-side proof of concept for adaptive point-cloud rendering:
//! internal octree nodes are rendered as representative surfels, while leaves
//! choose one of several prebuilt point-count levels from their projected
//! screen coverage.

use std::{cell::RefCell, collections::BTreeMap};

use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point3, Vector3, Vector4};

use crate::{
    Pass, WCx,
    drawable::Drawable,
    mesh::{Color, Mesh, MeshStreamId, Normal, Position},
    provided::pipelines::{ColoredSplatPipeline, FlatPipeline, LitMaterial},
};

mod cache;
mod config;
mod point;
mod builder;
mod node;
mod aabb;
mod drawing;

pub use cache::{PC_LOD_PAYLOAD_CHUNK_SIZE, PcLodCacheParts, PcLodPayloadChunkRequest};
pub use config::*;
pub use point::*;
use builder::*;
use node::*;
use aabb::*;

/// Hierarchical point-cloud renderer.
pub struct PcLod {
    config: PcLodConfig,
    nodes: Vec<PcLodNode>,
    root: Option<usize>,
    total_points: usize,
    leaf_meshes: Vec<Mesh>,
    leaf_lods: Vec<Vec<Mesh>>,
    node_lods: Vec<Vec<Mesh>>,
    cache_payloads: Vec<cache::PcLodPayloadDesc>,
    requested_cache_payload_chunks: RefCell<BTreeMap<usize, u8>>,
    proxy_mesh: RefCell<Mesh>,
    bounds_mesh: RefCell<Mesh>,
    pipeline: ColoredSplatPipeline,
    bounds_pipeline: FlatPipeline,
    material: LitMaterial,
    last_stats: RefCell<PcLodStats>,
    draw_bounds: bool,
}

impl PcLod {
    pub fn from_points(points: Vec<PcLodPoint>, config: PcLodConfig) -> Self {
        let total_points = points.len();
        let mut builder = PcLodBuilder {
            config: config.clone(),
            nodes: Vec::new(),
            leaf_meshes: Vec::new(),
            leaf_lods: Vec::new(),
            node_lods: Vec::new(),
        };
        let root = if points.is_empty() {
            None
        } else {
            Some(builder.build_node(points, 0))
        };

        Self {
            config,
            nodes: builder.nodes,
            root,
            total_points,
            leaf_meshes: builder.leaf_meshes,
            leaf_lods: builder.leaf_lods,
            node_lods: builder.node_lods,
            cache_payloads: Vec::new(),
            requested_cache_payload_chunks: RefCell::new(BTreeMap::new()),
            proxy_mesh: RefCell::new(dynamic_proxy_mesh()),
            bounds_mesh: RefCell::new(dynamic_bounds_mesh()),
            pipeline: ColoredSplatPipeline::new(),
            bounds_pipeline: FlatPipeline::depthless_line_list(),
            material: LitMaterial::leios_blue().with_front_color([1.0, 1.0, 1.0, 1.0]),
            last_stats: RefCell::new(PcLodStats::default()),
            draw_bounds: false,
        }
    }

    /// A convenience constructor for building a [`PcLod`] from separate point streams.
    ///
    /// This simply combines [`PcLodPoint::from_streams`] and [`PcLod::from_points`].
    pub fn from_streams(
        positions: Vec<Position>,
        normals: Vec<Normal>,
        colors: Vec<Color>,
        config: PcLodConfig,
    ) -> Result<Self, PcLodBuildError> {
        let points = PcLodPoint::from_streams(positions, normals, colors)?.collect();
        Ok(Self::from_points(points, config))
    }

    pub fn set_material(&mut self, material: LitMaterial) {
        self.material = material;
    }

    pub fn material(&self) -> &LitMaterial {
        &self.material
    }

    pub fn material_mut(&mut self) -> &mut LitMaterial {
        &mut self.material
    }

    pub fn config(&self) -> &PcLodConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut PcLodConfig {
        &mut self.config
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn leaf_count(&self) -> usize {
        self.leaf_meshes.len()
    }

    pub fn total_points(&self) -> usize {
        self.total_points
    }

    pub fn stats(&self) -> PcLodStats {
        *self.last_stats.borrow()
    }

    pub fn draw_bounds(&self) -> bool {
        self.draw_bounds
    }

    pub fn set_draw_bounds(&mut self, draw_bounds: bool) {
        self.draw_bounds = draw_bounds;
    }

    fn select(&self, pass: &Pass) -> PcLodSelection {
        let mut selection = PcLodSelection::default();
        selection.stats.total_points = self.total_points;
        selection.stats.total_nodes = self.nodes.len();
        selection.stats.total_leaf_chunks = self.leaf_meshes.len();

        let Some(root) = self.root else {
            return selection;
        };
        let Some(trackball) = pass.trackball else {
            self.collect_leaves(root, &mut selection.leaf_meshes);
            return selection;
        };

        let size = pass.target().size();
        let viewport = [size.width.max(1) as f32, size.height.max(1) as f32];
        let view = trackball.view_matrix();
        let proj = trackball.projection_matrix(viewport[0] / viewport[1]);
        let view_proj = proj * view;
        self.select_node(
            root,
            view,
            view_proj,
            viewport,
            trackball.fov_y_deg,
            &mut selection,
        );
        selection
    }

    fn collect_leaves(&self, node_id: usize, leaves: &mut Vec<SelectedLeafMesh>) {
        let node = &self.nodes[node_id];
        if let Some(leaf_mesh) = node.leaf_mesh {
            leaves.push(SelectedLeafMesh {
                leaf: leaf_mesh,
                lod: None,
            });
            return;
        }

        for child in node.children.iter().flatten() {
            self.collect_leaves(*child, leaves);
        }
    }

    fn select_node(
        &self,
        node_id: usize,
        view: Matrix4<f32>,
        view_proj: Matrix4<f32>,
        viewport: [f32; 2],
        fov_y_deg: f32,
        selection: &mut PcLodSelection,
    ) -> bool {
        selection.stats.visited_nodes += 1;
        let node = &self.nodes[node_id];
        let Some(screen_rect) = node.projected_screen_rect(view_proj, viewport) else {
            selection.stats.culled_nodes += 1;
            return true;
        };

        let diameter_px = node.projected_diameter_px(view, viewport[1], fov_y_deg);
        if diameter_px <= self.config.proxy_diameter_px {
            selection.proxies.push(node.representative);
            selection.bounds.push((node.bounds, PcLodBoundsKind::Proxy));
            selection.stats.selected_proxy_points += 1;
            return true;
        }

        let node_lod = node
            .leaf_mesh
            .is_none()
            .then(|| self.node_lod_for(node, screen_rect))
            .flatten();
        if node.leaf_mesh.is_none()
            && let Some(lod) = node_lod
            && lod.satisfies_desired
        {
            selection.node_lods.push(SelectedNodeLod {
                node_lods: lod.node_lods,
                lod: lod.lod,
                satisfies_desired: lod.satisfies_desired,
            });
            selection
                .bounds
                .push((node.bounds, PcLodBoundsKind::NodeLod));
            return true;
        }

        if let Some(leaf_mesh) = node.leaf_mesh {
            let selected = self.leaf_lod_for(leaf_mesh, screen_rect);
            let is_drawable = self.selected_leaf_is_loaded(leaf_mesh, selected);
            if is_drawable {
                selection.leaf_meshes.push(SelectedLeafMesh {
                    leaf: leaf_mesh,
                    lod: selected,
                });
            }
            if !is_drawable {
                self.request_leaf_mesh_payload(leaf_mesh);
            }
            selection.bounds.push((node.bounds, PcLodBoundsKind::Leaf));
            return is_drawable;
        }

        let mut children_covered = true;
        for child in node.children.iter().flatten() {
            children_covered &=
                self.select_node(*child, view, view_proj, viewport, fov_y_deg, selection);
        }
        if !children_covered && let Some(lod) = node_lod {
            selection.node_lods.push(SelectedNodeLod {
                node_lods: lod.node_lods,
                lod: lod.lod,
                satisfies_desired: lod.satisfies_desired,
            });
            selection
                .bounds
                .push((node.bounds, PcLodBoundsKind::NodeLod));
            return true;
        }
        children_covered
    }

    fn node_lod_for(&self, node: &PcLodNode, screen_rect: ScreenRect) -> Option<SelectedNodeLod> {
        let node_lods = node.node_lods?;
        let lods = self.node_lods.get(node_lods)?;
        let desired_points = (screen_rect.area() * self.config.points_per_pixel)
            .ceil()
            .clamp(1.0, node.point_count as f32) as usize;

        let mut desired = None;
        let mut best_loaded = None;
        let mut first_unloaded = None;

        for (index, mesh) in lods.iter().enumerate() {
            let loaded_count = mesh.vertex_count().unwrap_or_default();
            let target = cache::PayloadTarget::NodeLod {
                set: node_lods,
                lod: index,
            };
            let count =
                loaded_count.max(self.cache_payload_point_count(target).unwrap_or_default());
            if count == 0 || count >= node.point_count || count > self.config.node_lod_point_count {
                continue;
            }

            if loaded_count > 0 {
                best_loaded = Some(index);
            } else if first_unloaded.is_none() {
                first_unloaded = Some(index);
            }

            if count >= desired_points {
                desired = Some(index);
                break;
            }
        }

        if let Some(next) =
            first_unloaded.filter(|next| desired.is_none_or(|desired| *next <= desired))
        {
            self.request_node_lod_payload(node_lods, next);
        }

        best_loaded.map(|lod| SelectedNodeLod {
            node_lods,
            lod,
            satisfies_desired: desired.is_some_and(|desired| lod >= desired),
        })
    }

    fn selected_leaf_is_loaded(&self, leaf: usize, lod: Option<usize>) -> bool {
        match lod {
            Some(lod) => self
                .leaf_lods
                .get(leaf)
                .and_then(|lods| lods.get(lod))
                .and_then(|mesh| mesh.vertex_count())
                .is_some_and(|count| count > 0),
            None => self
                .leaf_meshes
                .get(leaf)
                .and_then(|mesh| mesh.vertex_count())
                .is_some_and(|count| count > 0),
        }
    }

    fn leaf_lod_for(&self, leaf: usize, screen_rect: ScreenRect) -> Option<usize> {
        let loaded_full_count = self
            .leaf_meshes
            .get(leaf)
            .and_then(|mesh| mesh.vertex_count())
            .unwrap_or_default();
        let full_count = loaded_full_count.max(
            self.cache_payload_point_count(cache::PayloadTarget::LeafMesh(leaf))
                .unwrap_or_default(),
        );
        if full_count == 0 {
            return None;
        }

        let projected_area = screen_rect.area();
        let desired_points = (projected_area * self.config.points_per_pixel)
            .ceil()
            .clamp(1.0, full_count as f32) as usize;

        if let Some(lods) = self.leaf_lods.get(leaf) {
            let mut desired = None;
            let mut best_loaded = None;
            let mut first_unloaded = None;

            for (index, mesh) in lods.iter().enumerate() {
                let loaded_count = mesh.vertex_count().unwrap_or_default();
                let target = cache::PayloadTarget::LeafLod {
                    set: leaf,
                    lod: index,
                };
                let count =
                    loaded_count.max(self.cache_payload_point_count(target).unwrap_or_default());
                if count == 0 || count >= full_count {
                    continue;
                }

                if loaded_count > 0 {
                    best_loaded = Some(index);
                } else if first_unloaded.is_none() {
                    first_unloaded = Some(index);
                }

                if count >= desired_points {
                    desired = Some(index);
                    break;
                }
            }

            if let Some(next) =
                first_unloaded.filter(|next| desired.is_none_or(|desired| *next <= desired))
            {
                self.request_leaf_lod_payload(leaf, next);
            }

            if desired.is_some() || best_loaded.is_some() {
                return best_loaded;
            }
        }

        if loaded_full_count == 0 {
            self.request_leaf_mesh_payload(leaf);
        }
        None
    }
}

#[derive(Debug)]
pub enum PcLodBuildError {
    LengthMismatch {
        positions: usize,
        normals: usize,
        colors: usize,
    },
}

impl std::fmt::Display for PcLodBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthMismatch {
                positions,
                normals,
                colors,
            } => write!(
                f,
                "point-cloud stream length mismatch: positions={positions}, normals={normals}, colors={colors}"
            ),
        }
    }
}

impl std::error::Error for PcLodBuildError {}

#[derive(Debug, Clone, Copy, Default)]
pub struct PcLodStats {
    pub total_points: usize,
    pub total_nodes: usize,
    pub total_leaf_chunks: usize,
    pub visited_nodes: usize,
    pub culled_nodes: usize,
    pub selected_proxy_points: usize,
    pub selected_node_lod_chunks: usize,
    pub selected_node_lod_points: usize,
    pub selected_leaf_chunks: usize,
    pub selected_leaf_points: usize,
    pub selected_leaf_lod_chunks: usize,
    pub selected_full_leaf_chunks: usize,
}

impl PcLodStats {
    pub fn drawn_points(self) -> usize {
        self.selected_proxy_points + self.selected_node_lod_points + self.selected_leaf_points
    }
}

#[derive(Default)]
struct PcLodSelection {
    proxies: Vec<PcLodPoint>,
    node_lods: Vec<SelectedNodeLod>,
    leaf_meshes: Vec<SelectedLeafMesh>,
    bounds: Vec<(Aabb, PcLodBoundsKind)>,
    stats: PcLodStats,
}

#[derive(Debug, Clone, Copy)]
struct SelectedNodeLod {
    node_lods: usize,
    lod: usize,
    satisfies_desired: bool,
}

#[derive(Debug, Clone, Copy)]
struct SelectedLeafMesh {
    leaf: usize,
    lod: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
enum PcLodBoundsKind {
    Proxy,
    NodeLod,
    Leaf,
}

impl PcLodSelection {
    fn finish_stats(&mut self, lod: &PcLod) {
        self.stats.selected_proxy_points = self.proxies.len();
        self.stats.selected_node_lod_chunks = self.node_lods.len();
        self.stats.selected_node_lod_points = self
            .node_lods
            .iter()
            .filter_map(|selected| {
                lod.node_lods
                    .get(selected.node_lods)
                    .and_then(|node_lods| node_lods.get(selected.lod))
            })
            .filter_map(|mesh| mesh.positions().len())
            .sum();
        self.stats.selected_leaf_chunks = self.leaf_meshes.len();
        self.stats.selected_leaf_lod_chunks = self
            .leaf_meshes
            .iter()
            .filter(|leaf| leaf.lod.is_some())
            .count();
        self.stats.selected_full_leaf_chunks =
            self.leaf_meshes.len() - self.stats.selected_leaf_lod_chunks;
        self.stats.selected_leaf_points = self
            .leaf_meshes
            .iter()
            .filter_map(|leaf| match leaf.lod {
                Some(lod_index) => lod
                    .leaf_lods
                    .get(leaf.leaf)
                    .and_then(|leaf_lods| leaf_lods.get(lod_index)),
                None => lod.leaf_meshes.get(leaf.leaf),
            })
            .filter_map(|mesh| mesh.positions().len())
            .sum();
    }
}

#[derive(Debug, Clone, Copy)]
struct ScreenRect {
    min: [f32; 2],
    max: [f32; 2],
}

impl ScreenRect {
    fn full(viewport: [f32; 2]) -> Self {
        Self {
            min: [0.0, 0.0],
            max: viewport,
        }
    }

    fn area(self) -> f32 {
        let width = (self.max[0] - self.min[0]).abs().max(1.0);
        let height = (self.max[1] - self.min[1]).abs().max(1.0);
        width * height
    }
}

fn mesh_from_points(points: Vec<PcLodPoint>) -> Mesh {
    let mut positions = Vec::with_capacity(points.len());
    let mut normals = Vec::with_capacity(points.len());
    let mut colors = Vec::with_capacity(points.len());
    for point in points {
        positions.push(point.position);
        normals.push(point.normal);
        colors.push(point.color);
    }

    Mesh::new(positions)
        .with_normals(normals)
        .with_colors(colors)
}

fn sample_points(points: &[PcLodPoint], target_count: usize) -> Vec<PcLodPoint> {
    if target_count >= points.len() {
        return points.to_vec();
    }

    let mut sampled = Vec::with_capacity(target_count);
    for index in 0..target_count {
        sampled.push(points[index * points.len() / target_count]);
    }
    sampled
}

fn dynamic_proxy_mesh() -> Mesh {
    let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
    Mesh::new_with_usage(usage, Vec::new())
        .with_stream_usage(MeshStreamId::NORMAL, usage, Vec::<Normal>::new())
        .with_stream_usage(MeshStreamId::COLOR, usage, Vec::<Color>::new())
}

fn dynamic_bounds_mesh() -> Mesh {
    let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
    Mesh::new_with_usage(usage, Vec::new()).with_stream_usage(
        MeshStreamId::COLOR,
        usage,
        Vec::<Color>::new(),
    )
}

fn point_to_position(point: Point3<f32>) -> Position {
    [point.x, point.y, point.z]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trips_tree_and_payload_counts() {
        let points = (0..128)
            .map(|i| {
                let x = (i % 8) as f32;
                let y = ((i / 8) % 4) as f32;
                let z = (i / 32) as f32;
                PcLodPoint {
                    position: [x, y, z],
                    normal: [0.0, 1.0, 0.0],
                    color: [1.0, 0.5, 0.25, 1.0],
                }
            })
            .collect();
        let lod = PcLod::from_points(
            points,
            PcLodConfig {
                leaf_point_count: 8,
                node_lod_point_count: 32,
                ..Default::default()
            },
        );

        let parts = lod.to_cache_parts().expect("split cache write");
        let loaded = PcLod::from_cache_parts(parts).expect("split cache read");

        assert_eq!(loaded.total_points(), lod.total_points());
        assert_eq!(loaded.node_count(), lod.node_count());
        assert_eq!(loaded.leaf_count(), lod.leaf_count());

        let parts = lod.to_cache_parts().expect("metadata cache write");
        let mut loaded = PcLod::from_cache_metadata_bytes(&parts.metadata).expect("metadata read");
        assert_eq!(loaded.total_points(), lod.total_points());
        assert_eq!(loaded.node_count(), lod.node_count());
        assert_eq!(loaded.leaf_count(), lod.leaf_count());
        loaded
            .apply_cache_payloads(&parts.payloads)
            .expect("payload apply");

        let bytes = lod.to_cache_bytes().expect("bundled cache write");
        let loaded = PcLod::from_cache_bytes(&bytes).expect("bundled cache read");

        assert_eq!(loaded.total_points(), lod.total_points());
        assert_eq!(loaded.node_count(), lod.node_count());
        assert_eq!(loaded.leaf_count(), lod.leaf_count());
    }
}

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

pub use cache::{PC_LOD_PAYLOAD_CHUNK_SIZE, PcLodCacheParts, PcLodPayloadChunkRequest};
pub use config::*;
pub use point::*;

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

impl Drawable for PcLod {
    fn draw(&self, cx: &mut WCx, pass: &mut Pass) {
        let mut selection = self.select(pass);
        selection.finish_stats(self);
        *self.last_stats.borrow_mut() = selection.stats;

        if !selection.proxies.is_empty() {
            let mut positions = Vec::with_capacity(selection.proxies.len());
            let mut normals = Vec::with_capacity(selection.proxies.len());
            let mut colors = Vec::with_capacity(selection.proxies.len());
            for proxy in selection.proxies {
                positions.push(proxy.position);
                normals.push(proxy.normal);
                colors.push(proxy.color);
            }
            let mut proxy_mesh = self.proxy_mesh.borrow_mut();
            proxy_mesh.set_positions(positions);
            proxy_mesh.set_normals(normals);
            proxy_mesh.set_colors(colors);
            self.pipeline
                .draw_mesh_with_material(cx, pass, &proxy_mesh, &self.material);
        }

        for selected in selection.node_lods {
            if let Some(mesh) = self
                .node_lods
                .get(selected.node_lods)
                .and_then(|lods| lods.get(selected.lod))
            {
                self.pipeline
                    .draw_mesh_with_material(cx, pass, mesh, &self.material);
            }
        }

        for leaf_mesh in selection.leaf_meshes {
            let mesh = match leaf_mesh.lod {
                Some(lod) => self
                    .leaf_lods
                    .get(leaf_mesh.leaf)
                    .and_then(|lods| lods.get(lod)),
                None => self.leaf_meshes.get(leaf_mesh.leaf),
            };
            if let Some(mesh) = mesh {
                self.pipeline
                    .draw_mesh_with_material(cx, pass, mesh, &self.material);
            }
        }

        if self.draw_bounds {
            let (positions, colors) = bounds_lines(&selection.bounds);
            let mut bounds_mesh = self.bounds_mesh.borrow_mut();
            bounds_mesh.set_positions(positions);
            bounds_mesh.set_colors(colors);
            self.bounds_pipeline.draw_mesh(cx, pass, &bounds_mesh);
        }
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

struct PcLodBuilder {
    config: PcLodConfig,
    nodes: Vec<PcLodNode>,
    leaf_meshes: Vec<Mesh>,
    leaf_lods: Vec<Vec<Mesh>>,
    node_lods: Vec<Vec<Mesh>>,
}

impl PcLodBuilder {
    fn build_node(&mut self, points: Vec<PcLodPoint>, depth: u32) -> usize {
        let bounds = Aabb::from_points(&points);
        let representative = representative_point(&points);
        let point_count = points.len();
        let node_lods = self.node_lods_from_points(&points);
        let node_id = self.nodes.len();
        self.nodes.push(PcLodNode {
            bounds,
            representative,
            point_count,
            node_lods,
            children: [None; 8],
            leaf_mesh: None,
        });

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

    fn node_lods_from_points(&mut self, points: &[PcLodPoint]) -> Option<usize> {
        let min_source_points = self.config.leaf_point_count.saturating_mul(4).max(1);
        if points.len() < min_source_points || self.config.node_lod_point_count == 0 {
            return None;
        }

        let max_target = self.config.node_lod_point_count.min(points.len() / 2);
        let lods = point_lods_from_targets(points, &NODE_LOD_TARGETS, max_target);
        if lods.is_empty() {
            return None;
        }

        let index = self.node_lods.len();
        self.node_lods.push(lods);
        Some(index)
    }
}

struct PcLodNode {
    bounds: Aabb,
    representative: PcLodPoint,
    point_count: usize,
    node_lods: Option<usize>,
    children: [Option<usize>; 8],
    leaf_mesh: Option<usize>,
}

impl PcLodNode {
    fn projected_diameter_px(
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

    fn projected_screen_rect(
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
            let clip = view_proj * Vector4::new(corner.x, corner.y, corner.z, 1.0);

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

#[derive(Debug, Clone, Copy)]
struct Aabb {
    min: Point3<f32>,
    max: Point3<f32>,
}

impl Aabb {
    fn from_points(points: &[PcLodPoint]) -> Self {
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

    fn center(self) -> Point3<f32> {
        Point3::from_vec((self.min.to_vec() + self.max.to_vec()) * 0.5)
    }

    fn radius(self) -> f32 {
        (self.max - self.center()).magnitude()
    }

    fn corners(self) -> [Point3<f32>; 8] {
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

fn representative_point(points: &[PcLodPoint]) -> PcLodPoint {
    let mut position = Vector3::new(0.0, 0.0, 0.0);
    let mut normal = Vector3::new(0.0, 0.0, 0.0);
    let mut color = [0.0; 4];
    let inv_len = 1.0 / points.len().max(1) as f32;

    for point in points {
        position += Vector3::new(point.position[0], point.position[1], point.position[2]);
        normal += Vector3::new(point.normal[0], point.normal[1], point.normal[2]);
        for (dst, src) in color.iter_mut().zip(point.color) {
            *dst += src;
        }
    }

    position *= inv_len;
    normal = if normal.magnitude2() > 0.0 {
        normal.normalize()
    } else {
        Vector3::unit_y()
    };
    for channel in &mut color {
        *channel *= inv_len;
    }

    PcLodPoint {
        position: position.into(),
        normal: normal.into(),
        color,
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

fn leaf_lods_from_points(points: &[PcLodPoint]) -> Vec<Mesh> {
    point_lods_from_targets(points, &LEAF_LOD_TARGETS, points.len().saturating_sub(1))
}

fn point_lods_from_targets(
    points: &[PcLodPoint],
    targets: &[usize],
    max_target: usize,
) -> Vec<Mesh> {
    let mut lods = Vec::new();
    let mut last_count = 0usize;

    for target in targets {
        let target = (*target).min(max_target);
        if target == 0 || target >= points.len() {
            break;
        }

        let sampled = sample_points(points, target);
        if sampled.len() == last_count || sampled.len() >= points.len() {
            continue;
        }
        last_count = sampled.len();
        lods.push(mesh_from_points(sampled));
    }

    lods
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

fn bounds_lines(bounds: &[(Aabb, PcLodBoundsKind)]) -> (Vec<Position>, Vec<Color>) {
    let mut positions = Vec::with_capacity(bounds.len() * 24);
    let mut colors = Vec::with_capacity(bounds.len() * 24);

    for (bounds, kind) in bounds {
        let color = match kind {
            PcLodBoundsKind::Proxy => [0.15, 0.95, 1.0, 0.95],
            PcLodBoundsKind::NodeLod => [1.0, 0.45, 0.0, 0.85],
            PcLodBoundsKind::Leaf => [1.0, 0.85, 0.10, 0.75],
        };
        let corners = bounds.corners();
        for (a, b) in AABB_EDGES {
            positions.push(point_to_position(corners[a]));
            positions.push(point_to_position(corners[b]));
            colors.push(color);
            colors.push(color);
        }
    }

    (positions, colors)
}

const AABB_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

fn point_to_position(point: Point3<f32>) -> Position {
    [point.x, point.y, point.z]
}

fn octant(position: Position, center: Point3<f32>) -> usize {
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

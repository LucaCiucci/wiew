use std::{
    cell::Cell,
    io::{Cursor, Read},
};

use anyhow::{Context, Result, bail};
use cgmath::Point3;

use crate::{
    mesh::{Color, Mesh, MeshStreamId, Normal, Position},
    provided::pipelines::{ColoredSplatPipeline, FlatPipeline, LitMaterial},
};

use super::{
    Aabb, PcLod, PcLodConfig, PcLodNode, PcLodPoint, PcLodStats, dynamic_bounds_mesh,
    dynamic_proxy_mesh,
};

const MAGIC: &[u8; 8] = b"WIEWLOD\0";
const VERSION: u32 = 2;
const PACK_MAGIC: &[u8; 8] = b"WIEWPKG\0";
const PACK_VERSION: u32 = 1;
const NONE_U64: u64 = u64::MAX;
pub const PC_LOD_PAYLOAD_CHUNK_SIZE: usize = 4 * 1024 * 1024;

pub struct PcLodCacheParts {
    pub metadata: Vec<u8>,
    pub payloads: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcLodPayloadChunkRequest {
    pub chunk_index: usize,
    pub offset: usize,
    pub byte_len: usize,
}

pub(super) struct PcLodPayloadDesc {
    target: PayloadTarget,
    point_count: usize,
    offset: usize,
    byte_len: usize,
    loaded: Cell<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PayloadTarget {
    LeafMesh(usize),
    LeafLod { set: usize, lod: usize },
    NodeLod { set: usize, lod: usize },
}

#[derive(Debug, Clone, Copy)]
enum PayloadFamily {
    LeafMesh,
    LeafLod,
    NodeLod,
}

impl PcLod {
    pub fn to_cache_parts(&self) -> Result<PcLodCacheParts> {
        let mut payloads = PayloadWriter::default();
        let mut metadata = CacheWriter::default();
        metadata.bytes.extend_from_slice(MAGIC);
        metadata.u32(VERSION);

        metadata.config(&self.config);
        metadata.option_usize(self.root);
        metadata.usize(self.total_points);
        metadata.nodes(&self.nodes);
        metadata.meshes(&self.leaf_meshes, &mut payloads)?;
        metadata.mesh_lod_sets(&self.leaf_lods, &mut payloads)?;
        metadata.mesh_lod_sets(&self.node_lods, &mut payloads)?;

        Ok(PcLodCacheParts {
            metadata: metadata.bytes,
            payloads: payloads.bytes,
        })
    }

    pub fn from_cache_parts(parts: PcLodCacheParts) -> Result<Self> {
        Self::from_cache_parts_bytes(&parts.metadata, &parts.payloads)
    }

    pub fn from_cache_parts_bytes(metadata: &[u8], payloads: &[u8]) -> Result<Self> {
        let mut reader = CacheReader::new(metadata);
        reader.magic()?;
        let version = reader.u32()?;
        if version != VERSION {
            bail!("unsupported PcLod metadata version {version}");
        }

        Self::read_split_cache(reader, payloads)
    }

    pub fn from_cache_metadata_bytes(metadata: &[u8]) -> Result<Self> {
        let mut reader = CacheReader::new(metadata);
        reader.magic()?;
        let version = reader.u32()?;
        if version != VERSION {
            bail!("unsupported PcLod metadata version {version}");
        }

        Self::read_metadata_cache(reader)
    }

    pub fn apply_cache_payloads(&mut self, payloads: &[u8]) -> Result<()> {
        let mut ready = Vec::new();
        for (index, descriptor) in self.cache_payloads.iter().enumerate() {
            let end = descriptor
                .offset
                .checked_add(descriptor.byte_len)
                .context("PcLod payload range overflow")?;
            let payload = payloads
                .get(descriptor.offset..end)
                .context("PcLod payload range is outside payload byte buffer")?;
            let mesh = mesh_from_payload_bytes(descriptor.point_count, payload)?;
            ready.push((index, descriptor.target, mesh));
        }

        for (index, target, mesh) in ready {
            self.apply_payload_mesh(target, mesh);
            if let Some(descriptor) = self.cache_payloads.get(index) {
                descriptor.loaded.set(true);
            }
        }
        Ok(())
    }

    pub fn apply_cache_payload_chunk(&mut self, offset: usize, payload: &[u8]) -> Result<()> {
        let end = offset
            .checked_add(payload.len())
            .context("PcLod payload chunk range overflow")?;
        let mut ready = Vec::new();
        for (index, descriptor) in self.cache_payloads.iter().enumerate() {
            if descriptor.loaded.get() {
                continue;
            }
            let descriptor_end = descriptor
                .offset
                .checked_add(descriptor.byte_len)
                .context("PcLod payload descriptor range overflow")?;
            if descriptor.offset >= offset && descriptor_end <= end {
                let start = descriptor.offset - offset;
                let stop = start + descriptor.byte_len;
                let mesh = mesh_from_payload_bytes(descriptor.point_count, &payload[start..stop])?;
                ready.push((index, descriptor.target, mesh));
            }
        }

        for (index, target, mesh) in ready {
            self.apply_payload_mesh(target, mesh);
            if let Some(descriptor) = self.cache_payloads.get(index) {
                descriptor.loaded.set(true);
            }
        }
        Ok(())
    }

    pub fn take_requested_cache_payload_chunks(
        &self,
        chunk_size: usize,
    ) -> Vec<PcLodPayloadChunkRequest> {
        let mut requested = self.requested_cache_payload_chunks.borrow_mut();
        let mut requests = requested
            .iter()
            .map(|(chunk_index, priority)| (*priority, *chunk_index))
            .collect::<Vec<_>>();
        requests.sort_unstable();
        let requests = requests
            .into_iter()
            .map(|(_, chunk_index)| PcLodPayloadChunkRequest {
                chunk_index,
                offset: chunk_index.saturating_mul(chunk_size),
                byte_len: chunk_size,
            })
            .collect::<Vec<_>>();
        requested.clear();
        requests
    }

    pub(super) fn request_leaf_mesh_payload(&self, index: usize) {
        self.request_payload(PayloadTarget::LeafMesh(index));
    }

    pub(super) fn request_leaf_lod_payload(&self, set: usize, lod: usize) {
        self.request_payload(PayloadTarget::LeafLod { set, lod });
    }

    pub(super) fn request_node_lod_payload(&self, set: usize, lod: usize) {
        self.request_payload(PayloadTarget::NodeLod { set, lod });
    }

    pub(super) fn cache_payload_point_count(&self, target: PayloadTarget) -> Option<usize> {
        self.cache_payloads
            .iter()
            .find(|descriptor| descriptor.target == target)
            .map(|descriptor| descriptor.point_count)
    }

    fn request_payload(&self, target: PayloadTarget) {
        if let Some(descriptor) = self
            .cache_payloads
            .iter()
            .find(|descriptor| descriptor.target == target && !descriptor.loaded.get())
        {
            let first = descriptor.offset / PC_LOD_PAYLOAD_CHUNK_SIZE;
            let last = (descriptor.offset + descriptor.byte_len.saturating_sub(1))
                / PC_LOD_PAYLOAD_CHUNK_SIZE;
            let mut requested = self.requested_cache_payload_chunks.borrow_mut();
            let priority = payload_priority(target);
            for chunk in first..=last {
                requested
                    .entry(chunk)
                    .and_modify(|old_priority| *old_priority = (*old_priority).min(priority))
                    .or_insert(priority);
            }
        }
    }

    fn apply_payload_mesh(&mut self, target: PayloadTarget, mesh: Mesh) {
        match target {
            PayloadTarget::LeafMesh(index) => {
                if let Some(target) = self.leaf_meshes.get_mut(index) {
                    replace_mesh_data(target, mesh);
                }
            }
            PayloadTarget::LeafLod { set, lod } => {
                if let Some(target) = self
                    .leaf_lods
                    .get_mut(set)
                    .and_then(|lods| lods.get_mut(lod))
                {
                    replace_mesh_data(target, mesh);
                }
            }
            PayloadTarget::NodeLod { set, lod } => {
                if let Some(target) = self
                    .node_lods
                    .get_mut(set)
                    .and_then(|lods| lods.get_mut(lod))
                {
                    replace_mesh_data(target, mesh);
                }
            }
        }
    }

    pub fn to_cache_bytes(&self) -> Result<Vec<u8>> {
        let parts = self.to_cache_parts()?;
        let mut writer = CacheWriter::default();
        writer.bytes.extend_from_slice(PACK_MAGIC);
        writer.u32(PACK_VERSION);
        writer.usize(parts.metadata.len());
        writer.usize(parts.payloads.len());
        writer.bytes.extend_from_slice(&parts.metadata);
        writer.bytes.extend_from_slice(&parts.payloads);
        Ok(writer.bytes)
    }

    pub fn from_cache_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.starts_with(PACK_MAGIC) {
            let mut reader = CacheReader::new(bytes);
            reader.pack_magic()?;
            let version = reader.u32()?;
            if version != PACK_VERSION {
                bail!("unsupported PcLod cache package version {version}");
            }

            let metadata_len = reader.usize()?;
            let payloads_len = reader.usize()?;
            let metadata = reader.bytes(metadata_len)?;
            let payloads = reader.bytes(payloads_len)?;
            reader.finish()?;
            return Self::from_cache_parts_bytes(metadata, payloads);
        }

        let mut reader = CacheReader::new(bytes);
        reader.magic()?;
        let version = reader.u32()?;
        if version == 1 {
            return Self::read_inline_v1_cache(reader);
        }
        if version != VERSION {
            bail!("unsupported PcLod metadata version {version}");
        }

        Self::read_split_cache(reader, &[])
    }

    fn read_split_cache(mut reader: CacheReader<'_>, payloads: &[u8]) -> Result<Self> {
        let config = reader.config()?;
        let root = reader.option_usize()?;
        let total_points = reader.usize()?;
        let nodes = reader.nodes()?;
        let leaf_meshes = reader.meshes_from_payloads(payloads)?;
        let leaf_lods = reader.mesh_lod_sets_from_payloads(payloads)?;
        let node_lods = reader.mesh_lod_sets_from_payloads(payloads)?;
        reader.finish()?;

        Ok(Self::from_cache_data(
            config,
            nodes,
            root,
            total_points,
            leaf_meshes,
            leaf_lods,
            node_lods,
            Vec::new(),
        ))
    }

    fn read_metadata_cache(mut reader: CacheReader<'_>) -> Result<Self> {
        let config = reader.config()?;
        let root = reader.option_usize()?;
        let total_points = reader.usize()?;
        let nodes = reader.nodes()?;
        let mut cache_payloads = Vec::new();
        let leaf_meshes = reader.meshes_unloaded(&mut cache_payloads, PayloadFamily::LeafMesh)?;
        let leaf_lods =
            reader.mesh_lod_sets_unloaded(&mut cache_payloads, PayloadFamily::LeafLod)?;
        let node_lods =
            reader.mesh_lod_sets_unloaded(&mut cache_payloads, PayloadFamily::NodeLod)?;
        reader.finish()?;

        Ok(Self::from_cache_data(
            config,
            nodes,
            root,
            total_points,
            leaf_meshes,
            leaf_lods,
            node_lods,
            cache_payloads,
        ))
    }

    fn read_inline_v1_cache(mut reader: CacheReader<'_>) -> Result<Self> {
        let config = reader.config()?;
        let root = reader.option_usize()?;
        let total_points = reader.usize()?;
        let nodes = reader.nodes()?;
        let leaf_meshes = reader.meshes_inline()?;
        let leaf_lods = reader.mesh_lod_sets_inline()?;
        let node_lods = reader.mesh_lod_sets_inline()?;
        reader.finish()?;

        Ok(Self::from_cache_data(
            config,
            nodes,
            root,
            total_points,
            leaf_meshes,
            leaf_lods,
            node_lods,
            Vec::new(),
        ))
    }

    fn from_cache_data(
        config: PcLodConfig,
        nodes: Vec<PcLodNode>,
        root: Option<usize>,
        total_points: usize,
        leaf_meshes: Vec<Mesh>,
        leaf_lods: Vec<Vec<Mesh>>,
        node_lods: Vec<Vec<Mesh>>,
        cache_payloads: Vec<PcLodPayloadDesc>,
    ) -> Self {
        Self {
            config,
            nodes,
            root,
            total_points,
            leaf_meshes,
            leaf_lods,
            node_lods,
            cache_payloads,
            requested_cache_payload_chunks: std::cell::RefCell::new(
                std::collections::BTreeMap::new(),
            ),
            proxy_mesh: std::cell::RefCell::new(dynamic_proxy_mesh()),
            bounds_mesh: std::cell::RefCell::new(dynamic_bounds_mesh()),
            pipeline: ColoredSplatPipeline::new(),
            bounds_pipeline: FlatPipeline::depthless_line_list(),
            material: LitMaterial::leios_blue().with_front_color([1.0, 1.0, 1.0, 1.0]),
            last_stats: std::cell::RefCell::new(PcLodStats::default()),
            draw_bounds: false,
        }
    }
}

fn payload_priority(target: PayloadTarget) -> u8 {
    match target {
        PayloadTarget::NodeLod { lod, .. } => lod.min(31) as u8,
        PayloadTarget::LeafLod { lod, .. } => 64 + lod.min(31) as u8,
        PayloadTarget::LeafMesh(_) => 128,
    }
}

#[derive(Default)]
struct CacheWriter {
    bytes: Vec<u8>,
}

impl CacheWriter {
    fn config(&mut self, config: &PcLodConfig) {
        self.u32(config.max_depth);
        self.usize(config.leaf_point_count);
        self.f32(config.proxy_diameter_px);
        self.f32(config.points_per_pixel);
        self.usize(config.node_lod_point_count);
    }

    fn nodes(&mut self, nodes: &[PcLodNode]) {
        self.usize(nodes.len());
        for node in nodes {
            self.point3(node.bounds.min);
            self.point3(node.bounds.max);
            self.point(&node.representative);
            self.usize(node.point_count);
            self.option_usize(node.node_lods);
            for child in node.children {
                self.option_usize(child);
            }
            self.option_usize(node.leaf_mesh);
        }
    }

    fn mesh_lod_sets(&mut self, sets: &[Vec<Mesh>], payloads: &mut PayloadWriter) -> Result<()> {
        self.usize(sets.len());
        for set in sets {
            self.meshes(set, payloads)?;
        }
        Ok(())
    }

    fn meshes(&mut self, meshes: &[Mesh], payloads: &mut PayloadWriter) -> Result<()> {
        self.usize(meshes.len());
        for mesh in meshes {
            self.mesh(mesh, payloads)?;
        }
        Ok(())
    }

    fn mesh(&mut self, mesh: &Mesh, payloads: &mut PayloadWriter) -> Result<()> {
        let positions = mesh
            .positions()
            .to_vec()
            .context("cannot cache a mesh whose positions are loader-backed")?;
        let normals = mesh
            .stream::<Normal>(MeshStreamId::NORMAL)
            .context("cannot cache a PcLod mesh without normals")?
            .to_vec()
            .context("cannot cache a mesh whose normals are loader-backed")?;
        let colors = mesh
            .stream::<Color>(MeshStreamId::COLOR)
            .context("cannot cache a PcLod mesh without colors")?
            .to_vec()
            .context("cannot cache a mesh whose colors are loader-backed")?;

        if positions.len() != normals.len() || positions.len() != colors.len() {
            bail!(
                "cannot cache malformed PcLod mesh: positions={}, normals={}, colors={}",
                positions.len(),
                normals.len(),
                colors.len()
            );
        }

        let point_count = positions.len();
        self.usize(point_count);
        let byte_len = payload_byte_len(point_count)?;
        payloads.align_for(byte_len);
        let offset = payloads.len();
        for ((position, normal), color) in positions.into_iter().zip(normals).zip(colors) {
            payloads.position(position);
            payloads.normal(normal);
            payloads.color(color);
        }
        self.usize(offset);
        self.usize(byte_len);
        Ok(())
    }

    fn point(&mut self, point: &PcLodPoint) {
        self.position(point.position);
        self.normal(point.normal);
        self.color(point.color);
    }

    fn point3(&mut self, point: Point3<f32>) {
        self.f32(point.x);
        self.f32(point.y);
        self.f32(point.z);
    }

    fn position(&mut self, position: Position) {
        for value in position {
            self.f32(value);
        }
    }

    fn normal(&mut self, normal: Normal) {
        for value in normal {
            self.f32(value);
        }
    }

    fn color(&mut self, color: Color) {
        for value in color {
            self.f32(value);
        }
    }

    fn option_usize(&mut self, value: Option<usize>) {
        self.usize(value.unwrap_or(usize::MAX));
    }

    fn usize(&mut self, value: usize) {
        self.u64(value as u64);
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[derive(Default)]
struct PayloadWriter {
    bytes: Vec<u8>,
}

impl PayloadWriter {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn align_for(&mut self, byte_len: usize) {
        if byte_len > PC_LOD_PAYLOAD_CHUNK_SIZE {
            return;
        }

        let used = self.bytes.len() % PC_LOD_PAYLOAD_CHUNK_SIZE;
        if used == 0 || used + byte_len <= PC_LOD_PAYLOAD_CHUNK_SIZE {
            return;
        }

        self.bytes
            .resize(self.bytes.len() + (PC_LOD_PAYLOAD_CHUNK_SIZE - used), 0);
    }

    fn position(&mut self, position: Position) {
        for value in position {
            self.f32(value);
        }
    }

    fn normal(&mut self, normal: Normal) {
        for value in normal {
            self.f32(value);
        }
    }

    fn color(&mut self, color: Color) {
        for value in color {
            self.f32(value);
        }
    }

    fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
}

struct CacheReader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> CacheReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(bytes),
        }
    }

    fn magic(&mut self) -> Result<()> {
        let mut magic = [0u8; 8];
        self.cursor.read_exact(&mut magic)?;
        if &magic != MAGIC {
            bail!("not a PcLod cache file");
        }
        Ok(())
    }

    fn pack_magic(&mut self) -> Result<()> {
        let mut magic = [0u8; 8];
        self.cursor.read_exact(&mut magic)?;
        if &magic != PACK_MAGIC {
            bail!("not a PcLod cache package");
        }
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        if self.cursor.position() != self.cursor.get_ref().len() as u64 {
            bail!("PcLod cache has trailing bytes");
        }
        Ok(())
    }

    fn config(&mut self) -> Result<PcLodConfig> {
        Ok(PcLodConfig {
            max_depth: self.u32()?,
            leaf_point_count: self.usize()?,
            proxy_diameter_px: self.f32()?,
            points_per_pixel: self.f32()?,
            node_lod_point_count: self.usize()?,
        })
    }

    fn nodes(&mut self) -> Result<Vec<PcLodNode>> {
        let len = self.usize()?;
        let mut nodes = Vec::with_capacity(len);
        for _ in 0..len {
            let bounds = Aabb {
                min: self.point3()?,
                max: self.point3()?,
            };
            let representative = self.point()?;
            let point_count = self.usize()?;
            let node_lods = self.option_usize()?;
            let mut children = [None; 8];
            for child in &mut children {
                *child = self.option_usize()?;
            }
            let leaf_mesh = self.option_usize()?;
            nodes.push(PcLodNode {
                bounds,
                representative,
                point_count,
                node_lods,
                children,
                leaf_mesh,
            });
        }
        Ok(nodes)
    }

    fn mesh_lod_sets_from_payloads(&mut self, payloads: &[u8]) -> Result<Vec<Vec<Mesh>>> {
        let len = self.usize()?;
        let mut sets = Vec::with_capacity(len);
        for _ in 0..len {
            sets.push(self.meshes_from_payloads(payloads)?);
        }
        Ok(sets)
    }

    fn meshes_from_payloads(&mut self, payloads: &[u8]) -> Result<Vec<Mesh>> {
        let len = self.usize()?;
        let mut meshes = Vec::with_capacity(len);
        for _ in 0..len {
            meshes.push(self.mesh_from_payloads(payloads)?);
        }
        Ok(meshes)
    }

    fn mesh_lod_sets_unloaded(
        &mut self,
        descriptors: &mut Vec<PcLodPayloadDesc>,
        family: PayloadFamily,
    ) -> Result<Vec<Vec<Mesh>>> {
        let len = self.usize()?;
        let mut sets = Vec::with_capacity(len);
        for set in 0..len {
            sets.push(self.meshes_unloaded_set(descriptors, family, set)?);
        }
        Ok(sets)
    }

    fn meshes_unloaded(
        &mut self,
        descriptors: &mut Vec<PcLodPayloadDesc>,
        family: PayloadFamily,
    ) -> Result<Vec<Mesh>> {
        let len = self.usize()?;
        let mut meshes = Vec::with_capacity(len);
        for index in 0..len {
            meshes.push(self.mesh_unloaded(descriptors, family, index, 0)?);
        }
        Ok(meshes)
    }

    fn meshes_unloaded_set(
        &mut self,
        descriptors: &mut Vec<PcLodPayloadDesc>,
        family: PayloadFamily,
        set: usize,
    ) -> Result<Vec<Mesh>> {
        let len = self.usize()?;
        let mut meshes = Vec::with_capacity(len);
        for lod in 0..len {
            meshes.push(self.mesh_unloaded(descriptors, family, set, lod)?);
        }
        Ok(meshes)
    }

    fn mesh_unloaded(
        &mut self,
        descriptors: &mut Vec<PcLodPayloadDesc>,
        family: PayloadFamily,
        first: usize,
        second: usize,
    ) -> Result<Mesh> {
        let point_count = self.usize()?;
        let offset = self.usize()?;
        let byte_len = self.usize()?;
        validate_payload_len(point_count, byte_len)?;
        let target = match family {
            PayloadFamily::LeafMesh => PayloadTarget::LeafMesh(first),
            PayloadFamily::LeafLod => PayloadTarget::LeafLod {
                set: first,
                lod: second,
            },
            PayloadFamily::NodeLod => PayloadTarget::NodeLod {
                set: first,
                lod: second,
            },
        };
        descriptors.push(PcLodPayloadDesc {
            target,
            point_count,
            offset,
            byte_len,
            loaded: Cell::new(false),
        });
        Ok(empty_mesh())
    }

    fn mesh_from_payloads(&mut self, payloads: &[u8]) -> Result<Mesh> {
        let len = self.usize()?;
        let offset = self.usize()?;
        let byte_len = self.usize()?;
        validate_payload_len(len, byte_len)?;
        let end = offset
            .checked_add(byte_len)
            .context("PcLod payload range overflow")?;
        let payload = payloads
            .get(offset..end)
            .context("PcLod payload range is outside payload byte buffer")?;
        mesh_from_payload_bytes(len, payload)
    }

    fn mesh_lod_sets_inline(&mut self) -> Result<Vec<Vec<Mesh>>> {
        let len = self.usize()?;
        let mut sets = Vec::with_capacity(len);
        for _ in 0..len {
            sets.push(self.meshes_inline()?);
        }
        Ok(sets)
    }

    fn meshes_inline(&mut self) -> Result<Vec<Mesh>> {
        let len = self.usize()?;
        let mut meshes = Vec::with_capacity(len);
        for _ in 0..len {
            meshes.push(self.mesh_inline()?);
        }
        Ok(meshes)
    }

    fn mesh_inline(&mut self) -> Result<Mesh> {
        let len = self.usize()?;
        self.mesh_points(len)
    }

    fn mesh_points(&mut self, len: usize) -> Result<Mesh> {
        let mut positions = Vec::with_capacity(len);
        let mut normals = Vec::with_capacity(len);
        let mut colors = Vec::with_capacity(len);
        for _ in 0..len {
            positions.push(self.position()?);
            normals.push(self.normal()?);
            colors.push(self.color()?);
        }
        Ok(Mesh::new(positions)
            .with_normals(normals)
            .with_colors(colors))
    }

    fn point(&mut self) -> Result<PcLodPoint> {
        Ok(PcLodPoint {
            position: self.position()?,
            normal: self.normal()?,
            color: self.color()?,
        })
    }

    fn point3(&mut self) -> Result<Point3<f32>> {
        Ok(Point3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    fn position(&mut self) -> Result<Position> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn normal(&mut self) -> Result<Normal> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn color(&mut self) -> Result<Color> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }

    fn option_usize(&mut self) -> Result<Option<usize>> {
        match self.u64()? {
            NONE_U64 => Ok(None),
            value => Ok(Some(usize::try_from(value)?)),
        }
    }

    fn usize(&mut self) -> Result<usize> {
        Ok(usize::try_from(self.u64()?)?)
    }

    fn u32(&mut self) -> Result<u32> {
        let mut bytes = [0u8; 4];
        self.cursor.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64> {
        let mut bytes = [0u8; 8];
        self.cursor.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn f32(&mut self) -> Result<f32> {
        let mut bytes = [0u8; 4];
        self.cursor.read_exact(&mut bytes)?;
        Ok(f32::from_le_bytes(bytes))
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let start = usize::try_from(self.cursor.position())?;
        let end = start
            .checked_add(len)
            .context("PcLod cache byte slice range overflow")?;
        let bytes = self
            .cursor
            .get_ref()
            .get(start..end)
            .context("PcLod cache byte slice range is outside buffer")?;
        self.cursor.set_position(end as u64);
        Ok(bytes)
    }
}

fn validate_payload_len(point_count: usize, byte_len: usize) -> Result<()> {
    let expected_len = payload_byte_len(point_count)?;
    if byte_len != expected_len {
        bail!(
            "PcLod payload length mismatch: expected {expected_len} bytes for {point_count} points, got {byte_len}"
        );
    }
    Ok(())
}

fn payload_byte_len(point_count: usize) -> Result<usize> {
    point_count
        .checked_mul(10)
        .and_then(|values| values.checked_mul(std::mem::size_of::<f32>()))
        .context("PcLod payload byte length overflow")
}

fn mesh_from_payload_bytes(point_count: usize, payload: &[u8]) -> Result<Mesh> {
    validate_payload_len(point_count, payload.len())?;
    let mut payload = CacheReader::new(payload);
    let mesh = payload.mesh_points(point_count)?;
    payload.finish()?;
    Ok(mesh)
}

fn empty_mesh() -> Mesh {
    Mesh::new(Vec::new())
        .with_normals(Vec::<Normal>::new())
        .with_colors(Vec::<Color>::new())
}

fn replace_mesh_data(target: &mut Mesh, source: Mesh) {
    if let Some(positions) = source.positions().to_vec() {
        target.set_positions(positions);
    }
    if let Some(normals) = source
        .stream::<Normal>(MeshStreamId::NORMAL)
        .and_then(|normals| normals.to_vec())
    {
        target.set_normals(normals);
    }
    if let Some(colors) = source
        .stream::<Color>(MeshStreamId::COLOR)
        .and_then(|colors| colors.to_vec())
    {
        target.set_colors(colors);
    }
}

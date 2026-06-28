use std::io::{Cursor, Read};

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
const VERSION: u32 = 1;
const NONE_U64: u64 = u64::MAX;

impl PcLod {
    pub fn to_cache_bytes(&self) -> Result<Vec<u8>> {
        let mut writer = CacheWriter::default();
        writer.bytes.extend_from_slice(MAGIC);
        writer.u32(VERSION);

        writer.config(&self.config);
        writer.option_usize(self.root);
        writer.usize(self.total_points);
        writer.nodes(&self.nodes);
        writer.meshes(&self.leaf_meshes)?;
        writer.mesh_lod_sets(&self.leaf_lods)?;
        writer.mesh_lod_sets(&self.node_lods)?;

        Ok(writer.bytes)
    }

    pub fn from_cache_bytes(bytes: &[u8]) -> Result<Self> {
        let mut reader = CacheReader::new(bytes);
        reader.magic()?;
        let version = reader.u32()?;
        if version != VERSION {
            bail!("unsupported PcLod cache version {version}");
        }

        let config = reader.config()?;
        let root = reader.option_usize()?;
        let total_points = reader.usize()?;
        let nodes = reader.nodes()?;
        let leaf_meshes = reader.meshes()?;
        let leaf_lods = reader.mesh_lod_sets()?;
        let node_lods = reader.mesh_lod_sets()?;
        reader.finish()?;

        Ok(Self {
            config,
            nodes,
            root,
            total_points,
            leaf_meshes,
            leaf_lods,
            node_lods,
            proxy_mesh: std::cell::RefCell::new(dynamic_proxy_mesh()),
            bounds_mesh: std::cell::RefCell::new(dynamic_bounds_mesh()),
            pipeline: ColoredSplatPipeline::new(),
            bounds_pipeline: FlatPipeline::depthless_line_list(),
            material: LitMaterial::leios_blue().with_front_color([1.0, 1.0, 1.0, 1.0]),
            last_stats: std::cell::RefCell::new(PcLodStats::default()),
            draw_bounds: false,
        })
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

    fn mesh_lod_sets(&mut self, sets: &[Vec<Mesh>]) -> Result<()> {
        self.usize(sets.len());
        for set in sets {
            self.meshes(set)?;
        }
        Ok(())
    }

    fn meshes(&mut self, meshes: &[Mesh]) -> Result<()> {
        self.usize(meshes.len());
        for mesh in meshes {
            self.mesh(mesh)?;
        }
        Ok(())
    }

    fn mesh(&mut self, mesh: &Mesh) -> Result<()> {
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

        self.usize(positions.len());
        for ((position, normal), color) in positions.into_iter().zip(normals).zip(colors) {
            self.position(position);
            self.normal(normal);
            self.color(color);
        }
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

    fn mesh_lod_sets(&mut self) -> Result<Vec<Vec<Mesh>>> {
        let len = self.usize()?;
        let mut sets = Vec::with_capacity(len);
        for _ in 0..len {
            sets.push(self.meshes()?);
        }
        Ok(sets)
    }

    fn meshes(&mut self) -> Result<Vec<Mesh>> {
        let len = self.usize()?;
        let mut meshes = Vec::with_capacity(len);
        for _ in 0..len {
            meshes.push(self.mesh()?);
        }
        Ok(meshes)
    }

    fn mesh(&mut self) -> Result<Mesh> {
        let len = self.usize()?;
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
}

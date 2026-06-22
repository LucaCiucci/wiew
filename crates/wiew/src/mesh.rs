use std::{any::Any, collections::HashMap};

use crate::{Buf, BufElement, GpuBuffer, WCx, resource::H};

pub type Position = [f32; 3];
pub type Color = [f32; 4];
pub type Normal = [f32; 3];

const POSITION_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![7 => Float32x3];
const COLOR_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![8 => Float32x4];
const NORMAL_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![9 => Float32x3];

pub fn position_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Position>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &POSITION_ATTRIBUTES,
    }
}

pub fn color_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Color>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &COLOR_ATTRIBUTES,
    }
}

pub fn normal_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Normal>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &NORMAL_ATTRIBUTES,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshStreamId(&'static str);

impl MeshStreamId {
    pub const COLOR: Self = Self("wiew.color");
    pub const NORMAL: Self = Self("wiew.normal");

    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// A CPU-side mesh source. Can be stored in the app document —
/// contains no GPU resources.
pub struct Mesh {
    positions: Buf<Position>,
    indices: Option<MeshIndices>,
    streams: HashMap<MeshStreamId, Box<dyn Any + Send + Sync>>,
}

impl Mesh {
    pub fn new(positions: Vec<Position>) -> Self {
        Self {
            positions: Buf::new(wgpu::BufferUsages::VERTEX, positions),
            indices: None,
            streams: HashMap::new(),
        }
    }

    pub fn with_colors(mut self, colors: Vec<Color>) -> Self {
        self.set_colors(colors);
        self
    }

    pub fn with_normals(mut self, normals: Vec<Normal>) -> Self {
        self.set_normals(normals);
        self
    }

    pub fn with_stream<T: BufElement>(mut self, id: MeshStreamId, data: Vec<T>) -> Self {
        self.insert_stream(id, data);
        self
    }

    pub fn with_indices_u16(mut self, indices: Vec<u16>) -> Self {
        self.set_indices_u16(indices);
        self
    }

    pub fn with_indices_u32(mut self, indices: Vec<u32>) -> Self {
        self.set_indices_u32(indices);
        self
    }

    pub fn set_positions(&self, positions: Vec<Position>) {
        self.positions.set_data(positions);
    }

    pub fn positions(&self) -> &Buf<Position> {
        &self.positions
    }

    pub fn bind_positions(&self, cx: &mut WCx) -> H<GpuBuffer> {
        self.positions.get(cx)
    }

    pub fn set_indices_u16(&mut self, indices: Vec<u16>) {
        self.indices = Some(MeshIndices::U16(Buf::new(
            wgpu::BufferUsages::INDEX,
            indices,
        )));
    }

    pub fn set_indices_u32(&mut self, indices: Vec<u32>) {
        self.indices = Some(MeshIndices::U32(Buf::new(
            wgpu::BufferUsages::INDEX,
            indices,
        )));
    }

    pub fn clear_indices(&mut self) {
        self.indices = None;
    }

    pub fn indices(&self) -> Option<&MeshIndices> {
        self.indices.as_ref()
    }

    pub fn bind_indices(&self, cx: &mut WCx) -> Option<BoundMeshIndices> {
        self.indices.as_ref().map(|indices| indices.bind(cx))
    }

    pub fn set_colors(&mut self, colors: Vec<Color>) {
        self.insert_stream(MeshStreamId::COLOR, colors);
    }

    pub fn clear_colors(&mut self) {
        self.remove_stream(MeshStreamId::COLOR);
    }

    pub fn set_normals(&mut self, normals: Vec<Normal>) {
        self.insert_stream(MeshStreamId::NORMAL, normals);
    }

    pub fn clear_normals(&mut self) {
        self.remove_stream(MeshStreamId::NORMAL);
    }

    pub fn insert_stream<T: BufElement>(&mut self, id: MeshStreamId, data: Vec<T>) {
        if let Some(current) = self.stream::<T>(id) {
            current.set_data(data);
        } else {
            self.streams
                .insert(id, Box::new(Buf::new(wgpu::BufferUsages::VERTEX, data)));
        }
    }

    pub fn remove_stream(&mut self, id: MeshStreamId) {
        self.streams.remove(&id);
    }

    pub fn stream<T: BufElement>(&self, id: MeshStreamId) -> Option<&Buf<T>> {
        self.streams.get(&id)?.downcast_ref()
    }

    pub fn bind_stream<T: BufElement>(
        &self,
        cx: &mut WCx,
        id: MeshStreamId,
    ) -> Option<H<GpuBuffer>> {
        match self.stream::<T>(id) {
            Some(stream) => Some(stream.get(cx)),
            None => {
                cx.push_error(anyhow::anyhow!(
                    "mesh stream '{}' is missing or has the wrong element type",
                    id.as_str()
                ));
                None
            }
        }
    }

    pub fn vertex_count(&self) -> Option<usize> {
        self.positions.len()
    }
}

pub enum MeshIndices {
    U16(Buf<u16>),
    U32(Buf<u32>),
}

impl MeshIndices {
    pub fn len(&self) -> u32 {
        match self {
            Self::U16(indices) => indices.len().unwrap_or_default() as u32,
            Self::U32(indices) => indices.len().unwrap_or_default() as u32,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn format(&self) -> wgpu::IndexFormat {
        match self {
            Self::U16(_) => wgpu::IndexFormat::Uint16,
            Self::U32(_) => wgpu::IndexFormat::Uint32,
        }
    }

    pub fn bind(&self, cx: &mut WCx) -> BoundMeshIndices {
        match self {
            Self::U16(indices) => BoundMeshIndices {
                buffer: indices.get(cx),
                len: indices.len().unwrap_or_default() as u32,
                format: wgpu::IndexFormat::Uint16,
            },
            Self::U32(indices) => BoundMeshIndices {
                buffer: indices.get(cx),
                len: indices.len().unwrap_or_default() as u32,
                format: wgpu::IndexFormat::Uint32,
            },
        }
    }
}

pub struct BoundMeshIndices {
    pub buffer: H<GpuBuffer>,
    pub len: u32,
    pub format: wgpu::IndexFormat,
}

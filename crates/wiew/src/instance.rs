//! Instance data for instanced rendering.
//!
//! Provides a single [`Instance`] type that can be used as a per-instance
//! vertex buffer (step mode `Instance`) containing a 4×4 model matrix.

use wgpu::{Buffer, BufferUsages, util::DeviceExt};

use crate::context::WCx;

/// Per-instance GPU data.
///
/// Matches the `InstanceInput` block in the provided flat pipeline shader:
/// ```wgsl
/// @location(0) model_0: vec4<f32>,
/// @location(1) model_1: vec4<f32>,
/// @location(2) model_2: vec4<f32>,
/// @location(3) model_3: vec4<f32>,
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
}

impl Instance {
    /// Identity instance (no transformation).
    pub fn identity() -> Self {
        Self {
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Instance from a matrix.
    pub fn from_matrix(model: cgmath::Matrix4<f32>) -> Self {
        Self {
            model: model.into(),
        }
    }

    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
    ];

    /// WGPU vertex buffer layout for this type (step mode = Instance).
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

struct IdentityInstanceBuffer {
    buffer: Buffer,
}

pub(crate) fn identity_instance_buffer(cx: &mut WCx) -> Buffer {
    {
        let storage = cx.storage.data.lock().unwrap();
        if let Some(buffer) = storage.get::<IdentityInstanceBuffer>() {
            return buffer.buffer.clone();
        }
    }

    let buffer = cx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wiew identity instance buffer"),
            contents: bytemuck::bytes_of(&Instance::identity()),
            usage: BufferUsages::VERTEX,
        });

    let mut storage = cx.storage.data.lock().unwrap();
    storage.insert(IdentityInstanceBuffer {
        buffer: buffer.clone(),
    });
    buffer
}

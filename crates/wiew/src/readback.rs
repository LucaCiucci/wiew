use std::sync::mpsc;

use crate::{context::WCx, render_target::RenderTargetTextures};

#[derive(Debug)]
pub enum ReadTextureError {
    Map(wgpu::BufferAsyncError),
    Poll(wgpu::PollError),
    CallbackDropped,
}

impl std::fmt::Display for ReadTextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Map(err) => write!(f, "failed to map readback buffer: {err}"),
            Self::Poll(err) => write!(f, "failed to poll device for readback: {err}"),
            Self::CallbackDropped => write!(f, "readback map callback was dropped"),
        }
    }
}

impl std::error::Error for ReadTextureError {}

pub struct RgbaImageData {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// Read back the colour attachment of a render target to CPU memory.
///
/// This uses a blocking GPU round-trip (submit → map → read) and is
/// **not supported on Wasm**.  Use [`crate::view::View3d::render_no_readback`]
/// + `egui_wgpu::Renderer::register_native_texture` when you need GPU-only
/// integration (e.g. for egui on the web).
pub fn read_render_target_rgba(
    cx: &WCx,
    target: &RenderTargetTextures,
) -> Result<RgbaImageData, ReadTextureError> {
    let size = target.size();
    let bytes_per_pixel = 4;
    let unpadded_bytes_per_row = size.width * bytes_per_pixel;
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let output_buffer_size = padded_bytes_per_row as u64 * size.height as u64;

    let output_buffer = cx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wiew readback buffer"),
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = cx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wiew readback encoder"),
        });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target.color_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &output_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );

    cx.queue.submit([encoder.finish()]);

    let buffer_slice = output_buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    cx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(ReadTextureError::Poll)?;
    rx.recv()
        .map_err(|_| ReadTextureError::CallbackDropped)?
        .map_err(ReadTextureError::Map)?;

    let mapped = buffer_slice.get_mapped_range();
    let mut bytes = Vec::with_capacity((unpadded_bytes_per_row * size.height) as usize);
    for row in mapped
        .chunks(padded_bytes_per_row as usize)
        .take(size.height as usize)
    {
        bytes.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    output_buffer.unmap();

    Ok(RgbaImageData {
        width: size.width,
        height: size.height,
        bytes,
    })
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

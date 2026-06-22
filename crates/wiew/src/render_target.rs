use crate::{
    context::WCx,
    readback::{ReadTextureError, RgbaImageData, read_render_target_rgba},
    resource::{H, Res},
};

/// Default MSAA sample count (1 = no antialiasing).
pub const DEFAULT_SAMPLES: u32 = 1;

// ---------------------------------------------------------------------------

/// A configurable off-screen render target.
///
/// Supports MSAA via [`with_samples`](Self::with_samples).
/// Call [`get`](Self::get) each frame to obtain the current GPU textures
/// (they are lazily recreated when the configuration changes).
pub struct RenderTarget {
    resources: Res<RenderTargetConfig>,
}

impl RenderTarget {
    /// Create a new render target with the given size and default
    /// settings (no MSAA).
    pub fn new(size: wgpu::Extent3d) -> Self {
        RenderTarget {
            resources: Res::new(RenderTargetConfig::default_with_size(size)),
        }
    }

    /// Set the MSAA sample count (1 = no MSAA, 4 = 4× MSAA, etc.).
    pub fn with_samples(self, samples: u32) -> Self {
        let mut cfg = (*self.resources.source()).clone();
        cfg.samples = samples;
        self.resources.update(cfg);
        self
    }

    /// Set the MSAA sample count (1 = no MSAA, 4 = 4× MSAA, etc.).
    pub fn set_samples(&self, samples: u32) {
        let mut cfg = (*self.resources.source()).clone();
        cfg.samples = samples;
        self.resources.update(cfg);
    }

    pub fn size(&self) -> wgpu::Extent3d {
        self.resources.source().size
    }

    pub fn samples(&self) -> u32 {
        self.resources.source().samples
    }

    pub fn resize(&self, new_size: wgpu::Extent3d) {
        let mut cfg = (*self.resources.source()).clone();
        cfg.size = new_size;
        self.resources.update(cfg);
    }

    pub fn get(&self, cx: &WCx) -> H<RenderTargetTextures> {
        cx.resources
            .get_or_instantiate_with(&self.resources, |source, _old_value| {
                RenderTargetTextures::build(source, &cx.device)
            })
    }
}

#[derive(Clone)]
struct RenderTargetConfig {
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    samples: u32,
}

impl RenderTargetConfig {
    fn default_with_size(size: wgpu::Extent3d) -> Self {
        Self {
            size,
            format: wgpu::TextureFormat::Rgba8Unorm,
            depth_format: DEPTH_FORMAT,
            samples: DEFAULT_SAMPLES,
        }
    }
}

/// GPU textures for a single render target.
///
/// When `sample_count > 1` (MSAA enabled):
/// - `color_texture` / `color_view` is the **resolve** target (single-sample, readable)
/// - `msaa_texture` / `msaa_view` is the multisampled attachment
/// - `depth_texture` is also multisampled
///
/// When `sample_count == 1`:
/// - `color_texture` is the direct render target
/// - `msaa_texture` is `None`
pub struct RenderTargetTextures {
    /// Single-sample color texture — used as the resolve target (MSAA)
    /// or direct render target (no MSAA).  This is what readback copies from.
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    /// Multisampled color texture — only present when `sample_count > 1`.
    pub msaa_texture: Option<wgpu::Texture>,
    pub msaa_view: Option<wgpu::TextureView>,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    size: wgpu::Extent3d,
    sample_count: u32,
    format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
}

impl RenderTargetTextures {
    pub fn target_key(&self) -> TargetKey {
        TargetKey {
            format: self.format,
            depth_format: Some(self.depth_format),
            sample_count: self.sample_count.max(1),
        }
    }

    pub fn size(&self) -> wgpu::Extent3d {
        self.size
    }

    pub fn samples(&self) -> u32 {
        self.sample_count
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn depth_format(&self) -> wgpu::TextureFormat {
        self.depth_format
    }

    pub fn render_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        clear_color: Option<wgpu::Color>,
        draw: impl FnOnce(&mut wgpu::RenderPass),
    ) {
        let mut pass = self.begin_render_pass(encoder, clear_color);
        draw(&mut pass);
    }

    pub fn begin_render_pass<'encoder>(
        &'encoder self,
        encoder: &'encoder mut wgpu::CommandEncoder,
        clear_color: Option<wgpu::Color>,
    ) -> wgpu::RenderPass<'encoder> {
        let (view, resolve_target) = match &self.msaa_view {
            Some(msaa) => (msaa, Some(&self.color_view)),
            None => (&self.color_view, None),
        };

        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("wiew render target pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target,
                ops: wgpu::Operations {
                    load: match clear_color {
                        Some(color) => wgpu::LoadOp::Clear(color),
                        None => wgpu::LoadOp::Load,
                    },
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        })
    }

    fn build(config: &RenderTargetConfig, device: &wgpu::Device) -> Self {
        let sample_count = config.samples.max(1);

        // Single-sample resolve texture (also used as direct RT when no MSAA)
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: config.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            label: None,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&Default::default());

        // Multisampled color texture (only when MSAA is active)
        let (msaa_texture, msaa_view) = if sample_count > 1 {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                size: config.size,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                label: Some("wiew msaa color texture"),
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            (Some(tex), Some(view))
        } else {
            (None, None)
        };

        // Depth texture — must match sample_count so the render pass is valid
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("wiew depth texture"),
            size: config.size,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: config.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            color_texture,
            color_view,
            msaa_texture,
            msaa_view,
            depth_texture,
            depth_view,
            size: config.size,
            sample_count,
            format: config.format,
            depth_format: config.depth_format,
        }
    }

    pub fn readback_rgba(&self, cx: &WCx) -> Result<RgbaImageData, ReadTextureError> {
        read_render_target_rgba(cx, self)
    }
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetKey {
    pub format: wgpu::TextureFormat,
    pub depth_format: Option<wgpu::TextureFormat>,
    pub sample_count: u32,
}

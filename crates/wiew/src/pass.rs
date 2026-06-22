use crate::{
    camera::{Camera, TrackballCamera},
    context::WCx,
    drawable::Drawable,
    render_target::RenderTargetTextures,
};

pub struct Pass<'rp, 'render, 'target> {
    rp: &'rp mut wgpu::RenderPass<'render>,
    target: &'target RenderTargetTextures,
    pub camera: Option<wgpu::BindGroup>,
    pub trackball: Option<TrackballCamera>,
}

impl<'rp, 'render, 'target> Pass<'rp, 'render, 'target> {
    pub fn new(
        rp: &'rp mut wgpu::RenderPass<'render>,
        target: &'target RenderTargetTextures,
    ) -> Self {
        Self {
            rp,
            target,
            camera: None,
            trackball: None,
        }
    }

    pub fn with_camera(mut self, camera: &Camera) -> Self {
        self.camera = Some(camera.bind_group.clone());
        self
    }

    pub fn set_camera(&mut self, camera: &Camera) {
        self.camera = Some(camera.bind_group.clone());
    }

    pub fn with_trackball(mut self, trackball: TrackballCamera) -> Self {
        self.trackball = Some(trackball);
        self
    }

    pub fn set_trackball(&mut self, trackball: TrackballCamera) {
        self.trackball = Some(trackball);
    }

    pub fn target(&self) -> &'target RenderTargetTextures {
        self.target
    }

    pub fn render_pass(&mut self) -> &mut wgpu::RenderPass<'render> {
        self.rp
    }

    pub fn draw<D: Drawable>(&mut self, cx: &mut WCx, drawable: &D) {
        drawable.draw(cx, self);
    }
}

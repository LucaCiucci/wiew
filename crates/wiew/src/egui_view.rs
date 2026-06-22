use crate::{Pass, context::WCx, readback::RgbaImageData, view::View3d};

// ---------------------------------------------------------------------------
// EguiView3d
// ---------------------------------------------------------------------------

/// Wraps a [`View3d`] and manages an egui texture + convenience methods.
pub struct EguiView3d {
    /// The underlying 3D view.
    pub view: View3d,
    texture_name: String,
    /// Cached egui texture handle for the rendered scene (CPU-readback path).
    scene_tex: Option<egui::TextureHandle>,
    /// Cached egui texture ID for the GPU-direct path.
    cached_tex_id: Option<egui::TextureId>,
}

impl EguiView3d {
    /// Create a new egui-integrated 3D view.
    ///
    /// See [`View3d::new`] for parameter details.
    ///
    /// Call [`with_samples`](Self::with_samples) to enable MSAA:
    ///
    /// ```ignore
    /// EguiView3d::new(device, queue, 900, 600, 6.0).with_samples(4)
    /// ```
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
        distance: f32,
    ) -> Self {
        Self {
            view: View3d::new(device, queue, width, height, distance),
            texture_name: "scene".to_owned(),
            scene_tex: None,
            cached_tex_id: None,
        }
    }

    /// Set the egui texture name used by this view.
    ///
    /// Use distinct names when multiple views live in the same egui context.
    pub fn with_texture_name(mut self, name: impl Into<String>) -> Self {
        self.texture_name = name.into();
        self
    }

    /// Enable MSAA with the given sample count (pass `4` for 4× MSAA).
    #[inline]
    pub fn with_samples(self, samples: u32) -> Self {
        Self {
            view: self.view.with_samples(samples),
            texture_name: self.texture_name,
            scene_tex: self.scene_tex,
            cached_tex_id: None, // will be re-registered on next render
        }
    }

    /// Enable MSAA with the given sample count (pass `4` for 4× MSAA).
    #[inline]
    pub fn set_samples(&mut self, samples: u32) {
        self.view.set_samples(samples);
    }

    /// Current MSAA sample count.
    #[inline]
    pub fn samples(&self) -> u32 {
        self.view.samples()
    }

    // ── Input ──────────────────────────────────────────────────────────

    /// Poll egui input and forward it to the trackball camera.
    ///
    /// Prefer [`central_panel_interactive`](Self::central_panel_interactive)
    /// when rendering inside egui: it only reacts to input over the viewport.
    pub fn handle_input(&mut self, ctx: &egui::Context) {
        let (pd, sd, cursor, scroll_y) = ctx.input(|i| {
            (
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Secondary),
                i.pointer.hover_pos().map(|p| (p.x, p.y)),
                i.smooth_scroll_delta.y,
            )
        });
        self.view.handle_egui_interaction(pd, sd, cursor, scroll_y);
    }

    // ── Render ─────────────────────────────────────────────────────────

    /// Update the camera and render with a higher-level drawable pass.
    pub fn render_drawables<'cx>(
        &'cx mut self,
        draw: impl FnOnce(&mut WCx, &mut Pass),
    ) -> RgbaImageData {
        self.view.update_camera();
        self.view.render_drawables(draw)
    }

    /// Update the camera and render directly to an egui texture via egui-wgpu.
    ///
    /// This is the simplest way to use `EguiView3d` with egui when using the
    /// wgpu backend — no CPU readback, no manual texture registration.
    ///
    /// `render_state` must be obtained from
    /// `eframe::CreationContext::wgpu_render_state`.
    pub fn render_to_egui(
        &mut self,
        render_state: &egui_wgpu::RenderState,
        draw: impl FnOnce(&mut WCx, &mut Pass),
    ) -> egui::TextureId {
        self.view.update_camera();
        let cached_id = &mut self.cached_tex_id;
        self.view.render_drawables_no_readback(draw, |view| {
            let mut renderer = render_state.renderer.write();
            match *cached_id {
                Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    view,
                    wgpu::FilterMode::Linear,
                    id,
                ),
                None => {
                    let id = renderer.register_native_texture(
                        &render_state.device,
                        view,
                        wgpu::FilterMode::Linear,
                    );
                    *cached_id = Some(id);
                }
            }
        });
        self.cached_tex_id.unwrap()
    }

    /// Update the camera and render without CPU readback.
    ///
    /// After the command buffer is submitted, `use_view` is called with
    /// the colour attachment's [`wgpu::TextureView`] so the caller can
    /// register it directly with a GPU renderer
    /// (e.g. `egui_wgpu::Renderer::register_native_texture`).
    pub fn render_drawables_texture(
        &mut self,
        draw: impl FnOnce(&mut WCx, &mut Pass),
        use_view: impl FnOnce(&wgpu::TextureView),
    ) {
        self.view.update_camera();
        self.view.render_drawables_no_readback(draw, use_view);
    }

    // ── Texture ────────────────────────────────────────────────────────

    /// Update (or create) the egui texture from raw RGBA pixels and return
    /// its texture ID for use with [`egui::widgets::Image`].
    ///
    /// The pixel data must have `width × height × 4` bytes and be in
    /// non-premultiplied RGBA order.
    pub fn update_texture(&mut self, ctx: &egui::Context, rgba: &[u8]) -> egui::TextureId {
        let w = self.view.img_w as usize;
        let h = self.view.img_h as usize;
        let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], rgba);

        let tex = self.scene_tex.get_or_insert_with(|| {
            ctx.load_texture(
                self.texture_name.clone(),
                color_image.clone(),
                egui::TextureOptions::LINEAR,
            )
        });
        tex.set(color_image, egui::TextureOptions::LINEAR);
        tex.id()
    }

    // ── Panels ─────────────────────────────────────────────────────────

    /// Show a [`CentralPanel`](egui::CentralPanel) with an [`Image`](egui::widgets::Image)
    /// filling the available space, and return the size the user chose.
    ///
    /// This is intentionally a static method so you can call it even when
    /// `&mut self` is already borrowed for rendering.
    pub fn central_panel(ui: &mut egui::Ui, tex_id: egui::TextureId) -> egui::Vec2 {
        egui::CentralPanel::default()
            .show_inside(ui, |ui| {
                let avail = ui.available_size();
                ui.add_sized(
                    avail,
                    egui::widgets::Image::new(egui::load::SizedTexture::new(tex_id, avail)),
                );
                avail
            })
            .inner
    }

    /// Show a viewport image, handle pointer input over it, and return its size.
    pub fn central_panel_interactive(
        &mut self,
        ui: &mut egui::Ui,
        tex_id: egui::TextureId,
    ) -> egui::Vec2 {
        egui::CentralPanel::default()
            .show_inside(ui, |ui| self.viewport_interactive(ui, tex_id))
            .inner
    }

    /// Show an interactive viewport in the available space of `ui`.
    pub fn viewport_interactive(
        &mut self,
        ui: &mut egui::Ui,
        tex_id: egui::TextureId,
    ) -> egui::Vec2 {
        let avail = ui.available_size();
        let response = ui.add_sized(
            avail,
            egui::widgets::Image::new(egui::load::SizedTexture::new(tex_id, avail))
                .sense(egui::Sense::drag()),
        );

        let scroll_y = if response.hovered() {
            ui.input(|i| i.smooth_scroll_delta.y)
        } else {
            0.0
        };
        let (primary_down, secondary_down) = ui.input(|i| {
            (
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Secondary),
            )
        });
        let primary_active = primary_down
            && (response.hovered() || response.dragged_by(egui::PointerButton::Primary));
        let secondary_active = secondary_down
            && (response.hovered() || response.dragged_by(egui::PointerButton::Secondary));
        let pointer_pos = if primary_active || secondary_active || response.hovered() {
            ui.input(|i| i.pointer.latest_pos())
                .map(|p| (p.x - response.rect.left(), p.y - response.rect.top()))
        } else {
            None
        };

        self.view
            .handle_egui_interaction(primary_active, secondary_active, pointer_pos, scroll_y);

        avail
    }

    // ── Resize ─────────────────────────────────────────────────────────

    /// Resize the underlying render target to match the egui panel size.
    ///
    /// Designed to be called with the return value of [`central_panel`](Self::central_panel).
    /// Clamps to at least 100×100 so degenerate sizes are harmless.
    pub fn resize_from(&mut self, size: egui::Vec2) {
        let w = (size.x.round() as u32).max(100);
        let h = (size.y.round() as u32).max(100);
        self.view.resize(w, h);
    }
}

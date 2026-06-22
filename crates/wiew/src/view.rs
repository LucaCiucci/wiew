use crate::{
    Pass,
    camera::{Camera, CameraLayout, TrackballCamera},
    context::WCx,
    readback::RgbaImageData,
    render_target::RenderTarget,
};

/// Bundles everything needed for an interactive 3D viewport.
pub struct View3d {
    /// Low-level wgpu context (device + queue + resource manager).
    pub cx: WCx,
    /// GPU camera object (uniform buffer + bind group).
    pub camera: Camera,
    /// CPU-side trackball camera for interaction.
    pub trackball: TrackballCamera,
    /// Off-screen render target.
    pub target: RenderTarget,
    /// Bind group layout for the camera — needed when creating pipelines
    /// and provided components.
    pub camera_layout: CameraLayout,
    /// Colour attachment format of the render target.
    pub format: wgpu::TextureFormat,
    /// Depth attachment format.
    pub depth_format: wgpu::TextureFormat,
    /// Current viewport width.
    pub img_w: u32,
    /// Current viewport height.
    pub img_h: u32,
    /// Previous cursor position (used for drag deltas).
    pub prev_cursor: Option<(f32, f32)>,
}

impl View3d {
    /// Create a new interactive view.
    ///
    /// `distance` is the initial camera distance from the origin (passed
    /// to [`TrackballCamera`]; use `6.0` as a reasonable default).
    /// Create a new interactive view.
    ///
    /// `distance` is the initial camera distance from the origin (passed
    /// to [`TrackballCamera`]; use `6.0` as a reasonable default).
    ///
    /// By default MSAA is disabled (`samples = 1`).  Call
    /// [`with_samples`](Self::with_samples) to enable antialiasing:
    ///
    /// ```ignore
    /// View3d::new(device, queue, 900, 600, 6.0).with_samples(4)
    /// ```
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
        distance: f32,
    ) -> Self {
        let mut cx = WCx::new(device, queue);

        let target = RenderTarget::new(wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        });
        let textures = target.get(&cx);
        let format = textures.format();
        let depth_format = textures.depth_format();
        drop(textures);

        let camera_layout = Camera::bind_group_layout();
        let mut camera = Camera::new(&mut cx, &camera_layout);
        let trackball = TrackballCamera {
            distance,
            ..Default::default()
        };
        camera.update_with_viewport(
            &cx.queue,
            trackball.view_matrix(),
            trackball.projection_matrix(width as f32 / height as f32),
            trackball.eye(),
            [width as f32, height as f32],
        );

        Self {
            cx,
            camera,
            trackball,
            target,
            camera_layout,
            format,
            depth_format,
            img_w: width,
            img_h: height,
            prev_cursor: None,
        }
    }

    /// Enable MSAA with the given sample count (pass `4` for 4× MSAA).
    ///
    /// Must be called **before** the first render (or after `resize`)
    /// to take effect.
    pub fn with_samples(self, samples: u32) -> Self {
        self.target.set_samples(samples.max(1));
        self
    }

    /// Enable MSAA with the given sample count (pass `4` for 4× MSAA).
    ///
    /// Must be called **before** the first render (or after `resize`)
    /// to take effect.
    pub fn set_samples(&mut self, samples: u32) {
        self.target.set_samples(samples.max(1));
    }

    /// Current MSAA sample count.
    pub fn samples(&self) -> u32 {
        self.target.samples()
    }

    /// Resize the render target (takes effect on the next [`render`] call).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == self.img_w && height == self.img_h {
            return;
        }
        self.target.resize(wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        });
        self.img_w = width;
        self.img_h = height;
    }

    /// Update the GPU camera from the current trackball state.
    ///
    /// Called automatically inside [`render`]; you only need this if you
    /// read [`Self::camera`] outside of rendering.
    pub fn update_camera(&mut self) {
        let aspect = self.img_w as f32 / self.img_h as f32;
        self.camera.update_with_viewport(
            &self.cx.queue,
            self.trackball.view_matrix(),
            self.trackball.projection_matrix(aspect),
            self.trackball.eye(),
            [self.img_w as f32, self.img_h as f32],
        );
    }

    /// Handle mouse / scroll events for the trackball camera.
    ///
    /// Call this at the start of every frame, before [`render`].
    ///
    /// * `primary_down` — whether the primary mouse button is held.
    /// * `secondary_down` — whether the secondary button is held.
    /// * `cursor` — current mouse position, if any.
    /// * `scroll_delta_y` — vertical scroll delta (positive = scroll down).
    pub fn handle_egui_interaction(
        &mut self,
        primary_down: bool,
        secondary_down: bool,
        cursor: Option<(f32, f32)>,
        scroll_delta_y: f32,
    ) {
        let prev = self.prev_cursor;
        self.prev_cursor = cursor;

        // Left-drag rotate (not while right-dragging)
        if primary_down && !secondary_down {
            if let (Some(to), Some(from)) = (cursor, prev) {
                if to != from {
                    self.trackball
                        .mouse_rotation(from, to, self.img_w as f32, self.img_h as f32);
                }
            }
        }

        // Scroll zoom
        if scroll_delta_y != 0.0 {
            // Negate: positive delta = scroll down → zoom in.
            // Factor 0.01 ≈ LC's angleDelta/90*0.25 per pixel.
            self.trackball.mouse_zoom(-scroll_delta_y * 0.01);
        }

        // Right-drag = pan
        if secondary_down {
            if let (Some(to), Some(from)) = (cursor, prev) {
                if to != from {
                    self.trackball
                        .mouse_pan(from, to, self.img_w as f32, self.img_h as f32);
                }
            }
        }
    }

    /// Render the scene and return RGBA pixel data.
    ///
    /// The `draw` closure receives the active render pass, the [`WCx`],
    /// and the [`Camera`] bind group.  Call the closure's methods to draw
    /// background, grid, gizmo, custom meshes, etc.
    ///
    /// After the closure returns the pass is ended, the command buffer is
    /// submitted, and the colour pixels are read back to the CPU.
    pub fn render(
        &mut self,
        draw: impl FnOnce(
            &mut wgpu::RenderPass,
            &mut WCx,
            &Camera,
            &crate::render_target::RenderTargetTextures,
        ),
    ) -> RgbaImageData {
        let textures = self.target.get(&self.cx);
        let mut encoder = self
            .cx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wiew view encoder"),
            });

        textures.render_pass(
            &mut encoder,
            Some(wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            |pass| draw(pass, &mut self.cx, &self.camera, &textures),
        );

        self.cx.queue.submit([encoder.finish()]);
        textures
            .readback_rgba(&self.cx)
            .expect("wiew readback failed")
    }

    /// Render the scene without CPU readback.
    ///
    /// The `draw` closure works the same as in [`render`](Self::render).
    /// After the command buffer is submitted, `use_view` is called with
    /// the colour attachment's texture view so the caller can bind it
    /// directly (e.g. via `egui_wgpu::Renderer::register_native_texture`).
    ///
    /// This is the method to use for GPU-only integration (egui on the web)
    /// or any time you don't need pixels on the CPU.
    pub fn render_no_readback(
        &mut self,
        draw: impl FnOnce(
            &mut wgpu::RenderPass,
            &mut WCx,
            &Camera,
            &crate::render_target::RenderTargetTextures,
        ),
        use_view: impl FnOnce(&wgpu::TextureView),
    ) {
        let textures = self.target.get(&self.cx);
        let mut encoder = self
            .cx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wiew view encoder"),
            });

        textures.render_pass(
            &mut encoder,
            Some(wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            |pass| draw(pass, &mut self.cx, &self.camera, &textures),
        );

        self.cx.queue.submit([encoder.finish()]);
        use_view(&textures.color_view);
    }

    /// Render with a higher-level [`provided::Pass`](crate::provided::Pass).
    ///
    /// This keeps draw sites compact for reusable components while preserving
    /// explicit render order.
    pub fn render_drawables<'cx>(
        &'cx mut self,
        draw: impl FnOnce(&mut WCx, &mut Pass),
    ) -> RgbaImageData {
        let trackball = self.trackball;
        self.render(|rp, cx, camera, textures| {
            let mut pass = Pass::new(rp, &textures)
                .with_camera(camera)
                .with_trackball(trackball);
            draw(cx, &mut pass);
        })
    }

    /// Render with a higher-level [`provided::Pass`](crate::provided::Pass)
    /// without CPU readback.
    ///
    /// After the command buffer is submitted, `use_view` is called with
    /// the colour attachment's texture view.
    pub fn render_drawables_no_readback(
        &mut self,
        draw: impl FnOnce(&mut WCx, &mut Pass),
        use_view: impl FnOnce(&wgpu::TextureView),
    ) {
        let trackball = self.trackball;
        self.render_no_readback(
            |rp, cx, camera, textures| {
                let mut pass = Pass::new(rp, &textures)
                    .with_camera(camera)
                    .with_trackball(trackball);
                draw(cx, &mut pass);
            },
            use_view,
        );
    }
}

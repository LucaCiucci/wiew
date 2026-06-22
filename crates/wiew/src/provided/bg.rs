use crate::{
    Pass,
    context::WCx,
    drawable::Drawable,
    mesh::{Color, Mesh, Position},
    provided::pipelines::BgPipeline,
};

/// Per-corner colours for a 4-vertex gradient quad.
///
/// The quad is drawn as 4 indexed triangles with a centre vertex for a
/// smooth gradient across the full viewport.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuadBackgroundConfig {
    pub top_left: [f32; 4],
    pub top_right: [f32; 4],
    pub bottom_left: [f32; 4],
    pub bottom_right: [f32; 4],
}

impl QuadBackgroundConfig {
    /// Rich rainbow-ish gradient — matches wiew2's `DEFAULT_BG_RAINBOW`.
    pub const RAINBOW: Self = QuadBackgroundConfig {
        top_left: [14.0 / 255.0, 41.0 / 255.0, 29.0 / 255.0, 1.0],
        top_right: [54.0 / 255.0, 22.0 / 255.0, 22.0 / 255.0, 1.0],
        bottom_left: [20.0 / 255.0, 17.0 / 255.0, 51.0 / 255.0, 1.0],
        bottom_right: [42.0 / 255.0, 20.0 / 255.0, 55.0 / 255.0, 1.0],
    };

    pub const fn transparent() -> Self {
        Self {
            top_left: [0.0; 4],
            top_right: [0.0; 4],
            bottom_left: [0.0; 4],
            bottom_right: [0.0; 4],
        }
    }

    /// Build the 5 vertices (4 corners + centre) and 12 indices.
    fn build(&self) -> (Vec<Position>, Vec<Color>, Vec<u16>) {
        let avg = [
            (self.top_left[0] + self.top_right[0] + self.bottom_left[0] + self.bottom_right[0])
                / 4.0,
            (self.top_left[1] + self.top_right[1] + self.bottom_left[1] + self.bottom_right[1])
                / 4.0,
            (self.top_left[2] + self.top_right[2] + self.bottom_left[2] + self.bottom_right[2])
                / 4.0,
            (self.top_left[3] + self.top_right[3] + self.bottom_left[3] + self.bottom_right[3])
                / 4.0,
        ];

        let positions = vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let colors = vec![
            self.bottom_left,
            self.bottom_right,
            self.top_right,
            self.top_left,
            avg,
        ];

        #[rustfmt::skip]
        let indices = vec![
            0u16, 1, 4,
            1, 2, 4,
            2, 3, 4,
            3, 0, 4,
        ];

        (positions, colors, indices)
    }
}

impl Default for QuadBackgroundConfig {
    fn default() -> Self {
        Self::RAINBOW
    }
}

/// Reusable full-screen gradient background.
pub struct QuadBackground {
    mesh: Mesh,
    pipeline: BgPipeline,
}

impl QuadBackground {
    /// Create a background quad with the given pipeline description and
    /// color configuration.
    ///
    /// No wgpu parameters needed — GPU resources are created lazily
    /// through the resource manager when [`draw`](Self::draw) is called.
    pub fn new(config: QuadBackgroundConfig) -> Self {
        let (positions, colors, inds) = config.build();
        Self {
            mesh: Mesh::new(positions)
                .with_colors(colors)
                .with_indices_u16(inds),
            pipeline: BgPipeline,
        }
    }

    /// Update the colour configuration (triggers re-instantiation of
    /// vertex and index buffers).
    pub fn set_config(&mut self, config: QuadBackgroundConfig) {
        let (positions, colors, inds) = config.build();
        self.mesh.set_positions(positions);
        self.mesh.set_colors(colors);
        self.mesh.set_indices_u16(inds);
    }

    pub fn with_pipeline(mut self, pipeline: BgPipeline) -> Self {
        self.pipeline = pipeline;
        self
    }

    pub fn set_pipeline(&mut self, pipeline: BgPipeline) {
        self.pipeline = pipeline;
    }
}

impl Drawable for QuadBackground {
    fn draw(&self, cx: &mut WCx, pass: &mut Pass) {
        self.pipeline.draw_mesh(cx, pass, &self.mesh);
    }
}

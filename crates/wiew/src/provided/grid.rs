//! Ground-plane grid helper.
//!
//! Draws a configurable grid on the XZ plane (y=0) with major and minor
//! subdivisions.  The centre axes are highlighted in red (X) and blue (Z).
//!
//! Mirrors the logic from wiew2's `Grid` component.

use crate::{
    Pass,
    context::WCx,
    mesh::{Color, Mesh, Position},
    provided::{Drawable, pipelines::FlatPipeline},
};

/// A reusable ground-plane grid.
///
/// No wgpu resources are stored directly — the pipeline is resolved
/// through the resource manager when [`draw`](Self::draw) is called.
///
/// ```ignore
/// let grid = Grid::new(10);
///
/// // Every frame, after the background:
/// grid.draw(&mut pass, &cx, &camera);
/// ```
pub struct Grid {
    mesh: Mesh,
    pipeline: FlatPipeline,
    n: u32,
}

impl Grid {
    /// Create a grid with `n` subdivisions in each direction.
    ///
    /// The pipeline is described by `desc` and resolved per-[`WCx`]
    /// through the resource manager — no wgpu objects are stored directly.
    pub fn new(n: u32) -> Self {
        let (positions, colors) = grid_vertices(n);
        Self {
            mesh: Mesh::new(positions).with_colors(colors),
            pipeline: FlatPipeline::line_list(),
            n,
        }
    }

    pub fn with_pipeline(mut self, pipeline: FlatPipeline) -> Self {
        self.pipeline = pipeline;
        self
    }

    pub fn set_pipeline(&mut self, pipeline: FlatPipeline) {
        self.pipeline = pipeline;
    }

    /// Number of subdivisions in each direction.
    pub fn extent(&self) -> u32 {
        self.n
    }
}

impl Drawable for Grid {
    fn draw(&self, cx: &mut WCx, pass: &mut Pass) {
        self.pipeline.draw_mesh(cx, pass, &self.mesh);
    }
}

// ---- vertex generation ------------------------------------------------

fn grid_vertices(n: u32) -> (Vec<Position>, Vec<Color>) {
    let mut positions = Vec::new();
    let mut colors = Vec::new();

    let mut v = |position, color| {
        positions.push(position);
        colors.push(color);
    };

    const N_DIV: isize = 5;
    const A: f32 = 0.25; // alpha for minor lines

    for i in -(n as i32)..=(n as i32) {
        let major = i as f32;
        let l = n as f32;

        // X-axis line (from -l to l at z=major)
        v([-l, 0.0, major], [0.5, 0.5, 0.5, A]);
        v(
            [if i != 0 { l } else { 0.0 }, 0.0, major],
            [0.5, 0.5, 0.5, A],
        );
        // Z-axis line (from -l to l at x=major)
        v([major, 0.0, -l], [0.5, 0.5, 0.5, A]);
        v(
            [major, 0.0, if i != 0 { l } else { 0.0 }],
            [0.5, 0.5, 0.5, A],
        );

        // Highlighted axes at i == 0
        if i == 0 {
            v([0.0, 0.0, major], [1.0, 0.25, 0.25, 1.0]);
            v([l, 0.0, major], [1.0, 0.25, 0.25, 1.0]);
            v([major, 0.0, 0.0], [0.25, 0.25, 1.0, 1.0]);
            v([major, 0.0, l], [0.25, 0.25, 1.0, 1.0]);
        }

        if i == n as i32 {
            break;
        }

        // Minor subdivisions
        for minor in 1..N_DIV {
            let t = major + minor as f32 / N_DIV as f32;
            v([-l, 0.0, t], [0.25, 0.25, 0.25, A]);
            v([l, 0.0, t], [0.25, 0.25, 0.25, A]);
            v([t, 0.0, -l], [0.25, 0.25, 0.25, A]);
            v([t, 0.0, l], [0.25, 0.25, 0.25, A]);
        }
    }

    (positions, colors)
}

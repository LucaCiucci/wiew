//! Trackball gizmo — three coloured rings + axes indicators.
//!
//! Renders three circles in the XY (blue), XZ (red), and YZ (green) planes,
//! plus short axis-arrow stubs in each direction.  When drawn through a
//! [`Pass`], the gizmo follows the pass trackball target and scales with
//! the current trackball radius.
//!
//! Mirrors the logic from wiew2's `Trackball` component.

use std::f32::consts;

use cgmath::{Matrix4, Vector3};
use wgpu::BufferUsages;

use crate::{
    Buf, Pass,
    camera::TrackballCamera,
    context::WCx,
    instance::Instance,
    mesh::{Color, Mesh, Position},
    provided::{Drawable, pipelines::FlatPipeline},
};

/// Reusable trackball / axis gizmo.
///
/// Draws three coloured rings (XY=blue, XZ=red, YZ=green) and short
/// axis-arrow stubs.  Drawn with depth testing so it behaves naturally
/// in a 3D scene.
///
/// No wgpu resources are stored directly — the pipeline is resolved
/// through the resource manager when [`draw`](Self::draw) is called.
///
/// ```ignore
/// let gizmo = TrackballGizmo::new();
///
/// // Every frame:
/// gizmo.draw(&mut pass, &cx, &camera, &instances, 1);
/// ```
pub struct TrackballGizmo {
    mesh: Mesh,
    faded_mesh: Mesh,
    pipeline: FlatPipeline,
    faded_pipeline: FlatPipeline,
    instance_buf: Buf<Instance>,
}

impl TrackballGizmo {
    /// Create the trackball gizmo.
    ///
    pub fn new() -> Self {
        let (positions, colors) = gizmo_vertices();
        let faded_colors = fade_colors(&colors, 0.18);
        Self {
            mesh: Mesh::new(positions.clone()).with_colors(colors),
            faded_mesh: Mesh::new(positions).with_colors(faded_colors),
            pipeline: FlatPipeline::line_list(),
            faded_pipeline: FlatPipeline::depthless_line_list(),
            instance_buf: Buf::new(
                BufferUsages::VERTEX | BufferUsages::COPY_DST,
                vec![Instance::identity()],
            )
            .with_label("wiew trackball gizmo instance"),
        }
    }

    pub fn with_pipeline(mut self, pipeline: FlatPipeline) -> Self {
        self.pipeline = pipeline;
        self
    }

    pub fn with_faded_pipeline(mut self, pipeline: FlatPipeline) -> Self {
        self.faded_pipeline = pipeline;
        self
    }

    pub fn set_pipeline(&mut self, pipeline: FlatPipeline) {
        self.pipeline = pipeline;
    }

    pub fn set_faded_pipeline(&mut self, pipeline: FlatPipeline) {
        self.faded_pipeline = pipeline;
    }

    pub fn draw_for_trackball(&self, cx: &mut WCx, pass: &mut Pass, trackball: &TrackballCamera) {
        let radius = trackball.trackball_world_radius();
        let model = Matrix4::from_translation(Vector3::new(
            trackball.target.x,
            trackball.target.y,
            trackball.target.z,
        )) * Matrix4::from_scale(radius);

        self.instance_buf
            .set_data(vec![Instance::from_matrix(model)]);
        let instance_buf = self.instance_buf.get(cx);
        self.faded_pipeline
            .draw_mesh_instanced(cx, pass, &self.faded_mesh, &instance_buf, 1);
        self.pipeline
            .draw_mesh_instanced(cx, pass, &self.mesh, &instance_buf, 1);
    }
}

impl Drawable for TrackballGizmo {
    fn draw(&self, cx: &mut WCx, pass: &mut Pass) {
        if let Some(trackball) = pass.trackball {
            self.draw_for_trackball(cx, pass, &trackball);
        } else {
            self.pipeline.draw_mesh(cx, pass, &self.mesh);
        }
    }
}

// ---- vertex generation ------------------------------------------------

fn gizmo_vertices() -> (Vec<Position>, Vec<Color>) {
    const N: usize = 100;
    const R: f32 = 0.8;
    const L: f32 = 0.25; // axis-arrow half-length
    const A: f32 = 0.5; // alpha for rings
    const TMP: f32 = 0.25; // dim for non-dominant channels

    let mut positions = Vec::new();
    let mut colors = Vec::new();

    let mut v = |position, color| {
        positions.push(position);
        colors.push(color);
    };

    // Three rings
    for i in 0..N {
        let a1 = (i as f32 / N as f32) * consts::TAU;
        let a2 = ((i + 1) as f32 / N as f32) * consts::TAU;
        let (c1, s1) = (a1.cos(), a1.sin());
        let (c2, s2) = (a2.cos(), a2.sin());

        // YZ plane — red   (x=0)
        v([0.0, c1 * R, s1 * R], [1.0, TMP, TMP, A]);
        v([0.0, c2 * R, s2 * R], [1.0, TMP, TMP, A]);

        // XZ plane — green (y=0)
        v([c1 * R, 0.0, s1 * R], [TMP, 1.0, TMP, A]);
        v([c2 * R, 0.0, s2 * R], [TMP, 1.0, TMP, A]);

        // XY plane — blue  (z=0)
        v([c1 * R, s1 * R, 0.0], [TMP, TMP, 1.0, A]);
        v([c2 * R, s2 * R, 0.0], [TMP, TMP, 1.0, A]);
    }

    // Axis stubs (short arrows from origin)
    // X axis: red
    v([-L * 0.5, 0.0, 0.0], [1.0, TMP, TMP, 1.0]);
    v([L, 0.0, 0.0], [1.0, TMP, TMP, 1.0]);
    // Y axis: green
    v([0.0, -L * 0.5, 0.0], [TMP, 1.0, TMP, 1.0]);
    v([0.0, L, 0.0], [TMP, 1.0, TMP, 1.0]);
    // Z axis: blue
    v([0.0, 0.0, -L * 0.5], [TMP, TMP, 1.0, 1.0]);
    v([0.0, 0.0, L], [TMP, TMP, 1.0, 1.0]);

    (positions, colors)
}

fn fade_colors(colors: &[Color], alpha_scale: f32) -> Vec<Color> {
    colors
        .iter()
        .map(|color| [color[0], color[1], color[2], color[3] * alpha_scale])
        .collect()
}

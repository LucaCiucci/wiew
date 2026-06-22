//! Camera types for wiew.
//!
//! Provides a GPU-side [`Camera`] (uniform buffer + bind group) and a
//! CPU-side [`TrackballCamera`] with orbit interaction logic.

use std::f32::consts;

use cgmath::{
    InnerSpace, Matrix4, Point3, Quaternion, Rad, Rotation, Rotation3, SquareMatrix, Vector3,
};
use wgpu::util::DeviceExt;

use crate::context::{
    WCx,
    ext::{BindGroupLayout, UseBindGroupLayouts},
};

/// Shader-facing camera uniform (std140 layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    pub view_pos: [f32; 4],
    pub view_inverse: [[f32; 4]; 4],
    pub viewport_size: [f32; 4],
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view: Matrix4::identity().into(),
            proj: Matrix4::identity().into(),
            view_pos: [0.0; 4],
            view_inverse: Matrix4::identity().into(),
            viewport_size: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// OpenGL to wgpu clip-space correction.
#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: Matrix4<f32> = Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

/// A GPU camera object: owns the uniform buffer and its bind group.
///
/// Create the bind group layout once with [`Camera::bind_group_layout`],
/// then construct a `Camera` and call [`Camera::update`] every frame.
pub struct Camera {
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    uniform: CameraUniform,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CameraLayout;

impl BindGroupLayout for CameraLayout {
    fn build(&self, cx: &mut WCx) -> wgpu::BindGroupLayout {
        cx.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("wiew camera bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
    }
}

impl Camera {
    pub fn new(cx: &mut WCx, layout: &CameraLayout) -> Self {
        let uniform = CameraUniform::new();
        let layout = cx.use_bind_group_layout(layout);

        let buffer = cx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("wiew camera buffer"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let bind_group = cx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wiew camera bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self {
            buffer,
            bind_group,
            uniform,
        }
    }

    /// Upload new view/projection matrices to the GPU.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        view: Matrix4<f32>,
        proj: Matrix4<f32>,
        view_pos: Point3<f32>,
    ) {
        self.update_with_viewport(queue, view, proj, view_pos, [1.0, 1.0]);
    }

    /// Upload new view/projection matrices and viewport size to the GPU.
    pub fn update_with_viewport(
        &mut self,
        queue: &wgpu::Queue,
        view: Matrix4<f32>,
        proj: Matrix4<f32>,
        view_pos: Point3<f32>,
        viewport_size: [f32; 2],
    ) {
        let view_inverse = view.invert().unwrap_or_else(Matrix4::identity);
        let viewport_size = [viewport_size[0].max(1.0), viewport_size[1].max(1.0)];
        self.uniform.view = view.into();
        self.uniform.proj = proj.into();
        self.uniform.view_pos = [view_pos.x, view_pos.y, view_pos.z, 0.0];
        self.uniform.view_inverse = view_inverse.into();
        self.uniform.viewport_size = [
            viewport_size[0],
            viewport_size[1],
            1.0 / viewport_size[0],
            1.0 / viewport_size[1],
        ];
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
    }

    /// Create the standard bind-group layout for a single camera uniform.
    ///
    /// ```wgsl
    /// @group(0) @binding(0) var<uniform> camera: CameraUniform;
    /// ```
    pub fn bind_group_layout() -> CameraLayout {
        CameraLayout
    }
}

/// A CPU-side arcball / trackball camera.
///
/// Uses a quaternion-based rotation (no gimbal lock). The camera looks at
/// [`target`] from a [`distance`] away, oriented by [`rotation`].
///
/// The interaction model mirrors the one from the wiew2 framework and is
/// designed to feel responsive at any zoom level.
#[derive(Debug, Clone, Copy)]
pub struct TrackballCamera {
    /// Point the camera looks at.
    pub target: Point3<f32>,
    /// Distance from eye to target.
    pub distance: f32,
    /// Orientation as a quaternion (arcball rotation).
    pub rotation: Quaternion<f32>,
    /// Vertical field of view in degrees.
    pub fov_y_deg: f32,
    /// Virtual trackball radius relative to distance.
    /// `1.0 / distance` gives a consistent 1-world-unit ball.
    pub trackball_radius: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Default for TrackballCamera {
    fn default() -> Self {
        const D: f32 = 5.0;
        Self {
            target: Point3::new(0.0, 0.0, 0.0),
            distance: D,
            // yaw=45°, pitch=-30° (negative pitch = looking down) — matches wiew2
            rotation: Quaternion::from_angle_y(Rad(consts::FRAC_PI_4))
                * Quaternion::from_angle_x(Rad(-consts::FRAC_PI_6)),
            fov_y_deg: 30.0,
            trackball_radius: 1.0 / D,
            znear: 0.05,
            zfar: 1000.0,
        }
    }
}

impl TrackballCamera {
    /// Eye position: target + rotated offset.
    pub fn eye(&self) -> Point3<f32> {
        let offset = self
            .rotation
            .rotate_vector(Vector3::unit_z() * self.distance);
        self.target + offset
    }

    /// View matrix — uses the **rotated** up vector so there is no fixed
    /// world-up axis.  This avoids gimbal lock entirely.
    pub fn view_matrix(&self) -> Matrix4<f32> {
        let eye = self.eye();
        let up = self.rotation.rotate_vector(Vector3::unit_y());
        Matrix4::look_at_rh(eye, self.target, up)
    }

    /// Projection matrix, multiplied by [`OPENGL_TO_WGPU_MATRIX`].
    pub fn projection_matrix(&self, aspect: f32) -> Matrix4<f32> {
        let fov_y = Rad(self.fov_y_deg * consts::PI / 180.0);
        OPENGL_TO_WGPU_MATRIX * cgmath::perspective(fov_y, aspect, self.znear, self.zfar)
    }

    /// World-space radius of the visible trackball guide.
    pub fn trackball_world_radius(&self) -> f32 {
        self.distance * self.trackball_radius
    }

    // ── Interaction methods ──────────────────────────────────────────

    /// Arcball rotation from a mouse drag.
    ///
    /// `from` / `to` are **screen-space** pixel coordinates (origin top-left).
    /// `width` / `height` are the viewport dimensions.
    ///
    /// Projects the 2D positions onto a virtual hemisphere and computes the
    /// shortest arc between them.  This is the same algorithm used in wiew2.
    pub fn mouse_rotation(&mut self, from: (f32, f32), to: (f32, f32), width: f32, height: f32) {
        let fovy_rad = self.fov_y_deg * consts::PI / 180.0;
        let factor = fovy_rad.tan() * (1.0 - self.trackball_radius) / height;

        let project = |x: f32, y: f32| -> Vector3<f32> {
            let x = (x - width / 2.0) * factor;
            let y = (height / 2.0 - y) * factor;
            Vector3::new(x, y, self.trackball_radius).normalize()
        };

        let p_prev = project(from.0, from.1);
        let p_curr = project(to.0, to.1);

        // Arcball: rotation from current position back to previous = delta to apply.
        let delta = Quaternion::from_arc(p_curr, p_prev, Some(Vector3::unit_x()));
        self.rotation = self.rotation * delta;
    }

    /// Pan (translate the target) from a mouse drag.
    ///
    /// Moves the target in the camera's local screen-plane, scaling by
    /// distance so panning feels consistent at any zoom level.
    pub fn mouse_pan(&mut self, from: (f32, f32), to: (f32, f32), _width: f32, height: f32) {
        let fovy_rad = self.fov_y_deg * consts::PI / 180.0;
        let factor = fovy_rad / height * self.distance;
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);

        let i = self.rotation.rotate_vector(Vector3::unit_x());
        let j = self.rotation.rotate_vector(Vector3::unit_y());

        self.target += (i * (-dx) + j * dy) * factor;
    }

    /// Zoom by a multiplicative factor (exponential, like wiew2).
    ///
    /// `delta` is typically a scroll-wheel value (positive = zoom in).
    pub fn mouse_zoom(&mut self, delta: f32) {
        self.distance *= 1.25f32.powf(delta);
        self.distance = self
            .distance
            .clamp(self.znear / self.trackball_radius, self.zfar);
    }
}

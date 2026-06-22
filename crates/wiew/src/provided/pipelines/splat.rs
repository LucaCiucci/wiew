use crate::{
    Pass,
    camera::CameraLayout,
    context::{
        WCx,
        ext::{Pipeline, UseBindGroupLayouts, UsePipelines},
    },
    mesh::{Mesh, color_layout, normal_layout, position_layout},
    provided::pipelines::{
        lit::{LitMaterial, LitMaterialLayout},
        streams::{bind_position_normal_color_mesh, bind_position_normal_mesh},
    },
    render_target::{RenderTargetTextures, TargetKey},
};

/// Lit point splat pipeline.
///
/// This expands each position/normal point into a small camera-facing quad in
/// the vertex shader. It is intended for scanner point clouds where raw
/// `PointList` rendering leaves too many holes.
///
/// The nominal point size is read from [`LitMaterial::with_point_size`] and is
/// measured in world units. The shader clamps the projected splat size in
/// pixels, so points can fill gaps when close without becoming huge when the
/// view is zoomed out. The current implementation ignores mesh indices and
/// object instancing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SplatPipeline {
    pub depth_write_enabled: bool,
}

impl SplatPipeline {
    pub fn new() -> Self {
        Self {
            depth_write_enabled: true,
        }
    }

    pub fn bind(
        &self,
        cx: &mut WCx,
        target: &RenderTargetTextures,
    ) -> std::sync::Arc<wgpu::RenderPipeline> {
        cx.use_pipeline(&target.target_key(), self)
    }

    pub fn draw_mesh(&self, cx: &mut WCx, pass: &mut Pass, mesh: &Mesh) {
        self.draw_mesh_with_material(
            cx,
            pass,
            mesh,
            &LitMaterial::default().with_point_size(0.02),
        );
    }

    pub fn draw_mesh_with_material(
        &self,
        cx: &mut WCx,
        pass: &mut Pass,
        mesh: &Mesh,
        material: &LitMaterial,
    ) {
        let camera = pass
            .camera
            .as_ref()
            .expect("splat mesh draw requires a camera")
            .clone();
        let target = pass.target();
        let Some(buffers) = bind_position_normal_mesh(cx, mesh, "splat pipeline") else {
            return;
        };
        let material = material.bind(cx);
        let pipeline = self.bind(cx, target);

        let pass = pass.render_pass();
        pass.set_bind_group(0, &camera, &[]);
        pass.set_bind_group(1, &material.bind_group, &[]);
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffers.positions.slice(..));
        pass.set_vertex_buffer(1, buffers.normals.slice(..));
        pass.draw(0..6, 0..buffers.vertex_count);
    }
}

impl Default for SplatPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline for SplatPipeline {
    fn build(&self, cx: &mut WCx, target: &TargetKey) -> wgpu::RenderPipeline {
        build_splat_pipeline(cx, *target, self.depth_write_enabled)
    }
}

fn build_splat_pipeline(
    cx: &mut WCx,
    target: TargetKey,
    depth_write_enabled: bool,
) -> wgpu::RenderPipeline {
    build_splat_pipeline_with_layout(
        cx,
        target,
        depth_write_enabled,
        "wiew splat pipeline",
        "vs_main",
        &[instance_position_layout(), instance_normal_layout()],
    )
}

/// Lit point splat pipeline using a per-point color stream.
///
/// The mesh must provide positions, normals, and colors. The color stream is
/// treated as the source albedo; the material front/back colors act as tints,
/// while the light color and lighting parameters keep their usual meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColoredSplatPipeline {
    pub depth_write_enabled: bool,
}

impl ColoredSplatPipeline {
    pub fn new() -> Self {
        Self {
            depth_write_enabled: true,
        }
    }

    pub fn bind(
        &self,
        cx: &mut WCx,
        target: &RenderTargetTextures,
    ) -> std::sync::Arc<wgpu::RenderPipeline> {
        cx.use_pipeline(&target.target_key(), self)
    }

    pub fn draw_mesh(&self, cx: &mut WCx, pass: &mut Pass, mesh: &Mesh) {
        self.draw_mesh_with_material(
            cx,
            pass,
            mesh,
            &LitMaterial::default().with_point_size(0.02),
        );
    }

    pub fn draw_mesh_with_material(
        &self,
        cx: &mut WCx,
        pass: &mut Pass,
        mesh: &Mesh,
        material: &LitMaterial,
    ) {
        let camera = pass
            .camera
            .as_ref()
            .expect("colored splat mesh draw requires a camera")
            .clone();
        let target = pass.target();
        let Some(buffers) = bind_position_normal_color_mesh(cx, mesh, "colored splat pipeline")
        else {
            return;
        };
        let material = material.bind(cx);
        let pipeline = self.bind(cx, target);

        let pass = pass.render_pass();
        pass.set_bind_group(0, &camera, &[]);
        pass.set_bind_group(1, &material.bind_group, &[]);
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffers.positions.slice(..));
        pass.set_vertex_buffer(1, buffers.normals.slice(..));
        pass.set_vertex_buffer(2, buffers.colors.slice(..));
        pass.draw(0..6, 0..buffers.vertex_count);
    }
}

impl Default for ColoredSplatPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline for ColoredSplatPipeline {
    fn build(&self, cx: &mut WCx, target: &TargetKey) -> wgpu::RenderPipeline {
        build_colored_splat_pipeline(cx, *target, self.depth_write_enabled)
    }
}

fn build_colored_splat_pipeline(
    cx: &mut WCx,
    target: TargetKey,
    depth_write_enabled: bool,
) -> wgpu::RenderPipeline {
    build_splat_pipeline_with_layout(
        cx,
        target,
        depth_write_enabled,
        "wiew colored splat pipeline",
        "vs_colored_main",
        &[
            instance_position_layout(),
            instance_normal_layout(),
            instance_color_layout(),
        ],
    )
}

fn instance_position_layout() -> wgpu::VertexBufferLayout<'static> {
    let mut layout = position_layout();
    layout.step_mode = wgpu::VertexStepMode::Instance;
    layout
}

fn instance_normal_layout() -> wgpu::VertexBufferLayout<'static> {
    let mut layout = normal_layout();
    layout.step_mode = wgpu::VertexStepMode::Instance;
    layout
}

fn instance_color_layout() -> wgpu::VertexBufferLayout<'static> {
    let mut layout = color_layout();
    layout.step_mode = wgpu::VertexStepMode::Instance;
    layout
}

fn build_splat_pipeline_with_layout(
    cx: &mut WCx,
    target: TargetKey,
    depth_write_enabled: bool,
    label: &'static str,
    vertex_entry: &'static str,
    vertex_buffers: &[wgpu::VertexBufferLayout<'static>],
) -> wgpu::RenderPipeline {
    let camera_layout = cx.use_bind_group_layout(&CameraLayout);
    let material_layout = cx.use_bind_group_layout(&LitMaterialLayout);

    let shader = cx
        .device
        .create_shader_module(wgpu::include_wgsl!("splat.wgsl"));

    let layout = cx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wiew splat pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });

    cx.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(vertex_entry),
                buffers: vertex_buffers,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target
                    .depth_format
                    .expect("splat pipeline requires a depth target"),
                depth_write_enabled: Some(depth_write_enabled),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: target.sample_count.max(1),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        })
}

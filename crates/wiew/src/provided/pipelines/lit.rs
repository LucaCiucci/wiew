use crate::{
    Pass,
    camera::CameraLayout,
    context::{
        WCx,
        ext::{BindGroupLayout, Pipeline, UseBindGroupLayouts, UsePipelines},
    },
    instance::{Instance, identity_instance_buffer},
    mesh::{Mesh, normal_layout, position_layout},
    provided::pipelines::streams::bind_position_normal_mesh,
    render_target::{RenderTargetTextures, TargetKey},
    resource::{H, Res},
};

use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LitMaterialUniform {
    pub front_color: [f32; 4],
    pub back_color: [f32; 4],
    pub light_color: [f32; 4],
    /// Camera-space offset from the camera position.
    pub light_offset: [f32; 4],
    /// x = ambient strength, y = specular strength, z = shininess, w = point size.
    pub params: [f32; 4],
}

impl Default for LitMaterialUniform {
    fn default() -> Self {
        Self {
            front_color: [0.74, 0.86, 1.0, 1.0],
            back_color: [0.42, 0.58, 0.78, 1.0],
            light_color: [0.92, 0.97, 1.0, 1.0],
            light_offset: [0.35, 0.45, 0.20, 0.0],
            params: [0.22, 0.55, 32.0, 0.0],
        }
    }
}

pub struct LitMaterial {
    uniform: Res<LitMaterialUniform>,
}

impl LitMaterial {
    pub fn new(uniform: LitMaterialUniform) -> Self {
        Self {
            uniform: Res::new(uniform),
        }
    }

    pub fn leios_blue() -> Self {
        Self::default()
            .with_front_color([0.30, 0.57, 0.88, 1.0])
            .with_back_color([0.62, 0.64, 0.12, 1.0])
            .with_light_color([0.95, 0.98, 1.0, 1.0])
            .with_light_offset([0.15, 0.25, 0.0])
            .with_lighting(0.52, 0.5, 32.0)
    }

    pub fn leios_dark_blue() -> Self {
        Self::default()
            .with_front_color([0.16, 0.28, 0.42, 1.0])
            .with_back_color([0.46, 0.48, 0.09, 1.0])
            .with_light_color([0.95, 0.98, 1.0, 1.0])
            .with_light_offset([0.15, 0.25, 0.0])
            .with_lighting(0.52, 0.5, 32.0)
    }

    pub fn with_front_color(self, color: [f32; 4]) -> Self {
        self.set_front_color(color);
        self
    }

    pub fn set_front_color(&self, color: [f32; 4]) {
        let mut uniform = *self.uniform.source();
        uniform.front_color = color;
        self.uniform.update(uniform);
    }

    pub fn get_front_color(&self) -> [f32; 4] {
        self.uniform.source().front_color
    }

    pub fn with_back_color(self, color: [f32; 4]) -> Self {
        self.set_back_color(color);
        self
    }

    pub fn set_back_color(&self, color: [f32; 4]) {
        let mut uniform = *self.uniform.source();
        uniform.back_color = color;
        self.uniform.update(uniform);
    }

    pub fn get_back_color(&self) -> [f32; 4] {
        self.uniform.source().back_color
    }

    pub fn with_light_color(self, color: [f32; 4]) -> Self {
        self.set_light_color(color);
        self
    }

    pub fn set_light_color(&self, color: [f32; 4]) {
        let mut uniform = *self.uniform.source();
        uniform.light_color = color;
        self.uniform.update(uniform);
    }

    pub fn get_light_color(&self) -> [f32; 4] {
        self.uniform.source().light_color
    }

    pub fn with_light_offset(self, offset: [f32; 3]) -> Self {
        self.set_light_offset(offset);
        self
    }

    pub fn set_light_offset(&self, offset: [f32; 3]) {
        let mut uniform = *self.uniform.source();
        uniform.light_offset = [offset[0], offset[1], offset[2], 0.0];
        self.uniform.update(uniform);
    }

    pub fn get_light_offset(&self) -> [f32; 3] {
        let uniform = self.uniform.source();
        [
            uniform.light_offset[0],
            uniform.light_offset[1],
            uniform.light_offset[2],
        ]
    }

    pub fn with_lighting(self, ambient: f32, specular: f32, shininess: f32) -> Self {
        self.set_lighting(ambient, specular, shininess);
        self
    }

    pub fn set_lighting(&self, ambient: f32, specular: f32, shininess: f32) {
        let mut uniform = *self.uniform.source();
        uniform.params = [ambient, specular, shininess, uniform.params[3]];
        self.uniform.update(uniform);
    }

    pub fn get_lighting(&self) -> (f32, f32, f32) {
        let uniform = self.uniform.source();
        (uniform.params[0], uniform.params[1], uniform.params[2])
    }

    pub fn with_point_size(self, size: f32) -> Self {
        self.set_point_size(size);
        self
    }

    pub fn set_point_size(&self, size: f32) {
        let mut uniform = *self.uniform.source();
        uniform.params[3] = size.max(0.0);
        self.uniform.update(uniform);
    }

    pub fn get_point_size(&self) -> f32 {
        self.uniform.source().params[3]
    }

    pub(crate) fn bind(&self, cx: &mut WCx) -> H<LitMaterialGpu> {
        let layout = cx.use_bind_group_layout(&LitMaterialLayout);

        cx.resources.get_or_instantiate_with(
            &self.uniform,
            |uniform, _old: Option<LitMaterialGpu>| {
                let buffer = cx
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("wiew lit material buffer"),
                        contents: bytemuck::bytes_of(uniform),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    });

                let bind_group = cx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("wiew lit material bind group"),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                });

                LitMaterialGpu { buffer, bind_group }
            },
        )
    }
}

impl Default for LitMaterial {
    fn default() -> Self {
        Self::new(LitMaterialUniform::default())
    }
}

pub(crate) struct LitMaterialGpu {
    #[allow(dead_code)]
    buffer: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct LitMaterialLayout;

impl BindGroupLayout for LitMaterialLayout {
    fn build(&self, cx: &mut WCx) -> wgpu::BindGroupLayout {
        cx.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("wiew lit material bind group layout"),
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

/// A simple two-sided Blinn-Phong lit shader with camera-relative light and
/// separate front/back material colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LitPipeline {
    pub topology: wgpu::PrimitiveTopology,
    pub depth_write_enabled: bool,
    pub depth_compare: wgpu::CompareFunction,
}

impl LitPipeline {
    pub fn triangle_list() -> Self {
        Self {
            topology: wgpu::PrimitiveTopology::TriangleList,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
        }
    }

    pub fn point_list() -> Self {
        Self {
            topology: wgpu::PrimitiveTopology::PointList,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
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
        self.draw_mesh_with_material(cx, pass, mesh, &LitMaterial::default());
    }

    pub fn draw_mesh_with_material(
        &self,
        cx: &mut WCx,
        pass: &mut Pass,
        mesh: &Mesh,
        material: &LitMaterial,
    ) {
        let instances = identity_instance_buffer(cx);
        self.draw_mesh_instanced_with_material(cx, pass, mesh, material, &instances, 1);
    }

    pub fn draw_mesh_instanced(
        &self,
        cx: &mut WCx,
        pass: &mut Pass,
        mesh: &Mesh,
        instances: &wgpu::Buffer,
        instance_count: u32,
    ) {
        self.draw_mesh_instanced_with_material(
            cx,
            pass,
            mesh,
            &LitMaterial::default(),
            instances,
            instance_count,
        );
    }

    pub fn draw_mesh_instanced_with_material(
        &self,
        cx: &mut WCx,
        pass: &mut Pass,
        mesh: &Mesh,
        material: &LitMaterial,
        instances: &wgpu::Buffer,
        instance_count: u32,
    ) {
        let camera = pass
            .camera
            .as_ref()
            .expect("lit mesh draw requires a camera")
            .clone();
        let target = pass.target();
        let Some(buffers) = bind_position_normal_mesh(cx, mesh, "lit pipeline") else {
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
        pass.set_vertex_buffer(2, instances.slice(..));

        if let Some(indices) = mesh.bind_indices(cx) {
            pass.set_index_buffer(indices.buffer.slice(..), indices.format);
            pass.draw_indexed(0..indices.len, 0, 0..instance_count);
        } else {
            pass.draw(0..buffers.vertex_count, 0..instance_count);
        }
    }
}

impl Default for LitPipeline {
    fn default() -> Self {
        Self::triangle_list()
    }
}

impl Pipeline for LitPipeline {
    fn build(&self, cx: &mut WCx, target: &TargetKey) -> wgpu::RenderPipeline {
        build_lit_pipeline(
            cx,
            *target,
            self.topology,
            self.depth_write_enabled,
            self.depth_compare,
        )
    }
}

fn build_lit_pipeline(
    cx: &mut WCx,
    target: TargetKey,
    topology: wgpu::PrimitiveTopology,
    depth_write_enabled: bool,
    depth_compare: wgpu::CompareFunction,
) -> wgpu::RenderPipeline {
    let camera_layout = cx.use_bind_group_layout(&CameraLayout);
    let material_layout = cx.use_bind_group_layout(&LitMaterialLayout);

    let shader = cx
        .device
        .create_shader_module(wgpu::include_wgsl!("lit.wgsl"));

    let layout = cx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wiew lit pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });

    cx.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wiew lit pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[position_layout(), normal_layout(), Instance::desc()],
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
                topology,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target
                    .depth_format
                    .expect("lit pipeline requires a depth target"),
                depth_write_enabled: Some(depth_write_enabled),
                depth_compare: Some(depth_compare),
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

use crate::{
    Pass,
    camera::CameraLayout,
    context::{
        WCx,
        ext::{Pipeline, UseBindGroupLayouts, UsePipelines},
    },
    instance::{Instance, identity_instance_buffer},
    mesh::{Mesh, color_layout, position_layout},
    provided::pipelines::streams::bind_position_color_mesh,
    render_target::{RenderTargetTextures, TargetKey},
};

/// Semantic description of the provided flat-colour render pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlatPipeline {
    pub topology: wgpu::PrimitiveTopology,
    pub depth_write_enabled: bool,
    pub depth_compare: wgpu::CompareFunction,
}

impl FlatPipeline {
    pub fn triangle_list() -> Self {
        Self {
            topology: wgpu::PrimitiveTopology::TriangleList,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
        }
    }

    pub fn line_list() -> Self {
        Self {
            topology: wgpu::PrimitiveTopology::LineList,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
        }
    }

    pub fn depthless_line_list() -> Self {
        Self {
            topology: wgpu::PrimitiveTopology::LineList,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
        }
    }

    pub fn depthless_triangle_list() -> Self {
        Self {
            topology: wgpu::PrimitiveTopology::TriangleList,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
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
        let instances = identity_instance_buffer(cx);
        self.draw_mesh_instanced(cx, pass, mesh, &instances, 1);
    }

    pub fn draw_mesh_instanced(
        &self,
        cx: &mut WCx,
        pass: &mut Pass,
        mesh: &Mesh,
        instances: &wgpu::Buffer,
        instance_count: u32,
    ) {
        let camera = pass
            .camera
            .as_ref()
            .expect("flat mesh draw requires a camera")
            .clone();
        let target = pass.target();
        let Some(buffers) = bind_position_color_mesh(cx, mesh, "flat pipeline") else {
            return;
        };
        let pipeline = self.bind(cx, target);

        let pass = pass.render_pass();
        pass.set_bind_group(0, &camera, &[]);
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffers.positions.slice(..));
        pass.set_vertex_buffer(1, buffers.colors.slice(..));
        pass.set_vertex_buffer(2, instances.slice(..));

        if let Some(indices) = mesh.bind_indices(cx) {
            pass.set_index_buffer(indices.buffer.slice(..), indices.format);
            pass.draw_indexed(0..indices.len, 0, 0..instance_count);
        } else {
            pass.draw(0..buffers.vertex_count, 0..instance_count);
        }
    }
}

impl Default for FlatPipeline {
    fn default() -> Self {
        Self::triangle_list()
    }
}

impl Pipeline for FlatPipeline {
    fn build(&self, cx: &mut WCx, target: &TargetKey) -> wgpu::RenderPipeline {
        build_flat_pipeline(
            cx,
            *target,
            self.topology,
            self.depth_write_enabled,
            self.depth_compare,
        )
    }
}

fn build_flat_pipeline(
    cx: &mut WCx,
    target: TargetKey,
    topology: wgpu::PrimitiveTopology,
    depth_write_enabled: bool,
    depth_compare: wgpu::CompareFunction,
) -> wgpu::RenderPipeline {
    let camera_layout = cx.use_bind_group_layout(&CameraLayout);

    let shader = cx
        .device
        .create_shader_module(wgpu::include_wgsl!("flat.wgsl"));

    let layout = cx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wiew flat pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });

    cx.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wiew flat pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[position_layout(), color_layout(), Instance::desc()],
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
                    .expect("flat pipeline requires a depth target"),
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

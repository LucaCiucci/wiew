use crate::{
    Pass,
    context::{
        WCx,
        ext::{Pipeline, UsePipelines},
    },
    mesh::{Mesh, color_layout, position_layout},
    provided::pipelines::streams::bind_position_color_mesh,
    render_target::{RenderTargetTextures, TargetKey},
};

/// Semantic description of the provided full-screen background pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BgPipeline;

impl BgPipeline {
    pub fn bind(
        &self,
        cx: &mut WCx,
        target: &RenderTargetTextures,
    ) -> std::sync::Arc<wgpu::RenderPipeline> {
        cx.use_pipeline(&target.target_key(), self)
    }

    pub fn draw_mesh(&self, cx: &mut WCx, pass: &mut Pass, mesh: &Mesh) {
        let target = pass.target();
        let Some(buffers) = bind_position_color_mesh(cx, mesh, "background pipeline") else {
            return;
        };
        let pipeline = self.bind(cx, target);

        let pass = pass.render_pass();
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffers.positions.slice(..));
        pass.set_vertex_buffer(1, buffers.colors.slice(..));

        if let Some(indices) = mesh.bind_indices(cx) {
            pass.set_index_buffer(indices.buffer.slice(..), indices.format);
            pass.draw_indexed(0..indices.len, 0, 0..1);
        } else {
            pass.draw(0..buffers.vertex_count, 0..1);
        }
    }
}

impl Pipeline for BgPipeline {
    fn build(&self, cx: &mut WCx, target: &TargetKey) -> wgpu::RenderPipeline {
        build_bg_pipeline(cx, *target)
    }
}

fn build_bg_pipeline(cx: &mut WCx, target: TargetKey) -> wgpu::RenderPipeline {
    let shader = cx
        .device
        .create_shader_module(wgpu::include_wgsl!("bg.wgsl"));

    let layout = cx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wiew bg pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

    cx.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wiew bg pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[position_layout(), color_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target.format,
                    blend: None,
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
                    .expect("background pipeline requires a depth target"),
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
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

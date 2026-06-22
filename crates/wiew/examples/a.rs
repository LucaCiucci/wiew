use std::{error::Error, path::Path};

use wiew::{
    WCx,
    camera::{Camera, TrackballCamera},
    instance::Instance,
    mesh::{Color, Mesh, MeshStreamId},
    provided::pipelines::FlatPipeline,
    readback::read_render_target_rgba,
    render_target::RenderTarget,
};

use wgpu::util::DeviceExt;

fn main() -> Result<(), Box<dyn Error>> {
    pollster::block_on(render_to_image("target/wiew-first-render.png"))
}

async fn render_to_image(path: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
    let mut cx = WCx::new_headless().await?;
    let target = RenderTarget::new(wgpu::Extent3d {
        width: 512,
        height: 512,
        depth_or_array_layers: 1,
    });
    let mesh = Mesh::new(vec![
        [0.0, 0.65, 0.0],
        [-0.65, -0.55, 0.0],
        [0.65, -0.55, 0.0],
    ])
    .with_colors(vec![
        [1.0, 0.15, 0.10, 1.0],
        [0.10, 0.85, 0.25, 1.0],
        [0.15, 0.35, 1.0, 1.0],
    ]);

    let textures = target.get(&cx);

    let pipeline = FlatPipeline {
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::LessEqual,
    };

    let camera_layout = Camera::bind_group_layout();
    let mut camera = Camera::new(&mut cx, &camera_layout);
    let trackball = TrackballCamera {
        distance: 2.0,
        ..Default::default()
    };
    camera.update(
        &cx.queue,
        trackball.view_matrix(),
        trackball.projection_matrix(1.0),
        trackball.eye(),
    );

    let instances = [Instance::identity()];
    let instance_buffer = cx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wiew first render instances"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let positions = mesh.bind_positions(&mut cx);
    let colors = mesh
        .bind_stream::<Color>(&mut cx, MeshStreamId::COLOR)
        .expect("example mesh has a color stream");

    let mut encoder = cx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wiew first render encoder"),
        });

    {
        let mut pass = textures.begin_render_pass(
            &mut encoder,
            Some(wgpu::Color {
                r: 0.05,
                g: 0.06,
                b: 0.07,
                a: 1.0,
            }),
        );
        pass.set_pipeline(&pipeline.bind(&mut cx, &textures));
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_vertex_buffer(0, positions.slice(..));
        pass.set_vertex_buffer(1, colors.slice(..));
        pass.set_vertex_buffer(2, instance_buffer.slice(..));
        pass.draw(0..positions.len, 0..1);
    }

    cx.queue.submit([encoder.finish()]);

    let image_data = read_render_target_rgba(&cx, &textures)?;
    image::save_buffer(
        path,
        &image_data.bytes,
        image_data.width,
        image_data.height,
        image::ColorType::Rgba8,
    )?;

    Ok(())
}

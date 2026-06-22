//! Comprehensive example: headless render of a scene with camera, grid,
//! background, instanced triangle, saved to PNG.
//!
//! Reproduces the look and capabilities of the wiew2 example.

use std::error::Error;

use wiew::{
    WCx,
    camera::{Camera, TrackballCamera},
    instance::Instance,
    mesh::{Color, Mesh, MeshStreamId, Position},
    provided::pipelines::FlatPipeline,
    readback::read_render_target_rgba,
    render_target::RenderTarget,
};

use wgpu::util::DeviceExt;

type VertexData = (Position, Color);

fn mesh_from_vertices(vertices: Vec<VertexData>) -> Mesh {
    let (positions, colors) = vertices.into_iter().unzip();
    Mesh::new(positions).with_colors(colors)
}

fn bind_color_mesh(
    cx: &mut WCx,
    mesh: &Mesh,
) -> (
    wiew::resource::H<wiew::GpuBuffer>,
    wiew::resource::H<wiew::GpuBuffer>,
    u32,
) {
    let positions = mesh.bind_positions(cx);
    let colors = mesh
        .bind_stream::<Color>(cx, MeshStreamId::COLOR)
        .expect("example mesh has a color stream");
    let vertex_count = positions.len;
    (positions, colors, vertex_count)
}

fn main() -> Result<(), Box<dyn Error>> {
    pollster::block_on(render_scene_to_image("target/wiew-scene.png", 800, 600))
}

// ---------------------------------------------------------------------------
// Scene geometry helpers
// ---------------------------------------------------------------------------

/// A full-screen quad (2 triangles) with per-corner colours.
fn background_quad() -> Vec<VertexData> {
    let tl = [0.06, 0.16, 0.11, 1.0]; // top-left
    let tr = [0.21, 0.09, 0.09, 1.0]; // top-right
    let bl = [0.08, 0.07, 0.20, 1.0]; // bottom-left
    let br = [0.16, 0.08, 0.22, 1.0]; // bottom-right
    vec![
        ([-1.0, -1.0, 0.0], bl),
        ([1.0, -1.0, 0.0], br),
        ([1.0, 1.0, 0.0], tr),
        ([-1.0, -1.0, 0.0], bl),
        ([1.0, 1.0, 0.0], tr),
        ([-1.0, 1.0, 0.0], tl),
    ]
}

/// A ground-plane grid (lines) centred at the origin.
fn grid_lines(n: u32) -> Vec<VertexData> {
    let alpha = 0.3;
    let mut out = Vec::new();

    for i in -(n as i32)..=n as i32 {
        let fi = i as f32;
        let extent = n as f32;

        // Lines parallel to X
        out.push(([-extent, 0.0, fi], [0.5, 0.5, 0.5, alpha]));
        out.push(([extent, 0.0, fi], [0.5, 0.5, 0.5, alpha]));
        // Lines parallel to Z
        out.push(([fi, 0.0, -extent], [0.5, 0.5, 0.5, alpha]));
        out.push(([fi, 0.0, extent], [0.5, 0.5, 0.5, alpha]));

        // Highlight major axes
        if i == 0 {
            // X axis (red)
            out.push(([0.0, 0.0, 0.0], [1.0, 0.25, 0.25, 1.0]));
            out.push(([extent, 0.0, 0.0], [1.0, 0.25, 0.25, 1.0]));
            // Z axis (blue)
            out.push(([0.0, 0.0, 0.0], [0.25, 0.25, 1.0, 1.0]));
            out.push(([0.0, 0.0, extent], [0.25, 0.25, 1.0, 1.0]));
        }
    }
    out
}

/// A single triangle.
fn triangle() -> Vec<VertexData> {
    vec![
        ([0.0, 0.65, 0.0], [1.0, 0.15, 0.10, 1.0]),
        ([-0.65, -0.55, 0.0], [0.10, 0.85, 0.25, 1.0]),
        ([0.65, -0.55, 0.0], [0.15, 0.35, 1.0, 1.0]),
    ]
}

/// A coloured cube as 12 triangles (wireframe-ish look via flat shading).
fn cube() -> Vec<VertexData> {
    // Each face as 2 triangles with per-face colour.
    let f = 0.6; // half-size
    let verts: &[([f32; 3], [f32; 4])] = &[
        // Front (red)
        ([-f, -f, f], [1.0, 0.3, 0.3, 1.0]),
        ([f, -f, f], [1.0, 0.3, 0.3, 1.0]),
        ([f, f, f], [1.0, 0.3, 0.3, 1.0]),
        ([-f, -f, f], [1.0, 0.3, 0.3, 1.0]),
        ([f, f, f], [1.0, 0.3, 0.3, 1.0]),
        ([-f, f, f], [1.0, 0.3, 0.3, 1.0]),
        // Back (green)
        ([f, -f, -f], [0.3, 1.0, 0.3, 1.0]),
        ([-f, -f, -f], [0.3, 1.0, 0.3, 1.0]),
        ([-f, f, -f], [0.3, 1.0, 0.3, 1.0]),
        ([f, -f, -f], [0.3, 1.0, 0.3, 1.0]),
        ([-f, f, -f], [0.3, 1.0, 0.3, 1.0]),
        ([f, f, -f], [0.3, 1.0, 0.3, 1.0]),
        // Top (blue)
        ([-f, f, f], [0.3, 0.3, 1.0, 1.0]),
        ([f, f, f], [0.3, 0.3, 1.0, 1.0]),
        ([f, f, -f], [0.3, 0.3, 1.0, 1.0]),
        ([-f, f, f], [0.3, 0.3, 1.0, 1.0]),
        ([f, f, -f], [0.3, 0.3, 1.0, 1.0]),
        ([-f, f, -f], [0.3, 0.3, 1.0, 1.0]),
        // Bottom (yellow)
        ([-f, -f, -f], [1.0, 1.0, 0.3, 1.0]),
        ([f, -f, -f], [1.0, 1.0, 0.3, 1.0]),
        ([f, -f, f], [1.0, 1.0, 0.3, 1.0]),
        ([-f, -f, -f], [1.0, 1.0, 0.3, 1.0]),
        ([f, -f, f], [1.0, 1.0, 0.3, 1.0]),
        ([-f, -f, f], [1.0, 1.0, 0.3, 1.0]),
        // Right (cyan)
        ([f, -f, f], [0.3, 1.0, 1.0, 1.0]),
        ([f, -f, -f], [0.3, 1.0, 1.0, 1.0]),
        ([f, f, -f], [0.3, 1.0, 1.0, 1.0]),
        ([f, -f, f], [0.3, 1.0, 1.0, 1.0]),
        ([f, f, -f], [0.3, 1.0, 1.0, 1.0]),
        ([f, f, f], [0.3, 1.0, 1.0, 1.0]),
        // Left (magenta)
        ([-f, -f, -f], [1.0, 0.3, 1.0, 1.0]),
        ([-f, -f, f], [1.0, 0.3, 1.0, 1.0]),
        ([-f, f, f], [1.0, 0.3, 1.0, 1.0]),
        ([-f, -f, -f], [1.0, 0.3, 1.0, 1.0]),
        ([-f, f, f], [1.0, 0.3, 1.0, 1.0]),
        ([-f, f, -f], [1.0, 0.3, 1.0, 1.0]),
    ];
    verts.to_vec()
}

// ---------------------------------------------------------------------------
// Instance helpers
// ---------------------------------------------------------------------------

fn single_instance() -> Vec<Instance> {
    vec![Instance::identity()]
}

fn multiple_instances() -> Vec<Instance> {
    let count = 5;
    let spacing = 1.8;
    let mut instances = Vec::with_capacity(count * count);
    for x in 0..count {
        for z in 0..count {
            let xf = (x as f32 - (count as f32 - 1.0) / 2.0) * spacing;
            let zf = (z as f32 - (count as f32 - 1.0) / 2.0) * spacing;
            instances.push(Instance::from_matrix(cgmath::Matrix4::from_translation(
                cgmath::Vector3::new(xf, 0.0, zf),
            )));
        }
    }
    instances
}

// ---------------------------------------------------------------------------
// GPU buffer for instances (simple — no Res/caching for now since instances
// are static in this example)
// ---------------------------------------------------------------------------

fn upload_instances(device: &wgpu::Device, instances: &[Instance]) -> GpuInstanceBuffer {
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("instance buffer"),
        contents: bytemuck::cast_slice(instances),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    GpuInstanceBuffer {
        buffer,
        len: instances.len() as u32,
    }
}

struct GpuInstanceBuffer {
    buffer: wgpu::Buffer,
    len: u32,
}

// ---------------------------------------------------------------------------
// Scene render
// ---------------------------------------------------------------------------

async fn render_scene_to_image(path: &str, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
    let mut cx = WCx::new_headless().await?;

    // --- Render target ---
    let target = RenderTarget::new(wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    });

    // --- Camera ---
    let trackball = TrackballCamera {
        distance: 6.0,
        ..Default::default()
    };

    let camera_layout = Camera::bind_group_layout();
    let mut camera = Camera::new(&mut cx, &camera_layout);

    let aspect = width as f32 / height as f32;
    camera.update(
        &cx.queue,
        trackball.view_matrix(),
        trackball.projection_matrix(aspect),
        trackball.eye(),
    );

    // --- Pipeline ---
    let pipeline = FlatPipeline {
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::LessEqual,
    };
    let grid_pipeline = FlatPipeline {
        topology: wgpu::PrimitiveTopology::LineList,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::LessEqual,
    };

    // --- Meshes ---
    let bg_mesh = mesh_from_vertices(background_quad());
    let grid_mesh = mesh_from_vertices(grid_lines(10));
    let tri_mesh = mesh_from_vertices(triangle());
    let cube_mesh = mesh_from_vertices(cube());

    // --- Instance buffers ---
    let tri_instances = upload_instances(&cx.device, &single_instance());
    let cube_instances = upload_instances(&cx.device, &multiple_instances());

    // --- Encode render commands ---
    let textures = target.get(&cx);
    let mut encoder = cx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wiew scene encoder"),
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

        let rp = pipeline.bind(&mut cx, &textures);
        let grid_rp = grid_pipeline.bind(&mut cx, &textures);

        pass.set_bind_group(0, &camera.bind_group, &[]);

        // 1. Background quad (no depth test)
        let (bg_positions, bg_colors, bg_vertex_count) = bind_color_mesh(&mut cx, &bg_mesh);
        pass.set_pipeline(&rp);
        pass.set_vertex_buffer(0, bg_positions.slice(..));
        pass.set_vertex_buffer(1, bg_colors.slice(..));
        pass.set_vertex_buffer(2, tri_instances.buffer.slice(..));
        pass.draw(0..bg_vertex_count, 0..tri_instances.len);

        // 2. Grid (with depth test)
        let (grid_positions, grid_colors, grid_vertex_count) = bind_color_mesh(&mut cx, &grid_mesh);
        pass.set_pipeline(&grid_rp);
        pass.set_vertex_buffer(0, grid_positions.slice(..));
        pass.set_vertex_buffer(1, grid_colors.slice(..));
        pass.set_vertex_buffer(2, tri_instances.buffer.slice(..));
        pass.draw(0..grid_vertex_count, 0..tri_instances.len);

        // 3. Triangle (center, instanced once)
        let (tri_positions, tri_colors, tri_vertex_count) = bind_color_mesh(&mut cx, &tri_mesh);
        pass.set_pipeline(&rp);
        pass.set_vertex_buffer(0, tri_positions.slice(..));
        pass.set_vertex_buffer(1, tri_colors.slice(..));
        pass.set_vertex_buffer(2, tri_instances.buffer.slice(..));
        pass.draw(0..tri_vertex_count, 0..tri_instances.len);

        // 4. Cubes (grid of cubes)
        let (cube_positions, cube_colors, cube_vertex_count) = bind_color_mesh(&mut cx, &cube_mesh);
        pass.set_pipeline(&rp);
        pass.set_vertex_buffer(0, cube_positions.slice(..));
        pass.set_vertex_buffer(1, cube_colors.slice(..));
        pass.set_vertex_buffer(2, cube_instances.buffer.slice(..));
        pass.draw(0..cube_vertex_count, 0..cube_instances.len);
    }

    cx.queue.submit([encoder.finish()]);

    // --- Readback ---
    let image_data = read_render_target_rgba(&cx, &textures)?;
    image::save_buffer(
        path,
        &image_data.bytes,
        width,
        height,
        image::ColorType::Rgba8,
    )?;
    println!("Saved {}", path);

    Ok(())
}

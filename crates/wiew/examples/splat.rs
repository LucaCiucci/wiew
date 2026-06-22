//! Headless lit point-splat render saved to `target/wiew-splat.png`.

use std::error::Error;

use wiew::{
    Pass, WCx,
    camera::{Camera, TrackballCamera},
    mesh::{Mesh, Normal, Position},
    provided::pipelines::{LitMaterial, SplatPipeline},
    readback::read_render_target_rgba,
    render_target::RenderTarget,
};

fn main() -> Result<(), Box<dyn Error>> {
    pollster::block_on(render_to_image("target/wiew-splat.png"))
}

async fn render_to_image(path: &str) -> Result<(), Box<dyn Error>> {
    let mut cx = WCx::new_headless().await?;
    let target = RenderTarget::new(wgpu::Extent3d {
        width: 800,
        height: 600,
        depth_or_array_layers: 1,
    });

    let trackball = TrackballCamera {
        distance: 4.0,
        ..Default::default()
    };
    let camera_layout = Camera::bind_group_layout();
    let mut camera = Camera::new(&mut cx, &camera_layout);
    camera.update_with_viewport(
        &cx.queue,
        trackball.view_matrix(),
        trackball.projection_matrix(800.0 / 600.0),
        trackball.eye(),
        [800.0, 600.0],
    );

    let mesh = bumpy_point_cloud(220, 120);
    let pipeline = SplatPipeline::new();
    let material = LitMaterial::leios_blue().with_point_size(0.035);

    let textures = target.get(&cx);
    let mut encoder = cx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wiew splat example encoder"),
        });

    textures.render_pass(
        &mut encoder,
        Some(wgpu::Color {
            r: 0.05,
            g: 0.06,
            b: 0.07,
            a: 1.0,
        }),
        |rp| {
            let mut pass = Pass::new(rp, &textures).with_camera(&camera);
            pipeline.draw_mesh_with_material(&mut cx, &mut pass, &mesh, &material);
        },
    );

    cx.queue.submit([encoder.finish()]);

    let image_data = read_render_target_rgba(&cx, &textures)?;
    image::save_buffer(
        path,
        &image_data.bytes,
        image_data.width,
        image_data.height,
        image::ColorType::Rgba8,
    )?;
    println!("Saved {path}");
    Ok(())
}

fn bumpy_point_cloud(n_theta: usize, n_phi: usize) -> Mesh {
    let mut positions = Vec::<Position>::with_capacity(n_theta * n_phi);
    let mut normals = Vec::<Normal>::with_capacity(n_theta * n_phi);
    let d_theta = std::f32::consts::PI / n_theta as f32;
    let d_phi = std::f32::consts::TAU / n_phi as f32;

    for theta_idx in 0..n_theta {
        let theta = ((theta_idx as f32 + 0.5) / n_theta as f32) * std::f32::consts::PI;
        for phi_idx in 0..n_phi {
            let phi = (phi_idx as f32 / n_phi as f32) * std::f32::consts::TAU;
            let position = bumpy_position(theta, phi);
            let d_theta = sub(
                bumpy_position(theta + d_theta, phi),
                bumpy_position(theta - d_theta, phi),
            );
            let d_phi = sub(
                bumpy_position(theta, phi + d_phi),
                bumpy_position(theta, phi - d_phi),
            );
            positions.push(position);
            normals.push(normalize(cross(d_phi, d_theta)));
        }
    }

    Mesh::new(positions).with_normals(normals)
}

fn bumpy_position(theta: f32, phi: f32) -> Position {
    let theta = theta.clamp(0.001, std::f32::consts::PI - 0.001);
    let direction = [
        theta.sin() * phi.cos(),
        theta.cos(),
        theta.sin() * phi.sin(),
    ];
    let ripple = 0.12 * (direction[0] * 8.0 + direction[2] * 5.0).sin()
        + 0.08 * (theta * 7.0).sin() * (phi * 3.0).cos();
    let radius = 1.0 + ripple;
    [
        direction[0] * radius,
        direction[1] * radius,
        direction[2] * radius,
    ]
}

fn sub(a: Position, b: Position) -> Position {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Position, b: Position) -> Position {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: Position) -> Normal {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

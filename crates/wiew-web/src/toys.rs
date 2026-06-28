use wasm_bindgen::prelude::*;
use wiew::{
    Pass, WCx,
    drawable::Drawable,
    mesh::{Mesh, Normal, Position},
    provided::pipelines::{LitMaterial, LitPipeline},
};

use crate::{ADDITIONAL_OBJECTS, CTX};

// ---------------------------------------------------------------------------
// Wasm entry point – callable from JavaScript
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn load_toys() -> Result<(), JsValue> {
    // Simulated scan: bumpy hemisphere, rendered as triangles
    let scan = ScanSurface::new(60, 40, [0.0, 0.0, 0.0], LitMaterial::leios_blue());

    ADDITIONAL_OBJECTS.with(|objs| {
        objs.borrow_mut().push(Box::new(scan));
    });

    CTX.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.request_repaint();
        }
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// ScanSurface — bumpy hemisphere (simulated LiDAR scan)
// ---------------------------------------------------------------------------

struct ScanSurface {
    mesh: Mesh,
    pipeline: LitPipeline,
    material: LitMaterial,
}

impl ScanSurface {
    fn new(n_theta: usize, n_phi: usize, center: [f32; 3], material: LitMaterial) -> Self {
        let mesh = simulated_scan_mesh(n_theta, n_phi, center);
        Self {
            mesh,
            pipeline: LitPipeline::triangle_list(),
            material,
        }
    }
}

impl Drawable for ScanSurface {
    fn draw(&self, cx: &mut WCx, pass: &mut Pass) {
        self.pipeline
            .draw_mesh_with_material(cx, pass, &self.mesh, &self.material);
    }
}

// ---------------------------------------------------------------------------
// Simulated scan mesh (bumpy hemisphere, triangle-indexed)
// ---------------------------------------------------------------------------

fn simulated_scan_mesh(n_theta: usize, n_phi: usize, center: [f32; 3]) -> Mesh {
    let count = n_theta * n_phi;
    let mut positions = Vec::<Position>::with_capacity(count);
    let mut normals = Vec::<Normal>::with_capacity(count);
    let mut indices = Vec::<u32>::with_capacity(n_theta * n_phi * 6);

    let d_theta = std::f32::consts::PI / n_theta as f32;
    let d_phi = std::f32::consts::TAU / n_phi as f32;

    for theta_idx in 0..n_theta {
        let theta = ((theta_idx as f32 + 0.5) / n_theta as f32) * std::f32::consts::PI;

        for phi_idx in 0..n_phi {
            let phi = (phi_idx as f32 / n_phi as f32) * std::f32::consts::TAU / 2.0;

            let position = scan_position(center, theta, phi);
            let p_theta_prev = scan_position(center, theta - d_theta, phi);
            let p_theta_next = scan_position(center, theta + d_theta, phi);
            let p_phi_prev = scan_position(center, theta, phi - d_phi);
            let p_phi_next = scan_position(center, theta, phi + d_phi);

            let d_theta = sub(p_theta_next, p_theta_prev);
            let d_phi = sub(p_phi_next, p_phi_prev);
            let normal = normalize(cross(d_phi, d_theta));

            normals.push(normal);
            positions.push(position);
        }
    }

    // Build triangle indices from the grid
    for row in 0..(n_theta - 1) {
        for col in 0..(n_phi - 1) {
            let a = (row * n_phi + col) as u32;
            let b = a + 1;
            let c = ((row + 1) * n_phi + col) as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    Mesh::new(positions)
        .with_normals(normals)
        .with_indices_u32(indices)
}

fn scan_position(center: [f32; 3], theta: f32, phi: f32) -> [f32; 3] {
    let theta = theta.clamp(0.001, std::f32::consts::PI - 0.001);
    let sin_theta = theta.sin();
    let cos_theta = theta.cos();
    let direction = [sin_theta * phi.cos(), cos_theta, sin_theta * phi.sin()];
    let ripple = 0.12 * (direction[0] * 8.0 + direction[2] * 5.0).sin()
        + 0.08 * (theta * 7.0).sin() * (phi * 3.0).cos();
    let radius = 1.0 + ripple;

    [
        center[0] + direction[0] * radius,
        center[1] + direction[1] * radius,
        center[2] + direction[2] * radius,
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

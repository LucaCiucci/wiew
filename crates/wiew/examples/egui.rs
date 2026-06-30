//! egui demo: interactive 3D scene with trackball camera.
//!
//! Drag to rotate, scroll to zoom, right-drag to pan.
//!
//! Showcases [`EguiView3d`] and the provided components.

use eframe::egui;
use wiew::{
    Pass, WCx,
    egui_view::EguiView3d,
    mesh::{Mesh, Normal, Position},
    provided::{
        Grid, QuadBackground, QuadBackgroundConfig, TrackballGizmo,
        pipelines::{FlatPipeline, LitMaterial, LitPipeline},
    },
    view::View3d,
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "wiew 3D Demo",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    left_view: EguiView3d,
    right_view: EguiView3d,
    scene: Scene,
    render_state: egui_wgpu::RenderState,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("this example requires the wgpu backend")
            .clone();
        let device = render_state.device.clone();
        let queue = render_state.queue.clone();

        let left_view = EguiView3d::new(device.clone(), queue.clone(), 450, 600, 6.0)
            .with_texture_name("left scene");
        let right_view =
            EguiView3d::new(device, queue, 450, 600, 6.0).with_texture_name("right scene");
        let scene = Scene::new(&left_view.view);

        Self {
            left_view,
            right_view,
            scene,
            render_state,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let left_tex_id = self
            .left_view
            .render_to_egui(&self.render_state, |cx, pass| self.scene.render(cx, pass));
        let right_tex_id = self
            .right_view
            .render_to_egui(&self.render_state, |cx, pass| self.scene.render(cx, pass));

        egui::Panel::top("header").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("wiew 3D · egui demo");
                ui.separator();
                ui.label("MSAA:");
                egui::ComboBox::from_id_salt("msaa")
                    .selected_text(format!("{}×", self.left_view.samples()))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.left_view.samples() == 1, "Off (1×)")
                            .clicked()
                        {
                            self.left_view.set_samples(1);
                            self.right_view.set_samples(1);
                        }
                        if ui
                            .selectable_label(self.left_view.samples() == 4, "4×")
                            .clicked()
                        {
                            self.left_view.set_samples(4);
                            self.right_view.set_samples(4);
                        }
                    });
            });
            ui.label("Drag to rotate · Scroll to zoom · Right-drag to pan");
        });

        let available = ui.available_size();
        let gap = ui.spacing().item_spacing.x;
        let view_size = egui::vec2((available.x - gap).max(0.0) * 0.5, available.y);
        let mut left_desired = egui::Vec2::ZERO;
        let mut right_desired = egui::Vec2::ZERO;

        ui.horizontal(|ui| {
            ui.set_height(view_size.y);
            ui.allocate_ui_with_layout(view_size, egui::Layout::top_down(egui::Align::Min), |ui| {
                left_desired = self.left_view.viewport_interactive(ui, left_tex_id).rect.size();
            });
            ui.allocate_ui_with_layout(view_size, egui::Layout::top_down(egui::Align::Min), |ui| {
                right_desired = self.right_view.viewport_interactive(ui, right_tex_id).rect.size();
            });
        });

        self.left_view.resize_from(left_desired);
        self.right_view.resize_from(right_desired);
        ui.ctx().request_repaint();
    }
}

struct Scene {
    bg: QuadBackground,
    grid: Grid,
    gizmo: TrackballGizmo,
    tri_mesh: Mesh,
    tri_pipeline: FlatPipeline,
    sphere_mesh: Mesh,
    sphere_pipeline: LitPipeline,
    sphere_material: LitMaterial,
    point_cloud: Mesh,
    point_pipeline: LitPipeline,
    point_material: LitMaterial,
}

impl Scene {
    fn new(_view: &View3d) -> Self {
        let bg = QuadBackground::new(QuadBackgroundConfig::default());
        let grid = Grid::new(10);
        let gizmo = TrackballGizmo::new();

        let tri_pipeline = FlatPipeline {
            topology: wgpu::PrimitiveTopology::TriangleList,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
        };
        let sphere_pipeline = LitPipeline::triangle_list();
        let point_pipeline = LitPipeline::point_list();
        let sphere_material = LitMaterial::leios_dark_blue();
        let point_material = LitMaterial::leios_blue();

        Self {
            bg,
            grid,
            gizmo,
            tri_mesh: triangle_mesh(),
            tri_pipeline,
            sphere_mesh: sphere_mesh(48, 24),
            sphere_pipeline,
            sphere_material,
            point_cloud: simulated_point_cloud(1400 * 2, 1000 * 2),
            point_pipeline,
            point_material,
        }
    }

    fn render(&mut self, cx: &mut WCx, pass: &mut Pass) {
        pass.draw(cx, &self.bg);
        pass.draw(cx, &self.grid);
        self.tri_pipeline.draw_mesh(cx, pass, &self.tri_mesh);
        self.sphere_pipeline.draw_mesh_with_material(
            cx,
            pass,
            &self.sphere_mesh,
            &self.sphere_material,
        );
        self.point_pipeline.draw_mesh_with_material(
            cx,
            pass,
            &self.point_cloud,
            &self.point_material,
        );
        pass.draw(cx, &self.gizmo);
    }
}

fn triangle_mesh() -> Mesh {
    Mesh::new(vec![[0.0, 0.5, 0.0], [-0.5, -0.4, 0.0], [0.5, -0.4, 0.0]]).with_colors(vec![
        [1.0, 0.15, 0.10, 1.0],
        [0.10, 0.85, 0.25, 1.0],
        [0.15, 0.35, 1.0, 1.0],
    ])
}

fn sphere_mesh(columns: u32, rows: u32) -> Mesh {
    let mut positions = Vec::<Position>::new();
    let mut normals = Vec::<Normal>::new();
    let mut indices = Vec::<u32>::new();

    let radius = 0.75;
    let center = [-1.35, 0.85, 0.0];

    for row in 0..=rows {
        let v = row as f32 / rows as f32;
        let theta = v * std::f32::consts::PI / 2.0;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();

        for col in 0..=columns {
            let u = col as f32 / columns as f32;
            let phi = u * std::f32::consts::TAU;
            let normal = [sin_theta * phi.cos(), cos_theta, sin_theta * phi.sin()];
            normals.push(normal);
            positions.push([
                center[0] + normal[0] * radius,
                center[1] + normal[1] * radius,
                center[2] + normal[2] * radius,
            ]);
        }
    }

    let stride = columns + 1;
    for row in 0..rows {
        for col in 0..columns {
            let a = row * stride + col;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    Mesh::new(positions)
        .with_normals(normals)
        .with_indices_u32(indices)
}

fn simulated_point_cloud(n_theta: usize, n_phi: usize) -> Mesh {
    let count = n_theta * n_phi;
    let mut positions = Vec::<Position>::with_capacity(count);
    let mut normals = Vec::<Normal>::with_capacity(count);

    let center = [1.35, 0.82, 0.0];
    let d_theta = std::f32::consts::PI / n_theta as f32;
    let d_phi = std::f32::consts::TAU / n_phi as f32;

    for theta_idx in 0..n_theta {
        let theta = ((theta_idx as f32 + 0.5) / n_theta as f32) * std::f32::consts::PI;

        for phi_idx in 0..n_phi {
            let phi = (phi_idx as f32 / n_phi as f32) * std::f32::consts::TAU / 2.0;

            let position = simulated_point_cloud_position(center, theta, phi);
            let p_theta_prev = simulated_point_cloud_position(center, theta - d_theta, phi);
            let p_theta_next = simulated_point_cloud_position(center, theta + d_theta, phi);
            let p_phi_prev = simulated_point_cloud_position(center, theta, phi - d_phi);
            let p_phi_next = simulated_point_cloud_position(center, theta, phi + d_phi);

            let d_theta = sub(p_theta_next, p_theta_prev);
            let d_phi = sub(p_phi_next, p_phi_prev);
            let normal = normalize(cross(d_phi, d_theta));

            normals.push(normal);
            positions.push(position);
        }
    }

    Mesh::new(positions).with_normals(normals)
}

fn simulated_point_cloud_position(center: Position, theta: f32, phi: f32) -> Position {
    let theta = theta.clamp(0.001, std::f32::consts::PI - 0.001);
    let sin_theta = theta.sin();
    let cos_theta = theta.cos();
    let direction = [sin_theta * phi.cos(), cos_theta, sin_theta * phi.sin()];
    let ripple = 0.12 * (direction[0] * 8.0 + direction[2] * 5.0).sin()
        + 0.08 * (theta * 7.0).sin() * (phi * 3.0).cos();
    let radius = 0.75 + ripple;

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

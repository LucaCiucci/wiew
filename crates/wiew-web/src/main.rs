//! Web demo of the wiew point-cloud viewer.
//!
//! This crate compiles to Wasm and renders an interactive point cloud
//! inside an egui canvas.  The point data is embedded at compile time so
//! no network requests are needed.
//!
//! Build
//! -----
//! ```sh
//! trunk build --release
//! ```
//!
//! Run locally
//! -----------
//! ```sh
//! trunk serve --release
//! # open http://127.0.0.1:8080
//! ```

use std::str::FromStr;

use wasm_bindgen::prelude::*;
use wiew::{
    Pass, WCx,
    egui_view::EguiView3d,
    mesh::{Color, Mesh, Normal, Position},
    provided::{
        Grid, QuadBackground, QuadBackgroundConfig, TrackballGizmo,
        pipelines::{ColoredSplatPipeline, LitMaterial, SplatPipeline},
    },
};

// ---------------------------------------------------------------------------
// Entry point (wasm)
// ---------------------------------------------------------------------------

fn main() {
    console_error_panic_hook::set_once();
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("wiew_canvas")
            .expect("canvas #wiew_canvas not found")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("element is not a canvas");

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(App::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct App {
    view: EguiView3d,
    scene: Scene,
    colors: bool,
    render_state: wiew::egui_wgpu::RenderState,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("the wgpu backend is required")
            .clone();
        let device = render_state.device.clone();
        let queue = render_state.queue.clone();

        let view = EguiView3d::new(device, queue, 900, 600, 5.0)
            .with_texture_name("wiew web scene");

        let point_cloud = load_embedded_points();
        let ply_mesh = load_ply_points();
        let scene = Scene::new(point_cloud, ply_mesh);

        Self {
            view,
            scene,
            colors: false,
            render_state,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let tex_id = self.view.render_to_egui(
            &self.render_state,
            |cx, pass| self.scene.render(cx, pass, self.colors),
        );

        egui::Panel::top("header").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("wiew point cloud");
                ui.separator();
                ui.label(format!("{} pts", self.scene.point_count));
                ui.separator();
                ui.label(format!("{} PLY pts", self.scene.ply_count));
            });
            ui.label("Drag to rotate · Scroll to zoom · Right-drag to pan");
        });

        egui::Panel::right("material_panel")
            .resizable(true)
            .default_size(260.0)
            .show_inside(ui, |ui| {
                ui.heading("Material");
                ui.separator();
                material_ui(
                    ui,
                    &mut self.colors,
                    &mut self.scene.point_material,
                    &mut self.scene.point_colored_material,
                );
            });

        let desired = self.view.central_panel_interactive(ui, tex_id);
        self.view.resize_from(desired);
    }
}

// ---------------------------------------------------------------------------
// Material UI
// ---------------------------------------------------------------------------

fn material_ui(
    ui: &mut egui::Ui,
    colors: &mut bool,
    mono_mat: &mut LitMaterial,
    color_mat: &mut LitMaterial,
) {
    let mat = if *colors { color_mat } else { mono_mat };

    egui::Grid::new("material_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Colors");
            ui.checkbox(colors, "Enable colors");
            ui.end_row();

            ui.label("Front tint");
            let mut c = mat.get_front_color();
            if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() {
                mat.set_front_color(c);
            }
            ui.end_row();

            ui.label("Back tint");
            let mut c = mat.get_back_color();
            if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() {
                mat.set_back_color(c);
            }
            ui.end_row();

            ui.label("Light color");
            let mut c = mat.get_light_color();
            if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() {
                mat.set_light_color(c);
            }
            ui.end_row();

            ui.label("Light offset");
            let mut off = mat.get_light_offset();
            let mut changed = false;
            ui.vertical(|ui| {
                changed |= ui
                    .add(egui::Slider::new(&mut off[0], -2.0..=2.0).text("x"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut off[1], -2.0..=2.0).text("y"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut off[2], -2.0..=2.0).text("z"))
                    .changed();
            });
            if changed {
                mat.set_light_offset(off);
            }
            ui.end_row();
        });

    ui.separator();
    ui.label("Lighting");
    let (mut ambient, mut specular, mut shininess) = mat.get_lighting();
    let mut changed = false;
    changed |= ui
        .add(egui::Slider::new(&mut ambient, 0.0..=1.0).text("Ambient"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut specular, 0.0..=1.0).text("Specular"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut shininess, 1.0..=256.0).text("Shininess"))
        .changed();
    if changed {
        mat.set_lighting(ambient, specular, shininess);
    }

    ui.separator();
    let mut point_size = mat.get_point_size();
    if ui
        .add(
            egui::Slider::new(&mut point_size, 0.001..=0.05)
                .logarithmic(true)
                .text("Point size"),
        )
        .changed()
    {
        mat.set_point_size(point_size);
    }
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

struct Scene {
    bg: QuadBackground,
    grid: Grid,
    gizmo: TrackballGizmo,
    point_cloud: Mesh,
    point_count: usize,
    ply_mesh: Mesh,
    ply_count: usize,
    point_pipeline: SplatPipeline,
    point_colored_pipeline: ColoredSplatPipeline,
    point_material: LitMaterial,
    point_colored_material: LitMaterial,
    ply_material: LitMaterial,
}

impl Scene {
    fn new(point_cloud: LoadedPointCloud, ply_mesh: LoadedPointCloud) -> Self {
        let point_count = point_cloud.point_count;
        let point_material = LitMaterial::leios_blue().with_point_size(0.003);
        let point_colored_material = LitMaterial::leios_blue()
            .with_front_color([1.0, 1.0, 1.0, 1.0])
            .with_point_size(0.003);
        let ply_material = LitMaterial::leios_blue()
            .with_front_color([1.0, 0.3, 0.3, 1.0]) // reddish to distinguish
            .with_back_color([0.5, 0.1, 0.1, 1.0])
            .with_point_size(0.001);

        Self {
            bg: QuadBackground::new(QuadBackgroundConfig::default()),
            grid: Grid::new(10),
            gizmo: TrackballGizmo::new(),
            point_cloud: point_cloud.mesh,
            point_count,
            ply_mesh: ply_mesh.mesh,
            ply_count: ply_mesh.point_count,
            point_pipeline: SplatPipeline::new(),
            point_colored_pipeline: ColoredSplatPipeline::new(),
            point_material,
            point_colored_material,
            ply_material,
        }
    }

    fn render(&mut self, cx: &mut WCx, pass: &mut Pass, colors: bool) {
        pass.draw(cx, &self.bg);
        pass.draw(cx, &self.grid);
        if colors {
            self.point_colored_pipeline.draw_mesh_with_material(
                cx,
                pass,
                &self.point_cloud,
                &self.point_colored_material,
            );
        } else {
            self.point_pipeline.draw_mesh_with_material(
                cx,
                pass,
                &self.point_cloud,
                &self.point_material,
            );
        }
        self.point_pipeline.draw_mesh_with_material(
            cx,
            pass,
            &self.ply_mesh,
            &self.ply_material,
        );
        pass.draw(cx, &self.gizmo);
    }
}

// ---------------------------------------------------------------------------
// Point data – embedded at compile time
// ---------------------------------------------------------------------------

struct LoadedPointCloud {
    mesh: Mesh,
    point_count: usize,
}

/// Raw text of the sample points file, baked into the Wasm binary.
static EMBEDDED_POINTS: &str = include_str!("../../../points_6.txt");

fn load_embedded_points() -> LoadedPointCloud {
    let mut positions = Vec::<Position>::new();
    let mut normals = Vec::<Normal>::new();
    let mut colors = Vec::<Color>::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for line in EMBEDDED_POINTS.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let values: Vec<f32> = line
            .split_whitespace()
            .map(|s| f32::from_str(s).expect("invalid float in embedded points"))
            .collect();
        if values.len() < 10 {
            panic!("expected at least 10 values per line, got {}", values.len());
        }

        let position = [values[0], values[1], values[2]];
        let normal = normalize([values[3], values[4], values[5]]);
        let color = parse_color(values[6], values[7], values[8], values[9]);

        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
        positions.push(position);
        normals.push(normal);
        colors.push(color);
    }

    assert!(!positions.is_empty(), "embedded points file is empty");

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = [
        (max[0] - min[0]).max(0.0001),
        (max[1] - min[1]).max(0.0001),
        (max[2] - min[2]).max(0.0001),
    ];
    let scale = 3.0 / extent[0].max(extent[1]).max(extent[2]);

    for position in &mut positions {
        position[0] = (position[0] - center[0]) * scale;
        position[1] = (position[1] - center[1]) * scale;
        position[2] = (position[2] - center[2]) * scale;
    }

    let point_count = positions.len();
    LoadedPointCloud {
        mesh: Mesh::new(positions)
            .with_normals(normals)
            .with_colors(colors),
        point_count,
    }
}

// ---------------------------------------------------------------------------
// PLY file — embedded binary, first 100 vertices only
// ---------------------------------------------------------------------------

/// Raw bytes of `tot.ply` (binary PLY, ~400 MB of vertices — we only read the
/// header and first 100 points to keep the Wasm binary sane).
static PLY_BYTES: &[u8] = include_bytes!("../../../tot_fiori.ply");

fn load_ply_points() -> LoadedPointCloud {
    // Find the end_header marker (in ASCII, even for binary PLY files).
    let header_end = PLY_BYTES
        .windows(b"end_header\n".len())
        .position(|w| w == b"end_header\n")
        .expect("PLY file missing end_header")
        + b"end_header\n".len();

    // Read up to 100 vertices, each is 6 × f32_le = 24 bytes.
    const VERTEX_STRIDE: usize = 24; // x,y,z,nx,ny,nz as f32_le
    const MAX_PLY_POINTS: usize = 1000000_000;
    let data = &PLY_BYTES[header_end..];
    let count = (data.len() / VERTEX_STRIDE).min(MAX_PLY_POINTS);

    let mut positions = Vec::<Position>::new();
    let mut normals = Vec::<Normal>::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for i in 0..count {
        let offset = i * VERTEX_STRIDE;
        let chunk = &data[offset..offset + VERTEX_STRIDE];

        let x = f32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let y = f32::from_le_bytes(chunk[4..8].try_into().unwrap());
        let z = f32::from_le_bytes(chunk[8..12].try_into().unwrap());
        let nx = f32::from_le_bytes(chunk[12..16].try_into().unwrap());
        let ny = f32::from_le_bytes(chunk[16..20].try_into().unwrap());
        let nz = f32::from_le_bytes(chunk[20..24].try_into().unwrap());

        let position = [x, y, z];
        let normal = normalize([nx, ny, nz]);

        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
        positions.push(position);
        normals.push(normal);
    }

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = [
        (max[0] - min[0]).max(0.0001),
        (max[1] - min[1]).max(0.0001),
        (max[2] - min[2]).max(0.0001),
    ];
    let scale = 3.0 / extent[0].max(extent[1]).max(extent[2]);

    for position in &mut positions {
        position[0] = (position[0] - center[0]) * scale;
        position[1] = (position[1] - center[1]) * scale;
        position[2] = (position[2] - center[2]) * scale;
    }

    let point_count = positions.len();
    LoadedPointCloud {
        mesh: Mesh::new(positions).with_normals(normals),
        point_count,
    }
}

fn parse_color(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    let rgb_scale = if r > 1.0 || g > 1.0 || b > 1.0 {
        1.0 / 255.0
    } else {
        1.0
    };
    [
        r * rgb_scale,
        g * rgb_scale,
        b * rgb_scale,
        a.clamp(0.0, 1.0),
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

//! Interactive point-cloud file viewer for `points_6.txt`.
//!
//! Expected row format:
//! `x y z nx ny nz r g b a`

use std::{
    error::Error,
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use eframe::egui;
use wiew::{
    Pass, WCx,
    egui_view::EguiView3d,
    mesh::{Color, Mesh, Normal, Position},
    provided::{
        Grid, PC_LOD_PAYLOAD_CHUNK_SIZE, PcLod, QuadBackground, QuadBackgroundConfig,
        TrackballGizmo,
        pipelines::{ColoredSplatPipeline, LitMaterial, SplatPipeline},
    },
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "wiew point-cloud file",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    view: EguiView3d,
    scene: Scene,
    colors: bool,
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

        let view =
            EguiView3d::new(device, queue, 900, 600, 5.0).with_texture_name("points file scene");
        let point_cloud =
            load_points_file(&points_file_path()).expect("failed to load points_6.txt");
        let ply_lod = load_ply_points().expect("failed to load stress-test PLY files");
        let scene = Scene::new(point_cloud, ply_lod);

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
        let tex_id = self.view.render_to_egui(&self.render_state, |cx, pass| {
            self.scene.render(cx, pass, self.colors)
        });

        egui::Panel::top("header").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("wiew point cloud");
                ui.separator();
                ui.label(format!("{} pts", self.scene.point_count));
                ui.separator();
                ui.label(format!("{} PLY pts", self.scene.ply_count));
                ui.separator();
                ui.label("MSAA:");
                egui::ComboBox::from_id_salt("msaa")
                    .selected_text(format!("{}x", self.view.samples()))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.view.samples() == 1, "Off (1x)")
                            .clicked()
                        {
                            self.view.set_samples(1);
                        }
                        if ui
                            .selectable_label(self.view.samples() == 4, "4x")
                            .clicked()
                        {
                            self.view.set_samples(4);
                        }
                    });
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
                ui.separator();
                lod_ui(ui, &mut self.scene.ply_lod);
            });

        let desired = self.view.central_panel_interactive(ui, tex_id);
        self.view.resize_from(desired);
    }
}

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

fn lod_ui(ui: &mut egui::Ui, lod: &mut PcLod) {
    ui.heading("PLY LOD");
    let mut draw_bounds = lod.draw_bounds();
    if ui
        .checkbox(&mut draw_bounds, "Draw selected boxes")
        .changed()
    {
        lod.set_draw_bounds(draw_bounds);
    }

    let mut proxy_diameter_px = lod.config().proxy_diameter_px;
    if ui
        .add(
            egui::Slider::new(&mut proxy_diameter_px, 0.5..=128.0)
                .logarithmic(true)
                .text("Proxy px"),
        )
        .changed()
    {
        lod.config_mut().proxy_diameter_px = proxy_diameter_px;
    }

    let mut points_per_pixel = lod.config().points_per_pixel;
    if ui
        .add(
            egui::Slider::new(&mut points_per_pixel, 0.001..=5.0)
                .logarithmic(true)
                .text("Pts/px"),
        )
        .changed()
    {
        lod.config_mut().points_per_pixel = points_per_pixel;
    }

    let mut node_lod_point_count = lod.config().node_lod_point_count;
    if ui
        .add(
            egui::Slider::new(&mut node_lod_point_count, 512..=65_536)
                .logarithmic(true)
                .text("Node pts"),
        )
        .changed()
    {
        lod.config_mut().node_lod_point_count = node_lod_point_count;
    }

    let stats = lod.stats();
    let drawn = stats.drawn_points();
    let percent = if stats.total_points > 0 {
        drawn as f64 * 100.0 / stats.total_points as f64
    } else {
        0.0
    };

    egui::Grid::new("pc_lod_stats")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Tree");
            ui.label(format!(
                "{} nodes / {} leaves",
                stats.total_nodes, stats.total_leaf_chunks
            ));
            ui.end_row();

            ui.label("Visited");
            ui.label(format!(
                "{} nodes ({} culled)",
                stats.visited_nodes, stats.culled_nodes
            ));
            ui.end_row();

            ui.label("Selected");
            ui.label(format!(
                "{} proxies / {} node LODs / {} leaves",
                stats.selected_proxy_points,
                stats.selected_node_lod_chunks,
                stats.selected_leaf_chunks
            ));
            ui.end_row();

            ui.label("Leaf LOD");
            ui.label(format!(
                "{} coarse / {} full",
                stats.selected_leaf_lod_chunks, stats.selected_full_leaf_chunks
            ));
            ui.end_row();

            ui.label("Drawn");
            ui.label(format!(
                "{} / {} pts ({percent:.1}%)",
                drawn, stats.total_points
            ));
            ui.end_row();
        });
}

struct Scene {
    bg: QuadBackground,
    grid: Grid,
    gizmo: TrackballGizmo,
    point_cloud: Mesh,
    point_count: usize,
    ply_lod: PcLod,
    ply_count: usize,
    point_pipeline: SplatPipeline,
    point_colored_pipeline: ColoredSplatPipeline,
    point_material: LitMaterial,
    point_colored_material: LitMaterial,
}

impl Scene {
    fn new(point_cloud: LoadedPointCloud, mut ply_lod: LoadedPointCloudLod) -> Self {
        let point_count = point_cloud.point_count;
        let point_material = LitMaterial::leios_blue().with_point_size(0.003);
        let point_colored_material = LitMaterial::leios_blue()
            .with_front_color([1.0, 1.0, 1.0, 1.0])
            .with_point_size(0.003);
        let ply_material = LitMaterial::leios_blue()
            .with_front_color([1.0, 0.3, 0.3, 1.0])
            .with_back_color([0.5, 0.1, 0.1, 1.0])
            .with_point_size(0.001);
        ply_lod.lod.set_material(ply_material);

        Self {
            bg: QuadBackground::new(QuadBackgroundConfig::default()),
            grid: Grid::new(10),
            gizmo: TrackballGizmo::new(),
            point_cloud: point_cloud.mesh,
            point_count,
            ply_lod: ply_lod.lod,
            ply_count: ply_lod.point_count,
            point_pipeline: SplatPipeline::new(),
            point_colored_pipeline: ColoredSplatPipeline::new(),
            point_material,
            point_colored_material,
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
        pass.draw(cx, &self.ply_lod);
        pass.draw(cx, &self.gizmo);
    }
}

struct LoadedPointCloud {
    mesh: Mesh,
    point_count: usize,
}

struct LoadedPointCloudLod {
    lod: PcLod,
    point_count: usize,
}

fn points_file_path() -> PathBuf {
    let cwd_path = PathBuf::from("points_6.txt");
    if cwd_path.exists() {
        return cwd_path;
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("points_6.txt")
}

fn load_points_file(path: &Path) -> Result<LoadedPointCloud, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut positions = Vec::<Position>::new();
    let mut normals = Vec::<Normal>::new();
    let mut colors = Vec::<Color>::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for (line_number, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let values = line
            .split_whitespace()
            .map(str::parse::<f32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("{}:{}: {err}", path.display(), line_number + 1))?;

        if values.len() < 10 {
            return Err(format!(
                "{}:{}: expected at least 10 values, got {}",
                path.display(),
                line_number + 1,
                values.len()
            )
            .into());
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

    if positions.is_empty() {
        return Err(format!("{} contains no points", path.display()).into());
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
    Ok(LoadedPointCloud {
        mesh: Mesh::new(positions)
            .with_normals(normals)
            .with_colors(colors),
        point_count,
    })
}

// ---------------------------------------------------------------------------
// PLY file loader for binary little-endian x y z nx ny nz vertex data.
// ---------------------------------------------------------------------------

fn ply_file_paths() -> Vec<PathBuf> {
    //["tot.ply", "tot_fiori.ply", "punti_fibbia.ply"]
    ["tot.ply"]
        .into_iter()
        .map(|file| {
            let cwd_path = PathBuf::from(file);
            if cwd_path.exists() {
                cwd_path
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(file)
            }
        })
        .collect()
}

fn load_ply_points() -> Result<LoadedPointCloudLod, Box<dyn Error>> {
    let (metadata_path, payloads_prefix) = ply_lod_cache_paths();
    if metadata_path.exists() {
        let metadata = fs::read(&metadata_path)?;
        let mut lod = PcLod::from_cache_metadata_bytes(&metadata)?;
        for chunk_index in 0.. {
            let path = payload_chunk_path(&payloads_prefix, chunk_index);
            if !path.exists() {
                break;
            }
            let payloads = fs::read(path)?;
            lod.apply_cache_payload_chunk(chunk_index * PC_LOD_PAYLOAD_CHUNK_SIZE, &payloads)?;
        }
        let point_count = lod.total_points();
        return Ok(LoadedPointCloudLod { lod, point_count });
    }

    let mut positions = Vec::<Position>::new();
    let mut normals = Vec::<Normal>::new();
    let mut colors = Vec::<Color>::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut loaded_files = 0usize;

    for path in ply_file_paths() {
        if !path.exists() {
            continue;
        }

        load_one_ply_points(
            &path,
            &mut positions,
            &mut normals,
            &mut colors,
            &mut min,
            &mut max,
        )?;
        loaded_files += 1;
    }

    if loaded_files == 0 {
        return Err("none of the stress-test PLY files were found".into());
    }

    if positions.is_empty() {
        return Err("PLY files contain no readable points".into());
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
    let lod = PcLod::from_streams(positions, normals, colors, Default::default())?;
    let parts = lod.to_cache_parts()?;
    fs::write(metadata_path, parts.metadata)?;
    write_payload_chunks(&payloads_prefix, &parts.payloads)?;
    Ok(LoadedPointCloudLod { lod, point_count })
}

fn ply_lod_cache_paths() -> (PathBuf, PathBuf) {
    (
        PathBuf::from("crates/wiew-web/dist/stress_test.meta.wlod"),
        PathBuf::from("crates/wiew-web/dist/stress_test.payloads"),
    )
}

fn payload_chunk_path(prefix: &Path, chunk_index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{chunk_index:05}.bin", prefix.display()))
}

fn write_payload_chunks(prefix: &Path, payloads: &[u8]) -> Result<(), Box<dyn Error>> {
    for (chunk_index, chunk) in payloads.chunks(PC_LOD_PAYLOAD_CHUNK_SIZE).enumerate() {
        fs::write(payload_chunk_path(prefix, chunk_index), chunk)?;
    }
    Ok(())
}

fn load_one_ply_points(
    path: &Path,
    positions: &mut Vec<Position>,
    normals: &mut Vec<Normal>,
    colors: &mut Vec<Color>,
    min: &mut [f32; 3],
    max: &mut [f32; 3],
) -> Result<(), Box<dyn Error>> {
    let mut file = File::open(path)?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)?;

    // Find end_header marker (in ASCII, even for binary PLY)
    let header_end = raw
        .windows(b"end_header\n".len())
        .position(|w| w == b"end_header\n")
        .ok_or_else(|| format!("{}: PLY file missing end_header", path.display()))?
        + b"end_header\n".len();

    // Read up to 100 vertices, each is 6 × f32_le = 24 bytes
    const VERTEX_STRIDE: usize = 24;
    const MAX_PLY_POINTS: usize = 1000_000_00;
    let data = &raw[header_end..];
    let count = (data.len() / VERTEX_STRIDE).min(MAX_PLY_POINTS);
    positions.reserve(count);
    normals.reserve(count);
    colors.reserve(count);

    for i in 0..count {
        let offset = i * VERTEX_STRIDE;
        let chunk = &data[offset..offset + VERTEX_STRIDE];

        let x = f32::from_le_bytes(chunk[0..4].try_into()?);
        let y = f32::from_le_bytes(chunk[4..8].try_into()?);
        let z = f32::from_le_bytes(chunk[8..12].try_into()?);
        let nx = f32::from_le_bytes(chunk[12..16].try_into()?);
        let ny = f32::from_le_bytes(chunk[16..20].try_into()?);
        let nz = f32::from_le_bytes(chunk[20..24].try_into()?);

        let position = [x, y, z];
        let normal = normalize([nx, ny, nz]);

        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
        positions.push(position);
        normals.push(normal);
        colors.push([1.0, 1.0, 1.0, 1.0]);
    }

    Ok(())
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

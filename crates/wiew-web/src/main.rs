mod loader;
mod scene;
mod toys;

use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wiew::{drawable::Drawable, egui_view::EguiView3d};

use crate::scene::Scene;

thread_local! {
    static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    static ADDITIONAL_OBJECTS: RefCell<Vec<Box<dyn Drawable>>> = const { RefCell::new(Vec::new()) };
}

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
        let loading_text = document
            .get_element_by_id("loading_text")
            .expect("element #loading_text not found");
        loading_text.remove();

        eframe::WebRunner::new()
            .start(
                document
                    .get_element_by_id("wiew_canvas")
                    .expect("canvas #wiew_canvas not found")
                    .dyn_into::<web_sys::HtmlCanvasElement>()
                    .expect("element is not a canvas"),
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

        let view =
            EguiView3d::new(device, queue, 900, 600, 5.0).with_texture_name("wiew web scene");

        CTX.with(|c| *c.borrow_mut() = Some(cc.egui_ctx.clone()));

        Self {
            view,
            scene: Scene::new(),
            render_state,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let tex_id = self
            .view
            .render_to_egui(&self.render_state, |cx, pass| self.scene.render(cx, pass));

        let desired = self.view.central_panel_interactive(ui, tex_id);
        self.view.resize_from(desired);
    }
}

//! The browser shell: web `Services` and the eframe web runner.

use figcraft_engine::Session;
use figcraft_ui_egui::{FigApp, Services};
use wasm_bindgen::JsCast as _;

const CANVAS_ID: &str = "figcraft_canvas";
const LOADING_ID: &str = "figcraft_loading";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let mut options = eframe::WebOptions::default();
        // `?webgl`: force the WebGL2 backend (troubleshooting, and headless browsers whose WebGPU
        // adapter request returns nothing).
        if query().contains("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| {
                    if let Some(rs) = &cc.wgpu_render_state {
                        log::info!("figcraft-web: wgpu backend {:?}", rs.adapter.get_info().backend);
                    }
                    let reduce_motion = web_sys::window().and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten());
                    let mut session = Session::new();
                    // Live sessions: the browser joins sessions a desktop app hosts; whatever arrives
                    // on the socket wakes the page for a frame.
                    let ctx = cc.egui_ctx.clone();
                    session.live.install(Box::new(figcraft_collab_net::NetLink::new(std::sync::Arc::new(move || ctx.request_repaint()))));
                    #[allow(unused_mut)]
                    let mut fig = FigApp::new(session, Box::new(WebServices { reduce_motion, ctx: cc.egui_ctx.clone() }));
                    #[cfg(feature = "icp-cloud")]
                    let cloud = {
                        let cloud = sovereign_cloud_ui::Cloud::new(sovereign_cloud_ui::AppSpec::FIG, cc.egui_ctx.clone());
                        let menu = cloud.menu_handle();
                        fig.file_menu = Some(Box::new(move |ui, has_document| {
                            figcraft_ui_egui::theme::on_dark(ui, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                                menu.ui(ui, has_document);
                            });
                            menu.has_pending()
                        }));
                        cloud
                    };
                    Ok(Box::new(WebShell {
                        fig,
                        #[cfg(feature = "icp-cloud")]
                        cloud,
                    }))
                }),
            )
            .await;
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>FigCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

struct WebShell {
    fig: FigApp,
    #[cfg(feature = "icp-cloud")]
    cloud: sovereign_cloud_ui::Cloud,
}

impl eframe::App for WebShell {
    // `FigApp::ui` runs its own per-frame logic first (fonts, theme), like the desktop app.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.fig.ui(ui);
        #[cfg(feature = "icp-cloud")]
        self.cloud.ui(ui.ctx(), &mut crate::cloud::Adapter::new(&mut self.fig.session));
        // Include every tab: navigating away must not discard another design or branch.
        let dirty = self.fig.session.docs.iter().any(|d| d.is_dirty() || d.text_edit.is_some() || d.interaction.is_some());
        if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            let value = if dirty { "true" } else { "false" };
            if root.get_attribute("data-craft-dirty").as_deref() != Some(value) {
                let _ = root.set_attribute("data-craft-dirty", value);
            }
        }
    }
}

/// Browser services. File dialogs return nothing for now (the trait's defaults). Copies go to the
/// system clipboard through egui's output; pastes arrive as paste events with their text (a page
/// can't read the clipboard when it likes).
struct WebServices {
    /// `prefers-reduced-motion: reduce` (it follows the setting while the page is open).
    reduce_motion: Option<web_sys::MediaQueryList>,
    ctx: egui::Context,
}

impl Services for WebServices {
    fn clipboard_set_text(&mut self, s: &str) {
        figcraft_ui_egui::egui_copy_text(&self.ctx, s);
    }
    fn clipboard_set_png(&mut self, w: u32, h: u32, rgba: &[u8]) {
        figcraft_ui_egui::egui_copy_png(&self.ctx, w, h, rgba);
    }
    fn open_url(&mut self, url: &str) {
        let Some(window) = web_sys::window() else { return };
        if let Err(e) = window.open_with_url_and_target(url, "_blank") {
            log::error!("couldn't open {url}: {e:?}");
        }
    }
    fn reduce_motion(&self) -> bool {
        self.reduce_motion.as_ref().is_some_and(web_sys::MediaQueryList::matches)
    }
    /// The browser's language (`navigator.language`).
    fn system_language(&self) -> Option<String> {
        web_sys::window().and_then(|w| w.navigator().language())
    }
    /// The Gamepad API's controllers with the standard mapping (its button order is the layout's;
    /// others are skipped). Browsers list controllers once one of their buttons is pressed.
    fn gamepad_held(&mut self) -> u32 {
        let Some(pads) = web_sys::window().and_then(|w| w.navigator().get_gamepads().ok()) else { return 0 };
        let mut held = 0;
        for pad in pads.iter().filter_map(|p| p.dyn_into::<web_sys::Gamepad>().ok()) {
            if pad.mapping() != web_sys::GamepadMappingType::Standard {
                continue;
            }
            for (i, b) in pad.buttons().iter().take(32).enumerate() {
                if b.dyn_into::<web_sys::GamepadButton>().is_ok_and(|b| b.pressed()) {
                    held |= 1 << i;
                }
            }
        }
        held
    }
}

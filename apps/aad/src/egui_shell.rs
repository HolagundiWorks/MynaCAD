//! egui/eframe desktop shell (non-Windows default; Windows with `--features egui` and `--egui`).

use aad_engine::Session;
use aad_ui_egui::{CadApp, Services, i18n};

use crate::{assistant, control_server, workspace};

struct App {
    cad: CadApp,
    workspace: workspace::Workspace,
    workspace_error: Option<String>,
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::native_menu::NativeMenu>,
}

impl App {
    fn persist_workspace(&mut self, final_attempt: bool) {
        let result = if final_attempt { self.workspace.save_on_exit(&self.cad.ui) } else { self.workspace.save_if_changed(&self.cad.ui) };
        if let Err(error) = result {
            if self.workspace_error.as_ref() != Some(&error) {
                eprintln!("O-Draft workspace: {error}");
                self.cad.set_status(format!("Workspace could not be saved: {error}"));
                self.workspace_error = Some(error);
            }
        } else {
            self.workspace_error = None;
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.native_menu.is_none() && std::env::var_os("AAD_NO_NATIVE_MENU").is_none() {
                self.native_menu = Some(crate::native_menu::NativeMenu::install(&mut self.cad));
            }
            if let Some(m) = &mut self.native_menu {
                m.poll(&mut self.cad, ctx);
            }
        }
        self.cad.logic(ctx);
        self.persist_workspace(false);
        if std::mem::take(&mut self.cad.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.cad.raw_input_hook(raw);
        aad_ui_egui::cmdline::capture_tab(ctx, raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);
        self.persist_workspace(false);
    }
    fn on_exit(&mut self) {
        self.persist_workspace(true);
    }
    /// Preferences (the interface theme) survive restarts; window and egui state are not kept.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(aad_ui_egui::PREFS_KEY, self.cad.prefs_json());
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// File format hooks for the engine (it stays I/O-agnostic).
pub fn install_io() {
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| aad_io::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    aad_engine::cmd::file::set_pdf_reader(|bytes, page| aad_io::pdfread::read_page(bytes, page).map_err(|e| e.to_string()));
    aad_engine::cmd::file::set_pdf_merge(|parts| aad_io::pdf::merge(parts).map_err(|e| e.to_string()));
    aad_engine::cmd::file::set_raster_reader(|bytes| {
        aad_io::raster::read_gray(bytes).map(|g| aad_engine::cmd::file::Raster { width: g.width, height: g.height, gray: g.gray })
    });
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter(i18n::t("Drawings (DWG, DXF)"), &["dwg", "dxf"])
                .add_filter(i18n::t("All files"), &["*"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| {
            filters_for_save(name)
                .iter()
                .fold(rfd::FileDialog::new().set_file_name(name), |d, (label, ext)| d.add_filter(i18n::t(label), &[*ext]))
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        assistant: Some(Box::new(assistant::Desktop::new())),
    }
}

/// File types for the Save dialog, the suggested name's type first: Windows adds the first
/// filter's extension, and macOS accepts only listed extensions, replacing any other with the
/// first one. DWG must be listed for a `.dwg` name to survive, and a drawing opened from DWG
/// then defaults to DWG; other names keep DXF first.
pub fn filters_for_save(name: &str) -> [(&'static str, &'static str); 5] {
    let mut filters = [("Drawing (DXF)", "dxf"), ("Drawing (DWG)", "dwg"), ("SVG", "svg"), ("PNG", "png"), ("PDF", "pdf")];
    let lower = name.to_ascii_lowercase();
    filters.sort_by_key(|(_, ext)| !lower.ends_with(&format!(".{ext}")));
    filters
}

const APP_ID: &str = "io.github.holagundiworks.aad";

#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/aad-macos-512.png");
#[cfg(not(target_os = "macos"))]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/aad-256.png");

fn app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).map_err(|e| log::warn!("app icon: {e}")).ok()
}

pub fn run(control_port: Option<u16>, sample: bool, files: Vec<String>) -> eframe::Result {
    install_io();
    let surface = aad_ui_egui::gpu::surface_config(std::env::var("AAD_VSYNC").ok().as_deref(), cfg!(all(unix, not(target_os = "macos"))));
    let mut options = eframe::NativeOptions {
        // The crosshair is drawn by the app: present without a queue on Linux/BSD, where X11
        // swapchains hold two frames back under vsync (`AAD_VSYNC=1` restores vsync).
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(surface),
        viewport: egui::ViewportBuilder::default()
            .with_title("O-Draft")
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_app_id(APP_ID),
        persist_window: false,
        ..Default::default()
    };
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 only (see graphics.rs).
    #[cfg(all(target_os = "windows", feature = "egui"))]
    crate::graphics::configure(&mut options, eframe::wgpu::Backends::from_env());
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping files from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; AAD_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("AAD_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    eframe::run_native(
        "AAD",
        options,
        Box::new(move |cc| {
            let mut app = CadApp::new(Session::empty(), services());
            let (workspace, restored, workspace_error) = workspace::Workspace::load();
            if let Some(ui) = restored {
                app.ui = ui;
            }
            if let Some(error) = &workspace_error {
                eprintln!("O-Draft workspace: {error}");
                app.set_status(error);
            }
            if let Some(prefs) = cc.storage.and_then(|s| s.get_string(aad_ui_egui::PREFS_KEY).or_else(|| s.get_string(aad_ui_egui::LEGACY_PREFS_KEY)))
            {
                app.load_prefs(&prefs);
            }
            app.integrated_titlebar = cfg!(target_os = "macos");
            if surface.present_mode == eframe::wgpu::PresentMode::AutoNoVsync {
                app.frame_cap = Some(aad_ui_egui::gpu::FrameCap::new(240));
            }
            if let Some(rs) = &cc.wgpu_render_state {
                app.set_wgpu(rs);
            }
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            if sample {
                let _ = app.run("ui.sample", serde_json::json!({}));
            }
            for f in &files {
                app.open_path(f);
            }
            if app.session.docs.is_empty() {
                app.session.new_drawing(false);
            }
            Ok(Box::new(App {
                cad: app,
                workspace,
                workspace_error,
                #[cfg(target_os = "macos")]
                native_menu: None,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::filters_for_save;

    #[test]
    fn save_dialog_offers_dwg_with_the_drawing_format_first() {
        let first = |name: &str| filters_for_save(name)[0].1;
        assert_eq!(first("Waco Winnelson Floorplan.dwg"), "dwg");
        assert_eq!(first("FLOORPLAN.DWG"), "dwg");
        assert_eq!(first("Drawing1.dxf"), "dxf");
        assert_eq!(first("Drawing1"), "dxf");
        assert_eq!(first("plan.pdf"), "pdf");
        for name in ["a.dwg", "b.dxf", "c"] {
            let exts: Vec<_> = filters_for_save(name).iter().map(|(_, e)| *e).collect();
            assert!(exts.contains(&"dwg") && exts.contains(&"dxf"), "{name}");
        }
    }
}

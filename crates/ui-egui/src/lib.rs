//! The AAD egui front end.
//!
//! A thin shell over [`aad_engine::Session`]: panels read engine state and act through
//! commands (`app.run(id, params)`) or the command line (`app.cmdline(text)`). Nothing here owns
//! drawing data. The layout follows the familiar CAD desktop: title and tool bar, file tabs,
//! Tool Sets on the left, the drawing area with its command line, Layers and Properties on the
//! right, and drafting toggles in the status bar. Paper layouts are not tabs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod assistant;
pub mod autocollapse;
pub mod blocks;
pub mod canvas;
pub mod chrome;
pub mod clipboard;
pub mod closing;
pub mod cmdinput;
pub mod cmdline;
pub mod context_menu;
pub mod contextbar;
pub mod control;
pub mod dialogs;
pub mod docking;

pub mod dyninput;
pub mod formdialog;
pub mod furniture;
pub mod gpu;
pub mod i18n;
pub mod icons;
pub mod layers;
pub mod managers;
pub mod menus;
pub mod palettes;
pub mod plotstyles;
pub mod plotwindow;
pub mod quick;
pub mod theme;
pub mod toolbars;
pub mod topbar;
pub mod ucsicon;
pub mod units_panel;
pub mod widgets;

use std::sync::mpsc::Receiver;

use aad_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse, PendingReply};

/// The host storage key for [`CadApp::prefs_json`].
pub const PREFS_KEY: &str = "aad.prefs";
/// The key older builds stored the preferences under (before the rename to AAD): read when the new one is empty.
pub const LEGACY_PREFS_KEY: &str = "cadcraft.prefs";

/// Persisted UI state (serde, so automation can read and set it).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub docking: docking::Workspace,

    pub show_palettes: bool,
    pub show_toolbar: bool,
    pub show_file_tabs: bool,
    pub show_status_bar: bool,
    pub show_command_line: bool,
    pub show_ucs_icon: bool,
    /// The Calculator and Pomodoro widgets ([`widgets`]).
    pub show_calculator: bool,
    pub show_pomodoro: bool,
    pub show_layer_list: bool,
    /// Keep the system cursor visible over the drawing area (a small crosshair at the centre of the
    /// drawn one) instead of hiding it. Screen magnifiers (Windows Magnifier "follow the mouse
    /// pointer"), screen readers, recorders and remote desktops track the system cursor and lose it
    /// when it is hidden. Off by default; `AAD_SYSTEM_CURSOR=1` turns it on at startup.
    pub system_cursor: bool,
    pub properties_all: bool,
    /// Show an in-window menu bar (platforms without a native menu bar, or on request).
    pub in_window_menu: bool,
    pub start_tab: bool,
    pub dialog: Option<String>,
    pub history_lines: usize,
    /// Interface theme: "system" (follow the OS light/dark appearance), "light" or "dark".
    pub theme: theme::ThemePref,
    /// The format Save As suggests for a drawing that has no file yet: "dxf" or "dwg".
    pub save_format: SaveFormat,
    /// Pro or Classic workspace.
    pub workspace: WorkspaceMode,
    /// Whether the palettes were open when Pro was entered, restored when Classic returns.
    pub classic_palettes: bool,
    /// Keep the side panels open. Unpinned, they collapse after `auto_collapse_ms` away
    /// from the pointer. This is the live value for the current workspace mode: Classic starts pinned,
    /// Pro starts unpinned (collapsed, with the edge strips), and each mode keeps its own ([`UiState::mode_slots`]).
    pub pin_panels: bool,
    /// Milliseconds before unpinned panels collapse.
    pub auto_collapse_ms: u32,
    /// The panel arrangement and pin state of the workspace mode that is *not* active, as saved when
    /// the mode was left (`{"docking": ..., "pin": bool}`), keyed by mode name. Kept as raw JSON so a
    /// layout that no longer reads (an older panel set) is dropped on its own and never invalidates
    /// the rest of the saved workspace. Switching modes swaps the live layout with this slot, so one
    /// mode never overwrites the other's arrangement.
    pub mode_slots: std::collections::BTreeMap<String, Value>,
    /// The tools on the first line of the top bar ([`topbar`]): command ids, `-` for a separator.
    pub top_bar: Vec<String>,
    /// The typography preset ([`aad_fonts::registry`]): the interface fonts follow it.
    pub font_preset: String,
}

/// The workspace. Pro hides the persistent toolbars and side palettes for a full-screen drafting
/// surface (command line and status bar stay); Classic shows the toolbars, the Properties panel
/// on the left and the Layers/Blocks panel on the right. Switching changes visibility only: the
/// drawing, selection, view and command history are untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceMode {
    #[default]
    Classic,
    Pro,
}

impl WorkspaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceMode::Classic => "classic",
            WorkspaceMode::Pro => "pro",
        }
    }

    pub fn parse(s: &str) -> Option<WorkspaceMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "classic" => Some(WorkspaceMode::Classic),
            "pro" => Some(WorkspaceMode::Pro),
            _ => None,
        }
    }
}

/// The default save format for new drawings, like AutoCAD's Options > Open and Save > "Save as".
/// A drawing opened from a file keeps that file's format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveFormat {
    /// AAD's native format, and the default.
    #[default]
    Dxf,
    Dwg,
}

impl SaveFormat {
    pub const ALL: [SaveFormat; 2] = [SaveFormat::Dxf, SaveFormat::Dwg];

    /// The file extension, which is also the saved name ("dxf" or "dwg").
    pub fn as_str(self) -> &'static str {
        match self {
            SaveFormat::Dxf => "dxf",
            SaveFormat::Dwg => "dwg",
        }
    }

    pub fn parse(s: &str) -> Option<SaveFormat> {
        SaveFormat::ALL.into_iter().find(|f| f.as_str().eq_ignore_ascii_case(s.trim().trim_start_matches('.')))
    }

    /// The name Save As suggests: a title that already has an extension (a drawing opened from a
    /// file) keeps it; an untitled drawing gets this format's extension.
    pub fn suggested_name(self, title: Option<&str>) -> String {
        match title.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) if std::path::Path::new(t).extension().is_some() => t.to_owned(),
            Some(t) => format!("{t}.{}", self.as_str()),
            None => format!("Drawing.{}", self.as_str()),
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            docking: docking::Workspace::default(),

            show_palettes: true,
            show_toolbar: true,
            show_file_tabs: true,
            show_status_bar: true,
            show_command_line: true,
            show_ucs_icon: true,
            show_calculator: false,
            show_pomodoro: false,
            show_layer_list: false,
            system_cursor: std::env::var("AAD_SYSTEM_CURSOR").is_ok_and(|v| !matches!(v.trim(), "" | "0" | "false" | "off")),
            properties_all: true,
            in_window_menu: !cfg!(target_os = "macos"),
            start_tab: false,
            dialog: None,
            history_lines: 2,
            theme: theme::ThemePref::default(),
            save_format: SaveFormat::default(),
            workspace: WorkspaceMode::default(),
            classic_palettes: true,
            pin_panels: true,
            auto_collapse_ms: 1500,
            mode_slots: std::collections::BTreeMap::new(),
            top_bar: topbar::default_items(),
            font_preset: aad_fonts::registry::DEFAULT_PRESET.into(),
        }
    }
}

/// Host services (file pickers, clipboard) injected by the app so this crate stays portable.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Option<String>>>,
    pub pick_save: Option<Box<dyn Fn(&str) -> Option<String>>>,
    /// The local assistant ([`assistant`]); `None` in builds without a language model (the web build).
    pub assistant: Option<Box<dyn assistant::Backend>>,
}

pub struct CadApp {
    pub session: Session,
    pub ui: UiState,
    pub services: Services,
    pub canvas: canvas::CanvasState,
    /// Pin and auto-collapse state of the side panels.
    pub collapse: autocollapse::CollapseState,
    /// The context bar ([`contextbar`]).
    pub ctx_bar: contextbar::State,
    /// What the command bar remembers about the assistant ([`assistant`]).
    pub assistant: assistant::Ui,
    /// State of the Calculator and Pomodoro widgets.
    pub widgets: widgets::WidgetState,
    pub cmd: cmdline::CmdLine,
    pub status: Option<(String, f64)>,
    pub integrated_titlebar: bool,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_shots: Vec<(u64, Option<String>, std::sync::mpsc::Sender<Value>, f64)>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    pub synthetic: Vec<egui::Event>,
    /// Caps the frame rate when presenting without vsync (see [`gpu::FrameCap`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub frame_cap: Option<gpu::FrameCap>,
    styled: bool,
    /// The font preset the interface fonts were built for (a change in `ui.font_preset` rebuilds them).
    fonts_applied: String,
    /// The last font install defined the named families (condensed, heading).
    fonts_named: bool,
    /// The theme last installed into egui (`None` until the first frame).
    shown_theme: Option<egui::Theme>,
    pub frame_ms: f64,
    pub quit_requested: bool,
    /// The app may close although drawings have unsaved changes (they were dealt with, or the
    /// quit came from the control channel, which never asks).
    pub quit_confirmed: bool,
    /// A close request waiting for "Save changes?" answers ([`closing`]).
    pub closing: Option<closing::Closing>,
}

impl CadApp {
    pub fn new(session: Session, services: Services) -> Self {
        CadApp {
            session,
            ui: UiState::default(),
            services,
            canvas: canvas::CanvasState::default(),
            collapse: autocollapse::CollapseState::default(),
            ctx_bar: contextbar::State::default(),
            assistant: assistant::Ui::default(),
            widgets: widgets::WidgetState::default(),
            cmd: cmdline::CmdLine::default(),
            status: None,
            integrated_titlebar: false,
            control_rx: None,
            pending_shots: Vec::new(),
            queued_shots: Vec::new(),
            shot_token: 0,
            synthetic: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            frame_cap: None,
            styled: false,
            fonts_applied: String::new(),
            fonts_named: false,
            shown_theme: None,
            frame_ms: 0.0,
            quit_requested: false,
            quit_confirmed: false,
            closing: None,
        }
    }

    /// Draw the canvas on the GPU with the app's wgpu render state (eframe's
    /// `CreationContext::wgpu_render_state`). Without it the canvas draws on the CPU.
    pub fn set_wgpu(&mut self, rs: &egui_wgpu::RenderState) {
        self.canvas.gpu = Some(gpu::install(rs));
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// Run a command programmatically (or a UI command).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if let Some(r) = menus::run_ui_command(self, id, &params) {
            return r;
        }
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        if let Ok(v) = &r
            && let Some(m) = v.get("message").and_then(Value::as_str)
        {
            for l in m.lines() {
                self.session.echo(l.to_string());
            }
        }
        if let Err(e) = &r {
            self.session.echo(e.clone());
        }
        r
    }

    /// Start a command as if typed (interactive when it has prompts).
    pub fn start(&mut self, name: &str) {
        if menus::run_ui_command(self, name, &Value::Null).is_some() {
            return;
        }
        if formdialog::open(self, name) {
            return;
        }
        if let Err(e) = self.session.start(name) {
            self.session.echo(e.to_string());
        }
    }

    /// Submit a line of command-line text. A refused line is echoed to the history; the error is
    /// also returned.
    pub fn cmdline(&mut self, text: &str) -> Option<String> {
        // The bar takes native commands and requests for the assistant alike: what the first word
        // resolves to decides (`cmdinput::classify`). Not while a command is waiting for input.
        if self.session.running.is_none() {
            let kind = cmdinput::classify(text);
            if cmdinput::is_ai(kind) && (assistant::available(self) || kind == cmdinput::Kind::Reference) {
                return assistant::ask(self, text);
            }
        }
        if self.session.running.is_none() {
            let first = text.split_whitespace().next().unwrap_or("");
            // `cmd {json}` is a programmatic call: the engine runs it with those parameters.
            let json_form = text.trim_start().get(first.len()..).is_some_and(|rest| rest.trim_start().starts_with('{'));
            if !first.is_empty() && !json_form && menus::run_ui_command(self, &first.to_ascii_lowercase(), &Value::Null).is_some() {
                return None;
            }
        }
        let e = self.session.cmdline(text).err()?.to_string();
        self.session.echo(e.clone());
        Some(e)
    }

    /// Whether the persistent toolbars are drawn: the Tool Bar switch is on and the workspace is Classic.
    pub fn toolbars_shown(&self) -> bool {
        self.ui.show_toolbar && self.ui.workspace == WorkspaceMode::Classic
    }

    /// The theme the interface shows now.
    pub fn shown_theme(&self) -> egui::Theme {
        self.shown_theme.unwrap_or_else(|| self.ui.theme.resolve())
    }

    /// Preferences kept across restarts, as JSON for the host's storage ([`PREFS_KEY`]).
    pub fn prefs_json(&self) -> String {
        json!({
            "theme": self.ui.theme.as_str(),
            "saveFormat": self.ui.save_format.as_str(),
            "workspace": self.ui.workspace.as_str(),
            "pinPanels": self.ui.pin_panels,
            "autoCollapseMs": self.ui.auto_collapse_ms,
            "classicPalettes": self.ui.classic_palettes,
        })
        .to_string()
    }

    /// Restore preferences saved by [`Self::prefs_json`]; unknown or malformed values are ignored.
    pub fn load_prefs(&mut self, json: &str) {
        let Ok(v) = serde_json::from_str::<Value>(json) else { return };
        if let Some(p) = v.get("pinPanels").and_then(Value::as_bool) {
            self.ui.pin_panels = p;
        }
        if let Some(ms) = v.get("autoCollapseMs").and_then(Value::as_u64) {
            self.ui.auto_collapse_ms = ms.clamp(250, 30_000) as u32;
        }
        if let Some(p) = v.get("classicPalettes").and_then(Value::as_bool) {
            self.ui.classic_palettes = p;
        }
        if let Some(w) = v.get("workspace").and_then(Value::as_str).and_then(WorkspaceMode::parse) {
            self.ui.workspace = w;
            if w == WorkspaceMode::Pro {
                self.ui.show_palettes = false;
            }
        }
        if let Some(t) = v.get("theme").and_then(Value::as_str).and_then(theme::ThemePref::parse) {
            self.ui.theme = t;
        }
        if let Some(f) = v.get("saveFormat").and_then(Value::as_str).and_then(SaveFormat::parse) {
            self.ui.save_format = f;
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), now_ms()));
    }

    pub fn open_path(&mut self, path: &str) {
        let r = self.run("open", json!({ "path": path }));
        self.opened(r);
    }

    /// The largest file [`Self::open_bytes`] opens. A drawing opened from its contents (the
    /// browser, where files have no path) is held in memory several times over while it is decoded,
    /// so it is capped below the DWG reader's own limit instead of running out of memory.
    pub const MAX_OPEN_BYTES: u64 = 256 << 20;

    /// Why a `len`-byte file called `name` is too large for [`Self::open_bytes`], if it is.
    pub fn open_size_error(name: &str, len: u64) -> Option<String> {
        (len > Self::MAX_OPEN_BYTES).then(|| {
            format!("Open: {name} is {:.1} MB, larger than the {} MB MynaCAD opens in the browser", len as f64 / 1048576.0, Self::MAX_OPEN_BYTES >> 20)
        })
    }

    /// Open a drawing from its contents where there is no path to open (on the web: the file
    /// picker, a dropped file). Runs OPEN with `{data, name}` as [`Self::open_path`] runs it with
    /// `{path}`; failures show on the command line and in the status bar.
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) {
        if let Some(e) = Self::open_size_error(name, bytes.len() as u64) {
            return self.open_failed(e);
        }
        let data = aad_engine::cmd::file::base64_encode(bytes);
        let r = self.run("open", json!({ "data": data, "name": name }));
        self.opened(r);
    }

    /// Report a file that could not be opened before OPEN ran (unreadable, too large) as a failed
    /// OPEN is reported: on the command line and in the status bar.
    pub fn open_failed(&mut self, e: impl Into<String>) {
        let e = e.into();
        self.session.echo(e.clone());
        self.set_status(e);
    }

    fn opened(&mut self, r: Result<Value, String>) {
        if let Err(e) = r {
            self.set_status(e);
        } else {
            self.ui.start_tab = false;
            self.canvas.zoom_pending = true;
        }
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        // Closing the window (title bar, File ▸ Exit, Cmd+Q) with unsaved changes asks first.
        if ctx.input(|i| i.viewport().close_requested()) && self.request_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let mut installed_now = false;
        if self.styled && self.fonts_applied != self.ui.font_preset {
            self.fonts_named = theme::install_fonts(ctx, &self.ui.font_preset);
            installed_now = true;
            self.fonts_applied = self.ui.font_preset.clone();
        }
        if !self.styled {
            self.fonts_named = theme::install_fonts(ctx, &self.ui.font_preset);
            installed_now = true;
            self.fonts_applied = self.ui.font_preset.clone();
            self.styled = true;
            // Classic always starts with its panels open, whatever a saved layout says.
            if self.ui.workspace == WorkspaceMode::Classic {
                self.ui.show_palettes = true;
            }
        }
        // The named font families are usable from the frame after the fonts were installed.
        // Whether this context has the named families is remembered in the context itself, so a
        // different context (tests make several) never uses a family it does not have.
        let installed_id = egui::Id::new("aad_fonts_installed");
        let ready_id = egui::Id::new("aad_named_fonts_ready");
        if installed_now && self.fonts_named {
            ctx.data_mut(|d| d.insert_temp(installed_id, true));
        }
        let installed_here = ctx.data(|d| d.get_temp::<bool>(installed_id)).unwrap_or(false);
        let mut ready_here = ctx.data(|d| d.get_temp::<bool>(ready_id)).unwrap_or(false);
        if installed_here && !installed_now && !ready_here {
            ctx.data_mut(|d| d.insert_temp(ready_id, true));
            ready_here = true;
            self.shown_theme = None; // re-apply the styles that use them
        }
        theme::set_named_ready(ready_here);
        let shown = self.ui.theme.resolve();
        theme::set_active(shown);
        if self.shown_theme != Some(shown) {
            theme::apply(ctx, shown);
            self.shown_theme = Some(shown);
        }
        if let Some(req) = self.session.take_plot_request() {
            menus::finish_plot(self, req);
        }
        self.drain_control(ctx);
        assistant::poll(self, ctx);
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        menus::shortcuts(self, ctx);
        #[cfg(not(target_arch = "wasm32"))]
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                self.open_path(&p);
            }
        }
    }

    /// Wait for the frame cap (if any), then inject synthetic events (one pointer event per frame).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(cap) = &mut self.frame_cap {
            cap.wait();
        }
        if self.synthetic.is_empty() {
            return;
        }
        let n = match self.synthetic.first() {
            Some(egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }) => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        if let Some(egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. }) = self.synthetic.first() {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        let n = n.min(self.synthetic.len());
        raw.events.extend(self.synthetic.drain(..n));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.draw(ui);
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let t0 = now_ms();
        control::begin_frame(ui.ctx());
        let t = theme::Tokens::get();
        let classic_order = !self.integrated_titlebar;
        if classic_order && self.ui.in_window_menu {
            menus::menu_bar(self, ui);
        }
        chrome::title_and_toolbar(self, ui);
        if !classic_order && self.ui.in_window_menu {
            menus::menu_bar(self, ui);
        }
        let has_doc = !self.session.docs.is_empty() && !self.ui.start_tab;
        // The top bar: its first line (tools you choose, Classic only), then its second line (layer,
        // floor, drawing space), above the file tabs.
        if self.toolbars_shown() {
            topbar::show(self, ui);
        }
        if has_doc {
            contextbar::show(self, ui);
        }
        if self.ui.show_status_bar {
            chrome::status_bar(self, ui);
        }
        if self.ui.show_file_tabs {
            chrome::file_tabs(self, ui);
        }

        if has_doc && self.toolbars_shown() {
            toolbars::draw_toolbar(self, ui);
            toolbars::modify_toolbar(self, ui);
        }
        let mut dock = egui::Rect::NOTHING;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
            dock = ui.max_rect();
            if has_doc {
                docking::show(self, ui);
            } else {
                chrome::start_page(self, ui);
            }
        });
        if has_doc {
            autocollapse::run(self, ui.ctx(), dock);
        }
        dialogs::show(self, ui.ctx());
        widgets::show(self, ui.ctx());
        closing::prompt(self, ui.ctx());
        self.frame_ms = now_ms() - t0;
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            // Timed out while queued: the client was told it did not run (#345).
            if !req.begin() {
                continue;
            }
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    self.shot_token += 1;
                    let token = self.shot_token;
                    self.queued_shots.push((token, now_ms() + 120.0, 0));
                    self.pending_shots.push((token, path, reply, now_ms() + 8000.0));
                }
            }
            ctx.request_repaint();
        }
        self.control_rx = Some(rx);
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.pending_shots.iter().position(|(t, ..)| *t == token) {
                let (_, path, reply, _) = self.pending_shots.remove(i);
                let _ = reply.send(control::save_screenshot(&image, path.as_deref()));
            }
        }
        let now = now_ms();
        self.pending_shots.retain(|(_, _, reply, deadline)| {
            if now < *deadline {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": "no frame was presented (screen locked or window hidden); use ui.render"}));
            false
        });
    }
}

pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theme::{ThemePref, Tokens};

    fn app() -> CadApp {
        CadApp::new(Session::empty(), Services::default())
    }

    /// One frame of `logic` with the OS reporting `system` as its appearance.
    fn frame(app: &mut CadApp, ctx: &egui::Context, system: Option<egui::Theme>) {
        let raw = egui::RawInput { system_theme: system, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.logic(ui.ctx()));
        out.textures_delta.clear();
    }

    #[test]
    fn theme_choice_persists_and_drives_tokens() {
        let mut a = app();
        assert_eq!(a.ui.theme, ThemePref::Dark, "default keeps today's look");
        assert!(a.run("ui.theme", json!({ "theme": "Purple" })).is_err());
        assert_eq!(a.run("ui.theme", json!({ "theme": "light" })).ok(), Some(json!({ "theme": "light" })));
        // Saved and restored by the host.
        let mut b = app();
        b.load_prefs(&a.prefs_json());
        assert_eq!(b.ui.theme, ThemePref::Light);
        b.load_prefs("not json");
        b.load_prefs(r#"{"theme": "sepia"}"#);
        assert_eq!(b.ui.theme, ThemePref::Light, "bad prefs are ignored");
        // A fixed choice ignores the OS; the tokens and egui follow the choice.
        let ctx = egui::Context::default();
        frame(&mut b, &ctx, Some(egui::Theme::Dark));
        assert_eq!(Tokens::get(), Tokens::LIGHT);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        // The menu items reach the same setting; so does `ui.set` (the UiState JSON).
        b.start("ui.theme.dark");
        frame(&mut b, &ctx, Some(egui::Theme::Light));
        assert_eq!(Tokens::get(), Tokens::DARK);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        // One toggle flips between the two themes, and the drawing area follows: black or white.
        b.start("ui.theme.toggle");
        frame(&mut b, &ctx, None);
        assert_eq!((b.ui.theme, Tokens::get().canvas), (ThemePref::Light, egui::Color32::WHITE));
        b.start("ui.theme.toggle");
        frame(&mut b, &ctx, None);
        assert_eq!((b.ui.theme, Tokens::get().canvas), (ThemePref::Dark, egui::Color32::BLACK));
    }

    #[test]
    fn default_save_format_persists_and_names_new_drawings() {
        let mut a = app();
        assert_eq!(a.ui.save_format, SaveFormat::Dxf, "default keeps DXF, AAD's native format");
        assert!(a.run("ui.saveformat", json!({ "format": "pdf" })).is_err());
        assert_eq!(a.run("ui.saveformat", json!({ "format": "DWG" })).ok(), Some(json!({ "saveFormat": "dwg" })));
        assert_eq!(a.run("ui.saveformat", json!({})).ok(), Some(json!({ "saveFormat": "dwg" })), "no format: report it");
        // Saved and restored by the host; bad values are ignored.
        let mut b = app();
        b.load_prefs(&a.prefs_json());
        assert_eq!(b.ui.save_format, SaveFormat::Dwg);
        b.load_prefs(r#"{"saveFormat": "svg"}"#);
        assert_eq!(b.ui.save_format, SaveFormat::Dwg);
        // The menu items reach the same setting, and resetting the palettes keeps it.
        b.start("ui.saveformat.dxf");
        assert_eq!(b.ui.save_format, SaveFormat::Dxf);
        b.start("ui.saveformat.dwg");
        b.start("ui.resetpalettes");
        assert_eq!(b.ui.save_format, SaveFormat::Dwg);
        // Untitled drawings get the format's extension; files keep their own.
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("Drawing1")), "Drawing1.dwg");
        assert_eq!(SaveFormat::Dxf.suggested_name(Some("Drawing1")), "Drawing1.dxf");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("Floorplan.dxf")), "Floorplan.dxf");
        assert_eq!(SaveFormat::Dxf.suggested_name(Some("Floorplan.DWG")), "Floorplan.DWG");
        assert_eq!(SaveFormat::Dwg.suggested_name(None), "Drawing.dwg");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("  ")), "Drawing.dwg");
        let ui: UiState = serde_json::from_value(json!({ "saveFormat": "dwg" })).unwrap_or_default();
        assert_eq!(ui.save_format, SaveFormat::Dwg);
    }
}

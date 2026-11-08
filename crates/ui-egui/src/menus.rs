//! Menus (in-window menu bar; the native macOS menu is built from the same tree), UI-only
//! commands and keyboard shortcuts.

use serde_json::{Value, json};

use crate::CadApp;

/// The top-level menus, in order.
pub const MENUS: &[&str] = &["File", "Edit", "View", "Insert", "Format", "Tools", "Draw", "Dimension", "Modify", "Architecture", "Window", "Help"];

/// UI-only commands: (id, label, menu path, shortcut).
pub const UI_COMMANDS: &[(&str, &str, &[&str], Option<&str>)] = &[
    ("ui.dock", "Arrange Panel", &[], None),
    ("ui.dock.reset", "Reset Panel Layout", &["Window", "Reset Panel Layout"], None),
    ("ui.open", "Open...", &[], Some("Cmd+O")),
    ("ui.saveas", "Save As...", &[], None),
    ("ui.sample", "Open Sample Drawing", &["Help", "Open Sample Drawing"], None),
    ("ui.panel.properties", "Properties Inspector", &["Window", "Properties Inspector"], Some("Cmd+1")),
    ("ui.panel.layers", "Layers Palette", &["Window", "Layers Palette"], None),
    ("ui.toggle.systemcursor", "Show System Cursor", &["View", "Accessibility", "Show System Cursor"], None),
    ("ui.toggle.toolbar", "Tool Bar", &["Window", "Tool Bar"], None),
    ("ui.toggle.filetabs", "File Tab", &["Window", "File Tab"], None),
    ("ui.toggle.statusbar", "Status Bar", &["Window", "Status Bar"], None),
    ("ui.toggle.cmdline", "Command Line", &["Window", "Command Line"], Some("Cmd+9")),
    ("ui.toggle.pinpanels", "Pin Side Panels", &["Window", "Pin Side Panels"], None),
    ("ui.toggle.ucsicon", "UCS Icon", &["View", "UCS Icon", "On"], None),
    ("ui.toggle.calculator", "Calculator", &["Tools", "Calculator"], None),
    ("ui.toggle.pomodoro", "Pomodoro Timer", &["Tools", "Pomodoro Timer"], None),
    ("ui.calculator", "Calculator Keys", &[], None),
    ("ui.pomodoro", "Pomodoro Control", &[], None),
    ("ui.theme.toggle", "Toggle Light/Dark Theme", &["View", "Toggle Light/Dark Theme"], None),
    ("ui.workspace", "Workspace", &[], None),
    ("ui.workspace.classic", "Classic Workspace", &["Window", "Workspace", "Classic"], None),
    ("ui.workspace.pro", "Pro Workspace", &["Window", "Workspace", "Pro"], None),
    ("ui.workspace.reset", "Reset Workspace", &["Window", "Workspace", "Reset This Workspace"], None),
    ("resetui", "Reset Toolbars and Menus", &["View", "Reset Toolbars and Menus"], Some("Cmd+Shift+F11")),
    ("ui.dialog.customizetop", "Customize Top Bar...", &["View", "Customize Top Bar..."], None),
    ("ui.topbar.set", "Set Top Bar Tools", &[], None),
    ("ui.topbar.get", "Top Bar Tools", &[], None),
    ("ui.workspace.toggle", "Toggle Pro/Classic Workspace", &["Window", "Toggle Pro/Classic Workspace"], Some("Ctrl+F11")),
    ("ui.theme.light", "Light", &[], None),
    ("ui.theme.dark", "Dark", &[], None),
    ("ui.theme", "Interface Theme", &[], None),
    ("ui.fonts.engineering", "Engineering", &["View", "Font Preset", "Engineering"], None),
    ("ui.fonts.editorial", "Architectural Editorial", &["View", "Font Preset", "Architectural Editorial"], None),
    ("ui.fonts.multilingual", "Multilingual Documentation", &["View", "Font Preset", "Multilingual Documentation"], None),
    ("ui.fonts.compact", "Compact Interface", &["View", "Font Preset", "Compact Interface"], None),
    ("ui.fonts.drafting", "Drafting Utility", &["View", "Font Preset", "Drafting Utility"], None),
    ("ui.fonts", "Interface Font Preset", &[], None),
    ("ui.saveformat.dxf", "DXF", &["File", "Default Save Format", "DXF"], None),
    ("ui.saveformat.dwg", "DWG", &["File", "Default Save Format", "DWG"], None),
    ("ui.saveformat", "Default Save Format", &[], None),
    ("ui.toggle.menubar", "In-window Menu Bar", &["Window", "In-window Menu Bar"], None),
    ("ui.hidepalettes", "Hide Palettes", &["Window", "Hide Palettes"], None),
    ("ui.resetpalettes", "Reset Palettes", &["Window", "Reset Palettes"], None),
    ("ui.start", "Start", &["Window", "Start"], None),
    ("ui.dialog.layers", "Layer Properties Manager", &["Window", "Layers"], None),
    ("ui.dialog.blocks", "Blocks", &["Window", "Blocks"], None),
    ("ui.dialog.qselect", "Quick Select...", &[], None),
    ("ui.dialog.parameters", "Parameters Manager", &["Window", "Parameters Manager"], None),
    ("ui.dialog.dsettings", "Drafting Settings...", &[], None),
    ("ui.dialog.about", "About Tren", &["Help", "About Tren"], None),
    ("ui.dialog.commands", "Command Reference", &["Help", "Tren Help"], Some("F1")),
    ("ui.toggle.history", "Command History", &["Window", "Command History"], Some("F2")),
    ("ui.history.clear", "Clear Command History", &["Window", "Clear Command History"], None),
    ("ui.cmdline.lines", "Command Line History Lines", &[], None),
    ("ui.noop", "", &[], None),
    ("ui.quit", "Quit Tren", &[], Some("Cmd+Q")),
];

pub fn is_ui_command(id: &str) -> bool {
    UI_COMMANDS.iter().any(|c| c.0 == id)
}

/// Run a UI-only command. `None` if `id` isn't one.
pub fn run_ui_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    run_ui_command_inner(app, id, params)
}

fn run_ui_command_inner(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(r) = crate::widgets::route(app, id, params) {
        return Some(r);
    }
    if let Some(r) = crate::managers::route(app, id, params) {
        return Some(r);
    }
    if matches!(id, "ui.dock" | "ui.dock.reset") {
        return Some(crate::docking::command(app, id == "ui.dock.reset", params));
    }
    if matches!(id, "ui.panel.properties" | "ui.panel.layers") {
        let panel = if id == "ui.panel.properties" { "properties" } else { "layers" };
        return Some(crate::docking::command(app, false, &json!({"operation":"open", "panel":panel})));
    }
    if let Some(r) = crate::plotstyles::route(app, id, params) {
        return Some(r);
    }
    let toggle = |b: &mut bool, p: &Value| {
        *b = p.get("on").and_then(Value::as_bool).unwrap_or(!*b);
    };
    // When a file command should ask for a file. The `ui.*` commands are the pickers; the engine's
    // OPEN/SAVEAS/QSAVE ask only when typed or chosen from a menu (no parameters), so JSON calls
    // never open a picker.
    let no_path =
        if id.starts_with("ui.") { params.is_null() || (params.get("path").is_none() && params.get("data").is_none()) } else { params.is_null() };
    let r = match id {
        "ui.open" | "open" if no_path => {
            let picked = app.services.pick_open.as_ref().and_then(|f| f());
            if let Some(p) = picked {
                app.open_path(&p);
            }
            Ok(Value::Null)
        }
        // RECOVER from the menu or typed: pick the file. Without a picker the engine asks for the
        // file name on the command line.
        "recover" if no_path && app.services.pick_open.is_some() => {
            if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                let r = app.run("recover", json!({ "path": p }));
                app.opened(r);
            }
            Ok(Value::Null)
        }
        // Batch Plot (menu or typed): choose what to plot in the window; the command itself runs
        // from the window's Plot button, never from a bare menu click.
        "plot.batch" if params.is_null() => {
            crate::plotwindow::open(app);
            Ok(Value::Null)
        }
        // Open Project (menu or typed): pick the project file or its main drawing.
        "project.open" if params.is_null() && app.services.pick_open.is_some() => {
            if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                let r = app.run("project.open", json!({ "path": p }));
                app.opened(r);
            }
            Ok(Value::Null)
        }
        // Import from AQC (menu or typed): pick the file, then show what would change. Nothing is
        // applied until the window's Apply button (which runs `bridge.import` with `apply`).
        "bridge.import" if params.is_null() && app.services.pick_open.is_some() => {
            if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                crate::widgets::start_bridge_import(app, &p);
            }
            Ok(Value::Null)
        }
        "ui.saveas" | "saveas" if no_path => {
            let title = app.session.state().map(|s| s.title.clone()).ok();
            let name = app.ui.save_format.suggested_name(title.as_deref());
            if let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(&name)) {
                return Some(app.session.execute("saveas", &json!({ "path": p })).map_err(|e| e.to_string()));
            }
            Ok(Value::Null)
        }
        "qsave" if no_path && app.session.state().is_ok_and(|s| s.path.is_none()) => return run_ui_command(app, "ui.saveas", &Value::Null),
        // Menu, toolbar, Cmd+P or typed: ask where to save the PDF. JSON calls (scripts, control
        // channel, MCP) always carry params, never get here and never open a dialog.
        // In model space it first asks what to plot: two corners of a window (or Extents, or
        // Display); `finish_plot` then asks where to save. A layout is plotted as it is.
        "plot" | "print" | "exportpdf"
            if params.is_null() && app.session.running.is_none() && app.session.state().is_ok_and(|s| matches!(s.space, aad_doc::Space::Model)) =>
        {
            app.start("plot.window");
            Ok(Value::Null)
        }
        "plot" | "print" | "exportpdf" if params.is_null() && app.session.state().is_ok() => {
            let pick = app.services.pick_save.as_ref()?;
            let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
            let Some(path) = pick(&format!("{}.pdf", stem.unwrap_or_else(|| "Drawing".into()))) else { return Some(Ok(Value::Null)) };
            let cmd = if id == "exportpdf" { "exportpdf" } else { "plot" };
            let r = app.session.execute(cmd, &json!({ "path": path })).map_err(|e| e.to_string());
            app.session.echo(match &r {
                Ok(_) => format!("Plotted to {path}"),
                Err(e) => e.clone(),
            });
            r
        }
        "ui.sample" => {
            let d = aad_engine::sample::default_sample();
            app.session.open_drawing(d, "Bracket", None);
            app.canvas.zoom_pending = true;
            app.ui.start_tab = false;
            Ok(Value::Null)
        }
        "ui.toggle.pinpanels" => {
            toggle(&mut app.ui.pin_panels, params);
            Ok(Value::Null)
        }
        "ui.toggle.palettes" => {
            toggle(&mut app.ui.show_palettes, params);
            Ok(Value::Null)
        }
        "ui.toggle.toolbar" => {
            toggle(&mut app.ui.show_toolbar, params);
            Ok(Value::Null)
        }
        "ui.toggle.filetabs" => {
            toggle(&mut app.ui.show_file_tabs, params);
            Ok(Value::Null)
        }
        "ui.toggle.statusbar" => {
            toggle(&mut app.ui.show_status_bar, params);
            Ok(Value::Null)
        }
        "ui.toggle.cmdline" => {
            toggle(&mut app.ui.show_command_line, params);
            Ok(Value::Null)
        }
        "ui.toggle.systemcursor" => {
            toggle(&mut app.ui.system_cursor, params);
            Ok(Value::Null)
        }
        "ui.toggle.ucsicon" => {
            toggle(&mut app.ui.show_ucs_icon, params);
            Ok(Value::Null)
        }
        "ui.toggle.menubar" => {
            toggle(&mut app.ui.in_window_menu, params);
            Ok(Value::Null)
        }
        "ui.workspace.classic" => Ok(set_workspace(app, crate::WorkspaceMode::Classic)),
        "ui.workspace.pro" => Ok(set_workspace(app, crate::WorkspaceMode::Pro)),
        "ui.workspace.reset" | "resetui" => Ok(reset_workspace(app)),
        "ui.workspace.toggle" => {
            let next = if app.ui.workspace == crate::WorkspaceMode::Pro { crate::WorkspaceMode::Classic } else { crate::WorkspaceMode::Pro };
            Ok(set_workspace(app, next))
        }
        // `{"mode": "pro"|"classic"}` sets the workspace; without it, reports it.
        "ui.workspace" => match params.get("mode").and_then(Value::as_str) {
            Some(name) => match crate::WorkspaceMode::parse(name) {
                Some(mode) => Ok(set_workspace(app, mode)),
                None => Err(format!("unknown workspace \"{name}\" (pro or classic)")),
            },
            None => Ok(json!({ "workspace": app.ui.workspace.as_str() })),
        },
        "ui.theme.toggle" => set_theme(app, app.ui.theme.toggled()),
        "ui.theme.light" | "ui.theme.dark" => {
            let pref = crate::theme::ThemePref::parse(id.trim_start_matches("ui.theme.")).unwrap_or_default();
            set_theme(app, pref)
        }
        // `{"preset": id}` sets the typography preset of the interface; without it, reports it.
        "ui.fonts.engineering" | "ui.fonts.editorial" | "ui.fonts.multilingual" | "ui.fonts.compact" | "ui.fonts.drafting" => {
            set_font_preset(app, id.trim_start_matches("ui.fonts."))
        }
        "ui.fonts" => match params.get("preset").and_then(Value::as_str) {
            Some(name) => set_font_preset(app, name),
            None => Ok(fonts_json(app)),
        },
        // `{"theme": "light"|"dark"}` sets the choice; without it, reports it.
        "ui.theme" => match params.get("theme").and_then(Value::as_str) {
            Some(name) => match crate::theme::ThemePref::parse(name) {
                Some(pref) => set_theme(app, pref),
                None => Err(format!("unknown theme \"{name}\" (light or dark)")),
            },
            None => Ok(theme_json(app)),
        },
        "ui.saveformat.dxf" | "ui.saveformat.dwg" => {
            let format = crate::SaveFormat::parse(id.trim_start_matches("ui.saveformat.")).unwrap_or_default();
            Ok(set_save_format(app, format))
        }
        // `{"format": "dxf"|"dwg"}` sets the default save format for new drawings; without it,
        // reports it.
        "ui.saveformat" => match params.get("format").and_then(Value::as_str) {
            Some(name) => match crate::SaveFormat::parse(name) {
                Some(format) => Ok(set_save_format(app, format)),
                None => Err(format!("unknown save format \"{name}\" (dxf or dwg)")),
            },
            None => Ok(json!({ "saveFormat": app.ui.save_format.as_str() })),
        },
        "ui.hidepalettes" => {
            app.ui.show_palettes = false;
            Ok(Value::Null)
        }
        "ui.resetpalettes" => {
            let (menu, theme, save_format) = (app.ui.in_window_menu, app.ui.theme, app.ui.save_format);
            app.ui = crate::UiState { in_window_menu: menu, theme, save_format, ..Default::default() };
            Ok(Value::Null)
        }
        "ui.start" => {
            app.ui.start_tab = true;
            Ok(Value::Null)
        }
        // The expanded command history (AutoCAD's text window; TEXTSCR opens it).
        "ui.toggle.history" | "textscr" => {
            let open = json!({"on": true});
            toggle(&mut app.cmd.expanded, if id == "textscr" { &open } else { params });
            Ok(json!({ "on": app.cmd.expanded }))
        }
        // Forget the command log shown above the bar and the commands typed so far (Up/Down).
        "ui.history.clear" => {
            app.session.log.clear();
            app.cmd.history.clear();
            app.cmd.history_pos = None;
            Ok(Value::Null)
        }
        // `{"lines": n}`: how many history lines show above the command line (0–12); without it, reports it.
        "ui.cmdline.lines" => {
            if let Some(n) = params.get("lines").and_then(Value::as_u64) {
                app.ui.history_lines = n.min(crate::cmdline::MAX_LINES as u64) as usize;
            }
            Ok(json!({ "lines": app.ui.history_lines }))
        }
        "ui.topbar.set" => crate::topbar::command(app, params, true),
        "ui.topbar.get" => crate::topbar::command(app, params, false),
        "ui.dialog.layers"
        | "ui.dialog.blocks"
        | "ui.dialog.dsettings"
        | "ui.dialog.about"
        | "ui.dialog.commands"
        | "ui.dialog.qselect"
        | "ui.dialog.customizetop"
        | "ui.dialog.parameters" => {
            app.ui.dialog = Some(id.trim_start_matches("ui.dialog.").to_string());
            Ok(Value::Null)
        }
        "ui.dialog.close" => {
            app.ui.dialog = None;
            Ok(Value::Null)
        }
        "ui.quit" => {
            app.quit_requested = true;
            Ok(Value::Null)
        }
        "ui.noop" => Ok(Value::Null),
        // From a menu or typed, closing asks about unsaved changes; JSON calls close without asking.
        "close" if params.is_null() => {
            app.request_close(None);
            Ok(Value::Null)
        }
        "closeall" if params.is_null() => {
            app.request_close_all();
            Ok(Value::Null)
        }
        "layer" | "la" | "layers" if params.is_null() => {
            app.ui.dialog = Some("layers".into());
            Ok(Value::Null)
        }
        // Typed or menu-invoked (no parameters) these open their dialogs; JSON calls run the command.
        "qselect" | "qs" if params.is_null() => {
            app.ui.dialog = Some("qselect".into());
            Ok(Value::Null)
        }
        "units" | "un" if params.is_null() => {
            app.ui.dialog = Some("units".into());
            Ok(Value::Null)
        }
        "dsettings" | "ds" | "se" if params.is_null() => {
            app.ui.dialog = Some("dsettings".into());
            Ok(Value::Null)
        }
        "properties" | "pr" | "props" | "ch" if params.is_null() => {
            crate::docking::command(app, false, &json!({"operation":"open", "panel":"properties"}))
        }
        _ => return None,
    };
    Some(r)
}

fn set_save_format(app: &mut CadApp, format: crate::SaveFormat) -> Value {
    app.ui.save_format = format;
    app.session.echo(format!("New drawings will be saved as {}", format.as_str().to_ascii_uppercase()));
    json!({ "saveFormat": format.as_str() })
}

fn set_theme(app: &mut CadApp, pref: crate::theme::ThemePref) -> Result<Value, String> {
    app.ui.theme = pref;
    Ok(theme_json(app))
}

/// The theme choice.
fn theme_json(app: &CadApp) -> Value {
    json!({ "theme": app.ui.theme.as_str() })
}

/// A menu entry.
#[derive(Clone, Debug)]
pub enum Entry {
    Item { label: String, id: String, shortcut: Option<String>, enabled: bool },
    Sub { label: String, children: Vec<Entry> },
}

/// The full menu tree from the command registry plus UI commands, in registration order.
pub fn tree(app: &CadApp) -> Vec<(String, Vec<Entry>)> {
    let mut out: Vec<(String, Vec<Entry>)> = MENUS.iter().map(|m| (m.to_string(), Vec::new())).collect();
    let mut add = |path: &[&str], label: &str, id: &str, shortcut: Option<&str>, enabled: bool| {
        let Some((top, rest)) = path.split_first() else { return };
        let Some((_, entries)) = out.iter_mut().find(|(m, _)| m == top) else { return };
        let mut cur = entries;
        let n = rest.len();
        for (i, seg) in rest.iter().enumerate() {
            if i + 1 == n {
                cur.push(Entry::Item {
                    label: if seg.is_empty() { label.to_string() } else { seg.to_string() },
                    id: id.to_string(),
                    shortcut: shortcut.map(str::to_string),
                    enabled,
                });
            } else {
                let pos = cur.iter().position(|e| matches!(e, Entry::Sub { label, .. } if label == seg));
                let idx = match pos {
                    Some(p) => p,
                    None => {
                        cur.push(Entry::Sub { label: seg.to_string(), children: Vec::new() });
                        cur.len() - 1
                    }
                };
                let Some(Entry::Sub { children, .. }) = cur.get_mut(idx) else { return };
                cur = children;
            }
        }
    };
    for c in aad_engine::command_specs() {
        if !c.menu.is_empty() {
            add(c.menu, c.label, c.id, c.shortcut, (c.enabled)(&app.session).is_ok());
        }
    }
    for (id, label, menu, sc) in UI_COMMANDS {
        if !menu.is_empty() {
            add(menu, label, id, *sc, true);
        }
    }
    out
}

/// Turn a registry shortcut ("Cmd+Shift+Z") into the form shown on this platform: macOS symbols ("⇧⌘Z") or
/// Windows/Linux text ("Ctrl+Shift+Z"). `mac` comes from the runtime OS (`ctx.os()`), so the web build is right too.
pub(crate) fn shortcut_label(s: &str, mac: bool) -> String {
    if !mac {
        return s.replace("Cmd+", "Ctrl+");
    }
    let shift = if s.contains("Shift+") { "⇧" } else { "" };
    let cmd = if s.contains("Cmd+") { "⌘" } else { "" };
    format!("{shift}{cmd}{}", s.replace("Cmd+", "").replace("Shift+", ""))
}

pub(crate) fn entry_ui(ui: &mut egui::Ui, e: &Entry, clicked: &mut Option<String>) {
    match e {
        Entry::Item { label, id, shortcut, enabled } => {
            let mut b = egui::Button::new(crate::i18n::t(label));
            if let Some(s) = shortcut {
                b = b.shortcut_text(shortcut_label(s, ui.ctx().os().is_mac()));
            }
            if ui.add_enabled(*enabled, b).clicked() {
                *clicked = Some(id.clone());
                ui.close();
            }
        }
        Entry::Sub { label, children } => {
            ui.menu_button(crate::i18n::t(label), |ui| {
                for c in children {
                    entry_ui(ui, c, clicked);
                }
            });
        }
    }
}

pub fn menu_bar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let tree = tree(app);
    let mut clicked = None;
    egui::Panel::top("cc_menubar").exact_size(22.0).frame(egui::Frame::NONE.fill(t.chrome_dark).inner_margin(egui::Margin::symmetric(6, 0))).show(
        ui,
        |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for (name, entries) in &tree {
                    ui.menu_button(crate::i18n::t(name), |ui| {
                        for e in entries {
                            entry_ui(ui, e, &mut clicked);
                        }
                    });
                }
            });
        },
    );
    if let Some(id) = clicked {
        activate(app, &id);
    }
}

/// Like choosing a menu item: starts the command interactively.
pub fn activate(app: &mut CadApp, id: &str) {
    app.cmd.buffer.clear();
    app.start(id);
}

/// Keyboard shortcuts and function keys.
pub fn shortcuts(app: &mut CadApp, ctx: &egui::Context) {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let pairs: &[(KeyboardShortcut, &str)] = &[
        (sc(cmd_shift, Key::Z), "redo"),
        (sc(cmd, Key::Z), "u"),
        (sc(cmd, Key::Y), "redo"),
        (sc(cmd, Key::N), "new"),
        (sc(cmd, Key::O), "ui.open"),
        (sc(cmd_shift, Key::S), "ui.saveas"),
        (sc(cmd, Key::S), "qsave"),
        (sc(cmd, Key::P), "plot"),
        (sc(cmd, Key::A), "selectall"),
        (sc(cmd_shift, Key::C), "copybase"),
        (sc(cmd, Key::C), "copyclip"),
        (sc(cmd, Key::X), "cutclip"),
        (sc(cmd_shift, Key::V), "pasteblock"),
        (sc(cmd, Key::V), "pasteclip"),
        (sc(cmd, Key::Num1), "ui.panel.properties"),
        (sc(cmd, Key::Num9), "ui.toggle.cmdline"),
        (sc(cmd_shift, Key::F11), "resetui"),
        (sc(Modifiers::NONE, Key::F1), "ui.dialog.commands"),
        (sc(Modifiers::NONE, Key::F2), "ui.toggle.history"),
        (sc(Modifiers::NONE, Key::F3), "osnap"),
        (sc(Modifiers::NONE, Key::F8), "ortho"),
        (sc(Modifiers::NONE, Key::F10), "polar"),
        (sc(Modifiers::NONE, Key::F11), "otrack"),
        (sc(Modifiers::NONE, Key::F12), "dynmode"),
    ];
    crate::clipboard::mirror(app, ctx);
    if ctx.egui_wants_keyboard_input() || app.closing.is_some() {
        return;
    }
    // Cmd/Ctrl+C, X and V arrive as clipboard events, not key presses.
    let mut fire = crate::clipboard::shortcut_commands(app, ctx);
    ctx.input_mut(|i| {
        for (s, id) in pairs {
            if i.consume_shortcut(s) && !fire.contains(id) {
                fire.push(*id);
            }
        }
    });
    for id in fire {
        // Toggles run transparently (they don't cancel the active command).
        let transparent = ["osnap", "ortho", "polar", "otrack", "dynmode"].contains(&id);
        if transparent || id.starts_with("ui.") {
            match app.session.execute(id, &json!({})) {
                Ok(v) => {
                    if let Some(m) = v.get("message").and_then(Value::as_str) {
                        app.session.echo(m.to_string());
                    }
                }
                Err(_) => {
                    let _ = app.run(id, json!({}));
                }
            }
        } else {
            activate(app, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use aad_engine::Session;
    use aad_engine::cmd::file::{IoHooks, io, set_io};
    use serde_json::{Value, json};

    use crate::{CadApp, Services};

    fn fake_plot(_: &aad_doc::Drawing, _: &aad_doc::Space, opts: &Value) -> Result<Vec<u8>, String> {
        Ok(format!("%PDF-1.4 test {}", opts.get("window").map(|w| w.to_string()).unwrap_or_default()).into_bytes())
    }

    /// An app whose save dialog records the suggested name and answers `answer`.
    fn app_with_picker(answer: Option<String>) -> (CadApp, Rc<RefCell<Vec<String>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&asked);
        let services = Services {
            pick_open: None,
            pick_save: Some(Box::new(move |name: &str| {
                log.borrow_mut().push(name.to_string());
                answer.clone()
            })),
            ..Services::default()
        };
        (CadApp::new(Session::new(), services), asked)
    }

    /// The application's half of PLOT: take what `plot.window` chose and finish it (the frame loop
    /// does this every frame).
    fn finish(app: &mut CadApp) {
        if let Some(req) = app.session.take_plot_request() {
            super::finish_plot(app, req);
        }
    }

    fn install_fake_plotter() -> bool {
        set_io(IoHooks { read: |_, _| Err("no reader in tests".into()), write: |_, _| Err("no writer in tests".into()), plot: Some(fake_plot) });
        io().and_then(|h| h.plot).is_some() // false when another test installed hooks without a plotter first
    }

    #[test]
    fn print_in_model_space_asks_for_the_window_first_and_then_where_to_save() {
        if !install_fake_plotter() {
            return;
        }
        let path = std::env::temp_dir().join(format!("aad-print-test-{}.pdf", std::process::id()));
        let ps = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);
        let (mut app, asked) = app_with_picker(Some(ps.clone()));

        // Toolbar, menu, Cmd+P and the typed name all start the same thing: the window question.
        app.start("plot");
        assert!(app.session.running.as_ref().is_some_and(|r| r.id == "plot.window"), "it asks what to plot first");
        assert!(asked.borrow().is_empty(), "no file dialog yet");
        app.cmdline("10,20");
        assert!(app.session.running.is_some(), "the opposite corner is still to come");
        app.cmdline("110,70");
        assert!(app.session.running.is_none());
        finish(&mut app);
        assert_eq!(*asked.borrow(), vec!["Drawing1.pdf".to_string()], "now it asks where to save");
        let pdf = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert!(pdf.starts_with("%PDF") && pdf.contains("[10.0,20.0,110.0,70.0]"), "only the window is plotted: {pdf}");
        assert!(app.session.log.iter().any(|l| l.contains("the window") && l.contains(&ps)));
        let _ = std::fs::remove_file(&path);

        // Corners in any order give the same rectangle; Extents plots the whole drawing.
        app.cmdline("print");
        app.cmdline("110,70");
        app.cmdline("10,20");
        finish(&mut app);
        let pdf = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert!(pdf.contains("[10.0,20.0,110.0,70.0]"));
        app.start("exportpdf");
        app.cmdline("Extents");
        finish(&mut app);
        let pdf = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert!(!pdf.contains("window") && !pdf.contains("[10.0"), "no window: the whole drawing: {pdf}");
        assert!(app.session.log.iter().any(|l| l.contains("the whole drawing")));
        assert_eq!(asked.borrow().len(), 3);
        let _ = std::fs::remove_file(&path);

        // Display plots what is on screen.
        app.start("plot");
        app.cmdline("Display");
        let req = app.session.take_plot_request().expect("a request");
        let w = req.window.expect("the visible area");
        assert!(w[2] > w[0] && w[3] > w[1]);

        // Programmatic calls never ask anything.
        let r = app.run("plot", json!({})).unwrap();
        assert!(r["data"].is_string());
        let r = app.run("plot", json!({"window": [0, 0, 5, 5]})).unwrap();
        assert!(r["data"].is_string());
        assert_eq!(asked.borrow().len(), 3);
        assert!(app.run("plot", json!({"window": [0, 0, 0, 5]})).is_err(), "a window needs an area");
        assert!(app.run("plot", json!({"window": "everything"})).is_err());
    }

    #[test]
    fn cancelling_the_window_or_the_file_dialog_plots_nothing() {
        if !install_fake_plotter() {
            return;
        }
        let (mut app, asked) = app_with_picker(None);
        app.start("plot");
        app.session.cancel();
        finish(&mut app);
        assert!(app.session.running.is_none() && asked.borrow().is_empty(), "no window, no dialog");
        app.start("plot");
        app.cmdline("0,0");
        app.cmdline("0,5");
        assert!(app.session.running.is_some(), "a rectangle with no width is asked for again");
        app.cmdline("4,5");
        finish(&mut app);
        assert_eq!(asked.borrow().len(), 1, "the dialog was cancelled: nothing is written");
    }

    #[test]
    fn a_layout_is_plotted_as_it_is_without_asking_for_a_window() {
        if !install_fake_plotter() {
            return;
        }
        let path = std::env::temp_dir().join(format!("aad-print-layout-{}.pdf", std::process::id()));
        let ps = path.to_string_lossy().to_string();
        let (mut app, asked) = app_with_picker(Some(ps));
        let name = app.run("layout.new", json!({})).unwrap()["name"].as_str().unwrap().to_string();
        app.run("layout", json!({"option": "set", "name": name})).unwrap();
        app.start("plot");
        assert!(app.session.running.is_none(), "no window question in paper space");
        assert_eq!(asked.borrow().len(), 1, "it goes straight to where to save");
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod shortcut_label_tests {
    use super::shortcut_label;

    #[test]
    fn shortcut_label_per_platform() {
        assert_eq!(shortcut_label("Cmd+Shift+Z", true), "⇧⌘Z");
        assert_eq!(shortcut_label("Cmd+Z", true), "⌘Z");
        assert_eq!(shortcut_label("Cmd+Shift+Z", false), "Ctrl+Shift+Z");
        assert_eq!(shortcut_label("Cmd+Z", false), "Ctrl+Z");
        assert_eq!(shortcut_label("Shift+F3", false), "Shift+F3");
        assert_eq!(shortcut_label("Shift+F3", true), "⇧F3");
        assert_eq!(shortcut_label("F1", false), "F1");
        assert_eq!(shortcut_label("F1", true), "F1");
    }
}

#[cfg(test)]
mod units_dialog_tests {
    use aad_engine::Session;
    use serde_json::json;

    use crate::{CadApp, Services};

    #[test]
    fn units_typed_opens_dialog_json_runs_command() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.cmdline("UNITS");
        assert_eq!(app.ui.dialog.as_deref(), Some("units"));
        app.ui.dialog = None;
        app.start("un");
        assert_eq!(app.ui.dialog.as_deref(), Some("units"));
        app.ui.dialog = None;
        let r = app.run("units", json!({ "lunits": 4, "luprec": 3, "insunits": 4 })).unwrap();
        assert_eq!(r["lunits"], 4);
        assert!(app.ui.dialog.is_none(), "JSON calls never open dialogs");
        let h = &app.session.doc().unwrap().header;
        assert_eq!((h.i64("LUNITS", 0), h.i64("LUPREC", 0), h.i64("INSUNITS", 0)), (4, 3, 4));
    }
}

/// Switch workspace. Only visibility changes: Pro hides the toolbars and side palettes (and
/// remembers whether the palettes were open), Classic brings them back.
fn set_workspace(app: &mut CadApp, mode: crate::WorkspaceMode) -> Value {
    if app.ui.workspace != mode {
        // Leaving a mode saves its arrangement and pin state; entering one restores its own (or the
        // defaults of that mode). The two never share or overwrite each other's.
        let old = app.ui.workspace;
        let slot = json!({"docking": app.ui.docking, "pin": app.ui.pin_panels, "toolbar": app.ui.show_toolbar, "topBar": app.ui.top_bar});
        app.ui.mode_slots.insert(old.as_str().to_string(), slot);
        let (docking, pin, toolbar, top_bar) = restore_slot(app, mode);
        app.ui.docking = docking;
        app.ui.show_toolbar = toolbar;
        app.ui.top_bar = top_bar;
        app.ui.pin_panels = pin;
        match mode {
            crate::WorkspaceMode::Pro => {
                app.ui.classic_palettes = app.ui.show_palettes;
                app.ui.show_palettes = false;
            }
            crate::WorkspaceMode::Classic => app.ui.show_palettes = true,
        }
        app.ui.workspace = mode;
    }
    json!({ "workspace": app.ui.workspace.as_str() })
}

/// The panel arrangement and pin state a mode starts with: Classic is pinned, Pro is collapsed.
fn mode_defaults(mode: crate::WorkspaceMode) -> (crate::docking::Workspace, bool) {
    (crate::docking::Workspace::default(), mode == crate::WorkspaceMode::Classic)
}

/// The saved slot of `mode`, or its defaults when there is none or it no longer reads.
fn restore_slot(app: &mut CadApp, mode: crate::WorkspaceMode) -> (crate::docking::Workspace, bool, bool, Vec<String>) {
    let (default_docking, default_pin) = mode_defaults(mode);
    let Some(slot) = app.ui.mode_slots.remove(mode.as_str()) else { return (default_docking, default_pin, true, crate::topbar::default_items()) };
    let docking =
        slot.get("docking").and_then(|d| serde_json::from_value::<crate::docking::Workspace>(d.clone()).ok()).filter(|d| d.validate().is_ok());
    let pin = slot.get("pin").and_then(Value::as_bool).unwrap_or(default_pin);
    let toolbar = slot.get("toolbar").and_then(Value::as_bool).unwrap_or(true);
    let top_bar = slot
        .get("topBar")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>())
        .map(|v| crate::topbar::sanitize(&v))
        .unwrap_or_else(crate::topbar::default_items);
    (docking.unwrap_or(default_docking), pin, toolbar, top_bar)
}

/// `ui.workspace.reset` and `resetui` (typed at the command line, or View > Reset Toolbars and Menus):
/// the current mode's panels, pin state, toolbars, menu bar, status bar, file tabs and command line back to their defaults.
/// The other mode's saved arrangement is not touched.
fn reset_workspace(app: &mut CadApp) -> Value {
    let mode = app.ui.workspace;
    let (docking, pin) = mode_defaults(mode);
    app.ui.docking = docking;
    app.ui.pin_panels = pin;
    app.ui.show_palettes = mode == crate::WorkspaceMode::Classic;
    app.ui.show_toolbar = true;
    app.ui.top_bar = crate::topbar::default_items();
    app.ui.show_command_line = true;
    app.ui.show_status_bar = true;
    app.ui.show_file_tabs = true;
    // The menu bar too: with it switched off there would be no menu to find this command in.
    app.ui.in_window_menu = !cfg!(target_os = "macos");
    app.ui.show_ucs_icon = true;
    app.ui.properties_all = true;
    app.ui.history_lines = 2;
    app.ui.dialog = None;
    app.collapse = Default::default();
    json!({ "workspace": mode.as_str(), "reset": true })
}

fn fonts_json(app: &CadApp) -> Value {
    let p = aad_fonts::registry::preset(&app.ui.font_preset).unwrap_or(&aad_fonts::registry::PRESETS[0]);
    let t = aad_fonts::registry::Typography { preset: p.id.into(), overrides: Vec::new() };
    let roles: Vec<Value> = t
        .resolve_all()
        .into_iter()
        .map(|r| json!({"role": r.role.id(), "family": r.family, "weight": r.weight, "requested": r.requested, "substituted": r.substituted, "weightInstalled": r.weight_installed}))
        .collect();
    json!({"preset": p.id, "name": p.name, "roles": roles})
}

/// Select the interface typography preset. The fonts are rebuilt on the next frame; a preset whose
/// fonts are not installed keeps the system font and the answer says which roles were substituted.
fn set_font_preset(app: &mut CadApp, id: &str) -> Result<Value, String> {
    if aad_fonts::registry::preset(id).is_none() {
        return Err(format!("unknown font preset \"{id}\""));
    }
    app.ui.font_preset = id.to_string();
    Ok(fonts_json(app))
}

/// The second half of PLOT in model space: the window (or the whole drawing) has been chosen, so ask
/// where to save and plot it. Cancelling the file dialog plots nothing.
pub(crate) fn finish_plot(app: &mut CadApp, req: aad_engine::PlotRequest) {
    let Some(pick) = app.services.pick_save.as_ref() else { return };
    let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
    let Some(path) = pick(&format!("{}.pdf", stem.unwrap_or_else(|| "Drawing".into()))) else {
        app.session.echo("*Cancel*");
        return;
    };
    let mut params = json!({ "path": path });
    if let (Some(w), Some(o)) = (req.window, params.as_object_mut()) {
        o.insert("window".into(), json!(w));
    }
    let r = app.session.execute("plot", &params).map_err(|e| e.to_string());
    app.session.echo(match &r {
        Ok(_) => format!("Plotted {} to {path}", if req.window.is_some() { "the window" } else { "the whole drawing" }),
        Err(e) => e.clone(),
    });
}

//! Window chrome: title and tool bar, file tabs, status bar and the Start page.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

const TOOLBAR: &[&[(Icon, &str, &str, Option<&str>)]] = &[
    &[
        (Icon::New, "new", "New Drawing", Some("Cmd+N")),
        (Icon::Open, "open", "Open", Some("Cmd+O")),
        (Icon::Save, "qsave", "Save", Some("Cmd+S")),
        (Icon::SaveAs, "saveas", "Save As", None),
    ],
    &[(Icon::Undo, "u", "Undo", Some("Cmd+Z")), (Icon::Redo, "redo", "Redo", Some("Cmd+Shift+Z"))],
    &[
        (Icon::Plot, "plot", "Print", None),
        (Icon::Publish, "publish", "Batch Publish", None),
        (Icon::PageSetup, "pagesetup", "Page Setup Manager", None),
    ],
    &[(Icon::Import, "import", "Import", None), (Icon::Export, "export", "Export", None), (Icon::Attach, "attach", "Attach", None)],
    &[(Icon::ZoomWindow, "zoom.window", "Zoom Window", None), (Icon::Pan, "pan", "Pan", None)],
    &[
        (Icon::ZoomExtents, "zoom.extents", "Zoom Extents", None),
        (Icon::Properties, "ui.toggle.palettes", "Properties", None),
        (Icon::Layers, "ui.dialog.layers", "Layer Properties Manager", None),
        (Icon::Blocks, "ui.dialog.blocks", "Blocks", None),
    ],
    &[(Icon::Measure, "dist", "Measure Distance", None), (Icon::List, "list", "List", None), (Icon::Area, "area", "Area", None)],
];

/// Height of the macOS integrated title row (#161). AppKit draws the traffic lights itself,
/// centred in the standard 28 pt title-bar band at the top of the window; moving them would take
/// AppKit calls (`unsafe`, which the workspace forbids). So the title row is that band, and what
/// it holds is centred on the same line as the lights.
const MAC_TITLE_BAR: f32 = 28.0;
/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;
const MAC_LIGHT_PITCH: f32 = 20.0;
const MAC_LIGHTS_LEFT: f32 = 10.0;
/// Where title-row content may start to the right of the traffic lights: the end of the three
/// lights plus the same gap the lights keep from the top of the window.
const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;

/// Left edge of the window title in a title row starting at `left`, `width` wide: centred, but
/// never under the traffic lights (a long drawing name in a narrow window slides right instead).
fn title_left(left: f32, width: f32, text_w: f32) -> f32 {
    (left + (width - text_w) / 2.0).max(left + MAC_CONTENT_LEFT)
}

/// Title row (with the integrated macOS title bar) and the Tool Bar.
pub fn title_and_toolbar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let title_h = if app.integrated_titlebar { MAC_TITLE_BAR } else { 0.0 };
    let tb_h = if app.toolbars_shown() { 34.0 } else { 0.0 };
    egui::Panel::top("cc_title_toolbar").exact_size(title_h + tb_h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        if title_h > 0.0 {
            let title = match app.session.state() {
                Ok(st) if !app.ui.start_tab => format!("O-Draft  {}{}", st.title, if st.title.contains('.') { "" } else { ".dwg" }),
                _ => format!("O-Draft  {}", crate::tl!("Start")),
            };
            // Centred on the traffic lights' centre line (the row's middle): with the system font
            // the capitals' middle is the text box's middle, so no optical nudge.
            let galley = ui.painter().layout_no_wrap(title, egui::FontId::proportional(13.0), t.text_dim);
            let x = title_left(r.left(), r.width(), galley.size().x);
            ui.painter().galley(pos2(x, r.top() + (title_h - galley.size().y) / 2.0), galley, t.text_dim);
            // Allow dragging the window by the title row.
            let tr = Rect::from_min_size(r.min, vec2(r.width(), title_h));
            let resp = ui.interact(tr, ui.id().with("titledrag"), Sense::CLICK | Sense::DRAG);
            if resp.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if resp.double_clicked() {
                let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
        }
        if tb_h > 0.0 {
            let row = Rect::from_min_size(pos2(r.left(), r.top() + title_h), vec2(r.width(), tb_h));
            let mut x = r.left() + if app.integrated_titlebar { 120.0 } else { 12.0 };
            let size = 26.0;
            let mut clicked = None;
            let mac = ui.ctx().os().is_mac();
            let widths: Vec<_> = TOOLBAR.iter().map(|group| group.len() as f32 * (size + 6.0) - 6.0).collect();
            let available = (row.right() - 12.0 - x).max(0.0);
            let Some(fit) = craft_ui::layout::fit_toolbar(&widths, available, 24.0, 30.0) else {
                // Very small windows still expose the complete command row by scrolling.
                ui.scope_builder(egui::UiBuilder::new().max_rect(row.shrink2(vec2(4.0, 0.0))), |ui| {
                    egui::ScrollArea::horizontal().id_salt("toolbar_small").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for group in TOOLBAR {
                                for (icon, cmd, tip, _) in *group {
                                    if icons::button(ui, *icon, size, tip, false).clicked() {
                                        clicked = Some(*cmd);
                                    }
                                }
                                ui.add_space(18.0);
                            }
                        });
                    });
                });
                if let Some(command) = clicked {
                    app.start(command);
                }
                return;
            };
            for (index, _) in &fit.shown {
                let Some(group) = TOOLBAR.get(*index) else { continue };
                for (icon, cmd, label, shortcut) in group.iter() {
                    let br = Rect::from_min_size(pos2(x, row.center().y - size / 2.0), vec2(size, size));
                    let resp = ui.interact(br, ui.id().with(("tb", *cmd)), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
                    }
                    icons::paint(ui.painter(), br.shrink(4.0), *icon, false);
                    let tip = match shortcut {
                        Some(sc) => format!("{} ({})", crate::i18n::t(label), crate::menus::shortcut_label(sc, mac)),
                        None => crate::i18n::t(label).to_string(),
                    };
                    icons::describe_control(ui, &resp, &tip, None, false);
                    if resp.on_hover_text(tip).clicked() {
                        clicked = Some(*cmd);
                    }
                    x += size + 6.0;
                }
                x += 18.0;
            }
            if !fit.overflow.is_empty() {
                let br = Rect::from_min_size(pos2(x, row.center().y - size / 2.0), vec2(fit.overflow_width, size));
                let response = ui.interact(br, ui.id().with("toolbar_more"), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
                }
                for offset in [-5.0, 0.0, 5.0] {
                    ui.painter().circle_filled(br.center() + vec2(offset, 0.0), 1.5, t.text);
                }
                icons::describe_control(ui, &response, "More toolbar commands", None, false);
                egui::Popup::menu(&response).show(|ui| {
                    for (position, index) in fit.overflow.iter().enumerate() {
                        let Some(group) = TOOLBAR.get(*index) else { continue };
                        if position > 0 {
                            ui.separator();
                        }
                        for (_, command, label, _) in *group {
                            let response = ui.button(crate::i18n::t(label));
                            crate::control::record_widget(ui, &response, label, "button", None);
                            if response.clicked() {
                                clicked = Some(*command);
                                ui.close();
                            }
                        }
                    }
                });
                response.on_hover_text("More toolbar commands");
            }
            if let Some(c) = clicked {
                app.start(c);
            }
        }
    });
}

/// File tabs: switcher, "+", Start, open drawings.
pub fn file_tabs(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::top("cc_file_tabs").exact_size(26.0).frame(egui::Frame::NONE.fill(t.chrome_dark)).show(ui, |ui| {
        let r = ui.max_rect();
        let mut x = r.left() + 4.0;
        let ib = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(18.0, 18.0));
        icons::paint(ui.painter(), ib.shrink(1.0), Icon::Switcher, false);
        x += 24.0;
        let plus = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(18.0, 18.0));
        let presp = ui.interact(plus, ui.id().with("tab+"), Sense::click());
        icons::paint(ui.painter(), plus.shrink(2.0), Icon::Plus, false);
        icons::describe_control(ui, &presp, "New Drawing", None, false);
        if presp.on_hover_text(crate::tl!("New Drawing")).clicked() {
            app.start("new");
            app.ui.start_tab = false;
        }
        x += 24.0;
        let mut tab = |ui: &mut egui::Ui, label: &str, active: bool, closable: bool, w: f32, id: egui::Id| -> (bool, bool) {
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()));
            let resp = ui.interact(tr, id, Sense::click());
            ui.painter().rect_filled(
                tr,
                0.0,
                if active {
                    t.tab_active
                } else if resp.hovered() {
                    t.chrome
                } else {
                    t.chrome_dark
                },
            );
            ui.painter().text(
                pos2(tr.left() + 8.0, tr.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                crate::theme::body(),
                if active { t.text } else { t.text_dim },
            );
            let mut close = false;
            if closable && (active || resp.hovered()) {
                let cr = Rect::from_center_size(pos2(tr.right() - 11.0, tr.center().y), vec2(12.0, 12.0));
                let cresp = ui.interact(cr, id.with("x"), Sense::click());
                icons::paint(ui.painter(), cr, Icon::Close, false);
                icons::describe_control(ui, &cresp, &format!("Close {label}"), None, false);
                close = cresp.clicked();
            }
            icons::describe_control(ui, &resp, label, Some(active), true);
            ui.painter().vline(tr.right(), tr.y_range(), Stroke::new(1.0, t.border));
            x += w;
            (resp.clicked(), close)
        };
        let (c, _) = tab(ui, crate::tl!("Start"), app.ui.start_tab || app.session.docs.is_empty(), false, 110.0, ui.id().with("tab_start"));
        if c {
            app.ui.start_tab = true;
        }
        let mut switch_to = None;
        let mut close = None;
        let titles: Vec<(usize, String, bool)> = app
            .session
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| (i, format!("{}{}", d.title, if d.is_dirty() { "*" } else { "" }), i == app.session.active))
            .collect();
        for (i, title, active) in titles {
            let (c, x) = tab(ui, &title, active && !app.ui.start_tab, true, 170.0, ui.id().with(("tab", i)));
            if c {
                switch_to = Some(i);
            }
            if x {
                close = Some(i);
            }
        }
        if let Some(i) = switch_to {
            let _ = app.run("document.switch", json!({ "index": i }));
            app.ui.start_tab = false;
        }
        if let Some(i) = close {
            app.request_close(Some(i));
        }
    });
}

/// Status bar: coordinates and drafting toggles. Paper layouts are not tabs; a print block names a region of the model.
pub fn status_bar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::bottom("cc_status").exact_size(26.0).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let x = r.left() + 8.0;
        // Clear the status message on time even when no input arrives (the app only repaints on input).
        if let Some((_, at)) = &app.status
            && crate::now_ms() - at < 5000.0
        {
            let left = std::time::Duration::try_from_secs_f64((5000.0 - (crate::now_ms() - at)) / 1000.0).unwrap_or_default();
            ui.ctx().request_repaint_after(left);
        }
        // Right side: toggles, right-aligned.
        let s = app.session.settings.clone();
        use aad_engine::snap::mode;
        let toggles: Vec<(Icon, bool, &str, &str)> = vec![
            (Icon::Ortho, s.orthomode, "ortho", "Ortho Mode (F8)"),
            (Icon::Polar, s.polarmode, "polar", "Polar Tracking (F10)"),
            (Icon::OTrack, s.otrack, "otrack", "Object Snap Tracking (F11)"),
            (Icon::Osnap, s.osmode & mode::OFF == 0 && s.osmode != 0, "osnap", "Object Snap (F3)"),
            (Icon::Lineweight, s.lwdisplay, "lwdisplay", "Show/Hide Lineweight"),
            (Icon::Transparency, s.transparency_display, "transparencydisplay", "Show/Hide Transparency"),
            (Icon::DynInput, s.dynmode, "dynmode", "Dynamic Input (F12)"),
            (Icon::QuickProps, s.qpmode, "qpmode", "Quick Properties"),
            (Icon::Annotation, s.annoallvisible, "ui.noop", "Annotation Visibility"),
            (Icon::Workspace, false, "ui.noop", "Workspace Switching"),
            (Icon::List, app.ui.show_command_line, "ui.toggle.cmdline", "Command Line (Ctrl+9)"),
            (Icon::Gear, false, "ui.dialog.dsettings", "Drafting Settings"),
        ];
        let size = 20.0;
        let mut rx = r.right() - 8.0 - 60.0 - 128.0 - 200.0 - toggles.len() as f32 * (size + 4.0) - (58.0 + 6.0 + 40.0 + 8.0);
        // Light/dark theme: a switch with a sun and a moon. The knob sits on the side of the theme
        // in use (moon, right: dark; sun, left: light); clicking it switches to the other theme.
        let theme_br = Rect::from_min_max(pos2(r.right() - 8.0 - 46.0, r.center().y - 10.0), pos2(r.right() - 8.0, r.center().y + 10.0));
        let dark = app.ui.theme == crate::theme::ThemePref::Dark;
        let theme_resp = ui.interact(theme_br, ui.id().with("theme_toggle"), Sense::click());
        p.rect_filled(theme_br, 10.0, if theme_resp.hovered() { t.control_hover } else { t.control });
        p.rect_stroke(theme_br, 10.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        let (sun_c, moon_c) = (pos2(theme_br.left() + 12.0, theme_br.center().y), pos2(theme_br.right() - 12.0, theme_br.center().y));
        let knob = if dark { moon_c } else { sun_c };
        p.circle_filled(knob, 8.0, t.accent);
        // Sun: a disc with rays. Moon: a disc with a bite out of it.
        let ink = |on: bool| if on { egui::Color32::WHITE } else { t.text_dim };
        let sc = ink(!dark);
        p.circle_filled(sun_c, 3.0, sc);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let (dx, dy) = (a.cos(), a.sin());
            p.line_segment([sun_c + vec2(dx * 4.8, dy * 4.8), sun_c + vec2(dx * 6.2, dy * 6.2)], Stroke::new(1.2, sc));
        }
        let mc = ink(dark);
        p.circle_filled(moon_c, 5.0, mc);
        p.circle_filled(
            moon_c + vec2(2.6, -2.0),
            4.3,
            if dark {
                t.accent
            } else if theme_resp.hovered() {
                t.control_hover
            } else {
                t.control
            },
        );
        let tip = if dark { crate::tl!("Dark theme: switch to light") } else { crate::tl!("Light theme: switch to dark") };
        if theme_resp.on_hover_text(tip).clicked() {
            let _ = app.run("ui.theme.toggle", json!({}));
        }
        // Workspace selector `Pro | Classic` left of the theme toggle; the active side is lit.
        let ws_w = 112.0;
        let ws_br = Rect::from_min_max(pos2(theme_br.left() - 8.0 - ws_w, r.center().y - 9.0), pos2(theme_br.left() - 8.0, r.center().y + 9.0));
        p.rect_filled(ws_br, 3.0, t.control);
        for (i, (mode, label)) in [(crate::WorkspaceMode::Pro, "Pro"), (crate::WorkspaceMode::Classic, "Classic")].into_iter().enumerate() {
            let half = Rect::from_min_max(
                pos2(ws_br.left() + if i == 0 { 0.0 } else { 40.0 }, ws_br.top()),
                pos2(if i == 0 { ws_br.left() + 40.0 } else { ws_br.right() }, ws_br.bottom()),
            );
            let resp = ui.interact(half, ui.id().with(("workspace", label)), Sense::click());
            let active = app.ui.workspace == mode;
            if active {
                p.rect_filled(half, 3.0, t.accent);
            } else if resp.hovered() {
                p.rect_filled(half, 3.0, t.control_hover);
            }
            p.text(half.center(), egui::Align2::CENTER_CENTER, label, crate::theme::small(), if active { egui::Color32::WHITE } else { t.text });
            icons::describe_control(ui, &resp, label, Some(active), false);
            let tip = if mode == crate::WorkspaceMode::Pro {
                "Pro: full-screen drafting, no toolbars or palettes"
            } else {
                "Classic: toolbars, Properties, Layers and Blocks"
            };
            if resp.on_hover_text(tip).clicked() {
                let _ = app.run(if mode == crate::WorkspaceMode::Pro { "ui.workspace.pro" } else { "ui.workspace.classic" }, json!({}));
            }
        }
        // Units and plot popup left of the workspace selector.
        let units_text = crate::units_panel::label(app);
        let units_w = (units_text.chars().count() as f32 * 6.6 + 16.0).clamp(80.0, 190.0);
        let units_br = Rect::from_min_max(pos2(ws_br.left() - 8.0 - units_w, r.center().y - 9.0), pos2(ws_br.left() - 8.0, r.center().y + 9.0));
        let units_resp = ui.interact(units_br, ui.id().with("units_button"), Sense::click());
        p.rect_filled(units_br, 3.0, if units_resp.hovered() { t.control_hover } else { t.control });
        p.text(units_br.center(), egui::Align2::CENTER_CENTER, &units_text, crate::theme::small(), t.text);
        icons::describe_control(ui, &units_resp, "Units and plot", None, false);
        egui::Popup::from_toggle_button_response(&units_resp.on_hover_text(crate::tl!("Drawing units, display units, plot scale and annotation")))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| crate::units_panel::show(app, ui));
        // Pomodoro and Calculator buttons left of the units control: the timer shows its countdown
        // even with its window closed; a click opens or closes the window.
        {
            let (y0, y1) = (r.center().y - 9.0, r.center().y + 9.0);
            let (ptext, active, running, fraction) = {
                let pm = &app.widgets.pomodoro;
                (pm.label(), pm.active(), pm.running(), pm.fraction())
            };
            let pbr = Rect::from_min_max(pos2(units_br.left() - 8.0 - 58.0, y0), pos2(units_br.left() - 8.0, y1));
            let presp = ui.interact(pbr, ui.id().with("pomodoro_button"), Sense::click());
            p.rect_filled(pbr, 3.0, if presp.hovered() { t.control_hover } else { t.control });
            if active {
                let fill = Rect::from_min_max(pbr.min, pos2(pbr.left() + pbr.width() * fraction as f32, pbr.bottom()));
                p.rect_filled(fill, 3.0, t.accent.gamma_multiply(if running { 0.55 } else { 0.25 }));
            }
            p.text(pbr.center(), egui::Align2::CENTER_CENTER, &ptext, crate::theme::small(), t.text);
            icons::describe_control(ui, &presp, "Pomodoro timer", Some(app.ui.show_pomodoro), false);
            if presp.on_hover_text(crate::tl!("Pomodoro timer: click to open or close")).clicked() {
                let _ = app.run("ui.toggle.pomodoro", json!({}));
            }
            let cbr = Rect::from_min_max(pos2(pbr.left() - 6.0 - 40.0, y0), pos2(pbr.left() - 6.0, y1));
            let cresp = ui.interact(cbr, ui.id().with("calculator_button"), Sense::click());
            let open = app.ui.show_calculator;
            p.rect_filled(
                cbr,
                3.0,
                if open {
                    t.accent
                } else if cresp.hovered() {
                    t.control_hover
                } else {
                    t.control
                },
            );
            p.text(
                cbr.center(),
                egui::Align2::CENTER_CENTER,
                crate::tl!("Calc"),
                crate::theme::small(),
                if open { egui::Color32::WHITE } else { t.text },
            );
            icons::describe_control(ui, &cresp, "Calculator", Some(open), false);
            if cresp.on_hover_text(crate::tl!("Architect calculator: feet, inches, length, area and volume")).clicked() {
                let _ = app.run("ui.toggle.calculator", json!({}));
            }
        }
        // Coordinates.
        let coord = app.canvas.cursor.map(|c| {
            let (lu, lp) = app.session.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
            let units = match app.session.doc().map(|d| d.header.i64("INSUNITS", 0)).unwrap_or(0) {
                1 => "in",
                2 => "ft",
                4 => "mm",
                5 => "cm",
                6 => "m",
                _ => "units",
            };
            let selected = app.session.selection().len();
            let sel = if selected > 0 { format!("  |  {selected} selected") } else { String::new() };
            format!(
                "{}, {}, {}  ({units}){sel}",
                aad_engine::units::format_distance(c.x, lu, lp),
                aad_engine::units::format_distance(c.y, lu, lp),
                aad_engine::units::format_distance(0.0, lu, lp)
            )
        });
        // MODEL / PAPER (layouts only): toggles MSPACE and PSPACE like AutoCAD's status button.
        let mut coord_right = rx - 8.0;
        if matches!(app.session.layout_space(), aad_doc::Space::Paper(_)) {
            let model = app.session.state().is_ok_and(|st| st.mspace.is_some());
            let label = if model { "MODEL" } else { "PAPER" };
            let br = Rect::from_min_max(pos2(rx - 60.0, r.center().y - 9.0), pos2(rx - 6.0, r.center().y + 9.0));
            let resp = ui.interact(br, ui.id().with("mspace_toggle"), Sense::click());
            p.rect_filled(br, 3.0, if resp.hovered() { t.control_hover } else { t.toggle_on.gamma_multiply(0.25) });
            p.text(br.center(), egui::Align2::CENTER_CENTER, crate::i18n::t(label), crate::theme::small(), t.text);
            icons::describe_control(ui, &resp, label, None, false);
            if resp.on_hover_text(crate::tl!("Switch between model space in a viewport and paper space")).clicked() {
                let _ = app.run(if model { "pspace" } else { "mspace" }, json!({}));
                app.canvas.list = None;
            }
            coord_right = br.left() - 10.0;
        }
        // A transient message occupies the same bounded readout slot as coordinates.
        // Ellipsis and a tooltip keep either value readable in the status readout.
        let message = app.status.as_ref().filter(|(_, at)| crate::now_ms() - at < 5000.0).map(|(message, _)| message);
        let readout = message.cloned().or(coord);
        if let Some(text) = readout {
            let left = x + 8.0;
            let width = (coord_right - left).max(0.0);
            if width > 4.0 {
                let color = if message.is_some() { t.warn } else { t.text };
                let mut job = egui::text::LayoutJob::simple_singleline(text.clone(), crate::theme::body(), color);
                job.wrap.max_width = width;
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                let galley = p.layout_job(job);
                let rect = Rect::from_min_max(pos2(left, r.top()), pos2(coord_right, r.bottom()));
                let text_x = if message.is_some() { left } else { coord_right - galley.size().x };
                p.with_clip_rect(rect).galley(pos2(text_x, r.center().y - galley.size().y / 2.0), galley, color);
                ui.interact(rect, ui.id().with("status_readout"), Sense::hover()).on_hover_text(text);
            }
        }
        let mut toggle_cmd = None;
        for (icon, on, cmd, tip) in toggles {
            let br = Rect::from_min_size(pos2(rx, r.center().y - size / 2.0), vec2(size, size));
            let resp = ui.interact(br, ui.id().with(("tg", tip)), Sense::click());
            if on {
                p.rect_filled(br, 3.0, t.toggle_on.gamma_multiply(0.35));
            } else if resp.hovered() {
                p.rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.5));
            }
            icons::paint(&p, br.shrink(2.5), icon, false);
            if on {
                // Re-tint the icon blue-ish by an underline.
                p.hline(br.x_range().shrink(4.0), br.bottom() - 1.0, Stroke::new(1.5, t.toggle_on));
            }
            icons::describe_control(ui, &resp, tip, Some(on), false);
            if resp.on_hover_text(crate::i18n::t(tip)).clicked() {
                toggle_cmd = Some(cmd);
            }
            rx += size + 4.0;
        }
        if let Some(c) = toggle_cmd {
            app.start(c);
        }
    });
}

/// The Start page (no drawing open, or the Start tab).
pub fn start_page(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let r = ui.max_rect();
    let p = ui.painter().clone();
    let left = Rect::from_min_size(r.min, vec2(300.0, r.height()));
    p.rect_filled(left, 0.0, t.chrome_dark);
    p.rect_filled(Rect::from_min_max(pos2(left.right(), r.top()), r.max), 0.0, t.chrome);
    p.text(pos2(left.left() + 34.0, left.top() + 70.0), egui::Align2::LEFT_CENTER, "O-Draft", egui::FontId::proportional(28.0), t.text);
    p.text(
        pos2(left.left() + 34.0, left.top() + 98.0),
        egui::Align2::LEFT_CENTER,
        format!("Drafting - {}", crate::tf!("Version {version}", version = env!("CARGO_PKG_VERSION"))),
        crate::theme::small(),
        t.text_faint,
    );
    let mut y = left.top() + 140.0;
    let mut action = None;
    for (label, cmd) in [("Open...", "open"), ("New", "new"), ("New (metric)", "new.metric"), ("Open sample drawing", "sample")] {
        let br = Rect::from_min_size(pos2(left.left() + 34.0, y), vec2(220.0, 34.0));
        let resp = ui.interact(br, ui.id().with(("start", cmd)), Sense::click());
        p.rect_stroke(br, 2.0, Stroke::new(1.0, if resp.hovered() { t.text } else { t.text_dim }), egui::StrokeKind::Inside);
        p.text(pos2(br.left() + 16.0, br.center().y), egui::Align2::LEFT_CENTER, crate::i18n::t(label), crate::theme::body(), t.text);
        if resp.clicked() {
            action = Some(cmd);
        }
        y += 44.0;
    }
    let main = Rect::from_min_max(pos2(left.right() + 36.0, r.top() + 40.0), r.max);
    p.text(main.min, egui::Align2::LEFT_TOP, crate::tl!("Recent"), egui::FontId::proportional(22.0), t.text);
    p.text(
        pos2(main.center().x, main.center().y),
        egui::Align2::CENTER_CENTER,
        crate::tl!("There is nothing here yet."),
        egui::FontId::proportional(18.0),
        t.text,
    );
    p.text(
        pos2(main.center().x, main.center().y + 26.0),
        egui::Align2::CENTER_CENTER,
        crate::tl!("To get started, create or open a drawing."),
        crate::theme::body(),
        t.text_dim,
    );
    match action {
        Some("open") => app.start("ui.open"),
        Some("new") => {
            app.start("new");
            app.ui.start_tab = false;
        }
        Some("new.metric") => {
            let _ = app.run("new", json!({ "metric": true }));
            app.ui.start_tab = false;
        }
        Some("sample") => {
            app.start("ui.sample");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_title_row_centres_and_clears_the_traffic_lights() {
        // The lights are centred in the row, and content keeps the same gap after them.
        let margin = (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(MAC_CONTENT_LEFT - lights_end, margin);
        // A short title is centred; one too wide for that starts after the lights.
        assert_eq!(title_left(100.0, 1000.0, 200.0), 500.0);
        assert_eq!(title_left(100.0, 900.0, 800.0), 100.0 + MAC_CONTENT_LEFT);
    }
}

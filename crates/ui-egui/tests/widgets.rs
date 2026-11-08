//! The Calculator and Pomodoro widgets: opened from the Tools menu commands, driven by the same
//! commands the buttons send, and the timer keeps counting while its window is closed.

use aad_engine::Session;
use aad_ui_egui::{CadApp, Services};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.in_window_menu = false;
    app.integrated_titlebar = false;
    app.run("line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn has(h: &Harness<'static, CadApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

#[test]
fn the_calculator_is_typed_into_and_answers_as_you_type() {
    use egui::accesskit::Role;
    let mut h = harness();
    assert!(!has(&h, "Length"), "closed until asked for");
    h.state_mut().run("ui.toggle.calculator", json!({})).unwrap();
    h.run_steps(5);
    assert!(has(&h, "Length"));
    assert!(!has(&h, "Del") && !has(&h, "×"), "no on-screen keypad");

    // The cursor is already in the field: type, and the answer follows.
    let n = h.query_all_by_role(Role::TextInput).count();
    h.query_all_by_role(Role::TextInput).nth(n - 1).unwrap().type_text("10' 6\" x 20'");
    h.run_steps(3);
    let r = h.state_mut().run("ui.calculator", json!({})).unwrap();
    assert_eq!(r["kind"], "area");
    assert_eq!(r["results"]["m2"], "19.51");
    assert!(has(&h, "Area"), "the kind label follows what is typed");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state_mut().run("ui.calculator", json!({})).unwrap()["display"], "210.00 ft²");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state_mut().run("ui.calculator", json!({})).unwrap()["display"], "0");

    // Typing in the field never reaches the command line, and the drawing is untouched.
    assert!(h.state().cmd.buffer.is_empty());
    assert_eq!(h.state().session.doc().unwrap().model.iter().count(), 1);
    h.state_mut().run("ui.toggle.calculator", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert!(!has(&h, "Length"));
}

#[test]
fn the_pomodoro_is_a_dial_on_the_canvas_click_to_start_and_drag_to_set() {
    let mut h = harness();
    let r = h.state_mut().run("ui.pomodoro", json!({})).unwrap();
    assert_eq!((r["label"].as_str(), r["running"].as_bool(), r["minutes"].as_u64()), (Some("25:00"), Some(false), Some(25)));
    h.state_mut().run("ui.toggle.pomodoro", json!({})).unwrap();
    h.run_steps(5);
    let canvas = h.state().canvas.rect.unwrap();
    let centre = egui::pos2(canvas.right() - 16.0 - 52.0, canvas.bottom() - 16.0 - 52.0);
    for label in ["Start", "Pause", "Reset"] {
        assert!(h.query_all_by_label(label).all(|n| n.rect().min.x < 900.0), "the dial has no buttons ({label})");
    }

    // A click starts it, another stops it.
    let click = |h: &mut Harness<'static, CadApp>| {
        h.event(egui::Event::PointerMoved(centre));
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: centre, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.run_steps(1);
        }
        h.run_steps(2);
    };
    click(&mut h);
    assert_eq!(h.state_mut().run("ui.pomodoro", json!({})).unwrap()["running"], true);
    click(&mut h);
    assert_eq!(h.state_mut().run("ui.pomodoro", json!({})).unwrap()["running"], false);
    assert_eq!(h.state().session.doc().unwrap().model.iter().count(), 1, "the click did not reach the drawing");

    // Dragging to 3 o'clock sets 15 minutes.
    h.drag_at(centre + egui::vec2(0.0, -30.0));
    h.run_steps(2);
    h.hover_at(centre + egui::vec2(30.0, 0.0));
    h.run_steps(2);
    h.drop_at(centre + egui::vec2(30.0, 0.0));
    h.run_steps(3);
    let r = h.state_mut().run("ui.pomodoro", json!({})).unwrap();
    assert_eq!((r["label"].as_str(), r["minutes"].as_u64()), (Some("15:00"), Some(15)));

    let r = h.state_mut().run("ui.pomodoro", json!({"action": "set", "minutes": 47})).unwrap();
    assert_eq!((r["minutes"].as_u64(), r["label"].as_str()), (Some(45), Some("45:00")));
    assert!(h.state_mut().run("ui.pomodoro", json!({"action": "set"})).is_err());
    assert!(h.state_mut().run("ui.pomodoro", json!({"action": "explode"})).is_err());

    // Hiding the dial leaves the timer running.
    h.state_mut().run("ui.pomodoro", json!({"action": "start"})).unwrap();
    h.state_mut().run("ui.toggle.pomodoro", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state_mut().run("ui.pomodoro", json!({})).unwrap()["running"], true);
}

#[test]
fn importing_an_aqc_file_previews_then_applies_on_the_button() {
    let mut h = harness();
    let path = std::env::temp_dir().join(format!("aad-aqc-ui-{}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{"format":"hcw-aqc-bridge","version":1,"source":{"app":"AQC","drawing_id":"ui-1"},"units":"mm","project":{"name":"Rao House"}}"#,
    )
    .unwrap();
    aad_ui_egui::widgets::start_bridge_import(h.state_mut(), path.to_str().unwrap());
    h.run_steps(5);
    assert!(has(&h, "Overwrite fields already filled in Tren"), "the preview window is open");
    assert_eq!(h.state_mut().run("project", json!({})).unwrap()["details"]["title"], serde_json::Value::Null, "nothing applied yet");
    h.query_all_by_label("Apply").last().unwrap().click();
    h.run_steps(5);
    std::fs::remove_file(&path).ok();
    assert_eq!(h.state_mut().run("project", json!({})).unwrap()["details"]["title"], "Rao House");
    assert!(!has(&h, "Overwrite fields already filled in Tren"), "the window closes");
}

#[test]
fn menu_commands_that_need_input_open_their_dialog() {
    use egui::accesskit::Role;
    let mut h = harness();

    // Project details: type a title, press OK, and the drawing remembers it.
    h.state_mut().start("project");
    h.run_steps(5);
    assert!(has(&h, "Project title"), "the Project Details dialog is open");
    let n = h.query_all_by_role(Role::TextInput).count();
    h.query_all_by_role(Role::TextInput).nth(n - 13).unwrap().click();
    h.run_steps(2);
    h.query_all_by_role(Role::TextInput).nth(n - 13).unwrap().type_text("Rao House");
    h.run_steps(3);
    h.query_all_by_label("OK").last().unwrap().click();
    h.run_steps(5);
    assert!(!has(&h, "Project title"), "the dialog closes after OK");
    assert_eq!(h.state_mut().run("project", json!({})).unwrap()["details"]["title"], "Rao House");

    // Floors: a table, prefilled from the drawing.
    h.state_mut().start("floors");
    h.run_steps(5);
    assert!(has(&h, "Add row") && has(&h, "Floor names"));
    h.query_all_by_label("Cancel").last().unwrap().click();
    h.run_steps(3);
    assert!(!has(&h, "Add row"));

    // Room: the room type is required; OK without it says so and stays open.
    h.state_mut().start("room");
    h.run_steps(5);
    assert!(has(&h, "Room type"));
    h.query_all_by_label("OK").last().unwrap().click();
    h.run_steps(3);
    assert!(has(&h, "Room type"), "still open");
    assert!(h.query_all_by_label_contains("is required").next().is_some(), "the reason is shown");
}

#[test]
fn a_saved_panel_layout_from_before_the_layout_panel_was_removed_is_refused_not_a_crash() {
    use aad_ui_egui::docking::Workspace;
    // The default layout has no Layout tab and a saved one that names it cannot be read as a
    // layout: the loader (`apps/aad/src/workspace.rs`) then keeps the other preferences and
    // uses the default panel arrangement.
    let mut saved = serde_json::to_value(Workspace::default()).unwrap();
    assert!(!saved.to_string().contains("\"layout\":\"layout\"") && !saved.to_string().contains("\"layout\"]"));
    let text = saved.to_string().replace("\"blocks\"", "\"blocks\",\"layout\"");
    saved = serde_json::from_str(&text).unwrap();
    assert!(serde_json::from_value::<Workspace>(saved).is_err(), "an unknown panel name is an error");
    Workspace::default().validate().unwrap();
}

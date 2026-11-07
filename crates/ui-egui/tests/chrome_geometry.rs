//! The real app's painted geometry at native and compact window sizes.

use aad_engine::Session;
use aad_geom::Vec2;
use aad_ui_egui::{CadApp, Services};
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness(width: f32, height: f32, scale: f32, message: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    if message {
        app.set_status("Drawing saved successfully with a long status message");
    }
    let mut h = Harness::builder().with_size(vec2(width, height)).with_pixels_per_point(scale).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.canvas.cursor = Some(Vec2::new(1234.5, -9876.25));
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn visible_text(shape: &Shape, clip: Rect, result: &mut Vec<(String, Rect)>) {
    match shape {
        Shape::Text(text) => {
            let rect = text.visual_bounding_rect().intersect(clip);
            if rect.is_positive() {
                result.push((text.galley.text().to_string(), rect));
            }
        }
        Shape::Vec(shapes) => {
            for shape in shapes {
                visible_text(shape, clip, result);
            }
        }
        _ => {}
    }
}

#[test]
fn status_labels_never_overlap_at_standard_and_compact_sizes() {
    let mut overlaps = Vec::new();
    for (width, height) in [(1200.0, 760.0), (900.0, 560.0)] {
        for scale in [1.0, 2.0] {
            for message in [false, true] {
                let mut h = harness(width, height, scale, message);
                if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
                    std::fs::create_dir_all(&directory).unwrap();
                    h.render().unwrap().save(format!("{directory}/cad-{width}-{scale}-{message}.png")).unwrap();
                }
                let mut texts = Vec::new();
                for shape in &h.output().shapes {
                    visible_text(&shape.shape, shape.clip_rect, &mut texts);
                }
                texts.retain(|(_, rect)| rect.center().y > height - 26.0);
                // Paper layouts are no longer tabs in the status bar (a print block names a region of the
                // model). The left side shows the status message when there is one, else the cursor
                // coordinates the harness sets.
                let expected = if message { "Drawing saved" } else { "1234" };
                assert!(texts.iter().any(|(text, _)| text.contains(expected)), "status bar shows {expected:?}: {texts:?}");
                for (i, (first, first_rect)) in texts.iter().enumerate() {
                    for (second, second_rect) in texts.iter().skip(i + 1) {
                        if first_rect.intersects(*second_rect) {
                            overlaps.push(format!("{width}px @{scale}: {first:?} {first_rect:?} overlaps {second:?} {second_rect:?}"));
                        }
                    }
                }
            }
        }
    }
    assert!(overlaps.is_empty(), "{}", overlaps.join("\n"));
}

#[test]
fn compact_toolbar_overflow_keeps_the_last_tools_reachable() {
    let mut h = harness(900.0, 560.0, 1.0, false);
    h.get_by_label("More toolbar commands").click();
    h.run_steps(3);
    h.get_by_label("Area").click();
    h.run_steps(3);
    assert!(h.state().session.running.as_ref().is_some_and(|r| r.id == "area"), "the last tool of the bar starts from the overflow menu");
}

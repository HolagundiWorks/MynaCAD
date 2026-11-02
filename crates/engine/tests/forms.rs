//! Input forms: commands that take JSON are asked for on the command line and the drawing, and a
//! dialog can answer the settings first.

use aad_engine::doc::EntityKind;
use aad_engine::geom::Vec2;
use aad_engine::{Input, Session, forms};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s
}

fn count(s: &Session, layer: &str) -> usize {
    s.doc().unwrap().model.iter().filter(|e| e.common.layer == layer).count()
}

#[test]
fn a_dialog_answers_the_settings_and_the_points_are_picked() {
    let mut s = session();
    s.start_form("wall.draw", json!({"thickness": "200"})).unwrap();
    let p = s.current_prompt().unwrap();
    assert!(p.message.contains("pick points"), "{}", p.message);
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(4000.0, 0.0))).unwrap();
    assert_eq!(count(&s, "A-WALL"), 0, "nothing is drawn until the points are finished");
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "the command finished");
    assert_eq!(count(&s, "A-WALL"), 4, "two faces and two end caps");
    // One undo removes the whole wall.
    s.undo().unwrap();
    assert_eq!(count(&s, "A-WALL"), 0);

    s.start_form("wall.draw", json!({"thickness": "200"})).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none() && count(&s, "A-WALL") == 0, "Enter with no points cancels");
}

#[test]
fn the_command_line_asks_for_what_is_missing() {
    let mut s = session();
    s.start("stair.plan").unwrap();
    assert!(s.current_prompt().unwrap().message.contains("rise"), "asks the required fields in order after the choice default");
    // `kind` is optional and has a default, so the first question is the rise.
    s.input(Input::Text("3000".into())).unwrap();
    s.input(Input::Text("1100".into())).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("Start of the stair"));
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(s.running.is_none());
    assert!(count(&s, "A-STAIR") > 10);

    // A bad answer is refused and the question stays.
    s.start("stair.plan").unwrap();
    s.input(Input::Text("tall".into())).ok();
    assert!(s.current_prompt().unwrap().message.contains("rise"), "a bad answer keeps the question");
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
}

#[test]
fn optional_points_can_be_skipped_with_enter() {
    let mut s = session();
    s.start("grid.axis").unwrap();
    s.input(Input::Text("4000 5000".into())).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("origin"), "{:?}", s.current_prompt());
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert!(count(&s, "AN-GRID") > 0);
    s.execute("grid.axis", &json!({"bays": [4000, 5000], "rows": [3000, 3000]})).unwrap();
    // Columns: size first, then positions; Enter takes every grid intersection.
    s.start_form("column.place", json!({"size": "300"})).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    let cols = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::LwPolyline(_)) && e.common.layer == "A-COL").count();
    assert!(cols >= 3, "{cols}");
}

#[test]
fn forms_exist_for_the_commands_people_open_from_menus() {
    for id in [
        "project",
        "floors",
        "beams",
        "schedule",
        "room",
        "wall.draw",
        "wall.door",
        "wall.window",
        "grid.axis",
        "column.place",
        "stair.plan",
        "level.mark",
        "takeoff.brick",
        "bridge.export",
    ] {
        assert!(forms::get(id).is_some(), "{id}");
    }
    let s = session();
    let f = forms::get("floors").unwrap();
    let pre = forms::prefill(&s, f);
    assert_eq!(pre["floors"][0]["name"], "Ground", "the dialog starts from the drawing's floors");
    let room = forms::get("room").unwrap();
    // The room form asks for the room (from a list) and picks the corners: the floor comes from the top bar and the
    // units, box, text height and font from the Room Label Settings, so none of those are asked here.
    assert!(room.has_picks() && room.dialog_fields().count() == 1);
    assert!(forms::room_types().iter().any(|(c, _)| *c == "br"));
}

#[test]
fn project_and_floors_forms_round_trip_through_their_commands() {
    let mut s = session();
    let f = forms::get("project").unwrap();
    let params =
        forms::parse_input(f, &json!({"title": "Rao House", "owner": "S. Rao", "siteArea": "186.5", "slabMm": "150", "beamMm": "450"})).unwrap();
    s.execute("project", &params).unwrap();
    assert_eq!(forms::prefill(&s, f)["title"], "Rao House");
    assert_eq!(forms::prefill(&s, f)["slabMm"], 150.0);
    let f = forms::get("floors").unwrap();
    let params = forms::parse_input(f, &json!({"floors": [["Ground"], ["First"]]})).unwrap();
    s.execute("floors", &params).unwrap();
    assert_eq!(forms::prefill(&s, f)["floors"].as_array().unwrap().len(), 2);
    let f = forms::get("takeoff.rates").unwrap();
    let params = forms::parse_input(f, &json!({"rates": [["Bricks", "m3", "5200"]]})).unwrap();
    s.execute("takeoff.rates", &params).unwrap();
    let rows = forms::prefill(&s, forms::get("takeoff.rates").unwrap());
    assert_eq!(rows["rates"][0]["takeoff"], "Bricks");
}

#[test]
fn level_forms_round_trip_through_their_commands() {
    let mut s = session();
    let names = |s: &mut Session| -> Vec<String> {
        s.execute("level.list", &json!({})).unwrap()["levels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
    };
    let new = forms::get("level.new").unwrap();
    s.execute("level.new", &forms::parse_input(new, &json!({"name": "Roof", "elevation": "6000", "height": ""})).unwrap()).unwrap();
    assert_eq!(names(&mut s).len(), 2, "the implicit level and Roof");
    let set = forms::get("level.set").unwrap();
    s.execute("level.set", &forms::parse_input(set, &json!({"level": "Roof", "name": "Terrace", "elevation": "6200"})).unwrap()).unwrap();
    let list = s.execute("level.list", &json!({})).unwrap();
    let terrace = list["levels"].as_array().unwrap().iter().find(|l| l["name"] == "Terrace").unwrap().clone();
    assert_eq!(terrace["elevation"], 6200.0);
    let del = forms::get("level.delete").unwrap();
    s.execute("level.delete", &forms::parse_input(del, &json!({"level": "Terrace", "force": true})).unwrap()).unwrap();
    assert_eq!(names(&mut s).len(), 1);
}

#[test]
fn the_trace_form_adds_and_edits_a_trace() {
    let mut s = session();
    s.execute("level.setup", &json!({"floors": ["Ground", "First"], "sheetSets": false})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    let f = forms::get("level.trace").unwrap();
    let go = |s: &mut Session, input: serde_json::Value| s.execute("level.trace", &forms::parse_input(f, &input).unwrap()).unwrap();
    let r = go(&mut s, json!({"add": "Ground", "fade": "25", "colour": "TRUE", "order": "above", "snap": true, "layers": "A-WALL"}));
    let t = r["traces"][0].clone();
    assert_eq!(
        (t["fade"].as_u64(), t["colour"].as_str(), t["order"].as_str(), t["snap"].as_bool()),
        (Some(25), Some("true"), Some("above"), Some(true))
    );
    assert_eq!(t["layers"], json!(["A-WALL"]));
    // The same dialog edits it: only what is typed changes, and `*` clears the layer filter.
    let r = go(&mut s, json!({"add": "Ground", "fade": "70", "colour": "grey", "order": "below", "snap": false, "layers": "*"}));
    let t = r["traces"][0].clone();
    assert_eq!(
        (t["fade"].as_u64(), t["colour"].as_str(), t["order"].as_str(), t["snap"].as_bool()),
        (Some(70), Some("grey"), Some("below"), Some(false))
    );
    assert!(t["layers"].is_null());
    assert!(forms::parse_input(f, &json!({"add": "Ground", "colour": "red"})).is_err());
}

/// Vertical extent of the A-WALL lines: the wall's thickness for a wall drawn along x.
fn wall_thickness(s: &Session) -> f64 {
    let ys: Vec<f64> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| e.common.layer == "A-WALL")
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some([l.a.y, l.b.y]) } else { None })
        .flatten()
        .collect();
    ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min)
}

#[test]
fn architecture_commands_take_a_length_in_any_unit() {
    // A millimetre drawing: every spelling of 304.8 mm gives the same wall.
    for t in ["1'", "12\"", "304.8mm", "30.48cm", "0.3048m", "1 ft", "304.8"] {
        let mut s = session();
        s.execute("wall.draw", &json!({"points": [[0, 0], [4000, 0]], "thickness": t})).unwrap_or_else(|e| panic!("{t}: {e}"));
        assert!((wall_thickness(&s) - 304.8).abs() < 1e-6, "{t}: {}", wall_thickness(&s));
    }
    // A metre drawing: a bare number is metres, a suffix converts.
    let mut m = Session::new();
    m.execute("drawing.units", &json!({"units": "m", "mode": "metadata"})).unwrap();
    m.execute("wall.draw", &json!({"points": [[0, 0], [4, 0]], "thickness": "230mm"})).unwrap();
    assert!((wall_thickness(&m) - 0.23).abs() < 1e-9, "{}", wall_thickness(&m));
    // Numbers still work, and a word that is not a length is left for the command to refuse.
    let mut n = session();
    n.execute("wall.draw", &json!({"points": [[0, 0], [4000, 0]], "thickness": 200})).unwrap();
    assert!((wall_thickness(&n) - 200.0).abs() < 1e-6);
}

#[test]
fn lists_and_fixed_unit_fields_take_units_too() {
    use aad_engine::annot::Unit;
    // Column size and grid bays are lists in the drawing's unit: `12"` and `300mm` in one call.
    let c = forms::normalize_lengths("column.place", &json!({"size": ["12\"", "300mm"]}), Some(Unit::Mm)).unwrap();
    assert_eq!(c["size"], json!([304.8, 300.0]));
    let g = forms::normalize_lengths("grid.axis", &json!({"bays": "4m, 5000mm, 15'"}), Some(Unit::Mm)).unwrap();
    assert_eq!(g["bays"], json!([4000.0, 5000.0, 4572.0]));
    assert!(forms::normalize_lengths("column.place", &json!({"size": [300, 450]}), Some(Unit::Mm)).is_none(), "numbers need no change");
    // Beam depths are millimetres whatever the drawing unit is.
    let v = forms::normalize_lengths("beams", &json!({"depths": ["1'", "450mm", "0.5m"]}), Some(aad_engine::annot::Unit::M));
    assert_eq!(v.unwrap()["depths"], json!([304.8, 450.0, 500.0]));
    // Level heights are metres.
    let l = forms::normalize_lengths("level.mark", &json!({"value": "3000mm"}), Some(aad_engine::annot::Unit::Mm)).unwrap();
    assert_eq!(l["value"], json!(3.0));
}

#[test]
fn the_openings_table_takes_a_unit_in_each_length_cell() {
    let f = forms::get("schedule").expect("schedule form");
    let p = forms::parse_input(f, &json!({"openings": [["D1", "door", "", "900mm", "2100", "", "", ""], ["W1", "window", "", "3'", "1.2", "0.9m", "", ""]]})).unwrap();
    let rows = p["openings"].as_array().unwrap();
    assert_eq!(rows[0]["length"], json!(0.9));
    assert_eq!(rows[0]["height"], json!(2100.0), "a bare number stays in metres");
    assert!((rows[1]["length"].as_f64().unwrap() - 0.9144).abs() < 1e-9);
    assert_eq!(rows[1]["sill"], json!(0.9));
    let e = forms::parse_input(f, &json!({"openings": [["D1", "door", "", "wide", "", "", "", ""]]})).unwrap_err().to_string();
    assert!(e.contains("not a length"), "{e}");
}

#[test]
fn the_dialog_and_prompts_take_units_and_say_what_is_wrong() {
    let f = forms::get("wall.draw").unwrap();
    let p = aad_engine::units::with_drawn_unit(Some(aad_engine::annot::Unit::Mm), || {
        forms::parse_input(f, &json!({"thickness": "9\"", "points": [[0, 0], [1, 0]]}))
    })
    .unwrap();
    assert_eq!(p["thickness"], json!(228.6));
    let e = forms::parse_input(f, &json!({"thickness": "nine", "points": [[0, 0], [1, 0]]})).unwrap_err().to_string();
    assert!(e.contains("not a length"), "{e}");
    // A plain number field still says number.
    let t = forms::get("level.trace").unwrap();
    let e = forms::parse_input(t, &json!({"add": "L2", "fade": "x"})).unwrap_err().to_string();
    assert!(e.contains("not a number"), "{e}");
}

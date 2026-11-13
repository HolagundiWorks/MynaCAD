//! Sections and elevations: view spaces, markers, guides, draft and check.

use aad_engine::Session;
use serde_json::{Value, json};

/// Millimetre drawing, Ground (0, 3000 high) and First (3150, 3000 high); on Ground a 4 m wall of
/// two faces 230 apart crossing y = 0 (x = 4000 and x = 4230), with a door in it. Ground is active.
fn project() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground", "elevation": 3150.0, "height": 3000.0})).unwrap();
    s.execute("level.set", &json!({"level": "Ground", "elevation": 0.0, "height": 3000.0})).unwrap();
    s.execute("wall.draw", &json!({"points": [[4115.0, -3000.0], [4115.0, 3000.0]], "thickness": 230.0})).unwrap();
    s
}

fn section(s: &mut Session) -> Value {
    s.execute("section.new", &json!({"p1": [0.0, 0.0], "p2": [10000.0, 0.0], "depth": 5000.0})).unwrap()
}

fn guides(s: &mut Session, view: &str) -> Vec<Value> {
    s.execute("view.guides", &json!({"view": view})).unwrap()["lines"].as_array().unwrap().clone()
}

fn xs_of(g: &[Value], kind: &str) -> Vec<f64> {
    let mut v: Vec<f64> = g.iter().filter(|l| l["kind"] == kind).filter_map(|l| l["a"][0].as_f64()).collect();
    v.sort_by(f64::total_cmp);
    v.dedup();
    v
}

#[test]
fn a_section_is_created_with_a_linked_marker_and_does_not_switch_to_it() {
    let mut s = project();
    let r = section(&mut s);
    assert_eq!(r["name"], "Section A-A");
    assert!(r["handle"].is_string(), "the marker is linked by handle");
    let d = s.doc().unwrap();
    assert_eq!(d.view_spaces.len(), 1);
    assert_eq!(d.active_view, None, "creating from JSON does not switch");
    assert_eq!(d.active_level_ref().unwrap().name, "Ground");
    // The next one takes the next letter.
    let r2 = s.execute("section.new", &json!({"p1": [0.0, 5.0], "p2": [10000.0, 5.0]})).unwrap();
    assert_eq!(r2["name"], "Section B-B");
    let l = s.execute("view.list", &json!({})).unwrap();
    assert_eq!(l["views"].as_array().unwrap().len(), 2);
}

#[test]
fn creating_from_a_drawing_without_levels_makes_the_level_explicit() {
    let mut s = Session::new();
    s.execute("elevation.new", &json!({"at": [0.0, 0.0], "angle": 90.0})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.has_levels() && d.view_spaces.len() == 1);
    assert_eq!(d.view_spaces[0].rec.name, "Elevation A");
}

#[test]
fn guides_show_levels_and_the_cut_wall_and_follow_a_moved_wall() {
    let mut s = project();
    section(&mut s);
    let g = guides(&mut s, "Section A-A");
    let floors: Vec<f64> = g.iter().filter(|l| l["kind"] == "floor").map(|l| l["a"][1].as_f64().unwrap()).collect();
    assert!(floors.contains(&0.0) && floors.contains(&3150.0), "{floors:?}");
    let walls = xs_of(&g, "wall");
    assert_eq!(walls.len(), 2, "both faces: {walls:?}");
    assert!((walls[1] - walls[0] - 230.0).abs() < 1e-6);
    // Move the wall on the plan 150 mm: its guide moves, nothing else does.
    let hs: Vec<String> = s.doc().unwrap().model.iter().filter(|e| e.common.layer == "A-WALL").map(|e| e.handle.hex()).collect();
    s.execute("move", &json!({"handles": hs, "delta": [150.0, 0.0]})).unwrap();
    let after = xs_of(&guides(&mut s, "Section A-A"), "wall");
    assert!((after[0] - walls[0] - 150.0).abs() < 1e-6, "{after:?}");
}

#[test]
fn draft_adds_objects_once_and_check_reports_a_wall_moved_since() {
    let mut s = project();
    section(&mut s);
    let r = s.execute("section.draft", &json!({"view": "Section A-A"})).unwrap();
    assert!(r["added"].as_u64().unwrap() > 0);
    assert_eq!(s.doc().unwrap().active_view, None, "draft returns to where it was");
    let n = s.doc().unwrap().view_store(s.doc().unwrap().view_spaces[0].rec.id).unwrap().len();
    assert!(n > 0);
    let again = s.execute("section.draft", &json!({"view": "Section A-A"})).unwrap();
    assert_eq!(again["added"], 0, "running again adds nothing new");
    let clean = s.execute("section.check", &json!({"view": "Section A-A"})).unwrap();
    assert_eq!(clean["gaps"].as_array().unwrap().len(), 0, "{clean}");
    // Plan change.
    let hs: Vec<String> = s.doc().unwrap().model.iter().filter(|e| e.common.layer == "A-WALL").map(|e| e.handle.hex()).collect();
    s.execute("move", &json!({"handles": hs, "delta": [150.0, 0.0]})).unwrap();
    let bad = s.execute("section.check", &json!({"view": "Section A-A"})).unwrap();
    let gaps = bad["gaps"].as_array().unwrap();
    assert!(!gaps.is_empty(), "{bad}");
    assert!(gaps.iter().any(|g| (g["gap"].as_f64().unwrap_or(0.0) - 150.0).abs() < 1e-6), "the gap is the 150 mm the wall moved: {bad}");
    // Drafted objects did not move, and check changed nothing.
    let n2 = s.doc().unwrap().view_store(s.doc().unwrap().view_spaces[0].rec.id).unwrap().len();
    assert_eq!(n, n2);
    // Draft again adds the new position only.
    let more = s.execute("section.draft", &json!({"view": "Section A-A"})).unwrap();
    assert!(more["added"].as_u64().unwrap() > 0 && more["added"].as_u64().unwrap() < n as u64 + 3);
}

#[test]
fn moving_the_marker_moves_the_view_and_deleting_it_is_noted() {
    let mut s = project();
    let r = section(&mut s);
    let h = r["handle"].as_str().unwrap().to_string();
    s.execute("move", &json!({"handles": [h.clone()], "delta": [0.0, 500.0]})).unwrap();
    let a = s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    assert!(a["note"].is_string());
    let line = s.doc().unwrap().view_spaces[0].rec.line;
    assert!((line.0.y - 500.0).abs() < 1e-6 && (line.1.y - 500.0).abs() < 1e-6, "{line:?}");
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    assert_eq!(s.doc().unwrap().active_view, None, "choosing the level leaves the view");
    s.execute("erase", &json!({"handles": [h]})).unwrap();
    let a = s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    assert!(a["note"].as_str().unwrap().contains("deleted"));
    assert!(s.doc().unwrap().view_spaces[0].rec.marker.is_none());
}

#[test]
fn activate_draw_switch_and_undo() {
    let mut s = project();
    section(&mut s);
    s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    s.execute("line", &json!({"points": [[0.0, 0.0], [100.0, 100.0]]})).unwrap();
    let id = s.doc().unwrap().view_spaces[0].rec.id;
    assert_eq!(s.doc().unwrap().view_store(id).unwrap().len(), 1);
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    assert_eq!(s.doc().unwrap().view_store(id).unwrap().len(), 1, "the drafted line waits in the view");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().active_view, Some(id), "undo returns to the view");
    // Delete refuses a view with objects unless forced.
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    assert!(s.execute("view.delete", &json!({"view": "Section A-A"})).is_err());
    s.execute("view.delete", &json!({"view": "Section A-A", "force": true})).unwrap();
    assert!(s.doc().unwrap().view_spaces.is_empty());
}

#[test]
fn deleting_a_level_removes_its_views_and_purge_audit_and_layer_walks_see_view_objects() {
    let mut s = project();
    section(&mut s);
    s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    s.execute("layer.new", &json!({"name": "ONLYINVIEW", "current": true})).unwrap();
    s.execute("circle", &json!({"center": [10.0, 10.0], "radius": 5.0})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    assert!(s.execute("layer.delete", &json!({"name": "ONLYINVIEW"})).is_err(), "the layer is in use inside the view");
    s.execute("audit", &json!({})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    s.execute("level.delete", &json!({"level": "Ground", "force": true})).unwrap();
    assert!(s.doc().unwrap().view_spaces.is_empty(), "the view of a deleted level goes with it");
}

#[test]
fn an_elevation_sees_the_wall_in_its_depth_and_looks_the_other_way_to_see_nothing() {
    let mut s = project();
    // North elevation from y = -6000 looking up (+y) at the wall that spans y -3000..3000.
    s.execute("elevation.new", &json!({"at": [4115.0, -6000.0], "angle": 90.0, "depth": 10000.0, "width": 8000.0})).unwrap();
    let g = guides(&mut s, "Elevation A");
    assert!(!xs_of(&g, "wall").is_empty(), "wall edges are seen: {g:?}");
    s.execute("view.edit", &json!({"view": "Elevation A", "depth": 1000.0})).unwrap();
    assert!(xs_of(&guides(&mut s, "Elevation A"), "wall").is_empty(), "beyond the depth");
    assert!(s.execute("view.edit", &json!({"view": "Elevation A", "depth": 0.0})).is_err());
    assert!(s.execute("view.edit", &json!({"view": "Elevation A", "depth": "NaN"})).is_err());
}

#[test]
fn views_round_trip_through_dxf_with_links_intact() {
    let mut s = project();
    let r = section(&mut s);
    s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    s.execute("line", &json!({"points": [[0.0, 0.0], [100.0, 100.0]]})).unwrap();
    let bytes = aad_io::write(s.doc().unwrap(), "v.dxf").unwrap();
    let back = aad_io::read(&bytes, "v.dxf").unwrap();
    assert_eq!(back.view_spaces.len(), 1);
    let v = &back.view_spaces[0];
    assert_eq!(v.rec.marker.map(|h| h.hex()), r["handle"].as_str().map(str::to_string));
    assert_eq!(back.view_store(v.rec.id).unwrap().len(), 1);
    assert_eq!(back.active_view, Some(v.rec.id));
    // The marker is still on the plan and the rest is unchanged.
    assert_eq!(back.entity_count(), s.doc().unwrap().entity_count());
    // A reopened drawing computes the same guides.
    install_io();
    let mut s2 = Session::new();
    s2.execute("open", &json!({"data": aad_engine::cmd::file::base64_encode(&bytes), "name": "v.dxf"})).unwrap();
    let g = guides(&mut s2, "Section A-A");
    assert_eq!(xs_of(&g, "wall").len(), 2);
}

fn install_io() {
    use aad_engine::cmd::file::{IoHooks, set_io};
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        set_io(IoHooks {
            read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
            plot: None,
        });
    });
}

#[test]
fn hostile_inputs_are_refused_not_crashes() {
    let mut s = project();
    let bad = [
        json!({"p1": [0.0, 0.0], "p2": [0.0, 0.0]}),
        json!({"p1": [0.0, 0.0], "p2": [1e308, 1e308]}),
        json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "depth": -5.0}),
        json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "depth": 0.0}),
        json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "look": "up"}),
        json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "label": "TOOLONG"}),
        json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "name": ""}),
        json!({"p1": "x", "p2": [1, 2]}),
        json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "level": "Nope"}),
    ];
    for p in &bad {
        assert!(s.execute("section.new", p).is_err(), "{p}");
    }
    assert!(s.doc().unwrap().view_spaces.is_empty(), "a refusal leaves no view behind");
    for p in [
        json!({"at": [0.0, 0.0], "depth": 0.0}),
        json!({"at": [f64::NAN, 0.0]}),
        json!({"at": [0.0, 0.0], "width": -1.0}),
        json!({"at": [0.0, 0.0], "angle": 1e300}),
    ] {
        assert!(s.execute("elevation.new", &p).is_err(), "{p}");
    }
    section(&mut s);
    assert!(s.execute("section.new", &json!({"p1": [0.0, 0.0], "p2": [1000.0, 0.0], "name": "section a-a"})).is_err(), "names are unique");
    for c in ["view.edit", "view.delete", "view.activate", "view.trace", "view.guides", "section.draft", "section.check", "section.levelmarks"] {
        for p in [
            json!({}),
            json!({"view": "nope"}),
            json!({"view": 99999}),
            json!({"view": [1]}),
            json!({"view": "Section A-A", "depth": "x", "tolerance": -1, "slabThickness": -3}),
        ] {
            let _ = s.execute(c, &p);
        }
    }
    assert!(s.execute("view.trace", &json!({"view": "Section A-A", "add": "Section A-A"})).is_err(), "no self trace");
    assert!(s.execute("section.link", &json!({"handle": "zz"})).is_err());
    // The cap on views.
    for i in 0..aad_doc::MAX_VIEW_SPACES + 2 {
        let _ = s.execute("section.new", &json!({"p1": [0.0, i as f64], "p2": [1000.0, i as f64], "name": format!("v{i}"), "label": "A"}));
    }
    assert_eq!(s.doc().unwrap().view_spaces.len(), aad_doc::MAX_VIEW_SPACES);
}

#[test]
fn link_adopts_a_loose_line_once() {
    let mut s = project();
    let r = s.execute("line", &json!({"points": [[0.0, 100.0], [8000.0, 100.0]]})).unwrap();
    let h = r["handles"][0].as_str().or_else(|| r["handle"].as_str()).unwrap().to_string();
    let v = s.execute("section.link", &json!({"handle": h, "name": "Linked"})).unwrap();
    assert_eq!(v["name"], "Linked");
    assert!(s.execute("section.link", &json!({"handle": h})).is_err(), "one view per marker");
}

#[test]
fn levelmarks_and_traces_between_views() {
    let mut s = project();
    section(&mut s);
    s.execute("section.new", &json!({"p1": [0.0, 800.0], "p2": [10000.0, 800.0]})).unwrap();
    let r = s.execute("section.levelmarks", &json!({"view": "Section A-A"})).unwrap();
    assert!(r["placed"].as_u64().unwrap() >= 2);
    let t = s.execute("view.trace", &json!({"view": "Section A-A", "add": "Section B-B"})).unwrap();
    assert_eq!(t["traces"].as_array().unwrap().len(), 1);
    s.execute("view.delete", &json!({"view": "Section B-B"})).unwrap();
    let t = s.execute("view.trace", &json!({"view": "Section A-A"})).unwrap();
    assert!(t["traces"].as_array().unwrap().is_empty(), "deleting a view removes traces of it");
}

#[test]
fn guides_snap_in_an_open_view_but_cannot_be_picked_and_are_not_saved() {
    use aad_engine::doc::Space;
    use aad_engine::geom::Vec2;
    use aad_engine::snap::{mode, osnap};
    let mut s = project();
    section(&mut s);
    // Not open yet: the plan's wall faces are not on offer in model space of Ground at the view's coordinates.
    s.execute("view.activate", &json!({"view": "Section A-A"})).unwrap();
    // The cut wall face is a vertical guide at x = 4000 from 0 to 3000: its top end is a snap point.
    let hit = osnap(s.doc().unwrap(), &Space::Model, Vec2::new(10005.0, 3003.0), 20.0, mode::END, None, false);
    assert_eq!(hit.map(|h| h.point), Some(Vec2::new(10000.0, 3000.0)), "the end of the top-of-storey guide");
    assert!(aad_engine::select::pick(s.doc().unwrap(), &Space::Model, Vec2::new(4000.0, 1500.0), 20.0).is_none(), "guides are not selectable");
    assert_eq!(s.doc().unwrap().model.len(), 0, "and are not objects of the view");
    let bytes = aad_io::write(s.doc().unwrap(), "g.dxf").unwrap();
    assert_eq!(aad_io::read(&bytes, "g.dxf").unwrap().entity_count(), s.doc().unwrap().entity_count(), "never saved");
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    let off = osnap(s.doc().unwrap(), &Space::Model, Vec2::new(10005.0, 3003.0), 20.0, mode::END, None, false);
    assert_ne!(off.map(|h| h.point), Some(Vec2::new(10000.0, 3000.0)), "off again once the view is left");
}

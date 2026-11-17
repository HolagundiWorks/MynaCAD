//! Levels: named model spaces in one drawing. Phase 1 core (types, switching, commands).

use aad_doc::EntityKind;
use aad_engine::Session;
use serde_json::{Value, json};

fn line(s: &mut Session, y: f64) {
    s.execute("line", &json!({"points": [[0.0, y], [10.0, y]]})).unwrap();
}

fn levels(s: &mut Session) -> Vec<Value> {
    s.execute("level.list", &json!({})).unwrap()["levels"].as_array().unwrap().clone()
}

fn objects(v: &Value) -> u64 {
    v["objects"].as_u64().unwrap()
}

#[test]
fn a_drawing_without_levels_is_one_implicit_level_until_the_first_new_level() {
    let mut s = Session::new();
    line(&mut s, 0.0);
    let r = s.execute("level.list", &json!({})).unwrap();
    assert_eq!(r["implicit"], true);
    assert_eq!(r["levels"][0]["name"], "Level 1");
    assert_eq!(objects(&r["levels"][0]), 1);
    assert!(!s.doc().unwrap().has_levels(), "listing never changes the drawing");

    s.execute("level.new", &json!({"name": "First", "elevation": 3000.0, "height": 3000.0})).unwrap();
    let ls = levels(&mut s);
    assert_eq!(ls.len(), 2);
    assert_eq!((ls[0]["name"].as_str(), ls[0]["active"].as_bool(), objects(&ls[0])), (Some("Level 1"), Some(true), 1));
    assert_eq!((ls[1]["name"].as_str(), ls[1]["active"].as_bool(), objects(&ls[1])), (Some("First"), Some(false), 0));
    assert_eq!(ls[1]["elevation"], 3000.0);
}

#[test]
fn switching_levels_keeps_each_levels_objects_and_undo_restores_the_tab() {
    let mut s = Session::new();
    line(&mut s, 0.0);
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0, "the new level starts empty");
    line(&mut s, 5.0);
    line(&mut s, 6.0);

    let ls = levels(&mut s);
    assert_eq!((objects(&ls[0]), objects(&ls[1])), (1, 2));
    assert_eq!(s.doc().unwrap().entity_count(), 3, "drawing-wide counts see every level");

    // A handle on the inactive level is still found, but is not part of the level being edited.
    let ground_handle = s.doc().unwrap().level_store(aad_engine::doc::LevelId(1)).unwrap().iter().next().unwrap().handle;
    assert!(s.doc().unwrap().entity(ground_handle).is_some());
    assert!(!s.doc().unwrap().model.contains(ground_handle));
    let before = s.doc().unwrap().entity(ground_handle).unwrap().clone();
    let moved = s.execute("move", &json!({"handles": [ground_handle.hex()], "from": [0, 0], "to": [1, 1]}));
    assert!(moved.map(|r| r["moved"] == 0).unwrap_or(true), "an object on another level is not moved from this one");
    assert_eq!(*s.doc().unwrap().entity(ground_handle).unwrap(), before, "and it is unchanged");

    s.execute("level.activate", &json!({"level": 1})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().active_level_ref().unwrap().name, "First", "undo returns to the tab the user was on");
    assert_eq!(s.doc().unwrap().model.len(), 2);
}

#[test]
fn levels_are_renamed_reordered_and_names_stay_unique() {
    let mut s = Session::new();
    s.execute("level.new", &json!({"name": "Ground", "first": "Basement"})).unwrap();
    s.execute("level.new", &json!({"name": "Terrace"})).unwrap();
    s.execute("level.new", &json!({"name": "First", "after": "Ground"})).unwrap();
    let names = |s: &mut Session| levels(s).iter().map(|l| l["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&mut s), ["Basement", "Ground", "First", "Terrace"]);

    assert!(s.execute("level.new", &json!({"name": "ground"})).is_err(), "names are unique ignoring case");
    assert!(s.execute("level.set", &json!({"level": "First", "name": "BASEMENT"})).is_err());
    s.execute("level.set", &json!({"level": "Terrace", "name": "Roof", "elevation": 9000.0, "order": 0})).unwrap();
    assert_eq!(names(&mut s), ["Roof", "Basement", "Ground", "First"]);
    assert_eq!(levels(&mut s)[0]["elevation"], 9000.0);
    s.execute("level.set", &json!({"level": "Roof", "name": "Roof"})).unwrap();
}

#[test]
fn deleting_a_level_needs_force_when_it_holds_objects_and_the_last_one_stays() {
    let mut s = Session::new();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    line(&mut s, 0.0);
    assert!(s.execute("level.delete", &json!({"level": "Ground"})).is_ok(), "an empty level goes without force");
    assert_eq!(levels(&mut s).len(), 1);
    assert!(s.execute("level.delete", &json!({"level": "First", "force": true})).is_err(), "the last level cannot be deleted");

    s.execute("level.new", &json!({"name": "Second"})).unwrap();
    let r = s.execute("level.delete", &json!({"level": "First"}));
    assert!(r.is_err(), "First holds an object");
    let r = s.execute("level.delete", &json!({"level": "First", "force": true})).unwrap();
    assert_eq!(r["active"], "Second", "deleting the active level activates its neighbour");
    assert_eq!(s.doc().unwrap().entity_count(), 0);
}

#[test]
fn hostile_level_parameters_never_panic() {
    let mut s = Session::new();
    s.execute("level.new", &json!({"name": "A", "first": "B"})).unwrap();
    let long = "x".repeat(500);
    for (cmd, p) in [
        ("level.new", json!({})),
        ("level.new", json!({"name": ""})),
        ("level.new", json!({"name": long})),
        ("level.new", json!({"name": "ctl\u{7}"})),
        ("level.new", json!({"name": "N", "elevation": "high"})),
        ("level.new", json!({"name": "N", "height": -1.0})),
        ("level.new", json!({"name": "N", "after": "nope"})),
        ("level.set", json!({"level": u64::MAX, "name": "Z"})),
        ("level.set", json!({"level": "A", "order": u64::MAX})),
        ("level.set", json!({"level": "A", "order": -1})),
        ("level.set", json!({"level": "A", "height": f64::NAN})),
        ("level.set", json!({"level": [1]})),
        ("level.delete", json!({"level": null})),
        ("level.activate", json!({"level": "-1"})),
        ("level.activate", json!({})),
    ] {
        let _ = s.execute(cmd, &p);
    }
    let n = levels(&mut s).len();
    assert!((2..=3).contains(&n), "{n} levels");

    // The 64-level cap holds on the JSON path.
    for i in 0..80 {
        let _ = s.execute("level.new", &json!({"name": format!("L{i}")}));
    }
    assert_eq!(levels(&mut s).len(), 64);
}

/// Two levels: `Ground` (active) and `First`, with a block `B` whose only use is on `First`.
fn block_used_only_on_the_other_level() -> Session {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "S-WALL"})).unwrap();
    s.execute("layer.new", &json!({"name": "S-ONLY-FIRST"})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    s.execute("layer.current", &json!({"name": "S-ONLY-FIRST"})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap();
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle;
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": [h.hex()], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "B", "at": [1, 1]})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    s
}

#[test]
fn purge_and_layer_delete_see_the_objects_of_every_level() {
    let mut s = block_used_only_on_the_other_level();
    s.execute("purge", &json!({"type": "blocks"})).ok();
    assert!(s.doc().unwrap().block("B").is_some(), "a block used only on another level is not purged");
    assert!(s.execute("layer.delete", &json!({"name": "S-ONLY-FIRST"})).is_err(), "a layer used only on another level is in use");
    assert_eq!(s.doc().unwrap().entity_count(), 1);
}

#[test]
fn renaming_a_layer_repoints_objects_on_every_level() {
    let mut s = block_used_only_on_the_other_level();
    s.execute("layer.set", &json!({"name": "S-WALL", "newName": "S-WALL-2"})).unwrap();
    // the layer the other level's block lives on keeps working after a rename of an unrelated layer
    s.execute("layer.set", &json!({"name": "S-ONLY-FIRST", "newName": "S-FIRST"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    let layers: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.common.layer.clone()).collect();
    assert!(layers.iter().all(|l| l != "S-ONLY-FIRST"), "{layers:?}");
}

#[test]
fn audit_and_extents_do_not_lose_the_other_levels() {
    let mut s = block_used_only_on_the_other_level();
    let r = s.execute("audit", &json!({})).unwrap();
    assert!(r.get("error").is_none());
    assert_eq!(s.doc().unwrap().entity_count(), 1, "audit does not drop objects on a level it is not editing");
}

#[test]
fn levels_round_trip_through_a_dxf_file() {
    let mut s = Session::new();
    line(&mut s, 0.0);
    s.execute("level.new", &json!({"name": "First", "first": "Ground", "elevation": 3000.0})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    line(&mut s, 5.0);
    let bytes = aad_io::write(s.doc().unwrap(), "a.dxf").unwrap();
    assert!(aad_io::write(s.doc().unwrap(), "a.svg").is_ok(), "images show the active level");

    let back = aad_io::read(&bytes, "a.dxf").unwrap();
    assert_eq!(back.levels.len(), 2);
    assert_eq!(back.active_level_ref().unwrap().name, "First");
    assert_eq!(back.entity_count(), 2);
    assert_eq!(back.level_by_name("First").unwrap().elevation, 3000.0);
}

fn xs(s: &Session, level: &str) -> usize {
    let d = s.doc().unwrap();
    d.level_store(d.level_by_name(level).unwrap().id).unwrap().len()
}

#[test]
fn objects_move_and_copy_between_levels_at_the_same_coordinates() {
    let mut s = Session::new();
    for y in [0.0, 1.0, 2.0] {
        line(&mut s, y);
    }
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.new", &json!({"name": "Roof"})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();

    let r = s.execute("level.copyto", &json!({"levels": ["First", "Roof"], "handles": [hs[0], hs[1]]})).unwrap();
    assert_eq!(r["copied"], 4);
    assert_eq!((xs(&s, "Ground"), xs(&s, "First"), xs(&s, "Roof")), (3, 2, 2));
    let d = s.doc().unwrap();
    let first = d.level_store(d.level_by_name("First").unwrap().id).unwrap();
    assert!(first.iter().all(|e| !d.model.contains(e.handle)), "copies get new handles");

    let r = s.execute("level.moveto", &json!({"level": "Roof", "handles": [hs[2]]})).unwrap();
    assert_eq!(r["moved"], 1);
    assert_eq!((xs(&s, "Ground"), xs(&s, "Roof")), (2, 3));
    assert_eq!(s.doc().unwrap().entity_count(), 7);

    assert!(s.execute("level.moveto", &json!({"level": "Ground", "handles": [hs[0]]})).is_err(), "already there");
    assert!(s.execute("level.copyto", &json!({"levels": ["Ground"], "handles": [hs[0]]})).is_err(), "the source level");
    assert!(s.execute("level.copyto", &json!({"levels": [], "handles": [hs[0]]})).is_err());
    assert!(s.execute("level.moveto", &json!({"level": "First", "handles": ["FFFFFF"]})).is_err(), "nothing of this level");

    s.execute("undo", &json!({})).unwrap();
    assert_eq!((xs(&s, "Ground"), xs(&s, "Roof")), (3, 2), "undo restores the move");
}

#[test]
fn send_to_level_shifts_by_origin_match_defaulting_to_site() {
    let mut s = Session::new();
    line(&mut s, 100.0);
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("site.origin", &json!({ "point": [0.0, 0.0] })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    // Line sits at y=100; pick origin (0,100) → site (0,0) so it lands on Ground's base.
    let r = s.execute("level.moveto", &json!({ "level": "First", "handles": [h], "from": [0.0, 100.0] })).unwrap();
    assert_eq!(r["moved"], 1);
    assert_eq!(r["shifted"], json!([0.0, -100.0]));
    let d = s.doc().unwrap();
    let first = d.level_store(d.level_by_name("First").unwrap().id).unwrap();
    let EntityKind::Line(geom) = &first.iter().next().unwrap().kind else { panic!("expected a line") };
    assert!((geom.a.y - 0.0).abs() < 1e-9 && (geom.b.y - 0.0).abs() < 1e-9, "shifted onto site origin");

    s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
    line(&mut s, 50.0);
    let h2 = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let r = s.execute("level.copyto", &json!({ "level": "First", "handles": [h2], "from": [0.0, 50.0], "to": [10.0, 20.0] })).unwrap();
    assert_eq!(r["copied"], 1);
    assert_eq!(r["shifted"], json!([10.0, -30.0]));
    assert_eq!(xs(&s, "Ground"), 1, "source stays");
}

#[test]
fn objects_on_a_locked_layer_do_not_move_to_another_level() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "LOCKED"})).unwrap();
    s.execute("layer.current", &json!({"name": "LOCKED"})).unwrap();
    line(&mut s, 0.0);
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    s.execute("layer.set", &json!({"name": "LOCKED", "locked": true})).unwrap();
    s.execute("level.new", &json!({"name": "First"})).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    assert!(s.execute("level.moveto", &json!({"level": "First", "handles": [h]})).is_err());
    assert_eq!(s.doc().unwrap().model.len(), 1);
}

#[test]
fn levels_come_from_the_floor_list_and_a_rename_keeps_the_floor_data_in_step() {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    line(&mut s, 0.0);
    assert!(s.execute("level.fromfloors", &json!({})).is_err(), "no floor list yet");
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.0}, {"name": "First", "floorToFloor": 3.2}, {"name": "Terrace", "floorToFloor": 3.0}]})).unwrap();
    let r = s.execute("level.fromfloors", &json!({})).unwrap();
    assert_eq!(r["created"], json!(["Ground", "First", "Terrace"]));
    let ls = levels(&mut s);
    assert_eq!(ls.iter().map(|l| l["elevation"].as_f64().unwrap()).collect::<Vec<_>>(), [0.0, 3000.0, 6200.0]);
    assert_eq!(objects(&ls[0]), 1, "the existing objects stay on the first floor");
    assert_eq!(s.execute("level.fromfloors", &json!({})).unwrap()["created"], json!([]), "running it again adds nothing");

    s.execute("level.set", &json!({"level": "First", "name": "Mezzanine"})).unwrap();
    let names: Vec<String> =
        s.execute("floors", &json!({})).unwrap()["floors"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["Ground", "Mezzanine", "Terrace"]);
}

#[test]
fn audit_finds_and_repairs_errors_on_a_level_that_is_not_being_edited() {
    use aad_engine::doc::{Common, Entity, EntityKind, Line};
    use aad_engine::geom::Vec3;
    let mut s = Session::new();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    let first = s.doc().unwrap().level_by_name("First").unwrap().id;
    // An object on a layer that does not exist, on the inactive level.
    {
        let d = s.doc_mut().unwrap();
        let h = d.new_handle();
        let kind = EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) });
        let e = Entity { handle: h, common: Common { layer: "Ghost".into(), ..Common::default() }, kind };
        d.levels.iter_mut().find(|l| l.id == first).unwrap().entities.push(e);
    }
    let r = s.execute("audit", &json!({})).unwrap();
    assert!(r["errors"].as_u64().unwrap() >= 1, "the error on the other level is reported: {r}");
    s.execute("audit", &json!({"fix": true})).unwrap();
    assert!(s.doc().unwrap().layer("Ghost").is_some(), "and repaired");
    assert_eq!(s.execute("audit", &json!({})).unwrap()["errors"], 0);
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
fn opening_a_file_with_damaged_level_data_tells_the_user_and_still_opens() {
    use aad_engine::cmd::file::base64_encode;
    install_io();
    let mut src = Session::new();
    line(&mut src, 0.0);
    // Level metadata that is not valid JSON, as a damaged or hand-edited file might carry.
    src.doc_mut().unwrap().project.insert("levels2".into(), "{ not json".into());
    let bytes = aad_io::write(src.doc().unwrap(), "damaged.dxf").unwrap();

    let mut s = Session::new();
    s.execute("open", &json!({"data": base64_encode(&bytes), "name": "damaged.dxf"})).unwrap();
    assert_eq!(s.doc().unwrap().entity_count(), 1, "the drawing opens");
    assert!(!s.doc().unwrap().has_levels(), "as one level");
    assert!(s.log.iter().any(|m| m.contains("level data that could not be read")), "{:?}", s.log);
    assert!(!s.doc().unwrap().project.contains_key("levelsNote"), "the note is not kept in the drawing");
}

#[test]
fn a_healthy_multi_level_file_opens_without_a_warning() {
    use aad_engine::cmd::file::base64_encode;
    install_io();
    let mut src = Session::new();
    line(&mut src, 0.0);
    src.execute("level.new", &json!({"name": "First"})).unwrap();
    let bytes = aad_io::write(src.doc().unwrap(), "ok.dxf").unwrap();
    let mut s = Session::new();
    s.execute("open", &json!({"data": base64_encode(&bytes), "name": "ok.dxf"})).unwrap();
    assert_eq!(s.doc().unwrap().levels.len(), 2);
    assert!(!s.log.iter().any(|m| m.contains("level")), "{:?}", s.log);
}

/// The traces of a level, read from the drawing (so reading adds no undo step).
fn traces(s: &mut Session, on: &str) -> Vec<Value> {
    let d = s.doc().unwrap();
    let l = d.level_by_name(on).unwrap();
    l.traces
        .iter()
        .map(|t| {
            json!({
                "name": d.level(t.level).unwrap().name, "fade": t.fade, "snap": t.snap, "layers": t.layers,
                "colour": if t.colour == aad_engine::doc::TraceColour::Grey { "grey" } else { "true" },
                "order": if t.order == aad_engine::doc::TraceOrder::Below { "below" } else { "above" },
            })
        })
        .collect()
}

#[test]
fn a_level_can_trace_several_others_with_their_own_settings() {
    let mut s = Session::new();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.new", &json!({"name": "Terrace"})).unwrap();

    let r = s.execute("level.trace", &json!({"level": "First", "add": ["Ground", "Terrace"], "fade": 70, "snap": true})).unwrap();
    assert_eq!(r["level"], "First");
    let ts = r["traces"].as_array().unwrap();
    assert_eq!(ts.len(), 2);
    assert!(ts.iter().all(|t| t["fade"] == 70 && t["snap"] == true && t["colour"] == "grey" && t["order"] == "below"));

    s.execute("level.trace", &json!({"level": "First", "set": "Terrace", "colour": "true", "order": "above", "fade": 20, "layers": ["A-WALL"]}))
        .unwrap();
    let ts = traces(&mut s, "First");
    let terrace = ts.iter().find(|t| t["name"] == "Terrace").unwrap();
    assert_eq!((terrace["colour"].as_str(), terrace["order"].as_str(), terrace["fade"].as_u64()), (Some("true"), Some("above"), Some(20)));
    assert_eq!(terrace["layers"], json!(["A-WALL"]));
    assert_eq!(ts.iter().find(|t| t["name"] == "Ground").unwrap()["fade"], 70, "the other trace is untouched");

    // Adding again updates instead of duplicating; remove drops one.
    s.execute("level.trace", &json!({"level": "First", "add": "Ground", "fade": 10})).unwrap();
    assert_eq!(traces(&mut s, "First").len(), 2);
    s.execute("level.trace", &json!({"level": "First", "remove": "Ground"})).unwrap();
    assert_eq!(traces(&mut s, "First").len(), 1);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(traces(&mut s, "First").len(), 2, "one undo step");
}

#[test]
fn a_level_never_traces_itself_and_bad_settings_change_nothing() {
    let mut s = Session::new();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    assert!(s.execute("level.trace", &json!({"level": "First", "add": "First"})).is_err());
    assert!(s.execute("level.trace", &json!({"level": "First", "add": "Ground", "fade": 91})).is_err());
    assert!(s.execute("level.trace", &json!({"level": "First", "add": "Ground", "colour": "red"})).is_err());
    assert!(s.execute("level.trace", &json!({"level": "First", "set": "Ground"})).is_err(), "not traced yet");
    assert!(s.execute("level.trace", &json!({"level": "First", "add": "Nope"})).is_err());
    assert!(traces(&mut s, "First").is_empty());

    // The cap of 8 traces holds, and a failed add leaves no partial traces.
    for i in 0..9 {
        s.execute("level.new", &json!({"name": format!("L{i}")})).unwrap();
    }
    let all: Vec<String> = (0..9).map(|i| format!("L{i}")).collect();
    assert!(s.execute("level.trace", &json!({"level": "First", "add": all})).is_err());
    assert!(traces(&mut s, "First").is_empty());
    s.execute("level.trace", &json!({"level": "First", "add": &all[..8]})).unwrap();
    assert_eq!(traces(&mut s, "First").len(), 8);

    // Deleting a traced level removes the traces of it.
    s.execute("level.delete", &json!({"level": "L0", "force": true})).unwrap();
    assert_eq!(traces(&mut s, "First").len(), 7);
}

#[test]
fn hostile_trace_parameters_never_panic() {
    let mut s = Session::new();
    assert!(s.execute("level.trace", &json!({})).is_err(), "one level has nothing to trace");
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    let long = "x".repeat(400);
    for p in [
        json!({"add": null}),
        json!({"add": {"a": 1}}),
        json!({"add": [[]]}),
        json!({"add": "Ground", "fade": -1}),
        json!({"add": "Ground", "fade": u64::MAX}),
        json!({"add": "Ground", "fade": 1.5}),
        json!({"add": "Ground", "snap": "yes"}),
        json!({"add": "Ground", "layers": [1, 2]}),
        json!({"add": "Ground", "layers": [long]}),
        json!({"add": "Ground", "layers": "A"}),
        json!({"level": [1], "add": "Ground"}),
        json!({"add": "Ground", "order": 3}),
    ] {
        let _ = s.execute("level.trace", &p);
    }
}

#[test]
fn next_and_previous_level_cycle_through_the_tabs_and_have_shortcuts() {
    let mut s = Session::new();
    assert!(s.execute("level.next", &json!({})).is_err(), "one level has nothing to step to");
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.new", &json!({"name": "Roof"})).unwrap();
    let active = |s: &Session| s.doc().unwrap().active_level_ref().unwrap().name.clone();
    assert_eq!(active(&s), "Ground");
    s.execute("level.next", &json!({})).unwrap();
    assert_eq!(active(&s), "First");
    s.execute("level.next", &json!({})).unwrap();
    s.execute("level.next", &json!({})).unwrap();
    assert_eq!(active(&s), "Ground", "the last wraps to the first");
    s.execute("level.prev", &json!({})).unwrap();
    assert_eq!(active(&s), "Roof", "the first wraps to the last");

    let key = |id: &str| aad_engine::find_command(id).and_then(|c| c.shortcut);
    assert_eq!((key("level.next"), key("level.prev")), (Some("Cmd+PageDown"), Some("Cmd+PageUp")));
}

fn layer_names(s: &mut Session, show_for: &str) -> Vec<String> {
    let r = s.execute("layer", &json!({"showFor": show_for})).unwrap();
    r["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_layer_list_can_show_only_the_layers_of_this_level_or_this_sheet_set() {
    let mut s = Session::new();
    for l in ["G-WALL", "F-WALL", "E-TEXT", "E-EMPTY", "NOBODY"] {
        s.execute("layer.new", &json!({"name": l})).unwrap();
    }
    let draw_on = |s: &mut Session, layer: &str| {
        s.execute("layer.current", &json!({"name": layer})).unwrap();
        line(s, 0.0);
        s.execute("layer.current", &json!({"name": "0"})).unwrap();
    };
    draw_on(&mut s, "G-WALL");
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    draw_on(&mut s, "F-WALL");
    s.execute("sheetset.new", &json!({"name": "Electrical", "layerPrefix": "E-"})).unwrap();

    // Design on First: its own walls (not Ground's), nothing for a sheet set.
    let has = |v: &[String], n: &str| v.iter().any(|x| x == n);
    let level = layer_names(&mut s, "level");
    assert!(has(&level, "F-WALL") && !has(&level, "G-WALL") && !has(&level, "NOBODY"), "{level:?}");
    assert!(layer_names(&mut s, "sheetSet").is_empty(), "Design has no sheet set");
    assert_eq!(layer_names(&mut s, "all").len(), s.doc().unwrap().layers.len());

    // In the Electrical sheet view: the annotation's layers and anything with its prefix.
    s.execute("sheetset.activate", &json!({"set": "Electrical"})).unwrap();
    s.execute("text", &json!({"at": [1.0, 1.0], "text": "DB", "height": 0.5})).unwrap();
    let sheet = layer_names(&mut s, "sheetSet");
    assert!(has(&sheet, "E-TEXT") && has(&sheet, "E-EMPTY") && !has(&sheet, "F-WALL"), "{sheet:?}");
    let level = layer_names(&mut s, "level");
    assert!(has(&level, "E-TEXT") && has(&level, "F-WALL"), "this level includes the annotation drawn on it: {level:?}");

    let r = s.execute("layer", &json!({})).unwrap();
    assert_eq!((r["scope"]["level"].as_str(), r["scope"]["sheetSet"].as_str()), (Some("First"), Some("Electrical")));
    let row = r["layers"].as_array().unwrap().iter().find(|l| l["name"] == "E-EMPTY").unwrap();
    assert_eq!((row["used"].as_bool(), row["onSheetSet"].as_bool(), row["onLevel"].as_bool()), (Some(false), Some(true), Some(false)));

    for bad in [json!({"showFor": "nope"}), json!({"showFor": 3}), json!({"showFor": ["all"]})] {
        assert!(s.execute("layer", &bad).is_err(), "{bad}");
    }
}

#[test]
fn a_drawing_without_levels_counts_model_space_as_its_one_level() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "USED"})).unwrap();
    s.execute("layer.new", &json!({"name": "UNUSED"})).unwrap();
    s.execute("layer.current", &json!({"name": "USED"})).unwrap();
    line(&mut s, 0.0);
    let level = layer_names(&mut s, "level");
    assert!(level.contains(&"USED".to_string()) && !level.contains(&"UNUSED".to_string()));
}

fn mm_session() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s
}

#[test]
fn a_new_drawing_is_set_up_with_a_level_per_floor_the_floor_list_and_the_sheet_sets() {
    let mut s = mm_session();
    line(&mut s, 0.0); // already drawn: belongs on the ground floor
    let r = s.execute("level.setup", &json!({"floors": {"count": 3, "basements": 1, "terrace": true}})).unwrap();
    assert_eq!(r["active"], "Ground Floor");
    let ls = levels(&mut s);
    let row =
        |ls: &[Value]| ls.iter().map(|l| (l["name"].as_str().unwrap().to_string(), l["elevation"].as_f64().unwrap(), objects(l))).collect::<Vec<_>>();
    assert_eq!(
        row(&ls),
        [
            ("Basement 1".to_string(), -3000.0, 0),
            ("Ground Floor".to_string(), 0.0, 1),
            ("First Floor".to_string(), 3000.0, 0),
            ("Second Floor".to_string(), 6000.0, 0),
            ("Terrace".to_string(), 9000.0, 0)
        ],
        "basements below zero, the drawing's objects on the ground floor"
    );
    assert_eq!(ls[1]["active"], true);
    assert_eq!(s.doc().unwrap().sheet_sets.len(), 5, "the five presets");
    let floors = s.execute("floors", &json!({})).unwrap();
    let fl: Vec<&str> = floors["floors"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(fl, ["Basement 1", "Ground Floor", "First Floor", "Second Floor", "Terrace"], "the floor list matches the levels");

    // It survives a save and a reopen, and is one undo step.
    let back = aad_io::read(&aad_io::write(s.doc().unwrap(), "a.dxf").unwrap(), "a.dxf").unwrap();
    assert_eq!((back.levels.len(), back.sheet_sets.len()), (5, 5));
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().has_levels() && s.doc().unwrap().sheet_sets.is_empty());
    assert_eq!(s.doc().unwrap().model.len(), 1, "undo gives the plain drawing back");
}

#[test]
fn level_setup_takes_a_floor_list_and_refuses_bad_input_without_changing_anything() {
    let mut s = mm_session();
    let r = s.execute("level.setup", &json!({"floors": ["Ground", {"name": "Mezzanine", "height": 2.4}, {"name": "Roof", "height": 3.5, "elevation": 6.0}], "sheetSets": false})).unwrap();
    assert_eq!(r["sheetSets"], json!([]));
    let ls = levels(&mut s);
    assert_eq!(ls.iter().map(|l| l["elevation"].as_f64().unwrap()).collect::<Vec<_>>(), [0.0, 3000.0, 6000.0]);
    assert_eq!(ls[1]["height"], 2400.0);
    assert!(s.execute("level.setup", &json!({"floors": ["A"]})).is_err(), "already has levels");

    let mut s = mm_session();
    let before = s.doc().unwrap().clone();
    let long = "x".repeat(100);
    for p in [
        json!({}),
        json!({"floors": []}),
        json!({"floors": "Ground"}),
        json!({"floors": [1]}),
        json!({"floors": ["A", "a"]}),
        json!({"floors": [""]}),
        json!({"floors": [long]}),
        json!({"floors": [{"name": "A", "height": 1.0}]}),
        json!({"floors": [{"name": "A", "height": 99.0}]}),
        json!({"floors": [{"name": "A", "height": "tall"}]}),
        json!({"floors": [{"name": "A", "elevation": 1e9}]}),
        json!({"floors": {"count": 0}}),
        json!({"floors": {"count": 61}}),
        json!({"floors": {"count": 60, "basements": 10, "terrace": true}}),
        json!({"floors": (0..70).map(|i| format!("F{i}")).collect::<Vec<_>>()}),
    ] {
        assert!(s.execute("level.setup", &p).is_err(), "{p}");
        assert_eq!(*s.doc().unwrap(), before, "{p} changed nothing");
    }
}

#[test]
fn twenty_levels_switch_save_and_undo_without_copying_their_objects() {
    let mut s = mm_session();
    s.execute("level.setup", &json!({"floors": {"count": 20}, "sheetSets": false})).unwrap();
    for i in 0..20 {
        s.execute("level.activate", &json!({"level": i + 1})).unwrap();
        for k in 0..200 {
            let d = s.doc_mut().unwrap();
            let handle = d.new_handle();
            d.model.push(aad_engine::doc::Entity {
                handle,
                common: aad_engine::doc::Common::default(),
                kind: aad_engine::doc::EntityKind::Line(aad_engine::doc::Line {
                    a: aad_engine::geom::Vec3::new(0.0, f64::from(k), 0.0),
                    b: aad_engine::geom::Vec3::new(10.0, f64::from(k), 0.0),
                }),
            });
        }
    }
    assert_eq!(s.doc().unwrap().entity_count(), 4000);
    // Switching is a swap of stores; the undo history of all those switches holds shared stores.
    for i in 0..60 {
        s.execute("level.activate", &json!({"level": (i % 20) + 1})).unwrap();
    }
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().entity_count(), 4000, "undo of a switch loses nothing");
    let back = aad_io::read(&aad_io::write(s.doc().unwrap(), "a.dxf").unwrap(), "a.dxf").unwrap();
    assert_eq!((back.levels.len(), back.entity_count()), (20, 4000));
}

/// Working set of this process in MB (Windows; 0 elsewhere).
fn working_set_mb() -> f64 {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("(Get-Process -Id {}).WorkingSet64", std::process::id())])
        .output();
    out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).map_or(0.0, |b| b / 1_048_576.0)
}

/// 20 levels of 25 000 objects (500 000 in all): memory, switching, undo, save and open. Run with
/// `cargo test --release -p aad-engine --test levels -- --ignored --nocapture`.
#[test]
#[ignore = "load probe"]
fn probe_twenty_levels_of_25000_objects() {
    use std::time::Instant;
    let base = working_set_mb();
    let mut s = mm_session();
    s.execute("level.setup", &json!({"floors": {"count": 20}, "sheetSets": false})).unwrap();
    let t = Instant::now();
    for i in 0..20 {
        s.execute("level.activate", &json!({"level": i + 1})).unwrap();
        let d = s.doc_mut().unwrap();
        for k in 0..25_000 {
            let handle = d.new_handle();
            d.model.push(aad_engine::doc::Entity {
                handle,
                common: aad_engine::doc::Common::default(),
                kind: aad_engine::doc::EntityKind::Line(aad_engine::doc::Line {
                    a: aad_engine::geom::Vec3::new(f64::from(k % 500), f64::from(k / 500), 0.0),
                    b: aad_engine::geom::Vec3::new(f64::from(k % 500) + 1.0, f64::from(k / 500), 0.0),
                }),
            });
        }
    }
    let build = t.elapsed();
    let loaded = working_set_mb();
    let t = Instant::now();
    for i in 0..100 {
        s.execute("level.activate", &json!({"level": (i % 20) + 1})).unwrap();
    }
    let switching = t.elapsed();
    let after_switches = working_set_mb();
    let t = Instant::now();
    let bytes = aad_io::write(s.doc().unwrap(), "a.dxf").unwrap();
    let write = t.elapsed();
    let t = Instant::now();
    let back = aad_io::read(&bytes, "a.dxf").unwrap();
    let read = t.elapsed();
    eprintln!(
        "{} objects in {} levels: build {build:?}; memory {base:.0} -> {loaded:.0} MB loaded, {after_switches:.0} MB after 100 switches (100 undo snapshots); 100 switches {switching:?}; save {write:?} ({} MB); open {read:?}",
        s.doc().unwrap().entity_count(),
        s.doc().unwrap().levels.len(),
        bytes.len() / 1_048_576,
    );
    assert_eq!(back.entity_count(), 500_000);
}

#[test]
fn activating_a_level_moves_the_floor_selector_to_the_same_floor() {
    let mut s = Session::new();
    s.execute("level.setup", &json!({"floors": ["Ground", "First", "Roof"], "sheetSets": false})).unwrap();
    let floor = |s: &mut Session| aad_engine::cmd::context::resolve(s, "floor").unwrap()["current"].clone();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    assert_eq!(floor(&mut s), json!("First"));
    s.execute("level.next", &json!({})).unwrap();
    assert_eq!(floor(&mut s), json!("Roof"));
    // A level that is not a listed floor leaves the selector where it was.
    s.execute("level.new", &json!({"name": "Plant room"})).unwrap();
    s.execute("level.activate", &json!({"level": "Plant room"})).unwrap();
    assert_eq!(floor(&mut s), json!("Roof"));
}

#[test]
fn trace_settings_accept_what_a_dialog_sends() {
    let mut s = Session::new();
    s.execute("level.setup", &json!({"floors": ["Ground", "First"], "sheetSets": false})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    // A dialog sends the fade as a float and the layers as one text.
    let r = s.execute("level.trace", &json!({"add": "Ground", "fade": 40.0, "layers": " A-WALL, A-DOOR ,", "snap": true})).unwrap();
    assert_eq!(r["traces"][0]["fade"], 40);
    assert_eq!(r["traces"][0]["layers"], json!(["A-WALL", "A-DOOR"]));
    // `*` (or nothing) means every layer again; editing keeps what is not given.
    let r = s.execute("level.trace", &json!({"add": "Ground", "layers": "*"})).unwrap();
    assert_eq!((r["traces"][0]["fade"].as_u64(), r["traces"][0]["snap"].as_bool()), (Some(40), Some(true)));
    assert!(r["traces"][0]["layers"].is_null());
    for bad in [json!({"add": "Ground", "fade": 40.5}), json!({"add": "Ground", "fade": 91.0}), json!({"add": "Ground", "fade": -1.0})] {
        assert!(s.execute("level.trace", &bad).is_err(), "{bad}");
    }
    let too_many = (0..300).map(|i| format!("L{i}")).collect::<Vec<_>>().join(",");
    assert!(s.execute("level.trace", &json!({"add": "Ground", "layers": too_many})).is_err());
}

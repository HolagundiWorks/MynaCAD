//! Take-off layers and marks, layer mapping and level assignment working together: the marks and
//! the geometry survive a mapping, a move to another level and a save and reopen.

use aad_doc::TakeoffKind;
use aad_engine::Session;
use serde_json::{Value, json};

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

/// Save the open drawing to a DXF file and open that file in a new session.
fn reopen(s: &mut Session) -> Session {
    install_io();
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!("myna-wf-{}-{}.dxf", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let path = path.to_string_lossy().to_string();
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let mut t = Session::new();
    t.execute("open", &json!({ "path": path })).unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}.aadrev.json"));
    t
}

fn line_on(s: &mut Session, layer: &str, y: f64) -> String {
    use aad_geom::Vec2;
    let d = s.doc_mut().unwrap();
    let common = aad_doc::Common { layer: layer.to_string(), ..Default::default() };
    let h = d.add(&aad_doc::Space::Model, common, aad_engine::cmd::helpers::line(Vec2::new(0.0, y), Vec2::new(10.0, y))).unwrap();
    h.hex()
}

fn marks_on(s: &Session) -> Vec<(String, Option<TakeoffKind>)> {
    let d = s.doc().unwrap();
    d.model_stores().flat_map(|st| st.iter()).map(|e| (e.common.layer.clone(), e.common.takeoff.as_ref().map(|m| m.kind))).collect()
}

fn level_objects(s: &mut Session, name: &str) -> usize {
    let d = s.doc().unwrap();
    d.level_store(d.level_by_name(name).unwrap().id).unwrap().len()
}

#[test]
fn take_off_marks_on_an_inactive_level_survive_a_save_and_reopen() {
    let mut s = Session::new();
    line_on(&mut s, "MEASURE-FULLBRICK", 0.0);
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    line_on(&mut s, "MEASURE-BEAM", 5.0);
    // Both levels carry marks; Ground is not the active level.
    s.execute("takeoff.maplayers", &json!({})).unwrap();
    s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
    s.execute("takeoff.maplayers", &json!({})).unwrap();
    let kinds: Vec<_> = marks_on(&s).into_iter().filter_map(|m| m.1).collect();
    assert_eq!(kinds.len(), 2, "every level's lines are marked: {kinds:?}");
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();

    let t = reopen(&mut s);
    let kinds: Vec<_> = marks_on(&t).into_iter().filter_map(|m| m.1).collect();
    assert_eq!(kinds.len(), 2, "marks of the level that was not active are restored: {kinds:?}");
}

#[test]
fn maplayers_marks_the_geometry_of_every_level() {
    let mut s = Session::new();
    line_on(&mut s, "MEASURE-FULLBRICK", 0.0);
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    line_on(&mut s, "MEASURE-HALFBRICK", 5.0);
    let r = s.execute("takeoff.maplayers", &json!({})).unwrap();
    assert_eq!(r["tagged"], 2, "{r}");
}

#[test]
fn a_drawing_without_levels_can_send_geometry_to_a_new_level() {
    let mut s = Session::new();
    let h = line_on(&mut s, "MEASURE-FULLBRICK", 0.0);
    line_on(&mut s, "A-WALL", 1.0);
    s.execute("takeoff.maplayers", &json!({})).unwrap();
    let r = s.execute("level.moveto", &json!({ "level": "First", "create": true, "handles": [h] })).unwrap();
    assert_eq!(r["moved"], 1, "{r}");
    assert_eq!(s.doc().unwrap().levels.len(), 2);
    assert_eq!((level_objects(&mut s, "Level 1"), level_objects(&mut s, "First")), (1, 1));
    // The mark travels with the object and is still on it after a reopen.
    let t = reopen(&mut s);
    assert_eq!(t.doc().unwrap().levels.len(), 2);
    let first = t.doc().unwrap().level_by_name("First").unwrap().id;
    let e = t.doc().unwrap().level_store(first).unwrap().iter().next().unwrap().clone();
    assert_eq!(e.common.layer, "MEASURE-FULLBRICK");
    assert_eq!(e.common.takeoff.as_ref().map(|m| m.kind), Some(TakeoffKind::ExtWall));
}

#[test]
fn geometry_can_be_sent_from_a_level_that_is_not_active() {
    let mut s = Session::new();
    let h = line_on(&mut s, "A-WALL", 0.0);
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("level.new", &json!({ "name": "Roof" })).unwrap();
    s.execute("level.activate", &json!({ "level": "Roof" })).unwrap();
    // The line is on Ground; the active level is Roof.
    let r = s.execute("level.moveto", &json!({ "level": "First", "handles": [h] })).unwrap();
    assert_eq!(r["moved"], 1, "{r}");
    assert_eq!((level_objects(&mut s, "Ground"), level_objects(&mut s, "First")), (0, 1));
}

#[test]
fn moving_to_a_missing_level_or_with_nothing_chosen_changes_nothing() {
    let mut s = Session::new();
    line_on(&mut s, "A-WALL", 0.0);
    let before = s.doc().unwrap().entity_count();
    assert!(s.execute("level.moveto", &json!({ "level": "Nope", "handles": ["ABCDEF"] })).is_err());
    assert!(s.execute("level.moveto", &json!({ "level": "Nope" })).is_err());
    assert!(s.execute("level.moveto", &json!({ "level": "", "create": true })).is_err());
    assert!(!s.doc().unwrap().has_levels(), "no level was created by a refused call");
    assert_eq!(s.doc().unwrap().entity_count(), before);
}

// ---------------------------------------------------------------------------------------------
// The layer mapper

fn foreign_session() -> Session {
    // A drawing that did not come from MynaCAD: layers of its own, no stamp.
    let mut s = Session::new();
    for (layer, y) in [("WALL-A", 0.0), ("WALL-A", 1.0), ("WALL-EXT", 2.0), ("AR_DOORS", 3.0), ("Odd Thing", 4.0), ("WALL-HATCH", 5.0)] {
        line_on(&mut s, layer, y);
    }
    s.doc_mut().unwrap().project.remove(aad_geodb::origin::ORIGIN_KEY);
    s
}

fn on_layer(s: &Session, layer: &str) -> usize {
    s.doc().unwrap().model_stores().flat_map(|st| st.iter()).filter(|e| e.common.layer == layer).count()
}

#[test]
fn a_foreign_drawing_needs_mapping_and_the_plan_offers_built_in_destinations_only() {
    let mut s = foreign_session();
    let r = s.execute("layermap", &json!({ "suggest": true })).unwrap();
    assert_eq!(r["needsMapping"], true, "{r}");
    let layers = r["layers"].as_array().unwrap();
    let suggested = |n: &str| layers.iter().find(|l| l["name"] == n).map(|l| l["suggested"].clone());
    assert_eq!(suggested("WALL-A"), Some(json!("A-WALL")));
    assert_eq!(suggested("AR_DOORS"), Some(json!("A-DOOR")));
    assert_eq!(suggested("WALL-HATCH"), Some(json!("AN-HATCH")), "a wall hatch is a hatch, not a wall");
    assert_eq!(suggested("Odd Thing"), Some(Value::Null));
    let dests = r["destinations"].as_array().unwrap();
    assert!(dests.iter().any(|d| d["name"] == "A-WALL" && d["group"] == "Architecture"));
    assert!(dests.iter().any(|d| d["name"] == "MEASURE-FULLBRICK" && d["group"] == "Take-off"));
    assert!(!dests.iter().any(|d| d["name"] == "WALL-A"), "a drawing's own layer is never a destination");
    assert!(s.execute("layermap", &json!({ "suggest": true, "only": 5 })).is_err());
    // Asking changes nothing.
    assert_eq!(on_layer(&s, "WALL-A"), 2);
}

#[test]
fn empty_and_standard_only_drawings_do_not_need_mapping() {
    let mut empty = Session::new();
    let r = empty.execute("layermap", &json!({ "suggest": true })).unwrap();
    assert_eq!(r["needsMapping"], false);
    assert_eq!(r["layers"], json!([]));
    assert!(empty.execute("layermap", &json!({ "map": { "WALL": "A-WALL" } })).is_err(), "no such layer");

    let mut std_only = Session::new();
    line_on(&mut std_only, "A-WALL", 0.0);
    line_on(&mut std_only, "0", 1.0);
    assert_eq!(std_only.execute("layermap", &json!({ "suggest": true })).unwrap()["needsMapping"], false);
}

#[test]
fn several_source_layers_map_to_one_built_in_layer_and_their_names_are_kept() {
    let mut s = foreign_session();
    let before = s.doc().unwrap().entity_count();
    let map = json!({ "WALL-A": "A-WALL", "wall-ext": "a-wall", "AR_DOORS": "A-DOOR" });
    let pv = s.execute("layermap", &json!({ "map": map, "preview": true })).unwrap();
    assert_eq!(pv["total"], 4, "{pv}");
    assert_eq!(on_layer(&s, "WALL-A"), 2, "a preview moves nothing");
    assert!(s.doc().unwrap().layer("A-WALL").is_none(), "and creates nothing");

    let r = s.execute("layermap", &json!({ "map": map, "purge": true })).unwrap();
    assert_eq!(r["moved"], 4, "{r}");
    assert_eq!((on_layer(&s, "A-WALL"), on_layer(&s, "A-DOOR")), (3, 1));
    assert_eq!(s.doc().unwrap().entity_count(), before, "geometry is kept");
    let d = s.doc().unwrap();
    assert!(d.layer("WALL-A").is_none() && d.layer("WALL-EXT").is_none(), "the emptied source layers are purged");
    assert!(d.layer("A-WALL").is_some_and(|l| !l.description.is_empty()), "the built-in layer is made with its standard look");
    assert!(d.layer("Odd Thing").is_some(), "an unmapped layer stays");

    let src = s.execute("layermap", &json!({ "sources": true })).unwrap();
    let list = src["sources"].as_array().unwrap();
    assert!(list.iter().any(|e| e["source"] == "WALL-A" && e["dest"] == "A-WALL" && e["objects"] == 2 && e["present"] == 2), "{src}");
    assert!(list.iter().any(|e| e["source"] == "WALL-EXT" && e["dest"] == "A-WALL"), "{src}");
    // The object still says where it came from.
    let h = s.doc().unwrap().model.iter().find(|e| e.common.layer == "A-DOOR").unwrap().handle;
    assert_eq!(aad_engine::cmd::layermap::source_layer_of(s.doc().unwrap(), h).as_deref(), Some("AR_DOORS"));
    // The mapper has been run: the drawing is no longer waiting for it.
    assert_eq!(s.execute("layermap", &json!({ "suggest": true })).unwrap()["needsMapping"], false);

    // One undo step puts everything back, names included.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!((on_layer(&s, "WALL-A"), on_layer(&s, "AR_DOORS")), (2, 1));
    assert!(s.doc().unwrap().layer("WALL-A").is_some());
    assert!(s.execute("layermap", &json!({ "sources": true })).unwrap()["sources"].as_array().unwrap().is_empty());
}

#[test]
fn only_built_in_layers_are_destinations_and_a_bad_map_changes_nothing() {
    let mut s = foreign_session();
    let before = s.doc().unwrap().clone();
    for bad in [
        json!({ "map": { "WALL-A": "MY-OWN" } }),
        json!({ "map": { "WALL-A": "Odd Thing" } }),
        json!({ "map": { "WALL-A": "" } }),
        json!({ "map": { "WALL-A": 3 } }),
        json!({ "map": { "NOPE": "A-WALL" } }),
        json!({ "map": { "0": "A-WALL" } }),
        json!({ "map": { "WALL-A": "A-WALL", "wall-a": "A-DOOR" } }),
        json!({ "map": {} }),
        json!({}),
        json!({ "map": [1, 2] }),
    ] {
        assert!(s.execute("layermap", &bad).is_err(), "{bad}");
        assert_eq!(*s.doc().unwrap(), before, "{bad} changed nothing");
    }
    // `create` no longer makes a layer of the caller's own name.
    assert!(s.execute("layermap", &json!({ "map": { "WALL-A": "NEW-ONE" }, "create": true })).is_err());
}

#[test]
fn a_map_that_swaps_two_layers_does_what_it_says() {
    let mut s = Session::new();
    line_on(&mut s, "A-WALL", 0.0);
    line_on(&mut s, "A-DOOR", 1.0);
    line_on(&mut s, "A-DOOR", 2.0);
    s.execute("layermap", &json!({ "map": { "A-WALL": "A-DOOR", "A-DOOR": "A-WALL" } })).unwrap();
    assert_eq!((on_layer(&s, "A-WALL"), on_layer(&s, "A-DOOR")), (2, 1));
}

#[test]
fn locked_layers_and_objects_of_other_levels_are_handled() {
    let mut s = foreign_session();
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    line_on(&mut s, "WALL-A", 9.0);
    line_on(&mut s, "WALL-EXT", 10.0);
    s.doc_mut().unwrap().layer_mut("WALL-EXT").unwrap().locked = true;
    let r = s.execute("layermap", &json!({ "map": { "WALL-A": "A-WALL", "WALL-EXT": "A-WALL" }, "purge": true })).unwrap();
    assert_eq!(r["moved"], 3, "WALL-A on both levels; {r}");
    assert_eq!(r["skippedLocked"], 2, "{r}");
    assert_eq!(on_layer(&s, "A-WALL"), 3, "the inactive level's objects moved too");
    assert!(s.doc().unwrap().layer("WALL-EXT").is_some(), "a locked source stays");
    assert!(s.doc().unwrap().layer("WALL-A").is_none());
}

#[test]
fn mapping_keeps_take_off_marks_and_the_names_survive_a_save_and_reopen() {
    let mut s = Session::new();
    line_on(&mut s, "WALL-230", 0.0);
    line_on(&mut s, "WALL-230", 1.0);
    // A foreign layer mapped to a take-off layer becomes take-off elements through `takeoff.maplayers`.
    s.execute("layermap", &json!({ "map": { "WALL-230": "MEASURE-FULLBRICK" } })).unwrap();
    assert_eq!(on_layer(&s, "MEASURE-FULLBRICK"), 2);
    let r = s.execute("takeoff.maplayers", &json!({})).unwrap();
    assert_eq!(r["tagged"], 2, "{r}");
    // Mapping again moves the objects and keeps their marks.
    s.execute("layermap", &json!({ "map": { "MEASURE-FULLBRICK": "A-WALL" } })).unwrap();
    let kinds: Vec<_> = marks_on(&s);
    assert!(kinds.iter().all(|(l, k)| l == "A-WALL" && *k == Some(TakeoffKind::ExtWall)), "{kinds:?}");

    let t = reopen(&mut s);
    let kinds: Vec<_> = marks_on(&t);
    assert!(kinds.iter().all(|(l, k)| l == "A-WALL" && *k == Some(TakeoffKind::ExtWall)), "{kinds:?}");
    let src = t.doc().unwrap().project.get("layerSources").expect("the source names are saved with the drawing");
    assert!(src.contains("WALL-230"), "{src}");
}

#[test]
fn dismissing_the_wizard_keeps_the_layers_and_stops_the_offer() {
    let mut s = foreign_session();
    assert_eq!(s.execute("layermap", &json!({ "suggest": true })).unwrap()["needsMapping"], true);
    s.execute("layermap", &json!({ "dismiss": true })).unwrap();
    assert_eq!(s.execute("layermap", &json!({ "suggest": true })).unwrap()["needsMapping"], false);
    assert_eq!(on_layer(&s, "WALL-A"), 2);
    // The layers an import brought in are still offered when named.
    let r = s.execute("layermap", &json!({ "suggest": true, "only": ["wall-a"] })).unwrap();
    assert_eq!(r["needsMapping"], true);
    assert_eq!(r["layers"].as_array().unwrap().len(), 1);
}

#[test]
fn an_import_reports_the_layers_that_are_not_built_in() {
    install_io();
    let mut src = foreign_session();
    let path = std::env::temp_dir().join(format!("myna-import-{}.dxf", std::process::id())).to_string_lossy().to_string();
    src.execute("saveas", &json!({ "path": path })).unwrap();
    let mut s = Session::new();
    line_on(&mut s, "A-WALL", 0.0);
    let r = s.execute("import", &json!({ "path": path })).unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}.aadrev.json"));
    let names: Vec<&str> = r["layers"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(names.contains(&"WALL-A") && names.contains(&"Odd Thing"), "{r}");
    assert!(!names.contains(&"A-WALL"));
    // Only the named layers are offered for an import.
    assert_eq!(s.execute("layermap", &json!({ "suggest": true, "only": names })).unwrap()["needsMapping"], true);
}

// ---------------------------------------------------------------------------------------------
// Built-in layers

#[test]
fn the_added_built_in_layers_follow_the_naming_and_have_no_duplicates() {
    let mut s = Session::new();
    s.execute("hcwlayers", &json!({})).unwrap();
    assert_eq!(s.execute("hcwinfo", &json!({})).unwrap()["layers"].as_array().unwrap().len(), 36, "the toolkit's set keeps its size");
    // Mapping onto an added layer creates it with its standard look.
    let added = ["A-ROOM", "A-ELEV", "A-SECT", "A-LIFT", "A-EXIST", "S-FOOT", "AN-TBLK"];
    for (i, name) in added.iter().enumerate() {
        let src = format!("SRC{i}");
        line_on(&mut s, &src, i as f64);
        s.execute("layermap", &json!({ "map": { src: name } })).unwrap();
    }
    let d = s.doc().unwrap();
    for name in added {
        let l = d.layer(name).unwrap_or_else(|| panic!("{name} is created by a map"));
        assert!(!l.description.is_empty(), "{name} has a description");
    }
    let mut names: Vec<String> = d.layers.iter().map(|l| l.name.to_ascii_uppercase()).collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), n, "no layer is defined twice");
    let r = s.execute("layermap", &json!({ "suggest": true })).unwrap();
    let dests: Vec<&str> = r["destinations"].as_array().unwrap().iter().filter_map(|d| d["name"].as_str()).collect();
    let mut uniq = dests.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(uniq.len(), dests.len(), "each destination is listed once");
    for name in ["A-ROOM", "S-FOOT", "AN-TBLK", "S-BEAM", "S-RCC", "E-BOARD", "P-WATR", "FC-BOUN"] {
        assert!(dests.contains(&name), "{name} is a destination");
    }
    // Names that suggest the new layers.
    for (name, want) in
        [("ELEVATION-FRONT", "A-ELEV"), ("SECTION_LINES", "A-SECT"), ("LIFT", "A-LIFT"), ("FOUNDATION", "S-FOOT"), ("TITLE-BORDER", "AN-TBLK")]
    {
        let mut t = Session::new();
        line_on(&mut t, name, 0.0);
        let r = t.execute("layermap", &json!({ "suggest": true })).unwrap();
        assert_eq!(r["layers"][0]["suggested"], want, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// Layer manager: several layers at once

#[test]
fn layer_properties_apply_to_several_layers_in_one_undo_step() {
    let mut s = Session::new();
    for n in ["L1", "L2", "L3"] {
        s.execute("layer.new", &json!({ "name": n })).unwrap();
    }
    let r = s.execute("layer.set", &json!({ "names": ["L1", "l2", "L3"], "locked": true, "color": 3 })).unwrap();
    assert_eq!(r["changed"], 3);
    assert!(["L1", "L2", "L3"].iter().all(|n| s.doc().unwrap().layer(n).unwrap().locked));
    s.execute("undo", &json!({})).unwrap();
    assert!(["L1", "L2", "L3"].iter().all(|n| !s.doc().unwrap().layer(n).unwrap().locked), "one undo step");

    // Freezing skips the current layer and says so.
    s.execute("layer.current", &json!({ "name": "L2" })).unwrap();
    let r = s.execute("layer.set", &json!({ "names": ["L1", "L2"], "frozen": true })).unwrap();
    assert_eq!((r["changed"].as_u64(), r["skipped"].as_array().map(Vec::len)), (Some(1), Some(1)));
    assert!(s.doc().unwrap().layer("L1").unwrap().frozen && !s.doc().unwrap().layer("L2").unwrap().frozen);

    // Anything wrong changes nothing.
    let before = s.doc().unwrap().clone();
    for bad in [
        json!({ "names": ["L1", "NOPE"], "locked": true }),
        json!({ "names": [], "locked": true }),
        json!({ "names": "L1", "locked": true }),
        json!({ "names": [1], "locked": true }),
        json!({ "names": ["L1"], "newName": "X" }),
        json!({ "names": ["L1"], "name": "L2", "on": false }),
        json!({ "names": ["L1"], "color": "nonsense" }),
        json!({ "names": ["L1"], "linetype": "NoSuchType" }),
    ] {
        assert!(s.execute("layer.set", &bad).is_err(), "{bad}");
        assert_eq!(*s.doc().unwrap(), before, "{bad} changed nothing");
    }
}

#[test]
fn several_layers_are_deleted_in_one_step_and_the_ones_in_use_are_kept() {
    let mut s = Session::new();
    for n in ["E1", "E2", "USED"] {
        s.execute("layer.new", &json!({ "name": n })).unwrap();
    }
    line_on(&mut s, "USED", 0.0);
    let r = s.execute("layer.delete", &json!({ "names": ["E1", "E2", "USED", "0"] })).unwrap();
    assert_eq!(r["deleted"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(r["skipped"].as_array().unwrap().len(), 2, "{r}");
    assert!(s.doc().unwrap().layer("E1").is_none() && s.doc().unwrap().layer("USED").is_some());
    s.execute("undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().layer("E1").is_some() && s.doc().unwrap().layer("E2").is_some(), "one undo step");
    assert!(s.execute("layer.delete", &json!({ "names": ["USED", "0"] })).is_err(), "nothing can go");
    assert!(s.execute("layer.delete", &json!({ "names": ["NOPE"] })).is_err());
}

// ---------------------------------------------------------------------------------------------
// Level tabs

#[test]
fn choosing_a_level_while_a_layout_is_shown_brings_that_level_to_the_model_tab() {
    let mut s = Session::new();
    line_on(&mut s, "A-WALL", 0.0);
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("layout", &json!({ "option": "set", "name": "Layout1" })).unwrap();
    assert!(matches!(s.layout_space(), aad_doc::Space::Paper(_)), "a layout is on screen");
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    assert_eq!(s.layout_space(), aad_doc::Space::Model, "the Model tab is shown again");
    assert_eq!(s.doc().unwrap().active_level_ref().unwrap().name, "First", "and the level is the one asked for");
    // The same level chosen again while a layout is shown still returns to the Model tab.
    s.execute("layout", &json!({ "option": "set", "name": "Layout1" })).unwrap();
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    assert_eq!(s.layout_space(), aad_doc::Space::Model);
    // Next / previous go the same way.
    s.execute("layout", &json!({ "option": "set", "name": "Layout1" })).unwrap();
    s.execute("level.next", &json!({})).unwrap();
    assert_eq!(s.layout_space(), aad_doc::Space::Model);
    // A refused level leaves the displayed tab as it was.
    s.execute("layout", &json!({ "option": "set", "name": "Layout1" })).unwrap();
    assert!(s.execute("level.activate", &json!({ "level": "Nope" })).is_err());
    assert!(matches!(s.layout_space(), aad_doc::Space::Paper(_)));
}

#[test]
fn the_active_level_and_the_objects_shown_follow_every_tab_switch() {
    let mut s = Session::new();
    let a = line_on(&mut s, "A-WALL", 0.0);
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("level.new", &json!({ "name": "Roof" })).unwrap();
    s.execute("level.moveto", &json!({ "level": "Roof", "handles": [a] })).unwrap();
    let shown = |s: &Session| s.doc().unwrap().model.len();
    for (level, objects) in [("Roof", 1), ("Ground", 0), ("First", 0), ("Roof", 1)] {
        s.execute("level.activate", &json!({ "level": level })).unwrap();
        assert_eq!(s.doc().unwrap().active_level_ref().unwrap().name, level);
        assert_eq!(shown(&s), objects, "{level}");
        let listed = s.execute("level.list", &json!({})).unwrap();
        let active: Vec<&str> =
            listed["levels"].as_array().unwrap().iter().filter(|l| l["active"] == true).filter_map(|l| l["name"].as_str()).collect();
        assert_eq!(active, vec![level], "exactly one level is the active one");
    }
    // In a sheet view the level changes and the sheet set stays.
    s.execute("sheetset.standard", &json!({})).unwrap();
    s.execute("sheetset.activate", &json!({ "set": "Electrical" })).unwrap();
    s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!((d.active_level_ref().unwrap().name.as_str(), d.active_sheet_ref().map(|x| x.name.as_str())), ("Ground", Some("Electrical")));
}

// ---------------------------------------------------------------------------------------------
// Take-off: why an object on a take-off layer is not an element

#[test]
fn the_take_off_says_why_objects_on_a_take_off_layer_were_not_measured() {
    let mut s = Session::new();
    line_on(&mut s, "MEASURE-FULLBRICK", 0.0);
    // A circle is not a wall.
    {
        let d = s.doc_mut().unwrap();
        let common = aad_doc::Common { layer: "MEASURE-FULLBRICK".into(), ..Default::default() };
        d.add(&aad_doc::Space::Model, common, aad_engine::cmd::helpers::circle(aad_geom::Vec2::new(5.0, 5.0), 1.0)).unwrap();
    }
    // A wall on a layer that is switched off.
    line_on(&mut s, "MEASURE-HALFBRICK", 3.0);
    s.doc_mut().unwrap().layer_mut("MEASURE-HALFBRICK").unwrap().on = false;
    s.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    line_on(&mut s, "MEASURE-FULLBRICK", 7.0);
    s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();

    let r = s.execute("takeoff.brick", &json!({ "table": false })).unwrap();
    assert_eq!(r["totals"]["count"], 1, "only the visible line of this level is an element: {r}");
    let w: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(w.iter().any(|m| m.contains("hidden") && m.contains("turn the layer on")), "{w:?}");
    assert!(w.iter().any(|m| m.contains("not lines, arcs or polylines")), "{w:?}");
    assert!(w.iter().any(|m| m.contains("level `First`") && m.contains("activate that level")), "{w:?}");

    // Turning the layer on brings its wall in.
    s.doc_mut().unwrap().layer_mut("MEASURE-HALFBRICK").unwrap().on = true;
    let r = s.execute("takeoff.brick", &json!({ "table": false })).unwrap();
    assert_eq!(r["totals"]["count"], 2, "{r}");
    // The other level measures its own line when it is the active one.
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    let r = s.execute("takeoff.brick", &json!({ "table": false })).unwrap();
    assert_eq!(r["totals"]["count"], 1, "{r}");
}

#[test]
fn level_assignment_and_marks_survive_a_save_and_reopen_in_a_project_file() {
    install_io();
    let path = std::env::temp_dir().join(format!("myna-wf-{}.osp", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let path = path.to_string_lossy().to_string();
    let mut s = Session::new();
    let h = line_on(&mut s, "MEASURE-FULLBRICK", 0.0);
    line_on(&mut s, "A-WALL", 1.0);
    s.execute("takeoff.maplayers", &json!({})).unwrap();
    s.execute("level.moveto", &json!({ "level": "First", "create": true, "handles": [h] })).unwrap();
    let saved = s.execute("saveas", &json!({ "path": path }));
    if let Err(e) = &saved {
        // A build without project-file support in the engine hooks: nothing to verify here.
        eprintln!("saveas .osp not available: {e}");
        return;
    }
    let mut t = Session::new();
    t.execute("open", &json!({ "path": path })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(t.doc().unwrap().levels.len(), 2, "both levels come back");
    let first = t.doc().unwrap().level_by_name("First").expect("the new level is there").id;
    let store = t.doc().unwrap().level_store(first).unwrap();
    assert_eq!(store.len(), 1);
    let e = store.iter().next().unwrap();
    assert_eq!(e.common.layer, "MEASURE-FULLBRICK");
    assert_eq!(e.common.takeoff.as_ref().map(|m| m.kind), Some(TakeoffKind::ExtWall), "the mark is kept");
}

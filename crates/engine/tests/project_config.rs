//! `project.config.apply` and `project.config.propose`: the project configuration MynaOps writes, applied to the drawing (docs/ops-first-workflow.md 4.4, 4.5).

use aad_engine::Session;
use serde_json::{Value, json};

fn levels_json(items: Value, next: u32) -> Value {
    json!({ "v": 1, "next_cid": next, "items": items })
}

fn l(cid: u32, name: &str, el: f64, h: f64) -> Value {
    json!({ "cid": cid, "name": name, "elevation_mm": el, "height_mm": h })
}

/// A configuration with the schema and revision, plus the given parts.
fn config(parts: Value) -> Value {
    let mut c = json!({ "schema": { "v": 1, "min_reader": 1 }, "rev": { "v": 1, "rev": 1, "app": "mynaops", "time": "2026-11-15T09:30:12Z" } });
    for (k, v) in parts.as_object().unwrap() {
        c[k] = v.clone();
    }
    c
}

fn apply(s: &mut Session, c: &Value) -> Value {
    s.execute("project.config.apply", &json!({ "config": c })).unwrap()
}

fn level_names(s: &mut Session) -> Vec<String> {
    s.execute("level.list", &json!({})).unwrap()["levels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
}

fn level(s: &mut Session, name: &str) -> Value {
    s.execute("level.list", &json!({})).unwrap()["levels"].as_array().unwrap().iter().find(|l| l["name"] == name).cloned().unwrap()
}

fn undo_len(s: &Session) -> usize {
    s.state().unwrap().undo.len()
}

fn item<'a>(r: &'a Value, key: &str) -> &'a Value {
    r["items"].as_array().unwrap().iter().find(|i| i["key"] == key).unwrap_or_else(|| panic!("no item {key} in {r}"))
}

/// A drawing with three configured levels, a tab, the standard layers and project details (what an adopted project looks like).
fn adopted() -> (Session, Value) {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({ "units": "mm", "mode": "metadata" })).unwrap();
    s.execute("level.setup", &json!({ "floors": [{ "name": "Ground", "height": 3.0 }, { "name": "First", "height": 3.0 }, { "name": "Second", "height": 3.0 }], "sheetSets": false }))
        .unwrap();
    s.execute("hcwlayers", &json!({ "set": "standard" })).unwrap();
    s.execute("sheetset.new", &json!({ "name": "Working drawing", "layerPrefix": "W-", "layers": ["W-*"], "blocks": [] })).unwrap();
    s.execute("project", &json!({ "details": { "title": "Hospet House", "owner": "R. Rao" } })).unwrap();
    let c = config(json!({
        "units": { "v": 1, "drawing_unit": "mm" },
        "project": { "v": 1, "title": "Hospet House", "owner": "R. Rao" },
        "layer_sets": { "v": 1, "sets": ["standard"] },
        "levels": levels_json(json!([]), 1),
        "tabs": { "v": 1, "items": [{ "tid": 1, "name": "Working drawing", "layer_prefix": "W-", "layers": ["W-*"], "blocks": [] }] },
    }));
    // The levels come from what the drawing says (a proposal's levels become the configuration, cid = LevelId).
    let prop = s.execute("project.config.propose", &json!({})).unwrap()["proposal"].clone();
    let items: Vec<Value> = prop["levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            l(p["cid"].as_u64().unwrap() as u32, p["name"].as_str().unwrap(), p["elevation_mm"].as_f64().unwrap(), p["height_mm"].as_f64().unwrap())
        })
        .collect();
    let next = items.iter().map(|i| i["cid"].as_u64().unwrap()).max().unwrap() as u32 + 1;
    let mut c = c;
    c["levels"] = levels_json(Value::Array(items), next);
    (s, c)
}

#[test]
fn the_proposal_lists_what_a_legacy_drawing_has() {
    let (mut s, _) = adopted();
    let p = s.execute("project.config.propose", &json!({})).unwrap();
    assert_eq!(p["recorded"], false);
    let p = &p["proposal"];
    assert_eq!(p["drawing_unit"], "mm");
    let lv = p["levels"].as_array().unwrap();
    assert_eq!(lv.len(), 3);
    assert_eq!((lv[1]["name"].as_str(), lv[1]["elevation_mm"].as_f64(), lv[1]["height_mm"].as_f64()), (Some("First"), Some(3000.0), Some(3000.0)));
    assert_eq!(p["layer_sets"], json!(["standard"]));
    assert_eq!(p["tabs"][0]["name"], "Working drawing");
    assert_eq!(p["project"]["title"], "Hospet House");
}

#[test]
fn applying_what_the_drawing_already_has_changes_nothing() {
    let (mut s, c) = adopted();
    let before = undo_len(&s);
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "ok", "{r}");
    assert_eq!(r["changed"], false, "{r}");
    assert_eq!(undo_len(&s), before, "no undo entry for a no-op");
    assert_eq!(r["rev"], 1);
    // The binding is not written either on a second run.
    let again = apply(&mut s, &c);
    assert_eq!(again["changed"], false);
}

#[test]
fn a_new_project_gets_its_levels_in_order_with_floors_details_tabs_and_scheme() {
    let mut s = Session::new();
    let c = config(json!({
        "units": { "v": 1, "drawing_unit": "mm" },
        "project": { "v": 1, "title": "Hospet House", "owner": "R. Rao", "location": "Hospet" },
        "team": { "v": 1, "architect": "A. Kulkarni" },
        "layer_sets": { "v": 1, "sets": ["standard", "permit"] },
        "levels": { "v": 1, "next_cid": 4, "defaults": { "slab_mm": 175, "beam_mm": 500 },
                    "items": [l(1, "Ground", 0.0, 3000.0), l(2, "First", 3000.0, 3200.0), l(3, "Terrace", 6200.0, 2400.0)] },
        "tabs": { "v": 1, "items": [{ "tid": 1, "name": "Electrical", "layer_prefix": "E-", "layers": ["E-*"], "blocks": ["E_*"] }] },
        "sheets": { "v": 1, "scheme": { "pattern": "{D}-{N}", "width": 4, "step": 5, "disciplines": [{ "code": "a", "name": "Architectural", "start": 101 }] } },
        "titleblock": { "v": 1, "fields": { "CLIENT": "R. Rao" } },
    }));
    let before = undo_len(&s);
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "ok", "{r}");
    assert_eq!(r["changed"], true);
    assert_eq!(undo_len(&s), before + 1, "one undo entry");
    assert_eq!(s.state().unwrap().undo.last().unwrap().label, "Project configuration");
    assert_eq!(level_names(&mut s), ["Ground", "First", "Terrace"], "no stray implicit level");
    let first = level(&mut s, "First");
    assert_eq!((first["elevation"].as_f64(), first["height"].as_f64()), (Some(3000.0), Some(3200.0)));
    let d = s.doc().unwrap();
    assert!(d.project.get("config.levelmap").is_some_and(|t| t.contains("\"1\"") && t.contains("\"3\"")), "the binding is stored in the drawing");
    let floors = s.execute("floors", &json!({})).unwrap()["floors"].clone();
    assert_eq!(floors.as_array().unwrap().len(), 3);
    assert_eq!(floors[1]["name"], "First");
    assert_eq!((floors[1]["floorToFloor"].as_f64(), floors[1]["slab"].as_f64(), floors[1]["beam"].as_f64()), (Some(3.2), Some(175.0), Some(500.0)));
    let details = s.execute("project", &json!({})).unwrap()["details"].clone();
    assert_eq!(
        (details["title"].as_str(), details["location"].as_str(), details["architect"].as_str()),
        (Some("Hospet House"), Some("Hospet"), Some("A. Kulkarni"))
    );
    let sets = s.execute("sheetset.list", &json!({})).unwrap()["sheetSets"].clone();
    assert_eq!(sets[0]["name"], "Electrical");
    assert_eq!(sets[0]["layers"], json!(["E-*"]));
    let scheme = s.execute("sheet.scheme", &json!({})).unwrap()["scheme"].clone();
    assert_eq!((scheme["width"].as_u64(), scheme["step"].as_u64(), scheme["disciplines"][0]["code"].as_str()), (Some(4), Some(5), Some("A")));
    assert!(s.doc().unwrap().layer("BP-SITE-BOUNDARY").is_some(), "the permit set was created");
    assert!(s.doc().unwrap().project.contains_key("config.titleblock.defaults"));
    // Applying the same configuration again is a no-op.
    let n = undo_len(&s);
    assert_eq!(apply(&mut s, &c)["changed"], false);
    assert_eq!(undo_len(&s), n);
}

#[test]
fn one_undo_puts_everything_back() {
    let mut s = Session::new();
    let c = config(json!({
        "units": { "v": 1, "drawing_unit": "mm" },
        "project": { "v": 1, "title": "House" },
        "layer_sets": { "v": 1, "sets": ["standard"] },
        "levels": levels_json(json!([l(1, "Ground", 0.0, 3000.0), l(2, "First", 3000.0, 3000.0)]), 3),
        "tabs": { "v": 1, "items": [{ "tid": 1, "name": "Electrical", "layer_prefix": "E-" }] },
    }));
    apply(&mut s, &c);
    assert!(s.doc().unwrap().has_levels() && !s.doc().unwrap().sheet_sets.is_empty());
    assert_eq!(s.undo().unwrap().as_deref(), Some("Project configuration"));
    let d = s.doc().unwrap();
    assert!(!d.has_levels() && d.sheet_sets.is_empty() && d.layer("A-WALL").is_none() && !d.project.contains_key("config.levelmap"));
}

#[test]
fn an_implicit_level_with_objects_becomes_the_configured_level_of_that_name() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
    let c = config(json!({ "levels": levels_json(json!([l(1, "Ground", 0.0, 3000.0), l(2, "First", 3000.0, 3000.0)]), 3) }));
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "ok", "{r}");
    assert_eq!(level_names(&mut s), ["Ground", "First"]);
    assert_eq!(level(&mut s, "Ground")["objects"], 1, "the drawn line stayed on the first configured level");
}

#[test]
fn a_new_level_a_rename_and_an_elevation_change_each_apply_without_moving_objects() {
    let (mut s, mut c) = adopted();
    s.execute("level.activate", &json!({ "level": "First" })).unwrap();
    s.execute("line", &json!({ "points": [[0.0, 0.0], [5.0, 0.0]] })).unwrap();
    let first_id = c["levels"]["items"][1]["cid"].clone();

    // New level inserted in the middle, a rename of "First", a new elevation for Second.
    let ground = c["levels"]["items"][0].clone();
    let mut first = c["levels"]["items"][1].clone();
    first["name"] = json!("First Floor");
    let mut second = c["levels"]["items"][2].clone();
    second["elevation_mm"] = json!(6500.0);
    second["height_mm"] = json!(3300.0);
    c["levels"] = levels_json(json!([ground, l(10, "Mezzanine", 4500.0, 1500.0), first, second]), 11);
    // (Mezzanine is between Ground and First Floor in the configuration.)
    let before = undo_len(&s);
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "ok", "{r}");
    assert_eq!(undo_len(&s), before + 1);
    assert_eq!(level_names(&mut s), ["Ground", "Mezzanine", "First Floor", "Second"]);
    assert_eq!(level(&mut s, "First Floor")["objects"], 1, "renaming moved nothing");
    assert_eq!(level(&mut s, "First Floor")["id"], first_id, "the same level, renamed, not re-created");
    let sec = level(&mut s, "Second");
    assert_eq!((sec["elevation"].as_f64(), sec["height"].as_f64()), (Some(6500.0), Some(3300.0)));
    assert_eq!(level(&mut s, "Mezzanine")["objects"], 0);
}

#[test]
fn a_removed_level_with_objects_stays_and_is_reported_and_an_empty_one_is_deleted() {
    let (mut s, mut c) = adopted();
    // An earlier apply that made a level wrote the binding, so the levels are known as configured (an adopted drawing that needed no change has no
    // binding, and a level removed later is only reported).
    let mut with_extra = c.clone();
    let mut extra = c["levels"]["items"].as_array().unwrap().clone();
    extra.push(l(10, "Roof", 9000.0, 1000.0));
    with_extra["levels"] = levels_json(Value::Array(extra), 11);
    apply(&mut s, &with_extra);
    s.execute("level.activate", &json!({ "level": "Second" })).unwrap();
    s.execute("line", &json!({ "points": [[0.0, 0.0], [5.0, 0.0]] })).unwrap();
    s.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
    let items = c["levels"]["items"].as_array().unwrap().clone();
    // Both First (empty) and Second (one object) leave the configuration.
    c["levels"] = levels_json(json!([items[0].clone()]), 5);
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "partial", "{r}");
    assert_eq!(level_names(&mut s), ["Ground", "Second"], "the empty one was deleted, the one with an object stays");
    let it = item(&r, "levels");
    assert_eq!(it["status"], "orphans");
    assert!(it["detail"].as_str().unwrap().contains("Second (1 objects)"), "{it}");
    assert!(it["detail"].as_str().unwrap().contains("First deleted"), "{it}");
    // Applying again: still an orphan, nothing else changes.
    let again = apply(&mut s, &c);
    assert_eq!(again["changed"], false);
    assert_eq!(item(&again, "levels")["status"], "orphans");
    assert_eq!(level(&mut s, "Second")["objects"], 1);
}

#[test]
fn a_layer_set_is_added_and_one_that_is_dropped_keeps_its_layers() {
    let (mut s, mut c) = adopted();
    assert!(s.doc().unwrap().layer("BP-SITE-BOUNDARY").is_none());
    c["layer_sets"] = json!({ "v": 1, "sets": ["permit"] });
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "ok", "{r}");
    assert!(s.doc().unwrap().layer("BP-SITE-BOUNDARY").is_some());
    assert!(s.doc().unwrap().layer("A-WALL").is_some(), "the layers of the set that is no longer configured stay");
    assert!(item(&r, "layer_sets")["detail"].as_str().unwrap().contains("standard is no longer configured"));
}

#[test]
fn a_unit_change_after_content_exists_is_refused_and_the_rest_still_applies() {
    let (mut s, mut c) = adopted();
    s.execute("line", &json!({ "points": [[0.0, 0.0], [5.0, 0.0]] })).unwrap();
    c["units"] = json!({ "v": 1, "drawing_unit": "m", "locked": true });
    c["project"]["title"] = json!("Renamed House");
    let r = apply(&mut s, &c);
    assert_eq!(r["result"], "partial", "{r}");
    assert_eq!(item(&r, "units")["status"], "refused");
    let unit = s.execute("drawing.units", &json!({ "units": "mm", "mode": "metadata" })).unwrap();
    assert_eq!(unit["units"], "mm");
    assert_eq!(s.execute("project", &json!({})).unwrap()["details"]["title"], "Renamed House");

    // An empty drawing that is not locked may still change its unit.
    let mut empty = Session::new();
    empty.execute("drawing.units", &json!({ "units": "mm", "mode": "metadata" })).unwrap();
    let r = apply(&mut empty, &config(json!({ "units": { "v": 1, "drawing_unit": "ft", "locked": false } })));
    assert_eq!(r["result"], "ok", "{r}");
    // Locked wins even without content.
    let r = apply(&mut empty, &config(json!({ "units": { "v": 1, "drawing_unit": "cm", "locked": true } })));
    assert_eq!(item(&r, "units")["status"], "refused");
}

#[test]
fn a_part_that_fails_puts_the_drawing_back_exactly() {
    let (mut s, mut c) = adopted();
    // The project details apply first and succeed; the levels part then fails (a ceiling above the floor height), so everything is undone.
    c["project"]["title"] = json!("Changed Title");
    let mut bad = c["levels"]["items"][0].clone();
    bad["ceiling_mm"] = json!(9000.0);
    c["levels"]["items"][0] = bad;
    let before = undo_len(&s);
    let doc_before = s.state().unwrap().doc.clone();
    let err = s.execute("project.config.apply", &json!({ "config": c })).unwrap_err().to_string();
    assert!(err.contains("drawing is unchanged"), "{err}");
    assert_eq!(undo_len(&s), before, "no undo entry for a failed apply");
    assert!(std::sync::Arc::ptr_eq(&doc_before, &s.state().unwrap().doc), "the very same drawing");
    assert_eq!(s.execute("project", &json!({})).unwrap()["details"]["title"], "Hospet House");
}

#[test]
fn a_project_without_a_schema_or_with_garbage_is_an_error() {
    let mut s = Session::new();
    assert!(s.execute("project.config.apply", &json!({})).is_err());
    assert!(s.execute("project.config.apply", &json!({ "config": { "levels": { "v": 1, "next_cid": 1, "items": [] } } })).is_err(), "no schema");
    assert!(s.execute("project.config.apply", &json!({ "config": "text" })).is_err());
    assert!(s.execute("project.config.apply", &json!({ "path": "Z:/does/not/exist.osp" })).is_err());
    let r = s.execute("project.config.apply", &json!({ "config": config(json!({ "levels": { "v": 2, "x": 1 } })) })).unwrap();
    assert_eq!(item(&r, "config")["status"], "refused", "a part from a newer version is left out and said so: {r}");
    assert_eq!(r["result"], "partial");
}

#[test]
fn hostile_config_params_never_panic() {
    let mut s = Session::new();
    let huge: Vec<Value> = (0..400).map(|i| l(i, &format!("L{i}"), 0.0, 3000.0)).collect();
    let cases = [
        json!({ "config": null }),
        json!({ "config": [] }),
        json!({ "config": { "schema": { "v": 1, "min_reader": 9 } } }),
        json!({ "config": config(json!({ "levels": levels_json(Value::Array(huge), 1000) })) }),
        json!({ "config": config(json!({ "levels": levels_json(json!([l(1, "A", 1e300, 3000.0)]), 2) })) }),
        json!({ "config": config(json!({ "levels": levels_json(json!([l(1, "A", f64::MAX, f64::MAX)]), 2) })) }),
        json!({ "config": config(json!({ "units": { "v": 1, "drawing_unit": "\u{0}" } })) }),
        json!({ "config": config(json!({ "tabs": { "v": 1, "items": [{ "tid": 1, "name": "A", "layers": vec!["x"; 500] }] } })) }),
        json!({ "path": "\u{0}" }),
        json!({ "path": ".", "record": true }),
        json!({ "path": 5, "record": "yes" }),
        json!({ "config": 7, "path": "x" }),
    ];
    for c in cases {
        let _ = s.execute("project.config.apply", &c);
        let _ = s.execute("project.config.propose", &c);
    }
    // A damaged binding in the drawing is rebuilt, not trusted.
    s.execute("level.new", &json!({ "name": "A" })).unwrap();
    s.doc_mut().unwrap().project.insert("config.levelmap".into(), "{\"v\":1,\"map\":{\"1\":99999,\"x\":-1}}".into());
    let c = config(json!({ "levels": levels_json(json!([l(1, "A", 0.0, 3000.0)]), 2) }));
    let _ = s.execute("project.config.apply", &json!({ "config": c }));
}

#[test]
fn the_result_is_recorded_into_the_file_as_a_local_key_and_a_proposal_for_a_legacy_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.osp");
    let path_s = path.to_string_lossy().to_string();
    let (mut s, c) = adopted();

    // A legacy file: the drawing proposes what it has.
    aad_odp::Odp::create(&path, "House").unwrap();
    let r = s.execute("project.config.propose", &json!({ "path": path_s, "record": true })).unwrap();
    assert_eq!(r["recorded"], true);
    let loaded = aad_odp::config::probe(&path).unwrap();
    assert!(!loaded.configured());
    assert_eq!(loaded.proposal.unwrap().levels.len(), 3);
    let again = s.execute("project.config.propose", &json!({ "path": path_s, "record": true })).unwrap();
    assert_eq!(again["recorded"], false, "one time");

    // MynaOps adopts: one revision for the configuration, then MynaCAD records what it applied without a revision.
    let mut odp = aad_odp::Odp::open(&path).unwrap();
    let parsed = aad_odp::config::ProjectConfig::from_json(&c).unwrap().config;
    let w = aad_odp::config::write(&mut odp, &parsed, "mynaops").unwrap();
    assert_eq!(w.revision, 1);
    drop(odp);
    let r = s.execute("project.config.apply", &json!({ "path": path_s, "record": true })).unwrap();
    assert_eq!((r["result"].as_str(), r["changed"].as_bool(), r["recorded"].as_bool()), (Some("ok"), Some(false), Some(true)), "{r}");
    let loaded = aad_odp::config::probe(&path).unwrap();
    let applied = loaded.applied.unwrap();
    assert_eq!((applied.rev, applied.result.as_str()), (1, "ok"));
    assert_eq!(loaded.revision, 1, "the report is not a revision");
}

#[test]
fn a_bound_level_carries_its_configuration_id_in_the_takeoff_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.osp");
    let mut s = Session::new();
    apply(
        &mut s,
        &config(
            json!({ "units": { "v": 1, "drawing_unit": "mm" }, "levels": levels_json(json!([l(7, "Ground", 0.0, 3000.0), l(9, "First", 3000.0, 3000.0)]), 10) }),
        ),
    );
    s.execute("takeoff.record", &json!({ "path": path.to_string_lossy(), "annotate": false })).unwrap();
    let odp = aad_odp::Odp::open(&path).unwrap();
    let header = &odp.takeoff_headers().unwrap()[0].json;
    let levels = header["levels"].as_array().unwrap();
    let id_of = |n: &str| levels.iter().find(|x| x["name"] == n).unwrap().clone();
    assert_eq!(id_of("Ground")["config_id"], 7);
    assert_eq!(id_of("First")["config_id"], 9);
    assert!(id_of("First")["id"].is_number());
}

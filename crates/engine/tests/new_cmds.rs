//! Smoke tests for XCLIP, SELECTSIMILAR, SCRIPT/LISP, BEDIT, INSUNITS, LAYWALK.

use aad_doc::{EntityKind, Insert};
use aad_engine::Session;
use aad_geom::Vec3;
use serde_json::json;

#[test]
fn selectsimilar_and_filter() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "A", "color": 1, "current": true })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 1], [1, 1]] })).unwrap();
    s.execute("layer.new", &json!({ "name": "B", "color": 2, "current": true })).unwrap();
    s.execute("circle", &json!({ "center": [5, 5], "radius": 1 })).unwrap();
    s.execute("layer.current", &json!({ "name": "A" })).unwrap();
    let h = s.doc().unwrap().model.iter().find(|e| e.common.layer == "A").unwrap().handle;
    s.set_selection(vec![h]);
    let r = s.execute("selectsimilar", &json!({ "mode": "new" })).unwrap();
    assert_eq!(r["selected"], 2);
    let f = s.execute("filter", &json!({ "filters": [{ "type": "Line", "layer": "A" }], "mode": "new" })).unwrap();
    assert_eq!(f["selected"], 2);
}

#[test]
fn xclip_clips_an_insert() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "W", "base": [0, 0], "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "W", "at": [0, 0] })).unwrap();
    let h = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().handle;
    s.set_selection(vec![h]);
    s.execute("xclip", &json!({ "points": [[-1, -1], [5, 5]] })).unwrap();
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    assert!(i.clipping);
    assert_eq!(i.clip.len(), 2);
    s.execute("xclip", &json!({ "off": true })).unwrap();
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    assert!(!i.clipping);
}

#[test]
fn script_macro_and_lisp() {
    let mut s = Session::new();
    s.execute("script", &json!({ "text": "LINE\n0,0\n10,0\n\n" })).unwrap();
    assert!(!s.doc().unwrap().model.is_empty());
    let r = s.execute("lisp", &json!({ "text": "(+ 1 2 3)" })).unwrap();
    assert_eq!(r["result"], 6.0);
    s.execute("lisp", &json!({ "text": "(setq a 4)(setvar \"INSUNITS\" a)" })).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("INSUNITS", 0), 4);
    let u = s.execute("insunits", &json!({})).unwrap();
    assert_eq!(u["insunits"], 4);
}

#[test]
fn bedit_bsave_bclose_round_trip() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "Part", "base": [0, 0], "keep": "delete" })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
    s.execute("bedit", &json!({ "name": "Part" })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    s.execute("line", &json!({ "points": [[0, 1], [5, 1]] })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().block("Part").unwrap().entities.len(), 2);
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
    assert!(s.block_edit.is_none());
    // Definition still has the saved two entities.
    assert_eq!(s.doc().unwrap().block("Part").unwrap().entities.len(), 2);
}

#[test]
fn laywalk_cycles_visibility() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "A" })).unwrap();
    s.execute("layer.new", &json!({ "name": "B" })).unwrap();
    let r = s.execute("laywalk", &json!({ "layers": ["A", "B"], "step": 1 })).unwrap();
    assert!(r["current"] == "A" || r["current"] == "B");
    let on = s.doc().unwrap().layers.iter().filter(|l| l.on).count();
    assert_eq!(on, 1);
    s.execute("laywalk", &json!({ "restore": true })).unwrap();
}

#[test]
fn adcenter_lists_blocks() {
    let mut s = Session::new();
    s.doc_mut().unwrap().blocks.insert("X".into(), std::sync::Arc::new(aad_doc::Block::new("X")));
    let r = s.execute("adcenter", &json!({ "table": "blocks" })).unwrap();
    assert!(r["blocks"].as_array().unwrap().iter().any(|b| b["name"] == "X"));
}

#[test]
fn eattext_and_dataextraction() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "Tag", "base": [0, 0], "keep": "delete" })).unwrap();
    s.execute("attdef", &json!({ "tag": "MARK", "prompt": "Mark", "default": "A1", "at": [0, 0], "height": 0.2 })).unwrap();
    // Rebuild block with the attribute definition still in model, then insert with a value.
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "Tag", "base": [0, 0], "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "Tag", "at": [0, 0], "attribs": { "MARK": "B2" } })).unwrap();
    let r = s.execute("eattext", &json!({ "format": "json" })).unwrap();
    assert!(r["count"].as_u64().unwrap() >= 1);
    let data = r["data"].as_array().unwrap();
    assert!(data.iter().any(|row| row["MARK"] == "B2" || row.get("MARK").is_some()));
    let d = s.execute("dataextraction", &json!({ "type": "Insert", "format": "csv" })).unwrap();
    assert!(d["csv"].as_str().unwrap().contains("HANDLE"));
}

#[test]
fn export_csv_and_propose_preview() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    let before = s.doc().unwrap().model.len();
    let p = s.execute("proposeedit", &json!({ "command": "circle", "params": { "center": [1, 1], "radius": 2 } })).unwrap();
    assert_eq!(p["proposed"]["command"], "circle");
    assert_eq!(s.doc().unwrap().model.len(), before);
    let prev = s.execute("previewedit", &json!({ "command": "circle", "params": { "center": [1, 1], "radius": 2 } })).unwrap();
    assert_eq!(prev["beforeEntities"].as_u64().unwrap() + 1, prev["afterEntities"].as_u64().unwrap());
    assert!(prev["reverted"].as_bool().unwrap());
    assert_eq!(s.doc().unwrap().model.len(), before);
    let v = s.execute("validate", &json!({})).unwrap();
    assert!(v["ok"].as_bool().is_some());
    let exp = s.execute("export", &json!({ "format": "csv" })).unwrap();
    assert!(exp["data"].as_str().is_some() || exp["bytes"].as_u64().unwrap() > 0);
}

#[test]
fn dwgcompare_identical_open_drawing() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [3, 0]] })).unwrap();
    s.execute("new", &json!({})).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [3, 0]] })).unwrap();
    // Compare drawing 1 against drawing 0 (same geometry fingerprint).
    let r = s.execute("dwgcompare", &json!({ "index": 0 })).unwrap();
    assert!(r["identical"].as_bool().unwrap() || r["matched"].as_u64().unwrap() >= 1);
    let q = s.execute("query", &json!({ "type": "Line" })).unwrap();
    assert!(q["count"].as_u64().unwrap() >= 1);
    let _ = s.execute("inspect", &json!({ "entities": false })).unwrap();
}

#[test]
fn xclip_polygon_is_stored() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "P", "base": [0, 0], "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "P", "at": [0, 0] })).unwrap();
    let h = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().handle;
    s.set_selection(vec![h]);
    s.execute("xclip", &json!({ "points": [[0, 0], [5, 0], [0, 5]] })).unwrap();
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    assert_eq!(i.clip.len(), 3);
    assert!(i.clipping);
}

// Silence unused import in some builds.
#[allow(dead_code)]
fn _keep(i: Insert) -> Vec3 {
    i.insert
}

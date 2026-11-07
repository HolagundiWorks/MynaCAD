//! Smart blocks: insert with parameters, edit, read back, survive a DXF round trip, and refuse
//! hostile definitions and values without changing the drawing.

use aad_engine::Session;
use aad_engine::doc::EntityKind;
use serde_json::{Value, json};

fn metric() -> Session {
    let mut s = Session::empty();
    s.new_drawing(true);
    s
}

/// (block name, x, y, scale, layer) of the reference `h`.
fn reference(s: &Session, h: &str) -> (String, f64, f64, f64, String) {
    let h = aad_engine::doc::Handle::parse_hex(h).unwrap();
    let e = s.doc().unwrap().entity(h).unwrap();
    let EntityKind::Insert(i) = &e.kind else { panic!("not a block reference") };
    (i.block.clone(), i.insert.x, i.insert.y, i.scale.x, e.common.layer.clone())
}

fn bars(s: &Session, block: &str) -> usize {
    let b = s.doc().unwrap().block(block).unwrap();
    b.entities.iter().filter(|e| matches!(&e.kind, EntityKind::LwPolyline(p) if p.const_width > 0.0)).count()
}

#[test]
fn insert_reuses_blocks_and_edit_repoints_the_reference() {
    let mut s = metric();
    let a = s.execute("smartblock.insert", &json!({ "id": "rcc.column.rect", "at": [1000, 2000] })).unwrap();
    let b = s.execute("smartblock.insert", &json!({ "id": "rcc.column.rect", "at": [5000, 2000] })).unwrap();
    // The new reference is selected, so Properties lists its parameters.
    assert_eq!(s.selection().iter().map(|h| h.hex()).collect::<Vec<_>>(), vec![b["handle"].as_str().unwrap().to_string()]);
    assert_eq!(a["block"], b["block"], "same parameters share one block");
    let h = a["handle"].as_str().unwrap().to_string();
    let (name, x, y, scale, layer) = reference(&s, &h);
    assert_eq!((x, y, scale, layer.as_str()), (1000.0, 2000.0, 1.0, "S-RCC"));
    assert_eq!(bars(&s, &name), 6, "2 bars on each width face and 1 more on each depth face");

    let e = s.execute("smartblock.edit", &json!({ "handle": h, "params": "ny=4, bar=20" })).unwrap();
    let (edited, x2, y2, _, _) = reference(&s, &h);
    assert_ne!(edited, name);
    assert_eq!(e["blocks"][0], edited.as_str());
    assert_eq!((x2, y2), (1000.0, 2000.0), "an edit keeps the position");
    assert_eq!(bars(&s, &edited), 8);
    let got = s.execute("smartblock.get", &json!({ "handle": h })).unwrap();
    assert_eq!(got["id"], "rcc.column.rect");
    assert_eq!(got["params"]["ny"], 4.0);
    assert_eq!(got["params"]["bar"], 20.0);
    assert_eq!(got["params"]["width"], 230.0, "unchanged parameters stay");
    // The other reference is untouched.
    assert_eq!(reference(&s, b["handle"].as_str().unwrap()).0, name);

    s.execute("undo", &json!({})).unwrap();
    assert_eq!(reference(&s, &h).0, name, "undo restores the previous block");
}

#[test]
fn choices_by_label_and_text_labels() {
    let mut s = metric();
    let r = s.execute("smartblock.insert", &json!({ "id": "plumb.riser", "at": [0, 0], "params": { "kind": "RWP", "dia": 100 } })).unwrap();
    let b = s.doc().unwrap().block(r["block"].as_str().unwrap()).unwrap().clone();
    let texts: Vec<String> = b.entities.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
    assert_eq!(texts, vec!["RWP 100".to_string()]);
    assert_eq!(r["params"]["kind"], 3.0);
}

#[test]
fn drawing_units_scale_the_millimetre_geometry() {
    let mut s = metric();
    let r = s.execute("smartblock.insert", &json!({ "id": "int.bed", "at": [0, 0], "units": "m" })).unwrap();
    let (_, _, _, scale, _) = reference(&s, r["handle"].as_str().unwrap());
    assert!((scale - 0.001).abs() < 1e-12);
    let r = s.execute("smartblock.insert", &json!({ "id": "int.bed", "at": [0, 0], "scale": 2 })).unwrap();
    assert_eq!(reference(&s, r["handle"].as_str().unwrap()).3, 2.0);
}

#[test]
fn every_library_block_inserts_with_defaults() {
    let mut s = metric();
    let list = s.execute("smartblock.list", &json!({})).unwrap();
    let blocks = list["blocks"].as_array().unwrap();
    assert!(blocks.len() >= 39);
    for (i, b) in blocks.iter().enumerate() {
        let id = b["id"].as_str().unwrap();
        let r = s.execute("smartblock.insert", &json!({ "id": id, "at": [i as f64 * 3000.0, 0] }));
        assert!(r.is_ok(), "{id}: {r:?}");
    }
    let rcc = s.execute("smartblock.list", &json!({ "discipline": "rcc" })).unwrap();
    assert!(rcc["blocks"].as_array().unwrap().iter().all(|b| b["discipline"] == "rcc"));
}

#[test]
fn smart_blocks_survive_a_dxf_round_trip() {
    let mut s = metric();
    let r = s.execute("smartblock.insert", &json!({ "id": "int.sofa", "at": [0, 0], "params": { "seats": 2 } })).unwrap();
    let def = json!({
        "id": "my.tag", "title": "My tag", "layer": "A-TAG",
        "params": [{ "name": "size", "default": 300, "min": 100, "max": 900 }],
        "items": [{ "crect": [0, 0, "size", "size"] }]
    });
    let t = s.execute("smartblock.insert", &json!({ "definition": def, "at": [5000, 0] })).unwrap();
    let text = aad_io::write_dxf(s.doc().unwrap());
    let back = aad_io::read(text.as_bytes(), "sb.dxf").unwrap();
    let mut u = Session::empty();
    u.open_drawing(back, "sb", None);
    let h = r["handle"].as_str().unwrap();
    assert_eq!(u.execute("smartblock.get", &json!({ "handle": h })).unwrap()["params"]["seats"], 2.0);
    u.execute("smartblock.edit", &json!({ "handle": h, "params": { "seats": 4 } })).unwrap();
    assert_eq!(u.execute("smartblock.get", &json!({ "handle": h })).unwrap()["params"]["seats"], 4.0);
    // The inline definition was kept in the drawing, so its reference stays editable.
    let th = t["handle"].as_str().unwrap();
    u.execute("smartblock.edit", &json!({ "handle": th, "params": "size=500" })).unwrap();
    assert_eq!(u.execute("smartblock.get", &json!({ "handle": th })).unwrap()["params"]["size"], 500.0);
    let listed = u.execute("smartblock.list", &json!({})).unwrap();
    assert!(listed["blocks"].as_array().unwrap().iter().any(|b| b["id"] == "my.tag" && b["stored"] == true));
}

fn refused(s: &mut Session, p: Value) {
    let before = s.doc().unwrap().blocks.len();
    let ents = s.doc().unwrap().model.iter().count();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.execute("smartblock.insert", &p)));
    assert!(matches!(r, Ok(Err(_))), "{p} must be refused without a panic: {r:?}");
    assert_eq!(s.doc().unwrap().blocks.len(), before, "{p} made a block");
    assert_eq!(s.doc().unwrap().model.iter().count(), ents, "{p} added objects");
}

#[test]
fn hostile_params_never_panic() {
    let mut s = metric();
    let col = "rcc.column.rect";
    for p in [
        json!({ "id": col }),
        json!({ "id": "nope", "at": [0, 0] }),
        json!({ "id": col, "at": [0, 0], "params": { "width": 10 } }),
        json!({ "id": col, "at": [0, 0], "params": { "width": 1e308 } }),
        json!({ "id": col, "at": [0, 0], "params": { "nx": 2.5 } }),
        json!({ "id": col, "at": [0, 0], "params": { "bar": 17 } }),
        json!({ "id": col, "at": [0, 0], "params": { "bogus": 1 } }),
        json!({ "id": col, "at": [0, 0], "params": "width" }),
        json!({ "id": col, "at": [0, 0], "params": "width=abc" }),
        json!({ "id": col, "at": [0, 0], "params": [1, 2] }),
        json!({ "id": col, "at": [0, 0], "scale": 0 }),
        json!({ "id": col, "at": [0, 0], "scale": -1 }),
        json!({ "id": col, "at": [0, 0], "layer": "a/b" }),
        json!({ "definition": "x", "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "repeat": { "var": "i", "count": 1e9, "items": [{ "line": [0, 0, 1, 1] }] } }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "repeat": { "var": "i", "count": 1000, "items": [{ "repeat": { "var": "j", "count": 1000, "items": [{ "line": [0, 0, 1, 1] }] } }] } }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, "1/0", 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, "sqrt(-1)", 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, "1e300*1e300", 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, format!("{}1{}", "(".repeat(5000), ")".repeat(5000)), 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, "undefined_name", 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "hatch": { "pts": [[0, 0], [1, 0], [1, 1]], "pattern": "NOPE" } }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, 1, 1], "linetype": "NOPE" }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [] }, "at": [0, 0] }),
        json!({ "definition": { "id": "bad id!", "title": "X", "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "params": [{ "name": "mm", "default": 1 }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "params": [{ "name": "a", "default": 5, "max": 1 }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "text": { "at": [0, 0], "height": 0, "value": "x" } }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "poly": { "pts": [[0, 0]] } }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "extra": 1, "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "items": [{ "line": [0, 0, "1 +", 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "grips": [{ "param": "nope", "at": [0, 0], "axis": "x" }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "params": [{ "name": "a", "default": 1 }], "grips": [{ "param": "a", "at": [0, 0], "axis": "z" }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "params": [{ "name": "a", "default": 1 }], "grips": [{ "param": "a", "at": [0, 0], "axis": "x", "per": 0 }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "params": [{ "name": "a", "default": 1 }], "grips": [{ "param": "a", "at": [0, 0], "axis": "x", "step": -1 }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
        json!({ "definition": { "id": "x", "title": "X", "params": [{ "name": "a", "default": 1, "choices": [{ "label": "One", "value": 1 }] }], "grips": [{ "param": "a", "at": [0, 0], "axis": "x" }], "items": [{ "line": [0, 0, 1, 1] }] }, "at": [0, 0] }),
    ] {
        refused(&mut s, p);
    }
    // Edits of something that is not a smart block, or with bad values, are refused too.
    let l = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    // Whatever the line command reports, this is the handle of the one object that is not a smart block.
    let lh = l["handles"][0].as_str().unwrap_or_default().to_string();
    assert!(!lh.is_empty(), "the line reports its handle: {l}");
    assert!(s.execute("smartblock.edit", &json!({ "handle": lh, "params": {} })).is_err());
    let r = s.execute("smartblock.insert", &json!({ "id": "rcc.column.rect", "at": [0, 0] })).unwrap();
    assert!(s.execute("smartblock.edit", &json!({ "handle": r["handle"], "params": { "width": -5 } })).is_err());
    assert!(s.execute("smartblock.get", &json!({ "handle": lh })).is_err());
}

/// (parameter grips, all grips) of reference `h`.
fn grip_points(s: &Session, h: &str) -> (Vec<aad_engine::geom::Vec2>, Vec<aad_engine::geom::Vec2>) {
    let h = aad_engine::doc::Handle::parse_hex(h).unwrap();
    let all = s.grips_of(h).unwrap();
    let own = s.doc().unwrap().entity(h).unwrap().kind.grips().len();
    (all[own..].to_vec(), all)
}

#[test]
fn every_parameter_grip_sits_on_the_block_and_drags_its_parameter() {
    let list = metric().execute("smartblock.list", &json!({})).unwrap();
    let mut with_grips = 0;
    for b in list["blocks"].as_array().unwrap() {
        let id = b["id"].as_str().unwrap();
        let specs = b["grips"].as_array().unwrap();
        let mut s = metric();
        let r = s.execute("smartblock.insert", &json!({ "id": id, "at": [0, 0] })).unwrap();
        let h = r["handle"].as_str().unwrap().to_string();
        let (pg, all) = grip_points(&s, &h);
        assert_eq!(pg.len(), specs.len(), "{id}: one grip per grip definition");
        if specs.is_empty() {
            continue;
        }
        with_grips += 1;
        let ext = s.doc().unwrap().extents(&aad_engine::doc::Space::Model).expand(1.0);
        for (k, g) in pg.iter().enumerate() {
            assert!(ext.contains(*g), "{id}: grip {k} at {g:?} is off the block");
        }
        for (k, spec) in specs.iter().enumerate() {
            let mut s = metric();
            let h = s.execute("smartblock.insert", &json!({ "id": id, "at": [0, 0] })).unwrap()["handle"].as_str().unwrap().to_string();
            let name = spec["param"].as_str().unwrap();
            let before = s.execute("smartblock.get", &json!({ "handle": h })).unwrap()["params"][name].as_f64().unwrap();
            let d = if spec["axis"] == "x" { [20.0, 0.0] } else { [0.0, 20.0] };
            let to = [pg[k].x + d[0], pg[k].y + d[1]];
            let index = all.len() - pg.len() + k;
            s.execute("grip.move", &json!({ "handle": h, "index": index, "to": to })).unwrap_or_else(|e| panic!("{id} grip {name}: {e}"));
            let after = s.execute("smartblock.get", &json!({ "handle": h })).unwrap()["params"][name].as_f64().unwrap();
            let max = b["params"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap()["max"].as_f64();
            assert!(after > before || Some(before) == max, "{id}: dragging the {name} grip outwards gave {before} -> {after}");
        }
    }
    assert!(with_grips >= 30, "most library blocks have grips ({with_grips})");
}

#[test]
fn a_grip_drag_follows_rotation_units_and_range_and_undoes() {
    let mut s = metric();
    let r = s.execute("smartblock.insert", &json!({ "id": "rcc.column.rect", "at": [10, 5], "rotation": 90, "units": "m" })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    let (pg, all) = grip_points(&s, &h);
    let width = all.len() - pg.len();
    // Rotated 90°: the width grip (block +x) is above the insertion point, 0.115 m away.
    assert!((pg[0].x - 10.0).abs() < 1e-9 && (pg[0].y - 5.115).abs() < 1e-9, "{:?}", pg[0]);
    // Drag 0.1 m further out along block +x (world +y): width grows by 2 × 100 mm.
    s.execute("grip.move", &json!({ "handle": h, "index": width, "to": [10.3, 5.215] })).unwrap();
    let got = s.execute("smartblock.get", &json!({ "handle": h })).unwrap();
    assert_eq!(got["params"]["width"], 430.0);
    assert_eq!(reference(&s, &h).1, 10.0, "the reference stays where it was");
    // Far beyond the range: kept at the maximum.
    s.execute("grip.move", &json!({ "handle": h, "index": width, "to": [10.0, 50.0] })).unwrap();
    assert_eq!(s.execute("smartblock.get", &json!({ "handle": h })).unwrap()["params"]["width"], 2000.0);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.execute("smartblock.get", &json!({ "handle": h })).unwrap()["params"]["width"], 430.0);
    // A grip index past the last one is refused.
    assert!(s.execute("grip.move", &json!({ "handle": h, "index": all.len(), "to": [0, 0] })).is_err());
}

#[test]
fn the_properties_rows_show_and_set_parameters() {
    let mut s = metric();
    let r = s.execute("smartblock.insert", &json!({ "id": "rcc.column.rect", "at": [0, 0] })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    let fields = {
        let d = s.doc().unwrap();
        let e = d.entity(aad_engine::doc::Handle::parse_hex(&h).unwrap()).unwrap();
        let EntityKind::Insert(i) = &e.kind else { panic!() };
        aad_engine::smart_block_fields(d, i)
    };
    let row = |k: &str| fields.iter().find(|f| f["key"] == k).cloned().unwrap_or_else(|| panic!("no row {k}"));
    assert_eq!(row("sb:width")["value"], "230");
    assert_eq!(row("sb:width")["label"], "Width (mm)");
    assert_eq!(row("sb:bar")["value"], "16");
    assert!(row("sb:bar")["choices"].as_array().unwrap().iter().any(|c| c == "20"));
    assert_eq!(fields[0]["label"], "Parametric block");

    s.execute("properties.set", &json!({ "handles": [h], "sb:width": 300, "sb:bar": "20" })).unwrap();
    let got = s.execute("smartblock.get", &json!({ "handle": h })).unwrap();
    assert_eq!((got["params"]["width"].as_f64(), got["params"]["bar"].as_f64()), (Some(300.0), Some(20.0)));
    assert!(s.execute("properties.set", &json!({ "handles": [h], "sb:width": 5 })).is_err(), "out of range is refused");
    assert!(s.execute("properties.set", &json!({ "handles": [h], "sb:nope": 5 })).is_err());
    // A plain line has no smart block rows.
    let l = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    assert!(s.execute("properties.set", &json!({ "handles": [l["handle"]], "sb:width": 300 })).is_err());
}

#[test]
fn the_insert_form_lists_the_library() {
    let f = aad_engine::forms::get("smartblock.insert").unwrap();
    let p = aad_engine::forms::parse_input(f, &json!({ "id": "elec.socket", "params": "amp=16", "at": [10, 20] })).unwrap();
    let mut s = metric();
    let r = s.execute("smartblock.insert", &p).unwrap();
    assert_eq!(r["params"]["amp"], 16.0);
}

#[test]
fn the_edit_form_changes_the_selection() {
    let mut s = metric();
    let r = s.execute("smartblock.insert", &json!({ "id": "int.sofa", "at": [0, 0] })).unwrap();
    let h = aad_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.set_selection(vec![h]);
    let f = aad_engine::forms::get("smartblock.edit").unwrap();
    let p = aad_engine::forms::parse_input(f, &json!({ "params": "seats=5" })).unwrap();
    s.execute("smartblock.edit", &p).unwrap();
    assert_eq!(s.execute("smartblock.get", &json!({})).unwrap()["params"]["seats"], 5.0);
}

#[test]
fn every_discipline_dxf_in_the_catalogs_reads() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/blocks");
    let mut n = 0;
    for d in ["plumbing", "hvac", "electrical", "structural", "interior"] {
        let dir = root.join(d);
        let cat: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("catalog.json")).unwrap()).unwrap();
        assert_eq!(cat["licence"], "CC0 1.0");
        for it in cat["items"].as_array().unwrap() {
            let file = dir.join(it["file"].as_str().unwrap());
            let d = aad_io::read(&std::fs::read(&file).unwrap(), &file.to_string_lossy()).unwrap();
            assert!(d.model.iter().count() > 0, "{} is empty", file.display());
            assert!(it["source"].as_str().unwrap().starts_with("https://www.innerscene.com/"));
            n += 1;
        }
    }
    assert_eq!(n, 79);
}

#[test]
fn a_metric_plan_flagged_as_inches_still_gets_blocks_at_millimetre_scale() {
    // A plan 57 m long drawn in millimetres whose header says inches (the default of an untouched
    // template): the blocks must come in at 1:1 in millimetres, not shrunk by 25.4.
    let mut s = Session::empty();
    s.new_drawing(false);
    s.doc_mut().unwrap().header.set_i64("INSUNITS", 1);
    s.execute("line", &json!({"points": [[0, 0], [57000, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [0, 7400]]})).unwrap();
    let r = s.execute("smartblock.insert", &json!({ "id": "int.bed", "at": [1000, 1000] })).unwrap();
    assert_eq!(reference(&s, r["handle"].as_str().unwrap()).3, 1.0, "millimetres inferred from the size of the plan");
    // A small drawing that says inches stays in inches, and a stated metric unit is trusted.
    let mut small = Session::empty();
    small.new_drawing(false);
    small.doc_mut().unwrap().header.set_i64("INSUNITS", 1);
    small.execute("line", &json!({"points": [[0, 0], [400, 0]]})).unwrap();
    let r = small.execute("smartblock.insert", &json!({ "id": "int.bed", "at": [0, 0] })).unwrap();
    assert!((reference(&small, r["handle"].as_str().unwrap()).3 - 0.001 / 0.0254).abs() < 1e-9);
    let mut metres = Session::empty();
    metres.new_drawing(false);
    metres.doc_mut().unwrap().header.set_i64("INSUNITS", 6);
    metres.execute("line", &json!({"points": [[0, 0], [57000, 0]]})).unwrap();
    let r = metres.execute("smartblock.insert", &json!({ "id": "int.bed", "at": [0, 0] })).unwrap();
    assert!((reference(&metres, r["handle"].as_str().unwrap()).3 - 0.001).abs() < 1e-12);
}

#[test]
fn every_smart_block_has_a_thumbnail_and_previewing_changes_nothing() {
    let mut s = metric();
    let ids: Vec<String> = s.execute("smartblock.list", &json!({})).unwrap()["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap().to_string())
        .collect();
    let before = s.doc().unwrap().blocks.len();
    for id in &ids {
        let r = s.execute("blocks.preview", &json!({ "smartblock": id })).unwrap();
        assert!(!r["strokes"].as_array().unwrap().is_empty(), "{id} has no thumbnail strokes");
    }
    assert_eq!(s.doc().unwrap().blocks.len(), before);
    assert!(s.execute("blocks.preview", &json!({ "smartblock": "no.such" })).is_err());
}

#[test]
fn the_rcc_tools_place_their_blocks_and_the_defaults_comply_with_is_456() {
    let mut s = metric();
    for (cmd, id) in [
        ("rcc.column", "rcc.column.rect"),
        ("rcc.column.round", "rcc.column.circular"),
        ("rcc.beam", "rcc.beam.section"),
        ("rcc.footing", "rcc.footing.section"),
        ("rcc.slab", "rcc.slab.section"),
        ("rcc.lintel", "rcc.lintel.section"),
    ] {
        let r = s.execute(cmd, &json!({ "at": [0, 0] })).unwrap();
        let got = s.execute("smartblock.get", &json!({ "handle": r["handle"] })).unwrap();
        assert_eq!(got["id"], id, "{cmd}");
        let check = s.execute("smartblock.check", &json!({ "handle": r["handle"] })).unwrap();
        assert_eq!(check["ok"], true, "{id}: {}", check["issues"]);
        assert!(!check["lines"].as_array().unwrap().is_empty(), "{id} reports no quantities");
    }
}

#[test]
fn electrical_and_plumbing_tools_place_their_symbols() {
    let mut s = metric();
    for (cmd, id) in [
        ("elec.light", "elec.light"),
        ("elec.socket", "elec.socket"),
        ("elec.switchboard", "elec.switchboard"),
        ("elec.fan.ceiling", "elec.fan.ceiling"),
        ("elec.downlight", "elec.downlight"),
        ("elec.db", "elec.db"),
        ("plumb.wc.european", "plumb.wc.european"),
        ("plumb.wc.indian", "plumb.wc.indian"),
        ("plumb.basin", "plumb.basin"),
        ("plumb.sink.kitchen", "plumb.sink.kitchen"),
        ("plumb.shower", "plumb.shower"),
        ("plumb.floor.trap", "plumb.floor.trap"),
        ("plumb.chamber", "plumb.chamber"),
        ("plumb.tank.round", "plumb.tank.round"),
        ("plumb.riser", "plumb.riser"),
        ("int.bed", "int.bed"),
        ("hvac.diffuser.square", "hvac.diffuser.square"),
    ] {
        let r = s.execute(cmd, &json!({ "at": [0.0, 0.0] })).unwrap();
        let got = s.execute("smartblock.get", &json!({ "handle": r["handle"] })).unwrap();
        assert_eq!(got["id"], id, "{cmd}");
    }
}

#[test]
fn properties_list_the_is_456_result_and_follow_an_edit() {
    let mut s = metric();
    let r = s.execute("rcc.column", &json!({ "at": [0, 0] })).unwrap();
    let h = aad_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let rows = |s: &Session| -> Vec<(String, String)> {
        let doc = s.doc().unwrap();
        let EntityKind::Insert(i) = &doc.entity(h).unwrap().kind else { panic!("not a block reference") };
        aad_engine::smart_block_fields(doc, i)
            .iter()
            .map(|r| (r["label"].as_str().unwrap_or("").to_string(), r["value"].as_str().unwrap_or("").to_string()))
            .collect()
    };
    let before = rows(&s);
    assert!(before.contains(&("IS 456:2000".to_string(), "Complies".to_string())), "{before:?}");
    assert!(before.iter().any(|(l, _)| l == "Longitudinal bars"), "{before:?}");
    assert!(before.iter().any(|(l, _)| l == "Concrete grade"), "the grade is a parameter: {before:?}");
    // Reinforcement is edited in Properties (`sb:` keys): a 10 mm bar breaks the 12 mm minimum.
    s.execute("properties.set", &json!({ "handles": [r["handle"]], "sb:bar": "10", "sb:tiesp": "300" })).unwrap();
    let after = rows(&s);
    assert!(after.iter().any(|(l, v)| l == "Check, cl. 26.5.3.1(c)" && v.contains("12 mm")), "{after:?}");
    assert!(after.iter().any(|(l, _)| l == "Check, cl. 26.5.3.2(c)(1)"), "tie spacing 300 exceeds the 230 mm least side: {after:?}");
    assert!(!after.contains(&("IS 456:2000".to_string(), "Complies".to_string())));
}

#[test]
fn a_definition_can_be_checked_with_parameters_before_it_is_placed() {
    let mut s = metric();
    let ok = s.execute("smartblock.check", &json!({ "id": "rcc.slab.section" })).unwrap();
    assert_eq!(ok["ok"], true);
    let bad = s.execute("smartblock.check", &json!({ "id": "rcc.slab.section", "params": "spacing=450, exposure=3, cover=20" })).unwrap();
    assert_eq!(bad["ok"], false);
    let clauses: Vec<&str> = bad["issues"].as_array().unwrap().iter().map(|i| i["clause"].as_str().unwrap()).collect();
    assert!(clauses.contains(&"26.3.3(b)") && clauses.contains(&"26.4.2, Table 16"), "{clauses:?}");
    assert!(s.execute("smartblock.check", &json!({ "id": "rcc.footing.plan" })).is_err(), "a plan has no reinforcement to check");
    assert!(s.execute("smartblock.check", &json!({ "id": "int.bed" })).is_err());
}

#[test]
fn the_structural_details_list_each_placed_rcc_block_with_its_parameters() {
    let mut s = metric();
    let beam = s.execute("rcc.beam", &json!({ "at": [0, 0], "params": "width=300, nbot=4, barbot=20" })).unwrap();
    s.execute("rcc.column", &json!({ "at": [5000, 0] })).unwrap();
    s.execute("smartblock.insert", &json!({ "id": "int.bed", "at": [9000, 0] })).unwrap(); // not structural
    s.execute("rcc.footing", &json!({ "at": [0, 5000] })).unwrap();
    let details = aad_engine::structural_details(s.doc().unwrap(), "plan.dxf");
    let kinds: Vec<(&str, &str)> = details.iter().map(|d| (d.kind.as_str(), d.block.as_str())).collect();
    assert_eq!(details.len(), 3, "{kinds:?}");
    for want in [("beams", "rcc.beam.section"), ("columns", "rcc.column.rect"), ("footings", "rcc.footing.section")] {
        assert!(kinds.contains(&want), "{want:?} in {kinds:?}");
    }
    let b = details.iter().find(|d| d.kind == "beams").unwrap();
    assert_eq!(b.handle, beam["handle"].as_str().unwrap());
    assert_eq!(b.id, format!("plan.dxf/{}", b.handle));
    assert_eq!(b.layer, "S-RCC");
    assert_eq!(b.params["width"], 300.0);
    assert_eq!(b.params["nbot"], 4.0);
    assert_eq!(b.params["barbot"], 20.0);
    assert_eq!(b.params["cover"], 25.0, "unset values are the defaults, so a reader needs no definition");
    assert_eq!(b.params["fck"], 25.0);
    assert!(b.label.starts_with("RCC beam cross-section") && b.label.ends_with("(300 x 450)"), "{}", b.label);
}

/// The reinforced concrete blocks draw on an ISO-style set of layers, each with a colour, a linetype and a pen width, made in the drawing with the block.
#[test]
fn structural_blocks_draw_on_layers_with_colours_linetypes_and_lineweights() {
    use aad_engine::doc::Lineweight;
    let mut s = metric();
    let c = s.execute("smartblock.insert", &json!({ "id": "rcc.column.rect", "at": [0, 0] })).unwrap();
    s.execute("smartblock.insert", &json!({ "id": "rcc.footing.plan", "at": [4000, 0] })).unwrap();
    s.execute("smartblock.insert", &json!({ "id": "rcc.footing.section", "at": [8000, 0] })).unwrap();
    let d = s.doc().unwrap();
    let look = |name: &str| {
        let l = d.layer(name).unwrap_or_else(|| panic!("layer {name} was not made"));
        (l.color.to_aci(), l.linetype.clone(), l.lineweight)
    };
    assert_eq!(look("S-CONC-OUTL"), (7, "Continuous".into(), Lineweight::Mm100(50)));
    assert_eq!(look("S-REBR-MAIN"), (1, "Continuous".into(), Lineweight::Mm100(50)));
    assert_eq!(look("S-REBR-LINK"), (3, "Continuous".into(), Lineweight::Mm100(25)));
    assert_eq!(look("S-ANNO-DIMS"), (30, "Continuous".into(), Lineweight::Mm100(18)));
    assert_eq!(look("S-CONC-HIDD"), (8, "HIDDEN".into(), Lineweight::Mm100(25)));
    assert_eq!(look("S-GRND"), (8, "CENTER".into(), Lineweight::Mm100(18)));
    assert!(d.linetype("HIDDEN").is_some() && d.linetype("CENTER").is_some(), "the linetypes are loaded with the layers");
    // Nothing is left on layer 0 or on a colour of its own where the layer gives it: bars and ties take their colour from their layer.
    let (name, ..) = reference(&s, c["handle"].as_str().unwrap());
    let b = d.block(&name).unwrap();
    assert!(b.entities.iter().all(|e| e.common.layer.starts_with("S-")), "every part is on a structural layer");
    assert!(b.entities.iter().all(|e| matches!(e.common.color, aad_color::Color::ByLayer)), "colours come from the layers");
    let on = |layer: &str| b.entities.iter().filter(|e| e.common.layer == layer).count();
    assert_eq!(on("S-REBR-MAIN"), 6, "the bars of the default column");
    assert!(on("S-ANNO-DIMS") > 0 && on("S-ANNO-NOTE") > 0 && on("S-REBR-LINK") > 0);
}

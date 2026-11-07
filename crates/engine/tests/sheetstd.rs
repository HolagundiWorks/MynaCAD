//! The standard sheet-set mapping: layers and block names per discipline tab, the symbol blocks,
//! the layer a tab makes current, and the mapping surviving a file round trip.

use aad_engine::Session;
use serde_json::{Value, json};

fn sets(s: &mut Session) -> Vec<Value> {
    s.execute("sheetset.list", &json!({})).unwrap()["sheetSets"].as_array().unwrap().clone()
}

fn set(s: &mut Session, name: &str) -> Value {
    sets(s).into_iter().find(|x| x["name"] == name).unwrap_or_else(|| panic!("no sheet set {name}"))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

#[test]
fn the_standard_creates_the_tabs_with_their_layers_symbols_and_mapping() {
    let mut s = Session::new();
    let r = s.execute("sheetset.standard", &json!({})).unwrap();
    assert_eq!(r["sets"].as_array().unwrap().len(), 5);
    assert!(r["layersCreated"].as_u64().unwrap() >= 20, "{r}");
    assert_eq!(r["blocksCreated"].as_u64(), Some(29), "{r}");
    let d = s.doc().unwrap();
    for layer in ["A-WALL", "E-POINT", "E-CEIL", "P-FIXT", "P-WATR", "I-FURN", "FC-PANL"] {
        assert!(d.layer(layer).is_some(), "layer {layer}");
    }
    for block in ["E-SW1", "E-LIGHT", "E-FAN", "P-WC", "P-BASIN", "P-TAP", "I-BED-D", "FC-LIGHT"] {
        let b = d.block(block).unwrap_or_else(|| panic!("block {block}"));
        assert!(!b.entities.is_empty() && !b.description.is_empty(), "{block} is drawn and described");
    }
    let electrical = set(&mut s, "Electrical");
    assert_eq!(electrical["layerPrefix"], "E-");
    assert_eq!(strings(&electrical["layers"]), ["E-*"]);
    assert!(strings(&electrical["blocks"]).contains(&"E-*".to_string()));
    // Nothing is made twice, and the layers keep the colour and lineweight of the table.
    let again = s.execute("sheetset.standard", &json!({})).unwrap();
    assert_eq!((again["layersCreated"].as_u64(), again["blocksCreated"].as_u64()), (Some(0), Some(0)));
    assert_eq!(s.doc().unwrap().sheet_sets.len(), 5);
    assert_eq!(s.doc().unwrap().layer("E-BOARD").unwrap().color.name(), "Red");
}

#[test]
fn the_standard_is_one_undo_step_and_can_be_limited_to_named_tabs() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({"sets": ["Plumbing"], "symbols": false})).unwrap();
    assert_eq!(sets(&mut s).len(), 1);
    assert!(s.doc().unwrap().layer("P-FIXT").is_some() && s.doc().unwrap().layer("E-POINT").is_none());
    assert!(s.doc().unwrap().block("P-WC").is_none(), "symbols: false adds no blocks");
    s.execute("u", &json!({})).unwrap();
    assert!(sets(&mut s).is_empty() && s.doc().unwrap().layer("P-FIXT").is_none(), "one undo reverses it all");
    assert!(s.execute("sheetset.standard", &json!({"sets": ["Landscape"]})).is_err(), "only the standard tabs");
    assert!(s.execute("sheetset.standard", &json!({"sets": 5})).is_err());
    assert!(sets(&mut s).is_empty(), "a refused call changes nothing");
}

#[test]
fn symbols_are_drawn_in_millimetres_whatever_the_drawing_unit() {
    let extent = |s: &Session, name: &str| -> f64 {
        let b = s.doc().unwrap().block(name).unwrap().clone();
        b.entities
            .iter()
            .filter_map(|e| match &e.kind {
                aad_engine::doc::EntityKind::Circle(c) => Some(c.radius),
                _ => None,
            })
            .fold(0.0, f64::max)
    };
    let mut mm = Session::new();
    mm.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    mm.execute("sheetset.standard", &json!({"sets": ["Electrical"]})).unwrap();
    let mut m = Session::new();
    m.execute("drawing.units", &json!({"units": "m", "mode": "metadata"})).unwrap();
    m.execute("sheetset.standard", &json!({"sets": ["Electrical"]})).unwrap();
    assert!((extent(&mm, "E-FAN") - 300.0).abs() < 1e-9, "a 600 mm fan in a millimetre drawing");
    assert!((extent(&m, "E-FAN") - 0.3).abs() < 1e-9, "the same fan in a metre drawing");
}

#[test]
fn the_mapping_can_be_edited_cleared_and_is_validated() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({})).unwrap();
    s.execute("sheetset.set", &json!({"set": "Electrical", "layers": " E-*, LIGHTS ,e-*", "blocks": ["SW*", "fan-*"]})).unwrap();
    let e = set(&mut s, "Electrical");
    assert_eq!(strings(&e["layers"]), ["E-*", "LIGHTS"], "trimmed and without duplicates");
    assert_eq!(strings(&e["blocks"]), ["SW*", "fan-*"]);
    s.execute("sheetset.set", &json!({"set": "Electrical", "layerPrefix": "EL-"})).unwrap();
    assert_eq!(strings(&set(&mut s, "Electrical")["layers"]), ["E-*", "LIGHTS"], "other settings leave the mapping alone");
    s.execute("sheetset.set", &json!({"set": "Electrical", "blocks": null})).unwrap();
    assert!(strings(&set(&mut s, "Electrical")["blocks"]).is_empty(), "null clears");
    let too_many: Vec<String> = (0..65).map(|i| format!("L{i}*")).collect();
    for bad in [
        json!({"set": "Electrical", "layers": too_many}),
        json!({"set": "Electrical", "layers": "bad\u{7}pattern"}),
        json!({"set": "Electrical", "layers": "x".repeat(65)}),
        json!({"set": "Electrical", "blocks": [1, 2]}),
        json!({"set": "Electrical", "blocks": {"a": 1}}),
    ] {
        assert!(s.execute("sheetset.set", &bad).is_err(), "{bad}");
    }
    assert_eq!(strings(&set(&mut s, "Electrical")["layers"]), ["E-*", "LIGHTS"], "a refused edit changes nothing");
    // A new sheet set can carry a mapping from the start.
    s.execute("sheetset.new", &json!({"name": "Landscape", "layers": "S-*", "blocks": "TREE*"})).unwrap();
    assert_eq!(strings(&set(&mut s, "Landscape")["layers"]), ["S-*"]);
}

#[test]
fn the_layer_list_for_a_tab_follows_its_mapping() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({})).unwrap();
    s.execute("sheetset.activate", &json!({"set": "Plumbing"})).unwrap();
    let r = s.execute("layer", &json!({"showFor": "sheetSet"})).unwrap();
    let names: Vec<&str> = r["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"P-FIXT") && names.contains(&"P-WATR"), "{names:?}");
    assert!(names.iter().all(|n| n.starts_with("P-")), "only plumbing layers: {names:?}");
    s.execute("sheetset.set", &json!({"set": "Plumbing", "layers": "P-*, E-POINT"})).unwrap();
    let r = s.execute("layer", &json!({"showFor": "sheetSet"})).unwrap();
    assert!(r["layers"].as_array().unwrap().iter().any(|l| l["name"] == "E-POINT"), "a mapped layer of another discipline shows too");
}

#[test]
fn entering_a_tab_makes_one_of_its_layers_current_and_keeps_new_objects_on_it() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CLAYER", ""), "0");
    s.execute("sheetset.activate", &json!({"set": "Electrical"})).unwrap();
    let current = s.doc().unwrap().header.str("CLAYER", "");
    assert_eq!(current, "E-CEIL", "the first electrical layer in the table");
    s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let layers: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.common.layer.clone()).collect();
    assert_eq!(layers, ["E-CEIL"], "a mapped layer is not rerouted to E-ANNO");
    // A current layer the tab already maps stays; leaving the tab leaves the layer.
    s.execute("layer.current", &json!({"name": "E-WIRE"})).unwrap();
    s.execute("sheetset.activate", &json!({"set": "Plumbing"})).unwrap();
    s.execute("sheetset.activate", &json!({"set": "Electrical"})).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CLAYER", ""), "E-CEIL", "Plumbing made P-FIXT current, so Electrical picks its first layer");
    s.execute("layer.current", &json!({"name": "E-WIRE"})).unwrap();
    s.execute("sheetset.activate", &json!({"set": null})).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CLAYER", ""), "E-WIRE", "Design leaves the current layer alone");
    // A tab with no usable layer leaves the current one.
    s.execute("sheetset.new", &json!({"name": "Landscape", "layerPrefix": "", "layers": "NOSUCH*"})).unwrap();
    s.execute("sheetset.activate", &json!({"set": "Landscape"})).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CLAYER", ""), "E-WIRE");
}

#[test]
fn the_layer_scope_is_kept_in_the_drawing_and_reported() {
    let mut s = Session::new();
    assert_eq!(s.execute("layer.scope", &json!({})).unwrap()["mode"], "tab");
    s.execute("layer.scope", &json!({"mode": "all"})).unwrap();
    assert_eq!(s.execute("layer.scope", &json!({})).unwrap()["mode"], "all");
    assert!(aad_engine::cmd::sheetstd::show_all(s.doc().unwrap()));
    assert!(s.execute("layer.scope", &json!({"mode": "some"})).is_err());
    assert!(s.execute("layer.scope", &json!({"mode": 3})).is_err());
    let bytes = aad_io::write(s.doc().unwrap(), "a.dxf").unwrap();
    let d = aad_io::read(&bytes, "a.dxf").unwrap();
    assert!(aad_engine::cmd::sheetstd::show_all(&d), "the choice survives a save");
    s.execute("layer.scope", &json!({"mode": "level"})).unwrap();
    assert_eq!(s.execute("layer.scope", &json!({})).unwrap()["mode"], "level");
    assert!(aad_engine::cmd::sheetstd::show_level_only(s.doc().unwrap()) && !aad_engine::cmd::sheetstd::show_all(s.doc().unwrap()));
    s.execute("layer.scope", &json!({"mode": "tab"})).unwrap();
    assert!(!aad_engine::cmd::sheetstd::show_all(s.doc().unwrap()));
    assert!(!aad_engine::cmd::sheetstd::show_level_only(s.doc().unwrap()));
}

#[test]
fn the_mapping_survives_a_file_round_trip_and_damaged_mappings_open_as_plain_sets() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({})).unwrap();
    s.execute("sheetset.set", &json!({"set": "Interior", "layers": "I-*, MY-LAYER", "blocks": "FURN-*, SOFA*"})).unwrap();
    s.execute("sheetset.activate", &json!({"set": "Electrical"})).unwrap();
    let bytes = aad_io::write(s.doc().unwrap(), "a.dxf").unwrap();
    let d = aad_io::read(&bytes, "a.dxf").unwrap();
    let interior = d.sheet_set_by_name("Interior").unwrap();
    assert_eq!(interior.layers, ["I-*", "MY-LAYER"]);
    assert_eq!(interior.blocks, ["FURN-*", "SOFA*"]);
    assert!(d.sheet_set_by_name("Electrical").unwrap().maps_layer("e-point"));
    assert!(d.block("E-SW1").is_some(), "the symbols are ordinary blocks and travel with the file");
    // An older file (no mapping fields) opens with empty mappings; the prefix still maps.
    let mut old = d.clone();
    for set in &mut old.sheet_sets {
        set.layers.clear();
        set.blocks.clear();
    }
    let reread = aad_io::read(&aad_io::write(&old, "a.dxf").unwrap(), "a.dxf").unwrap();
    assert!(reread.sheet_set_by_name("Electrical").unwrap().layers.is_empty());
    assert!(reread.sheet_set_by_name("Electrical").unwrap().maps_layer("E-ANYTHING"));
}

#[test]
fn the_standard_symbols_insert_and_the_new_drawing_flow_includes_them() {
    let mut s = Session::new();
    s.execute("level.setup", &json!({"floors": ["Ground", "First"]})).unwrap();
    assert_eq!(sets(&mut s).len(), 5);
    assert!(s.doc().unwrap().block("P-WC").is_some() && s.doc().unwrap().layer("E-POINT").is_some(), "level.setup applies the standard");
    s.execute("sheetset.activate", &json!({"set": "Electrical"})).unwrap();
    s.execute("insert", &json!({"name": "E-SW1", "at": [1000.0, 1000.0]})).unwrap();
    let layer = s.doc().unwrap().model.iter().next().map(|e| e.common.layer.clone()).unwrap();
    assert_eq!(layer, "E-CEIL", "an insert lands on the tab's current layer");
}

#[test]
fn hostile_mapping_parameters_never_panic() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({})).unwrap();
    for params in [
        json!({"set": "Electrical", "layers": "*".repeat(64)}),
        json!({"set": "Electrical", "layers": vec!["*"; 64]}),
        json!({"set": "Electrical", "layers": [null]}),
        json!({"set": "Electrical", "layers": 1e308}),
        json!({"set": "Electrical", "blocks": ""}),
        json!({"set": "Electrical", "blocks": ",,,"}),
    ] {
        let _ = s.execute("sheetset.set", &params);
    }
    // A pattern built to make a naive matcher backtrack for ever.
    let nasty = "a*".repeat(30) + "b";
    s.execute("sheetset.set", &json!({"set": "Electrical", "layers": nasty})).unwrap();
    s.execute("layer.new", &json!({"name": "a".repeat(200)})).unwrap();
    s.execute("sheetset.activate", &json!({"set": "Electrical"})).unwrap();
    let _ = s.execute("layer", &json!({"showFor": "sheetSet"})).unwrap();
}

#[test]
fn smart_blocks_belong_to_the_tab_whose_layers_they_use() {
    let mut s = Session::new();
    s.execute("sheetset.standard", &json!({})).unwrap();
    let ids = |s: &mut Session, tab: &str| -> Vec<String> {
        s.execute("smartblock.list", &json!({"sheetSet": tab})).unwrap()["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].as_str().unwrap().to_string())
            .collect()
    };
    let all = s.execute("smartblock.list", &json!({})).unwrap()["blocks"].as_array().unwrap().len();
    let mut seen = std::collections::BTreeSet::new();
    for (tab, prefixes) in [
        ("Electrical", &["elec."][..]),
        ("Plumbing", &["plumb."]),
        ("Interior", &["int."]),
        // Ducts and diffusers, and the electrical fittings that hang from the ceiling (layer E-CEIL).
        ("False ceiling", &["hvac.", "elec."]),
        ("Working drawing", &["struct.", "rcc."]),
    ] {
        let list = ids(&mut s, tab);
        assert!(!list.is_empty() && list.iter().all(|id| prefixes.iter().any(|p| id.starts_with(p))), "{tab}: {list:?}");
        seen.extend(list.iter().cloned());
        // Whatever is inserted from a tab's list is a block that tab shows in the Blocks panel.
        for id in &list {
            let r = s.execute("smartblock.insert", &json!({"id": id, "at": [0.0, 0.0]})).unwrap();
            let block = r["block"].as_str().unwrap().to_string();
            assert!(s.doc().unwrap().sheet_set_by_name(tab).unwrap().maps_block(&block), "{tab} does not map {block} ({id})");
        }
    }
    assert_eq!(seen.len(), all, "every smart block belongs to at least one standard tab");
    let ceiling = ids(&mut s, "False ceiling");
    assert_eq!(ceiling.iter().filter(|id| id.starts_with("elec.")).count(), 3, "{ceiling:?}");
    // The tab the user is on decides `sheetSet: true`; a missing or odd value is refused.
    assert!(s.execute("smartblock.list", &json!({"sheetSet": true})).is_err(), "no tab is active");
    s.execute("sheetset.activate", &json!({"set": "Plumbing"})).unwrap();
    assert_eq!(s.execute("smartblock.list", &json!({"sheetSet": true})).unwrap()["blocks"].as_array().unwrap().len(), ids(&mut s, "Plumbing").len());
    assert!(s.execute("smartblock.list", &json!({"sheetSet": "Landscape"})).is_err());
    assert!(s.execute("smartblock.list", &json!({"sheetSet": 5})).is_err());
    // Its blocks land on a layer the tab maps, so they are not moved to the annotation layer.
    s.execute("smartblock.insert", &json!({"id": "plumb.basin", "at": [10.0, 10.0]})).unwrap();
    // The new label (the block is numbered when it is inserted) goes on the annotation layer, so look at the block itself.
    let block =
        s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, aad_engine::doc::EntityKind::Insert(_))).last().map(|e| e.common.layer.clone());
    assert_eq!(block.as_deref(), Some("P-FIXT"));
}

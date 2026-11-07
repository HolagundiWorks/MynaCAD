//! Plumbing layout: block mapping, IDs, pipe networks, the schedules and the take-off record.

use aad_engine::Session;
use aad_engine::doc::EntityKind;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s
}

fn place(s: &mut Session, id: &str, x: i64, y: i64) {
    s.execute("smartblock.insert", &json!({"id": id, "at": [x, y]})).unwrap();
}

fn pipe(s: &mut Session, service: &str, pts: &[[i64; 2]]) -> Value {
    s.execute("plumbing.pipe", &json!({"service": service, "points": pts})).unwrap()
}

fn texts(s: &Session, layer: &str) -> Vec<String> {
    let mut v: Vec<String> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| e.common.layer == layer)
        .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None })
        .collect();
    v.sort();
    v
}

/// A tank feeding a closet and a basin, a chamber draining the closet only.
fn bathroom() -> Session {
    let mut s = session();
    place(&mut s, "plumb.tank.round", 0, 0);
    place(&mut s, "plumb.wc.european", 3000, 0);
    place(&mut s, "plumb.basin", 6000, 0);
    place(&mut s, "plumb.chamber", 3000, -4000);
    pipe(&mut s, "supply", &[[0, 0], [3000, 0]]);
    pipe(&mut s, "supply", &[[3000, 0], [6000, 0]]);
    pipe(&mut s, "drain", &[[3000, 0], [3000, -4000]]);
    s
}

#[test]
fn toolbar_symbols_are_recognised_by_id_and_are_not_taken_for_other_disciplines() {
    let mut s = session();
    place(&mut s, "plumb.wc.european", 0, 0);
    place(&mut s, "plumb.wc.indian", 1000, 0);
    place(&mut s, "plumb.basin", 2000, 0);
    place(&mut s, "plumb.riser", 3000, 0);
    // An electrical symbol and a structural one are named `SB_...` too and must not be seen here.
    place(&mut s, "elec.socket", 4000, 0);
    place(&mut s, "rcc.column.rect", 5000, 0);
    let r = s.execute("plumbing.blocks", &json!({})).unwrap();
    assert_eq!(r["found"], json!({"WC": 1, "IWC": 1, "WB": 1, "RS": 1}), "{r}");
    // ...and the electrical side sees only its own symbol, not the plumbing ones as switchboards.
    let e = s.execute("electrical.blocks", &json!({})).unwrap();
    assert_eq!(e["found"], json!({"P5": 1}), "{e}");
    assert!(s.execute("plumbing.blocks", &json!({"blocks": {"XX": ["A"]}})).is_err());
    let both = s.execute("plumbing.blocks", &json!({"blocks": {"WC": ["WCX"], "WB": ["WCX"]}})).unwrap_err().to_string();
    assert!(both.contains("both"), "{both}");
}

#[test]
fn blocks_made_by_hand_are_mapped_by_name() {
    let mut s = session();
    for name in ["WB", "FT"] {
        s.execute("rectang", &json!({"p1": [0, 0], "p2": [300, 300]})).unwrap();
        let h = s.doc().unwrap().model.iter().last().unwrap().handle.hex();
        s.execute("block", &json!({"name": name, "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
    }
    s.execute("insert", &json!({"name": "WB", "at": [0, 0]})).unwrap();
    s.execute("insert", &json!({"name": "FT", "at": [1000, 0]})).unwrap();
    let r = s.execute("plumbing.blocks", &json!({})).unwrap();
    assert_eq!(r["found"], json!({"WB": 1, "FT": 1}), "{r}");
}

#[test]
fn placed_symbols_are_numbered_left_to_right_and_ids_are_stable() {
    let mut s = session();
    place(&mut s, "plumb.wc.european", 4000, 0);
    place(&mut s, "plumb.wc.european", 2000, 0);
    place(&mut s, "plumb.tank.round", 0, 0);
    assert_eq!(texts(&s, "PL-LABELS"), ["TK-01", "WC-01", "WC-02"], "numbered on insert; the closet at x=2000 is WC-01");
    place(&mut s, "plumb.wc.european", 1000, 0);
    assert_eq!(texts(&s, "PL-LABELS"), ["TK-01", "WC-01", "WC-02", "WC-03"], "a new block takes the next number, old IDs stay");
    let r = s.execute("plumbing.number", &json!({})).unwrap();
    assert_eq!(r["numbered"], 0);
    let r = s.execute("plumbing.number", &json!({"renumber": true})).unwrap();
    assert_eq!(r["numbered"], 4);
    assert!(s.execute("plumbing.number", &json!({"codes": ["ZZ"]})).is_err());
    s.execute("plumbing.autonumber", &json!({"enabled": false})).unwrap();
    place(&mut s, "plumb.basin", 9000, 0);
    assert_eq!(texts(&s, "PL-LABELS").len(), 4, "autonumber off: the basin is left until numbered");
}

#[test]
fn supply_and_drain_are_separate_networks_and_flag_what_is_not_connected() {
    let mut s = bathroom();
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    let svc = |code: &str| r["services"].as_array().unwrap().iter().find(|x| x["service"] == code).unwrap().clone();
    let sup = svc("SUP");
    let tank = sup["sources"].as_array().unwrap().iter().find(|x| x["id"] == "TK-01").expect("tank is a supply source");
    assert_eq!(tank["fixtures"], json!(["WC-01", "WB-01"]), "the run carries on through a T to the basin: {r}");
    assert_eq!(sup["unconnected"], json!([]));
    let drn = svc("DRN");
    assert_eq!(drn["unconnected"], json!(["WB-01"]), "the basin has supply but no drain: {r}");
    let ic = drn["sources"].as_array().unwrap().iter().find(|x| x["id"] == "IC-01").unwrap();
    assert_eq!(ic["fixtures"], json!(["WC-01"]));
    // A fixture is a leaf: the supply run does not carry on through the closet into its drain pipe.
    assert!(!tank["fixtures"].as_array().unwrap().iter().any(|f| f == "IC-01"));
    assert_eq!(texts(&s, "PL-FLAGS"), ["NO DRAIN"], "one flag, beside the basin");
    // The piping is checked again after a fix: connect the basin to the chamber.
    pipe(&mut s, "drain", &[[6000, 0], [6000, -4000], [3000, -4000]]);
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    assert_eq!(r["services"][1]["unconnected"], json!([]), "{r}");
    assert!(texts(&s, "PL-FLAGS").is_empty(), "the flags are replaced");
}

#[test]
fn a_fixture_reaching_two_sources_of_one_service_is_ambiguous() {
    let mut s = bathroom();
    place(&mut s, "plumb.riser", 6000, 3000);
    pipe(&mut s, "supply", &[[6000, 0], [6000, 3000]]);
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    let sup = r["services"].as_array().unwrap().iter().find(|x| x["service"] == "SUP").unwrap();
    // The riser joins the same connected run as the tank, so every fixture on that run has two sources.
    assert_eq!(sup["ambiguous"], json!(["WC-01", "WB-01"]), "{r}");
    assert_eq!(texts(&s, "PL-FLAGS"), ["ON 2 SUP SOURCES", "ON 2 SUP SOURCES, NO DRAIN"]);
}

#[test]
fn a_source_that_serves_nothing_is_reported() {
    let mut s = bathroom();
    place(&mut s, "plumb.tank.round", 0, 9000);
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    assert_eq!(r["emptySources"], json!(["TK-02"]), "{r}");
}

#[test]
fn pipes_are_drawn_on_the_service_layer_with_a_diameter_and_layers_can_be_remapped() {
    let mut s = session();
    let r = s.execute("plumbing.pipe", &json!({"points": [[0, 0], [2000, 0], [2000, 1500]], "dia": 25})).unwrap();
    assert_eq!(r["layer"], "P-WATR");
    assert_eq!(r["dia"], 25.0);
    assert!((r["length"].as_f64().unwrap() - 3.5).abs() < 1e-9, "metres: {r}");
    let h = r["handle"].as_str().unwrap();
    let e = s.doc().unwrap().entity(aad_engine::doc::Handle::parse_hex(h).unwrap()).unwrap();
    assert_eq!(e.common.layer, "P-WATR");
    let d = s.execute("plumbing.pipe", &json!({"service": "drain", "points": [[0, 0], [0, 3000]]})).unwrap();
    assert_eq!((d["layer"].as_str().unwrap(), d["dia"].as_f64().unwrap()), ("P-DRAN", 100.0));
    for bad in [json!({"points": [[0, 0]]}), json!({"points": [[0, 0], [1, 1]], "dia": 0}), json!({"points": [[0, 0], [1, 1]], "service": "gas"})] {
        assert!(s.execute("plumbing.pipe", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("plumbing.layers", &json!({"supply": []})).is_err());
    assert_eq!(s.execute("plumbing.layers", &json!({"supply": ["P-CW"]})).unwrap()["supply"], json!(["P-CW"]));
    let r = s.execute("plumbing.pipe", &json!({"points": [[0, 0], [100, 0]]})).unwrap();
    assert_eq!(r["layer"], "P-CW");
}

#[test]
fn the_three_schedules_count_fixtures_list_them_and_total_the_pipe() {
    let mut s = bathroom();
    let r = s.execute("plumbing.schedule", &json!({})).unwrap();
    assert_eq!(r["header"], json!(["Source", "Service", "WC", "WB"]), "{r}");
    let rows = r["rows"].as_array().unwrap();
    let row = |first: &str, svc: &str| -> Vec<String> {
        let x = rows.iter().find(|x| x[0] == first && x[1] == svc).unwrap_or_else(|| panic!("no {first} {svc} in {r}"));
        x.as_array().unwrap().iter().skip(2).map(|c| c.as_str().unwrap().to_string()).collect()
    };
    assert_eq!(row("TK-01", "SUP"), ["1", "1"]);
    assert_eq!(row("IC-01", "DRN"), ["1", "0"]);
    assert_eq!(row("NOT CONNECTED", "DRN"), ["0", "1"]);
    assert_eq!(rows.last().unwrap()[0], "TOTAL");
    let total: Vec<&str> = rows.last().unwrap().as_array().unwrap().iter().skip(2).map(|c| c.as_str().unwrap()).collect();
    assert_eq!(total, ["1", "1"], "every fixture placed, once");
    assert_eq!(texts(&s, "PL-TABLE").iter().filter(|t| *t == "PLUMBING SCHEDULE").count(), 1);

    let r = s.execute("plumbing.schedule", &json!({"mode": "fixture"})).unwrap();
    assert_eq!(r["header"], json!(["Fixture", "Kind", "Supply from", "Drain to"]));
    assert_eq!(r["rows"], json!([["WC-01", "Water closet (European)", "TK-01", "IC-01"], ["WB-01", "Wash basin", "TK-01", "NOT CONNECTED"]]), "{r}");

    let r = s.execute("plumbing.schedule", &json!({"mode": "pipes"})).unwrap();
    assert_eq!(r["rows"], json!([["Water supply", "20", "6.00"], ["Drainage", "100", "4.00"], ["TOTAL", "", "10.00"]]), "{r}");
    assert_eq!(
        texts(&s, "PL-TABLE").iter().filter(|t| *t == "PLUMBING SCHEDULE").count(),
        0,
        "one schedule is kept: the last drawn replaces the first"
    );
    assert!(s.execute("plumbing.schedule", &json!({"mode": "nope"})).is_err());
}

#[test]
fn update_numbers_checks_and_redraws_the_schedule() {
    let mut s = bathroom();
    s.execute("plumbing.schedule", &json!({"mode": "fixture", "at": [20000, 0]})).unwrap();
    place(&mut s, "plumb.shower", 8000, 0);
    let r = s.execute("plumbing.update", &json!({})).unwrap();
    assert_eq!(r["numbered"], 0, "inserting numbered it");
    let sh = texts(&s, "PL-TABLE");
    assert_eq!(sh.iter().filter(|t| *t == "PLUMBING FIXTURES").count(), 1, "one schedule, redrawn: {sh:?}");
    assert!(sh.contains(&"SH-01".to_string()), "the new shower is in the redrawn schedule: {sh:?}");
    assert!(texts(&s, "PL-FLAGS").iter().any(|t| t.contains("NO SUPPLY")), "the shower has no pipes yet");
}

#[test]
fn connect_persists_the_tables_and_the_takeoff_record_carries_them() {
    let mut s = bathroom();
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    let stacks = r["stacks"].as_array().unwrap();
    assert_eq!(stacks.len(), 2, "the tank on supply and the chamber on drain: {r}");
    for col in ["ref", "handle", "source", "kind", "service", "european_wc", "indian_wc", "basin", "sink", "shower", "floor_trap", "pipe_m"] {
        assert!(stacks[0].get(col).is_some(), "missing {col} in {}", stacks[0]);
    }
    let tank = stacks.iter().find(|x| x["source"] == "TK-01").unwrap();
    assert_eq!((tank["service"].as_str().unwrap(), tank["european_wc"].as_u64().unwrap(), tank["basin"].as_u64().unwrap()), ("SUP", 1, 1));
    assert!((tank["pipe_m"].as_f64().unwrap() - 6.0).abs() < 1e-9, "{tank}");
    let fixtures = r["fixtures"].as_array().unwrap();
    assert_eq!(fixtures.len(), 2);
    let wb = fixtures.iter().find(|x| x["fixture"] == "WB-01").unwrap();
    assert_eq!((wb["supply"].as_str().unwrap(), wb["drain"].as_str().unwrap()), ("TK-01", ""));

    let kept: Value = s.doc().unwrap().project.get("plumbing").map(|t| serde_json::from_str(t).unwrap()).unwrap();
    assert_eq!(kept["stacks"].as_array().unwrap().len(), stacks.len());
    assert_eq!(kept["fixtures"].as_array().unwrap().len(), 2);

    let path = std::env::temp_dir().join(format!("aad-plumb-takeoff-{}.osp", std::process::id()));
    let rec = s.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false})).unwrap();
    assert_eq!(rec["counts"]["plumbing"], 2, "{rec}");
    assert_eq!(rec["counts"]["plumbing_fixtures"], 2, "{rec}");
    let kinds: Vec<u64> = aad_odp::Odp::open(&path).unwrap().takeoff_as_records().unwrap().iter().map(|(id, _)| id >> 56).collect();
    assert_eq!(kinds.iter().filter(|k| **k == 12).count(), 2, "kind 12 source rows: {kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == 13).count(), 2, "kind 13 fixture rows: {kinds:?}");
    let _ = std::fs::remove_file(&path);
}

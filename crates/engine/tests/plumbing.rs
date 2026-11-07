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
    let sup = svc("CW");
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
    assert_eq!(r["services"][2]["unconnected"], json!([]), "{r}");
    assert!(texts(&s, "PL-FLAGS").is_empty(), "the flags are replaced");
}

#[test]
fn a_fixture_reaching_two_sources_of_one_service_is_ambiguous() {
    let mut s = bathroom();
    place(&mut s, "plumb.riser", 6000, 3000);
    pipe(&mut s, "supply", &[[6000, 0], [6000, 3000]]);
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    let sup = r["services"].as_array().unwrap().iter().find(|x| x["service"] == "CW").unwrap();
    // The riser joins the same connected run as the tank, so every fixture on that run has two sources.
    assert_eq!(sup["ambiguous"], json!(["WC-01", "WB-01"]), "{r}");
    assert_eq!(texts(&s, "PL-FLAGS"), ["ON 2 CW SOURCES", "ON 2 CW SOURCES, NO DRAIN"]);
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
    assert_eq!(s.execute("plumbing.layers", &json!({"cold": ["P-CW"]})).unwrap()["cold"], json!(["P-CW"]));
    assert_eq!(
        s.execute("plumbing.layers", &json!({"supply": ["P-COLD"]})).unwrap()["cold"],
        json!(["P-COLD"]),
        "`supply` is the old name of `cold`"
    );
    s.execute("plumbing.layers", &json!({"cold": ["P-CW"]})).unwrap();
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
    assert_eq!(row("TK-01", "CW"), ["1", "1"]);
    assert_eq!(row("IC-01", "DRN"), ["1", "0"]);
    assert_eq!(row("NOT CONNECTED", "DRN"), ["0", "1"]);
    assert_eq!(rows.last().unwrap()[0], "TOTAL");
    let total: Vec<&str> = rows.last().unwrap().as_array().unwrap().iter().skip(2).map(|c| c.as_str().unwrap()).collect();
    assert_eq!(total, ["1", "1"], "every fixture placed, once");
    assert_eq!(texts(&s, "PL-TABLE").iter().filter(|t| *t == "PLUMBING SCHEDULE").count(), 1);

    let r = s.execute("plumbing.schedule", &json!({"mode": "fixture"})).unwrap();
    assert_eq!(r["header"], json!(["Fixture", "Kind", "Cold from", "Hot from", "Drain to"]));
    assert_eq!(
        r["rows"],
        json!([["WC-01", "Water closet (European)", "TK-01", "-", "IC-01"], ["WB-01", "Wash basin", "TK-01", "-", "NOT CONNECTED"]]),
        "{r}"
    );

    let r = s.execute("plumbing.schedule", &json!({"mode": "pipes"})).unwrap();
    assert_eq!(r["rows"], json!([["Cold water", "20", "6.00"], ["Drainage", "100", "4.00"], ["TOTAL", "", "10.00"]]), "{r}");
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
    assert!(texts(&s, "PL-FLAGS").iter().any(|t| t.contains("NO COLD WATER")), "the shower has no pipes yet");
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
    assert_eq!((tank["service"].as_str().unwrap(), tank["european_wc"].as_u64().unwrap(), tank["basin"].as_u64().unwrap()), ("CW", 1, 1));
    assert!((tank["pipe_m"].as_f64().unwrap() - 6.0).abs() < 1e-9, "{tank}");
    let fixtures = r["fixtures"].as_array().unwrap();
    assert_eq!(fixtures.len(), 2);
    let wb = fixtures.iter().find(|x| x["fixture"] == "WB-01").unwrap();
    assert_eq!((wb["cold"].as_str().unwrap(), wb["hot"].as_str().unwrap(), wb["drain"].as_str().unwrap()), ("TK-01", "", ""));

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

#[test]
fn cold_hot_and_drain_pipes_have_their_own_layer_colour_linetype_and_width() {
    let mut s = session();
    let layer =
        |s: &Session, name: &str| s.doc().unwrap().layer(name).map(|l| (l.color, l.linetype.clone())).unwrap_or_else(|| panic!("no layer {name}"));
    let cold = s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [3000, 0]]})).unwrap();
    let hot = s.execute("plumbing.pipe.hot", &json!({"points": [[0, 500], [3000, 500]]})).unwrap();
    let drain = s.execute("plumbing.pipe.drain", &json!({"points": [[0, 1000], [3000, 1000]]})).unwrap();
    assert_eq!([&cold["layer"], &hot["layer"], &drain["layer"]], ["P-WATR", "P-HOTW", "P-DRAN"]);
    assert_eq!([&cold["service"], &hot["service"], &drain["service"]], ["cold", "hot", "drain"]);
    assert_eq!(
        [cold["dia"].as_f64().unwrap(), hot["dia"].as_f64().unwrap(), drain["dia"].as_f64().unwrap()],
        [20.0, 15.0, 100.0],
        "the default size of each"
    );
    let (c, h, d) = (layer(&s, "P-WATR"), layer(&s, "P-HOTW"), layer(&s, "P-DRAN"));
    assert!(c.0 != h.0 && h.0 != d.0 && c.0 != d.0, "three colours: {c:?} {h:?} {d:?}");
    assert!(h.1.eq_ignore_ascii_case("DASHED") || h.1.eq_ignore_ascii_case("Continuous"), "hot is dashed where the linetype is loaded: {h:?}");
    // The line is as wide as the pipe: a 100 mm drain is five times a 20 mm cold pipe.
    let width = |s: &Session, r: &Value| match &s
        .doc()
        .unwrap()
        .entity(aad_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
        .unwrap()
        .kind
    {
        EntityKind::LwPolyline(p) => p.const_width,
        k => panic!("expected a polyline, got {k:?}"),
    };
    assert_eq!((width(&s, &cold), width(&s, &drain)), (20.0, 100.0));
    // Solid, whatever the layer's linetype: a dash pattern on a band as wide as the pipe looks like a ladder.
    for r in [&cold, &hot, &drain] {
        let e = s.doc().unwrap().entity(aad_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()).unwrap();
        assert!(e.common.linetype.eq_ignore_ascii_case("Continuous"), "{}: {}", r["service"], e.common.linetype);
    }
    // The drain layer is the standard one (green, dashed); the hot layer is added to the standard set.
    assert_eq!((format!("{:?}", d.0), d.1.to_ascii_uppercase()), ("Index(3)".to_string(), "DASHED".to_string()));
    assert_eq!(format!("{:?}", h.0), "Index(1)");
}

#[test]
fn pipe_sizes_can_be_chosen_from_a_form_and_any_other_size_is_checked() {
    let mut s = session();
    // A form's choice arrives as text.
    let r = s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [1000, 0]], "dia": "32"})).unwrap();
    assert_eq!(r["dia"], 32.0);
    assert_eq!(s.execute("plumbing.pipe.hot", &json!({"points": [[0, 0], [1000, 0]], "dia": 25})).unwrap()["dia"], 25.0);
    assert_eq!(s.execute("plumbing.pipe.drain", &json!({"points": [[0, 0], [1000, 0]], "dia": "150"})).unwrap()["dia"], 150.0);
    for bad in [
        json!({"points": [[0, 0], [1, 1]], "dia": "wide"}),
        json!({"points": [[0, 0], [1, 1]], "dia": -5}),
        json!({"points": [[0, 0], [1, 1]], "dia": 5000}),
    ] {
        assert!(s.execute("plumbing.pipe.cold", &bad).is_err(), "{bad}");
    }
    // The generic command takes the service by name, and `supply` still means cold.
    for (service, layer) in [("cold", "P-WATR"), ("supply", "P-WATR"), ("hot", "P-HOTW"), ("drain", "P-DRAN")] {
        let r = s.execute("plumbing.pipe", &json!({"service": service, "points": [[0, 0], [1000, 0]]})).unwrap();
        assert_eq!(r["layer"], layer, "{service}");
    }
    assert!(s.execute("plumbing.pipe", &json!({"service": "steam", "points": [[0, 0], [1, 1]]})).is_err());
    // The schedule totals each service and size on its own row, in metres.
    s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [2000, 0]], "dia": 25})).unwrap();
    let r = s.execute("plumbing.schedule", &json!({"mode": "pipes"})).unwrap();
    let rows: Vec<Vec<String>> =
        r["rows"].as_array().unwrap().iter().map(|x| x.as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect()).collect();
    let find = |svc: &str, dia: &str| rows.iter().find(|x| x[0] == svc && x[1] == dia).map(|x| x[2].clone());
    assert_eq!(find("Cold water", "25").as_deref(), Some("2.00"), "{r}");
    assert_eq!(find("Cold water", "32").as_deref(), Some("1.00"), "{r}");
    assert_eq!(find("Hot water", "25").as_deref(), Some("1.00"), "{r}");
    assert_eq!(find("Drainage", "150").as_deref(), Some("1.00"), "{r}");
}

#[test]
fn hot_water_is_its_own_network_with_a_heater_as_the_source_and_is_never_flagged_when_missing() {
    let mut s = session();
    // A heater is a block made by hand: it has no toolbar symbol, so it is matched by name.
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [400, 400]})).unwrap();
    let h = s.doc().unwrap().model.iter().last().unwrap().handle.hex();
    s.execute("block", &json!({"name": "WH", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "WH", "at": [0, 0]})).unwrap();
    place(&mut s, "plumb.basin", 3000, 0);
    place(&mut s, "plumb.shower", 6000, 0);
    place(&mut s, "plumb.tank.round", 0, 3000);
    pipe(&mut s, "cold", &[[0, 3000], [3000, 0]]);
    pipe(&mut s, "cold", &[[3000, 0], [6000, 0]]);
    s.execute("plumbing.pipe.hot", &json!({"points": [[0, 0], [3000, 0]]})).unwrap();
    let r = s.execute("plumbing.connect", &json!({})).unwrap();
    let svc = |code: &str| r["services"].as_array().unwrap().iter().find(|x| x["service"] == code).unwrap().clone();
    let hot = svc("HW");
    let heater = hot["sources"].as_array().unwrap().iter().find(|x| x["id"] == "WH-01").expect("the heater is a hot water source");
    assert_eq!(heater["fixtures"], json!(["WB-01"]), "hot reaches the basin and not the shower: {r}");
    assert_eq!(hot["unconnected"], json!([]), "hot water is optional, so the shower is not unconnected: {r}");
    assert_eq!(svc("CW")["sources"][0]["fixtures"], json!(["WB-01", "SH-01"]), "{r}");
    let flags = texts(&s, "PL-FLAGS");
    assert!(flags.iter().all(|t| !t.contains("HOT")), "no hot water flag: {flags:?}");
    let f = s.execute("plumbing.schedule", &json!({"mode": "fixture"})).unwrap();
    assert_eq!(f["rows"][1], json!(["SH-01", "Shower", "TK-01", "-", "NOT CONNECTED"]), "{f}");
    assert_eq!(f["rows"][0], json!(["WB-01", "Wash basin", "TK-01", "WH-01", "NOT CONNECTED"]), "{f}");
}

fn on_layer(s: &Session, layer: &str) -> usize {
    s.doc().unwrap().model.iter().filter(|e| e.common.layer == layer).count()
}

/// The fittings as `("Elbow 90", "20")` pairs, from a fresh redraw.
fn fittings(s: &mut Session) -> Vec<(String, String)> {
    let r = s.execute("plumbing.fittings", &json!({})).unwrap();
    let mut v: Vec<(String, String)> = r["fittings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["kind"].as_str().unwrap().to_string(), f["size"].as_str().unwrap().to_string()))
        .collect();
    v.sort();
    v
}

fn one(kind: &str, size: &str) -> Vec<(String, String)> {
    vec![(kind.to_string(), size.to_string())]
}

#[test]
fn a_bend_in_a_run_gets_an_elbow_by_its_angle_and_a_straight_run_gets_none() {
    let mut s = session();
    let r = s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [2000, 0], [2000, 2000]]})).unwrap();
    assert_eq!(r["fittings"], 1, "drawing the pipe adds its fitting: {r}");
    assert_eq!(fittings(&mut s), one("Elbow 90", "20"));
    assert_eq!(on_layer(&s, "P-FITG"), 2, "a ring and a dot");

    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0], [3414, 1414]]);
    assert_eq!(fittings(&mut s), one("Elbow 45", "20"));
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0], [4000, 700]]);
    assert_eq!(fittings(&mut s), one("Bend", "20"), "about 19 degrees is neither");
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0], [4000, 0]]);
    assert!(fittings(&mut s).is_empty(), "a vertex on a straight run is not a bend");
    assert_eq!(on_layer(&s, "P-FITG"), 0);
}

#[test]
fn a_run_ending_on_the_side_of_another_gets_a_tee_with_both_sizes() {
    let mut s = session();
    s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [4000, 0]], "dia": 25})).unwrap();
    let r = s.execute("plumbing.pipe.cold", &json!({"points": [[2000, 0], [2000, 2000]], "dia": 20})).unwrap();
    assert_eq!(r["fittings"], 1, "{r}");
    assert_eq!(fittings(&mut s), one("Tee", "25x20"));
    assert_eq!(on_layer(&s, "P-FITG"), 2, "a ring and the stem of the branch");
    // Same size on both: one number.
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [4000, 0]]);
    pipe(&mut s, "cold", &[[2000, 0], [2000, 2000]]);
    assert_eq!(fittings(&mut s), one("Tee", "20"));
    // Missing the side of the other run by more than the snap distance: no tee.
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [4000, 0]]);
    pipe(&mut s, "cold", &[[2000, 100], [2000, 2000]]);
    assert!(fittings(&mut s).is_empty());
}

#[test]
fn runs_meeting_end_to_end_get_an_elbow_a_reducer_or_nothing() {
    // Straight on, same size: nothing. A bigger run: a reducer. A turn: an elbow.
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0]]);
    pipe(&mut s, "cold", &[[2000, 0], [4000, 0]]);
    assert!(fittings(&mut s).is_empty(), "a joint between runs of one size in line needs no fitting");
    s.execute("plumbing.pipe.cold", &json!({"points": [[4000, 0], [6000, 0]], "dia": 32})).unwrap();
    assert_eq!(fittings(&mut s), one("Reducer", "32x20"));
    assert_eq!(on_layer(&s, "P-FITG"), 2, "a reducer is two rings, and the redraw replaced rather than stacked them");
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0]]);
    pipe(&mut s, "cold", &[[2000, 0], [2000, 2000]]);
    assert_eq!(fittings(&mut s), one("Elbow 90", "20"));
}

#[test]
fn pipes_of_different_services_never_make_a_fitting_together() {
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [4000, 0]]);
    s.execute("plumbing.pipe.hot", &json!({"points": [[2000, 0], [2000, 2000]]})).unwrap();
    s.execute("plumbing.pipe.drain", &json!({"points": [[0, 0], [0, 2000]]})).unwrap();
    assert!(fittings(&mut s).is_empty());
    // A hot run with its own bend gets an elbow of its own size.
    s.execute("plumbing.pipe.hot", &json!({"points": [[0, 3000], [2000, 3000], [2000, 5000]], "dia": 25})).unwrap();
    assert_eq!(fittings(&mut s), one("Elbow 90", "25"));
}

#[test]
fn fittings_are_redrawn_not_stacked_and_follow_the_pipes() {
    let mut s = session();
    let r = s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [2000, 0], [2000, 2000]]})).unwrap();
    assert_eq!(on_layer(&s, "P-FITG"), 2);
    fittings(&mut s);
    fittings(&mut s);
    assert_eq!(on_layer(&s, "P-FITG"), 2, "redrawing twice does not duplicate them");
    // Another bend adds its own.
    pipe(&mut s, "drain", &[[0, 5000], [3000, 5000], [3000, 7000]]);
    assert_eq!(on_layer(&s, "P-FITG"), 4);
    // Erasing a pipe and redrawing removes its fittings.
    s.execute("erase", &json!({"handles": [r["handle"]]})).unwrap();
    assert_eq!(fittings(&mut s), one("Elbow 90", "100"));
    assert_eq!(on_layer(&s, "P-FITG"), 2);
}

#[test]
fn auto_add_can_be_turned_off_and_fittings_still_come_on_request() {
    let mut s = session();
    assert_eq!(s.execute("plumbing.autofit", &json!({})).unwrap()["enabled"], true, "on by default");
    assert_eq!(s.execute("plumbing.autofit", &json!({"enabled": false})).unwrap()["enabled"], false);
    let r = s.execute("plumbing.pipe.cold", &json!({"points": [[0, 0], [2000, 0], [2000, 2000]]})).unwrap();
    assert_eq!(r["fittings"], 0);
    assert_eq!(on_layer(&s, "P-FITG"), 0, "nothing is added while it is off");
    assert_eq!(fittings(&mut s), one("Elbow 90", "20"), "the command still adds them");
    assert_eq!(on_layer(&s, "P-FITG"), 2);
    s.execute("plumbing.autofit", &json!({"enabled": true})).unwrap();
    pipe(&mut s, "drain", &[[0, 5000], [3000, 5000], [3000, 7000]]);
    assert_eq!(on_layer(&s, "P-FITG"), 4, "on again: drawing adds them");
}

#[test]
fn the_fittings_schedule_counts_each_type_and_size() {
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0], [2000, 2000], [4000, 2000]]);
    s.execute("plumbing.pipe.cold", &json!({"points": [[1000, 0], [1000, -2000]], "dia": 15})).unwrap();
    pipe(&mut s, "drain", &[[0, 8000], [3000, 8000], [3000, 10000]]);
    let r = s.execute("plumbing.schedule", &json!({"mode": "fittings"})).unwrap();
    assert_eq!(r["header"], json!(["Fitting", "Size (mm)", "Service", "Count"]));
    assert_eq!(
        r["rows"],
        json!([
            ["Elbow 90", "20", "Cold water", "2"],
            ["Tee", "20x15", "Cold water", "1"],
            ["Elbow 90", "100", "Drainage", "1"],
            ["TOTAL", "", "", "4"]
        ]),
        "{r}"
    );
    assert_eq!(texts(&s, "PL-TABLE").iter().filter(|t| *t == "PLUMBING FITTINGS").count(), 1);
}

#[test]
fn update_redraws_the_fittings_too() {
    let mut s = session();
    pipe(&mut s, "cold", &[[0, 0], [2000, 0], [2000, 2000]]);
    s.execute("plumbing.autofit", &json!({"enabled": false})).unwrap();
    s.execute("erase", &json!({"handles": [s.doc().unwrap().model.iter().find(|e| e.common.layer == "P-WATR").unwrap().handle.hex()]})).unwrap();
    s.execute("plumbing.autofit", &json!({"enabled": true})).unwrap();
    s.execute("plumbing.update", &json!({})).unwrap();
    assert_eq!(on_layer(&s, "P-FITG"), 0, "the pipe is gone, so update clears its fittings");
}

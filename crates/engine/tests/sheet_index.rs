//! Structured sheet set: records, numbering, build from levels, title-block fill, revisions, issue
//! sets, the drawn table, persistence in the project data, and hostile input.

use aad_engine::Session;
use serde_json::{Value, json};

fn numbers(s: &mut Session) -> Vec<String> {
    let r = s.execute("sheet.list", &json!({})).unwrap();
    r["sheets"].as_array().unwrap().iter().map(|x| x["number"].as_str().unwrap().to_string()).collect()
}

fn two_levels() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s
}

#[test]
fn numbering_follows_the_discipline_scheme() {
    let mut s = Session::new();
    s.execute("sheet.new", &json!({"title": "Ground Plan"})).unwrap();
    s.execute("sheet.new", &json!({"title": "First Plan"})).unwrap();
    s.execute("sheet.new", &json!({"title": "Foundation", "discipline": "S"})).unwrap();
    s.execute("sheet.new", &json!({"title": "Power", "discipline": "E"})).unwrap();
    s.execute("sheet.new", &json!({"title": "Drainage", "discipline": "P"})).unwrap();
    assert_eq!(numbers(&mut s), ["A-101", "A-102", "S-201", "E-301", "P-401"]);
    // a custom pattern and width
    s.execute("sheet.scheme", &json!({"pattern": "{D}{N}", "width": 2, "step": 10})).unwrap();
    let r = s.execute("sheet.renumber", &json!({"discipline": "A"})).unwrap();
    assert_eq!(r["changes"][0]["to"], "A101");
    assert_eq!(r["changes"][1]["to"], "A111");
    assert_eq!(numbers(&mut s), ["A101", "A111", "S-201", "E-301", "P-401"]);
}

#[test]
fn renumber_dry_run_changes_nothing_and_clash_is_refused() {
    let mut s = Session::new();
    s.execute("sheet.new", &json!({"title": "One", "number": "A-105"})).unwrap();
    let r = s.execute("sheet.renumber", &json!({"dryRun": true})).unwrap();
    assert_eq!(r["applied"], false);
    assert_eq!(r["changes"][0]["to"], "A-101");
    assert_eq!(numbers(&mut s), ["A-105"]);
    // a void sheet keeps A-101, so A-105 -> A-101 would repeat it: refused, nothing changes.
    s.execute("sheet.new", &json!({"title": "Two", "number": "A-101", "status": "void"})).unwrap();
    assert!(s.execute("sheet.renumber", &json!({})).is_err());
    assert!(s.execute("sheet.renumber", &json!({"dryRun": true})).is_err());
    assert_eq!(numbers(&mut s), ["A-105", "A-101"]);
    assert!(s.execute("sheet.new", &json!({"title": "Dup", "number": "a-105"})).is_err());
}

#[test]
fn move_reorders_within_a_discipline_and_changes_discipline() {
    let mut s = Session::new();
    for t in ["One", "Two", "Three"] {
        s.execute("sheet.new", &json!({"title": t})).unwrap();
    }
    s.execute("sheet.move", &json!({"sheet": "A-103", "position": 0})).unwrap();
    assert_eq!(numbers(&mut s), ["A-103", "A-101", "A-102"]);
    s.execute("sheet.move", &json!({"sheet": "A-103", "after": "A-102", "renumber": true})).unwrap();
    assert_eq!(numbers(&mut s), ["A-101", "A-102", "A-103"]);
    let r = s.execute("sheet.move", &json!({"sheet": "A-101", "discipline": "S"})).unwrap();
    assert_eq!(r["sheet"], "S-201");
    assert!(s.execute("sheet.move", &json!({"sheet": "A-102", "before": "S-201"})).is_err());
    assert!(s.execute("sheet.move", &json!({"sheet": "A-102", "before": "A-103", "after": "A-103"})).is_err());
}

#[test]
fn build_makes_a_layout_per_level_with_a_bound_viewport_and_is_repeatable() {
    let mut s = two_levels();
    let r = s.execute("sheetindex.build", &json!({"scale": "1:50", "paper": "A2"})).unwrap();
    assert_eq!(r["created"].as_array().unwrap().len(), 2);
    assert_eq!(r["created"][0]["number"], "A-101");
    let layout = r["created"][1]["layout"].as_str().unwrap().to_string();
    assert!(layout.starts_with("A-102"), "{layout}");
    let d = s.doc().unwrap();
    assert_eq!(d.layout(&layout).unwrap().page.paper, "A2");
    // the viewport shows the First level
    let vps = s.execute("viewport.list", &json!({"layout": layout})).unwrap();
    assert!(vps.to_string().contains("First"), "{vps}");
    // title block carries number, title, scale and floor
    let f = s.execute("sheet.fields", &json!({"layout": layout})).unwrap();
    assert_eq!(f["fields"]["SHEET"], "A-102");
    assert_eq!(f["fields"]["TITLE"], "First Plan");
    assert_eq!(f["fields"]["SCALE"], "1:50");
    assert_eq!(f["fields"]["FLOOR"], "First");
    // second build creates nothing
    let again = s.execute("sheetindex.build", &json!({})).unwrap();
    assert_eq!(again["created"].as_array().unwrap().len(), 0);
    assert_eq!(again["skipped"].as_array().unwrap().len(), 2);
    assert_eq!(numbers(&mut s), ["A-101", "A-102"]);
}

#[test]
fn build_with_missing_views_still_works() {
    let mut s = two_levels();
    let r = s.execute("sheetindex.build", &json!({"levels": false, "views": ["Section A-A"]})).unwrap();
    assert_eq!(r["created"].as_array().unwrap().len(), 1);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("Section A-A")));
    let l = s.execute("sheet.list", &json!({})).unwrap();
    assert_eq!(l["sheets"][0]["source"]["kind"], "view");
}

#[test]
fn build_from_compose_views_and_master() {
    let mut s = two_levels();
    s.execute("master.new", &json!({"name": "A1 L", "paper": "A1"})).unwrap();
    s.execute("compose.view.save", &json!({"name": "GF Plan", "scale": "1:200"})).unwrap();
    let r = s.execute("sheetindex.build", &json!({"levels": false, "composeViews": true, "master": "A1 L"})).unwrap();
    assert_eq!(r["created"].as_array().unwrap().len(), 1);
    let layout = r["created"][0]["layout"].as_str().unwrap();
    assert!(s.doc().unwrap().layout(layout).unwrap().master.is_some());
    assert!(s.execute("sheetindex.build", &json!({"master": "nope"})).is_err());
}

#[test]
fn set_rewrites_the_title_block_and_revisions_fill_rev() {
    let mut s = two_levels();
    s.execute("sheetindex.build", &json!({})).unwrap();
    s.execute("sheet.set", &json!({"sheet": "A-101", "title": "Ground Floor Plan", "drawn": "VH", "date": "2026-11-11"})).unwrap();
    let layout = s.execute("sheet.list", &json!({})).unwrap()["sheets"][0]["layout"].as_str().unwrap().to_string();
    let f = s.execute("sheet.fields", &json!({"layout": layout})).unwrap();
    assert_eq!(f["fields"]["TITLE"], "Ground Floor Plan");
    assert_eq!(f["fields"]["DRAWN"], "VH");
    assert_eq!(f["fields"]["DATE"], "2026-11-11");
    let r = s.execute("sheet.revision.add", &json!({"sheet": "A-101", "description": "Walls moved", "date": "2026-11-12"})).unwrap();
    assert_eq!(r["rev"], "A");
    let r = s.execute("sheet.revision.add", &json!({"sheet": "A-101", "description": "Door added", "date": "2026-11-13", "by": "VH"})).unwrap();
    assert_eq!(r["rev"], "B");
    assert!(s.execute("sheet.revision.add", &json!({"sheet": "A-101", "description": "x", "rev": "b"})).is_err());
    let f = s.execute("sheet.fields", &json!({"layout": layout})).unwrap();
    assert_eq!(f["fields"]["REV"], "B");
    let l = s.execute("sheet.revision.list", &json!({})).unwrap();
    assert_eq!(l["revisions"][0]["description"], "Door added");
    assert_eq!(l["revisions"].as_array().unwrap().len(), 2);
}

#[test]
fn an_issue_freezes_its_sheets_and_survives_renumbering() {
    let mut s = Session::new();
    for t in ["One", "Two"] {
        s.execute("sheet.new", &json!({"title": t})).unwrap();
    }
    s.execute("sheet.new", &json!({"title": "Frame", "discipline": "S"})).unwrap();
    let r = s.execute("issue.create", &json!({"name": "Permit", "date": "2026-11-20", "discipline": "A"})).unwrap();
    assert_eq!(r["issue"]["rev"], "A");
    assert_eq!(r["issue"]["sheets"].as_array().unwrap().len(), 2);
    let l = s.execute("sheet.list", &json!({})).unwrap();
    assert_eq!(l["sheets"][0]["status"], "issued");
    assert_eq!(l["sheets"][0]["rev"], "A");
    assert_eq!(l["sheets"][2]["status"], "draft"); // structural sheet was not in the issue
    // later edits do not rewrite the frozen row
    s.execute("sheet.set", &json!({"sheet": "A-101", "title": "Renamed", "number": "A-150"})).unwrap();
    let i = s.execute("issue.list", &json!({})).unwrap();
    assert_eq!(i["issues"][0]["sheets"][0]["number"], "A-101");
    assert_eq!(i["issues"][0]["sheets"][0]["title"], "One");
    // names are unique, an empty selection is refused
    assert!(s.execute("issue.create", &json!({"name": "permit"})).is_err());
    assert!(s.execute("issue.create", &json!({"name": "Empty", "discipline": "M"})).is_err());
    // second issue gets the next letter; the first sheet already has rev B? no: A then B
    let r = s.execute("issue.create", &json!({"name": "Tender"})).unwrap();
    assert_eq!(r["issue"]["rev"], "B");
}

#[test]
fn the_table_is_drawn_and_a_redraw_replaces_it() {
    let mut s = two_levels();
    s.execute("sheetindex.build", &json!({})).unwrap();
    s.execute("sheet.new", &json!({"title": "Power", "discipline": "E"})).unwrap();
    let first = s.execute("sheetindex.table", &json!({})).unwrap();
    assert_eq!(first["layout"], "Sheet Index");
    let n = first["objects"].as_u64().unwrap();
    assert!(n > 20, "{n}");
    let count = |s: &Session| s.doc().unwrap().layout("Sheet Index").unwrap().entities.len();
    let before = count(&s);
    let second = s.execute("sheetindex.table", &json!({})).unwrap();
    assert_eq!(second["objects"].as_u64().unwrap(), n);
    assert_eq!(count(&s), before);
    s.execute("sheet.revision.add", &json!({"sheet": "A-101", "description": "R"})).unwrap();
    let rv = s.execute("sheetindex.table", &json!({"kind": "revisions", "layout": "Sheet Index"})).unwrap();
    assert!(rv["objects"].as_u64().unwrap() > 6);
    assert!(s.execute("sheetindex.table", &json!({"kind": "bogus"})).is_err());
    assert!(s.execute("sheetindex.table", &json!({"height": 0.0})).is_err());
    assert!(s.execute("sheetindex.table", &json!({"at": "x"})).is_err());
}

#[test]
fn the_index_is_stored_under_one_new_project_key_and_round_trips() {
    let mut s = two_levels();
    s.execute("sheetindex.build", &json!({})).unwrap();
    s.execute("issue.create", &json!({"name": "Permit"})).unwrap();
    let project = s.doc().unwrap().project.clone();
    assert!(project.contains_key("sheetidx1"));
    // a fresh session given the same project data reads the same index
    let mut t = Session::new();
    t.doc_mut().unwrap().project = project;
    assert_eq!(numbers(&mut t), ["A-101", "A-102"]);
    assert_eq!(t.execute("issue.list", &json!({})).unwrap()["issues"][0]["name"], "Permit");
    // undo removes what the command added
    let mut u = Session::new();
    u.execute("sheet.new", &json!({"title": "X"})).unwrap();
    u.execute("undo", &json!({})).unwrap();
    assert!(!u.doc().unwrap().project.contains_key("sheetidx1"));
}

#[test]
fn damaged_or_oversize_stored_data_is_dropped_not_fatal() {
    let mut s = Session::new();
    for junk in [
        "not json",
        "[1,2,3]",
        "{\"sheets\": 5}",
        "{\"sheets\": [{\"id\": \"x\"}]}",
        "{\"scheme\": {\"pattern\": \"zzz\", \"width\": 99, \"step\": 0, \"disciplines\": []}}",
    ] {
        s.doc_mut().unwrap().project.insert("sheetidx1".into(), junk.into());
        let r = s.execute("sheet.list", &json!({})).unwrap();
        assert!(r["sheets"].as_array().unwrap().is_empty(), "{junk}");
        s.execute("sheet.new", &json!({"title": "ok"})).unwrap();
        s.execute("undo", &json!({})).unwrap();
    }
}

#[test]
fn hostile_parameters_never_panic() {
    let mut s = Session::new();
    s.execute("sheet.new", &json!({"title": "One"})).unwrap();
    let long = "x".repeat(500);
    let bad: Vec<(&str, Value)> = vec![
        ("sheet.new", json!({})),
        ("sheet.new", json!({"title": long})),
        ("sheet.new", json!({"title": "a\u{0007}b"})),
        ("sheet.new", json!({"title": 5.0e300, "discipline": "ZZZZZZ"})),
        ("sheet.new", json!({"title": "t", "discipline": "Q"})),
        ("sheet.new", json!({"title": "t", "number": "a/b"})),
        ("sheet.new", json!({"title": "t", "scale": "1:0"})),
        ("sheet.new", json!({"title": "t", "scale": "1:99999999999"})),
        ("sheet.new", json!({"title": "t", "paper": "A9"})),
        ("sheet.new", json!({"title": "t", "status": "done"})),
        ("sheet.new", json!({"title": "t", "date": "2026-13-45"})),
        ("sheet.new", json!({"title": "t", "date": "yesterday"})),
        ("sheet.new", json!({"title": "t", "layout": "no such layout"})),
        ("sheet.new", json!({"title": "t", "source": {"kind": "level", "level": "nowhere"}})),
        ("sheet.new", json!({"title": "t", "source": {"kind": "weird"}})),
        ("sheet.new", json!({"title": ["x"]})),
        ("sheet.set", json!({})),
        ("sheet.set", json!({"sheet": "nope"})),
        ("sheet.set", json!({"sheet": -1})),
        ("sheet.set", json!({"sheet": 1.5e300})),
        ("sheet.delete", json!({"sheet": null})),
        ("sheet.scheme", json!({"pattern": "{N}{N}"})),
        ("sheet.scheme", json!({"pattern": "{D}"})),
        ("sheet.scheme", json!({"pattern": "{D}/{N}"})),
        ("sheet.scheme", json!({"width": 0})),
        ("sheet.scheme", json!({"width": 1e9})),
        ("sheet.scheme", json!({"step": -3})),
        ("sheet.scheme", json!({"disciplines": []})),
        ("sheet.scheme", json!({"disciplines": [{"code": "X", "start": -1}]})),
        ("sheet.scheme", json!({"disciplines": [{"code": "X", "start": 1}]})), // drops A, which has a sheet
        ("sheet.scheme", json!({"disciplines": [{"code": "A", "start": 1}, {"code": "a", "start": 2}]})),
        ("sheet.move", json!({"sheet": "A-101", "position": -1})),
        ("sheet.move", json!({"sheet": "A-101", "before": "nope"})),
        ("sheet.renumber", json!({"discipline": "ZZ"})),
        ("sheet.revision.add", json!({"sheet": "A-101"})),
        ("sheet.revision.add", json!({"sheet": "A-101", "description": "d", "rev": "123456789"})),
        ("sheet.revision.add", json!({"sheet": "A-101", "description": "d", "date": "13/13/2020"})),
        ("sheet.revision.list", json!({"sheet": "zzz"})),
        ("issue.create", json!({})),
        ("issue.create", json!({"name": long})),
        ("issue.create", json!({"name": "i", "sheets": ["nope"]})),
        ("issue.create", json!({"name": "i", "sheets": "A-101"})),
        ("issue.create", json!({"name": "i", "rev": "123456789"})),
        ("sheetindex.build", json!({"discipline": "ZZ"})),
        ("sheetindex.build", json!({"scale": "banana"})),
        ("sheetindex.build", json!({"views": [1]})),
        ("sheetindex.build", json!({"views": "x"})),
        ("sheetindex.publish", json!({"issue": "none"})),
        ("sheetindex.publish", json!({"discipline": "ZZ"})),
        ("sheetindex.publish", json!({"sheets": [true]})),
        ("sheetindex.publish", json!({})), // sheets exist but have no layout
        ("sheetindex.fill", json!({"sheet": "zzz"})),
    ];
    for (cmd, p) in bad {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
    }
    // still intact
    assert_eq!(numbers(&mut s), ["A-101"]);
}

#[test]
fn the_sheet_count_is_capped() {
    let mut s = Session::new();
    s.execute("sheet.scheme", &json!({"pattern": "{N}", "width": 4, "disciplines": [{"code": "A", "start": 0}]})).unwrap();
    for _ in 0..1000 {
        s.execute("sheet.new", &json!({"title": "t"})).unwrap();
    }
    assert!(s.execute("sheet.new", &json!({"title": "one too many"})).is_err());
}

#[test]
fn deleting_a_sheet_can_take_its_layout() {
    let mut s = two_levels();
    s.execute("sheetindex.build", &json!({})).unwrap();
    let layout = s.execute("sheet.list", &json!({})).unwrap()["sheets"][0]["layout"].as_str().unwrap().to_string();
    s.execute("sheet.delete", &json!({"sheet": "A-101", "layout": true})).unwrap();
    assert!(s.doc().unwrap().layout(&layout).is_none());
    assert_eq!(numbers(&mut s), ["A-102"]);
}

//! `takeoff.record` with `levels`: every requested level is measured and recorded in one run, each row carries its
//! level's `level_id`, the header says which levels were recorded (`level_status`), and `takeoff.levels` says which of
//! them changed since. Hostile `levels` values are refused without changing anything.

use aad_engine::Session;
use serde_json::{Value, json};

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-takeoff-levels-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("house.osp")
}

fn cleanup(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        std::fs::remove_dir_all(dir).ok();
    }
}

fn layer(s: &mut Session, name: &str) {
    s.execute("layer.current", &json!({"name": name})).unwrap();
}

/// A drawing with Ground (a 9 m and a 6 m brick wall), First (one 5 m brick wall) and Roof (nothing). Ground is active
/// and measured; First and Roof are not measured by the caller.
fn house() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]}))
        .unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 6000], [6000, 6000]]})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.new", &json!({"name": "Roof"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [5000, 0]]})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    s
}

fn records(path: &std::path::Path) -> Vec<(u64, Value)> {
    aad_odp::Odp::open(path)
        .unwrap()
        .takeoff_as_records()
        .unwrap()
        .into_iter()
        .map(|(id, payload)| (id, serde_json::from_slice(&payload).unwrap_or(Value::Null)))
        .collect()
}

fn header(path: &std::path::Path) -> Value {
    records(path).into_iter().find(|(id, _)| id >> 56 == 1).map(|(_, v)| v).expect("a header")
}

/// The wall rows `(level, level_id, length_mm)` of a project file.
fn walls(path: &std::path::Path) -> Vec<(String, Option<u64>, f64)> {
    let mut v: Vec<_> = records(path)
        .into_iter()
        .filter(|(id, _)| id >> 56 == 3)
        .map(|(_, r)| (r["level"].as_str().unwrap_or("").to_string(), r["level_id"].as_u64(), r["length_mm"].as_f64().unwrap_or(0.0)))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.total_cmp(&b.2)));
    v
}

fn record(s: &mut Session, path: &std::path::Path, extra: Value) -> Result<Value, aad_engine::EngineError> {
    let mut p = json!({"path": path.to_string_lossy(), "annotate": false});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    s.execute("takeoff.record", &p)
}

fn level_id(s: &Session, name: &str) -> u64 {
    u64::from(s.doc().unwrap().level_by_name(name).unwrap().id.0)
}

#[test]
fn all_levels_are_recorded_in_one_run_each_with_its_level_id() {
    let mut s = house();
    let path = temp("all");
    let (ground, first) = (level_id(&s, "Ground"), level_id(&s, "First"));
    let objects = |s: &Session, n: &str| s.doc().unwrap().level_store(s.doc().unwrap().level_by_name(n).unwrap().id).unwrap().len();
    let before = (objects(&s, "Ground"), objects(&s, "First"), objects(&s, "Roof"));

    let r = record(&mut s, &path, json!({"levels": "all"})).unwrap();
    assert_eq!(r["state"], "complete", "{r}");
    let w = walls(&path);
    assert_eq!(
        w,
        vec![("First".to_string(), Some(first), 5000.0), ("Ground".to_string(), Some(ground), 6000.0), ("Ground".to_string(), Some(ground), 9000.0),],
        "{w:?}"
    );
    // The open drawing was not touched: same active level, no labels or rows left on any level.
    assert_eq!(s.doc().unwrap().active_level_ref().unwrap().name, "Ground");
    assert_eq!((objects(&s, "Ground"), objects(&s, "First"), objects(&s, "Roof")), before);

    let h = header(&path);
    let st = &h["level_status"];
    assert_eq!(st["mode"], "all");
    assert!(st["recorded_at"].as_str().is_some_and(|t| t.ends_with('Z')), "{st}");
    let lv = st["levels"].as_array().unwrap();
    assert_eq!(lv.len(), 3);
    let by = |n: &str| lv.iter().find(|l| l["name"] == n).unwrap().clone();
    assert_eq!((by("Ground")["recorded"].clone(), by("Ground")["rows"].clone()), (json!(true), json!(2)));
    assert_eq!((by("First")["recorded"].clone(), by("First")["rows"].clone()), (json!(true), json!(1)));
    assert_eq!(by("Roof")["rows"], 0);
    assert!(by("First")["fingerprint"].as_str().is_some_and(|f| f.len() == 16));
    assert!(lv.iter().all(|l| l["stale"] == false));
    // The result carries the same list.
    assert_eq!(r["levels"].as_array().map(Vec::len), Some(3));
    cleanup(&path);
}

#[test]
fn a_list_of_levels_by_name_or_id_records_only_those() {
    let mut s = house();
    let path = temp("list");
    let first = level_id(&s, "First");
    record(&mut s, &path, json!({"levels": [first]})).unwrap();
    let w = walls(&path);
    assert_eq!(w, vec![("First".to_string(), Some(first), 5000.0)], "the active level is not recorded unless asked: {w:?}");
    let st = &header(&path)["level_status"];
    assert_eq!(st["mode"], "list");
    let rec: Vec<_> = st["levels"].as_array().unwrap().iter().filter(|l| l["recorded"] == true).map(|l| l["name"].clone()).collect();
    assert_eq!(rec, vec![json!("First")]);

    // A new run replaces the drawing's records: now Ground by name (case-insensitive), First is gone.
    record(&mut s, &path, json!({"levels": ["ground"]})).unwrap();
    assert!(walls(&path).iter().all(|(l, _, _)| l == "Ground"));
    assert_eq!(walls(&path).len(), 2);
    cleanup(&path);
}

#[test]
fn a_plain_recording_of_a_drawing_with_levels_reports_the_active_level() {
    let mut s = house();
    let path = temp("plain");
    record(&mut s, &path, json!({})).unwrap();
    let st = &header(&path)["level_status"];
    assert_eq!(st["mode"], "active");
    let rec: Vec<_> = st["levels"].as_array().unwrap().iter().filter(|l| l["recorded"] == true).map(|l| l["name"].clone()).collect();
    assert_eq!(rec, vec![json!("Ground")]);
    cleanup(&path);
}

#[test]
fn a_changed_level_is_stale_until_it_is_recorded_again() {
    let mut s = house();
    let path = temp("stale");
    record(&mut s, &path, json!({"levels": "all"})).unwrap();
    let flags = |s: &mut Session| -> Vec<(String, bool, bool)> {
        let r = s.execute("takeoff.levels", &json!({})).unwrap();
        r["levels"].as_array().unwrap().iter().map(|l| (l["name"].as_str().unwrap().to_string(), l["recorded"] == true, l["stale"] == true)).collect()
    };
    assert_eq!(flags(&mut s), vec![("Ground".into(), true, false), ("First".into(), true, false), ("Roof".into(), true, false)]);

    // Change First while Ground is active.
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    s.execute("line", &json!({"points": [[0, 1000], [5000, 1000]]})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    assert_eq!(flags(&mut s), vec![("Ground".into(), true, false), ("First".into(), true, true), ("Roof".into(), true, false)]);

    // `update` writes the flags into the project file.
    let r = s.execute("takeoff.levels", &json!({"update": true, "path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["updated"], true, "{r}");
    let h = header(&path);
    let stale: Vec<_> = h["level_status"]["levels"].as_array().unwrap().iter().filter(|l| l["stale"] == true).map(|l| l["name"].clone()).collect();
    assert_eq!(stale, vec![json!("First")]);
    assert_eq!(h["state"], "complete");

    // Recording again clears it.
    record(&mut s, &path, json!({"levels": "all"})).unwrap();
    assert!(flags(&mut s).iter().all(|(_, _, stale)| !stale));
    assert!(header(&path)["level_status"]["levels"].as_array().unwrap().iter().all(|l| l["stale"] == false));
    // A level that was never recorded is not stale.
    record(&mut s, &path, json!({"levels": ["Ground"]})).unwrap();
    assert_eq!(flags(&mut s), vec![("Ground".into(), true, false), ("First".into(), false, false), ("Roof".into(), false, false)]);
    cleanup(&path);
}

#[test]
fn a_deleted_recorded_level_shows_as_stale() {
    let mut s = house();
    let path = temp("deleted");
    record(&mut s, &path, json!({"levels": "all"})).unwrap();
    s.execute("level.delete", &json!({"level": "Roof"})).ok();
    let r = s.execute("takeoff.levels", &json!({})).unwrap();
    let names: Vec<_> = r["levels"].as_array().unwrap().iter().map(|l| l["name"].clone()).collect();
    if !names.contains(&json!("Roof")) {
        let gone = r["levels"].as_array().unwrap().iter().find(|l| l["deleted"] == true).expect("the deleted level is reported");
        assert_eq!(gone["stale"], true);
    }
    cleanup(&path);
}

#[test]
fn a_drawing_without_named_levels_records_as_before() {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let path = temp("nolevels");
    let r = record(&mut s, &path, json!({"levels": "all"})).unwrap();
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("no named levels")), "{r}");
    assert!(header(&path).get("level_status").is_none(), "no level_status without levels");
    assert_eq!(walls(&path).len(), 1);
    assert!(walls(&path).iter().all(|(_, id, _)| id.is_none()));
    // A list needs levels to name.
    let e = record(&mut s, &path, json!({"levels": ["Ground"]})).unwrap_err().to_string();
    assert!(e.contains("no named levels"), "{e}");
    // And the status command still answers.
    let st = s.execute("takeoff.levels", &json!({})).unwrap();
    assert_eq!(st["levels"], json!([]));
    cleanup(&path);
}

#[test]
fn hostile_levels_values_are_refused_and_change_nothing() {
    let mut s = house();
    let path = temp("hostile");
    let many: Vec<Value> = (0..65).map(|i| json!(format!("L{i}"))).collect();
    let nested = json!([["First"]]);
    let bad: Vec<Value> = vec![
        json!([]),
        json!(many),
        json!(nested),
        json!(["Nope"]),
        json!(["First", "Nope"]),
        json!(99_999),
        json!(-1),
        json!(1.5),
        json!(u64::MAX),
        json!(true),
        json!({"a": 1}),
        json!("x".repeat(100_000)),
        json!([null]),
        json!(["First\u{0}"]),
    ];
    let objects = s.doc().unwrap().model.len();
    for v in bad {
        let r = record(&mut s, &path, json!({"levels": v}));
        assert!(r.is_err(), "{v} must be refused");
    }
    // Both `level` and `levels`.
    assert!(record(&mut s, &path, json!({"levels": "all", "level": "First"})).is_err());
    assert!(!path.exists() || records(&path).is_empty(), "nothing was written");
    assert_eq!(s.doc().unwrap().model.len(), objects);
    assert_eq!(s.doc().unwrap().active_level_ref().unwrap().name, "Ground");
    // Duplicates are folded, and `null` means "not given".
    record(&mut s, &path, json!({"levels": ["First", "first", "FIRST"]})).unwrap();
    assert_eq!(walls(&path).len(), 1);
    record(&mut s, &path, json!({"levels": null})).unwrap();
    assert_eq!(header(&path)["level_status"]["mode"], "active");
    cleanup(&path);
}

#[test]
fn a_header_without_level_status_still_reads_and_updates_as_nothing() {
    // A project file recorded before this key existed: `takeoff.levels` with `update` changes nothing and says so.
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let path = temp("old");
    record(&mut s, &path, json!({})).unwrap();
    let r = s.execute("takeoff.levels", &json!({"update": true, "path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["updated"], false, "{r}");
    assert_eq!(header(&path)["state"], "complete");
    cleanup(&path);
}

/// A drawing whose levels are `levels` (name, elevation in the level's own unit), each with one measured brick wall,
/// linked to one company-file project so recordings into the same file are checked against each other.
fn linked(levels: &[(&str, f64)]) -> Session {
    let context = json!({
        "v": 1, "prepared_on": "2026-11-15", "company": { "name": "Studio One" },
        "project": { "project": { "name": "Mehta residence" }, "client": { "name": "Mehta" },
            "project_ref": { "v": 1, "company_uuid": "u-1", "project_id": 7 } }
    });
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("companyfile", &json!({ "context": context })).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    s.execute("level.new", &json!({"name": levels[1].0, "elevation": levels[1].1, "first": levels[0].0})).unwrap();
    for (n, (name, elevation)) in levels.iter().enumerate() {
        if n > 1 {
            s.execute("level.new", &json!({"name": name, "elevation": elevation})).unwrap();
        }
        s.execute("level.activate", &json!({"level": name})).unwrap();
        s.execute("level.set", &json!({"level": name, "elevation": elevation})).unwrap();
        layer(&mut s, "MEASURE-FULLBRICK");
        s.execute("line", &json!({"points": [[0, 0], [4000 + n as i64 * 100, 0]]})).unwrap();
        s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    }
    s
}

fn double_counts(r: &Value) -> Vec<String> {
    r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|t| t.contains("double-count")).map(str::to_string).collect()
}

#[test]
fn the_double_count_warning_compares_floors_by_elevation_when_both_levels_have_one() {
    let path = temp("overlap");
    let mut a = linked(&[("Ground", 0.0), ("First", 3.0)]);
    assert!(double_counts(&record(&mut a, &path, json!({"levels": "all"})).unwrap()).is_empty(), "the first drawing alone is not a double-count");

    // Other names, same elevations (0 and 3 m): the same floors.
    let mut b = linked(&[("Base", 0.0), ("Upper", 3.0)]);
    let w = double_counts(&record(&mut b, &path, json!({"levels": "all"})).unwrap());
    assert!(w.iter().any(|t| t.contains("`Base`")) && w.iter().any(|t| t.contains("`Upper`")), "{w:?}");

    // The same name at another elevation is another floor: "First" at -2 m matches nothing in A, although A has a "First".
    let mut g = linked(&[("First", -2.0), ("Top", 6.0)]);
    let w = double_counts(&record(&mut g, &path, json!({"levels": "all"})).unwrap());
    assert!(w.iter().all(|t| !t.contains("`First`")), "{w:?}");

    // Without a level list a drawing with no named levels is compared by name, as before (see takeoff_record.rs).
    cleanup(&path);
}

//! Live sync, MynaCAD side (`docs/live-sync.md`): `project.sync` tells that another program changed the project file, accepts MynaCAD's own
//! writes, stamps the take-off per level, and reports damaged, missing and replaced files without panicking or writing.

use aad_engine::Session;
use serde_json::{Value, json};

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-project-sync-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("house.osp")
}

fn cleanup(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        std::fs::remove_dir_all(dir).ok();
    }
}

fn house() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    s.execute("layer.current", &json!({"name": "MEASURE-FULLBRICK"})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    s.execute("layer.current", &json!({"name": "MEASURE-FULLBRICK"})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    s
}

fn record(s: &mut Session, path: &std::path::Path) {
    s.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false, "levels": "all"})).unwrap();
}

fn sync(s: &mut Session, path: &std::path::Path, ack: bool) -> Value {
    s.execute("project.sync", &json!({"path": path.to_string_lossy(), "ack": ack})).unwrap()
}

#[test]
fn recording_is_accepted_and_another_programs_write_is_reported_until_acknowledged() {
    let path = temp("flow");
    let mut s = house();
    record(&mut s, &path);
    let r = sync(&mut s, &path, false);
    assert_eq!(r["state"], "synced", "{r}");
    assert_eq!(r["text"], "Synced");
    let t = &r["takeoff"][0];
    assert!(t["slot"].is_u64());
    assert_eq!(t["levels"].as_array().map(Vec::len), Some(2), "both levels are stamped: {r}");
    assert!(r["staleSlots"].as_array().unwrap().is_empty());

    // Another program writes (the project record): reported, not reloaded, and the file is not touched by the check.
    let mut odp = aad_odp::Odp::open(&path).unwrap();
    let mut e = odp.edit().unwrap();
    e.put_project_doc("{\"name\":\"x\"}").unwrap();
    let rev = e.commit().unwrap();
    drop(odp);
    let before = std::fs::read(&path).unwrap();
    let r = sync(&mut s, &path, false);
    assert_eq!((r["state"].as_str(), r["revision"].as_i64()), (Some("updated"), Some(rev)), "{r}");
    assert!(r["text"].as_str().unwrap().starts_with("Updated by "));
    assert_eq!(std::fs::read(&path).unwrap(), before, "the check wrote nothing");
    assert_eq!(sync(&mut s, &path, false)["state"], "updated", "it stays reported until acknowledged");

    assert_eq!(sync(&mut s, &path, true)["state"], "synced");
    // MynaCAD's own record afterwards is not "changed elsewhere".
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    s.execute("layer.current", &json!({"name": "MEASURE-FULLBRICK"})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [5000, 0]]})).unwrap();
    s.execute("level.activate", &json!({"level": "Ground"})).unwrap();
    record(&mut s, &path);
    assert_eq!(sync(&mut s, &path, false)["state"], "synced");
    cleanup(&path);
}

#[test]
fn a_saved_drawing_without_a_new_record_marks_the_takeoff_out_of_date() {
    let path = temp("stale");
    let mut s = house();
    record(&mut s, &path);
    let mut odp = aad_odp::Odp::open(&path).unwrap();
    let mut e = odp.edit().unwrap();
    e.put_drawing("house.dxf", "dxf", b"0\nEOF\n").unwrap();
    e.commit().unwrap();
    drop(odp);
    aad_engine::projectsync::note_own_write(&path, 0, 0, "");
    let r = sync(&mut s, &path, true);
    assert_eq!(r["state"], "outofdate", "{r}");
    assert_eq!(r["text"], "Take-off out of date");
    record(&mut s, &path);
    assert_eq!(sync(&mut s, &path, false)["state"], "synced");
    cleanup(&path);
}

#[test]
fn missing_damaged_and_replaced_files_are_reported() {
    let path = temp("bad");
    let mut s = house();
    assert_eq!(sync(&mut s, &path, false)["state"], "missing");
    record(&mut s, &path);
    assert_eq!(sync(&mut s, &path, false)["state"], "synced");
    // Replaced by an older copy: the revision goes backwards.
    let copy = path.with_file_name("copy.osp");
    {
        let mut odp = aad_odp::Odp::open(&path).unwrap();
        let mut e = odp.edit().unwrap();
        e.put_project_doc("{}").unwrap();
        e.commit().unwrap();
        odp.checkpoint().unwrap();
    }
    assert_eq!(sync(&mut s, &path, true)["state"], "synced");
    let older = path.with_file_name("older.osp");
    {
        let mut o = aad_odp::Odp::create(&older, "Other").unwrap();
        let mut e = o.edit().unwrap();
        e.put_project_doc("{}").unwrap();
        e.commit().unwrap();
    }
    std::fs::copy(&older, &copy).unwrap();
    std::fs::remove_file(path.with_extension("osp-wal")).ok();
    std::fs::remove_file(path.with_extension("osp-shm")).ok();
    std::fs::copy(&older, &path).unwrap();
    assert_eq!(sync(&mut s, &path, false)["state"], "replaced");
    // Garbage in place of the file.
    std::fs::write(&path, b"not a database, just text that is long enough to look like a header").unwrap();
    let r = sync(&mut s, &path, false);
    assert_eq!(r["state"], "unreadable", "{r}");
    assert!(r["reason"].as_str().is_some());
    std::fs::write(&path, b"").unwrap();
    assert_eq!(sync(&mut s, &path, false)["state"], "unreadable");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(sync(&mut s, &path, false)["state"], "missing");
    cleanup(&path);
}

#[test]
fn hostile_parameters_and_no_project_never_panic() {
    let mut s = Session::new();
    assert_eq!(s.execute("project.sync", &json!({})).unwrap()["state"], "none");
    for p in [json!({"path": 5}), json!({"path": ""}), json!({"path": "\u{0}x.osp"}), json!({"ack": "yes"}), json!({"path": "C:/no/such/dir/x.osp", "ack": true})] {
        let r = s.execute("project.sync", &p);
        assert!(r.is_ok(), "{p}: {r:?}");
    }
}

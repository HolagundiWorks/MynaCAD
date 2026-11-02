//! Version history sidecar and revision register.

use std::path::PathBuf;
use std::sync::Once;

use aad_engine::Session;
use aad_engine::cmd::file::{IoHooks, set_io};
use aad_engine::doc::EntityKind;
use serde_json::{Value, json};

fn install_io() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        set_io(IoHooks {
            read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
            plot: None,
        });
    });
}

fn temp_dxf(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-history-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("house.dxf")
}

fn session_with_path(path: &std::path::Path) -> Session {
    install_io();
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.state_mut().unwrap().path = Some(path.to_string_lossy().into());
    // Touch the drawing file so sidecar paths resolve beside a real place.
    std::fs::write(path, b"").unwrap();
    s
}

#[test]
fn save_and_restore_round_trip_model() {
    let path = temp_dxf("round");
    let mut s = session_with_path(&path);
    s.execute("line", &json!({"points": [[0.0, 0.0], [100.0, 0.0]]})).unwrap();
    s.execute("circle", &json!({"center": [50.0, 50.0], "radius": 10.0})).unwrap();
    let n0 = s.doc().unwrap().model.len();
    assert!(n0 >= 2);

    let saved = s.execute("history.save", &json!({"name": "m1", "scope": "project"})).unwrap();
    let id = saved["id"].as_u64().unwrap();
    assert!({
        // the history of a drawing that is not a project file is the sidecar path + ".history.osp"
        let mut p = path.as_os_str().to_os_string();
        p.push(".history.osp");
        PathBuf::from(p).exists()
    });

    let handles: Vec<_> = s.doc().unwrap().model.handles().iter().map(|h| json!(h.hex())).collect();
    s.execute("erase", &json!({"handles": handles})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);

    let r = s.execute("history.restore", &json!({"id": id})).unwrap();
    assert!(!r["stores"].as_array().unwrap().is_empty());
    assert_eq!(s.doc().unwrap().model.len(), n0);

    // Undo restores the empty model.
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
}

#[test]
fn history_rename_promotes_auto() {
    let path = temp_dxf("rename");
    let mut s = session_with_path(&path);
    s.execute("line", &json!({"points": [[0.0, 0.0], [1.0, 0.0]]})).unwrap();
    let saved = s.execute("history.save", &json!({"name": "auto-point", "scope": "project", "kind": "auto"})).unwrap();
    let id = saved["id"].as_u64().unwrap();
    let r = s.execute("history.rename", &json!({"id": id, "name": "Named"})).unwrap();
    assert_eq!(r["name"], "Named");
    assert_eq!(r["kind"], "milestone");
    let list = s.execute("history.list", &json!({})).unwrap();
    let m = list["milestones"].as_array().unwrap().iter().find(|m| m["id"] == id).unwrap();
    assert_eq!(m["kind"], "milestone");
    assert_eq!(m["name"], "Named");
}

#[test]
fn second_unchanged_milestone_shares_object_rows() {
    let path = temp_dxf("share");
    let mut s = session_with_path(&path);
    s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    s.execute("history.save", &json!({"name": "a", "scope": "project"})).unwrap();
    s.execute("history.save", &json!({"name": "b", "scope": "project"})).unwrap();

    let mut hist = path.as_os_str().to_os_string();
    hist.push(".history.osp");
    let records = aad_odp::Odp::open(&PathBuf::from(hist)).unwrap().history_records().unwrap();
    let objects = records.keys().filter(|id| aad_odp::history::kind_of(**id) == aad_odp::history::KIND_OBJECT).count();
    // One line → one object row, shared by both milestones.
    assert_eq!(objects, 1);
}

#[test]
fn revision_issue_updates_register_and_rev_field() {
    let path = temp_dxf("rev");
    let mut s = session_with_path(&path);
    s.execute("project", &json!({"details": {"title": "Test"}})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.execute("sheet.titleblock", &json!({})).unwrap();
    s.execute("line", &json!({"points": [[0.0, 0.0], [5.0, 0.0]]})).unwrap();

    let r = s
        .execute(
            "revision.issue",
            &json!({
                "description": "First issue",
                "status": "for construction",
                "by": "AB",
                "scope": "project",
            }),
        )
        .unwrap();
    assert_eq!(r["code"], "A");
    assert!(r["milestone"].as_u64().unwrap() >= 1);

    let list = s.execute("revision.list", &json!({})).unwrap();
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["code"], "A");
    assert_eq!(entries[0]["description"], "First issue");

    let fields = s.execute("sheet.fields", &json!({"layout": "Layout1"})).unwrap();
    assert_eq!(fields["fields"]["REV"], "A");

    // Void keeps the milestone; second issue gets B.
    s.execute("revision.void", &json!({"code": "A"})).unwrap();
    let r2 = s.execute("revision.issue", &json!({"description": "Second", "scope": "project"})).unwrap();
    assert_eq!(r2["code"], "B");

    // DXF round-trip of the register.
    let bytes = aad_io::write(s.doc().unwrap(), "house.dxf").unwrap();
    let back = aad_io::read(&bytes, "house.dxf").unwrap();
    let text = back.project.get("revisions").cloned().unwrap_or_default();
    let reg: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(reg["entries"].as_array().unwrap().len(), 2);
}

#[test]
fn revision_table_replaces_on_second_draw() {
    let path = temp_dxf("table");
    let mut s = session_with_path(&path);
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.execute("revision.issue", &json!({"description": "One", "scope": "project", "code": "P1"})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let d1 = s.execute("revision.table", &json!({"layout": "Layout1"})).unwrap();
    let n1 = d1["drawn"][0]["objects"].as_u64().unwrap();
    let count = |s: &Session| {
        s.doc().unwrap().layouts.iter().find(|l| l.name == "Layout1").unwrap().entities.iter().filter(|e| e.common.layer == "A-ANNO-REVT").count()
    };
    assert_eq!(count(&s), n1 as usize);
    s.execute("revision.table", &json!({"layout": "Layout1"})).unwrap();
    assert_eq!(count(&s), n1 as usize, "second draw replaces the first table");
    assert!(
        s.doc()
            .unwrap()
            .layouts
            .iter()
            .find(|l| l.name == "Layout1")
            .unwrap()
            .entities
            .iter()
            .any(|e| { e.common.layer == "A-ANNO-REVT" && matches!(e.kind, EntityKind::Text(_)) })
    );
}

#[test]
fn revision_cannot_be_deleted_from_history() {
    let path = temp_dxf("lock");
    let mut s = session_with_path(&path);
    let r = s.execute("revision.issue", &json!({"description": "Locked", "scope": "project", "code": "Z"})).unwrap();
    let id = r["milestone"].as_u64().unwrap();
    assert!(s.execute("history.delete", &json!({"id": id})).is_err());
}

#[test]
fn hostile_history_file_does_not_panic() {
    let path = temp_dxf("hostile");
    let mut s = session_with_path(&path);
    let mut hist = path.as_os_str().to_os_string();
    hist.push(".history.osp");
    std::fs::write(&hist, b"NOT A PROJECT FILE, only garbage").unwrap();
    // Ensure a drawing id so open is attempted.
    let _ = s.execute("history.list", &json!({}));
    // Damaged file should error, not panic.
    assert!(s.execute("history.save", &json!({"name": "x", "scope": "project"})).is_err());
}

#[test]
fn compare_counts_added_removed_changed() {
    let path = temp_dxf("cmp");
    let mut s = session_with_path(&path);
    s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let id = s.execute("history.save", &json!({"name": "base", "scope": "project"})).unwrap()["id"].as_u64().unwrap();
    s.execute("circle", &json!({"center": [5.0, 5.0], "radius": 2.0})).unwrap();
    let handles: Vec<_> = s.doc().unwrap().model.handles().iter().map(|h| json!(h.hex())).collect();
    // Erase the original line → removed from now vs milestone; circle is added.
    let line_h = handles.first().cloned().unwrap();
    s.execute("erase", &json!({"handles": [line_h]})).unwrap();

    let cmp = s.execute("history.compare", &json!({"from": id})).unwrap();
    assert!(cmp["counts"]["added"].as_u64().unwrap() >= 1, "{cmp}");
    assert!(cmp["counts"]["removed"].as_u64().unwrap() >= 1, "{cmp}");
}

#[test]
fn preview_and_esc_clear() {
    let path = temp_dxf("prev");
    let mut s = session_with_path(&path);
    s.execute("line", &json!({"points": [[0.0, 0.0], [1.0, 0.0]]})).unwrap();
    let id = s.execute("history.save", &json!({"name": "p", "scope": "project"})).unwrap()["id"].as_u64().unwrap();
    s.execute("circle", &json!({"center": [0.0, 0.0], "radius": 1.0})).unwrap();
    let r = s.execute("history.preview", &json!({"id": id})).unwrap();
    assert!(r["preview"]["added"].as_u64().unwrap() >= 1);
    assert!(!s.history_ghost().is_empty());
    s.cancel();
    assert!(s.history_preview.is_none());
    assert!(s.history_ghost().is_empty());
}

#[test]
fn revision_clouds_propose_and_accept() {
    let path = temp_dxf("clouds");
    let mut s = session_with_path(&path);
    s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let id = s.execute("history.save", &json!({"name": "c0", "scope": "project"})).unwrap()["id"].as_u64().unwrap();
    s.execute("line", &json!({"points": [[100.0, 100.0], [110.0, 100.0]]})).unwrap();
    let prop = s.execute("revision.clouds", &json!({"from": id, "gap": 50.0})).unwrap();
    assert!(!prop["proposals"].as_array().unwrap().is_empty(), "{prop}");
    let before = s.doc().unwrap().model.len();
    let placed = s.execute("revision.clouds", &json!({"from": id, "gap": 50.0, "accept": true})).unwrap();
    assert!(!placed["placed"].as_array().unwrap().is_empty());
    assert!(s.doc().unwrap().model.len() > before);
}

#[test]
fn history_mirror_not_available() {
    let path = temp_dxf("mir");
    let mut s = session_with_path(&path);
    let err = s.execute("history.mirror", &json!({})).unwrap_err().to_string();
    assert!(err.contains("not available"), "{err}");
}

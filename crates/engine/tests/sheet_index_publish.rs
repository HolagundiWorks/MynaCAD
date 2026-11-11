//! Publish by issue set or by discipline through the Publisher. Plot and DWG hooks are fakes.

use std::sync::OnceLock;

use aad_engine::Session;
use aad_engine::cmd::file::{IoHooks, set_io, set_pdf_merge};
use serde_json::json;

fn hooks() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        set_io(IoHooks { read: |_, _| Err("no reader".into()), write: |_, _| Ok(b"DWG".to_vec()), plot: Some(|_, _, _| Ok(b"PDF".to_vec())) });
        set_pdf_merge(|parts| Ok(parts.concat()));
    });
}

fn project() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("sheetindex.build", &json!({})).unwrap();
    s.execute("sheet.new", &json!({"title": "Power plan", "discipline": "E"})).unwrap();
    s
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-sidx-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn publishing_an_issue_plots_exactly_its_frozen_sheets() {
    hooks();
    let mut s = project();
    s.execute("issue.create", &json!({"name": "Permit", "discipline": "A"})).unwrap();
    // A later sheet is not part of the issue.
    s.execute("sheetindex.build", &json!({"levels": false, "views": ["Section 1"]})).unwrap();
    let dir = tmp("issue");
    let r = s.execute("sheetindex.publish", &json!({"issue": "Permit", "folder": dir.to_string_lossy(), "takeoff": false})).unwrap();
    assert_eq!(r["set"], "Issue Permit");
    assert_eq!(r["files"].as_array().unwrap().len(), 2);
    for f in r["files"].as_array().unwrap() {
        assert!(std::path::Path::new(f["path"].as_str().unwrap()).exists());
    }
    // the Publisher Set exists and lists the two layouts in index order
    let sets = s.execute("publisher.list", &json!({})).unwrap();
    assert_eq!(sets["sets"][0]["name"], "Issue Permit");
    assert_eq!(sets["sets"][0]["layouts"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn publishing_a_discipline_skips_void_sheets_and_sheets_without_a_layout() {
    hooks();
    let mut s = project();
    s.execute("sheet.new", &json!({"title": "No layout"})).unwrap(); // A-103, no layout
    s.execute("sheet.set", &json!({"sheet": "A-102", "status": "void"})).unwrap();
    let dir = tmp("disc");
    let r =
        s.execute("sheetindex.publish", &json!({"discipline": "A", "singleFile": true, "folder": dir.to_string_lossy(), "takeoff": false})).unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 1);
    assert_eq!(r["files"][0]["pages"], 1);
    let skipped = r["skipped"].as_array().unwrap();
    assert!(skipped.iter().any(|x| x["sheet"] == "A-102" && x["reason"] == "void"));
    assert!(skipped.iter().any(|x| x["sheet"] == "A-103" && x["reason"] == "no layout"));
    // DWG goes through the same path
    let r = s.execute("sheetindex.publish", &json!({"discipline": "A", "format": "dwg", "folder": dir.to_string_lossy(), "takeoff": false})).unwrap();
    assert!(r["files"][0]["path"].as_str().unwrap().ends_with(".dwg"));
    // checkOnly writes nothing
    let r = s.execute("sheetindex.publish", &json!({"checkOnly": true, "folder": dir.to_string_lossy()})).unwrap();
    assert!(r["files"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

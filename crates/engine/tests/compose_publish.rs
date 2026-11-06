//! Publisher output: one PDF, one DWG, and a recorded take-off attempt. Hooks are fakes so the test
//! does not need a plotter or a DWG writer.

use std::sync::{Mutex, OnceLock};

use aad_engine::cmd::file::{IoHooks, set_io, set_pdf_merge};
use aad_engine::Session;
use serde_json::json;

static PLOTS: Mutex<usize> = Mutex::new(0);
static WRITES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn hooks() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        set_io(IoHooks {
            read: |_, _| Err("no reader".into()),
            write: |_, name| {
                WRITES.lock().map(|mut w| w.push(name.to_string())).ok();
                Ok(b"DWG".to_vec())
            },
            plot: Some(|_, space, _| {
                PLOTS.lock().map(|mut n| *n += 1).ok();
                Ok(format!("PDF:{space:?}").into_bytes())
            }),
        });
        set_pdf_merge(|parts| {
            let mut out = Vec::new();
            for part in parts {
                out.extend_from_slice(part);
                out.push(b'\n');
            }
            Ok(out)
        });
    });
}

#[test]
fn single_file_pdf_joins_pages_and_records_the_takeoff_attempt() {
    hooks();
    let mut s = Session::new();
    s.execute("publisher.new", &json!({"name": "Issue", "layouts": ["Layout1", "Layout2"], "singleFile": true})).unwrap();
    let dir = std::env::temp_dir().join(format!("aad-pub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let r = s
        .execute("publisher.publish", &json!({"set": "Issue", "folder": dir.to_string_lossy(), "takeoff": true}))
        .unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 1);
    assert!(r["files"][0]["path"].as_str().unwrap().ends_with(".pdf"));
    assert_eq!(r["files"][0]["pages"], 2);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("take-off")));
    let bytes = std::fs::read(r["files"][0]["path"].as_str().unwrap()).unwrap();
    assert!(bytes.windows(4).any(|w| w == b"PDF:"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dwg_publisher_writes_one_file_per_layout_or_one_when_joined() {
    hooks();
    let mut s = Session::new();
    s.execute("publisher.new", &json!({"name": "Cad", "layouts": ["Layout1", "Layout2"], "format": "dwg"})).unwrap();
    let dir = std::env::temp_dir().join(format!("aad-pub-dwg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let many = s.execute("publisher.publish", &json!({"set": "Cad", "folder": dir.to_string_lossy(), "takeoff": false})).unwrap();
    assert_eq!(many["files"].as_array().unwrap().len(), 2);
    s.execute("publisher.set", &json!({"set": "Cad", "singleFile": true})).unwrap();
    let one = s.execute("publisher.publish", &json!({"set": "Cad", "folder": dir.to_string_lossy(), "takeoff": false})).unwrap();
    assert_eq!(one["files"].as_array().unwrap().len(), 1);
    assert!(one["files"][0]["path"].as_str().unwrap().ends_with(".dwg"));
    let _ = std::fs::remove_dir_all(&dir);
}

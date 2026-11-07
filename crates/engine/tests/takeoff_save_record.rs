//! After a DXF/DWG save, take-off is recorded for O-CoreOp when the drawing has measured
//! take-off (or schedule / electrical) data. Failure is echoed and never fails the save.

use aad_engine::Session;
use aad_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

fn install_write() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
}

fn measured_session() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    s.execute("layer.current", &json!({"name": "MEASURE-FULLBRICK"})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    s
}

#[test]
fn saving_a_measured_drawing_tries_to_record_takeoff_and_still_succeeds() {
    install_write();
    let mut s = measured_session();
    let path = std::env::temp_dir().join(format!("aad-takeoff-save-{}.dxf", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["path"].as_str().unwrap(), path.to_string_lossy());
    let echoed = s.log.iter().any(|l| l.starts_with("Take-off recorded for Falcon") || l.starts_with("Take-off was not recorded:"));
    assert!(echoed, "expected a take-off record attempt after save; log={:?}", s.log);
}

#[test]
fn saving_an_empty_drawing_does_not_record_takeoff() {
    install_write();
    let mut s = Session::new();
    let path = std::env::temp_dir().join(format!("aad-takeoff-empty-{}.dxf", std::process::id()));
    let before = s.log.len();
    s.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&path);
    let extra: Vec<_> = s.log[before..].iter().filter(|l| l.contains("Take-off")).collect();
    assert!(extra.is_empty(), "empty drawing must not auto-record; got {extra:?}");
}

#[test]
fn exporting_a_png_does_not_record_takeoff() {
    install_write();
    let mut s = measured_session();
    let path = std::env::temp_dir().join(format!("aad-takeoff-save-{}.png", std::process::id()));
    let before = s.log.len();
    s.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&path);
    let extra: Vec<_> = s.log[before..].iter().filter(|l| l.contains("Take-off")).collect();
    assert!(extra.is_empty(), "image export is not a drawing save; got {extra:?}");
}

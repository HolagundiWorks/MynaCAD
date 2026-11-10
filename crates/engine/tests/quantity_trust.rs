//! Quantity-trust checklist: known wall / opening numbers through
//! fixture drawing → `takeoff.record` / bridge / `.osp`, catching double-count,
//! `wall_ref`, and thickness regressions.
//!
//! Expected numbers are also published as
//! `crates/engine/tests/fixtures/quantity-trust-checklist.json` for MynaOps
//! CadImport tests and the clean-VM runbook.
//!
//! Optional export for CoreOp hand-off:
//! `TAKEOFF_TRUST_EXPORT=/path/to/dir cargo test -p aad-engine --test quantity_trust -- --nocapture`

use aad_engine::Session;
use aad_engine::doc::Handle;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Company-file project the checklist drawing is linked to.
const COMPANY_UUID: &str = "quantity-trust-co";
const PROJECT_ID: i64 = 42;
const DRAWING_ID_HINT: &str = "quantity-trust-checklist";

/// Known geometry (millimetres). Two measured walls of 6000; door D2 and window W3 on the front wall.
const WALL_LEN_MM: f64 = 6000.0;
const WALL_HEIGHT_MM: f64 = 3000.0;
const WALL_THICK_MM: f64 = 230.0;
const DEPTH_MM: f64 = 4000.0;
const DOOR_W_MM: f64 = 900.0;
const DOOR_H_MM: f64 = 2100.0;
const WIN_W_MM: f64 = 1200.0;
const WIN_H_MM: f64 = 1200.0;

/// Face-area checklist (m²) for human / QS audit — CadImport stores facts; net m² is documented here.
const FRONT_GROSS_M2: f64 = 18.0; // 6.0 m × 3.0 m
const DOOR_DEDUCT_M2: f64 = 1.89; // 0.9 × 2.1
const WIN_DEDUCT_M2: f64 = 1.44; // 1.2 × 1.2
const FRONT_NET_M2: f64 = 14.67; // 18 − 1.89 − 1.44
const BACK_NET_M2: f64 = 18.0;
const TOTAL_NET_M2: f64 = 32.67;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s
}

fn last_handle(s: &Session) -> Handle {
    s.doc().unwrap().model.iter().last().unwrap().handle
}

fn layer(s: &mut Session, name: &str) {
    s.execute("layer.current", &json!({"name": name})).unwrap();
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-quantity-trust-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("quantity-trust.osp")
}

fn cleanup(path: &Path) {
    if let Some(dir) = path.parent() {
        std::fs::remove_dir_all(dir).ok();
    }
}

fn read_vdb(path: &Path) -> std::collections::BTreeMap<u64, Value> {
    aad_odp::Odp::open(path)
        .unwrap()
        .takeoff_as_records()
        .unwrap()
        .into_iter()
        .map(|(id, payload)| (id, serde_json::from_slice(&payload).unwrap_or(Value::Null)))
        .collect()
}

fn kind(id: u64) -> u64 {
    id >> 56
}

fn rows_of(db: &std::collections::BTreeMap<u64, Value>, k: u64) -> Vec<&Value> {
    db.iter().filter(|(id, _)| kind(**id) == k).map(|(_, p)| p).collect()
}

fn header(db: &std::collections::BTreeMap<u64, Value>) -> &Value {
    rows_of(db, 1).into_iter().next().expect("header")
}

struct TrustScene {
    front: Handle,
    back: Handle,
}

/// Deterministic checklist drawing: closed wall box, D2 + W3 on the front edge, two measured brick lines.
fn trust_scene(s: &mut Session) -> TrustScene {
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]}))
        .unwrap();
    s.execute("openings.std", &json!({})).unwrap();
    // A-WALL faces at 230 mm so thickness is measured, not nominal.
    s.execute(
        "wall.draw",
        &json!({"points": [[0, 0], [WALL_LEN_MM, 0], [WALL_LEN_MM, DEPTH_MM], [0, DEPTH_MM], [0, 0]], "thickness": WALL_THICK_MM}),
    )
    .unwrap();
    s.execute("wall.door", &json!({"at": [1500, 0], "name": "D2", "width": DOOR_W_MM})).unwrap();
    s.execute("wall.window", &json!({"at": [4000, 0], "name": "W3", "width": WIN_W_MM})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [WALL_LEN_MM, 0]]})).unwrap();
    let front = last_handle(s);
    s.execute("line", &json!({"points": [[0, DEPTH_MM], [WALL_LEN_MM, DEPTH_MM]]})).unwrap();
    let back = last_handle(s);
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    TrustScene { front, back }
}

fn link_project(s: &mut Session) {
    s.execute(
        "companyfile",
        &json!({
            "context": {
                "v": 1,
                "prepared_on": "2026-10-28",
                "company": { "name": "Quantity Trust Studio" },
                "project": {
                    "project": { "name": "Quantity trust checklist" },
                    "client": { "name": "QS audit" },
                    "project_ref": { "v": 1, "company_uuid": COMPANY_UUID, "project_id": PROJECT_ID }
                }
            }
        }),
    )
    .unwrap();
}

fn record(s: &mut Session, path: &Path) -> Value {
    s.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false})).unwrap()
}

fn assert_checklist_numbers(db: &std::collections::BTreeMap<u64, Value>, scene: &TrustScene) {
    let walls = rows_of(db, 3);
    assert_eq!(walls.len(), 2, "checklist has two measured walls: {walls:?}");
    for w in &walls {
        assert_eq!(w["length_mm"].as_f64(), Some(WALL_LEN_MM), "{w}");
        assert_eq!(w["height_mm"].as_f64(), Some(WALL_HEIGHT_MM), "{w}");
        assert_eq!(w["thickness_mm"].as_f64(), Some(WALL_THICK_MM), "{w}");
        assert_eq!(w["level"], "Ground");
        assert!(w.get("_note").is_none(), "faces measured → no nominal thickness note: {w}");
    }
    let front_ref = format!("W{}", scene.front.hex());
    let back_ref = format!("W{}", scene.back.hex());
    let refs: Vec<&str> = walls.iter().filter_map(|w| w["ref"].as_str()).collect();
    assert!(refs.contains(&front_ref.as_str()) && refs.contains(&back_ref.as_str()), "{refs:?}");

    let openings = rows_of(db, 4);
    assert_eq!(openings.len(), 2, "{openings:?}");
    let mut door = None;
    let mut window = None;
    for o in &openings {
        assert_eq!(o["wall_ref"], json!(front_ref), "both openings deduct from the front wall: {o}");
        assert!(o.get("_note").is_none(), "MEASURE-DEDUCT match → no proximity note: {o}");
        match o["mark"].as_str() {
            Some("D2") => door = Some(*o),
            Some("W3") => window = Some(*o),
            other => panic!("unexpected mark {other:?} in {o}"),
        }
    }
    let door = door.expect("D2");
    let window = window.expect("W3");
    assert_eq!(door["kind"], "Door");
    assert_eq!(door["width_mm"].as_f64(), Some(DOOR_W_MM));
    assert_eq!(door["height_mm"].as_f64(), Some(DOOR_H_MM));
    assert_eq!(window["kind"], "Window");
    assert_eq!(window["width_mm"].as_f64(), Some(WIN_W_MM));
    assert_eq!(window["height_mm"].as_f64(), Some(WIN_H_MM));

    let h = header(db);
    assert_eq!(h["state"], "complete");
    assert_eq!(h["format"], "hcw-takeoff-shilpi");
    assert_eq!(h["source"]["project_ref"]["company_uuid"], COMPANY_UUID);
    assert_eq!(h["source"]["project_ref"]["project_id"], PROJECT_ID);
}

fn maybe_export(bridge: &Value, vdb: &Path, scene: &TrustScene) {
    let Ok(dir) = std::env::var("TAKEOFF_TRUST_EXPORT") else { return };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let checklist = json!({
        "name": DRAWING_ID_HINT,
        "description": "Known wall/opening numbers for Draft→project file→CoreOp quantity trust",
        "project_ref": { "v": 1, "company_uuid": COMPANY_UUID, "project_id": PROJECT_ID },
        "level": "Ground",
        "walls": [
            { "role": "front", "ref": format!("W{}", scene.front.hex()), "length_mm": WALL_LEN_MM, "height_mm": WALL_HEIGHT_MM, "thickness_mm": WALL_THICK_MM },
            { "role": "back", "ref": format!("W{}", scene.back.hex()), "length_mm": WALL_LEN_MM, "height_mm": WALL_HEIGHT_MM, "thickness_mm": WALL_THICK_MM }
        ],
        "openings": [
            { "mark": "D2", "kind": "Door", "width_mm": DOOR_W_MM, "height_mm": DOOR_H_MM, "wall_role": "front" },
            { "mark": "W3", "kind": "Window", "width_mm": WIN_W_MM, "height_mm": WIN_H_MM, "wall_role": "front" }
        ],
        "expected_face_m2": {
            "front_gross": FRONT_GROSS_M2,
            "door_deduct": DOOR_DEDUCT_M2,
            "window_deduct": WIN_DEDUCT_M2,
            "front_net": FRONT_NET_M2,
            "back_net": BACK_NET_M2,
            "total_net": TOTAL_NET_M2
        },
        "bridge": bridge,
    });
    std::fs::write(dir.join("quantity-trust-checklist.json"), serde_json::to_string_pretty(&checklist).unwrap()).unwrap();
    std::fs::copy(vdb, dir.join("quantity-trust.osp")).unwrap();
    eprintln!("quantity-trust export written under {}", dir.display());
}

#[test]
fn checklist_drawing_records_known_wall_and_opening_numbers() {
    let mut s = session();
    let scene = trust_scene(&mut s);
    link_project(&mut s);
    let path = temp("checklist");
    let r = record(&mut s, &path);
    assert_eq!(r["counts"]["walls"], 2);
    assert_eq!(r["counts"]["openings"], 2);
    assert!(
        !r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("nominal") || t.contains("double-count"))),
        "clean checklist must not warn about nominal thickness or double-count: {r}"
    );

    let db = read_vdb(&path);
    assert_checklist_numbers(&db, &scene);

    let bridge: Value = serde_json::from_str(s.execute("bridge.export", &json!({"write": false})).unwrap()["json"].as_str().unwrap()).unwrap();
    assert_eq!(bridge["walls"].as_array().unwrap().len(), 2);
    assert_eq!(bridge["openings"].as_array().unwrap().len(), 2);
    assert_eq!(bridge["source"]["project_ref"]["company_uuid"], COMPANY_UUID);
    assert_eq!(bridge["source"]["project_ref"]["project_id"], PROJECT_ID);
    // Face-area numbers the QS runbook checks by hand (and CoreOp documents).
    assert!((FRONT_NET_M2 - (FRONT_GROSS_M2 - DOOR_DEDUCT_M2 - WIN_DEDUCT_M2)).abs() < 1e-9);
    assert!((TOTAL_NET_M2 - (FRONT_NET_M2 + BACK_NET_M2)).abs() < 1e-9);

    maybe_export(&bridge, &path, &scene);
    cleanup(&path);
}

#[test]
fn re_recording_the_checklist_does_not_double_count_rows() {
    let mut s = session();
    let scene = trust_scene(&mut s);
    link_project(&mut s);
    let path = temp("rerecord");
    record(&mut s, &path);
    let first = read_vdb(&path);
    let again = record(&mut s, &path);
    assert_eq!(again["counts"]["walls"], 2);
    assert_eq!(again["counts"]["openings"], 2);
    let second = read_vdb(&path);
    assert_eq!(rows_of(&first, 3).len(), rows_of(&second, 3).len());
    assert_eq!(rows_of(&first, 4).len(), rows_of(&second, 4).len());
    assert_eq!(rows_of(&second, 1).len(), 1, "still one header");
    assert_checklist_numbers(&second, &scene);
    cleanup(&path);
}

#[test]
fn second_checklist_drawing_on_same_project_and_floor_warns_double_count() {
    let path = temp("overlap");
    let mut a = session();
    trust_scene(&mut a);
    link_project(&mut a);
    let first = record(&mut a, &path);
    assert!(!first["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("double-count"))), "{first}");

    let mut b = session();
    trust_scene(&mut b);
    link_project(&mut b);
    let second = record(&mut b, &path);
    assert!(
        second["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("double-count") && t.contains("Ground"))),
        "second drawing on same project+floor must warn: {second}"
    );
    // Both drawings remain in the file (warn, do not block).
    let db = read_vdb(&path);
    assert_eq!(rows_of(&db, 1).len(), 2, "two headers");
    assert_eq!(rows_of(&db, 3).len(), 4, "two walls per drawing");
    cleanup(&path);
}

#[test]
fn opening_without_wall_ref_is_caught_and_thickness_regression_is_caught() {
    let mut s = session();
    let scene = trust_scene(&mut s);
    link_project(&mut s);
    let path = temp("regress");
    record(&mut s, &path);
    let db = read_vdb(&path);
    assert_checklist_numbers(&db, &scene);

    // Stray opening far from walls → no wall_ref (regression catcher for wall_ref logic).
    s.execute("wall.window", &json!({"at": [3000, 2000], "name": "W3", "width": WIN_W_MM, "angle": 0})).unwrap();
    let again = record(&mut s, &path);
    let db = read_vdb(&path);
    let stray: Vec<_> = rows_of(&db, 4).into_iter().filter(|o| o.get("wall_ref").is_none()).collect();
    assert_eq!(stray.len(), 1, "{stray:?}");
    assert!(again["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("not on a measured wall"))));

    // Nominal-only wall (no A-WALL faces) must keep 230 + honesty note — thickness regression catcher.
    let mut n = session();
    n.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]}))
        .unwrap();
    n.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut n, "MEASURE-FULLBRICK");
    n.execute("line", &json!({"points": [[0, 0], [WALL_LEN_MM, 0]]})).unwrap();
    n.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let npath = temp("nominal");
    let nr = record(&mut n, &npath);
    let ndb = read_vdb(&npath);
    let walls = rows_of(&ndb, 3);
    assert_eq!(walls.len(), 1);
    assert_eq!(walls[0]["thickness_mm"].as_f64(), Some(WALL_THICK_MM));
    assert!(walls[0]["_note"].as_str().is_some_and(|t| t.contains("nominal")), "{}", walls[0]);
    assert!(nr["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("nominal"))));
    cleanup(&path);
    cleanup(&npath);
}

//! Take-off phases: the estimation phase and running-bill phase are chosen in the drawing, stay on the dimension,
//! and come out on the exported rows that Falcon shows in its tables.

use aad_engine::Session;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
    s
}

fn export(s: &mut Session) -> Value {
    let r = s.execute("bridge.export", &json!({})).unwrap();
    serde_json::from_str(r["json"].as_str().unwrap()).unwrap()
}

/// A 9 × 6 m box of walls with a door and a window on the front wall, measured by layer.
fn measured_house(s: &mut Session) {
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]}))
        .unwrap();
    s.execute("openings.std", &json!({})).unwrap();
    s.execute("wall.draw", &json!({"points": [[0, 0], [9000, 0], [9000, 6000], [0, 6000], [0, 0]], "thickness": 230})).unwrap();
    s.execute("wall.door", &json!({"at": [2000, 0], "name": "D2", "width": 900})).unwrap();
    s.execute("wall.window", &json!({"at": [5000, 0], "name": "W3", "width": 1200})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    s.execute("layer.current", &json!({"name": "MEASURE-FULLBRICK"})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 6000], [9000, 6000]]})).unwrap();
}

#[test]
fn a_new_drawing_offers_the_default_phases_and_none_is_current() {
    let mut s = session();
    let p = s.execute("takeoff.phases", &json!({})).unwrap();
    assert_eq!(p["estimation"], json!(["Foundation", "Plinth", "Superstructure", "Finishing"]));
    assert_eq!(p["billing"], json!(["Phase 1", "Phase 2", "Phase 3"]));
    assert_eq!(p["current"], json!({"estimation": null, "billing": null}));
}

#[test]
fn the_lists_are_edited_cleaned_and_the_current_phase_must_be_in_them() {
    let mut s = session();
    let p = s
        .execute("takeoff.phases", &json!({"estimation": [" Site work ", "site work", "Frame", ""], "billing": ["RA 1"], "current": {"estimation": "FRAME", "billing": "ra 1"}}))
        .unwrap();
    assert_eq!(p["estimation"], json!(["Site work", "Frame"]), "trimmed, no duplicates, no blanks");
    assert_eq!(p["current"], json!({"estimation": "Frame", "billing": "RA 1"}), "stored with the list's own spelling");
    // A name outside the list is refused and nothing changes.
    assert!(s.execute("takeoff.phases", &json!({"current": {"estimation": "Roofing"}})).is_err());
    assert_eq!(s.execute("takeoff.phases", &json!({})).unwrap()["current"]["estimation"], "Frame");
    // Renaming the list drops a current phase that is no longer in it.
    let p = s.execute("takeoff.phases", &json!({"estimation": ["Roofing"]})).unwrap();
    assert_eq!(p["current"]["estimation"], Value::Null);
    // Hostile input is refused.
    for bad in
        [json!({"estimation": []}), json!({"estimation": [1]}), json!({"billing": "x"}), json!({"current": 3}), json!({"current": {"billing": 5}})]
    {
        assert!(s.execute("takeoff.phases", &bad).is_err(), "{bad}");
    }
    let many: Vec<String> = (0..200).map(|i| format!("P{i}")).collect();
    assert_eq!(s.execute("takeoff.phases", &json!({"estimation": many})).unwrap()["estimation"].as_array().unwrap().len(), 50);
}

#[test]
fn dimensions_drawn_after_choosing_a_phase_carry_it_and_it_can_be_changed() {
    let mut s = session();
    s.execute("takeoff.phases", &json!({"current": {"estimation": "Foundation", "billing": "Phase 1"}})).unwrap();
    let r = s.execute("takeoff.draw", &json!({"kind": "extWall", "points": [[0, 0], [5000, 0]]})).unwrap();
    assert_eq!((r["phase"].as_str(), r["billPhase"].as_str()), (Some("Foundation"), Some("Phase 1")));
    let h = r["handles"][0].as_str().unwrap().to_string();
    let mark = |s: &Session| s.doc().unwrap().entity(aad_engine::doc::Handle::parse_hex(&h).unwrap()).unwrap().common.takeoff.clone().unwrap();
    assert_eq!((mark(&s).phase.as_deref(), mark(&s).bill_phase.as_deref()), (Some("Foundation"), Some("Phase 1")));

    // A call can name its own, or clear them.
    let other =
        s.execute("takeoff.draw", &json!({"kind": "intWall", "points": [[0, 100], [5000, 100]], "phase": "Plinth", "billPhase": null})).unwrap();
    assert_eq!((other["phase"].as_str(), other["billPhase"].as_str()), (Some("Plinth"), None));
    assert!(s.execute("takeoff.draw", &json!({"kind": "extWall", "points": [[0, 0], [1, 0]], "phase": "Nonsense"})).is_err());

    // Change the phase of what is measured; unmarked objects are skipped, not guessed.
    s.execute("line", &json!({"points": [[0, 500], [100, 500]]})).unwrap();
    let plain = s.doc().unwrap().model.iter().last().unwrap().handle.hex();
    let r = s.execute("takeoff.phase", &json!({"estimation": "Superstructure", "billing": null, "handles": [h, plain]})).unwrap();
    assert_eq!((r["changed"].as_u64(), r["skipped"].as_u64()), (Some(1), Some(1)));
    assert_eq!((mark(&s).phase.as_deref(), mark(&s).bill_phase.as_deref()), (Some("Superstructure"), None));
    assert!(s.execute("takeoff.phase", &json!({"handles": [h]})).is_err(), "something to set is required");
    assert!(s.execute("takeoff.phase", &json!({"estimation": "Nonsense", "handles": [h]})).is_err());

    // One undo takes the change back.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(mark(&s).phase.as_deref(), Some("Foundation"));
}

#[test]
fn exported_rows_carry_the_phases_and_openings_follow_their_wall() {
    let mut s = session();
    measured_house(&mut s);
    s.execute("takeoff.phases", &json!({"current": {"estimation": "Superstructure", "billing": "Phase 2"}})).unwrap();
    s.execute("takeoff.maplayers", &json!({})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let f = export(&mut s);

    let walls = f["walls"].as_array().unwrap();
    assert!(!walls.is_empty(), "{f}");
    for w in walls {
        assert_eq!((w["phase"].as_str(), w["bill_phase"].as_str()), (Some("Superstructure"), Some("Phase 2")), "{w}");
    }
    let openings = f["openings"].as_array().unwrap();
    assert!(!openings.is_empty(), "{f}");
    for o in openings {
        assert!(o.get("wall_ref").is_some(), "{o}");
        assert_eq!(
            (o["phase"].as_str(), o["bill_phase"].as_str()),
            (Some("Superstructure"), Some("Phase 2")),
            "an opening is in its wall's phases: {o}"
        );
    }
}

#[test]
fn a_drawing_without_phases_exports_no_phase_columns() {
    let mut s = session();
    measured_house(&mut s);
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let f = export(&mut s);
    for w in f["walls"].as_array().unwrap() {
        assert!(w.get("phase").is_none() && w.get("bill_phase").is_none(), "{w}");
    }
}

#[test]
fn phases_are_mirrored_into_the_project_entry_that_dxf_keeps() {
    let mut s = session();
    s.execute("takeoff.phases", &json!({"current": {"estimation": "Plinth", "billing": "Phase 3"}})).unwrap();
    let r = s.execute("takeoff.draw", &json!({"kind": "extWall", "points": [[0, 0], [5000, 0]]})).unwrap();
    let h = r["handles"][0].as_str().unwrap().to_string();
    let doc = s.doc().unwrap();
    // The marks are mirrored into the project entry that a DXF round trip keeps.
    let meta = doc.project.get("takeoffMeta").expect("takeoffMeta");
    assert!(meta.contains(&h) && meta.contains("Plinth") && meta.contains("Phase 3"), "{meta}");
}

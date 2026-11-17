//! Projects of floor drawings joined by Xrefs: setup, floor switching, visibility, moving the folder.

use std::path::{Path, PathBuf};
use std::sync::Once;

use aad_engine::Session;
use aad_engine::doc::EntityKind;
use serde_json::{Value, json};

fn install_io() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
            read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
            plot: Some(|d, space, opts| aad_io::plot(d, space, opts).map_err(|e| e.to_string())),
        });
    });
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aad-proj-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn new_session() -> Session {
    install_io();
    Session::new()
}

fn setup(s: &mut Session, dir: &Path) -> Value {
    s.execute(
        "project.setup",
        &json!({
            "name": "Rao House", "folder": dir.to_str().unwrap(), "client": "S. Rao", "location": "Hospet",
            "floors": {"count": 2, "basements": 1, "terrace": true}, "categories": ["beam", "electrical"],
        }),
    )
    .unwrap()
}

fn overlays(s: &Session) -> Vec<String> {
    let mut v: Vec<String> = s.doc().unwrap().blocks.keys().filter(|k| k.starts_with("OVL|") && k.matches('|').count() == 1).cloned().collect();
    v.sort();
    v
}

fn doc_name(s: &Session) -> String {
    Path::new(s.state().unwrap().path.as_deref().unwrap()).file_name().unwrap().to_string_lossy().to_string()
}

fn block_names(path: &Path) -> Vec<String> {
    let mut s = new_session();
    s.execute("open", &json!({"path": path.to_str().unwrap()})).unwrap();
    s.doc().unwrap().blocks.keys().cloned().collect()
}

#[test]
fn setup_creates_the_folders_the_files_and_the_main_drawing_with_its_references() {
    let dir = temp("setup");
    let mut s = new_session();
    let r = setup(&mut s, &dir);
    let root = PathBuf::from(r["root"].as_str().unwrap());
    for f in [
        "project.aad.json",
        "Rao_House_Main.dxf",
        "Floors/Basement_1.dxf",
        "Floors/Ground_Floor.dxf",
        "Floors/First_Floor.dxf",
        "Floors/Terrace.dxf",
        "Layouts/Ground_Floor/Beam_Layout.dxf",
        "Layouts/First_Floor/Electrical_Layout.dxf",
        "Layouts/Terrace/Beam_Layout.dxf",
    ] {
        assert!(root.join(f).is_file(), "{f} is missing");
    }
    assert!(root.join("Exports").is_dir());
    assert_eq!(r["files"].as_array().unwrap().len(), 1 + 4 + 8);

    // The main drawing is open and shows every file as an xref at the same origin.
    assert_eq!(doc_name(&s), "Rao_House_Main.dxf");
    let l = s.execute("xref.list", &json!({})).unwrap();
    assert_eq!(l["xrefs"].as_array().unwrap().len(), 12);
    assert!(
        l["xrefs"].as_array().unwrap().iter().all(|x| x["status"] == "found" && !x["path"].as_str().unwrap().contains(':')),
        "paths are relative: {l}"
    );
    let at: Vec<(f64, f64)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some((i.insert.x, i.insert.y)) } else { None })
        .collect();
    assert!(at.iter().all(|p| *p == (0.0, 0.0)));

    // Floors sit at their elevations; the layouts start hidden, the floors shown.
    let f = s.execute("floor.list", &json!({})).unwrap();
    let elev: Vec<f64> = f["floors"].as_array().unwrap().iter().map(|x| x["elevation"].as_f64().unwrap()).collect();
    assert_eq!(elev, vec![-3.0, 0.0, 3.0, 6.0]);
    assert_eq!(f["active"], "Ground Floor");
    assert_eq!(f["floors"][1]["layouts"][0]["visible"], false);
    assert!(
        s.doc().unwrap().layer("Ground_Floor|A-WALL").unwrap().on
            || s.doc().unwrap().layers.iter().any(|l| l.name.starts_with("Ground_Floor|") && l.on)
    );
    assert!(s.doc().unwrap().layers.iter().filter(|l| l.name.starts_with("Ground_Floor_beam|")).all(|l| !l.on), "layouts start hidden");

    // Each floor file carries the project's data and a pointer to the manifest.
    let mut g = new_session();
    g.execute("open", &json!({"path": root.join("Layouts/Ground_Floor/Beam_Layout.dxf").to_str().unwrap()})).unwrap();
    assert!(g.doc().unwrap().layer("S-BEAM").is_some());
    assert_eq!(g.execute("project", &json!({})).unwrap()["details"]["owner"], "S. Rao");
    assert_eq!(g.doc().unwrap().project.get("projectManifest").map(String::as_str), Some("\"../../project.aad.json\""));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn setup_refuses_bad_input_and_an_existing_project() {
    let dir = temp("bad");
    let mut s = new_session();
    let f = dir.to_str().unwrap();
    for bad in [
        json!({"folder": f, "floors": {"count": 1}}),
        json!({"name": "X", "floors": {"count": 1}}),
        json!({"name": "X", "folder": f}),
        json!({"name": "X", "folder": f, "floors": []}),
        json!({"name": "X", "folder": f, "floors": [{"name": "A"}, {"name": "a"}]}),
        json!({"name": "X", "folder": f, "floors": [{"name": "A", "height": -3}]}),
        json!({"name": "X", "folder": f, "floors": [{"name": "A", "kind": "attic"}]}),
        json!({"name": "X", "folder": f, "floors": {"count": 500}}),
        json!({"name": "X", "folder": f, "floors": {"count": 1}, "units": "cubits"}),
        json!({"name": "X", "folder": f, "floors": {"count": 1}, "scale": 0}),
        json!({"name": "X", "folder": f, "floors": {"count": 1}, "categories": ["beam", "Beam"]}),
    ] {
        assert!(s.execute("project.setup", &bad).is_err(), "{bad}");
    }
    setup(&mut s, &dir);
    assert!(
        s.execute("project.setup", &json!({"name": "Rao House", "folder": f, "floors": {"count": 1}})).is_err(),
        "the folder already holds a project"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn activating_a_floor_opens_its_file_with_the_others_as_locked_overlays_and_new_work_goes_there() {
    let dir = temp("activate");
    let mut s = new_session();
    let root = PathBuf::from(setup(&mut s, &dir)["root"].as_str().unwrap());

    let r = s.execute("floor.activate", &json!({"name": "First Floor"})).unwrap();
    assert_eq!(doc_name(&s), "First_Floor.dxf");
    assert_eq!(r["overlays"].as_array().unwrap().len(), 3, "the other three floors; the layouts are hidden");
    assert_eq!(overlays(&s), vec!["OVL|Basement_1", "OVL|Ground_Floor", "OVL|Terrace"]);
    assert!(s.doc().unwrap().layers.iter().filter(|l| l.name.starts_with("OVL|")).all(|l| l.locked), "overlays cannot be edited");

    // A door drawn now lands in the First Floor file, and nowhere else.
    s.execute("openings.std", &json!({})).unwrap();
    s.execute("wall.draw", &json!({"points": [[0, 0], [6000, 0]], "thickness": 230})).unwrap();
    s.execute("wall.door", &json!({"at": [2000, 0], "name": "D2"})).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    assert!(block_names(&root.join("Floors/First_Floor.dxf")).iter().any(|b| b.starts_with("AADOPEN-")), "the door is in the first floor file");
    assert!(!block_names(&root.join("Floors/Ground_Floor.dxf")).iter().any(|b| b.starts_with("AADOPEN-")), "and not in the ground floor file");
    assert!(block_names(&root.join("Floors/First_Floor.dxf")).iter().all(|b| !b.starts_with("OVL|")), "overlays are never saved");

    // The active floor is recorded, and only it is unlocked.
    let l = s.execute("floor.list", &json!({})).unwrap();
    assert_eq!(l["active"], "First Floor");
    let locked: Vec<bool> = l["floors"].as_array().unwrap().iter().map(|f| f["locked"].as_bool().unwrap()).collect();
    assert_eq!(locked, vec![true, true, false, true]);

    // Switching back reopens (switches to) the ground floor file; the first floor stays open.
    s.execute("floor.activate", &json!({"name": "ground floor"})).unwrap();
    assert_eq!(doc_name(&s), "Ground_Floor.dxf");
    assert_eq!(s.docs.len(), 4, "the untitled drawing, main, first, ground");
    assert!(s.execute("floor.activate", &json!({"name": "Penthouse"})).is_err());
    assert!(s.execute("floor.activate", &json!({"name": "Ground Floor", "category": "plumbing"})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn floors_and_layouts_are_shown_frozen_locked_and_printed_independently() {
    let dir = temp("flags");
    let mut s = new_session();
    setup(&mut s, &dir);
    s.execute("floor.activate", &json!({"name": "First Floor"})).unwrap();

    // The ground floor's beam layout over the first floor's plan, without activating either.
    s.execute("floor.set", &json!({"name": "Ground Floor", "category": "beam", "visible": true})).unwrap();
    assert_eq!(overlays(&s), vec!["OVL|Basement_1", "OVL|Ground_Floor", "OVL|Ground_Floor_beam", "OVL|Terrace"]);
    // Hide the ground floor plan but keep its beam layout: they are independent.
    s.execute("floor.set", &json!({"name": "Ground Floor", "visible": false})).unwrap();
    assert_eq!(overlays(&s), vec!["OVL|Basement_1", "OVL|Ground_Floor_beam", "OVL|Terrace"]);
    // Freeze removes it from view; thaw brings it back.
    s.execute("floor.set", &json!({"name": "Basement 1", "frozen": true})).unwrap();
    assert!(!overlays(&s).contains(&"OVL|Basement_1".to_string()));
    s.execute("floor.set", &json!({"name": "Basement 1", "frozen": false})).unwrap();
    assert!(overlays(&s).contains(&"OVL|Basement_1".to_string()));
    // Print off: the overlay stays visible but is not plotted.
    s.execute("floor.set", &json!({"name": "Terrace", "print": false})).unwrap();
    let lay = |s: &Session, prefix: &str| {
        s.doc().unwrap().layers.iter().filter(|l| l.name.starts_with(prefix)).map(|l| (l.on, l.plot, l.locked)).collect::<Vec<_>>()
    };
    assert!(lay(&s, "OVL|Terrace|").iter().all(|&(on, plot, locked)| on && !plot && locked));
    assert!(lay(&s, "OVL|Basement_1|").iter().all(|&(_, plot, _)| plot));

    // The main drawing (open in another tab) got the same flags on its real xref layers.
    let main = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| p.ends_with("Main.dxf"))).unwrap();
    let flags: Vec<(bool, bool)> = s.docs[main].doc.layers.iter().filter(|l| l.name.starts_with("Terrace|")).map(|l| (l.on, l.plot)).collect();
    assert!(!flags.is_empty() && flags.iter().all(|&(on, plot)| on && !plot));
    let gp: Vec<bool> = s.docs[main].doc.layers.iter().filter(|l| l.name.starts_with("Ground_Floor|")).map(|l| l.on).collect();
    assert!(gp.iter().all(|on| !on), "the hidden ground floor is off in the main drawing too");

    // Activating a layout opens its file with the floor's plan underneath as an overlay.
    s.execute("floor.set", &json!({"name": "Ground Floor", "visible": true})).unwrap();
    s.execute("floor.activate", &json!({"name": "Ground Floor", "category": "beam"})).unwrap();
    assert_eq!(doc_name(&s), "Beam_Layout.dxf");
    assert!(overlays(&s).contains(&"OVL|Ground_Floor".to_string()) && !overlays(&s).contains(&"OVL|Ground_Floor_beam".to_string()));

    assert!(s.execute("floor.set", &json!({"name": "Ground Floor"})).is_err(), "nothing to set");
    assert!(s.execute("floor.set", &json!({"name": "Ground Floor", "visible": "yes"})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_moved_project_keeps_its_structure_and_visibility() {
    let dir = temp("moved");
    let mut s = new_session();
    let root = PathBuf::from(setup(&mut s, &dir)["root"].as_str().unwrap());
    s.execute("floor.set", &json!({"name": "Ground Floor", "category": "beam", "visible": true, "print": false})).unwrap();
    s.execute("floor.set", &json!({"name": "Terrace", "frozen": true})).unwrap();
    s.execute("floor.activate", &json!({"name": "First Floor"})).unwrap();
    drop(s);

    let moved = dir.with_file_name(format!("{}-moved", dir.file_name().unwrap().to_string_lossy()));
    std::fs::remove_dir_all(&moved).ok();
    std::fs::rename(&dir, &moved).unwrap();
    let root = moved.join(root.file_name().unwrap());

    let mut again = new_session();
    let r = again.execute("project.open", &json!({"path": root.to_str().unwrap()})).unwrap();
    assert_eq!(r["floors"], 4);
    assert_eq!(doc_name(&again), "Rao_House_Main.dxf");
    let l = again.execute("xref.list", &json!({})).unwrap();
    assert!(l["xrefs"].as_array().unwrap().iter().all(|x| x["status"] == "found"), "every reference still resolves: {l}");
    let f = again.execute("floor.list", &json!({})).unwrap();
    assert_eq!(f["active"], "First Floor");
    assert_eq!((f["floors"][1]["layouts"][0]["visible"].as_bool(), f["floors"][1]["layouts"][0]["print"].as_bool()), (Some(true), Some(false)));
    assert_eq!(f["floors"][3]["frozen"], true);
    // The saved visibility is applied to the main drawing on open.
    assert!(again.doc().unwrap().layers.iter().filter(|l| l.name.starts_with("Terrace|")).all(|l| l.frozen));
    assert!(again.doc().unwrap().layers.iter().filter(|l| l.name.starts_with("Ground_Floor_beam|")).all(|l| l.on && !l.plot));
    // Opening via the main drawing's path finds the project too, and activation still works.
    let mut third = new_session();
    third.execute("project.open", &json!({"path": root.join("Rao_House_Main.dxf").to_str().unwrap()})).unwrap();
    third.execute("floor.activate", &json!({"name": "Ground Floor"})).unwrap();
    assert_eq!(doc_name(&third), "Ground_Floor.dxf");
    assert!(
        overlays(&third).contains(&"OVL|First_Floor".to_string()) && !overlays(&third).contains(&"OVL|Terrace".to_string()),
        "frozen stays frozen"
    );
    std::fs::remove_dir_all(&moved).ok();
}

#[test]
fn a_damaged_or_hostile_project_file_is_refused() {
    let dir = temp("hostile");
    let mut s = new_session();
    let root = PathBuf::from(setup(&mut s, &dir)["root"].as_str().unwrap());
    let manifest = root.join("project.aad.json");
    let good = std::fs::read_to_string(&manifest).unwrap();
    let mut m: Value = serde_json::from_str(&good).unwrap();
    s.execute("floor.activate", &json!({"name": "Ground Floor"})).unwrap();

    m["floors"][0]["file"] = json!("../../outside.dxf");
    std::fs::write(&manifest, m.to_string()).unwrap();
    assert!(s.execute("floor.list", &json!({})).is_err(), "a path that leaves the project is refused");
    m["floors"][0]["file"] = json!("C:/Windows/x.dxf");
    std::fs::write(&manifest, m.to_string()).unwrap();
    assert!(s.execute("floor.list", &json!({})).is_err());
    std::fs::write(&manifest, "{not json").unwrap();
    assert!(s.execute("floor.list", &json!({})).is_err());
    std::fs::write(&manifest, json!({"format": "other"}).to_string()).unwrap();
    assert!(s.execute("floor.list", &json!({})).is_err());
    std::fs::write(&manifest, &good).unwrap();
    assert!(s.execute("floor.list", &json!({})).is_ok(), "the good file works again");

    // A drawing that is not in a project says so.
    let mut lone = new_session();
    assert!(lone.execute("floor.list", &json!({})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

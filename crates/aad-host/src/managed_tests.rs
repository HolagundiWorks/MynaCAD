//! Tests of managed mode (`managed.rs`): the policy, the configuration apply when idle, and import inside the project.

use std::path::Path;

use serde_json::{Value, json};

use crate::Host;
use crate::managed::{OPEN_REFUSED, refuse, same_file};

fn levels(items: Value, next: u32) -> Value {
    json!({ "schema": { "v": 1, "min_reader": 1 }, "rev": { "v": 1, "rev": 1, "app": "mynaops", "time": "2026-11-15T09:30:12Z" },
            "units": { "v": 1, "drawing_unit": "mm" },
            "levels": { "v": 1, "next_cid": next, "items": items } })
}

fn write_config(path: &Path, c: &Value) {
    let read = aad_odp::config::ProjectConfig::from_json(c).unwrap();
    let mut odp = aad_odp::Odp::open(path).unwrap();
    aad_odp::config::write(&mut odp, &read.config, "mynaops").unwrap();
}

fn names(host: &mut Host) -> Vec<String> {
    let r = host.execute("level.list", &json!({})).unwrap();
    r["levels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
}

fn open_managed(host: &mut Host) {
    host.open_managed_start().unwrap();
    while host.open_poll().unwrap() == 1 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn managed_host_refuses_opening_applies_configuration_and_imports_inside_the_project() {
    let dir = std::env::temp_dir().join(format!("aad-host-managed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let project = dir.join("house.osp");
    // A project as MynaOps would leave it: a drawing and a configuration with two levels.
    let mut maker = Host::create(false);
    maker.execute("drawing.units", &json!({ "units": "mm", "mode": "metadata" })).unwrap();
    maker.execute("line", &json!({ "points": [[0.0, 0.0], [3000.0, 0.0]] })).unwrap();
    maker.execute("saveas", &json!({ "path": project.to_string_lossy() })).unwrap();
    let two = json!([{ "cid": 1, "name": "Ground", "elevation_mm": 0.0, "height_mm": 3000.0 }, { "cid": 2, "name": "First", "elevation_mm": 3000.0, "height_mm": 3000.0 }]);
    write_config(&project, &levels(two, 3));
    // A foreign drawing to import.
    let dxf = src.join("plan.dxf");
    let mut other = Host::create(false);
    other.execute("circle", &json!({ "center": [500.0, 500.0], "radius": 200.0 })).unwrap();
    other.execute("saveas", &json!({ "path": dxf.to_string_lossy() })).unwrap();

    let mut host = Host::create(false);
    assert!(host.set_managed(&dxf.to_string_lossy(), None).is_err(), "a drawing is not a project");
    assert!(host.set_managed("relative.osp", None).is_err());
    assert!(host.set_managed(&dir.join("missing.osp").to_string_lossy(), None).is_err());
    assert_eq!(host.managed_info()["managed"], false);
    host.set_managed(&project.to_string_lossy(), Some(4242)).unwrap();
    assert_eq!(host.managed_info()["opsPid"], 4242);

    // Every way of opening another file is refused with the one message; the project itself opens through the managed call.
    let refused = host.control_one("app.open", &json!({ "path": project.to_string_lossy() }));
    assert_eq!((refused["ok"].clone(), refused["error"].as_str().unwrap_or("")), (json!(false), OPEN_REFUSED), "{refused}");
    assert!(host.open_start(&dxf.to_string_lossy()).is_err());
    assert!(host.execute("open", &json!({ "path": dxf.to_string_lossy() })).unwrap_err().to_string().contains(OPEN_REFUSED));
    open_managed(&mut host);
    assert!(host.session.state().unwrap().path.as_deref().is_some_and(|p| same_file(p, &project)));
    assert!(host.execute("new", &json!({})).is_err());
    assert!(host.execute("saveas", &json!({ "path": dir.join("copy.osp").to_string_lossy() })).is_err());
    assert!(host.save_start(Some(&dir.join("copy.osp").to_string_lossy())).is_err());
    assert!(host.save_start(Some(&dir.join("notes.txt").to_string_lossy())).is_err());
    assert!(host.app("backup.restore", &json!({ "backup": "x", "path": "y" })).is_err());
    for (id, params) in [
        ("level.new", json!({ "name": "Roof" })),
        ("level.delete", json!({ "level": "First" })),
        ("level.set", json!({ "level": "First", "name": "Renamed" })),
        ("hcwlayers", json!({ "set": "standard" })),
        ("sheetset.new", json!({ "name": "X" })),
        ("project", json!({ "details": { "title": "x" } })),
        ("project.wizard", json!({})),
        ("level.import", json!({ "path": dxf.to_string_lossy() })),
    ] {
        let e = host.execute(id, &params).unwrap_err().to_string();
        assert!(e.contains("MynaOps"), "{id}: {e}");
    }
    let typed = host.session.start("level.new").unwrap_err().to_string();
    assert!(typed.contains("MynaOps"), "a command typed at the command line is refused too: {typed}");
    assert!(!dir.join("copy.osp").exists());

    // The configuration is applied when idle, not before.
    let waiting = host.config_poll(&json!({ "idle": false })).unwrap();
    assert_eq!(waiting["state"], "waiting", "{waiting}");
    assert_eq!(waiting["text"], "Configuration update waiting");
    let applied = host.config_poll(&json!({ "idle": true })).unwrap();
    assert_eq!((applied["state"].clone(), applied["result"].clone()), (json!("applied"), json!("ok")), "{applied}");
    assert_eq!(names(&mut host), ["Ground", "First"]);
    assert_eq!(host.config_poll(&json!({ "idle": true })).unwrap()["state"], "current");
    // It was recorded for MynaOps.
    let recorded = aad_odp::config::probe(&project).unwrap();
    assert_eq!(recorded.applied.as_ref().map(|a| (a.rev, a.result.as_str())), Some((1, "ok")));
    // MynaOps renames a level and adds one (a second revision): it arrives by id, and a running command holds it back.
    let three = json!([{ "cid": 1, "name": "Ground", "elevation_mm": 0.0, "height_mm": 3000.0 }, { "cid": 2, "name": "Level 1", "elevation_mm": 3000.0, "height_mm": 3000.0 },
                       { "cid": 3, "name": "Roof", "elevation_mm": 6000.0, "height_mm": 1000.0 }]);
    write_config(&project, &levels(three, 4));
    host.session.start("line").unwrap();
    assert_eq!(host.config_poll(&json!({ "idle": true })).unwrap()["state"], "waiting");
    host.session.cancel();
    let applied = host.config_poll(&json!({ "idle": true })).unwrap();
    assert_eq!(applied["state"], "applied", "{applied}");
    assert_eq!(names(&mut host), ["Ground", "Level 1", "Roof"]);
    assert_eq!(aad_odp::config::probe(&project).unwrap().applied.map(|a| a.rev), Some(2));

    // Import: the drawing becomes content of the project; nothing appears beside the source or next to the project.
    let listing = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    };
    let src_before = listing(&src);
    let before = host.stats().entities;
    host.execute("import", &json!({ "path": dxf.to_string_lossy() })).unwrap();
    assert!(host.stats().entities > before);
    assert_eq!(host.control_one("app.save", &json!({}))["ok"], true);
    assert_eq!(listing(&src), src_before, "nothing is written beside the source");
    assert!(!listing(&src).iter().any(|n| n.ends_with(".osp") && n != "plan.dxf.history.osp"), "no working copy beside the source");
    assert!(!dir.join("plan.osp").exists());
    assert!(listing(&dir).iter().all(|n| !n.starts_with("plan")), "nothing named after the import in the project folder: {:?}", listing(&dir));
    // Export is still a copy.
    let out = dir.join("out.dxf");
    assert_eq!(host.control_one("app.save", &json!({ "path": out.to_string_lossy() }))["ok"], true);
    assert!(out.is_file());
    assert!(host.session.state().unwrap().path.as_deref().is_some_and(|p| same_file(p, &project)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unadopted_project_is_not_configured_by_the_host() {
    let dir = std::env::temp_dir().join(format!("aad-host-unadopted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.join("old.osp");
    let mut maker = Host::create(false);
    maker.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
    maker.execute("saveas", &json!({ "path": project.to_string_lossy() })).unwrap();
    let mut host = Host::create(false);
    host.set_managed(&project.to_string_lossy(), None).unwrap();
    open_managed(&mut host);
    let first = host.config_poll(&json!({ "idle": true })).unwrap();
    assert_eq!((first["state"].clone(), first["text"].clone()), (json!("unadopted"), json!("Adopt this project in MynaOps first")), "{first}");
    assert_eq!(first["recorded"], true, "the proposal is written for the Adopt page of MynaOps");
    assert!(aad_odp::config::probe(&project).unwrap().proposal.is_some());
    assert_eq!(host.config_poll(&json!({ "idle": true })).unwrap()["recorded"], false, "offered once per run");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_managed_parameters_never_panic() {
    for command in ["open", "level.set", "project", "saveas", "qsave", "level.import", "", "\u{0}", "project.config.apply"] {
        for params in [
            json!(null),
            json!(1),
            json!("x"),
            json!([1, 2]),
            json!({ "path": 5 }),
            json!({ "path": null }),
            json!({ "name": {}, "sheetSet": [] }),
            json!({ "path": "\u{0}.osp" }),
        ] {
            let _ = refuse(command, &params);
        }
    }
    assert!(refuse("project.config.apply", &json!({})).is_none(), "the apply command is the one way the configuration arrives");
    assert!(refuse("import", &json!({ "path": "a.dxf" })).is_none());
    assert!(refuse("level.set", &json!({ "level": "A", "traces": true })).is_none(), "view settings of a level stay");
    let mut host = Host::create(false);
    for bad in ["", "\u{0}", &"x".repeat(5000), "C:\\no\\such\\file.osp", "rel.osp"] {
        assert!(host.set_managed(bad, None).is_err());
    }
    assert!(host.open_managed_start().is_err());
}

/// Not a test: makes the stub project the hand runs of the shell use (`AAD_STUB_PROJECT` is the file to make, `AAD_STUB_REV` 1 or 2: 2 renames a level
/// and adds one, as MynaOps would in a second revision). Run with `cargo test -p aad-host --lib stub_project -- --ignored`.
#[test]
#[ignore]
fn stub_project() {
    let Ok(path) = std::env::var("AAD_STUB_PROJECT") else { return };
    let path = std::path::PathBuf::from(path);
    let rev = std::env::var("AAD_STUB_REV").unwrap_or_else(|_| "1".into());
    if !path.exists() {
        let mut maker = Host::create(false);
        maker.execute("line", &json!({ "points": [[0.0, 0.0], [3000.0, 0.0]] })).unwrap();
        maker.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
    }
    let items = if rev == "1" {
        json!([{ "cid": 1, "name": "Ground", "elevation_mm": 0.0, "height_mm": 3000.0 }, { "cid": 2, "name": "First", "elevation_mm": 3000.0, "height_mm": 3000.0 }])
    } else {
        json!([{ "cid": 1, "name": "Ground", "elevation_mm": 0.0, "height_mm": 3000.0 }, { "cid": 2, "name": "Level 1", "elevation_mm": 3000.0, "height_mm": 3000.0 },
               { "cid": 3, "name": "Roof", "elevation_mm": 6000.0, "height_mm": 1000.0 }])
    };
    write_config(&path, &levels(items, if rev == "1" { 3 } else { 4 }));
}

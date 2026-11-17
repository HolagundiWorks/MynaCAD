//! Migration into levels: `level.import`, `level.export`, `project.merge`.

use std::path::{Path, PathBuf};
use std::sync::Once;

use aad_engine::Session;
use aad_engine::cmd::file::{IoHooks, set_io};
use aad_engine::doc::{AssocSnap, EntityKind};
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

fn temp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("aad-migrate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A session whose drawing is in `unit` ("mm", "m").
fn session(unit: &str) -> Session {
    install_io();
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": unit, "mode": "metadata"})).unwrap();
    s
}

/// Write a drawing built by `build` to `path`.
fn write_file(path: &Path, unit: &str, build: impl FnOnce(&mut Session)) {
    let mut s = session(unit);
    build(&mut s);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, aad_io::write(s.doc().unwrap(), "x.dxf").unwrap()).unwrap();
}

fn line(s: &mut Session, y: f64) {
    s.execute("line", &json!({"points": [[0.0, y], [10.0, y]]})).unwrap();
}

fn names(s: &Session) -> Vec<String> {
    s.doc().unwrap().levels.iter().map(|l| l.name.clone()).collect()
}

fn objects(s: &Session, level: &str) -> usize {
    let d = s.doc().unwrap();
    d.level_store(d.level_by_name(level).unwrap().id).unwrap().len()
}

// ---------------- level.import ----------------

#[test]
fn layouts_of_a_foreign_file_are_not_imported_and_are_named_in_the_report() {
    use aad_engine::doc::{Common, Line, Space};
    use aad_geom::Vec3;
    let dir = temp("layouts");
    let file = dir.join("With sheets.dxf");
    write_file(&file, "mm", |s| {
        line(s, 0.0);
        // Layout1 stays empty (every DXF has one); Layout2 holds a paper annotation.
        let d = s.doc_mut().unwrap();
        d.add(
            &Space::Paper("Layout2".into()),
            Common::default(),
            aad_engine::doc::EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(5.0, 0.0, 0.0) }),
        )
        .unwrap();
        if d.layout("Layout2").is_none() {
            panic!("the drawing has no Layout2 to hold the paper line");
        }
    });
    let mut s = session("mm");
    line(&mut s, 50.0);
    let r = s.execute("level.import", &json!({"path": file.to_string_lossy(), "name": "Sheets"})).unwrap();
    assert_eq!(r["objects"], 1, "only model space is imported");
    let listed = r["layoutsNotImported"].as_array().expect("the report lists the skipped layouts");
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["name"], "Layout2");
    assert_eq!(listed[0]["status"], "not imported: sheets are made in MynaOps");
    assert!(s.doc().unwrap().layouts.iter().all(|l| l.entities.is_empty()), "no layout content came along");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_becomes_a_level_with_fresh_handles_and_its_dimension_still_attached() {
    let dir = temp("import");
    let file = dir.join("Basement plan.dxf");
    write_file(&file, "mm", |s| {
        line(s, 0.0);
        s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 2]})).unwrap();
    });
    let mut s = session("mm");
    line(&mut s, 50.0);
    line(&mut s, 60.0);
    let r = s.execute("level.import", &json!({"path": file.to_string_lossy(), "elevation": -3000.0, "height": 3000.0})).unwrap();
    assert_eq!(r["level"], "Basement plan");
    assert_eq!(names(&s), ["Level 1", "Basement plan"], "the existing drawing became the first level");
    assert_eq!((objects(&s, "Level 1"), objects(&s, "Basement plan")), (2, 2));
    let d = s.doc().unwrap();
    let b = d.level_by_name("Basement plan").unwrap();
    assert_eq!((b.elevation, b.height), (-3000.0, 3000.0));

    // Handles are fresh: nothing collides with the existing drawing, and the count is drawing-wide.
    assert_eq!(d.entity_count(), 4);
    let imported = d.level_store(b.id).unwrap();
    let handles: std::collections::BTreeSet<_> = d.model_stores().flat_map(|st| st.iter().map(|e| e.handle)).collect();
    assert_eq!(handles.len(), 4, "every handle in the drawing is unique");
    // The dimension is attached to the imported line, not to a dangling handle.
    let line_h = imported.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
    let dim = imported.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.clone()) } else { None }).unwrap();
    assert!(!dim.assoc.is_empty() && dim.assoc.iter().all(|a| a.handle == line_h), "association follows the new handle: {:?}", dim.assoc);
    assert!(dim.assoc.iter().all(|a| !matches!(a.snap, AssocSnap::Intersection { .. })));

    s.execute("undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().has_levels(), "one undo step takes the whole import back");
    assert_eq!(s.doc().unwrap().entity_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blocks_and_layers_come_along_and_existing_definitions_win() {
    let dir = temp("blocks");
    let file = dir.join("lvl.dxf");
    write_file(&file, "mm", |s| {
        s.execute("layer.new", &json!({"name": "S-COL"})).unwrap();
        s.execute("circle", &json!({"center": [0, 0], "radius": 1.0})).unwrap();
        let h = s.doc().unwrap().model.iter().next().unwrap().handle;
        s.execute("block", &json!({"name": "COL", "base": [0, 0], "handles": [h.hex()], "keep": "delete"})).unwrap();
        s.execute("insert", &json!({"name": "COL", "at": [5, 5]})).unwrap();
    });
    let mut s = session("mm");
    s.execute("layer.new", &json!({"name": "S-COL", "color": 3})).ok();
    s.execute("level.import", &json!({"path": file.to_string_lossy(), "name": "Upper"})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("COL").is_some(), "the block the level uses is available");
    assert!(d.layer("S-COL").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_in_other_units_are_refused_unless_told_otherwise_and_a_blank_drawing_adopts_them() {
    let dir = temp("units");
    let file = dir.join("m.dxf");
    write_file(&file, "m", |s| line(s, 0.0));
    let mut s = session("mm");
    line(&mut s, 1.0);
    let err = s.execute("level.import", &json!({"path": file.to_string_lossy()})).unwrap_err().to_string();
    assert!(err.contains("units"), "{err}");
    assert!(!s.doc().unwrap().has_levels(), "nothing changed");
    s.execute("level.import", &json!({"path": file.to_string_lossy(), "ignoreUnits": true})).unwrap();
    assert_eq!(s.doc().unwrap().levels.len(), 2);

    // A blank drawing (even a new inch one) takes the first file's unit instead of refusing it.
    install_io();
    let mut blank = Session::new();
    blank.execute("level.import", &json!({"path": file.to_string_lossy()})).unwrap();
    assert_eq!(aad_engine::annot::drawing_unit(blank.doc().unwrap()).map(|u| u.metres()), Some(1.0));
    assert_eq!(names(&blank), ["m"], "and the file is the first level, not a second one after an empty one");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_can_be_imported_as_a_sheet_sets_annotation_of_a_level() {
    let dir = temp("ann");
    let file = dir.join("notes.dxf");
    write_file(&file, "mm", |s| {
        s.execute("text", &json!({"at": [1.0, 1.0], "text": "DB", "height": 0.5})).unwrap();
        s.execute("text", &json!({"at": [3.0, 1.0], "text": "SW", "height": 0.5})).unwrap();
    });
    let mut s = session("mm");
    line(&mut s, 0.0);
    s.execute("sheetset.new", &json!({"preset": true})).unwrap();
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    let r = s.execute("level.import", &json!({"path": file.to_string_lossy(), "sheetSet": "Electrical", "level": "First"})).unwrap();
    assert_eq!((r["sheetSet"].as_str(), r["level"].as_str(), r["objects"].as_u64()), (Some("Electrical"), Some("First"), Some(2)));
    assert_eq!(names(&s), ["Ground", "First"], "no new level");
    let d = s.doc().unwrap();
    let (set, first) = (d.sheet_set_by_name("Electrical").unwrap().id, d.level_by_name("First").unwrap().id);
    assert_eq!(d.annotation_store(set, first).map(aad_engine::doc::EntityStore::len), Some(2));
    assert_eq!(objects(&s, "First"), 0, "the level's own geometry is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_imports_change_nothing() {
    let dir = temp("bad");
    let file = dir.join("ok.dxf");
    write_file(&file, "mm", |s| line(s, 0.0));
    let junk = dir.join("junk.dxf");
    std::fs::write(&junk, b"this is not a drawing").unwrap();
    let mut s = session("mm");
    line(&mut s, 5.0);
    s.execute("level.new", &json!({"name": "Roof", "first": "Ground"})).unwrap();
    let before = s.doc().unwrap().clone();
    let p = file.to_string_lossy().to_string();
    for params in [
        json!({}),
        json!({"path": ""}),
        json!({"path": "a\0b"}),
        json!({"path": dir.join("missing.dxf").to_string_lossy()}),
        json!({"path": junk.to_string_lossy()}),
        json!({"path": p, "name": "Roof"}),
        json!({"path": p, "name": ""}),
        json!({"path": p, "after": "Nope"}),
        json!({"path": p, "sheetSet": "Nope"}),
        json!({"path": p, "sheetSet": "Electrical", "level": "Nope"}),
        json!({"path": p, "elevation": "high"}),
        json!({"path": p, "height": -1}),
    ] {
        assert!(s.execute("level.import", &params).is_err(), "{params}");
        assert_eq!(*s.doc().unwrap(), before, "{params} left the drawing as it was");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------- level.export ----------------

#[test]
fn a_level_exports_as_a_plain_file_that_imports_back() {
    let dir = temp("export");
    let mut s = session("mm");
    line(&mut s, 0.0);
    s.execute("level.new", &json!({"name": "First", "first": "Ground"})).unwrap();
    s.execute("level.activate", &json!({"level": "First"})).unwrap();
    line(&mut s, 5.0);
    line(&mut s, 6.0);
    let out = dir.join("first.dxf");
    let r = s.execute("level.export", &json!({"path": out.to_string_lossy()})).unwrap();
    assert_eq!(r["objects"], 2, "the active level by default");
    let plain = aad_io::read(&std::fs::read(&out).unwrap(), "x.dxf").unwrap();
    assert!(plain.levels.is_empty() && plain.sheet_sets.is_empty());
    assert_eq!(plain.model.len(), 2);

    assert!(s.execute("level.export", &json!({"path": out.to_string_lossy()})).is_err(), "no silent overwrite");
    s.execute("level.export", &json!({"level": "Ground", "path": out.to_string_lossy(), "overwrite": true})).unwrap();
    assert_eq!(aad_io::read(&std::fs::read(&out).unwrap(), "x.dxf").unwrap().model.len(), 1);
    assert!(s.execute("level.export", &json!({"path": dir.to_string_lossy()})).is_err(), "a folder is not a file");
    assert!(s.execute("level.export", &json!({"level": "Nope", "path": out.to_string_lossy()})).is_err());

    let mut one = session("mm");
    line(&mut one, 0.0);
    assert!(one.execute("level.export", &json!({"path": dir.join("one.dxf").to_string_lossy()})).is_err(), "one level: just save");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------- project.merge ----------------

/// A real project on disk: Ground Floor and First Floor, ceiling and electrical layouts. Content:
/// a wall on each floor, a note only on the ground ceiling layout, a pipe-like line on the ground
/// electrical layout, nothing on the first floor's layouts.
fn project(dir: &Path) -> PathBuf {
    install_io();
    let mut s = Session::new();
    let r = s
        .execute(
            "project.setup",
            &json!({
                "name": "Rao House", "folder": dir.to_string_lossy(), "units": "metric", "drawingUnits": "mm",
                "floors": [{"name": "Ground Floor", "kind": "ground", "height": 3.0}, {"name": "First Floor", "height": 3.2}],
                "categories": ["ceiling", "electrical"], "open": false,
            }),
        )
        .unwrap();
    let root = PathBuf::from(r["root"].as_str().unwrap());
    let m: Value = serde_json::from_str(&std::fs::read_to_string(root.join("project.aad.json")).unwrap()).unwrap();
    let floors = m["floors"].as_array().unwrap();
    for (i, f) in floors.iter().enumerate() {
        write_file(&root.join(f["file"].as_str().unwrap()), "mm", |s| line(s, i as f64 * 7.0));
    }
    let layout = |floor: usize, id: &str| {
        root.join(floors[floor]["layouts"].as_array().unwrap().iter().find(|l| l["id"] == id).unwrap()["file"].as_str().unwrap())
    };
    write_file(&layout(0, "ceiling"), "mm", |s| {
        s.execute("text", &json!({"at": [1.0, 1.0], "text": "FC 2700", "height": 0.5})).unwrap();
    });
    write_file(&layout(0, "electrical"), "mm", |s| line(s, 3.0));
    root
}

#[test]
fn a_floor_project_merges_into_levels_and_sheet_sets_in_one_undo_step() {
    let dir = temp("merge");
    let root = project(&dir.join("proj"));
    let mut s = session("mm");
    let r = s.execute("project.merge", &json!({"path": root.to_string_lossy()})).unwrap();

    assert_eq!(
        names(&s),
        ["Ground Floor", "Ground Floor - Electrical Layout", "First Floor"],
        "a layout that holds geometry sits right after its floor, and there is no empty first level"
    );
    let d = s.doc().unwrap();
    let ground = d.level_by_name("Ground Floor").unwrap();
    let first = d.level_by_name("First Floor").unwrap();
    assert_eq!((ground.elevation, ground.height), (0.0, 3000.0), "metres became drawing units (millimetres)");
    assert_eq!((first.elevation, first.height), (3000.0, 3200.0));
    assert_eq!(d.level_by_name("Ground Floor - Electrical Layout").unwrap().elevation, 0.0, "a layout level sits at its floor");

    // The note-only ceiling layout is the False ceiling annotation of the ground floor.
    let set = d.sheet_set_by_name("False ceiling").expect("created from the layout's category");
    assert_eq!(d.annotation_store(set.id, ground.id).map(aad_engine::doc::EntityStore::len), Some(1));
    assert!(r["sheetSets"].as_array().unwrap().contains(&json!("False ceiling")));
    assert_eq!((objects(&s, "Ground Floor"), objects(&s, "First Floor")), (1, 1));

    // Empty layouts are reported, not imported.
    let skipped: Vec<_> = r["skipped"].as_array().unwrap().iter().map(|x| x["reason"].as_str().unwrap()).collect();
    assert!(skipped.iter().all(|r| *r == "the layout is empty"), "{skipped:?}");
    assert_eq!(skipped.len(), 2, "the first floor's ceiling and electrical layouts are empty");

    // The floor list travels with it, so the levels and floors stay in step.
    assert!(s.execute("floor.current", &json!({})).is_ok());

    s.execute("undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().has_levels() && s.doc().unwrap().sheet_sets.is_empty(), "one undo step");
    assert_eq!(s.doc().unwrap().entity_count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merging_can_keep_layouts_as_levels_and_a_project_found_from_a_folder_or_a_file() {
    let dir = temp("merge2");
    let root = project(&dir.join("proj"));
    let mut s = session("mm");
    s.execute("project.merge", &json!({"path": root.join("project.aad.json").to_string_lossy(), "annotation": false})).unwrap();
    assert!(names(&s).iter().any(|n| n == "Ground Floor - Ceiling Layout"), "annotation: false keeps the ceiling layout as a level: {:?}", names(&s));
    assert!(s.doc().unwrap().sheet_sets.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merging_is_refused_into_a_main_drawing_and_for_bad_projects_and_changes_nothing() {
    let dir = temp("merge3");
    let root = project(&dir.join("proj"));
    let mut s = session("mm");
    // A drawing with an Xref looks like the project's main drawing.
    {
        let d = s.doc_mut().unwrap();
        let mut b = aad_engine::doc::Block::new("Ground_Floor");
        b.xref_path = Some("Floors/Ground_Floor.dxf".into());
        d.blocks.insert("Ground_Floor".into(), std::sync::Arc::new(b));
    }
    let before = s.doc().unwrap().clone();
    assert!(s.execute("project.merge", &json!({"path": root.to_string_lossy()})).is_err());
    assert_eq!(*s.doc().unwrap(), before);

    let mut s = session("mm");
    let before = s.doc().unwrap().clone();
    let bad_manifest = dir.join("bad.aad.json");
    std::fs::write(&bad_manifest, b"{\"format\": \"nope\"}").unwrap();
    for path in [
        json!(null),
        json!(""),
        json!("a\0b"),
        json!(dir.join("nowhere").to_string_lossy()),
        json!(bad_manifest.to_string_lossy()),
        json!(dir.to_string_lossy()),
    ] {
        assert!(s.execute("project.merge", &json!({"path": path})).is_err(), "{path}");
        assert_eq!(*s.doc().unwrap(), before, "{path} changed nothing");
    }
    // A unit mismatch stops the whole merge, not just one floor.
    let mut metres = session("m");
    line(&mut metres, 0.0);
    let before = metres.doc().unwrap().clone();
    assert!(metres.execute("project.merge", &json!({"path": root.to_string_lossy()})).is_err());
    assert_eq!(*metres.doc().unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_layer_map_puts_a_files_objects_on_standard_layers_and_the_plan_suggests_them() {
    let dir = temp("layermap");
    let file = dir.join("Plan.dxf");
    write_file(&file, "mm", |s| {
        for l in ["AR_WALLS", "ST_COLUMNS", "N_TXT", "Odd Thing"] {
            s.execute("layer.new", &json!({"name": l})).unwrap();
        }
        for (i, l) in ["AR_WALLS", "AR_WALLS", "ST_COLUMNS", "N_TXT", "Odd Thing"].iter().enumerate() {
            s.execute("layer.current", &json!({"name": l})).unwrap();
            line(s, i as f64);
        }
    });
    let mut s = session("mm");
    let plan = s.execute("layer.mapplan", &json!({"path": file.to_string_lossy()})).unwrap();
    let suggested = |name: &str| plan["layers"].as_array().unwrap().iter().find(|l| l["name"] == name).map(|l| l["suggested"].clone());
    assert_eq!(suggested("AR_WALLS"), Some(json!("A-WALL")));
    assert_eq!(suggested("ST_COLUMNS"), Some(json!("S-RCC")));
    assert_eq!(suggested("N_TXT"), Some(json!("AN-TEXT")));
    assert_eq!(suggested("Odd Thing"), Some(Value::Null));
    for (name, want) in [("MEASURE-DEDUCT", None), ("MEASURE-COLUMN", None), ("DUCTS", Some("M-HVAC")), ("Fancy", None)] {
        assert_eq!(aad_engine::cmd::sheetstd::suggest_target(name), want, "{name}");
    }
    assert!(plan["targets"].as_array().unwrap().iter().any(|t| t["name"] == "A-WALL" && t["tab"] == "Working drawing"));
    assert!(s.doc().unwrap().entity_count() == 0, "planning changes nothing");

    s.execute(
        "level.import",
        &json!({"path": file.to_string_lossy(), "layerMap": {"ar_walls": "A-WALL", "ST_COLUMNS": "S-RCC", "N_TXT": "AN-TEXT"}}),
    )
    .unwrap();
    let d = s.doc().unwrap();
    let store = d.level_store(d.level_by_name("Plan").unwrap().id).unwrap();
    let on = |l: &str| store.iter().filter(|e| e.common.layer == l).count();
    assert_eq!((on("A-WALL"), on("S-RCC"), on("AN-TEXT"), on("Odd Thing")), (2, 1, 1, 1));
    assert!(d.layer("A-WALL").is_some() && d.layer("AR_WALLS").is_none(), "the mapped layer takes the standard look and the file's own is not kept");
    assert!(d.layer("Odd Thing").is_some(), "an unmapped layer keeps its name");

    for bad in [json!({"layerMap": 5}), json!({"layerMap": {"A": ""}}), json!({"layerMap": {"A": 3}})] {
        let mut params = bad.clone();
        params["path"] = json!(file.to_string_lossy());
        assert!(s.execute("level.import", &params).is_err(), "{bad}");
    }
    assert!(s.execute("layer.mapplan", &json!({})).is_err());
}

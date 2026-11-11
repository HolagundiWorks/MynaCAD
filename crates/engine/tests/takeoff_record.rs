//! `takeoff.record`: the take-off as records in the project file (layout shared with MynaOps `OspTakeoff`, version 1).
//! The tests follow the hand-off list: the records match the bridge rows, recording
//! again replaces, a removed item is deleted, ids carry the kind, everything is in millimetres, a rectangular slab
//! has its spans, and refs survive a rebuilt take-off.

use aad_engine::Session;
use aad_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn session(units: &str) -> Session {
    let mut s = Session::new();
    s.execute("drawing.units", &json!({"units": units, "mode": "metadata"})).unwrap();
    s
}

fn last_handle(s: &Session) -> Handle {
    s.doc().unwrap().model.iter().last().unwrap().handle
}

fn layer(s: &mut Session, name: &str) {
    s.execute("layer.current", &json!({"name": name})).unwrap();
}

/// Handles of what the scene measured, so a test can change or remove one thing.
struct Scene {
    wall_a: Handle,
    wall_b: Handle,
}

/// A small house in millimetres: two measured walls, a door and a window, a column, a lintel, a rectangular slab
/// and a room. Take-off tables are not drawn.
fn scene(s: &mut Session) -> Scene {
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]}))
        .unwrap();
    s.execute("openings.std", &json!({})).unwrap();
    s.execute("wall.draw", &json!({"points": [[0, 0], [9000, 0], [9000, 6000], [0, 6000], [0, 0]], "thickness": 230})).unwrap();
    s.execute("wall.door", &json!({"at": [2000, 0], "name": "D2", "width": 900})).unwrap();
    s.execute("wall.window", &json!({"at": [5000, 0], "name": "W3", "width": 1200})).unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [9000, 0]]})).unwrap();
    let wall_a = last_handle(s);
    s.execute("line", &json!({"points": [[0, 6000], [9000, 6000]]})).unwrap();
    let wall_b = last_handle(s);
    layer(s, "MEASURE-COLUMN");
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [230, 450]})).unwrap();
    layer(s, "MEASURE-LINTEL");
    s.execute("line", &json!({"points": [[1550, 0], [2450, 0]]})).unwrap();
    layer(s, "MEASURE-SLAB");
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [9000, 6000]})).unwrap();
    layer(s, "0");
    s.execute("room", &json!({"p1": [100, 100], "p2": [4100, 3100], "name": "br", "units": "mm", "textHeight": 150})).unwrap();
    measure(s);
    Scene { wall_a, wall_b }
}

/// Run every take-off tool again from the geometry on the measure layers.
fn measure(s: &mut Session) {
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    s.execute("takeoff.columns", &json!({"table": false})).unwrap();
    s.execute("takeoff.beams", &json!({"table": false})).unwrap();
    s.execute("takeoff.slab", &json!({"table": false})).unwrap();
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-takeoff-record-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("house.osp")
}

fn record(s: &mut Session, path: &std::path::Path) -> Value {
    s.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false})).unwrap()
}

/// The take-off records of a project file: id -> (payload as JSON, box). The project file keeps no boxes, so they read as zero.
fn read(path: &std::path::Path) -> std::collections::BTreeMap<u64, (Value, [f64; 4])> {
    records(path).into_iter().map(|(id, payload)| (id, (serde_json::from_slice(&payload).unwrap_or(Value::Null), [0.0; 4]))).collect()
}

/// The raw records `(id, payload)` of a project file's take-off.
fn records(path: &std::path::Path) -> Vec<(u64, Vec<u8>)> {
    aad_odp::Odp::open(path).unwrap().takeoff_as_records().unwrap()
}

fn kind(id: u64) -> u64 {
    id >> 56
}

fn slot(id: u64) -> u64 {
    (id >> 32) & 0x00FF_FFFF
}

/// The id of an item whose entity handle is `n` (small handles keep their value as the number) in `slot`.
fn item_id(kind: u64, slot: u64, n: u64) -> u64 {
    (kind << 56) | (slot << 32) | n
}

/// The one header in a database.
fn header_id(db: &std::collections::BTreeMap<u64, (Value, [f64; 4])>) -> u64 {
    let mut ids = db.keys().filter(|id| kind(**id) == 1);
    let id = *ids.next().expect("a header record");
    assert!(ids.next().is_none(), "one header per drawing");
    id
}

fn cleanup(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn the_records_match_the_bridge_rows_and_the_header_is_complete() {
    let mut s = session("mm");
    scene(&mut s);
    let path = temp("rows");
    let r = record(&mut s, &path);
    let db = read(&path);
    let hid = header_id(&db);
    assert_eq!(r["headerId"], json!(hid));
    assert_eq!(hid, item_id(1, slot(hid), 0), "the header is number 0 of its slot");
    assert!((1..=0x00FF_FFFF).contains(&slot(hid)));

    let header = &db.get(&hid).expect("a header record").0;
    assert_eq!(header["state"], "complete", "a finished recording is complete");
    assert!(header.get("annotated_pdf").is_none(), "no PDF was asked for");
    assert_eq!((header["format"].as_str(), header["version"].as_u64(), header["units"].as_str()), (Some("hcw-takeoff-shilpi"), Some(1), Some("mm")));
    assert_eq!(header["source"]["app"], "O-Draft");
    assert!(header["source"]["drawing_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(header["levels"][0]["name"], "Ground");
    assert!(header["opening_schedule"].as_array().is_some_and(|a| !a.is_empty()));
    assert_eq!(db.keys().filter(|id| kind(**id) == 1).count(), 1, "exactly one header");

    // The same rows `bridge.export` writes.
    let exported: Value = serde_json::from_str(s.execute("bridge.export", &json!({"write": false})).unwrap()["json"].as_str().unwrap()).unwrap();
    for (section, k) in [("walls", 3), ("openings", 4), ("columns", 5), ("lintels", 6), ("rooms", 7), ("slabs", 8)] {
        let rows = exported[section].as_array().unwrap();
        let recorded: Vec<&Value> = db.iter().filter(|(id, _)| kind(**id) == k).map(|(_, (p, _))| p).collect();
        assert_eq!(recorded.len(), rows.len(), "{section}: one record per row");
        assert!(!rows.is_empty(), "{section}: the scene has some");
        for row in rows {
            assert!(recorded.contains(&row), "{section}: the record equals the bridge row {row}");
        }
    }
    assert_eq!(r["counts"]["walls"], 2);
    assert_eq!(r["counts"]["openings"], 2);
    assert_eq!(r["counts"]["rooms"], 1);
    cleanup(&path);
}

#[test]
fn ids_carry_the_kind_the_slot_and_the_number() {
    let mut s = session("mm");
    scene(&mut s);
    let path = temp("ids");
    record(&mut s, &path);
    let db = read(&path);
    let sl = slot(header_id(&db));
    for (id, (payload, _)) in db {
        let k = kind(id);
        assert_eq!(slot(id), sl, "one drawing, one slot");
        assert!([1, 3, 4, 5, 6, 7, 8].contains(&k), "kind {k}");
        if k != 1 {
            // The ref names the entity handle the number came from: W<hex>, O<hex>, C<hex>, L<hex>, R<hex>, S<hex>.
            let r = payload["ref"].as_str().unwrap();
            let handle = u64::from_str_radix(&r[1..], 16).unwrap();
            assert_eq!(id & 0xFFFF_FFFF, handle, "{r}");
        }
    }
    cleanup(&path);
}

#[test]
fn recording_again_replaces_and_a_changed_wall_changes_one_record() {
    let mut s = session("mm");
    let scene = scene(&mut s);
    let path = temp("again");
    let first = record(&mut s, &path);
    let before = read(&path);
    let second = record(&mut s, &path);
    let same = read(&path);
    assert_eq!(before.keys().collect::<Vec<_>>(), same.keys().collect::<Vec<_>>(), "the same ids");
    assert_eq!(before.len(), same.len(), "no duplicates");
    assert_eq!((second["added"].as_u64(), second["changed"].as_u64(), second["deleted"].as_u64()), (Some(0), Some(0), Some(0)), "{second}");
    assert_eq!(second["unchanged"].as_u64(), first["written"].as_u64().map(|n| n - 1), "every row but the header is unchanged");

    // Make one wall 1 m longer, measure again, record again: that wall's record changes, nothing else.
    s.doc_mut()
        .unwrap()
        .modify_entity(scene.wall_a, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.b.x = 10_000.0;
            }
        })
        .unwrap();
    measure(&mut s);
    let third = record(&mut s, &path);
    assert_eq!((third["added"].as_u64(), third["changed"].as_u64(), third["deleted"].as_u64()), (Some(0), Some(1), Some(0)), "{third}");
    let after = read(&path);
    let id = item_id(3, slot(header_id(&after)), scene.wall_a.0);
    assert_eq!(after[&id].0["length_mm"], json!(10_000.0));
    assert_eq!(before[&id].0["length_mm"], json!(9000.0));
    cleanup(&path);
}

#[test]
fn a_wall_that_is_no_longer_measured_is_deleted_from_the_database() {
    let mut s = session("mm");
    let scene = scene(&mut s);
    let path = temp("gone");
    record(&mut s, &path);
    let before = read(&path);
    let id = item_id(3, slot(header_id(&before)), scene.wall_b.0);
    assert!(before.contains_key(&id));
    s.execute("erase", &json!({"handles": [scene.wall_b.hex()]})).unwrap();
    measure(&mut s);
    let r = record(&mut s, &path);
    assert_eq!(r["deleted"], 1, "{r}");
    let db = read(&path);
    assert!(!db.contains_key(&id), "its record is gone");
    assert_eq!(db.keys().filter(|id| kind(**id) == 3).count(), 1);
    cleanup(&path);
}

#[test]
fn refs_survive_a_rebuilt_take_off() {
    let mut s = session("mm");
    scene(&mut s);
    let path = temp("stable");
    record(&mut s, &path);
    let before: Vec<u64> = read(&path).keys().copied().collect();
    // Clear the saved results and measure the same geometry again.
    s.execute("takeoff.clear", &json!({})).ok();
    measure(&mut s);
    record(&mut s, &path);
    let after: Vec<u64> = read(&path).keys().copied().collect();
    assert_eq!(before, after, "the same ids for the same things");
    cleanup(&path);
}

#[test]
fn everything_is_in_millimetres_whatever_the_drawing_units() {
    // A 10 ft wall drawn in inches: 120 in = 3048 mm.
    let mut s = session("in");
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [120, 0]]})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let path = temp("imperial");
    record(&mut s, &path);
    let db = read(&path);
    assert_eq!(db[&header_id(&db)].0["units"], "mm");
    let (_, (wall, _)) = db.iter().find(|(id, _)| kind(**id) == 3).unwrap();
    let len = wall["length_mm"].as_f64().unwrap();
    assert!((len - 3048.0).abs() < 0.5, "length {len}");
    cleanup(&path);
}

#[test]
fn a_rectangular_slab_carries_its_spans_and_an_l_shaped_one_does_not() {
    let mut s = session("mm");
    scene(&mut s);
    // An L-shaped slab on the same layer.
    layer(&mut s, "MEASURE-SLAB");
    s.execute(
        "pline",
        &json!({"points": [[20_000, 0], [26_000, 0], [26_000, 2000], [22_000, 2000], [22_000, 5000], [20_000, 5000]], "closed": true}),
    )
    .unwrap();
    layer(&mut s, "0");
    measure(&mut s);
    let path = temp("slab");
    record(&mut s, &path);
    let slabs: Vec<Value> = read(&path).into_iter().filter(|(id, _)| kind(*id) == 8).map(|(_, (p, _))| p).collect();
    assert_eq!(slabs.len(), 2);
    let rect = slabs.iter().find(|s| s.get("rectangular").is_some()).expect("the rectangle has spans");
    assert_eq!((rect["length_mm"].as_f64(), rect["breadth_mm"].as_f64(), rect["rectangular"].as_bool()), (Some(9000.0), Some(6000.0), Some(true)));
    let l = slabs.iter().find(|s| s.get("rectangular").is_none()).expect("the L has none");
    assert!(l.get("length_mm").is_none() && l.get("breadth_mm").is_none());
    assert!(l["area_m2"].as_f64().unwrap() > 0.0, "the area is still there");
    cleanup(&path);
}

#[test]
fn the_default_file_is_the_open_project_and_nothing_else_takes_a_take_off() {
    let mut s = session("mm");
    scene(&mut s);
    let dir = std::env::temp_dir().join(format!("aad-takeoff-record-{}-beside", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // No path and no saved drawing: an error that says what to do.
    assert!(s.execute("takeoff.record", &json!({})).is_err());
    // A drawing that is not a project file has nowhere to record into.
    s.state_mut().unwrap().path = Some(dir.join("house.dxf").to_string_lossy().to_string());
    let err = s.execute("takeoff.record", &json!({"annotate": false})).unwrap_err().to_string();
    assert!(err.contains(".osp"), "{err}");
    // A project file is where it goes.
    let project = dir.join("house.osp");
    s.state_mut().unwrap().path = Some(project.to_string_lossy().to_string());
    let r = s.execute("takeoff.record", &json!({"annotate": false})).unwrap();
    assert!(r["path"].as_str().unwrap().ends_with("house.osp"), "{r}");
    assert!(project.exists());
    // A file that is not a project file is refused and left alone.
    let other = dir.join("entities.vdb");
    std::fs::write(&other, b"not a project").unwrap();
    let err = s.execute("takeoff.record", &json!({"path": other.to_string_lossy()})).unwrap_err().to_string();
    assert!(err.contains(".osp"), "{err}");
    assert_eq!(std::fs::read(&other).unwrap(), b"not a project", "the file was not touched");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_empty_drawing_records_just_the_header() {
    let mut s = session("mm");
    let path = temp("empty");
    let r = record(&mut s, &path);
    assert_eq!(r["written"], 1);
    let db = read(&path);
    assert_eq!(db.len(), 1);
    assert_eq!(db[&header_id(&db)].0["state"], "complete");
    cleanup(&path);
}

#[test]
fn the_slot_is_kept_in_the_drawing_and_two_drawings_share_a_database_without_touching_each_other() {
    let path = temp("two");
    let mut a = session("mm");
    scene(&mut a);
    let first = record(&mut a, &path);
    let mut b = session("mm");
    b.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut b, "MEASURE-FULLBRICK");
    b.execute("line", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    b.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let second = record(&mut b, &path);
    assert_ne!(first["slot"], second["slot"], "two drawings, two slots");
    let db = read(&path);
    assert_eq!(db.keys().filter(|id| kind(**id) == 1).count(), 2, "two headers");
    let (sa, sb) = (first["slot"].as_u64().unwrap(), second["slot"].as_u64().unwrap());
    assert_eq!(db.keys().filter(|id| slot(**id) == sb && kind(**id) == 3).count(), 1);
    let rows_of_a = db.keys().filter(|id| slot(**id) == sa).count();

    // Recording A again does not touch B, and A keeps its slot.
    let again = record(&mut a, &path);
    assert_eq!(again["slot"], first["slot"], "the slot does not change");
    let db2 = read(&path);
    assert_eq!(db2.keys().filter(|id| slot(**id) == sa).count(), rows_of_a);
    assert_eq!(db2.keys().filter(|id| slot(**id) == sb).count(), db.keys().filter(|id| slot(**id) == sb).count());
    cleanup(&path);
}

/// A plotter that keeps what it was asked to plot and returns a PDF-looking buffer of known size.
static PLOTTED: std::sync::Mutex<Vec<aad_engine::doc::Drawing>> = std::sync::Mutex::new(Vec::new());

fn fake_plot(d: &aad_engine::doc::Drawing, _: &aad_engine::doc::Space, _: &Value) -> Result<Vec<u8>, String> {
    PLOTTED.lock().unwrap().push(d.clone());
    let mut pdf = b"%PDF-1.4\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n".to_vec();
    pdf.resize(1_200_000, b'x');
    Ok(pdf)
}

/// The annotated drawing: every measured item is drawn again on `TAKEOFF-ANNOT` with a label, the drawing itself is
/// untouched, and the PDF is stored in chunks that add up to the file the header describes.
#[test]
fn the_annotated_drawing_marks_what_was_measured_and_is_stored_in_chunks() {
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |_, _| Err("no".into()),
        write: |_, _| Ok(Vec::new()),
        plot: Some(fake_plot),
    });
    let mut s = session("mm");
    scene(&mut s);
    let before = s.doc().unwrap().model.len();
    let path = temp("annotate");
    let r = s.execute("takeoff.record", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["state"], "complete");
    assert_eq!(s.doc().unwrap().model.len(), before, "the drawing itself is untouched");

    // What the plotter was given: the measured items again, on the annotation layer, plus labels, legend and title.
    let plotted = PLOTTED.lock().unwrap().pop().expect("the plotter was called");
    let drawn: Vec<_> = plotted.model.iter().filter(|e| e.common.layer == "TAKEOFF-ANNOT").collect();
    let texts = drawn.iter().filter(|e| matches!(e.kind, EntityKind::Text(_))).count();
    let shapes = drawn.len() - texts;
    assert!(shapes >= 7, "walls, openings, column, lintel, room and slab are drawn again: {shapes}");
    assert!(texts > shapes, "each has a label, and the legend and title add more: {texts}");
    assert!(drawn.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value.starts_with("Walls: 2"))), "the legend counts the walls");

    // The stored PDF: chunk records of kind 9 that add up to what the header says.
    let db = read(&path);
    let header = &db[&header_id(&db)].0;
    let p = &header["annotated_pdf"];
    assert_eq!(p["bytes"], 1_200_000);
    assert_eq!(p["pages"], 1);
    let mut chunks: Vec<(u64, usize)> = Vec::new();
    let raw = records(&path);
    for (id, _) in db.iter().filter(|(id, _)| kind(**id) == 9) {
        let (_, payload) = raw.iter().find(|(rid, _)| rid == id).unwrap();
        chunks.push((*id & 0xFFFF_FFFF, payload.len()));
    }
    chunks.sort();
    assert_eq!(p["chunks"].as_u64(), Some(chunks.len() as u64));
    assert_eq!(chunks.len(), 3, "1.2 MB in chunks of at most 512 KiB");
    assert!(chunks.iter().enumerate().all(|(i, c)| c.0 == i as u64 && c.1 <= 512 * 1024));
    assert_eq!(chunks.iter().map(|c| c.1).sum::<usize>(), 1_200_000);

    // Recording again without the PDF drops the old chunks and the header's entry.
    s.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false})).unwrap();
    let db = read(&path);
    assert_eq!(db.keys().filter(|id| kind(**id) == 9).count(), 0);
    assert!(db[&header_id(&db)].0.get("annotated_pdf").is_none());
    cleanup(&path);
}

/// What MynaOps asked for after reading the first recording: rooms carry their level and sides, openings name their
/// wall, walls carry a thickness, and a ventilator is a window.
#[test]
fn rows_carry_what_o_coreop_needs_to_use_them_without_guessing() {
    let mut s = session("mm");
    let scene = scene(&mut s);
    let path = temp("needs");
    let r = record(&mut s, &path);
    let db = read(&path);
    let rows = |k: u64| -> Vec<Value> { db.iter().filter(|(id, _)| kind(**id) == k).map(|(_, (p, _))| p.clone()).collect() };

    // Walls: A-WALL faces give a measured thickness (scene uses wall.draw at 230 mm); no nominal note.
    for w in rows(3) {
        assert_eq!(w["thickness_mm"], json!(230.0), "{w}");
        assert!(w.get("_note").is_none(), "measured thickness needs no honesty note: {w}");
    }
    assert!(
        !r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("nominal"))),
        "faces were measured, so thickness is not called nominal: {r}"
    );

    // Rooms: level and both sides, rectangular.
    let room = &rows(7)[0];
    assert_eq!(room["level"], "Ground");
    assert_eq!(
        (room["length_mm"].as_f64(), room["breadth_mm"].as_f64(), room["rectangular"].as_bool()),
        (Some(4000.0), Some(3000.0), Some(true)),
        "{room}"
    );

    // Openings: MEASURE-DEDUCT matches the front wall (y = 0); no proximity honesty note.
    let front = format!("W{}", scene.wall_a.hex());
    for o in rows(4) {
        assert_eq!(o["wall_ref"], json!(front), "{o}");
        assert!(o.get("_note").is_none(), "deduct match needs no proximity note: {o}");
    }
    assert!(!r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("not on a measured wall"))));

    // An opening far from every measured wall gets no wall_ref, and the export says so.
    s.execute("wall.window", &json!({"at": [4500, 3000], "name": "W3", "width": 1200, "angle": 0})).unwrap();
    let again = record(&mut s, &path);
    let db = read(&path);
    let stray: Vec<Value> =
        db.iter().filter(|(id, _)| kind(**id) == 4).map(|(_, (p, _))| p.clone()).filter(|o| o.get("wall_ref").is_none()).collect();
    assert_eq!(stray.len(), 1, "{stray:?}");
    assert!(again["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("1 opening(s) are not on a measured wall"))));
    cleanup(&path);
}

#[test]
fn brick_walls_without_a_wall_faces_keep_nominal_thickness_and_note() {
    let mut s = session("mm");
    s.execute("floors", &json!({"floors": [{"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450}]}))
        .unwrap();
    s.execute("takeoff.start", &json!({})).unwrap();
    layer(&mut s, "MEASURE-FULLBRICK");
    s.execute("line", &json!({"points": [[0, 0], [5000, 0]]})).unwrap();
    s.execute("takeoff.brick", &json!({"table": false})).unwrap();
    let path = temp("nominal-only");
    let r = record(&mut s, &path);
    let db = read(&path);
    let walls: Vec<_> = db.iter().filter(|(id, _)| kind(**id) == 3).map(|(_, (p, _))| p.clone()).collect();
    assert_eq!(walls.len(), 1);
    assert_eq!(walls[0]["thickness_mm"], json!(230.0));
    assert!(walls[0]["_note"].as_str().is_some_and(|n| n.contains("nominal")), "{}", walls[0]);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("nominal"))), "{r}");
    cleanup(&path);
}

#[test]
fn a_second_drawing_on_the_same_project_and_floor_is_warned_at_record_time() {
    let path = temp("overlap");
    let context = json!({
        "v": 1, "prepared_on": "2026-10-28", "company": { "name": "Studio One" },
        "project": { "project": { "name": "Mehta residence" }, "client": { "name": "Mehta" },
            "project_ref": { "v": 1, "company_uuid": "u-1", "project_id": 7 } }
    });
    let mut a = session("mm");
    scene(&mut a);
    a.execute("companyfile", &json!({ "context": context })).unwrap();
    let first = record(&mut a, &path);
    assert!(
        !first["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("double-count"))),
        "first drawing alone is not a double-count: {first}"
    );

    let mut b = session("mm");
    scene(&mut b);
    b.execute("companyfile", &json!({ "context": context })).unwrap();
    let second = record(&mut b, &path);
    assert!(
        second["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| { w.as_str().is_some_and(|t| t.contains("double-count") && t.contains("Ground") && t.contains("same company-file project")) }),
        "second drawing on the same project and floor must warn: {second}"
    );

    // Different floor: no overlap warning.
    let mut c = session("mm");
    scene(&mut c);
    c.execute("companyfile", &json!({ "context": context })).unwrap();
    let other_floor = c.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false, "level": "First"})).unwrap();
    assert!(
        !other_floor["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("double-count"))),
        "a different stamped level is not an overlap: {other_floor}"
    );
    cleanup(&path);
}

#[test]
fn floor_regions_stamp_each_measured_wall_with_its_assigned_floor() {
    let mut s = session("mm");
    let scene = scene(&mut s);
    s.execute(
        "floors",
        &json!({"floors": [
            {"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450},
            {"name": "First", "floorToFloor": 3.0, "ceiling": 2.85, "lintel": 2.1, "slab": 150, "beam": 450}
        ]}),
    )
    .unwrap();
    s.execute("floor.declare", &json!({"name": "Ground", "handles": [scene.wall_a.hex()]})).unwrap();
    s.execute("floor.declare", &json!({"name": "First", "handles": [scene.wall_b.hex()]})).unwrap();
    let path = temp("entity-floors");
    let r = record(&mut s, &path);
    assert!(
        r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("floor regions") && t.contains("2 floor"))),
        "{r}"
    );
    let db = read(&path);
    let walls: Vec<_> = db.iter().filter(|(id, _)| kind(**id) == 3).map(|(_, (p, _))| p.clone()).collect();
    let by_ref: std::collections::BTreeMap<_, _> =
        walls.into_iter().filter_map(|w| Some((w["ref"].as_str()?.to_string(), w["level"].as_str()?.to_string()))).collect();
    assert_eq!(by_ref.get(&format!("W{}", scene.wall_a.hex())).map(String::as_str), Some("Ground"));
    assert_eq!(by_ref.get(&format!("W{}", scene.wall_b.hex())).map(String::as_str), Some("First"));
    cleanup(&path);
}

#[test]
fn the_header_names_the_company_file_project_so_o_coreop_can_match_the_take_off() {
    let mut s = session("mm");
    scene(&mut s);
    let path = temp("project-ref");
    let db = read(&path_after(&mut s, &path));
    assert!(db[&header_id(&db)].0["source"].get("project_ref").is_none(), "an unlinked drawing names no project");

    let context = json!({
        "v": 1, "prepared_on": "2026-10-20", "company": { "name": "Studio One" },
        "project": { "project": { "name": "Mehta residence" }, "client": { "name": "Mehta family" }, "project_ref": { "v": 1, "company_uuid": "u-1", "project_id": 7 } }
    });
    s.execute("companyfile", &json!({ "context": context })).unwrap();
    let db = read(&path_after(&mut s, &path));
    assert_eq!(db[&header_id(&db)].0["source"]["project_ref"], json!({ "v": 1, "company_uuid": "u-1", "project_id": 7 }));
    cleanup(&path);
}

/// Records the take-off and returns where it went.
fn path_after(s: &mut Session, path: &std::path::Path) -> std::path::PathBuf {
    record(s, path);
    path.to_path_buf()
}

#[test]
fn multi_floor_drawing_warns_when_level_is_not_passed() {
    let mut s = session("mm");
    scene(&mut s);
    s.execute(
        "floors",
        &json!({"floors": [
            {"name": "Ground", "floorToFloor": 3.15, "ceiling": 3.0, "lintel": 2.1, "slab": 150, "beam": 450},
            {"name": "First", "floorToFloor": 3.0, "ceiling": 2.85, "lintel": 2.1, "slab": 150, "beam": 450}
        ]}),
    )
    .unwrap();
    let path = temp("multilevel");
    let r = record(&mut s, &path);
    assert!(
        r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("2 floors") && t.contains("Pass `level`"))),
        "{r}"
    );
    let explicit = s.execute("takeoff.record", &json!({"path": path.to_string_lossy(), "annotate": false, "level": "First"})).unwrap();
    assert!(!explicit["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|t| t.contains("Pass `level`"))), "{explicit}");
    let db = read(&path);
    let walls: Vec<_> = db.iter().filter(|(id, _)| kind(**id) == 3).map(|(_, (p, _))| p.clone()).collect();
    assert!(!walls.is_empty());
    for w in walls {
        assert_eq!(w["level"], "First", "{w}");
    }
    cleanup(&path);
}

/// The rows of one kind in a project file's take-off.
fn rows_of(path: &std::path::Path, k: u64) -> Vec<Value> {
    read(path).into_iter().filter(|(id, _)| kind(*id) == k).map(|(_, (p, _))| p).collect()
}

#[test]
fn a_drawing_without_levels_writes_no_level_ids() {
    let mut s = session("mm");
    scene(&mut s);
    assert!(!s.doc().unwrap().has_levels());
    let path = temp("nolevelids");
    record(&mut s, &path);
    let db = read(&path);
    let header = &db.get(&header_id(&db)).unwrap().0;
    assert_eq!(header["levels"][0]["name"], "Ground");
    assert!(header["levels"][0].get("id").is_none() && header["levels"][0].get("elevation_mm").is_none(), "{header}");
    for k in [3, 4, 5, 6, 7, 8] {
        for row in rows_of(&path, k) {
            assert!(row.get("level_id").is_none(), "kind {k}: {row}");
            assert_eq!(row["level"], "Ground", "the name stays on every row: {row}");
        }
    }
    cleanup(&path);
}

#[test]
fn named_levels_give_every_row_a_level_id_and_the_header_lists_the_levels() {
    let mut s = session("mm");
    scene(&mut s);
    let made = s.execute("level.new", &json!({"name": "First", "elevation": 3.15, "height": 3.0, "first": "Ground"})).unwrap();
    let first_id = made["id"].as_u64().unwrap();
    let ground_id = made["levels"].as_array().unwrap().iter().find(|l| l["name"] == "Ground").unwrap()["id"].as_u64().unwrap();
    assert_ne!(first_id, ground_id);

    // Active level (Ground): every row carries its id.
    let path = temp("levelids");
    record(&mut s, &path);
    let db = read(&path);
    let header = &db.get(&header_id(&db)).unwrap().0;
    let levels = header["levels"].as_array().unwrap();
    let ground = levels.iter().find(|l| l["name"] == "Ground").unwrap();
    let first = levels.iter().find(|l| l["name"] == "First").unwrap();
    assert_eq!(ground["id"], json!(ground_id));
    assert_eq!(ground["height_mm"].as_f64(), Some(3150.0), "the existing floor-to-floor `height_mm` keeps its meaning");
    assert_eq!(ground["slab_thickness_mm"].as_f64(), Some(150.0), "the existing floor fields are kept");
    assert_eq!((first["id"].as_u64(), first["elevation_mm"].as_f64(), first["height_mm"].as_f64()), (Some(first_id), Some(3150.0), Some(3000.0)));
    let mut seen = 0;
    for k in [3, 4, 5, 6, 7, 8] {
        for row in rows_of(&path, k) {
            assert_eq!(row["level"], "Ground", "{row}");
            assert_eq!(row["level_id"], json!(ground_id), "kind {k}: {row}");
            seen += 1;
        }
    }
    assert!(seen >= 6, "walls, openings, column, lintel, room and slab were checked: {seen}");

    // A stamped `level` that is another named level gets that level's id; a name no level has gets none.
    let again = temp("levelids-first");
    s.execute("takeoff.record", &json!({"path": again.to_string_lossy(), "annotate": false, "level": "First"})).unwrap();
    for row in rows_of(&again, 3) {
        assert_eq!((row["level"].as_str(), row["level_id"].as_u64()), (Some("First"), Some(first_id)), "{row}");
    }
    let unknown = temp("levelids-unknown");
    s.execute("takeoff.record", &json!({"path": unknown.to_string_lossy(), "annotate": false, "level": "Terrace"})).unwrap();
    for row in rows_of(&unknown, 3) {
        assert_eq!(row["level"], "Terrace");
        assert!(row.get("level_id").is_none(), "{row}");
    }
    cleanup(&path);
    cleanup(&again);
    cleanup(&unknown);
}

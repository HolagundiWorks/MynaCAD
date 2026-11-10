//! Opening a DWG or DXF through its native working copy (`aad_engine::nativefile`, docs/NATIVE-OPEN.md).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aad_engine::nativefile::{self, OnChanged};
use aad_engine::{Session, doc::Drawing};
use serde_json::json;

/// The cache folder is process-wide: the tests run one at a time.
static ENV: Mutex<()> = Mutex::new(());

fn install() {
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    aad_engine::cmd::file::set_container(nativefile::container_hooks());
}

struct Dirs {
    _tmp: tempfile::TempDir,
    work: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}

fn dirs() -> Dirs {
    install();
    let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    nativefile::set_cache_root(Some(tmp.path().join("cache")));
    Dirs { _tmp: tmp, work, _guard: guard }
}

/// A DXF with the sample drawing's content (blocks, text, dimensions, hatches) and `n` more lines.
fn write_dxf(path: &Path, n: usize) {
    let mut s = Session::empty();
    s.open_drawing(aad_engine::sample::default_sample(), "Sample", None);
    for i in 0..n {
        s.execute("line", &json!({ "points": [[i as f64, 0.0], [i as f64, 10.0]] })).unwrap();
    }
    let bytes = aad_io::write(s.doc().unwrap(), "x.dxf").unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn direct(path: &Path) -> Drawing {
    aad_io::read(&std::fs::read(path).unwrap(), &path.to_string_lossy()).unwrap()
}

fn quiet(_: &str) {}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("importing") || n.ends_with("-wal") || n.ends_with("-shm") || n.contains("write-test") || n.contains("tmp"))
        .collect()
}

#[test]
fn the_first_open_converts_beside_the_source_and_the_next_reuses_it() {
    let d = dirs();
    let src = d.work.join("Plan.dxf");
    write_dxf(&src, 40);
    let before = std::fs::read(&src).unwrap();
    let want = direct(&src);

    let first = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!((first.info.action.as_str(), first.info.location.as_str()), ("imported", "beside"));
    assert_eq!(Path::new(&first.path), d.work.join("Plan.osp"));
    assert!(!first.info.from_native);
    assert_eq!(first.drawing, want, "the converted drawing is the drawing a direct read gives");
    assert!(leftovers(&d.work).is_empty(), "{:?}", leftovers(&d.work));

    let again = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(again.info.action, "reused");
    assert!(again.info.from_native, "the second open reads the native model, not the DXF");
    assert!(!again.info.ms.contains_key("convert"));
    assert_eq!(again.drawing, want, "the native model round-trips the drawing exactly");
    assert_eq!(again.drawing.entity_count(), want.entity_count());
    assert_eq!(std::fs::read(&src).unwrap(), before, "the source is only read");

    // The project file keeps the source bytes as its drawing (what MynaOps reads) and says where it came from.
    let odp = aad_odp::Odp::open(Path::new(&first.path)).unwrap();
    assert_eq!(odp.drawing("Plan.dxf").unwrap().unwrap().bytes, before);
    assert_eq!(odp.meta_get("source.name").unwrap().as_deref(), Some("Plan.dxf"));
    assert_eq!(odp.meta_get("source.sha256").unwrap().unwrap(), aad_odp::sha256_hex(&before));
    assert_eq!(odp.meta_get("source.size").unwrap().unwrap(), before.len().to_string());
    assert_eq!(odp.meta_get("source.import_version").unwrap().unwrap(), nativefile::IMPORT_VERSION.to_string());
    assert!(odp.meta_get("source.mtime_ns").unwrap().is_some() && odp.meta_get("source.insunits").unwrap().is_some());
    assert_eq!(odp.revision().unwrap(), 1, "the import is one revision");
    assert_eq!(nativefile::peek(&src.to_string_lossy()).unwrap().state, "current");
}

#[test]
fn a_project_of_the_users_with_the_same_name_is_never_touched() {
    let d = dirs();
    let src = d.work.join("Plan.dxf");
    write_dxf(&src, 3);
    // The user's own project file has the name the working copy would take.
    let own = d.work.join("Plan.osp");
    {
        let mut odp = aad_odp::Odp::create(&own, "Mine").unwrap();
        let mut e = odp.edit().unwrap();
        e.put_project_doc("{\"project\":{\"name\":\"mine\"}}").unwrap();
        e.commit().unwrap();
    }
    let own_before = aad_odp::Odp::open(&own).unwrap().meta().unwrap();
    assert_eq!(nativefile::peek(&src.to_string_lossy()).unwrap().state, "new");
    let opened = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(opened.info.location, "cache");
    assert_eq!(Path::new(&opened.path), nativefile::cache_path(&src));
    assert!(opened.info.message.contains("cache"));
    let own_after = aad_odp::Odp::open(&own).unwrap();
    assert_eq!(own_after.meta().unwrap(), own_before);
    assert!(own_after.drawing_names().unwrap().is_empty() && own_after.native_stamp().unwrap().is_none());
    // The cached copy is found again.
    let again = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!((again.info.action.as_str(), again.info.location.as_str()), ("reused", "cache"));
    // A file that is not a project at all is not touched either.
    let other = d.work.join("Other.dxf");
    write_dxf(&other, 1);
    std::fs::write(d.work.join("Other.osp"), b"not a project file").unwrap();
    let opened = nativefile::open_for_edit(&other.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(opened.info.location, "cache");
    assert_eq!(std::fs::read(d.work.join("Other.osp")).unwrap(), b"not a project file");
}

#[test]
fn a_changed_source_is_converted_again_when_nothing_was_saved_into_the_copy() {
    let d = dirs();
    let src = d.work.join("Plan.dxf");
    write_dxf(&src, 5);
    let first = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    let id = aad_odp::Odp::open(Path::new(&first.path)).unwrap().meta().unwrap().project_id;
    write_dxf(&src, 9);
    assert_eq!(nativefile::peek(&src.to_string_lossy()).unwrap().state, "changed");
    assert!(!nativefile::peek(&src.to_string_lossy()).unwrap().edited);
    let second = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(second.info.action, "reimported");
    assert_eq!(second.drawing, direct(&src));
    assert_eq!(second.drawing.entity_count(), first.drawing.entity_count() + 4);
    // The earlier copy is kept, and the project keeps its identity (take-off and project records live on).
    let backup = second.info.backup.clone().unwrap();
    assert!(Path::new(&backup).is_file() && backup.contains("before-reimport"));
    assert_eq!(nativefile::read_project(&backup).unwrap().drawing, first.drawing);
    let odp = aad_odp::Odp::open(Path::new(&second.path)).unwrap();
    assert_eq!(odp.meta().unwrap().project_id, id);
    assert_eq!(odp.revision().unwrap(), 2);
    let third = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(third.info.action, "reused");
    assert!(third.info.from_native);
}

#[test]
fn a_changed_source_does_not_replace_saved_edits_unless_told() {
    let d = dirs();
    let src = d.work.join("Plan.dxf");
    write_dxf(&src, 5);
    let first = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    // The user edits and saves the working copy.
    let mut s = Session::empty();
    let prepared = aad_engine::cmd::file::prepare(first.drawing);
    aad_engine::cmd::file::finish_open(&mut s, prepared, &first.path).unwrap();
    s.execute("circle", &json!({ "center": [5.0, 5.0], "radius": 2.0 })).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    let edited = s.doc().unwrap().clone();
    assert!(!s.state().unwrap().is_dirty());
    assert_eq!(nativefile::peek(&src.to_string_lossy()).unwrap().state, "current");
    assert!(nativefile::peek(&src.to_string_lossy()).unwrap().edited);

    // The saved working copy opens from its native model, with the edit.
    let reopened = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(reopened.info.action, "reused");
    assert!(reopened.info.from_native && reopened.info.edited);
    assert_eq!(reopened.drawing, edited);

    write_dxf(&src, 9);
    let kept = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert_eq!(kept.info.action, "kept");
    assert!(kept.info.source_changed && kept.info.edited);
    assert_eq!(kept.drawing, edited, "the edits are still there");
    let keep = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Keep, &quiet).unwrap();
    assert_eq!(keep.info.action, "kept");

    let fresh = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Reimport, &quiet).unwrap();
    assert_eq!(fresh.info.action, "reimported");
    assert_eq!(fresh.drawing, direct(&src));
    let backup = fresh.info.backup.unwrap();
    assert_eq!(nativefile::read_project(&backup).unwrap().drawing, edited, "the edited copy is the backup");
}

#[test]
fn a_drawing_replaced_by_another_program_is_read_from_its_bytes() {
    let d = dirs();
    let src = d.work.join("Plan.dxf");
    write_dxf(&src, 5);
    let first = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    assert!(nativefile::read_project(&first.path).unwrap().native);
    // MynaOps (or an older MynaCAD) writes the drawing row and knows nothing of the native model.
    let other = d.work.join("Other.dxf");
    write_dxf(&other, 2);
    {
        let mut odp = aad_odp::Odp::open(Path::new(&first.path)).unwrap();
        let mut e = odp.edit().unwrap();
        e.put_drawing("Plan.dxf", "dxf", &std::fs::read(&other).unwrap()).unwrap();
        e.commit().unwrap();
    }
    let read = nativefile::read_project(&first.path).unwrap();
    assert!(!read.native, "a stale native model is not used");
    assert!(read.fallback.unwrap().contains("another program"));
    assert_eq!(read.drawing, direct(&other));
    // The next save from MynaCAD writes the native model again.
    nativefile::write_project(&first.path, &read.drawing, None).unwrap();
    let read = nativefile::read_project(&first.path).unwrap();
    assert!(read.native);
}

#[test]
fn a_failed_conversion_leaves_no_file_and_the_source_untouched() {
    let d = dirs();
    let src = d.work.join("Broken.dwg");
    std::fs::write(&src, b"AC1032 this is not a drawing at all").unwrap();
    let e = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).err().unwrap();
    assert!(!e.is_empty());
    assert!(!d.work.join("Broken.osp").exists());
    assert!(leftovers(&d.work).is_empty(), "{:?}", leftovers(&d.work));
    assert_eq!(std::fs::read(&src).unwrap(), b"AC1032 this is not a drawing at all");
    assert!(nativefile::open_for_edit(&d.work.join("missing.dxf").to_string_lossy(), OnChanged::Auto, &quiet).is_err());
    // An explicit conversion never replaces a file.
    let good = d.work.join("Good.dxf");
    write_dxf(&good, 1);
    let out = d.work.join("Out.osp");
    nativefile::import_to(&good.to_string_lossy(), &out.to_string_lossy()).unwrap();
    assert!(nativefile::import_to(&good.to_string_lossy(), &out.to_string_lossy()).unwrap_err().contains("already exists"));
}

#[test]
fn saving_a_working_copy_keeps_revisions_honest_and_exporting_leaves_it_in_place() {
    let d = dirs();
    let src = d.work.join("Plan.dxf");
    write_dxf(&src, 3000);
    let source_bytes = std::fs::read(&src).unwrap();
    let first = nativefile::open_for_edit(&src.to_string_lossy(), OnChanged::Auto, &quiet).unwrap();
    let mut s = Session::empty();
    aad_engine::cmd::file::finish_open(&mut s, aad_engine::cmd::file::prepare(first.drawing), &first.path).unwrap();
    assert_eq!(s.state().unwrap().title, "Plan.osp");
    s.execute("circle", &json!({ "center": [5.0, 5.0], "radius": 2.0 })).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    let rev = aad_odp::Odp::open(Path::new(&first.path)).unwrap().revision().unwrap();
    // A save with nothing changed is not a revision (other programs do not reload).
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(aad_odp::Odp::open(Path::new(&first.path)).unwrap().revision().unwrap(), rev);

    // Save As to DXF from a project is an export: the file is written, the drawing stays in its project file.
    s.execute("circle", &json!({ "center": [9.0, 9.0], "radius": 1.0 })).unwrap();
    let out = d.work.join("Copy.dxf");
    s.execute("saveas", &json!({ "path": out.to_string_lossy() })).unwrap();
    assert!(out.is_file());
    let st = s.state().unwrap();
    assert_eq!(st.path.as_deref(), Some(first.path.as_str()));
    assert_eq!(st.title, "Plan.osp");
    assert!(st.is_dirty(), "an export does not save the project");
    assert!(s.log.iter().any(|l| l.contains("Exported a copy")));
    assert_eq!(direct(&out).entity_count(), s.doc().unwrap().entity_count());
    assert_eq!(std::fs::read(&src).unwrap(), source_bytes, "the source was never written");
}

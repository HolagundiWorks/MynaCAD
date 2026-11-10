//! The native copy of a drawing is the same drawing: the same entity count, the same display list and the same picture as a direct read
//! (docs/NATIVE-OPEN.md). The second test runs on a real drawing:
//! `AAD_PERF_FILE=<drawing.dwg> cargo test --release -p aad-cli --test native_fidelity -- --ignored --nocapture`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aad-cli")).args(args).output().expect("aad-cli runs")
}

fn ok(o: &Output) {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aad-cli-native-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn entity_count(info: &Output) -> u64 {
    let v: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    v["entityCount"].as_u64().unwrap()
}

/// The display list a drawing gives at full detail: primitives, line vertices, triangle vertices.
fn display_counts(d: &aad_engine::doc::Drawing) -> (usize, usize, usize) {
    let opts = aad_engine::render::Options {
        tolerance: 0.5,
        min_dash: 1.0,
        text: true,
        fill: true,
        lineweights: true,
        view_height: 0.0,
        ..Default::default()
    };
    let list = aad_engine::render::build(d, &aad_engine::doc::Space::Model, &opts);
    (list.prims.len(), list.verts.len(), list.tris.len())
}

/// Pixels that differ by more than `tolerance` grey levels, and the picture size.
fn differing_pixels(a: &Path, b: &Path, tolerance: u8) -> (usize, usize) {
    let (a, b) = (aad_io::raster::read_gray(&std::fs::read(a).unwrap()).unwrap(), aad_io::raster::read_gray(&std::fs::read(b).unwrap()).unwrap());
    assert_eq!((a.width, a.height), (b.width, b.height));
    (a.gray.iter().zip(&b.gray).filter(|(x, y)| x.abs_diff(**y) > tolerance).count(), a.gray.len())
}

fn compare(source: &Path, dir: &Path) {
    let osp = dir.join("copy.osp");
    let t = Instant::now();
    ok(&cli(&["convert", source.to_str().unwrap(), osp.to_str().unwrap()]));
    println!("convert to .osp: {:.0} ms, {} bytes", t.elapsed().as_secs_f64() * 1e3, std::fs::metadata(&osp).unwrap().len());

    // The same entity count, through the same command a user would run.
    let (t_direct, direct) = (Instant::now(), cli(&["info", source.to_str().unwrap()]));
    let t_direct = t_direct.elapsed();
    let (t_native, native) = (Instant::now(), cli(&["info", osp.to_str().unwrap()]));
    let t_native = t_native.elapsed();
    ok(&direct);
    ok(&native);
    assert_eq!(entity_count(&direct), entity_count(&native));
    println!(
        "info: direct {:.0} ms, native {:.0} ms, {} entities",
        t_direct.as_secs_f64() * 1e3,
        t_native.as_secs_f64() * 1e3,
        entity_count(&native)
    );

    // The same drawing in memory, and so the same display list.
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let a = aad_io::read(&std::fs::read(source).unwrap(), source.to_str().unwrap()).unwrap();
    let read = aad_engine::nativefile::read_project(osp.to_str().unwrap()).unwrap();
    assert!(read.native, "the project file is read from its native model");
    assert!(read.drawing == a, "the native model is the drawing a direct read gives");
    let (da, db) = (display_counts(&a), display_counts(&read.drawing));
    assert_eq!(da, db);
    println!("display list: {} primitives, {} line vertices, {} triangle vertices (both)", da.0, da.1, da.2);

    // The same picture.
    let (png_direct, png_native) = (dir.join("direct.png"), dir.join("native.png"));
    ok(&cli(&["convert", source.to_str().unwrap(), png_direct.to_str().unwrap()]));
    ok(&cli(&["convert", osp.to_str().unwrap(), png_native.to_str().unwrap()]));
    let (differ, total) = differing_pixels(&png_direct, &png_native, 2);
    println!("png: {differ} of {total} pixels differ by more than 2 grey levels");
    assert!(differ * 10_000 <= total, "{differ} of {total} pixels differ");
}

#[test]
fn the_sample_is_the_same_drawing_and_picture_through_its_native_copy() {
    let dir = fresh_dir("sample");
    let dxf = dir.join("sample.dxf");
    ok(&cli(&["run", "--sample", "--script", "LINE 0,0 40,25\nCIRCLE 10,10 4\n", "--save", dxf.to_str().unwrap()]));
    compare(&dxf, &dir);
    // `info`, `run` and `convert` read a DWG or DXF directly: no working copy appears beside it.
    ok(&cli(&[
        "run",
        dxf.to_str().unwrap(),
        "--script",
        "CIRCLE 0,0 3
",
    ]));
    assert!(!dir.join("sample.osp").exists());
    // The conversion never replaces a file, and the source is as it was.
    let before = std::fs::read(&dxf).unwrap();
    let again = cli(&["convert", dxf.to_str().unwrap(), dir.join("copy.osp").to_str().unwrap()]);
    assert!(!again.status.success());
    assert_eq!(std::fs::read(&dxf).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn a_real_drawing_is_the_same_through_its_native_copy() {
    let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
    let dir = fresh_dir("real");
    compare(Path::new(&path), &dir);
    let _ = std::fs::remove_dir_all(&dir);
}

//! A timing probe, not a pass/fail test: set AAD_PERF_FILE to a drawing and run
//! `cargo test --release -p aad-cli --test perf_probe -- --nocapture --ignored` to see where the
//! time goes when it is opened and edited. Ignored by default (it needs a file and a release build).

use std::time::Instant;

use aad_engine::Session;
use serde_json::json;

fn t<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t0 = Instant::now();
    let r = f();
    println!("{label:<34} {:>8.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
    r
}

#[test]
#[ignore]
fn where_the_time_goes() {
    let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| aad_io::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    let bytes = t("read file bytes", || std::fs::read(&path).unwrap());
    let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("x.dwg").to_string();
    let drawing = t("parse (aad_io::read)", || aad_io::read(&bytes, &name).unwrap());
    let mut s = Session::new();
    t("open_drawing", || s.open_drawing(drawing, &name, None));
    let n = s.doc().unwrap().model.len();
    println!("entities: {n}");
    let doc = s.state().unwrap().doc.clone();
    let space = s.space();
    let opts = aad_engine::render::Options {
        tolerance: 0.5,
        min_dash: 1.0,
        text: true,
        fill: true,
        lineweights: true,
        view_height: 0.0,
        ..Default::default()
    };
    let list = t("display list (parallel)", || aad_engine::render::build(&doc, &space, &opts));
    println!("primitives: {} verts: {} tris: {}", list.prims.len(), list.verts.len(), list.tris.len());
    t("spatial index (first pick)", || aad_engine::select::pick(&doc, &space, aad_engine::geom::Vec2::new(1.0, 1.0), 0.5));
    t("pick x100 (warm)", || {
        for i in 0..100 {
            let _ = aad_engine::select::pick(&doc, &space, aad_engine::geom::Vec2::new(f64::from(i), f64::from(i)), 0.5);
        }
    });
    t("select all", || s.execute("selectall", &json!({})).ok());
    t("erase selection", || s.execute("erase.selection", &json!({})).ok());
    t("undo", || s.execute("undo", &json!({})).ok());
    t("audit", || s.execute("audit", &json!({})).ok());
    t("zoom extents", || s.execute("zoom.extents", &json!({})).ok());
    let bytes = t("write DXF", || aad_io::write(s.doc().unwrap(), "x.dxf").unwrap());
    println!("dxf bytes: {}", bytes.len());
    t("re-read DXF", || aad_io::read(&bytes, "x.dxf").unwrap());
}

#[test]
#[ignore]
fn which_objects_make_the_triangles() {
    let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
    let bytes = std::fs::read(&path).unwrap();
    let drawing = aad_io::read(&bytes, "x.dwg").unwrap();
    let opts = aad_engine::render::Options {
        tolerance: 0.5,
        min_dash: 1.0,
        text: true,
        fill: true,
        lineweights: true,
        view_height: 0.0,
        ..Default::default()
    };
    let list = aad_engine::render::build(&drawing, &aad_engine::doc::Space::Model, &opts);
    let mut by: std::collections::BTreeMap<String, (usize, usize, usize)> = Default::default();
    for p in &list.prims {
        let kind = drawing.entity(p.handle).map_or("(block content)".to_string(), |e| e.kind.type_name().to_string());
        let e = by.entry(kind).or_default();
        e.0 += 1;
        match p.kind {
            aad_engine::render::Kind::Tris | aad_engine::render::Kind::Mask => e.2 += p.len as usize,
            _ => e.1 += p.len as usize,
        }
    }
    println!("{:<22} {:>9} {:>12} {:>12}", "entity", "prims", "line verts", "tri verts");
    for (k, (n, v, t)) in by {
        println!("{k:<22} {n:>9} {v:>12} {t:>12}");
    }
}

#[test]
#[ignore]
fn display_list_cost_by_feature() {
    let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
    let drawing = aad_io::read(&std::fs::read(&path).unwrap(), "x.dwg").unwrap();
    for (label, text, fill) in [("all", true, true), ("no text", false, true), ("no fill", true, false), ("neither", false, false)] {
        let opts =
            aad_engine::render::Options { tolerance: 0.5, min_dash: 1.0, text, fill, lineweights: true, view_height: 0.0, ..Default::default() };
        let t0 = Instant::now();
        let list = aad_engine::render::build(&drawing, &aad_engine::doc::Space::Model, &opts);
        println!(
            "{label:<10} {:>8.0} ms  prims {:>8} lineverts {:>9} triverts {:>10}",
            t0.elapsed().as_secs_f64() * 1000.0,
            list.prims.len(),
            list.verts.len(),
            list.tris.len()
        );
    }
}

/// What an edit costs on a real drawing: the engine command, then what the shell does after it (rebuild the
/// retained scene from the changed tiles, or from scratch when that is refused). Same file as above:
/// `AAD_PERF_FILE=<drawing> cargo test --release -p aad-cli --test perf_probe edit_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn edit_cost_by_operation() {
    let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |b, name| aad_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| aad_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| aad_io::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    let bytes = std::fs::read(&path).unwrap();
    let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("x.dwg").to_string();
    let drawing = aad_io::read(&bytes, &name).unwrap();
    let mut s = Session::new();
    s.open_drawing(drawing, &name, None);
    s.execute("zoom.extents", &json!({})).ok();
    let view = s.state().unwrap().view();
    let px = view.height / 700.0;
    let band = aad_engine::render::tessellation_band(px);
    let lod = aad_engine::render::lod_flags_for_view(px.max(1e-300), 1.0);
    let opts = aad_engine::render::Options {
        tolerance: aad_engine::render::tessellation_tolerance(band),
        min_dash: px * 2.0,
        text: lod.text,
        fill: lod.fill,
        text_greek: lod.text_greek,
        fill_tint: lod.fill_tint,
        lineweights: true,
        view_height: view.height,
        view_center: Some(view.center),
        plot_style_table: None,
        screen_px: px,
    };
    let space = s.space();
    let old_doc = s.state().unwrap().doc.clone();
    let edge = aad_engine::render::scene_tile_edge(opts.view_height);
    println!(
        "entities {} | view height {:.0} | scene tiling viable: {}",
        old_doc.model.len(),
        view.height,
        aad_engine::render::scene_tiling_viable(&old_doc, edge)
    );
    let t0 = Instant::now();
    let old_scene = aad_engine::render::build_scene(&old_doc, &opts);
    println!("baseline scene build (view at extents) {:>8.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
    {
        // Why an edit may be refused: the inputs of each condition in `rebuild_scene`.
        let mut per_tile: std::collections::BTreeMap<_, usize> = std::collections::BTreeMap::new();
        for (_, id) in &old_scene.assignment {
            *per_tile.entry(*id).or_insert(0) += 1;
        }
        let largest = per_tile.values().max().copied().unwrap_or(0);
        println!(
            "scene: {} tiles, edge {:.0}, largest tile {} of {} entities (limit for a change: {}), expanded {} of {} (half the cap is the limit), cap on tiles {}",
            old_scene.tiles.len(),
            old_scene.edge,
            largest,
            old_doc.model.len(),
            old_doc.model.len() / 4 + 64,
            old_scene.expanded,
            aad_engine::render::MAX_BLOCK_EXPANSION,
            aad_engine::render::mesh::MAX_MESH_TILES
        );
    }
    let handles: Vec<String> = old_doc.model.iter().map(|e| e.handle.hex()).collect();
    println!("{:<26} {:>10} {:>16} {:>16} {:>12}", "operation", "engine ms", "rebuild_scene ms", "full rebuild ms", "first pick ms");
    for op in ["move", "copy"] {
        for n in [1usize, 100, 2_000, handles.len()] {
            let sel: Vec<&String> = handles.iter().take(n).collect();
            let t = Instant::now();
            let ok = s.execute(op, &json!({ "handles": sel, "delta": [5000.0, 0.0] })).is_ok();
            let engine = t.elapsed().as_secs_f64() * 1000.0;
            let new_doc = s.state().unwrap().doc.clone();
            let t = Instant::now();
            let inc = aad_engine::render::rebuild_scene(&old_scene, &old_doc, &new_doc, &opts);
            let inc_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let full = aad_engine::render::build_scene(&new_doc, &opts);
            let _list = aad_engine::render::flatten_scene(&full);
            let full_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let _ = aad_engine::select::pick(&new_doc, &space, aad_engine::geom::Vec2::new(1.0, 1.0), 0.5);
            let pick_ms = t.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{:<26} {:>10.1} {:>16} {:>16.1} {:>12.1}   {}",
                format!("{op} {n}"),
                engine,
                inc.map_or_else(|| format!("refused ({inc_ms:.0})"), |_| format!("{inc_ms:.1}")),
                full_ms,
                pick_ms,
                if ok { "" } else { "(command failed)" }
            );
            s.execute("undo", &json!({})).ok();
        }
    }
}

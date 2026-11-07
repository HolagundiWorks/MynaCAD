//! Phase C: tiled model-space scene — frustum skips far tiles; dirty rebuild; hostile caps.

use std::sync::Arc;

use aad_doc::{Common, Drawing, EntityKind, Line, Space};
use aad_geom::{Vec2, Vec3};
use aad_render::scene::{MAX_SCENE_TILES, build_scene, flatten_scene, rebuild_scene, scene_tile_edge, scene_tiling_viable};
use aad_render::{Options, build};

fn spaced_lines(n: usize, spacing: f64) -> Drawing {
    let mut d = Drawing::new_metric();
    for i in 0..n {
        let x = i as f64 * spacing;
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 1.0, 0.0, 0.0) })).unwrap();
    }
    d
}

#[test]
fn scene_flatten_matches_build_in_document_order() {
    let d = spaced_lines(10, 400.0);
    let opts = Options { view_height: 80.0, tolerance: 0.01, ..Default::default() };
    assert!(scene_tiling_viable(&d, scene_tile_edge(opts.view_height)));
    let scene = build_scene(&d, &opts);
    assert!(scene.tiles.len() >= 2);
    let flat = flatten_scene(&scene);
    let mono = build(&d, &Space::Model, &opts);
    assert_eq!(flat.prims.len(), mono.prims.len());
    let handles_a: Vec<_> = flat.prims.iter().map(|p| p.handle).collect();
    let handles_b: Vec<_> = mono.prims.iter().map(|p| p.handle).collect();
    assert_eq!(handles_a, handles_b, "flatten keeps document order");
}

#[test]
fn frustum_build_skips_far_tiles_but_keeps_extents() {
    let d = spaced_lines(5, 2500.0);
    let full = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
    let zoomed = Options { view_height: 100.0, view_center: Some(Vec2::new(0.5, 0.0)), tolerance: 0.01, ..Default::default() };
    let all = build_scene(&d, &full);
    let near = build_scene(&d, &zoomed);
    assert!(near.assignment.len() < all.assignment.len(), "frustum drops far entities from tessellation");
    assert!(near.extents.width() >= all.extents.width() * 0.9, "extents still span the model");
    let flat = flatten_scene(&near);
    assert!(!flat.prims.is_empty());
    assert!(flat.prims.len() < flatten_scene(&all).prims.len());
}

#[test]
fn dirty_rebuild_retains_clean_tile_arcs() {
    let old = spaced_lines(8, 500.0);
    let opts = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
    let scene = build_scene(&old, &opts);
    let before_ptrs: Vec<_> = scene.tiles.values().map(|t| Arc::as_ptr(&t.list) as usize).collect();
    let handles: Vec<_> = old.model.iter().map(|e| e.handle).collect();
    let mut new = old.clone();
    let edit = *handles.last().unwrap();
    new.model.modify(edit, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.x += 3.0;
        }
    });
    let next = rebuild_scene(&scene, &old, &new, &opts).expect("scene rebuild");
    assert_eq!(next.tiles_rebuilt, 1);
    let mut shared = 0usize;
    for t in next.tiles.values() {
        if before_ptrs.contains(&(Arc::as_ptr(&t.list) as usize)) {
            shared += 1;
        }
    }
    assert!(shared >= next.tiles.len().saturating_sub(1), "clean tiles keep their Arc");
    assert_eq!(flatten_scene(&next).prims.len(), build(&new, &Space::Model, &opts).prims.len());
}

#[test]
fn hostile_tile_count_is_capped() {
    // Extremely fine edge relative to a huge span → many tiles; viability must refuse.
    let d = spaced_lines(MAX_SCENE_TILES + 10, 1.0);
    let edge = 0.05;
    assert!(!scene_tiling_viable(&d, edge), "above MAX_SCENE_TILES must not claim tiling is viable");
    // build_scene still returns a usable single-tile fallback.
    let opts = Options { view_height: edge / 4.0, tolerance: 0.01, ..Default::default() };
    let scene = build_scene(&d, &opts);
    assert!(!scene.tiles.is_empty());
    assert_eq!(flatten_scene(&scene).prims.len(), build(&d, &Space::Model, &opts).prims.len());
}

/// A grid of short lines, `n` per side, `spacing` apart.
fn grid_of_lines(n: usize, spacing: f64) -> Drawing {
    let mut d = Drawing::new_metric();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (i as f64 * spacing, j as f64 * spacing);
            d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, y, 0.0), b: Vec3::new(x + 1.0, y, 0.0) })).unwrap();
        }
    }
    d
}

fn per_tile(scene: &aad_render::Scene) -> std::collections::BTreeMap<aad_render::mesh::TileId, usize> {
    let mut m = std::collections::BTreeMap::new();
    for (_, id) in &scene.assignment {
        *m.entry(*id).or_insert(0) += 1;
    }
    m
}

#[test]
fn a_crowded_grid_is_split_so_one_edit_dirties_one_small_tile() {
    // 3 600 lines in a 590 unit square. A view this tall would put them all in one tile, and an edit would rebuild all of it.
    let old = grid_of_lines(60, 10.0);
    let opts = Options { view_height: 1000.0, tolerance: 0.01, ..Default::default() };
    let scene = build_scene(&old, &opts);
    assert!(scene.tiles.len() >= 4, "the crowded tile is split: {} tiles", scene.tiles.len());
    assert!(per_tile(&scene).values().all(|n| *n <= 1000), "no tile is crowded: {:?}", per_tile(&scene));
    assert_eq!(scene.info.len(), old.model.len(), "every entity is measured once and kept");

    let handle = old.model.iter().nth(1234).unwrap().handle;
    let mut new = old.clone();
    new.model.modify(handle, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.y += 3.0;
        }
    });
    let next = rebuild_scene(&scene, &old, &new, &opts).expect("an edit in a crowded drawing is rebuilt, not refused");
    assert_eq!(next.tiles_rebuilt, 1, "only the tile of the edited line is tessellated again");
    assert_eq!(flatten_scene(&next).prims.len(), build(&new, &Space::Model, &opts).prims.len());
    assert_eq!(next.edge, scene.edge, "the grid stays put across edits");
}

#[test]
fn an_edit_is_never_refused_for_touching_too_much() {
    let old = grid_of_lines(40, 10.0);
    let opts = Options { view_height: 600.0, tolerance: 0.01, ..Default::default() };
    let scene = build_scene(&old, &opts);
    // Move every line: all tiles are dirty, which used to send the caller back to a full build.
    let mut new = old.clone();
    let handles: Vec<_> = old.model.iter().map(|e| e.handle).collect();
    for h in handles {
        new.model.modify(h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.a.x += 2.0;
                l.b.x += 2.0;
            }
        });
    }
    let next = rebuild_scene(&scene, &old, &new, &opts).expect("a rebuild of every tile is still a rebuild");
    assert_eq!(flatten_scene(&next).prims.len(), build(&new, &Space::Model, &opts).prims.len());
    let handles_a: Vec<_> = flatten_scene(&next).prims.iter().map(|p| p.handle).collect();
    let handles_b: Vec<_> = build(&new, &Space::Model, &opts).prims.iter().map(|p| p.handle).collect();
    assert_eq!(handles_a, handles_b, "document order is kept");
}

#[test]
fn removed_and_added_entities_update_the_tiles_and_the_extents() {
    let old = grid_of_lines(60, 10.0);
    let opts = Options { view_height: 1000.0, tolerance: 0.01, ..Default::default() };
    let scene = build_scene(&old, &opts);
    assert!(scene.tiles.len() >= 4, "split into tiles, so this exercises the incremental path");
    let mut new = old.clone();
    // Remove the lines at the far corner and add one near the origin.
    let far: Vec<_> = old.model.iter().filter(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x > 450.0)).map(|e| e.handle).collect();
    assert!(!far.is_empty());
    for h in &far {
        new.remove_entity(*h);
    }
    new.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(5.0, 5.0, 0.0), b: Vec3::new(6.0, 5.0, 0.0) })).unwrap();
    let next = rebuild_scene(&scene, &old, &new, &opts).expect("scene rebuild");
    let fresh = build_scene(&new, &opts);
    assert_eq!(next.info.len(), new.model.len());
    assert_eq!(next.assignment.len(), fresh.assignment.len());
    assert_eq!(flatten_scene(&next).prims.len(), flatten_scene(&fresh).prims.len());
    assert!((next.extents.max.x - fresh.extents.max.x).abs() < 1e-9, "extents shrink with the model: {:?} vs {:?}", next.extents, fresh.extents);
}

#[test]
fn a_tiny_drawing_is_not_tiled_and_a_spaced_one_is() {
    let one = spaced_lines(1, 10.0);
    let opts = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
    assert!(aad_render::scene::build_scene_if_viable(&one, &opts).is_none());
    let many = spaced_lines(10, 400.0);
    assert!(aad_render::scene::build_scene_if_viable(&many, &opts).is_some());
    // The fallback scene keeps no per-entity data, so an edit asks for a full build instead of reusing it.
    let scene = build_scene(&one, &opts);
    let mut edited = one.clone();
    let h = one.model.iter().next().unwrap().handle;
    edited.model.modify(h, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.x += 1.0;
        }
    });
    assert!(rebuild_scene(&scene, &one, &edited, &opts).is_none());
}

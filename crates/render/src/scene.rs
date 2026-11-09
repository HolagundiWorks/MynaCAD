//! Tiled model-space display-list scene (Phase C).
//!
//! Entities are assigned to a world grid by bounds centroid. Each tile holds an [`Arc`] of its
//! [`DisplayList`] fragment so rebuilds can remesh / retessellate only dirty tiles. Flattening
//! restores document draw order. Rays/xlines without a finite centroid land in [`GLOBAL_TILE`].
//!
//! Every model entity's tile and bounds are measured once (in parallel) and kept in [`Scene::info`]. An edit
//! measures only the entities that changed and reuses the rest, so a rebuild costs what the dirty tiles cost
//! and not another pass over a drawing of text, hatches and block references.
//!
//! Layout viewport model tessellation uses the same process-wide LRU ([`crate::vp_model_get`]) —
//! no longer `thread_local` — so worker threads can share it.
//!
//! Phase D text/hatch retention uses process-wide content-keyed caches
//! ([`crate::glyph_mesh`], hatch pattern expansion in [`crate::hatch`]). Dirty-tile
//! [`rebuild_scene`] does not flush them; a new tessellation band selects different keys.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use aad_doc::{Drawing, Entity, EntityKind, Handle, Space};
use aad_geom::{Bounds2, Mat3, Vec2};
use rayon::prelude::*;

use crate::mesh::{MAX_MESH_TILES, MIN_MESH_TILES, TileId, mesh_tile_edge, tile_id_at};
use crate::{Builder, DisplayList, MAX_BLOCK_EXPANSION, Options, PARALLEL_CHUNK, append, entity, model_cull_window, top_ctx, with_point_view};

/// Sentinel tile for infinites / entities without a finite centroid.
pub const GLOBAL_TILE: TileId = TileId { x: i32::MIN, y: i32::MIN };

/// Cap on scene tiles (hostile extents); above this, callers should use monolithic [`crate::build`].
pub const MAX_SCENE_TILES: usize = MAX_MESH_TILES;

/// Prefer tiling when at least this many non-empty tiles would be produced.
pub const MIN_SCENE_TILES: usize = MIN_MESH_TILES;

/// One retained tile: Arc so clean tiles survive a dirty rebuild without cloning prims.
#[derive(Clone, Debug)]
pub struct SceneTile {
    pub id: TileId,
    pub list: Arc<DisplayList>,
}

/// Where one model entity sits in the scene grid. Kept on the scene so an edit measures only what changed.
#[derive(Clone, Copy, Debug)]
pub struct EntInfo {
    pub tile: TileId,
    /// The entity's bounds; empty for a ray or anything with nothing to draw.
    pub bounds: Bounds2,
    /// False when the view window left it out of the scene.
    pub included: bool,
}

/// Model-space scene: per-tile display lists plus a document-order assignment for flattening.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// Every model entity's tile, bounds and inclusion (`included == false` for those outside the view window).
    pub info: HashMap<Handle, EntInfo>,
    pub edge: f64,
    pub tiles: BTreeMap<TileId, SceneTile>,
    /// Handles in model iteration order with the tile they were built into.
    pub assignment: Vec<(Handle, TileId)>,
    /// Cheap AABB of every model entity (including those skipped by the frustum).
    pub extents: Bounds2,
    pub expanded: usize,
    /// Tiles tessellated on the last [`build_scene`] / [`rebuild_scene`] (tests / inspect).
    pub tiles_rebuilt: usize,
    /// The view window the `included` flags were decided against. An edit can patch the scene only while the window is the same.
    pub cull: Option<Bounds2>,
}

/// World-space tile edge for the scene grid (same policy as mesh tiles).
pub fn scene_tile_edge(view_height: f64) -> f64 {
    mesh_tile_edge(view_height)
}

/// Whether model-space scene tiling is worthwhile for this drawing and edge.
pub fn scene_tiling_viable(d: &Drawing, edge: f64) -> bool {
    if !(edge.is_finite() && edge > 0.0) || d.model.is_empty() {
        return false;
    }
    tile_count_ok(measure_all(d, edge, None).iter().map(|(i, _)| i.tile))
}

/// The number of distinct tiles is in the range tiling is worthwhile for (and not hostile).
fn tile_count_ok(tiles: impl Iterator<Item = TileId>) -> bool {
    let seen: HashSet<TileId> = tiles.collect();
    (MIN_SCENE_TILES..=MAX_SCENE_TILES).contains(&seen.len())
}

/// An entity's bounds, and whether it is a ray or xline (no finite centre: it lives in the global tile).
fn bounds_of(d: &Drawing, e: &Entity) -> (Bounds2, bool) {
    (aad_doc::entity_bounds(d, e, 0), matches!(e.kind, EntityKind::Ray(_) | EntityKind::XLine(_)))
}

/// Place a measured entity: the tile its centre falls in, and whether the view window keeps it.
fn place(bounds: Bounds2, ray: bool, edge: f64, cull: Option<&Bounds2>) -> EntInfo {
    let tile = if ray || bounds.is_empty() { GLOBAL_TILE } else { tile_id_at(bounds.center(), edge).unwrap_or(GLOBAL_TILE) };
    let mut included = true;
    if let Some(win) = cull
        && tile != GLOBAL_TILE
    {
        included = if bounds.is_empty() { tile_intersects_cull(tile, edge, win) } else { bounds.intersects(win) };
    }
    EntInfo { tile, bounds, included }
}

fn classify(d: &Drawing, e: &Entity, edge: f64, cull: Option<&Bounds2>) -> EntInfo {
    let (bounds, ray) = bounds_of(d, e);
    place(bounds, ray, edge, cull)
}

/// No tile should hold more entities than this when the drawing allows it: an edit rebuilds the tiles it touches.
fn crowded_limit(entities: usize) -> usize {
    (entities / 24).max(1_000)
}

/// The largest tile's entity count and the number of tiles when these entities are gridded at `edge`.
fn bucket_stats(measured: &[(Bounds2, bool)], edge: f64) -> (usize, usize) {
    let mut count: HashMap<TileId, usize> = HashMap::new();
    for (b, ray) in measured {
        if !*ray && !b.is_empty() {
            *count.entry(tile_id_at(b.center(), edge).unwrap_or(GLOBAL_TILE)).or_insert(0) += 1;
        }
    }
    (count.values().copied().max().unwrap_or(0), count.len())
}

/// The grid edge for these entities: the view's edge, halved while a tile is crowded and the finer grid still fits well
/// under the tile cap (a halving can quadruple the tile count, so the next step is checked before it is taken). At full
/// zoom-out the view's edge puts a whole drawing in two or three tiles, and an edit then rebuilds most of it.
fn refine_edge(measured: &[(Bounds2, bool)], base: f64) -> f64 {
    let limit = crowded_limit(measured.len());
    let min_edge = base / 1024.0;
    let mut edge = base;
    loop {
        let (largest, _) = bucket_stats(measured, edge);
        let next = edge * 0.5;
        if largest <= limit || next < min_edge {
            return edge;
        }
        let (_, next_tiles) = bucket_stats(measured, next);
        if next_tiles + 1 > MAX_SCENE_TILES / 2 {
            return edge;
        }
        edge = next;
    }
}

/// Measure every model entity, in document order and in parallel (the second field is always `false`: none is reused).
fn measure_all(d: &Drawing, edge: f64, cull: Option<&Bounds2>) -> Vec<(EntInfo, bool)> {
    let ents: Vec<&Arc<Entity>> = d.model.iter().collect();
    if ents.len() >= PARALLEL_CHUNK * 2 {
        ents.par_iter().map(|e| (classify(d, e, edge, cull), false)).collect()
    } else {
        ents.iter().map(|e| (classify(d, e, edge, cull), false)).collect()
    }
}

fn tile_intersects_cull(id: TileId, edge: f64, cull: &Bounds2) -> bool {
    if id == GLOBAL_TILE {
        return true;
    }
    if !(edge.is_finite() && edge > 0.0) || cull.is_empty() {
        return true;
    }
    let min = Vec2::new(f64::from(id.x) * edge, f64::from(id.y) * edge);
    let max = min + Vec2::new(edge, edge);
    Bounds2::new(min, max).intersects(cull)
}

fn build_entities_list(d: &Drawing, ents: &[&Entity], opts: &Options, plotting: bool, budget: &AtomicUsize) -> DisplayList {
    aad_fonts::with_world_chord_tol(opts.tolerance, || {
        let mut b = Builder { list: DisplayList::default(), opts, plotting, styles: None, named: None, budget: Some(budget) };
        for e in ents {
            entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
        }
        b.list
    })
}

/// A tile's entities, the handle order for flattening, and the whole model's extents.
type Assembled<'a> = (BTreeMap<TileId, Vec<&'a Entity>>, Vec<(Handle, TileId)>, Bounds2);

/// Group the entities the window keeps by tile (document order preserved within each tile) from their measurements.
fn assemble<'a>(d: &'a Drawing, infos: &[(EntInfo, bool)]) -> Assembled<'a> {
    let mut map: BTreeMap<TileId, Vec<&Entity>> = BTreeMap::new();
    let mut assignment = Vec::new();
    let mut extents = Bounds2::default();
    for (e, (info, _)) in d.model.iter().zip(infos) {
        if !info.bounds.is_empty() {
            extents = extents.union(&info.bounds);
        }
        if info.included {
            assignment.push((e.handle, info.tile));
            map.entry(info.tile).or_default().push(e);
        }
    }
    (map, assignment, extents)
}

fn info_map(d: &Drawing, infos: &[(EntInfo, bool)]) -> HashMap<Handle, EntInfo> {
    d.model.iter().zip(infos).map(|(e, (i, _))| (e.handle, *i)).collect()
}

/// Tessellate one tile; a big one is cut into chunks that run in parallel and are joined in order.
fn tessellate_one(d: &Drawing, ents: &[&Entity], opts: &Options, plotting: bool, budget: &AtomicUsize) -> DisplayList {
    if ents.len() >= PARALLEL_CHUNK * 2 {
        let parts: Vec<DisplayList> = ents.par_chunks(PARALLEL_CHUNK).map(|chunk| build_entities_list(d, chunk, opts, plotting, budget)).collect();
        let mut acc = DisplayList::default();
        for part in parts {
            append(&mut acc, part);
        }
        acc
    } else {
        build_entities_list(d, ents, opts, plotting, budget)
    }
}

fn tessellate_tiles(
    d: &Drawing,
    map: &BTreeMap<TileId, Vec<&Entity>>,
    opts: &Options,
    plotting: bool,
) -> (BTreeMap<TileId, SceneTile>, usize, usize) {
    let budget = AtomicUsize::new(0);
    let keys: Vec<TileId> = map.keys().copied().collect();
    let one = |id: &TileId| -> Option<(TileId, DisplayList)> {
        let ents = map.get(id)?;
        if ents.is_empty() {
            return None;
        }
        Some((*id, tessellate_one(d, ents, opts, plotting, &budget)))
    };
    let built: Vec<(TileId, DisplayList)> = if keys.len() >= 2 && map.values().map(Vec::len).sum::<usize>() >= 256 {
        keys.par_iter().filter_map(one).collect()
    } else {
        keys.iter().filter_map(one).collect()
    };
    let rebuilt = built.len();
    let mut tiles = BTreeMap::new();
    let mut expanded = 0usize;
    for (id, list) in built {
        expanded = expanded.saturating_add(list.expanded);
        tiles.insert(id, SceneTile { id, list: Arc::new(list) });
    }
    (tiles, expanded, rebuilt)
}

/// Build a model-space scene. Paper space and plotting should keep using [`crate::build`] /
/// [`crate::build_plot`]. When `opts.view_center` is set, only tiles intersecting the frustum
/// (plus global infinites) are tessellated; [`Scene::extents`] still covers the whole model.
pub fn build_scene(d: &Drawing, opts: &Options) -> Scene {
    if let Some(scene) = build_scene_if_viable(d, opts) {
        return scene;
    }
    let opts = with_point_view(d, &Space::Model, opts);
    let edge = scene_tile_edge(opts.view_height);
    // Single-tile fallback: one GLOBAL tile with a full model build so callers always get a Scene.
    let list = crate::build(d, &Space::Model, &opts);
    let mut tiles = BTreeMap::new();
    let assignment: Vec<(Handle, TileId)> = d.model.iter().map(|e| (e.handle, GLOBAL_TILE)).collect();
    let extents = list.bounds;
    let expanded = list.expanded;
    tiles.insert(GLOBAL_TILE, SceneTile { id: GLOBAL_TILE, list: Arc::new(list) });
    Scene { info: HashMap::new(), edge, tiles, assignment, extents, expanded, tiles_rebuilt: 1, cull: None }
}

/// [`build_scene`] when tiling is worthwhile for this drawing, else `None`. The entities are measured once for both
/// the decision and the tiles; asking [`scene_tiling_viable`] first and then building measured them twice.
pub fn build_scene_if_viable(d: &Drawing, opts: &Options) -> Option<Scene> {
    let opts = with_point_view(d, &Space::Model, opts);
    let base = scene_tile_edge(opts.view_height);
    if !(base.is_finite() && base > 0.0) || d.model.is_empty() {
        return None;
    }
    let cull = model_cull_window(&opts);
    let ents: Vec<&Arc<Entity>> = d.model.iter().collect();
    let measured: Vec<(Bounds2, bool)> = if ents.len() >= PARALLEL_CHUNK * 2 {
        ents.par_iter().map(|e| bounds_of(d, e)).collect()
    } else {
        ents.iter().map(|e| bounds_of(d, e)).collect()
    };
    let edge = refine_edge(&measured, base);
    let infos: Vec<(EntInfo, bool)> = measured.iter().map(|(b, ray)| (place(*b, *ray, edge, cull.as_ref()), false)).collect();
    if !tile_count_ok(infos.iter().map(|(i, _)| i.tile)) {
        return None;
    }
    let (map, assignment, extents) = assemble(d, &infos);
    let (tiles, expanded, rebuilt) = tessellate_tiles(d, &map, &opts, false);
    Some(Scene { info: info_map(d, &infos), edge, tiles, assignment, extents, expanded, tiles_rebuilt: rebuilt, cull })
}

/// Flatten a scene into a single display list in document order (handle assignment order).
pub fn flatten_scene(scene: &Scene) -> DisplayList {
    // Index each tile's prims by handle (contiguous runs).
    let mut runs: HashMap<Handle, (TileId, usize, usize)> = HashMap::new();
    for (id, tile) in &scene.tiles {
        let list = tile.list.as_ref();
        let mut i = 0usize;
        while i < list.prims.len() {
            let h = list.prims[i].handle;
            let mut j = i + 1;
            while list.prims.get(j).is_some_and(|p| p.handle == h) {
                j += 1;
            }
            runs.insert(h, (*id, i, j));
            i = j;
        }
    }
    let mut out = DisplayList { expanded: scene.expanded, ..DisplayList::default() };
    out.bounds = scene.extents;
    for (handle, tile_id) in &scene.assignment {
        let Some((id, a, b)) = runs.get(handle).copied() else { continue };
        if id != *tile_id {
            continue;
        }
        let Some(tile) = scene.tiles.get(tile_id) else { continue };
        let list = tile.list.as_ref();
        let Some(prims) = list.prims.get(a..b) else { continue };
        let is_tris = |p: &crate::DPrim| matches!(p.kind, crate::Kind::Tris | crate::Kind::Mask);
        let span = |tris: bool| -> Option<(u32, u32)> {
            let mut it = prims.iter().filter(|p| is_tris(p) == tris);
            let first = it.next()?.start;
            let end = prims.iter().filter(|p| is_tris(p) == tris).map(|p| p.start + p.len).max()?;
            Some((first, end))
        };
        let (vo, to) = (out.verts.len() as i64, out.tris.len() as i64);
        let vspan = span(false);
        let tspan = span(true);
        if let Some((s, e)) = vspan
            && let Some(slice) = list.verts.get(s as usize..e as usize)
        {
            out.verts.extend_from_slice(slice);
        }
        if let Some((s, e)) = tspan
            && let Some(slice) = list.tris.get(s as usize..e as usize)
        {
            out.tris.extend_from_slice(slice);
        }
        for p in prims {
            let mut p = *p;
            let (base, off) = if is_tris(&p) { (tspan.map(|s| s.0).unwrap_or(0), to) } else { (vspan.map(|s| s.0).unwrap_or(0), vo) };
            if let Ok(start) = u32::try_from(i64::from(p.start) - i64::from(base) + off) {
                p.start = start;
                out.prims.push(p);
            }
        }
    }
    // Prefer tight bounds from tessellated prims when we have them; keep extents as a floor.
    if !out.prims.is_empty() {
        let mut b = Bounds2::default();
        for p in &out.prims {
            for q in out.points(p) {
                b.add(*q);
            }
        }
        if !b.is_empty() {
            out.bounds = b.union(&scene.extents);
        }
    }
    out
}

/// Update `old` for `new_doc`: unchanged tiles keep their `Arc` lists; only dirty tiles retessellate. Only the entities
/// that changed (or are new) are measured; every other entity keeps the tile and bounds stored on `old`.
/// `None` when the environment changed or the scene cannot be reused (caller should [`build_scene`]).
///
/// A small edit takes the delta path ([`rebuild_scene_delta`]): the changed and erased entities come from comparing the shared
/// chunks of the two entity stores, so the cost follows the edit and not the size of the drawing.
pub fn rebuild_scene(old: &Scene, old_doc: &Drawing, new_doc: &Drawing, opts: &Options) -> Option<Scene> {
    if let Some(scene) = rebuild_scene_delta(old, old_doc, new_doc, opts) {
        return Some(scene);
    }
    rebuild_scene_full(old, old_doc, new_doc, opts)
}

/// The edit-sized rebuild. `None` (use [`rebuild_scene_full`]) when the environment or the view window changed, the scene keeps
/// no per-entity data, or so much changed that comparing everything is no dearer.
pub(crate) fn rebuild_scene_delta(old: &Scene, old_doc: &Drawing, new_doc: &Drawing, opts: &Options) -> Option<Scene> {
    if old.info.is_empty() || old.expanded >= MAX_BLOCK_EXPANSION / 2 || !crate::same_environment(old_doc, new_doc) {
        return None;
    }
    if new_doc.header.f64("PDSIZE", 0.0) <= 0.0 && crate::point::sized(new_doc.header.i64("PDMODE", 0)) {
        return None;
    }
    let opts = with_point_view(new_doc, &Space::Model, opts);
    let edge = old.edge;
    if !(edge.is_finite() && edge > 0.0) || new_doc.model.is_empty() {
        return None;
    }
    let cull = model_cull_window(&opts);
    if cull != old.cull {
        return None;
    }
    let changed = new_doc.model.changed_since(&old_doc.model);
    let gone = new_doc.model.removed_since(&old_doc.model);
    if changed.len() + gone.len() > (old.info.len() / 8).max(256) {
        return None;
    }
    // Patch the per-entity data: erased entities go, changed and new ones are measured again, the rest is kept as it was.
    let mut info = old.info.clone();
    let mut dirty: HashSet<TileId> = HashSet::new();
    for h in &gone {
        if let Some(prev) = info.remove(h)
            && prev.included
        {
            dirty.insert(prev.tile);
        }
    }
    let measured: Vec<EntInfo> = if changed.len() >= 256 {
        changed.par_iter().map(|e| classify(new_doc, e, edge, cull.as_ref())).collect()
    } else {
        changed.iter().map(|e| classify(new_doc, e, edge, cull.as_ref())).collect()
    };
    for (e, now) in changed.iter().zip(measured) {
        if let Some(prev) = old.info.get(&e.handle)
            && prev.included
        {
            dirty.insert(prev.tile);
        }
        if now.included {
            dirty.insert(now.tile);
        }
        info.insert(e.handle, now);
    }
    if info.len() != new_doc.model.len() {
        return None;
    }
    // Draw-order assignment and, for the dirty tiles only, their entities in draw order: one parallel pass over the chunks.
    let slices: Vec<&[Arc<Entity>]> = new_doc.model.chunk_slices().map(|(_, s)| s).collect();
    let parts: Vec<(Vec<(Handle, TileId)>, Vec<(TileId, &Entity)>)> = slices
        .par_iter()
        .map(|chunk| {
            let mut assigned = Vec::with_capacity(chunk.len());
            let mut members = Vec::new();
            for e in *chunk {
                if let Some(i) = info.get(&e.handle)
                    && i.included
                {
                    assigned.push((e.handle, i.tile));
                    if dirty.contains(&i.tile) {
                        members.push((i.tile, e.as_ref()));
                    }
                }
            }
            (assigned, members)
        })
        .collect();
    let mut assignment = Vec::with_capacity(old.assignment.len() + changed.len());
    let mut members: BTreeMap<TileId, Vec<&Entity>> = BTreeMap::new();
    for (assigned, mem) in parts {
        assignment.extend(assigned);
        for (tile, e) in mem {
            members.entry(tile).or_default().push(e);
        }
    }
    let extents = info.values().filter(|i| !i.bounds.is_empty()).fold(Bounds2::default(), |a, i| a.union(&i.bounds));
    // The tile set after the edit: untouched tiles stay, dirty tiles stay only if something is left in them.
    let mut live: HashSet<TileId> = old.tiles.keys().copied().filter(|id| !dirty.contains(id)).collect();
    live.extend(members.keys().copied());
    if !tile_count_ok(live.iter().copied()) {
        return None;
    }
    let budget = AtomicUsize::new(old.expanded.min(MAX_BLOCK_EXPANSION / 2));
    let todo: Vec<(&TileId, &Vec<&Entity>)> = members.iter().collect();
    let rebuilt: Vec<(TileId, DisplayList)> =
        todo.par_iter().map(|(id, ents)| (**id, tessellate_one(new_doc, ents, &opts, false, &budget))).collect();
    let rebuilt_count = rebuilt.len();
    let mut tiles = old.tiles.clone();
    for id in &dirty {
        tiles.remove(id);
    }
    for (id, list) in rebuilt {
        tiles.insert(id, SceneTile { id, list: Arc::new(list) });
    }
    let expanded = tiles.values().fold(0usize, |a, t| a.saturating_add(t.list.expanded));
    Some(Scene { info, edge, tiles, assignment, extents, expanded, tiles_rebuilt: rebuilt_count, cull })
}

/// The rebuild that looks at every entity: the fallback of [`rebuild_scene`], and the reference the delta path is tested against.
pub(crate) fn rebuild_scene_full(old: &Scene, old_doc: &Drawing, new_doc: &Drawing, opts: &Options) -> Option<Scene> {
    if old.expanded >= MAX_BLOCK_EXPANSION / 2 || !crate::same_environment(old_doc, new_doc) {
        return None;
    }
    if new_doc.header.f64("PDSIZE", 0.0) <= 0.0 && crate::point::sized(new_doc.header.i64("PDMODE", 0)) {
        return None;
    }
    // A scene built as the single-tile fallback keeps no per-entity data to reuse.
    if old.info.is_empty() && !new_doc.model.is_empty() {
        return None;
    }
    let opts = with_point_view(new_doc, &Space::Model, opts);
    let edge = if old.edge.is_finite() && old.edge > 0.0 { old.edge } else { scene_tile_edge(opts.view_height) };
    if !(edge.is_finite() && edge > 0.0) || new_doc.model.is_empty() {
        return None;
    }
    let cull = model_cull_window(&opts);
    // Reuse what is untouched (the same `Arc` as before); measure the rest.
    let ents: Vec<&Arc<Entity>> = new_doc.model.iter().collect();
    let measure = |e: &&Arc<Entity>| -> (EntInfo, bool) {
        match old.info.get(&e.handle) {
            Some(i) if old_doc.model.get(e.handle).is_some_and(|o| Arc::ptr_eq(o, e)) => (*i, true),
            _ => (classify(new_doc, e, edge, cull.as_ref()), false),
        }
    };
    let infos: Vec<(EntInfo, bool)> =
        if ents.len() >= PARALLEL_CHUNK * 2 { ents.par_iter().map(measure).collect() } else { ents.iter().map(measure).collect() };
    if !tile_count_ok(infos.iter().map(|(i, _)| i.tile)) {
        return None;
    }
    let (new_map, assignment, extents) = assemble(new_doc, &infos);
    if new_map.len() > MAX_SCENE_TILES {
        return None;
    }

    let mut dirty: HashSet<TileId> = HashSet::new();
    for (e, (info, unchanged)) in ents.iter().zip(&infos) {
        if *unchanged {
            continue;
        }
        // A changed or moved entity dirties the tile it was in and the tile it is in now.
        if let Some(prev) = old.info.get(&e.handle)
            && prev.included
        {
            dirty.insert(prev.tile);
        }
        if info.included {
            dirty.insert(info.tile);
        }
    }
    // Removed entities dirty their previous tile.
    for (h, prev) in &old.info {
        if prev.included && new_doc.model.get(*h).is_none() {
            dirty.insert(prev.tile);
        }
    }
    // Tiles that disappeared from the partition are dropped (not dirty rebuild).
    for id in old.tiles.keys() {
        if !new_map.contains_key(id) {
            dirty.insert(*id);
        }
    }

    // However many tiles are dirty the rebuild is no dearer than a full build now that nothing is measured twice,
    // so there is no "too much changed" refusal.
    let budget = AtomicUsize::new(old.expanded.min(MAX_BLOCK_EXPANSION / 2));
    let dirty_ids: Vec<TileId> = dirty.into_iter().filter(|id| new_map.contains_key(id)).collect();
    let rebuilt_parts: Vec<(TileId, DisplayList)> = dirty_ids
        .par_iter()
        .filter_map(|id| {
            let ents = new_map.get(id)?;
            Some((*id, tessellate_one(new_doc, ents, &opts, false, &budget)))
        })
        .collect();

    let mut tiles = BTreeMap::new();
    for (id, ents) in &new_map {
        if ents.is_empty() {
            continue;
        }
        if let Some((_, list)) = rebuilt_parts.iter().find(|(i, _)| i == id) {
            tiles.insert(*id, SceneTile { id: *id, list: Arc::new(list.clone()) });
        } else if let Some(prev) = old.tiles.get(id) {
            tiles.insert(*id, prev.clone());
        } else {
            // Should not happen: new tile must be dirty. Build serially as a fallback.
            let list = tessellate_one(new_doc, ents, &opts, false, &budget);
            tiles.insert(*id, SceneTile { id: *id, list: Arc::new(list) });
        }
    }
    let expanded = tiles.values().fold(0usize, |a, t| a.saturating_add(t.list.expanded));
    Some(Scene { info: info_map(new_doc, &infos), edge, tiles, assignment, extents, expanded, tiles_rebuilt: rebuilt_parts.len(), cull })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use aad_doc::{Common, EntityKind, Line};
    use aad_geom::Vec3;

    fn line_drawing(n: usize, spacing: f64) -> Drawing {
        let mut d = Drawing::new_metric();
        for i in 0..n {
            let x = i as f64 * spacing;
            d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 1.0, 0.0, 0.0) })).unwrap();
        }
        d
    }

    #[test]
    fn flatten_matches_monolithic_build_without_frustum() {
        let d = line_drawing(8, 500.0);
        let opts = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
        let scene = build_scene(&d, &opts);
        assert!(scene.tiles.len() >= 2, "spaced lines must tile");
        let flat = flatten_scene(&scene);
        let mono = crate::build(&d, &Space::Model, &opts);
        assert_eq!(flat.prims.len(), mono.prims.len());
        for (a, b) in flat.prims.iter().zip(&mono.prims) {
            assert_eq!(a.handle, b.handle);
            assert_eq!(a.kind, b.kind);
        }
    }

    #[test]
    fn frustum_skips_far_tiles() {
        let d = line_drawing(4, 2000.0);
        let full = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
        let zoomed = Options { view_height: 100.0, view_center: Some(Vec2::new(0.5, 0.0)), tolerance: 0.01, ..Default::default() };
        let all = build_scene(&d, &full);
        let near = build_scene(&d, &zoomed);
        assert!(near.tiles.len() < all.tiles.len() || near.assignment.len() < all.assignment.len(), "frustum must drop far tiles");
        assert!(!near.extents.is_empty(), "extents still cover the model");
        // Extents should still span the far geometry.
        assert!(near.extents.width() > 1000.0, "extents width {}", near.extents.width());
    }

    /// Everything that decides what is drawn, in a form two scenes can be compared by.
    fn same_scene(a: &Scene, b: &Scene) {
        assert_eq!(a.info.len(), b.info.len(), "entity count");
        for (h, ia) in &a.info {
            let ib = b.info.get(h).unwrap_or_else(|| panic!("{h:?} missing"));
            assert_eq!((ia.tile, ia.included), (ib.tile, ib.included), "{h:?}");
            assert_eq!(ia.bounds, ib.bounds, "{h:?}");
        }
        assert_eq!(a.assignment, b.assignment, "draw-order assignment");
        assert_eq!(a.extents, b.extents, "extents");
        assert_eq!(a.tiles.keys().collect::<Vec<_>>(), b.tiles.keys().collect::<Vec<_>>(), "tile set");
        let (fa, fb) = (flatten_scene(a), flatten_scene(b));
        assert_eq!(fa.prims.len(), fb.prims.len(), "prims");
        for (x, y) in fa.prims.iter().zip(&fb.prims) {
            assert_eq!((x.handle, x.kind, x.start, x.len), (y.handle, y.kind, y.start, y.len));
        }
        assert_eq!((fa.verts.len(), fa.tris.len()), (fb.verts.len(), fb.tris.len()));
    }

    /// The delta rebuild must give exactly what the rebuild that looks at every entity gives, across moves, additions, erasures
    /// and draw-order changes, applied in runs and undone.
    #[test]
    fn the_delta_rebuild_equals_the_full_rebuild() {
        let opts = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
        let mut doc = line_drawing(60, 500.0);
        let mut scene = build_scene(&doc, &opts);
        assert!(scene.tiles.len() >= 20);
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut history: Vec<(Drawing, Scene)> = Vec::new();
        let mut taken = 0;
        for round in 0..60 {
            let before = doc.clone();
            let handles: Vec<Handle> = doc.model.iter().map(|e| e.handle).collect();
            for _ in 0..(1 + next(4)) {
                let h = handles[next(handles.len() as u64) as usize];
                match next(5) {
                    0 | 1 => {
                        // move (sometimes far, into another tile)
                        let dx = if next(2) == 0 { 3.0 } else { 700.0 * next(9) as f64 };
                        doc.model.modify(h, |e| {
                            if let EntityKind::Line(l) = &mut e.kind {
                                l.a.x += dx;
                                l.b.x += dx;
                            }
                        });
                    }
                    2 => {
                        let x = 500.0 * next(80) as f64;
                        let _ = doc.add(
                            &Space::Model,
                            Common::default(),
                            EntityKind::Line(Line { a: Vec3::new(x, 40.0, 0.0), b: Vec3::new(x + 1.0, 40.0, 0.0) }),
                        );
                    }
                    3 => {
                        if doc.model.len() > 30 {
                            doc.model.remove(h);
                        }
                    }
                    _ => {
                        doc.model.bring_to_front(h);
                    }
                }
            }
            let delta = rebuild_scene_delta(&scene, &before, &doc, &opts);
            let full = rebuild_scene_full(&scene, &before, &doc, &opts);
            match (delta, full) {
                (Some(d), Some(f)) => {
                    same_scene(&d, &f);
                    taken += 1;
                    scene = d;
                }
                (None, Some(f)) => scene = f,
                (d, f) => panic!("round {round}: delta {} full {}", d.is_some(), f.is_some()),
            }
            history.push((before, scene.clone()));
            // Now and then step back to an earlier state (undo): the delta path must handle a jump over several edits.
            if round % 11 == 10 {
                let (old_doc, old_scene) = history.remove(history.len() - 4);
                let delta = rebuild_scene_delta(&scene, &doc, &old_doc, &opts);
                let full = rebuild_scene_full(&scene, &doc, &old_doc, &opts);
                if let (Some(d), Some(f)) = (&delta, &full) {
                    same_scene(d, f);
                }
                doc = old_doc;
                scene = delta.or(full).unwrap_or(old_scene);
            }
        }
        assert!(taken >= 45, "the delta path should take nearly every small edit, took {taken} of 60");
    }

    #[test]
    fn rebuild_only_retessellates_dirty_tiles() {
        let old = line_drawing(6, 500.0);
        let opts = Options { view_height: 100.0, tolerance: 0.01, ..Default::default() };
        let scene = build_scene(&old, &opts);
        assert!(scene.tiles.len() >= 2);
        let total = scene.tiles.len();
        let cold = scene.tiles_rebuilt;
        assert_eq!(cold, total);

        let handles: Vec<_> = old.model.iter().map(|e| e.handle).collect();
        let mut new = old.clone();
        let edit = handles[handles.len() - 1];
        new.model.modify(edit, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.b.x += 5.0;
            }
        });
        let next = rebuild_scene(&scene, &old, &new, &opts).expect("incremental");
        assert_eq!(next.tiles_rebuilt, 1, "only the dirty tile rebuilds, got {}", next.tiles_rebuilt);
        assert_eq!(next.tiles.len(), scene.tiles.len());
        let flat = flatten_scene(&next);
        let mono = crate::build(&new, &Space::Model, &opts);
        assert_eq!(flat.prims.len(), mono.prims.len());
    }
}

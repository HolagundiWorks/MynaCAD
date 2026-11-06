//! Levels and sheet sets in DXF/DWG files (design: `docs/levels-and-sheet-sets.md`, "DXF and DWG
//! round-trip").
//!
//! The file stays an ordinary DXF: the first level in tab order is model space, every other level
//! is a block `ODRAFT$LVL$<id>`, a sheet set's annotation of a level is a block
//! `ODRAFT$ANN$<set>$<level>`, one INSERT of each such block sits in model space on the frozen,
//! non-plotting layer `ODRAFT-STORES` (so `PURGE` elsewhere keeps the blocks), and the level and
//! sheet-set records ride in the project data under `levels2`. Reading reverses this. Damaged or
//! missing metadata never fails the open: the file then loads as one level and the blocks stay
//! blocks.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use aad_doc::{
    Common, Drawing, Entity, EntityKind, EntityStore, Insert, LEVELS_NOTE_KEY, Level, LevelId, MAX_LEVELS, MAX_MAP_PATTERNS, MAX_SHEET_SETS,
    MAX_TRACES, SheetSet, SheetSetId, Trace, TraceColour, TraceOrder, layer_prefix_ok, level_name_ok, pattern_ok,
};
use aad_geom::Vec2;
use serde::{Deserialize, Serialize};

const KEY: &str = "levels2";
const STORES_LAYER: &str = "ODRAFT-STORES";
const BLOCK_PREFIX: &str = "ODRAFT$LVL$";
const ANN_PREFIX: &str = "ODRAFT$ANN$";

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    seed: u32,
    active: u32,
    levels: Vec<LevelMeta>,
    #[serde(default)]
    sheet_seed: u32,
    /// The sheet set of the sheet view being edited; 0 is design mode.
    #[serde(default)]
    active_sheet: u32,
    #[serde(default)]
    sets: Vec<SetMeta>,
}

#[derive(Serialize, Deserialize)]
struct LevelMeta {
    id: u32,
    name: String,
    elevation: f64,
    height: f64,
    #[serde(default)]
    traces: Vec<TraceMeta>,
    #[serde(default)]
    view: Option<[f64; 3]>,
}

#[derive(Serialize, Deserialize)]
struct TraceMeta {
    level: u32,
    fade: u8,
    true_colour: bool,
    above: bool,
    snap: bool,
    #[serde(default)]
    layers: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize)]
struct SetMeta {
    id: u32,
    name: String,
    prefix: String,
    #[serde(default)]
    space: Option<String>,
    #[serde(default)]
    dim_style: Option<String>,
    #[serde(default)]
    text_style: Option<String>,
    /// Layer and block name patterns the set maps (see `SheetSet::layers`).
    #[serde(default)]
    layers: Vec<String>,
    #[serde(default)]
    blocks: Vec<String>,
}

fn block_name(id: LevelId) -> String {
    format!("{BLOCK_PREFIX}{}", id.0)
}

fn ann_name(set: u32, level: u32) -> String {
    format!("{ANN_PREFIX}{set}${level}")
}

/// Put `store` in a block of the written drawing, kept alive by a hidden INSERT.
fn park(w: &mut Drawing, name: String, store: EntityStore) {
    let mut b = aad_doc::Block::new(&name);
    b.entities = store;
    w.blocks.insert(name.clone(), std::sync::Arc::new(b));
    let handle = w.new_handle();
    let common = Common { layer: STORES_LAYER.into(), ..Common::default() };
    w.model.push(Entity { handle, common, kind: EntityKind::Insert(Insert { block: name, ..Insert::default() }) });
}

/// The drawing as it is written: unchanged unless it has two or more levels, any sheet set, or
/// compose / master / publisher data.
pub(crate) fn to_file(d: &Drawing) -> Cow<'_, Drawing> {
    let needs_levels = d.levels.len() >= 2 || !d.sheet_sets.is_empty();
    let needs_compose = !d.compose_views.is_empty() || !d.master_layouts.is_empty() || !d.publisher_sets.is_empty()
        || d.layouts.iter().any(|l| l.master.is_some() || l.entities.iter().any(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.compose_view.is_some())));
    if !needs_levels && !needs_compose {
        return Cow::Borrowed(d);
    }
    if !needs_levels {
        let mut w = d.clone();
        w.sync_compose_to_project();
        return Cow::Owned(w);
    }
    let mut w = d.clone();
    let stores: Vec<(LevelId, EntityStore)> = d.levels.iter().map(|l| (l.id, d.level_store(l.id).cloned().unwrap_or_default())).collect();
    let meta = Meta {
        version: 1,
        seed: d.level_seed,
        active: d.active_level.map_or(0, |a| a.0),
        sheet_seed: d.sheet_seed,
        active_sheet: d.active_sheet.map_or(0, |a| a.0),
        sets: d
            .sheet_sets
            .iter()
            .map(|s| SetMeta {
                id: s.id.0,
                name: s.name.clone(),
                prefix: s.layer_prefix.clone(),
                space: s.space.clone(),
                dim_style: s.dim_style.clone(),
                text_style: s.text_style.clone(),
                layers: s.layers.clone(),
                blocks: s.blocks.clone(),
            })
            .collect(),
        levels: d
            .levels
            .iter()
            .map(|l| LevelMeta {
                id: l.id.0,
                name: l.name.clone(),
                elevation: l.elevation,
                height: l.height,
                traces: l
                    .traces
                    .iter()
                    .map(|t| TraceMeta {
                        level: t.level.0,
                        fade: t.fade,
                        true_colour: t.colour == TraceColour::TrueColour,
                        above: t.order == TraceOrder::Above,
                        snap: t.snap,
                        layers: t.layers.clone(),
                    })
                    .collect(),
                view: l.view.map(|(c, h)| [c.x, c.y, h]),
            })
            .collect(),
    };
    // Annotation first (needs the sheet sets), then clear the in-memory structure.
    let annotations: Vec<(String, EntityStore)> = d
        .sheet_sets
        .iter()
        .flat_map(|s| {
            d.levels
                .iter()
                .filter_map(move |l| d.annotation_store(s.id, l.id).filter(|st| !st.is_empty()).map(|st| (ann_name(s.id.0, l.id.0), st.clone())))
        })
        .collect();
    w.levels.clear();
    w.active_level = None;
    w.sheet_sets.clear();
    w.active_sheet = None;
    let mut stores = stores.into_iter();
    if let Some((_, first)) = stores.next() {
        w.model = first;
    } else if d.active_sheet.is_some() {
        // Cannot happen (a sheet view needs a level), but never write annotation as design.
        w.model = EntityStore::new();
    }
    if w.layer(STORES_LAYER).is_none() {
        w.layers.push(aad_doc::Layer { name: STORES_LAYER.into(), frozen: true, plot: false, ..aad_doc::Layer::default() });
    }
    for (id, store) in stores {
        park(&mut w, block_name(id), store);
    }
    for (name, store) in annotations {
        park(&mut w, name, store);
    }
    if let Ok(json) = serde_json::to_string(&meta) {
        w.project.insert(KEY.into(), json);
    }
    w.sync_compose_to_project();
    Cow::Owned(w)
}

fn valid(meta: &Meta) -> bool {
    if meta.version != 1 || meta.levels.len() > MAX_LEVELS || meta.sets.len() > MAX_SHEET_SETS {
        return false;
    }
    // Level data is written for two or more levels, or for any sheet set.
    if meta.levels.len() < 2 && meta.sets.is_empty() {
        return false;
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for l in &meta.levels {
        if l.id == 0 || l.id > meta.seed || !ids.insert(l.id) || !level_name_ok(&l.name) || !names.insert(l.name.trim().to_ascii_lowercase()) {
            return false;
        }
        if !l.elevation.is_finite() || !l.height.is_finite() || l.height < 0.0 || l.traces.len() > MAX_TRACES {
            return false;
        }
        if l.view.is_some_and(|v| v.iter().any(|n| !n.is_finite()) || v[2] <= 0.0) {
            return false;
        }
    }
    if meta.levels.is_empty() {
        if meta.active != 0 || meta.active_sheet != 0 {
            return false;
        }
    } else if !ids.contains(&meta.active) {
        return false;
    }
    let mut set_ids = HashSet::new();
    let mut set_names = HashSet::new();
    for s in &meta.sets {
        if s.id == 0
            || s.id > meta.sheet_seed
            || !set_ids.insert(s.id)
            || !level_name_ok(&s.name)
            || !set_names.insert(s.name.trim().to_ascii_lowercase())
        {
            return false;
        }
        if !layer_prefix_ok(&s.prefix) {
            return false;
        }
        if [&s.layers, &s.blocks].iter().any(|l| l.len() > MAX_MAP_PATTERNS || l.iter().any(|p| !pattern_ok(p))) {
            return false;
        }
    }
    if meta.active_sheet != 0 && !set_ids.contains(&meta.active_sheet) {
        return false;
    }
    meta.levels.iter().all(|l| l.traces.iter().all(|t| t.level != l.id && ids.contains(&t.level) && t.fade <= 90))
}

/// Rebuild the levels and sheet sets of a drawing read from a file. A file without valid
/// `levels2` data is returned unchanged.
pub(crate) fn from_file(mut d: Drawing) -> Drawing {
    d.load_compose_from_project();
    let Some(text) = d.project.get(KEY).cloned() else { return d };
    d.project.remove(KEY);
    let damaged = "This file has level data that could not be read, so it opened as a single level; the other levels stay in the file as blocks named ODRAFT$LVL$.";
    let Ok(meta) = serde_json::from_str::<Meta>(&text) else {
        d.project.insert(LEVELS_NOTE_KEY.into(), damaged.into());
        return d;
    };
    if !valid(&meta) {
        d.project.insert(LEVELS_NOTE_KEY.into(), damaged.into());
        return d;
    }
    let mut missing = Vec::new();
    // The keep-alive INSERTs and their layer are file plumbing, not drawing content.
    let mut names: HashSet<String> = meta.levels.iter().skip(1).map(|l| block_name(LevelId(l.id))).collect();
    for s in &meta.sets {
        for l in &meta.levels {
            names.insert(ann_name(s.id, l.id));
        }
    }
    let keep: Vec<_> = d
        .model
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if names.contains(&i.block)) && e.common.layer.eq_ignore_ascii_case(STORES_LAYER))
        .map(|e| e.handle)
        .collect();
    for h in keep {
        d.model.remove(h);
    }
    if !d.model.iter().any(|e| e.common.layer.eq_ignore_ascii_case(STORES_LAYER)) {
        d.layers.retain(|l| !l.name.eq_ignore_ascii_case(STORES_LAYER));
    }
    // Sheet sets and their annotation (a level without a block has none).
    let mut sets = Vec::with_capacity(meta.sets.len());
    for s in &meta.sets {
        let mut annotations = BTreeMap::new();
        for l in &meta.levels {
            if let Some(b) = d.blocks.remove(&ann_name(s.id, l.id)) {
                annotations.insert(LevelId(l.id), b.entities.clone());
            }
        }
        sets.push(SheetSet {
            id: SheetSetId(s.id),
            name: s.name.trim().to_string(),
            layer_prefix: s.prefix.clone(),
            space: s.space.clone(),
            dim_style: s.dim_style.clone(),
            text_style: s.text_style.clone(),
            layers: s.layers.clone(),
            blocks: s.blocks.clone(),
            annotations,
        });
    }
    d.sheet_sets = sets;
    d.sheet_seed = meta.sheet_seed;
    if meta.levels.is_empty() {
        // Sheet sets of a drawing that has only its implicit level.
        return d;
    }
    let first_store = std::mem::take(&mut d.model);
    let mut levels = Vec::with_capacity(meta.levels.len());
    for (i, m) in meta.levels.iter().enumerate() {
        let entities = if i == 0 {
            first_store.clone()
        } else {
            // A missing block gives an empty level.
            match d.blocks.remove(&block_name(LevelId(m.id))) {
                Some(b) => b.entities.clone(),
                None => {
                    missing.push(m.name.trim().to_string());
                    EntityStore::new()
                }
            }
        };
        levels.push(Level {
            id: LevelId(m.id),
            name: m.name.trim().to_string(),
            elevation: m.elevation,
            height: m.height,
            traces: m
                .traces
                .iter()
                .map(|t| Trace {
                    level: LevelId(t.level),
                    fade: t.fade,
                    colour: if t.true_colour { TraceColour::TrueColour } else { TraceColour::Grey },
                    order: if t.above { TraceOrder::Above } else { TraceOrder::Below },
                    snap: t.snap,
                    layers: t.layers.clone(),
                })
                .collect(),
            view: m.view.map(|v| (Vec2::new(v[0], v[1]), v[2])),
            entities,
        });
    }
    // Level 0 owns `model` (its slot stays empty), then the saved view takes over.
    if let Some(first) = levels.first_mut() {
        d.model = std::mem::take(&mut first.entities);
        d.active_level = Some(first.id);
    }
    d.levels = levels;
    d.level_seed = meta.seed;
    let _ = d.switch_level(LevelId(meta.active));
    if meta.active_sheet != 0 {
        let _ = d.enter_sheet_view(SheetSetId(meta.active_sheet));
    }
    if !missing.is_empty() {
        let note = format!("The file has no objects for level {}; the level opened empty.", missing.join(", "));
        d.project.insert(LEVELS_NOTE_KEY.into(), note);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use aad_doc::{Common, EntityKind, Line};
    use aad_geom::Vec3;

    fn line(d: &mut Drawing, y: f64) -> aad_doc::Handle {
        let h = d.new_handle();
        let kind = EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) });
        d.model.push(Entity { handle: h, common: Common::default(), kind });
        h
    }

    /// Ground (1 line) and First (2 lines) and Roof (0), with First active.
    fn three_levels() -> (Drawing, Vec<aad_doc::Handle>) {
        let mut d = Drawing::new_metric();
        let mut hs = vec![line(&mut d, 0.0)];
        let g = d.materialize_levels("Ground", 0.0, 3000.0).unwrap();
        let f = d.add_level("First", 3000.0, 3000.0, None).unwrap();
        d.add_level("Roof", 6000.0, 0.0, None).unwrap();
        d.switch_level(f).unwrap();
        hs.push(line(&mut d, 5.0));
        hs.push(line(&mut d, 6.0));
        d.levels.iter_mut().find(|l| l.id == g).unwrap().traces.push(Trace::new(f));
        (d, hs)
    }

    #[test]
    fn levels_survive_a_write_and_read_with_handles_names_and_the_active_tab() {
        let (d, hs) = three_levels();
        let bytes = crate::write_dxf(&d);
        assert!(bytes.contains("ODRAFT$LVL$"), "other CAD programs see the levels as blocks");
        let r = crate::read_dxf(bytes.as_bytes()).unwrap();
        let names: Vec<_> = r.levels.iter().map(|l| (l.name.as_str(), l.elevation)).collect();
        assert_eq!(names, [("Ground", 0.0), ("First", 3000.0), ("Roof", 6000.0)]);
        assert_eq!(r.active_level_ref().unwrap().name, "First");
        let count = |n: &str| r.level_store(r.level_by_name(n).unwrap().id).unwrap().len();
        assert_eq!((count("Ground"), count("First"), count("Roof")), (1, 2, 0));
        assert!(r.model.contains(hs[1]) && r.model.contains(hs[2]), "handles are kept");
        assert!(r.level_store(r.level_by_name("Ground").unwrap().id).unwrap().contains(hs[0]));
        assert!(r.block("ODRAFT$LVL$2").is_none() && r.layer(STORES_LAYER).is_none(), "file plumbing is hidden again");
        assert!(!r.project.contains_key(KEY));
        assert_eq!(r.level_by_name("Ground").unwrap().traces.len(), 1);
        assert_eq!(r.entity_count(), 3);
        // A second round trip keeps the structure (the keep-alive INSERTs get fresh handles).
        let again = crate::read_dxf(crate::write_dxf(&r).as_bytes()).unwrap();
        assert_eq!(again.levels.iter().map(|l| l.name.clone()).collect::<Vec<_>>(), r.levels.iter().map(|l| l.name.clone()).collect::<Vec<_>>());
        assert_eq!(again.entity_count(), 3);
    }

    #[test]
    fn a_single_level_or_no_levels_writes_exactly_as_before() {
        let mut d = Drawing::new_metric();
        line(&mut d, 0.0);
        let plain = crate::write_dxf(&d);
        d.materialize_levels("Only", 0.0, 0.0).unwrap();
        assert_eq!(crate::write_dxf(&d), plain);
        assert!(!plain.contains("levels2") && !plain.contains("ODRAFT"));
    }

    #[test]
    fn damaged_level_data_opens_as_one_level_and_never_fails() {
        let (d, _) = three_levels();
        let good = to_file(&d).into_owned();
        let bad_json = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut w = good.clone();
            let mut v: serde_json::Value = serde_json::from_str(&w.project[KEY]).unwrap();
            f(&mut v);
            w.project.insert(KEY.into(), v.to_string());
            w
        };
        let cases: Vec<Drawing> = vec![
            {
                let mut w = good.clone();
                w.project.insert(KEY.into(), "not json".into());
                w
            },
            bad_json(&|v| v["version"] = 9.into()),
            bad_json(&|v| v["levels"] = serde_json::json!([])),
            bad_json(&|v| v["levels"][1]["id"] = v["levels"][0]["id"].clone()),
            bad_json(&|v| v["levels"][1]["name"] = v["levels"][0]["name"].clone()),
            bad_json(&|v| v["levels"][0]["name"] = "".into()),
            bad_json(&|v| v["levels"][0]["id"] = 0.into()),
            bad_json(&|v| v["active"] = 999.into()),
            bad_json(&|v| v["seed"] = 0.into()),
            bad_json(&|v| {
                v["levels"][0]["traces"] = serde_json::json!([{"level": 999, "fade": 10, "true_colour": false, "above": false, "snap": false}])
            }),
            bad_json(&|v| {
                v["levels"][0]["traces"] = serde_json::json!([{"level": 1, "fade": 10, "true_colour": false, "above": false, "snap": false}])
            }),
            bad_json(&|v| {
                v["levels"][0]["traces"] = serde_json::json!([{"level": 2, "fade": 200, "true_colour": false, "above": false, "snap": false}])
            }),
            bad_json(&|v| v["levels"][0]["height"] = (-1).into()),
        ];
        for (i, w) in cases.into_iter().enumerate() {
            let r = from_file(w);
            assert!(r.levels.is_empty(), "case {i} loads as a single level");
            assert!(r.block("ODRAFT$LVL$2").is_some(), "case {i}: the blocks stay ordinary blocks");
            assert!(r.project.contains_key(LEVELS_NOTE_KEY), "case {i}: the user is told");
        }
    }

    #[test]
    fn a_missing_level_block_gives_an_empty_level() {
        let (d, _) = three_levels();
        let mut w = to_file(&d).into_owned();
        w.blocks.remove("ODRAFT$LVL$2");
        let r = from_file(w);
        assert_eq!(r.levels.len(), 3);
        assert_eq!(r.level_store(r.level_by_name("First").unwrap().id).unwrap().len(), 0);
        assert!(r.project[LEVELS_NOTE_KEY].contains("First"), "the note names the level");
    }

    #[test]
    fn the_level_count_is_capped_on_read() {
        let (d, _) = three_levels();
        let mut w = to_file(&d).into_owned();
        let mut v: serde_json::Value = serde_json::from_str(&w.project[KEY]).unwrap();
        let first = v["levels"][0].clone();
        let many: Vec<_> = (0..(MAX_LEVELS + 5))
            .map(|i| {
                let mut l = first.clone();
                l["id"] = (i as u64 + 1).into();
                l["name"] = format!("L{i}").into();
                l["traces"] = serde_json::json!([]);
                l
            })
            .collect();
        v["levels"] = many.into();
        v["seed"] = 500.into();
        v["active"] = 1.into();
        w.project.insert(KEY.into(), v.to_string());
        assert!(from_file(w).levels.is_empty());
    }

    /// Ground and First, a set `E` with a note on each level, First active in the sheet view.
    fn with_sheet_sets() -> Drawing {
        let (mut d, _) = three_levels();
        let e = d.add_sheet_set("Electrical", "E-").unwrap();
        d.add_sheet_set("Plumbing", "P-").unwrap();
        d.enter_sheet_view(e).unwrap();
        line(&mut d, 7.0);
        d.switch_level(d.level_by_name("Ground").unwrap().id).unwrap();
        line(&mut d, 8.0);
        d
    }

    #[test]
    fn sheet_sets_round_trip_through_blocks_and_the_project_record() {
        let d = with_sheet_sets();
        let r = crate::read_dxf(crate::write_dxf(&d).as_bytes()).unwrap();
        assert_eq!(r.sheet_sets.len(), 2);
        assert_eq!(r.active_sheet_ref().unwrap().name, "Electrical");
        assert_eq!(r.active_level_ref().unwrap().name, "Ground");
        assert_eq!(r.model.len(), 1, "the Ground annotation is the store being edited");
        let first = r.level_by_name("First").unwrap().id;
        assert_eq!(r.annotation_store(r.active_sheet.unwrap(), first).map(EntityStore::len), Some(1));
        assert_eq!(r.entity_count(), d.entity_count());
        assert!(r.blocks.keys().all(|k| !k.starts_with("ODRAFT$")));
        assert!(r.sheet_seed >= 2 && r.level_seed >= 3, "ids are never reused after a reopen");
    }

    #[test]
    fn damaged_sheet_set_records_open_as_one_level_without_the_sets() {
        let good = to_file(&with_sheet_sets()).into_owned();
        let damaged = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut w = good.clone();
            let mut v: serde_json::Value = serde_json::from_str(&w.project[KEY]).unwrap();
            f(&mut v);
            w.project.insert(KEY.into(), v.to_string());
            from_file(w)
        };
        let cases = [
            damaged(&|v| v["sets"][1]["id"] = v["sets"][0]["id"].clone()),
            damaged(&|v| v["sets"][1]["name"] = v["sets"][0]["name"].clone()),
            damaged(&|v| v["sets"][0]["prefix"] = "has space".into()),
            damaged(&|v| v["sets"][0]["prefix"] = "WAY-TOO-LONG".into()),
            damaged(&|v| v["sets"][0]["id"] = 0.into()),
            damaged(&|v| v["sheet_seed"] = 0.into()),
            damaged(&|v| v["active_sheet"] = 99.into()),
            damaged(&|v| {
                v["sets"] =
                    serde_json::json!((0..40).map(|i| serde_json::json!({"id": i + 1, "name": format!("S{i}"), "prefix": ""})).collect::<Vec<_>>())
            }),
        ];
        for (i, r) in cases.into_iter().enumerate() {
            assert!(r.levels.is_empty() && r.sheet_sets.is_empty() && r.active_sheet.is_none(), "case {i} opens as plain design");
            assert!(r.project.contains_key(LEVELS_NOTE_KEY), "case {i}: the user is told");
        }
    }

    #[test]
    fn a_viewport_keeps_its_level_sheet_set_and_traces_through_a_file() {
        let (mut d, _) = three_levels();
        let (ground, first) = (d.level_by_name("Ground").unwrap().id, d.level_by_name("First").unwrap().id);
        let set = d.add_sheet_set("Electrical", "E-").unwrap();
        let mut t = Trace::new(ground);
        t.fade = 40;
        t.layers = Some(vec!["A-WALL".into()]);
        let vp = aad_doc::Viewport {
            id: 2,
            width: 100.0,
            height: 80.0,
            view_height: 50.0,
            level: Some(first),
            sheet_set: Some(set),
            traces: vec![t.clone()],
            ..Default::default()
        };
        let plain = aad_doc::Viewport { id: 3, width: 10.0, height: 10.0, view_height: 5.0, ..Default::default() };
        let h = d.new_handle();
        let h2 = d.new_handle();
        let common = Common::default();
        d.layouts[0].entities.push(Entity { handle: h, common: common.clone(), kind: EntityKind::Viewport(vp) });
        d.layouts[0].entities.push(Entity { handle: h2, common, kind: EntityKind::Viewport(plain) });
        let r = crate::read_dxf(crate::write_dxf(&d).as_bytes()).unwrap();
        let get = |h| match &r.layouts[0].entities.get(h).unwrap().kind {
            EntityKind::Viewport(v) => v.clone(),
            k => panic!("{k:?}"),
        };
        let v = get(h);
        assert_eq!((v.level, v.sheet_set), (Some(first), Some(set)));
        assert_eq!(v.traces, vec![t]);
        let p = get(h2);
        assert_eq!((p.level, p.sheet_set, p.traces.len()), (None, None, 0), "an unbound viewport stays unbound");
    }
}

//! Version history: milestones and automatic restore points in the project file (`.osp`), or beside a drawing that is not one in `<drawing>.history.osp`.
//!
//! Commands: `history.save`, `history.list`, `history.restore`, `history.rename`, `history.delete`, `history.prune`,
//! `history.compare`, `history.preview`, `history.mirror`.
//! Automatic points run after a successful drawing save (never fail the save).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aad_doc::{
    Block, DimStyle, Drawing, EditTarget, Entity, EntityStore, Handle, Layer, LevelId, Linetype, MAX_BLOCK_DEPTH, SheetSetId, TextStyle,
    entity_bounds,
};
use aad_odp::history::{self as hvdb, HistoryDb, MilestoneIn, MilestoneOut, RowIn, StoreBundle, StoreMeta, StoreOut, sidecar_path};
use serde_json::{Map, Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

const HIST_KEY: &str = "history";
const MAX_OBJECTS_PER_STORE: usize = 500_000;
const MAX_COMPARE: usize = 100_000;

/// Canvas overlay for [`history.preview`] (not selectable; Esc / `off` clears).
#[derive(Clone, Debug, Default)]
pub struct HistoryPreviewState {
    pub milestone: u32,
    pub added: Vec<Entity>,
    pub removed: Vec<Entity>,
    pub changed: Vec<Entity>,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("history.save", "Save Milestone", run_save)
            .menu(&["File", "History", "Save Milestone…"])
            .alias(&["hmile", "savemilestone"])
            .params(
                "{name, scope?: current|level|sheet|project|object, level?, sheetSet?, kind?: milestone|auto|revision, author?} → {id, path, stores}",
            )
            .noundo(),
        CommandSpec::new("history.list", "History", run_list)
            .menu(&["File", "History", "List…"])
            .alias(&["hlist"])
            .params("{kind?: auto|milestone|revision, scope?} → {milestones: [...], path?, drawingId}")
            .noundo(),
        CommandSpec::new("history.restore", "Restore Milestone", run_restore)
            .menu(&["File", "History", "Restore…"])
            .alias(&["hrestore"])
            .params("{id, scope?: subset of milestone, mode?: replace|copy, definitions?: keep|snapshot, layouts?: bool} → {id, stores, warnings}"),
        CommandSpec::new("history.rename", "Name Version", run_rename)
            .menu(&["File", "History", "Name This Version…"])
            .alias(&["hrename"])
            .params("{id, name} → {id, name, kind}")
            .noundo(),
        CommandSpec::new("history.delete", "Delete Milestone", run_delete)
            .menu(&["File", "History", "Delete Milestone…"])
            .params("{id} → {deleted}")
            .noundo(),
        CommandSpec::new("history.prune", "Prune History", run_prune)
            .menu(&["File", "History", "Prune Automatic Points"])
            .params("{keep?: 20, daily?: 30} → {removed}")
            .noundo(),
        CommandSpec::new("history.compare", "Compare History", run_compare)
            .menu(&["File", "History", "Compare…"])
            .alias(&["hcompare"])
            .params("{from, to?: milestone id or \"now\" (default now), scope?} → {added, removed, changed, counts}")
            .noundo(),
        CommandSpec::new("history.preview", "Preview Milestone", run_preview)
            .menu(&["File", "History", "Preview…"])
            .alias(&["hpreview"])
            .params("{id, scope?} | {off: true} → {preview: {milestone, added, removed, changed} | null}")
            .noundo(),
        CommandSpec::new("history.mirror", "Mirror History to Office", run_mirror)
            .menu(&["File", "History", "Mirror to Office…"])
            .params("{} → error (office shared history is not available yet)")
            .noundo(),
    ]
}

/// Overlay entities for the host canvas (Handle 0).
pub fn preview_entities(state: &HistoryPreviewState) -> HistoryGhostEntities {
    HistoryGhostEntities {
        added: state.added.clone(),
        removed: state
            .removed
            .iter()
            .cloned()
            .map(|mut e| {
                e.common.linetype = "DASHED".into();
                e
            })
            .collect(),
        changed: state.changed.clone(),
    }
}

/// Three tint groups for the host overlay.
#[derive(Clone, Debug, Default)]
pub struct HistoryGhostEntities {
    pub added: Vec<Entity>,
    pub removed: Vec<Entity>,
    pub changed: Vec<Entity>,
}

impl HistoryGhostEntities {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

// ---------------- drawing id + path ----------------

fn drawing_id(d: &Drawing) -> Option<String> {
    project::get(d, HIST_KEY).get("drawingId").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Ensure project data carries a stable history `drawingId`. Used before a drawing-file save so the
/// written DXF/DWG includes the id and the post-save auto snapshot does not dirty a just-saved drawing.
pub(crate) fn ensure_drawing_id(s: &mut Session) -> Result<String> {
    if let Some(id) = drawing_id(s.doc()?) {
        return Ok(id);
    }
    let salt = match s.state()?.path.clone() {
        Some(p) => p,
        None => format!("unsaved-{}", s.doc()?.handseed),
    };
    let id = new_drawing_id(&salt);
    project::set(s.doc_mut()?, HIST_KEY, &json!({ "drawingId": id }));
    Ok(id)
}

fn new_drawing_id(salt: &str) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    #[cfg(target_arch = "wasm32")]
    let nanos = 0u128;
    let a = fnv(nanos as u64, salt.as_bytes());
    let b = fnv((nanos >> 64) as u64 ^ a, &[a as u8, (a >> 8) as u8]);
    format!("{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}", (a >> 32) as u32, (a >> 16) as u16, a & 0xfff, (b >> 52) & 0xfff, b & 0xffff_ffff_ffff)
}

fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn history_path(s: &Session) -> Result<PathBuf> {
    let path = s.state()?.path.as_ref().ok_or_else(|| bad("history", "save the drawing first so its history can be kept with it"))?;
    Ok(sidecar_path(Path::new(path)))
}

fn open_db(s: &mut Session) -> Result<(HistoryDb, PathBuf)> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        return Err(bad("history", "drawing history is not available in this build"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let id = ensure_drawing_id(s)?;
        let path = history_path(s)?;
        let db = HistoryDb::open(&path, &id).map_err(|e| bad("history", e.to_string()))?;
        Ok((db, path))
    }
}

fn iso_now() -> String {
    super::bridge::iso_time().unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

// ---------------- scope → stores ----------------

#[derive(Clone, Debug)]
enum StoreRef {
    Level(LevelId),
    Sheet(SheetSetId, LevelId),
}

impl StoreRef {
    fn target_key(&self) -> String {
        match self {
            StoreRef::Level(l) => format!("level:{}", l.0),
            StoreRef::Sheet(s, l) => format!("sheet:{}:{}", s.0, l.0),
        }
    }
}

fn parse_target_key(s: &str) -> Option<StoreRef> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.as_slice() {
        ["level", id] => id.parse().ok().map(|n| StoreRef::Level(LevelId(n))),
        ["sheet", set, level] => match (set.parse(), level.parse()) {
            (Ok(a), Ok(b)) => Some(StoreRef::Sheet(SheetSetId(a), LevelId(b))),
            _ => None,
        },
        _ => None,
    }
}

fn resolve_scope(d: &Drawing, p: &Value) -> Result<(Value, Vec<StoreRef>)> {
    let kind = str_param(p, "scope").unwrap_or("current");
    match kind {
        "project" => {
            let mut refs = Vec::new();
            for l in &d.levels {
                refs.push(StoreRef::Level(l.id));
            }
            // No named levels: the single model store is "level:0" synthetic — use active or invent.
            if refs.is_empty() {
                // Implicit single-model drawing: treat as one store tagged level:0 even without levels.
                refs.push(StoreRef::Level(LevelId(0)));
            }
            for s in &d.sheet_sets {
                for level in d.levels.iter().map(|l| l.id).chain(if d.levels.is_empty() { vec![LevelId(0)] } else { vec![] }) {
                    if d.annotation_store(s.id, level).is_some()
                        || d.target() == Some(EditTarget::Sheet(s.id, level))
                        || s.annotations.contains_key(&level)
                    {
                        refs.push(StoreRef::Sheet(s.id, level));
                    }
                }
                // Also include every annotation key present.
                for &level in s.annotations.keys() {
                    let r = StoreRef::Sheet(s.id, level);
                    if !refs.iter().any(|x| x.target_key() == r.target_key()) {
                        refs.push(r);
                    }
                }
            }
            if refs.len() > hvdb::MAX_STORES {
                return Err(bad("history.save", format!("at most {} stores in one milestone", hvdb::MAX_STORES)));
            }
            Ok((json!({"type": "project"}), refs))
        }
        "level" => {
            let id = level_param(d, p)?;
            Ok((json!({"type": "level", "level": id.0}), vec![StoreRef::Level(id)]))
        }
        "sheet" => {
            let set = sheet_param(d, p)?;
            let level = level_param(d, p).unwrap_or_else(|_| d.active_level.unwrap_or(LevelId(0)));
            Ok((json!({"type": "sheet", "sheetSet": set.0, "level": level.0}), vec![StoreRef::Sheet(set, level)]))
        }
        "current" | "" => match d.target() {
            Some(EditTarget::Sheet(s, l)) => Ok((json!({"type": "sheet", "sheetSet": s.0, "level": l.0}), vec![StoreRef::Sheet(s, l)])),
            Some(EditTarget::Level(l)) => Ok((json!({"type": "level", "level": l.0}), vec![StoreRef::Level(l)])),
            // Milestones do not cover sections and elevations yet: a view saves the level it sits on.
            Some(EditTarget::View(_)) => {
                let l = d.active_level.unwrap_or(LevelId(0));
                Ok((json!({"type": "level", "level": l.0}), vec![StoreRef::Level(l)]))
            }
            None => Ok((json!({"type": "level", "level": 0}), vec![StoreRef::Level(LevelId(0))])),
        },
        other => Err(bad("history.save", format!("scope must be current, level, sheet or project, not `{other}`"))),
    }
}

fn level_param(d: &Drawing, p: &Value) -> Result<LevelId> {
    if let Some(n) = p.get("level").and_then(Value::as_u64) {
        let id = LevelId(n as u32);
        if id.0 == 0 && d.levels.is_empty() {
            return Ok(id);
        }
        if d.level(id).is_some() {
            return Ok(id);
        }
        return Err(bad("history", format!("no level {}", id.0)));
    }
    if let Some(name) = str_param(p, "level") {
        if let Some(l) = d.levels.iter().find(|l| l.name.eq_ignore_ascii_case(name)) {
            return Ok(l.id);
        }
        return Err(bad("history", format!("no level named `{name}`")));
    }
    d.active_level.ok_or_else(|| bad("history", "`level` is required when the drawing has no active level"))
}

fn sheet_param(d: &Drawing, p: &Value) -> Result<SheetSetId> {
    if let Some(n) = p.get("sheetSet").and_then(Value::as_u64) {
        let id = SheetSetId(n as u32);
        if d.sheet_set(id).is_some() {
            return Ok(id);
        }
        return Err(bad("history", format!("no sheet set {}", id.0)));
    }
    if let Some(name) = str_param(p, "sheetSet") {
        if let Some(s) = d.sheet_sets.iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
            return Ok(s.id);
        }
        return Err(bad("history", format!("no sheet set named `{name}`")));
    }
    d.active_sheet.ok_or_else(|| bad("history", "`sheetSet` is required"))
}

fn store_entities<'a>(d: &'a Drawing, r: &StoreRef) -> Result<&'a EntityStore> {
    match *r {
        StoreRef::Level(id) if id.0 == 0 && d.levels.is_empty() => Ok(&d.model),
        StoreRef::Level(id) => d.level_store(id).ok_or_else(|| bad("history", format!("no level {}", id.0))),
        StoreRef::Sheet(set, level) => {
            if let Some(s) = d.annotation_store(set, level) {
                Ok(s)
            } else if d.target() == Some(EditTarget::Sheet(set, level)) {
                Ok(&d.model)
            } else {
                // Empty annotation is valid.
                Err(bad("history", format!("no annotation store for sheet set {} level {}", set.0, level.0)))
            }
        }
    }
}

fn store_name(d: &Drawing, r: &StoreRef) -> String {
    match *r {
        StoreRef::Level(id) => d
            .level(id)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| if id.0 == 0 && d.levels.is_empty() { "Model".into() } else { format!("Level {}", id.0) }),
        StoreRef::Sheet(set, level) => {
            let sn = d.sheet_set(set).map(|s| s.name.as_str()).unwrap_or("?");
            let ln = d.level(level).map(|l| l.name.as_str()).unwrap_or("?");
            format!("{sn} / {ln}")
        }
    }
}

// ---------------- snapshot builder ----------------

fn entity_row(d: &Drawing, e: &Entity) -> Result<RowIn> {
    let payload = serde_json::to_vec(e).map_err(|err| bad("history.save", err.to_string()))?;
    if payload.len() > aad_odp::history::MAX_PAYLOAD {
        return Err(bad("history.save", format!("object {} is too large to store", e.handle.hex())));
    }
    let b = entity_bounds(d, e, 0);
    let bbox = if b.is_empty() { [0.0, 0.0, 0.0, 0.0] } else { [b.min.x, b.min.y, b.max.x, b.max.y] };
    if !bbox.iter().all(|v| v.is_finite()) {
        return Err(bad("history.save", format!("object {} has a non-finite box", e.handle.hex())));
    }
    Ok(RowIn { bbox, payload })
}

fn collect_defs(d: &Drawing, store: &EntityStore) -> Result<Vec<RowIn>> {
    let mut layers: BTreeSet<String> = BTreeSet::new();
    let mut linetypes: BTreeSet<String> = BTreeSet::new();
    let mut text_styles: BTreeSet<String> = BTreeSet::new();
    let mut dim_styles: BTreeSet<String> = BTreeSet::new();
    let mut blocks: BTreeSet<String> = BTreeSet::new();

    for e in store.iter() {
        layers.insert(e.common.layer.clone());
        if !e.common.linetype.is_empty() {
            linetypes.insert(e.common.linetype.clone());
        }
        collect_entity_refs(d, e, &mut blocks, &mut text_styles, &mut dim_styles, 0);
    }

    let mut rows = Vec::new();
    for name in layers {
        if let Some(l) = d.layers.iter().find(|l| l.name == name) {
            rows.push(def_row("layer", l)?);
        }
    }
    for name in linetypes {
        if let Some(l) = d.linetypes.iter().find(|l| l.name == name) {
            rows.push(def_row("linetype", l)?);
        }
    }
    for name in text_styles {
        if let Some(t) = d.text_styles.iter().find(|t| t.name == name) {
            rows.push(def_row("textStyle", t)?);
        }
    }
    for name in dim_styles {
        if let Some(t) = d.dim_styles.iter().find(|t| t.name == name) {
            rows.push(def_row("dimStyle", t)?);
        }
    }
    // Nested blocks: expand referenced names up to depth.
    let mut queue: Vec<String> = blocks.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(b) = d.blocks.get(&name) else { continue };
        rows.push(def_row("block", b.as_ref())?);
        for e in b.entities.iter() {
            if let aad_doc::EntityKind::Insert(i) = &e.kind
                && seen.len() < 10_000
            {
                queue.push(i.block.clone());
            }
        }
    }
    Ok(rows)
}

fn collect_entity_refs(
    d: &Drawing,
    e: &Entity,
    blocks: &mut BTreeSet<String>,
    text_styles: &mut BTreeSet<String>,
    dim_styles: &mut BTreeSet<String>,
    depth: usize,
) {
    match &e.kind {
        aad_doc::EntityKind::Insert(i) => {
            blocks.insert(i.block.clone());
            if depth < MAX_BLOCK_DEPTH
                && let Some(b) = d.blocks.get(&i.block)
            {
                for child in b.entities.iter() {
                    collect_entity_refs(d, child, blocks, text_styles, dim_styles, depth + 1);
                }
            }
        }
        aad_doc::EntityKind::Text(t) => {
            text_styles.insert(t.style.clone());
        }
        aad_doc::EntityKind::MText(t) => {
            text_styles.insert(t.style.clone());
        }
        aad_doc::EntityKind::Dimension(dim) => {
            dim_styles.insert(dim.style.clone());
        }
        _ => {}
    }
}

fn def_row<T: serde::Serialize>(kind: &str, v: &T) -> Result<RowIn> {
    let mut map = Map::new();
    map.insert("kind".into(), json!(kind));
    map.insert("data".into(), serde_json::to_value(v).map_err(|e| bad("history.save", e.to_string()))?);
    let payload = serde_json::to_vec(&Value::Object(map)).map_err(|e| bad("history.save", e.to_string()))?;
    if payload.len() > aad_odp::history::MAX_PAYLOAD {
        return Err(bad("history.save", format!("{kind} definition too large")));
    }
    Ok(RowIn { bbox: [0.0, 0.0, 0.0, 0.0], payload })
}

fn content_hash(objects: &[RowIn]) -> String {
    let mut bytes = Vec::new();
    for o in objects {
        bytes.extend_from_slice(&o.payload);
    }
    format!("{:016x}", hvdb::content_n(&bytes))
}

fn build_bundles(d: &Drawing, refs: &[StoreRef]) -> Result<Vec<StoreBundle>> {
    let mut out = Vec::with_capacity(refs.len());
    for r in refs {
        let store = match store_entities(d, r) {
            Ok(s) => s,
            Err(_) if matches!(r, StoreRef::Sheet(..)) => {
                // Missing annotation → empty store snapshot.
                out.push(StoreBundle {
                    meta: StoreMeta {
                        target: r.target_key(),
                        name: store_name(d, r),
                        object_count: 0,
                        content_hash: content_hash(&[]),
                        member_chunks: 0,
                        definitions: 0,
                    },
                    objects: Vec::new(),
                    definitions: Vec::new(),
                });
                continue;
            }
            Err(e) => return Err(e),
        };
        if store.len() > MAX_OBJECTS_PER_STORE {
            return Err(bad("history.save", format!("store {} has too many objects", r.target_key())));
        }
        let mut objects = Vec::with_capacity(store.len());
        for e in store.iter() {
            objects.push(entity_row(d, e)?);
        }
        let definitions = collect_defs(d, store)?;
        let hash = content_hash(&objects);
        out.push(StoreBundle {
            meta: StoreMeta {
                target: r.target_key(),
                name: store_name(d, r),
                object_count: objects.len() as u32,
                content_hash: hash,
                member_chunks: 0,
                definitions: 0,
            },
            objects,
            definitions,
        });
    }
    Ok(out)
}

fn build_layouts(d: &Drawing) -> Result<Vec<RowIn>> {
    let mut rows = Vec::new();
    for lay in &d.layouts {
        let ents: Vec<&Entity> = lay.entities.iter().map(|e| e.as_ref()).collect();
        // Cap hostile layouts.
        if ents.len() > MAX_OBJECTS_PER_STORE {
            return Err(bad("history.save", format!("layout `{}` has too many objects", lay.name)));
        }
        let mut entity_vals = Vec::with_capacity(ents.len());
        for e in ents {
            entity_vals.push(serde_json::to_value(e).map_err(|err| bad("history.save", err.to_string()))?);
        }
        let payload = serde_json::to_vec(&json!({
            "name": lay.name,
            "tabOrder": lay.tab_order,
            "page": lay.page,
            "view": lay.view,
            "entities": entity_vals,
        }))
        .map_err(|e| bad("history.save", e.to_string()))?;
        if payload.len() > aad_odp::history::MAX_PAYLOAD {
            return Err(bad("history.save", format!("layout `{}` is too large", lay.name)));
        }
        let b = d.extents(&aad_doc::Space::Paper(lay.name.clone()));
        let bbox = if b.is_empty() { [0.0, 0.0, 0.0, 0.0] } else { [b.min.x, b.min.y, b.max.x, b.max.y] };
        rows.push(RowIn { bbox, payload });
    }
    Ok(rows)
}

/// Save a milestone (shared by `history.save`, auto-save, and `revision.issue`).
pub(crate) fn save_milestone(s: &mut Session, p: &Value, default_kind: &str) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("Milestone").trim();
    if name.is_empty() || name.len() > 200 {
        return Err(bad("history.save", "name must be 1..200 characters"));
    }
    let kind = str_param(p, "kind").unwrap_or(default_kind);
    if !matches!(kind, "auto" | "milestone" | "revision") {
        return Err(bad("history.save", "kind must be auto, milestone or revision"));
    }
    let author = str_param(p, "author").unwrap_or("").to_string();
    let (scope, refs) = resolve_scope(s.doc()?, p)?;
    let include_layouts = str_param(p, "scope") == Some("project") || bool_or(p, "layouts", false);
    let bundles = build_bundles(s.doc()?, &refs)?;
    let layouts = if include_layouts { build_layouts(s.doc()?)? } else { Vec::new() };
    let (mut db, path) = open_db(s)?;
    let id = db
        .save_milestone(MilestoneIn {
            name: name.to_string(),
            kind: kind.into(),
            time: iso_now(),
            author,
            scope: scope.clone(),
            parent: p.get("parent").and_then(Value::as_u64).map(|n| n as u32),
            stores: bundles,
            layouts,
        })
        .map_err(|e| bad("history.save", e.to_string()))?;
    Ok(json!({
        "id": id,
        "name": name,
        "kind": kind,
        "path": path.to_string_lossy(),
        "stores": refs.len(),
        "scope": scope,
    }))
}

/// After a successful drawing save: write an automatic restore point. Failures are echoed, never returned.
pub(crate) fn auto_snapshot_after_save(s: &mut Session, drawing_path: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, drawing_path);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !is_history_host_path(drawing_path) {
            return;
        }
        // Ensure path is set (commit_save may have just set it).
        let params = json!({
            "name": format!("Auto {}", iso_now()),
            "kind": "auto",
            "scope": "project",
            "author": "",
        });
        match save_milestone(s, &params, "auto") {
            Ok(v) => {
                let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
                // Best-effort prune.
                if let Ok((mut db, _)) = open_db(s) {
                    let _ = db.prune_auto(20, 30);
                }
                let _ = id;
            }
            Err(e) => s.echo(format!("History restore point not saved: {e}")),
        }
    }
}

// ---------------- commands ----------------

fn run_save(s: &mut Session, p: &Value) -> Result<Value> {
    save_milestone(s, p, "milestone")
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let (db, path) = open_db(s)?;
    let kind_filter = str_param(p, "kind");
    let milestones: Vec<Value> = db
        .list()
        .iter()
        .rev()
        .filter(|m| kind_filter.is_none_or(|k| m.kind == k))
        .map(|m| {
            json!({
                "id": m.id,
                "name": m.name,
                "kind": m.kind,
                "time": m.time,
                "author": m.author,
                "scope": m.scope,
                "parent": m.parent,
                "stores": m.stores,
            })
        })
        .collect();
    Ok(json!({
        "milestones": milestones,
        "path": path.to_string_lossy(),
        "drawingId": db.drawing_id(),
    }))
}

fn run_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("history.rename", "`id` is required"))? as u32;
    let name = str_param(p, "name").unwrap_or("").trim();
    if name.is_empty() || name.len() > 200 {
        return Err(bad("history.rename", "name must be 1..200 characters"));
    }
    let (mut db, _) = open_db(s)?;
    let m = db.rename_milestone(id, name).map_err(|e| bad("history.rename", e.to_string()))?;
    Ok(json!({ "id": m.id, "name": m.name, "kind": m.kind }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("history.delete", "`id` is required"))? as u32;
    let (mut db, _) = open_db(s)?;
    db.delete_milestone(id).map_err(|e| bad("history.delete", e.to_string()))?;
    Ok(json!({ "deleted": id }))
}

fn run_prune(s: &mut Session, p: &Value) -> Result<Value> {
    let keep = p.get("keep").and_then(Value::as_u64).unwrap_or(20) as usize;
    let daily = p.get("daily").and_then(Value::as_u64).unwrap_or(30) as u32;
    let (mut db, _) = open_db(s)?;
    let removed = db.prune_auto(keep, daily).map_err(|e| bad("history.prune", e.to_string()))?;
    Ok(json!({ "removed": removed }))
}

fn run_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("history.restore", "`id` is required"))? as u32;
    let mode = str_param(p, "mode").unwrap_or("replace");
    if mode != "replace" && mode != "copy" {
        return Err(bad("history.restore", "mode must be replace or copy"));
    }
    let defs_mode = str_param(p, "definitions").unwrap_or("keep");
    let restore_layouts = bool_or(p, "layouts", false);

    // Auto "Before restoring …" point (best effort).
    let before_name = {
        let (db, _) = open_db(s)?;
        let name = db.list().iter().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_else(|| format!("#{id}"));
        format!("Before restoring {name}")
    };
    let _ = save_milestone(s, &json!({ "name": before_name, "kind": "auto", "scope": "project" }), "auto");

    let (db, _) = open_db(s)?;
    let loaded = db.load_milestone(id).map_err(|e| bad("history.restore", e.to_string()))?;

    // Optional subset of stores via scope filter on targets.
    let want: Option<BTreeSet<String>> = if p.get("scope").is_some() && str_param(p, "scope") != Some("project") {
        let (_, refs) = resolve_scope(s.doc()?, p)?;
        Some(refs.into_iter().map(|r| r.target_key()).collect())
    } else {
        None
    };

    let mut d = s.doc()?.clone();
    let mut warnings = Vec::new();
    let mut restored = Vec::new();

    for store in &loaded.stores {
        if want.as_ref().is_some_and(|w| !w.contains(&store.meta.target)) {
            continue;
        }
        let Some(r) = parse_target_key(&store.meta.target) else {
            warnings.push(format!("unknown store target {}", store.meta.target));
            continue;
        };

        // Apply definitions first.
        for def_bytes in &store.definitions {
            apply_def(&mut d, def_bytes, defs_mode, &mut warnings)?;
        }

        let mut entities = Vec::with_capacity(store.objects.len());
        for bytes in &store.objects {
            let e: Entity = serde_json::from_slice(bytes).map_err(|e| bad("history.restore", format!("bad object row: {e}")))?;
            entities.push(e);
        }

        if mode == "copy" {
            match r {
                StoreRef::Level(src) => {
                    let base = store.meta.name.clone();
                    let name = unique_level_name(&d, &format!("{base} (restored)"));
                    let elev = d.level(src).map(|l| l.elevation).unwrap_or(0.0);
                    let height = d.level(src).map(|l| l.height).unwrap_or(0.0);
                    let new_id = d.add_level(&name, elev, height, None).map_err(|e| bad("history.restore", e.to_string()))?;
                    write_store(&mut d, StoreRef::Level(new_id), entities, &mut warnings)?;
                    restored.push(json!({"target": format!("level:{}", new_id.0), "name": name, "objects": store.objects.len()}));
                }
                StoreRef::Sheet(set, level) => {
                    // Copy into a new sheet-set name suffix if needed — for v1, replace annotation on a new level copy is heavier;
                    // copy mode for sheets writes into the same set under a duplicate level when possible.
                    let base = d.level(level).map(|l| l.name.clone()).unwrap_or_else(|| "Level".into());
                    let name = unique_level_name(&d, &format!("{base} (restored)"));
                    let elev = d.level(level).map(|l| l.elevation).unwrap_or(0.0);
                    let height = d.level(level).map(|l| l.height).unwrap_or(0.0);
                    let new_level = d.add_level(&name, elev, height, None).map_err(|e| bad("history.restore", e.to_string()))?;
                    write_store(&mut d, StoreRef::Sheet(set, new_level), entities, &mut warnings)?;
                    restored.push(json!({"target": format!("sheet:{}:{}", set.0, new_level.0), "name": name, "objects": store.objects.len()}));
                }
            }
        } else {
            ensure_store_exists(&mut d, &r, &store.meta.name, &mut warnings)?;
            write_store(&mut d, r, entities, &mut warnings)?;
            restored.push(json!({"target": store.meta.target, "name": store.meta.name, "objects": store.objects.len()}));
        }
    }

    if restore_layouts {
        for bytes in &loaded.layouts {
            apply_layout(&mut d, bytes, &mut warnings)?;
        }
    }

    *s.doc_mut()? = d;
    Ok(json!({ "id": id, "stores": restored, "warnings": warnings, "mode": mode }))
}

fn unique_level_name(d: &Drawing, base: &str) -> String {
    if d.levels.iter().all(|l| !l.name.eq_ignore_ascii_case(base)) {
        return base.to_string();
    }
    for i in 2..10_000 {
        let n = format!("{base} {i}");
        if d.levels.iter().all(|l| !l.name.eq_ignore_ascii_case(&n)) {
            return n;
        }
    }
    format!("{base} {}", d.level_seed.saturating_add(1))
}

fn ensure_store_exists(d: &mut Drawing, r: &StoreRef, name_at_time: &str, warnings: &mut Vec<String>) -> Result<()> {
    match *r {
        StoreRef::Level(id) if id.0 == 0 && d.levels.is_empty() => Ok(()),
        StoreRef::Level(id) => {
            if d.level(id).is_some() {
                return Ok(());
            }
            // Recreate with old id: levels API allocates new ids. Recreate by name and warn.
            let name = unique_level_name(d, name_at_time);
            warnings.push(format!("level {} was missing; recreated as `{name}` with a new id", id.0));
            let _ = d.add_level(&name, 0.0, 0.0, None).map_err(|e| bad("history.restore", e.to_string()))?;
            Ok(())
        }
        StoreRef::Sheet(set, level) => {
            if d.sheet_set(set).is_none() {
                let name = format!("{name_at_time}-set");
                warnings.push(format!("sheet set {} was missing; recreated as `{name}`", set.0));
                let _ = d.add_sheet_set(&name, "A-").map_err(|e| bad("history.restore", e.to_string()))?;
            }
            if level.0 != 0 && d.level(level).is_none() && !d.levels.is_empty() {
                warnings.push(format!("level {} for sheet annotation was missing", level.0));
            }
            let _ = (set, level);
            Ok(())
        }
    }
}

fn write_store(d: &mut Drawing, r: StoreRef, entities: Vec<Entity>, warnings: &mut Vec<String>) -> Result<()> {
    // Handles that already live in another store get remapped.
    let mut occupied: BTreeSet<Handle> = BTreeSet::new();
    for e in d.model_stores().flat_map(|s| s.iter()) {
        occupied.insert(e.handle);
    }
    // Exclude the store we are about to replace.
    let replacing = match r {
        StoreRef::Level(id) if id.0 == 0 && d.levels.is_empty() => {
            for e in d.model.iter() {
                occupied.remove(&e.handle);
            }
            true
        }
        StoreRef::Level(id) => {
            if let Some(s) = d.level_store(id) {
                for e in s.iter() {
                    occupied.remove(&e.handle);
                }
            }
            true
        }
        StoreRef::Sheet(set, level) => {
            if let Some(s) = d.annotation_store(set, level) {
                for e in s.iter() {
                    occupied.remove(&e.handle);
                }
            }
            true
        }
    };
    let _ = replacing;

    let mut map: BTreeMap<Handle, Handle> = BTreeMap::new();
    let mut out = EntityStore::new();
    for mut e in entities {
        if occupied.contains(&e.handle) {
            let fresh = d.new_handle();
            warnings.push(format!("handle {} was in use elsewhere; remapped to {}", e.handle.hex(), fresh.hex()));
            map.insert(e.handle, fresh);
            e.handle = fresh;
        } else {
            occupied.insert(e.handle);
            if e.handle.0 >= d.handseed {
                d.handseed = e.handle.0.saturating_add(1);
            }
        }
        // Remap dimension associations / insert refs later if needed — handles in kind stay as stored.
        out.push(e);
    }

    match r {
        StoreRef::Level(id) if id.0 == 0 && d.levels.is_empty() => {
            d.model = out;
        }
        StoreRef::Level(id) => {
            if d.target() == Some(EditTarget::Level(id)) {
                d.model = out;
            } else if let Some(slot) = d.levels.iter_mut().find(|l| l.id == id) {
                slot.entities = out;
            } else {
                return Err(bad("history.restore", format!("no level {}", id.0)));
            }
        }
        StoreRef::Sheet(set, level) => {
            if d.target() == Some(EditTarget::Sheet(set, level)) {
                d.model = out;
            } else if let Some(ss) = d.sheet_sets.iter_mut().find(|s| s.id == set) {
                ss.annotations.insert(level, out);
            } else {
                return Err(bad("history.restore", format!("no sheet set {}", set.0)));
            }
        }
    }
    let _ = map;
    Ok(())
}

fn apply_def(d: &mut Drawing, bytes: &[u8], mode: &str, warnings: &mut Vec<String>) -> Result<()> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| bad("history.restore", format!("bad definition: {e}")))?;
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or("");
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    match kind {
        "layer" => {
            let layer: Layer = serde_json::from_value(data).map_err(|e| bad("history.restore", e.to_string()))?;
            if let Some(existing) = d.layers.iter_mut().find(|l| l.name == layer.name) {
                if mode == "snapshot" {
                    *existing = layer;
                } else if *existing != layer {
                    warnings.push(format!("kept current layer `{}`", existing.name));
                }
            } else {
                d.layers.push(layer);
            }
        }
        "linetype" => {
            let lt: Linetype = serde_json::from_value(data).map_err(|e| bad("history.restore", e.to_string()))?;
            if d.linetypes.iter().all(|l| l.name != lt.name) {
                d.linetypes.push(lt);
            } else if mode == "snapshot"
                && let Some(e) = d.linetypes.iter_mut().find(|l| l.name == lt.name)
            {
                *e = lt;
            }
        }
        "textStyle" => {
            let t: TextStyle = serde_json::from_value(data).map_err(|e| bad("history.restore", e.to_string()))?;
            if d.text_styles.iter().all(|x| x.name != t.name) {
                d.text_styles.push(t);
            } else if mode == "snapshot"
                && let Some(e) = d.text_styles.iter_mut().find(|x| x.name == t.name)
            {
                *e = t;
            }
        }
        "dimStyle" => {
            let t: DimStyle = serde_json::from_value(data).map_err(|e| bad("history.restore", e.to_string()))?;
            if d.dim_styles.iter().all(|x| x.name != t.name) {
                d.dim_styles.push(t);
            } else if mode == "snapshot"
                && let Some(e) = d.dim_styles.iter_mut().find(|x| x.name == t.name)
            {
                *e = t;
            }
        }
        "block" => {
            let b: Block = serde_json::from_value(data).map_err(|e| bad("history.restore", e.to_string()))?;
            if !d.blocks.contains_key(&b.name) || mode == "snapshot" {
                d.blocks.insert(b.name.clone(), Arc::new(b));
            } else {
                warnings.push(format!("kept current block `{}`", b.name));
            }
        }
        other => warnings.push(format!("skipped unknown definition kind `{other}`")),
    }
    Ok(())
}

fn apply_layout(d: &mut Drawing, bytes: &[u8], warnings: &mut Vec<String>) -> Result<()> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| bad("history.restore", format!("bad layout: {e}")))?;
    let name = v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    if name.is_empty() {
        warnings.push("skipped layout with empty name".into());
        return Ok(());
    }
    let ents = v.get("entities").and_then(Value::as_array).cloned().unwrap_or_default();
    if ents.len() > MAX_OBJECTS_PER_STORE {
        return Err(bad("history.restore", format!("layout `{name}` has too many objects")));
    }
    let mut store = EntityStore::new();
    for e in ents {
        let ent: Entity = serde_json::from_value(e).map_err(|err| bad("history.restore", err.to_string()))?;
        if ent.handle.0 >= d.handseed {
            d.handseed = ent.handle.0.saturating_add(1);
        }
        store.push(ent);
    }
    if let Some(lay) = d.layouts.iter_mut().find(|l| l.name == name) {
        lay.entities = store;
        if let Some(to) = v.get("tabOrder").and_then(Value::as_u64) {
            lay.tab_order = to as u32;
        }
    } else {
        let mut lay = aad_doc::Layout::new(&name, d.layouts.len() as u32 + 1);
        lay.entities = store;
        d.layouts.push(lay);
    }
    Ok(())
}

/// Whether `path` looks like a DXF/DWG (used by the save hook). Re-export helper for file.rs.
pub(crate) fn is_history_host_path(path: &str) -> bool {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "dxf" | "dwg")
}

// ---------------- compare / preview / mirror ----------------

#[derive(Clone, Debug)]
struct ObjRow {
    handle: Handle,
    hash: u64,
    bbox: [f64; 4],
    entity: Entity,
}

fn parse_object_row(bytes: &[u8]) -> Result<ObjRow> {
    let entity: Entity = serde_json::from_slice(bytes).map_err(|e| bad("history.compare", format!("bad object row: {e}")))?;
    let hash = hvdb::content_n(bytes);
    let b = entity_bounds_from_entity(&entity);
    Ok(ObjRow { handle: entity.handle, hash, bbox: b, entity })
}

fn entity_bounds_from_entity(e: &Entity) -> [f64; 4] {
    // Approximate without full block expansion (preview/compare only).
    let blank = Drawing::new_metric();
    let b = entity_bounds(&blank, e, 0);
    if b.is_empty() { [0.0, 0.0, 0.0, 0.0] } else { [b.min.x, b.min.y, b.max.x, b.max.y] }
}

fn rows_from_store_out(store: &StoreOut) -> Result<BTreeMap<Handle, ObjRow>> {
    if store.objects.len() > MAX_COMPARE {
        return Err(bad("history.compare", "too many objects to compare"));
    }
    let mut map = BTreeMap::new();
    for bytes in &store.objects {
        let row = parse_object_row(bytes)?;
        map.insert(row.handle, row);
    }
    Ok(map)
}

fn rows_from_drawing_store(d: &Drawing, store: &EntityStore) -> Result<BTreeMap<Handle, ObjRow>> {
    if store.len() > MAX_COMPARE {
        return Err(bad("history.compare", "too many objects to compare"));
    }
    let mut map = BTreeMap::new();
    for e in store.iter() {
        let payload = serde_json::to_vec(e.as_ref()).map_err(|err| bad("history.compare", err.to_string()))?;
        let b = entity_bounds(d, e, 0);
        let bbox = if b.is_empty() { [0.0, 0.0, 0.0, 0.0] } else { [b.min.x, b.min.y, b.max.x, b.max.y] };
        map.insert(e.handle, ObjRow { handle: e.handle, hash: hvdb::content_n(&payload), bbox, entity: (**e).clone() });
    }
    Ok(map)
}

fn compare_maps(from: &BTreeMap<Handle, ObjRow>, to: &BTreeMap<Handle, ObjRow>) -> (Vec<Value>, Vec<Value>, Vec<Value>, HistoryPreviewState) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut preview = HistoryPreviewState::default();
    for (h, row) in to {
        match from.get(h) {
            None => {
                added.push(diff_json("added", row));
                let mut e = row.entity.clone();
                e.handle = Handle(0);
                preview.added.push(e);
            }
            Some(old) if old.hash != row.hash => {
                changed.push(json!({
                    "status": "changed",
                    "handle": h.hex(),
                    "bbox": row.bbox,
                    "fromHash": format!("{:016x}", old.hash),
                    "toHash": format!("{:016x}", row.hash),
                }));
                let mut e = row.entity.clone();
                e.handle = Handle(0);
                preview.changed.push(e);
            }
            Some(_) => {}
        }
    }
    for (h, row) in from {
        if !to.contains_key(h) {
            removed.push(diff_json("removed", row));
            let mut e = row.entity.clone();
            e.handle = Handle(0);
            preview.removed.push(e);
        }
    }
    (added, removed, changed, preview)
}

fn diff_json(status: &str, row: &ObjRow) -> Value {
    json!({
        "status": status,
        "handle": row.handle.hex(),
        "bbox": row.bbox,
        "hash": format!("{:016x}", row.hash),
    })
}

/// Compare two sides across matching store targets. `to_id` None means the live drawing.
pub(crate) fn compare_milestones(s: &mut Session, from_id: u32, to_id: Option<u32>, scope_p: &Value) -> Result<(Value, HistoryPreviewState)> {
    let (db, _) = open_db(s)?;
    let from_ms = db.load_milestone(from_id).map_err(|e| bad("history.compare", e.to_string()))?;
    let want = optional_target_filter(s.doc()?, scope_p)?;

    let mut added_all = Vec::new();
    let mut removed_all = Vec::new();
    let mut changed_all = Vec::new();
    let mut preview = HistoryPreviewState { milestone: from_id, ..HistoryPreviewState::default() };

    match to_id {
        Some(tid) => {
            let to_ms = db.load_milestone(tid).map_err(|e| bad("history.compare", e.to_string()))?;
            let from_by = index_stores(&from_ms, &want)?;
            let to_by = index_stores(&to_ms, &want)?;
            let targets: BTreeSet<_> = from_by.keys().chain(to_by.keys()).cloned().collect();
            for target in targets {
                let empty = BTreeMap::new();
                let a = from_by.get(&target).unwrap_or(&empty);
                let b = to_by.get(&target).unwrap_or(&empty);
                let (ad, rm, ch, mut p) = compare_maps(a, b);
                added_all.extend(ad);
                removed_all.extend(rm);
                changed_all.extend(ch);
                preview.added.append(&mut p.added);
                preview.removed.append(&mut p.removed);
                preview.changed.append(&mut p.changed);
            }
            preview.milestone = tid;
        }
        None => {
            // Milestone → now: "added" = in drawing not milestone, "removed" = in milestone not drawing.
            let from_by = index_stores(&from_ms, &want)?;
            for (target, from_rows) in &from_by {
                let Some(r) = parse_target_key(target) else { continue };
                let store = match store_entities(s.doc()?, &r) {
                    Ok(st) => st,
                    Err(_) => {
                        // Missing store: everything in from is removed.
                        for row in from_rows.values() {
                            removed_all.push(diff_json("removed", row));
                            let mut e = row.entity.clone();
                            e.handle = Handle(0);
                            preview.removed.push(e);
                        }
                        continue;
                    }
                };
                let to_rows = rows_from_drawing_store(s.doc()?, store)?;
                let (ad, rm, ch, mut p) = compare_maps(from_rows, &to_rows);
                added_all.extend(ad);
                removed_all.extend(rm);
                changed_all.extend(ch);
                preview.added.append(&mut p.added);
                preview.removed.append(&mut p.removed);
                preview.changed.append(&mut p.changed);
            }
        }
    }

    // Cap overlay size.
    const MAX_GHOST: usize = 5_000;
    if preview.added.len() > MAX_GHOST {
        preview.added.truncate(MAX_GHOST);
    }
    if preview.removed.len() > MAX_GHOST {
        preview.removed.truncate(MAX_GHOST);
    }
    if preview.changed.len() > MAX_GHOST {
        preview.changed.truncate(MAX_GHOST);
    }

    let counts = json!({
        "added": added_all.len(),
        "removed": removed_all.len(),
        "changed": changed_all.len(),
    });
    Ok((
        json!({
            "from": from_id,
            "to": to_id.map(Value::from).unwrap_or(json!("now")),
            "added": added_all,
            "removed": removed_all,
            "changed": changed_all,
            "counts": counts,
        }),
        preview,
    ))
}

fn index_stores(ms: &MilestoneOut, want: &Option<BTreeSet<String>>) -> Result<BTreeMap<String, BTreeMap<Handle, ObjRow>>> {
    let mut out = BTreeMap::new();
    for store in &ms.stores {
        if want.as_ref().is_some_and(|w| !w.contains(&store.meta.target)) {
            continue;
        }
        out.insert(store.meta.target.clone(), rows_from_store_out(store)?);
    }
    Ok(out)
}

fn optional_target_filter(d: &Drawing, p: &Value) -> Result<Option<BTreeSet<String>>> {
    if p.get("scope").is_none() || str_param(p, "scope") == Some("project") {
        return Ok(None);
    }
    let (_, refs) = resolve_scope(d, p)?;
    Ok(Some(refs.into_iter().map(|r| r.target_key()).collect()))
}

fn run_compare(s: &mut Session, p: &Value) -> Result<Value> {
    let from = p.get("from").and_then(Value::as_u64).ok_or_else(|| bad("history.compare", "`from` (milestone id) is required"))? as u32;
    let to = match p.get("to") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.eq_ignore_ascii_case("now") => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| bad("history.compare", "`to` must be a milestone id or \"now\""))? as u32),
    };
    let (result, _) = compare_milestones(s, from, to, p)?;
    Ok(result)
}

fn run_preview(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "off", false) {
        s.history_preview = None;
        return Ok(json!({ "preview": Value::Null }));
    }
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("history.preview", "`id` is required (or pass `off: true`)"))? as u32;
    // Preview = compare milestone → now, overlay the diffs.
    let (_, mut preview) = compare_milestones(s, id, None, p)?;
    preview.milestone = id;
    let summary = json!({
        "milestone": id,
        "added": preview.added.len(),
        "removed": preview.removed.len(),
        "changed": preview.changed.len(),
    });
    s.history_preview = Some(preview);
    Ok(json!({ "preview": summary }))
}

fn run_mirror(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("history.mirror", "office shared history is not available yet; history stays in the local history file"))
}

/// Diff boxes for revision cloud clustering (`revision.clouds`).
pub(crate) fn compare_boxes(s: &mut Session, from: u32, to: Option<u32>, scope_p: &Value) -> Result<Vec<[f64; 4]>> {
    let (result, _) = compare_milestones(s, from, to, scope_p)?;
    let mut boxes = Vec::new();
    for key in ["added", "removed", "changed"] {
        if let Some(arr) = result.get(key).and_then(Value::as_array) {
            for item in arr {
                if let Some(b) = item.get("bbox").and_then(Value::as_array)
                    && b.len() == 4
                {
                    let nums: Option<[f64; 4]> = (|| Some([b[0].as_f64()?, b[1].as_f64()?, b[2].as_f64()?, b[3].as_f64()?]))();
                    if let Some(bb) = nums.filter(|n| n.iter().all(|v| v.is_finite())) {
                        boxes.push(bb);
                    }
                }
            }
        }
    }
    if boxes.len() > MAX_COMPARE {
        return Err(bad("revision.clouds", "too many changes to cluster"));
    }
    Ok(boxes)
}

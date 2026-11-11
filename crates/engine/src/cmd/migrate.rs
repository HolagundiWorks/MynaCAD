//! Migration into levels (design: `docs/levels-and-sheet-sets.md`, "Migration").
//!
//! - `level.import {path, ...}` reads a drawing file (flattened to 2D) and adds its model space to
//!   this drawing as a new level, or, with `sheetSet`, as that sheet set's annotation of a level.
//! - `level.export {level, path}` writes one level as a plain single-level DXF.
//! - `project.merge` does `level.import` for every floor of a floor project (a folder of drawings
//!   joined by Xrefs, see `projfile.rs`): each floor file becomes a level with the floor's
//!   elevation and height, and each discipline layout file becomes annotation of a sheet set when it
//!   holds only annotation, or a level of its own when it holds geometry.
//!
//! Everything is worked out on a scratch copy of the drawing, so a command that fails changes
//! nothing and one that succeeds is one undo step. Units are never converted: a file in other
//! units than this drawing is refused (`ignoreUnits` imports it unscaled).

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use aad_doc::{Drawing, EntityKind, EntityStore, Handle, LevelId, SheetSetId};
use serde_json::{Value, json};

use super::blocks::{adopt, block_names, merge_tables, read_drawing};
#[cfg(not(target_arch = "wasm32"))]
use super::project;
use super::*;
use crate::annot;
use crate::{Result, Session};

/// Most objects one import adds (hostile files).
const MAX_IMPORT_OBJECTS: usize = 5_000_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("level.import", "Import Level", run_import)
            .menu(&["Architecture", "Levels", "Import Level"])
            .params("{path, name? (default the file name), elevation?, height?, after?, sheetSet? (import as that sheet set's annotation of `level` instead), level? (default the active one), ignoreUnits?, layerMap? ({file layer: layer to put its objects on}; layers not listed keep their names)} → {level, id, objects, ...}. Reads a DXF/DWG (flattened to 2D) as a new level."),
        CommandSpec::new("layer.mapplan", "Plan Layer Mapping", run_mapplan)
            .menu(&["Architecture", "Levels", "Plan Layer Mapping"])
            .params("{path} → {layers: [{name, objects, suggested?}], targets: [{name, tab, description}]}. Lists the layers of a DXF/DWG with their object counts and the standard layer each most likely stands for, to be passed as `layerMap` to `level.import`. Changes nothing.")
            .noundo(),
        CommandSpec::new("level.export", "Export Level", run_export)
            .menu(&["Architecture", "Levels", "Export Level"])
            .params("{level (default the active one), path, overwrite?} → {path, objects}. One level as a plain single-level DXF.")
            .noundo(),
        CommandSpec::new("project.merge", "Merge Project into Levels", run_merge)
            .menu(&["Architecture", "Levels", "Merge Project"])
            .params("{path? (project folder or project.aad.json; default the project the drawing belongs to), annotation? (default true: annotation-only layouts become sheet-set annotation), ignoreUnits?} → {levels, sheetSets, skipped}. Every floor of a floor project becomes a level of this drawing in one undo step."),
    ]
}

// ---------------- reading ----------------

/// A drawing file, read through the io hooks and flattened to 2D.
pub(super) fn load(path: &str) -> Result<Drawing> {
    let mut src = read_drawing(path)?;
    crate::flatten::flatten_drawing(&mut src);
    if src.model.len() > MAX_IMPORT_OBJECTS {
        return Err(bad("level.import", format!("{path}: too many objects to import")));
    }
    Ok(src)
}

fn units_ok(d: &Drawing, src: &Drawing, path: &str, ignore: bool) -> Result<()> {
    if ignore {
        return Ok(());
    }
    match (annot::drawing_unit(d), annot::drawing_unit(src)) {
        (Some(a), Some(b)) if a.insunits() != b.insunits() => Err(bad(
            "level.import",
            format!("{path} is drawn in different units than this drawing; units are not converted. Pass `ignoreUnits` to import it unscaled"),
        )),
        _ => Ok(()),
    }
}

/// Bring `src`'s model space into `d`: tables merged, needed blocks copied, fresh handles, and
/// dimension associations kept for objects that came along. Returns the objects.
fn adopt_model(d: &mut Drawing, src: &Drawing) -> EntityStore {
    merge_tables(d, src);
    let renames = block_names(d, src, "");
    for (from, to) in &renames {
        let Some(b) = src.blocks.get(from) else { continue };
        let mut nb = (**b).clone();
        nb.name = to.clone();
        nb.entities = EntityStore::new();
        for e in b.entities.iter() {
            let e = adopt(d, e, &renames);
            nb.entities.push(e);
        }
        d.blocks.insert(to.clone(), Arc::new(nb));
    }
    let map: BTreeMap<Handle, Handle> = src.model.iter().map(|e| (e.handle, d.new_handle())).collect();
    let mut out = EntityStore::new();
    for e in src.model.iter() {
        let mut c = (**e).clone();
        c.handle = map.get(&e.handle).copied().unwrap_or(e.handle);
        match &mut c.kind {
            EntityKind::Insert(i) => {
                if let Some(n) = renames.get(&i.block) {
                    i.block = n.clone();
                }
            }
            EntityKind::Dimension(dm) => {
                if let Some(n) = dm.block.as_ref().and_then(|b| renames.get(b)) {
                    dm.block = Some(n.clone());
                }
                dm.assoc.retain_mut(|a| {
                    let Some(h) = map.get(&a.handle) else { return false };
                    a.handle = *h;
                    if let aad_doc::AssocSnap::Intersection { other } = &mut a.snap {
                        match map.get(other) {
                            Some(o) => *other = *o,
                            None => return false,
                        }
                    }
                    true
                });
            }
            _ => {}
        }
        d.ensure_layer(&c.common.layer);
        out.push(c);
    }
    out
}

fn file_stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

/// Make the implicit level explicit when the drawing has none yet.
fn ensure_levels(d: &mut Drawing) -> Result<()> {
    if !d.has_levels() {
        let name = super::levels::implicit_name(d);
        d.materialize_levels(&name, 0.0, 0.0)?;
    }
    Ok(())
}

/// Add `store` to the drawing as a new level; returns its id.
fn add_level_with(d: &mut Drawing, name: &str, elevation: f64, height: f64, after: Option<LevelId>, store: EntityStore) -> Result<LevelId> {
    if is_blank(d) {
        // Nothing to keep: the new level is the first level, not a second one after an empty one.
        let id = d.materialize_levels(name, elevation, height)?;
        d.model = store;
        return Ok(id);
    }
    ensure_levels(d)?;
    let id = d.add_level(name, elevation, height, after)?;
    if let Some(l) = d.levels.iter_mut().find(|l| l.id == id) {
        l.entities = store;
    }
    Ok(id)
}

/// Add `store` to a sheet set's annotation of `level` (appending to what is there).
fn add_annotation(d: &mut Drawing, set: SheetSetId, level: LevelId, store: EntityStore) -> Result<()> {
    if d.sheet_set(set).is_none() || d.level(level).is_none() {
        return Err(bad("level.import", "no such sheet set or level"));
    }
    let in_model = d.target() == Some(aad_doc::EditTarget::Sheet(set, level));
    for e in store.iter() {
        let e = (**e).clone();
        if in_model {
            d.model.push(e);
        } else if let Some(s) = d.sheet_sets.iter_mut().find(|s| s.id == set) {
            s.annotations.entry(level).or_default().push(e);
        }
    }
    Ok(())
}

/// Whether the drawing has nothing in it yet: no objects and no explicit levels.
fn is_blank(d: &Drawing) -> bool {
    d.entity_count() == 0 && !d.has_levels()
}

/// Units for a file about to be added: a blank drawing (or one with no unit) takes the file's;
/// otherwise the file must be in the drawing's unit (or `ignore` is given).
fn take_units(d: &mut Drawing, src: &Drawing, path: &str, ignore: bool) -> Result<()> {
    let theirs = annot::drawing_unit(src).and_then(|u| u.insunits());
    if (is_blank(d) || annot::drawing_unit(d).is_none())
        && let Some(code) = theirs
    {
        d.header.set_i64("INSUNITS", code);
        return Ok(());
    }
    units_ok(d, src, path, ignore)
}

// ---------------- level.import ----------------

fn num(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| bad("level.import", format!("`{key}` must be a finite number"))),
    }
}

/// A level named by id (number or numeric text) or by name, looked up in `d`.
fn level_in(d: &Drawing, v: &Value) -> Option<LevelId> {
    if let Some(n) = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
        let id = LevelId(u32::try_from(n).unwrap_or(u32::MAX));
        if d.level(id).is_some() {
            return Some(id);
        }
    }
    v.as_str().and_then(|name| d.level_by_name(name)).map(|l| l.id)
}

/// A sheet set named by id or by name, looked up in `d`.
fn set_in(d: &Drawing, v: &Value) -> Option<SheetSetId> {
    if let Some(n) = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
        let id = SheetSetId(u32::try_from(n).unwrap_or(u32::MAX));
        if d.sheet_set(id).is_some() {
            return Some(id);
        }
    }
    v.as_str().and_then(|name| d.sheet_set_by_name(name)).map(|x| x.id)
}

/// The `layerMap` parameter as upper-case file layer -> target layer.
fn layer_map(p: &Value) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    match p.get("layerMap") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) if m.len() <= 4096 => {
            for (from, to) in m {
                let to = to.as_str().map(str::trim).filter(|t| !t.is_empty() && t.len() <= 255 && !t.chars().any(char::is_control));
                let to = to.ok_or_else(|| bad("level.import", format!("`layerMap` target for `{from}` must be a layer name")))?;
                if from.len() > 255 {
                    return Err(bad("level.import", "`layerMap` has a layer name that is too long"));
                }
                let dest = super::layermap::dest_of(to).ok_or_else(|| {
                    bad("level.import", format!("`layerMap`: \"{to}\" is not a built-in layer; only built-in layers are destinations"))
                })?;
                out.insert(from.trim().to_ascii_uppercase(), dest.name.to_string());
            }
        }
        Some(_) => return Err(bad("level.import", "`layerMap` is an object of file layer -> layer (at most 4096)")),
    }
    Ok(out)
}

/// Put the objects of mapped layers on their target layers (creating a standard target layer with its
/// standard look) and drop the emptied source layers, so the file's layers do not come along unused.
fn remap_layers(d: &mut Drawing, src: &mut Drawing, map: &BTreeMap<String, String>) {
    if map.is_empty() {
        return;
    }
    let mut out = EntityStore::new();
    // The file's layer names stay on record (project entry `layerSources`): source, destination, count.
    let mut moved: BTreeMap<(String, String), usize> = BTreeMap::new();
    for e in src.model.iter() {
        let mut c = (**e).clone();
        if let Some(to) = map.get(&c.common.layer.to_ascii_uppercase())
            && !to.eq_ignore_ascii_case(&c.common.layer)
        {
            *moved.entry((c.common.layer.clone(), to.clone())).or_insert(0) += 1;
            c.common.layer = to.clone();
        }
        out.push(c);
    }
    let record: Vec<(String, String, usize, Vec<aad_doc::Handle>)> = moved.into_iter().map(|((s, t), n)| (s, t, n, Vec::new())).collect();
    super::layermap::record_sources(d, &record);
    src.model = out;
    src.layers.retain(|l| !map.contains_key(&l.name.to_ascii_uppercase()) || map.values().any(|t| t.eq_ignore_ascii_case(&l.name)));
    for to in map.values() {
        if let Some(dest) = super::layermap::dest_of(to) {
            super::layermap::ensure_dest(d, &dest);
        }
    }
}

fn run_mapplan(_s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path")
        .filter(|t| !t.trim().is_empty() && t.len() < 1024 && !t.contains('\0'))
        .ok_or_else(|| bad("layer.mapplan", "`path` is required"))?;
    let src = load(path)?;
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for l in &src.layers {
        counts.entry(l.name.to_ascii_uppercase()).or_insert_with(|| (l.name.clone(), 0));
    }
    for e in src.model.iter() {
        let c = counts.entry(e.common.layer.to_ascii_uppercase()).or_insert_with(|| (e.common.layer.clone(), 0));
        c.1 = c.1.saturating_add(1);
    }
    let mut rows: Vec<(String, usize)> = counts.into_values().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let layers: Vec<Value> =
        rows.iter().take(2000).map(|(name, n)| json!({ "name": name, "objects": n, "suggested": super::sheetstd::suggest_target(name) })).collect();
    let targets: Vec<Value> =
        super::sheetstd::standard_targets().iter().map(|(n, tab, desc)| json!({ "name": n, "tab": tab, "description": desc })).collect();
    Ok(json!({ "path": path, "layers": layers, "targets": targets }))
}

fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path")
        .filter(|t| !t.trim().is_empty() && t.len() < 1024 && !t.contains('\0'))
        .ok_or_else(|| bad("level.import", "`path` is required"))?
        .to_string();
    if s.block_edit.is_some() {
        return Err(bad("level.import", "close the block editor first"));
    }
    let (elevation, height) = (num(p, "elevation")?.unwrap_or(0.0), num(p, "height")?.unwrap_or(0.0));
    let ignore = bool_or(p, "ignoreUnits", false);
    let mut src = load(&path)?;
    let map = layer_map(p)?;
    // Everything is done on a copy and put in place at the end, so a failure changes nothing.
    let mut d = s.doc()?.clone();
    take_units(&mut d, &src, &path, ignore)?;
    remap_layers(&mut d, &mut src, &map);
    let objects = src.model.len();
    let given = |key: &str| p.get(key).filter(|v| !v.is_null());
    let out = if let Some(sv) = given("sheetSet") {
        // Annotation of an existing sheet set on a level.
        let set = set_in(&d, sv).ok_or_else(|| bad("level.import", format!("no sheet set `{sv}`")))?;
        ensure_levels(&mut d)?;
        let level = match given("level") {
            Some(lv) => level_in(&d, lv).ok_or_else(|| bad("level.import", format!("no level `{lv}`")))?,
            None => d.active_level.ok_or_else(|| bad("level.import", "no active level"))?,
        };
        let store = adopt_model(&mut d, &src);
        add_annotation(&mut d, set, level, store)?;
        json!({ "sheetSet": d.sheet_set(set).map(|x| x.name.clone()), "level": d.level(level).map(|l| l.name.clone()), "objects": objects })
    } else {
        let after = match given("after") {
            Some(av) => Some(level_in(&d, av).ok_or_else(|| bad("level.import", format!("no level `{av}`")))?),
            None => None,
        };
        let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| file_stem(&path));
        let store = adopt_model(&mut d, &src);
        let id = add_level_with(&mut d, &name, elevation, height, after, store)?;
        json!({ "level": d.level(id).map(|l| l.name.clone()), "id": id.0, "objects": objects })
    };
    *s.doc_mut()? = d;
    Ok(out)
}

// ---------------- level.export ----------------

fn run_export(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("level.export", "files are not available on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = str_param(p, "path")
            .filter(|t| !t.trim().is_empty() && t.len() < 1024 && !t.contains('\0'))
            .ok_or_else(|| bad("level.export", "`path` is required"))?;
        let hooks = super::file::io().ok_or_else(|| bad("level.export", "file formats are not available in this build"))?;
        if !s.doc()?.has_levels() {
            return Err(bad("level.export", "the drawing has one level, so a normal save is already a plain file"));
        }
        let id = if p.get("level").is_some_and(|v| !v.is_null()) {
            super::levels::find(s, "level.export", p, "level")?
        } else {
            s.doc()?.active_level.ok_or_else(|| bad("level.export", "no active level"))?
        };
        let d = s.doc()?;
        let store = d.level_store(id).cloned().ok_or_else(|| bad("level.export", "no such level"))?;
        let objects = store.len();
        let target = Path::new(path);
        if target.is_dir() {
            return Err(bad("level.export", format!("`{path}` is a folder")));
        }
        if target.exists() && !bool_or(p, "overwrite", false) {
            return Err(bad("level.export", format!("`{path}` already exists; pass `overwrite` to replace it")));
        }
        let bytes = (hooks.write)(&super::exchange::plain(d, store), path).map_err(|e| bad("level.export", format!("{path}: {e}")))?;
        std::fs::write(target, bytes).map_err(|e| bad("level.export", format!("{path}: {e}")))?;
        Ok(json!({ "path": path, "objects": objects }))
    }
}

// ---------------- project.merge ----------------

/// The sheet set (name, layer prefix) a discipline layout category belongs to.
fn set_for(category: &str) -> (&'static str, &'static str) {
    match category {
        "ceiling" => ("False ceiling", "FC-"),
        "electrical" => ("Electrical", "E-"),
        "plumbing" => ("Plumbing", "P-"),
        "furniture" => ("Interior", "I-"),
        _ => ("Working drawing", "W-"),
    }
}

/// A drawing that holds only annotation (text, dimensions, leaders, hatches) and at least one object.
fn annotation_only(src: &Drawing) -> bool {
    !src.model.is_empty()
        && src.model.iter().all(|e| {
            matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_) | EntityKind::MLeader(_) | EntityKind::Hatch(_))
        })
}

fn unique_level_name(d: &Drawing, want: &str) -> String {
    let base: String = want.chars().take(60).collect();
    if d.level_by_name(&base).is_none() {
        return base;
    }
    (2..1000).map(|i| format!("{base} {i}")).find(|n| d.level_by_name(n).is_none()).unwrap_or(base)
}

fn project_path(s: &Session, p: &Value) -> Result<PathBuf> {
    match str_param(p, "path").filter(|t| !t.trim().is_empty() && t.len() < 1024 && !t.contains('\0')) {
        Some(t) => {
            let path = PathBuf::from(t);
            Ok(if path.is_dir() { path.join(super::projfile::MANIFEST) } else { path })
        }
        None => super::projfile::find_manifest(s).map(|(path, _)| path),
    }
}

fn run_merge(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("project.merge", "files are not available on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if s.block_edit.is_some() {
            return Err(bad("project.merge", "close the block editor first"));
        }
        let ignore = bool_or(p, "ignoreUnits", false);
        let as_sets = bool_or(p, "annotation", true);
        let manifest_path = project_path(s, p)?;
        let m = super::projfile::read_manifest(&manifest_path)?;
        let dir = manifest_path.parent().map(Path::to_path_buf).unwrap_or_default();
        // The project's own main drawing shows every floor as an Xref; merging into it would double them.
        if s.doc()?.blocks.values().any(|b| b.xref_path.is_some()) {
            return Err(bad(
                "project.merge",
                "this drawing has Xrefs (it looks like the project's main drawing). Create a new drawing and merge there",
            ));
        }
        let to_m = if m["units"].as_str() == Some("imperial") { 0.3048 } else { 1.0 };
        let mut d = s.doc()?.clone();
        let mut levels = Vec::new();
        let mut sets_used: Vec<String> = Vec::new();
        let mut skipped: Vec<Value> = Vec::new();
        let mut first_src: Option<Drawing> = None;
        for f in m["floors"].as_array().into_iter().flatten() {
            let name = f["name"].as_str().unwrap_or("Floor").to_string();
            let file = dir.join(f["file"].as_str().unwrap_or(""));
            let shown = file.display().to_string();
            if !file.is_file() {
                skipped.push(json!({ "file": f["file"], "reason": "the floor file does not exist" }));
                continue;
            }
            let src = load(&shown)?;
            take_units(&mut d, &src, &shown, ignore)?;
            let m_per_du = annot::drawing_unit(&d).map_or(0.001, |u| u.metres());
            let elevation = f["elevation"].as_f64().filter(|n| n.is_finite()).unwrap_or(0.0) * to_m / m_per_du;
            let height = f["height"].as_f64().filter(|n| n.is_finite() && *n >= 0.0).unwrap_or(0.0) * to_m / m_per_du;
            let store = adopt_model(&mut d, &src);
            let lname = unique_level_name(&d, &name);
            let id = add_level_with(&mut d, &lname, elevation, height, None, store)?;
            levels.push(json!({ "level": lname, "file": f["file"], "objects": src.model.len() }));
            if first_src.is_none() {
                first_src = Some(src);
            }
            for l in f["layouts"].as_array().into_iter().flatten() {
                let lfile = dir.join(l["file"].as_str().unwrap_or(""));
                let lshown = lfile.display().to_string();
                if !lfile.is_file() {
                    skipped.push(json!({ "file": l["file"], "reason": "the layout file does not exist" }));
                    continue;
                }
                let lsrc = load(&lshown)?;
                if lsrc.model.is_empty() {
                    skipped.push(json!({ "file": l["file"], "reason": "the layout is empty" }));
                    continue;
                }
                take_units(&mut d, &lsrc, &lshown, ignore)?;
                let category = l["id"].as_str().unwrap_or("").to_ascii_lowercase();
                if as_sets && annotation_only(&lsrc) {
                    let (set_name, prefix) = set_for(&category);
                    let set = match d.sheet_set_by_name(set_name).map(|x| x.id) {
                        Some(id) => id,
                        None => d.add_sheet_set(set_name, prefix)?,
                    };
                    let store = adopt_model(&mut d, &lsrc);
                    add_annotation(&mut d, set, id, store)?;
                    if !sets_used.iter().any(|n| n == set_name) {
                        sets_used.push(set_name.to_string());
                    }
                    levels.push(json!({ "sheetSet": set_name, "onLevel": lname, "file": l["file"], "objects": lsrc.model.len() }));
                } else {
                    let title = l["name"].as_str().unwrap_or(&category);
                    let nm = unique_level_name(&d, &format!("{lname} - {title}"));
                    let store = adopt_model(&mut d, &lsrc);
                    add_level_with(&mut d, &nm, elevation, height, None, store)?;
                    levels.push(json!({ "level": nm, "file": l["file"], "objects": lsrc.model.len() }));
                }
            }
        }
        if levels.is_empty() {
            return Err(bad("project.merge", "the project has no floor files to merge"));
        }
        // Keep the floor list in step with the levels when this drawing has none of its own.
        if let Some(src) = first_src {
            for key in ["floors", "details"] {
                if project::get(&d, key).is_null() {
                    let v = project::get(&src, key);
                    if !v.is_null() {
                        project::set(&mut d, key, &v);
                    }
                }
            }
        }
        *s.doc_mut()? = d;
        Ok(json!({ "levels": levels, "sheetSets": sets_used, "skipped": skipped }))
    }
}

//! The project configuration MynaOps writes into the `.osp` (`aad-odp::config`), applied to the drawing: `project.config.apply`, and the one-time
//! proposal a legacy drawing offers for adoption: `project.config.propose`. Spec: `docs/ops-first-workflow.md` sections 4.4 and 4.5.
//!
//! `project.config.apply` is one engine step (one undo entry, "Project configuration") that is whole or nothing: every part runs through the existing
//! commands (`drawing.units`, `project`, `floors`, `hcwlayers`, `level.new/set/delete`, `sheetset.new/set`, `sheet.scheme`); if a part fails the drawing and
//! the selection are put back exactly as they were and the failure is reported. A change that is already in the drawing is not written again, so applying
//! a configuration the drawing already has changes nothing and makes no undo entry. Nothing is ever deleted that has content: a level removed from the
//! configuration that holds objects stays and is reported as an orphan.
//!
//! The ids of the configuration (`cid`, `tid`) are bound to the drawing's own ids in the drawing's project map (`config.levelmap`, `config.tabmap`), so no
//! engine struct changes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aad_doc::{Drawing, LevelId};
use aad_odp::config::{self as cfg, Applied, AppliedItem, ProjectConfig, Proposal, ProposalLevel, ProposalTab};
use serde_json::{Map, Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("project.config.apply", "Project configuration", run_apply).params(
            "{config?: object (the JSON form of aad_odp::config::ProjectConfig: a member per part, `levels`, `units`, ..., each with `v`) | path?: text (a project file whose config.* keys are read, read only), record?: bool (write the result as the local key config.applied.cad into `path`; default false)} → {rev, result: ok|partial|failed, changed, items: [{key, status: ok|orphans|refused|failed, detail}], notes, recorded}. Applies what MynaOps configured: unit, project details, layer sets, levels, discipline tabs, sheet numbering scheme. One undo step (\"Project configuration\"); a failing part puts the drawing back as it was and is an error; a unit change after content exists is refused; a level with objects that left the configuration stays (orphan); nothing is deleted that has content. Applying what the drawing already has changes nothing.",
        ),
        CommandSpec::new("project.config.propose", "Project configuration proposal", run_propose).noundo().params(
            "{path?: text (a project file), record?: bool (write the proposal as the local key config.proposal into `path` when it has no configuration; default false), replace?: bool (replace a proposal already written)} → {proposal, recorded}. What this drawing already has (levels with their ids, tabs, layer sets present, unit, project details, numbering scheme), for MynaOps' Adopt page. Changes nothing in the drawing.",
        ),
    ]
}

// ---------------------------------------------------------------------------------------------------- the report

#[derive(Default)]
struct Report {
    items: Vec<(String, String, String)>,
}

impl Report {
    fn add(&mut self, key: &str, status: &str, detail: impl Into<String>) {
        self.items.push((key.to_string(), status.to_string(), detail.into()));
    }

    /// One line per key (worst status first, details joined), in the order the keys first appeared.
    fn collapse(&self) -> Vec<AppliedItem> {
        let rank = |s: &str| match s {
            "failed" => 3,
            "refused" => 2,
            "orphans" => 1,
            _ => 0,
        };
        let mut order: Vec<&str> = Vec::new();
        for (k, ..) in &self.items {
            if !order.contains(&k.as_str()) {
                order.push(k);
            }
        }
        order
            .into_iter()
            .take(cfg::MAX_ITEMS_REPORT)
            .map(|k| {
                let mine: Vec<&(String, String, String)> = self.items.iter().filter(|(x, ..)| x == k).collect();
                let status = mine.iter().map(|(_, s, _)| s.as_str()).max_by_key(|s| rank(s)).unwrap_or("ok");
                let mut detail = mine.iter().map(|(_, _, d)| d.as_str()).filter(|d| !d.is_empty()).collect::<Vec<_>>().join("; ");
                if detail.chars().count() > 1000 {
                    detail = detail.chars().take(1000).collect::<String>() + "...";
                }
                AppliedItem { key: k.to_string(), status: status.to_string(), detail }
            })
            .collect()
    }
}

fn call(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let spec = find_command(id).ok_or_else(|| bad("project.config.apply", format!("no command `{id}`")))?;
    (spec.run)(s, p)
}

fn has_content(d: &Drawing) -> bool {
    d.model_stores().any(|st| !st.is_empty())
}

fn mm_to_m(mm: f64) -> f64 {
    mm / 1000.0
}

/// Metres in one drawing unit; a drawing whose unit is not set counts as millimetres (what `level.setup` assumes).
fn unit_m(d: &Drawing) -> f64 {
    crate::annot::drawing_unit(d).map_or(0.001, |u| u.metres())
}

/// A level's elevation or height is kept in drawing units (`level.setup` converts the metres of the floor list to them): millimetres to drawing units.
fn mm_to_du(mm: f64, unit_m: f64) -> f64 {
    mm * 0.001 / unit_m
}

/// Drawing units to millimetres, to the third decimal.
fn du_to_mm(du: f64, unit_m: f64) -> f64 {
    (du * unit_m * 1000.0 * 1000.0).round() / 1000.0
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ---------------------------------------------------------------------------------------------------- reading the input

/// The configuration to apply: from `config` (the JSON form), else from the project file at `path`.
fn input(p: &Value) -> Result<cfg::Read> {
    const CMD: &str = "project.config.apply";
    match p.get("config") {
        Some(Value::Null) | None => {}
        Some(v) => return ProjectConfig::from_json(v).map_err(|e| bad(CMD, e.to_string())),
    }
    let path = str_param(p, "path").ok_or_else(|| bad(CMD, "`config` (the configuration) or `path` (a project file) is required"))?;
    read_file(path).map_err(|e| bad(CMD, e))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &str) -> std::result::Result<cfg::Read, String> {
    cfg::probe(std::path::Path::new(path)).map(|l| l.read).map_err(|e| format!("{path}: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn read_file(_path: &str) -> std::result::Result<cfg::Read, String> {
    Err("a project file cannot be read here".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn record_applied(path: &str, applied: &Applied) -> std::result::Result<(), String> {
    let mut odp = aad_odp::Odp::open(std::path::Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    cfg::write_applied(&mut odp, applied).map_err(|e| format!("{path}: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn record_applied(_path: &str, _applied: &Applied) -> std::result::Result<(), String> {
    Err("a project file cannot be written here".to_string())
}

// ---------------------------------------------------------------------------------------------------- apply

fn run_apply(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "project.config.apply";
    if s.block_edit.is_some() {
        return Err(bad(CMD, "close the block editor first"));
    }
    let read = input(p)?;
    let config = read.config;
    if !config.configured() {
        return Err(bad(CMD, "the project has no configuration (config.schema is missing): adopt it in MynaOps first"));
    }
    let rev = config.revision();
    let original = s.state()?.doc.clone();
    let selection = s.state()?.selection.clone();
    let mut rep = Report::default();
    let outcome = apply_parts(s, &config, &mut rep);
    let recording = bool_or(p, "record", false).then(|| str_param(p, "path")).flatten();
    let content = has_content(s.doc()?);
    for n in &read.notes {
        rep.add("config", "refused", n.clone());
    }
    if let Err(e) = outcome {
        // The drawing goes back exactly as it was (the same Arc: no undo entry is made for a step that changed nothing).
        let st = s.state_mut()?;
        st.doc = original;
        st.selection = selection;
        let msg = e.to_string();
        rep.add("config", "failed", msg.clone());
        if let Some(path) = recording {
            let applied = Applied { v: 1, rev, time: aad_odp::now_utc(), result: "failed".into(), has_content: content, items: rep.collapse() };
            let _ = record_applied(path, &applied);
        }
        return Err(bad(CMD, format!("the configuration was not applied and the drawing is unchanged: {msg}")));
    }
    let items = rep.collapse();
    let partial = items.iter().any(|i| i.status != "ok");
    let result = if partial { "partial" } else { "ok" };
    let changed = !Arc::ptr_eq(&original, &s.state()?.doc);
    let mut recorded = false;
    if let Some(path) = recording {
        let applied = Applied { v: 1, rev, time: aad_odp::now_utc(), result: result.into(), has_content: content, items: items.clone() };
        record_applied(path, &applied).map_err(|e| bad(CMD, format!("the configuration was applied but the result could not be recorded: {e}")))?;
        recorded = true;
    }
    Ok(json!({ "rev": rev, "result": result, "changed": changed, "items": items, "notes": read.notes, "recorded": recorded }))
}

fn apply_parts(s: &mut Session, c: &ProjectConfig, rep: &mut Report) -> Result<()> {
    if let Some(u) = &c.units {
        apply_units(s, u, rep)?;
    }
    apply_details(s, c, rep)?;
    if let Some(ls) = &c.layer_sets {
        apply_layer_sets(s, ls, rep)?;
    }
    if let Some(l) = &c.levels {
        apply_levels(s, l, rep)?;
    }
    if let Some(t) = &c.tabs {
        apply_tabs(s, t, rep)?;
    }
    if let Some(sh) = &c.sheets {
        apply_scheme(s, sh, rep)?;
    }
    if let Some(tb) = &c.titleblock {
        apply_titleblock(s, tb, rep)?;
    }
    Ok(())
}

// ---- unit

fn apply_units(s: &mut Session, u: &cfg::Units, rep: &mut Report) -> Result<()> {
    let want = crate::annot::Unit::parse(&u.drawing_unit).ok_or_else(|| bad("project.config.apply", "the unit is not one the drawing knows"))?;
    let (cur, content) = {
        let d = s.doc()?;
        (crate::annot::drawing_unit(d), has_content(d))
    };
    if cur == Some(want) {
        rep.add("units", "ok", "");
    } else if cur.is_some() && (u.locked || content) {
        let why = if u.locked { "the unit is locked" } else { "the drawing has content" };
        rep.add("units", "refused", format!("{why}: the drawing stays in {}; it is not rescaled", cur.map_or("its unit", |c| c.name())));
    } else {
        call(s, "drawing.units", &json!({ "units": want.name(), "mode": "metadata" }))?;
        rep.add("units", "ok", format!("set to {}", want.name()));
    }
    Ok(())
}

// ---- project details, team, structure defaults

fn detail_text(d: &Drawing, key: &str) -> String {
    match super::project::get(d, "details").get(key) {
        Some(Value::String(t)) => t.clone(),
        Some(v) if v.is_number() => v.to_string(),
        _ => String::new(),
    }
}

fn apply_details(s: &mut Session, c: &ProjectConfig, rep: &mut Report) -> Result<()> {
    let mut want: Vec<(&str, String)> = Vec::new();
    if let Some(p) = &c.project {
        let owner = if p.owner.is_empty() { p.client.clone() } else { p.owner.clone() };
        want.extend([
            ("title", p.title.clone()),
            ("owner", owner),
            ("location", p.location.clone()),
            ("address", p.address.clone()),
            ("pid", p.pid.clone()),
            ("siteArea", p.site_area.clone()),
            ("plotUse", p.plot_use.clone()),
            ("authority", p.authority.clone()),
            ("date", p.date.clone()),
            ("summary", p.summary.clone()),
        ]);
    }
    if let Some(t) = &c.team {
        want.extend([("architect", t.architect.clone()), ("engineer", t.engineer.clone())]);
    }
    if !want.is_empty() {
        let diff: Map<String, Value> = {
            let d = s.doc()?;
            want.iter().filter(|(k, v)| detail_text(d, k) != *v).map(|(k, v)| ((*k).to_string(), json!(v))).collect()
        };
        if diff.is_empty() {
            rep.add("project", "ok", "");
        } else {
            let n = diff.len();
            call(s, "project", &json!({ "details": Value::Object(diff) }))?;
            rep.add("project", "ok", format!("{n} details set"));
        }
    }
    if let Some(l) = &c.levels {
        let (slab, beam) = super::project::structure_mm(s.doc()?);
        let (ws, wb) = (l.defaults.slab_mm.unwrap_or(slab), l.defaults.beam_mm.unwrap_or(beam));
        if !close(ws, slab) || !close(wb, beam) {
            call(s, "project", &json!({ "slabMm": ws, "beamMm": wb }))?;
            rep.add("project", "ok", "slab and beam defaults set");
        }
    }
    Ok(())
}

// ---- layer sets

fn layer_set_names(set: &str) -> Vec<&'static str> {
    match set {
        "standard" => super::hcw::STANDARD_LAYERS.iter().map(|l| l.0).collect(),
        "permit" => super::hcw::PERMIT_LAYERS.iter().map(|l| l.0).collect(),
        "presentation" => super::hcw::PRESENTATION_LAYERS.iter().map(|l| l.0).collect(),
        _ => Vec::new(),
    }
}

fn layer_set_present(d: &Drawing, set: &str) -> bool {
    layer_set_names(set).into_iter().all(|n| d.layer(n).is_some())
}

fn apply_layer_sets(s: &mut Session, ls: &cfg::LayerSets, rep: &mut Report) -> Result<()> {
    let wanted: Vec<String> = ls.sets.iter().map(|x| x.trim().to_ascii_lowercase()).collect();
    for set in &wanted {
        if layer_set_present(s.doc()?, set) {
            rep.add("layer_sets", "ok", "");
        } else {
            let r = call(s, "hcwlayers", &json!({ "set": set }))?;
            rep.add("layer_sets", "ok", format!("{set}: {} layers created", r["created"].as_u64().unwrap_or(0)));
        }
    }
    for set in cfg::LAYER_SETS {
        if !wanted.iter().any(|w| w == set) && layer_set_present(s.doc()?, set) {
            rep.add("layer_sets", "ok", format!("{set} is no longer configured; its layers stay"));
        }
    }
    Ok(())
}

// ---- levels

fn map_of(d: &Drawing, key: &str) -> Option<BTreeMap<u32, u32>> {
    d.project.get(key).map(|t| cfg::levelmap_from_text(t))
}

fn put_map(s: &mut Session, key: &str, map: &BTreeMap<u32, u32>) -> Result<()> {
    let text = cfg::levelmap_to_text(map);
    let same = s.doc()?.project.get(key).is_some_and(|t| cfg::same_json(t, &text));
    if !same {
        s.doc_mut()?.project.insert(key.to_string(), text);
    }
    Ok(())
}

fn objects_in(d: &Drawing, id: LevelId) -> usize {
    d.level_store(id).map_or(0, aad_doc::EntityStore::len)
}

fn apply_levels(s: &mut Session, l: &cfg::Levels, rep: &mut Report) -> Result<()> {
    const CMD: &str = "project.config.apply";
    let had_map = s.doc()?.project.contains_key(cfg::KEY_LEVELMAP);
    let mut map = map_of(s.doc()?, cfg::KEY_LEVELMAP).unwrap_or_default();
    let in_config: BTreeSet<u32> = l.items.iter().map(|i| i.cid).collect();
    let um = unit_m(s.doc()?);

    // The implicit single level of a drawing without level tabs becomes the configured level it matches (by name), else the first one.
    let mut implicit_bound: Option<(usize, LevelId)> = None;
    if !l.items.is_empty() && !s.doc()?.has_levels() {
        let implicit = super::levels::implicit_name(s.doc()?);
        let at = l.items.iter().position(|i| i.name.trim().eq_ignore_ascii_case(&implicit)).unwrap_or(0);
        if let Some(item) = l.items.get(at) {
            let id = s.doc_mut()?.materialize_levels(&item.name, mm_to_du(item.elevation_mm, um), mm_to_du(item.height_mm, um))?;
            implicit_bound = Some((at, id));
        }
    }

    // Which drawing level each configured level is: the binding, else a level of the same name, else (a file adopted before any binding exists) the
    // level whose id is the cid.
    let mut resolved: Vec<Option<LevelId>> = Vec::new();
    let mut taken: BTreeSet<LevelId> = BTreeSet::new();
    for (i, item) in l.items.iter().enumerate() {
        let d = s.doc()?;
        let bound_elsewhere: BTreeSet<LevelId> =
            map.iter().filter(|(cid, _)| **cid != item.cid && in_config.contains(cid)).map(|(_, lid)| LevelId(*lid)).collect();
        let free = |id: LevelId| d.level(id).is_some() && !taken.contains(&id);
        let found = implicit_bound
            .filter(|(at, _)| *at == i)
            .map(|(_, id)| id)
            .or_else(|| map.get(&item.cid).map(|n| LevelId(*n)).filter(|id| free(*id)))
            .or_else(|| d.level_by_name(&item.name).map(|lv| lv.id).filter(|id| free(*id) && !bound_elsewhere.contains(id)))
            .or_else(|| (!had_map).then_some(LevelId(item.cid)).filter(|id| free(*id) && !bound_elsewhere.contains(id)));
        if let Some(id) = found {
            taken.insert(id);
        }
        resolved.push(found);
    }

    let mut made: BTreeSet<usize> = BTreeSet::new();
    let mut changes: Vec<String> = Vec::new();
    for i in 0..l.items.len() {
        let Some(item) = l.items.get(i) else { continue };
        let (el, h) = (mm_to_du(item.elevation_mm, um), mm_to_du(item.height_mm, um));
        match resolved.get(i).copied().flatten() {
            Some(id) => {
                let (cur_name, cur_el, cur_h) = {
                    let lv = s.doc()?.level(id).ok_or_else(|| bad(CMD, "a level disappeared while applying"))?;
                    (lv.name.clone(), lv.elevation, lv.height)
                };
                let mut set = Map::new();
                if cur_name.trim() != item.name.trim() {
                    set.insert("name".into(), json!(item.name.trim()));
                    changes.push(format!("{cur_name} renamed {}", item.name.trim()));
                }
                if !close(cur_el, el) {
                    set.insert("elevation".into(), json!(el));
                }
                if !close(cur_h, h) {
                    set.insert("height".into(), json!(h));
                }
                if !set.is_empty() {
                    set.insert("level".into(), json!(id.0));
                    call(s, "level.set", &Value::Object(set))?;
                }
            }
            None => {
                let after = i.checked_sub(1).and_then(|j| resolved.get(j).copied().flatten());
                let mut np = json!({ "name": item.name.trim(), "elevation": el, "height": h });
                if let Some(a) = after {
                    np["after"] = json!(a.0);
                }
                let r = call(s, "level.new", &np)?;
                let id = r["id"].as_u64().and_then(|n| u32::try_from(n).ok()).map(LevelId).ok_or_else(|| bad(CMD, "level.new gave no id"))?;
                if let Some(slot) = resolved.get_mut(i) {
                    *slot = Some(id);
                }
                made.insert(i);
                changes.push(format!("{} added", item.name.trim()));
            }
        }
    }

    // The configured levels keep the configuration's order among the places they occupy now.
    let ids: Vec<LevelId> = resolved.iter().flatten().copied().collect();
    let slots: Vec<usize> = {
        let d = s.doc()?;
        let mut v: Vec<usize> = d.levels.iter().enumerate().filter(|(_, lv)| ids.contains(&lv.id)).map(|(i, _)| i).collect();
        v.sort_unstable();
        v
    };
    let current: Vec<LevelId> = {
        let d = s.doc()?;
        slots.iter().filter_map(|i| d.levels.get(*i).map(|lv| lv.id)).collect()
    };
    if current != ids {
        for (slot, id) in slots.iter().zip(ids.iter()) {
            call(s, "level.set", &json!({ "level": id.0, "order": slot }))?;
        }
        changes.push("order set".into());
    }

    // The binding.
    for (item, id) in l.items.iter().zip(resolved.iter()) {
        if let Some(id) = id {
            map.insert(item.cid, id.0);
        }
    }

    // Levels that left the configuration: an empty one is deleted, one with objects stays and is reported.
    let gone: Vec<(u32, LevelId)> = map.iter().filter(|(cid, _)| !in_config.contains(cid)).map(|(c, lid)| (*c, LevelId(*lid))).collect();
    for (cid, id) in gone {
        let Some(name) = s.doc()?.level(id).map(|lv| lv.name.clone()) else {
            map.remove(&cid);
            continue;
        };
        let (n, traced, count) = {
            let d = s.doc()?;
            (objects_in(d, id), d.level_is_traced(id), d.levels.len())
        };
        if n == 0 && !traced && count > 1 {
            call(s, "level.delete", &json!({ "level": id.0 }))?;
            map.remove(&cid);
            changes.push(format!("{name} deleted (empty)"));
        } else {
            let why = if n > 0 {
                format!("{n} objects")
            } else if traced {
                "traced by another level".to_string()
            } else {
                "the last level".to_string()
            };
            rep.add("levels", "orphans", format!("{name} ({why}) is no longer configured and stays"));
        }
    }
    // Levels the drawing has that were never in the configuration (a legacy drawing): reported, never touched.
    let bound: BTreeSet<LevelId> = map.values().map(|n| LevelId(*n)).collect();
    let strays: Vec<(LevelId, String)> = s.doc()?.levels.iter().filter(|lv| !bound.contains(&lv.id)).map(|lv| (lv.id, lv.name.clone())).collect();
    for (id, name) in strays {
        rep.add("levels", "orphans", format!("{name} ({} objects) is not in the configuration", objects_in(s.doc()?, id)));
    }

    // The binding is written when it carries information: a drawing adopted as it is (every cid is its LevelId, nothing made) needs none, so applying
    // what the drawing already has changes nothing.
    if had_map || !made.is_empty() || map.iter().any(|(c, l)| c != l) {
        put_map(s, cfg::KEY_LEVELMAP, &map)?;
    }
    apply_floors(s, l, &made, rep)?;
    rep.add("levels", "ok", changes.join(", "));
    Ok(())
}

/// The floor list (project data the take-off reads) follows the configured levels: only the values the configuration gives, and only for floors that
/// exist, were just made, or whose height changed; a floor the drawing has no entry for is not invented.
fn apply_floors(s: &mut Session, l: &cfg::Levels, made: &BTreeSet<usize>, rep: &mut Report) -> Result<()> {
    let stored = matches!(super::project::get(s.doc()?, "floors"), Value::Array(ref a) if !a.is_empty());
    let mut list: Vec<Value> = if stored { super::project::floors(s.doc()?) } else { Vec::new() };
    let mut changed = false;
    // A drawing that has no floor list yet and just got its levels gets a floor entry for every configured level (the take-off reads them).
    let full = !stored && !made.is_empty();
    let d_slab = l.defaults.slab_mm;
    let d_beam = l.defaults.beam_mm;
    for (i, item) in l.items.iter().enumerate() {
        let pos = list.iter().position(|f| f["name"].as_str().is_some_and(|n| n.trim().eq_ignore_ascii_case(item.name.trim())));
        let h = mm_to_m(item.height_mm);
        let ceiling = item.ceiling_mm.or(l.defaults.ceiling_mm).map(mm_to_m);
        let lintel = item.lintel_bottom_mm.or(l.defaults.lintel_bottom_mm).map(mm_to_m);
        let slab = item.slab_mm.or(d_slab);
        let beam = item.beam_mm.or(d_beam);
        match pos {
            Some(at) => {
                let Some(f) = list.get_mut(at) else { continue };
                let mut want = Vec::new();
                if h > 0.0 {
                    want.push(("floorToFloor", h));
                }
                want.extend(ceiling.map(|v| ("ceiling", v)));
                want.extend(lintel.map(|v| ("lintel", v)));
                want.extend(slab.map(|v| ("slab", v)));
                want.extend(beam.map(|v| ("beam", v)));
                for (k, v) in want {
                    if !f.get(k).and_then(Value::as_f64).is_some_and(|c| close(c, v)) {
                        f[k] = json!(v);
                        changed = true;
                    }
                }
            }
            None if made.contains(&i) || full => {
                let (cs, cb) = super::project::structure_mm(s.doc()?);
                let ftf = if h > 0.0 { h } else { 3.0 };
                let c = ceiling.unwrap_or_else(|| (ftf - 0.15).min(2.85));
                let li = lintel.unwrap_or_else(|| (c - 0.3).min(2.1));
                let entry = json!({ "name": item.name.trim(), "floorToFloor": ftf, "ceiling": c, "lintel": li, "slab": slab.unwrap_or(cs), "beam": beam.unwrap_or(cb) });
                // Before the next configured floor that already has an entry, else at the end.
                let next = l
                    .items
                    .iter()
                    .skip(i + 1)
                    .find_map(|n| list.iter().position(|f| f["name"].as_str().is_some_and(|x| x.trim().eq_ignore_ascii_case(n.name.trim()))));
                match next {
                    Some(at) => list.insert(at, entry),
                    None => list.push(entry),
                }
                changed = true;
            }
            None => {}
        }
    }
    if changed {
        call(s, "floors", &json!({ "floors": list }))?;
        rep.add("levels", "ok", "floor heights set");
    }
    Ok(())
}

// ---- tabs

fn apply_tabs(s: &mut Session, t: &cfg::Tabs, rep: &mut Report) -> Result<()> {
    let had_map = s.doc()?.project.contains_key(cfg::KEY_TABMAP);
    let mut created = false;
    let mut map = map_of(s.doc()?, cfg::KEY_TABMAP).unwrap_or_default();
    let mut taken: BTreeSet<u32> = BTreeSet::new();
    let mut changes: Vec<String> = Vec::new();
    for item in &t.items {
        let found = {
            let d = s.doc()?;
            let exists = |id: u32| d.sheet_sets.iter().any(|x| x.id.0 == id) && !taken.contains(&id);
            map.get(&item.tid)
                .copied()
                .filter(|id| exists(*id))
                .or_else(|| {
                    d.sheet_set_by_name(&item.name)
                        .map(|x| x.id.0)
                        .filter(|id| exists(*id) && !map.iter().any(|(tid, lid)| lid == id && *tid != item.tid))
                })
                // A drawing adopted before any binding exists: the tid is the sheet set's own id.
                .or_else(|| (!had_map).then_some(item.tid).filter(|id| exists(*id)))
        };
        let id = match found {
            Some(id) => {
                let (name, prefix, layers, blocks) = {
                    let x = s
                        .doc()?
                        .sheet_sets
                        .iter()
                        .find(|x| x.id.0 == id)
                        .ok_or_else(|| bad("project.config.apply", "a tab disappeared while applying"))?;
                    (x.name.clone(), x.layer_prefix.clone(), x.layers.clone(), x.blocks.clone())
                };
                let mut set = Map::new();
                if name.trim() != item.name.trim() {
                    set.insert("name".into(), json!(item.name.trim()));
                    changes.push(format!("{name} renamed {}", item.name.trim()));
                }
                if prefix != item.layer_prefix {
                    set.insert("layerPrefix".into(), json!(item.layer_prefix));
                }
                if layers != item.layers {
                    set.insert("layers".into(), json!(item.layers));
                }
                if blocks != item.blocks {
                    set.insert("blocks".into(), json!(item.blocks));
                }
                if !set.is_empty() {
                    set.insert("set".into(), json!(id));
                    call(s, "sheetset.set", &Value::Object(set))?;
                    changes.push(format!("{} mapping set", item.name.trim()));
                }
                id
            }
            None => {
                let r = call(
                    s,
                    "sheetset.new",
                    &json!({ "name": item.name.trim(), "layerPrefix": item.layer_prefix, "layers": item.layers, "blocks": item.blocks }),
                )?;
                changes.push(format!("{} added", item.name.trim()));
                created = true;
                r["id"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad("project.config.apply", "sheetset.new gave no id"))?
            }
        };
        taken.insert(id);
        map.insert(item.tid, id);
    }
    // A tab that left the configuration is never deleted (it may hold annotation).
    let configured: BTreeSet<u32> = t.items.iter().map(|i| i.tid).collect();
    let bound: BTreeSet<u32> = map.values().copied().collect();
    let strays: Vec<String> = s
        .doc()?
        .sheet_sets
        .iter()
        .filter(|x| !bound.contains(&x.id.0) || map.iter().any(|(tid, lid)| *lid == x.id.0 && !configured.contains(tid)))
        .map(|x| x.name.clone())
        .collect();
    if !strays.is_empty() {
        rep.add("tabs", "ok", format!("not in the configuration, kept: {}", strays.join(", ")));
    }
    if had_map || created || map.iter().any(|(t, l)| t != l) {
        put_map(s, cfg::KEY_TABMAP, &map)?;
    }
    rep.add("tabs", "ok", changes.join(", "));
    Ok(())
}

// ---- sheet numbering scheme, title block defaults

fn apply_scheme(s: &mut Session, sh: &cfg::Sheets, rep: &mut Report) -> Result<()> {
    let sc = &sh.scheme;
    let want = json!({
        "pattern": sc.pattern, "width": sc.width, "step": sc.step,
        "disciplines": sc.disciplines.iter().map(|d| json!({ "code": d.code.trim().to_ascii_uppercase(), "name": d.name.trim(), "start": d.start })).collect::<Vec<_>>(),
    });
    if super::sheetindex::scheme_value(s.doc()?) == want {
        rep.add("sheets", "ok", "");
        return Ok(());
    }
    call(s, "sheet.scheme", &want)?;
    rep.add("sheets", "ok", "numbering scheme set");
    Ok(())
}

fn apply_titleblock(s: &mut Session, tb: &cfg::TitleBlock, rep: &mut Report) -> Result<()> {
    // The project-wide title-block defaults are kept in the drawing's project data. No title-block command reads them yet.
    let text = json!({ "v": 1, "fields": tb.fields }).to_string();
    let same = s.doc()?.project.get(cfg::KEY_TITLEBLOCK).is_some_and(|t| cfg::same_json(t, &text));
    if !same {
        s.doc_mut()?.project.insert(cfg::KEY_TITLEBLOCK.to_string(), text);
    }
    rep.add("titleblock", "ok", "stored as project data (no title-block command reads it yet)");
    Ok(())
}

// ---------------------------------------------------------------------------------------------------- propose

fn build_proposal(d: &Drawing) -> Proposal {
    let um = unit_m(d);
    let levels = if d.has_levels() {
        d.levels
            .iter()
            .take(cfg::MAX_LEVELS)
            .filter(|l| l.elevation.is_finite() && l.height.is_finite())
            .map(|l| ProposalLevel { cid: l.id.0, name: l.name.clone(), elevation_mm: du_to_mm(l.elevation, um), height_mm: du_to_mm(l.height, um) })
            .collect()
    } else {
        Vec::new()
    };
    let tabs = d
        .sheet_sets
        .iter()
        .take(cfg::MAX_TABS)
        .map(|x| ProposalTab {
            tid: x.id.0,
            name: x.name.clone(),
            layer_prefix: x.layer_prefix.clone(),
            layers: x.layers.clone(),
            blocks: x.blocks.clone(),
        })
        .collect();
    let layer_sets = cfg::LAYER_SETS.iter().filter(|set| layer_set_present(d, set)).map(|x| (*x).to_string()).collect();
    let mut project = BTreeMap::new();
    if let Value::Object(o) = super::project::get(d, "details") {
        for (k, v) in o.iter().take(cfg::MAX_MAP_ENTRIES) {
            let text = match v {
                Value::String(t) => t.clone(),
                Value::Number(n) => n.to_string(),
                _ => continue,
            };
            project.insert(k.clone(), text.chars().take(cfg::MAX_TEXT).collect());
        }
    }
    let scheme = d
        .project
        .contains_key(super::sheetindex::KEY)
        .then(|| super::sheetindex::scheme_value(d))
        .and_then(|v| serde_json::from_value::<cfg::Scheme>(v).ok());
    Proposal {
        v: 1,
        time: aad_odp::now_utc(),
        has_content: has_content(d),
        drawing_unit: crate::annot::drawing_unit(d).map(|u| u.name().to_string()),
        levels,
        tabs,
        layer_sets,
        project,
        scheme,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn record_proposal(path: &str, p: &Proposal, replace: bool) -> std::result::Result<bool, String> {
    let mut odp = aad_odp::Odp::open(std::path::Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    cfg::write_proposal(&mut odp, p, replace).map_err(|e| format!("{path}: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn record_proposal(_path: &str, _p: &Proposal, _replace: bool) -> std::result::Result<bool, String> {
    Err("a project file cannot be written here".to_string())
}

fn run_propose(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "project.config.propose";
    let proposal = build_proposal(s.doc()?);
    let mut recorded = false;
    if bool_or(p, "record", false) {
        let path = str_param(p, "path").ok_or_else(|| bad(CMD, "`path` (the project file) is required to record"))?;
        recorded = record_proposal(path, &proposal, bool_or(p, "replace", false)).map_err(|e| bad(CMD, e))?;
    }
    Ok(json!({ "proposal": serde_json::to_value(&proposal).unwrap_or(Value::Null), "recorded": recorded }))
}

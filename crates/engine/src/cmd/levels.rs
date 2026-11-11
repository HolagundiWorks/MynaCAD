//! Levels: named model spaces in one drawing (design: `docs/levels-and-sheet-sets.md`).
//!
//! `Drawing::model` holds the active level's objects, so every drafting command keeps working on
//! the active level. These commands add, rename, reorder, delete and switch levels. A drawing with
//! no explicit levels is one implicit level; the first `level.new` makes it explicit, named after
//! the current floor (else `Level 1`), and adds the new level after it.

use aad_doc::{Level, LevelId, MAX_LEVELS, Trace, TraceColour, TraceOrder};
use serde_json::{Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("level.list", "List Levels", run_list)
            .menu(&["Architecture", "Levels", "List"])
            .params("{} → {levels: [{id, name, active, elevation, height, objects, traces}], implicit}")
            .noundo(),
        CommandSpec::new("level.new", "New Level", run_new)
            .menu(&["Architecture", "Levels", "New Level"])
            .params("{name, elevation?, height?, after? (level id or name), first? (name for the existing level when the drawing has none yet)} → {id, name, levels}"),
        CommandSpec::new("level.set", "Story settings", run_set)
            .menu(&["Architecture", "Levels", "Story settings"])
            .params("{level (id or name), name?, elevation?, height?, order? (0-based tab position)} → {level}"),
        CommandSpec::new("level.delete", "Delete Level", run_delete)
            .menu(&["Architecture", "Levels", "Delete Level"])
            .params("{level, force? (delete although objects or traces are on it)} → {deleted, active}"),
        CommandSpec::new("level.trace", "Trace Levels", run_trace)
            .menu(&["Architecture", "Levels", "Trace"])
            .params("{level? (default the active one), add? (level or list), remove? (level or list), set? (an existing traced level to edit), fade? 0..90, colour? grey|true, order? below|above, snap? bool, layers? [names] | \"A, B\" | null or \"*\" (all)} → {level, traces}. A trace shows another level read-only and faded under (or over) this one; settings apply to each added trace and to `set`."),
        CommandSpec::new("level.moveto", "Move to Level", run_moveto)
            .menu(&["Architecture", "Levels", "Move to Level"])
            .params("{level, create? (make the level when it does not exist; a drawing with one implicit level gets it first), handles? (default the selection; objects on any level), from?: [x,y] (origin on the selection), to?: [x,y] (matching point; default site origin when `from` is set)} → {moved, created?, shifted?}. Moves objects to another level, whichever level they are on now; marks, layers and every other property go with them. With `from`/`to` they are shifted so the origin matches."),
        CommandSpec::new("level.copyto", "Copy to Levels", run_copyto)
            .menu(&["Architecture", "Levels", "Copy to Levels"])
            .params("{levels?: [level..] | level?: one level, handles?, from?: [x,y], to?: [x,y]} → {copied, shifted?}. Copies objects to other levels; with `from`/`to` copies are shifted so the origin matches (source stays put)."),
        CommandSpec::new("level.fromfloors", "Levels from Floors", run_fromfloors)
            .menu(&["Architecture", "Levels", "Levels from Floors"])
            .params("{} → {created: [names], levels}. Adds an empty level for each floor of the project data that has none; elevation is the sum of the floor heights below it."),
        CommandSpec::new("level.next", "Next Level", run_next)
            .menu(&["Architecture", "Levels", "Next Level"])
            .key("Cmd+PageDown")
            .params("{} → {active, objects}. Activates the level after the active one (the last wraps to the first)."),
        CommandSpec::new("level.prev", "Previous Level", run_prev)
            .menu(&["Architecture", "Levels", "Previous Level"])
            .key("Cmd+PageUp")
            .params("{} → {active, objects}. Activates the level before the active one (the first wraps to the last)."),
        CommandSpec::new("level.setup", "Levels for a New Drawing", run_setup)
            .menu(&["Architecture", "Levels", "Levels for This Drawing"])
            .params("{floors: [\"Name\" | {name, height? (m, default 3), elevation? (m)}] | {count, basements?, terrace?}, sheetSets? (default true: the five presets)} → {created, sheetSets, active}. Sets the drawing up with a level per floor, the floor list, and the sheet sets. One undo step; the drawing must not have levels yet."),
        CommandSpec::new("level.activate", "Activate Level", run_activate)
            .menu(&["Architecture", "Levels", "Activate Level"])
            .alias(&["levelactivate"])
            .params("{level} → {active, objects}. Makes the level the one drafting commands edit."),
    ]
}

/// A level named by id (number or numeric text) or by name.
pub(super) fn find(s: &Session, cmd: &str, p: &Value, key: &str) -> Result<LevelId> {
    let d = s.doc()?;
    let v = p.get(key).ok_or_else(|| bad(cmd, format!("`{key}` (a level id or name) is required")))?;
    if let Some(n) = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
        let id = LevelId(u32::try_from(n).unwrap_or(u32::MAX));
        if d.level(id).is_some() {
            return Ok(id);
        }
    }
    if let Some(name) = v.as_str()
        && let Some(l) = d.level_by_name(name)
    {
        return Ok(l.id);
    }
    Err(bad(cmd, format!("no level `{v}`; the levels are {}", names(d))))
}

fn names(d: &aad_doc::Drawing) -> String {
    d.levels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn level_json(d: &aad_doc::Drawing, l: &Level) -> Value {
    let objects = d.level_store(l.id).map_or(0, aad_doc::EntityStore::len);
    json!({
        "id": l.id.0, "name": l.name, "active": d.active_level == Some(l.id),
        "elevation": l.elevation, "height": l.height, "objects": objects,
        "traces": l.traces.iter().map(|t| t.level.0).collect::<Vec<_>>(),
    })
}

fn all(d: &aad_doc::Drawing) -> Vec<Value> {
    d.levels.iter().map(|l| level_json(d, l)).collect()
}

fn num(p: &Value, cmd: &str, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
    }
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    if d.has_levels() {
        return Ok(json!({ "levels": all(d), "implicit": false }));
    }
    let name = implicit_name(d);
    let one = json!({
        "id": 0, "name": name, "active": true, "elevation": 0.0, "height": 0.0,
        "objects": d.model.len(), "traces": Vec::<u32>::new(),
    });
    Ok(json!({ "levels": [one], "implicit": true }))
}

/// The name an implicit level takes when it becomes explicit: the current floor, else `Level 1`.
pub(super) fn implicit_name(d: &aad_doc::Drawing) -> String {
    project::active_floor(d).map(|(_, n)| n).filter(|n| aad_doc::level_name_ok(n)).unwrap_or_else(|| "Level 1".to_string())
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("level.new", "`name` is required"))?.to_string();
    let elevation = num(p, "level.new", "elevation")?.unwrap_or(0.0);
    let height = num(p, "level.new", "height")?.unwrap_or(0.0);
    if s.block_edit.is_some() {
        return Err(bad("level.new", "close the block editor first"));
    }
    let first = match str_param(p, "first") {
        Some(f) => f.to_string(),
        None => implicit_name(s.doc()?),
    };
    let after = if p.get("after").is_some_and(|v| !v.is_null()) { Some(find(s, "level.new", p, "after")?) } else { None };
    let d = s.doc_mut()?;
    if !d.has_levels() {
        if first.trim().eq_ignore_ascii_case(name.trim()) {
            return Err(bad("level.new", format!("`{}` is the name of the existing level; pick another name", first.trim())));
        }
        d.materialize_levels(&first, 0.0, 0.0)?;
    }
    let id = d.add_level(&name, elevation, height, after)?;
    Ok(json!({ "id": id.0, "name": name.trim(), "levels": all(d) }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let id = find(s, "level.set", p, "level")?;
    let elevation = num(p, "level.set", "elevation")?;
    let height = num(p, "level.set", "height")?;
    if height.is_some_and(|h| h < 0.0) {
        return Err(bad("level.set", "`height` cannot be negative"));
    }
    let order = match p.get("order") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("level.set", "`order` must be a whole number from 0"))?),
    };
    let d = s.doc_mut()?;
    if let Some(n) = str_param(p, "name") {
        let old = d.level(id).map(|l| l.name.clone()).unwrap_or_default();
        d.rename_level(id, n)?;
        let new = d.level(id).map(|l| l.name.clone()).unwrap_or_default();
        if old != new {
            sync_floor_name(d, &old, &new);
        }
    }
    if let Some(l) = d.levels.iter_mut().find(|l| l.id == id) {
        if let Some(e) = elevation {
            l.elevation = e;
        }
        if let Some(h) = height {
            l.height = h;
        }
    }
    if let Some(o) = order {
        d.move_level(id, o)?;
    }
    let out = d.level(id).map(|l| level_json(d, l)).unwrap_or(Value::Null);
    Ok(json!({ "level": out }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = find(s, "level.delete", p, "level")?;
    if s.block_edit.is_some() {
        return Err(bad("level.delete", "close the block editor first"));
    }
    let force = bool_or(p, "force", false);
    let d = s.doc()?;
    if d.levels.len() <= 1 {
        return Err(bad("level.delete", "the last level cannot be deleted"));
    }
    let objects = d.level_store(id).map_or(0, aad_doc::EntityStore::len);
    if !force && objects > 0 {
        return Err(bad("level.delete", format!("the level has {objects} objects; pass `force` to delete them")));
    }
    if !force && d.level_is_traced(id) {
        return Err(bad("level.delete", "another level traces this one; pass `force` to delete it and those traces"));
    }
    let name = d.level(id).map(|l| l.name.clone()).unwrap_or_default();
    let was_active = d.active_level == Some(id);
    s.doc_mut()?.remove_level(id)?;
    if was_active {
        s.set_selection(Vec::new());
    }
    let d = s.doc()?;
    Ok(json!({ "deleted": name, "active": d.active_level_ref().map(|l| l.name.clone()) }))
}

fn step(s: &mut Session, cmd: &str, delta: isize) -> Result<Value> {
    let d = s.doc()?;
    if d.levels.len() < 2 {
        return Err(bad(cmd, "the drawing has fewer than two levels"));
    }
    let n = d.levels.len() as isize;
    let at = d.levels.iter().position(|l| Some(l.id) == d.active_level).unwrap_or(0) as isize;
    let to = d.levels.get((at + delta).rem_euclid(n) as usize).map(|l| l.id).ok_or_else(|| bad(cmd, "no such level"))?;
    run_activate(s, &json!({ "level": to.0 }))
}

fn run_next(s: &mut Session, _p: &Value) -> Result<Value> {
    step(s, "level.next", 1)
}

fn run_prev(s: &mut Session, _p: &Value) -> Result<Value> {
    step(s, "level.prev", -1)
}

/// A layout (paper space) is on screen: choosing a level shows that level in model space, so the
/// tab that is highlighted, the drawing that is displayed and the active level always agree.
fn show_model_tab(s: &mut Session) -> Result<()> {
    let st = s.state_mut()?;
    if matches!(st.space, aad_doc::Space::Paper(_)) {
        st.space = aad_doc::Space::Model;
        st.mspace = None;
        st.selection.clear();
    }
    Ok(())
}

fn run_activate(s: &mut Session, p: &Value) -> Result<Value> {
    let id = find(s, "level.activate", p, "level")?;
    if s.block_edit.is_some() {
        return Err(bad("level.activate", "close the block editor first"));
    }
    show_model_tab(s)?;
    // A section or elevation tab counts as another place: choosing the level leaves it.
    if s.doc()?.active_level != Some(id) || s.doc()?.active_view.is_some() {
        s.doc_mut()?.switch_level(id)?;
        s.set_selection(Vec::new());
        // The floor selector follows the level when the level is one of the project's floors.
        let name = s.doc()?.level(id).map(|l| l.name.clone());
        if let Some(name) = name {
            let d = s.doc_mut()?;
            if crate::cmd::project_floors(d).iter().any(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&name))) {
                project::set(d, "activeFloor", &json!(name));
            }
        }
    }
    let d = s.doc()?;
    Ok(json!({ "active": d.active_level_ref().map(|l| l.name.clone()), "objects": d.model.len() }))
}

/// Keep the project's floor list, the chosen floor and the title blocks' `FLOOR` fields in step
/// with a level that was renamed.
fn sync_floor_name(d: &mut aad_doc::Drawing, old: &str, new: &str) {
    if let Value::Array(mut fl) = project::get(d, "floors") {
        let mut changed = false;
        for f in &mut fl {
            if f.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(old)) {
                f["name"] = json!(new);
                changed = true;
            }
        }
        if changed {
            project::set(d, "floors", &Value::Array(fl));
            if project::get(d, "activeFloor").as_str().is_some_and(|n| n.eq_ignore_ascii_case(old)) {
                project::set(d, "activeFloor", &json!(new));
            }
        }
    }
    for layout in &mut d.layouts {
        let hits: Vec<aad_doc::Handle> = layout
            .entities
            .iter()
            .filter(|e| matches!(&e.kind, aad_doc::EntityKind::Insert(i) if i.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("FLOOR") && a.text.value.eq_ignore_ascii_case(old))))
            .map(|e| e.handle)
            .collect();
        for h in hits {
            layout.entities.modify(h, |e| {
                if let aad_doc::EntityKind::Insert(i) = &mut e.kind {
                    for a in &mut i.attribs {
                        if a.tag.eq_ignore_ascii_case("FLOOR") && a.text.value.eq_ignore_ascii_case(old) {
                            a.text.value = new.to_string();
                        }
                    }
                }
            });
        }
    }
}

/// The objects to carry to another level: `handles` or the selection, kept to the active level.
fn objects_of_active(s: &Session, cmd: &str, p: &Value) -> Result<Vec<aad_doc::Handle>> {
    if s.doc()?.active_sheet.is_some() {
        return Err(bad(
            cmd,
            "this is a sheet view: the objects are annotation, not level geometry. Switch to Design first (sheetset.activate with null)",
        ));
    }
    if !s.doc()?.has_levels() {
        return Err(bad(cmd, "the drawing has one level; add another with level.new first"));
    }
    let d = s.doc()?;
    let hs: Vec<_> = targets(s, p)?.into_iter().filter(|h| d.model.contains(*h)).collect();
    if hs.is_empty() {
        return Err(bad(cmd, "no objects of the active level are selected"));
    }
    Ok(hs)
}

/// Optional world shift so a picked origin lands on a matching point (default: site origin).
fn origin_shift(s: &Session, cmd: &str, p: &Value) -> Result<Option<aad_geom::Vec2>> {
    let Some(from) = point_param(p, "from") else {
        return Ok(None);
    };
    if !from.is_finite() {
        return Err(bad(cmd, "`from` must be a finite point"));
    }
    let to = match point_param(p, "to") {
        Some(t) if t.is_finite() => t,
        Some(_) => return Err(bad(cmd, "`to` must be a finite point")),
        None => s.doc()?.site_origin(),
    };
    let delta = to - from;
    if !delta.is_finite() {
        return Err(bad(cmd, "origin shift is not finite"));
    }
    if delta.len() < 1e-12 {
        return Ok(None);
    }
    Ok(Some(delta))
}

/// Where an object lives: the active level's `model`, or the store of another level. Objects that
/// are not on a level (layout objects, sheet annotation, block definitions) are not listed.
fn level_of_object(d: &aad_doc::Drawing, h: aad_doc::Handle) -> Option<Option<LevelId>> {
    if d.model.contains(h) {
        return Some(d.active_level);
    }
    d.levels.iter().find(|l| l.entities.contains(h)).map(|l| Some(l.id))
}

fn run_moveto(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "level.moveto";
    if s.block_edit.is_some() {
        return Err(bad(CMD, "close the block editor first"));
    }
    if s.doc()?.active_sheet.is_some() {
        return Err(bad(
            CMD,
            "this is a sheet view: the objects are annotation, not level geometry. Switch to Design first (sheetset.activate with null)",
        ));
    }
    let create = bool_or(p, "create", false);
    let wanted = match p.get("level") {
        Some(Value::String(t)) => Some(t.trim().to_string()),
        _ => None,
    };
    // Resolve the target without changing anything: an existing level, or (with `create`) a name that can become one.
    let existing = find(s, CMD, p, "level").ok();
    if existing.is_none() {
        match (&wanted, create) {
            (Some(name), true) if aad_doc::level_name_ok(name) => {}
            (_, true) => return Err(bad(CMD, "`level` must be a level name of 1 to 80 characters to create it")),
            _ => {
                find(s, CMD, p, "level")?;
            }
        }
    }
    let d = s.doc()?;
    let requested = targets(s, p)?;
    // Objects of any level; those already on the target level stay where they are.
    let mut hs: Vec<(aad_doc::Handle, Option<LevelId>)> = Vec::new();
    for h in requested {
        if let Some(from) = level_of_object(d, h) {
            if existing.is_some() && from == existing {
                continue;
            }
            if !hs.iter().any(|(x, _)| *x == h) {
                hs.push((h, from));
            }
        }
    }
    if hs.is_empty() {
        return Err(bad(
            CMD,
            if existing.is_some() && d.has_levels() {
                "the objects are already on that level, or none of them is on a level"
            } else {
                "no objects of a level are selected"
            },
        ));
    }
    let shift = origin_shift(s, CMD, p)?;
    // Everything is checked; from here on the drawing changes. Create the level first.
    let mut created = None;
    let to = match existing {
        Some(id) => id,
        None => {
            let name = wanted.clone().unwrap_or_default();
            let first = implicit_name(s.doc()?);
            // On a copy, so a refused level (a duplicate name, too many levels) leaves the drawing as it was.
            let mut nd = s.doc()?.clone();
            if !nd.has_levels() {
                if first.trim().eq_ignore_ascii_case(name.trim()) {
                    return Err(bad(CMD, format!("`{}` is the name of the existing level; pick another name", first.trim())));
                }
                nd.materialize_levels(&first, 0.0, 0.0)?;
            }
            let id = nd.add_level(&name, 0.0, 0.0, None)?;
            *s.doc_mut()? = nd;
            created = Some(name);
            id
        }
    };
    let d = s.doc_mut()?;
    let mut moved = Vec::with_capacity(hs.len());
    for (h, from) in &hs {
        // An implicit level became the first explicit one above, and is the active level now.
        let from = from.or(d.active_level);
        if from == Some(to) {
            continue;
        }
        let taken = if from == d.active_level {
            d.model.remove(*h)
        } else {
            d.levels.iter_mut().find(|l| Some(l.id) == from).and_then(|l| l.entities.remove(*h))
        };
        if let Some(e) = taken {
            moved.push(e);
        }
    }
    let n = moved.len();
    // The target is either another level's store, or `model` when it is the active level.
    let shifted = |e: std::sync::Arc<aad_doc::Entity>| {
        let mut c = (*e).clone();
        if let Some(delta) = shift {
            c.kind.transform(&aad_geom::Mat3::translate(delta));
        }
        c
    };
    if d.active_level == Some(to) {
        for e in moved {
            d.model.push(shifted(e));
        }
    } else if let Some(l) = d.levels.iter_mut().find(|l| l.id == to) {
        for e in moved {
            l.entities.push(shifted(e));
        }
    }
    s.set_selection(Vec::new());
    let mut out = json!({ "moved": n });
    if let Some(name) = created {
        out["created"] = json!(name);
    }
    if let Some(delta) = shift {
        out["shifted"] = json!([delta.x, delta.y]);
    }
    Ok(out)
}

/// Most objects one `level.copyto` may create.
const MAX_COPIES: usize = 2_000_000;

fn copyto_targets(s: &Session, p: &Value) -> Result<Vec<LevelId>> {
    let raw: Vec<Value> = if let Some(arr) = p.get("levels").and_then(Value::as_array) {
        arr.clone()
    } else if let Some(v) = p.get("level") {
        // One target (form / shortcut menu) or a comma-separated name list.
        match v {
            Value::String(text) if text.contains(',') => {
                text.split(',').map(str::trim).filter(|n| !n.is_empty()).map(|n| Value::String(n.to_string())).collect()
            }
            other => vec![other.clone()],
        }
    } else {
        return Err(bad("level.copyto", "`levels` (a list) or `level` (one id/name) is required"));
    };
    if raw.is_empty() || raw.len() > MAX_LEVELS {
        return Err(bad("level.copyto", format!("need 1 to {MAX_LEVELS} target levels")));
    }
    let mut to = Vec::new();
    for v in raw {
        let id = find(s, "level.copyto", &json!({ "level": v }), "level")?;
        if s.doc()?.active_level == Some(id) {
            return Err(bad("level.copyto", "the active level is the source; pick other levels"));
        }
        if !to.contains(&id) {
            to.push(id);
        }
    }
    Ok(to)
}

fn run_copyto(s: &mut Session, p: &Value) -> Result<Value> {
    let to = copyto_targets(s, p)?;
    let hs = objects_of_active(s, "level.copyto", p)?;
    if hs.len().saturating_mul(to.len()) > MAX_COPIES {
        return Err(bad("level.copyto", "too many objects to copy at once"));
    }
    let shift = origin_shift(s, "level.copyto", p)?;
    let d = s.doc_mut()?;
    let source: Vec<_> = hs.iter().filter_map(|h| d.model.get(*h).cloned()).collect();
    let mut copied = 0usize;
    for id in to {
        let mut fresh = Vec::with_capacity(source.len());
        for e in &source {
            let mut c = (**e).clone();
            c.handle = d.new_handle();
            if let Some(delta) = shift {
                c.kind.transform(&aad_geom::Mat3::translate(delta));
            }
            fresh.push(c);
        }
        copied += fresh.len();
        if let Some(l) = d.levels.iter_mut().find(|l| l.id == id) {
            for e in fresh {
                l.entities.push(e);
            }
        }
    }
    let mut out = json!({ "copied": copied });
    if let Some(delta) = shift {
        out["shifted"] = json!([delta.x, delta.y]);
    }
    Ok(out)
}

/// Add an empty level for each floor of `floors` that has none; the existing objects become the
/// first floor's level. A floor's elevation is its own `elevation` (metres) or the sum of the
/// heights below it. Returns the names created.
fn levels_from_floors(d: &mut aad_doc::Drawing, floors: &[Value]) -> Result<Vec<String>> {
    let m = crate::annot::drawing_unit(d).map_or(0.001, |u| u.metres());
    let mut created = Vec::new();
    let mut running = 0.0;
    for f in floors {
        let Some(name) = f.get("name").and_then(Value::as_str).filter(|n| aad_doc::level_name_ok(n)) else { continue };
        let ftf = f.get("floorToFloor").and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0).unwrap_or(3.0);
        let elevation = f.get("elevation").and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(running);
        running = elevation + ftf;
        if d.level_by_name(name).is_some() {
            continue;
        }
        let (elev_du, height_du) = (elevation / m, ftf / m);
        if !d.has_levels() {
            d.materialize_levels(name, elev_du, height_du)?;
        } else {
            d.add_level(name, elev_du, height_du, None)?;
        }
        created.push(name.to_string());
    }
    Ok(created)
}

fn run_fromfloors(s: &mut Session, _p: &Value) -> Result<Value> {
    if s.block_edit.is_some() {
        return Err(bad("level.fromfloors", "close the block editor first"));
    }
    let Value::Array(floors) = project::get(s.doc()?, "floors") else {
        return Err(bad("level.fromfloors", "the drawing has no floor list; set one with the `floors` command"));
    };
    if floors.is_empty() || floors.len() > MAX_LEVELS {
        return Err(bad("level.fromfloors", format!("the floor list needs 1 to {MAX_LEVELS} floors")));
    }
    let mut d = s.doc()?.clone();
    let created = levels_from_floors(&mut d, &floors)?;
    let out = all(&d);
    *s.doc_mut()? = d;
    Ok(json!({ "created": created, "levels": out }))
}

/// The floors `level.setup` was asked for, as floor-list entries (`name`, `floorToFloor` in
/// metres, optional `elevation`, with the ceiling and lintel the floor list needs).
pub(super) fn setup_floors(p: &Value) -> Result<Vec<Value>> {
    const CMD: &str = "level.setup";
    let mut raw: Vec<(String, f64, Option<f64>)> = Vec::new();
    match p.get("floors") {
        Some(Value::Array(a)) => {
            for f in a {
                let (name, h, el) = match f {
                    Value::String(n) => (n.clone(), 3.0, None),
                    Value::Object(_) => (
                        f.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        f.get("height")
                            .or_else(|| f.get("floorToFloor"))
                            .map_or(Some(3.0), Value::as_f64)
                            .ok_or_else(|| bad(CMD, "a floor `height` is a number of metres"))?,
                        f.get("elevation").map(|v| v.as_f64().ok_or_else(|| bad(CMD, "a floor `elevation` is a number of metres"))).transpose()?,
                    ),
                    _ => return Err(bad(CMD, "each floor is a name or {name, height?, elevation?}")),
                };
                raw.push((name, h, el));
            }
        }
        Some(Value::Object(o)) => {
            let count =
                o.get("count").and_then(Value::as_u64).filter(|c| (1..=60).contains(c)).ok_or_else(|| bad(CMD, "`floors.count` is 1 to 60"))?;
            let basements = o.get("basements").and_then(Value::as_u64).unwrap_or(0).min(10);
            // One floor-to-floor height (metres) for every floor of the count form; 3 m by default.
            let h = match o.get("floorToFloor").or_else(|| o.get("height")) {
                None | Some(Value::Null) => 3.0,
                Some(v) => {
                    v.as_f64().filter(|n| n.is_finite() && (2.0..=30.0).contains(n)).ok_or_else(|| bad(CMD, "`floorToFloor` is 2 to 30 metres"))?
                }
            };
            for b in (1..=basements).rev() {
                raw.push((format!("Basement {b}"), h, Some(-h * b as f64)));
            }
            let ordinal = ["Ground", "First", "Second", "Third", "Fourth", "Fifth", "Sixth", "Seventh", "Eighth", "Ninth", "Tenth"];
            for i in 0..count {
                raw.push((ordinal.get(i as usize).map_or_else(|| format!("Floor {i}"), |n| format!("{n} Floor")), h, None));
            }
            if o.get("terrace").and_then(Value::as_bool).unwrap_or(false) {
                raw.push(("Terrace".into(), h, None));
            }
        }
        _ => return Err(bad(CMD, "`floors` is a list of floors or {count, basements?, terrace?}")),
    }
    if raw.is_empty() || raw.len() > MAX_LEVELS {
        return Err(bad(CMD, format!("between 1 and {MAX_LEVELS} floors")));
    }
    let mut seen = std::collections::HashSet::new();
    raw.into_iter()
        .map(|(name, h, el)| {
            if !aad_doc::level_name_ok(&name) || !seen.insert(name.trim().to_ascii_lowercase()) {
                return Err(bad(CMD, format!("floor `{name}`: a name is 1 to 64 characters and unique")));
            }
            if !(h.is_finite() && (2.0..=30.0).contains(&h)) || el.is_some_and(|e| !e.is_finite() || e.abs() > 1000.0) {
                return Err(bad(CMD, format!("floor `{name}`: the height is 2 to 30 m and the elevation within 1000 m")));
            }
            let ceiling = (h - 0.15).min(2.85);
            let mut f = json!({ "name": name.trim(), "floorToFloor": h, "ceiling": ceiling, "lintel": (ceiling - 0.3).min(2.1) });
            if let Some(e) = el {
                f["elevation"] = json!(e);
            }
            Ok(f)
        })
        .collect()
}

pub(super) fn run_setup(s: &mut Session, p: &Value) -> Result<Value> {
    if s.block_edit.is_some() {
        return Err(bad("level.setup", "close the block editor first"));
    }
    if s.doc()?.has_levels() {
        return Err(bad("level.setup", "the drawing already has levels; add floors with level.new or level.fromfloors"));
    }
    let floors = setup_floors(p)?;
    let with_sets = bool_or(p, "sheetSets", true);
    let mut d = s.doc()?.clone();
    // What is already drawn stays on the ground floor, not on whichever floor is lowest.
    let existing = std::mem::take(&mut d.model);
    project::set(&mut d, "floors", &Value::Array(floors.clone()));
    let created = levels_from_floors(&mut d, &floors)?;
    let mut sets = Vec::new();
    if with_sets {
        // The standard tabs with their layer and block mapping, layers and symbols.
        let missing: Vec<String> =
            super::sheetset::PRESETS.iter().filter(|(n, _)| d.sheet_set_by_name(n).is_none()).map(|(n, _)| (*n).to_string()).collect();
        if !missing.is_empty() {
            sets = super::sheetstd::apply_standard(&mut d, Some(&missing), true, true)?.sets;
        }
    }
    // Drafting starts on the ground floor (the lowest floor at or above zero), else the first.
    let ground = d.levels.iter().find(|l| l.elevation >= 0.0).or_else(|| d.levels.first()).map(|l| (l.id, l.name.clone()));
    if let Some((id, name)) = ground {
        d.switch_level(id)?;
        project::set(&mut d, "activeFloor", &json!(name));
    }
    d.model = existing;
    let active = d.active_level_ref().map(|l| l.name.clone());
    *s.doc_mut()? = d;
    Ok(json!({ "created": created, "sheetSets": sets, "active": active }))
}

/// Levels named by a single value or a list, resolved to ids (at most `MAX_LEVELS`).
fn level_refs(s: &Session, cmd: &str, p: &Value, key: &str) -> Result<Vec<LevelId>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let items: Vec<&Value> = match v.as_array() {
        Some(a) if a.len() <= MAX_LEVELS => a.iter().collect(),
        Some(_) => return Err(bad(cmd, format!("`{key}` lists too many levels"))),
        None => vec![v],
    };
    items.into_iter().map(|i| find(s, cmd, &json!({ "level": i }), "level")).collect()
}

/// Apply the trace settings given in `p` to `t`.
pub(super) fn apply_trace_settings(t: &mut Trace, p: &Value) -> Result<()> {
    const CMD: &str = "level.trace";
    if let Some(v) = p.get("fade").filter(|v| !v.is_null()) {
        // A dialog sends numbers as floats: a whole float is fine.
        let whole = v.as_u64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && (0.0..=90.0).contains(f)).map(|f| f as u64));
        let n = whole.filter(|n| *n <= 90).ok_or_else(|| bad(CMD, "`fade` is a whole number from 0 to 90 (percent)"))?;
        t.fade = n as u8;
    }
    if let Some(c) = str_param(p, "colour").or_else(|| str_param(p, "color")) {
        t.colour = match c.to_ascii_lowercase().as_str() {
            "grey" | "gray" => TraceColour::Grey,
            "true" | "truecolour" | "truecolor" => TraceColour::TrueColour,
            _ => return Err(bad(CMD, "`colour` is `grey` or `true`")),
        };
    }
    if let Some(o) = str_param(p, "order") {
        t.order = match o.to_ascii_lowercase().as_str() {
            "below" => TraceOrder::Below,
            "above" => TraceOrder::Above,
            _ => return Err(bad(CMD, "`order` is `below` or `above`")),
        };
    }
    if let Some(v) = p.get("snap").filter(|v| !v.is_null()) {
        t.snap = v.as_bool().ok_or_else(|| bad(CMD, "`snap` is true or false"))?;
    }
    match p.get("layers") {
        None => {}
        Some(Value::Null) => t.layers = None,
        Some(Value::Array(a)) if a.len() <= 256 => {
            let names: Option<Vec<String>> = a.iter().map(|v| v.as_str().filter(|n| n.chars().count() <= 255).map(str::to_string)).collect();
            t.layers = Some(names.ok_or_else(|| bad(CMD, "`layers` is a list of layer names"))?);
        }
        // Typed in a dialog: names separated by commas, `*` or nothing for all layers.
        Some(Value::String(text)) => {
            let names: Vec<String> = text.split(',').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect();
            if names.len() > 256 || names.iter().any(|n| n.chars().count() > 255) {
                return Err(bad(CMD, "`layers` is at most 256 layer names of at most 255 characters"));
            }
            t.layers = if names.is_empty() || names == ["*"] { None } else { Some(names) };
        }
        Some(_) => return Err(bad(CMD, "`layers` is a list of at most 256 layer names, a comma-separated text, or null for all")),
    }
    Ok(())
}

fn trace_json(d: &aad_doc::Drawing, t: &Trace) -> Value {
    json!({
        "level": t.level.0, "name": d.level(t.level).map(|l| l.name.clone()), "fade": t.fade,
        "colour": if t.colour == TraceColour::Grey { "grey" } else { "true" },
        "order": if t.order == TraceOrder::Below { "below" } else { "above" },
        "snap": t.snap, "layers": t.layers,
    })
}

fn run_trace(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "level.trace";
    if !s.doc()?.has_levels() {
        return Err(bad(CMD, "the drawing has one level; add another with level.new first"));
    }
    let on = if p.get("level").is_some_and(|v| !v.is_null()) {
        find(s, CMD, p, "level")?
    } else {
        s.doc()?.active_level.ok_or_else(|| bad(CMD, "no active level"))?
    };
    let add = level_refs(s, CMD, p, "add")?;
    let remove = level_refs(s, CMD, p, "remove")?;
    let set = level_refs(s, CMD, p, "set")?;
    // Work out every trace first so a bad parameter changes nothing.
    let existing = |id: LevelId| s.doc().ok().and_then(|d| d.level(on)).and_then(|l| l.traces.iter().find(|t| t.level == id).cloned());
    let mut new_traces = Vec::new();
    for id in add {
        let mut t = existing(id).unwrap_or_else(|| Trace::new(id));
        apply_trace_settings(&mut t, p)?;
        new_traces.push(t);
    }
    for id in set {
        let mut t = existing(id).ok_or_else(|| bad(CMD, "that level is not traced yet; use `add`"))?;
        apply_trace_settings(&mut t, p)?;
        new_traces.push(t);
    }
    // Apply to a scratch copy first, so a refused change (the cap, a bad level) leaves the
    // drawing exactly as it was.
    let mut scratch = s.doc()?.clone();
    for id in remove {
        scratch.remove_trace(on, id);
    }
    for t in new_traces {
        scratch.set_trace(on, t)?;
    }
    let result = scratch.level(on).map(|l| l.traces.clone()).unwrap_or_default();
    let d = s.doc_mut()?;
    if let Some(l) = d.levels.iter_mut().find(|l| l.id == on) {
        l.traces = result;
    }
    let traces: Vec<Value> = d.level(on).map(|l| l.traces.iter().map(|t| trace_json(d, t)).collect()).unwrap_or_default();
    Ok(json!({ "level": d.level(on).map(|l| l.name.clone()), "traces": traces }))
}

/// What a layout viewport shows, as given to `viewport.set` / `viewport.add`. An outer `None` is
/// "not given"; `Some(None)` clears the binding.
#[derive(Default)]
pub(super) struct Binding {
    pub level: Option<Option<LevelId>>,
    pub sheet_set: Option<Option<aad_doc::SheetSetId>>,
    pub traces: Option<Vec<Trace>>,
}

impl Binding {
    pub fn is_given(&self) -> bool {
        self.level.is_some() || self.sheet_set.is_some() || self.traces.is_some()
    }
}

/// Read `level`, `sheetSet` and `traces` (a list of `{level, fade?, colour?, order?, snap?, layers?}`,
/// or null to clear) from `p`, resolving names. Setting a binding needs explicit levels.
pub(super) fn parse_binding(s: &Session, cmd: &str, p: &Value) -> Result<Binding> {
    let mut b = Binding::default();
    let given = |k: &str| p.get(k).is_some();
    if (given("level") || given("sheetSet") || given("traces")) && !s.doc()?.has_levels() {
        let clearing = ["level", "sheetSet", "traces"].iter().all(|k| p.get(*k).is_none_or(Value::is_null));
        if !clearing {
            return Err(bad(cmd, "the drawing has one level; add another with level.new before binding a viewport"));
        }
    }
    match p.get("level") {
        None => {}
        Some(Value::Null) => b.level = Some(None),
        Some(v) => b.level = Some(Some(find(s, cmd, &json!({ "level": v }), "level")?)),
    }
    match p.get("sheetSet") {
        None => {}
        Some(Value::Null) => b.sheet_set = Some(None),
        Some(v) => b.sheet_set = Some(Some(super::sheetset::find(s, cmd, v)?)),
    }
    match p.get("traces") {
        None => {}
        Some(Value::Null) => b.traces = Some(Vec::new()),
        Some(Value::Array(a)) if a.len() <= aad_doc::MAX_TRACES => {
            let mut out = Vec::new();
            for item in a {
                let lv = item.get("level").ok_or_else(|| bad(cmd, "each trace needs a `level`"))?;
                let id = find(s, cmd, &json!({ "level": lv }), "level")?;
                if out.iter().any(|t: &Trace| t.level == id) {
                    return Err(bad(cmd, "a level is traced once per viewport"));
                }
                let mut t = Trace::new(id);
                apply_trace_settings(&mut t, item)?;
                out.push(t);
            }
            b.traces = Some(out);
        }
        Some(_) => return Err(bad(cmd, format!("`traces` is a list of at most {} traces, or null", aad_doc::MAX_TRACES))),
    }
    Ok(b)
}

/// The level, sheet set and traces of a viewport for the JSON listings.
pub(super) fn viewport_binding_json(v: &aad_doc::Viewport) -> Value {
    json!({
        "level": v.level.map(|l| l.0),
        "sheetSet": v.sheet_set.map(|x| x.0),
        "traces": v.traces.iter().map(|t| json!({
            "level": t.level.0, "fade": t.fade,
            "colour": if t.colour == TraceColour::Grey { "grey" } else { "true" },
            "order": if t.order == TraceOrder::Below { "below" } else { "above" },
            "snap": t.snap, "layers": t.layers,
        })).collect::<Vec<_>>(),
    })
}

//! Sections and elevations: view spaces tied to markers on a level (design:
//! `docs/levels-and-sheet-sets.md`, "Sections and elevations").
//!
//! A view space is a hand-drafted 2D store (X along the marker line, Y = height in drawing units).
//! `section.new` / `elevation.new` draw the existing loose marker (`section.marker`,
//! `elevation.marker`) and link the view to the marker's handle; `section.link` adopts a marker
//! drawn earlier. Projection guides come from `crate::sectionguide`; `section.draft` turns them
//! into ordinary objects on demand and `section.check` reports where the plan and the drafted
//! lines no longer agree. Nothing here changes the plan.

use aad_doc::{Drawing, EntityKind, Handle, LevelId, SpaceId, ViewKind, ViewLook, ViewRecord};
use aad_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::hcw::{add_on, ensure_layer};
use super::helpers::{line, lwpoly};
use super::*;
use crate::sectionguide::{self, GuideKind};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Layers the drafted view objects go on (they exist in the layer map).
pub const SECT_LAYER: &str = "A-SECT";
pub const ELEV_LAYER: &str = "A-ELEV";
/// Most guide lines listed in a JSON result.
const LIST_LIMIT: usize = 2_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("section.new", "New Section", run_section_new)
            .menu(&["Architecture", "Levels and Sections", "New Section View"])
            .params("{p1: [x,y], p2: [x,y], look?: left|right, depth? (drawing units, default 15 m), label?: \"A\", name?, level? (default the active level)} → {view, name, handle}. Draws the section marker on the level and creates its view space (it does not switch to it)")
            .interactive(|_| Ok(Box::new(SectionM::default()))),
        CommandSpec::new("elevation.new", "New Elevation", run_elevation_new)
            .menu(&["Architecture", "Levels and Sections", "New Elevation View"])
            .params("{at: [x,y], angle?: degrees (the way you look, default 90), depth?, width? (how much is shown across, default 20 m), name?, number?, level?} → {view, name, handle}")
            .interactive(|_| Ok(Box::new(ElevationM::default()))),
        CommandSpec::new("section.link", "Link Marker to View", run_link)
            .menu(&["Architecture", "Levels and Sections", "Link Marker"])
            .params("{handle: marker line, look?: left|right, depth?, name?, level?} → {view, name}. Adopts a section line that was drawn without a view"),
        CommandSpec::new("view.list", "List Views", run_list)
            .menu(&["Architecture", "Levels and Sections", "List Views"])
            .params("{} → {views: [{id, kind, name, level, active, objects, depth, look, marker, covers, traces}]}")
            .noundo(),
        CommandSpec::new("view.edit", "View Settings", run_set)
            .menu(&["Architecture", "Levels and Sections", "View Settings"])
            .params("{view (id or name), name?, depth?, look?, level?, covers? [levels] | null (all)} → {view}"),
        CommandSpec::new("view.delete", "Delete View", run_delete)
            .menu(&["Architecture", "Levels and Sections", "Delete View"])
            .params("{view, force? (delete although it holds drafted objects)} → {deleted}. The marker stays on the plan as plain linework"),
        CommandSpec::new("view.activate", "Open View", run_activate)
            .menu(&["Architecture", "Levels and Sections", "Open View"])
            .alias(&["viewactivate"])
            .params("{view} → {active, objects, note?}. Makes the section or elevation the space drafting commands edit; follows its marker first"),
        CommandSpec::new("view.trace", "Trace Views", run_trace)
            .menu(&["Architecture", "Levels and Sections", "Trace Views"])
            .params("{view?: default the active view, add?: view | [views], remove?: view | [views]} → {view, traces}. Another section or elevation shown faded under this one (levels are not offered: plan and section coordinates differ; the guides show them)"),
        CommandSpec::new("view.guides", "View Guides", run_guides)
            .menu(&["Architecture", "Levels and Sections", "List Guides"])
            .params("{view?: default the active view, limit?} → {count, byKind, truncated, lines: [{kind, level, a, b, label?}]}. The read-only projection guides; never saved")
            .noundo(),
        CommandSpec::new("section.draft", "Draft Section from Guides", run_draft)
            .menu(&["Architecture", "Levels and Sections", "Draft from Guides"])
            .params("{view?, slabThickness? (drawing units; adds a slab band under each floor line)} → {added, skipped}. Turns the current guides into ordinary objects in the view; running again adds only what is new and never moves drafted objects"),
        CommandSpec::new("section.check", "Check Section", run_check)
            .menu(&["Architecture", "Levels and Sections", "Check View"])
            .params("{view?, tolerance? (drawing units, default 1 mm)} → {checked, matched, gaps: [{kind, level, at, nearest?, gap?}]}. Lists guides with no drafted line on them (a wall moved since drafting); changes nothing")
            .noundo(),
        CommandSpec::new("section.levelmarks", "Level Marks on View", run_levelmarks)
            .menu(&["Architecture", "Levels and Sections", "Level Marks on View"])
            .params("{view?} → {placed}. A level mark at each floor line, with the value in metres"),
    ]
}

// ---------------------------------------------------------------------------------------------
// helpers

fn mpu(d: &Drawing) -> f64 {
    crate::annot::drawing_unit(d).map_or(0.0254, |u| u.metres()).max(1e-9)
}

fn num(p: &Value, cmd: &str, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
    }
}

fn look_param(p: &Value, cmd: &str) -> Result<Option<ViewLook>> {
    match str_param(p, "look").or_else(|| str_param(p, "side")).map(str::to_ascii_lowercase).as_deref() {
        None => Ok(None),
        Some("left") => Ok(Some(ViewLook::Left)),
        Some("right") => Ok(Some(ViewLook::Right)),
        Some(_) => Err(bad(cmd, "`look` must be left or right")),
    }
}

fn ids_of(p: &Value, key: &str) -> Vec<Value> {
    match p.get(key) {
        Some(Value::Array(a)) => a.iter().take(64).cloned().collect(),
        Some(v) if !v.is_null() => vec![v.clone()],
        _ => Vec::new(),
    }
}

fn view_of(d: &Drawing, v: &Value) -> Option<SpaceId> {
    if let Some(n) = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
        let id = SpaceId(u32::try_from(n).unwrap_or(u32::MAX));
        if d.view_space(id).is_some() {
            return Some(id);
        }
    }
    v.as_str().and_then(|n| d.view_space_by_name(n)).map(|v| v.rec.id)
}

/// A view named by id or name; with no `view` key, the view being edited.
fn find(s: &Session, cmd: &str, p: &Value, key: &str) -> Result<SpaceId> {
    let d = s.doc()?;
    match p.get(key).filter(|v| !v.is_null()) {
        Some(v) => view_of(d, v).ok_or_else(|| {
            bad(cmd, format!("no view `{v}`; the views are {}", d.view_spaces.iter().map(|v| v.rec.name.as_str()).collect::<Vec<_>>().join(", ")))
        }),
        None => d.active_view.ok_or_else(|| bad(cmd, format!("`{key}` (a view id or name) is required when no view is open"))),
    }
}

fn level_of(s: &Session, cmd: &str, p: &Value) -> Result<LevelId> {
    if p.get("level").is_some_and(|v| !v.is_null()) {
        return super::levels::find(s, cmd, p, "level");
    }
    s.doc()?.active_level.ok_or_else(|| bad(cmd, "the drawing has no levels"))
}

/// A drawing with one implicit level gets it made explicit first (the same rule as `level.new`).
fn ensure_levels(s: &mut Session) -> Result<()> {
    if !s.doc()?.has_levels() {
        let name = super::levels::implicit_name(s.doc()?);
        s.doc_mut()?.materialize_levels(&name, 0.0, 0.0)?;
    }
    Ok(())
}

fn view_json(d: &Drawing, v: &aad_doc::ViewSpace) -> Value {
    let r = &v.rec;
    json!({
        "id": r.id.0, "kind": r.kind.label(), "name": r.name, "level": r.level.0,
        "levelName": d.level(r.level).map(|l| l.name.clone()),
        "active": d.active_view == Some(r.id),
        "objects": d.view_store(r.id).map_or(0, aad_doc::EntityStore::len),
        "depth": r.depth, "look": if r.look == ViewLook::Left { "left" } else { "right" },
        "marker": r.marker.map(|h| h.hex()),
        "line": [[r.line.0.x, r.line.0.y], [r.line.1.x, r.line.1.y]],
        "covers": r.covers.as_ref().map(|c| c.iter().map(|l| l.0).collect::<Vec<_>>()),
        "traces": r.traces.iter().map(|t| t.0).collect::<Vec<_>>(),
    })
}

fn default_name(d: &Drawing, kind: ViewKind, label: &str) -> String {
    let base = match kind {
        ViewKind::Section => format!("Section {label}-{label}"),
        ViewKind::Elevation => format!("Elevation {label}"),
    };
    if d.view_space_by_name(&base).is_none() {
        return base;
    }
    for n in 2..200 {
        let c = format!("{base} ({n})");
        if d.view_space_by_name(&c).is_none() {
            return c;
        }
    }
    base
}

/// The first single capital letter not used by a section or elevation name yet.
fn next_label(d: &Drawing, kind: ViewKind) -> String {
    for c in 'A'..='Z' {
        let name = default_name_plain(kind, &c.to_string());
        if d.view_space_by_name(&name).is_none() {
            return c.to_string();
        }
    }
    "A".into()
}

fn default_name_plain(kind: ViewKind, label: &str) -> String {
    match kind {
        ViewKind::Section => format!("Section {label}-{label}"),
        ViewKind::Elevation => format!("Elevation {label}"),
    }
}

fn show_model_tab(s: &mut Session) -> Result<()> {
    let st = s.state_mut()?;
    if matches!(st.space, aad_doc::Space::Paper(_)) {
        st.space = aad_doc::Space::Model;
        st.mspace = None;
        st.selection.clear();
    }
    Ok(())
}

/// Run `f` with `id` as the edit target, then restore the previous target.
fn in_view<T>(s: &mut Session, id: SpaceId, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
    let prev = s.doc()?.target();
    s.doc_mut()?.enter_view_space(id)?;
    let r = f(s);
    if let Some(p) = prev {
        s.doc_mut()?.switch_target(p)?;
    }
    r
}

/// Follow the marker on the plan: a moved section line moves the view's line; a deleted marker
/// leaves the view with none. Returns a note when something changed.
pub(crate) fn sync_marker(d: &mut Drawing, id: SpaceId) -> Option<String> {
    let rec = d.view_space(id)?.rec.clone();
    let h = rec.marker?;
    let mut next = rec.clone();
    let mut note = None;
    match d.entity(h).map(|e| e.kind.clone()) {
        Some(EntityKind::Line(l)) => {
            let line = (l.a.xy(), l.b.xy());
            if line != rec.line {
                next.line = line;
                note = Some("the view follows its marker");
            }
        }
        Some(EntityKind::Circle(c)) => {
            let centre = c.center.xy();
            let mid = rec.line.0.lerp(rec.line.1, 0.5);
            if !centre.near(mid, 1e-9) {
                let delta = centre - mid;
                next.line = (rec.line.0 + delta, rec.line.1 + delta);
                note = Some("the view follows its marker");
            }
        }
        Some(_) => {}
        None => {
            next.marker = None;
            note = Some("the marker was deleted; the view keeps its last position");
        }
    }
    if note.is_some() && d.update_view_record(next).is_err() {
        return None;
    }
    note.map(str::to_string)
}

fn create(
    s: &mut Session,
    cmd: &str,
    kind: ViewKind,
    name: Option<&str>,
    label: &str,
    level: LevelId,
    line_pts: (Vec2, Vec2),
    look: ViewLook,
    depth: f64,
    marker: Option<Handle>,
) -> Result<SpaceId> {
    let name = match name {
        Some(n) => n.to_string(),
        None => default_name(s.doc()?, kind, label),
    };
    s.doc_mut()?.add_view_space(kind, &name, level, line_pts, look, depth, marker).map_err(|e| bad(cmd, e.to_string()))
}

fn depth_param(s: &Session, cmd: &str, p: &Value) -> Result<f64> {
    let default = 15.0 / mpu(s.doc()?);
    let d = num(p, cmd, "depth")?.unwrap_or(default);
    if !(d > 0.0 && d <= aad_doc::VIEW_COORD_MAX) {
        return Err(bad(cmd, "`depth` must be a positive number"));
    }
    Ok(d)
}

// ---------------------------------------------------------------------------------------------
// creation

fn make_section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section.new";
    let (a, b) = (point_req(cmd, p, "p1")?, point_req(cmd, p, "p2")?);
    let look = look_param(p, cmd)?.unwrap_or(ViewLook::Left);
    let depth = depth_param(s, cmd, p)?;
    if s.block_edit.is_some() {
        return Err(bad(cmd, "close the block editor first"));
    }
    // Check everything the view needs before the marker is drawn, so a refusal leaves no marker.
    let probe = aad_doc::ViewRecord {
        id: SpaceId(1),
        kind: ViewKind::Section,
        name: "x".into(),
        level: LevelId(1),
        marker: None,
        line: (a, b),
        look,
        depth,
        covers: None,
        traces: Vec::new(),
        view: None,
    };
    if !probe.is_valid() {
        return Err(bad(cmd, "the two points must differ and be finite"));
    }
    ensure_levels(s)?;
    let level = level_of(s, cmd, p)?;
    let label = match str_param(p, "label") {
        Some(l) => l.to_string(),
        None => next_label(s.doc()?, ViewKind::Section),
    };
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| default_name_plain(ViewKind::Section, &label));
    if s.doc()?.view_space_by_name(&name).is_some() {
        return Err(bad(cmd, format!("there is already a view named `{name}`")));
    }
    // The marker goes on the level it belongs to.
    let on = s.doc()?.active_level;
    if on != Some(level) {
        return Err(bad(cmd, "place the marker while the level is active (`level.activate`)"));
    }
    let r = super::symbols::run_section(
        s,
        &json!({ "p1": [a.x, a.y], "p2": [b.x, b.y], "label": label, "side": if look == ViewLook::Left { "left" } else { "right" } }),
    )?;
    let marker = r.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex);
    let id = create(s, cmd, ViewKind::Section, Some(&name), &label, level, (a, b), look, depth, marker)?;
    Ok(json!({ "view": id.0, "name": name, "handle": marker.map(|h| h.hex()) }))
}

fn run_section_new(s: &mut Session, p: &Value) -> Result<Value> {
    make_section(s, p)
}

fn elevation_line(at: Vec2, angle_deg: f64, width: f64) -> (Vec2, Vec2) {
    let a = angle_deg.to_radians();
    let d = Vec2::new(a.cos(), a.sin());
    // The viewer's right-hand axis, so X grows to the right as seen.
    let u = Vec2::new(d.y, -d.x);
    (at - u * (width / 2.0), at + u * (width / 2.0))
}

fn make_elevation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "elevation.new";
    let at = point_req(cmd, p, "at")?;
    let angle = num(p, cmd, "angle")?.unwrap_or(90.0);
    let depth = depth_param(s, cmd, p)?;
    let width = num(p, cmd, "width")?.unwrap_or(20.0 / mpu(s.doc()?));
    if !(width > 0.0 && width <= aad_doc::VIEW_COORD_MAX) || angle.abs() > 1.0e7 {
        return Err(bad(cmd, "`width` must be positive and `angle` a sane number of degrees"));
    }
    if s.block_edit.is_some() {
        return Err(bad(cmd, "close the block editor first"));
    }
    if !at.is_finite() || at.x.abs() > aad_doc::VIEW_COORD_MAX || at.y.abs() > aad_doc::VIEW_COORD_MAX {
        return Err(bad(cmd, "`at` is not a usable point"));
    }
    ensure_levels(s)?;
    let level = level_of(s, cmd, p)?;
    let label = match str_param(p, "number") {
        Some(l) => l.to_string(),
        None => next_label(s.doc()?, ViewKind::Elevation),
    };
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| default_name_plain(ViewKind::Elevation, &label));
    if s.doc()?.view_space_by_name(&name).is_some() {
        return Err(bad(cmd, format!("there is already a view named `{name}`")));
    }
    if s.doc()?.active_level != Some(level) {
        return Err(bad(cmd, "place the marker while the level is active (`level.activate`)"));
    }
    let r = super::symbols::run_elevation(s, &json!({ "at": [at.x, at.y], "number": label, "sheet": "-", "angle": angle }))?;
    let marker = r.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex);
    let id = create(s, cmd, ViewKind::Elevation, Some(&name), &label, level, elevation_line(at, angle, width), ViewLook::Left, depth, marker)?;
    Ok(json!({ "view": id.0, "name": name, "handle": marker.map(|h| h.hex()) }))
}

fn run_elevation_new(s: &mut Session, p: &Value) -> Result<Value> {
    make_elevation(s, p)
}

fn run_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section.link";
    let h = str_param(p, "handle").and_then(Handle::parse_hex).ok_or_else(|| bad(cmd, "`handle` (the marker line) is required"))?;
    let (a, b) = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Line(l)) => (l.a.xy(), l.b.xy()),
        Some(_) => return Err(bad(cmd, "the marker must be a line")),
        None => return Err(bad(cmd, "no such object")),
    };
    if s.doc()?.view_spaces.iter().any(|v| v.rec.marker == Some(h)) {
        return Err(bad(cmd, "that line is already the marker of a view"));
    }
    let look = look_param(p, cmd)?.unwrap_or(ViewLook::Left);
    let depth = depth_param(s, cmd, p)?;
    ensure_levels(s)?;
    let level = level_of(s, cmd, p)?;
    let label = next_label(s.doc()?, ViewKind::Section);
    let id = create(s, cmd, ViewKind::Section, str_param(p, "name"), &label, level, (a, b), look, depth, Some(h))?;
    let name = s.doc()?.view_space(id).map(|v| v.rec.name.clone()).unwrap_or_default();
    Ok(json!({ "view": id.0, "name": name }))
}

// ---------------------------------------------------------------------------------------------
// management

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    Ok(json!({ "views": d.view_spaces.iter().map(|v| view_json(d, v)).collect::<Vec<_>>() }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.edit";
    let id = find(s, cmd, p, "view")?;
    let mut rec: ViewRecord = s.doc()?.view_space(id).map(|v| v.rec.clone()).ok_or_else(|| bad(cmd, "no such view"))?;
    if let Some(n) = str_param(p, "name") {
        rec.name = n.to_string();
    }
    if let Some(dp) = num(p, cmd, "depth")? {
        rec.depth = dp;
    }
    if let Some(l) = look_param(p, cmd)? {
        rec.look = l;
    }
    if p.get("level").is_some_and(|v| !v.is_null()) {
        rec.level = super::levels::find(s, cmd, p, "level")?;
    }
    if p.get("covers").is_some() {
        let list = ids_of(p, "covers");
        rec.covers = if p["covers"].is_null() {
            None
        } else {
            let mut v = Vec::new();
            for item in &list {
                let tmp = json!({ "level": item });
                v.push(super::levels::find(s, cmd, &tmp, "level")?);
            }
            Some(v)
        };
    }
    s.doc_mut()?.update_view_record(rec).map_err(|e| bad(cmd, e.to_string()))?;
    let d = s.doc()?;
    Ok(json!({ "view": d.view_space(id).map(|v| view_json(d, v)) }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.delete";
    let id = find(s, cmd, p, "view")?;
    if s.block_edit.is_some() {
        return Err(bad(cmd, "close the block editor first"));
    }
    let d = s.doc()?;
    let objects = d.view_store(id).map_or(0, aad_doc::EntityStore::len);
    if objects > 0 && !bool_or(p, "force", false) {
        return Err(bad(cmd, format!("the view holds {objects} drafted objects; pass `force` to delete them")));
    }
    let name = d.view_space(id).map(|v| v.rec.name.clone()).unwrap_or_default();
    let was_active = d.active_view == Some(id);
    s.doc_mut()?.remove_view_space(id).map_err(|e| bad(cmd, e.to_string()))?;
    if was_active {
        s.set_selection(Vec::new());
    }
    Ok(json!({ "deleted": name }))
}

fn run_activate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.activate";
    let id = find(s, cmd, p, "view")?;
    if s.block_edit.is_some() {
        return Err(bad(cmd, "close the block editor first"));
    }
    show_model_tab(s)?;
    let note = sync_marker(s.doc_mut()?, id);
    if s.doc()?.active_view != Some(id) {
        s.doc_mut()?.enter_view_space(id).map_err(|e| bad(cmd, e.to_string()))?;
        s.set_selection(Vec::new());
    }
    let d = s.doc()?;
    Ok(json!({ "active": d.active_view_ref().map(|v| v.rec.name.clone()), "objects": d.model.len(), "note": note }))
}

fn run_trace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.trace";
    let id = find(s, cmd, p, "view")?;
    let mut rec = s.doc()?.view_space(id).map(|v| v.rec.clone()).ok_or_else(|| bad(cmd, "no such view"))?;
    let d = s.doc()?;
    for item in ids_of(p, "remove") {
        let t = view_of(d, &item).ok_or_else(|| bad(cmd, format!("no view `{item}` to remove")))?;
        rec.traces.retain(|x| *x != t);
    }
    for item in ids_of(p, "add") {
        let t = view_of(d, &item).ok_or_else(|| bad(cmd, format!("no view `{item}` to trace")))?;
        if t == id {
            return Err(bad(cmd, "a view cannot trace itself"));
        }
        if !rec.traces.contains(&t) {
            rec.traces.push(t);
        }
    }
    s.doc_mut()?.update_view_record(rec).map_err(|e| bad(cmd, e.to_string()))?;
    let traces: Vec<u32> = s.doc()?.view_space(id).map(|v| v.rec.traces.iter().map(|t| t.0).collect()).unwrap_or_default();
    Ok(json!({ "view": id.0, "traces": traces }))
}

// ---------------------------------------------------------------------------------------------
// guides, draft, check

/// The record of a view after following its marker (read-only copy).
fn fresh_record(s: &Session, id: SpaceId) -> Result<ViewRecord> {
    let d = s.doc()?;
    let mut scratch = d.clone();
    sync_marker(&mut scratch, id);
    scratch.view_space(id).map(|v| v.rec.clone()).ok_or_else(|| bad("view", "no such view"))
}

fn run_guides(s: &mut Session, p: &Value) -> Result<Value> {
    let id = find(s, "view.guides", p, "view")?;
    let rec = fresh_record(s, id)?;
    let g = sectionguide::compute(s.doc()?, &rec);
    let limit = p.get("limit").and_then(Value::as_u64).map_or(LIST_LIMIT, |n| (n as usize).min(LIST_LIMIT));
    let mut by_kind = serde_json::Map::new();
    for l in &g.lines {
        let n = by_kind.entry(l.kind.label().to_string()).or_insert(json!(0));
        *n = json!(n.as_u64().unwrap_or(0) + 1);
    }
    let lines: Vec<Value> = g
        .lines
        .iter()
        .take(limit)
        .map(|l| json!({ "kind": l.kind.label(), "level": l.level.map(|x| x.0), "a": [l.a.x, l.a.y], "b": [l.b.x, l.b.y], "label": l.label }))
        .collect();
    Ok(json!({ "count": g.lines.len(), "byKind": by_kind, "truncated": g.truncated, "segmentsCapped": g.segments_capped, "lines": lines }))
}

fn layer_for(kind: ViewKind) -> (&'static str, &'static str) {
    match kind {
        ViewKind::Section => (SECT_LAYER, "Section linework and cut outlines"),
        ViewKind::Elevation => (ELEV_LAYER, "Elevation linework and facade outlines"),
    }
}

/// Straight edges of a view store (lines and polylines), for matching.
fn edges(d: &Drawing, id: SpaceId, cap: usize) -> Vec<(Vec2, Vec2)> {
    let mut out = Vec::new();
    let Some(store) = d.view_store(id) else { return out };
    for e in store.iter() {
        match &e.kind {
            EntityKind::Line(l) => out.push((l.a.xy(), l.b.xy())),
            EntityKind::LwPolyline(pl) => {
                let n = pl.vertices.len();
                for i in 0..n {
                    if i + 1 == n && !pl.closed {
                        break;
                    }
                    if let (Some(a), Some(b)) = (pl.vertices.get(i), pl.vertices.get((i + 1) % n)) {
                        out.push((a.p, b.p));
                    }
                }
            }
            _ => {}
        }
        if out.len() >= cap {
            break;
        }
    }
    out
}

fn same_line(a: (Vec2, Vec2), b: (Vec2, Vec2), tol: f64) -> bool {
    (a.0.near(b.0, tol) && a.1.near(b.1, tol)) || (a.0.near(b.1, tol) && a.1.near(b.0, tol))
}

fn run_draft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section.draft";
    let id = find(s, cmd, p, "view")?;
    let slab = num(p, cmd, "slabThickness")?.unwrap_or(0.0);
    if slab < 0.0 || slab > aad_doc::VIEW_COORD_MAX {
        return Err(bad(cmd, "`slabThickness` cannot be negative"));
    }
    let rec = fresh_record(s, id)?;
    let g = sectionguide::compute(s.doc()?, &rec);
    let tol = 0.001 / mpu(s.doc()?);
    let thin = 0.6 / mpu(s.doc()?);
    let existing = edges(s.doc()?, id, 500_000);
    let (layer, desc) = layer_for(rec.kind);

    // Shapes to add: (is closed outline, points).
    let mut shapes: Vec<Vec<Vec2>> = Vec::new();
    // Wall and column faces on one level pair up into outlines when they are a wall thickness apart.
    for kind in [GuideKind::Wall, GuideKind::Column] {
        let mut by_level: std::collections::BTreeMap<u32, Vec<&sectionguide::Guide>> = std::collections::BTreeMap::new();
        for l in g.lines.iter().filter(|l| l.kind == kind) {
            by_level.entry(l.level.map_or(0, |x| x.0)).or_default().push(l);
        }
        for (_, mut v) in by_level {
            v.sort_by(|a, b| a.a.x.total_cmp(&b.a.x).then(a.a.y.total_cmp(&b.a.y)));
            let mut used = vec![false; v.len()];
            for i in 0..v.len() {
                if used[i] {
                    continue;
                }
                let Some(a) = v.get(i) else { continue };
                let mate = (i + 1..v.len().min(i + 8)).find(|j| {
                    v.get(*j).is_some_and(|b| {
                        !used[*j] && (b.a.x - a.a.x) > tol && (b.a.x - a.a.x) <= thin && (b.a.y - a.a.y).abs() < tol && (b.b.y - a.b.y).abs() < tol
                    })
                });
                match mate.and_then(|j| v.get(j).map(|b| (j, *b))) {
                    Some((j, b)) => {
                        used[j] = true;
                        shapes.push(vec![a.a, b.a, b.b, a.b]);
                    }
                    None => shapes.push(vec![a.a, a.b]),
                }
                used[i] = true;
            }
        }
    }
    for l in g.lines.iter().filter(|l| !matches!(l.kind, GuideKind::Wall | GuideKind::Column)) {
        shapes.push(vec![l.a, l.b]);
        if slab > 0.0 && l.kind == GuideKind::Floor {
            shapes.push(vec![l.a, l.b, Vec2::new(l.b.x, l.b.y - slab), Vec2::new(l.a.x, l.a.y - slab)]);
        }
    }
    // What is already drafted is skipped (a closed outline matches when all its edges exist).
    let is_new = |shape: &Vec<Vec2>| -> bool {
        let n = shape.len();
        let count = if n == 2 { 1 } else { n };
        (0..count).any(|i| match (shape.get(i), shape.get((i + 1) % n)) {
            (Some(a), Some(b)) => !existing.iter().any(|e| same_line(*e, (*a, *b), tol)),
            _ => false,
        })
    };
    let todo: Vec<&Vec<Vec2>> = shapes.iter().filter(|sh| is_new(sh)).collect();
    let skipped = shapes.len() - todo.len();
    let mut added = 0usize;
    in_view(s, id, |s| {
        ensure_layer(s.doc_mut()?, layer, 30, "Continuous", 35, desc);
        for sh in &todo {
            let kind = if sh.len() == 2 {
                match (sh.first(), sh.get(1)) {
                    (Some(a), Some(b)) => line(*a, *b),
                    _ => continue,
                }
            } else {
                lwpoly(sh.iter().map(|p| PolyVertex::new(*p)).collect(), true)
            };
            add_on(s, layer, kind)?;
            added += 1;
        }
        Ok(())
    })?;
    Ok(json!({ "added": added, "skipped": skipped, "truncated": g.truncated }))
}

fn run_check(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section.check";
    let id = find(s, cmd, p, "view")?;
    let rec = fresh_record(s, id)?;
    let tol = num(p, cmd, "tolerance")?.unwrap_or(0.001 / mpu(s.doc()?));
    if !(0.0..1.0e9).contains(&tol) {
        return Err(bad(cmd, "`tolerance` cannot be negative"));
    }
    let g = sectionguide::compute(s.doc()?, &rec);
    let drafted = edges(s.doc()?, id, 500_000);
    // Vertical edges sorted by X, horizontal by Y.
    let mut vert: Vec<(f64, f64, f64)> = Vec::new();
    let mut horiz: Vec<(f64, f64, f64)> = Vec::new();
    for (a, b) in &drafted {
        if (a.x - b.x).abs() <= tol && (a.y - b.y).abs() > tol {
            vert.push(((a.x + b.x) / 2.0, a.y.min(b.y), a.y.max(b.y)));
        } else if (a.y - b.y).abs() <= tol && (a.x - b.x).abs() > tol {
            horiz.push(((a.y + b.y) / 2.0, a.x.min(b.x), a.x.max(b.x)));
        }
    }
    vert.sort_by(|a, b| a.0.total_cmp(&b.0));
    horiz.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Nearest edge position (with overlapping span) to `pos`; `exact` when within `tol`.
    let nearest = |sorted: &[(f64, f64, f64)], pos: f64, lo: f64, hi: f64| -> Option<f64> {
        let at = sorted.partition_point(|e| e.0 < pos);
        let mut best: Option<f64> = None;
        let mut probe = |e: &(f64, f64, f64)| {
            if e.2 >= lo - tol && e.1 <= hi + tol {
                let gap = (e.0 - pos).abs();
                if best.is_none_or(|b| gap < b) {
                    best = Some(gap);
                }
            }
        };
        for e in sorted.get(at.saturating_sub(64)..at).into_iter().flatten().rev() {
            probe(e);
        }
        for e in sorted.get(at..(at + 64).min(sorted.len())).into_iter().flatten() {
            probe(e);
        }
        best
    };
    let mut gaps = Vec::new();
    let mut matched = 0usize;
    let mut checked = 0usize;
    // A guide set of this size is already beyond what a report is for.
    for l in g.lines.iter().take(20_000) {
        // Floor and top lines are datums, not drafted linework to compare.
        if matches!(l.kind, GuideKind::Floor | GuideKind::Top) {
            continue;
        }
        checked += 1;
        let vertical = (l.a.x - l.b.x).abs() <= tol;
        let (best, at) = if vertical {
            (nearest(&vert, l.a.x, l.a.y.min(l.b.y), l.a.y.max(l.b.y)), json!({ "x": l.a.x }))
        } else {
            (nearest(&horiz, l.a.y, l.a.x.min(l.b.x), l.a.x.max(l.b.x)), json!({ "y": l.a.y }))
        };
        match best {
            Some(gap) if gap <= tol => matched += 1,
            other => gaps.push(json!({ "kind": l.kind.label(), "level": l.level.map(|x| x.0), "at": at, "nearest": other.is_some(), "gap": other })),
        }
        if gaps.len() >= 1000 {
            break;
        }
    }
    Ok(json!({ "view": id.0, "checked": checked, "matched": matched, "gaps": gaps, "drafted": drafted.len() }))
}

fn run_levelmarks(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section.levelmarks";
    let id = find(s, cmd, p, "view")?;
    let rec = fresh_record(s, id)?;
    let g = sectionguide::compute(s.doc()?, &rec);
    let m = mpu(s.doc()?);
    let len = rec.length();
    let marks: Vec<(f64, String)> =
        g.lines.iter().filter(|l| l.kind == GuideKind::Floor).map(|l| (l.a.y, l.label.clone().unwrap_or_default())).collect();
    let mut placed = 0usize;
    in_view(s, id, |s| {
        for (y, _name) in &marks {
            super::symbols::run_level(s, &json!({ "at": [len + 0.5 / m, *y], "value": *y * m }))?;
            placed += 1;
        }
        Ok(())
    })?;
    Ok(json!({ "placed": placed }))
}

// ---------------------------------------------------------------------------------------------
// prompts

#[derive(Default)]
struct SectionM {
    p1: Option<Vec2>,
    p2: Option<Vec2>,
}

impl Interactive for SectionM {
    fn name(&self) -> &'static str {
        "SECTIONVIEW"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.p1, self.p2) {
            (None, _) => Prompt::new("Specify the first point of the section line", Accept::POINT),
            (Some(a), None) => Prompt::new("Specify the second point of the section line", Accept::POINT).base(a),
            (Some(_), Some(_)) => Prompt::new("Which way does the section look", Accept::TEXT).kw(&["Left", "Right"]).default("Left"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.p1, self.p2, i) {
            (None, _, Input::Point(p)) => self.p1 = Some(p),
            (Some(_), None, Input::Point(p)) => self.p2 = Some(p),
            (Some(a), Some(b), Input::Keyword(k) | Input::Text(k)) => return self.finish(s, a, b, &k),
            (Some(a), Some(b), Input::Enter) => return self.finish(s, a, b, "left"),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.p1.map(|a| vec![line(a, self.p2.unwrap_or(c))]).unwrap_or_default()
    }
}

impl SectionM {
    fn finish(&self, s: &mut Session, a: Vec2, b: Vec2, look: &str) -> Result<Step> {
        let look = if look.trim().to_ascii_lowercase().starts_with('r') { "right" } else { "left" };
        let r = make_section(s, &json!({ "p1": [a.x, a.y], "p2": [b.x, b.y], "look": look }))?;
        s.echo(format!("{} created; open it with VIEW.ACTIVATE or its tab", r["name"].as_str().unwrap_or("Section")));
        Ok(Step::Done)
    }
}

#[derive(Default)]
struct ElevationM {
    at: Option<Vec2>,
}

impl Interactive for ElevationM {
    fn name(&self) -> &'static str {
        "ELEVATIONVIEW"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.at {
            None => Prompt::new("Specify the elevation marker position", Accept::POINT),
            Some(a) => Prompt::new("Specify the direction you look", Accept::POINT).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.at, i) {
            (None, Input::Point(p)) => self.at = Some(p),
            (Some(a), Input::Point(b)) => {
                let angle = (b - a).y.atan2((b - a).x).to_degrees();
                if (b - a).len() < 1e-9 {
                    return Err(bad("elevation.new", "pick a point away from the marker to give the direction"));
                }
                let r = make_elevation(s, &json!({ "at": [a.x, a.y], "angle": angle }))?;
                s.echo(format!("{} created; open it with VIEW.ACTIVATE or its tab", r["name"].as_str().unwrap_or("Elevation")));
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
}

//! `takeoff.record` for several levels in one run, and the per-level status in the take-off header.
//!
//! A take-off measures the geometry of the **active level**, so recording more than one level means measuring each
//! of them. `levels: "all"` (or a list of level names or ids) does that on a scratch copy of the drawing: for every
//! requested level other than the active one the copy activates the level, runs again the measuring tools whose
//! results the drawing already holds (default parameters, no tables), and collects the bridge rows with that level as
//! the stamped level. The open drawing is not touched (no labels, no level switch). The active level keeps the saved
//! results it has. Every row carries the level's `level_id`.
//!
//! The header gets one more optional key, `level_status` (additive; readers that do not know it ignore it):
//!
//! ```json
//! { "mode": "all" | "list" | "active", "recorded_at": "2026-11-15T10:00:00Z",
//!   "levels": [ { "id": 2, "name": "First", "recorded": true, "rows": 14, "fingerprint": "00ab...", "stale": false },
//!               { "id": 3, "name": "Roof", "recorded": false, "rows": 0, "stale": false } ] }
//! ```
//!
//! `fingerprint` is a hash of the level's objects (the take-off's own labels and tables left out), its name,
//! elevation and height at the moment of recording. `takeoff.levels` compares it with the drawing now and says which
//! recorded levels changed since (`stale`); with `update` it writes those flags into the project file's header.

use std::fmt::Write as _;

use aad_doc::{Drawing, LevelId};
use serde_json::{Value, json};

use super::bridge::{self, Collected, Item};
use super::takeoff::{L_LABELS, L_TABLE};
use super::*;
use crate::{Result, Session};

/// The project-store key that keeps what was recorded per level (for the stale check).
pub(crate) const LEVELS_KEY: &str = "takeoffLevels";

/// Saved take-off name → the command that measures it again.
const TOOLS: [(&str, &str); 10] = [
    ("Linear", "takeoff.linear"),
    ("Bricks", "takeoff.brick"),
    ("Beams", "takeoff.beams"),
    ("Ceiling", "takeoff.ceiling"),
    ("Floor", "takeoff.floor"),
    ("Areas", "takeoff.area"),
    ("Slab", "takeoff.slab"),
    ("Columns", "takeoff.columns"),
    ("Rectangles", "takeoff.rects"),
    ("WallPaint", "takeoff.paint"),
];

/// Which levels a recording covers.
pub(crate) struct Request {
    pub ids: Vec<LevelId>,
    /// `"all"` or `"list"`.
    pub mode: &'static str,
}

/// Reads the `levels` parameter. `None`: the parameter is absent, or `"all"` on a drawing without named levels (one
/// implicit level: the recording is the usual one).
pub(crate) fn parse(d: &Drawing, p: &Value) -> Result<Option<Request>> {
    let Some(v) = p.get("levels").filter(|v| !v.is_null()) else { return Ok(None) };
    if str_param(p, "level").is_some() {
        return Err(bad(
            "takeoff.record",
            "give `level` (the name stamped on the active level's rows) or `levels` (which levels to record), not both",
        ));
    }
    let names: Vec<&Value> = match v {
        Value::String(t) if t.trim().eq_ignore_ascii_case("all") => {
            if !d.has_levels() {
                return Ok(None);
            }
            return Ok(Some(Request { ids: d.levels.iter().take(aad_doc::MAX_LEVELS).map(|l| l.id).collect(), mode: "all" }));
        }
        Value::String(_) | Value::Number(_) => vec![v],
        Value::Array(a) if !a.is_empty() && a.len() <= aad_doc::MAX_LEVELS => a.iter().collect(),
        Value::Array(_) => return Err(bad("takeoff.record", format!("`levels` lists 1 to {} levels", aad_doc::MAX_LEVELS))),
        _ => return Err(bad("takeoff.record", "`levels` is \"all\" or a list of level names or ids")),
    };
    if !d.has_levels() {
        return Err(bad("takeoff.record", "the drawing has no named levels (`level.new`): leave `levels` out to record the one level it has"));
    }
    let mut ids: Vec<LevelId> = Vec::new();
    for n in names {
        let found = match n {
            Value::String(t) => d.level_by_name(t).map(|l| l.id),
            Value::Number(x) => x.as_u64().and_then(|u| u32::try_from(u).ok()).map(LevelId).filter(|id| d.level(*id).is_some()),
            _ => return Err(bad("takeoff.record", "`levels` is \"all\" or a list of level names or ids")),
        };
        let id = found.ok_or_else(|| {
            let known: Vec<&str> = d.levels.iter().map(|l| l.name.as_str()).collect();
            bad("takeoff.record", format!("no such level: {}; the levels are {}", short(n), known.join(", ")))
        })?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(Some(Request { ids, mode: "list" }))
}

/// A parameter value as text for a message, cut short.
fn short(v: &Value) -> String {
    let t = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    t.chars().take(40).collect()
}

struct Fnv(u64);

impl std::fmt::Write for Fnv {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for b in s.bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        Ok(())
    }
}

/// A hash of a level's content: its name, elevation, height and objects (not the take-off's own labels and tables).
pub(crate) fn fingerprint(d: &Drawing, id: LevelId) -> Option<String> {
    let level = d.level(id)?;
    let store = d.level_store(id)?;
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    let _ = write!(h, "{}|{:?}|{:?}|", level.name, level.elevation, level.height);
    for e in store.iter() {
        if e.common.layer.eq_ignore_ascii_case(L_LABELS) || e.common.layer.eq_ignore_ascii_case(L_TABLE) {
            continue;
        }
        let _ = write!(h, "{e:?};");
    }
    Some(format!("{:016x}", h.0))
}

/// Collects the rows of every requested level (see the module text). The result is the active level's collection with
/// the merged rows; its `root` is the header's source.
pub(crate) fn collect(s: &mut Session, p: &Value, req: &Request) -> Result<Collected> {
    let active = s.doc()?.active_level;
    let mut p2 = p.clone();
    if let (Some(obj), Some(name)) = (p2.as_object_mut(), s.doc()?.active_level_ref().map(|l| l.name.clone())) {
        obj.insert("level".into(), json!(name));
    }
    let mut base = bridge::collect(s, &p2)?;
    let mut merged: Vec<Item> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut take = |items: Vec<Item>, merged: &mut Vec<Item>| {
        for it in items {
            if seen.insert((it.kind, it.n)) {
                merged.push(it);
            }
        }
    };
    let own = std::mem::take(&mut base.items);
    let mut own = Some(own);
    let tools: Vec<&str> = {
        let d = s.doc()?;
        TOOLS.iter().filter(|(name, _)| !project::get(d, &format!("takeoff:{name}")).is_null()).map(|(_, cmd)| *cmd).collect()
    };
    let mut scratch: Option<Session> = None;
    for id in &req.ids {
        let Some(name) = s.doc()?.level(*id).map(|l| l.name.clone()) else { continue };
        if Some(*id) == active {
            if let Some(items) = own.take() {
                take(items, &mut merged);
            }
            continue;
        }
        if scratch.is_none() {
            let mut t = Session::empty();
            t.open_drawing(s.doc()?.clone(), "take-off levels", None);
            scratch = Some(t);
        }
        let Some(t) = scratch.as_mut() else { continue };
        if let Err(e) = t.execute("level.activate", &json!({ "level": name })) {
            base.warnings.push(format!("level `{name}` not recorded: {e}"));
            continue;
        }
        for cmd in &tools {
            if let Err(e) = t.execute(cmd, &json!({ "table": false })) {
                base.warnings.push(format!("level `{name}`: {cmd} did not run: {e}"));
            }
        }
        let mut q = json!({ "level": name });
        if let Some(b) = p.get("bearing") {
            q["bearing"] = b.clone();
        }
        match bridge::collect(t, &q) {
            Ok(c) => {
                base.warnings.extend(c.warnings.into_iter().map(|w| format!("level `{name}`: {w}")));
                // Electrical and plumbing rows belong to the drawing, not to a level: they come from the active level's run.
                take(c.items.into_iter().filter(|i| i.kind < bridge::KIND_ELEC_BOARD).collect(), &mut merged);
            }
            Err(e) => base.warnings.push(format!("level `{name}` not recorded: {e}")),
        }
    }
    base.items = merged;
    Ok(base)
}

/// The `level_status` header value for a recording: `req` is the levels asked for, `None` for a plain recording
/// (then only the active level counts as recorded). `None` when the drawing has no named levels.
pub(crate) fn status(d: &Drawing, req: Option<&Request>, items: &[Item], at: &str) -> Option<Value> {
    if !d.has_levels() {
        return None;
    }
    let (mode, ids): (&str, Vec<LevelId>) = match req {
        Some(r) => (r.mode, r.ids.clone()),
        None => ("active", d.active_level.into_iter().collect()),
    };
    let rows_of = |id: LevelId| items.iter().filter(|i| i.row.get("level_id").and_then(Value::as_u64) == Some(u64::from(id.0))).count();
    let levels: Vec<Value> = d
        .levels
        .iter()
        .take(aad_doc::MAX_LEVELS)
        .map(|l| {
            let mut o = json!({ "id": l.id.0, "name": l.name, "recorded": ids.contains(&l.id), "rows": rows_of(l.id), "stale": false });
            if ids.contains(&l.id)
                && let Some(f) = fingerprint(d, l.id)
            {
                o["fingerprint"] = json!(f);
            }
            o
        })
        .collect();
    Some(json!({ "mode": mode, "recorded_at": at, "levels": levels }))
}

/// Per-level state now against what the last recording stored in the drawing: `[{id, name, recorded, rows, stale}]`.
pub(crate) fn live_status(d: &Drawing) -> Vec<Value> {
    let stored = project::get(d, LEVELS_KEY);
    let entries: &[Value] = stored.get("levels").and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let mut out: Vec<Value> = Vec::new();
    for l in d.levels.iter().take(aad_doc::MAX_LEVELS) {
        let rec = entries.iter().find(|e| e.get("id").and_then(Value::as_u64) == Some(u64::from(l.id.0)) && e.get("recorded") == Some(&json!(true)));
        let stale = rec.is_some_and(|e| e.get("fingerprint").and_then(Value::as_str) != fingerprint(d, l.id).as_deref());
        out.push(json!({ "id": l.id.0, "name": l.name, "recorded": rec.is_some(), "rows": rec.and_then(|e| e.get("rows")).cloned().unwrap_or(json!(0)), "stale": stale }));
    }
    // A recorded level that the drawing no longer has.
    for e in entries {
        let id = e.get("id").and_then(Value::as_u64);
        if e.get("recorded") == Some(&json!(true)) && id.is_some() && !d.levels.iter().any(|l| Some(u64::from(l.id.0)) == id) {
            out.push(json!({ "id": id, "name": e.get("name").cloned().unwrap_or(Value::Null), "recorded": true, "rows": e.get("rows").cloned().unwrap_or(json!(0)), "stale": true, "deleted": true }));
        }
    }
    out
}

/// What the drawing keeps of a recording, so a later `takeoff.levels` can tell what changed.
pub(crate) fn stored_value(status: &Value) -> Value {
    json!({ "recordedAt": status["recorded_at"], "levels": status["levels"] })
}

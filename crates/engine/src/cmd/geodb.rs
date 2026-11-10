//! The geometry database commands (docs/GEOMETRY-DB.md): `geodb.build`, `geodb.report`,
//! `geodb.rules`, `geodb.review`, `geodb.origin` and `geodb.apply`.
//!
//! Building and classifying never change the drawing. `geodb.apply` first makes a preview
//! (handle, from layer, to layer, rule, confidence) and only an explicit `approve: true` moves
//! anything, as one undo step. The tables are written to the open project file (`.osp`) when there
//! is one, and are never part of the project's change log, so they are never synced.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use aad_doc::{Drawing, Handle};
use aad_geodb::origin::{self, OriginReport};
use aad_geodb::units::{self, UNITS_KEY};
use aad_geodb::{BuildOptions, GeoModel, Rules};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

const CMD: &str = "geodb";
/// Rows of a preview or report returned in the JSON (the rest are counted).
const MAX_ROWS: usize = 2000;

/// Extra standard layers this feature needs that the standard set did not define: the beam layer.
const EXTRA_LAYERS: &[(&str, i16, &str, u16)] = &[("S-BEAM", 5, "Continuous", 35)];

/// What a session keeps per drawing between the geometry database commands.
pub(crate) struct GeoState {
    model: GeoModel,
    preview: Option<Preview>,
    /// Review decisions by feature key (also stored in the project file when there is one).
    reviews: HashMap<String, String>,
}

struct Preview {
    /// Drawing revision the preview was made at; an edit makes it stale.
    revision: u64,
    rows: Vec<Row>,
}

#[derive(Clone)]
struct Row {
    handle: u64,
    from: String,
    to: &'static str,
    rule: String,
    confidence: f64,
    key: String,
    kind: String,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("geodb.build", "Build Geometry Database", run_build)
            .menu(&["Tools", "Geometry Database", "Build"])
            .noundo()
            .params("{units?: mm|cm|m|in|ft (confirms a unitless drawing), rebuild?: bool, persist?: bool (default true with a project file open)} → {mode: full|incremental, summary, needsLayerMapping, needsUnitConfirmation, persisted?, origin}. Classifies from geometry only; changes nothing in the drawing"),
        CommandSpec::new("geodb.report", "Geometry Database Report", run_report)
            .menu(&["Tools", "Geometry Database", "Report"])
            .noundo()
            .params("{detail?: summary|ambiguous|features|rooms|regions, kind?: wall|beam|column|door|window|room|slab, limit?: n} → {summary, ...}; ambiguous lists the features under the confidence threshold with their numbers; rooms lists area and perimeter in mm; regions lists the page regions (plan, structure, other)"),
        CommandSpec::new("geodb.rules", "Geometry Rules", run_rules)
            .menu(&["Tools", "Geometry Database", "Rules"])
            .noundo()
            .params("{set?: {section: wall_types|cavity_parts|pair|door|window|evidence|column|beam|room|region, params}} → {rules, version}; a change bumps the version and reclassifies"),
        CommandSpec::new("geodb.review", "Review a Detected Feature", run_review)
            .noundo()
            .params("{key, state: accepted|rejected|edited, note?} | {key, clear: true} → {reviews}; keyed by the feature's geometry hash, so it survives a rebuild"),
        CommandSpec::new("geodb.origin", "Drawing Origin", run_origin)
            .menu(&["Tools", "Geometry Database", "Origin and Layer Map"])
            .noundo()
            .params("{} → {origin, suggestions: [{from, to?, objects, reason}]}; a drawing without the Tren stamp and with non-standard layers needs the layer mapper (layermap, suggest: true) first"),
        CommandSpec::new("geodb.apply", "Apply Geometry Classification", run_apply)
            .menu(&["Tools", "Geometry Database", "Move to Layers"])
            .params("{approve?: bool (false: preview only), minConfidence? (default 0.6), handles?: [only these], keys?: [only these features]} → preview: {preview: [{handle, from, to, rule, confidence}], total, approved: false}; approve: {moved, skippedLocked}. One undo step; nothing moves without approve"),
    ]
}

// ---- helpers ----

fn project_path(s: &Session) -> Option<std::path::PathBuf> {
    s.state().ok()?.path.as_deref().filter(|p| super::file::is_container(p)).map(std::path::PathBuf::from)
}

/// Every layer name of the app's fixed standard set.
pub(crate) fn standard_layer_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = super::hcw::STANDARD_LAYERS.iter().map(|l| l.0).collect();
    v.extend(super::hcw::PRESENTATION_LAYERS.iter().map(|l| l.0));
    v.extend(super::hcw::PERMIT_LAYERS.iter().map(|l| l.0));
    v.extend(super::legacytools::VH_LAYERS.iter().map(|l| l.0));
    v.extend(EXTRA_LAYERS.iter().map(|l| l.0));
    v.extend(["E-POWR", "E-LITE", "S-BEAM-TEXT", "P-TEXT", "A-FURN", "A-FURN-TEXT", "A-CLNG-GRID", "A-CLNG-TEXT", "ROOM-LABELS", "ROOM-RECT"]);
    v
}

/// The drawing's origin. A drawing that went through the layer mapper counts as handled.
pub(crate) fn origin_of(d: &Drawing) -> OriginReport {
    let mut r = origin::detect(d, &standard_layer_names());
    if r.foreign && d.project.contains_key(LAYER_MAP_DONE_KEY) {
        r.foreign = false;
        r.reasons.push("the layer mapper has been run".into());
    }
    r
}

/// Project key set when the layer mapper has moved the layers of a foreign drawing.
pub(crate) const LAYER_MAP_DONE_KEY: &str = "layerMapDone";

/// Keyword to standard layer, for suggestions only. The mapper never applies these by itself.
const KEYWORDS: &[(&str, &str)] = &[
    ("WALL", "A-WALL"),
    ("DOOR", "A-DOOR"),
    ("WINDOW", "A-WIND"),
    ("WIND", "A-WIND"),
    ("GLAZ", "A-GLAZ"),
    ("COLUMN", "A-COL"),
    ("BEAM", "S-BEAM"),
    ("STAIR", "A-STAIR"),
    ("FURN", "I-FURN"),
    ("DIM", "AN-DIMS"),
    ("TEXT", "AN-TEXT"),
    ("TXT", "AN-TEXT"),
    ("HATCH", "AN-HATCH"),
    ("GRID", "AN-GRID"),
    ("ROOF", "A-ROOF"),
    ("CEIL", "A-CLNG"),
];

/// Suggested layer maps for the layers with objects that are not in the standard set.
pub(crate) fn suggest_layer_map(d: &Drawing) -> Vec<Value> {
    let std_names = standard_layer_names();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in d.model.iter() {
        *counts.entry(e.common.layer.clone()).or_default() += 1;
    }
    let mut out = Vec::new();
    for (name, n) in counts {
        if name == "0" || name.eq_ignore_ascii_case("Defpoints") || std_names.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let up = name.to_ascii_uppercase();
        // Wall hatching and wall text are not walls: the more specific words win.
        let target = if up.contains("HATCH") {
            Some(("AN-HATCH", "the name says hatch"))
        } else if up.contains("TEXT") || up.contains("TXT") {
            Some(("AN-TEXT", "the name says text"))
        } else if up.contains("DIM") {
            Some(("AN-DIMS", "the name says dimension"))
        } else {
            KEYWORDS.iter().find(|(k, _)| up.contains(k)).map(|(_, t)| (*t, "the name contains a known word"))
        };
        out.push(match target {
            Some((to, why)) => json!({ "from": name, "to": to, "objects": n, "reason": why }),
            None => json!({ "from": name, "objects": n, "reason": "no known word in the name; choose a target" }),
        });
    }
    out
}

/// Called before a drawing is saved: stamp the origin and record the drafting units when the drawing declares them.
pub(crate) fn stamp_for_save(d: &mut Drawing) {
    origin::stamp(d, env!("CARGO_PKG_VERSION"));
    if let Some(u) = units::declared_units(d)
        && u.confirmed
    {
        d.project.insert(UNITS_KEY.into(), u.stamp().to_string());
    }
}

/// Record the drafting units of a new drawing (Tren creates them in a chosen system).
pub(crate) fn stamp_new(d: &mut Drawing, metric: bool) {
    let u =
        if metric { units::Units::new("mm", units::UnitSource::Project, true) } else { units::Units::new("in", units::UnitSource::Project, true) };
    if let Some(u) = u {
        d.project.insert(UNITS_KEY.into(), u.stamp().to_string());
    }
    origin::stamp(d, env!("CARGO_PKG_VERSION"));
}

/// The drawing's tables written next to its drawing in the project file; also the origin and units in `meta`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_project_meta(path: &str, d: &Drawing) {
    let Ok(conn) = aad_geodb::store::open(std::path::Path::new(path)) else { return };
    if let Some(v) = d.project.get(origin::ORIGIN_KEY) {
        let _ = aad_geodb::store::write_meta(&conn, "origin", v);
    }
    if let Some(raw) = d.project.get(UNITS_KEY)
        && let Ok(v) = serde_json::from_str::<Value>(raw)
    {
        if let Some(sys) = v.get("system").and_then(Value::as_str) {
            let _ = aad_geodb::store::write_meta(&conn, "drafting_units", sys);
        }
        if let Some(u) = v.get("unit").and_then(Value::as_str) {
            let _ = aad_geodb::store::write_meta(&conn, "drafting_unit", u);
        }
    }
}

/// Build off the calling thread: a host clones the drawing (cheap, copy-on-write), calls this on a worker and
/// hands the model to [`Session::geodb_install`] on the thread that owns the session.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_build(doc: Arc<Drawing>, opts: BuildOptions) -> std::thread::JoinHandle<std::result::Result<GeoModel, String>> {
    std::thread::spawn(move || GeoModel::build(&doc, &opts).map_err(|e| e.to_string()))
}

impl Session {
    /// Keep a model built elsewhere (a worker thread) for the active drawing.
    pub fn geodb_install(&mut self, model: GeoModel) -> Result<()> {
        let uid = self.state()?.uid;
        self.geodb.insert(uid, GeoState { model, preview: None, reviews: HashMap::new() });
        Ok(())
    }
}

fn stored_rules(_s: &Session) -> Option<Rules> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = project_path(_s)?;
        let conn = aad_geodb::store::open(&path).ok()?;
        aad_geodb::store::load_rules(&conn).ok().flatten()
    }
    #[cfg(target_arch = "wasm32")]
    None
}

fn state_mut<'a>(s: &'a mut Session, cmd: &str) -> Result<&'a mut GeoState> {
    let uid = s.state()?.uid;
    s.geodb.get_mut(&uid).ok_or_else(|| bad(cmd, "no geometry database for this drawing yet; run geodb.build"))
}

// ---- build ----

fn run_build(s: &mut Session, p: &Value) -> Result<Value> {
    let uid = s.state()?.uid;
    let units = str_param(p, "units").map(str::to_string);
    let rebuild = bool_or(p, "rebuild", false) || units.is_some();
    let doc = s.state()?.doc.clone();
    let origin = origin_of(&doc);
    let mut mode = "full";
    let mut update = None;
    let have = s.geodb.contains_key(&uid);
    let mut delta = None;
    if have && !rebuild {
        if let Some(st) = s.geodb.get_mut(&uid) {
            let (stats, d) = st.model.update(&doc);
            if stats.rebuilt {
                mode = "full";
            } else {
                mode = "incremental";
            }
            update = Some(stats);
            delta = Some(d);
            st.preview = None;
        }
    } else {
        let rules = s.geodb.get(&uid).map(|g| g.model.rules.clone()).or_else(|| stored_rules(s));
        let model = GeoModel::build(&doc, &BuildOptions { units, rules }).map_err(|e| bad(CMD, e.to_string()))?;
        let reviews = s.geodb.remove(&uid).map(|g| g.reviews).unwrap_or_default();
        s.geodb.insert(uid, GeoState { model, preview: None, reviews });
    }
    let revision = s.state()?.revision;
    #[allow(unused_mut)]
    let mut persisted = Value::Null;
    #[cfg(not(target_arch = "wasm32"))]
    if bool_or(p, "persist", true)
        && let Some(path) = project_path(s)
        && let Some(st) = s.geodb.get(&uid)
    {
        let name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        persisted = match persist(&path, &st.model, &name, revision as i64, delta.as_ref(), mode == "incremental") {
            Ok(v) => v,
            Err(e) => json!({ "error": e }),
        };
        write_project_meta(&path.to_string_lossy(), &doc);
    }
    let _ = &delta;
    let st = s.geodb.get(&uid).ok_or_else(|| bad(CMD, "no model"))?;
    let m = &st.model;
    let needs_units = !m.units.confirmed;
    // The shell shows the command log: one line of what was found, and what still needs a person.
    let min = m.rules.evidence.classify_min;
    let n = |k: &str| m.features.iter().filter(|f| f.kind == k && f.confidence >= min).count();
    let mut line = format!(
        "Geometry database ({mode}, {} ms): {} walls, {} beams, {} columns, {} rooms, {} doors, {} windows in {} {}; {} below the confidence threshold.",
        m.timings.total_ms,
        n("wall"),
        n("beam"),
        n("column"),
        n("room"),
        n("door"),
        n("window"),
        m.units.unit,
        if m.units.confirmed { "(units confirmed)" } else { "(UNITS NOT CONFIRMED)" },
        m.features.iter().filter(|f| f.confidence < min).count()
    );
    if needs_units {
        line.push_str(" Give the unit (geodb.build units: mm|cm|m|in|ft) before applying.");
    }
    if origin.foreign {
        line.push_str(" This drawing is from another program: run the layer mapper before applying.");
    }
    s.echo(line);
    let st = s.geodb.get(&uid).ok_or_else(|| bad(CMD, "no model"))?;
    let m = &st.model;
    Ok(json!({
        "mode": mode,
        "summary": m.summary(),
        "update": update,
        "needsUnitConfirmation": needs_units,
        "needsLayerMapping": origin.foreign,
        "origin": origin,
        "persisted": persisted,
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn persist(
    path: &std::path::Path,
    m: &GeoModel,
    name: &str,
    revision: i64,
    delta: Option<&aad_geodb::Delta>,
    incremental: bool,
) -> std::result::Result<Value, String> {
    let mut conn = aad_geodb::store::open(path).map_err(|e| e.to_string())?;
    aad_geodb::store::save_rules(&conn, &m.rules).map_err(|e| e.to_string())?;
    let w = match (incremental, delta) {
        (true, Some(d)) => aad_geodb::store::write_update(&mut conn, m, d, name, revision),
        _ => aad_geodb::store::write_full(&mut conn, m, name, revision),
    }
    .map_err(|e| e.to_string())?;
    Ok(json!({ "path": path.to_string_lossy(), "rows": w.rows, "ms": w.ms, "rtree": w.rtree, "incremental": incremental && delta.is_some() }))
}

// ---- report ----

fn feature_json(f: &aad_geodb::model::Feature) -> Value {
    json!({
        "key": f.key, "kind": f.kind, "subkind": f.subkind, "confidence": (f.confidence * 1000.0).round() / 1000.0, "rule": f.rule, "rulesVersion": f.rules_version,
        "from": [f.x1, f.y1], "to": [f.x2, f.y2], "width": f.width, "thickness": if f.kind == "wall" { f.depth } else { 0.0 }, "length": f.length,
        "members": f.members.len(), "props": f.props,
    })
}

fn run_report(s: &mut Session, p: &Value) -> Result<Value> {
    let uid = s.state()?.uid;
    let detail = str_param(p, "detail").unwrap_or("summary");
    let limit = (f64_or(p, "limit", 50.0).max(1.0) as usize).min(MAX_ROWS);
    let kind = str_param(p, "kind");
    let origin = origin_of(s.doc()?);
    let Some(st) = s.geodb.get(&uid) else {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = project_path(s)
            && let Ok(conn) = aad_geodb::store::open(&path)
            && let Ok(v) = aad_geodb::store::read_summary(&conn)
        {
            return Ok(
                json!({ "source": "project file", "stored": v, "origin": origin, "note": "tables read from the project file; run geodb.build to work on this drawing" }),
            );
        }
        return Err(bad(CMD, "no geometry database for this drawing yet; run geodb.build"));
    };
    let m = &st.model;
    let min = m.rules.evidence.classify_min;
    let mut out = json!({ "summary": m.summary(), "origin": origin });
    if detail == "rooms" || detail == "regions" {
        let list: Vec<Value> = if detail == "rooms" {
            m.rooms
                .iter()
                .filter(|r| kind.is_none_or(|k| r.kind == k))
                .take(limit)
                .map(|r| {
                    json!({
                        "key": r.key, "kind": r.kind, "region": r.region, "areaMm2": r.area_mm2.round(), "perimeterMm": r.perimeter_mm.round(),
                        "hatches": r.hatches, "outline": r.outline.iter().take(64).map(|p| json!([p[0].round(), p[1].round()])).collect::<Vec<_>>(),
                    })
                })
                .collect()
        } else {
            m.regions
                .iter()
                .filter(|r| r.kind != "empty")
                .take(limit)
                .map(|r| {
                    json!({
                        "id": r.id, "kind": r.kind, "box": r.bbox.map(f64::round), "score": (r.score * 100.0).round() / 100.0, "walls": r.walls,
                        "wallLengthMm": r.wall_len_mm.round(), "doors": r.doors, "columns": r.columns, "rooms": r.rooms, "closure": (r.closure * 100.0).round() / 100.0,
                    })
                })
                .collect()
        };
        out["total"] = json!(list.len());
        out["list"] = Value::Array(list);
        return Ok(out);
    }
    let mut pick: Vec<&aad_geodb::model::Feature> = match detail {
        "summary" => Vec::new(),
        "ambiguous" => m.features.iter().filter(|f| f.confidence < min && f.confidence >= min - 0.25).collect(),
        "features" => m.features.iter().filter(|f| f.confidence >= min).collect(),
        other => return Err(bad(CMD, format!("unknown detail `{other}` (summary, ambiguous, features, rooms, regions)"))),
    };
    if let Some(k) = kind {
        pick.retain(|f| f.kind == k);
    }
    if detail == "ambiguous" {
        pick.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    }
    out["total"] = json!(pick.len());
    out["list"] = Value::Array(pick.iter().take(limit).map(|f| feature_json(f)).collect());
    out["reviews"] = json!(st.reviews);
    Ok(out)
}

// ---- rules ----

fn run_rules(s: &mut Session, p: &Value) -> Result<Value> {
    let uid = s.state()?.uid;
    if let Some(set) = p.get("set").filter(|v| !v.is_null()) {
        let section = set.get("section").and_then(Value::as_str).ok_or_else(|| bad(CMD, "`set.section` is required"))?;
        let params = set.get("params").ok_or_else(|| bad(CMD, "`set.params` is required"))?;
        let mut rules = match s.geodb.get(&uid) {
            Some(g) => g.model.rules.clone(),
            None => stored_rules(s).unwrap_or_default(),
        };
        rules.set_section(section, params).map_err(|e| bad(CMD, e.to_string()))?;
        if let Some(g) = s.geodb.get_mut(&uid) {
            g.model.reclassify(rules.clone()).map_err(|e| bad(CMD, e.to_string()))?;
            g.preview = None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = project_path(s)
            && let Ok(conn) = aad_geodb::store::open(&path)
        {
            let _ = aad_geodb::store::save_rules(&conn, &rules);
        }
        return Ok(json!({ "rules": rules, "version": rules.version, "reclassified": s.geodb.contains_key(&uid) }));
    }
    let rules = match s.geodb.get(&uid) {
        Some(g) => g.model.rules.clone(),
        None => stored_rules(s).unwrap_or_default(),
    };
    Ok(
        json!({ "rules": rules, "version": rules.version, "note": "the cavity wall default of 300 mm (115 + 70 + 115) is an assumption; edit wall_types to suit" }),
    )
}

// ---- review ----

fn run_review(s: &mut Session, p: &Value) -> Result<Value> {
    let key = str_param(p, "key").ok_or_else(|| bad(CMD, "`key` is required"))?.to_string();
    let path = project_path(s);
    let clear = bool_or(p, "clear", false);
    let state = str_param(p, "state").map(str::to_string);
    let note = str_param(p, "note").unwrap_or("").to_string();
    if !clear && !matches!(state.as_deref(), Some("accepted" | "rejected" | "edited")) {
        return Err(bad(CMD, "`state` must be accepted, rejected or edited (or pass clear: true)"));
    }
    let st = state_mut(s, CMD)?;
    if !st.model.features.iter().any(|f| f.key == key) && !st.reviews.contains_key(&key) {
        return Err(bad(CMD, format!("no feature with key `{key}`")));
    }
    if clear {
        st.reviews.remove(&key);
    } else if let Some(state) = &state {
        st.reviews.insert(key.clone(), state.clone());
    }
    st.preview = None;
    let all = st.reviews.clone();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &path
        && let Ok(conn) = aad_geodb::store::open(path)
    {
        let _ = if clear {
            aad_geodb::store::review_clear(&conn, &key).map(|_| ())
        } else {
            aad_geodb::store::review_set(&conn, &key, state.as_deref().unwrap_or(""), &note, "tren", "")
        };
    }
    let _ = &note;
    Ok(json!({ "reviews": all }))
}

// ---- origin ----

fn run_origin(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let o = origin_of(d);
    let suggestions = if o.foreign { suggest_layer_map(d) } else { Vec::new() };
    Ok(
        json!({ "origin": o, "suggestions": suggestions, "next": if o.foreign { "layermap with the map you accept (suggest: true lists candidates), then geodb.build" } else { "geodb.build" } }),
    )
}

// ---- apply ----

fn target_of(kind: &str) -> Option<&'static str> {
    match kind {
        "wall" => Some("A-WALL"),
        "door" => Some("A-DOOR"),
        "window" => Some("A-WIND"),
        "column" => Some("A-COL"),
        "beam" => Some("S-BEAM"),
        _ => None,
    }
}

fn make_preview(s: &Session, p: &Value) -> Result<Vec<Row>> {
    let uid = s.state()?.uid;
    let st = s.geodb.get(&uid).ok_or_else(|| bad(CMD, "no geometry database for this drawing yet; run geodb.build"))?;
    let m = &st.model;
    let min = f64_or(p, "minConfidence", 0.6).max(m.rules.evidence.classify_min);
    let only_keys: Option<Vec<&str>> = p.get("keys").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect());
    let doc = s.doc()?;
    // handle -> (priority, row). An opening beats a wall for a shared object, then the higher confidence.
    let mut best: HashMap<u64, (u8, Row)> = HashMap::new();
    for f in &m.features {
        if f.confidence < min || st.reviews.get(&f.key).is_some_and(|r| r == "rejected") {
            continue;
        }
        if only_keys.as_ref().is_some_and(|k| !k.contains(&f.key.as_str())) {
            continue;
        }
        let Some(to) = target_of(&f.kind) else { continue };
        let prio = if matches!(f.kind.as_str(), "wall" | "beam") { 0 } else { 1 };
        for mem in &f.members {
            // A wall face inside a block would move the whole block; leave those to a person.
            if matches!(f.kind.as_str(), "wall" | "beam") && m.prim(mem.handle, mem.prim).is_some_and(|pr| pr.in_block()) {
                continue;
            }
            let Some(e) = doc.entity(Handle(mem.handle)) else { continue };
            let row = Row {
                handle: mem.handle,
                from: e.common.layer.clone(),
                to,
                rule: f.rule.clone(),
                confidence: f.confidence,
                key: f.key.clone(),
                kind: f.kind.clone(),
            };
            let better = match best.get(&mem.handle) {
                None => true,
                Some((pp, r)) => prio > *pp || (prio == *pp && row.confidence > r.confidence),
            };
            if better {
                best.insert(mem.handle, (prio, row));
            }
        }
    }
    let only: Option<Vec<u64>> =
        p.get("handles").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).map(|h| h.0).collect());
    let mut rows: Vec<Row> = best
        .into_values()
        .map(|(_, r)| r)
        .filter(|r| !r.from.eq_ignore_ascii_case(r.to))
        .filter(|r| only.as_ref().is_none_or(|o| o.contains(&r.handle)))
        .collect();
    rows.sort_by_key(|r| r.handle);
    Ok(rows)
}

fn row_json(r: &Row) -> Value {
    json!({ "handle": Handle(r.handle).hex(), "from": r.from, "to": r.to, "rule": r.rule, "confidence": (r.confidence * 1000.0).round() / 1000.0, "feature": r.key, "kind": r.kind })
}

fn run_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let uid = s.state()?.uid;
    let approve = bool_or(p, "approve", false);
    {
        let st = s.geodb.get(&uid).ok_or_else(|| bad(CMD, "no geometry database for this drawing yet; run geodb.build"))?;
        if !st.model.units.confirmed {
            return Err(bad(CMD, "the drafting units are not confirmed; run geodb.build with units: mm, cm, m, in or ft first"));
        }
    }
    let o = origin_of(s.doc()?);
    if o.foreign {
        return Err(bad(
            CMD,
            "this drawing did not come from Tren and its layers are not the standard set; run the layer mapper first (geodb.origin lists suggestions, layermap applies a map)",
        ));
    }
    let revision = s.state()?.revision;
    if !approve {
        let rows = make_preview(s, p)?;
        let total = rows.len();
        let json_rows: Vec<Value> = rows.iter().take(MAX_ROWS).map(row_json).collect();
        if let Some(st) = s.geodb.get_mut(&uid) {
            st.preview = Some(Preview { revision, rows });
        }
        return Ok(
            json!({ "approved": false, "total": total, "shown": json_rows.len(), "preview": json_rows, "hint": "review the rows, then run geodb.apply with approve: true; nothing has moved" }),
        );
    }
    let preview = s.geodb.get(&uid).and_then(|g| g.preview.as_ref());
    let Some(preview) = preview else {
        return Err(bad(CMD, "make a preview first (geodb.apply without approve), review it, then approve"));
    };
    if preview.revision != revision {
        return Err(bad(CMD, "the drawing changed since the preview; run geodb.apply again to see the current rows"));
    }
    // An approval may name a subset of the preview; anything else is not approved.
    let mut rows = preview.rows.clone();
    if let Some(only) = p.get("handles").and_then(Value::as_array) {
        let only: Vec<u64> = only.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).map(|h| h.0).collect();
        rows.retain(|r| only.contains(&r.handle));
    }
    let (mut moved, mut locked) = (0usize, 0usize);
    let d = s.doc_mut()?;
    for to in ["A-WALL", "A-DOOR", "A-WIND", "A-COL", "S-BEAM"] {
        if d.layer(to).is_some() {
            continue;
        }
        if let Some(l) = super::hcw::STANDARD_LAYERS.iter().find(|l| l.0 == to) {
            super::legacytools::apply_layer(d, l.0, l.1, l.2, l.3, true);
        } else if let Some(l) = EXTRA_LAYERS.iter().find(|l| l.0 == to) {
            super::legacytools::apply_layer(d, l.0, l.1, l.2, l.3, true);
        }
    }
    for r in &rows {
        let from_locked = d.layer(&r.from).is_some_and(|l| l.locked);
        if from_locked {
            locked += 1;
            continue;
        }
        if d.modify_entity(Handle(r.handle), |e| e.common.layer = r.to.to_string()).is_ok() {
            moved += 1;
        }
    }
    if let Some(st) = s.geodb.get_mut(&uid) {
        st.preview = None;
    }
    Ok(json!({ "approved": true, "moved": moved, "skippedLocked": locked }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &mut Session, pts: &[((f64, f64), (f64, f64))]) {
        for (a, b) in pts {
            s.execute("line", &json!({ "points": [[a.0, a.1], [b.0, b.1]] })).unwrap();
        }
    }

    /// A drawing as another program would have saved it: no Tren stamp, no recorded units.
    fn unstamp(s: &mut Session) {
        let d = s.doc_mut().unwrap();
        d.project.remove(aad_geodb::origin::ORIGIN_KEY);
        d.project.remove(UNITS_KEY);
    }

    fn two_walls(s: &mut Session) {
        unstamp(s);
        s.doc_mut().unwrap().header.set_i64("INSUNITS", 4);
        lines(
            s,
            &[((0.0, 0.0), (5000.0, 0.0)), ((0.0, 230.0), (5000.0, 230.0)), ((0.0, 3000.0), (5000.0, 3000.0)), ((0.0, 3230.0), (5000.0, 3230.0))],
        );
    }

    #[test]
    fn build_classifies_without_changing_the_drawing_and_apply_needs_a_preview_and_an_approval() {
        let mut s = Session::new();
        two_walls(&mut s);
        let before = s.state().unwrap().doc.clone();
        let r = s.execute("geodb.build", &json!({})).unwrap();
        assert_eq!(r["mode"], json!("full"));
        assert!(r["origin"]["foreign"].as_bool().unwrap(), "a new Session drawing made through `line` on layer 0 without a stamp: {r}");
        assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc), "build leaves the drawing alone");
        // Stamp it as a Tren drawing: apply may run.
        stamp_new(s.doc_mut().unwrap(), true);
        s.doc_mut().unwrap().header.set_i64("INSUNITS", 4);
        let r = s.execute("geodb.build", &json!({})).unwrap();
        assert_eq!(r["summary"]["features"].as_array().unwrap().iter().filter(|f| f["kind"] == json!("wall.full")).count(), 1, "{r}");
        let walls =
            r["summary"]["features"].as_array().unwrap().iter().find(|f| f["kind"] == json!("wall.full")).unwrap()["classified"].as_u64().unwrap();
        assert_eq!(walls, 2);
        // Approving without a preview is refused.
        assert!(s.execute("geodb.apply", &json!({ "approve": true })).is_err());
        let undo_before = s.state().unwrap().undo.len();
        let pv = s.execute("geodb.apply", &json!({})).unwrap();
        assert_eq!(pv["approved"], json!(false));
        assert_eq!(pv["total"], json!(4), "{pv}");
        assert_eq!(pv["preview"][0]["to"], json!("A-WALL"));
        assert_eq!(s.state().unwrap().undo.len(), undo_before, "a preview is not an undo step");
        assert!(s.doc().unwrap().model.iter().all(|e| e.common.layer == "0"), "nothing moved");
        // An edit makes the preview stale.
        s.execute("line", &json!({ "points": [[0, 9000], [10, 9000]] })).unwrap();
        assert!(s.execute("geodb.apply", &json!({ "approve": true })).is_err());
        s.execute("geodb.build", &json!({})).unwrap();
        s.execute("geodb.apply", &json!({})).unwrap();
        let undo_before = s.state().unwrap().undo.len();
        let r = s.execute("geodb.apply", &json!({ "approve": true })).unwrap();
        assert_eq!(r["moved"], json!(4));
        assert_eq!(s.state().unwrap().undo.len(), undo_before + 1, "one undo step");
        assert_eq!(s.doc().unwrap().model.iter().filter(|e| e.common.layer == "A-WALL").count(), 4);
        s.execute("undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().model.iter().all(|e| e.common.layer == "0"), "undo puts everything back");
    }

    #[test]
    fn unitless_drawings_are_not_applied_until_the_unit_is_confirmed() {
        let mut s = Session::new();
        s.doc_mut().unwrap().header.set_i64("INSUNITS", 0);
        stamp_new(s.doc_mut().unwrap(), true);
        s.doc_mut().unwrap().project.remove(UNITS_KEY);
        // Walls drawn in metres.
        lines(&mut s, &[((0.0, 0.0), (5.0, 0.0)), ((0.0, 0.23), (5.0, 0.23))]);
        let r = s.execute("geodb.build", &json!({})).unwrap();
        assert_eq!(r["needsUnitConfirmation"], json!(true), "{r}");
        assert!(s.execute("geodb.apply", &json!({})).is_err());
        let r = s.execute("geodb.build", &json!({ "units": "m" })).unwrap();
        assert_eq!(r["needsUnitConfirmation"], json!(false));
        assert!(s.execute("geodb.apply", &json!({})).is_ok());
    }

    #[test]
    fn rules_commands_edit_validate_and_reclassify() {
        let mut s = Session::new();
        two_walls(&mut s);
        stamp_new(s.doc_mut().unwrap(), true);
        s.execute("geodb.build", &json!({})).unwrap();
        let r = s.execute("geodb.rules", &json!({})).unwrap();
        assert_eq!(r["rules"]["wall_types"][1]["metric_mm"], json!(230.0));
        assert!(s.execute("geodb.rules", &json!({ "set": { "section": "pair", "params": { "tolerance_mm": -3 } } })).is_err());
        let r = s.execute("geodb.rules", &json!({ "set": { "section": "pair", "params": { "min_run_mm": 9000 } } })).unwrap();
        assert_eq!(r["version"], json!(2));
        let r = s.execute("geodb.report", &json!({ "detail": "features" })).unwrap();
        assert_eq!(r["total"], json!(0), "no run is 9 m long any more");
        // A review decision survives a rebuild.
        s.execute("geodb.rules", &json!({ "set": { "section": "pair", "params": { "min_run_mm": 600 } } })).unwrap();
        let key = s.execute("geodb.report", &json!({ "detail": "features" })).unwrap()["list"][0]["key"].as_str().unwrap().to_string();
        s.execute("geodb.review", &json!({ "key": key, "state": "rejected" })).unwrap();
        s.execute("geodb.build", &json!({ "rebuild": true })).unwrap();
        assert_eq!(s.execute("geodb.apply", &json!({})).unwrap()["total"], json!(2), "the rejected wall is not in the preview");
    }

    #[test]
    fn programmatic_calls_never_panic_on_hostile_params() {
        let mut s = Session::new();
        for p in [json!({ "units": 5 }), json!({ "units": "parsec" }), json!({ "rebuild": "yes" }), json!(null), json!({ "persist": [] })] {
            let _ = s.execute("geodb.build", &p);
        }
        for p in [json!({ "detail": 5 }), json!({ "limit": -1 }), json!({ "limit": 1e300 }), json!({ "kind": [] })] {
            let _ = s.execute("geodb.report", &p);
        }
        for p in [json!({ "set": 5 }), json!({ "set": { "section": 3 } }), json!({ "set": { "section": "pair", "params": 7 } })] {
            let _ = s.execute("geodb.rules", &p);
        }
        for p in [json!({ "minConfidence": "x" }), json!({ "handles": "z" }), json!({ "keys": 4 })] {
            let _ = s.execute("geodb.apply", &p);
        }
        let _ = s.execute("geodb.review", &json!({ "key": "nope", "state": "rejected" }));
    }

    #[test]
    fn origin_suggests_standard_layers_for_a_foreign_drawing() {
        let mut s = Session::new();
        two_walls(&mut s);
        let hs = s.doc().unwrap().model.handles();
        let d = s.doc_mut().unwrap();
        d.ensure_layer("WALL-EXT");
        d.ensure_layer("WALL HATCH");
        d.ensure_layer("MYSTERY");
        for (i, h) in hs.iter().enumerate() {
            let l = ["WALL-EXT", "WALL HATCH", "MYSTERY", "WALL-EXT"][i % 4];
            let _ = d.modify_entity(*h, |e| e.common.layer = l.into());
        }
        let r = s.execute("geodb.origin", &json!({})).unwrap();
        assert_eq!(r["origin"]["foreign"], json!(true));
        let sug = r["suggestions"].as_array().unwrap();
        let to = |from: &str| sug.iter().find(|x| x["from"] == json!(from)).and_then(|x| x["to"].as_str());
        assert_eq!(to("WALL-EXT"), Some("A-WALL"));
        assert_eq!(to("WALL HATCH"), Some("AN-HATCH"), "hatch is not a wall");
        assert_eq!(to("MYSTERY"), None);
    }

    /// Tren's own plan: walls, doors and windows drawn by the wall commands, every layer set to 0, then classified
    /// from geometry. The layers it had are the truth.
    #[test]
    fn a_plan_drawn_with_the_wall_commands_is_recovered_from_geometry_alone() {
        let mut s = Session::new();
        let i = s.new_drawing(true);
        s.active = i;
        s.execute("wall.draw", &json!({ "points": [[0, 0], [8000, 0], [8000, 5000], [0, 5000]], "closed": true, "thickness": 230 })).unwrap();
        s.execute("wall.draw", &json!({ "points": [[4000, 0], [4000, 5000]], "thickness": 115 })).unwrap();
        s.execute("wall.door", &json!({ "at": [2000, 0], "width": 900 })).unwrap();
        s.execute("wall.door", &json!({ "at": [6000, 0], "width": 900 })).unwrap();
        s.execute("wall.window", &json!({ "at": [2000, 5000], "width": 1200 })).unwrap();
        s.execute("wall.window", &json!({ "at": [6000, 5000], "width": 1500 })).unwrap();
        let truth: Vec<(u64, String)> = s.doc().unwrap().model.iter().map(|e| (e.handle.0, e.common.layer.clone())).collect();
        let hs = s.doc().unwrap().model.handles();
        let d = s.doc_mut().unwrap();
        for h in hs {
            let _ = d.modify_entity(h, |e| e.common.layer = "0".into());
        }
        let names: Vec<String> = d.blocks.keys().cloned().collect();
        for n in names {
            if let Some(b) = d.blocks.get_mut(&n) {
                let b = Arc::make_mut(b);
                for h in b.entities.handles() {
                    b.entities.modify(h, |e| e.common.layer = "0".into());
                }
            }
        }
        let r = s.execute("geodb.build", &json!({})).unwrap();
        assert_eq!(r["needsUnitConfirmation"], json!(false), "{r}");
        let list = s.execute("geodb.report", &json!({ "detail": "features", "limit": 500 })).unwrap();
        let kinds = |k: &str| list["list"].as_array().unwrap().iter().filter(|f| f["kind"] == json!(k)).count();
        assert_eq!(kinds("door"), 2, "{list}");
        assert_eq!(kinds("window"), 2, "{list}");
        // Entity-level wall scores against the layers the commands used.
        let st = &s.geodb.values().next().unwrap().model;
        let min = st.rules.evidence.classify_min;
        let pred: std::collections::HashSet<u64> =
            st.features.iter().filter(|f| f.kind == "wall" && f.confidence >= min).flat_map(|f| f.members.iter().map(|m| m.handle)).collect();
        let truth_walls: std::collections::HashSet<u64> = truth.iter().filter(|(_, l)| l == "A-WALL").map(|(h, _)| *h).collect();
        let tp = pred.intersection(&truth_walls).count() as f64;
        let (p, rec) = (tp / pred.len().max(1) as f64, tp / truth_walls.len().max(1) as f64);
        eprintln!("wall precision {p:.3} recall {rec:.3} ({} predicted, {} true)", pred.len(), truth_walls.len());
        assert!(p >= 0.95 && rec >= 0.9, "precision {p} recall {rec}");
    }

    #[test]
    fn beams_and_columns_go_to_their_own_layers_after_a_preview_and_an_approval() {
        let mut s = Session::new();
        unstamp(&mut s);
        s.doc_mut().unwrap().header.set_i64("INSUNITS", 4);
        // A dashed pair (a beam above) and a plain wall pair.
        lines(
            &mut s,
            &[((0.0, 0.0), (5000.0, 0.0)), ((0.0, 230.0), (5000.0, 230.0)), ((0.0, 3000.0), (5000.0, 3000.0)), ((0.0, 3230.0), (5000.0, 3230.0))],
        );
        let hs = s.doc().unwrap().model.handles();
        let d = s.doc_mut().unwrap();
        for h in hs.iter().skip(2) {
            let _ = d.modify_entity(*h, |e| e.common.linetype = "DASHED".into());
        }
        stamp_new(s.doc_mut().unwrap(), true);
        s.doc_mut().unwrap().header.set_i64("INSUNITS", 4);
        let r = s.execute("geodb.build", &json!({})).unwrap();
        let kinds: Vec<String> = r["summary"]["features"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap_or("").to_string()).collect();
        assert!(kinds.iter().any(|k| k.starts_with("beam.")), "{kinds:?}");
        let pv = s.execute("geodb.apply", &json!({})).unwrap();
        let to: Vec<&str> = pv["preview"].as_array().unwrap().iter().filter_map(|r| r["to"].as_str()).collect();
        assert_eq!(to.iter().filter(|t| **t == "S-BEAM").count(), 2, "{pv}");
        assert_eq!(to.iter().filter(|t| **t == "A-WALL").count(), 2, "{pv}");
        assert!(s.doc().unwrap().layer("S-BEAM").is_none(), "a preview creates nothing");
        let ap = s.execute("geodb.apply", &json!({ "approve": true })).unwrap();
        assert_eq!(ap["moved"], json!(4));
        assert_eq!(s.doc().unwrap().model.iter().filter(|e| e.common.layer == "S-BEAM").count(), 2);
        assert!(s.doc().unwrap().layer("S-BEAM").is_some());
        // The report lists rooms and regions as numbers.
        let rep = s.execute("geodb.report", &json!({ "detail": "regions" })).unwrap();
        assert!(rep["list"].is_array(), "{rep}");
        assert!(s.execute("geodb.report", &json!({ "detail": "rooms" })).unwrap()["list"].is_array());
    }

    #[test]
    fn the_stamp_and_the_units_survive_a_save_and_read() {
        let mut s = Session::new();
        s.new_drawing(true);
        stamp_for_save(s.doc_mut().unwrap());
        let d = s.doc().unwrap().clone();
        let bytes = aad_io::write(&d, "x.dxf").unwrap();
        let back = aad_io::read(&bytes, "x.dxf").unwrap();
        assert!(origin::is_stamped(&back), "{:?}", back.project.keys().collect::<Vec<_>>());
        let u = units::declared_units(&back).unwrap();
        assert_eq!((u.unit.as_str(), u.source), ("mm", aad_geodb::UnitSource::Project));
        assert!(!origin_of(&back).foreign);
    }
}

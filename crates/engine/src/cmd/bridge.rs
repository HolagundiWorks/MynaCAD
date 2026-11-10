//! The AQC bridge: a JSON file ("hcw-aqc-bridge", version 1, millimetres) that carries a
//! drawing's measured data to the AQC estimation program and AQC's project details, floors and
//! beam depths back.
//!
//! `bridge.export` writes what the drawing knows: project details, floors, beam depths, the
//! opening schedule and the door and window blocks placed in the drawing, and the saved take-off
//! results for walls, columns, lintels and slabs, and the rooms. Rows carry a `ref` made from the
//! entity handle they were measured from, so it is the same on every run. Take-off sections that
//! were never run are reported, not guessed. Brick take-off walls carry `thickness_mm` measured from
//! parallel `A-WALL` face spacing when faces are present; otherwise the nominal layer value (230 /
//! 115) with a row note. Walls committed from the parametric graph (`param.commit`) carry the
//! graph's measured `thickness_mm` and a `ref` made from their instance id. Openings prefer
//! `wall_ref` from their `MEASURE-DEDUCT`↔measure-wall match (same as `takeoff.brick`); nearest
//! insert-point proximity is only a fallback and is flagged with `_note`.
//!
//! `bridge.import` reads AQC's file. It never changes geometry. Without `apply` it only reports what
//! would change; with `apply` it updates the project data. A project field already filled in is
//! left alone unless `overwrite` is set.

use aad_doc::EntityKind;
use serde_json::{Map, Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

const FORMAT: &str = "hcw-aqc-bridge";
const MAX_FILE: u64 = 5_000_000;
const MAX_FLOORS: usize = 60;
const STORE: &str = "bridge";
/// AAD project detail → bridge project key.
const PROJECT_MAP: [(&str, &str); 6] = [
    ("title", "name"),
    ("owner", "client_name"),
    ("architect", "company_name"),
    ("engineer", "prepared_by_name"),
    ("location", "location"),
    ("address", "address"),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("bridge.export", "Export for AQC", run_export)
            .menu(&["Architecture", "AQC Bridge", "Export for AQC"])
            .alias(&["aqcexport"])
            .params("{path?: file (default: beside the saved drawing), write?: false, level?: floor name for the measured items, bearing?: lintel bearing in mm (150)} → {json, path?, counts, warnings}"),
        CommandSpec::new("bridge.import", "Import from AQC", run_import)
            .menu(&["Architecture", "AQC Bridge", "Import from AQC"])
            .alias(&["aqcimport"])
            .params("{path | json, apply?: bool, overwrite?: bool, acceptOtherDrawing?: bool} → {items, summary, applied}"),
    ]
}

fn mm(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn text_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

// ---------------- identity and time ----------------

fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325 ^ seed;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A new drawing id in the shape of a version-4 UUID. It only has to be unlikely to repeat.
fn new_id(salt: &str) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    #[cfg(target_arch = "wasm32")]
    let nanos = 0u128;
    let a = fnv(nanos as u64, salt.as_bytes());
    let b = fnv((nanos >> 64) as u64 ^ a, std::process::id().to_le_bytes().as_slice());
    format!("{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}", (a >> 32) as u32, (a >> 16) as u16, a & 0xfff, (b >> 52) & 0xfff, b & 0xffff_ffff_ffff)
}

pub(crate) fn iso_time() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(iso_from_unix(secs))
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn iso_from_unix(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn stored_id(d: &aad_doc::Drawing) -> Option<String> {
    project::get(d, STORE).get("drawingId").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

// ---------------- export ----------------

fn saved(d: &aad_doc::Drawing, name: &str) -> Option<Value> {
    let v = project::get(d, &format!("takeoff:{name}"));
    if v.is_null() { None } else { Some(v) }
}

/// (millimetres per length unit, square metres per area unit) of a saved take-off.
fn factors(t: &Value) -> (f64, f64) {
    let len = if t["lengthUnit"].as_str() == Some("ft") { 304.8 } else { 1000.0 };
    let area = if t["areaUnit"].as_str().is_some_and(|u| u.contains("ft")) { 0.092_903_04 } else { 1.0 };
    (len, area)
}

/// One measured thing: what kind it is, its stable number, the row as the bridge file carries it, and its box
/// in millimetres in the drawing's coordinates (`[min x, min y, max x, max y]`; all zero when it has no geometry).
pub(crate) struct Item {
    pub kind: u8,
    pub n: u64,
    pub row: Value,
}

/// Everything `bridge.export` knows: the file's content as an object, what it could not include, and the rows with the
/// numbers a take-off record needs.
pub(crate) struct Collected {
    pub root: Map<String, Value>,
    pub warnings: Vec<String>,
    pub drawing_id: String,
    pub items: Vec<Item>,
    pub counts: Value,
}

/// The kinds of take-off record (MynaOps `OspTakeoff` / `.osp` takeoff rows).
pub(crate) const KIND_WALL: u8 = 3;
pub(crate) const KIND_OPENING: u8 = 4;
pub(crate) const KIND_COLUMN: u8 = 5;
pub(crate) const KIND_LINTEL: u8 = 6;
pub(crate) const KIND_ROOM: u8 = 7;
pub(crate) const KIND_SLAB: u8 = 8;
pub(crate) const KIND_ELEC_BOARD: u8 = 10;
pub(crate) const KIND_ELEC_CIRCUIT: u8 = 11;
pub(crate) const KIND_PLUMB_STACK: u8 = 12;
pub(crate) const KIND_PLUMB_FIXTURE: u8 = 13;

/// The number of an item in its record id: the low 56 bits of the entity handle.
fn number_of(h: aad_doc::Handle) -> u64 {
    h.0 & 0x00FF_FFFF_FFFF_FFFF
}

/// The box of an entity in millimetres (`unit_mm` millimetres per drawing unit), or zeros.
fn box_mm(d: &aad_doc::Drawing, h: Option<aad_doc::Handle>, unit_mm: f64) -> [f64; 4] {
    let Some(e) = h.and_then(|h| d.entity(h)) else { return [0.0; 4] };
    let b = aad_doc::entity_bounds(d, e, 0);
    let v = [b.min.x * unit_mm, b.min.y * unit_mm, b.max.x * unit_mm, b.max.y * unit_mm];
    if b.is_empty() || v.iter().any(|x| !x.is_finite()) { [0.0; 4] } else { [mm(v[0]), mm(v[1]), mm(v[2]), mm(v[3])] }
}

/// Nominal thickness of a wall measured on the full-brick and half-brick layers, in millimetres.
const FULL_BRICK_MM: f64 = 230.0;
const HALF_BRICK_MM: f64 = 115.0;

/// The distance from a point to a measured line or polyline (straight segments), in drawing units.
fn distance_to_entity(d: &aad_doc::Drawing, h: aad_doc::Handle, p: aad_geom::Vec2) -> Option<f64> {
    let (pts, closed): (Vec<aad_geom::Vec2>, bool) = match &d.entity(h)?.kind {
        EntityKind::Line(l) => (vec![aad_geom::Vec2::new(l.a.x, l.a.y), aad_geom::Vec2::new(l.b.x, l.b.y)], false),
        EntityKind::LwPolyline(pl) => (pl.vertices.iter().map(|v| v.p).collect(), pl.closed),
        _ => return None,
    };
    let seg = |a: aad_geom::Vec2, b: aad_geom::Vec2| {
        let ab = b - a;
        let len2 = ab.x * ab.x + ab.y * ab.y;
        let t = if len2 > 0.0 { (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let q = aad_geom::Vec2::new(a.x + ab.x * t, a.y + ab.y * t);
        ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt()
    };
    let mut best: Option<f64> = None;
    let mut take = |x: f64| {
        if x.is_finite() {
            best = Some(best.map_or(x, |b| b.min(x)));
        }
    };
    for w in pts.windows(2) {
        take(seg(w[0], w[1]));
    }
    if closed && pts.len() > 2 {
        take(seg(pts[pts.len() - 1], pts[0]));
    }
    if pts.len() == 1 {
        take(seg(pts[0], pts[0]));
    }
    best
}

/// A rectangle's two spans in millimetres, when the outline is four right angles with straight sides.
fn rectangle_spans_mm(d: &aad_doc::Drawing, h: aad_doc::Handle, unit_mm: f64) -> Option<(f64, f64)> {
    let EntityKind::LwPolyline(pl) = &d.entity(h)?.kind else { return None };
    let mut v: Vec<aad_geom::Vec2> = pl.vertices.iter().map(|p| p.p).collect();
    if pl.vertices.iter().any(|p| p.bulge != 0.0) {
        return None;
    }
    if v.len() == 5 && v.first() == v.last() {
        v.pop();
    }
    if v.len() != 4 || !(pl.closed || pl.vertices.len() == 5) {
        return None;
    }
    let side = |i: usize| (v[(i + 1) % 4] - v[i], v[(i + 1) % 4].dist(v[i]));
    let (mut spans, mut right) = (Vec::new(), true);
    for i in 0..4 {
        let (a, la) = side(i);
        let (b, lb) = side((i + 1) % 4);
        if !(la.is_finite() && lb.is_finite()) || la <= 0.0 || lb <= 0.0 || (a.x * b.x + a.y * b.y).abs() > 1e-6 * la * lb {
            right = false;
        }
        spans.push(la);
    }
    right.then(|| (spans[0].max(spans[1]) * unit_mm, spans[0].min(spans[1]) * unit_mm))
}

fn run_export(s: &mut Session, p: &Value) -> Result<Value> {
    let c = collect(s, p)?;
    let (id, warnings) = (c.drawing_id, c.warnings);
    let text = serde_json::to_string_pretty(&Value::Object(c.root)).map_err(|e| bad("bridge.export", e.to_string()))?;
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut out = json!({
        "json": text,
        "drawingId": id,
        "counts": c.counts,
        "warnings": warnings,
    });
    #[cfg(target_arch = "wasm32")]
    {
        if str_param(p, "path").is_some() {
            return Err(bad("bridge.export", "files cannot be written on the web; read `json` from the result"));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Beside the saved drawing (`house.dxf` → `house.aqc.json`) unless `path` says otherwise or
        // `write` is false.
        let target = match str_param(p, "path") {
            Some(t) => Some(std::path::PathBuf::from(t)),
            None if bool_or(p, "write", true) => s.state()?.path.as_ref().map(|b| std::path::Path::new(b).with_extension("aqc.json")),
            None => None,
        };
        if let Some(path) = target {
            std::fs::write(&path, out["json"].as_str().unwrap_or(""))
                .map_err(|e| bad("bridge.export", format!("cannot write `{}`: {e}", path.display())))?;
            out["path"] = json!(path.display().to_string());
            out["message"] = json!(format!("Exported for AQC: {}", path.display()));
        }
    }
    Ok(out)
}

/// Collect what the drawing knows for the AQC hand-off: the bridge file's content and the rows with their
/// stable numbers and boxes. `bridge.export` writes it as a file; `takeoff.record` writes it as records in the project file.
pub(crate) fn collect(s: &mut Session, p: &Value) -> Result<Collected> {
    let bearing = f64_or(p, "bearing", 150.0);
    if !(bearing.is_finite() && bearing > 0.0 && bearing < 2000.0) {
        return Err(bad("bridge.export", "`bearing` must be between 0 and 2000 mm"));
    }
    let id = match stored_id(s.doc()?) {
        Some(id) => id,
        None => {
            let id = new_id(&format!("{:?}", s.state()?.path));
            project::set(s.doc_mut()?, STORE, &json!({ "drawingId": id }));
            id
        }
    };
    let d = s.doc()?;
    // Millimetres in one drawing unit, for the boxes.
    let unit_mm = super::hcw::drawing_units(d, "bridge.export", &Value::Null)?.metres() * 1000.0;
    let mut items: Vec<Item> = Vec::new();
    let floors = project::floors(d);
    let level_explicit = str_param(p, "level").is_some();
    // Prefer an explicit `level`, then the active drawing level, then the first floor name.
    let level = str_param(p, "level")
        .map(str::to_string)
        .or_else(|| d.active_level_ref().map(|l| l.name.clone()))
        .or_else(|| floors.first().and_then(|f| f["name"].as_str()).map(str::to_string))
        .unwrap_or_else(|| "Ground".into());
    let ceiling_of = |name: &str| -> f64 {
        floors.iter().find(|f| f["name"].as_str() == Some(name)).or(floors.first()).and_then(|f| f["ceiling"].as_f64()).map_or(2850.0, |c| c * 1000.0)
    };
    // Prefer a floor region assignment (`entityFloors`) for the entity; else the export-wide `level`.
    let level_of = |h: Option<aad_doc::Handle>| -> String { h.and_then(|h| super::floorregion::floor_of(d, h)).unwrap_or_else(|| level.clone()) };
    let mut warnings: Vec<String> = Vec::new();
    let region_floors = super::floorregion::assigned_floor_count(d);
    if floors.len() > 1 && !level_explicit && region_floors == 0 {
        warnings.push(format!(
            "drawing has {} floors; all measured rows are stamped as `{level}` (active level / first floor). Pass `level` explicitly or declare floor regions (`floor.declare`) if that is wrong",
            floors.len()
        ));
    } else if region_floors > 0 && !level_explicit {
        warnings.push(format!("floor regions assign entities to {region_floors} floor(s); unmapped measured rows still use `{level}`"));
    }
    // Take-off rows saved without an entity handle: their `ref` is a position, which can change when the take-off is run again.
    let mut unstable = 0usize;
    let mut root = Map::new();
    root.insert("format".into(), json!(FORMAT));
    root.insert("version".into(), json!(1));
    let mut source = json!({ "app": "AAD", "host": "AAD", "drawing_id": id, "drawing": "(unsaved)" });
    if let Some(path) = &s.state()?.path {
        source["drawing"] = json!(std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path));
    }
    if let Some(t) = iso_time() {
        source["exported"] = json!(t);
    }
    // The drawing revision this export was made from: the hash recorded when the file was saved,
    // given only while the file on disk still matches it.
    #[cfg(not(target_arch = "wasm32"))]
    match s.state()?.path.as_deref().map(crate::revision::check) {
        Some(Ok(c)) if c["status"] == "current" => source["revision"] = c["recorded"].clone(),
        Some(Ok(_)) => warnings.push("the drawing file has no matching saved revision: save the drawing so the export names it".into()),
        Some(Err(_)) => warnings.push("the drawing file could not be read to record its revision".into()),
        None => warnings.push("the drawing has not been saved, so the export names no revision".into()),
    }
    {
        let st = s.state()?;
        if !std::sync::Arc::ptr_eq(&st.doc, &st.saved) {
            warnings.push("the drawing has unsaved changes: this export reflects them, the saved revision does not".into());
        }
    }
    // The project in the company file that this drawing was linked to (`companyfile`): MynaOps matches the take-off to its project by it.
    if let Some(reference) = super::companyfile::company(s.doc()?).get("project_ref").filter(|r| r.is_object()) {
        source["project_ref"] = reference.clone();
    }
    root.insert("source".into(), source);
    root.insert("units".into(), json!("mm"));
    let rooms = super::roomtools::rooms_for_export(d, &s.space());
    if rooms.is_empty() {
        warnings.push("no room labels: run `room` to include rooms".into());
    }
    // Each room carries its floor and its two sides: the exact sides of a rectangular outline, else the sides of the outline's
    // box with `rectangular: false`. A room with no drawn outline sends its area only.
    let mut room_rows = Vec::new();
    let mut no_outline = 0usize;
    for r in &rooms {
        let mut row = r.row.clone();
        row["level"] = json!(level_of(r.outline.or(Some(r.handle))));
        super::classify::stamp_building_on_row(d, r.outline.or(Some(r.handle)), &mut row);
        match r.outline {
            Some(h) => match rectangle_spans_mm(d, h, unit_mm) {
                Some((length, breadth)) => {
                    row["length_mm"] = json!(mm(length));
                    row["breadth_mm"] = json!(mm(breadth));
                    row["rectangular"] = json!(true);
                }
                None => {
                    let b = box_mm(d, Some(h), unit_mm);
                    let (w, ht) = (b[2] - b[0], b[3] - b[1]);
                    row["length_mm"] = json!(mm(w.max(ht)));
                    row["breadth_mm"] = json!(mm(w.min(ht)));
                    row["rectangular"] = json!(false);
                }
            },
            None => no_outline += 1,
        }
        items.push(Item { kind: KIND_ROOM, n: number_of(r.handle), row: row.clone() });
        room_rows.push(row);
    }
    if no_outline > 0 {
        warnings.push(format!("{no_outline} room(s) have no drawn outline on ROOM-RECT, so their sides are not sent (only the area)"));
    }
    root.insert("rooms".into(), Value::Array(room_rows));

    // Project details.
    let details = project::get(d, "details");
    let mut proj = Map::new();
    for (ours, theirs) in PROJECT_MAP {
        let v = text_of(&details, ours);
        if !v.is_empty() {
            proj.insert(theirs.into(), json!(v));
        }
    }
    for (ours, theirs) in [("pid", "pid"), ("plotUse", "plot_use")] {
        let v = text_of(&details, ours);
        if !v.is_empty() {
            proj.insert(theirs.into(), json!(v));
        }
    }
    if let Ok(a) = text_of(&details, "siteArea").parse::<f64>()
        && a.is_finite()
    {
        proj.insert("site_area_m2".into(), json!(a));
    }
    if !proj.is_empty() {
        root.insert("project".into(), Value::Object(proj));
    }

    // Floors and beam depths.
    let levels: Vec<Value> = floors
        .iter()
        .map(|f| {
            json!({
                "name": f["name"], "height_mm": mm(f["floorToFloor"].as_f64().unwrap_or(0.0) * 1000.0),
                "slab_thickness_mm": f["slab"], "beam_depth_mm": f["beam"],
                "ceiling_mm": mm(f["ceiling"].as_f64().unwrap_or(0.0) * 1000.0), "lintel_bottom_mm": mm(f["lintel"].as_f64().unwrap_or(0.0) * 1000.0),
            })
        })
        .collect();
    root.insert("levels".into(), Value::Array(levels));
    let beams = project::get(d, "beams");
    if beams.as_array().is_some_and(|a| !a.is_empty()) {
        root.insert("beam_depths_mm".into(), beams);
    }

    // The opening schedule and the blocks placed in the drawing.
    let sched = project::get(d, "schedule");
    let entries: Vec<Value> = sched["openings"].as_array().cloned().unwrap_or_default();
    let kind_name = |k: &str| match k {
        "door" => "Door",
        "window" => "Window",
        // A ventilator is a window to the take-off contract; its mark and type still name it.
        _ => "Window",
    };
    let mut counts_by_block = std::collections::BTreeMap::<String, u64>::new();
    let mut inserts = Vec::new();
    for e in d.model.iter().filter(|e| d.is_visible(e)) {
        if let EntityKind::Insert(i) = &e.kind {
            let b = i.block.to_ascii_uppercase();
            *counts_by_block.entry(b.clone()).or_insert(0) += 1;
            inserts.push((e.handle, b));
        }
    }
    let entry_of = |block: &str| -> Option<&Value> {
        entries.iter().find(|e| text_of(e, "block").split(';').map(str::trim).any(|b| !b.is_empty() && b.eq_ignore_ascii_case(block)))
    };
    let mut schedule_rows = Vec::new();
    for e in &entries {
        let nos: u64 = text_of(e, "block")
            .split(';')
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(|b| counts_by_block.get(&b.to_ascii_uppercase()).copied().unwrap_or(0))
            .sum::<u64>()
            .max(e["count"].as_u64().unwrap_or(0));
        let mut row = json!({
            "mark": text_of(e, "name"), "kind": kind_name(&text_of(e, "kind")),
            "width_mm": mm(e["length"].as_f64().unwrap_or(0.0) * 1000.0), "height_mm": mm(e["height"].as_f64().unwrap_or(0.0) * 1000.0),
            "sill_mm": mm(e["sill"].as_f64().unwrap_or(0.0) * 1000.0), "nos": nos,
        });
        if !text_of(e, "type").is_empty() {
            row["type"] = json!(text_of(e, "type"));
        }
        schedule_rows.push(row);
    }
    let mut opening_rows = Vec::new();
    for (handle, b) in &inserts {
        let Some(e) = entry_of(b) else { continue };
        let mut row = json!({
            "ref": format!("O{}", handle.hex()), "kind": kind_name(&text_of(e, "kind")), "mark": text_of(e, "name"), "level": level_of(Some(*handle)),
            "width_mm": mm(e["length"].as_f64().unwrap_or(0.0) * 1000.0), "height_mm": mm(e["height"].as_f64().unwrap_or(0.0) * 1000.0),
            "sill_mm": mm(e["sill"].as_f64().unwrap_or(0.0) * 1000.0),
        });
        if !text_of(e, "type").is_empty() {
            row["type"] = json!(text_of(e, "type"));
        }
        super::takeoffphase::stamp_row(d, Some(*handle), &mut row);
        super::classify::stamp_building_on_row(d, Some(*handle), &mut row);
        items.push(Item { kind: KIND_OPENING, n: number_of(*handle), row: row.clone() });
        opening_rows.push(row);
    }
    if schedule_rows.is_empty() {
        warnings.push("no opening schedule: run `openings.std` and place doors and windows to include openings".into());
    }
    root.insert("opening_schedule".into(), Value::Array(schedule_rows.clone()));
    root.insert("openings".into(), Value::Array(opening_rows.clone()));

    // Walls from the brick take-off.
    let space = s.space();
    let mut walls = Vec::new();
    let mut nominal_thickness = 0usize;
    if let Some(t) = saved(d, "Bricks") {
        let (lf, _) = factors(&t);
        for it in t["items"].as_array().into_iter().flatten() {
            let (Some(mark), Some(len)) = (it["mark"].as_str(), it["length"].as_f64()) else { continue };
            let half = it["group"].as_str().is_some_and(|g| g.starts_with("HB")) || it["wallType"].as_str() == Some("internal");
            let layer = if half { "MEASURE-HALFBRICK" } else { "MEASURE-FULLBRICK" };
            let handle = stable_handle(it);
            let wall_ref = handle.map_or_else(|| mark.to_string(), |h| format!("W{}", h.hex()));
            let nominal = if half { HALF_BRICK_MM } else { FULL_BRICK_MM };
            let from_mark = it["thicknessMm"].as_f64().filter(|t| t.is_finite() && *t > 0.0);
            let measured = handle.and_then(|h| super::takeoff::thickness_from_faces_mm(d, &space, h, unit_mm));
            // Prefer A-WALL face spacing; otherwise mark / layer nominal.
            let thickness = measured.or(from_mark).unwrap_or(nominal);
            let wall_type = it["wallType"].as_str().unwrap_or(if half { "internal" } else { "external" });
            let paint = saved(d, "WallPaint").and_then(|t| {
                t["items"].as_array().and_then(|a| a.iter().find(|p| handle.is_some_and(|h| p["handle"].as_str() == Some(&h.hex()))).cloned())
            });
            let wall_level = level_of(handle);
            let mut row = json!({
                "ref": wall_ref, "level": wall_level, "length_mm": mm(len * lf), "height_mm": mm(ceiling_of(&wall_level)),
                "thickness_mm": thickness, "layer": layer, "wall_type": wall_type,
            });
            if measured.is_none() {
                nominal_thickness += 1;
                row["_note"] =
                    json!(format!("thickness {nominal} mm is the nominal value the measure layer implies, not a measured thickness; check it"));
            }
            super::classify::stamp_building_on_row(d, handle, &mut row);
            if let Some(p) = paint {
                if let Some(v) = p["externalPaint"].as_f64() {
                    row["paint_external_m2"] = json!((v * 100.0).round() / 100.0);
                }
                if let Some(v) = p["internalPaint"].as_f64() {
                    row["paint_internal_m2"] = json!((v * 100.0).round() / 100.0);
                }
            }
            if let Some(h) = handle {
                super::takeoffphase::stamp_row(d, Some(h), &mut row);
                items.push(Item { kind: KIND_WALL, n: number_of(h), row: row.clone() });
            } else {
                unstable += 1;
            }
            walls.push(row);
        }
    }
    // Walls committed from the parametric graph.
    let (param_walls, param_warnings) = super::param::takeoff_walls(d, &level);
    warnings.extend(param_warnings);
    if walls.is_empty() && param_walls.is_empty() {
        warnings.push("no saved brick take-off: run `takeoff.brick` to include walls".into());
    }
    for w in param_walls {
        let Some(first) = w.handles.first() else { continue };
        items.push(Item { kind: KIND_WALL, n: number_of(*first), row: w.row.clone() });
        walls.push(w.row);
    }
    if nominal_thickness > 0 {
        warnings.push(format!(
            "{nominal_thickness} brick wall(s) use the nominal thickness the measure layer implies (full brick {FULL_BRICK_MM} mm, half brick {HALF_BRICK_MM} mm); no A-WALL face spacing was measured"
        ));
    }
    root.insert("walls".into(), Value::Array(walls.clone()));

    // Which measured wall each opening sits in: prefer MEASURE-DEDUCT↔wall (brick take-off), else nearest insert point.
    {
        let wall_list: Vec<(String, aad_doc::Handle, f64)> = items
            .iter()
            .filter(|i| i.kind == KIND_WALL)
            .filter_map(|i| Some((i.row["ref"].as_str()?.to_string(), aad_doc::Handle(i.n), i.row["thickness_mm"].as_f64().unwrap_or(FULL_BRICK_MM))))
            .collect();
        let wall_handles: Vec<_> = wall_list.iter().map(|(_, h, _)| *h).collect();
        let tol = 10.0; // drawing units — same default as takeoff.brick
        let by_deduct = super::takeoff::opening_wall_by_deduct(d, &space, &wall_handles, tol);
        let mut unplaced = 0usize;
        let mut proximity = 0usize;
        for it in items.iter_mut().filter(|i| i.kind == KIND_OPENING) {
            if let Some(wh) = by_deduct.get(&it.n)
                && let Some((r, _, _)) = wall_list.iter().find(|(_, h, _)| *h == *wh)
            {
                it.row["wall_ref"] = json!(r);
                // An opening is measured in its wall's phases unless it was given its own.
                if it.row.get("phase").is_none() && it.row.get("bill_phase").is_none() {
                    super::takeoffphase::stamp_row(d, Some(*wh), &mut it.row);
                }
                continue;
            }
            let Some(EntityKind::Insert(ins)) = d.entity(aad_doc::Handle(it.n)).map(|e| &e.kind) else { continue };
            let at = aad_geom::Vec2::new(ins.insert.x, ins.insert.y);
            let best = wall_list
                .iter()
                .filter_map(|(r, h, t)| distance_to_entity(d, *h, at).map(|dist| (dist * unit_mm, r, *t, *h)))
                .filter(|(dist, _, t, _)| *dist <= t / 2.0 + 150.0)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match best {
                Some((_, r, _, wh)) => {
                    it.row["wall_ref"] = json!(r);
                    if it.row.get("phase").is_none() && it.row.get("bill_phase").is_none() {
                        super::takeoffphase::stamp_row(d, Some(wh), &mut it.row);
                    }
                    it.row["_note"] = json!("wall chosen by nearest measured line (opening has no MEASURE-DEDUCT match); check it");
                    proximity += 1;
                }
                None => unplaced += 1,
            }
        }
        if proximity > 0 {
            warnings.push(format!("{proximity} opening(s) got wall_ref from insert-point proximity (no MEASURE-DEDUCT match); check them"));
        }
        if unplaced > 0 {
            warnings.push(format!(
                "{unplaced} opening(s) are not on a measured wall (none within its half thickness and 150 mm): no wall_ref is sent for them"
            ));
        }
        let rows: Vec<Value> = items.iter().filter(|i| i.kind == KIND_OPENING).map(|i| i.row.clone()).collect();
        root.insert("openings".into(), Value::Array(rows));
    }

    // Columns.
    let mut columns = Vec::new();
    match saved(d, "Columns") {
        Some(t) => {
            let (lf, _) = factors(&t);
            for (n, it) in t["items"].as_array().into_iter().flatten().enumerate() {
                let (Some(mark), Some(l), Some(b)) = (it["mark"].as_str(), it["length"].as_f64(), it["breadth"].as_f64()) else { continue };
                let (w, dep) = (l.min(b) * lf, l.max(b) * lf);
                let handle = stable_handle(it);
                let col_ref = handle.map_or_else(|| format!("C{:04}", n + 1), |h| format!("C{}", h.hex()));
                let mut row = json!({ "ref": col_ref, "mark": mark, "level": level_of(handle), "width_mm": mm(w), "depth_mm": mm(dep) });
                if let Some(h) = handle {
                    super::takeoffphase::stamp_row(d, Some(h), &mut row);
                    super::classify::stamp_building_on_row(d, Some(h), &mut row);
                    items.push(Item { kind: KIND_COLUMN, n: number_of(h), row: row.clone() });
                } else {
                    unstable += 1;
                }
                columns.push(row);
            }
        }
        None => warnings.push("no saved column take-off: run `takeoff.columns` to include columns".into()),
    }
    root.insert("columns".into(), Value::Array(columns.clone()));

    // Lintels: the take-off group letters become marks L1, L2 ...
    let mut lintels = Vec::new();
    match saved(d, "Beams") {
        Some(t) => {
            let (lf, _) = factors(&t);
            let mut groups: Vec<String> = Vec::new();
            for it in t["items"].as_array().into_iter().flatten() {
                let (Some(group), Some(len)) = (it["group"].as_str(), it["length"].as_f64()) else { continue };
                if !group.starts_with("LT") {
                    continue;
                }
                let g = groups.iter().position(|x| x == group).unwrap_or_else(|| {
                    groups.push(group.to_string());
                    groups.len() - 1
                });
                let handle = stable_handle(it);
                let lintel_ref = handle.map_or_else(|| format!("L{:04}", lintels.len() + 1), |h| format!("L{}", h.hex()));
                let mut row = json!({
                    "ref": lintel_ref, "mark": format!("L{}", g + 1), "level": level_of(handle),
                    "opening_mm": mm((len * lf - 2.0 * bearing).max(0.0)), "bearing_mm": bearing,
                });
                if let Some(h) = handle {
                    super::takeoffphase::stamp_row(d, Some(h), &mut row);
                    super::classify::stamp_building_on_row(d, Some(h), &mut row);
                    items.push(Item { kind: KIND_LINTEL, n: number_of(h), row: row.clone() });
                } else {
                    unstable += 1;
                }
                lintels.push(row);
            }
        }
        None => warnings.push("no saved beam and lintel take-off: run `takeoff.beams` to include lintels".into()),
    }
    root.insert("lintels".into(), Value::Array(lintels.clone()));

    // Slabs.
    let slab_of = |name: &str| -> f64 {
        floors.iter().find(|f| f["name"].as_str() == Some(name)).or(floors.first()).and_then(|f| f["slab"].as_f64()).unwrap_or(150.0)
    };
    let mut slabs = Vec::new();
    match saved(d, "Slab") {
        Some(t) => {
            let (_, af) = factors(&t);
            for (n, it) in t["items"].as_array().into_iter().flatten().enumerate() {
                let Some(gross) = it["gross"].as_f64() else { continue };
                let handle = stable_handle(it);
                let slab_ref = handle.map_or_else(|| format!("S{:04}", n + 1), |h| format!("S{}", h.hex()));
                let slab_level = level_of(handle);
                let mut row = json!({
                    "ref": slab_ref, "level": slab_level, "area_m2": (gross * af * 1000.0).round() / 1000.0, "thickness_mm": slab_of(&slab_level),
                });
                // A rectangular slab carries its two spans; any other shape keeps only its area.
                if let Some((length, breadth)) = handle.and_then(|h| rectangle_spans_mm(d, h, unit_mm)) {
                    row["length_mm"] = json!(mm(length));
                    row["breadth_mm"] = json!(mm(breadth));
                    row["rectangular"] = json!(true);
                }
                if let Some(h) = handle {
                    super::takeoffphase::stamp_row(d, Some(h), &mut row);
                    super::classify::stamp_building_on_row(d, Some(h), &mut row);
                    items.push(Item { kind: KIND_SLAB, n: number_of(h), row: row.clone() });
                } else {
                    unstable += 1;
                }
                slabs.push(row);
            }
        }
        None => warnings.push("no saved slab take-off: run `takeoff.slab` to include slabs".into()),
    }
    root.insert("slabs".into(), Value::Array(slabs.clone()));

    // Electrical switchboard table and one row per wired point (kinds 10 and 11).
    let elec = project::get(d, "electrical");
    let mut boards = Vec::new();
    let mut circuits = Vec::new();
    for (i, row) in elec["wires"].as_array().into_iter().flatten().enumerate() {
        let mut row = row.clone();
        if row.get("level").is_none() {
            row["level"] = json!(level);
        }
        let n = row["handle"].as_str().and_then(aad_doc::Handle::parse_hex).map(number_of).unwrap_or(i as u64 + 1);
        items.push(Item { kind: KIND_ELEC_BOARD, n, row: row.clone() });
        boards.push(row);
    }
    for (i, row) in elec["circuits"].as_array().into_iter().flatten().enumerate() {
        let mut row = row.clone();
        if row.get("level").is_none() {
            row["level"] = json!(level);
        }
        let n = row["handle"].as_str().and_then(aad_doc::Handle::parse_hex).map(number_of).unwrap_or(i as u64 + 1);
        items.push(Item { kind: KIND_ELEC_CIRCUIT, n, row: row.clone() });
        circuits.push(row);
    }
    if !boards.is_empty() {
        root.insert("electrical".into(), Value::Array(boards.clone()));
    }
    if !circuits.is_empty() {
        root.insert("electrical_circuits".into(), Value::Array(circuits));
    }

    // Plumbing: a row per source and service (kind 12) and one per fixture (kind 13), from `plumbing.connect`.
    let plumb = project::get(d, "plumbing");
    for (key, kind, section) in [("stacks", KIND_PLUMB_STACK, "plumbing"), ("fixtures", KIND_PLUMB_FIXTURE, "plumbing_fixtures")] {
        let mut rows = Vec::new();
        for (i, row) in plumb[key].as_array().into_iter().flatten().enumerate() {
            let mut row = row.clone();
            if row.get("level").is_none() {
                row["level"] = json!(level);
            }
            let n = row["handle"].as_str().and_then(aad_doc::Handle::parse_hex).map(number_of).unwrap_or(i as u64 + 1);
            // A source appears once per service, so its number also takes the service.
            let n = if kind == KIND_PLUMB_STACK { (n.wrapping_mul(2) + u64::from(row["service"] == "DRN")) & 0x00FF_FFFF_FFFF_FFFF } else { n };
            items.push(Item { kind, n, row: row.clone() });
            rows.push(row);
        }
        if !rows.is_empty() {
            root.insert(section.into(), Value::Array(rows));
        }
    }

    if unstable > 0 {
        warnings.push(format!("{unstable} take-off row(s) have no entity handle, so their `ref` is a position and can change when the take-off is run again: run the take-off again"));
    }
    let counts = json!({
        "walls": walls.len(), "openings": opening_rows.len(), "openingSchedule": schedule_rows.len(),
        "columns": columns.len(), "lintels": lintels.len(), "slabs": slabs.len(),
        "rooms": items.iter().filter(|i| i.kind == KIND_ROOM).count(),
        "electrical": boards.len(), "electricalCircuits": items.iter().filter(|i| i.kind == KIND_ELEC_CIRCUIT).count(),
        "plumbing": items.iter().filter(|i| i.kind == KIND_PLUMB_STACK).count(), "plumbingFixtures": items.iter().filter(|i| i.kind == KIND_PLUMB_FIXTURE).count(),
    });
    Ok(Collected { root, warnings, drawing_id: id, items, counts })
}

/// The entity handle a saved take-off item was measured from: what keeps its `ref` the same when the take-off is run again.
fn stable_handle(item: &Value) -> Option<aad_doc::Handle> {
    item["handle"].as_str().and_then(aad_doc::Handle::parse_hex)
}

// ---------------- import ----------------

struct Plan {
    items: Vec<Value>,
}

impl Plan {
    fn add(&mut self, section: &str, action: &str, field: &str, from: Value, to: Value, reason: &str) {
        self.items.push(json!({ "section": section, "action": action, "field": field, "from": from, "to": to, "reason": reason }));
    }
    fn count(&self, action: &str) -> usize {
        self.items.iter().filter(|i| i["action"] == action).count()
    }
}

fn read_input(p: &Value) -> Result<String> {
    if let Some(j) = str_param(p, "json") {
        if j.len() as u64 > MAX_FILE {
            return Err(bad("bridge.import", "the bridge text is too large"));
        }
        return Ok(j.to_string());
    }
    let Some(path) = str_param(p, "path") else {
        return Err(bad("bridge.import", "give `path` (an AQC export file) or `json`"));
    };
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        Err(bad("bridge.import", "files cannot be read on the web; pass the text as `json`"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let meta = std::fs::metadata(path).map_err(|e| bad("bridge.import", format!("cannot read `{path}`: {e}")))?;
        if meta.len() > MAX_FILE {
            return Err(bad("bridge.import", "the file is too large for a bridge file"));
        }
        std::fs::read_to_string(path).map_err(|e| bad("bridge.import", format!("cannot read `{path}`: {e}")))
    }
}

fn pos(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0)
}

fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    let src = read_input(p)?;
    let root: Value = serde_json::from_str(&src).map_err(|e| bad("bridge.import", format!("not valid JSON: {e}")))?;
    if root["format"].as_str() != Some(FORMAT) {
        return Err(bad("bridge.import", "not an hcw-aqc-bridge file"));
    }
    if root["version"].as_f64() != Some(1.0) {
        return Err(bad("bridge.import", "unsupported bridge version (this reads version 1)"));
    }
    if !root["units"].as_str().is_some_and(|u| u.eq_ignore_ascii_case("mm")) {
        return Err(bad("bridge.import", "unsupported units (expected mm)"));
    }
    let file_id = root["source"]["drawing_id"]
        .as_str()
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .ok_or_else(|| bad("bridge.import", "source.drawing_id is missing"))?
        .to_string();
    let apply = bool_or(p, "apply", false);
    let overwrite = bool_or(p, "overwrite", false);
    let known = stored_id(s.doc()?);
    let other = known.as_ref().is_some_and(|k| *k != file_id);
    if apply && other && !bool_or(p, "acceptOtherDrawing", false) {
        return Err(bad("bridge.import", "this file came from a different drawing; check it, then set `acceptOtherDrawing`"));
    }

    let d = s.doc()?;
    let mut plan = Plan { items: Vec::new() };

    // Project details.
    let mut details = match project::get(d, "details") {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    if let Some(proj) = root["project"].as_object() {
        for (ours, theirs) in PROJECT_MAP {
            let Some(new) = proj.get(theirs).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) else { continue };
            if new.len() > 500 {
                plan.add("project", "flagged", ours, Value::Null, json!(new.len()), "too long, skipped");
                continue;
            }
            let old = details.get(ours).and_then(Value::as_str).unwrap_or("").to_string();
            if old == new {
                plan.add("project", "unchanged", ours, json!(old), json!(new), "");
            } else if old.is_empty() {
                plan.add("project", "add", ours, Value::Null, json!(new), "");
                details.insert(ours.into(), json!(new));
            } else if overwrite {
                plan.add("project", "update", ours, json!(old), json!(new), "overwritten");
                details.insert(ours.into(), json!(new));
            } else {
                plan.add("project", "flagged", ours, json!(old), json!(new), "already filled in AAD; set `overwrite` to replace it");
            }
        }
    }

    // Floors.
    let mut floors = project::floors(d);
    if let Some(levels) = root["levels"].as_array() {
        for l in levels.iter().take(MAX_FLOORS) {
            let name = text_of(l, "name");
            if name.is_empty() || name.chars().count() > 60 {
                plan.add("floors", "flagged", &name, Value::Null, Value::Null, "needs a name of up to 60 characters");
                continue;
            }
            let (Some(h), slab, beam) = (pos(l, "height_mm").filter(|h| *h <= 20_000.0), pos(l, "slab_thickness_mm"), pos(l, "beam_depth_mm")) else {
                plan.add("floors", "flagged", &name, Value::Null, Value::Null, "height must be between 0 and 20 000 mm");
                continue;
            };
            if slab.is_some_and(|x| x > 1000.0) || beam.is_some_and(|x| x > 3000.0) {
                plan.add("floors", "flagged", &name, Value::Null, Value::Null, "slab or beam depth is out of range");
                continue;
            }
            let height = h / 1000.0;
            match floors.iter().position(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&name))) {
                Some(i) => {
                    let ceiling = floors[i]["ceiling"].as_f64().unwrap_or(0.0);
                    if height <= ceiling {
                        plan.add(
                            "floors",
                            "flagged",
                            &name,
                            floors[i]["floorToFloor"].clone(),
                            json!(height),
                            "floor-to-floor height would be below the ceiling height",
                        );
                        continue;
                    }
                    let mut next = floors[i].clone();
                    next["floorToFloor"] = json!(height);
                    if let Some(x) = slab {
                        next["slab"] = json!(x);
                    }
                    if let Some(x) = beam {
                        next["beam"] = json!(x);
                    }
                    if next == floors[i] {
                        plan.add("floors", "unchanged", &name, floors[i]["floorToFloor"].clone(), json!(height), "");
                    } else {
                        plan.add("floors", "update", &name, floors[i]["floorToFloor"].clone(), json!(height), "");
                        floors[i] = next;
                    }
                }
                None => {
                    if floors.len() >= MAX_FLOORS {
                        plan.add("floors", "flagged", &name, Value::Null, json!(height), "too many floors");
                        continue;
                    }
                    let slab_m = slab.unwrap_or(150.0) / 1000.0;
                    let ceiling = ((height - slab_m) * 100.0).round() / 100.0;
                    floors.push(json!({ "name": name, "floorToFloor": height, "ceiling": ceiling, "lintel": ceiling.min(2.1), "slab": slab.unwrap_or(150.0), "beam": beam.unwrap_or(450.0) }));
                    plan.add("floors", "add", &name, Value::Null, json!(height), "");
                }
            }
        }
    }

    // Beam depths.
    let mut beams: Option<Vec<Value>> = None;
    if let Some(list) = root["beam_depths_mm"].as_array() {
        let depths: Vec<f64> = list.iter().filter_map(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0 && *x <= 3000.0).take(50).collect();
        if depths.is_empty() {
            plan.add("beams", "flagged", "beam_depths_mm", Value::Null, Value::Null, "no valid depths");
        } else {
            let new: Vec<Value> = depths.iter().map(|x| json!(x)).collect();
            let old = project::get(d, "beams");
            if old == Value::Array(new.clone()) {
                plan.add("beams", "unchanged", "beam_depths_mm", old, Value::Array(new), "");
            } else {
                plan.add("beams", "update", "beam_depths_mm", old, Value::Array(new.clone()), "replaces the saved depths");
                beams = Some(new);
            }
        }
    }

    // Geometry in the file is never applied.
    let ignored: Vec<&str> = ["walls", "openings", "columns", "lintels", "rooms", "slabs"]
        .into_iter()
        .filter(|k| root[*k].as_array().is_some_and(|a| !a.is_empty()))
        .collect();
    if !ignored.is_empty() {
        plan.add("geometry", "skipped", &ignored.join(", "), Value::Null, Value::Null, "measured data is not applied to the drawing");
    }

    let changes = plan.count("add") + plan.count("update");
    let mut applied = false;
    if apply && (changes > 0 || known.is_none()) {
        let dm = s.doc_mut()?;
        project::set(dm, "details", &Value::Object(details));
        project::set(dm, "floors", &Value::Array(floors));
        if let Some(b) = beams {
            project::set(dm, "beams", &Value::Array(b));
        }
        if known.is_none() {
            project::set(dm, STORE, &json!({ "drawingId": file_id }));
        }
        applied = true;
    }
    let summary = format!(
        "{} to add, {} to update, {} unchanged, {} flagged, {} skipped",
        plan.count("add"),
        plan.count("update"),
        plan.count("unchanged"),
        plan.count("flagged"),
        plan.count("skipped")
    );
    Ok(json!({
        "drawingId": file_id, "otherDrawing": other, "items": plan.items, "summary": summary, "applied": applied,
        "message": if applied { format!("Applied from AQC: {summary}") } else { format!("AQC import preview (nothing changed): {summary}") },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_time_matches_known_dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn ids_have_the_shape_of_a_uuid() {
        let id = new_id("x");
        assert_eq!(id.len(), 36, "{id}");
        assert_eq!(id.matches('-').count(), 4);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }
}

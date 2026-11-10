//! `takeoff.record`: the take-off as records in the project file, for MynaOps to read.
//!
//! The record layout matches MynaOps `OspTakeoff` / `.osp` `takeoff_*` tables, version 1 (revised 2026-10-19). One project file
//! holds many drawings, so every record id is `(kind << 56) | (slot << 32) | n`: the kind (1 header, 3 wall,
//! 4 opening, 5 column, 6 lintel, 7 room, 8 slab, 9 annotated-PDF chunk), the drawing's slot and the item's
//! number. The slot comes from the drawing id (`slot_for`), is probed past a slot another drawing holds, and is
//! kept in the drawing so it never changes. The payload of a row is the bridge row exactly as `bridge.export`
//! writes it; the box is in millimetres. Only measured facts are recorded.
//!
//! The writer's protocol has no transactions, so the header is the completion marker: it is written with
//! `"state": "recording"`, then the rows, then the stale rows of this slot are deleted, then the annotated PDF,
//! then the header again with `"state": "complete"`, then the database is saved. A reader refuses a drawing that
//! is still `recording`.
//!
//! The take-off goes into the open project file (`.osp`), where MynaOps reads it within seconds. `path` names another project file.

use serde_json::{Map, Value, json};

use super::bridge::{
    self, Collected, Item, KIND_COLUMN, KIND_ELEC_BOARD, KIND_ELEC_CIRCUIT, KIND_LINTEL, KIND_OPENING, KIND_PLUMB_FIXTURE, KIND_PLUMB_STACK,
    KIND_ROOM, KIND_SLAB, KIND_WALL,
};
use super::*;
use crate::{Result, Session};

const HEADER_FORMAT_WIRE: &str = "hcw-takeoff-shilpi";
const KIND_HEADER: u8 = 1;
/// An annotated-PDF chunk.
pub(crate) const KIND_PDF: u8 = 9;
/// The kinds this version writes. Records of any other kind belong to a later version and are left alone.
const OWN_KINDS: [u8; 12] = [
    KIND_HEADER,
    KIND_WALL,
    KIND_OPENING,
    KIND_COLUMN,
    KIND_LINTEL,
    KIND_ROOM,
    KIND_SLAB,
    KIND_PDF,
    KIND_ELEC_BOARD,
    KIND_ELEC_CIRCUIT,
    KIND_PLUMB_STACK,
    KIND_PLUMB_FIXTURE,
];
/// Rows one recording writes; a larger take-off is cut and the result says so.
const MAX_ROWS: usize = 500_000;
/// Largest PDF chunk, and the most a stored PDF may hold (contract section 6).
const CHUNK: usize = 512 * 1024;
const MAX_PDF: usize = 64 * 1024 * 1024;
/// The project-store key that keeps the drawing's slot.
const SLOT_KEY: &str = "takeoffSlot";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("takeoff.record", "Record Take-off", run_record)
            .menu(&["Architecture", "Take-off", "Record in project"])
            .params("{path?: another project file (.osp) to record into (default: the open one), annotate?: draw and store the annotated PDF (default true), paper?: \"A3\", level?, bearing?} → {path, headerId, slot, state, counts, written, added, changed?, unchanged?, deleted, pdf?, skipped, warnings}")
            .noundo(),
    ]
}

/// Load the `takeoff_tables` snapshot from the project file (for `takeoff.restore`).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_tables(s: &Session, p: &Value) -> Result<Value> {
    let file_path = match str_param(p, "path") {
        Some(t) => Some(std::path::PathBuf::from(t)),
        None => project_file_of(s, p),
    };
    let Some(path) = &file_path else {
        return Err(bad("takeoff.restore", "open or save the drawing as a project (.osp) first, or give `path`"));
    };
    let mut sink: Box<dyn Sink> = open_file_sink(path)?;
    let kept = {
        let v = project::get(s.doc()?, SLOT_KEY);
        v.get("slot").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())
    };
    let slot = if let Some(slot) = kept {
        slot
    } else {
        let ids = sink.ids()?;
        let mut found = None;
        for id in ids {
            if kind_of(id) != KIND_HEADER {
                continue;
            }
            let Some(p) = sink.payload(id)? else { continue };
            let Ok(v) = serde_json::from_slice::<Value>(&p) else { continue };
            if v.get("format").and_then(Value::as_str) == Some(HEADER_FORMAT_WIRE) && v.get("state").and_then(Value::as_str) == Some("complete") {
                found = Some(slot_of(id));
                break;
            }
        }
        found.ok_or_else(|| bad("takeoff.restore", "no complete take-off header found in the database"))?
    };
    let payload = sink.payload(header_id(slot))?.ok_or_else(|| bad("takeoff.restore", "header record is missing"))?;
    let header: Value = serde_json::from_slice(&payload).map_err(|e| bad("takeoff.restore", e.to_string()))?;
    if header.get("state").and_then(Value::as_str) != Some("complete") {
        return Err(bad("takeoff.restore", "the drawing is still recording; wait for a complete takeoff.record"));
    }
    header.get("takeoff_tables").cloned().filter(|t| t.as_object().is_some_and(|o| !o.is_empty())).ok_or_else(|| {
        bad("takeoff.restore", "the project's take-off header has no `takeoff_tables`; re-run takeoff then takeoff.record so tables are stored")
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn load_tables(_s: &Session, _p: &Value) -> Result<Value> {
    Err(bad("takeoff.restore", "project files cannot be read on the web"))
}

// ---------------- ids ----------------

pub(crate) fn fnv32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for b in bytes {
        h ^= u32::from(*b);
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// The slot a drawing id starts from: 32-bit FNV-1a folded to 24 bits, never 0.
pub(crate) fn slot_for(drawing_id: &str) -> u32 {
    let h = fnv32(drawing_id.as_bytes());
    let slot = (h ^ (h >> 24)) & 0x00FF_FFFF;
    if slot == 0 { 1 } else { slot }
}

/// The slot after `slot`: past the last one it wraps to 1.
fn next_slot(slot: u32) -> u32 {
    if slot >= 0x00FF_FFFF { 1 } else { slot + 1 }
}

/// The record id of item number `n` of `kind` in `slot`.
pub(crate) fn record_id(kind: u8, slot: u32, n: u32) -> u64 {
    (u64::from(kind) << 56) | (u64::from(slot & 0x00FF_FFFF) << 32) | u64::from(n)
}

pub(crate) fn header_id(slot: u32) -> u64 {
    record_id(KIND_HEADER, slot, 0)
}

pub(crate) fn kind_of(id: u64) -> u8 {
    (id >> 56) as u8
}

pub(crate) fn slot_of(id: u64) -> u32 {
    ((id >> 32) & 0x00FF_FFFF) as u32
}

/// An item's number inside its slot: the entity handle when it fits in 32 bits, else the FNV-1a of its hex text.
fn item_number(handle: u64) -> u32 {
    u32::try_from(handle).unwrap_or_else(|_| fnv32(aad_doc::Handle(handle).hex().as_bytes()))
}

fn kind_name(kind: u8) -> &'static str {
    match kind {
        KIND_WALL => "walls",
        KIND_OPENING => "openings",
        KIND_COLUMN => "columns",
        KIND_LINTEL => "lintels",
        KIND_ROOM => "rooms",
        KIND_SLAB => "slabs",
        KIND_ELEC_BOARD => "electrical",
        KIND_ELEC_CIRCUIT => "electrical_circuits",
        KIND_PLUMB_STACK => "plumbing",
        KIND_PLUMB_FIXTURE => "plumbing_fixtures",
        _ => "other",
    }
}

// ---------------- records ----------------

/// A record as it is written: id, box in millimetres, payload bytes.
#[derive(Clone)]
pub(crate) struct Rec {
    pub id: u64,
    pub payload: Vec<u8>,
}

/// The annotated PDF as it travels: the chunks and the header's `annotated_pdf` entry.
pub(crate) struct Pdf {
    pub chunks: Vec<Rec>,
    pub meta: Value,
}

/// Split a PDF into chunk records (kind 9, `n` from 0, zero box) and describe it.
pub(crate) fn chunk_pdf(bytes: &[u8], slot: u32, pages: u32, generated: &str) -> Result<Pdf> {
    if bytes.len() > MAX_PDF {
        return Err(bad("takeoff.record", format!("the annotated drawing is {} bytes; the most that is stored is {MAX_PDF}", bytes.len())));
    }
    let mut chunks = Vec::new();
    for (i, part) in bytes.chunks(CHUNK).enumerate() {
        let n = u32::try_from(i).map_err(|_| bad("takeoff.record", "too many chunks"))?;
        chunks.push(Rec { id: record_id(KIND_PDF, slot, n), payload: part.to_vec() });
    }
    let meta =
        json!({ "sha256": crate::revision::sha256_hex(bytes), "bytes": bytes.len(), "chunks": chunks.len(), "pages": pages, "generated": generated });
    Ok(Pdf { chunks, meta })
}

/// The header payload: the bridge file's `source`, `project`, `levels`, `beam_depths_mm` and `opening_schedule`, the
/// state, optional `takeoff_tables` (MEASURE table snapshots for `takeoff.restore`), and once stored, the annotated PDF.
pub(crate) fn header(root: &Map<String, Value>, complete: bool, pdf: Option<&Value>) -> Value {
    let mut source = Map::new();
    source.insert("app".into(), json!("O-Draft"));
    if let Some(src) = root.get("source") {
        for key in ["drawing", "drawing_id", "exported", "project_ref"] {
            if let Some(v) = src.get(key) {
                source.insert(key.into(), v.clone());
            }
        }
        if let Some(sha) = src.get("revision").and_then(|r| r.get("sha256")) {
            source.insert("revision_sha256".into(), sha.clone());
        }
    }
    let mut h = Map::new();
    h.insert("format".into(), json!(HEADER_FORMAT_WIRE));
    h.insert("version".into(), json!(1));
    h.insert("units".into(), json!("mm"));
    h.insert("state".into(), json!(if complete { "complete" } else { "recording" }));
    h.insert("source".into(), Value::Object(source));
    for key in ["project", "levels", "beam_depths_mm", "opening_schedule", "takeoff_tables"] {
        if let Some(v) = root.get(key) {
            h.insert(key.into(), v.clone());
        }
    }
    if let (true, Some(p)) = (complete, pdf) {
        h.insert("annotated_pdf".into(), p.clone());
    }
    Value::Object(h)
}

/// What a recording would write: the rows of one slot, and what it left out.
pub(crate) struct Built {
    pub rows: Vec<Rec>,
    pub counts: std::collections::BTreeMap<&'static str, usize>,
    pub skipped: Vec<String>,
}

pub(crate) fn build(c: &Collected, slot: u32) -> Result<Built> {
    let mut warnings: Vec<String> = Vec::new();
    let mut skipped = Vec::new();
    let mut counts = std::collections::BTreeMap::new();
    let mut rows: Vec<Rec> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cut = false;
    for Item { kind, n, row, .. } in &c.items {
        if rows.len() >= MAX_ROWS {
            cut = true;
            break;
        }
        let id = record_id(*kind, slot, item_number(*n));
        if !seen.insert(id) {
            let name = row.get("ref").and_then(Value::as_str).unwrap_or("?");
            skipped.push(format!("{} {name}: another row has the same number", kind_name(*kind)));
            continue;
        }
        let payload = serde_json::to_vec(row).map_err(|e| bad("takeoff.record", e.to_string()))?;
        *counts.entry(kind_name(*kind)).or_insert(0usize) += 1;
        rows.push(Rec { id, payload });
    }
    if cut {
        warnings.push(format!("more than {MAX_ROWS} rows: the rest were not recorded"));
    }
    rows.sort_by_key(|r| r.id);
    Ok(Built { rows, counts, skipped })
}

// ---------------- the database ----------------

/// Where the take-off records go: a project file, or memory in tests.
pub(crate) trait Sink {
    fn ids(&mut self) -> Result<Vec<u64>>;
    fn payload(&mut self, id: u64) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, recs: &[Rec]) -> Result<()>;
    fn delete(&mut self, id: u64) -> Result<bool>;
    fn save(&mut self) -> Result<()>;
}

/// The project file (`.osp`) the open drawing came from, when no `path` was asked for: the take-off then goes into the same file.
#[cfg(not(target_arch = "wasm32"))]
fn project_file_of(s: &Session, p: &Value) -> Option<std::path::PathBuf> {
    let _ = p;
    s.state().ok()?.path.as_deref().filter(|d| super::file::is_container(d)).map(std::path::PathBuf::from)
}

/// The sink for a project file (`.osp`); the take-off is recorded nowhere else.
#[cfg(not(target_arch = "wasm32"))]
fn open_file_sink(path: &std::path::Path) -> Result<Box<dyn Sink>> {
    if super::file::is_container(&path.to_string_lossy()) {
        return Ok(Box::new(OdpSink::open(path)?));
    }
    Err(bad("takeoff.record", format!("{}: the take-off is recorded into a project file (.osp)", path.display())))
}

/// A project file as a take-off store. The records are held in memory in the shape the recorder works in and written back in one revision on
/// `save` (only what changed), so the recorder's steps (claim the slot, write the rows, delete what is gone, write the header again) are unchanged.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct OdpSink {
    odp: aad_odp::Odp,
    records: std::collections::BTreeMap<u64, Vec<u8>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl OdpSink {
    pub(crate) fn open(path: &std::path::Path) -> Result<OdpSink> {
        let odp = if path.exists() {
            aad_odp::Odp::open(path)
        } else {
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Project");
            aad_odp::Odp::create(path, name)
        }
        .map_err(|e| bad("takeoff.record", format!("{}: {e}", path.display())))?;
        let records = odp.takeoff_as_records().map_err(|e| bad("takeoff.record", e.to_string()))?.into_iter().collect();
        Ok(OdpSink { odp, records })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Sink for OdpSink {
    fn ids(&mut self) -> Result<Vec<u64>> {
        Ok(self.records.keys().copied().collect())
    }
    fn payload(&mut self, id: u64) -> Result<Option<Vec<u8>>> {
        Ok(self.records.get(&id).cloned())
    }
    fn put(&mut self, recs: &[Rec]) -> Result<()> {
        for r in recs {
            self.records.insert(r.id, r.payload.clone());
        }
        Ok(())
    }
    fn delete(&mut self, id: u64) -> Result<bool> {
        Ok(self.records.remove(&id).is_some())
    }
    fn save(&mut self) -> Result<()> {
        let all: Vec<(u64, Vec<u8>)> = self.records.iter().map(|(k, v)| (*k, v.clone())).collect();
        self.odp.replace_takeoff(&all).map(|_| ()).map_err(|e| bad("takeoff.record", e.to_string()))
    }
}

/// The drawing id in a header payload, if it parses as a take-off header.
fn drawing_id_of(payload: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    v.get("source")?.get("drawing_id")?.as_str().map(str::to_string)
}

/// Company uuid + project id from `source.project_ref`, when both are present.
fn project_key_of(source: &Value) -> Option<(String, i64)> {
    let r = source.get("project_ref")?;
    let company = r.get("company_uuid")?.as_str()?.to_string();
    let id = r.get("project_id")?.as_i64().or_else(|| r.get("project_id")?.as_u64().and_then(|n| i64::try_from(n).ok()))?;
    Some((company, id))
}

fn project_key(header: &Value) -> Option<(String, i64)> {
    project_key_of(header.get("source")?)
}

/// Display name for another drawing in an overlap warning.
fn other_drawing_label(header: &Value) -> String {
    header
        .get("source")
        .and_then(|s| s.get("drawing").and_then(Value::as_str).filter(|n| !n.is_empty()).or_else(|| s.get("drawing_id").and_then(Value::as_str)))
        .unwrap_or("another drawing")
        .to_string()
}

/// Levels already recorded as measured rows in `slot` (walls, openings, columns, lintels, rooms, slabs).
fn levels_in_slot(sink: &mut dyn Sink, slot: u32) -> Result<std::collections::BTreeSet<String>> {
    let mut levels = std::collections::BTreeSet::new();
    for id in sink.ids()? {
        if slot_of(id) != slot || !OWN_KINDS.contains(&kind_of(id)) || kind_of(id) == KIND_HEADER || kind_of(id) == KIND_PDF {
            continue;
        }
        let Some(p) = sink.payload(id)? else { continue };
        let Ok(v) = serde_json::from_slice::<Value>(&p) else { continue };
        if let Some(name) = v.get("level").and_then(Value::as_str).filter(|n| !n.is_empty()) {
            levels.insert(name.to_string());
        }
    }
    Ok(levels)
}

/// Warn when another complete take-off in this database already measures the same company-file project and floor.
/// Mirrors MynaOps's import overlap warning (warn only; recording still completes).
fn warn_record_overlap(sink: &mut dyn Sink, our_slot: u32, root: &Map<String, Value>, stamped_level: &str) -> Result<Vec<String>> {
    let Some(ours) = root.get("source").and_then(project_key_of) else {
        return Ok(Vec::new());
    };
    if stamped_level.is_empty() {
        return Ok(Vec::new());
    }
    let mut warnings = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in sink.ids()? {
        if kind_of(id) != KIND_HEADER || slot_of(id) == our_slot {
            continue;
        }
        let Some(p) = sink.payload(id)? else { continue };
        let Ok(header) = serde_json::from_slice::<Value>(&p) else { continue };
        if header.get("format").and_then(Value::as_str) != Some(HEADER_FORMAT_WIRE) {
            continue;
        }
        if header.get("state").and_then(Value::as_str) != Some("complete") {
            continue;
        }
        let Some(theirs) = project_key(&header) else { continue };
        if theirs != ours {
            continue;
        }
        let other_slot = slot_of(id);
        let levels = levels_in_slot(sink, other_slot)?;
        if !levels.contains(stamped_level) {
            continue;
        }
        let label = other_drawing_label(&header);
        if seen.insert(format!("{label}|{stamped_level}")) {
            warnings.push(format!(
                "drawing `{label}` already has take-off on `{stamped_level}` for the same company-file project; \
                 recording this drawing too may double-count that floor (MynaOps also warns at import)"
            ));
        }
    }
    Ok(warnings)
}

/// The slot this drawing records into: the slot kept in the drawing when it is still ours, else the first free one
/// from `slot_for`, probing past a slot another drawing holds.
fn find_slot(sink: &mut dyn Sink, drawing_id: &str, kept: Option<u32>) -> Result<u32> {
    let mut slot = kept.filter(|s| (1..=0x00FF_FFFF).contains(s)).unwrap_or_else(|| slot_for(drawing_id));
    for _ in 0..=0x00FF_FFFFu32 {
        match sink.payload(header_id(slot))? {
            None => return Ok(slot),
            Some(p) if drawing_id_of(&p).as_deref() == Some(drawing_id) => return Ok(slot),
            Some(_) => slot = next_slot(slot),
        }
    }
    Err(bad("takeoff.record", "every slot in the database is taken"))
}

/// What a recording did.
struct Outcome {
    slot: u32,
    added: usize,
    changed: Option<usize>,
    unchanged: Option<usize>,
    deleted: usize,
    written: usize,
}

/// The writer's protocol (contract section 4.4) against any database.
fn write_slot(
    sink: &mut dyn Sink,
    c: &Collected,
    built: &Built,
    kept: Option<u32>,
    pdf: Option<(&[u8], u32, String)>,
    compare: bool,
) -> Result<Outcome> {
    let mut slot = find_slot(sink, &c.drawing_id, kept)?;
    let root = &c.root;
    // Claim the slot: write the header, read it back, and take the next slot if another writer won it.
    let mut claims = 0;
    loop {
        let rec = |slot: u32| -> Result<Rec> {
            let payload = serde_json::to_vec(&header(root, false, None)).map_err(|e| bad("takeoff.record", e.to_string()))?;
            Ok(Rec { id: header_id(slot), payload })
        };
        sink.put(&[rec(slot)?])?;
        match sink.payload(header_id(slot))? {
            Some(p) if drawing_id_of(&p).as_deref() == Some(c.drawing_id.as_str()) => break,
            _ => {
                claims += 1;
                if claims > 64 {
                    return Err(bad("takeoff.record", "could not claim a slot"));
                }
                slot = next_slot(slot);
            }
        }
    }
    // The built rows were numbered for the slot found before the claim; renumber them if it moved.
    let rows: Vec<Rec> = if built.rows.first().is_none_or(|r| slot_of(r.id) == slot) {
        built.rows.clone()
    } else {
        built.rows.iter().map(|r| Rec { id: (r.id & !(0x00FF_FFFFu64 << 32)) | (u64::from(slot) << 32), ..r.clone() }).collect()
    };
    let existing: std::collections::HashSet<u64> = sink.ids()?.into_iter().filter(|id| slot_of(*id) == slot).collect();
    let (mut changed, mut unchanged) = (0usize, 0usize);
    if compare {
        for r in &rows {
            if existing.contains(&r.id) {
                if sink.payload(r.id)?.as_deref() == Some(r.payload.as_slice()) {
                    unchanged += 1;
                } else {
                    changed += 1;
                }
            }
        }
    }
    let added = rows.iter().filter(|r| !existing.contains(&r.id)).count();
    sink.put(&rows)?;
    let new_ids: std::collections::HashSet<u64> = rows.iter().map(|r| r.id).collect();
    let mut deleted = 0usize;
    for id in &existing {
        let k = kind_of(*id);
        if *id != header_id(slot) && OWN_KINDS.contains(&k) && k != KIND_PDF && !new_ids.contains(id) && sink.delete(*id)? {
            deleted += 1;
        }
    }
    // The annotated PDF: the old chunks go, the new ones are written.
    for id in &existing {
        if kind_of(*id) == KIND_PDF {
            sink.delete(*id)?;
        }
    }
    let mut meta = None;
    if let Some((bytes, pages, generated)) = pdf {
        let p = chunk_pdf(bytes, slot, pages, &generated)?;
        sink.put(&p.chunks)?;
        meta = Some(p.meta);
    }
    let done = serde_json::to_vec(&header(root, true, meta.as_ref())).map_err(|e| bad("takeoff.record", e.to_string()))?;
    sink.put(&[Rec { id: header_id(slot), payload: done }])?;
    sink.save()?;
    Ok(Outcome { slot, added, changed: compare.then_some(changed), unchanged: compare.then_some(unchanged), deleted, written: rows.len() + 1 })
}

// ---------------- the command ----------------

fn run_record(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("takeoff.record", "project files cannot be written on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut c = bridge::collect(s, p)?;
        // Stash MEASURE table snapshots so takeoff.restore can redraw them from the project file.
        let tables = super::takeoff::snapshot_tables(s.doc()?);
        if let Some(obj) = tables.as_object()
            && !obj.is_empty()
        {
            c.root.insert("takeoff_tables".into(), tables);
        }
        let mut warnings = c.warnings.clone();
        let kept = {
            let v = project::get(s.doc()?, SLOT_KEY);
            let ours = v.get("drawingId").and_then(Value::as_str) == Some(c.drawing_id.as_str());
            if ours { v.get("slot").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) } else { None }
        };
        // The annotated drawing, when asked for and when this build can plot.
        let want_pdf = p.get("annotate").and_then(Value::as_bool).unwrap_or(true);
        let pdf = if want_pdf {
            match super::takeoffann::build_pdf(s, &c, str_param(p, "paper").unwrap_or("A3")) {
                Ok((bytes, _)) if bytes.len() > MAX_PDF => {
                    warnings.push(format!("the annotated drawing is {} bytes, over the {MAX_PDF} that are stored: not recorded", bytes.len()));
                    None
                }
                Ok((bytes, _)) => {
                    let pages = super::takeoffann::page_count(&bytes);
                    Some((bytes, pages, bridge::iso_time().unwrap_or_default()))
                }
                Err(e) => {
                    warnings.push(format!("no annotated drawing: {e}"));
                    None
                }
            }
        } else {
            None
        };
        let pdf_ref = pdf.as_ref().map(|(b, n, g)| (b.as_slice(), *n, g.clone()));

        let file_path = match str_param(p, "path") {
            Some(t) => Some(std::path::PathBuf::from(t)),
            None => project_file_of(s, p),
        };
        let Some(path) = &file_path else {
            return Err(bad("takeoff.record", "save the drawing as a project (.osp) first, or give `path`"));
        };
        let (target, mut sink): (Value, Box<dyn Sink>) = (json!({ "path": path.display().to_string() }), open_file_sink(path)?);
        let probe_slot = find_slot(sink.as_mut(), &c.drawing_id, kept)?;
        let built = build(&c, probe_slot)?;
        let out = write_slot(sink.as_mut(), &c, &built, Some(probe_slot), pdf_ref, file_path.is_some())?;
        project::set(s.doc_mut()?, SLOT_KEY, &json!({ "drawingId": c.drawing_id, "slot": out.slot }));

        let stamped_level = c.items.iter().find_map(|i| i.row.get("level").and_then(Value::as_str)).unwrap_or("");
        warnings.extend(warn_record_overlap(sink.as_mut(), out.slot, &c.root, stamped_level)?);

        let mut result = target;
        result["headerId"] = json!(header_id(out.slot));
        result["slot"] = json!(out.slot);
        result["state"] = json!("complete");
        result["counts"] = json!(built.counts);
        result["written"] = json!(out.written);
        result["added"] = json!(out.added);
        result["deleted"] = json!(out.deleted);
        if let Some(n) = out.changed {
            result["changed"] = json!(n);
        }
        if let Some(n) = out.unchanged {
            result["unchanged"] = json!(n);
        }
        if let Some((bytes, pages, _)) = &pdf {
            result["pdf"] = json!({ "bytes": bytes.len(), "pages": pages, "chunks": bytes.chunks(CHUNK).count() });
        }
        result["skipped"] = json!(built.skipped);
        result["warnings"] = json!(warnings);
        let text = built.counts.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ");
        s.echo(format!("Take-off recorded: {}.", if text.is_empty() { "no measured rows".to_string() } else { text }));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_contract_test_vectors() {
        assert_eq!(slot_for("0b7d9c34-1e6f-4f0b-8a13-9b2c7d5e41aa"), 6_389_047);
        assert_eq!(slot_for("6f1c1b0e-7a52-4d0a-9c1e-2d8a3f4b5c60"), 15_751_439);
        assert!((1..=0x00FF_FFFF).contains(&slot_for("")));
        assert_eq!(next_slot(0x00FF_FFFF), 1);
        assert_eq!(next_slot(5), 6);
    }

    #[test]
    fn ids_carry_kind_slot_and_number() {
        assert_eq!(header_id(0), 1u64 << 56);
        let id = record_id(3, 0x0ABCDE, 0x1A2);
        assert_eq!((kind_of(id), slot_of(id)), (3, 0x0ABCDE));
        assert_eq!(id & 0xFFFF_FFFF, 0x1A2);
        // A slot wider than 24 bits cannot spill into the kind.
        assert_eq!(kind_of(record_id(3, u32::MAX, 7)), 3);
        // A handle wider than 32 bits is hashed, not truncated.
        assert_eq!(item_number(0x1A2), 0x1A2);
        assert_eq!(item_number(0x1_0000_0001), fnv32(b"100000001"));
    }

    #[test]
    fn a_pdf_is_split_into_chunks_that_add_up() {
        let bytes: Vec<u8> = (0..(CHUNK * 2 + 100)).map(|i| (i % 251) as u8).collect();
        let p = chunk_pdf(&bytes, 7, 1, "2026-10-19T00:00:00Z").unwrap();
        assert_eq!(p.chunks.len(), 3);
        assert!(p.chunks.iter().all(|c| c.payload.len() <= CHUNK && kind_of(c.id) == KIND_PDF && slot_of(c.id) == 7));
        assert_eq!(p.chunks.iter().flat_map(|c| c.payload.clone()).collect::<Vec<u8>>(), bytes);
        assert_eq!(p.meta["bytes"], json!(bytes.len()));
        assert_eq!(p.meta["chunks"], json!(3));
        assert_eq!(p.meta["sha256"], json!(crate::revision::sha256_hex(&bytes)));
        assert!(chunk_pdf(&vec![0u8; MAX_PDF + 1], 7, 1, "").is_err());
    }

    /// A database in memory, for the slot rules.
    #[derive(Default)]
    struct Mem(std::collections::BTreeMap<u64, Vec<u8>>);
    impl Sink for Mem {
        fn ids(&mut self) -> Result<Vec<u64>> {
            Ok(self.0.keys().copied().collect())
        }
        fn payload(&mut self, id: u64) -> Result<Option<Vec<u8>>> {
            Ok(self.0.get(&id).cloned())
        }
        fn put(&mut self, recs: &[Rec]) -> Result<()> {
            for r in recs {
                self.0.insert(r.id, r.payload.clone());
            }
            Ok(())
        }
        fn delete(&mut self, id: u64) -> Result<bool> {
            Ok(self.0.remove(&id).is_some())
        }
        fn save(&mut self) -> Result<()> {
            Ok(())
        }
    }

    fn header_of(drawing_id: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({ "source": { "drawing_id": drawing_id } })).unwrap()
    }

    #[test]
    fn a_taken_slot_is_probed_past_and_an_own_slot_is_reused() {
        let id = "0b7d9c34-1e6f-4f0b-8a13-9b2c7d5e41aa";
        let mut db = Mem::default();
        assert_eq!(find_slot(&mut db, id, None).unwrap(), 6_389_047, "a free slot is the start slot");
        // Another drawing holds it: the next one is used.
        db.0.insert(header_id(6_389_047), header_of("someone-else"));
        assert_eq!(find_slot(&mut db, id, None).unwrap(), 6_389_048);
        // Our own header is found again, also from the slot kept in the drawing.
        db.0.insert(header_id(6_389_048), header_of(id));
        assert_eq!(find_slot(&mut db, id, None).unwrap(), 6_389_048);
        assert_eq!(find_slot(&mut db, id, Some(6_389_048)).unwrap(), 6_389_048);
        // A kept slot that another drawing has since taken is not trusted.
        db.0.insert(header_id(6_389_048), header_of("third"));
        assert_eq!(find_slot(&mut db, id, Some(6_389_048)).unwrap(), 6_389_049);
        // After the last slot comes 1.
        let last = "x";
        let mut full = Mem::default();
        full.0.insert(header_id(0x00FF_FFFF), header_of("other"));
        assert_eq!(find_slot(&mut full, last, Some(0x00FF_FFFF)).unwrap(), 1);
    }

    /// A database that fails its `n`th write, to see what a recording that dies half way leaves behind.
    struct FailAt {
        inner: Mem,
        puts_left: usize,
    }
    impl Sink for FailAt {
        fn ids(&mut self) -> Result<Vec<u64>> {
            self.inner.ids()
        }
        fn payload(&mut self, id: u64) -> Result<Option<Vec<u8>>> {
            self.inner.payload(id)
        }
        fn put(&mut self, recs: &[Rec]) -> Result<()> {
            if self.puts_left == 0 {
                return Err(bad("takeoff.record", "the server went away"));
            }
            self.puts_left -= 1;
            self.inner.put(recs)
        }
        fn delete(&mut self, id: u64) -> Result<bool> {
            self.inner.delete(id)
        }
        fn save(&mut self) -> Result<()> {
            self.inner.save()
        }
    }

    fn collected() -> Collected {
        let item = |kind: u8, n: u64| Item { kind, n, row: json!({ "ref": format!("W{n:X}"), "length_mm": 1000.0 }) };
        let mut root = Map::new();
        root.insert("source".into(), json!({ "drawing": "house.dxf", "drawing_id": "0b7d9c34-1e6f-4f0b-8a13-9b2c7d5e41aa" }));
        Collected {
            root,
            warnings: Vec::new(),
            drawing_id: "0b7d9c34-1e6f-4f0b-8a13-9b2c7d5e41aa".into(),
            items: vec![item(KIND_WALL, 0x10), item(KIND_WALL, 0x11)],
            counts: json!({}),
        }
    }

    fn state_of(db: &Mem, slot: u32) -> String {
        let p = db.0.get(&header_id(slot)).expect("a header");
        serde_json::from_slice::<Value>(p).unwrap()["state"].as_str().unwrap().to_string()
    }

    #[test]
    fn a_recording_that_dies_half_way_leaves_the_drawing_recording_and_the_next_one_completes_it() {
        let c = collected();
        let slot = slot_for(&c.drawing_id);
        let built = build(&c, slot).unwrap();
        // The claim, the rows and the final header are three writes; fail the last.
        let mut db = FailAt { inner: Mem::default(), puts_left: 2 };
        assert!(write_slot(&mut db, &c, &built, None, None, false).is_err());
        assert_eq!(state_of(&db.inner, slot), "recording", "a reader must refuse this drawing");
        assert_eq!(db.inner.0.keys().filter(|id| kind_of(**id) == KIND_WALL).count(), 2, "the rows are there but the header does not vouch for them");
        // Recording again, with the server back, completes the same slot.
        db.puts_left = usize::MAX;
        let done = write_slot(&mut db, &c, &built, Some(slot), None, false).unwrap();
        assert_eq!(done.slot, slot);
        assert_eq!(state_of(&db.inner, slot), "complete");
    }
}

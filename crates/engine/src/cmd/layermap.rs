//! `layermap`: the layer mapper. It puts the objects of a drawing's own (foreign) layers on the
//! application's built-in layers, and keeps what the layers were called.
//!
//! * Destinations are the built-in layers only (the standard set, the discipline tabs' layers and
//!   the take-off layers). A name that is not in that catalogue is refused, never created.
//! * Several source layers may go to one destination. Every source is read before any object
//!   moves, so a map that swaps two layers does what it says.
//! * Objects of every level, the layouts and the block definitions are moved. Geometry, marks
//!   (take-off, building element) and every other property stay on the objects; only the layer
//!   changes.
//! * The source layer name of every moved object is recorded in the project entry
//!   [`SOURCES_KEY`] (`{version, entries: [{source, dest, objects, handles?}]}`), so the
//!   imported names stay available after the old layers are gone.
//! * `layermap {suggest: true}` and `{map, preview: true}` change nothing. The wizard reads the
//!   first, shows the second as the review, and only then sends `{map}`: no automatic apply.
//!
//! JSON calls never open a dialog. The shell opens the mapping wizard when `layermap` reports
//! `needsMapping` after a drawing is opened or imported.

use std::collections::{BTreeMap, HashMap, HashSet};

use aad_doc::{Drawing, EntityStore, Handle};
use serde_json::{Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

/// Project entry that keeps the source layer of every mapped object.
pub(crate) const SOURCES_KEY: &str = "layerSources";
/// Project entry set when the person chose to keep the layers of this drawing as they are.
pub(crate) const DISMISSED_KEY: &str = "layerMapDismissed";
/// Most source layers one call maps.
const MAX_MAPPINGS: usize = 10_000;
/// Most handles kept per source entry; beyond that the entry keeps its count only.
const MAX_HANDLES: usize = 50_000;
/// Most entries the source record keeps.
const MAX_ENTRIES: usize = 4_096;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layermap", "Map Layers", run_layermap)
            .menu(&["Format", "Layer Tools", "Map Layers"])
            .params(
                "{suggest?: bool (list the drawing's layers that are not built in, a suggested built-in layer for each, the built-in destinations and whether the drawing `needsMapping`; changes nothing), \
                 only?: [layer names] (limit `suggest` and `map` to these layers, for the layers an import brought in), \
                 map?: {\"WALL-A\": \"A-WALL\", ...} (source layer -> built-in layer; several sources may share one), \
                 preview?: bool (report what `map` would move; changes nothing), purge?: bool (remove each source layer left empty), \
                 sources?: bool (the recorded source layer names), dismiss?: bool (keep this drawing's layers: the wizard is not offered again)} \
                 → {moved, purged, skippedLocked, mapped: [{from, to, moved}]} | {preview: true, mapped, total} | {layers, destinations, needsMapping, origin} | {sources}. \
                 Only built-in layers are destinations. Objects on locked layers stay. The imported names are kept in the project entry `layerSources`.",
            ),
    ]
}

// ---------------------------------------------------------------------------------------------
// The built-in destination catalogue

/// Built-in layers added for the mapper, beside the 36-layer standard set of the HCW toolkit port (which
/// keeps its size): places an imported drawing's geometry often has no standard layer for. Name, ACI
/// colour, linetype, lineweight (hundredths of mm), description; same discipline-prefix naming.
pub(crate) const MAPPER_LAYERS: &[(&str, i16, &str, u16, &str)] = &[
    ("A-ROOM", 150, "Continuous", 13, "Room outlines, area boundaries and room labels"),
    ("A-ELEV", 30, "Continuous", 25, "Elevation linework and facade outlines"),
    ("A-SECT", 30, "Continuous", 35, "Section linework and cut outlines"),
    ("A-LIFT", 5, "Continuous", 25, "Lift shafts, cars and machine rooms"),
    ("A-EXIST", 9, "Continuous", 18, "Existing elements that stay (not demolished)"),
    ("S-FOOT", 94, "Continuous", 35, "Footings, foundations and plinth beams"),
    ("AN-TBLK", 7, "Continuous", 25, "Title block, sheet border and drawing frame"),
];

/// One built-in layer a source layer can be mapped to.
pub(crate) struct Dest {
    pub name: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    aci: i16,
    linetype: &'static str,
    lineweight: u16,
}

fn group_of(name: &str) -> &'static str {
    let up = name.to_ascii_uppercase();
    let prefixed = |p: &str| up.starts_with(p);
    if prefixed("MEASURE-") {
        "Take-off"
    } else if prefixed("AN-") {
        "Annotation"
    } else if prefixed("A-") {
        "Architecture"
    } else if prefixed("I-") {
        "Interior"
    } else if prefixed("E-") {
        "Electrical"
    } else if prefixed("P-") {
        "Plumbing"
    } else if prefixed("FC-") || prefixed("M-") {
        "False ceiling and HVAC"
    } else if prefixed("S-") {
        "Site and structure"
    } else if prefixed("PR-") {
        "Presentation"
    } else {
        "Other"
    }
}

/// The catalogue: the standard architectural set, the layers of the discipline tabs, the layers the
/// geometry database sends objects to, and the take-off layers. Each name appears once.
pub(crate) fn destinations() -> Vec<Dest> {
    let mut out: Vec<Dest> = Vec::new();
    let mut add = |name: &'static str, aci: i16, linetype: &'static str, lineweight: u16, description: &'static str| {
        if !out.iter().any(|d| d.name.eq_ignore_ascii_case(name)) {
            out.push(Dest { name, group: group_of(name), description, aci, linetype, lineweight });
        }
    };
    for l in super::hcw::STANDARD_LAYERS {
        add(l.0, l.1, l.2, l.3, l.4);
    }
    for l in super::sheetstd::tab_layers() {
        add(l.0, l.1, l.2, l.3, l.4);
    }
    for l in MAPPER_LAYERS {
        add(l.0, l.1, l.2, l.3, l.4);
    }
    for l in super::geodb::EXTRA_LAYERS {
        add(l.0, l.1, l.2, l.3, "Beams (from the geometry database)");
    }
    for (name, aci, desc) in super::takeoff::measure_layers() {
        add(name, aci, "Continuous", 18, desc);
    }
    out
}

/// The built-in layer called `name` (case-insensitive).
pub(crate) fn dest_of(name: &str) -> Option<Dest> {
    destinations().into_iter().find(|d| d.name.eq_ignore_ascii_case(name.trim()))
}

pub(crate) fn ensure_dest(d: &mut Drawing, dest: &Dest) {
    super::hcw::ensure_layer(d, dest.name, dest.aci, dest.linetype, dest.lineweight, dest.description);
}

// ---------------------------------------------------------------------------------------------
// Reading the drawing

fn protected(name: &str) -> bool {
    name == "0" || name.eq_ignore_ascii_case("Defpoints") || name.contains('|')
}

/// Object count per layer, keyed by lower-case name (with the drawing's spelling): every level and
/// sheet annotation, the layouts and the block definitions that belong to this drawing.
fn layer_counts(d: &Drawing) -> BTreeMap<String, (String, usize)> {
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut count = |st: &EntityStore| {
        for e in st.iter().take(5_000_000) {
            let c = counts.entry(e.common.layer.to_ascii_lowercase()).or_insert_with(|| (e.common.layer.clone(), 0));
            c.1 = c.1.saturating_add(1);
        }
    };
    d.model_stores().for_each(&mut count);
    d.layouts.iter().for_each(|l| count(&l.entities));
    d.blocks.values().filter(|b| b.xref_path.is_none()).for_each(|b| count(&b.entities));
    counts
}

/// A layer the mapper has nothing to do with: layer 0, Defpoints, an xref's layer, or a built-in layer.
pub(crate) fn is_builtin(name: &str) -> bool {
    protected(name) || super::geodb::standard_layer_names().iter().any(|s| s.eq_ignore_ascii_case(name)) || dest_of(name).is_some()
}

/// The built-in layer a source layer probably stands for, with the reason. The more specific
/// words (hatch, text, dimension) win over the general ones, so `WALL-HATCH` is a hatch.
pub(crate) fn suggest(name: &str) -> Option<(&'static str, &'static str)> {
    let up = name.to_ascii_uppercase();
    if up.contains("HATCH") {
        return Some(("AN-HATCH", "the name says hatch"));
    }
    if up.contains("TEXT") || up.contains("TXT") || up.contains("NOTE") {
        return Some(("AN-TEXT", "the name says text"));
    }
    if up.contains("DIM") {
        return Some(("AN-DIMS", "the name says dimension"));
    }
    if let Some(t) = super::sheetstd::suggest_target(name) {
        return Some((t, "the name contains a known word"));
    }
    let extra: [(&str, &str); 8] = [
        ("BEAM", "S-BEAM"),
        ("ELEV", "A-ELEV"),
        ("SECT", "A-SECT"),
        ("LIFT", "A-LIFT"),
        ("FOOT", "S-FOOT"),
        ("FOUND", "S-FOOT"),
        ("EXIST", "A-EXIST"),
        ("ROOM", "A-ROOM"),
    ];
    if let Some((_, t)) = extra.iter().find(|(k, _)| up.contains(k)) {
        return Some((t, "the name contains a known word"));
    }
    let frame = ["BORDER", "TITLE", "FRAME", "TBLK"];
    if frame.iter().any(|k| up.contains(k)) {
        return Some(("AN-TBLK", "the name says title block or border"));
    }
    None
}

fn only_set(p: &Value) -> Result<Option<HashSet<String>>> {
    match p.get("only") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => Ok(Some(a.iter().filter_map(Value::as_str).take(MAX_MAPPINGS).map(|n| n.trim().to_ascii_lowercase()).collect())),
        Some(_) => Err(bad("layermap", "`only` is a list of layer names")),
    }
}

/// The layers with objects on them that are not built in: `{name, objects, suggested?, reason}`.
pub(crate) fn candidates(d: &Drawing, only: Option<&HashSet<String>>) -> Vec<Value> {
    let mut rows: Vec<(String, usize)> = layer_counts(d)
        .into_iter()
        .filter(|(k, (name, n))| *n > 0 && !is_builtin(name) && only.is_none_or(|o| o.contains(k)))
        .map(|(_, v)| v)
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.into_iter()
        .take(2_000)
        .map(|(name, n)| match suggest(&name) {
            Some((to, why)) => json!({ "name": name, "from": name, "objects": n, "suggested": to, "to": to, "reason": why }),
            None => json!({ "name": name, "from": name, "objects": n, "suggested": Value::Null, "reason": "no known word in the name; choose a built-in layer or keep it" }),
        })
        .collect()
}

/// Whether the opening wizard should be offered: the drawing did not come from MynaCAD (or the
/// caller named layers an import brought in), it has layers to map, and nobody has answered yet.
pub(crate) fn needs_mapping(d: &Drawing, only: Option<&HashSet<String>>) -> bool {
    if only.is_none() && (d.project.contains_key(DISMISSED_KEY) || !super::geodb::origin_of(d).foreign) {
        return false;
    }
    !candidates(d, only).is_empty()
}

// ---------------------------------------------------------------------------------------------
// The command

fn run_layermap(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "dismiss", false) {
        s.doc_mut()?.project.insert(DISMISSED_KEY.into(), "1".into());
        return Ok(json!({ "dismissed": true }));
    }
    if bool_or(p, "sources", false) {
        return Ok(sources_report(s.doc()?));
    }
    let only = only_set(p)?;
    if bool_or(p, "suggest", false) {
        let d = s.doc()?;
        let layers = candidates(d, only.as_ref());
        // The older shape (`suggestions`) stays for callers that read it.
        let suggestions: Vec<Value> =
            layers.iter().map(|l| json!({ "from": l["name"], "to": l["suggested"], "objects": l["objects"], "reason": l["reason"] })).collect();
        let destinations: Vec<Value> =
            destinations().iter().map(|x| json!({ "name": x.name, "group": x.group, "description": x.description })).collect();
        return Ok(json!({
            "origin": super::geodb::origin_of(d),
            "needsMapping": needs_mapping(d, only.as_ref()),
            "layers": layers,
            "suggestions": suggestions,
            "destinations": destinations,
        }));
    }
    let map = p
        .get("map")
        .and_then(Value::as_object)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| bad("layermap", "`map` is required: {\"source layer\": \"built-in layer\", ...}"))?;
    if map.len() > MAX_MAPPINGS {
        return Err(bad("layermap", format!("at most {MAX_MAPPINGS} mappings")));
    }
    let preview = bool_or(p, "preview", false);
    let purge = bool_or(p, "purge", false);
    // Check everything before moving anything.
    let d = s.doc()?;
    let mut pairs: Vec<(String, Dest)> = Vec::new();
    for (from, to) in map {
        let to = to
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| bad("layermap", format!("the target for \"{from}\" must be a layer name")))?;
        let source = d.layer(from.trim()).ok_or_else(|| bad("layermap", format!("no layer \"{from}\"")))?.name.clone();
        if protected(&source) {
            return Err(bad("layermap", format!("layer \"{source}\" always stays (layer 0, Defpoints and xref layers cannot be mapped)")));
        }
        let dest =
            dest_of(to).ok_or_else(|| bad("layermap", format!("\"{to}\" is not a built-in layer; only built-in layers are mapping destinations")))?;
        if let Some((_, other)) = pairs.iter().find(|(f, _)| f.eq_ignore_ascii_case(&source)) {
            if !other.name.eq_ignore_ascii_case(dest.name) {
                return Err(bad("layermap", format!("layer \"{source}\" is mapped twice, to {} and {}", other.name, dest.name)));
            }
            continue;
        }
        if !source.eq_ignore_ascii_case(dest.name) {
            pairs.push((source, dest));
        }
    }
    // Which objects each source holds, read before anything moves.
    let sources: HashSet<String> = pairs.iter().map(|(f, _)| f.to_ascii_lowercase()).collect();
    let by_source = handles_by_source(d, &sources);
    let counts = layer_counts(d);
    let mut mapped = Vec::new();
    let mut total = 0usize;
    let mut skipped = 0usize;
    let mut plan: Vec<(String, Dest, bool, usize)> = Vec::new();
    for (from, dest) in pairs {
        let key = from.to_ascii_lowercase();
        let objects = counts.get(&key).map_or(0, |c| c.1);
        let locked = d.layer(&from).is_some_and(|l| l.locked);
        if locked {
            skipped += objects;
        } else {
            total += objects;
        }
        mapped.push(json!({ "from": from, "to": dest.name, "moved": if locked { 0 } else { objects }, "locked": locked }));
        plan.push((from, dest, locked, objects));
    }
    if preview {
        return Ok(json!({ "preview": true, "mapped": mapped, "total": total, "skippedLocked": skipped }));
    }
    let d = s.doc_mut()?;
    let current = d.header.str("CLAYER", "0");
    let mut purged = 0usize;
    let mut record: Vec<(String, String, usize, Vec<Handle>)> = Vec::new();
    let to_of: HashMap<String, String> = plan.iter().filter(|p| !p.2).map(|(f, t, _, _)| (f.to_ascii_lowercase(), t.name.to_string())).collect();
    for (_, dest, locked, _) in &plan {
        if !*locked {
            ensure_dest(d, dest);
        }
    }
    // Move: one pass over every store, each object to its source's destination.
    retarget(d, &to_of);
    for (from, dest, locked, objects) in &plan {
        if *locked {
            continue;
        }
        let hs = by_source.get(&from.to_ascii_lowercase()).cloned().unwrap_or_default();
        record.push((from.clone(), dest.name.to_string(), *objects, hs));
    }
    if purge {
        for (from, _, locked, _) in &plan {
            if *locked || from.eq_ignore_ascii_case(&current) {
                continue;
            }
            if !layer_in_use(d, from) {
                d.layers.retain(|l| !l.name.eq_ignore_ascii_case(from));
                purged += 1;
            }
        }
    }
    if total > 0 {
        record_sources(d, &record);
        // A foreign drawing that went through the mapper is no longer waiting for it (geodb.apply checks).
        d.project.insert(super::geodb::LAYER_MAP_DONE_KEY.into(), "1".into());
    }
    Ok(json!({ "moved": total, "purged": purged, "skippedLocked": skipped, "mapped": mapped }))
}

/// Whether anything still sits on `layer`: an object in any space or a block definition.
fn layer_in_use(d: &Drawing, layer: &str) -> bool {
    d.model_stores()
        .chain(d.layouts.iter().map(|l| &l.entities))
        .chain(d.blocks.values().map(|b| &b.entities))
        .any(|st| st.iter().any(|e| e.common.layer.eq_ignore_ascii_case(layer)))
}

/// The handles of the objects of model space (every level) and the layouts on each source layer.
fn handles_by_source(d: &Drawing, sources: &HashSet<String>) -> HashMap<String, Vec<Handle>> {
    let mut out: HashMap<String, Vec<Handle>> = HashMap::new();
    for st in d.model_stores().chain(d.layouts.iter().map(|l| &l.entities)) {
        for e in st.iter() {
            let key = e.common.layer.to_ascii_lowercase();
            if sources.contains(&key) {
                let v = out.entry(key).or_default();
                if v.len() <= MAX_HANDLES {
                    v.push(e.handle);
                }
            }
        }
    }
    out
}

/// Put every object on a layer of `to_of` (lower-case source -> destination) on its destination:
/// every level, the layouts and the block definitions. Returns how many objects changed.
fn retarget(d: &mut Drawing, to_of: &HashMap<String, String>) -> usize {
    let change = |st: &mut EntityStore| -> usize {
        let hs: Vec<(Handle, String)> =
            st.iter().filter_map(|e| to_of.get(&e.common.layer.to_ascii_lowercase()).map(|t| (e.handle, t.clone()))).collect();
        for (h, t) in &hs {
            st.modify(*h, |e| e.common.layer = t.clone());
        }
        hs.len()
    };
    let mut n: usize = d.model_stores_mut().map(change).sum();
    for l in &mut d.layouts {
        n += change(&mut l.entities);
    }
    for b in d.blocks.values_mut() {
        if b.xref_path.is_none() && b.entities.iter().any(|e| to_of.contains_key(&e.common.layer.to_ascii_lowercase())) {
            n += change(&mut std::sync::Arc::make_mut(b).entities);
        }
    }
    n
}

// ---------------------------------------------------------------------------------------------
// Source layer names

/// Add the mapped source layers to the project entry. A source mapped again to the same
/// destination adds to its entry; a different destination makes another one.
pub(crate) fn record_sources(d: &mut Drawing, mapped: &[(String, String, usize, Vec<Handle>)]) {
    let mut entries: Vec<Value> = match project::get(d, SOURCES_KEY).get("entries").and_then(Value::as_array) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    for (source, dest, objects, handles) in mapped {
        if *objects == 0 {
            continue;
        }
        let at = entries.iter().position(|e| {
            e["source"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(source)) && e["dest"].as_str().is_some_and(|t| t.eq_ignore_ascii_case(dest))
        });
        let hex: Vec<Value> = handles.iter().map(|h| json!(h.hex())).collect();
        match at {
            Some(i) => {
                let n = entries[i]["objects"].as_u64().unwrap_or(0).saturating_add(*objects as u64);
                let mut all: Vec<Value> = entries[i]["handles"].as_array().cloned().unwrap_or_default();
                all.extend(hex);
                let truncated = entries[i]["truncated"].as_bool().unwrap_or(false) || all.len() > MAX_HANDLES;
                all.truncate(MAX_HANDLES);
                entries[i] = json!({ "source": source, "dest": dest, "objects": n, "handles": all, "truncated": truncated });
            }
            None if entries.len() < MAX_ENTRIES => {
                let truncated = hex.len() > MAX_HANDLES;
                let hex: Vec<Value> = hex.into_iter().take(MAX_HANDLES).collect();
                entries.push(json!({ "source": source, "dest": dest, "objects": objects, "handles": hex, "truncated": truncated }));
            }
            None => {}
        }
    }
    project::set(d, SOURCES_KEY, &json!({ "version": 1, "entries": entries }));
}

fn sources_report(d: &Drawing) -> Value {
    let rec = project::get(d, SOURCES_KEY);
    let entries: Vec<Value> = rec
        .get("entries")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|e| {
                    let live = e["handles"]
                        .as_array()
                        .map_or(0, |hs| hs.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).filter(|h| d.entity(*h).is_some()).count());
                    json!({ "source": e["source"], "dest": e["dest"], "objects": e["objects"], "present": live, "truncated": e["truncated"] })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "sources": entries })
}

/// The source layer an object came from, when the mapper recorded it (`None`: it was never mapped).
pub fn source_layer_of(d: &Drawing, h: Handle) -> Option<String> {
    let rec = project::get(d, SOURCES_KEY);
    let hex = h.hex();
    rec.get("entries")?
        .as_array()?
        .iter()
        .find(|e| e["handles"].as_array().is_some_and(|hs| hs.iter().any(|x| x.as_str() == Some(hex.as_str()))))
        .and_then(|e| e["source"].as_str().map(str::to_string))
}

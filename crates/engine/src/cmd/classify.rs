//! Building-element classification and the open Indian specification index.
//!
//! `classify.*` stamps [`aad_doc::BuildingElement`] on entities (and optionally on a block
//! definition). `spec.*` queries the curated index in `assets/specs/indian-index.json` (built in).
//! DXF round-trip keeps classifications via project key `buildingMeta` (same idea as take-off marks).

use std::sync::OnceLock;

use aad_doc::{BuildingElement, BuildingFamily, BuildingKind, Handle, SpecRef, SpecSystem};
use serde::Deserialize;
use serde_json::{Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

const META_KEY: &str = "buildingMeta";
const BLOCK_META_KEY: &str = "buildingBlockMeta";
const GROUP_NEST_KEY: &str = "groupNest";
const MAX_SPEC_REFS: usize = 32;
const MAX_SEARCH_HITS: usize = 200;
const INDEX_JSON: &str = include_str!("../../../../assets/specs/indian-index.json");

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    format: String,
    version: u32,
    entries: Vec<IndexEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndexEntry {
    pub id: String,
    pub system: String,
    pub code: String,
    pub title: String,
    #[serde(default)]
    pub applies: Vec<String>,
    #[serde(default)]
    pub ifc_types: Vec<String>,
    #[serde(default)]
    pub cpwd_item_hints: Vec<String>,
    #[serde(default)]
    pub fields: Value,
    #[serde(default)]
    pub measurement_rule: Option<String>,
}

fn index() -> &'static IndexFile {
    static INDEX: OnceLock<IndexFile> = OnceLock::new();
    INDEX.get_or_init(|| {
        let parsed: IndexFile = serde_json::from_str(INDEX_JSON).unwrap_or_else(|_| IndexFile {
            format: "osas-indian-spec-index".into(),
            version: 0,
            entries: Vec::new(),
        });
        if parsed.format != "osas-indian-spec-index" || parsed.entries.len() > 10_000 {
            return IndexFile { format: "osas-indian-spec-index".into(), version: 0, entries: Vec::new() };
        }
        parsed
    })
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.list", "List Spec Index", run_spec_list)
            .menu(&["Architecture", "Classify", "Spec Index"])
            .params("{applies?: kind, system?: IS|BIS|CPWD|IFC} → {format, version, entries: [...], disclaimer}")
            .noundo(),
        CommandSpec::new("spec.get", "Get Spec Entry", run_spec_get).params("{id} → the index entry or an error").noundo(),
        CommandSpec::new("spec.search", "Search Spec Index", run_spec_search).params("{q} → {hits: [...]} (id, code, title; capped)").noundo(),
        CommandSpec::new("classify.set", "Classify Selection", run_classify_set).menu(&["Architecture", "Classify", "Set"]).params(
            "{kind: column|beam|footing|wall|slab|plaster|paint|flooring|door|window|grill|other, \
                 family?: structural|finish|shop|other, \
                 ifcType?, measurementRule?, \
                 specRefs?: [{system, code, version?}] | specIds?: [index id], \
                 handles?: [hex], block?: block name} → {classified, warnings?}",
        ),
        CommandSpec::new("classify.get", "Get Classification", run_classify_get)
            .params("{handles?: [hex] (default selection), block?: name} → {items: [...], block?}")
            .noundo(),
        CommandSpec::new("classify.clear", "Clear Classification", run_classify_clear)
            .menu(&["Architecture", "Classify", "Clear"])
            .params("{handles?: [hex] (default selection), block?: name} → {cleared}"),
    ]
}

fn entry_json(e: &IndexEntry) -> Value {
    json!({
        "id": e.id,
        "system": e.system,
        "code": e.code,
        "title": e.title,
        "applies": e.applies,
        "ifcTypes": e.ifc_types,
        "cpwdItemHints": e.cpwd_item_hints,
        "fields": e.fields,
        "measurementRule": e.measurement_rule,
    })
}

fn run_spec_list(_s: &mut Session, p: &Value) -> Result<Value> {
    let idx = index();
    let applies = p.get("applies").and_then(Value::as_str).map(|s| s.to_ascii_lowercase());
    let system = p.get("system").and_then(Value::as_str).map(|s| s.to_ascii_uppercase());
    let entries: Vec<Value> = idx
        .entries
        .iter()
        .filter(|e| applies.as_ref().is_none_or(|a| e.applies.iter().any(|x| x.eq_ignore_ascii_case(a))))
        .filter(|e| system.as_ref().is_none_or(|sys| e.system.eq_ignore_ascii_case(sys)))
        .take(MAX_SEARCH_HITS)
        .map(entry_json)
        .collect();
    Ok(json!({
        "format": idx.format,
        "version": idx.version,
        "entries": entries,
        "disclaimer": "Open code index only; not a certified copy of BIS/IS/CPWD text.",
    }))
}

fn run_spec_get(_s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad("spec.get", "`id` is required"))?;
    let Some(e) = index().entries.iter().find(|e| e.id.eq_ignore_ascii_case(id)) else {
        return Err(bad("spec.get", format!("no index entry `{id}`")));
    };
    Ok(entry_json(e))
}

fn run_spec_search(_s: &mut Session, p: &Value) -> Result<Value> {
    let q = p.get("q").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad("spec.search", "`q` is required"))?;
    if q.len() > 200 {
        return Err(bad("spec.search", "`q` is too long"));
    }
    let needle = q.to_ascii_lowercase();
    let hits: Vec<Value> = index()
        .entries
        .iter()
        .filter(|e| {
            e.id.to_ascii_lowercase().contains(&needle)
                || e.code.to_ascii_lowercase().contains(&needle)
                || e.title.to_ascii_lowercase().contains(&needle)
        })
        .take(MAX_SEARCH_HITS)
        .map(|e| json!({ "id": e.id, "system": e.system, "code": e.code, "title": e.title }))
        .collect();
    Ok(json!({ "hits": hits }))
}

fn parse_kind(cmd: &str, p: &Value) -> Result<BuildingKind> {
    let s = p.get("kind").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`kind` is required"))?;
    BuildingKind::parse(s).ok_or_else(|| bad(cmd, format!("unknown kind `{s}`")))
}

fn parse_family(cmd: &str, p: &Value, kind: BuildingKind) -> Result<BuildingFamily> {
    match p.get("family").and_then(Value::as_str) {
        None | Some("") => Ok(kind.default_family()),
        Some(s) => BuildingFamily::parse(s).ok_or_else(|| bad(cmd, format!("unknown family `{s}`"))),
    }
}

fn parse_spec_refs(cmd: &str, p: &Value, warnings: &mut Vec<String>) -> Result<Vec<SpecRef>> {
    let mut refs = Vec::new();
    if let Some(arr) = p.get("specRefs").and_then(Value::as_array) {
        if arr.len() > MAX_SPEC_REFS {
            return Err(bad(cmd, format!("at most {MAX_SPEC_REFS} `specRefs`")));
        }
        for (i, v) in arr.iter().enumerate() {
            let system = v
                .get("system")
                .and_then(Value::as_str)
                .and_then(SpecSystem::parse)
                .ok_or_else(|| bad(cmd, format!("`specRefs[{i}].system` must be IS|BIS|CPWD|IFC")))?;
            let code = v
                .get("code")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() <= 64)
                .ok_or_else(|| bad(cmd, format!("`specRefs[{i}].code` is required")))?
                .to_string();
            let version = v.get("version").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
            refs.push(SpecRef { system, code, version });
        }
    }
    if let Some(arr) = p.get("specIds").and_then(Value::as_array) {
        if arr.len() > MAX_SPEC_REFS {
            return Err(bad(cmd, format!("at most {MAX_SPEC_REFS} `specIds`")));
        }
        for id in arr.iter().filter_map(Value::as_str) {
            match index().entries.iter().find(|e| e.id.eq_ignore_ascii_case(id)) {
                Some(e) => {
                    let Some(system) = SpecSystem::parse(&e.system) else {
                        warnings.push(format!("index entry `{id}` has an unknown system; skipped"));
                        continue;
                    };
                    refs.push(SpecRef { system, code: e.code.clone(), version: None });
                }
                None => warnings.push(format!("unknown spec id `{id}`")),
            }
        }
    }
    if refs.len() > MAX_SPEC_REFS {
        refs.truncate(MAX_SPEC_REFS);
        warnings.push(format!("spec refs truncated to {MAX_SPEC_REFS}"));
    }
    Ok(refs)
}

fn build_element(cmd: &str, p: &Value, warnings: &mut Vec<String>) -> Result<BuildingElement> {
    let kind = parse_kind(cmd, p)?;
    let family = parse_family(cmd, p, kind)?;
    let mut el = BuildingElement::new(kind);
    el.family = family;
    if let Some(t) = p.get("ifcType").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        if t.len() > 64 {
            return Err(bad(cmd, "`ifcType` is too long"));
        }
        el.ifc_type = Some(t.to_string());
    }
    if let Some(r) = p.get("measurementRule").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        if r.len() > 128 {
            return Err(bad(cmd, "`measurementRule` is too long"));
        }
        el.measurement_rule = Some(r.to_string());
    } else if let Some(first) = p.get("specIds").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str)
        && let Some(e) = index().entries.iter().find(|e| e.id.eq_ignore_ascii_case(first))
        && let Some(rule) = &e.measurement_rule
    {
        el.measurement_rule = Some(rule.clone());
    }
    el.spec_refs = parse_spec_refs(cmd, p, warnings)?;
    Ok(el)
}

fn handles_param(cmd: &str, s: &Session, p: &Value) -> Result<Vec<Handle>> {
    match p.get("handles") {
        None | Some(Value::Null) => Ok(s.selection().to_vec()),
        Some(Value::Array(a)) => {
            if a.len() > 10_000 {
                return Err(bad(cmd, "`handles` has too many entries"));
            }
            a.iter()
                .map(|v| {
                    v.as_str()
                        .and_then(Handle::parse_hex)
                        .or_else(|| v.as_u64().map(Handle))
                        .ok_or_else(|| bad(cmd, format!("`handles`: {v} is not an object handle")))
                })
                .collect()
        }
        Some(v) => Err(bad(cmd, format!("`handles` must be an array of object handles, got {v}"))),
    }
}

fn run_classify_set(s: &mut Session, p: &Value) -> Result<Value> {
    let mut warnings = Vec::new();
    let el = build_element("classify.set", p, &mut warnings)?;
    let mut classified = 0usize;
    if let Some(block) = p.get("block").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        let d = s.doc_mut()?;
        let key =
            d.blocks.keys().find(|k| k.eq_ignore_ascii_case(block)).cloned().ok_or_else(|| bad("classify.set", format!("no block `{block}`")))?;
        let b = d.blocks.get_mut(&key).map(std::sync::Arc::make_mut).ok_or_else(|| bad("classify.set", format!("no block `{block}`")))?;
        b.building = Some(el.clone());
        classified = 1;
        sync_meta(d);
    }
    let hs = handles_param("classify.set", s, p)?;
    if !hs.is_empty() {
        let d = s.doc_mut()?;
        for h in &hs {
            if d.modify_entity(*h, |e| {
                e.common.building = Some(el.clone());
            })
            .is_ok()
            {
                classified = classified.saturating_add(1);
            }
        }
        sync_meta(d);
    }
    if classified == 0 {
        return Err(bad("classify.set", "give `handles`, a selection, or `block`"));
    }
    let mut out = json!({ "classified": classified });
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(out)
}

fn run_classify_get(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut items = Vec::new();
    let hs = handles_param("classify.get", s, p)?;
    for h in hs {
        let Some(e) = d.entity(h) else { continue };
        items.push(json!({
            "handle": h.hex(),
            "building": e.common.building,
        }));
    }
    let mut out = json!({ "items": items });
    if let Some(block) = p.get("block").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        let b = d.block(block).ok_or_else(|| bad("classify.get", format!("no block `{block}`")))?;
        out["block"] = json!({ "name": b.name, "building": b.building });
    }
    Ok(out)
}

fn run_classify_clear(s: &mut Session, p: &Value) -> Result<Value> {
    let mut cleared = 0usize;
    if let Some(block) = p.get("block").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        let d = s.doc_mut()?;
        let key =
            d.blocks.keys().find(|k| k.eq_ignore_ascii_case(block)).cloned().ok_or_else(|| bad("classify.clear", format!("no block `{block}`")))?;
        let b = d.blocks.get_mut(&key).map(std::sync::Arc::make_mut).ok_or_else(|| bad("classify.clear", format!("no block `{block}`")))?;
        if b.building.take().is_some() {
            cleared += 1;
        }
        sync_meta(d);
    }
    let hs = handles_param("classify.clear", s, p)?;
    if !hs.is_empty() {
        let d = s.doc_mut()?;
        for h in &hs {
            let mut did = false;
            if d.modify_entity(*h, |e| {
                if e.common.building.take().is_some() {
                    did = true;
                }
            })
            .is_ok()
                && did
            {
                cleared = cleared.saturating_add(1);
            }
        }
        sync_meta(d);
    }
    if cleared == 0 && p.get("block").is_none() && hs.is_empty() {
        return Err(bad("classify.clear", "give `handles`, a selection, or `block`"));
    }
    Ok(json!({ "cleared": cleared }))
}

/// Copy classification onto a bridge / take-off row when the entity (or its INSERT's block) is classified.
pub(crate) fn stamp_building_on_row(d: &aad_doc::Drawing, handle: Option<Handle>, row: &mut Value) {
    let Some(h) = handle else { return };
    let Some(e) = d.entity(h) else { return };
    let el = e
        .common
        .building
        .clone()
        .or_else(|| if let aad_doc::EntityKind::Insert(i) = &e.kind { d.blocks.get(&i.block).and_then(|b| b.building.clone()) } else { None });
    let Some(el) = el else { return };
    row["ifc_type"] = json!(el.ifc_type_or_default());
    row["building_kind"] = json!(el.kind.as_str());
    row["building_family"] = json!(el.family.as_str());
    if !el.spec_refs.is_empty() {
        row["spec_refs"] = json!(el.spec_refs);
    }
    if let Some(rule) = &el.measurement_rule {
        row["measurement_rule"] = json!(rule);
    }
}

/// Restore classifications from project meta after DXF load.
pub(crate) fn apply_meta(d: &mut aad_doc::Drawing) {
    if let Some(obj) = project::get(d, META_KEY).as_object() {
        let pairs: Vec<(Handle, BuildingElement)> = obj
            .iter()
            .filter_map(|(k, v)| {
                let h = Handle::parse_hex(k)?;
                let el = serde_json::from_value(v.clone()).ok()?;
                Some((h, el))
            })
            .collect();
        for (h, el) in pairs {
            let _ = d.modify_entity(h, |e| {
                if e.common.building.is_none() {
                    e.common.building = Some(el);
                }
            });
        }
    }
    if let Some(obj) = project::get(d, BLOCK_META_KEY).as_object() {
        for (name, v) in obj {
            let Ok(el) = serde_json::from_value::<BuildingElement>(v.clone()) else { continue };
            if let Some(b) = d.blocks.get_mut(name).map(std::sync::Arc::make_mut)
                && b.building.is_none()
            {
                b.building = Some(el);
            }
        }
    }
    if let Some(obj) = project::get(d, GROUP_NEST_KEY).as_object() {
        for g in &mut d.groups {
            if let Some(arr) = obj.get(&g.name).and_then(Value::as_array) {
                g.child_groups = arr.iter().filter_map(Value::as_str).map(str::to_string).take(64).collect();
            }
        }
    }
}

/// Persist classifications (and group nesting) into project keys for DXF round-trip.
pub(crate) fn sync_meta(d: &mut aad_doc::Drawing) {
    let mut m = serde_json::Map::new();
    let stores: Vec<&aad_doc::EntityStore> = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).collect();
    for store in stores {
        for e in store.iter() {
            if let Some(el) = &e.common.building
                && let Ok(v) = serde_json::to_value(el)
            {
                m.insert(e.handle.hex(), v);
            }
        }
    }
    if m.is_empty() {
        d.project.remove(META_KEY);
    } else {
        project::set(d, META_KEY, &Value::Object(m));
    }

    let mut bm = serde_json::Map::new();
    for (name, b) in &d.blocks {
        if let Some(el) = &b.building
            && let Ok(v) = serde_json::to_value(el)
        {
            bm.insert(name.clone(), v);
        }
    }
    if bm.is_empty() {
        d.project.remove(BLOCK_META_KEY);
    } else {
        project::set(d, BLOCK_META_KEY, &Value::Object(bm));
    }

    let mut nest = serde_json::Map::new();
    for g in &d.groups {
        if !g.child_groups.is_empty() {
            nest.insert(g.name.clone(), json!(g.child_groups));
        }
    }
    if nest.is_empty() {
        d.project.remove(GROUP_NEST_KEY);
    } else {
        project::set(d, GROUP_NEST_KEY, &Value::Object(nest));
    }
}

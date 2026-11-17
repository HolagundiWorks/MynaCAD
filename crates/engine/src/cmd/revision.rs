//! Revision register in project data (`AAD_PROJECT` key `revisions`) and sheet revision tables.
//!
//! Snapshot data lives in the project file (`.osp`) or `<drawing>.history.osp`; the register round-trips with the DXF/DWG.

use std::collections::BTreeMap;

use aad_doc::{Drawing, EntityKind, HAlign, LevelId, SheetSetId, Space, VAlign};
use aad_geom::Vec2;
use serde_json::{Map, Value, json};

use super::draw::make_text;
use super::helpers::line;
use super::history;
use super::project;
use super::sheet::{self, write_fields};
use super::*;
use crate::{Result, Session};

const KEY: &str = "revisions";
const TABLE_LAYER: &str = "A-ANNO-REVT";
const TABLE_OWNED: &str = "revisionTableOwned";
const MAX_ENTRIES: usize = 500;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("revision.issue", "Issue Revision", run_issue)
            .menu(&["File", "Revisions", "Issue…"])
            .alias(&["revissue", "issuerev"])
            .params("{description, code?, status?: preliminary|for approval|for construction|void, scope?: current|level|sheet|project, by?, checked?, date?, author?} → {code, milestone, sheets}"),
        CommandSpec::new("revision.list", "Revision Register", run_list)
            .menu(&["File", "Revisions", "Register…"])
            .alias(&["revision", "revlist", "revisions"])
            .params("{} → {codes, entries: [{code, date, description, by, checked, status, scope, milestone}]}")
            .noundo(),
        CommandSpec::new("revision.set", "Edit Revision", run_set)
            .menu(&["File", "Revisions", "Edit…"])
            .params("{code, description?, by?, checked?, date?, status?} → {entry}"),
        CommandSpec::new("revision.void", "Void Revision", run_void)
            .menu(&["File", "Revisions", "Void…"])
            .params("{code} → {entry}"),
        CommandSpec::new("revision.table", "Revision Table", run_table)
            .menu(&["File", "Revisions", "Draw Revision Table"])
            .alias(&["revtable"])
            .params("{layout? | all?: bool, at?: [x,y], height?: mm} → {drawn: [...]}"),
        CommandSpec::new("revision.codes", "Revision Code Style", run_codes)
            .menu(&["File", "Revisions", "Code Style…"])
            .params("{codes?: letters|numbers|custom, customNext?} → {codes, next}")
            .noundo(),
        CommandSpec::new("revision.clouds", "Revision Clouds from Changes", run_clouds)
            .menu(&["File", "Revisions", "Clouds from Changes…"])
            .alias(&["revclouds"])
            .params("{from, to?: milestone|\"now\", gap?: number, accept?: bool, code?, layer?} → {proposals: [{min,max,count}], placed?}")
            ,
    ]
}

fn register(d: &Drawing) -> Map<String, Value> {
    match project::get(d, KEY) {
        Value::Object(o) => o,
        _ => {
            let mut m = Map::new();
            m.insert("codes".into(), json!("letters"));
            m.insert("next".into(), json!(0));
            m.insert("entries".into(), json!([]));
            m
        }
    }
}

fn entries(reg: &Map<String, Value>) -> Vec<Value> {
    reg.get("entries").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn save_register(d: &mut Drawing, reg: Map<String, Value>) {
    project::set(d, KEY, &Value::Object(reg));
}

fn next_code(reg: &Map<String, Value>) -> Result<String> {
    let style = reg.get("codes").and_then(Value::as_str).unwrap_or("letters");
    let next = reg.get("next").and_then(Value::as_u64).unwrap_or(0);
    match style {
        "numbers" => {
            let n = next.saturating_add(1);
            if n > 9_999 {
                return Err(bad("revision.issue", "revision numbers exhausted"));
            }
            Ok(n.to_string())
        }
        "custom" => {
            let s = reg.get("customNext").and_then(Value::as_str).unwrap_or("").trim();
            if s.is_empty() || s.len() > 16 {
                return Err(bad("revision.issue", "custom code style needs `customNext` (1..16 characters), or pass `code`"));
            }
            Ok(s.to_string())
        }
        _ => {
            // letters (default): A, B, … Z, AA, AB, …
            let mut n = next;
            if n > 26 * 26 + 25 {
                return Err(bad("revision.issue", "revision letters exhausted"));
            }
            let mut s = String::new();
            loop {
                let rem = (n % 26) as u8;
                s.insert(0, (b'A' + rem) as char);
                if n < 26 {
                    break;
                }
                n = n / 26 - 1;
            }
            Ok(s)
        }
    }
}

fn bump_next(reg: &mut Map<String, Value>, issued: &str) {
    let style = reg.get("codes").and_then(Value::as_str).unwrap_or("letters").to_string();
    let cur = reg.get("next").and_then(Value::as_u64).unwrap_or(0);
    match style.as_str() {
        "numbers" => {
            let n: u64 = issued.parse().unwrap_or(cur.saturating_add(1));
            reg.insert("next".into(), json!(n));
        }
        "custom" => {
            // Leave customNext for the user to advance; do not invent.
        }
        _ => {
            reg.insert("next".into(), json!(cur.saturating_add(1)));
        }
    }
}

fn status_ok(s: &str) -> bool {
    matches!(s, "preliminary" | "for approval" | "for construction" | "void")
}

/// Layouts whose viewports intersect the revision scope.
fn affected_layouts(d: &Drawing, scope: &Value) -> Vec<String> {
    let stype = scope.get("type").and_then(Value::as_str).unwrap_or("project");
    let nameplates = [sheet::TITLE_PLATE_L, sheet::TITLE_PLATE_P, sheet::TITLE_PLATE];
    d.layouts
        .iter()
        .filter(|l| !nameplates.iter().any(|n| l.name.eq_ignore_ascii_case(n)))
        .filter(|l| layout_matches(d, &l.name, stype, scope))
        .map(|l| l.name.clone())
        .collect()
}

fn layout_matches(d: &Drawing, layout: &str, stype: &str, scope: &Value) -> bool {
    if stype == "project" {
        return true;
    }
    let Some(lay) = d.layouts.iter().find(|l| l.name == layout) else {
        return false;
    };
    let mut any_vp = false;
    for e in lay.entities.iter() {
        let EntityKind::Viewport(vp) = &e.kind else { continue };
        any_vp = true;
        let level_ok = match scope.get("level").and_then(Value::as_u64) {
            Some(n) => vp.level.unwrap_or(LevelId(0)).0 == n as u32,
            None => true,
        };
        let sheet_ok = match scope.get("sheetSet").and_then(Value::as_u64) {
            Some(n) => vp.sheet_set == Some(SheetSetId(n as u32)),
            None => true,
        };
        match stype {
            "level" if level_ok => return true,
            "sheet" if level_ok && sheet_ok => return true,
            _ => {}
        }
    }
    // No bound viewport → project-scope behaviour for unbound sheets.
    !any_vp && stype == "project"
}

fn run_issue(s: &mut Session, p: &Value) -> Result<Value> {
    let description = str_param(p, "description").unwrap_or("").trim();
    if description.is_empty() || description.len() > 500 {
        return Err(bad("revision.issue", "`description` must be 1..500 characters"));
    }
    let status = str_param(p, "status").unwrap_or("for construction");
    if !status_ok(status) || status == "void" {
        return Err(bad("revision.issue", "status must be preliminary, for approval or for construction"));
    }
    let by = str_param(p, "by").unwrap_or("").to_string();
    let checked = str_param(p, "checked").unwrap_or("").to_string();
    let date = str_param(p, "date")
        .map(str::to_string)
        .unwrap_or_else(|| super::bridge::iso_time().map(|t| t.chars().take(10).collect()).unwrap_or_else(|| "1970-01-01".into()));
    if date.len() > 32 {
        return Err(bad("revision.issue", "date is too long"));
    }

    let mut reg = register(s.doc()?);
    let entries_now = entries(&reg);
    if entries_now.len() >= MAX_ENTRIES {
        return Err(bad("revision.issue", format!("at most {MAX_ENTRIES} revisions")));
    }
    let code = match str_param(p, "code") {
        Some(c) => {
            let c = c.trim();
            if c.is_empty() || c.len() > 16 {
                return Err(bad("revision.issue", "`code` must be 1..16 characters"));
            }
            if entries_now.iter().any(|e| e.get("code").and_then(Value::as_str) == Some(c)) {
                return Err(bad("revision.issue", format!("revision `{c}` already exists")));
            }
            c.to_string()
        }
        None => {
            let c = next_code(&reg)?;
            if entries_now.iter().any(|e| e.get("code").and_then(Value::as_str) == Some(c.as_str())) {
                return Err(bad("revision.issue", format!("next code `{c}` is already used; pass `code`")));
            }
            c
        }
    };

    // Snapshot first (does not change the drawing).
    let mut hp = p.clone();
    if let Some(o) = hp.as_object_mut() {
        o.insert("name".into(), json!(format!("Rev {code}: {description}")));
        o.insert("kind".into(), json!("revision"));
        if !o.contains_key("scope") {
            o.insert("scope".into(), json!("project"));
        }
    }
    let saved = history::save_milestone(s, &hp, "revision")?;
    let milestone = saved.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    let scope = saved.get("scope").cloned().unwrap_or(json!({"type": "project"}));

    let entry = json!({
        "code": code,
        "date": date,
        "description": description,
        "by": by,
        "checked": checked,
        "status": status,
        "scope": scope,
        "milestone": milestone,
    });

    // Update register + sheets on a scratch copy for one undo step.
    let mut d = s.doc()?.clone();
    bump_next(&mut reg, &code);
    let mut list = entries(&reg);
    list.push(entry.clone());
    reg.insert("entries".into(), Value::Array(list));
    save_register(&mut d, reg);

    let layouts = affected_layouts(&d, &scope);
    let mut fields = BTreeMap::new();
    fields.insert("REV".into(), code.clone());
    let mut updated = Vec::new();
    for layout in &layouts {
        write_fields(&mut d, layout, &fields);
        updated.push(layout.clone());
    }
    *s.doc_mut()? = d;

    // Redraw revision tables (mutates again — acceptable as part of the same command's undo if
    // table draw is folded in; for simplicity draw after swap so tables are included).
    let mut drawn = Vec::new();
    for layout in &updated {
        match draw_table_on(s, layout, None, 2.5) {
            Ok(n) => drawn.push(json!({"layout": layout, "objects": n})),
            Err(e) => s.echo(format!("Revision table on {layout}: {e}")),
        }
    }

    Ok(json!({
        "code": code,
        "milestone": milestone,
        "entry": entry,
        "sheets": updated,
        "tables": drawn,
        "history": saved,
    }))
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let reg = register(s.doc()?);
    let mut list = entries(&reg);
    list.reverse(); // newest first
    Ok(json!({
        "codes": reg.get("codes").cloned().unwrap_or(json!("letters")),
        "next": reg.get("next").cloned().unwrap_or(json!(0)),
        "entries": list,
    }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let code = str_param(p, "code").ok_or_else(|| bad("revision.set", "`code` is required"))?.trim();
    if code.is_empty() {
        return Err(bad("revision.set", "`code` is required"));
    }
    let mut reg = register(s.doc()?);
    let mut list = entries(&reg);
    let Some(entry) = list.iter_mut().find(|e| e.get("code").and_then(Value::as_str) == Some(code)) else {
        return Err(bad("revision.set", format!("no revision `{code}`")));
    };
    if entry.get("status").and_then(Value::as_str) == Some("void") {
        return Err(bad("revision.set", "a voided revision cannot be edited; issue a new one"));
    }
    if let Some(d) = str_param(p, "description") {
        if d.is_empty() || d.len() > 500 {
            return Err(bad("revision.set", "description must be 1..500 characters"));
        }
        entry.as_object_mut().map(|o| o.insert("description".into(), json!(d)));
    }
    if let Some(v) = str_param(p, "by") {
        entry.as_object_mut().map(|o| o.insert("by".into(), json!(v)));
    }
    if let Some(v) = str_param(p, "checked") {
        entry.as_object_mut().map(|o| o.insert("checked".into(), json!(v)));
    }
    if let Some(v) = str_param(p, "date") {
        if v.len() > 32 {
            return Err(bad("revision.set", "date is too long"));
        }
        entry.as_object_mut().map(|o| o.insert("date".into(), json!(v)));
    }
    if let Some(st) = str_param(p, "status") {
        if !status_ok(st) || st == "void" {
            return Err(bad("revision.set", "status must be preliminary, for approval or for construction (use revision.void to void)"));
        }
        entry.as_object_mut().map(|o| o.insert("status".into(), json!(st)));
    }
    let out = entry.clone();
    reg.insert("entries".into(), Value::Array(list));
    save_register(s.doc_mut()?, reg);
    Ok(json!({ "entry": out }))
}

fn run_void(s: &mut Session, p: &Value) -> Result<Value> {
    let code = str_param(p, "code").ok_or_else(|| bad("revision.void", "`code` is required"))?.trim();
    let mut reg = register(s.doc()?);
    let mut list = entries(&reg);
    let Some(entry) = list.iter_mut().find(|e| e.get("code").and_then(Value::as_str) == Some(code)) else {
        return Err(bad("revision.void", format!("no revision `{code}`")));
    };
    entry.as_object_mut().map(|o| o.insert("status".into(), json!("void")));
    let out = entry.clone();
    reg.insert("entries".into(), Value::Array(list));
    save_register(s.doc_mut()?, reg);
    Ok(json!({ "entry": out }))
}

fn run_codes(s: &mut Session, p: &Value) -> Result<Value> {
    let mut reg = register(s.doc()?);
    if let Some(c) = str_param(p, "codes") {
        if !matches!(c, "letters" | "numbers" | "custom") {
            return Err(bad("revision.codes", "codes must be letters, numbers or custom"));
        }
        reg.insert("codes".into(), json!(c));
    }
    if let Some(n) = str_param(p, "customNext") {
        if n.len() > 16 {
            return Err(bad("revision.codes", "customNext is at most 16 characters"));
        }
        reg.insert("customNext".into(), json!(n));
    }
    save_register(s.doc_mut()?, reg.clone());
    Ok(json!({
        "codes": reg.get("codes").cloned().unwrap_or(json!("letters")),
        "next": reg.get("next").cloned().unwrap_or(json!(0)),
        "customNext": reg.get("customNext").cloned().unwrap_or(Value::Null),
    }))
}

fn run_table(s: &mut Session, p: &Value) -> Result<Value> {
    let all = bool_or(p, "all", false);
    let height = f64_or(p, "height", 2.5);
    if !(height.is_finite() && height > 0.0 && height < 100.0) {
        return Err(bad("revision.table", "`height` must be a positive millimetre size"));
    }
    let at = point_param(p, "at");
    let layouts: Vec<String> = if all {
        let nameplates = [sheet::TITLE_PLATE_L, sheet::TITLE_PLATE_P, sheet::TITLE_PLATE];
        s.doc()?.layouts.iter().filter(|l| !nameplates.iter().any(|n| l.name.eq_ignore_ascii_case(n))).map(|l| l.name.clone()).collect()
    } else if let Some(n) = str_param(p, "layout") {
        vec![n.to_string()]
    } else {
        match s.layout_space() {
            Space::Paper(n) => vec![n],
            Space::Model => return Err(bad("revision.table", "switch to a layout, pass `layout`, or pass `all`")),
        }
    };
    let mut drawn = Vec::new();
    for layout in layouts {
        let n = draw_table_on(s, &layout, at, height)?;
        drawn.push(json!({ "layout": layout, "objects": n }));
    }
    Ok(json!({ "drawn": drawn }))
}

fn draw_table_on(s: &mut Session, layout: &str, at: Option<Vec2>, height_mm: f64) -> Result<usize> {
    let d = s.doc()?;
    let mut list = entries(&register(d));
    list.reverse(); // newest on top
    let rows: Vec<Value> = list
        .into_iter()
        .filter(|e| {
            let scope = e.get("scope").cloned().unwrap_or(json!({"type": "project"}));
            let stype = scope.get("type").and_then(Value::as_str).unwrap_or("project");
            layout_matches(d, layout, stype, &scope)
        })
        .collect();

    let sh = aad_render::sheet(d, layout).ok_or_else(|| bad("revision.table", format!("no layout named `{layout}`")))?;
    let k = 1.0 / sh.unit_mm;
    let th = height_mm * k;
    let at = at.unwrap_or_else(|| Vec2::new(sh.printable.min.x + 10.0 * k, sh.printable.min.y + 40.0 * k));
    let header = ["REV", "DATE", "DESCRIPTION", "BY", "CHK"];
    let body: Vec<[String; 5]> = rows
        .iter()
        .map(|r| {
            let c = |key: &str| r.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            [c("code"), c("date"), c("description"), c("by"), c("checked")]
        })
        .collect();

    let mut widths: Vec<f64> = header.iter().map(|h| h.chars().count() as f64).collect();
    for row in &body {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = w.max(cell.chars().count() as f64);
        }
    }
    // Cap description column width.
    if let Some(w) = widths.get_mut(2) {
        *w = (*w).min(40.0);
    }
    let widths: Vec<f64> = widths.iter().map(|w| w * th * 0.75 + th * 1.2).collect();
    let total: f64 = widths.iter().sum();
    let rh = th * 2.0;

    let old: Vec<aad_doc::Handle> = match project::get(s.doc()?, TABLE_OWNED).get(layout) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(aad_doc::Handle::parse_hex).collect(),
        _ => Vec::new(),
    };
    let space = Space::Paper(layout.to_string());
    for h in old {
        if s.doc()?.space(&space).is_some_and(|st| st.get(h).is_some()) {
            s.doc_mut()?.remove_entity(h);
        }
    }

    let nrows = body.len() + 1;
    let bottom = at.y - rh * nrows as f64;
    // Draw via temporary layout switch like sheet.index.
    let layout_owned = layout.to_string();
    let handles = {
        let prev = s.layout_space();
        super::view::show_space(s, &layout_owned)?;
        let result = (|| -> Result<Vec<aad_doc::Handle>> {
            super::hcw::ensure_layer(s.doc_mut()?, TABLE_LAYER, 7, "Continuous", 25, "Revision table");
            let mut hs = Vec::new();
            let mut y = at.y;
            for (ri, row) in std::iter::once(header.map(String::from)).chain(body.iter().cloned()).enumerate() {
                let mut x = at.x;
                for (cell, w) in row.iter().zip(&widths) {
                    let text = if cell.chars().count() > 40 { format!("{}…", cell.chars().take(39).collect::<String>()) } else { cell.clone() };
                    let kind = make_text(
                        s,
                        Vec2::new(x + th * 0.4, y - rh * 0.5),
                        &text,
                        if ri == 0 { th } else { th * 0.9 },
                        0.0,
                        (HAlign::Left, VAlign::Middle),
                    );
                    hs.push(super::hcw::add_on(s, TABLE_LAYER, kind)?);
                    x += w;
                }
                y -= rh;
            }
            for i in 0..=nrows {
                let y = at.y - rh * i as f64;
                hs.push(super::hcw::add_on(s, TABLE_LAYER, line(Vec2::new(at.x, y), Vec2::new(at.x + total, y)))?);
            }
            let mut x = at.x;
            for w in std::iter::once(&0.0).chain(widths.iter()) {
                x += w;
                hs.push(super::hcw::add_on(s, TABLE_LAYER, line(Vec2::new(x, at.y), Vec2::new(x, bottom)))?);
            }
            hs.push(super::hcw::add_on(s, TABLE_LAYER, line(Vec2::new(at.x, at.y), Vec2::new(at.x, bottom)))?);
            Ok(hs)
        })();
        let back = match prev {
            Space::Paper(n) => n,
            Space::Model => "Model".to_string(),
        };
        let _ = super::view::show_space(s, &back);
        result?
    };

    let mut owned = match project::get(s.doc()?, TABLE_OWNED) {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    let n = handles.len();
    owned.insert(layout.to_string(), Value::Array(handles.iter().map(|h| json!(h.hex())).collect()));
    project::set(s.doc_mut()?, TABLE_OWNED, &Value::Object(owned));
    Ok(n)
}

// ---------------- clouds from compare ----------------

const MAX_CLOUDS: usize = 200;
const MAX_CLUSTER_BOXES: usize = 20_000;

fn run_clouds(s: &mut Session, p: &Value) -> Result<Value> {
    let from = p.get("from").and_then(Value::as_u64).ok_or_else(|| bad("revision.clouds", "`from` (milestone id) is required"))? as u32;
    let to = match p.get("to") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.eq_ignore_ascii_case("now") => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| bad("revision.clouds", "`to` must be a milestone id or \"now\""))? as u32),
    };
    let gap = f64_or(p, "gap", 0.0);
    let gap = if gap.is_finite() && gap >= 0.0 {
        gap
    } else {
        return Err(bad("revision.clouds", "`gap` must be zero or positive"));
    };
    // Default gap: 5% of drawing extents diagonal, or 1000 drawing units.
    let gap = if gap > 0.0 {
        gap
    } else {
        let ext = s.doc()?.extents(&Space::Model);
        if ext.is_empty() {
            1000.0
        } else {
            let dx = ext.max.x - ext.min.x;
            let dy = ext.max.y - ext.min.y;
            (dx.hypot(dy) * 0.05).max(1.0)
        }
    };

    let boxes = history::compare_boxes(s, from, to, p)?;
    if boxes.len() > MAX_CLUSTER_BOXES {
        return Err(bad("revision.clouds", "too many change boxes to cluster"));
    }
    let clusters = cluster_boxes(&boxes, gap);
    if clusters.len() > MAX_CLOUDS {
        return Err(bad("revision.clouds", format!("more than {MAX_CLOUDS} clusters; increase `gap`")));
    }

    let proposals: Vec<Value> = clusters
        .iter()
        .map(|c| {
            json!({
                "min": [c[0], c[1]],
                "max": [c[2], c[3]],
                "count": 1,
            })
        })
        .collect();

    if !bool_or(p, "accept", false) {
        return Ok(json!({ "proposals": proposals, "gap": gap, "from": from, "to": to.map(Value::from).unwrap_or(json!("now")) }));
    }

    let layer = str_param(p, "layer").unwrap_or("A-REV").to_string();
    if layer.is_empty() || layer.len() > 64 {
        return Err(bad("revision.clouds", "`layer` must be 1..64 characters"));
    }
    let code = str_param(p, "code").unwrap_or("").to_string();
    super::hcw::ensure_layer(s.doc_mut()?, &layer, 1, "Continuous", 25, "Revision clouds");

    let mut placed = Vec::new();
    for c in &clusters {
        let p1 = Vec2::new(c[0], c[1]);
        let p2 = Vec2::new(c[2], c[3]);
        // Expand slightly so the cloud sits around the cluster.
        let pad = gap.max(1.0) * 0.25;
        let r = s.execute(
            "revcloud",
            &json!({
                "p1": [p1.x - pad, p1.y - pad],
                "p2": [p2.x + pad, p2.y + pad],
            }),
        )?;
        let handle = r.get("handle").cloned().unwrap_or(Value::Null);
        if let Some(h) = r.get("handle").and_then(Value::as_str).and_then(aad_doc::Handle::parse_hex) {
            s.doc_mut()?.modify_entity(h, |e| {
                e.common.layer = layer.clone();
                if !code.is_empty() {
                    // Tag via layer naming is enough for v1; xdata would need io support.
                    let _ = code;
                }
            })?;
        }
        placed.push(json!({ "handle": handle, "min": [c[0], c[1]], "max": [c[2], c[3]] }));
    }
    Ok(json!({ "proposals": proposals, "placed": placed, "gap": gap, "layer": layer }))
}

/// Union-find clustering of axis-aligned boxes that are within `gap` of each other.
fn cluster_boxes(boxes: &[[f64; 4]], gap: f64) -> Vec<[f64; 4]> {
    let n = boxes.len();
    if n == 0 {
        return Vec::new();
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    fn unite(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[rb] = ra;
        }
    }
    let near = |a: &[f64; 4], b: &[f64; 4], g: f64| -> bool {
        let dx = if a[2] < b[0] {
            b[0] - a[2]
        } else if b[2] < a[0] {
            a[0] - b[2]
        } else {
            0.0
        };
        let dy = if a[3] < b[1] {
            b[1] - a[3]
        } else if b[3] < a[1] {
            a[1] - b[3]
        } else {
            0.0
        };
        dx <= g && dy <= g
    };
    for i in 0..n {
        for j in (i + 1)..n {
            if near(&boxes[i], &boxes[j], gap) {
                unite(&mut parent, i, j);
            }
        }
    }
    let mut groups: BTreeMap<usize, [f64; 4]> = BTreeMap::new();
    for (i, b) in boxes.iter().enumerate() {
        let r = find(&mut parent, i);
        let e = groups.entry(r).or_insert(*b);
        e[0] = e[0].min(b[0]);
        e[1] = e[1].min(b[1]);
        e[2] = e[2].max(b[2]);
        e[3] = e[3].max(b[3]);
    }
    groups.into_values().collect()
}

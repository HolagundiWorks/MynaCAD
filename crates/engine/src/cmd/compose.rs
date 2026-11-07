//! Compose Views, Master Layouts and Publisher Sets — the ArchiCAD-style documentation pipeline.
//!
//! Design: Levels (stories) → Compose Views (saved display recipes) → Layouts with Masters →
//! Publisher Sets. See `docs/levels-and-sheet-sets.md` (Compose and publish) and the case study
//! canvas. Distinct from DXF `VIEW` (`NamedView`) and from Phase-6 section/elevation spaces.

use aad_doc::{ComposeView, ComposeViewId, MasterLayout, MasterLayoutId, PageSetup, PublishFormat, PublisherSet, Trace};
use serde_json::{Map, Value, json};

use super::*;
use crate::{Result, Session};

const PAPERS: [&str; 5] = ["A0", "A1", "A2", "A3", "A4"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("compose.view.list", "List Compose Views", run_view_list)
            .menu(&["Architecture", "Sheets", "Compose Views"])
            .params("{} → {views: [{id, name, level, sheetSet, scale, plotStyleTable, traces, viewCenter, viewHeight}]}")
            .noundo(),
        CommandSpec::new("compose.view.save", "Save Compose View", run_view_save)
            .menu(&["Architecture", "Sheets", "Save Compose View"])
            .params(
                "{name, level?, sheetSet?, scale?: \"1:50\"|50, plotStyleTable?, traces?, viewCenter?: [x,y], viewHeight?} → {id, name}",
            ),
        CommandSpec::new("compose.view.set", "Compose View Settings", run_view_set)
            .params("{view (id or name), name?, level?, sheetSet?, scale?, plotStyleTable?, traces?, viewCenter?, viewHeight?} → {view}"),
        CommandSpec::new("compose.view.delete", "Delete Compose View", run_view_delete)
            .params("{view} → {deleted}"),
        CommandSpec::new("compose.view.apply", "Apply Compose View to Viewport", run_view_apply)
            .menu(&["Architecture", "Sheets", "Apply Compose View"])
            .params("{view, handle? (viewport; default selection or active)} → {handle, view}"),
        CommandSpec::new("master.list", "List Master Layouts", run_master_list)
            .menu(&["Architecture", "Sheets", "Master Layouts"])
            .params("{} → {masters: [{id, name, paper, landscape, widthMm, heightMm, titleBlock, template}]}")
            .noundo(),
        CommandSpec::new("master.new", "New Master Layout", run_master_new)
            .menu(&["Architecture", "Sheets", "New Master Layout"])
            .params("{name, paper?: A0..A4 (default A1), landscape?: bool (default true), titleBlock?, template?} → {id, name}"),
        CommandSpec::new("master.set", "Master Layout Settings", run_master_set)
            .params("{master (id or name), name?, paper?, landscape?, titleBlock?, template?} → {master}"),
        CommandSpec::new("master.delete", "Delete Master Layout", run_master_delete).params("{master} → {deleted}"),
        CommandSpec::new("master.apply", "Apply Master Layout", run_master_apply)
            .menu(&["Architecture", "Sheets", "Apply Master Layout"])
            .params("{master, layout? (default current paper layout)} → {layout, master, titleBlock}"),
        CommandSpec::new("publisher.list", "List Publisher Sets", run_pub_list)
            .menu(&["File", "Publisher Sets"])
            .params("{} → {sets: [{name, layouts, format, singleFile, folder}]}")
            .noundo(),
        CommandSpec::new("publisher.new", "New Publisher Set", run_pub_new)
            .menu(&["File", "New Publisher Set"])
            .params("{name, layouts?: [layout names], format?: pdf|dwg (default pdf), singleFile?: bool, folder?} → {name}"),
        CommandSpec::new("publisher.set", "Publisher Set Settings", run_pub_set)
            .params("{set (name), name?, layouts?, format?, singleFile?, folder?} → {set}"),
        CommandSpec::new("publisher.delete", "Delete Publisher Set", run_pub_delete).params("{set} → {deleted}"),
        CommandSpec::new("publisher.check", "Check Publisher Fit", run_pub_check)
            .params("{set} → {fit: [{layout, fits, scale, nearest, reason}], fitOk}. Does not plot or record a take-off."),
        CommandSpec::new("publisher.publish", "Publish Set", run_pub_publish)
            .menu(&["File", "Publish…"])
            .alias(&["publisher"])
            .params("{set (name), folder?, checkOnly?: bool, takeoff?: bool (default true)} → {files, fit, fitOk, takeoff, warnings}. PDF sets plot each layout; singleFile joins them into one PDF. DWG sets write one file per layout, or one DWG of those layouts when singleFile is set. takeoff records into the open .osp when it can."),
    ]
}

fn paper_mm(paper: &str) -> (f64, f64) {
    match paper {
        "A0" => (1189.0, 841.0),
        "A2" => (594.0, 420.0),
        "A3" => (420.0, 297.0),
        "A4" => (297.0, 210.0),
        _ => (841.0, 594.0),
    }
}

fn parse_scale_denom(v: &Value) -> Option<u32> {
    let d = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(t) => {
            let t = t.trim();
            let tail = t.split_once([':', '/']).map_or(t, |(a, b)| if a.trim() == "1" { b } else { "" });
            tail.trim().parse::<f64>().ok()?
        }
        _ => return None,
    };
    if d.is_finite() && (0.0..=10000.0).contains(&d) { Some(d.round() as u32) } else { None }
}

fn find_view(s: &Session, cmd: &str, v: &Value) -> Result<ComposeViewId> {
    let d = s.doc()?;
    if let Some(n) = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
        let id = ComposeViewId(u32::try_from(n).unwrap_or(u32::MAX));
        if d.compose_view(id).is_some() {
            return Ok(id);
        }
    }
    if let Some(name) = v.as_str()
        && let Some(x) = d.compose_view_by_name(name)
    {
        return Ok(x.id);
    }
    Err(bad(cmd, format!("no compose view `{v}`")))
}

fn find_master(s: &Session, cmd: &str, v: &Value) -> Result<MasterLayoutId> {
    let d = s.doc()?;
    if let Some(n) = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
        let id = MasterLayoutId(u32::try_from(n).unwrap_or(u32::MAX));
        if d.master_layout(id).is_some() {
            return Ok(id);
        }
    }
    if let Some(name) = v.as_str()
        && let Some(x) = d.master_layout_by_name(name)
    {
        return Ok(x.id);
    }
    Err(bad(cmd, format!("no master layout `{v}`")))
}

fn view_json(d: &aad_doc::Drawing, v: &ComposeView) -> Value {
    json!({
        "id": v.id.0,
        "name": v.name,
        "level": v.level.and_then(|id| d.level(id).map(|l| l.name.clone())),
        "sheetSet": v.sheet_set.and_then(|id| d.sheet_set(id).map(|s| s.name.clone())),
        "scale": if v.scale_denom == 0 { Value::Null } else { json!(format!("1:{}", v.scale_denom)) },
        "plotStyleTable": v.plot_style_table,
        "traces": v.traces.iter().map(|t| json!({
            "level": d.level(t.level).map(|l| l.name.clone()),
            "fade": t.fade,
        })).collect::<Vec<_>>(),
        "viewCenter": v.view_center,
        "viewHeight": v.view_height,
    })
}

fn master_json(m: &MasterLayout) -> Value {
    json!({
        "id": m.id.0,
        "name": m.name,
        "paper": m.page.paper,
        "landscape": m.page.landscape,
        "widthMm": m.page.width_mm,
        "heightMm": m.page.height_mm,
        "titleBlock": m.title_block,
        "template": m.template,
    })
}

fn run_view_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    Ok(json!({ "views": d.compose_views.iter().map(|v| view_json(d, v)).collect::<Vec<_>>() }))
}

fn run_view_save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "compose.view.save";
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?;
    let level = match p.get("level") {
        None | Some(Value::Null) => None,
        Some(v) => Some(super::levels::find(s, cmd, &json!({ "level": v }), "level")?),
    };
    let sheet_set = match p.get("sheetSet") {
        None | Some(Value::Null) => None,
        Some(v) => Some(super::sheetset::find(s, cmd, v)?),
    };
    let scale_denom = match p.get("scale") {
        None => 0,
        Some(v) => parse_scale_denom(v).ok_or_else(|| bad(cmd, "`scale` must be like 1:50 or 0..10000"))?,
    };
    let plot_style_table = str_param(p, "plotStyleTable").unwrap_or("").to_string();
    if plot_style_table.chars().count() > 64 {
        return Err(bad(cmd, "`plotStyleTable` is too long"));
    }
    let traces = match p.get("traces") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.len() <= aad_doc::MAX_TRACES => {
            let mut out = Vec::new();
            for item in a {
                let lv = item.get("level").ok_or_else(|| bad(cmd, "each trace needs a `level`"))?;
                let id = super::levels::find(s, cmd, &json!({ "level": lv }), "level")?;
                out.push(Trace::new(id));
            }
            out
        }
        Some(_) => return Err(bad(cmd, format!("`traces` is a list of at most {} items", aad_doc::MAX_TRACES))),
    };
    let view_center = match p.get("viewCenter") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let pt = point_value(v).ok_or_else(|| bad(cmd, "`viewCenter` must be [x, y]"))?;
            Some([pt.x, pt.y])
        }
    };
    let view_height = match p.get("viewHeight") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|x| x.is_finite() && *x > 0.0).ok_or_else(|| bad(cmd, "`viewHeight` must be positive"))?),
    };
    let id = s.doc_mut()?.add_compose_view(ComposeView {
        id: ComposeViewId(0),
        name: name.to_string(),
        level,
        sheet_set,
        scale_denom,
        plot_style_table,
        traces,
        view_center,
        view_height,
    })?;
    Ok(json!({ "id": id.0, "name": name.trim() }))
}

fn run_view_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "compose.view.set";
    let id = find_view(s, cmd, p.get("view").ok_or_else(|| bad(cmd, "`view` is required"))?)?;
    let scale = match p.get("scale") {
        None => None,
        Some(v) => Some(parse_scale_denom(v).ok_or_else(|| bad(cmd, "`scale` must be like 1:50"))?),
    };
    let level = match p.get("level") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(Some(super::levels::find(s, cmd, &json!({ "level": v }), "level")?)),
    };
    let sheet_set = match p.get("sheetSet") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(Some(super::sheetset::find(s, cmd, v)?)),
    };
    let new_name = match str_param(p, "name") {
        None => None,
        Some(n) => {
            if !aad_doc::compose_name_ok(n) {
                return Err(bad(cmd, "bad name"));
            }
            if s.doc()?.compose_views.iter().any(|x| x.id != id && x.name.eq_ignore_ascii_case(n.trim())) {
                return Err(bad(cmd, format!("compose view `{n}` already exists")));
            }
            Some(n.trim().to_string())
        }
    };
    let plot_style = str_param(p, "plotStyleTable").map(str::to_string);
    {
        let d = s.doc_mut()?;
        let Some(v) = d.compose_views.iter_mut().find(|x| x.id == id) else {
            return Err(bad(cmd, "compose view went missing"));
        };
        if let Some(n) = new_name {
            v.name = n;
        }
        if let Some(lv) = level {
            v.level = lv;
        }
        if let Some(ss) = sheet_set {
            v.sheet_set = ss;
        }
        if let Some(sc) = scale {
            v.scale_denom = sc;
        }
        if let Some(t) = plot_style {
            v.plot_style_table = t;
        }
    }
    let (view, refreshed, warnings) = refresh_stamped(s, cmd, id)?;
    Ok(json!({ "view": view, "refreshed": refreshed, "warnings": warnings }))
}

fn run_view_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "compose.view.delete";
    let id = find_view(s, cmd, p.get("view").ok_or_else(|| bad(cmd, "`view` is required"))?)?;
    let name = s.doc()?.compose_view(id).map(|v| v.name.clone()).unwrap_or_default();
    s.doc_mut()?.remove_compose_view(id)?;
    Ok(json!({ "deleted": name }))
}

fn refresh_stamped(s: &mut Session, cmd: &str, id: ComposeViewId) -> Result<(Value, Vec<Value>, Vec<String>)> {
    let handles: Vec<String> = {
        let d = s.doc()?;
        let mut hs = Vec::new();
        for layout in &d.layouts {
            for e in layout.entities.iter() {
                if let aad_doc::EntityKind::Viewport(vp) = &e.kind
                    && vp.id != 1
                    && vp.compose_view == Some(id)
                {
                    hs.push(e.handle.hex());
                }
            }
        }
        hs
    };
    let mut refreshed = Vec::new();
    let mut warnings = Vec::new();
    for h in handles {
        match apply_compose_view(s, id, Some(&h)) {
            Ok(v) => refreshed.push(v.get("handle").cloned().unwrap_or_else(|| json!(h))),
            Err(e) => warnings.push(e.to_string()),
        }
    }
    let d = s.doc()?;
    let v = d.compose_view(id).ok_or_else(|| bad(cmd, "gone"))?;
    Ok((view_json(d, v), refreshed, warnings))
}

fn apply_compose_view(s: &mut Session, id: ComposeViewId, handle: Option<&str>) -> Result<Value> {
    let cmd = "compose.view.apply";
    let (view_name, level, sheet_set, scale_denom, center, height, traces) = {
        let d = s.doc()?;
        let v = d.compose_view(id).ok_or_else(|| bad(cmd, "gone"))?;
        (v.name.clone(), v.level, v.sheet_set, v.scale_denom, v.view_center, v.view_height, v.traces.clone())
    };
    let mut set = Map::new();
    if let Some(h) = handle {
        set.insert("handle".into(), json!(h));
    }
    if let Some(lv) = level {
        let name = s.doc()?.level(lv).map(|l| l.name.clone());
        if let Some(n) = name {
            set.insert("level".into(), json!(n));
        }
    }
    if let Some(ss) = sheet_set {
        let name = s.doc()?.sheet_set(ss).map(|x| x.name.clone());
        if let Some(n) = name {
            set.insert("sheetSet".into(), json!(n));
        }
    }
    if scale_denom > 0 {
        set.insert("scale".into(), json!(format!("1:{scale_denom}")));
    }
    if let Some([x, y]) = center {
        set.insert("center".into(), json!([x, y]));
    }
    if let Some(h) = height {
        set.insert("viewHeight".into(), json!(h));
    }
    if !traces.is_empty() {
        let d = s.doc()?;
        let list: Vec<Value> = traces.iter().filter_map(|t| d.level(t.level).map(|l| json!({ "level": l.name, "fade": t.fade }))).collect();
        set.insert("traces".into(), json!(list));
    }
    let out = s.execute("viewport.set", &Value::Object(set))?;
    // Stamp the compose view id on the viewport after viewport.set.
    let handle = out
        .as_array()
        .and_then(|a| a.first())
        .and_then(|row| row.get("handle"))
        .and_then(Value::as_str)
        .or(handle)
        .and_then(aad_doc::Handle::parse_hex);
    if let Some(h) = handle {
        let d = s.doc_mut()?;
        for layout in &mut d.layouts {
            if let Some(arc) = layout.entities.get(h).cloned() {
                let mut e = (*arc).clone();
                if let aad_doc::EntityKind::Viewport(vp) = &mut e.kind {
                    vp.compose_view = Some(id);
                    layout.entities.replace(e);
                    break;
                }
            }
        }
    }
    Ok(json!({ "handle": handle.map(|h| h.hex()), "view": view_name }))
}

fn run_view_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "compose.view.apply";
    let id = find_view(s, cmd, p.get("view").ok_or_else(|| bad(cmd, "`view` is required"))?)?;
    apply_compose_view(s, id, str_param(p, "handle"))
}

fn run_master_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    Ok(json!({ "masters": d.master_layouts.iter().map(master_json).collect::<Vec<_>>() }))
}

fn run_master_new(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "master.new";
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?;
    let paper = str_param(p, "paper").unwrap_or("A1").trim().to_ascii_uppercase();
    if !PAPERS.contains(&paper.as_str()) {
        return Err(bad(cmd, format!("`paper` must be one of {}", PAPERS.join(", "))));
    }
    let landscape = bool_or(p, "landscape", true);
    let (a, b) = paper_mm(&paper);
    let (width_mm, height_mm) = if landscape { (a.max(b), a.min(b)) } else { (a.min(b), a.max(b)) };
    let title_block = str_param(p, "titleBlock").unwrap_or("").to_string();
    let template = str_param(p, "template").unwrap_or("").to_string();
    let id = s.doc_mut()?.add_master_layout(MasterLayout {
        id: MasterLayoutId(0),
        name: name.to_string(),
        page: PageSetup { paper: paper.clone(), width_mm, height_mm, landscape, ..PageSetup::default() },
        title_block,
        template,
    })?;
    Ok(json!({ "id": id.0, "name": name.trim() }))
}

fn run_master_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "master.set";
    let id = find_master(s, cmd, p.get("master").ok_or_else(|| bad(cmd, "`master` is required"))?)?;
    if let Some(paper) = str_param(p, "paper") {
        let up = paper.trim().to_ascii_uppercase();
        if !PAPERS.contains(&up.as_str()) {
            return Err(bad(cmd, format!("`paper` must be one of {}", PAPERS.join(", "))));
        }
    }
    let d = s.doc_mut()?;
    let Some(m) = d.master_layouts.iter_mut().find(|x| x.id == id) else {
        return Err(bad(cmd, "master went missing"));
    };
    if let Some(n) = str_param(p, "name") {
        if !aad_doc::compose_name_ok(n) {
            return Err(bad(cmd, "bad name"));
        }
        m.name = n.trim().to_string();
    }
    if let Some(paper) = str_param(p, "paper") {
        let up = paper.trim().to_ascii_uppercase();
        let landscape = p.get("landscape").and_then(Value::as_bool).unwrap_or(m.page.landscape);
        let (a, b) = paper_mm(&up);
        let (w, h) = if landscape { (a.max(b), a.min(b)) } else { (a.min(b), a.max(b)) };
        m.page.paper = up;
        m.page.landscape = landscape;
        m.page.width_mm = w;
        m.page.height_mm = h;
    } else if let Some(land) = p.get("landscape").and_then(Value::as_bool) {
        let (a, b) = paper_mm(&m.page.paper);
        let (w, h) = if land { (a.max(b), a.min(b)) } else { (a.min(b), a.max(b)) };
        m.page.landscape = land;
        m.page.width_mm = w;
        m.page.height_mm = h;
    }
    if let Some(t) = str_param(p, "titleBlock") {
        m.title_block = t.to_string();
    }
    if let Some(t) = str_param(p, "template") {
        m.template = t.to_string();
    }
    let out = master_json(m);
    Ok(json!({ "master": out }))
}

fn run_master_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "master.delete";
    let id = find_master(s, cmd, p.get("master").ok_or_else(|| bad(cmd, "`master` is required"))?)?;
    let name = s.doc()?.master_layout(id).map(|m| m.name.clone()).unwrap_or_default();
    s.doc_mut()?.remove_master_layout(id)?;
    Ok(json!({ "deleted": name }))
}

fn current_layout_name(s: &Session) -> Result<String> {
    match s.space() {
        aad_doc::Space::Paper(n) => Ok(n),
        aad_doc::Space::Model => Err(bad("master.apply", "switch to a paper layout first, or pass `layout`")),
    }
}

fn run_master_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "master.apply";
    let id = find_master(s, cmd, p.get("master").ok_or_else(|| bad(cmd, "`master` is required"))?)?;
    let layout_name = match str_param(p, "layout") {
        Some(n) => n.to_string(),
        None => current_layout_name(s)?,
    };
    let (page, title_block, template, master_name) = {
        let d = s.doc()?;
        let m = d.master_layout(id).ok_or_else(|| bad(cmd, "gone"))?;
        if d.layout(&layout_name).is_none() {
            return Err(bad(cmd, format!("no layout `{layout_name}`")));
        }
        (m.page.clone(), m.title_block.clone(), m.template.clone(), m.name.clone())
    };
    {
        let d = s.doc_mut()?;
        if let Some(layout) = d.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layout_name)) {
            layout.page = page;
            layout.master = Some(id);
        }
    }
    if !template.is_empty() {
        let _ = s.execute("sheet.template.apply", &json!({ "name": template, "layout": layout_name }));
    }
    let tb = if title_block.is_empty() { "AAD-TITLEBLOCK" } else { title_block.as_str() };
    let placed = s.execute("sheet.titleblock", &json!({ "layout": layout_name }))?;
    Ok(json!({
        "layout": layout_name,
        "master": master_name,
        "titleBlock": tb,
        "placed": placed,
    }))
}

fn run_pub_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    Ok(json!({
        "sets": d.publisher_sets.iter().map(|p| json!({
            "name": p.name,
            "layouts": p.layouts,
            "format": match p.format { PublishFormat::Pdf => "pdf", PublishFormat::Dwg => "dwg" },
            "singleFile": p.single_file,
            "folder": p.folder,
        })).collect::<Vec<_>>()
    }))
}

fn run_pub_new(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "publisher.new";
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?;
    let layouts = match p.get("layouts") {
        None => Vec::new(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).take(aad_doc::MAX_PUBLISHER_ITEMS).collect(),
        Some(_) => return Err(bad(cmd, "`layouts` is a list of layout names")),
    };
    let format = match str_param(p, "format").unwrap_or("pdf").to_ascii_lowercase().as_str() {
        "pdf" => PublishFormat::Pdf,
        "dwg" => PublishFormat::Dwg,
        _ => return Err(bad(cmd, "`format` is pdf or dwg")),
    };
    s.doc_mut()?.add_publisher_set(PublisherSet {
        name: name.to_string(),
        layouts,
        format,
        single_file: bool_or(p, "singleFile", false),
        folder: str_param(p, "folder").unwrap_or("").to_string(),
    })?;
    Ok(json!({ "name": name.trim() }))
}

fn run_pub_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "publisher.set";
    let key = str_param(p, "set").ok_or_else(|| bad(cmd, "`set` (name) is required"))?;
    let d = s.doc_mut()?;
    let Some(set) = d.publisher_sets.iter_mut().find(|x| x.name.eq_ignore_ascii_case(key)) else {
        return Err(bad(cmd, format!("no publisher set `{key}`")));
    };
    if let Some(n) = str_param(p, "name") {
        if !aad_doc::compose_name_ok(n) {
            return Err(bad(cmd, "bad name"));
        }
        set.name = n.trim().to_string();
    }
    if let Some(Value::Array(a)) = p.get("layouts") {
        set.layouts = a.iter().filter_map(Value::as_str).map(str::to_string).take(aad_doc::MAX_PUBLISHER_ITEMS).collect();
    }
    if let Some(f) = str_param(p, "format") {
        set.format = match f.to_ascii_lowercase().as_str() {
            "pdf" => PublishFormat::Pdf,
            "dwg" => PublishFormat::Dwg,
            _ => return Err(bad(cmd, "`format` is pdf or dwg")),
        };
    }
    if let Some(b) = p.get("singleFile").and_then(Value::as_bool) {
        set.single_file = b;
    }
    if let Some(f) = str_param(p, "folder") {
        set.folder = f.to_string();
    }
    let name = set.name.clone();
    Ok(json!({ "set": name }))
}

fn run_pub_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "publisher.delete";
    let name = str_param(p, "set").ok_or_else(|| bad(cmd, "`set` is required"))?;
    s.doc_mut()?.remove_publisher_set(name)?;
    Ok(json!({ "deleted": name.trim() }))
}

fn run_pub_check(s: &mut Session, p: &Value) -> Result<Value> {
    let (fit, fit_ok, warnings) = fit_report(s, "publisher.check", p)?;
    Ok(json!({ "fit": fit, "fitOk": fit_ok, "warnings": warnings }))
}

fn fit_report(s: &Session, cmd: &str, p: &Value) -> Result<(Vec<Value>, bool, Vec<String>)> {
    let key = str_param(p, "set").ok_or_else(|| bad(cmd, "`set` is required"))?;
    let layouts = {
        let d = s.doc()?;
        let set = d.publisher_set_by_name(key).ok_or_else(|| bad(cmd, format!("no publisher set `{key}`")))?;
        if set.layouts.is_empty() {
            return Err(bad(cmd, "the publisher set has no layouts; add some with publisher.set"));
        }
        for name in &set.layouts {
            if d.layout(name).is_none() {
                return Err(bad(cmd, format!("publisher set names missing layout `{name}`")));
            }
        }
        set.layouts.clone()
    };
    let d = s.doc()?;
    let fit: Vec<Value> = layouts.iter().map(|name| super::layout::sheet_fit(d, name)).collect();
    let mut warnings = Vec::new();
    for row in &fit {
        if row.get("fits").and_then(Value::as_bool) == Some(false) {
            let layout = row.get("layout").and_then(Value::as_str).unwrap_or("layout");
            let nearest = row.get("nearest").and_then(Value::as_str).unwrap_or("");
            warnings.push(format!("{layout} does not fit the sheet ({nearest})"));
        }
    }
    let fit_ok = warnings.is_empty();
    Ok((fit, fit_ok, warnings))
}

fn publish_folder(s: &Session, p: &Value, stored: &str) -> std::path::PathBuf {
    let chosen = str_param(p, "folder").filter(|f| !f.is_empty()).unwrap_or(stored);
    if !chosen.is_empty() {
        return std::path::PathBuf::from(chosen);
    }
    s.state()
        .ok()
        .and_then(|st| st.path.as_deref().and_then(|x| std::path::Path::new(x).parent().map(|d| d.join("Exports"))))
        .unwrap_or_else(|| std::env::temp_dir().join("Exports"))
}

fn file_stem(name: &str) -> String {
    let t: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(80).collect();
    if t.is_empty() { "publish".into() } else { t }
}

fn run_pub_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "publisher.publish";
    let key = str_param(p, "set").ok_or_else(|| bad(cmd, "`set` is required"))?;
    let (set_name, layouts, folder_stored, format, single) = {
        let d = s.doc()?;
        let set = d.publisher_set_by_name(key).ok_or_else(|| bad(cmd, format!("no publisher set `{key}`")))?;
        (set.name.clone(), set.layouts.clone(), set.folder.clone(), set.format, set.single_file)
    };
    let (fit, fit_ok, mut warnings) = fit_report(s, cmd, p)?;
    if bool_or(p, "checkOnly", false) {
        return Ok(json!({ "fit": fit, "fitOk": fit_ok, "warnings": warnings, "files": [] }));
    }
    let out_dir = publish_folder(s, p, &folder_stored);
    std::fs::create_dir_all(&out_dir).map_err(|e| bad(cmd, format!("{}: {e}", out_dir.display())))?;
    let files = match format {
        PublishFormat::Pdf => publish_pdf(s, cmd, &set_name, &layouts, single, &out_dir)?,
        PublishFormat::Dwg => publish_dwg(s, cmd, &set_name, &layouts, single, &out_dir)?,
    };
    let takeoff = if bool_or(p, "takeoff", true) {
        match s.execute("takeoff.record", &json!({ "annotate": false })) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("take-off was not recorded: {e}"));
                Value::Null
            }
        }
    } else {
        Value::Null
    };
    Ok(json!({
        "files": files,
        "fit": fit,
        "fitOk": fit_ok,
        "warnings": warnings,
        "takeoff": takeoff,
        "folder": out_dir.display().to_string(),
        "message": format!("Published {} file(s) to {}", files.len(), out_dir.display()),
    }))
}

fn publish_pdf(s: &mut Session, cmd: &str, set_name: &str, layouts: &[String], single: bool, out_dir: &std::path::Path) -> Result<Vec<Value>> {
    let plot = super::file::io().and_then(|h| h.plot).ok_or_else(|| bad(cmd, "plotting is not available in this build"))?;
    let mut pages = Vec::new();
    let mut files = Vec::new();
    for name in layouts {
        let layout = s.doc()?.layout(name).map(|l| l.name.clone()).ok_or_else(|| bad(cmd, format!("no sheet named `{name}`")))?;
        let pdf = plot(s.doc()?, &aad_doc::Space::Paper(layout.clone()), &json!({})).map_err(|e| bad(cmd, format!("{layout}: {e}")))?;
        if pdf.len() > 40_000_000 {
            return Err(bad(cmd, format!("{layout}: the plot is too large")));
        }
        if single {
            pages.push(pdf);
        } else {
            let file = out_dir.join(format!("{}_{}.pdf", file_stem(set_name), file_stem(&layout)));
            std::fs::write(&file, &pdf).map_err(|e| bad(cmd, format!("{}: {e}", file.display())))?;
            files.push(json!({ "item": format!("sheet {layout}"), "path": file.display().to_string(), "bytes": pdf.len(), "space": layout }));
        }
    }
    if single {
        let merge = super::file::pdf_merge().ok_or_else(|| bad(cmd, "this build cannot join PDF pages into one file"))?;
        let bytes = merge(&pages).map_err(|e| bad(cmd, e))?;
        let file = out_dir.join(format!("{}.pdf", file_stem(set_name)));
        std::fs::write(&file, &bytes).map_err(|e| bad(cmd, format!("{}: {e}", file.display())))?;
        files.push(json!({ "item": set_name, "path": file.display().to_string(), "bytes": bytes.len(), "pages": pages.len() }));
    }
    Ok(files)
}

fn publish_dwg(s: &mut Session, cmd: &str, set_name: &str, layouts: &[String], single: bool, out_dir: &std::path::Path) -> Result<Vec<Value>> {
    let write = super::file::io().map(|h| h.write).ok_or_else(|| bad(cmd, "file formats are not available in this build"))?;
    let jobs: Vec<Vec<String>> = if single { vec![layouts.to_vec()] } else { layouts.iter().map(|n| vec![n.clone()]).collect() };
    let mut files = Vec::new();
    for names in jobs {
        let mut copy = s.doc()?.clone();
        copy.layouts.retain(|l| names.iter().any(|n| l.name.eq_ignore_ascii_case(n)));
        if copy.layouts.is_empty() {
            return Err(bad(cmd, "the publisher set names no layout in this drawing"));
        }
        let label = if single { set_name.to_string() } else { names.first().cloned().unwrap_or_else(|| set_name.to_string()) };
        let bytes = write(&copy, "sheet.dwg").map_err(|e| bad(cmd, e))?;
        let file = out_dir.join(format!("{}.dwg", file_stem(&label)));
        std::fs::write(&file, &bytes).map_err(|e| bad(cmd, format!("{}: {e}", file.display())))?;
        files.push(json!({ "item": label, "path": file.display().to_string(), "bytes": bytes.len(), "format": "dwg" }));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::json;

    #[test]
    fn master_view_publisher_round_trip_in_session() {
        let mut s = Session::new();
        s.execute("master.new", &json!({"name": "A1 L", "paper": "A1", "landscape": true})).unwrap();
        s.execute("compose.view.save", &json!({"name": "GF Plan", "scale": "1:100"})).unwrap();
        s.execute("publisher.new", &json!({"name": "Issue", "layouts": ["Layout1"]})).unwrap();
        let m = s.execute("master.list", &json!({})).unwrap();
        assert_eq!(m["masters"][0]["name"], "A1 L");
        let v = s.execute("compose.view.list", &json!({})).unwrap();
        assert_eq!(v["views"][0]["scale"], "1:100");
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        s.execute("master.apply", &json!({"master": "A1 L", "layout": "Layout1"})).unwrap();
        let d = s.doc().unwrap();
        assert!(d.layout("Layout1").unwrap().master.is_some());
        assert!((d.layout("Layout1").unwrap().page.width_mm - 841.0).abs() < 0.1);
    }

    #[test]
    fn editing_a_compose_view_updates_viewports_that_use_it() {
        let mut s = Session::new();
        let made = s.execute("mview", &json!({"layout": "Layout1", "count": 1})).unwrap();
        let handle = made["viewports"][0].as_str().unwrap();
        s.execute("compose.view.save", &json!({"name": "GF Plan", "scale": "1:100"})).unwrap();
        s.execute("compose.view.apply", &json!({"view": "GF Plan", "handle": handle})).unwrap();
        let before_h = match &s.doc().unwrap().entity(aad_doc::Handle::parse_hex(handle).unwrap()).unwrap().kind {
            aad_doc::EntityKind::Viewport(v) => v.view_height,
            _ => 0.0,
        };
        let updated = s.execute("compose.view.set", &json!({"view": "GF Plan", "scale": "1:50"})).unwrap();
        assert_eq!(updated["refreshed"].as_array().unwrap().len(), 1);
        match &s.doc().unwrap().entity(aad_doc::Handle::parse_hex(handle).unwrap()).unwrap().kind {
            aad_doc::EntityKind::Viewport(v) => {
                assert!(v.compose_view.is_some());
                assert!(before_h > 0.0);
                assert!((v.view_height - before_h / 2.0).abs() < 1e-6);
            }
            other => panic!("expected a viewport, got {other:?}"),
        }
    }

    #[test]
    fn publisher_check_reports_sheet_fit_without_plotting() {
        let mut s = Session::new();
        s.execute("publisher.new", &json!({"name": "Issue", "layouts": ["Layout1"]})).unwrap();
        let r = s.execute("publisher.check", &json!({"set": "Issue"})).unwrap();
        assert_eq!(r["fit"][0]["layout"], "Layout1");
        assert!(r["fitOk"].is_boolean());
        assert!(s.execute("publisher.check", &json!({"set": "missing"})).is_err());
    }

    #[test]
    fn hostile_names_refused() {
        let mut s = Session::new();
        assert!(s.execute("compose.view.save", &json!({"name": ""})).is_err());
        assert!(s.execute("master.new", &json!({"name": "x".repeat(100)})).is_err());
        assert!(s.execute("publisher.new", &json!({"name": "ok", "format": "tiff"})).is_err());
    }
}

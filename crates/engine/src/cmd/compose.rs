//! Compose Views: saved display recipes (level, sheet set, scale, traces, view window, plot style table).
//!
//! Master layouts, publisher sets and applying a view to a layout viewport were removed with paper
//! space (`docs/layout-book.md` 11.1); the `ComposeView` records stay and become the saved views of
//! the layout book (H2). The master and publisher records stay in the `compose1` bytes unchanged.

use aad_doc::{ComposeView, ComposeViewId, Trace};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("compose.view.list", "List Compose Views", run_view_list)
            .menu(&["Architecture", "Sheets", "Compose Views"])
            .params("{} → {views: [{id, name, level, sheetSet, scale, plotStyleTable, traces, viewCenter, viewHeight}]}")
            .noundo(),
        CommandSpec::new("compose.view.save", "Save Compose View", run_view_save)
            .menu(&["Architecture", "Sheets", "Save Compose View"])
            .params("{name, level?, sheetSet?, scale?: \"1:50\"|50, plotStyleTable?, traces?, viewCenter?: [x,y], viewHeight?} → {id, name}"),
        CommandSpec::new("compose.view.set", "Compose View Settings", run_view_set)
            .params("{view (id or name), name?, level?, sheetSet?, scale?, plotStyleTable?, traces?, viewCenter?, viewHeight?} → {view}"),
        CommandSpec::new("compose.view.delete", "Delete Compose View", run_view_delete).params("{view} → {deleted}"),
    ]
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
    let d = s.doc()?;
    let v = d.compose_view(id).ok_or_else(|| bad(cmd, "compose view went missing"))?;
    Ok(json!({ "view": view_json(d, v) }))
}

fn run_view_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "compose.view.delete";
    let id = find_view(s, cmd, p.get("view").ok_or_else(|| bad(cmd, "`view` is required"))?)?;
    let name = s.doc()?.compose_view(id).map(|v| v.name.clone()).unwrap_or_default();
    s.doc_mut()?.remove_compose_view(id)?;
    Ok(json!({ "deleted": name }))
}

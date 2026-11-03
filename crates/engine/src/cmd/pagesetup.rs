//! Page size, orientation and scale of each drawing tab (`sheet.page`): the Working drawing, Electrical, Plumbing and the other tabs
//! each keep their own, saved with the project. The shell reads them to fill the page layout flyout for the tab it was opened on,
//! and the canvas draws the active tab's sheet frame from them ([`tab_page`]).

use serde_json::{Map, Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

const KEY: &str = "pagesetup";
const PAPERS: [&str; 5] = ["A0", "A1", "A2", "A3", "A4"];
const DEFAULT_SCALE: f64 = 100.0;

/// What a drawing tab's sheet looks like: the paper in millimetres (as laid out, width first) and the scale as a denominator.
#[derive(Clone, Debug, PartialEq)]
pub struct TabPage {
    pub paper: String,
    pub landscape: bool,
    pub width_mm: f64,
    pub height_mm: f64,
    pub denominator: f64,
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

/// `1:50`, `1/50` or a bare denominator.
fn parse_scale(v: &Value) -> Option<f64> {
    let d = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(t) => {
            let t = t.trim();
            let tail = t.split_once([':', '/']).map_or(t, |(a, b)| if a.trim() == "1" { b } else { "" });
            tail.trim().parse::<f64>().ok()?
        }
        _ => return None,
    };
    (d.is_finite() && (1.0..=10000.0).contains(&d)).then_some(d)
}

/// The sheet of a drawing tab (A1 landscape at 1:100 until one is set).
pub fn tab_page(d: &aad_doc::Drawing, tab: &str) -> TabPage {
    let all = project::get(d, KEY);
    let entry = all.get(tab.trim().to_lowercase());
    let paper = entry.and_then(|e| e.get("paper")).and_then(Value::as_str).unwrap_or("A1").to_string();
    let landscape = entry.and_then(|e| e.get("landscape")).and_then(Value::as_bool).unwrap_or(true);
    let denominator = entry.and_then(|e| e.get("scale")).and_then(parse_scale).unwrap_or(DEFAULT_SCALE);
    let (a, b) = paper_mm(&paper);
    let (width_mm, height_mm) = if landscape { (a.max(b), a.min(b)) } else { (a.min(b), a.max(b)) };
    TabPage { paper, landscape, width_mm, height_mm, denominator }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("page.manager", "Page Layout Manager", run_manager)
            .menu(&["View", "Page Layout Manager..."])
            .alias(&["pagelayout", "pagemanager"])
            .params("{} (opens the Page Layout Manager in the shell: the paper, orientation and scale of each drawing tab)")
            .noundo(),
        CommandSpec::new("page.toggle", "Model or Page Layout", run_toggle)
            .menu(&["View", "Switch Model / Page Layout"])
            .alias(&["modelpage"])
            .params("{} → {space} (shows the page layout when the model is shown, and the model when a page layout is; the first page layout is made when there is none)")
            .noundo(),
        CommandSpec::new("sheet.page", "Page Size of a Drawing Tab", run_page).params(
            "{tab?: tab name (default \"Design\"), paper?: A0|A1|A2|A3|A4, landscape?: bool, scale?: \"1:50\" | 50} → {tab, paper, landscape, scale}: sets what is given and returns the tab's page (A1 landscape at 1:100 until one is set)",
        ),
    ]
}

/// The Page Layout Manager is a window of the shell, which opens it when this command is chosen.
fn run_manager(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({}))
}

fn run_toggle(s: &mut Session, _p: &Value) -> Result<Value> {
    if !matches!(s.space(), aad_doc::Space::Model) {
        s.execute("layout.set", &json!({ "name": "Model" }))?;
        return Ok(json!({ "space": "Model" }));
    }
    let listed = s.execute("layout.list", &json!({}))?;
    let first = listed.as_array().and_then(|rows| rows.iter().find_map(|r| r.get("name").and_then(Value::as_str))).map(str::to_string);
    let name = match first {
        Some(n) => n,
        None => {
            s.execute("layout.new", &json!({ "name": "Layout1" }))?;
            "Layout1".to_string()
        }
    };
    s.execute("layout.set", &json!({ "name": name }))?;
    Ok(json!({ "space": name }))
}

fn run_page(s: &mut Session, p: &Value) -> Result<Value> {
    let tab = str_param(p, "tab").map(str::trim).filter(|t| !t.is_empty()).unwrap_or("Design").to_string();
    if tab.chars().count() > 64 {
        return Err(bad("sheet.page", "`tab` is longer than 64 characters"));
    }
    let key = tab.to_lowercase();
    let mut all: Map<String, Value> = s.doc().map(|d| project::get(d, KEY)).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let mut entry = all.get(&key).and_then(Value::as_object).cloned().unwrap_or_default();
    let mut changed = false;
    if let Some(paper) = str_param(p, "paper") {
        let up = paper.trim().to_ascii_uppercase();
        if !PAPERS.contains(&up.as_str()) {
            return Err(bad("sheet.page", format!("`paper` must be one of {}", PAPERS.join(", "))));
        }
        entry.insert("paper".into(), json!(up));
        changed = true;
    }
    if let Some(land) = p.get("landscape").and_then(Value::as_bool) {
        entry.insert("landscape".into(), json!(land));
        changed = true;
    }
    if let Some(v) = p.get("scale") {
        let d = parse_scale(v).ok_or_else(|| bad("sheet.page", "`scale` must be like 1:50 (a denominator from 1 to 10000)"))?;
        entry.insert("scale".into(), json!(format!("1:{}", d.round())));
        changed = true;
    }
    if changed {
        all.insert(key, Value::Object(entry));
        project::set(s.doc_mut()?, KEY, &Value::Object(all));
    }
    let page = tab_page(s.doc()?, &tab);
    Ok(json!({ "tab": tab, "paper": page.paper, "landscape": page.landscape, "scale": format!("1:{}", page.denominator.round()) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_tab_keeps_its_own_page() {
        let mut s = Session::new();
        let a = s.execute("sheet.page", &json!({"tab": "Electrical", "paper": "a3", "landscape": false})).unwrap();
        assert_eq!((a["paper"].as_str(), a["landscape"].as_bool()), (Some("A3"), Some(false)));
        s.execute("sheet.page", &json!({"tab": "Plumbing", "paper": "A2"})).unwrap();
        let e = s.execute("sheet.page", &json!({"tab": "electrical"})).unwrap();
        assert_eq!((e["paper"].as_str(), e["landscape"].as_bool()), (Some("A3"), Some(false)), "names are not case sensitive");
        let p = s.execute("sheet.page", &json!({"tab": "Plumbing"})).unwrap();
        assert_eq!((p["paper"].as_str(), p["landscape"].as_bool()), (Some("A2"), Some(true)));
        let d = s.execute("sheet.page", &json!({})).unwrap();
        assert_eq!((d["tab"].as_str(), d["paper"].as_str()), (Some("Design"), Some("A1")));
        assert!(s.execute("sheet.page", &json!({"paper": "A9"})).is_err());
    }

    #[test]
    fn each_tab_keeps_its_own_scale_and_the_sheet_follows() {
        let mut s = Session::new();
        let r = s.execute("sheet.page", &json!({"tab": "Electrical", "paper": "A3", "scale": "1:50"})).unwrap();
        assert_eq!(r["scale"], json!("1:50"));
        s.execute("sheet.page", &json!({"tab": "Plumbing", "scale": 20})).unwrap();
        let e = tab_page(s.doc().unwrap(), "electrical");
        assert_eq!((e.width_mm, e.height_mm, e.denominator), (420.0, 297.0, 50.0));
        assert_eq!(tab_page(s.doc().unwrap(), "Plumbing").denominator, 20.0);
        assert_eq!(tab_page(s.doc().unwrap(), "Design").denominator, 100.0, "1:100 until one is set");
        s.execute("sheet.page", &json!({"tab": "Electrical", "landscape": false})).unwrap();
        let p = tab_page(s.doc().unwrap(), "Electrical");
        assert_eq!((p.width_mm, p.height_mm), (297.0, 420.0));
        assert!(s.execute("sheet.page", &json!({"scale": "1:0"})).is_err());
        assert!(s.execute("sheet.page", &json!({"scale": "big"})).is_err());
    }
}

//! Page size and orientation of each drawing tab (`sheet.page`): the Working drawing, Electrical, Plumbing and the other tabs
//! each keep their own, saved with the project. The shell reads them to fill the page layout flyout for the tab it was opened on.

use serde_json::{Map, Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

const KEY: &str = "pagesetup";
const PAPERS: [&str; 5] = ["A0", "A1", "A2", "A3", "A4"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("sheet.page", "Page Size of a Drawing Tab", run_page).params(
            "{tab?: tab name (default \"Design\"), paper?: A0|A1|A2|A3|A4, landscape?: bool} → {tab, paper, landscape}: sets what is given and returns the tab's page (A1 landscape until one is set)",
        ),
    ]
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
    let paper = entry.get("paper").and_then(Value::as_str).unwrap_or("A1").to_string();
    let landscape = entry.get("landscape").and_then(Value::as_bool).unwrap_or(true);
    if changed {
        all.insert(key, Value::Object(entry));
        project::set(s.doc_mut()?, KEY, &Value::Object(all));
    }
    Ok(json!({ "tab": tab, "paper": paper, "landscape": landscape }))
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
}

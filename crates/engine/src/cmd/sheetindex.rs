//! Sheet numbering scheme record (`sheet.scheme`) and the stored `sheetidx1` project value.
//!
//! The paper-space sheet index (sheets, revisions, issue sets, publish) was removed from the
//! user-facing product (`docs/layout-book.md` section 11.1). What stays: the numbering scheme, which
//! the Ops-first project configuration applies (`project.config.apply`, [`scheme_value`]), and the
//! serde model of the whole record so that a drawing that already holds sheets and issues keeps
//! them byte for byte when the scheme is changed. Every value from a parameter or from the stored
//! JSON is hostile: sizes are capped, text is length-checked, damaged stored data is dropped.

use std::collections::BTreeMap;

use aad_doc::Drawing;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

/// Project-data key of the sheet index.
pub const KEY: &str = "sheetidx1";
const MAX_SHEETS: usize = 1000;
const MAX_ISSUES: usize = 128;
const MAX_DISCIPLINES: usize = 64;
const MAX_TEXT: usize = 200;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("sheet.scheme", "Sheet Numbering Scheme", run_scheme)
            .params("{pattern?: \"{D}-{N}\", width?: 1..8, step?: 1..1000, disciplines?: [{code, name, start}]} → {scheme}. Omit fields to read."),
    ]
}

// ---------------------------------------------------------------- model

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Discipline {
    code: String,
    name: String,
    start: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scheme {
    pattern: String,
    width: usize,
    step: u32,
    disciplines: Vec<Discipline>,
}

impl Default for Scheme {
    fn default() -> Self {
        let d = |code: &str, name: &str, start: u32| Discipline { code: code.into(), name: name.into(), start };
        Scheme {
            pattern: "{D}-{N}".into(),
            width: 3,
            step: 1,
            disciplines: vec![
                d("G", "General", 1),
                d("A", "Architectural", 101),
                d("S", "Structural", 201),
                d("E", "Electrical", 301),
                d("P", "Plumbing", 401),
                d("M", "Mechanical", 501),
                d("I", "Interior", 601),
            ],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    level: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compose_view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Revision {
    rev: String,
    date: String,
    description: String,
    by: String,
    #[serde(default)]
    issue: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sheet {
    id: u32,
    number: String,
    discipline: String,
    title: String,
    #[serde(default)]
    layout: String,
    #[serde(default)]
    scale: String,
    #[serde(default)]
    paper: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    drawn: String,
    #[serde(default)]
    checked: String,
    #[serde(default)]
    approved: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    source: Source,
    #[serde(default)]
    revisions: Vec<Revision>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Frozen {
    sheet: u32,
    number: String,
    title: String,
    layout: String,
    rev: String,
    status: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Issue {
    id: u32,
    name: String,
    date: String,
    rev: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    frozen: Vec<Frozen>,
    #[serde(default)]
    publisher_set: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    #[serde(default)]
    v: u32,
    #[serde(default)]
    seed: u32,
    #[serde(default)]
    issue_seed: u32,
    #[serde(default)]
    scheme: Scheme,
    #[serde(default)]
    sheets: Vec<Sheet>,
    #[serde(default)]
    issues: Vec<Issue>,
    /// Layout name → handles (hex) of what the last drawn table put there.
    #[serde(default)]
    tables: BTreeMap<String, Vec<String>>,
}

impl Index {
    /// The stored index. Missing, damaged or over-size data is an empty index, never an error.
    fn load(d: &Drawing) -> Index {
        let mut idx: Index = serde_json::from_value(project::get(d, KEY)).unwrap_or_default();
        if idx.sheets.len() > MAX_SHEETS || idx.issues.len() > MAX_ISSUES || idx.scheme.disciplines.len() > MAX_DISCIPLINES {
            idx = Index::default();
        }
        if idx.scheme.disciplines.is_empty() || validate_pattern(&idx.scheme.pattern).is_err() || idx.scheme.width == 0 || idx.scheme.width > 8 {
            idx.scheme = Scheme::default();
        }
        idx.scheme.step = idx.scheme.step.clamp(1, 1000);
        idx.v = 1;
        let top = idx.sheets.iter().map(|s| s.id).max().unwrap_or(0);
        if idx.seed <= top {
            idx.seed = top.saturating_add(1);
        }
        let top = idx.issues.iter().map(|i| i.id).max().unwrap_or(0);
        if idx.issue_seed <= top {
            idx.issue_seed = top.saturating_add(1);
        }
        idx
    }

    fn save(&self, d: &mut Drawing) {
        // A numbering scheme that is not the default is kept even before the first sheet exists (the project configuration sets it).
        let default_scheme = serde_json::to_value(Scheme::default()).ok();
        let scheme_is_default = serde_json::to_value(&self.scheme).ok() == default_scheme;
        if self.sheets.is_empty() && self.issues.is_empty() && self.tables.is_empty() && scheme_is_default {
            d.project.remove(KEY);
            return;
        }
        match serde_json::to_value(self) {
            Ok(v) => project::set(d, KEY, &v),
            Err(_) => {
                d.project.remove(KEY);
            }
        }
    }
}

fn validate_pattern(p: &str) -> std::result::Result<(), String> {
    if p.is_empty() || p.chars().count() > 24 {
        return Err("`pattern` must be 1..24 characters".into());
    }
    if p.matches("{N}").count() != 1 || p.matches("{D}").count() > 1 {
        return Err("`pattern` needs {N} once and {D} at most once".into());
    }
    let rest = p.replace("{N}", "").replace("{D}", "");
    if !rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')) {
        return Err("`pattern` may only add letters, digits and - _ . space around {D} and {N}".into());
    }
    Ok(())
}

// ---------------------------------------------------------------- parameter helpers
fn clean(cmd: &str, key: &str, v: &str, max: usize) -> Result<String> {
    let t = v.trim();
    if t.chars().count() > max || t.chars().any(char::is_control) {
        return Err(bad(cmd, format!("`{key}` is longer than {max} characters or has control characters")));
    }
    Ok(t.to_string())
}

/// `None` when absent, `Some("")` for null (clear), else the checked text.
fn opt_text(cmd: &str, p: &Value, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(String::new())),
        Some(Value::String(t)) => clean(cmd, key, t, MAX_TEXT).map(Some),
        Some(Value::Number(n)) => clean(cmd, key, &n.to_string(), MAX_TEXT).map(Some),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

fn code_ok(cmd: &str, c: &str) -> Result<String> {
    let t = c.trim().to_ascii_uppercase();
    if t.is_empty() || t.len() > 4 || !t.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(bad(cmd, "a discipline code is 1..4 letters or digits"));
    }
    Ok(t)
}

fn load(s: &Session) -> Result<Index> {
    Ok(Index::load(s.doc()?))
}

/// The numbering scheme the drawing has now (`{pattern, width, step, disciplines: [{code, name, start}]}`), the default when none was stored.
pub(super) fn scheme_value(d: &Drawing) -> Value {
    serde_json::to_value(Index::load(d).scheme).unwrap_or(Value::Null)
}

fn store(s: &mut Session, idx: &Index) -> Result<()> {
    idx.save(s.doc_mut()?);
    Ok(())
}
fn run_scheme(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.scheme";
    let mut idx = load(s)?;
    let mut sc = idx.scheme.clone();
    if let Some(t) = opt_text(cmd, p, "pattern")? {
        validate_pattern(&t).map_err(|m| bad(cmd, m))?;
        sc.pattern = t;
    }
    if let Some(v) = p.get("width").filter(|v| !v.is_null()) {
        sc.width = v.as_u64().filter(|n| (1..=8).contains(n)).map(|n| n as usize).ok_or_else(|| bad(cmd, "`width` is 1..8"))?;
    }
    if let Some(v) = p.get("step").filter(|v| !v.is_null()) {
        sc.step = v.as_u64().filter(|n| (1..=1000).contains(n)).map(|n| n as u32).ok_or_else(|| bad(cmd, "`step` is 1..1000"))?;
    }
    if let Some(v) = p.get("disciplines").filter(|v| !v.is_null()) {
        let a = v
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= MAX_DISCIPLINES)
            .ok_or_else(|| bad(cmd, format!("`disciplines` is a list of 1..{MAX_DISCIPLINES}")))?;
        let mut out: Vec<Discipline> = Vec::new();
        for item in a {
            let code = code_ok(cmd, item.get("code").and_then(Value::as_str).unwrap_or(""))?;
            let name = clean(cmd, "name", item.get("name").and_then(Value::as_str).unwrap_or(&code), 40)?;
            let start = item
                .get("start")
                .and_then(Value::as_u64)
                .filter(|n| *n <= 999_999)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| bad(cmd, "each discipline needs `start` (0..999999)"))?;
            if out.iter().any(|d| d.code == code) {
                return Err(bad(cmd, format!("discipline `{code}` is listed twice")));
            }
            out.push(Discipline { code, name, start });
        }
        if let Some(used) = idx.sheets.iter().find(|sh| !out.iter().any(|d| d.code.eq_ignore_ascii_case(&sh.discipline))) {
            return Err(bad(cmd, format!("sheet {} uses discipline `{}`; keep it in the list", used.number, used.discipline)));
        }
        sc.disciplines = out;
    }
    idx.scheme = sc;
    store(s, &idx)?;
    Ok(json!({ "scheme": idx.scheme }))
}

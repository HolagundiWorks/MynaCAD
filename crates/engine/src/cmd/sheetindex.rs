//! Structured sheet set: a numbered, ordered sheet index by discipline, per-sheet revision history,
//! issue sets and publish by issue or discipline. Design: `docs/sheet-index.md`.
//!
//! The whole record lives in one JSON value under the new project key [`KEY`] (`sheetidx1`), so the
//! `.osp` and the DXF/DWG project data carry it with no change to either format and MynaOps, which
//! shares the `.osp`, ignores the key. Layouts, viewports, title blocks and publishing are made by
//! the existing commands (`layout.new`, `mview`, `viewport.set`, `master.apply`, `sheet.titleblock`,
//! `compose.view.apply`, `publisher.*`); this module only keeps the records and orders the work.
//! Every number that comes from a command parameter or from the stored JSON is treated as hostile:
//! sizes are capped, text is length-checked, damaged stored data is dropped, nothing indexes by
//! position without `get`.

use std::collections::{BTreeMap, BTreeSet};

use aad_doc::{Drawing, HAlign, LevelId, PageSetup, Space, VAlign};
use aad_geom::Vec2;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::draw::make_text;
use super::helpers::line;
use super::project;
use super::*;
use crate::{Result, Session};

/// Project-data key of the sheet index.
pub const KEY: &str = "sheetidx1";
const MAX_SHEETS: usize = 1000;
const MAX_REVISIONS: usize = 64;
const MAX_ISSUES: usize = 128;
const MAX_DISCIPLINES: usize = 64;
const MAX_TEXT: usize = 200;
const MAX_ISSUE_NAME: usize = 50;
const TABLE_LAYER: &str = "A-ANNO-SIDX";
const STATUSES: [&str; 5] = ["draft", "review", "approved", "issued", "void"];
const PAPERS: [&str; 5] = ["A0", "A1", "A2", "A3", "A4"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("sheet.list", "List Sheet Records", run_list)
            .menu(&["Architecture", "Sheets", "Sheet Index"])
            .params("{discipline?, status?} → {sheets: [{id, number, discipline, title, layout, layoutExists, scale, paper, status, drawn, checked, approved, date, rev, source, revisions}], disciplines, scheme}")
            .noundo(),
        CommandSpec::new("sheet.new", "New Sheet Record", run_new)
            .menu(&["Architecture", "Sheets", "New Sheet"])
            .params("{title, discipline? (default A), number? (default next in the discipline), layout? (existing layout to link), scale?: \"1:100\", paper?: A0..A4, status?, drawn?, checked?, approved?, date?, source?: {kind: custom|level|composeView|view, level?, composeView?, view?}} → {sheet}"),
        CommandSpec::new("sheet.set", "Sheet Record Settings", run_set)
            .params("{sheet (number or id), number?, title?, discipline?, layout?, scale?, paper?, status?, drawn?, checked?, approved?, date?, source?} → {sheet, titleBlock}. Rewrites the title-block fields."),
        CommandSpec::new("sheet.delete", "Delete Sheet Record", run_delete)
            .params("{sheet, layout?: bool (also delete its layout; default false)} → {deleted}"),
        CommandSpec::new("sheet.renumber", "Renumber Sheets", run_renumber)
            .menu(&["Architecture", "Sheets", "Renumber Sheets"])
            .params("{discipline? (default all), dryRun?: bool} → {changes: [{id, from, to}], applied}. Per discipline: start + position * step, void sheets keep their number; nothing changes when a number would repeat."),
        CommandSpec::new("sheet.move", "Move Sheet", run_move)
            .params("{sheet, before? | after? | position? (0-based within the discipline), discipline?, renumber?: bool} → {sheet, order}. A new discipline gives the sheet the next free number there."),
        CommandSpec::new("sheet.scheme", "Sheet Numbering Scheme", run_scheme)
            .params("{pattern?: \"{D}-{N}\", width?: 1..8, step?: 1..1000, disciplines?: [{code, name, start}]} → {scheme}. Omit fields to read."),
        CommandSpec::new("sheetindex.build", "Build Sheets from Levels", run_build)
            .menu(&["Architecture", "Sheets", "Build Sheet Index"])
            .params("{discipline? (A), scale? (1:100), paper?, landscape?, master?, levels?: bool (true), composeViews?: bool (false), views?: [view names], table?: bool} → {created, skipped, warnings}. Repeatable: a level or compose view that already has a sheet is skipped."),
        CommandSpec::new("sheetindex.fill", "Fill Title Blocks from Sheet Records", run_fill)
            .params("{sheet? (default all)} → {filled: [{sheet, layout, changed}]}"),
        CommandSpec::new("sheetindex.table", "Draw Sheet List", run_table)
            .menu(&["Architecture", "Sheets", "Draw Sheet List"])
            .params("{kind?: sheets|revisions (default sheets), layout? (default `Sheet Index`, created when missing), at?: [x,y], height?: 0.5..50 mm} → {layout, objects}. A snapshot: draw again after changes."),
        CommandSpec::new("sheet.revision.add", "Add Sheet Revision", run_rev_add)
            .params("{sheet, rev? (next letter), description, date? (today), by?, issue?} → {sheet, rev}"),
        CommandSpec::new("sheet.revision.list", "Sheet Revisions", run_rev_list)
            .params("{sheet? (default every sheet: the revision schedule)} → {revisions: [{sheet, rev, date, description, by, issue}]}, newest first")
            .noundo(),
        CommandSpec::new("issue.create", "Create Issue Set", run_issue_create)
            .menu(&["Architecture", "Sheets", "New Issue Set"])
            .params("{name, date? (today), rev?, description?, discipline? | sheets?: [numbers], stamp?: bool (true), publisherSet?: bool (true)} → {issue}. Freezes the chosen sheets (default every sheet that is not void), stamps a revision on each and marks them issued."),
        CommandSpec::new("issue.list", "Issue Sets", run_issue_list)
            .params("{} → {issues: [{id, name, date, rev, description, publisherSet, sheets: [{number, title, layout, rev, status}]}]}")
            .noundo(),
        CommandSpec::new("sheetindex.publish", "Publish Sheets", run_publish)
            .menu(&["File", "Publish Sheet Index…"])
            .params("{issue? | discipline? | sheets?: [numbers] (default all), format?: pdf|dwg, singleFile?, folder?, checkOnly?, takeoff?} → what publisher.publish returns plus {set, skipped}. Void sheets and sheets without a layout are skipped."),
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

impl Sheet {
    fn rev(&self) -> String {
        self.revisions.last().map(|r| r.rev.clone()).unwrap_or_default()
    }
    fn issue(&self) -> String {
        self.revisions.iter().rev().map(|r| r.issue.clone()).find(|i| !i.is_empty()).unwrap_or_default()
    }
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
        if self.sheets.is_empty() && self.issues.is_empty() && self.tables.is_empty() {
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

    fn find(&self, cmd: &str, v: &Value) -> Result<usize> {
        let by_id = |n: u64| self.sheets.iter().position(|s| u64::from(s.id) == n);
        let found = match v {
            Value::Number(n) => n.as_u64().and_then(by_id),
            Value::String(t) => {
                let t = t.trim();
                self.sheets.iter().position(|s| s.number.eq_ignore_ascii_case(t)).or_else(|| t.parse::<u64>().ok().and_then(by_id))
            }
            _ => None,
        };
        found.ok_or_else(|| bad(cmd, format!("no sheet `{v}`")))
    }

    fn discipline(&self, code: &str) -> Option<&Discipline> {
        self.scheme.disciplines.iter().find(|d| d.code.eq_ignore_ascii_case(code))
    }

    /// Sheet positions in index order: disciplines in scheme order, then record order; sheets of a
    /// discipline that the scheme no longer lists come last.
    fn ordered(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.sheets.len());
        for d in &self.scheme.disciplines {
            out.extend(self.sheets.iter().enumerate().filter(|(_, s)| s.discipline.eq_ignore_ascii_case(&d.code)).map(|(i, _)| i));
        }
        let seen: BTreeSet<usize> = out.iter().copied().collect();
        out.extend((0..self.sheets.len()).filter(|i| !seen.contains(i)));
        out
    }

    fn number_taken(&self, number: &str, except: Option<u32>) -> bool {
        self.sheets.iter().any(|s| s.number.eq_ignore_ascii_case(number) && Some(s.id) != except)
    }

    /// The first number of the discipline's series that no sheet uses.
    fn next_number(&self, cmd: &str, code: &str) -> Result<String> {
        let disc = self.discipline(code).ok_or_else(|| bad(cmd, format!("no discipline `{code}` in the numbering scheme")))?;
        for i in 0..=self.sheets.len() {
            let n = u64::from(disc.start).saturating_add(u64::from(self.scheme.step).saturating_mul(i as u64));
            let num = format_number(&self.scheme, &disc.code, n);
            if !self.number_taken(&num, None) {
                return Ok(num);
            }
        }
        Err(bad(cmd, "no free sheet number in that discipline"))
    }
}

fn format_number(scheme: &Scheme, code: &str, n: u64) -> String {
    let seq = format!("{n:0width$}", width = scheme.width);
    scheme.pattern.replace("{D}", code).replace("{N}", &seq)
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

fn need_text(cmd: &str, p: &Value, key: &str) -> Result<String> {
    match opt_text(cmd, p, key)? {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(bad(cmd, format!("`{key}` is required"))),
    }
}

fn number_ok(cmd: &str, n: &str) -> Result<String> {
    let t = n.trim();
    if t.is_empty() || t.chars().count() > 32 || !t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')) {
        return Err(bad(cmd, "a sheet number is 1..32 letters, digits and - _ . space"));
    }
    Ok(t.to_string())
}

fn code_ok(cmd: &str, c: &str) -> Result<String> {
    let t = c.trim().to_ascii_uppercase();
    if t.is_empty() || t.len() > 4 || !t.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(bad(cmd, "a discipline code is 1..4 letters or digits"));
    }
    Ok(t)
}

fn date_ok(cmd: &str, key: &str, d: &str) -> Result<String> {
    let t = d.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    let b = t.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    let part = |r: std::ops::Range<usize>| t.get(r).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    let shape = b.len() == 10 && digits(0..4) && b.get(4) == Some(&b'-') && digits(5..7) && b.get(7) == Some(&b'-') && digits(8..10);
    if !shape || !(1..=12).contains(&part(5..7)) || !(1..=31).contains(&part(8..10)) {
        return Err(bad(cmd, format!("`{key}` must be a date like 2026-11-11")));
    }
    Ok(t.to_string())
}

fn opt_date(cmd: &str, p: &Value, key: &str) -> Result<Option<String>> {
    match opt_text(cmd, p, key)? {
        None => Ok(None),
        Some(t) => date_ok(cmd, key, &t).map(Some),
    }
}

fn status_ok(cmd: &str, v: &str) -> Result<String> {
    let t = v.trim().to_ascii_lowercase();
    if STATUSES.contains(&t.as_str()) { Ok(t) } else { Err(bad(cmd, format!("`status` is one of {}", STATUSES.join(", ")))) }
}

/// `1:100`, `100` or `1/100` → `1:100`; empty clears.
fn scale_ok(cmd: &str, v: &str) -> Result<String> {
    let t = v.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    let tail = t.split_once([':', '/']).map_or(t, |(a, b)| if a.trim() == "1" { b } else { "" });
    match tail.trim().parse::<u32>() {
        Ok(n) if (1..=10_000).contains(&n) => Ok(format!("1:{n}")),
        _ => Err(bad(cmd, "`scale` must be like 1:50 (1..10000)")),
    }
}

fn paper_ok(cmd: &str, v: &str) -> Result<String> {
    let t = v.trim().to_ascii_uppercase();
    if PAPERS.contains(&t.as_str()) { Ok(t) } else { Err(bad(cmd, format!("`paper` must be one of {}", PAPERS.join(", ")))) }
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

fn today() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        String::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let z = secs / 86_400 + 719_468;
        let era = z / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + u64::from(month <= 2);
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// A letter code: 0 → A, 25 → Z, 26 → AA …
fn letters(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, char::from(b'A' + (n % 26) as u8));
        if n < 26 {
            return s;
        }
        n = n / 26 - 1;
    }
}

fn parse_source(s: &Session, cmd: &str, v: &Value) -> Result<Source> {
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or("custom").trim().to_string();
    let mut src = Source { kind: kind.clone(), ..Source::default() };
    match kind.as_str() {
        "custom" => {}
        "level" => {
            let lv = v.get("level").ok_or_else(|| bad(cmd, "a level source needs `level`"))?;
            src.level = Some(super::levels::find(s, cmd, &json!({ "level": lv }), "level")?.0);
        }
        "composeView" => {
            let n = v.get("composeView").and_then(Value::as_str).ok_or_else(|| bad(cmd, "a composeView source needs `composeView`"))?;
            src.compose_view =
                Some(s.doc()?.compose_view_by_name(n).map(|c| c.name.clone()).ok_or_else(|| bad(cmd, format!("no compose view `{n}`")))?);
        }
        "view" => {
            let n = v.get("view").and_then(Value::as_str).ok_or_else(|| bad(cmd, "a view source needs `view`"))?;
            src.view = Some(clean(cmd, "view", n, 64)?);
        }
        _ => return Err(bad(cmd, "`source.kind` is custom, level, composeView or view")),
    }
    Ok(src)
}

fn load(s: &Session) -> Result<Index> {
    Ok(Index::load(s.doc()?))
}

fn store(s: &mut Session, idx: &Index) -> Result<()> {
    idx.save(s.doc_mut()?);
    Ok(())
}

fn sheet_json(d: &Drawing, sh: &Sheet) -> Value {
    json!({
        "id": sh.id, "number": sh.number, "discipline": sh.discipline, "title": sh.title,
        "layout": sh.layout, "layoutExists": !sh.layout.is_empty() && d.layout(&sh.layout).is_some(),
        "scale": sh.scale, "paper": sh.paper, "status": sh.status,
        "drawn": sh.drawn, "checked": sh.checked, "approved": sh.approved, "date": sh.date,
        "rev": sh.rev(), "issue": sh.issue(), "source": sh.source, "revisions": sh.revisions.len(),
    })
}

// ---------------------------------------------------------------- title block

/// The title-block fields a sheet record supplies.
fn field_map(d: &Drawing, idx: &Index, sh: &Sheet) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: &str| {
        m.insert(k.to_string(), v.to_string());
    };
    put("SHEET", &sh.number);
    put("TITLE", &sh.title);
    put("SCALE", &sh.scale);
    put("REV", &sh.rev());
    put("DRAWN", &sh.drawn);
    put("CHECKED", &sh.checked);
    put("APPROVED", &sh.approved);
    put("DATE", &sh.date);
    put("DISCIPLINE", idx.discipline(&sh.discipline).map_or(sh.discipline.as_str(), |x| x.name.as_str()));
    put("STATUS", &sh.status);
    put("PAPER", &sh.paper);
    put("ISSUE", &sh.issue());
    if let Some(l) = sh.source.level.and_then(|id| d.level(LevelId(id))) {
        put("FLOOR", &l.name);
    }
    m
}

/// Write the record's fields into the sheet's title block; the count of attributes changed.
fn fill_sheet(s: &mut Session, idx: &Index, sh: &Sheet) -> Result<usize> {
    if sh.layout.is_empty() || s.doc()?.layout(&sh.layout).is_none() {
        return Ok(0);
    }
    let fields = field_map(s.doc()?, idx, sh);
    Ok(super::sheet::write_fields(s.doc_mut()?, &sh.layout, &fields))
}

// ---------------------------------------------------------------- sheet records

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.list";
    let idx = load(s)?;
    let d = s.doc()?;
    let disc = opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty());
    let status = opt_text(cmd, p, "status")?.filter(|t| !t.is_empty());
    let rows: Vec<Value> = idx
        .ordered()
        .into_iter()
        .filter_map(|i| idx.sheets.get(i))
        .filter(|sh| disc.as_deref().is_none_or(|x| sh.discipline.eq_ignore_ascii_case(x)))
        .filter(|sh| status.as_deref().is_none_or(|x| sh.status.eq_ignore_ascii_case(x)))
        .map(|sh| sheet_json(d, sh))
        .collect();
    Ok(json!({ "sheets": rows, "disciplines": idx.scheme.disciplines, "scheme": idx.scheme }))
}

/// Fields shared by `sheet.new` and `sheet.set`, applied to `sh`.
fn apply_fields(s: &Session, cmd: &str, p: &Value, idx: &Index, sh: &mut Sheet) -> Result<()> {
    if let Some(t) = opt_text(cmd, p, "title")? {
        sh.title = t;
    }
    if let Some(t) = opt_text(cmd, p, "scale")? {
        sh.scale = scale_ok(cmd, &t)?;
    }
    if let Some(t) = opt_text(cmd, p, "paper")? {
        sh.paper = if t.is_empty() { t } else { paper_ok(cmd, &t)? };
    }
    if let Some(t) = opt_text(cmd, p, "status")? {
        sh.status = status_ok(cmd, &t)?;
    }
    if let Some(t) = opt_text(cmd, p, "drawn")? {
        sh.drawn = t;
    }
    if let Some(t) = opt_text(cmd, p, "checked")? {
        sh.checked = t;
    }
    if let Some(t) = opt_text(cmd, p, "approved")? {
        sh.approved = t;
    }
    if let Some(t) = opt_date(cmd, p, "date")? {
        sh.date = t;
    }
    if let Some(src) = p.get("source").filter(|v| !v.is_null()) {
        sh.source = parse_source(s, cmd, src)?;
    }
    if let Some(l) = opt_text(cmd, p, "layout")? {
        if !l.is_empty() {
            let name = s.doc()?.layout(&l).map(|x| x.name.clone()).ok_or_else(|| bad(cmd, format!("no layout `{l}`")))?;
            if idx.sheets.iter().any(|o| o.id != sh.id && o.layout.eq_ignore_ascii_case(&name)) {
                return Err(bad(cmd, format!("layout `{name}` already belongs to another sheet")));
            }
            sh.layout = name;
        } else {
            sh.layout.clear();
        }
    }
    Ok(())
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.new";
    let mut idx = load(s)?;
    if idx.sheets.len() >= MAX_SHEETS {
        return Err(bad(cmd, format!("at most {MAX_SHEETS} sheets")));
    }
    let title = need_text(cmd, p, "title")?;
    let disc = match opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
        Some(c) => code_ok(cmd, &c)?,
        None => "A".to_string(),
    };
    let number = match opt_text(cmd, p, "number")?.filter(|t| !t.is_empty()) {
        Some(n) => number_ok(cmd, &n)?,
        None => idx.next_number(cmd, &disc)?,
    };
    if idx.discipline(&disc).is_none() {
        return Err(bad(cmd, format!("no discipline `{disc}` in the numbering scheme (sheet.scheme)")));
    }
    if idx.number_taken(&number, None) {
        return Err(bad(cmd, format!("sheet number `{number}` is already used")));
    }
    let mut sh = Sheet {
        id: idx.seed,
        number,
        discipline: disc,
        title,
        status: "draft".into(),
        source: Source { kind: "custom".into(), ..Source::default() },
        ..Sheet::default()
    };
    apply_fields(s, cmd, p, &idx, &mut sh)?;
    if sh.date.is_empty() {
        sh.date = today();
    }
    idx.seed = idx.seed.saturating_add(1);
    if !sh.layout.is_empty() && sh.paper.is_empty() {
        sh.paper = s.doc()?.layout(&sh.layout).map(|l| l.page.paper.clone()).unwrap_or_default();
    }
    fill_sheet(s, &idx, &sh)?;
    let out = sheet_json(s.doc()?, &sh);
    idx.sheets.push(sh);
    store(s, &idx)?;
    Ok(json!({ "sheet": out }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.set";
    let mut idx = load(s)?;
    let at = idx.find(cmd, p.get("sheet").ok_or_else(|| bad(cmd, "`sheet` is required"))?)?;
    let mut sh = idx.sheets.get(at).cloned().ok_or_else(|| bad(cmd, "sheet went missing"))?;
    if let Some(n) = opt_text(cmd, p, "number")?.filter(|t| !t.is_empty()) {
        let n = number_ok(cmd, &n)?;
        if idx.number_taken(&n, Some(sh.id)) {
            return Err(bad(cmd, format!("sheet number `{n}` is already used")));
        }
        sh.number = n;
    }
    if let Some(c) = opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
        let c = code_ok(cmd, &c)?;
        if idx.discipline(&c).is_none() {
            return Err(bad(cmd, format!("no discipline `{c}` in the numbering scheme")));
        }
        sh.discipline = c;
    }
    apply_fields(s, cmd, p, &idx, &mut sh)?;
    let changed = fill_sheet(s, &idx, &sh)?;
    let out = sheet_json(s.doc()?, &sh);
    if let Some(slot) = idx.sheets.get_mut(at) {
        *slot = sh;
    }
    store(s, &idx)?;
    Ok(json!({ "sheet": out, "titleBlock": changed }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.delete";
    let mut idx = load(s)?;
    let at = idx.find(cmd, p.get("sheet").ok_or_else(|| bad(cmd, "`sheet` is required"))?)?;
    let sh = idx.sheets.remove(at);
    if bool_or(p, "layout", false) && !sh.layout.is_empty() && s.doc()?.layout(&sh.layout).is_some() {
        s.execute("layout.delete", &json!({ "name": sh.layout }))?;
    }
    store(s, &idx)?;
    Ok(json!({ "deleted": sh.number }))
}

fn plan_renumber(idx: &Index, only: Option<&str>) -> Result<Vec<(usize, String)>> {
    let step = u64::from(idx.scheme.step);
    let mut changes = Vec::new();
    for d in &idx.scheme.disciplines {
        if only.is_some_and(|o| !o.eq_ignore_ascii_case(&d.code)) {
            continue;
        }
        let mut k = 0u64;
        for (i, sh) in idx.sheets.iter().enumerate().filter(|(_, x)| x.discipline.eq_ignore_ascii_case(&d.code)) {
            if sh.status == "void" {
                continue;
            }
            let n = u64::from(d.start).checked_add(step.checked_mul(k).ok_or_else(|| bad("sheet.renumber", "the sheet numbers overflow"))?);
            let n = n.ok_or_else(|| bad("sheet.renumber", "the sheet numbers overflow"))?;
            k += 1;
            let num = format_number(&idx.scheme, &d.code, n);
            if num != sh.number {
                changes.push((i, num));
            }
        }
    }
    // The result must be unique: check the final numbers, so a swap is fine and a clash is refused.
    let mut finals: Vec<String> = idx.sheets.iter().map(|s| s.number.to_ascii_lowercase()).collect();
    for (i, n) in &changes {
        if let Some(f) = finals.get_mut(*i) {
            *f = n.to_ascii_lowercase();
        }
    }
    let mut seen = BTreeSet::new();
    if let Some(dup) = finals.iter().find(|f| !seen.insert((*f).clone())) {
        return Err(bad("sheet.renumber", format!("number `{dup}` would be used twice; nothing was changed")));
    }
    Ok(changes)
}

fn run_renumber(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.renumber";
    let mut idx = load(s)?;
    let only = opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty());
    if let Some(o) = &only
        && idx.discipline(o).is_none()
    {
        return Err(bad(cmd, format!("no discipline `{o}`")));
    }
    let changes = plan_renumber(&idx, only.as_deref())?;
    let rows: Vec<Value> =
        changes.iter().filter_map(|(i, to)| idx.sheets.get(*i).map(|sh| json!({ "id": sh.id, "from": sh.number, "to": to }))).collect();
    let apply = !bool_or(p, "dryRun", false);
    if apply {
        for (i, n) in &changes {
            if let Some(sh) = idx.sheets.get_mut(*i) {
                sh.number = n.clone();
            }
        }
        for (i, _) in &changes {
            if let Some(sh) = idx.sheets.get(*i).cloned() {
                fill_sheet(s, &idx, &sh)?;
            }
        }
        store(s, &idx)?;
    }
    Ok(json!({ "changes": rows, "applied": apply }))
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.move";
    let mut idx = load(s)?;
    let at = idx.find(cmd, p.get("sheet").ok_or_else(|| bad(cmd, "`sheet` is required"))?)?;
    let anchor = |key: &str| -> Result<Option<u32>> {
        match p.get(key).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(v) => Ok(Some(idx.sheets.get(idx.find(cmd, v)?).map_or(0, |x| x.id))),
        }
    };
    let (before, after) = (anchor("before")?, anchor("after")?);
    let position = match p.get("position").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => Some(v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(cmd, "`position` must be a whole number of 0 or more"))?),
    };
    if [before.is_some(), after.is_some(), position.is_some()].iter().filter(|b| **b).count() > 1 {
        return Err(bad(cmd, "give only one of `before`, `after`, `position`"));
    }
    let mut sh = idx.sheets.remove(at);
    if let Some(c) = opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
        let c = code_ok(cmd, &c)?;
        if !c.eq_ignore_ascii_case(&sh.discipline) {
            sh.number = idx.next_number(cmd, &c)?;
            sh.discipline = c;
        }
    }
    let same = |o: &Sheet| o.discipline.eq_ignore_ascii_case(&sh.discipline);
    let target = match (before, after, position) {
        (Some(id), _, _) | (_, Some(id), _) => {
            let i = idx.sheets.iter().position(|o| o.id == id).ok_or_else(|| bad(cmd, "the anchor sheet is the sheet itself"))?;
            if idx.sheets.get(i).is_some_and(|o| !same(o)) {
                return Err(bad(cmd, "the anchor sheet is in another discipline"));
            }
            if after.is_some() { i + 1 } else { i }
        }
        (_, _, Some(pos)) => {
            let ours: Vec<usize> = idx.sheets.iter().enumerate().filter(|(_, o)| same(o)).map(|(i, _)| i).collect();
            match ours.get(pos) {
                Some(i) => *i,
                None => ours.last().map_or(idx.sheets.len(), |i| i + 1),
            }
        }
        _ => idx.sheets.len(),
    };
    let target = target.min(idx.sheets.len());
    let id = sh.id;
    idx.sheets.insert(target, sh);
    if bool_or(p, "renumber", false) {
        let disc = idx.sheets.get(target).map(|x| x.discipline.clone());
        let changes = plan_renumber(&idx, disc.as_deref())?;
        for (i, n) in changes {
            if let Some(x) = idx.sheets.get_mut(i) {
                x.number = n;
            }
        }
    }
    if let Some(sh) = idx.sheets.iter().find(|x| x.id == id).cloned() {
        fill_sheet(s, &idx, &sh)?;
    }
    store(s, &idx)?;
    let order: Vec<String> = idx.ordered().into_iter().filter_map(|i| idx.sheets.get(i)).map(|x| x.number.clone()).collect();
    let number = idx.sheets.iter().find(|x| x.id == id).map(|x| x.number.clone()).unwrap_or_default();
    Ok(json!({ "sheet": number, "order": order }))
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

// ---------------------------------------------------------------- build

/// A section or elevation that can be placed on a sheet.
pub(crate) struct ViewSource {
    pub name: String,
}

/// The section and elevation view spaces the drawing has. A view that does not exist is reported as
/// "not available" instead of failing.
pub(crate) fn view_sources(s: &Session) -> Vec<ViewSource> {
    match s.doc() {
        Ok(d) => d.view_spaces.iter().map(|v| ViewSource { name: v.rec.name.clone() }).collect(),
        Err(_) => Vec::new(),
    }
}

fn in_layout<T>(s: &mut Session, layout: &str, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
    let prev = s.layout_space();
    s.execute("layout.set", &json!({ "name": layout }))?;
    let r = f(s);
    let back = match prev {
        Space::Paper(n) => n,
        Space::Model => "Model".to_string(),
    };
    let _ = s.execute("layout.set", &json!({ "name": back }));
    r
}

fn layout_label(d: &Drawing, number: &str, title: &str) -> String {
    let raw = if title.is_empty() { number.to_string() } else { format!("{number} {title}") };
    let base: String =
        raw.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') { c } else { '_' }).take(80).collect();
    let base = base.trim().to_string();
    let base = if base.is_empty() || base.eq_ignore_ascii_case("model") { "Sheet".to_string() } else { base };
    let mut name = base.clone();
    let mut n = 2;
    while d.layout(&name).is_some() && n < 10_000 {
        name = format!("{base} ({n})");
        n += 1;
    }
    name
}

struct Job {
    title: String,
    source: Source,
}

struct BuildOpts {
    scale: String,
    paper: Option<String>,
    landscape: bool,
    master: Option<String>,
}

/// Make the layout of one sheet (page, master, one viewport bound to the source) and return its name.
fn make_layout(s: &mut Session, number: &str, job: &Job, o: &BuildOpts, warnings: &mut Vec<String>) -> Result<String> {
    let name = layout_label(s.doc()?, number, &job.title);
    s.execute("layout.new", &json!({ "name": name, "viewport": false }))?;
    let name = s.doc()?.layout(&name).map(|l| l.name.clone()).unwrap_or(name);
    if let Some(m) = &o.master {
        s.execute("master.apply", &json!({ "master": m, "layout": name }))?;
    } else if let Some(paper) = &o.paper {
        let (a, b) = paper_mm(paper);
        let (w, h) = if o.landscape { (a.max(b), a.min(b)) } else { (a.min(b), a.max(b)) };
        if let Some(l) = s.doc_mut()?.layouts.iter_mut().find(|l| l.name == name) {
            l.page = PageSetup { paper: paper.clone(), width_mm: w, height_mm: h, landscape: o.landscape, ..PageSetup::default() };
        }
    }
    let made = s.execute("mview", &json!({ "layout": name, "count": 1 }))?;
    let handle = made.get("viewports").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string);
    if let Some(h) = handle {
        let mut set = Map::new();
        set.insert("handle".into(), json!(h));
        if !o.scale.is_empty() {
            set.insert("scale".into(), json!(o.scale));
        }
        match job.source.kind.as_str() {
            "level" => {
                let lname = job.source.level.and_then(|id| s.doc().ok().and_then(|d| d.level(LevelId(id)).map(|l| l.name.clone())));
                if let Some(n) = lname {
                    set.insert("level".into(), json!(n));
                }
            }
            "view" => warnings.push(format!(
                "{}: view `{}` is not available yet; the sheet has an unbound viewport",
                job.title,
                job.source.view.clone().unwrap_or_default()
            )),
            _ => {}
        }
        s.execute("viewport.set", &Value::Object(set))?;
        if job.source.kind == "composeView"
            && let Some(cv) = &job.source.compose_view
        {
            s.execute("compose.view.apply", &json!({ "view": cv, "handle": h }))?;
        }
    }
    s.execute("sheet.titleblock", &json!({ "layout": name }))?;
    Ok(name)
}

fn run_build(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheetindex.build";
    let mut idx = load(s)?;
    let disc = match opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
        Some(c) => code_ok(cmd, &c)?,
        None => "A".to_string(),
    };
    if idx.discipline(&disc).is_none() {
        return Err(bad(cmd, format!("no discipline `{disc}` in the numbering scheme")));
    }
    let scale = match opt_text(cmd, p, "scale")? {
        Some(t) => scale_ok(cmd, &t)?,
        None => "1:100".to_string(),
    };
    let paper = match opt_text(cmd, p, "paper")?.filter(|t| !t.is_empty()) {
        Some(t) => Some(paper_ok(cmd, &t)?),
        None => None,
    };
    let master = match opt_text(cmd, p, "master")?.filter(|t| !t.is_empty()) {
        Some(m) => Some(s.doc()?.master_layout_by_name(&m).map(|x| x.name.clone()).ok_or_else(|| bad(cmd, format!("no master layout `{m}`")))?),
        None => None,
    };
    let opts = BuildOpts { scale, paper, landscape: bool_or(p, "landscape", true), master };

    let mut jobs: Vec<Job> = Vec::new();
    if bool_or(p, "levels", true) {
        let d = s.doc()?;
        if d.levels.is_empty() {
            jobs.push(Job { title: "Plan".into(), source: Source { kind: "level".into(), ..Source::default() } });
        }
        for l in &d.levels {
            jobs.push(Job { title: format!("{} Plan", l.name), source: Source { kind: "level".into(), level: Some(l.id.0), ..Source::default() } });
        }
    }
    if bool_or(p, "composeViews", false) {
        for v in &s.doc()?.compose_views {
            jobs.push(Job {
                title: v.name.clone(),
                source: Source { kind: "composeView".into(), compose_view: Some(v.name.clone()), ..Source::default() },
            });
        }
    }
    match p.get("views") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) if a.len() <= 128 => {
            for v in a {
                let n = v.as_str().ok_or_else(|| bad(cmd, "`views` is a list of view names"))?;
                let n = clean(cmd, "views", n, 64)?;
                jobs.push(Job { title: n.clone(), source: Source { kind: "view".into(), view: Some(n), ..Source::default() } });
            }
        }
        Some(_) => return Err(bad(cmd, "`views` is a list of at most 128 view names")),
    }
    if jobs.len() > MAX_SHEETS.saturating_sub(idx.sheets.len()) {
        return Err(bad(cmd, format!("that would make more than {MAX_SHEETS} sheets")));
    }
    let known: BTreeSet<String> = view_sources(s).into_iter().map(|v| v.name.to_ascii_lowercase()).collect();

    let (mut created, mut skipped, mut warnings) = (Vec::new(), Vec::new(), Vec::new());
    for job in jobs {
        let exists = idx.sheets.iter().any(|sh| {
            sh.source.kind == job.source.kind
                && match job.source.kind.as_str() {
                    "level" => sh.source.level == job.source.level,
                    "composeView" => sh.source.compose_view == job.source.compose_view,
                    _ => sh.source.view == job.source.view,
                }
        });
        if exists {
            skipped.push(json!({ "title": job.title, "reason": "already has a sheet" }));
            continue;
        }
        let mut job = job;
        if job.source.kind == "view" && !job.source.view.as_ref().is_some_and(|v| known.contains(&v.to_ascii_lowercase())) {
            warnings.push(format!("view `{}` does not exist; its sheet is made without content", job.title));
            job.source.view = job.source.view.take();
        }
        let number = idx.next_number(cmd, &disc)?;
        let layout = make_layout(s, &number, &job, &opts, &mut warnings)?;
        let paper = s.doc()?.layout(&layout).map(|l| l.page.paper.clone()).unwrap_or_default();
        let sh = Sheet {
            id: idx.seed,
            number: number.clone(),
            discipline: disc.clone(),
            title: job.title.clone(),
            layout: layout.clone(),
            scale: opts.scale.clone(),
            paper,
            status: "draft".into(),
            date: today(),
            source: job.source,
            ..Sheet::default()
        };
        idx.seed = idx.seed.saturating_add(1);
        fill_sheet(s, &idx, &sh)?;
        idx.sheets.push(sh);
        store(s, &idx)?; // keep the record in step with the layout made so far
        created.push(json!({ "number": number, "title": job.title, "layout": layout }));
    }
    let table = if bool_or(p, "table", false) { Some(draw_table(s, "sheets", None, None, None)?) } else { None };
    Ok(json!({ "created": created, "skipped": skipped, "warnings": warnings, "table": table }))
}

fn run_fill(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheetindex.fill";
    let idx = load(s)?;
    let picked: Vec<Sheet> = match p.get("sheet").filter(|v| !v.is_null()) {
        Some(v) => idx.sheets.get(idx.find(cmd, v)?).cloned().into_iter().collect(),
        None => idx.ordered().into_iter().filter_map(|i| idx.sheets.get(i).cloned()).collect(),
    };
    let mut rows = Vec::new();
    for sh in picked {
        let changed = fill_sheet(s, &idx, &sh)?;
        rows.push(json!({ "sheet": sh.number, "layout": sh.layout, "changed": changed }));
    }
    Ok(json!({ "filled": rows }))
}

// ---------------------------------------------------------------- tables

struct Row {
    cells: Vec<String>,
    band: bool,
}

fn table_rows(idx: &Index, kind: &str) -> (Vec<&'static str>, Vec<Row>) {
    let mut rows = Vec::new();
    if kind == "revisions" {
        let mut all: Vec<(&Sheet, &Revision)> = idx.sheets.iter().flat_map(|sh| sh.revisions.iter().map(move |r| (sh, r))).collect();
        all.sort_by(|a, b| b.1.date.cmp(&a.1.date).then_with(|| a.0.number.cmp(&b.0.number)));
        for (sh, r) in all {
            rows.push(Row {
                cells: vec![sh.number.clone(), r.rev.clone(), r.date.clone(), r.description.clone(), r.by.clone(), r.issue.clone()],
                band: false,
            });
        }
        return (vec!["SHEET", "REV", "DATE", "DESCRIPTION", "BY", "ISSUE"], rows);
    }
    let order = idx.ordered();
    let mut last: Option<String> = None;
    for sh in order.into_iter().filter_map(|i| idx.sheets.get(i)) {
        let key = sh.discipline.to_ascii_uppercase();
        if last.as_deref() != Some(key.as_str()) {
            let name = idx.discipline(&sh.discipline).map_or(sh.discipline.clone(), |d| format!("{} - {}", d.code, d.name.to_uppercase()));
            rows.push(Row { cells: vec![name], band: true });
            last = Some(key);
        }
        rows.push(Row {
            cells: vec![sh.number.clone(), sh.title.clone(), sh.scale.clone(), sh.paper.clone(), sh.rev(), sh.status.clone()],
            band: false,
        });
    }
    (vec!["SHEET NO.", "TITLE", "SCALE", "PAPER", "REV", "STATUS"], rows)
}

fn draw_table(s: &mut Session, kind: &str, layout: Option<&str>, at: Option<Vec2>, height: Option<f64>) -> Result<Value> {
    let cmd = "sheetindex.table";
    let mut idx = load(s)?;
    let layout = layout.map_or_else(|| "Sheet Index".to_string(), str::to_string);
    if s.doc()?.layout(&layout).is_none() {
        s.execute("layout.new", &json!({ "name": layout, "viewport": false }))?;
    }
    let layout = s.doc()?.layout(&layout).map(|l| l.name.clone()).ok_or_else(|| bad(cmd, "the layout could not be made"))?;
    let sh = aad_render::sheet(s.doc()?, &layout).ok_or_else(|| bad(cmd, format!("no layout `{layout}`")))?;
    let k = 1.0 / sh.unit_mm;
    let th = height.unwrap_or(2.5) * k;
    let at = at.unwrap_or_else(|| Vec2::new(sh.printable.min.x + 10.0 * k, sh.printable.max.y - 10.0 * k));
    let (header, body) = table_rows(&idx, kind);
    if body.len() > 2100 {
        return Err(bad(cmd, "the table would have too many rows"));
    }
    let mut widths: Vec<f64> = header.iter().map(|h| h.chars().count() as f64).collect();
    for r in body.iter().filter(|r| !r.band) {
        for (w, c) in widths.iter_mut().zip(&r.cells) {
            *w = w.max(c.chars().count().min(60) as f64);
        }
    }
    let widths: Vec<f64> = widths.iter().map(|w| w * th * 0.75 + th * 1.2).collect();
    let total: f64 = widths.iter().sum();
    let rh = th * 2.0;

    let old: Vec<aad_doc::Handle> =
        idx.tables.get(&format!("{kind}\\{layout}")).map(|v| v.iter().filter_map(|h| aad_doc::Handle::parse_hex(h)).collect()).unwrap_or_default();
    let space = Space::Paper(layout.clone());
    for h in old {
        if s.doc()?.space(&space).is_some_and(|st| st.get(h).is_some()) {
            s.doc_mut()?.remove_entity(h);
        }
    }
    let all_rows: Vec<Row> = std::iter::once(Row { cells: header.iter().map(|h| (*h).to_string()).collect(), band: false }).chain(body).collect();
    let nrows = all_rows.len();
    let handles = in_layout(s, &layout, |s| {
        super::hcw::ensure_layer(s.doc_mut()?, TABLE_LAYER, 7, "Continuous", 18, "Sheet index");
        let mut hs = Vec::new();
        let mut y = at.y;
        for (ri, row) in all_rows.iter().enumerate() {
            let mut x = at.x;
            for (ci, w) in widths.iter().enumerate() {
                let Some(cell) = row.cells.get(ci) else {
                    x += w;
                    continue;
                };
                let h = if ri == 0 || row.band { th } else { th * 0.9 };
                let kind = make_text(s, Vec2::new(x + th * 0.4, y - rh * 0.5), cell, h, 0.0, (HAlign::Left, VAlign::Middle));
                hs.push(super::hcw::add_on(s, TABLE_LAYER, kind)?);
                x += w;
                if row.band {
                    break;
                }
            }
            // Vertical rules: a band row has only the two outer ones.
            let stops: Vec<f64> = if row.band {
                vec![0.0, total]
            } else {
                std::iter::once(0.0)
                    .chain(widths.iter().scan(0.0, |acc, w| {
                        *acc += w;
                        Some(*acc)
                    }))
                    .collect()
            };
            for w in stops {
                hs.push(super::hcw::add_on(s, TABLE_LAYER, line(Vec2::new(at.x + w, y), Vec2::new(at.x + w, y - rh)))?);
            }
            y -= rh;
        }
        for i in 0..=nrows {
            let yy = at.y - rh * i as f64;
            hs.push(super::hcw::add_on(s, TABLE_LAYER, line(Vec2::new(at.x, yy), Vec2::new(at.x + total, yy)))?);
        }
        Ok(hs)
    })?;
    let n = handles.len();
    idx.tables.insert(format!("{kind}\\{layout}"), handles.iter().map(|h| h.hex()).collect());
    store(s, &idx)?;
    Ok(json!({ "layout": layout, "objects": n }))
}

fn run_table(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheetindex.table";
    let kind = match str_param(p, "kind").unwrap_or("sheets") {
        "sheets" => "sheets",
        "revisions" => "revisions",
        _ => return Err(bad(cmd, "`kind` is sheets or revisions")),
    };
    let height = match p.get("height").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            Some(v.as_f64().filter(|h| h.is_finite() && (0.5..=50.0).contains(h)).ok_or_else(|| bad(cmd, "`height` must be between 0.5 and 50 mm"))?)
        }
    };
    if p.get("at").is_some_and(|v| !v.is_null()) && point_param(p, "at").is_none() {
        return Err(bad(cmd, "`at` must be [x, y]"));
    }
    let layout = opt_text(cmd, p, "layout")?.filter(|t| !t.is_empty());
    draw_table(s, kind, layout.as_deref(), point_param(p, "at"), height)
}

// ---------------------------------------------------------------- revisions

fn rev_json(sh: &Sheet, r: &Revision) -> Value {
    json!({ "sheet": sh.number, "rev": r.rev, "date": r.date, "description": r.description, "by": r.by, "issue": r.issue })
}

/// Add a revision entry to the sheet at `at`; the code used.
fn add_revision(idx: &mut Index, cmd: &str, at: usize, rev: Option<String>, r: Revision) -> Result<String> {
    let sh = idx.sheets.get_mut(at).ok_or_else(|| bad(cmd, "sheet went missing"))?;
    if sh.revisions.len() >= MAX_REVISIONS {
        return Err(bad(cmd, format!("sheet {} already has {MAX_REVISIONS} revisions", sh.number)));
    }
    let code = match rev.filter(|t| !t.is_empty()) {
        Some(c) => {
            if c.chars().count() > 8 {
                return Err(bad(cmd, "`rev` is at most 8 characters"));
            }
            c
        }
        None => (0..=MAX_REVISIONS)
            .map(letters)
            .find(|c| !sh.revisions.iter().any(|x| x.rev.eq_ignore_ascii_case(c)))
            .ok_or_else(|| bad(cmd, "no free revision letter"))?,
    };
    if sh.revisions.iter().any(|x| x.rev.eq_ignore_ascii_case(&code)) {
        return Err(bad(cmd, format!("sheet {} already has revision `{code}`", sh.number)));
    }
    sh.revisions.push(Revision { rev: code.clone(), ..r });
    Ok(code)
}

fn run_rev_add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.revision.add";
    let mut idx = load(s)?;
    let at = idx.find(cmd, p.get("sheet").ok_or_else(|| bad(cmd, "`sheet` is required"))?)?;
    let description = need_text(cmd, p, "description")?;
    let date = opt_date(cmd, p, "date")?.filter(|d| !d.is_empty()).unwrap_or_else(today);
    let by = opt_text(cmd, p, "by")?.unwrap_or_default();
    let issue = opt_text(cmd, p, "issue")?.unwrap_or_default();
    let rev = opt_text(cmd, p, "rev")?;
    let code = add_revision(&mut idx, cmd, at, rev, Revision { rev: String::new(), date, description, by, issue })?;
    if let Some(sh) = idx.sheets.get(at).cloned() {
        fill_sheet(s, &idx, &sh)?;
    }
    store(s, &idx)?;
    let number = idx.sheets.get(at).map(|x| x.number.clone()).unwrap_or_default();
    Ok(json!({ "sheet": number, "rev": code }))
}

fn run_rev_list(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.revision.list";
    let idx = load(s)?;
    let picked: Vec<&Sheet> = match p.get("sheet").filter(|v| !v.is_null()) {
        Some(v) => idx.sheets.get(idx.find(cmd, v)?).into_iter().collect(),
        None => idx.sheets.iter().collect(),
    };
    let mut all: Vec<(&Sheet, &Revision)> = picked.into_iter().flat_map(|sh| sh.revisions.iter().rev().map(move |r| (sh, r))).collect();
    all.sort_by(|a, b| b.1.date.cmp(&a.1.date));
    Ok(json!({ "revisions": all.into_iter().map(|(sh, r)| rev_json(sh, r)).collect::<Vec<_>>() }))
}

// ---------------------------------------------------------------- issue sets

fn issue_json(i: &Issue) -> Value {
    json!({
        "id": i.id, "name": i.name, "date": i.date, "rev": i.rev, "description": i.description, "publisherSet": i.publisher_set,
        "sheets": i.frozen.iter().map(|f| json!({ "number": f.number, "title": f.title, "layout": f.layout, "rev": f.rev, "status": f.status })).collect::<Vec<_>>(),
    })
}

fn names_list(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec<String>>> {
    match p.get(key).filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(Value::Array(a)) if a.len() <= MAX_SHEETS => {
            let mut out = Vec::new();
            for v in a {
                out.push(
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                        .ok_or_else(|| bad(cmd, format!("`{key}` is a list of sheet numbers")))?,
                );
            }
            Ok(Some(out))
        }
        Some(_) => Err(bad(cmd, format!("`{key}` is a list of at most {MAX_SHEETS} sheet numbers"))),
    }
}

/// Upsert a Publisher Set with `layouts` in order.
fn upsert_set(s: &mut Session, name: &str, layouts: &[String], extra: &Value) -> Result<()> {
    let exists = s.doc()?.publisher_set_by_name(name).is_some();
    let mut params = Map::new();
    if let Some(o) = extra.as_object() {
        params.extend(o.clone());
    }
    params.insert("layouts".into(), json!(layouts));
    if exists {
        params.insert("set".into(), json!(name));
        s.execute("publisher.set", &Value::Object(params))?;
    } else {
        params.insert("name".into(), json!(name));
        s.execute("publisher.new", &Value::Object(params))?;
    }
    Ok(())
}

fn run_issue_create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "issue.create";
    let mut idx = load(s)?;
    if idx.issues.len() >= MAX_ISSUES {
        return Err(bad(cmd, format!("at most {MAX_ISSUES} issue sets")));
    }
    let name = clean(cmd, "name", &need_text(cmd, p, "name")?, MAX_ISSUE_NAME)?;
    if !aad_doc::compose_name_ok(&format!("Issue {name}")) || idx.issues.iter().any(|i| i.name.eq_ignore_ascii_case(&name)) {
        return Err(bad(cmd, format!("issue `{name}` already exists or the name is not usable")));
    }
    let date = opt_date(cmd, p, "date")?.filter(|d| !d.is_empty()).unwrap_or_else(today);
    let rev = match opt_text(cmd, p, "rev")?.filter(|t| !t.is_empty()) {
        Some(r) => clean(cmd, "rev", &r, 8)?,
        None => letters(idx.issues.len()),
    };
    let description = opt_text(cmd, p, "description")?.unwrap_or_default();
    let ordered = idx.ordered();
    let picked: Vec<usize> = if let Some(names) = names_list(cmd, p, "sheets")? {
        let mut out = Vec::new();
        for n in names {
            let at = idx.find(cmd, &json!(n))?;
            if !out.contains(&at) {
                out.push(at);
            }
        }
        out.sort_by_key(|a| ordered.iter().position(|o| o == a));
        out
    } else if let Some(c) = opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
        ordered.into_iter().filter(|i| idx.sheets.get(*i).is_some_and(|sh| sh.discipline.eq_ignore_ascii_case(&c) && sh.status != "void")).collect()
    } else {
        ordered.into_iter().filter(|i| idx.sheets.get(*i).is_some_and(|sh| sh.status != "void")).collect()
    };
    if picked.is_empty() {
        return Err(bad(cmd, "no sheets to issue"));
    }
    // Check every sheet can take a revision before changing any.
    let stamp = bool_or(p, "stamp", true);
    if stamp {
        for i in &picked {
            if let Some(sh) = idx.sheets.get(*i)
                && (sh.revisions.len() >= MAX_REVISIONS || sh.revisions.iter().any(|r| r.rev.eq_ignore_ascii_case(&rev)))
            {
                return Err(bad(cmd, format!("sheet {} cannot take revision `{rev}` (full or already used); pass another `rev`", sh.number)));
            }
        }
    }
    let mut frozen = Vec::new();
    for i in &picked {
        if stamp {
            let r = Revision {
                rev: rev.clone(),
                date: date.clone(),
                description: if description.is_empty() { name.clone() } else { description.clone() },
                by: String::new(),
                issue: name.clone(),
            };
            add_revision(&mut idx, cmd, *i, Some(rev.clone()), r)?;
        }
        if let Some(sh) = idx.sheets.get_mut(*i) {
            if sh.status != "void" {
                sh.status = "issued".into();
            }
            frozen.push(Frozen {
                sheet: sh.id,
                number: sh.number.clone(),
                title: sh.title.clone(),
                layout: sh.layout.clone(),
                rev: sh.rev(),
                status: sh.status.clone(),
            });
        }
    }
    let set_name = if bool_or(p, "publisherSet", true) {
        let layouts: Vec<String> = frozen
            .iter()
            .filter(|f| !f.layout.is_empty() && s.doc().is_ok_and(|d| d.layout(&f.layout).is_some()))
            .map(|f| f.layout.clone())
            .collect();
        let set = format!("Issue {name}");
        if layouts.is_empty() {
            String::new()
        } else {
            upsert_set(s, &set, &layouts, &json!({}))?;
            set
        }
    } else {
        String::new()
    };
    let issue = Issue { id: idx.issue_seed, name, date, rev, description, frozen, publisher_set: set_name };
    idx.issue_seed = idx.issue_seed.saturating_add(1);
    for i in &picked {
        if let Some(sh) = idx.sheets.get(*i).cloned() {
            fill_sheet(s, &idx, &sh)?;
        }
    }
    let out = issue_json(&issue);
    idx.issues.push(issue);
    store(s, &idx)?;
    Ok(json!({ "issue": out }))
}

fn run_issue_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let idx = load(s)?;
    Ok(json!({ "issues": idx.issues.iter().map(issue_json).collect::<Vec<_>>() }))
}

// ---------------------------------------------------------------- publish

fn run_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheetindex.publish";
    let idx = load(s)?;
    let mut skipped = Vec::new();
    // Layout name and sheet number, in index order.
    let (label, items): (String, Vec<(String, String)>) = if let Some(n) = opt_text(cmd, p, "issue")?.filter(|t| !t.is_empty()) {
        let issue = idx.issues.iter().find(|i| i.name.eq_ignore_ascii_case(&n)).ok_or_else(|| bad(cmd, format!("no issue `{n}`")))?;
        (format!("Issue {}", issue.name), issue.frozen.iter().map(|f| (f.layout.clone(), f.number.clone())).collect())
    } else {
        let ordered = idx.ordered();
        let picked: Vec<usize> = if let Some(names) = names_list(cmd, p, "sheets")? {
            let mut out = Vec::new();
            for n in names {
                out.push(idx.find(cmd, &json!(n))?);
            }
            out.sort_by_key(|a| ordered.iter().position(|o| o == a));
            out.dedup();
            out
        } else if let Some(c) = opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
            if idx.discipline(&c).is_none() {
                return Err(bad(cmd, format!("no discipline `{c}`")));
            }
            ordered.into_iter().filter(|i| idx.sheets.get(*i).is_some_and(|sh| sh.discipline.eq_ignore_ascii_case(&c))).collect()
        } else {
            ordered
        };
        let label = match opt_text(cmd, p, "discipline")?.filter(|t| !t.is_empty()) {
            Some(c) => format!("Discipline {}", c.to_ascii_uppercase()),
            None => "Sheets".to_string(),
        };
        let void = |i: &usize| idx.sheets.get(*i).is_some_and(|sh| sh.status == "void");
        for i in picked.iter().filter(|i| void(i)) {
            if let Some(sh) = idx.sheets.get(*i) {
                skipped.push(json!({ "sheet": sh.number, "reason": "void" }));
            }
        }
        (label, picked.into_iter().filter(|i| !void(i)).filter_map(|i| idx.sheets.get(i)).map(|sh| (sh.layout.clone(), sh.number.clone())).collect())
    };
    let mut layouts = Vec::new();
    for (layout, number) in items {
        if layout.is_empty() {
            skipped.push(json!({ "sheet": number, "reason": "no layout" }));
        } else if s.doc()?.layout(&layout).is_none() {
            skipped.push(json!({ "sheet": number, "reason": "layout missing" }));
        } else {
            layouts.push(layout);
        }
    }
    if layouts.is_empty() {
        return Err(bad(cmd, "there is no sheet with a layout to publish"));
    }
    let mut extra = Map::new();
    if let Some(f) = str_param(p, "format") {
        extra.insert("format".into(), json!(f));
    }
    if let Some(b) = p.get("singleFile").and_then(Value::as_bool) {
        extra.insert("singleFile".into(), json!(b));
    }
    upsert_set(s, &label, &layouts, &Value::Object(extra))?;
    let mut run = Map::new();
    run.insert("set".into(), json!(label));
    for k in ["folder", "checkOnly", "takeoff"] {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            run.insert(k.into(), v.clone());
        }
    }
    let mut out = s.execute("publisher.publish", &Value::Object(run))?;
    if let Some(o) = out.as_object_mut() {
        o.insert("set".into(), json!(label));
        o.insert("skipped".into(), json!(skipped));
    }
    Ok(out)
}

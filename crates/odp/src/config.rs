//! The project configuration kept in the `.osp` (`meta.config.*`), the contract between MynaOps (the only writer) and MynaCAD (reads and applies it).
//! Spec: `docs/ops-first-workflow.md` sections 4.1 to 4.5; the file layout is in `docs/PROJECT-FILE.md`.
//!
//! This part is plain data, parse and validate: it needs no SQLite and builds for the web too. Reading a file, the read-only probe and the writer used
//! by tests and tools are in `config_io.rs` (not built for the web).
//!
//! Rules every reader follows (all tested, see `tests`):
//! * Every key's value is JSON text of at most [`MAX_VALUE_BYTES`]; a longer one is an error before any of it is read.
//! * Every key carries `"v"`. A key whose version this reader does not know is treated as absent and says so in the report; it is never a crash.
//! * Unknown fields are ignored; counts, lengths, numbers and ids are checked, and a part that breaks a rule rejects the whole configuration.
//! * Input is hostile: nothing here indexes, unwraps or allocates by a number the file states.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

/// The version of every key this reader understands.
pub const READER_VERSION: i64 = 1;
/// Most bytes one `config.*` value may have.
pub const MAX_VALUE_BYTES: usize = 256 * 1024;

/// `meta` key prefix of the configuration.
pub const PREFIX: &str = "config.";
/// Short names of the keys MynaOps writes, in the order they are written. The full key is `config.<name>`.
pub const PARTS: [&str; 11] = ["schema", "rev", "project", "team", "units", "levels", "layer_sets", "tabs", "sheets", "naming", "titleblock"];
/// The key MynaCAD writes locally (no revision): what it has applied.
pub const KEY_APPLIED: &str = "config.applied.cad";
/// The key MynaCAD writes locally, once, for a file that has no configuration yet.
pub const KEY_PROPOSAL: &str = "config.proposal";
/// The drawing's project-map key that binds a configured level id to the drawing's `LevelId` (`{"v":1,"map":{"<cid>":<LevelId>}}`).
pub const KEY_LEVELMAP: &str = "config.levelmap";
/// The drawing's project-map key that binds a configured tab id to the drawing's sheet set id (same shape as [`KEY_LEVELMAP`]).
pub const KEY_TABMAP: &str = "config.tabmap";
/// The drawing's project-map key that holds the project-wide title-block defaults MynaCAD applied (`{"v":1,"fields":{...}}`). Not the `.osp` key
/// `config.titleblock`: this one lives in the drawing.
pub const KEY_TITLEBLOCK: &str = "config.titleblock.defaults";

pub const MAX_LEVELS: usize = 64;
pub const MAX_TABS: usize = 32;
pub const MAX_DISCIPLINES: usize = 64;
pub const MAX_PATTERNS: usize = 64;
pub const MAX_PATTERN_LEN: usize = 64;
pub const MAX_NAME: usize = 64;
pub const MAX_TEXT: usize = 256;
pub const MAX_LONG_TEXT: usize = 4096;
pub const MAX_MAP_ENTRIES: usize = 64;
pub const MAX_ITEMS_REPORT: usize = 256;
/// Layer sets `hcwlayers` knows.
pub const LAYER_SETS: [&str; 3] = ["standard", "permit", "presentation"];
/// Drawing units `drawing.units` knows.
pub const UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// A part breaks a rule: `key` is the `meta` key (or a name such as `config.levels.items[2]`).
    #[error("{key}: {msg}")]
    Invalid { key: String, msg: String },
    #[error("{key}: the value is {bytes} bytes; the limit is {max}")]
    TooLarge { key: String, bytes: u64, max: usize },
    /// `config.schema.min_reader` is newer than this reader.
    #[error("the project configuration needs a reader of version {0}; this one reads version {READER_VERSION}: update the program")]
    TooNew(i64),
    /// The file could not be read or written.
    #[error("{0}")]
    File(String),
}

pub type Result<T> = std::result::Result<T, ConfigError>;

fn invalid(key: &str, msg: impl Into<String>) -> ConfigError {
    ConfigError::Invalid { key: key.to_string(), msg: msg.into() }
}

// ---------------------------------------------------------------------------------------------------- the keys

/// `config.schema`: presence means "this project is configured by MynaOps".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub v: i64,
    #[serde(default = "one")]
    pub min_reader: i64,
}

/// `config.rev`: rises by one per configuration change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rev {
    pub v: i64,
    pub rev: u64,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub time: String,
}

/// `config.project`: the `Drawing::project` detail keys (same names), plus `code` and `persona`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub v: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub pid: String,
    #[serde(default)]
    pub site_area: String,
    #[serde(default)]
    pub plot_use: String,
    #[serde(default)]
    pub authority: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub persona: String,
}

/// `config.team`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Team {
    pub v: i64,
    #[serde(default)]
    pub architect: String,
    #[serde(default)]
    pub engineer: String,
    /// The imported company pack: text values by name.
    #[serde(default)]
    pub company: BTreeMap<String, String>,
}

/// `config.units`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Units {
    pub v: i64,
    pub drawing_unit: String,
    /// Set by MynaOps once MynaCAD reports content: the unit can no longer change.
    #[serde(default)]
    pub locked: bool,
}

/// Defaults for levels, millimetres.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelDefaults {
    #[serde(default)]
    pub height_mm: Option<f64>,
    #[serde(default)]
    pub slab_mm: Option<f64>,
    #[serde(default)]
    pub beam_mm: Option<f64>,
    #[serde(default)]
    pub ceiling_mm: Option<f64>,
    #[serde(default)]
    pub lintel_bottom_mm: Option<f64>,
}

/// One configured level. `cid` is MynaOps' id and is never reused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelItem {
    pub cid: u32,
    pub name: String,
    #[serde(default)]
    pub code: String,
    pub elevation_mm: f64,
    pub height_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slab_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beam_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lintel_bottom_mm: Option<f64>,
}

/// `config.levels`: the level list, ordered bottom to top.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Levels {
    pub v: i64,
    pub next_cid: u32,
    #[serde(default)]
    pub one_per_floor: bool,
    #[serde(default)]
    pub defaults: LevelDefaults,
    #[serde(default)]
    pub items: Vec<LevelItem>,
}

/// `config.layer_sets`: which `hcwlayers` sets exist (`standard`, `permit`, `presentation`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerSets {
    pub v: i64,
    #[serde(default)]
    pub sets: Vec<String>,
}

/// One discipline tab (a sheet set). `tid` is never reused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabItem {
    pub tid: u32,
    #[serde(default)]
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub layer_prefix: String,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
}

/// `config.tabs`. `next_tid` is optional (an addition to the first draft of the schema): when absent it is one more than the largest `tid`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tabs {
    pub v: i64,
    #[serde(default)]
    pub next_tid: Option<u32>,
    #[serde(default)]
    pub items: Vec<TabItem>,
}

/// One discipline of the sheet numbering scheme.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Discipline {
    pub code: String,
    pub name: String,
    pub start: u32,
}

/// The sheet numbering scheme: the arguments of `sheet.scheme`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scheme {
    pub pattern: String,
    pub width: u32,
    pub step: u32,
    #[serde(default)]
    pub disciplines: Vec<Discipline>,
}

/// `config.sheets`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheets {
    pub v: i64,
    pub scheme: Scheme,
}

/// File and drawing number patterns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NamingPatterns {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub drawing_number: String,
}

/// `config.naming`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Naming {
    pub v: i64,
    #[serde(default)]
    pub patterns: NamingPatterns,
}

/// `config.titleblock`: project-wide title-block field defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub v: i64,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// One result line of an apply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppliedItem {
    pub key: String,
    /// `ok`, `orphans`, `refused` or `failed`.
    pub status: String,
    #[serde(default)]
    pub detail: String,
}

/// `config.applied.cad`: what MynaCAD has applied. MynaCAD writes it as a local key (no revision); MynaOps shows "MynaCAD has applied revision N".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    pub v: i64,
    pub rev: u64,
    #[serde(default)]
    pub time: String,
    /// `ok`, `partial` or `failed`.
    pub result: String,
    #[serde(default)]
    pub has_content: bool,
    #[serde(default)]
    pub items: Vec<AppliedItem>,
}

/// One level a legacy drawing already has (`cid` is its `LevelId`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposalLevel {
    pub cid: u32,
    pub name: String,
    pub elevation_mm: f64,
    pub height_mm: f64,
}

/// One discipline tab a legacy drawing already has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposalTab {
    /// The sheet set's own id: an adopted tab's `tid`.
    #[serde(default)]
    pub tid: u32,
    pub name: String,
    #[serde(default)]
    pub layer_prefix: String,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
}

/// `config.proposal`: what a legacy drawing already has, written once by MynaCAD (a local key) for MynaOps' Adopt page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub v: i64,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub has_content: bool,
    #[serde(default)]
    pub drawing_unit: Option<String>,
    #[serde(default)]
    pub levels: Vec<ProposalLevel>,
    #[serde(default)]
    pub tabs: Vec<ProposalTab>,
    /// The `hcwlayers` sets whose layers all exist in the drawing.
    #[serde(default)]
    pub layer_sets: Vec<String>,
    #[serde(default)]
    pub project: BTreeMap<String, String>,
    #[serde(default)]
    pub scheme: Option<Scheme>,
}

fn one() -> i64 {
    1
}

// ---------------------------------------------------------------------------------------------------- checks

fn text_ok(key: &str, field: &str, s: &str, max: usize) -> Result<()> {
    if s.chars().count() > max {
        return Err(invalid(key, format!("`{field}` is longer than {max} characters")));
    }
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t') {
        return Err(invalid(key, format!("`{field}` has a control character")));
    }
    Ok(())
}

fn name_ok(key: &str, field: &str, s: &str) -> Result<()> {
    let n = s.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(invalid(key, format!("`{field}` is 1 to {MAX_NAME} characters without control characters")));
    }
    Ok(())
}

fn mm_ok(key: &str, field: &str, v: f64, lo: f64, hi: f64) -> Result<()> {
    if !v.is_finite() || v < lo || v > hi {
        return Err(invalid(key, format!("`{field}` must be a number from {lo} to {hi} millimetres")));
    }
    Ok(())
}

fn opt_mm(key: &str, field: &str, v: Option<f64>, lo: f64, hi: f64) -> Result<()> {
    v.map_or(Ok(()), |x| mm_ok(key, field, x, lo, hi))
}

fn patterns_ok(key: &str, field: &str, list: &[String]) -> Result<()> {
    if list.len() > MAX_PATTERNS {
        return Err(invalid(key, format!("`{field}` has more than {MAX_PATTERNS} entries")));
    }
    for p in list {
        if p.is_empty() || p.chars().count() > MAX_PATTERN_LEN || p.chars().any(char::is_control) {
            return Err(invalid(key, format!("a `{field}` pattern is 1 to {MAX_PATTERN_LEN} characters without control characters")));
        }
    }
    Ok(())
}

fn string_map_ok(key: &str, field: &str, m: &BTreeMap<String, String>) -> Result<()> {
    if m.len() > MAX_MAP_ENTRIES {
        return Err(invalid(key, format!("`{field}` has more than {MAX_MAP_ENTRIES} entries")));
    }
    for (k, v) in m {
        text_ok(key, &format!("{field} name"), k, 64)?;
        text_ok(key, field, v, MAX_TEXT)?;
    }
    Ok(())
}

/// A file-name pattern is a name, never a path: no separators, no drive, no `..`.
fn file_pattern_ok(key: &str, s: &str) -> Result<()> {
    text_ok(key, "patterns.file", s, 128)?;
    if s.contains("..") || s.contains('/') || s.contains('\\') || s.contains(':') {
        return Err(invalid(key, "`patterns.file` is a file name pattern: no `..`, `/`, `\\` or `:`"));
    }
    Ok(())
}

fn scheme_ok(key: &str, sc: &Scheme) -> Result<()> {
    let p = &sc.pattern;
    if p.is_empty() || p.chars().count() > 24 || p.matches("{N}").count() != 1 || p.matches("{D}").count() > 1 {
        return Err(invalid(key, "`scheme.pattern` is 1 to 24 characters with {N} once and {D} at most once"));
    }
    let rest = p.replace("{N}", "").replace("{D}", "");
    if !rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')) {
        return Err(invalid(key, "`scheme.pattern` may only add letters, digits and - _ . space around {D} and {N}"));
    }
    if !(1..=8).contains(&sc.width) || !(1..=1000).contains(&sc.step) {
        return Err(invalid(key, "`scheme.width` is 1 to 8 and `scheme.step` is 1 to 1000"));
    }
    if sc.disciplines.is_empty() || sc.disciplines.len() > MAX_DISCIPLINES {
        return Err(invalid(key, format!("`scheme.disciplines` has 1 to {MAX_DISCIPLINES} entries")));
    }
    let mut seen = BTreeSet::new();
    for d in &sc.disciplines {
        let code = d.code.trim().to_ascii_uppercase();
        if code.is_empty() || code.len() > 4 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(invalid(key, "a discipline `code` is 1 to 4 letters or digits"));
        }
        text_ok(key, "discipline name", &d.name, 40)?;
        if d.start > 999_999 {
            return Err(invalid(key, "a discipline `start` is 0 to 999999"));
        }
        if !seen.insert(code) {
            return Err(invalid(key, "a discipline code is listed twice"));
        }
    }
    Ok(())
}

impl Schema {
    fn check(&self, key: &str) -> Result<()> {
        if self.min_reader < 1 {
            return Err(invalid(key, "`min_reader` is 1 or more"));
        }
        if self.min_reader > READER_VERSION {
            return Err(ConfigError::TooNew(self.min_reader));
        }
        Ok(())
    }
}

impl Rev {
    fn check(&self, key: &str) -> Result<()> {
        text_ok(key, "app", &self.app, 64)?;
        text_ok(key, "time", &self.time, 40)
    }
}

impl ProjectInfo {
    fn check(&self, key: &str) -> Result<()> {
        for (f, s, max) in [
            ("title", &self.title, MAX_TEXT),
            ("client", &self.client, MAX_TEXT),
            ("owner", &self.owner, MAX_TEXT),
            ("location", &self.location, MAX_TEXT),
            ("address", &self.address, MAX_TEXT * 2),
            ("pid", &self.pid, MAX_TEXT),
            ("siteArea", &self.site_area, MAX_TEXT),
            ("plotUse", &self.plot_use, MAX_TEXT),
            ("authority", &self.authority, MAX_TEXT),
            ("date", &self.date, MAX_TEXT),
            ("summary", &self.summary, MAX_LONG_TEXT),
            ("code", &self.code, 32),
            ("persona", &self.persona, 64),
        ] {
            text_ok(key, f, s, max)?;
        }
        Ok(())
    }
}

impl Team {
    fn check(&self, key: &str) -> Result<()> {
        text_ok(key, "architect", &self.architect, MAX_TEXT)?;
        text_ok(key, "engineer", &self.engineer, MAX_TEXT)?;
        string_map_ok(key, "company", &self.company)
    }
}

impl Units {
    fn check(&self, key: &str) -> Result<()> {
        if !UNITS.contains(&self.drawing_unit.as_str()) {
            return Err(invalid(key, "`drawing_unit` is mm, cm, m, in or ft"));
        }
        Ok(())
    }
}

impl Levels {
    fn check(&self, key: &str) -> Result<()> {
        if self.items.len() > MAX_LEVELS {
            return Err(invalid(key, format!("more than {MAX_LEVELS} levels")));
        }
        let d = &self.defaults;
        opt_mm(key, "defaults.height_mm", d.height_mm, 1.0, 100_000.0)?;
        opt_mm(key, "defaults.slab_mm", d.slab_mm, 1.0, 5000.0)?;
        opt_mm(key, "defaults.beam_mm", d.beam_mm, 1.0, 5000.0)?;
        opt_mm(key, "defaults.ceiling_mm", d.ceiling_mm, 1.0, 100_000.0)?;
        opt_mm(key, "defaults.lintel_bottom_mm", d.lintel_bottom_mm, 1.0, 100_000.0)?;
        let (mut cids, mut names) = (BTreeSet::new(), BTreeSet::new());
        let mut top: Option<u32> = None;
        for (i, l) in self.items.iter().enumerate() {
            let at = format!("{key}.items[{i}]");
            name_ok(&at, "name", &l.name)?;
            text_ok(&at, "code", &l.code, 16)?;
            mm_ok(&at, "elevation_mm", l.elevation_mm, -1_000_000.0, 1_000_000.0)?;
            mm_ok(&at, "height_mm", l.height_mm, 0.0, 100_000.0)?;
            opt_mm(&at, "slab_mm", l.slab_mm, 1.0, 5000.0)?;
            opt_mm(&at, "beam_mm", l.beam_mm, 1.0, 5000.0)?;
            opt_mm(&at, "ceiling_mm", l.ceiling_mm, 1.0, 100_000.0)?;
            opt_mm(&at, "lintel_bottom_mm", l.lintel_bottom_mm, 1.0, 100_000.0)?;
            if !cids.insert(l.cid) {
                return Err(invalid(&at, format!("`cid` {} is used twice", l.cid)));
            }
            if !names.insert(l.name.trim().to_lowercase()) {
                return Err(invalid(&at, format!("the name `{}` is used twice (names are compared without case)", l.name.trim())));
            }
            top = Some(top.map_or(l.cid, |t| t.max(l.cid)));
        }
        if top.is_some_and(|t| self.next_cid <= t) {
            return Err(invalid(key, "`next_cid` must be larger than every `cid` (ids are never reused)"));
        }
        Ok(())
    }
}

impl LayerSets {
    fn check(&self, key: &str) -> Result<()> {
        if self.sets.len() > LAYER_SETS.len() {
            return Err(invalid(key, "more layer sets than exist"));
        }
        let mut seen = BTreeSet::new();
        for s in &self.sets {
            let n = s.trim().to_ascii_lowercase();
            if !LAYER_SETS.contains(&n.as_str()) {
                return Err(invalid(key, "layer sets are standard, permit and presentation"));
            }
            if !seen.insert(n) {
                return Err(invalid(key, "a layer set is listed twice"));
            }
        }
        Ok(())
    }
}

impl Tabs {
    fn check(&self, key: &str) -> Result<()> {
        if self.items.len() > MAX_TABS {
            return Err(invalid(key, format!("more than {MAX_TABS} tabs")));
        }
        let (mut tids, mut names) = (BTreeSet::new(), BTreeSet::new());
        let mut top: Option<u32> = None;
        for (i, t) in self.items.iter().enumerate() {
            let at = format!("{key}.items[{i}]");
            name_ok(&at, "name", &t.name)?;
            text_ok(&at, "code", &t.code, 16)?;
            if t.layer_prefix.chars().count() > 8 || t.layer_prefix.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Err(invalid(&at, "`layer_prefix` is at most 8 characters without spaces"));
            }
            patterns_ok(&at, "layers", &t.layers)?;
            patterns_ok(&at, "blocks", &t.blocks)?;
            if !tids.insert(t.tid) {
                return Err(invalid(&at, format!("`tid` {} is used twice", t.tid)));
            }
            if !names.insert(t.name.trim().to_lowercase()) {
                return Err(invalid(&at, format!("the name `{}` is used twice (names are compared without case)", t.name.trim())));
            }
            top = Some(top.map_or(t.tid, |x| x.max(t.tid)));
        }
        if let (Some(next), Some(t)) = (self.next_tid, top)
            && next <= t
        {
            return Err(invalid(key, "`next_tid` must be larger than every `tid`"));
        }
        Ok(())
    }
}

impl Sheets {
    fn check(&self, key: &str) -> Result<()> {
        scheme_ok(key, &self.scheme)
    }
}

impl Naming {
    fn check(&self, key: &str) -> Result<()> {
        file_pattern_ok(key, &self.patterns.file)?;
        text_ok(key, "patterns.drawing_number", &self.patterns.drawing_number, 128)
    }
}

impl TitleBlock {
    fn check(&self, key: &str) -> Result<()> {
        string_map_ok(key, "fields", &self.fields)
    }
}

impl Applied {
    fn check(&self, key: &str) -> Result<()> {
        if !matches!(self.result.as_str(), "ok" | "partial" | "failed") {
            return Err(invalid(key, "`result` is ok, partial or failed"));
        }
        text_ok(key, "time", &self.time, 40)?;
        if self.items.len() > MAX_ITEMS_REPORT {
            return Err(invalid(key, "too many items"));
        }
        for it in &self.items {
            text_ok(key, "item key", &it.key, 64)?;
            text_ok(key, "item status", &it.status, 16)?;
            text_ok(key, "item detail", &it.detail, MAX_LONG_TEXT)?;
        }
        Ok(())
    }
}

impl Proposal {
    fn check(&self, key: &str) -> Result<()> {
        text_ok(key, "time", &self.time, 40)?;
        if self.levels.len() > MAX_LEVELS || self.tabs.len() > MAX_TABS || self.layer_sets.len() > LAYER_SETS.len() {
            return Err(invalid(key, "too many levels, tabs or layer sets"));
        }
        if let Some(u) = &self.drawing_unit
            && !UNITS.contains(&u.as_str())
        {
            return Err(invalid(key, "`drawing_unit` is mm, cm, m, in or ft"));
        }
        for (i, l) in self.levels.iter().enumerate() {
            let at = format!("{key}.levels[{i}]");
            name_ok(&at, "name", &l.name)?;
            mm_ok(&at, "elevation_mm", l.elevation_mm, -1_000_000.0, 1_000_000.0)?;
            mm_ok(&at, "height_mm", l.height_mm, 0.0, 100_000.0)?;
        }
        for (i, t) in self.tabs.iter().enumerate() {
            let at = format!("{key}.tabs[{i}]");
            name_ok(&at, "name", &t.name)?;
            patterns_ok(&at, "layers", &t.layers)?;
            patterns_ok(&at, "blocks", &t.blocks)?;
        }
        string_map_ok(key, "project", &self.project)?;
        if let Some(sc) = &self.scheme {
            scheme_ok(key, sc)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------- parse one key

/// What parsing one key gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Part<T> {
    Value(T),
    /// The key's `v` is newer than this reader knows: treat the part as absent and say so.
    Newer(i64),
}

trait Check {
    fn check(&self, key: &str) -> Result<()>;
}

macro_rules! check_impl {
    ($($t:ty),*) => { $(impl Check for $t { fn check(&self, key: &str) -> Result<()> { <$t>::check(self, key) } })* };
}
check_impl!(Schema, Rev, ProjectInfo, Team, Units, Levels, LayerSets, Tabs, Sheets, Naming, TitleBlock, Applied, Proposal);

/// Parse the JSON text of one key. `key` is only used in messages.
fn parse_part<T: DeserializeOwned + Check>(key: &str, text: &str) -> Result<Part<T>> {
    if text.len() > MAX_VALUE_BYTES {
        return Err(ConfigError::TooLarge { key: key.to_string(), bytes: text.len() as u64, max: MAX_VALUE_BYTES });
    }
    let value: Value = serde_json::from_str(text).map_err(|e| invalid(key, format!("not valid JSON: {e}")))?;
    let Some(obj) = value.as_object() else { return Err(invalid(key, "the value is not a JSON object")) };
    let v = obj.get("v").and_then(Value::as_i64).ok_or_else(|| invalid(key, "`v` (a whole number) is missing"))?;
    if v < 1 {
        return Err(invalid(key, "`v` is 1 or more"));
    }
    if v > READER_VERSION {
        return Ok(Part::Newer(v));
    }
    let typed: T = serde_json::from_value(value).map_err(|e| invalid(key, e.to_string()))?;
    typed.check(key)?;
    Ok(Part::Value(typed))
}

/// Parse `config.applied.cad` (written by MynaCAD).
pub fn parse_applied(text: &str) -> Result<Part<Applied>> {
    parse_part(KEY_APPLIED, text)
}

/// Parse `config.proposal` (written by MynaCAD).
pub fn parse_proposal(text: &str) -> Result<Part<Proposal>> {
    parse_part(KEY_PROPOSAL, text)
}

// ---------------------------------------------------------------------------------------------------- the whole configuration

/// The configuration of a project: every part MynaOps has written. A part that is `None` is absent (not written, or a newer version).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<Rev>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<Team>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<Units>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<Levels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_sets: Option<LayerSets>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<Tabs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheets: Option<Sheets>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub naming: Option<Naming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titleblock: Option<TitleBlock>,
}

/// The configuration read from its keys, with what was not understood.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    pub config: ProjectConfig,
    /// One line per key that was left out (a newer version).
    pub notes: Vec<String>,
}

impl ProjectConfig {
    /// Whether the project is configured by MynaOps (`config.schema` is present).
    pub fn configured(&self) -> bool {
        self.schema.is_some()
    }

    /// The configuration revision (`config.rev.rev`), 0 when there is none.
    pub fn revision(&self) -> u64 {
        self.rev.as_ref().map_or(0, |r| r.rev)
    }

    /// Build the configuration from key texts (`parts` maps the short name, `levels`, to the key's JSON text). Names that are not parts are ignored.
    /// Any part that breaks a rule rejects the whole; a part with a newer `v` is left out and noted.
    pub fn from_texts(parts: &BTreeMap<String, String>) -> Result<Read> {
        let mut out = Read::default();
        let c = &mut out.config;
        let notes = &mut out.notes;
        fn take<T: DeserializeOwned + Check>(name: &str, parts: &BTreeMap<String, String>, notes: &mut Vec<String>) -> Result<Option<T>> {
            let Some(text) = parts.get(name) else { return Ok(None) };
            let key = format!("{PREFIX}{name}");
            match parse_part::<T>(&key, text)? {
                Part::Value(t) => Ok(Some(t)),
                Part::Newer(v) => {
                    notes.push(format!("{key} is version {v}; this reader knows {READER_VERSION}: left out"));
                    Ok(None)
                }
            }
        }
        // The schema first: a project that needs a newer reader is refused before anything else is looked at.
        c.schema = take("schema", parts, notes)?;
        c.rev = take("rev", parts, notes)?;
        c.project = take("project", parts, notes)?;
        c.team = take("team", parts, notes)?;
        c.units = take("units", parts, notes)?;
        c.levels = take("levels", parts, notes)?;
        c.layer_sets = take("layer_sets", parts, notes)?;
        c.tabs = take("tabs", parts, notes)?;
        c.sheets = take("sheets", parts, notes)?;
        c.naming = take("naming", parts, notes)?;
        c.titleblock = take("titleblock", parts, notes)?;
        Ok(out)
    }

    /// The JSON text of every part that is present, by short name. `schema` and `rev` are included as they are.
    pub fn to_texts(&self) -> Result<BTreeMap<String, String>> {
        let v = serde_json::to_value(self).map_err(|e| invalid(PREFIX, e.to_string()))?;
        let mut out = BTreeMap::new();
        if let Value::Object(o) = v {
            for (k, part) in o {
                out.insert(k, part.to_string());
            }
        }
        Ok(out)
    }

    /// Check every part (the same rules a reader applies) by writing it out and parsing it back.
    pub fn validate(&self) -> Result<()> {
        Self::from_texts(&self.to_texts()?).map(|_| ())
    }

    /// The JSON form given to `project.config.apply`: an object with a member per part (`levels`, `units`, ...), each carrying its `v`.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Read the JSON form (see [`Self::to_json`]); the same checks as a file read.
    pub fn from_json(v: &Value) -> Result<Read> {
        let Some(o) = v.as_object() else { return Err(invalid("config", "the configuration is not a JSON object")) };
        let mut parts = BTreeMap::new();
        for name in PARTS {
            match o.get(name) {
                None | Some(Value::Null) => {}
                Some(p) => {
                    parts.insert(name.to_string(), p.to_string());
                }
            }
        }
        Self::from_texts(&parts)
    }

    /// A configuration with the given parts at version 1 and `config.schema`, for tests and tools. `rev` is left for the writer.
    pub fn new() -> Self {
        ProjectConfig { schema: Some(Schema { v: 1, min_reader: 1 }), ..ProjectConfig::default() }
    }
}

/// Whether two JSON texts hold the same value (a writer skips a key that would not change).
pub fn same_json(a: &str, b: &str) -> bool {
    match (serde_json::from_str::<Value>(a), serde_json::from_str::<Value>(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// The binding `cid → LevelId` in the drawing's project map: `{"v":1,"map":{"<cid>":<LevelId>}}`.
pub fn levelmap_to_text(map: &BTreeMap<u32, u32>) -> String {
    let m: Map<String, Value> = map.iter().map(|(k, v)| (k.to_string(), Value::from(*v))).collect();
    serde_json::json!({ "v": 1, "map": m }).to_string()
}

/// Read the binding; a damaged one reads as empty (the drawing is the truth, the binding is rebuilt by the next apply).
pub fn levelmap_from_text(text: &str) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();
    if text.len() > MAX_VALUE_BYTES {
        return out;
    }
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(text) else { return out };
    if o.get("v").and_then(Value::as_i64) != Some(1) {
        return out;
    }
    if let Some(Value::Object(m)) = o.get("map") {
        for (k, v) in m.iter().take(MAX_LEVELS * 4) {
            if let (Ok(cid), Some(id)) = (k.parse::<u32>(), v.as_u64().and_then(|n| u32::try_from(n).ok())) {
                out.insert(cid, id);
            }
        }
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "config_io.rs"]
mod io;
#[cfg(not(target_arch = "wasm32"))]
pub use io::*;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

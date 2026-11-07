//! The command registry. Ids are command names in lower case; menu paths follow
//! menu bar so the catalog doubles as the parity metric.

mod aicards;
mod annot;
mod annotate;
mod array;
mod audit;
pub(crate) mod bedit;
mod blocks;
mod bplt;
mod bridge;
mod classify;
pub mod clipboard;
mod clipcmd;
pub mod companion;
mod companyfile;
mod component;
mod compose;
pub mod context;
mod draw;
mod draw2;
mod edit;
mod electrical;
mod exchange;
mod extract;
pub mod file;
mod floorregion;
mod gripcmds;
pub(crate) mod groups;
mod hatch;
mod hcw;
mod hcwlayers;
pub mod history;
mod import;
mod inquiry;
pub mod layer;
pub mod layout;
mod legacytools;
mod legend;
mod levels;
mod lighting;
mod migrate;
mod mleader_content;
mod modify;
pub(crate) mod modify2;
mod mtext_opts;
mod notes;
mod package;
mod param;
mod pdfimport;
mod pline_opts;
mod printblock;
pub(crate) mod project;
mod projfile;
mod projwizard;
mod props;
mod qselect;
mod revision;
mod roomtools;
mod schedule;
mod scriptcmd;
mod selectmore;
mod settings;
mod sheet;
pub(crate) mod sheetset;
pub mod sheetstd;
pub mod smartblock;
mod space;
mod spatialq;
mod spline_opts;
mod structure;
mod svgimport;
mod symbols;
mod takeoff;
mod takeoffann;
mod takeoffphase;
pub use takeoffphase::phases_json as takeoff_phases_json;
pub mod pagesetup;
mod takeoffrec;
mod texttools;
mod trimextend;
mod typography;
mod underlay;
mod utility;
mod view;
mod walls;
mod wizard;
mod xref;

pub mod curves;
pub mod helpers;
pub mod machines;

use aad_doc::Handle;
use aad_geom::Vec2;
use serde::Serialize;
use serde_json::Value;

use crate::{EngineError, Interactive, Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;
pub type Factory = fn(&Session) -> Result<Box<dyn Interactive>>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Draw", "Circle", "Center, Radius"]`. Empty = not in menus.
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    pub undoable: bool,
    pub interactive: Option<Factory>,
    /// Can run inside another command with a leading apostrophe.
    pub transparent: bool,
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, run: Run) -> Self {
        CommandSpec {
            id,
            label,
            menu: &[],
            shortcut: None,
            aliases: &[],
            params: "",
            enabled: has_doc,
            run,
            undoable: true,
            interactive: None,
            transparent: false,
        }
    }
    pub const fn menu(mut self, m: &'static [&'static str]) -> Self {
        self.menu = m;
        self
    }
    pub const fn key(mut self, k: &'static str) -> Self {
        self.shortcut = Some(k);
        self
    }
    pub const fn alias(mut self, a: &'static [&'static str]) -> Self {
        self.aliases = a;
        self
    }
    pub const fn params(mut self, p: &'static str) -> Self {
        self.params = p;
        self
    }
    pub const fn enabled(mut self, e: Enabled) -> Self {
        self.enabled = e;
        self
    }
    pub const fn noundo(mut self) -> Self {
        self.undoable = false;
        self
    }
    pub const fn interactive(mut self, f: Factory) -> Self {
        self.interactive = Some(f);
        self
    }
    pub const fn transparent(mut self) -> Self {
        self.transparent = true;
        self
    }
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            aliases: self.aliases.to_vec(),
            params: self.params,
            interactive: self.interactive.is_some(),
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub aliases: Vec<&'static str>,
    pub params: &'static str,
    pub interactive: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
pub fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.state().map(|_| ()).map_err(|_| "no drawing open".into())
}
pub fn has_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.selection().is_empty() { Err("nothing selected".into()) } else { Ok(()) }
}

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(file::specs());
        v.extend(audit::specs());
        v.extend(edit::specs());
        v.extend(qselect::specs());
        v.extend(selectmore::specs());
        v.extend(view::specs());
        v.extend(draw::specs());
        v.extend(draw2::specs());
        v.extend(annotate::specs());
        v.extend(hatch::specs());
        v.extend(history::specs());
        v.extend(revision::specs());
        v.extend(blocks::specs());
        v.extend(printblock::specs());
        v.extend(bedit::specs());
        v.extend(modify::specs());
        v.extend(clipcmd::specs());
        v.extend(array::specs());
        v.extend(modify2::specs());
        v.extend(groups::specs());
        v.extend(classify::specs());
        v.extend(component::specs());
        v.extend(gripcmds::specs());
        v.extend(layer::specs());
        v.extend(hcw::specs());
        v.extend(texttools::specs());
        v.extend(hcwlayers::specs());
        v.extend(bplt::specs());
        v.extend(legacytools::specs());
        v.extend(roomtools::specs());
        v.extend(annot::specs());
        v.extend(project::specs());
        v.extend(companyfile::specs());
        v.extend(space::specs());
        v.extend(takeoff::specs());
        v.extend(takeoffann::specs());
        v.extend(takeoffphase::specs());
        v.extend(takeoffrec::specs());
        v.extend(floorregion::specs());
        v.extend(underlay::specs());
        v.extend(pagesetup::specs());
        v.extend(pdfimport::specs());
        v.extend(import::specs());
        v.extend(schedule::specs());
        v.extend(notes::specs());
        v.extend(legend::specs());
        v.extend(electrical::specs());
        v.extend(walls::specs());
        v.extend(param::specs());
        v.extend(smartblock::specs());
        v.extend(companion::specs());
        v.extend(structure::specs());
        v.extend(symbols::specs());
        v.extend(bridge::specs());
        v.extend(xref::specs());
        v.extend(package::specs());
        v.extend(extract::specs());
        v.extend(scriptcmd::specs());
        v.extend(projfile::specs());
        v.extend(projwizard::specs());
        v.extend(typography::specs());
        v.extend(spatialq::specs());
        v.extend(aicards::specs());
        v.extend(levels::specs());
        v.extend(sheetset::specs());
        v.extend(sheetstd::specs());
        v.extend(wizard::specs());
        v.extend(exchange::specs());
        v.extend(migrate::specs());
        v.extend(sheet::specs());
        v.extend(compose::specs());
        v.extend(layout::specs());
        v.extend(props::specs());
        v.extend(inquiry::specs());
        v.extend(settings::specs());
        v.extend(utility::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    let l = id.to_ascii_lowercase();
    command_specs().iter().find(|c| c.id == l)
}

/// Resolve a typed name or alias (classic short names such as `L`, `C`, `PL`) to an id.
pub fn resolve_alias(name: &str) -> String {
    let l = name.to_ascii_lowercase();
    if find_command(&l).is_some() {
        return l;
    }
    command_specs().iter().find(|c| c.aliases.iter().any(|a| a.eq_ignore_ascii_case(&l))).map(|c| c.id.to_string()).unwrap_or(l)
}

// ---------- param helpers ----------

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}
pub(crate) fn f64_or(p: &Value, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default)
}
pub(crate) fn f64_req(cmd: &str, p: &Value, key: &str) -> Result<f64> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, format!("`{key}` (number) is required")))
}
/// An optional size: `None` when absent or null, else a finite number above zero (or zero too
/// with `zero_ok`). Anything else is refused, not clamped or made absolute, so a bad value never
/// makes a degenerate object.
pub(crate) fn size_param(cmd: &str, p: &Value, key: &str, zero_ok: bool) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64().filter(|x| x.is_finite()) {
            Some(x) if x > 0.0 || (zero_ok && x == 0.0) => Ok(Some(x)),
            _ => Err(bad(cmd, format!("`{key}` must be {}", if zero_ok { "zero or positive" } else { "positive" }))),
        },
    }
}
/// `pts` without consecutive repeats (closer than `1e-12`), as the interactive LINE ignores a
/// point picked twice.
pub(crate) fn distinct_points(pts: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        if !out.last().is_some_and(|l| l.near(*p, 1e-12)) {
            out.push(*p);
        }
    }
    out
}
pub(crate) fn bool_or(p: &Value, key: &str, default: bool) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(default)
}
pub(crate) fn str_param<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}
pub fn point_value(v: &Value) -> Option<Vec2> {
    if let Some(a) = v.as_array() {
        let x = a.first()?.as_f64()?;
        let y = a.get(1)?.as_f64()?;
        let p = Vec2::new(x, y);
        return p.is_finite().then_some(p);
    }
    if let Some(s) = v.as_str() {
        return crate::prompt::parse_point(s, Vec2::ZERO);
    }
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    let p = Vec2::new(x, y);
    p.is_finite().then_some(p)
}
pub(crate) fn point_param(p: &Value, key: &str) -> Option<Vec2> {
    p.get(key).and_then(point_value)
}
pub(crate) fn point_req(cmd: &str, p: &Value, key: &str) -> Result<Vec2> {
    point_param(p, key).ok_or_else(|| bad(cmd, format!("`{key}` ([x, y]) is required")))
}
pub(crate) fn points_param(p: &Value, key: &str) -> Option<Vec<Vec2>> {
    p.get(key)?.as_array()?.iter().map(point_value).collect()
}
/// Handles from `handles` (hex strings or numbers), `handle`, or else the current selection. A
/// `handle`/`handles` that can't be read is an error, never a silent fall back to the selection.
pub(crate) fn targets(s: &Session, p: &Value) -> Result<Vec<Handle>> {
    let handle = |v: &Value| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle));
    let not_handle = |key: &str, v: &Value| EngineError::Other(format!("`{key}`: {v} is not an object handle (a hex string such as \"1A2\")"));
    match p.get("handles") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => return a.iter().map(|v| handle(v).ok_or_else(|| not_handle("handles", v))).collect(),
        Some(v) => return Err(EngineError::Other(format!("`handles` must be an array of object handles, got {v}"))),
    }
    match p.get("handle") {
        None | Some(Value::Null) => Ok(s.selection()),
        Some(v) => Ok(vec![handle(v).ok_or_else(|| not_handle("handle", v))?]),
    }
}
pub(crate) fn ok() -> Result<Value> {
    Ok(Value::Null)
}

/// `(code, name)` of the room types the ROOM command knows (for the room form).
pub fn room_types() -> Vec<(&'static str, &'static str)> {
    hcw::room_types()
}

/// A saved project value, for form prefill.
pub(crate) fn project_value(d: &aad_doc::Drawing, key: &str) -> Value {
    project::get(d, key)
}

/// The floors, with defaults filled in.
pub(crate) fn project_floors(d: &aad_doc::Drawing) -> Vec<Value> {
    project::floors(d)
}

/// Default slab thickness and beam depth (mm) from project settings.
pub(crate) fn project_structure_mm(d: &aad_doc::Drawing) -> (f64, f64) {
    project::structure_mm(d)
}

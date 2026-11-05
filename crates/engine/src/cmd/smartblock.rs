//! Smart blocks: parametric block definitions (O-Draft's own format, `osas-smartblocks` v1).
//!
//! A definition names its parameters (number, range, whole number or a list of choices), derived
//! values (`vars`, formulas of the parameters) and the geometry as items whose coordinates are
//! formulas in millimetres. Items can repeat (`repeat`, bars on a circle with `ring`) and can be
//! shown or hidden by a formula (`when`), which covers what stretch, array and visibility states
//! do in other programs. Formulas are the `aad-param` formula language (no trigonometry).
//!
//! * `smartblock.list` lists the built-in library (`assets/smartblocks/*.json`, built into the
//!   program) and the definitions stored in the drawing.
//! * `smartblock.insert` evaluates a definition with the given parameters into an ordinary block
//!   named `SB_<ID>_<hash>` and inserts it. The same parameters reuse the same block. The block's
//!   description keeps `{osas-smartblock, id, params}`, so the reference stays editable after a DXF
//!   round trip and in other programs it is a plain block.
//! * `smartblock.get` reads the definition and current parameters of a reference.
//! * `smartblock.edit` changes parameters of references: each one is pointed at the block for its
//!   new parameters (position, rotation, scale and layer stay).
//!
//! A definition given inline (`definition`) is checked like a built-in one and stored in the
//! drawing's project data (`smartBlockDefs`), so its references can be edited later.
//!
//! A definition may declare grips (`grips`): a point (a formula) tied to one parameter. The grips
//! follow the reference's own grips ([`crate::grips::grips`]); dragging one with `grip.move`
//! changes that parameter by the drag along the grip's axis, like `smartblock.edit`.
//!
//! Formulas are parsed once, when a definition is read; built-in definitions are read once per
//! process and shared.
//!
//! Limits: 64 KB per definition, 48 parameters, 64 derived values, 2000 items, repeats nested 4
//! deep and 1000 times each, 5000 objects per block, coordinates within 1e9 mm.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use aad_doc::{Block, Entity, EntityKind, HAlign, Hatch, HatchLoop, VAlign};
use aad_geom::{PolyVertex, Vec2};
use aad_param::formula;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::hcw::{drawing_units, ensure_layer};
use super::helpers::{circle, line, lwpoly, v3};
use super::*;
use crate::{EngineError, Result, Session};

const FORMAT: &str = "osas-smartblocks";
const TAG: &str = "osas-smartblock";
const DEFS_KEY: &str = "smartBlockDefs";
const MAX_DEF_BYTES: usize = 64 * 1024;
const MAX_PARAMS: usize = 48;
const MAX_VARS: usize = 64;
const MAX_ITEMS: usize = 2000;
const MAX_DEPTH: usize = 4;
const MAX_REPEAT: f64 = 1000.0;
const MAX_OBJECTS: usize = 5000;
const MAX_COORD: f64 = 1e9;
const MAX_TEXT: usize = 200;
const MAX_STORED_DEFS: usize = 500;
const MAX_GRIPS: usize = 16;

/// The built-in library files, one per discipline.
const LIBRARY: [(&str, &str); 6] = [
    ("rcc", include_str!("../../../../assets/smartblocks/rcc.json")),
    ("structural", include_str!("../../../../assets/smartblocks/structural.json")),
    ("plumbing", include_str!("../../../../assets/smartblocks/plumbing.json")),
    ("hvac", include_str!("../../../../assets/smartblocks/hvac.json")),
    ("electrical", include_str!("../../../../assets/smartblocks/electrical.json")),
    ("interior", include_str!("../../../../assets/smartblocks/interior.json")),
];

/// Built-in definition ids, for the insert form's list (a test keeps it equal to the library).
pub(crate) const IDS: &[&str] = &[
    "rcc.column.rect",
    "rcc.column.circular",
    "rcc.beam.section",
    "rcc.beam.elevation",
    "rcc.stirrup.detail",
    "rcc.footing.plan",
    "rcc.footing.section",
    "rcc.footing.combined",
    "rcc.raft.lift",
    "rcc.lintel.section",
    "rcc.slab.section",
    "struct.ibeam",
    "struct.channel",
    "struct.angle",
    "struct.hollow.rect",
    "struct.baseplate",
    "plumb.wc.european",
    "plumb.wc.indian",
    "plumb.basin",
    "plumb.sink.kitchen",
    "plumb.shower",
    "plumb.floor.trap",
    "plumb.chamber",
    "plumb.tank.round",
    "plumb.riser",
    "hvac.duct.rect",
    "hvac.diffuser.square",
    "hvac.grille",
    "hvac.split.indoor",
    "hvac.split.outdoor",
    "hvac.fan.exhaust",
    "elec.switchboard",
    "elec.socket",
    "elec.light",
    "elec.fan.ceiling",
    "elec.db",
    "elec.downlight",
    "int.wardrobe",
    "int.kitchen.counter",
    "int.dining.set",
    "int.bed",
    "int.sofa",
    "int.shelf",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("smartblock.list", "List Parametric Blocks", run_list)
            .alias(&["sblist"])
            .params("{discipline?: rcc|structural|plumbing|hvac|electrical|interior, sheetSet? (true: the active tab; or a sheet set name): only the smart blocks on layers that tab maps} → {blocks: [{id, title, discipline, layer, params: [{name, label, unit, default, min, max, integer, choices}]}]}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("smartblock.insert", "Parametric Block...", run_insert)
            .alias(&["sbinsert", "smartblock"])
            .params("{id | definition: {…osas-smartblocks block…}, at: [x,y], params?: {name: value} | \"name=value, …\", rotation? (degrees), scale? (default 1; geometry is millimetres, scaled to the drawing units), layer?} → {handle, block, params}"),
        CommandSpec::new("smartblock.get", "Parametric Block Parameters", run_get)
            .params("{handle?} (else the first selected smart block) → {handle, id, title, block, params: {name: value}, spec: [...]}")
            .noundo(),
        CommandSpec::new("rcc.column", "Column (rectangular)", run_rcc_column)
            .menu(&["Architecture", "RCC Structure", "Column (rectangular)"])
            .params("{at: [x,y], params?: {name: value} | \"name=value, …\", rotation?, scale?, layer?} → as smartblock.insert, for `rcc.column.rect`; in the Windows app the point is picked on the drawing and the reinforcement is edited in the Properties panel (checked to IS 456:2000)"),
        CommandSpec::new("rcc.column.round", "Column (circular)", run_rcc_column_round)
            .menu(&["Architecture", "RCC Structure", "Column (circular)"])
            .params("{at: [x,y], params?: {name: value} | \"name=value, …\", rotation?, scale?, layer?} → as smartblock.insert, for `rcc.column.circular`; in the Windows app the point is picked on the drawing and the reinforcement is edited in the Properties panel (checked to IS 456:2000)"),
        CommandSpec::new("rcc.beam", "Beam section", run_rcc_beam)
            .menu(&["Architecture", "RCC Structure", "Beam section"])
            .params("{at: [x,y], params?: {name: value} | \"name=value, …\", rotation?, scale?, layer?} → as smartblock.insert, for `rcc.beam.section`; in the Windows app the point is picked on the drawing and the reinforcement is edited in the Properties panel (checked to IS 456:2000)"),
        CommandSpec::new("rcc.footing", "Footing (section)", run_rcc_footing)
            .menu(&["Architecture", "RCC Structure", "Footing (section)"])
            .params("{at: [x,y], params?: {name: value} | \"name=value, …\", rotation?, scale?, layer?} → as smartblock.insert, for `rcc.footing.section`; in the Windows app the point is picked on the drawing and the reinforcement is edited in the Properties panel (checked to IS 456:2000)"),
        CommandSpec::new("rcc.slab", "Slab section", run_rcc_slab)
            .menu(&["Architecture", "RCC Structure", "Slab section"])
            .params("{at: [x,y], params?: {name: value} | \"name=value, …\", rotation?, scale?, layer?} → as smartblock.insert, for `rcc.slab.section`; in the Windows app the point is picked on the drawing and the reinforcement is edited in the Properties panel (checked to IS 456:2000)"),
        CommandSpec::new("rcc.lintel", "Lintel", run_rcc_lintel)
            .menu(&["Architecture", "RCC Structure", "Lintel"])
            .params("{at: [x,y], params?: {name: value} | \"name=value, …\", rotation?, scale?, layer?} → as smartblock.insert, for `rcc.lintel.section`; in the Windows app the point is picked on the drawing and the reinforcement is edited in the Properties panel (checked to IS 456:2000)"),
        CommandSpec::new("smartblock.check", "Check Block to IS 456", run_check)
            .menu(&["Architecture", "RCC Structure", "Check to IS 456"])
            .alias(&["sbcheck", "is456"])
            .params("{handle?} (else the first selected RCC block) or {id, params?: {name: value} | \"name=value, …\"} → {id, ok, lines: [{label, value}], issues: [{clause, message}]} (IS 456:2000 clauses for cover, steel percentage, bar and tie spacing)")
            .noundo(),
        CommandSpec::new("smartblock.edit", "Edit Block Parameters", run_edit)
            .menu(&["Modify", "Object", "Block Parameters..."])
            .alias(&["sbedit"])
            .params("{handle? | handles? (else the selection), params: {name: value} | \"name=value, …\"} → {edited, blocks}"),
    ]
}

// ---------------------------------------------------------------------------------------------
// Definitions

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LibraryFile {
    format: String,
    version: u32,
    discipline: String,
    #[serde(default)]
    #[allow(dead_code)]
    note: String,
    blocks: Vec<Def>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Def {
    id: String,
    title: String,
    #[serde(default)]
    discipline: String,
    #[serde(default)]
    description: String,
    /// The layer the reference goes on, with its colour index and lineweight (1/100 mm).
    #[serde(default)]
    layer: String,
    #[serde(default = "seven")]
    color: i16,
    #[serde(default = "lw_default")]
    lineweight: u16,
    #[serde(default)]
    params: Vec<ParamDef>,
    #[serde(default)]
    vars: Vec<VarDef>,
    #[serde(default)]
    grips: Vec<GripDef>,
    /// The layers the items are drawn on, each with its colour, linetype and lineweight (an ISO-style layer scheme: concrete outline, bars by
    /// kind, ties, dimensions, notes, hatches). They are created in the drawing when the block is made, so the drawing's layer manager owns them.
    #[serde(default)]
    layers: Vec<LayerSpec>,
    items: Vec<Item>,
}

/// A layer a definition draws on.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerSpec {
    name: String,
    #[serde(default = "seven")]
    color: i16,
    #[serde(default = "continuous")]
    linetype: String,
    /// Hundredths of a millimetre (ISO 128 pen widths: 9, 13, 18, 25, 35, 50, 70, 100).
    #[serde(default = "lw_default")]
    lineweight: u16,
    #[serde(default)]
    description: String,
}

fn continuous() -> String {
    "Continuous".into()
}

/// A grip that sets parameter `param`: drawn at `at`; dragging it by `d` along `axis` changes the
/// parameter by `per * d`, rounded to `step` and kept in the parameter's range.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GripDef {
    param: String,
    at: [Num; 2],
    axis: Axis,
    #[serde(default)]
    per: Option<Num>,
    #[serde(default)]
    step: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Axis {
    X,
    Y,
}

fn seven() -> i16 {
    7
}
fn lw_default() -> u16 {
    18
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamDef {
    name: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    unit: String,
    default: f64,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    integer: bool,
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    label: String,
    value: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VarDef {
    name: String,
    expr: Num,
}

/// A number or a formula of the parameters and derived values. A formula is parsed when the
/// definition is read, so a syntax error refuses the definition and evaluation never re-parses.
#[derive(Clone, Debug)]
enum Num {
    N(f64),
    F(Box<(String, formula::Expr)>),
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(f64),
            F(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(v) => Ok(Num::N(v)),
            Raw::F(t) => {
                let e = formula::parse(&t).map_err(|e| serde::de::Error::custom(format!("formula `{t}`: {e}")))?;
                Ok(Num::F(Box::new((t, e))))
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Item {
    /// Drawn only when this formula is greater than zero.
    #[serde(default)]
    when: Option<Num>,
    /// A layer for this item; otherwise layer 0, so it follows the reference's layer.
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    linetype: Option<String>,
    /// An AutoCAD colour index for this item (bars by diameter, ties, dimensions); otherwise the reference's colour.
    #[serde(default)]
    color: Option<i16>,
    #[serde(flatten)]
    shape: Shape,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Shape {
    /// `[x1, y1, x2, y2]`
    Line([Num; 4]),
    /// `[x, y, width, height]` from a corner.
    Rect([Num; 4]),
    /// `[cx, cy, width, height]` about a centre.
    Crect([Num; 4]),
    Poly(PolyDef),
    /// `[cx, cy, r]`
    Circle([Num; 3]),
    /// A filled dot `[cx, cy, r]` (a bar in section).
    Dot([Num; 3]),
    /// `[cx, cy, r, start°, end°]`, counter-clockwise.
    Arc([Num; 5]),
    Text(TextDef),
    Hatch(HatchDef),
    /// Dots evenly on a circle (bars of a circular column).
    Ring(RingDef),
    Repeat(RepeatDef),
    /// A dimension along the x or y axis, with extension lines, ticks and its text (orange by default).
    Dim(DimDef),
    /// A leader: a line from a point to a landing, with text after it.
    Leader(LeaderDef),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DimDef {
    from: [Num; 2],
    to: [Num; 2],
    /// How far the dimension line is from the measured points: above (x axis) or to the right (y axis) when positive.
    off: Num,
    height: Num,
    /// The text; the measured length (in the unit chosen by the `ftin` parameter) when left out.
    #[serde(default)]
    text: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaderDef {
    from: [Num; 2],
    to: [Num; 2],
    height: Num,
    text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolyDef {
    pts: Vec<[Num; 2]>,
    #[serde(default)]
    closed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextDef {
    at: [Num; 2],
    height: Num,
    /// `{name}` is replaced by a parameter's value (a choice's label) or a derived value.
    value: String,
    /// `l` (left baseline, default) or `c` (middle centre).
    #[serde(default)]
    align: Option<String>,
    #[serde(default)]
    rotation: Option<Num>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HatchDef {
    pts: Vec<[Num; 2]>,
    pattern: String,
    #[serde(default)]
    scale: Option<Num>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RingDef {
    c: [Num; 2],
    r: Num,
    count: Num,
    dot: Num,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepeatDef {
    var: String,
    count: Num,
    items: Vec<Item>,
}

fn err(msg: impl Into<String>) -> EngineError {
    EngineError::Other(msg.into())
}

/// The built-in library, parsed once. A definition that does not parse is an error, never a panic.
fn library() -> std::result::Result<&'static [Arc<Def>], String> {
    static LIB: OnceLock<std::result::Result<Vec<Arc<Def>>, String>> = OnceLock::new();
    LIB.get_or_init(|| {
        let mut out = Vec::new();
        for (name, text) in LIBRARY {
            let f: LibraryFile = serde_json::from_str(text).map_err(|e| format!("smart block library `{name}`: {e}"))?;
            if f.format != FORMAT || f.version != 1 {
                return Err(format!("smart block library `{name}`: not {FORMAT} version 1"));
            }
            for mut d in f.blocks {
                if d.discipline.is_empty() {
                    d.discipline = f.discipline.clone();
                }
                check_def(&d).map_err(|e| format!("smart block `{}`: {e}", d.id))?;
                out.push(Arc::new(d));
            }
        }
        Ok(out)
    })
    .as_ref()
    .map(Vec::as_slice)
    .map_err(Clone::clone)
}

fn builtin(id: &str) -> Result<Option<&'static Arc<Def>>> {
    Ok(library().map_err(err)?.iter().find(|d| d.id == id))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_')
}

fn count_items(items: &[Item], depth: usize, n: &mut usize) -> std::result::Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("repeats are nested more than {MAX_DEPTH} deep"));
    }
    for it in items {
        *n += 1;
        if *n > MAX_ITEMS {
            return Err(format!("more than {MAX_ITEMS} items"));
        }
        if let Shape::Repeat(r) = &it.shape {
            if !formula::valid_input_name(&r.var) {
                return Err(format!("repeat variable `{}` is not a valid name", r.var));
            }
            count_items(&r.items, depth + 1, n)?;
        }
    }
    Ok(())
}

/// Structure checks that do not need values: names, counts and ranges.
fn check_def(d: &Def) -> std::result::Result<(), String> {
    if !valid_id(&d.id) {
        return Err("`id` must be 1 to 64 letters, digits, `.`, `-` or `_`".into());
    }
    if d.title.trim().is_empty() || d.title.len() > 120 {
        return Err("`title` must be 1 to 120 characters".into());
    }
    for l in &d.layers {
        if !valid_layer(&l.name) {
            return Err(format!("layer `{}` is not a valid name", l.name));
        }
    }
    if !d.layer.is_empty() && !valid_layer(&d.layer) {
        return Err(format!("layer `{}` is not a valid name", d.layer));
    }
    if d.params.len() > MAX_PARAMS || d.vars.len() > MAX_VARS {
        return Err(format!("at most {MAX_PARAMS} parameters and {MAX_VARS} derived values"));
    }
    let mut names = std::collections::BTreeSet::new();
    for p in &d.params {
        if !formula::valid_input_name(&p.name) || !names.insert(p.name.as_str()) {
            return Err(format!("parameter `{}` is not a valid, unique name", p.name));
        }
        if !p.default.is_finite() || p.min.is_some_and(|m| !m.is_finite()) || p.max.is_some_and(|m| !m.is_finite()) {
            return Err(format!("parameter `{}`: default and range must be finite", p.name));
        }
        if p.choices.len() > 64 || p.choices.iter().any(|c| !c.value.is_finite() || c.label.trim().is_empty() || c.label.len() > 40) {
            return Err(format!("parameter `{}`: at most 64 choices, each a label and a finite value", p.name));
        }
        check_value(p, p.default).map_err(|e| format!("default: {e}"))?;
    }
    for v in &d.vars {
        if !formula::valid_input_name(&v.name) || !names.insert(v.name.as_str()) {
            return Err(format!("derived value `{}` is not a valid, unique name", v.name));
        }
    }
    if d.grips.len() > MAX_GRIPS {
        return Err(format!("at most {MAX_GRIPS} grips"));
    }
    for g in &d.grips {
        let Some(p) = d.params.iter().find(|p| p.name == g.param) else {
            return Err(format!("grip: no parameter `{}`", g.param));
        };
        if !p.choices.is_empty() {
            return Err(format!("grip: `{}` is a list of choices", g.param));
        }
        if matches!(g.per, Some(Num::N(v)) if !v.is_finite() || v == 0.0) || g.step.is_some_and(|s| !(s.is_finite() && s > 0.0)) {
            return Err(format!("grip `{}`: `per` must be a non-zero number and `step` positive", g.param));
        }
    }
    let mut n = 0;
    count_items(&d.items, 0, &mut n)
}

fn valid_layer(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// Whether `v` is allowed for parameter `p`.
fn check_value(p: &ParamDef, v: f64) -> std::result::Result<(), String> {
    if !v.is_finite() {
        return Err(format!("`{}` must be a finite number", p.name));
    }
    if !p.choices.is_empty() {
        return if p.choices.iter().any(|c| c.value == v) {
            Ok(())
        } else {
            let list: Vec<&str> = p.choices.iter().map(|c| c.label.as_str()).collect();
            Err(format!("`{}` must be one of: {}", p.name, list.join(", ")))
        };
    }
    if p.integer && v.fract() != 0.0 {
        return Err(format!("`{}` must be a whole number", p.name));
    }
    if let Some(m) = p.min
        && v < m
    {
        return Err(format!("`{}` must be at least {}", p.name, fmt_num(m)));
    }
    if let Some(m) = p.max
        && v > m
    {
        return Err(format!("`{}` must be at most {}", p.name, fmt_num(m)));
    }
    Ok(())
}

/// A definition found by id, with its stored JSON when the drawing keeps it (an inline one).
#[derive(Clone)]
struct Found {
    def: Arc<Def>,
    stored: Option<Value>,
}

/// A definition by id: the drawing's own (inline ones) first, then the built-in library (shared,
/// not copied).
fn find_def(doc: &aad_doc::Drawing, id: &str) -> Result<Found> {
    let stored = project::get(doc, DEFS_KEY);
    if let Some(v) = stored.get(id) {
        let d: Def = serde_json::from_value(v.clone()).map_err(|e| err(format!("stored smart block `{id}`: {e}")))?;
        check_def(&d).map_err(|e| err(format!("stored smart block `{id}`: {e}")))?;
        return Ok(Found { def: Arc::new(d), stored: Some(v.clone()) });
    }
    let def = builtin(id)?.cloned().ok_or_else(|| err(format!("no smart block `{id}` (see smartblock.list)")))?;
    Ok(Found { def, stored: None })
}

// ---------------------------------------------------------------------------------------------
// Parameters

/// Parameter values from `{name: value}` or `"name=value, name=value"`; a choice may be given by
/// its label.
fn given_params(cmd: &str, p: &Value) -> Result<Map<String, Value>> {
    match p.get("params") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(m)) => Ok(m.clone()),
        Some(Value::String(t)) => {
            if t.len() > 2000 {
                return Err(bad(cmd, "`params` is too long"));
            }
            let mut m = Map::new();
            for part in t.split([',', ';', '\n']).map(str::trim).filter(|x| !x.is_empty()) {
                let (k, v) = part.split_once('=').ok_or_else(|| bad(cmd, format!("`{part}`: write name=value")))?;
                let v = v.trim();
                m.insert(k.trim().to_string(), v.parse::<f64>().map_or_else(|_| json!(v), |n| json!(n)));
            }
            Ok(m)
        }
        Some(_) => Err(bad(cmd, "`params` must be an object or \"name=value, …\"")),
    }
}

/// Every parameter's value: `given` over `base` over the defaults, checked against its range.
fn resolve(cmd: &str, d: &Def, base: &Map<String, Value>, given: &Map<String, Value>) -> Result<BTreeMap<String, f64>> {
    for k in given.keys() {
        if !d.params.iter().any(|p| &p.name == k) {
            let names: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
            return Err(bad(cmd, format!("`{}` has no parameter `{k}` (parameters: {})", d.id, names.join(", "))));
        }
    }
    let mut out = BTreeMap::new();
    for p in &d.params {
        let v = match given.get(&p.name).or_else(|| base.get(&p.name)) {
            None => p.default,
            Some(Value::Number(n)) => n.as_f64().ok_or_else(|| bad(cmd, format!("`{}` must be a number", p.name)))?,
            Some(Value::String(t)) => {
                let t = t.trim();
                match p.choices.iter().find(|c| c.label.eq_ignore_ascii_case(t)) {
                    Some(c) => c.value,
                    None => t.parse::<f64>().map_err(|_| bad(cmd, format!("`{}`: `{t}` is not a number or a choice", p.name)))?,
                }
            }
            Some(other) => return Err(bad(cmd, format!("`{}`: {other} is not a number", p.name))),
        };
        check_value(p, v).map_err(|e| bad(cmd, e))?;
        out.insert(p.name.clone(), v);
    }
    Ok(out)
}

fn params_json(vals: &BTreeMap<String, f64>) -> Value {
    Value::Object(vals.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

fn fmt_num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
}

// ---------------------------------------------------------------------------------------------
// Evaluation

struct Ctx<'a> {
    def: &'a Def,
    vars: BTreeMap<String, f64>,
    out: Vec<Made>,
}

/// One object a definition makes: its layer, linetype and colour index when given, and the object.
type Made = (Option<String>, Option<String>, Option<i16>, EntityKind);

impl Ctx<'_> {
    fn num(&self, n: &Num) -> Result<f64> {
        let v = match n {
            Num::N(v) => *v,
            Num::F(f) => formula::eval(&f.1, &self.vars).map_err(|e| err(format!("`{}`: formula `{}`: {e}", self.def.id, f.0)))?,
        };
        if v.is_finite() && v.abs() <= MAX_COORD { Ok(v) } else { Err(err(format!("`{}`: a value is out of range", self.def.id))) }
    }
    fn pt(&self, p: &[Num; 2]) -> Result<Vec2> {
        Ok(Vec2::new(self.num(&p[0])?, self.num(&p[1])?))
    }
    fn push(&mut self, it: &Item, k: EntityKind) -> Result<()> {
        if self.out.len() >= MAX_OBJECTS {
            return Err(err(format!("`{}` makes more than {MAX_OBJECTS} objects", self.def.id)));
        }
        self.out.push((it.layer.clone(), it.linetype.clone(), it.color, k));
        Ok(())
    }
    /// As `push`, with `color` when the item names none.
    fn push_or(&mut self, it: &Item, color: i16, k: EntityKind) -> Result<()> {
        if self.out.len() >= MAX_OBJECTS {
            return Err(err(format!("`{}` makes more than {MAX_OBJECTS} objects", self.def.id)));
        }
        // On a layer the colour is the layer's; the default colour is for an item with no layer.
        let color = it.color.or(if it.layer.is_some() { None } else { Some(color) });
        self.out.push((it.layer.clone(), it.linetype.clone(), color, k));
        Ok(())
    }
    /// Whether lengths are shown in feet and inches (the `ftin` parameter above zero) rather than millimetres.
    fn ftin(&self) -> bool {
        self.vars.get("ftin").is_some_and(|v| *v > 0.0)
    }
    fn length(&self, mm: f64) -> String {
        if self.ftin() { fmt_ftin(mm) } else { fmt_num(mm) }
    }
    fn text(&self, t: &str) -> String {
        let mut s = String::new();
        let mut rest = t;
        while let Some(i) = rest.find('{') {
            s.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            match after.find('}') {
                Some(j) => {
                    let name = &after[..j];
                    let shown = match name.strip_suffix(":len") {
                        Some(n) => self.vars.get(n).map(|v| self.length(*v)),
                        None => self.label(name),
                    };
                    s.push_str(&shown.unwrap_or_else(|| format!("{{{name}}}")));
                    rest = &after[j + 1..];
                }
                None => {
                    s.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        s.push_str(rest);
        s.chars().take(MAX_TEXT).collect()
    }
    fn label(&self, name: &str) -> Option<String> {
        let v = *self.vars.get(name)?;
        if let Some(p) = self.def.params.iter().find(|p| p.name == name)
            && let Some(c) = p.choices.iter().find(|c| c.value == v)
        {
            return Some(c.label.clone());
        }
        Some(fmt_num(v))
    }

    fn items(&mut self, items: &[Item]) -> Result<()> {
        for it in items {
            if let Some(w) = &it.when
                && self.num(w)? <= 0.0
            {
                continue;
            }
            self.item(it)?;
        }
        Ok(())
    }

    fn item(&mut self, it: &Item) -> Result<()> {
        match &it.shape {
            Shape::Line(a) => {
                let k = line(Vec2::new(self.num(&a[0])?, self.num(&a[1])?), Vec2::new(self.num(&a[2])?, self.num(&a[3])?));
                self.push(it, k)
            }
            Shape::Rect(a) | Shape::Crect(a) => {
                let (x, y, w, h) = (self.num(&a[0])?, self.num(&a[1])?, self.num(&a[2])?, self.num(&a[3])?);
                let (x, y) = if matches!(it.shape, Shape::Crect(_)) { (x - w / 2.0, y - h / 2.0) } else { (x, y) };
                let vs = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(a, b)| PolyVertex::new(Vec2::new(a, b)));
                self.push(it, lwpoly(vs.to_vec(), true))
            }
            Shape::Poly(p) => {
                let pts = p.pts.iter().map(|q| self.pt(q).map(PolyVertex::new)).collect::<Result<Vec<_>>>()?;
                if pts.len() < 2 {
                    return Err(err(format!("`{}`: a polyline needs two points", self.def.id)));
                }
                self.push(it, lwpoly(pts, p.closed))
            }
            Shape::Circle(a) => {
                let r = self.num(&a[2])?;
                if r <= 0.0 {
                    return Ok(());
                }
                let k = circle(Vec2::new(self.num(&a[0])?, self.num(&a[1])?), r);
                self.push(it, k)
            }
            Shape::Dot(a) => {
                let (c, r) = (Vec2::new(self.num(&a[0])?, self.num(&a[1])?), self.num(&a[2])?);
                if r > 0.0 {
                    self.push(it, dot(c, r))?;
                }
                Ok(())
            }
            Shape::Arc(a) => {
                let r = self.num(&a[2])?;
                if r <= 0.0 {
                    return Ok(());
                }
                let k = EntityKind::Arc(aad_doc::Arc {
                    center: v3(Vec2::new(self.num(&a[0])?, self.num(&a[1])?)),
                    radius: r,
                    start: self.num(&a[3])?.to_radians(),
                    end: self.num(&a[4])?.to_radians(),
                });
                self.push(it, k)
            }
            Shape::Text(t) => {
                let h = self.num(&t.height)?;
                if h <= 0.0 {
                    return Err(err(format!("`{}`: text height must be positive", self.def.id)));
                }
                let rot = match &t.rotation {
                    Some(r) => self.num(r)?.to_radians(),
                    None => 0.0,
                };
                let j = if t.align.as_deref() == Some("c") { (HAlign::Center, VAlign::Middle) } else { (HAlign::Left, VAlign::Baseline) };
                let at = self.pt(&t.at)?;
                let value = self.text(&t.value);
                // The text style is set when the block is built (see `build`).
                let k = text_kind(at, &value, h, rot, j);
                self.push(it, k)
            }
            Shape::Hatch(hd) => {
                let pts = hd.pts.iter().map(|q| self.pt(q).map(PolyVertex::new)).collect::<Result<Vec<_>>>()?;
                if pts.len() < 3 {
                    return Err(err(format!("`{}`: a hatch needs three points", self.def.id)));
                }
                let solid = hd.pattern.eq_ignore_ascii_case("SOLID");
                if !solid && aad_doc::library::pattern(&hd.pattern).is_none() {
                    return Err(err(format!("`{}`: no hatch pattern `{}`", self.def.id, hd.pattern)));
                }
                let scale = match &hd.scale {
                    Some(n) => self.num(n)?,
                    None => 1.0,
                };
                if scale <= 0.0 {
                    return Err(err(format!("`{}`: hatch scale must be positive", self.def.id)));
                }
                let k = EntityKind::Hatch(Hatch {
                    pattern: hd.pattern.to_ascii_uppercase(),
                    solid,
                    loops: vec![HatchLoop { vertices: pts, outer: true }],
                    scale,
                    angle: 0.0,
                    associative: false,
                    style: 0,
                    elevation: 0.0,
                    gradient: None,
                    origin: Vec2::ZERO,
                    background: None,
                    pattern_lines: Vec::new(),
                });
                self.push(it, k)
            }
            Shape::Ring(r) => {
                let (c, rad, dr) = (self.pt(&r.c)?, self.num(&r.r)?, self.num(&r.dot)?);
                let n = self.count(&r.count)?;
                for i in 0..n {
                    let a = aad_geom::TAU * i as f64 / n as f64 + std::f64::consts::FRAC_PI_2;
                    self.push(it, dot(Vec2::polar(c, rad, a), dr))?;
                }
                Ok(())
            }
            Shape::Dim(d) => self.dim(it, d),
            Shape::Leader(l) => self.leader(it, l),
            Shape::Repeat(r) => {
                let n = self.count(&r.count)?;
                let saved = self.vars.insert(r.var.clone(), 0.0);
                for i in 0..n {
                    if let Some(x) = self.vars.get_mut(&r.var) {
                        *x = i as f64;
                    }
                    self.items(&r.items)?;
                }
                match saved {
                    Some(v) => self.vars.insert(r.var.clone(), v),
                    None => self.vars.remove(&r.var),
                };
                Ok(())
            }
        }
    }

    fn dim(&mut self, it: &Item, d: &DimDef) -> Result<()> {
        const COLOR: i16 = 30;
        let (a, b, off, h) = (self.pt(&d.from)?, self.pt(&d.to)?, self.num(&d.off)?, self.num(&d.height)?);
        if h <= 0.0 {
            return Err(err(format!("`{}`: dimension text height must be positive", self.def.id)));
        }
        let horizontal = (b.x - a.x).abs() >= (b.y - a.y).abs();
        let len = if horizontal { (b.x - a.x).abs() } else { (b.y - a.y).abs() };
        if len < 1e-6 {
            return Ok(());
        }
        // `m` runs along the dimension and `n` across it; the dimension line is at `off` across.
        let at = |m: f64, n: f64| if horizontal { Vec2::new(m, n) } else { Vec2::new(n, m) };
        let (m0, m1) = if horizontal { (a.x, b.x) } else { (a.y, b.y) };
        let (n0, n1) = if horizontal { (a.y, b.y) } else { (a.x, b.x) };
        let side = if off < 0.0 { -1.0 } else { 1.0 };
        let line_n = n0 + off;
        self.push_or(it, COLOR, line(at(m0, line_n), at(m1, line_n)))?;
        for (m, n) in [(m0, n0), (m1, n1)] {
            self.push_or(it, COLOR, line(at(m, n + side * h * 0.3), at(m, line_n + side * h * 0.5)))?;
            let t = h * 0.45;
            self.push_or(it, COLOR, line(at(m - t, line_n - t), at(m + t, line_n + t)))?;
        }
        let value = match &d.text {
            Some(t) => self.text(t),
            None => self.length(len),
        };
        let mid = (m0 + m1) / 2.0;
        let k = if horizontal {
            // Above the line when the dimension is above, below it otherwise.
            let (y, v) = if side > 0.0 { (line_n + h * 0.35, VAlign::Baseline) } else { (line_n - h * 0.35, VAlign::Top) };
            text_kind(Vec2::new(mid, y), &value, h, 0.0, (HAlign::Center, v))
        } else {
            // Turned to read from the right, on the side of the line away from the measured points.
            let v = if side > 0.0 { VAlign::Top } else { VAlign::Bottom };
            text_kind(Vec2::new(line_n + side * h * 0.35, mid), &value, h, std::f64::consts::FRAC_PI_2, (HAlign::Center, v))
        };
        self.push_or(it, COLOR, k)
    }

    fn leader(&mut self, it: &Item, l: &LeaderDef) -> Result<()> {
        const COLOR: i16 = 30;
        let (a, b, h) = (self.pt(&l.from)?, self.pt(&l.to)?, self.num(&l.height)?);
        if h <= 0.0 {
            return Err(err(format!("`{}`: leader text height must be positive", self.def.id)));
        }
        let side = if b.x < a.x { -1.0 } else { 1.0 };
        let land = Vec2::new(b.x + side * h * 1.5, b.y);
        self.push_or(it, COLOR, lwpoly([a, b, land].into_iter().map(PolyVertex::new).collect(), false))?;
        let value = self.text(&l.text);
        let j = if side < 0.0 { (HAlign::Right, VAlign::Baseline) } else { (HAlign::Left, VAlign::Baseline) };
        let k = text_kind(Vec2::new(land.x + side * h * 0.3, land.y - h * 0.35), &value, h, 0.0, j);
        self.push_or(it, COLOR, k)
    }

    fn count(&self, n: &Num) -> Result<usize> {
        let c = self.num(n)?.round();
        if !(0.0..=MAX_REPEAT).contains(&c) {
            return Err(err(format!("`{}`: a repeat count must be 0 to {MAX_REPEAT}", self.def.id)));
        }
        Ok(c as usize)
    }
}

/// A length in feet and inches to the nearest half inch: `1'-6"`, `9"`, `1.5"`.
fn fmt_ftin(mm: f64) -> String {
    let inches = (mm / 25.4 * 2.0).round() / 2.0;
    let feet = (inches / 12.0).floor();
    let rest = inches - feet * 12.0;
    let r = fmt_num(rest);
    if feet >= 1.0 { format!("{}'-{r}\"", fmt_num(feet)) } else { format!("{r}\"") }
}

fn text_kind(at: Vec2, value: &str, height: f64, rotation: f64, j: (HAlign, VAlign)) -> EntityKind {
    let left = j == (HAlign::Left, VAlign::Baseline);
    EntityKind::Text(aad_doc::Text {
        insert: v3(at),
        align_pt: if left { None } else { Some(v3(at)) },
        height,
        value: value.into(),
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: j.0,
        valign: j.1,
    })
}

/// A filled disc of radius `r`: a closed two-arc polyline of half the radius and width `r`.
fn dot(c: Vec2, r: f64) -> EntityKind {
    let half = r / 2.0;
    let vs = vec![PolyVertex::with_bulge(Vec2::new(c.x - half, c.y), 1.0), PolyVertex::with_bulge(Vec2::new(c.x + half, c.y), 1.0)];
    EntityKind::LwPolyline(aad_doc::LwPolyline { vertices: vs, closed: true, const_width: r, elevation: 0.0, plinegen: false })
}

/// The parameters and derived values of `d` with parameter values `vals`.
fn context<'a>(d: &'a Def, vals: &BTreeMap<String, f64>) -> Result<Ctx<'a>> {
    let mut cx = Ctx { def: d, vars: vals.clone(), out: Vec::new() };
    for v in &d.vars {
        let x = cx.num(&v.expr)?;
        cx.vars.insert(v.name.clone(), x);
    }
    Ok(cx)
}

/// The objects of smart block `id` with its default parameters (millimetres, about the origin), for a
/// thumbnail. Nothing is added to the drawing.
pub(super) fn default_entities(d: &aad_doc::Drawing, id: &str) -> Result<Vec<Entity>> {
    let found = find_def(d, id)?;
    let vals = resolve("blocks.preview", &found.def, &Map::new(), &Map::new())?;
    let objs = evaluate(&found.def, &vals)?;
    Ok(objs
        .into_iter()
        .map(|(layer, _, color, kind)| {
            let mut e = Entity::new(aad_doc::Handle(0), kind);
            if let Some(l) = layer {
                e.common.layer = l;
            }
            let layer_color = e.common.layer.as_str();
            let from_layer = found.def.layers.iter().find(|l| l.name == layer_color).map(|l| l.color);
            if let Some(c) = color.or(from_layer) {
                e.common.color = aad_color::Color::from_aci(c);
            }
            e
        })
        .collect())
}

/// Evaluate `d` with parameter values `vals` into objects (millimetres, about the origin).
fn evaluate(d: &Def, vals: &BTreeMap<String, f64>) -> Result<Vec<Made>> {
    let mut cx = context(d, vals)?;
    cx.items(&d.items)?;
    if cx.out.is_empty() {
        return Err(err(format!("`{}` draws nothing with these parameters", d.id)));
    }
    Ok(cx.out)
}

fn block_name(d: &Def, vals: &BTreeMap<String, f64>, stored: Option<&Value>) -> String {
    let mut key = json!({ "id": d.id, "params": params_json(vals) });
    if let Some(v) = stored {
        // An inline definition may differ from a built-in one with the same id.
        key["def"] = json!(aad_param::fnv64(aad_param::canonical(v).as_bytes()));
    }
    let h = aad_param::fnv64(aad_param::canonical(&key).as_bytes()) & 0xFFFF_FFFF;
    let id: String = d.id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
    format!("SB_{id}_{h:08X}")
}

/// Make (or reuse) the block for `d` with `vals`; returns its name.
fn build(s: &mut Session, d: &Def, vals: &BTreeMap<String, f64>, stored: Option<&Value>) -> Result<String> {
    let name = block_name(d, vals, stored);
    if s.doc()?.block(&name).is_some() {
        return Ok(name);
    }
    let objs = evaluate(d, vals)?;
    let style = s.doc()?.header.str("TEXTSTYLE", "Standard");
    let doc = s.doc_mut()?;
    for ls in &d.layers {
        super::hcw::ensure_layer(doc, &ls.name, ls.color, &ls.linetype, ls.lineweight, &ls.description);
    }
    let mut b = Block::new(&name);
    b.description = json!({ TAG: 1, "id": d.id, "params": params_json(vals) }).to_string();
    for (layer, lt, color, mut kind) in objs {
        if let EntityKind::Text(t) = &mut kind {
            t.style = style.clone();
        }
        let mut e = Entity::new(doc.new_handle(), kind);
        if let Some(c) = color {
            e.common.color = aad_color::Color::from_aci(c);
        }
        if let Some(l) = layer {
            if !valid_layer(&l) {
                return Err(err(format!("`{}`: layer `{l}` is not a valid name", d.id)));
            }
            doc.ensure_layer(&l);
            e.common.layer = l;
        }
        if let Some(lt) = lt {
            e.common.linetype = load_linetype(doc, &lt).ok_or_else(|| err(format!("`{}`: no linetype `{lt}`", d.id)))?;
        }
        b.entities.push(e);
    }
    doc.blocks.insert(name.clone(), Arc::new(b));
    Ok(name)
}

/// Make the library linetype `name` available; returns its name as stored.
fn load_linetype(d: &mut aad_doc::Drawing, name: &str) -> Option<String> {
    if let Some(l) = d.linetype(name) {
        return Some(l.name.clone());
    }
    let lt = aad_doc::library::standard_linetypes_for(d).into_iter().find(|l| l.name.eq_ignore_ascii_case(name))?;
    let n = lt.name.clone();
    d.linetypes.push(lt);
    Some(n)
}

/// What a block's description says about the smart block it was made from.
fn smart_of(b: &Block) -> Option<(String, Map<String, Value>)> {
    let v: Value = serde_json::from_str(&b.description).ok()?;
    v.get(TAG)?;
    Some((v.get("id")?.as_str()?.to_string(), v.get("params")?.as_object()?.clone()))
}

/// The smart block a reference uses: (handle, block name, id, stored parameters).
fn smart_ref(s: &Session, h: aad_doc::Handle) -> Result<Option<(String, String, Map<String, Value>)>> {
    let d = s.doc()?;
    let Some(e) = d.entity(h) else { return Err(err(format!("no object {}", h.hex()))) };
    let EntityKind::Insert(i) = &e.kind else { return Ok(None) };
    let Some(b) = d.block(&i.block) else { return Ok(None) };
    Ok(smart_of(b).map(|(id, p)| (i.block.clone(), id, p)))
}

/// The O-CoreOp element page a reinforced concrete block is the detail of, if it is one.
fn detail_kind(id: &str) -> Option<&'static str> {
    Some(match id {
        "rcc.beam.section" | "rcc.beam.elevation" => "beams",
        "rcc.column.rect" | "rcc.column.circular" => "columns",
        "rcc.slab.section" => "slabs",
        "rcc.footing.section" | "rcc.footing.combined" | "rcc.raft.lift" => "footings",
        "rcc.lintel.section" => "lintels",
        _ => return None,
    })
}

/// The structural details of a drawing: every reference to a reinforced concrete block in the model, with the parameters it was placed with.
/// The Windows app records them in the project file when the drawing is saved into it, for O-CoreOp to draw (read only).
#[cfg(not(target_arch = "wasm32"))]
pub fn structural_details(doc: &aad_doc::Drawing, drawing: &str) -> Vec<aad_odp::Detail> {
    let mut out = Vec::new();
    for store in doc.model_stores() {
        for e in store.iter() {
            let EntityKind::Insert(i) = &e.kind else { continue };
            let Some(b) = doc.block(&i.block) else { continue };
            let Some((id, params)) = smart_of(b) else { continue };
            let Some(kind) = detail_kind(&id) else { continue };
            let Ok(found) = find_def(doc, &id) else { continue };
            let Ok(vals) = resolve("", &found.def, &params, &Map::new()) else { continue };
            let size = |k: &str| vals.get(k).map_or_else(String::new, |v| fmt_num(*v));
            let dims = match kind {
                "beams" | "columns" if id == "rcc.column.circular" => format!("{} dia", size("dia")),
                "beams" | "columns" => format!("{} x {}", size("width"), size("depth")),
                "slabs" => format!("{} thick", size("thick")),
                "footings" => format!("{} x {} deep", size("length"), size("depth")),
                _ => format!("{} span, {} deep", size("span"), size("depth")),
            };
            let handle = e.handle.hex();
            out.push(aad_odp::Detail {
                id: format!("{drawing}/{handle}"),
                drawing: drawing.to_string(),
                handle,
                kind: kind.to_string(),
                block: id,
                params: params_json(&vals),
                layer: e.common.layer.clone(),
                label: format!("{} ({dims})", found.def.title.split(" (").next().unwrap_or(&found.def.title)),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// The smart blocks a drawing can insert (the definitions kept in it, then the built-in library), as
/// `smartblock.list` reports them. With a sheet set, only those whose layer the set maps: Electrical
/// gets the electrical blocks (`E-*` layers), Plumbing the plumbing ones, and so on.
pub fn library_entries(d: &aad_doc::Drawing, set: Option<&aad_doc::SheetSet>) -> Vec<Value> {
    let mut blocks: Vec<Value> = Vec::new();
    if let Some(m) = project::get(d, DEFS_KEY).as_object() {
        for v in m.values() {
            if let Ok(def) = serde_json::from_value::<Def>(v.clone()) {
                let mut j = spec_json(&def);
                j["stored"] = json!(true);
                blocks.push(j);
            }
        }
    }
    if let Ok(lib) = library() {
        for def in lib {
            if !blocks.iter().any(|b| b["id"] == def.id.as_str()) {
                blocks.push(spec_json(def));
            }
        }
    }
    if let Some(set) = set {
        blocks.retain(|b| b["layer"].as_str().is_some_and(|layer| set.maps_layer(layer)));
    }
    blocks
}

fn spec_json(d: &Def) -> Value {
    json!({
        "id": d.id,
        "title": d.title,
        "discipline": d.discipline,
        "description": d.description,
        "layer": d.layer,
        "grips": d.grips.iter().map(|g| json!({ "param": g.param, "axis": if g.axis == Axis::X { "x" } else { "y" } })).collect::<Vec<_>>(),
        "params": d.params.iter().map(|p| json!({
            "name": p.name,
            "label": if p.label.is_empty() { &p.name } else { &p.label },
            "unit": p.unit,
            "default": p.default,
            "min": p.min,
            "max": p.max,
            "integer": p.integer,
            "choices": p.choices.iter().map(|c| json!({"label": c.label, "value": c.value})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------------------------
// Commands

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let want = str_param(p, "discipline").map(str::to_ascii_lowercase);
    let d = s.doc()?;
    let set = match p.get("sheetSet") {
        None | Some(Value::Null | Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(d.active_sheet_ref().ok_or_else(|| err("`sheetSet: true` needs a sheet set to be the active tab"))?),
        Some(Value::String(name)) => Some(d.sheet_set_by_name(name).ok_or_else(|| err(format!("no sheet set `{name}`")))?),
        Some(_) => return Err(err("`sheetSet` is true or a sheet set name")),
    };
    let mut blocks = library_entries(d, set);
    if let Some(w) = &want {
        blocks.retain(|b| b["discipline"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(w)));
    }
    Ok(json!({ "blocks": blocks }))
}

fn run_rcc_column(s: &mut Session, p: &Value) -> Result<Value> {
    insert_preset(s, p, "rcc.column.rect")
}

fn run_rcc_column_round(s: &mut Session, p: &Value) -> Result<Value> {
    insert_preset(s, p, "rcc.column.circular")
}

fn run_rcc_beam(s: &mut Session, p: &Value) -> Result<Value> {
    insert_preset(s, p, "rcc.beam.section")
}

fn run_rcc_footing(s: &mut Session, p: &Value) -> Result<Value> {
    insert_preset(s, p, "rcc.footing.section")
}

fn run_rcc_slab(s: &mut Session, p: &Value) -> Result<Value> {
    insert_preset(s, p, "rcc.slab.section")
}

fn run_rcc_lintel(s: &mut Session, p: &Value) -> Result<Value> {
    insert_preset(s, p, "rcc.lintel.section")
}

/// `smartblock.insert` for the block `id` unless the parameters name another.
fn insert_preset(s: &mut Session, p: &Value, id: &str) -> Result<Value> {
    let mut q = p.as_object().cloned().unwrap_or_default();
    q.entry("id".to_string()).or_insert_with(|| Value::String(id.to_string()));
    run_insert(s, &Value::Object(q))
}

fn run_insert(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartblock.insert";
    let (found, inline) = match p.get("definition") {
        Some(v) if !v.is_null() => {
            if v.to_string().len() > MAX_DEF_BYTES {
                return Err(bad(CMD, "the definition is larger than 64 KB"));
            }
            let d: Def = serde_json::from_value(v.clone()).map_err(|e| bad(CMD, format!("definition: {e}")))?;
            check_def(&d).map_err(|e| bad(CMD, format!("definition: {e}")))?;
            (Found { def: Arc::new(d), stored: Some(v.clone()) }, true)
        }
        _ => {
            let id = str_param(p, "id").ok_or_else(|| bad(CMD, "`id` or `definition` is required (see smartblock.list)"))?;
            (find_def(s.doc()?, id)?, false)
        }
    };
    let def = &*found.def;
    let at = point_req(CMD, p, "at")?;
    let vals = resolve(CMD, def, &Map::new(), &given_params(CMD, p)?)?;
    let user_scale = f64_or(p, "scale", 1.0);
    if !user_scale.is_finite() || user_scale <= 0.0 {
        return Err(bad(CMD, "`scale` must be a positive number"));
    }
    let rot = f64_or(p, "rotation", 0.0);
    if !rot.is_finite() {
        return Err(bad(CMD, "`rotation` must be a number"));
    }
    let units = drawing_units(s.doc()?, CMD, p)?;
    let scale = user_scale * 0.001 / units.metres();
    let layer = match str_param(p, "layer") {
        Some(l) if valid_layer(l) => Some(l.to_string()),
        Some(_) => return Err(bad(CMD, "`layer` is not a valid name")),
        None if !def.layer.is_empty() => Some(def.layer.clone()),
        None => None,
    };
    if inline {
        let mut stored = project::get(s.doc()?, DEFS_KEY).as_object().cloned().unwrap_or_default();
        if stored.len() >= MAX_STORED_DEFS && !stored.contains_key(&def.id) {
            return Err(bad(CMD, format!("the drawing already keeps {MAX_STORED_DEFS} smart block definitions")));
        }
        // Built first: a definition that fails to draw is not kept.
        let name = build(s, def, &vals, found.stored.as_ref())?;
        if let Some(v) = &found.stored {
            stored.insert(def.id.clone(), v.clone());
        }
        project::set(s.doc_mut()?, DEFS_KEY, &Value::Object(stored));
        return place(s, def, &name, at, scale, rot, layer, &vals);
    }
    let name = build(s, def, &vals, found.stored.as_ref())?;
    place(s, def, &name, at, scale, rot, layer, &vals)
}

/// Insert block `name` and put the reference on `layer` (the definition's layer is created with its look).
#[allow(clippy::too_many_arguments)]
fn place(
    s: &mut Session,
    def: &Def,
    name: &str,
    at: Vec2,
    scale: f64,
    rot: f64,
    layer: Option<String>,
    vals: &BTreeMap<String, f64>,
) -> Result<Value> {
    if let Some(l) = &layer {
        if *l == def.layer {
            ensure_layer(s.doc_mut()?, &def.layer, def.color, "Continuous", def.lineweight, &def.title);
        } else {
            s.doc_mut()?.ensure_layer(l);
        }
    }
    let h = super::blocks::insert(s, name, at, scale, rot.to_radians(), &Map::new())?;
    if let Some(l) = layer {
        s.doc_mut()?.modify_entity(h, |e| e.common.layer = l)?;
    }
    // Selected, so the Properties panel lists its parameters.
    s.set_selection(vec![h]);
    Ok(json!({ "handle": h.hex(), "block": name, "id": def.id, "params": params_json(vals) }))
}

fn run_get(s: &mut Session, p: &Value) -> Result<Value> {
    for h in targets(s, p)? {
        if let Some((block, id, params)) = smart_ref(s, h)? {
            let found = find_def(s.doc()?, &id)?;
            let def = &*found.def;
            return Ok(json!({ "handle": h.hex(), "id": id, "title": def.title, "block": block, "params": params, "spec": spec_json(def) }));
        }
    }
    Err(bad("smartblock.get", "no smart block reference given or selected"))
}

/// `smartblock.check`: the IS 456 report for a placed RCC block (`handle`, else the selection) or for
/// a definition with the given parameters.
fn run_check(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartblock.check";
    let (id, vals) = if let Some(id) = str_param(p, "id") {
        let found = find_def(s.doc()?, id)?;
        (id.to_string(), resolve(CMD, &found.def, &Map::new(), &given_params(CMD, p)?)?)
    } else {
        let mut hit = None;
        for h in targets(s, p)? {
            if let Some((_, id, params)) = smart_ref(s, h)? {
                let found = find_def(s.doc()?, &id)?;
                hit = Some((id, resolve(CMD, &found.def, &params, &Map::new())?));
                break;
            }
        }
        hit.ok_or_else(|| bad(CMD, "no smart block reference given or selected"))?
    };
    let report = crate::is456::check(&id, &vals).ok_or_else(|| bad(CMD, format!("`{id}` has no IS 456 check (reinforced concrete blocks only)")))?;
    Ok(report_json(&id, &report))
}

fn report_json(id: &str, r: &crate::is456::Report) -> Value {
    json!({
        "id": id,
        "ok": r.ok(),
        "lines": r.lines.iter().map(|l| json!({ "label": l.label, "value": l.value })).collect::<Vec<_>>(),
        "issues": r.issues.iter().map(|i| json!({ "clause": i.clause, "message": i.message })).collect::<Vec<_>>(),
    })
}

/// Point reference `h` at the block of `found` with `vals` (made when missing); returns its name.
fn repoint(s: &mut Session, h: aad_doc::Handle, found: &Found, vals: &BTreeMap<String, f64>) -> Result<String> {
    let name = build(s, &found.def, vals, found.stored.as_ref())?;
    let n = name.clone();
    s.doc_mut()?.modify_entity(h, move |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = n;
        }
    })?;
    Ok(name)
}

fn run_edit(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartblock.edit";
    let given = given_params(CMD, p)?;
    let hs = targets(s, p)?;
    edit_handles(s, CMD, &hs, &given)
}

/// Set parameters `given` on the smart block references among `hs` (others are skipped); an error
/// when there are none. `smartblock.edit` and the Properties panel (`properties.set` with
/// `sb:<name>` keys) use it.
pub(crate) fn edit_handles(s: &mut Session, cmd: &str, hs: &[aad_doc::Handle], given: &Map<String, Value>) -> Result<Value> {
    let mut edited = Vec::new();
    let mut blocks = Vec::new();
    // One lookup per definition, however many references share it.
    let mut defs: BTreeMap<String, Found> = BTreeMap::new();
    for &h in hs {
        let Some((_, id, stored)) = smart_ref(s, h)? else { continue };
        let found = match defs.get(&id) {
            Some(f) => f.clone(),
            None => {
                let f = find_def(s.doc()?, &id)?;
                defs.insert(id, f.clone());
                f
            }
        };
        let vals = resolve(cmd, &found.def, &stored, given)?;
        blocks.push(repoint(s, h, &found, &vals)?);
        edited.push(h.hex());
    }
    if edited.is_empty() {
        return Err(bad(cmd, "no smart block reference given or selected"));
    }
    Ok(json!({ "edited": edited, "blocks": blocks }))
}

/// Prefix of the Properties panel keys for smart block parameters (`sb:width`).
pub(crate) const FIELD_PREFIX: &str = "sb:";

/// Properties panel rows for a smart block reference's parameters (none for other inserts):
/// `{key: "sb:<name>", label, value, text: false, choices?: [labels]}`; a choice's value is its
/// label. `properties.set` with these keys edits the parameters like `smartblock.edit`.
pub fn property_fields(doc: &aad_doc::Drawing, ins: &aad_doc::Insert) -> Vec<Value> {
    let Some(b) = doc.block(&ins.block) else { return Vec::new() };
    let Some((id, params)) = smart_of(b) else { return Vec::new() };
    let Ok(found) = find_def(doc, &id) else { return Vec::new() };
    let Ok(vals) = resolve("", &found.def, &params, &Map::new()) else { return Vec::new() };
    let mut out = vec![json!({ "label": "Parametric block", "value": found.def.title, "text": true })];
    let report = crate::is456::check(&id, &vals);
    // The IS 456 verdict comes first, so a broken limit shows without scrolling past the parameters.
    if let Some(r) = &report {
        let summary = if r.ok() { "Complies".to_string() } else { format!("{} to review (see below)", r.issues.len()) };
        out.push(json!({ "label": "IS 456:2000", "value": summary, "text": true }));
    }
    for p in &found.def.params {
        let v = vals.get(&p.name).copied().unwrap_or(p.default);
        let label = match (p.label.is_empty(), p.unit.is_empty()) {
            (true, true) => p.name.clone(),
            (true, false) => format!("{} ({})", p.name, p.unit),
            (false, true) => p.label.clone(),
            (false, false) => format!("{} ({})", p.label, p.unit),
        };
        let key = format!("{FIELD_PREFIX}{}", p.name);
        if p.choices.is_empty() {
            out.push(json!({ "key": key, "label": label, "value": fmt_num(v), "text": false }));
        } else {
            let value = p.choices.iter().find(|c| c.value == v).map_or_else(|| fmt_num(v), |c| c.label.clone());
            let choices: Vec<&str> = p.choices.iter().map(|c| c.label.as_str()).collect();
            out.push(json!({ "key": key, "label": label, "value": value, "text": false, "choices": choices }));
        }
    }
    // The IS 456 result of a reinforced concrete block: a summary, the quantities the code limits, and
    // each limit the parameters break with its clause. Read only: the parameters above are the inputs.
    if let Some(r) = &report {
        for l in &r.lines {
            out.push(json!({ "label": l.label, "value": l.value, "text": true }));
        }
        for i in &r.issues {
            out.push(json!({ "label": format!("Check, cl. {}", i.clause), "value": i.message, "text": true }));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Grips

/// A smart block reference's definition, parameter values and block-to-world transform.
fn grip_context(doc: &aad_doc::Drawing, ins: &aad_doc::Insert) -> Option<(Found, BTreeMap<String, f64>, aad_geom::Mat3)> {
    let b = doc.block(&ins.block)?;
    let (id, params) = smart_of(b)?;
    let found = find_def(doc, &id).ok()?;
    if found.def.grips.is_empty() {
        return None;
    }
    let vals = resolve("", &found.def, &params, &Map::new()).ok()?;
    Some((found, vals, ins.transform(b.base.xy())))
}

/// The parameter grips of a smart block reference, in world coordinates (none for other inserts).
pub(crate) fn param_grips(doc: &aad_doc::Drawing, ins: &aad_doc::Insert) -> Vec<Vec2> {
    let Some((found, vals, m)) = grip_context(doc, ins) else { return Vec::new() };
    let Ok(cx) = context(&found.def, &vals) else { return Vec::new() };
    found.def.grips.iter().filter_map(|g| cx.pt(&g.at).ok()).map(|q| m.apply(q)).collect()
}

/// Drag parameter grip `index` of reference `h` to world point `to`: the parameter changes by the
/// drag along the grip's axis (block millimetres times `per`), rounded to the grip's step and kept
/// in range. One undo step with the rest of the command.
pub(crate) fn drag_grip(s: &mut Session, cmd: &str, h: aad_doc::Handle, index: usize, to: Vec2) -> Result<Value> {
    let doc = s.doc()?;
    let Some(EntityKind::Insert(ins)) = doc.entity(h).map(|e| &e.kind) else { return Err(bad(cmd, "no such grip")) };
    let (found, mut vals, m) = grip_context(doc, ins).ok_or_else(|| bad(cmd, "no such grip"))?;
    let g = found.def.grips.get(index).ok_or_else(|| bad(cmd, "no such grip"))?;
    let p = found.def.params.iter().find(|p| p.name == g.param).ok_or_else(|| bad(cmd, "no such grip"))?;
    let cx = context(&found.def, &vals)?;
    let from = cx.pt(&g.at)?;
    let per = match &g.per {
        Some(n) => cx.num(n)?,
        None => 1.0,
    };
    let local = m.inverse().ok_or_else(|| bad(cmd, "the reference has zero scale"))?.apply(to);
    if !local.is_finite() {
        return Err(bad(cmd, "invalid point"));
    }
    let d = if g.axis == Axis::X { local.x - from.x } else { local.y - from.y };
    let old = vals.get(&p.name).copied().unwrap_or(p.default);
    let step = if p.integer { g.step.unwrap_or(1.0).max(1.0).round() } else { g.step.unwrap_or(1.0) };
    let mut v = ((old + per * d) / step).round() * step;
    if let Some(lo) = p.min {
        v = v.max(lo);
    }
    if let Some(hi) = p.max {
        v = v.min(hi);
    }
    if !v.is_finite() || check_value(p, v).is_err() {
        return Err(bad(cmd, format!("`{}` cannot take that value", p.name)));
    }
    vals.insert(p.name.clone(), v);
    let name = repoint(s, h, &found, &vals)?;
    Ok(json!({ "handle": h.hex(), "block": name, "param": p.name, "value": v }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_parses_and_ids_match_the_form_list() {
        let lib = library().unwrap();
        let ids: Vec<&str> = lib.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, IDS, "IDS must list the library in order");
        for want in ["rcc", "structural", "plumbing", "hvac", "electrical", "interior"] {
            assert!(lib.iter().any(|d| d.discipline == want), "no {want} smart block");
        }
    }

    #[test]
    fn every_definition_draws_at_its_defaults_and_range_ends() {
        for d in library().unwrap() {
            let base = resolve("t", d, &Map::new(), &Map::new()).unwrap();
            assert!(!evaluate(d, &base).unwrap().is_empty(), "{}", d.id);
            for p in &d.params {
                for v in [p.min, p.max].into_iter().flatten() {
                    let mut vals = base.clone();
                    vals.insert(p.name.clone(), if p.integer { v.round() } else { v });
                    evaluate(d, &vals).unwrap_or_else(|e| panic!("{} {}={v}: {e}", d.id, p.name));
                }
                for c in &p.choices {
                    let mut vals = base.clone();
                    vals.insert(p.name.clone(), c.value);
                    evaluate(d, &vals).unwrap_or_else(|e| panic!("{} {}={}: {e}", d.id, p.name, c.label));
                }
            }
        }
    }

    #[test]
    fn text_template_uses_values_and_choice_labels() {
        let d: Def = serde_json::from_value(json!({
            "id": "t", "title": "T",
            "params": [{"name": "w", "default": 230}, {"name": "k", "default": 2, "choices": [{"label": "Two", "value": 2}]}],
            "items": [{"text": {"at": [0, 0], "height": 10, "value": "{w} {k} {zz}"}}]
        }))
        .unwrap();
        let vals = resolve("t", &d, &Map::new(), &Map::new()).unwrap();
        let out = evaluate(&d, &vals).unwrap();
        let EntityKind::Text(t) = &out[0].3 else { panic!() };
        assert_eq!(t.value, "230 Two {zz}");
    }
}

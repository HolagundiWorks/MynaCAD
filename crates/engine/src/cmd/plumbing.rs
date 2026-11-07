//! Plumbing layout, the counterpart of `electrical`: fixtures and terminals are mapped, numbered,
//! connected by pipes, checked and scheduled.
//!
//! * `plumbing.blocks` says which blocks are water closets, basins, sinks, showers, traps, tanks,
//!   inspection chambers and risers. Toolbar symbols are recognised by their smart block id; other
//!   blocks by name (names may use `*`). The choice is kept in the drawing.
//! * `plumbing.number` gives each block an ID (`WC-01`, `RS-02`) and writes it beside the block.
//! * `plumbing.layers` names the layers the supply and the drain pipes are drawn on.
//! * `plumbing.pipe` draws a pipe run on the service's layer and keeps its diameter.
//! * `plumbing.connect` works out from the pipes which fixtures reach which source and flags what is
//!   not connected. Supply and drainage are separate networks, because a fixture normally has both. A
//!   pipe reaches a block when one of its vertices lies within about 20 mm of the block's extents;
//!   pipes that meet end to end or in a T are one run. A tank, inspection chamber or riser is a
//!   source (the tank and the riser for supply, the chamber and the riser for drainage); a fixture is
//!   a leaf, so a run does not carry on through it.
//! * `plumbing.schedule` draws the schedule: fixtures per source, one row per fixture, or the pipe
//!   length by service and diameter.
//!
//! Not implemented: pipe sizing, fixture units, slope and invert levels, fittings and valves, and hot
//! water. A riser's service (soil, waste, rainwater, vent, cold, hot) is a block parameter that the
//! block name does not carry, so every riser counts as a source for both supply and drainage.

use std::collections::{BTreeMap, BTreeSet};

use aad_doc::{Drawing, EntityKind, HAlign, Handle, Space, VAlign, entity_bounds};
use aad_geom::{Bounds2, Vec2};
use serde_json::{Map, Value, json};

use super::draw::make_text;
use super::hcw::{add_on, ensure_layer};
use super::qselect::wildcard;
use super::smartblock::smart_key;
use super::takeoff::{segments_of, table, table_ctx};
use super::*;
use crate::annot::{self as ua};
use crate::{Result, Session};

/// One kind of block: code, description, whether it is a source (a terminal of the pipe network), and
/// the smart block id (see `smartblock::smart_key`) of its toolbar symbol.
pub struct Kind {
    pub code: &'static str,
    pub desc: &'static str,
    pub source: bool,
    pub smart: &'static str,
}

pub const KINDS: &[Kind] = &[
    Kind { code: "WC", desc: "Water closet (European)", source: false, smart: "PLUMB_WC_EUROPEAN" },
    Kind { code: "IWC", desc: "Water closet (Indian)", source: false, smart: "PLUMB_WC_INDIAN" },
    Kind { code: "WB", desc: "Wash basin", source: false, smart: "PLUMB_BASIN" },
    Kind { code: "KS", desc: "Kitchen sink", source: false, smart: "PLUMB_SINK_KITCHEN" },
    Kind { code: "SH", desc: "Shower", source: false, smart: "PLUMB_SHOWER" },
    Kind { code: "FT", desc: "Floor trap", source: false, smart: "PLUMB_FLOOR_TRAP" },
    Kind { code: "IC", desc: "Inspection chamber", source: true, smart: "PLUMB_CHAMBER" },
    Kind { code: "TK", desc: "Water tank", source: true, smart: "PLUMB_TANK_ROUND" },
    Kind { code: "RS", desc: "Riser or stack", source: true, smart: "PLUMB_RISER" },
];

/// A pipe network: its pipes' default layer, the sources that feed it, and the fixtures that need it.
struct Service {
    code: &'static str,
    key: &'static str,
    title: &'static str,
    layer: &'static str,
    default_dia: f64,
    sources: &'static [&'static str],
    needs: &'static [&'static str],
}

const SERVICES: &[Service] = &[
    Service {
        code: "SUP",
        key: "supply",
        title: "Water supply",
        layer: "P-WATR",
        default_dia: 20.0,
        sources: &["TK", "RS"],
        needs: &["WC", "IWC", "WB", "KS", "SH"],
    },
    Service {
        code: "DRN",
        key: "drain",
        title: "Drainage",
        layer: "P-DRAN",
        default_dia: 100.0,
        sources: &["IC", "RS"],
        needs: &["WC", "IWC", "WB", "KS", "SH", "FT"],
    },
];

const L_LABELS: &str = "PL-LABELS";
const L_FLAGS: &str = "PL-FLAGS";
const L_TABLE: &str = "PL-TABLE";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("plumbing.blocks", "Map Plumbing Blocks", run_blocks)
            .menu(&["Architecture", "Plumbing", "Map Blocks"])
            .alias(&["plblocks"])
            .params("{blocks?: {WC: [\"WC*\"], ...}} → {blocks, found: {code: count}} (codes: WC IWC WB KS SH FT IC TK RS)"),
        CommandSpec::new("plumbing.number", "Number Plumbing Blocks", run_number)
            .menu(&["Architecture", "Plumbing", "Number Blocks"])
            .alias(&["plnum"])
            .params("{codes?: [\"WC\", \"RS\", ...] (default all), renumber?: bool (default false: IDs already given are kept)} → {ids}"),
        CommandSpec::new("plumbing.layers", "Pipe Layers", run_layers)
            .menu(&["Architecture", "Plumbing", "Pipe Layers"])
            .alias(&["pllayers"])
            .params("{supply?: [layer], drain?: [layer]} → {supply, drain}"),
        CommandSpec::new("plumbing.pipe", "Draw Pipe", run_pipe)
            .menu(&["Architecture", "Plumbing", "Draw Pipe"])
            .alias(&["pipe"])
            .params("{points: [[x,y],...] (2 or more), service?: \"supply\" (default) | \"drain\", dia?: mm (default 20 supply, 100 drain)} → {handle, service, layer, dia, length}"),
        CommandSpec::new("plumbing.connect", "Check Piping", run_connect)
            .menu(&["Architecture", "Plumbing", "Check Piping"])
            .alias(&["plconnect"])
            .params("{snap?: drawing units (default 20 mm), flag?: bool (default true)} → {services: [{service, sources: [{id, fixtures, counts, pipe_m}], unconnected, ambiguous}], emptySources, stacks, fixtures}"),
        CommandSpec::new("plumbing.schedule", "Plumbing Schedule", run_schedule)
            .menu(&["Architecture", "Plumbing", "Schedule"])
            .alias(&["plschedule"])
            .params("{mode?: \"matrix\" (default) | \"fixture\" | \"pipes\", at?: [x,y]} → {header, rows, drawn}"),
        CommandSpec::new("plumbing.update", "Update Plumbing", run_update)
            .menu(&["Architecture", "Plumbing", "Update"])
            .alias(&["plupdate"])
            .params("{} → {numbered, connected} (numbers new blocks, moves labels, re-checks the piping, redraws the schedule)"),
        CommandSpec::new("plumbing.autonumber", "Auto-number Plumbing Blocks", run_autonumber)
            .menu(&["Architecture", "Plumbing", "Auto-number on Insert"])
            .params("{enabled?: bool} → {enabled}. When on (the default), inserting a mapped block numbers it."),
    ]
}

fn all_codes() -> Vec<&'static str> {
    KINDS.iter().map(|k| k.code).collect()
}

fn kind_of(code: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.code == code)
}

/// Number a newly inserted block when it is a plumbing fixture or source. Default on.
pub(crate) fn on_insert(s: &mut Session) -> Result<()> {
    let mut o = state(s.doc()?);
    if o.get("autonumber").and_then(Value::as_bool) == Some(false) {
        return Ok(());
    }
    let space = s.space();
    let list = items(s.doc()?, &space, &o);
    if list.is_empty() {
        return Ok(());
    }
    let ids = labels_of(&o);
    if list.iter().all(|it| ids.contains_key(&it.handle.hex())) {
        return Ok(());
    }
    number(s, &mut o, &all_codes(), false)?;
    save(s.doc_mut()?, &o);
    Ok(())
}

fn run_autonumber(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    if let Some(enabled) = p.get("enabled").and_then(Value::as_bool) {
        o.insert("autonumber".into(), json!(enabled));
        save(s.doc_mut()?, &o);
    }
    let enabled = o.get("autonumber").and_then(Value::as_bool).unwrap_or(true);
    Ok(json!({ "enabled": enabled }))
}

fn state(d: &Drawing) -> Map<String, Value> {
    match project::get(d, "plumbing") {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

fn save(d: &mut Drawing, o: &Map<String, Value>) {
    project::set(d, "plumbing", &Value::Object(o.clone()));
}

/// Block name patterns of a kind: the saved choice, else the kind code and `code-*` / `code_*`.
fn patterns(o: &Map<String, Value>, code: &str) -> Vec<String> {
    let chosen = o
        .get("blocks")
        .and_then(|b| b.get(code))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>());
    chosen.filter(|v| !v.is_empty()).unwrap_or_else(|| vec![code.to_string(), format!("{code}-*"), format!("{code}_*")])
}

struct Item {
    handle: Handle,
    code: &'static str,
    bounds: Bounds2,
    at: Vec2,
}

fn items(d: &Drawing, space: &Space, o: &Map<String, Value>) -> Vec<Item> {
    let mut out = Vec::new();
    let pats: Vec<(&'static str, Vec<String>)> = KINDS.iter().map(|k| (k.code, patterns(o, k.code))).collect();
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| d.is_visible(e)) {
            let EntityKind::Insert(i) = &e.kind else { continue };
            // A toolbar symbol is recognised by its smart block id; other smart blocks are not plumbing.
            let code = match smart_key(&i.block) {
                Some(key) => KINDS.iter().find(|k| k.smart == key).map(|k| k.code),
                None => pats
                    .iter()
                    .find(|(_, ps)| ps.iter().any(|p| wildcard(&p.to_ascii_uppercase(), &i.block.to_ascii_uppercase())))
                    .map(|(code, _)| *code),
            };
            let Some(code) = code else { continue };
            let bounds = entity_bounds(d, e, 0);
            out.push(Item { handle: e.handle, code, bounds, at: i.insert.xy() });
        }
    }
    // Left to right, then bottom to top.
    out.sort_by(|a, b| a.at.x.total_cmp(&b.at.x).then(a.at.y.total_cmp(&b.at.y)).then(a.handle.0.cmp(&b.handle.0)));
    out
}

fn run_blocks(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    if let Some(Value::Object(b)) = p.get("blocks") {
        let mut clean = Map::new();
        for (code, v) in b {
            if kind_of(code).is_none() {
                return Err(bad("plumbing.blocks", format!("unknown kind `{code}`")));
            }
            let names: Vec<Value> = v
                .as_array()
                .ok_or_else(|| bad("plumbing.blocks", format!("`{code}` must be a list of block names")))?
                .iter()
                .filter(|n| n.as_str().is_some_and(|t| !t.is_empty() && t.len() <= 80))
                .cloned()
                .collect();
            clean.insert(code.clone(), Value::Array(names));
        }
        // A block has one role.
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for (code, v) in &clean {
            for n in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                if let Some(prev) = seen.insert(n.to_ascii_uppercase(), code.clone())
                    && prev != *code
                {
                    return Err(bad("plumbing.blocks", format!("block `{n}` is chosen for both {prev} and {code}")));
                }
            }
        }
        o.insert("blocks".into(), Value::Object(clean));
        save(s.doc_mut()?, &o);
    }
    let space = s.space();
    let found: BTreeMap<&str, usize> = items(s.doc()?, &space, &o).iter().fold(BTreeMap::new(), |mut m, i| {
        *m.entry(i.code).or_insert(0) += 1;
        m
    });
    let blocks: Map<String, Value> = KINDS.iter().map(|k| (k.code.to_string(), json!(patterns(&o, k.code)))).collect();
    Ok(json!({ "blocks": blocks, "found": found }))
}

fn labels_of(o: &Map<String, Value>) -> BTreeMap<String, (String, Option<String>)> {
    o.get("ids")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(h, v)| (h.clone(), (v["id"].as_str().unwrap_or("").to_string(), v["label"].as_str().map(str::to_string)))).collect())
        .unwrap_or_default()
}

fn text_height(d: &Drawing) -> f64 {
    ua::drawing_unit(d).map_or(0.125, |u| 0.125 / u.metres())
}

/// Write each block's ID beside it, moving or removing earlier labels. Returns the count.
fn write_labels(s: &mut Session, o: &mut Map<String, Value>, list: &[Item]) -> Result<usize> {
    ensure_layer(s.doc_mut()?, L_LABELS, 3, "Continuous", 18, "Plumbing block IDs");
    let th = text_height(s.doc()?);
    let mut ids = o.get("ids").and_then(Value::as_object).cloned().unwrap_or_default();
    let live: BTreeSet<String> = list.iter().map(|i| i.handle.hex()).collect();
    // Labels of blocks that no longer exist go.
    for (h, v) in ids.clone() {
        if !live.contains(&h) {
            if let Some(lh) = v["label"].as_str().and_then(Handle::parse_hex) {
                s.doc_mut()?.remove_entity(lh);
            }
            ids.remove(&h);
        }
    }
    let mut n = 0;
    for it in list {
        let key = it.handle.hex();
        let Some(id) = ids.get(&key).and_then(|v| v["id"].as_str()).map(str::to_string) else { continue };
        let at = Vec2::new(it.bounds.max.x + th * 0.5, (it.bounds.min.y + it.bounds.max.y) / 2.0);
        let existing =
            ids.get(&key).and_then(|v| v["label"].as_str()).and_then(Handle::parse_hex).filter(|h| s.doc().is_ok_and(|d| d.entity(*h).is_some()));
        let label = match existing {
            Some(h) => {
                s.doc_mut()?.modify_entity(h, |e| {
                    if let EntityKind::Text(t) = &mut e.kind {
                        t.value = id.clone();
                        t.insert.x = at.x;
                        t.insert.y = at.y;
                        t.align_pt = Some(aad_geom::Vec3::new(at.x, at.y, 0.0));
                    }
                })?;
                h
            }
            None => {
                let kind = make_text(s, at, &id, th, 0.0, (HAlign::Left, VAlign::Middle));
                add_on(s, L_LABELS, kind)?
            }
        };
        ids.insert(key, json!({ "id": id, "label": label.hex() }));
        n += 1;
    }
    o.insert("ids".into(), Value::Object(ids));
    Ok(n)
}

fn number(s: &mut Session, o: &mut Map<String, Value>, codes: &[&str], renumber: bool) -> Result<usize> {
    let space = s.space();
    let list = items(s.doc()?, &space, o);
    let mut ids = o.get("ids").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut numbered = 0;
    for code in codes {
        let mut used: BTreeSet<u32> = BTreeSet::new();
        if !renumber {
            for it in list.iter().filter(|i| i.code == *code) {
                if let Some(n) =
                    ids.get(&it.handle.hex()).and_then(|v| v["id"].as_str()).and_then(|t| t.rsplit('-').next()).and_then(|t| t.parse::<u32>().ok())
                {
                    used.insert(n);
                }
            }
        }
        let mut next = 1u32;
        for it in list.iter().filter(|i| i.code == *code) {
            let key = it.handle.hex();
            if !renumber && ids.contains_key(&key) {
                continue;
            }
            while used.contains(&next) {
                next += 1;
            }
            used.insert(next);
            let prev_label = ids.get(&key).and_then(|v| v["label"].clone().as_str().map(str::to_string));
            ids.insert(key, json!({ "id": format!("{code}-{next:02}"), "label": prev_label }));
            numbered += 1;
        }
    }
    o.insert("ids".into(), Value::Object(ids));
    let list = items(s.doc()?, &space, o);
    write_labels(s, o, &list)?;
    Ok(numbered)
}

fn run_number(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    let codes: Vec<&'static str> = match p.get("codes").and_then(Value::as_array) {
        Some(a) => {
            let mut v = Vec::new();
            for c in a {
                let c = c.as_str().unwrap_or("");
                v.push(
                    KINDS
                        .iter()
                        .find(|k| k.code.eq_ignore_ascii_case(c))
                        .map(|k| k.code)
                        .ok_or_else(|| bad("plumbing.number", format!("unknown kind `{c}`")))?,
                );
            }
            v
        }
        None => all_codes(),
    };
    let n = number(s, &mut o, &codes, bool_or(p, "renumber", false))?;
    save(s.doc_mut()?, &o);
    let ids: BTreeMap<String, String> = labels_of(&o).into_iter().map(|(h, (id, _))| (h, id)).collect();
    Ok(json!({ "numbered": n, "ids": ids }))
}

/// The layers a service's pipes are on: the saved choice, else the service's own layer.
fn pipe_layers(o: &Map<String, Value>, svc: &Service) -> Vec<String> {
    o.get("layers")
        .and_then(|l| l.get(svc.key))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![svc.layer.to_string()])
}

fn run_layers(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    let mut layers = o.get("layers").and_then(Value::as_object).cloned().unwrap_or_default();
    for svc in SERVICES {
        if let Some(Value::Array(a)) = p.get(svc.key) {
            let v: Vec<Value> = a.iter().filter(|n| n.as_str().is_some_and(|t| !t.is_empty() && t.len() <= 80)).cloned().collect();
            if v.is_empty() {
                return Err(bad("plumbing.layers", format!("`{}` needs at least one layer name", svc.key)));
            }
            layers.insert(svc.key.into(), Value::Array(v));
        }
    }
    o.insert("layers".into(), Value::Object(layers));
    save(s.doc_mut()?, &o);
    let mut out = Map::new();
    for svc in SERVICES {
        out.insert(svc.key.into(), json!(pipe_layers(&o, svc)));
    }
    Ok(Value::Object(out))
}

fn service_by_key(command: &str, key: &str) -> Result<&'static Service> {
    SERVICES
        .iter()
        .find(|s| s.key.eq_ignore_ascii_case(key) || s.code.eq_ignore_ascii_case(key))
        .ok_or_else(|| bad(command, format!("`service` must be supply or drain, not `{key}`")))
}

/// Millimetres per drawing unit's metre: how many metres one drawing unit is (a drawing without a unit is taken as mm).
fn metres_per_unit(d: &Drawing) -> f64 {
    ua::drawing_unit(d).map_or(0.001, |u| u.metres())
}

fn run_pipe(s: &mut Session, p: &Value) -> Result<Value> {
    let svc = service_by_key("plumbing.pipe", str_param(p, "service").unwrap_or("supply"))?;
    let pts = points_param(p, "points").ok_or_else(|| bad("plumbing.pipe", "`points` [[x,y],...] (2 or more) is required"))?;
    if pts.len() < 2 {
        return Err(bad("plumbing.pipe", "need at least 2 points"));
    }
    let dia = f64_or(p, "dia", svc.default_dia);
    if !(dia.is_finite() && dia > 0.0 && dia <= 2000.0) {
        return Err(bad("plumbing.pipe", "`dia` must be a pipe diameter in mm, above 0 and at most 2000"));
    }
    let mut o = state(s.doc()?);
    let layer = pipe_layers(&o, svc).remove(0);
    ensure_layer(s.doc_mut()?, &layer, if svc.code == "SUP" { 5 } else { 30 }, "Continuous", 25, svc.title);
    let drawn = s.execute("pline", &json!({ "points": pts.iter().map(|q| json!([q.x, q.y])).collect::<Vec<_>>() }))?;
    let handle =
        drawn.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| bad("plumbing.pipe", "the pipe was not drawn"))?;
    s.doc_mut()?.modify_entity(handle, |e| e.common.layer = layer.clone())?;
    let mut pipes = o.get("pipes").and_then(Value::as_object).cloned().unwrap_or_default();
    pipes.insert(handle.hex(), json!(dia));
    o.insert("pipes".into(), Value::Object(pipes));
    save(s.doc_mut()?, &o);
    let per_unit = metres_per_unit(s.doc()?);
    let length: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>() * per_unit;
    Ok(json!({ "handle": handle.hex(), "service": svc.key, "layer": layer, "dia": dia, "length": length }))
}

// ---------------------------------------------------------------------------------------------
// Connectivity

struct Pipe {
    service: usize,
    segs: Vec<(Vec2, Vec2)>,
    /// Length in metres.
    length: f64,
    dia: f64,
}

fn pipes(d: &Drawing, space: &Space, o: &Map<String, Value>) -> Vec<Pipe> {
    let layers: Vec<Vec<String>> = SERVICES.iter().map(|s| pipe_layers(o, s).iter().map(|l| l.to_ascii_lowercase()).collect()).collect();
    let dias = o.get("pipes").and_then(Value::as_object);
    let per_unit = metres_per_unit(d);
    let mut out = Vec::new();
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| d.is_visible(e)) {
            let layer = e.common.layer.to_ascii_lowercase();
            let Some(service) = layers.iter().position(|ls| ls.contains(&layer)) else { continue };
            let segs: Vec<(Vec2, Vec2)> = segments_of(&e.kind).iter().map(|sg| (sg.start(), sg.end())).collect();
            if segs.is_empty() {
                continue;
            }
            let length = segs.iter().map(|(a, b)| a.dist(*b)).sum::<f64>() * per_unit;
            let dia = dias.and_then(|m| m.get(&e.handle.hex())).and_then(Value::as_f64).unwrap_or(SERVICES[service].default_dia);
            out.push(Pipe { service, segs, length, dia });
        }
    }
    out
}

fn dist_to_bounds(b: &Bounds2, p: Vec2) -> f64 {
    let dx = (b.min.x - p.x).max(0.0).max(p.x - b.max.x);
    let dy = (b.min.y - p.y).max(0.0).max(p.y - b.max.y);
    dx.hypot(dy)
}

fn seg_point_dist(a: Vec2, b: Vec2, p: Vec2) -> f64 {
    aad_geom::Line::new(a, b).closest(p).dist(p)
}

/// The network of one service: nodes are the items first, then that service's pipes.
struct Net {
    adj: Vec<Vec<usize>>,
    /// The index in `pipes` of each pipe node (node `items + k` is `pipes[idx[k]]`).
    idx: Vec<usize>,
}

fn build(list: &[Item], ps: &[Pipe], service: usize, snap: f64) -> Net {
    let idx: Vec<usize> = ps.iter().enumerate().filter(|(_, p)| p.service == service).map(|(i, _)| i).collect();
    let n = list.len() + idx.len();
    let mut adj = vec![Vec::new(); n];
    let link = |adj: &mut Vec<Vec<usize>>, a: usize, b: usize| {
        adj[a].push(b);
        adj[b].push(a);
    };
    for (k, &pi) in idx.iter().enumerate() {
        let node = list.len() + k;
        let ends: Vec<Vec2> = ps[pi].segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
        for (ii, it) in list.iter().enumerate() {
            if ends.iter().any(|p| dist_to_bounds(&it.bounds, *p) <= snap) {
                link(&mut adj, ii, node);
            }
        }
        for (k2, &pj) in idx.iter().enumerate().skip(k + 1) {
            let ends2: Vec<Vec2> = ps[pj].segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
            let touches = |ends: &[Vec2], other: &Pipe| ends.iter().any(|p| other.segs.iter().any(|(a, b)| seg_point_dist(*a, *b, *p) <= snap));
            if touches(&ends, &ps[pj]) || touches(&ends2, &ps[pi]) {
                link(&mut adj, node, list.len() + k2);
            }
        }
    }
    Net { adj, idx }
}

/// The fixtures and the pipes reached from source `start` without passing through a fixture or another source.
fn reach(net: &Net, list: &[Item], start: usize) -> (BTreeSet<usize>, BTreeSet<usize>) {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut fixtures = BTreeSet::new();
    let mut runs = BTreeSet::new();
    let mut stack = vec![start];
    seen.insert(start);
    while let Some(n) = stack.pop() {
        for &m in &net.adj[n] {
            if !seen.insert(m) {
                continue;
            }
            if m < list.len() {
                // An item at the end of a run: a fixture is served, another source ends the run.
                if !kind_of(list[m].code).is_some_and(|k| k.source) {
                    fixtures.insert(m);
                }
                continue;
            }
            runs.insert(net.idx[m - list.len()]);
            stack.push(m);
        }
    }
    (fixtures, runs)
}

/// What one service found: for each source, the fixtures it serves and the pipes it feeds.
struct ServiceResult {
    service: usize,
    sources: Vec<(usize, Vec<usize>, Vec<usize>)>,
    unconnected: Vec<usize>,
    ambiguous: Vec<usize>,
}

struct Analysis {
    services: Vec<ServiceResult>,
    pipes: Vec<Pipe>,
}

fn analyse(s: &Session, o: &Map<String, Value>, snap: f64) -> Result<(Vec<Item>, Analysis)> {
    let space = s.space();
    let d = s.doc()?;
    let list = items(d, &space, o);
    let ps = pipes(d, &space, o);
    let mut services = Vec::new();
    for (si, svc) in SERVICES.iter().enumerate() {
        let net = build(&list, &ps, si, snap);
        let mut owner: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut sources = Vec::new();
        for (ti, _) in list.iter().enumerate().filter(|(_, it)| svc.sources.contains(&it.code)) {
            let (fx, runs) = reach(&net, &list, ti);
            for f in &fx {
                owner.entry(*f).or_default().push(ti);
            }
            sources.push((ti, fx.into_iter().collect::<Vec<_>>(), runs.into_iter().collect::<Vec<_>>()));
        }
        let unconnected = list.iter().enumerate().filter(|(i, it)| svc.needs.contains(&it.code) && !owner.contains_key(i)).map(|(i, _)| i).collect();
        let ambiguous = owner.iter().filter(|(_, t)| t.len() > 1).map(|(f, _)| *f).collect();
        services.push(ServiceResult { service: si, sources, unconnected, ambiguous });
    }
    Ok((list, Analysis { services, pipes: ps }))
}

fn snap_of(s: &Session, p: &Value) -> Result<f64> {
    let per_mm = ua::drawing_unit(s.doc()?).map_or(1.0, |u| 0.001 / u.metres());
    let snap = f64_or(p, "snap", 20.0 * per_mm);
    if snap.is_finite() && snap >= 0.0 { Ok(snap) } else { Err(bad("plumbing.connect", "`snap` must be zero or positive")) }
}

fn id_lookup<'a>(o: &'a Map<String, Value>, list: &'a [Item]) -> impl Fn(usize) -> String + 'a {
    let ids = labels_of(o);
    move |i: usize| ids.get(&list[i].handle.hex()).map_or_else(|| list[i].handle.hex(), |(id, _)| id.clone())
}

fn count_by_code(list: &[Item], idx: &[usize]) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for i in idx {
        *m.entry(list[*i].code).or_insert(0) += 1;
    }
    m
}

fn connect(s: &mut Session, o: &Map<String, Value>, snap: f64, flag: bool) -> Result<Value> {
    let (list, a) = analyse(s, o, snap)?;
    let id_of = id_lookup(o, &list);
    // Replace earlier flags.
    let old: Vec<Handle> = s.doc()?.model.iter().filter(|e| e.common.layer == L_FLAGS).map(|e| e.handle).collect();
    for h in old {
        s.doc_mut()?.remove_entity(h);
    }
    if flag {
        ensure_layer(s.doc_mut()?, L_FLAGS, 1, "Continuous", 25, "Plumbing piping flags");
        let th = text_height(s.doc()?);
        let mut seen: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for r in &a.services {
            let svc = &SERVICES[r.service];
            for i in &r.unconnected {
                seen.entry(*i).or_default().push(format!("NO {}", if svc.code == "SUP" { "SUPPLY" } else { "DRAIN" }));
            }
            for i in &r.ambiguous {
                seen.entry(*i).or_default().push(format!("ON 2 {} SOURCES", svc.code));
            }
        }
        for (i, whys) in seen {
            let it = &list[i];
            let kind = make_text(s, Vec2::new(it.bounds.min.x, it.bounds.min.y - th), &whys.join(", "), th, 0.0, (HAlign::Left, VAlign::Middle));
            add_on(s, L_FLAGS, kind)?;
        }
    }
    let mut services = Vec::new();
    let mut stacks = Vec::new();
    let mut served: BTreeMap<usize, BTreeMap<&str, String>> = BTreeMap::new();
    for r in &a.services {
        let svc = &SERVICES[r.service];
        let amb: BTreeSet<usize> = r.ambiguous.iter().copied().collect();
        let mut sources = Vec::new();
        for (ti, fx, runs) in &r.sources {
            let counts = count_by_code(&list, fx);
            let pipe_m: f64 = runs.iter().map(|k| a.pipes[*k].length).sum();
            sources
                .push(json!({ "id": id_of(*ti), "fixtures": fx.iter().map(|f| id_of(*f)).collect::<Vec<_>>(), "counts": counts, "pipe_m": pipe_m }));
            let own: Vec<usize> = fx.iter().copied().filter(|f| !amb.contains(f)).collect();
            let c = count_by_code(&list, &own);
            let at = list[*ti].handle.hex();
            stacks.push(json!({
                "ref": format!("PS{}-{}", at, svc.code),
                "handle": at,
                "source": id_of(*ti),
                "kind": list[*ti].code,
                "service": svc.code,
                "european_wc": c.get("WC").copied().unwrap_or(0),
                "indian_wc": c.get("IWC").copied().unwrap_or(0),
                "basin": c.get("WB").copied().unwrap_or(0),
                "sink": c.get("KS").copied().unwrap_or(0),
                "shower": c.get("SH").copied().unwrap_or(0),
                "floor_trap": c.get("FT").copied().unwrap_or(0),
                "pipe_m": pipe_m,
            }));
            for f in &own {
                served.entry(*f).or_default().insert(svc.code, id_of(*ti));
            }
        }
        services.push(json!({
            "service": svc.code,
            "title": svc.title,
            "sources": sources,
            "unconnected": r.unconnected.iter().map(|i| id_of(*i)).collect::<Vec<_>>(),
            "ambiguous": r.ambiguous.iter().map(|i| id_of(*i)).collect::<Vec<_>>(),
        }));
    }
    // A source that serves nothing in any service it belongs to.
    let empty: Vec<String> = list
        .iter()
        .enumerate()
        .filter(|(i, it)| {
            kind_of(it.code).is_some_and(|k| k.source)
                && a.services
                    .iter()
                    .filter(|r| SERVICES[r.service].sources.contains(&it.code))
                    .all(|r| r.sources.iter().any(|(t, fx, _)| t == i && fx.is_empty()))
        })
        .map(|(i, _)| id_of(i))
        .collect();
    // One row per fixture: where its supply comes from and where its drain goes.
    let mut fixtures = Vec::new();
    for (i, it) in list.iter().enumerate().filter(|(_, it)| kind_of(it.code).is_some_and(|k| !k.source)) {
        let h = it.handle.hex();
        let from = |code: &str| served.get(&i).and_then(|m| m.get(code)).cloned().unwrap_or_default();
        fixtures.push(json!({
            "ref": format!("PF{h}"),
            "handle": h,
            "fixture": id_of(i),
            "kind": it.code,
            "supply": from("SUP"),
            "drain": from("DRN"),
        }));
    }
    Ok(json!({ "services": services, "emptySources": empty, "stacks": stacks, "fixtures": fixtures }))
}

fn run_connect(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    // Piping needs IDs to name things; give any block without one an ID first.
    number(s, &mut o, &all_codes(), false)?;
    save(s.doc_mut()?, &o);
    let snap = snap_of(s, p)?;
    let result = connect(s, &o, snap, bool_or(p, "flag", true))?;
    let mut o = state(s.doc()?);
    for key in ["stacks", "fixtures"] {
        if let Some(rows) = result.get(key) {
            o.insert(key.into(), rows.clone());
        }
    }
    save(s.doc_mut()?, &o);
    Ok(result)
}

fn schedule_rows(s: &Session, o: &Map<String, Value>, mode: &str, snap: f64) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let (list, a) = analyse(s, o, snap)?;
    let id_of = id_lookup(o, &list);
    let strs = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
    if mode == "pipes" {
        // Length by service and diameter, in metres.
        let mut by: BTreeMap<(usize, u64), f64> = BTreeMap::new();
        for p in &a.pipes {
            *by.entry((p.service, p.dia.to_bits())).or_insert(0.0) += p.length;
        }
        let mut rows: Vec<Vec<String>> = by
            .iter()
            .map(|((si, dia), len)| vec![SERVICES[*si].title.to_string(), format!("{}", f64::from_bits(*dia)), format!("{len:.2}")])
            .collect();
        if !rows.is_empty() {
            rows.push(vec!["TOTAL".into(), String::new(), format!("{:.2}", a.pipes.iter().map(|p| p.length).sum::<f64>())]);
        }
        return Ok((strs(&["Service", "Dia (mm)", "Length (m)"]), rows));
    }
    let fixture_idx: Vec<usize> = list.iter().enumerate().filter(|(_, it)| kind_of(it.code).is_some_and(|k| !k.source)).map(|(i, _)| i).collect();
    if mode == "fixture" {
        let mut rows = Vec::new();
        for i in &fixture_idx {
            let cell = |si: usize| -> String {
                let svc = &SERVICES[si];
                let r = &a.services[si];
                if !svc.needs.contains(&list[*i].code) {
                    return "-".into();
                }
                if r.ambiguous.contains(i) {
                    return "2 SOURCES".into();
                }
                r.sources.iter().find(|(_, fx, _)| fx.contains(i)).map_or_else(|| "NOT CONNECTED".to_string(), |(t, _, _)| id_of(*t))
            };
            rows.push(vec![id_of(*i), kind_of(list[*i].code).map_or("", |k| k.desc).to_string(), cell(0), cell(1)]);
        }
        return Ok((strs(&["Fixture", "Kind", "Supply from", "Drain to"]), rows));
    }
    // matrix: a row per source and service, a column per kind of fixture placed.
    let used: Vec<&str> = KINDS.iter().filter(|k| !k.source && fixture_idx.iter().any(|i| list[*i].code == k.code)).map(|k| k.code).collect();
    let mut header = vec!["Source".to_string(), "Service".to_string()];
    header.extend(used.iter().map(|c| (*c).to_string()));
    let mut rows = Vec::new();
    for r in &a.services {
        let svc = &SERVICES[r.service];
        let amb: BTreeSet<usize> = r.ambiguous.iter().copied().collect();
        for (ti, fx, _) in &r.sources {
            let own: Vec<usize> = fx.iter().copied().filter(|f| !amb.contains(f)).collect();
            let c = count_by_code(&list, &own);
            let mut row = vec![id_of(*ti), svc.code.to_string()];
            row.extend(used.iter().map(|code| c.get(code).copied().unwrap_or(0).to_string()));
            rows.push(row);
        }
        for (label, set) in [("ON 2 SOURCES", &r.ambiguous), ("NOT CONNECTED", &r.unconnected)] {
            if !set.is_empty() {
                let c = count_by_code(&list, set);
                let mut row = vec![label.to_string(), svc.code.to_string()];
                row.extend(used.iter().map(|code| c.get(code).copied().unwrap_or(0).to_string()));
                rows.push(row);
            }
        }
    }
    if !rows.is_empty() || !fixture_idx.is_empty() {
        let c = count_by_code(&list, &fixture_idx);
        let mut t = vec!["TOTAL".to_string(), String::new()];
        t.extend(used.iter().map(|code| c.get(code).copied().unwrap_or(0).to_string()));
        rows.push(t);
    }
    Ok((header, rows))
}

fn draw_schedule(s: &mut Session, o: &Map<String, Value>, mode: &str, at: Option<Vec2>, snap: f64) -> Result<usize> {
    let (header, rows) = schedule_rows(s, o, mode, snap)?;
    if rows.is_empty() {
        return Ok(0);
    }
    let old: Vec<Handle> = s.doc()?.model.iter().filter(|e| e.common.layer == L_TABLE).map(|e| e.handle).collect();
    for h in old {
        s.doc_mut()?.remove_entity(h);
    }
    ensure_layer(s.doc_mut()?, L_TABLE, 7, "Continuous", 18, "Plumbing schedules");
    let th = text_height(s.doc()?);
    let ext = s.doc()?.extents(&Space::Model);
    let origin = at.unwrap_or(if ext.is_empty() { Vec2::ZERO } else { Vec2::new(ext.max.x + 4.0 * th, ext.max.y) });
    // The shared table drawer puts its objects on the take-off table layer; move them here.
    let before: BTreeSet<u64> = s.doc()?.model.iter().map(|e| e.handle.0).collect();
    let title = match mode {
        "fixture" => "PLUMBING FIXTURES",
        "pipes" => "PLUMBING PIPES",
        _ => "PLUMBING SCHEDULE",
    };
    table(s, &table_ctx(th), origin, title, &header, &rows)?;
    let new: Vec<Handle> = s.doc()?.model.iter().filter(|e| !before.contains(&e.handle.0)).map(|e| e.handle).collect();
    for h in new {
        s.doc_mut()?.modify_entity(h, |e| e.common.layer = L_TABLE.to_string())?;
    }
    Ok(rows.len())
}

fn run_schedule(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("matrix");
    if !["matrix", "fixture", "pipes"].contains(&mode) {
        return Err(bad("plumbing.schedule", "`mode` must be matrix, fixture or pipes"));
    }
    let mut o = state(s.doc()?);
    number(s, &mut o, &all_codes(), false)?;
    let snap = snap_of(s, p)?;
    let (header, rows) = schedule_rows(s, &o, mode, snap)?;
    let drawn = draw_schedule(s, &o, mode, point_param(p, "at"), snap)?;
    o.insert("schedule".into(), json!({ "mode": mode, "at": point_param(p, "at").map(|v| [v.x, v.y]) }));
    save(s.doc_mut()?, &o);
    Ok(json!({ "header": header, "rows": rows, "drawn": drawn }))
}

fn run_update(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    let before = labels_of(&o).len();
    let numbered = number(s, &mut o, &all_codes(), false)?;
    let snap = snap_of(s, p)?;
    let connected = connect(s, &o, snap, true)?;
    for key in ["stacks", "fixtures"] {
        if let Some(rows) = connected.get(key) {
            o.insert(key.into(), rows.clone());
        }
    }
    // Redraw the schedule where it was drawn before.
    if let Some(sc) = o.get("schedule").cloned() {
        let mode = sc["mode"].as_str().unwrap_or("matrix").to_string();
        let at = sc["at"].as_array().and_then(|a| Some(Vec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
        draw_schedule(s, &o, &mode, at, snap)?;
    }
    save(s.doc_mut()?, &o);
    Ok(json!({ "numbered": numbered, "labelsBefore": before, "connected": connected }))
}

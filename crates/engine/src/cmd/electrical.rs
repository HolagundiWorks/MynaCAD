//! Electrical layout (the kit's ELBLOCKS, ELNUM, SBNUM, LPNUM, FPNUM, ELLAYERS, ELCONNECT,
//! ELSCHEDULE and ELUPDATE).
//!
//! * `electrical.blocks` says which blocks are switchboards, light points, fan points, switches,
//!   sockets and appliances (names may use `*`); the choice is kept in the drawing.
//! * `electrical.number` gives each block an ID (`SB-01`, `LP-02`) and writes it beside the block.
//! * `electrical.layers` names the layers the wiring is drawn on (lighting, power).
//! * `electrical.connect` works out from the wiring which points reach which board and flags what
//!   is not wired. A wire reaches a block when one of its vertices lies within about 20 mm of the
//!   block's extents; wires that meet end to end or in a T are one run. A board is a terminal; a
//!   point, switch or appliance is a junction (a run from one light to the next carries on).
//! * `electrical.schedule` draws the schedule: a row per board and a column per kind of point.
//!
//! Not implemented: circuit sizing, connected-load and demand schedules, cable lengths, and room
//! grouping, which the kit also has.

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

/// Kind code, description, group (LT lighting, PW power).
pub const KINDS: &[(&str, &str, &str)] = &[
    ("SB", "Switchboard", "BD"),
    ("LP", "Light point", "LT"),
    ("FP", "Fan point", "LT"),
    ("EF", "Exhaust fan", "LT"),
    ("SW1", "One way switch", "LT"),
    ("SW2", "Two way switch", "LT"),
    ("CB", "Calling bell", "LT"),
    ("P5", "5 amp socket", "PW"),
    ("P15", "15 amp socket", "PW"),
    ("AC", "Air conditioner", "PW"),
    ("WP", "Water purifier", "PW"),
    ("GY", "Geyser", "PW"),
    ("FR", "Fridge", "PW"),
    ("OV", "Oven", "PW"),
    ("WF", "WiFi router", "PW"),
    ("TV", "TV", "PW"),
    ("INV", "Inverter", "PW"),
];

const L_LABELS: &str = "EL-LABELS";
const L_FLAGS: &str = "EL-FLAGS";
const L_TABLE: &str = "EL-TABLE";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.blocks", "Map Electrical Blocks", run_blocks)
            .menu(&["Architecture", "Electrical", "Map Blocks"])
            .alias(&["elblocks"])
            .params("{blocks?: {SB: [\"SB*\"], LP: [...], ...}} → {blocks, found: {code: count}} (codes: SB LP FP EF SW1 SW2 CB P5 P15 AC WP GY FR OV WF TV INV)"),
        CommandSpec::new("electrical.number", "Number Electrical Blocks", run_number)
            .menu(&["Architecture", "Electrical", "Number Blocks"])
            .alias(&["elnum", "sbnum", "lpnum", "fpnum"])
            .params("{codes?: [\"SB\", \"LP\", ...] (default all), renumber?: bool (default false: IDs already given are kept)} → {ids}"),
        CommandSpec::new("electrical.layers", "Wiring Layers", run_layers)
            .menu(&["Architecture", "Electrical", "Wiring Layers"])
            .alias(&["ellayers"])
            .params("{lighting?: [layer], power?: [layer]} → {lighting, power}"),
        CommandSpec::new("electrical.connect", "Check Wiring", run_connect)
            .menu(&["Architecture", "Electrical", "Check Wiring"])
            .alias(&["elconnect"])
            .params("{snap?: drawing units (default 20 mm), flag?: bool (default true)} → {boards: [{id, points, counts}], unwired, ambiguous, emptyBoards}"),
        CommandSpec::new("electrical.schedule", "Electrical Schedule", run_schedule)
            .menu(&["Architecture", "Electrical", "Schedule"])
            .alias(&["elschedule"])
            .params("{mode?: \"matrix\" (default) | \"point\", at?: [x,y]} → {rows}"),
        CommandSpec::new("electrical.update", "Update Electrical", run_update)
            .menu(&["Architecture", "Electrical", "Update"])
            .alias(&["elupdate"])
            .params("{} → {numbered, connected} (numbers new blocks, moves labels, re-checks the wiring)"),
        CommandSpec::new("electrical.autonumber", "Auto-number Electrical Blocks", run_autonumber)
            .menu(&["Architecture", "Electrical", "Auto-number on Insert"])
            .params("{enabled?: bool} → {enabled}. When on (the default), inserting a mapped block numbers it."),
    ]
}

/// Number a newly inserted block when it matches a switchboard or fixture map. Default on.
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
    number(s, &mut o, &KINDS.iter().map(|(c, _, _)| *c).collect::<Vec<_>>(), false)?;
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
    match project::get(d, "electrical") {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

fn save(d: &mut Drawing, o: &Map<String, Value>) {
    project::set(d, "electrical", &Value::Object(o.clone()));
}

/// Block name patterns of a kind: the saved choice, else the kind code and `code*`.
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

/// The kind of an electrical toolbar symbol, by its smart block id (see [`smart_key`]). The socket's amperage
/// is a parameter the block name does not carry, so a socket counts as a 5 amp socket here; map blocks
/// by name with `electrical.blocks` to count 15 amp sockets.
fn smart_code(key: &str) -> Option<&'static str> {
    Some(match key {
        "ELEC_SWITCHBOARD" | "ELEC_DB" => "SB",
        "ELEC_LIGHT" | "ELEC_DOWNLIGHT" => "LP",
        "ELEC_FAN_CEILING" => "FP",
        "ELEC_SOCKET" => "P5",
        _ => return None,
    })
}

fn items(d: &Drawing, space: &Space, o: &Map<String, Value>) -> Vec<Item> {
    let mut out = Vec::new();
    let pats: Vec<(&'static str, Vec<String>)> = KINDS.iter().map(|(c, _, _)| (*c, patterns(o, c))).collect();
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| d.is_visible(e)) {
            let EntityKind::Insert(i) = &e.kind else { continue };
            // A symbol from the toolbar is recognised by its smart block id; the name patterns below must not
            // see it (every smart block is named `SB_...`, which the switchboard pattern would take).
            let code = match smart_key(&i.block) {
                Some(key) => smart_code(key),
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
            if !KINDS.iter().any(|(c, _, _)| c == code) {
                return Err(bad("electrical.blocks", format!("unknown kind `{code}`")));
            }
            let names: Vec<Value> = v
                .as_array()
                .ok_or_else(|| bad("electrical.blocks", format!("`{code}` must be a list of block names")))?
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
                    return Err(bad("electrical.blocks", format!("block `{n}` is chosen for both {prev} and {code}")));
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
    let blocks: Map<String, Value> = KINDS.iter().map(|(c, _, _)| ((*c).to_string(), json!(patterns(&o, c)))).collect();
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
    ensure_layer(s.doc_mut()?, L_LABELS, 3, "Continuous", 18, "Electrical block IDs");
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
                        .find(|(k, _, _)| k.eq_ignore_ascii_case(c))
                        .map(|(k, _, _)| *k)
                        .ok_or_else(|| bad("electrical.number", format!("unknown kind `{c}`")))?,
                );
            }
            v
        }
        None => KINDS.iter().map(|(c, _, _)| *c).collect(),
    };
    let n = number(s, &mut o, &codes, bool_or(p, "renumber", false))?;
    save(s.doc_mut()?, &o);
    let ids: BTreeMap<String, String> = labels_of(&o).into_iter().map(|(h, (id, _))| (h, id)).collect();
    Ok(json!({ "numbered": n, "ids": ids }))
}

fn wiring_layers(o: &Map<String, Value>) -> (Vec<String>, Vec<String>) {
    let get = |k: &str| {
        o.get("layers")
            .and_then(|l| l.get(k))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["E-WIRE".to_string()])
    };
    (get("lighting"), get("power"))
}

fn run_layers(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    let mut layers = o.get("layers").and_then(Value::as_object).cloned().unwrap_or_default();
    for (key, name) in [("lighting", "lighting"), ("power", "power")] {
        if let Some(Value::Array(a)) = p.get(name) {
            let v: Vec<Value> = a.iter().filter(|n| n.as_str().is_some_and(|t| !t.is_empty() && t.len() <= 80)).cloned().collect();
            if v.is_empty() {
                return Err(bad("electrical.layers", format!("`{name}` needs at least one layer name")));
            }
            layers.insert(key.into(), Value::Array(v));
        }
    }
    o.insert("layers".into(), Value::Object(layers));
    save(s.doc_mut()?, &o);
    let (l, w) = wiring_layers(&o);
    Ok(json!({ "lighting": l, "power": w }))
}

// ---------------------------------------------------------------------------------------------
// Connectivity

struct Wire {
    segs: Vec<(Vec2, Vec2)>,
    group: &'static str,
}

fn wires(d: &Drawing, space: &Space, o: &Map<String, Value>) -> Vec<Wire> {
    let (lt, pw) = wiring_layers(o);
    let mut out = Vec::new();
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| d.is_visible(e)) {
            let layer = e.common.layer.to_ascii_lowercase();
            let group = if lt.iter().any(|l| l.to_ascii_lowercase() == layer) {
                "LT"
            } else if pw.iter().any(|l| l.to_ascii_lowercase() == layer) {
                "PW"
            } else {
                continue;
            };
            let segs: Vec<(Vec2, Vec2)> = segments_of(&e.kind).iter().map(|sg| (sg.start(), sg.end())).collect();
            if !segs.is_empty() {
                out.push(Wire { segs, group });
            }
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

struct Net {
    /// Node ids: items first, then wires.
    adj: Vec<Vec<usize>>,
}

fn build(list: &[Item], ws: &[Wire], snap: f64) -> Net {
    let n = list.len() + ws.len();
    let mut adj = vec![Vec::new(); n];
    let link = |adj: &mut Vec<Vec<usize>>, a: usize, b: usize| {
        adj[a].push(b);
        adj[b].push(a);
    };
    for (wi, w) in ws.iter().enumerate() {
        let node = list.len() + wi;
        let ends: Vec<Vec2> = w.segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
        for (ii, it) in list.iter().enumerate() {
            if ends.iter().any(|p| dist_to_bounds(&it.bounds, *p) <= snap) {
                link(&mut adj, ii, node);
            }
        }
        for (wj, w2) in ws.iter().enumerate().skip(wi + 1) {
            if w.group != w2.group {
                continue;
            }
            let ends2: Vec<Vec2> = w2.segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
            let touches = |ends: &[Vec2], other: &Wire| ends.iter().any(|p| other.segs.iter().any(|(a, b)| seg_point_dist(*a, *b, *p) <= snap));
            if touches(&ends, w2) || touches(&ends2, w) {
                link(&mut adj, node, list.len() + wj);
            }
        }
    }
    Net { adj }
}

/// The points reached from board `start` without passing through another board.
fn reach(net: &Net, list: &[Item], start: usize) -> BTreeSet<usize> {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut stack = vec![start];
    seen.insert(start);
    while let Some(n) = stack.pop() {
        for &m in &net.adj[n] {
            if seen.contains(&m) {
                continue;
            }
            if m < list.len() && list[m].code == "SB" {
                continue; // a board is a terminal
            }
            seen.insert(m);
            stack.push(m);
        }
    }
    seen.into_iter().filter(|i| *i < list.len() && *i != start).collect()
}

struct Analysis {
    boards: Vec<(String, Vec<usize>)>,
    unwired: Vec<usize>,
    ambiguous: Vec<usize>,
    empty: Vec<String>,
}

fn analyse(s: &Session, o: &Map<String, Value>, snap: f64) -> Result<(Vec<Item>, Analysis)> {
    let space = s.space();
    let d = s.doc()?;
    let list = items(d, &space, o);
    let ws = wires(d, &space, o);
    let net = build(&list, &ws, snap);
    let ids = labels_of(o);
    let id_of = |i: usize| ids.get(&list[i].handle.hex()).map_or_else(|| list[i].handle.hex(), |(id, _)| id.clone());
    let mut owner: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut boards = Vec::new();
    let mut empty = Vec::new();
    for (bi, _) in list.iter().enumerate().filter(|(_, i)| i.code == "SB") {
        let pts = reach(&net, &list, bi);
        for p in &pts {
            owner.entry(*p).or_default().push(bi);
        }
        if pts.is_empty() {
            empty.push(id_of(bi));
        }
        boards.push((id_of(bi), pts.into_iter().collect::<Vec<_>>()));
    }
    let unwired: Vec<usize> = list.iter().enumerate().filter(|(i, it)| it.code != "SB" && !owner.contains_key(i)).map(|(i, _)| i).collect();
    let ambiguous: Vec<usize> = owner.iter().filter(|(_, b)| b.len() > 1).map(|(p, _)| *p).collect();
    Ok((list, Analysis { boards, unwired, ambiguous, empty }))
}

fn connect(s: &mut Session, o: &Map<String, Value>, snap: f64, flag: bool) -> Result<Value> {
    let (list, a) = analyse(s, o, snap)?;
    let ids = labels_of(o);
    let id_of = |i: usize| ids.get(&list[i].handle.hex()).map_or_else(|| list[i].handle.hex(), |(id, _)| id.clone());
    // Replace earlier flags.
    let old: Vec<Handle> = s.doc()?.model.iter().filter(|e| e.common.layer == L_FLAGS).map(|e| e.handle).collect();
    for h in old {
        s.doc_mut()?.remove_entity(h);
    }
    if flag {
        ensure_layer(s.doc_mut()?, L_FLAGS, 1, "Continuous", 25, "Electrical wiring flags");
        let th = text_height(s.doc()?);
        for (i, why) in a.unwired.iter().map(|i| (*i, "NOT WIRED")).chain(a.ambiguous.iter().map(|i| (*i, "REACHES 2 BOARDS"))) {
            let it = &list[i];
            let kind = make_text(s, Vec2::new(it.bounds.min.x, it.bounds.min.y - th), why, th, 0.0, (HAlign::Left, VAlign::Middle));
            add_on(s, L_FLAGS, kind)?;
        }
    }
    let boards: Vec<Value> = a
        .boards
        .iter()
        .map(|(id, pts)| {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for p in pts {
                *counts.entry(list[*p].code).or_insert(0) += 1;
            }
            json!({ "id": id, "points": pts.iter().map(|p| id_of(*p)).collect::<Vec<_>>(), "counts": counts })
        })
        .collect();
    let amb: BTreeSet<usize> = a.ambiguous.iter().copied().collect();
    let fixtures = ["LP", "FP", "EF", "CB", "AC", "WP", "GY", "FR", "OV", "WF", "TV", "INV"];
    let mut wires = Vec::new();
    let mut circuits = Vec::new();
    for (board, pts) in &a.boards {
        let count = |code: &str| pts.iter().filter(|p| !amb.contains(*p) && list[**p].code == code).count();
        let fixture: usize = fixtures.iter().map(|c| count(c)).sum();
        let board_h = list.iter().enumerate().find(|(i, it)| it.code == "SB" && id_of(*i) == *board).map(|(_, it)| it.handle.hex());
        wires.push(json!({
            "ref": format!("EB{}", board_h.clone().unwrap_or_else(|| board.clone())),
            "handle": board_h,
            "switchboard": board,
            "height": Value::Null,
            "5amp_socket": count("P5"),
            "15amp_socket": count("P15"),
            "connected_fixture": fixture,
            "two_way_switches": count("SW2"),
            "one_way_switches": count("SW1"),
        }));
        for p in pts {
            if amb.contains(p) {
                continue;
            }
            let code = list[*p].code;
            circuits.push(json!({
                "ref": format!("EC{}", list[*p].handle.hex()),
                "handle": list[*p].handle.hex(),
                "switchboard": board,
                "point": id_of(*p),
                "kind": code,
                "height": Value::Null,
                "5amp_socket": u8::from(code == "P5"),
                "15amp_socket": u8::from(code == "P15"),
                // Count column (0 or 1), same shape as the board wire table — not the kind string.
                "connected_fixture": u8::from(fixtures.contains(&code)),
                "two_way_switches": u8::from(code == "SW2"),
                "one_way_switches": u8::from(code == "SW1"),
            }));
        }
    }
    Ok(json!({
        "boards": boards,
        "wires": wires,
        "circuits": circuits,
        "unwired": a.unwired.iter().map(|i| id_of(*i)).collect::<Vec<_>>(),
        "ambiguous": a.ambiguous.iter().map(|i| id_of(*i)).collect::<Vec<_>>(),
        "emptyBoards": a.empty,
    }))
}

fn snap_of(s: &Session, p: &Value) -> Result<f64> {
    let per_mm = ua::drawing_unit(s.doc()?).map_or(1.0, |u| 0.001 / u.metres());
    let snap = f64_or(p, "snap", 20.0 * per_mm);
    if snap.is_finite() && snap >= 0.0 { Ok(snap) } else { Err(bad("electrical.connect", "`snap` must be zero or positive")) }
}

fn run_connect(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = state(s.doc()?);
    let space = s.space();
    let list = items(s.doc()?, &space, &o);
    // Wiring needs IDs to name things; give any block without one an ID first.
    number(s, &mut o, &KINDS.iter().map(|(c, _, _)| *c).collect::<Vec<_>>(), false)?;
    let _ = list;
    save(s.doc_mut()?, &o);
    let snap = snap_of(s, p)?;
    let result = connect(s, &o, snap, bool_or(p, "flag", true))?;
    let mut o = state(s.doc()?);
    if let Some(wires) = result.get("wires") {
        o.insert("wires".into(), wires.clone());
    }
    if let Some(circuits) = result.get("circuits") {
        o.insert("circuits".into(), circuits.clone());
    }
    save(s.doc_mut()?, &o);
    Ok(result)
}

fn schedule_rows(s: &Session, o: &Map<String, Value>, mode: &str, snap: f64) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let (list, a) = analyse(s, o, snap)?;
    let ids = labels_of(o);
    let id_of = |i: usize| ids.get(&list[i].handle.hex()).map_or_else(|| list[i].handle.hex(), |(id, _)| id.clone());
    if mode == "point" {
        let mut rows = Vec::new();
        for (board, pts) in &a.boards {
            for p in pts {
                rows.push(vec![id_of(*p), list[*p].code.to_string(), board.clone()]);
            }
        }
        for i in &a.unwired {
            rows.push(vec![id_of(*i), list[*i].code.to_string(), "NOT WIRED".into()]);
        }
        return Ok((["Point", "Kind", "Board"].iter().map(|x| (*x).to_string()).collect(), rows));
    }
    let amb: BTreeSet<usize> = a.ambiguous.iter().copied().collect();
    let used: Vec<&str> = KINDS.iter().map(|(c, _, _)| *c).filter(|c| *c != "SB" && list.iter().any(|i| i.code == *c)).collect();
    let mut header = vec!["SB no".to_string()];
    header.extend(used.iter().map(|c| (*c).to_string()));
    let mut rows = Vec::new();
    let mut totals = vec![0usize; used.len()];
    for (board, pts) in &a.boards {
        let mut row = vec![board.clone()];
        for (ci, c) in used.iter().enumerate() {
            let n = pts.iter().filter(|p| !amb.contains(*p) && list[**p].code == *c).count();
            totals[ci] += n;
            row.push(n.to_string());
        }
        rows.push(row);
    }
    if !amb.is_empty() {
        let mut row = vec!["REACHES 2 BOARDS".to_string()];
        for (ci, c) in used.iter().enumerate() {
            let n = amb.iter().filter(|p| list[**p].code == *c).count();
            totals[ci] += n;
            row.push(n.to_string());
        }
        rows.push(row);
    }
    if !a.unwired.is_empty() {
        let mut row = vec!["NOT WIRED".to_string()];
        for c in &used {
            row.push(a.unwired.iter().filter(|p| list[**p].code == *c).count().to_string());
        }
        rows.push(row);
    }
    if !rows.is_empty() {
        let mut t = vec!["TOTAL".to_string()];
        t.extend(totals.iter().map(usize::to_string));
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
    ensure_layer(s.doc_mut()?, L_TABLE, 7, "Continuous", 18, "Electrical schedules");
    let th = text_height(s.doc()?);
    let ext = s.doc()?.extents(&Space::Model);
    let origin = at.unwrap_or(if ext.is_empty() { Vec2::ZERO } else { Vec2::new(ext.max.x + 4.0 * th, ext.max.y) });
    // The shared table drawer puts its objects on the take-off table layer; move them here.
    let before: BTreeSet<u64> = s.doc()?.model.iter().map(|e| e.handle.0).collect();
    table(s, &table_ctx(th), origin, if mode == "point" { "ELECTRICAL POINTS" } else { "ELECTRICAL SCHEDULE" }, &header, &rows)?;
    let new: Vec<Handle> = s.doc()?.model.iter().filter(|e| !before.contains(&e.handle.0)).map(|e| e.handle).collect();
    for h in new {
        s.doc_mut()?.modify_entity(h, |e| e.common.layer = L_TABLE.to_string())?;
    }
    Ok(rows.len())
}

fn run_schedule(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("matrix");
    if !["matrix", "point"].contains(&mode) {
        return Err(bad("electrical.schedule", "`mode` must be matrix or point"));
    }
    let mut o = state(s.doc()?);
    number(s, &mut o, &KINDS.iter().map(|(c, _, _)| *c).collect::<Vec<_>>(), false)?;
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
    let numbered = number(s, &mut o, &KINDS.iter().map(|(c, _, _)| *c).collect::<Vec<_>>(), false)?;
    let snap = snap_of(s, p)?;
    let connected = connect(s, &o, snap, true)?;
    // Redraw the schedule where it was drawn before.
    if let Some(sc) = o.get("schedule").cloned() {
        let mode = sc["mode"].as_str().unwrap_or("matrix").to_string();
        let at = sc["at"].as_array().and_then(|a| Some(Vec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
        draw_schedule(s, &o, &mode, at, snap)?;
    }
    save(s.doc_mut()?, &o);
    Ok(json!({ "numbered": numbered, "labelsBefore": before, "connected": connected }))
}

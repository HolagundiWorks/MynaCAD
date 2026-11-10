//! Parametric graphs in the drawing (the `aad-param` crate does the evaluating).
//!
//! The graph is kept in the drawing's project data under `parametric`, with the links between
//! the components it produced and the drawing entities that show them, so it is saved with the
//! drawing and every stored edit is one step on the drawing's undo stack.
//!
//! * `param.graph` reads or replaces the graph; `param.wall.new` adds a wall (path, height,
//!   thickness, wall component, measurements) following a line or polyline or given points;
//!   `param.set` changes a node's value, or only previews the change.
//! * `param.evaluate` evaluates without changing anything and returns the staged geometry.
//! * `param.commit` writes the staged walls into the drawing as wall face lines on `A-WALL`. It
//!   refuses while the graph has an error, when the caller's graph or drawing revision is old, and
//!   when committed lines were edited by hand (unless `force`). Lines already linked to a wall are
//!   updated in place when their number is unchanged.
//! * `param.status` reports each wall as linked, stale or detached; `param.detach` keeps a wall's
//!   lines as plain drawing geometry; `param.deps` lists what a node depends on and drives.
//! * `param.takeoff` gives the measured facts of the committed walls (the MynaOps take-off
//!   contract's wall row plus provenance). `bridge.export` and `takeoff.record` include the
//!   rows of linked, current walls; finishes, net areas and costs stay with MynaOps.

use aad_doc::{Drawing, EntityKind, Handle};
use aad_geom::Vec2;
use aad_param::{self as pm, Graph};
use serde_json::{Map, Value, json};

use super::hcw::{add_on, ensure_layer};
use super::helpers::{line, v3};
use super::walls::{WALL, offset_path};
use super::*;
use crate::annot as ua;
use crate::{Result, Session};

const KEY: &str = "parametric";
/// Wall instances one graph may commit, and face lines per wall.
const MAX_WALLS: usize = 2_000;
const MAX_FACES: usize = 20_002;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("param.graph", "Parametric Graph", run_graph)
            .params("{graph?: graph JSON (replaces the stored graph), remove?: bool} → {graph, revision, diagnostics}")
            .menu(&["Architecture", "Parametric", "Graph"]),
        CommandSpec::new("param.library", "Parametric Library", run_library)
            .params("{} → {nodes, components}")
            .noundo(),
        CommandSpec::new("param.wall.new", "Parametric Wall", run_wall_new)
            .menu(&["Architecture", "Parametric", "New Wall"])
            .params("{handle? (line or polyline to follow) | points?: [[x,y],...] (drawing units), closed?, height?: metres or \"3 m\", thickness?: mm or \"230 mm\", id?} → {instanceId, nodes, revision}"),
        CommandSpec::new("param.set", "Set Parametric Value", run_set)
            .params("{node, value?, config?: {..} (merged), preview?: bool} → {revision?, evaluation}"),
        CommandSpec::new("param.evaluate", "Evaluate Parametric Graph", run_evaluate)
            .menu(&["Architecture", "Parametric", "Preview"])
            .params("{} → {evaluation, staged: [{instanceId, lines: [[x0,y0,x1,y1],...]}], sourcesChanged, revision, drawingRevision}")
            .noundo(),
        CommandSpec::new("param.commit", "Commit Parametric Graph", run_commit)
            .menu(&["Architecture", "Parametric", "Commit"])
            .params("{graphRevision?, drawingRevision?, force?: bool (replace hand-edited lines)} → {committed, created, updated, revision}"),
        CommandSpec::new("param.status", "Parametric Status", run_status)
            .menu(&["Architecture", "Parametric", "Status"])
            .params("{} → {revision, walls: [{instanceId, state: linked|stale|detached, reasons, handles}]}")
            .noundo(),
        CommandSpec::new("param.detach", "Detach Parametric Wall", run_detach)
            .params("{instance} → {detached} (its lines stay as plain geometry)"),
        CommandSpec::new("param.deps", "Parametric Dependencies", run_deps)
            .params("{node} → {upstream, downstream}")
            .noundo(),
        CommandSpec::new("param.takeoff", "Parametric Take-off", run_takeoff)
            .menu(&["Architecture", "Parametric", "Take-off Facts"])
            .params("{level?} → {walls: [{row, provenance}], warnings}")
            .noundo(),
    ]
}

// ---------------- storage ----------------

fn load_graph(d: &Drawing, cmd: &str) -> Result<Option<Graph>> {
    match project::get(d, KEY).get("graph") {
        None | Some(Value::Null) => Ok(None),
        Some(g) => Graph::from_json(g).map(Some).map_err(|e| bad(cmd, format!("the stored graph cannot be read: {e}"))),
    }
}

fn need_graph(d: &Drawing, cmd: &str) -> Result<Graph> {
    load_graph(d, cmd)?.ok_or_else(|| bad(cmd, "the drawing has no parametric graph: run `param.wall.new` or `param.graph`"))
}

fn load_links(d: &Drawing) -> Map<String, Value> {
    project::get(d, KEY).get("links").and_then(Value::as_object).cloned().unwrap_or_default()
}

fn store(d: &mut Drawing, g: Option<&Graph>, links: &Map<String, Value>) {
    let graph = g.map_or(Value::Null, Graph::to_json);
    project::set(d, KEY, &json!({ "graph": graph, "links": links }));
}

/// Metres in one drawing unit, and whether that was assumed (the drawing names no unit).
fn metres_per_unit(d: &Drawing) -> (f64, bool) {
    ua::drawing_unit(d).map_or((0.001, true), |u| (u.metres(), false))
}

fn context(d: &Drawing) -> pm::Context {
    let mut c = pm::Context::default();
    if let Some((_, name)) = project::active_floor(d) {
        for f in project::floors(d) {
            if let (Some(n), Some(h)) = (f["name"].as_str(), f["ceiling"].as_f64())
                && h.is_finite()
                && h > 0.0
            {
                c.level_heights.insert(n.to_string(), h);
            }
        }
        c.default_level = Some(name);
    }
    c
}

fn handle_of(v: &Value) -> Option<Handle> {
    v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))
}

fn handles_of(link: &Value) -> Vec<Handle> {
    link["handles"].as_array().map(|a| a.iter().filter_map(handle_of).collect()).unwrap_or_default()
}

/// A fingerprint of entities as they are now (a missing one counts too).
fn fingerprint(d: &Drawing, hs: &[Handle]) -> String {
    let mut text = String::new();
    for h in hs {
        match d.entity(*h) {
            Some(e) => text.push_str(&format!("{}|{}|{:?};", h.hex(), e.common.layer, e.kind)),
            None => text.push_str(&format!("{}|-;", h.hex())),
        }
    }
    format!("{:016x}", pm::fnv64(text.as_bytes()))
}

/// The centre line of a line or polyline, in drawing units.
fn entity_path(d: &Drawing, h: Handle) -> Option<(Vec<Vec2>, bool)> {
    match &d.entity(h)?.kind {
        EntityKind::Line(l) => Some((vec![l.a.xy(), l.b.xy()], false)),
        EntityKind::LwPolyline(pl) => {
            let mut v: Vec<Vec2> = pl.vertices.iter().map(|x| x.p).collect();
            v.dedup_by(|a, b| a.near(*b, 1e-9));
            (v.len() >= 2 && v.len() <= pm::MAX_PATH_POINTS && v.iter().all(|p| p.is_finite())).then_some((v, pl.closed))
        }
        _ => None,
    }
}

fn to_metres(pts: &[Vec2], m: f64) -> Vec<[f64; 2]> {
    pts.iter().map(|p| [p.x * m, p.y * m]).collect()
}

/// Bring path nodes that follow a drawing entity up to date with it. Returns the nodes that
/// changed, and the paths whose entity is gone or is no longer a line or polyline.
fn refresh_sources(d: &Drawing, g: &mut Graph) -> (Vec<String>, Vec<pm::Diagnostic>) {
    let m = g.units_context.drawing_unit_m;
    let (mut changed, mut problems) = (Vec::new(), Vec::new());
    for n in g.nodes.iter_mut().filter(|n| n.node_type == "geom.path") {
        let Some(h) = n.config.get("source").and_then(|s| s.get("handle")).and_then(handle_of) else { continue };
        match entity_path(d, h) {
            Some((pts, closed)) => {
                let fp = fingerprint(d, &[h]);
                if n.config["source"]["fingerprint"].as_str() != Some(fp.as_str()) {
                    n.config["points"] = json!(to_metres(&pts, m));
                    n.config["closed"] = json!(closed);
                    n.config["source"]["fingerprint"] = json!(fp);
                    changed.push(n.node_id.clone());
                }
            }
            None => problems.push(pm::Diagnostic::error(
                pm::Code::StaleRevision,
                Some(&n.node_id),
                None,
                format!("the line or polyline this path follows ({}) is no longer in the drawing", h.hex()),
            )),
        }
    }
    (changed, problems)
}

fn evaluate(s: &mut Session, g: &Graph) -> Result<pm::Evaluation> {
    let ctx = context(s.doc()?);
    Ok(pm::evaluate(g, &ctx, &mut s.param_cache, None))
}

fn first_error(ev: &pm::Evaluation, extra: &[pm::Diagnostic]) -> Option<String> {
    extra.iter().chain(ev.diagnostics.iter()).find(|d| d.is_error()).map(|d| match &d.node_id {
        Some(n) => format!("{n}: {}", d.message),
        None => d.message.clone(),
    })
}

/// The face lines of a wall in drawing units: both faces of every segment, and the end caps of
/// an open wall.
fn faces(w: &pm::WallValue, m: f64) -> Vec<(Vec2, Vec2)> {
    let pts: Vec<Vec2> = w.path.iter().map(|p| Vec2::new(p[0] / m, p[1] / m)).collect();
    let n = pts.len();
    let closed = w.closed && n > 2;
    let h = w.thickness / m / 2.0;
    let (left, right) = (offset_path(&pts, closed, h), offset_path(&pts, closed, -h));
    let mut out = Vec::new();
    let count = if closed { n } else { n.saturating_sub(1) };
    for i in 0..count {
        let j = (i + 1) % n.max(1);
        for face in [&left, &right] {
            if let (Some(a), Some(b)) = (face.get(i), face.get(j)) {
                out.push((*a, *b));
            }
        }
    }
    if !closed {
        for k in [0, n.saturating_sub(1)] {
            if let (Some(a), Some(b)) = (left.get(k), right.get(k)) {
                out.push((*a, *b));
            }
        }
    }
    out.retain(|(a, b)| a.is_finite() && b.is_finite());
    out
}

// ---------------- commands ----------------

fn run_library(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({ "nodes": pm::library(), "components": pm::component_types() }))
}

fn run_graph(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "param.graph";
    let old = load_graph(s.doc()?, CMD)?;
    if bool_or(p, "remove", false) {
        let mut links = load_links(s.doc()?);
        for l in links.values_mut() {
            l["state"] = json!("detached");
        }
        store(s.doc_mut()?, None, &links);
        s.param_cache.clear();
        return Ok(json!({ "graph": null, "removed": old.is_some() }));
    }
    let g = match p.get("graph") {
        None | Some(Value::Null) => match old {
            Some(g) => g,
            None => return Ok(json!({ "graph": null, "revision": 0, "diagnostics": [] })),
        },
        Some(v) => {
            let mut g = Graph::from_json(v).map_err(|e| bad(CMD, e.to_string()))?;
            g.graph_revision = old.as_ref().map_or(0, |o| o.graph_revision).saturating_add(1);
            let links = load_links(s.doc()?);
            store(s.doc_mut()?, Some(&g), &links);
            g
        }
    };
    let (_, diags) = pm::validate(&g);
    Ok(json!({ "graph": g.to_json(), "revision": g.graph_revision, "diagnostics": diags }))
}

/// A length parameter: a number in `unit`, or text with a unit such as `"3.2 m"`.
fn length_param(cmd: &str, p: &Value, key: &str, unit: &str, default: f64) -> Result<f64> {
    let to = |v: f64| pm::units::from_si(v, pm::Dim::LENGTH, unit);
    let v = match p.get(key) {
        None | Some(Value::Null) => Some(default),
        Some(Value::String(t)) => pm::units::parse_quantity(t).filter(|(_, d)| *d == pm::Dim::LENGTH).and_then(|(v, _)| to(v)),
        Some(v) => v.as_f64(),
    };
    v.filter(|x| x.is_finite() && *x > 0.0)
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a positive length (a number in {unit}, or text such as \"3 m\")")))
}

fn run_wall_new(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "param.wall.new";
    let d = s.doc()?;
    let (m, assumed) = metres_per_unit(d);
    let (pts, closed, source) = match p.get("handle") {
        Some(v) if !v.is_null() => {
            let h = handle_of(v).ok_or_else(|| bad(CMD, "`handle` is not an object handle"))?;
            let (pts, closed) = entity_path(d, h).ok_or_else(|| bad(CMD, "`handle` must be a line or a polyline of two or more points"))?;
            (pts, closed, Some(json!({ "handle": h.hex(), "fingerprint": fingerprint(d, &[h]) })))
        }
        _ => {
            let pts = points_param(p, "points").ok_or_else(|| bad(CMD, "give `handle` (a line or polyline) or `points`"))?;
            let pts = distinct_points(&pts);
            if pts.len() < 2 || pts.len() > pm::MAX_PATH_POINTS {
                return Err(bad(CMD, format!("`points` needs 2 to {} distinct points", pm::MAX_PATH_POINTS)));
            }
            (pts, bool_or(p, "closed", false), None)
        }
    };
    let height = length_param(CMD, p, "height", "m", 3.0)?;
    let thickness = length_param(CMD, p, "thickness", "mm", 230.0)?;
    let mut g = load_graph(d, CMD)?.unwrap_or_else(|| {
        let mut g = Graph::new("drawing");
        g.units_context.drawing_unit_m = m;
        g.units_context.display_length = if m < 0.01 { "mm".into() } else { "m".into() };
        g
    });
    if (g.units_context.drawing_unit_m - m).abs() > 1e-12 {
        return Err(bad(CMD, "the drawing's units changed since the graph was made: update `unitsContext.drawingUnitM` with `param.graph`"));
    }
    let taken = |id: &str| g.nodes.iter().any(|n| n.node_id.starts_with(&format!("{id}.")));
    let id = match str_param(p, "id") {
        Some(id) => {
            if !pm::graph::valid_id(id) || id.len() > pm::MAX_ID_LEN - 12 {
                return Err(bad(CMD, "`id` must be 1-52 letters, digits, `-`, `_` or `.`"));
            }
            if taken(id) {
                return Err(bad(CMD, format!("the graph already has a wall `{id}`")));
            }
            id.to_string()
        }
        None => (1..=pm::MAX_NODES).map(|i| format!("wall-{i}")).find(|id| !taken(id)).ok_or_else(|| bad(CMD, "the graph is full"))?,
    };
    if g.nodes.len().saturating_add(5) > pm::MAX_NODES {
        return Err(bad(CMD, format!("a graph holds at most {} nodes", pm::MAX_NODES)));
    }
    let nodes = pm::template::add_wall(&mut g, &id, &to_metres(&pts, m), closed, height, thickness, source);
    g.graph_revision = g.graph_revision.saturating_add(1);
    let links = load_links(s.doc()?);
    store(s.doc_mut()?, Some(&g), &links);
    Ok(json!({
        "instanceId": id,
        "nodes": { "path": nodes.path, "height": nodes.height, "thickness": nodes.thickness, "wall": nodes.wall, "measure": nodes.measure },
        "revision": g.graph_revision,
        "unitsAssumed": assumed,
    }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "param.set";
    let mut g = need_graph(s.doc()?, CMD)?;
    let id = str_param(p, "node").ok_or_else(|| bad(CMD, "`node` is required"))?;
    let node = g.node_mut(id).ok_or_else(|| bad(CMD, format!("no node `{id}`")))?;
    if !node.config.is_object() {
        node.config = json!({});
    }
    if let Some(v) = p.get("value") {
        node.config["value"] = v.clone();
    }
    if let Some(c) = p.get("config") {
        let c = c.as_object().ok_or_else(|| bad(CMD, "`config` must be an object"))?;
        for (k, v) in c {
            node.config[k.as_str()] = v.clone();
        }
    }
    if p.get("value").is_none() && p.get("config").is_none() {
        return Err(bad(CMD, "give `value` or `config`"));
    }
    if pm::canonical(&node.config).len() > 1_000_000 {
        return Err(bad(CMD, "the node's settings are too large"));
    }
    let preview = bool_or(p, "preview", false);
    if !preview {
        g.graph_revision = g.graph_revision.saturating_add(1);
        let links = load_links(s.doc()?);
        store(s.doc_mut()?, Some(&g), &links);
    }
    let ev = evaluate(s, &g)?;
    Ok(json!({ "revision": g.graph_revision, "preview": preview, "evaluation": ev }))
}

fn run_evaluate(s: &mut Session, _: &Value) -> Result<Value> {
    const CMD: &str = "param.evaluate";
    let mut g = need_graph(s.doc()?, CMD)?;
    let (changed, problems) = refresh_sources(s.doc()?, &mut g);
    let mut ev = evaluate(s, &g)?;
    if !problems.is_empty() {
        ev.diagnostics.extend(problems);
        ev.status = pm::Status::Failed;
    }
    let m = g.units_context.drawing_unit_m;
    let staged: Vec<Value> = ev
        .walls()
        .iter()
        .take(MAX_WALLS)
        .map(|w| json!({ "instanceId": w.instance_id, "lines": faces(w, m).iter().map(|(a, b)| [a.x, a.y, b.x, b.y]).collect::<Vec<_>>() }))
        .collect();
    Ok(json!({ "evaluation": ev, "staged": staged, "sourcesChanged": changed, "revision": g.graph_revision, "drawingRevision": s.state()?.revision }))
}

fn run_commit(s: &mut Session, p: &Value) -> Result<Value> {
    let before = s.state()?.doc.clone();
    let r = commit(s, p);
    if r.is_err()
        && let Ok(st) = s.state_mut()
    {
        st.doc = before;
    }
    r
}

fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "param.commit";
    let mut g = need_graph(s.doc()?, CMD)?;
    if let Some(want) = p.get("graphRevision").and_then(Value::as_u64)
        && want != g.graph_revision
    {
        return Err(bad(CMD, format!("the graph is at revision {}, not {want}: evaluate it again", g.graph_revision)));
    }
    let drawing_rev = s.state()?.revision;
    if let Some(want) = p.get("drawingRevision").and_then(Value::as_u64)
        && want != drawing_rev
    {
        return Err(bad(CMD, format!("the drawing is at revision {drawing_rev}, not {want}: evaluate it again")));
    }
    let (changed, problems) = refresh_sources(s.doc()?, &mut g);
    if !changed.is_empty() {
        g.graph_revision = g.graph_revision.saturating_add(1);
    }
    let ev = evaluate(s, &g)?;
    if let Some(e) = first_error(&ev, &problems) {
        return Err(bad(CMD, format!("the graph has errors, nothing was changed: {e}")));
    }
    if ev.status != pm::Status::Ok {
        return Err(bad(CMD, "the evaluation did not finish, nothing was changed"));
    }
    let walls = ev.walls();
    if walls.len() > MAX_WALLS {
        return Err(bad(CMD, format!("a graph commits at most {MAX_WALLS} walls")));
    }
    let force = bool_or(p, "force", false);
    let mut links = load_links(s.doc()?);
    let m = g.units_context.drawing_unit_m;
    for w in &walls {
        let Some(l) = links.get(&w.instance_id) else { continue };
        if l["state"] == "detached" {
            continue;
        }
        if !force && l["fingerprint"].as_str() != Some(fingerprint(s.doc()?, &handles_of(l)).as_str()) {
            return Err(bad(
                CMD,
                format!(
                    "the lines of wall `{}` were edited in the drawing since they were committed: detach it (`param.detach`) to keep the edits, or commit with `force` to replace them",
                    w.instance_id
                ),
            ));
        }
    }
    ensure_layer(s.doc_mut()?, WALL, 1, "Continuous", 35, "Walls");
    let (mut committed, mut created, mut updated) = (Vec::new(), 0usize, 0usize);
    for w in &walls {
        let link = links.get(&w.instance_id).cloned().unwrap_or(Value::Null);
        if link["state"] == "detached" {
            continue;
        }
        let segs = faces(w, m);
        if segs.is_empty() || segs.len() > MAX_FACES {
            return Err(bad(CMD, format!("wall `{}` has no drawable faces", w.instance_id)));
        }
        let old: Vec<Handle> = handles_of(&link);
        let reuse = old.len() == segs.len()
            && old.iter().all(|h| matches!(s.doc().ok().and_then(|d| d.entity(*h)).map(|e| &e.kind), Some(EntityKind::Line(_))));
        let mut hs = Vec::with_capacity(segs.len());
        if reuse {
            let d = s.doc_mut()?;
            for (h, (a, b)) in old.iter().zip(&segs) {
                d.modify_entity(*h, |e| {
                    if let EntityKind::Line(l) = &mut e.kind {
                        l.a = v3(*a);
                        l.b = v3(*b);
                    }
                    e.common.layer = WALL.into();
                })?;
                hs.push(*h);
            }
            updated += hs.len();
        } else {
            for h in &old {
                s.doc_mut()?.remove_entity(*h);
            }
            for (a, b) in &segs {
                hs.push(add_on(s, WALL, line(*a, *b))?);
            }
            created += hs.len();
        }
        let fp = fingerprint(s.doc()?, &hs);
        links.insert(
            w.instance_id.clone(),
            json!({
                "state": "linked",
                "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>(),
                "fingerprint": fp,
                "graphRevision": g.graph_revision,
                "inputHash": ev.input_hash,
                "drawingRevision": drawing_rev.saturating_add(1),
                "facts": {
                    "componentType": w.component_type, "schemaVersion": w.schema_version,
                    "length_mm": w.length * 1000.0, "height_mm": w.height * 1000.0, "thickness_mm": w.thickness * 1000.0,
                    "level": w.level, "material": w.material, "finish": w.finish,
                },
            }),
        );
        committed.push(w.instance_id.clone());
    }
    // A wall the graph no longer makes keeps its lines; only the link is let go.
    for (id, l) in links.iter_mut() {
        if !committed.contains(id) && l["state"] == "linked" {
            l["state"] = json!("detached");
            l["reason"] = json!("no longer in the graph");
        }
    }
    store(s.doc_mut()?, Some(&g), &links);
    Ok(
        json!({ "committed": committed, "created": created, "updated": updated, "revision": g.graph_revision, "sourcesChanged": changed, "inputHash": ev.input_hash }),
    )
}

/// Why a linked wall is not current: its lines were edited, the graph changed since the commit,
/// or the line it follows moved.
fn stale_reasons(d: &Drawing, g: Option<&Graph>, refreshed: &[String], id: &str, link: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if link["fingerprint"].as_str() != Some(fingerprint(d, &handles_of(link)).as_str()) {
        out.push("its lines were edited in the drawing".to_string());
    }
    match g {
        None => out.push("the graph was removed".into()),
        Some(g) => {
            let at = link["graphRevision"].as_u64().unwrap_or(0);
            let wall_node = g.nodes.iter().find(|n| n.node_type == "component.wall" && n.config["instanceId"].as_str() == Some(id));
            match wall_node {
                None => out.push("the graph no longer has this wall".into()),
                Some(n) => {
                    if g.graph_revision != at {
                        out.push(format!("the graph changed since the commit (revision {at} → {})", g.graph_revision));
                    }
                    let (up, _) = pm::dependencies(g, &n.node_id);
                    if up.iter().any(|u| refreshed.contains(u)) {
                        out.push("the line or polyline it follows changed".into());
                    }
                }
            }
        }
    }
    out
}

struct Instance {
    id: String,
    state: &'static str,
    reasons: Vec<String>,
    link: Value,
}

fn instances(d: &Drawing) -> Result<(Option<Graph>, Vec<Instance>)> {
    let g = load_graph(d, "param.status")?;
    let refreshed = g.clone().map(|mut c| refresh_sources(d, &mut c).0).unwrap_or_default();
    let out = load_links(d)
        .into_iter()
        .map(|(id, link)| {
            let (state, reasons) = if link["state"] == "detached" {
                ("detached", link["reason"].as_str().map(|r| vec![r.to_string()]).unwrap_or_default())
            } else {
                let r = stale_reasons(d, g.as_ref(), &refreshed, &id, &link);
                (if r.is_empty() { "linked" } else { "stale" }, r)
            };
            Instance { id, state, reasons, link }
        })
        .collect();
    Ok((g, out))
}

fn run_status(s: &mut Session, _: &Value) -> Result<Value> {
    let (g, list) = instances(s.doc()?)?;
    let walls: Vec<Value> = list
        .iter()
        .map(|i| json!({ "instanceId": i.id, "state": i.state, "reasons": i.reasons, "handles": i.link["handles"], "graphRevision": i.link["graphRevision"] }))
        .collect();
    Ok(json!({ "revision": g.map_or(0, |g| g.graph_revision), "walls": walls }))
}

fn run_detach(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "param.detach";
    let id = str_param(p, "instance").ok_or_else(|| bad(CMD, "`instance` is required"))?;
    let g = load_graph(s.doc()?, CMD)?;
    let mut links = load_links(s.doc()?);
    let l = links.get_mut(id).ok_or_else(|| bad(CMD, format!("wall `{id}` was never committed")))?;
    l["state"] = json!("detached");
    l["reason"] = json!("detached by the user");
    store(s.doc_mut()?, g.as_ref(), &links);
    Ok(json!({ "detached": id }))
}

fn run_deps(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "param.deps";
    let g = need_graph(s.doc()?, CMD)?;
    let id = str_param(p, "node").ok_or_else(|| bad(CMD, "`node` is required"))?;
    if g.node(id).is_none() {
        return Err(bad(CMD, format!("no node `{id}`")));
    }
    let (up, down) = pm::dependencies(&g, id);
    Ok(json!({ "upstream": up, "downstream": down }))
}

/// A committed wall as the take-off contract's wall row, with its first face line (the record
/// number) and every face line (the record box).
pub(crate) struct WallFact {
    pub row: Value,
    pub provenance: Value,
    pub handles: Vec<Handle>,
}

fn mm(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// The rows of linked, current walls; stale and detached walls are named in the warnings.
pub(crate) fn takeoff_walls(d: &Drawing, level: &str) -> (Vec<WallFact>, Vec<String>) {
    let (mut rows, mut warnings) = (Vec::new(), Vec::new());
    let list = match instances(d) {
        Ok((_, l)) => l,
        Err(e) => {
            warnings.push(format!("parametric walls left out: {e}"));
            return (rows, warnings);
        }
    };
    for i in list {
        match i.state {
            "linked" => {
                let f = &i.link["facts"];
                let num = |k: &str| f[k].as_f64().filter(|v| v.is_finite()).map(mm);
                let (Some(len), Some(h), Some(t)) = (num("length_mm"), num("height_mm"), num("thickness_mm")) else {
                    warnings.push(format!("parametric wall `{}` has no committed measurements: commit it again", i.id));
                    continue;
                };
                let row = json!({
                    "ref": format!("PW-{}", i.id), "level": f["level"].as_str().unwrap_or(level),
                    "length_mm": len, "height_mm": h, "thickness_mm": t, "layer": WALL,
                });
                let provenance = json!({
                    "instanceId": i.id, "componentType": f["componentType"], "schemaVersion": f["schemaVersion"],
                    "material": f["material"], "finish": f["finish"], "measurementRule": "hcw-aqc-bridge/1#walls",
                    "graphRevision": i.link["graphRevision"], "inputHash": i.link["inputHash"], "drawingRevision": i.link["drawingRevision"],
                    "handles": i.link["handles"],
                });
                rows.push(WallFact { row, provenance, handles: handles_of(&i.link) });
            }
            "stale" => {
                warnings.push(format!("parametric wall `{}` is left out because it is stale ({}): commit it again", i.id, i.reasons.join("; ")))
            }
            _ => warnings
                .push(format!("parametric wall `{}` is detached: its lines are plain geometry and are not recorded as a parametric wall", i.id)),
        }
    }
    (rows, warnings)
}

fn run_takeoff(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let level = str_param(p, "level")
        .map(str::to_string)
        .or_else(|| project::floors(d).first().and_then(|f| f["name"].as_str()).map(str::to_string))
        .unwrap_or_else(|| "Ground".into());
    let (rows, warnings) = takeoff_walls(d, &level);
    let walls: Vec<Value> = rows.into_iter().map(|r| json!({ "row": r.row, "provenance": r.provenance })).collect();
    Ok(json!({ "walls": walls, "warnings": warnings }))
}

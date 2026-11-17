//! Projects made of drawing files: one drawing per floor, one per discipline layout of a floor, and
//! a main drawing that shows them all as external references (Xrefs, see [`super::xref`]).
//!
//! ```text
//! <Project>/
//!   project.aad.json          the manifest: floors, layouts, who is visible, locked, printed
//!   Project_Main.dxf          every floor and layout attached at the same origin
//!   Floors/<Floor>.dxf        the architectural plan of one floor (an ordinary drawing)
//!   Layouts/<Floor>/<Layout>.dxf    beam, ceiling, furniture, electrical, plumbing, ...
//!   Exports/
//! ```
//!
//! Every file is a normal drawing that opens anywhere. All paths in the manifest and in the
//! references are relative, so the folder can be moved or backed up as a whole. The manifest is the
//! one record of the project; drawings hold only a pointer to it (project data `projectManifest`).
//!
//! - `project.setup` creates the folders, the files and the main drawing, and opens the main drawing.
//! - `floor.activate` makes one floor (or one of its layouts) the drawing being edited: it opens
//!   that file and shows the other visible floors and layouts in it as locked overlays that are
//!   never saved. Everything drawn then goes into that floor's file.
//! - `floor.set` shows, freezes, locks and includes in printing a floor or layout; the flags apply
//!   to every open drawing of the project at once and are saved in the manifest.
//!
//! Visibility is per file: a floor and a layout of it are independent, so the first floor's plan
//! can be shown with the ground floor's beam layout over it without activating either.
//!
//! Not implemented here: enforcing that a floor file that is not the active one cannot be edited
//! (opening it directly still allows editing) and adding or removing a floor after setup. Batch
//! plot (`plot.batch`) and viewport floors (`floor.viewport`) were removed with paper space
//! (`docs/layout-book.md` 11.1).

use std::path::{Component, Path, PathBuf};

use aad_doc::Drawing;
use serde_json::{Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

pub const MANIFEST: &str = "project.aad.json";
const FORMAT: &str = "aad-project";
const MAX_FLOORS: usize = 60;
const MAX_LAYOUTS: usize = 20;
const MAX_MANIFEST: u64 = 5_000_000;

/// The discipline layouts a floor can have: id, title, and the layers its drawing starts with.
const KNOWN: &[(&str, &str, &[(&str, i16, &str)])] = &[
    ("beam", "Beam Layout", &[("S-BEAM", 5, "Beams"), ("S-BEAM-TEXT", 7, "Beam tags")]),
    ("ceiling", "Ceiling Layout", &[("A-CLNG", 4, "Ceiling outline"), ("A-CLNG-GRID", 8, "Ceiling grid"), ("A-CLNG-TEXT", 7, "Ceiling notes")]),
    ("furniture", "Furniture Layout", &[("A-FURN", 6, "Furniture"), ("A-FURN-TEXT", 7, "Furniture tags")]),
    ("electrical", "Electrical Layout", &[("E-POWR", 1, "Power outlets"), ("E-LITE", 2, "Lighting"), ("E-WIRE", 3, "Wiring")]),
    ("plumbing", "Plumbing Layout", &[("P-PIPE", 5, "Pipes"), ("P-FIXT", 4, "Fixtures"), ("P-TEXT", 7, "Plumbing notes")]),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("project.setup", "New Project", run_setup)
            .menu(&["File", "New Project"])
            .alias(&["projectsetup"])
            .params("{name, folder, id?, client?, location?, address?, type?, units?: metric|imperial, drawingUnits?: mm|cm|m|in|ft, scale? (plot scale, 100), floors?: [{name, elevation?, height?, kind?: basement|ground|upper|mezzanine|terrace|service}] | {count, basements?, terrace?}, categories?: [beam|ceiling|furniture|electrical|plumbing|<custom name>], open?: bool (true)} → {root, manifest, files}")
            .noundo(),
        CommandSpec::new("project.open", "Open Project", run_open)
            .menu(&["File", "Open Project"])
            .alias(&["projectopen"])
            .params("{path: the project folder, project.aad.json, or the main drawing} → {main, floors}")
            .noundo(),
        CommandSpec::new("floor.list", "Floors", run_list)
            .menu(&["Architecture", "Floors", "Floor List"])
            .alias(&["floors.list"])
            .params("{} → {project, active, floors: [{name, file, visible, frozen, locked, print, active, layouts: [...]}]}")
            .noundo(),
        CommandSpec::new("floor.activate", "Activate Floor", run_activate)
            .menu(&["Architecture", "Floors", "Activate Floor"])
            .alias(&["floor"])
            .params("{name: floor, category?: layout id (default the floor plan)} → {floor, file, index, overlays}")
            .noundo(),
        CommandSpec::new("floor.current", "Current Floor", run_current)
            .menu(&["Architecture", "Floors", "Current Floor"])
            .params("{name?: floor to make current} → {mode: project|drawing, floor, floors}")
            .noundo(),
        CommandSpec::new("floor.set", "Floor Visibility", run_set)
            .menu(&["Architecture", "Floors", "Floor Visibility"])
            .params("{name: floor, category?: layout id, visible?: bool, frozen?: bool, locked?: bool, print?: bool} → {entry}")
            .noundo(),
    ]
}

// ---------------- names and paths ----------------

fn safe(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').chars().take(60).collect()
}

/// A path that stays inside the project folder: relative, no `..`, no drive.
fn inside(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty() && !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

// ---------------- the manifest ----------------

pub(super) fn read_manifest(path: &Path) -> Result<Value> {
    let meta = std::fs::metadata(path).map_err(|e| bad("project", format!("{}: {e}", path.display())))?;
    if meta.len() > MAX_MANIFEST {
        return Err(bad("project", "the project file is too large"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| bad("project", format!("{}: {e}", path.display())))?;
    let m: Value = serde_json::from_str(&text).map_err(|e| bad("project", format!("{}: not valid JSON: {e}", path.display())))?;
    if m["format"].as_str() != Some(FORMAT) {
        return Err(bad("project", format!("{} is not an AAD project file", path.display())));
    }
    let floors = m["floors"].as_array().ok_or_else(|| bad("project", "the project has no floors"))?;
    if floors.len() > MAX_FLOORS {
        return Err(bad("project", "too many floors"));
    }
    for f in floors {
        let file = f["file"].as_str().unwrap_or("");
        if !inside(file) || f["key"].as_str().is_none_or(str::is_empty) {
            return Err(bad("project", "a floor in the project file has an invalid file or key"));
        }
        for l in f["layouts"].as_array().into_iter().flatten().take(MAX_LAYOUTS + 1) {
            if !inside(l["file"].as_str().unwrap_or("")) {
                return Err(bad("project", "a layout in the project file has an invalid file"));
            }
        }
    }
    if !inside(m["main"].as_str().unwrap_or("")) {
        return Err(bad("project", "the project file names no valid main drawing"));
    }
    Ok(m)
}

fn write_manifest(path: &Path, m: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(m).map_err(|e| bad("project", e.to_string()))?;
    std::fs::write(path, text).map_err(|e| bad("project", format!("{}: {e}", path.display())))
}

/// The manifest the active drawing belongs to: the one it points at, else the nearest above it.
pub(super) fn find_manifest(s: &Session) -> Result<(PathBuf, Value)> {
    let st = s.state()?;
    let doc_path = st.path.as_ref().map(PathBuf::from).ok_or_else(|| bad("floor", "save the drawing first: it is not in a project"))?;
    let dir = doc_path.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Some(rel) = project::get(&st.doc, "projectManifest").as_str().filter(|r| !r.is_empty()) {
        let p = dir.join(rel);
        if p.is_file() {
            let m = read_manifest(&p)?;
            return Ok((p, m));
        }
    }
    let mut cur = Some(dir.as_path());
    for _ in 0..6 {
        let Some(d) = cur else { break };
        let p = d.join(MANIFEST);
        if p.is_file() {
            let m = read_manifest(&p)?;
            return Ok((p, m));
        }
        cur = d.parent();
    }
    Err(bad("floor", "this drawing is not part of a project (no project.aad.json found); create one with project.setup"))
}

/// One drawing of the project: a floor plan or a layout.
#[derive(Clone, Debug)]
struct Entry {
    /// The xref name this drawing is attached under.
    key: String,
    floor: usize,
    /// `None` for the floor plan itself.
    layout: Option<usize>,
    file: String,
}

fn entries(m: &Value) -> Vec<Entry> {
    let mut out = Vec::new();
    for (i, f) in m["floors"].as_array().into_iter().flatten().enumerate() {
        let fk = f["key"].as_str().unwrap_or("").to_string();
        out.push(Entry { key: fk.clone(), floor: i, layout: None, file: f["file"].as_str().unwrap_or("").to_string() });
        for (j, l) in f["layouts"].as_array().into_iter().flatten().enumerate() {
            out.push(Entry {
                key: format!("{fk}_{}", l["id"].as_str().unwrap_or("")),
                floor: i,
                layout: Some(j),
                file: l["file"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    out
}

fn node<'a>(m: &'a Value, e: &Entry) -> &'a Value {
    match e.layout {
        None => &m["floors"][e.floor],
        Some(j) => &m["floors"][e.floor]["layouts"][j],
    }
}

fn node_mut<'a>(m: &'a mut Value, e: &Entry) -> &'a mut Value {
    match e.layout {
        None => &mut m["floors"][e.floor],
        Some(j) => &mut m["floors"][e.floor]["layouts"][j],
    }
}

fn flag(m: &Value, e: &Entry, k: &str, default: bool) -> bool {
    node(m, e)[k].as_bool().unwrap_or(default)
}

/// The floor named `name` (its name or key, any case) and, if given, its layout `category`.
fn find_entry(m: &Value, name: &str, category: Option<&str>) -> Result<Entry> {
    let fi = m["floors"]
        .as_array()
        .into_iter()
        .flatten()
        .position(|f| [f["name"].as_str(), f["key"].as_str()].into_iter().flatten().any(|n| n.eq_ignore_ascii_case(name)))
        .ok_or_else(|| bad("floor", format!("no floor named `{name}`")))?;
    let all = entries(m);
    match category {
        None => all.into_iter().find(|e| e.floor == fi && e.layout.is_none()).ok_or_else(|| bad("floor", "floor not found")),
        Some(c) => {
            let hit = all.into_iter().find(|e| {
                e.floor == fi && e.layout.is_some_and(|j| m["floors"][fi]["layouts"][j]["id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(c)))
            });
            hit.ok_or_else(|| bad("floor", format!("floor `{name}` has no layout `{c}`")))
        }
    }
}

// ---------------- applying flags to open drawings ----------------

/// Set the layer flags of every xref (and overlay) of the project in every open drawing.
fn apply_flags(s: &mut Session, m: &Value) {
    let all = entries(m);
    for st in &mut s.docs {
        let d = std::sync::Arc::make_mut(&mut st.doc);
        for e in &all {
            let n = node(m, e);
            let (visible, frozen, locked, print) = (
                n["visible"].as_bool().unwrap_or(true),
                n["frozen"].as_bool().unwrap_or(false),
                n["locked"].as_bool().unwrap_or(true),
                n["print"].as_bool().unwrap_or(true),
            );
            for prefix in [format!("{}|", e.key), format!("{}{}|", super::xref::OVERLAY, e.key)] {
                let overlay = prefix.starts_with(super::xref::OVERLAY);
                for l in d.layers.iter_mut().filter(|l| l.name.starts_with(&prefix)) {
                    l.on = visible;
                    l.frozen = frozen;
                    // An overlay is only for looking.
                    l.locked = locked || overlay;
                    l.plot = print;
                }
            }
        }
        st.revision += 1;
    }
}

// ---------------- opening files ----------------

fn open_or_switch(s: &mut Session, path: &Path) -> Result<usize> {
    if let Some(i) = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_file(Path::new(p), path))) {
        s.cancel();
        s.active = i;
        return Ok(i);
    }
    let r = s.execute("open", &json!({ "path": path.to_str().unwrap_or_default() }))?;
    Ok(r["index"].as_u64().map_or(s.active, |i| i as usize))
}

// ---------------- setup ----------------

fn new_drawing(imperial: bool) -> Drawing {
    if imperial { Drawing::new_imperial() } else { Drawing::new_metric() }
}

struct Floor {
    name: String,
    key: String,
    kind: String,
    height: f64,
    elevation: f64,
}

fn parse_floors(p: &Value, imperial: bool) -> Result<Vec<Floor>> {
    let unit_default = if imperial { 10.0 } else { 3.0 };
    let mut raw: Vec<(String, String, f64, Option<f64>)> = Vec::new();
    match p.get("floors") {
        Some(Value::Array(a)) => {
            for f in a {
                let name = f
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| bad("project.setup", "every floor needs a name"))?;
                let kind = f.get("kind").and_then(Value::as_str).unwrap_or("upper").to_ascii_lowercase();
                if !["basement", "ground", "upper", "mezzanine", "terrace", "service"].contains(&kind.as_str()) {
                    return Err(bad("project.setup", format!("floor `{name}`: unknown kind `{kind}`")));
                }
                raw.push((name.to_string(), kind, f64_or(f, "height", unit_default), f.get("elevation").and_then(Value::as_f64)));
            }
        }
        Some(Value::Object(o)) => {
            let count = o
                .get("count")
                .and_then(Value::as_u64)
                .filter(|c| (1..=MAX_FLOORS as u64).contains(c))
                .ok_or_else(|| bad("project.setup", "`floors.count` must be 1 to 60"))?;
            let basements = o.get("basements").and_then(Value::as_u64).unwrap_or(0).min(10);
            for b in (1..=basements).rev() {
                raw.push((format!("Basement {b}"), "basement".into(), unit_default, None));
            }
            let ordinal = ["Ground", "First", "Second", "Third", "Fourth", "Fifth", "Sixth", "Seventh", "Eighth", "Ninth", "Tenth"];
            for i in 0..count {
                let name = ordinal.get(i as usize).map_or_else(|| format!("Floor {i}"), |n| format!("{n} Floor"));
                raw.push((name, if i == 0 { "ground" } else { "upper" }.into(), unit_default, None));
            }
            if o.get("terrace").and_then(Value::as_bool).unwrap_or(false) {
                raw.push(("Terrace".into(), "terrace".into(), unit_default, None));
            }
        }
        _ => return Err(bad("project.setup", "`floors` is required: a list of floors, or {count, basements?, terrace?}")),
    }
    if raw.is_empty() || raw.len() > MAX_FLOORS {
        return Err(bad("project.setup", "between 1 and 60 floors"));
    }
    // The ground floor sits at 0; floors are listed bottom to top.
    let ground = raw.iter().position(|f| f.1 == "ground").or_else(|| raw.iter().position(|f| f.1 != "basement")).unwrap_or(0);
    let mut floors: Vec<Floor> = Vec::new();
    for (name, kind, height, _) in &raw {
        if !(height.is_finite() && *height > 0.0 && *height <= if imperial { 100.0 } else { 30.0 }) {
            return Err(bad(
                "project.setup",
                format!("floor `{name}`: the height must be between 0 and {}", if imperial { "100 ft" } else { "30 m" }),
            ));
        }
        let key = safe(name);
        if key.is_empty() {
            return Err(bad("project.setup", format!("floor `{name}` has no usable characters")));
        }
        if floors.iter().any(|f| f.key.eq_ignore_ascii_case(&key)) {
            return Err(bad("project.setup", format!("two floors are named `{name}`")));
        }
        floors.push(Floor { name: name.clone(), key, kind: kind.clone(), height: *height, elevation: 0.0 });
    }
    let mut above = 0.0;
    for f in floors.iter_mut().skip(ground) {
        f.elevation = above;
        above += f.height;
    }
    let mut below = 0.0;
    for i in (0..ground).rev() {
        below -= floors[i].height;
        floors[i].elevation = below;
    }
    for (i, f) in floors.iter_mut().enumerate() {
        if let Some(e) = raw[i].3 {
            if !e.is_finite() {
                return Err(bad("project.setup", "an elevation is not a number"));
            }
            f.elevation = e;
        }
    }
    Ok(floors)
}

fn parse_categories(p: &Value) -> Result<Vec<(String, String, Vec<(String, i16, String)>)>> {
    let mut out: Vec<(String, String, Vec<(String, i16, String)>)> = Vec::new();
    let given: Vec<String> = match p.get("categories") {
        None => KNOWN.iter().map(|k| k.0.to_string()).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::trim).map(str::to_string)).filter(|s| !s.is_empty()).collect(),
        Some(_) => return Err(bad("project.setup", "`categories` must be a list")),
    };
    if given.len() > MAX_LAYOUTS {
        return Err(bad("project.setup", "too many categories"));
    }
    for g in given {
        if let Some((id, title, layers)) = KNOWN.iter().find(|k| k.0.eq_ignore_ascii_case(&g)) {
            out.push((id.to_string(), title.to_string(), layers.iter().map(|(n, c, d)| (n.to_string(), *c, d.to_string())).collect()));
        } else {
            let id = safe(&g).to_ascii_lowercase();
            if id.is_empty() {
                return Err(bad("project.setup", format!("`{g}` has no usable characters")));
            }
            let layer = format!("X-{}", id.to_ascii_uppercase());
            out.push((id, g.clone(), vec![(layer, 7, g)]));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    if !out.iter().all(|o| seen.insert(o.0.clone())) {
        return Err(bad("project.setup", "a category is listed twice"));
    }
    Ok(out)
}

/// Make a new drawing, run `init` on it, write it to `path` and close it again.
fn make_file(s: &mut Session, path: &Path, imperial: bool, init: &dyn Fn(&mut Session) -> Result<()>) -> Result<()> {
    let keep = s.active;
    let had = s.docs.len();
    let i = s.open_drawing(new_drawing(imperial), "New", None);
    let r = init(s).and_then(|()| s.execute("saveas", &json!({ "path": path.to_str().unwrap_or_default() })).map(|_| ()));
    let _ = s.execute("close", &json!({ "index": i }));
    s.active = if keep < had { keep } else { s.docs.len().saturating_sub(1) };
    r
}

fn run_setup(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("project.setup", "`name` is required"))?.to_string();
    if name.chars().count() > 120 {
        return Err(bad("project.setup", "`name` is too long"));
    }
    let parent = str_param(p, "folder").ok_or_else(|| bad("project.setup", "`folder` is required: where the project folder goes"))?;
    let units = str_param(p, "units").unwrap_or("metric").to_ascii_lowercase();
    if units != "metric" && units != "imperial" {
        return Err(bad("project.setup", "`units` must be metric or imperial"));
    }
    let imperial = units == "imperial";
    let dunits = str_param(p, "drawingUnits").unwrap_or(if imperial { "in" } else { "mm" }).to_ascii_lowercase();
    if !["mm", "cm", "m", "in", "ft"].contains(&dunits.as_str()) {
        return Err(bad("project.setup", "`drawingUnits` must be mm, cm, m, in or ft"));
    }
    let scale = f64_or(p, "scale", 100.0);
    if !(scale.is_finite() && scale > 0.0 && scale <= 100_000.0) {
        return Err(bad("project.setup", "`scale` must be a positive plot scale such as 100"));
    }
    let floors = parse_floors(p, imperial)?;
    let cats = parse_categories(p)?;
    let root = Path::new(parent).join(safe(&name));
    if root.join(MANIFEST).exists() {
        return Err(bad("project.setup", format!("{} already holds a project", root.display())));
    }
    for sub in ["Floors", "Layouts", "Exports"] {
        std::fs::create_dir_all(root.join(sub)).map_err(|e| bad("project.setup", format!("{}: {e}", root.display())))?;
    }
    let id = str_param(p, "id")
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map_or_else(|| format!("{}-{:08x}", safe(&name).to_ascii_uppercase(), id_hash(&name, parent)), str::to_string);
    let text = |k: &str| str_param(p, k).map(str::trim).unwrap_or("").to_string();
    let to_m = if imperial { 0.3048 } else { 1.0 };

    // The manifest.
    let main_rel = format!("{}_Main.dxf", safe(&name));
    let mut fl = Vec::new();
    for (i, f) in floors.iter().enumerate() {
        let mut layouts = Vec::new();
        for c in &cats {
            layouts.push(json!({
                "id": c.0, "name": c.1, "file": format!("Layouts/{}/{}.dxf", f.key, safe(&c.1)),
                "visible": false, "frozen": false, "locked": true, "print": true,
            }));
        }
        fl.push(json!({
            "name": f.name, "key": f.key, "kind": f.kind, "sequence": i, "elevation": f.elevation, "height": f.height,
            "file": format!("Floors/{}.dxf", f.key), "visible": true, "frozen": false, "locked": true, "print": true, "layouts": layouts,
        }));
    }
    let active_floor = floors.iter().find(|f| f.kind == "ground").or(floors.first()).map(|f| f.name.clone()).unwrap_or_default();
    let mut manifest = json!({
        "format": FORMAT, "version": 1, "id": id, "name": name, "client": text("client"), "location": text("location"), "address": text("address"),
        "type": text("type"), "units": units, "drawingUnits": dunits, "scale": scale, "lengthUnit": if imperial { "ft" } else { "m" },
        "main": main_rel, "categories": cats.iter().map(|c| json!({ "id": c.0, "name": c.1 })).collect::<Vec<_>>(),
        "floors": fl, "active": active_floor, "activeCategory": Value::Null,
    });
    // The ground floor starts unlocked: it is the one being edited.
    if let Some(i) = floors.iter().position(|f| f.name == active_floor) {
        manifest["floors"][i]["locked"] = json!(false);
    }

    // Data every drawing of the project carries: details and floors (heights in metres).
    let details = json!({ "title": name, "owner": text("client"), "address": text("address"), "location": text("location") });
    let floor_data: Vec<Value> = floors
        .iter()
        .map(|f| json!({ "name": f.name, "floorToFloor": f.height * to_m, "ceiling": ((f.height * to_m - 0.15) * 100.0).round() / 100.0, "lintel": 2.1_f64.min(f.height * to_m - 0.15), "slab": 150, "beam": 450 }))
        .collect();
    let prep = |s: &mut Session, rel_manifest: &str, extra: Value| -> Result<()> {
        s.execute("drawing.units", &json!({ "units": dunits, "mode": "metadata" }))?;
        let _ = s.execute("plot.config", &json!({ "scale": scale }));
        let d = s.doc_mut()?;
        project::set(d, "details", &details);
        project::set(d, "floors", &Value::Array(floor_data.clone()));
        project::set(d, "projectManifest", &json!(rel_manifest));
        project::set(d, "projectFile", &extra);
        Ok(())
    };

    let mut files: Vec<String> = Vec::new();
    for f in floors.iter() {
        let rel = format!("Floors/{}.dxf", f.key);
        let abs = root.join(&rel);
        let info = json!({ "floor": f.name, "key": f.key, "elevation": f.elevation, "height": f.height, "category": Value::Null });
        make_file(s, &abs, imperial, &|s| {
            prep(s, "../project.aad.json", info.clone())?;
            s.execute("hcwlayers", &json!({ "set": "hcw" }))?;
            Ok(())
        })?;
        files.push(rel);
        for c in &cats {
            let rel = format!("Layouts/{}/{}.dxf", f.key, safe(&c.1));
            let abs = root.join(&rel);
            std::fs::create_dir_all(abs.parent().unwrap_or(&root)).map_err(|e| bad("project.setup", e.to_string()))?;
            let info = json!({ "floor": f.name, "key": f.key, "elevation": f.elevation, "height": f.height, "category": c.0 });
            make_file(s, &abs, imperial, &|s| {
                prep(s, "../../project.aad.json", info.clone())?;
                for (layer, aci, desc) in &c.2 {
                    super::hcw::ensure_layer(s.doc_mut()?, layer, *aci, "Continuous", 18, desc);
                }
                if c.0 == "electrical" {
                    // The electrical layout starts with the kit's labels and tables layers too.
                    let _ = s.execute("electrical.layers", &json!({}));
                }
                Ok(())
            })?;
            files.push(rel);
        }
    }

    // The main drawing: every file attached at the same origin.
    let main_abs = root.join(&main_rel);
    let i = s.open_drawing(new_drawing(imperial), &main_rel, None);
    let built = (|| -> Result<()> {
        prep(s, "project.aad.json", json!({ "main": true }))?;
        s.execute("saveas", &json!({ "path": main_abs.to_str().unwrap_or_default() }))?;
        for e in entries(&manifest) {
            s.execute("xref.attach", &json!({ "path": root.join(&e.file).to_str().unwrap_or_default(), "name": e.key, "at": [0, 0] }))?;
        }
        Ok(())
    })();
    if let Err(e) = built {
        let _ = s.execute("close", &json!({ "index": i }));
        return Err(e);
    }
    apply_flags(s, &manifest);
    s.execute("qsave", &json!({}))?;
    write_manifest(&root.join(MANIFEST), &manifest)?;
    let keep_open = bool_or(p, "open", true);
    if !keep_open {
        let _ = s.execute("close", &json!({ "index": i }));
    }
    files.insert(0, main_rel.clone());
    Ok(json!({
        "root": root.display().to_string(), "manifest": root.join(MANIFEST).display().to_string(), "main": main_rel, "files": files, "projectId": id,
        "message": format!("Project `{name}` created in {} ({} floor(s), {} file(s))", root.display(), floors.len(), files.len()),
    }))
}

fn id_hash(name: &str, parent: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes().chain(parent.bytes()) {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        h ^= t;
    }
    h
}

// ---------------- open, list, activate, set ----------------

fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    let raw = str_param(p, "path").ok_or_else(|| bad("project.open", "`path` is required"))?;
    let given = Path::new(raw);
    let manifest_path = if given.is_dir() {
        given.join(MANIFEST)
    } else if given.file_name().is_some_and(|n| n == MANIFEST) {
        given.to_path_buf()
    } else {
        // The main drawing (or any file of the project): look beside it and above.
        let mut found = None;
        let mut cur = given.parent();
        for _ in 0..6 {
            let Some(d) = cur else { break };
            if d.join(MANIFEST).is_file() {
                found = Some(d.join(MANIFEST));
                break;
            }
            cur = d.parent();
        }
        found.ok_or_else(|| bad("project.open", "no project.aad.json found there"))?
    };
    let m = read_manifest(&manifest_path)?;
    let dir = manifest_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let main = dir.join(m["main"].as_str().unwrap_or(""));
    let i = open_or_switch(s, &main)?;
    // Bring the references up to date and re-apply the saved visibility.
    let _ = s.execute("xref.reload", &json!({}));
    apply_flags(s, &m);
    Ok(
        json!({ "main": main.display().to_string(), "index": i, "floors": m["floors"].as_array().map_or(0, Vec::len), "message": format!("Opened project `{}`", m["name"].as_str().unwrap_or("")) }),
    )
}

fn summary(m: &Value, dir: &Path) -> Value {
    let active = m["active"].as_str().unwrap_or("");
    let active_cat = m["activeCategory"].as_str();
    let floors: Vec<Value> = m["floors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let is_active = f["name"].as_str() == Some(active);
            let layouts: Vec<Value> = f["layouts"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| {
                    json!({
                        "id": l["id"], "name": l["name"], "file": l["file"], "visible": l["visible"], "frozen": l["frozen"], "locked": l["locked"], "print": l["print"],
                        "exists": dir.join(l["file"].as_str().unwrap_or("")).is_file(), "active": is_active && active_cat == l["id"].as_str(),
                    })
                })
                .collect();
            json!({
                "name": f["name"], "key": f["key"], "kind": f["kind"], "elevation": f["elevation"], "height": f["height"], "file": f["file"],
                "visible": f["visible"], "frozen": f["frozen"], "locked": f["locked"], "print": f["print"],
                "exists": dir.join(f["file"].as_str().unwrap_or("")).is_file(), "active": is_active && active_cat.is_none(), "layouts": layouts,
            })
        })
        .collect();
    json!({
        "project": { "name": m["name"], "id": m["id"], "client": m["client"], "location": m["location"], "units": m["units"], "scale": m["scale"], "root": dir.display().to_string() },
        "active": m["active"], "activeCategory": m["activeCategory"], "floors": floors,
    })
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let (path, m) = find_manifest(s)?;
    Ok(summary(&m, path.parent().unwrap_or(Path::new(""))))
}

/// Show the other visible floors and layouts in the active drawing as locked overlays, and remove
/// the ones that are hidden or frozen. The active drawing's own file is never overlaid.
fn sync_overlays(s: &mut Session, m: &Value, dir: &Path, target: &Entry) -> Result<Vec<String>> {
    let main_abs = dir.join(m["main"].as_str().unwrap_or(""));
    if s.state()?.path.as_deref().is_some_and(|p| same_file(Path::new(p), &main_abs)) {
        return Ok(Vec::new());
    }
    let existing: Vec<String> = s.doc()?.blocks.keys().filter(|k| super::xref::is_overlay(k) && k.matches('|').count() == 1).cloned().collect();
    let mut shown = Vec::new();
    for e in entries(m) {
        let wanted = e.file != target.file && flag(m, &e, "visible", true) && !flag(m, &e, "frozen", false) && dir.join(&e.file).is_file();
        let name = format!("{}{}", super::xref::OVERLAY, e.key);
        let present = existing.iter().any(|x| x.eq_ignore_ascii_case(&name));
        match (wanted, present) {
            (true, false) => {
                s.execute(
                    "xref.attach",
                    &json!({ "path": dir.join(&e.file).to_str().unwrap_or_default(), "name": e.key, "overlay": true, "at": [0, 0] }),
                )?;
                shown.push(e.key.clone());
            }
            (true, true) => {
                s.execute("xref.reload", &json!({ "name": name }))?;
                shown.push(e.key.clone());
            }
            (false, true) => {
                s.execute("xref.detach", &json!({ "name": name }))?;
            }
            (false, false) => {}
        }
    }
    Ok(shown)
}

fn run_activate(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("floor.activate", "`name` (the floor) is required"))?;
    let category = str_param(p, "category");
    let (mpath, mut m) = find_manifest(s)?;
    let dir = mpath.parent().map(Path::to_path_buf).unwrap_or_default();
    let target = find_entry(&m, name, category)?;
    let abs = dir.join(&target.file);
    if !abs.is_file() {
        return Err(bad("floor.activate", format!("the file {} is missing", target.file)));
    }
    let floor_name = m["floors"][target.floor]["name"].as_str().unwrap_or("").to_string();
    // Only the active floor (and its layouts) can be edited; the others are locked.
    let n = m["floors"].as_array().map_or(0, Vec::len);
    for fi in 0..n {
        let editable = fi == target.floor;
        m["floors"][fi]["locked"] = json!(!editable);
        let l = m["floors"][fi]["layouts"].as_array().map_or(0, Vec::len);
        for li in 0..l {
            m["floors"][fi]["layouts"][li]["locked"] = json!(!editable);
        }
    }
    m["active"] = json!(floor_name);
    m["activeCategory"] = category.map_or(Value::Null, |c| json!(c.to_ascii_lowercase()));
    // The file being edited is shown, whatever its flags say.
    if let Some(o) = node_mut(&mut m, &target).as_object_mut() {
        o.insert("visible".into(), json!(true));
        o.insert("frozen".into(), json!(false));
        o.insert("locked".into(), json!(false));
    }
    write_manifest(&mpath, &m)?;
    let i = open_or_switch(s, &abs)?;
    let shown = sync_overlays(s, &m, &dir, &target)?;
    apply_flags(s, &m);
    s.active = i;
    Ok(json!({
        "floor": floor_name, "category": category, "file": target.file, "index": i, "overlays": shown,
        "message": format!("Active: {floor_name}{}", category.map_or(String::new(), |c| format!(" / {c}"))),
    }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("floor.set", "`name` (the floor) is required"))?;
    let category = str_param(p, "category");
    let (mpath, mut m) = find_manifest(s)?;
    let dir = mpath.parent().map(Path::to_path_buf).unwrap_or_default();
    let target = find_entry(&m, name, category)?;
    let mut changed = Vec::new();
    for k in ["visible", "frozen", "locked", "print"] {
        if let Some(v) = p.get(k) {
            let b = v.as_bool().ok_or_else(|| bad("floor.set", format!("`{k}` must be true or false")))?;
            node_mut(&mut m, &target)[k] = json!(b);
            changed.push(k);
        }
    }
    if changed.is_empty() {
        return Err(bad("floor.set", "give at least one of visible, frozen, locked, print"));
    }
    write_manifest(&mpath, &m)?;
    // Update the open drawings: overlays in the one being edited, layer flags everywhere.
    let here = s.active;
    let editing = s.state()?.path.as_deref().and_then(|ap| entries(&m).into_iter().find(|e| same_file(&dir.join(&e.file), Path::new(ap))));
    if let Some(cur) = editing {
        sync_overlays(s, &m, &dir, &cur)?;
    }
    s.active = here;
    apply_flags(s, &m);
    Ok(json!({ "entry": node(&m, &target).clone(), "changed": changed }))
}

/// `floor.current`: the floor new objects belong to. In a project it is the manifest's active floor
/// (changing it opens that floor's drawing, `floor.activate`); in a single drawing it is a stored
/// choice among the drawing's floors. Choosing a floor never moves or edits an object and never
/// changes a layer's state or the active drawing space.
fn run_current(s: &mut Session, p: &Value) -> Result<Value> {
    if find_manifest(s).is_ok() {
        if str_param(p, "name").is_some() {
            return s.execute("floor.activate", p);
        }
        let (path, m) = find_manifest(s)?;
        let mut v = summary(&m, path.parent().unwrap_or(Path::new("")));
        v["mode"] = json!("project");
        v["floor"] = v["active"].clone();
        return Ok(v);
    }
    let d = s.doc()?;
    let names: Vec<String> = project::floors(d).iter().filter_map(|f| f.get("name").and_then(Value::as_str).map(str::to_string)).collect();
    if let Some(n) = str_param(p, "name") {
        let found = names
            .iter()
            .find(|x| x.eq_ignore_ascii_case(n))
            .cloned()
            .ok_or_else(|| bad("floor.current", format!("no floor `{n}`; the floors are {}", names.join(", "))))?;
        // Effective current: stored activeFloor when valid, else first floor (including the virtual
        // default Ground when no floors are persisted yet). Do not COW just to write that default.
        let effective = project::active_floor(d).map(|(_, cur)| cur).or_else(|| names.first().cloned()).unwrap_or_default();
        if !effective.eq_ignore_ascii_case(&found) {
            project::set(s.doc_mut()?, "activeFloor", &json!(found));
        }
    }
    let d = s.doc()?;
    let (_, active) = project::active_floor(d).unwrap_or((0, names.first().cloned().unwrap_or_default()));
    Ok(json!({"mode": "drawing", "floor": active, "floors": names}))
}

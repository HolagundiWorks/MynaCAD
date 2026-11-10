//! SketchUp-style components: shared block definitions with optional building classification.
//!
//! Built on ordinary blocks (`block` / `insert` / `bedit`). `component.make` creates a definition
//! from the selection (or handles), optionally classifies it, and places one INSERT. `makeunique`
//! duplicates a definition so an instance can diverge.

use aad_doc::{BuildingElement, EntityKind, Handle};
use aad_geom::Vec2;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("component.make", "Make Component", run_make).menu(&["Tools", "Component", "Make"]).alias(&["cmake"]).params(
            "{name, handles? (default selection), base?: [x,y] (default selection centre), \
                 kind?: building kind, family?, ifcType?, measurementRule?, specIds?: [...], specRefs?: [...], \
                 keep?: \"convert\"|\"retain\"|\"delete\" (default convert)} → {block, handle?, building?}",
        ),
        CommandSpec::new("component.place", "Place Component", run_place)
            .menu(&["Tools", "Component", "Place"])
            .alias(&["cplace"])
            .params("{name, at: [x,y], scale?: number | [x,y], rotation? (degrees)} → insert result"),
        CommandSpec::new("component.makeunique", "Make Unique", run_makeunique)
            .menu(&["Tools", "Component", "Make Unique"])
            .alias(&["cmakeunique"])
            .params("{handle (an INSERT), name?: new definition name} → {block, handle}"),
        CommandSpec::new("component.edit", "Edit Component", run_edit)
            .menu(&["Tools", "Component", "Edit"])
            .alias(&["cedit"])
            .params("{name | handle (INSERT)} → starts bedit / refedit on the definition"),
    ]
}

fn handles_param(cmd: &str, s: &Session, p: &Value) -> Result<Vec<Handle>> {
    match p.get("handles") {
        None | Some(Value::Null) => {
            let sel = s.selection().to_vec();
            if sel.is_empty() { Err(bad(cmd, "nothing selected")) } else { Ok(sel) }
        }
        Some(Value::Array(a)) => {
            if a.len() > 10_000 {
                return Err(bad(cmd, "`handles` has too many entries"));
            }
            a.iter()
                .map(|v| {
                    v.as_str()
                        .and_then(Handle::parse_hex)
                        .or_else(|| v.as_u64().map(Handle))
                        .ok_or_else(|| bad(cmd, format!("`handles`: {v} is not an object handle")))
                })
                .collect()
        }
        Some(v) => Err(bad(cmd, format!("`handles` must be an array, got {v}"))),
    }
}

fn point2(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec2>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let a = v.as_array().ok_or_else(|| bad(cmd, format!("`{key}` must be [x,y]")))?;
            if a.len() < 2 {
                return Err(bad(cmd, format!("`{key}` must be [x,y]")));
            }
            let x = a[0].as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad(cmd, format!("`{key}` needs finite numbers")))?;
            let y = a[1].as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad(cmd, format!("`{key}` needs finite numbers")))?;
            Ok(Some(Vec2::new(x, y)))
        }
    }
}

fn selection_centre(s: &Session, hs: &[Handle]) -> Vec2 {
    let d = match s.doc() {
        Ok(d) => d,
        Err(_) => return Vec2::ZERO,
    };
    let mut n = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    for h in hs {
        if let Some(e) = d.entity(*h) {
            for p in e.kind.grips() {
                sx += p.x;
                sy += p.y;
                n += 1.0;
            }
        }
    }
    if n > 0.0 { Vec2::new(sx / n, sy / n) } else { Vec2::ZERO }
}

fn run_make(s: &mut Session, p: &Value) -> Result<Value> {
    let name =
        p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("component.make", "`name` is required"))?;
    let hs = handles_param("component.make", s, p)?;
    let hex: Vec<String> = hs.iter().map(|h| h.hex()).collect();
    let base = point2("component.make", p, "base")?.unwrap_or_else(|| selection_centre(s, &hs));
    let keep = p.get("keep").and_then(Value::as_str).unwrap_or("convert");
    let block_r = s.execute(
        "block",
        &json!({
            "name": name,
            "base": [base.x, base.y],
            "handles": hex,
            "keep": keep,
            "description": "MynaCAD component",
        }),
    )?;
    let mut building: Option<BuildingElement> = None;
    if p.get("kind").is_some() {
        let mut class = p.clone();
        if let Some(obj) = class.as_object_mut() {
            obj.insert("block".into(), json!(name));
            obj.remove("handles");
            obj.remove("name");
            obj.remove("base");
            obj.remove("keep");
        }
        s.execute("classify.set", &class)?;
        building = s.doc()?.block(name).and_then(|b| b.building.clone());
    }
    // Prefer an INSERT created by convert keep (last matching insert in model).
    let handle = s
        .doc()?
        .model
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)))
        .map(|e| e.handle.hex())
        .last();
    let mut out = json!({ "block": name, "blockResult": block_r });
    if let Some(h) = handle {
        out["handle"] = json!(h);
    }
    if let Some(b) = building {
        out["building"] = json!(b);
    }
    Ok(out)
}

fn run_place(s: &mut Session, p: &Value) -> Result<Value> {
    let name =
        p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("component.place", "`name` is required"))?;
    if s.doc()?.block(name).is_none() {
        return Err(bad("component.place", format!("no component / block `{name}`")));
    }
    let at = point2("component.place", p, "at")?.ok_or_else(|| bad("component.place", "`at` is required"))?;
    let mut args = json!({ "name": name, "at": [at.x, at.y] });
    if let Some(sc) = p.get("scale") {
        args["scale"] = sc.clone();
    }
    if let Some(r) = p.get("rotation") {
        args["rotation"] = r.clone();
    }
    s.execute("insert", &args)
}

fn unique_name(d: &aad_doc::Drawing, base: &str) -> String {
    let stem = if base.is_empty() { "Component" } else { base };
    for n in 1..10_000 {
        let cand = format!("{stem}_{n}");
        if d.blocks.keys().all(|k| !k.eq_ignore_ascii_case(&cand)) {
            return cand;
        }
    }
    format!("{stem}_copy")
}

fn run_makeunique(s: &mut Session, p: &Value) -> Result<Value> {
    let h = p
        .get("handle")
        .and_then(Value::as_str)
        .and_then(Handle::parse_hex)
        .ok_or_else(|| bad("component.makeunique", "`handle` (INSERT) is required"))?;
    let (old_name, insert_pt, scale, rotation) = {
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad("component.makeunique", "no such object"))?;
        let EntityKind::Insert(i) = &e.kind else {
            return Err(bad("component.makeunique", "object is not a block reference"));
        };
        (i.block.clone(), i.insert, i.scale, i.rotation)
    };
    let new_name = match p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => {
            if s.doc()?.blocks.keys().any(|k| k.eq_ignore_ascii_case(n)) {
                return Err(bad("component.makeunique", format!("a block named `{n}` already exists")));
            }
            n.to_string()
        }
        None => unique_name(s.doc()?, &old_name),
    };
    // Copy definition entities into a new block via wblock-style redefine: use block from exploded geometry.
    // Snapshot entities from the old block into the new one.
    {
        let d = s.doc_mut()?;
        let src = d.blocks.get(&old_name).ok_or_else(|| bad("component.makeunique", format!("no block `{old_name}`")))?.clone();
        let mut nb = aad_doc::Block::new(&new_name);
        nb.base = src.base;
        nb.description = src.description.clone();
        nb.explodable = src.explodable;
        nb.units = src.units;
        nb.building = src.building.clone();
        for e in src.entities.iter() {
            let mut copy = (**e).clone();
            copy.handle = d.new_handle();
            nb.entities.push(copy);
        }
        d.blocks.insert(new_name.clone(), std::sync::Arc::new(nb));
        let _ = d.modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.block = new_name.clone();
                i.insert = insert_pt;
                i.scale = scale;
                i.rotation = rotation;
            }
        });
        super::classify::sync_meta(d);
    }
    Ok(json!({ "block": new_name, "handle": h.hex() }))
}

fn run_edit(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        return s.execute("bedit", &json!({ "name": name }));
    }
    if let Some(h) = p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex) {
        s.set_selection(vec![h]);
        return s.execute("refedit", &json!({}));
    }
    Err(bad("component.edit", "give `name` or `handle`"))
}

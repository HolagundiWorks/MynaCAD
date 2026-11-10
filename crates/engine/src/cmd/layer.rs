//! Layers: LAYER (programmatic sub-ids, -LAYER prompts), layer tools (LAYMCUR, LAYISO, LAYOFF, LAYFRZ…).

mod prompts;

use aad_color::Color;
use aad_doc::{Layer, Lineweight};
use serde_json::{Value, json};

use super::machines::SelectRun;
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layer", "Layers", run_list)
            .menu(&["Format", "Layers"])
            .alias(&["la", "layers", "-layer"])
            .params("{showFor?: \"all\" | \"level\" | \"sheetSet\"} → layers (each with used, onLevel, onSheetSet), states, linetypes, scope: {level, sheetSet} (typed -LAYER: option prompts). `level` keeps the layers used on the active level and its sheet sets' annotation of it; `sheetSet` those used by the active sheet set's annotation or named with its prefix.")
            .interactive(|_| Ok(Box::new(prompts::LayerM::default()))),
        CommandSpec::new("layer.new", "New Layer", run_new)
            .menu(&["Format", "Layer Tools", "New Layer"])
            .alias(&["-layer.new"])
            .params("{name, color?, linetype?, lineweight? (mm), current?: bool}"),
        CommandSpec::new("layer.set", "Set Layer Properties", run_set)
            .alias(&["-layer.set"])
            .params("{name | names: [name, ...], on?, frozen?, locked?, plot?, color?, linetype?, lineweight?, transparency?, description?, newVpFreeze?, plotStyle? (named plot style), newName? (one layer only)} → null | {changed, skipped}. `names` applies the properties to every named layer in one undo step (a multiple selection in the Layer manager); freezing skips the current layer and says so, and any other problem changes nothing."),
        CommandSpec::new("layer.current", "Make Current", run_current)
            .menu(&["Format", "Layer Tools", "Make Current"])
            .alias(&["clayer"])
            .params("{name}"),
        CommandSpec::new("layer.delete", "Delete Layer", run_delete)
            .menu(&["Format", "Layer Tools", "Delete Layer"])
            .alias(&["-layer.delete"])
            .params("{name | names: [name, ...]} → null | {deleted, skipped}. `names` deletes every named layer that is empty (not 0, Defpoints, the current layer, or a layer with objects) in one undo step and lists the rest in `skipped`."),
        CommandSpec::new("laymcur", "Make Object's Layer Current", run_laymcur)
            .menu(&["Format", "Layer Tools", "Make Object's Layer Current"])
            .alias(&["mlc"])
            .params("{handles?}"),
        CommandSpec::new("laymch", "Layer Match", run_laymch).menu(&["Format", "Layer Tools", "Layer Match"]).params("{handles?, layer}"),
        CommandSpec::new("laycur", "Change to Current Layer", run_laycur)
            .menu(&["Format", "Layer Tools", "Change to Current Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("laycur", "LAYCUR")))),
        CommandSpec::new("layiso", "Isolate Layer", run_layiso)
            .menu(&["Format", "Layer Tools", "Isolate Layer"])
            .params("{handles?, name?, names?: [string]} (name/names isolate by layer without a pick)"),
        CommandSpec::new("layuniso", "Unisolate Layer", run_layuniso).menu(&["Format", "Layer Tools", "Unisolate Layer"]),
        CommandSpec::new("layfrz", "Freeze Layer", |s, p| set_obj_layers(s, p, |l| l.frozen = true))
            .menu(&["Format", "Layer Tools", "Freeze Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("layfrz", "LAYFRZ", "Select an object on the layer to be frozen")))),
        CommandSpec::new("layoff", "Layer Off", |s, p| set_obj_layers(s, p, |l| l.on = false))
            .menu(&["Format", "Layer Tools", "Layer Off"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("layoff", "LAYOFF", "Select an object on the layer to be turned off")))),
        CommandSpec::new("laylck", "Lock Layer", |s, p| set_obj_layers(s, p, |l| l.locked = true))
            .menu(&["Format", "Layer Tools", "Lock Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("laylck", "LAYLCK", "Select an object on the layer to be locked")))),
        CommandSpec::new("layulk", "Unlock Layer", |s, p| set_obj_layers(s, p, |l| l.locked = false))
            .menu(&["Format", "Layer Tools", "Unlock Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("layulk", "LAYULK", "Select an object on the layer to be unlocked")))),
        CommandSpec::new("layon", "Turn All Layers On", |s, _| all_layers(s, |l| l.on = true))
            .menu(&["Format", "Layer Tools", "Turn All Layers On"])
            .alias(&["layers.on"]),
        CommandSpec::new("laythw", "Thaw All Layers", |s, _| all_layers(s, |l| l.frozen = false))
            .menu(&["Format", "Layer Tools", "Thaw All Layers"])
            .alias(&["layers.thaw"]),
        CommandSpec::new("layerp", "Previous Layer", run_layerp).menu(&["Format", "Layer Tools", "Previous Layer"]),
        CommandSpec::new("layerstate.save", "Save Layer State", run_state_save).menu(&["Format", "Layer States Manager..."]).params("{name}"),
        CommandSpec::new("layerstate.restore", "Restore Layer State", run_state_restore)
            .menu(&["Format", "Layer States", "Restore"])
            .alias(&["las.restore"])
            .params("{name}"),
        CommandSpec::new("layerstate.list", "List Layer States", run_state_list)
            .menu(&["Format", "Layer States", "List"])
            .params("{} → states")
            .noundo(),
        CommandSpec::new("layerstate.delete", "Delete Layer State", run_state_delete)
            .menu(&["Format", "Layer States", "Delete"])
            .params("{name}"),
        CommandSpec::new("layerstate.rename", "Rename Layer State", run_state_rename)
            .menu(&["Format", "Layer States", "Rename"])
            .params("{from, to}"),
        CommandSpec::new("laymrg", "Layer Merge", merge::run_laymrg)
            .menu(&["Format", "Layer Tools", "Layer Merge"])
            .params("{from: [names], to} (the from layers' objects move to `to`, then the from layers are deleted)")
            .interactive(|_| Ok(Box::new(merge::LayerRemove::merge()))),
        CommandSpec::new("laydel", "Layer Delete", merge::run_laydel)
            .menu(&["Format", "Layer Tools", "Layer Delete"])
            .params("{names: [...]} (deletes the layers and every object on them)")
            .interactive(|_| Ok(Box::new(merge::LayerRemove::delete()))),
        CommandSpec::new("laytrans", "Layer Translate", run_laytrans)
            .menu(&["Format", "Layer Tools", "Layer Translate"])
            .alias(&["laytranslate"])
            .params("{map: {\"from\": \"to\", ...}, createMissing?: bool} → {renamed, created, moved}"),
        CommandSpec::new("laywalk", "Layer Walk", run_laywalk)
            .menu(&["Format", "Layer Tools", "Layer Walk"])
            .params("{step?: int (default 1), layers?: [names] (order to walk; default all), restore?: bool} → {current, on, index}")
            .noundo(),
    ]
}

fn layer_json(l: &Layer, current: bool) -> Value {
    json!({
        "name": l.name, "on": l.on, "frozen": l.frozen, "locked": l.locked, "plot": l.plot,
        "color": l.color.name(), "colorRgb": l.color.resolve(Color::Index(7), Color::Index(7)).hex(),
        "linetype": l.linetype, "lineweight": l.lineweight.name(), "transparency": l.transparency,
        "description": l.description, "current": current, "newVpFreeze": l.vp_freeze_new, "plotStyle": l.plot_style,
    })
}

/// Lower-case names of the layers used on the active level (its design objects and every sheet
/// set's annotation of it), and by the active sheet set (its annotation on any level, and layers
/// named with its prefix). A drawing without levels counts all of model space as its one level.
/// Lower-case names of the layers used on the active level (see `layer_scopes`).
pub fn layers_on_level(d: &aad_doc::Drawing) -> std::collections::HashSet<String> {
    layer_scopes(d).0
}

fn layer_scopes(d: &aad_doc::Drawing) -> (std::collections::HashSet<String>, std::collections::HashSet<String>) {
    let mut level = std::collections::HashSet::new();
    let mut sheet = std::collections::HashSet::new();
    let add = |set: &mut std::collections::HashSet<String>, st: &aad_doc::EntityStore| {
        for e in st.iter().take(5_000_000) {
            set.insert(e.common.layer.to_ascii_lowercase());
        }
    };
    match d.active_level {
        Some(l) => {
            if let Some(st) = d.level_store(l) {
                add(&mut level, st);
            }
            for set in &d.sheet_sets {
                if let Some(st) = d.annotation_store(set.id, l) {
                    add(&mut level, st);
                }
            }
        }
        None => add(&mut level, &d.model),
    }
    if let Some(set) = d.active_sheet_ref() {
        for l in &d.levels {
            if let Some(st) = d.annotation_store(set.id, l.id) {
                add(&mut sheet, st);
            }
        }
        // The layers the set maps (its prefix and its patterns).
        sheet.extend(d.layers.iter().filter(|l| set.maps_layer(&l.name)).map(|l| l.name.to_ascii_lowercase()));
    }
    (level, sheet)
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let show_for = match p.get("showFor") {
        None | Some(Value::Null) => "all".to_string(),
        Some(Value::String(t)) if matches!(t.to_ascii_lowercase().as_str(), "all" | "level" | "sheetset") => t.to_ascii_lowercase(),
        Some(_) => return Err(bad("layer", "`showFor` is `all`, `level` or `sheetSet`")),
    };
    let d = s.doc()?;
    let cur = d.header.str("CLAYER", "0");
    let used = used_layers(d);
    let (on_level, on_sheet) = layer_scopes(d);
    let layers: Vec<Value> = d
        .layers
        .iter()
        .filter(|l| {
            let key = l.name.to_ascii_lowercase();
            match show_for.as_str() {
                "level" => on_level.contains(&key),
                "sheetset" => on_sheet.contains(&key),
                _ => true,
            }
        })
        .map(|l| {
            let key = l.name.to_ascii_lowercase();
            let mut v = layer_json(l, l.name.eq_ignore_ascii_case(&cur));
            if let Some(o) = v.as_object_mut() {
                o.insert("used".into(), json!(used.contains(&key)));
                o.insert("onLevel".into(), json!(on_level.contains(&key)));
                o.insert("onSheetSet".into(), json!(on_sheet.contains(&key)));
            }
            v
        })
        .collect();
    let linetypes: Vec<String> = d.linetypes.iter().take(1_024).map(|t| t.name.clone()).collect();
    Ok(json!({
        "layers": layers,
        "states": d.layer_states.iter().map(|st| st.name.clone()).collect::<Vec<_>>(),
        "linetypes": linetypes,
        "scope": { "level": d.active_level_ref().map(|l| l.name.clone()), "sheetSet": d.active_sheet_ref().map(|x| x.name.clone()) },
    }))
}

/// Lower-case names of layers that have objects on them (model, layouts and block definitions).
pub fn used_layers(d: &aad_doc::Drawing) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let ents = d
        .model_stores()
        .flat_map(|s| s.iter())
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    for e in ents.take(5_000_000) {
        if !set.contains(&e.common.layer.to_ascii_lowercase()) {
            set.insert(e.common.layer.to_ascii_lowercase());
        }
    }
    set
}

fn parse_lw(v: &Value) -> Option<Lineweight> {
    if let Some(f) = v.as_f64() {
        return Some(Lineweight::Mm100((f * 100.0).round().clamp(0.0, 211.0) as u16));
    }
    match v.as_str()?.to_ascii_lowercase().as_str() {
        "bylayer" => Some(Lineweight::ByLayer),
        "byblock" => Some(Lineweight::ByBlock),
        "default" => Some(Lineweight::Default),
        s => s.trim_end_matches("mm").trim().parse::<f64>().ok().map(|f| Lineweight::Mm100((f * 100.0).round().clamp(0.0, 211.0) as u16)),
    }
}

fn apply(l: &mut Layer, p: &Value) -> Result<()> {
    if let Some(v) = p.get("on").and_then(Value::as_bool) {
        l.on = v;
    }
    if let Some(v) = p.get("frozen").and_then(Value::as_bool) {
        l.frozen = v;
    }
    if let Some(v) = p.get("locked").and_then(Value::as_bool) {
        l.locked = v;
    }
    if let Some(v) = p.get("plot").and_then(Value::as_bool) {
        l.plot = v;
    }
    if let Some(v) = str_param(p, "plotStyle").map(str::trim) {
        if v.is_empty() || v.eq_ignore_ascii_case("bylayer") || v.eq_ignore_ascii_case("byblock") {
            return Err(bad("layer", "a layer's plot style is a style name (such as Normal)"));
        }
        l.plot_style = v.chars().take(255).collect();
    }
    if let Some(c) = p.get("color") {
        let c = c
            .as_str()
            .and_then(Color::parse)
            .or_else(|| c.as_u64().and_then(|i| u8::try_from(i).ok()).filter(|i| *i > 0).map(Color::Index))
            .ok_or_else(|| bad("layer", "bad colour"))?;
        if matches!(c, Color::ByLayer | Color::ByBlock) {
            return Err(bad("layer", "a layer colour must be an index or true colour"));
        }
        l.color = c;
    }
    if let Some(lt) = str_param(p, "linetype") {
        l.linetype = lt.to_string();
    }
    if let Some(lw) = p.get("lineweight") {
        l.lineweight = parse_lw(lw).ok_or_else(|| bad("layer", "bad lineweight"))?;
    }
    if let Some(t) = p.get("transparency").and_then(Value::as_u64) {
        l.transparency = t.min(90) as u8;
    }
    if let Some(v) = p.get("newVpFreeze").and_then(Value::as_bool) {
        l.vp_freeze_new = v;
    }
    if let Some(d) = str_param(p, "description") {
        l.description = d.to_string();
    }
    Ok(())
}

/// The loaded linetype named by `linetype`, if given: a layer cannot use ByLayer, ByBlock or a
/// linetype the drawing does not have.
fn loaded_linetype(d: &aad_doc::Drawing, cmd: &str, p: &Value) -> Result<Option<String>> {
    let Some(lt) = str_param(p, "linetype") else { return Ok(None) };
    if lt.eq_ignore_ascii_case("ByLayer") || lt.eq_ignore_ascii_case("ByBlock") {
        return Err(bad(cmd, "a layer linetype cannot be ByLayer or ByBlock"));
    }
    let t = d.linetype(lt).ok_or_else(|| bad(cmd, format!("linetype `{lt}` is not loaded (load it with LINETYPE)")))?;
    Ok(Some(t.name.clone()))
}

fn valid_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layer.new", "`name` is required"))?.to_string();
    if !valid_name(&name) {
        return Err(bad("layer.new", "invalid layer name"));
    }
    let d = s.doc_mut()?;
    if d.layer(&name).is_some() {
        return Err(bad("layer.new", format!("layer `{name}` already exists")));
    }
    let lt = loaded_linetype(d, "layer.new", p)?;
    let mut l = Layer::new(&name);
    apply(&mut l, p)?;
    if let Some(lt) = lt {
        l.linetype = lt;
    }
    d.layers.push(l);
    if bool_or(p, "current", false) {
        d.header.set_str("CLAYER", &name);
    }
    ok()
}

/// `names` as a list of layer names (`None` when the call is for one `name`).
fn names_list(cmd: &str, p: &Value) -> Result<Option<Vec<String>>> {
    match p.get("names") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 10_000 => {
            let mut out: Vec<String> = Vec::new();
            for v in a {
                let n = v.as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`names` is a list of layer names"))?;
                if !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
                    out.push(n.to_string());
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(bad(cmd, "`names` is a list of 1 to 10000 layer names")),
    }
}

/// `layer.set {names, ...}`: the same properties on every named layer, or on none.
fn run_set_many(s: &mut Session, p: &Value, names: &[String]) -> Result<Value> {
    if p.get("newName").is_some_and(|v| !v.is_null()) || p.get("name").is_some_and(|v| !v.is_null()) {
        return Err(bad("layer.set", "`names` cannot be combined with `name` or `newName`: a rename is for one layer"));
    }
    let d = s.doc()?;
    let cur = d.header.str("CLAYER", "0");
    let lt = loaded_linetype(d, "layer.set", p)?;
    let freeze = p.get("frozen").and_then(Value::as_bool) == Some(true);
    let mut updates: Vec<(String, Layer)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for n in names {
        let mut l = d.layer(n).cloned().ok_or_else(|| bad("layer.set", format!("no layer `{n}`")))?;
        let old = l.name.clone();
        if freeze && old.eq_ignore_ascii_case(&cur) {
            skipped.push(old);
            continue;
        }
        apply(&mut l, p)?;
        if let Some(lt) = &lt {
            l.linetype = lt.clone();
        }
        updates.push((old, l));
    }
    let d = s.doc_mut()?;
    for (old, l) in &updates {
        if let Some(slot) = d.layer_mut(old) {
            *slot = l.clone();
        }
    }
    Ok(json!({ "changed": updates.len(), "skipped": skipped }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(names) = names_list("layer.set", p)? {
        return run_set_many(s, p, &names);
    }
    let name = str_param(p, "name").ok_or_else(|| bad("layer.set", "`name` is required"))?.to_string();
    let new_name = str_param(p, "newName").map(str::to_string);
    // Validate on a copy before borrowing the drawing mutably, so a rejected call
    // leaves the layer untouched and records no undo step.
    let d = s.doc()?;
    let cur = d.header.str("CLAYER", "0");
    let mut updated = d.layer(&name).cloned().ok_or_else(|| bad("layer.set", format!("no layer `{name}`")))?;
    let old = updated.name.clone();
    let lt = loaded_linetype(d, "layer.set", p)?;
    apply(&mut updated, p)?;
    if let Some(lt) = lt {
        updated.linetype = lt;
    }
    if p.get("frozen").and_then(Value::as_bool) == Some(true) && old.eq_ignore_ascii_case(&cur) {
        return Err(bad("layer.set", "cannot freeze the current layer"));
    }
    if let Some(nn) = &new_name {
        if old == "0" || old.eq_ignore_ascii_case("Defpoints") {
            return Err(bad("layer.set", "cannot rename layer 0 or Defpoints"));
        }
        if !valid_name(nn) {
            return Err(bad("layer.set", "invalid layer name"));
        }
        if !nn.eq_ignore_ascii_case(&old) && d.layer(nn).is_some() {
            return Err(bad("layer.set", format!("layer `{nn}` already exists")));
        }
    }
    let d = s.doc_mut()?;
    let l = d.layer_mut(&old).ok_or_else(|| bad("layer.set", format!("no layer `{name}`")))?;
    *l = updated;
    if let Some(nn) = new_name {
        l.name = nn.clone();
        // Re-point entities in model space, every layout and every block definition.
        for store in d.model_stores_mut() {
            repoint(store, &old, &nn);
        }
        for layout in &mut d.layouts {
            repoint(&mut layout.entities, &old, &nn);
        }
        for block in d.blocks.values_mut() {
            if block.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&old)) {
                repoint(&mut std::sync::Arc::make_mut(block).entities, &old, &nn);
            }
        }
        if cur.eq_ignore_ascii_case(&old) {
            d.header.set_str("CLAYER", &nn);
        }
        super::space::follow_rename(d, &old, &nn);
    }
    ok()
}

fn repoint(store: &mut aad_doc::EntityStore, old: &str, new: &str) {
    let hs: Vec<_> = store.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(old)).map(|e| e.handle).collect();
    for h in hs {
        store.modify(h, |e| e.common.layer = new.to_string());
    }
}

fn run_current(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layer.current", "`name` is required"))?.to_string();
    let n = {
        let d = s.doc()?;
        let l = d.layer(&name).ok_or_else(|| bad("layer.current", format!("no layer `{name}`")))?;
        // A layer nothing can be drawn on is not offered as the current one, and choosing a layer never
        // thaws or unlocks it as a side effect.
        if l.frozen {
            return Err(bad("layer.current", format!("layer `{}` is frozen: thaw it first", l.name)));
        }
        if l.locked {
            return Err(bad("layer.current", format!("layer `{}` is locked: unlock it first", l.name)));
        }
        // Already current: do not copy-on-write the drawing (shell chrome re-selects this often).
        if l.name.eq_ignore_ascii_case(&d.header.str("CLAYER", "0")) {
            return ok();
        }
        l.name.clone()
    };
    let d = s.doc_mut()?;
    prev_push(d);
    d.header.set_str("CLAYER", &n);
    ok()
}

fn prev_push(d: &mut aad_doc::Drawing) {
    let snap = serde_json::to_string(&d.layers).unwrap_or_default();
    d.header.set_str("AAD_LAYERP", &snap);
}

fn run_layerp(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    let snap = d.header.str("AAD_LAYERP", "");
    let layers: Vec<Layer> = serde_json::from_str(&snap).map_err(|_| bad("layerp", "No previous layer state."))?;
    d.layers = layers;
    Ok(json!({"message": "Restored previous layer states."}))
}

fn layer_has_objects(d: &aad_doc::Drawing, name: &str) -> bool {
    d.model_stores().flat_map(|s| s.iter()).any(|e| e.common.layer.eq_ignore_ascii_case(name))
        || d.layouts.iter().any(|l| l.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(name)))
        || d.blocks.values().any(|b| b.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(name)))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(names) = names_list("layer.delete", p)? {
        let d = s.doc()?;
        let cur = d.header.str("CLAYER", "0");
        let mut gone: Vec<String> = Vec::new();
        let mut skipped: Vec<Value> = Vec::new();
        for n in &names {
            let l = d.layer(n).ok_or_else(|| bad("layer.delete", format!("no layer `{n}`")))?;
            let why = if l.name == "0" || l.name.eq_ignore_ascii_case("Defpoints") || l.name.eq_ignore_ascii_case(&cur) {
                Some("layer 0, Defpoints and the current layer stay")
            } else if layer_has_objects(d, &l.name) {
                Some("it has objects on it")
            } else {
                None
            };
            match why {
                Some(w) => skipped.push(json!({ "name": l.name, "reason": w })),
                None => gone.push(l.name.clone()),
            }
        }
        if gone.is_empty() {
            return Err(bad("layer.delete", "none of the layers can be deleted (they are used, current or protected)"));
        }
        s.doc_mut()?.layers.retain(|l| !gone.iter().any(|g| g.eq_ignore_ascii_case(&l.name)));
        return Ok(json!({ "deleted": gone, "skipped": skipped }));
    }
    let name = str_param(p, "name").ok_or_else(|| bad("layer.delete", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    if name == "0" || name.eq_ignore_ascii_case("Defpoints") || d.header.str("CLAYER", "0").eq_ignore_ascii_case(&name) {
        return Err(bad("layer.delete", "cannot delete layer 0, Defpoints or the current layer"));
    }
    let used = d.model_stores().flat_map(|s| s.iter()).any(|e| e.common.layer.eq_ignore_ascii_case(&name))
        || d.layouts.iter().any(|l| l.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&name)))
        || d.blocks.values().any(|b| b.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&name)));
    if used {
        return Err(bad("layer.delete", "layer has objects on it"));
    }
    d.layers.retain(|l| !l.name.eq_ignore_ascii_case(&name));
    ok()
}

fn obj_layers(s: &Session, p: &Value) -> Result<Vec<String>> {
    let hs = targets(s, p)?;
    let d = s.doc()?;
    let mut v: Vec<String> = hs.iter().filter_map(|h| d.entity(*h).map(|e| e.common.layer.clone())).collect();
    v.sort();
    v.dedup();
    Ok(v)
}

/// Layer names from `name` / `names`, else from selected (or param) object handles.
fn named_or_obj_layers(s: &Session, p: &Value) -> Result<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    if let Some(name) = str_param(p, "name")
        && !name.is_empty()
    {
        v.push(name.to_string());
    }
    if let Some(arr) = p.get("names").and_then(Value::as_array) {
        for item in arr {
            if let Some(n) = item.as_str().filter(|s| !s.is_empty()) {
                v.push(n.to_string());
            }
        }
    }
    if v.is_empty() {
        return obj_layers(s, p);
    }
    v.sort();
    v.dedup();
    Ok(v)
}

fn run_laymcur(s: &mut Session, p: &Value) -> Result<Value> {
    let ls = obj_layers(s, p)?;
    let l = ls.first().cloned().ok_or_else(|| bad("laymcur", "select an object"))?;
    s.doc_mut()?.header.set_str("CLAYER", &l);
    Ok(json!({ "message": format!("{l} is now the current layer.") }))
}

fn run_laymch(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let layer = str_param(p, "layer").ok_or_else(|| bad("laymch", "`layer` is required"))?.to_string();
    let d = s.doc_mut()?;
    d.ensure_layer(&layer);
    for h in &hs {
        d.modify_entity(*h, |e| e.common.layer = layer.clone())?;
    }
    Ok(json!({ "changed": hs.len() }))
}

fn run_laycur(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = s.doc_mut()?;
    let cur = d.header.str("CLAYER", "0");
    for h in &hs {
        d.modify_entity(*h, |e| e.common.layer = cur.clone())?;
    }
    Ok(json!({ "changed": hs.len() }))
}

fn run_layiso(s: &mut Session, p: &Value) -> Result<Value> {
    let keep = named_or_obj_layers(s, p)?;
    if keep.is_empty() {
        return Err(bad("layiso", "give `name` / `names`, or select objects on the layers to isolate"));
    }
    let d = s.doc_mut()?;
    prev_push(d);
    // Re-isolating must restore the pre-iso table first, or the snapshot would be the thinned one.
    if let Ok(prior) = serde_json::from_str::<Vec<Layer>>(&d.header.str("AAD_LAYISO", "")) {
        d.layers = prior;
    }
    d.header.set_str("AAD_LAYISO", &serde_json::to_string(&d.layers).unwrap_or_default());
    for l in &mut d.layers {
        if !keep.iter().any(|k| k.eq_ignore_ascii_case(&l.name)) {
            l.on = false;
        }
    }
    if let Some(k) = keep.first() {
        d.header.set_str("CLAYER", k);
    }
    Ok(json!({ "message": format!("Layer(s) {} have been isolated.", keep.join(", ")) }))
}

fn run_layuniso(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    let snap = d.header.str("AAD_LAYISO", "");
    let layers: Vec<Layer> = serde_json::from_str(&snap).map_err(|_| bad("layuniso", "No isolated layers."))?;
    d.layers = layers;
    ok()
}

fn set_obj_layers(s: &mut Session, p: &Value, f: fn(&mut Layer)) -> Result<Value> {
    let ls = obj_layers(s, p)?;
    let d = s.doc_mut()?;
    let cur = d.header.str("CLAYER", "0");
    prev_push(d);
    for n in &ls {
        if let Some(l) = d.layer_mut(n) {
            let was_frozen = l.frozen;
            f(l);
            if l.frozen && !was_frozen && l.name.eq_ignore_ascii_case(&cur) {
                l.frozen = false;
            }
        }
    }
    s.set_selection(Vec::new());
    Ok(json!({ "layers": ls }))
}

fn all_layers(s: &mut Session, f: fn(&mut Layer)) -> Result<Value> {
    let d = s.doc_mut()?;
    prev_push(d);
    d.layers.iter_mut().for_each(f);
    ok()
}

fn run_state_save(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.save", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    let layers = d.layers.clone();
    d.layer_states.retain(|st| st.name != name);
    d.layer_states.push(aad_doc::LayerState { name, layers });
    ok()
}

fn run_state_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.restore", "`name` is required"))?;
    let d = s.doc_mut()?;
    let st = d.layer_states.iter().find(|st| st.name == name).cloned().ok_or_else(|| bad("layerstate.restore", "no such layer state"))?;
    for saved in st.layers {
        if let Some(l) = d.layer_mut(&saved.name) {
            *l = saved;
        }
    }
    ok()
}

fn run_state_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let states: Vec<Value> = d.layer_states.iter().map(|st| json!({ "name": st.name, "layers": st.layers.len() })).collect();
    let msg = if states.is_empty() {
        "No saved layer states.".to_string()
    } else {
        d.layer_states.iter().map(|st| st.name.clone()).collect::<Vec<_>>().join(", ")
    };
    Ok(json!({ "states": states, "message": msg }))
}

fn run_state_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.delete", "`name` is required"))?;
    let d = s.doc_mut()?;
    if !d.layer_states.iter().any(|st| st.name == name) {
        return Err(bad("layerstate.delete", "no such layer state"));
    }
    d.layer_states.retain(|st| st.name != name);
    ok()
}

fn run_laytrans(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laytrans";
    let map = p.get("map").and_then(Value::as_object).ok_or_else(|| bad(CMD, "`map` (object of from→to names) is required"))?;
    if map.is_empty() || map.len() > 10_000 {
        return Err(bad(CMD, "`map` needs 1 to 10000 entries"));
    }
    let create = bool_or(p, "createMissing", true);
    let mut renamed = 0usize;
    let mut created = 0usize;
    let mut moved = 0usize;
    // Apply as successive layer.set renames / entity remaps.
    for (from, to_v) in map {
        let to = to_v.as_str().ok_or_else(|| bad(CMD, "each map value must be a layer name"))?.trim();
        if from.contains('|') || to.contains('|') {
            return Err(bad(CMD, "xref-dependent layers cannot be translated"));
        }
        if from.eq_ignore_ascii_case(to) {
            continue;
        }
        let d = s.doc()?;
        let has_from = d.layer(from).is_some();
        let has_to = d.layer(to).is_some();
        if !has_from {
            continue;
        }
        if !has_to {
            if !create {
                return Err(bad(CMD, format!("no layer `{to}`")));
            }
            s.execute("layer.new", &json!({ "name": to }))?;
            created += 1;
        }
        // Merge objects onto `to`, then delete `from` when it is not 0/Defpoints.
        if from != "0" && !from.eq_ignore_ascii_case("Defpoints") {
            let r = s.execute("laymrg", &json!({ "from": [from], "to": to }))?;
            moved += r["moved"].as_u64().unwrap_or(0) as usize;
            renamed += 1;
        } else {
            // Cannot merge away 0: just retarget is impossible — skip.
        }
    }
    Ok(json!({ "renamed": renamed, "created": created, "moved": moved }))
}

fn run_laywalk(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laywalk";
    const STATE: &str = "*LAYWALK*";
    if bool_or(p, "restore", false) {
        let _ = s.execute("layerstate.restore", &json!({ "name": STATE }));
        let _ = s.execute("layerstate.delete", &json!({ "name": STATE }));
        return Ok(json!({ "restored": true }));
    }
    // First call saves the current layer visibility.
    let has = s.doc()?.layer_states.iter().any(|st| st.name == STATE);
    if !has {
        s.execute("layerstate.save", &json!({ "name": STATE }))?;
    }
    let names: Vec<String> = if let Some(a) = p.get("layers").and_then(Value::as_array) {
        a.iter().filter_map(Value::as_str).take(10_000).map(str::to_string).collect()
    } else {
        s.doc()?.layers.iter().map(|l| l.name.clone()).collect()
    };
    if names.is_empty() {
        return Err(bad(CMD, "no layers to walk"));
    }
    let step = p.get("step").and_then(Value::as_i64).unwrap_or(1);
    let cur = s.doc()?.header.str("CLAYER", "0");
    let idx = names.iter().position(|n| n.eq_ignore_ascii_case(&cur)).unwrap_or(0);
    let len = names.len() as i64;
    let next = ((idx as i64 + step).rem_euclid(len)) as usize;
    let Some(focus) = names.get(next).cloned() else {
        return Err(bad(CMD, "layer list changed"));
    };
    // Turn all off, focus on.
    let d = s.doc_mut()?;
    for l in &mut d.layers {
        l.on = l.name.eq_ignore_ascii_case(&focus);
    }
    d.header.set_str("CLAYER", &focus);
    Ok(json!({ "current": focus, "index": next, "count": names.len(), "on": [focus] }))
}

fn run_state_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").ok_or_else(|| bad("layerstate.rename", "`from` is required"))?;
    let to = str_param(p, "to").map(str::trim).ok_or_else(|| bad("layerstate.rename", "`to` is required"))?.to_string();
    if !valid_name(&to) {
        return Err(bad("layerstate.rename", "invalid name"));
    }
    let d = s.doc_mut()?;
    if d.layer_states.iter().any(|st| st.name == to) {
        return Err(bad("layerstate.rename", format!("layer state `{to}` already exists")));
    }
    let st = d.layer_states.iter_mut().find(|st| st.name == from).ok_or_else(|| bad("layerstate.rename", "no such layer state"))?;
    st.name = to;
    ok()
}

// LAYMRG and LAYDEL.
mod merge;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn layer_states_save_list_restore_delete() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": 1 })).unwrap();
        s.execute("layerstate.save", &json!({ "name": "base" })).unwrap();
        s.execute("layer.set", &json!({ "name": "A", "on": false, "color": 3 })).unwrap();
        let l = s.execute("layerstate.list", &json!({})).unwrap();
        assert_eq!(l["states"][0]["name"], "base");
        s.execute("layerstate.restore", &json!({ "name": "base" })).unwrap();
        let a = s.doc().unwrap().layer("A").unwrap().clone();
        assert!(a.on);
        assert_eq!(a.color, aad_color::Color::Index(1));
        s.execute("layerstate.rename", &json!({ "from": "base", "to": "start" })).unwrap();
        assert!(s.execute("layerstate.delete", &json!({ "name": "base" })).is_err());
        s.execute("layerstate.delete", &json!({ "name": "start" })).unwrap();
        assert!(s.execute("layerstate.list", &json!({})).unwrap()["states"].as_array().unwrap().is_empty());
        // `used` flags in the layer list.
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        let v = s.execute("layer", &json!({})).unwrap();
        let used: Vec<bool> = v["layers"].as_array().unwrap().iter().map(|l| l["used"].as_bool().unwrap()).collect();
        assert!(used.contains(&true) && used.contains(&false));
    }

    #[test]
    fn rejected_layer_set_changes_nothing() {
        let mut s = Session::new();
        // Layer 0 is current; freezing it is rejected and must not apply the colour either.
        let before = s.doc().unwrap().layer("0").unwrap().clone();
        let undo = s.state().unwrap().undo.len();
        assert!(s.execute("layer.set", &json!({ "name": "0", "frozen": true, "color": 3 })).is_err());
        assert_eq!(s.doc().unwrap().layer("0").unwrap(), &before);
        assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step for a rejected call");
        // Same for a rejected rename.
        assert!(s.execute("layer.set", &json!({ "name": "0", "newName": "X", "color": 3 })).is_err());
        assert_eq!(s.doc().unwrap().layer("0").unwrap(), &before);
        assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step for a rejected call");
        // A valid colour-only edit of the current layer still applies.
        s.execute("layer.set", &json!({ "name": "0", "color": 3 })).unwrap();
        assert_ne!(s.doc().unwrap().layer("0").unwrap(), &before);
    }

    #[test]
    fn making_the_current_layer_again_does_not_dirty_the_drawing() {
        let mut s = Session::new();
        assert!(!s.state().unwrap().is_dirty());
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        assert!(!s.state().unwrap().is_dirty(), "re-selecting CLAYER must not copy-on-write");
        s.execute("layer.new", &json!({ "name": "A" })).unwrap();
        assert!(s.state().unwrap().is_dirty());
        // Snapshot as saved so the next check is about layer.current alone.
        let st = s.state_mut().unwrap();
        st.saved = st.doc.clone();
        assert!(!s.state().unwrap().is_dirty());
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        assert!(!s.state().unwrap().is_dirty());
        s.execute("layer.current", &json!({ "name": "A" })).unwrap();
        assert!(s.state().unwrap().is_dirty());
        assert_eq!(s.doc().unwrap().header.str("CLAYER", ""), "A");
    }

    #[test]
    fn chrome_reselects_do_not_dirty_a_new_imperial_drawing() {
        let mut s = Session::new();
        assert!(!s.state().unwrap().is_dirty());
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        s.execute("floor.current", &json!({})).unwrap();
        s.execute("floor.current", &json!({ "name": "Ground" })).unwrap();
        s.execute("drawing.units", &json!({ "units": "in", "mode": "metadata" })).unwrap();
        s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
        assert!(!s.state().unwrap().is_dirty(), "shell chrome must not mark a blank drawing dirty");
    }

    #[test]
    fn delete_and_purge_keep_layers_used_by_block_definitions() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": 1 })).unwrap();
        s.execute("layer.current", &json!({ "name": "A" })).unwrap();
        let l = s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
        s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        s.execute("insert", &json!({ "name": "B", "at": [5, 5] })).unwrap();
        assert!(s.execute("layer.delete", &json!({ "name": "A" })).is_err());
        s.execute("purge", &json!({})).unwrap();
        assert!(s.doc().unwrap().layer("A").is_some());
    }

    #[test]
    fn rename_repoints_paper_space_and_block_entities() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": "red", "current": true })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("selectall", &json!({})).unwrap();
        s.execute("block", &json!({ "name": "Part", "base": [0, 0], "keep": "delete" })).unwrap();
        s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("layer.set", &json!({ "name": "A", "newName": "B" })).unwrap();
        let d = s.doc().unwrap();
        let on_layer = |name: &str| {
            let paper = d.layouts.iter().flat_map(|l| l.entities.iter());
            let blocks = d.blocks.values().flat_map(|b| b.entities.iter());
            paper.chain(blocks).filter(|e| e.common.layer == name).count()
        };
        assert_eq!(on_layer("A"), 0);
        assert_eq!(on_layer("B"), 2);
    }
}

//! Properties, layers and blocks for the WinUI shell.
//!
//! One JSON document, tagged with the drawing id and revision, so the shell rebuilds the panels
//! only when the drawing changed. Edits go back through engine commands, not through this snapshot.

use aad_color::Color;
use aad_engine::Session;
use aad_engine::doc::{Entity, EntityKind, Handle, Lineweight};
use serde_json::{Value, json};

const MAX_LAYERS: usize = 400;
const MAX_BLOCKS: usize = 400;
const MAX_SCAN: usize = 50_000;
const MAX_BLOCK_REFS: usize = 200;
const MAX_BLOCK_ATTRS: usize = 50;

/// The Classic panels: current properties, the layer table and block definitions.
/// `hover` is the entity under the cursor (not in the selection); the shell shows it when nothing is selected.
pub fn panels_json(session: &Session, hover: Option<Handle>) -> String {
    let Ok(state) = session.state() else {
        return json!({
            "uid": 0, "revision": 0, "selection": 0, "hover": null,
            "focusLayer": "", "focusBlock": "",
            "properties": [], "layers": [], "blocks": []
        })
        .to_string();
    };
    let doc = &state.doc;
    let selection = state.selection.len();
    let current = doc.header.str("CLAYER", "0");
    // Prefer the selection for tab focus; fall back to the hovered entity.
    let focus = focus_entity(doc, &state.selection, hover);
    let focus_layer = focus.as_ref().map(|e| e.common.layer.clone()).unwrap_or_default();
    let focus_block = focus
        .as_ref()
        .and_then(|e| match &e.kind {
            EntityKind::Insert(i) => Some(i.block.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let hover_handle = hover.filter(|h| !state.selection.iter().any(|s| s == h));
    // While a sheet set is the active tab, only the layers and blocks it maps are listed (unless the
    // drawing asks for all of them): Electrical lists its layers and symbols, Plumbing its own. The
    // current layer always stays in the list.
    let show_all = aad_engine::cmd::sheetstd::show_all(doc);
    let scope = if show_all { None } else { doc.active_sheet_ref() };
    let on_level = aad_engine::cmd::sheetstd::show_level_only(doc).then(|| aad_engine::cmd::layer::layers_on_level(doc));
    let mut layers_truncated = false;
    let mut layers = Vec::new();
    for layer in doc.layers.iter().take(MAX_LAYERS + 1) {
        if scope.is_some_and(|s| !s.maps_layer(&layer.name) && !layer.name.eq_ignore_ascii_case(&current)) {
            continue;
        }
        if on_level.as_ref().is_some_and(|used| !used.contains(&layer.name.to_ascii_lowercase()) && !layer.name.eq_ignore_ascii_case(&current)) {
            continue;
        }
        if layers.len() == MAX_LAYERS {
            layers_truncated = true;
            break;
        }
        let rgb = layer.color.resolve(Color::Index(7), Color::Index(7));
        let on_focus = !focus_layer.is_empty() && layer.name.eq_ignore_ascii_case(&focus_layer);
        layers.push(json!({
            "name": layer.name,
            "on": layer.on,
            "frozen": layer.frozen,
            "locked": layer.locked,
            "color": rgb.hex(),
            "colorName": layer.color.name(),
            "current": layer.name.eq_ignore_ascii_case(&current),
            "focus": on_focus,
            "selected": on_focus && selection > 0,
            "hovered": on_focus && selection == 0 && hover_handle.is_some(),
        }));
    }
    let refs = reference_handles(doc);
    let mut blocks_truncated = false;
    let blocks_total = doc.blocks.values().filter(|b| !b.anonymous).count();
    let mut blocks: Vec<&aad_engine::doc::Block> =
        doc.blocks.values().map(|b| b.as_ref()).filter(|b| !b.anonymous && scope.is_none_or(|s| s.maps_block(&b.name))).collect();
    blocks.sort_by_key(|block| block.name.to_ascii_lowercase());
    if blocks.len() > MAX_BLOCKS {
        blocks.truncate(MAX_BLOCKS);
        blocks_truncated = true;
    }
    let blocks: Vec<Value> = blocks
        .iter()
        .map(|b| {
            let (count, handles) =
                refs.get(&b.name).map_or((0u32, Vec::new()), |(total, list)| (*total, list.iter().map(|h| h.hex()).collect::<Vec<_>>()));
            let attributes: Vec<String> = b
                .entities
                .iter()
                .filter_map(|entity| match &entity.kind {
                    EntityKind::AttDef(attr) => Some(format!("{} = {}", attr.tag, attr.text.value)),
                    _ => None,
                })
                .take(MAX_BLOCK_ATTRS)
                .collect();
            let on_focus = !focus_block.is_empty() && b.name.eq_ignore_ascii_case(&focus_block);
            json!({
                "name": b.name,
                "description": b.description,
                "entities": b.entities.len(),
                "references": count,
                "handles": handles,
                "attributes": attributes,
                "focus": on_focus,
                "selected": on_focus && selection > 0,
                "hovered": on_focus && selection == 0 && hover_handle.is_some(),
            })
        })
        .collect();
    let hover_quick = hover_handle.and_then(|h| {
        let mut q = quick(doc, &[h]);
        if let Value::Object(ref mut o) = q {
            o.insert("source".into(), json!("hover"));
            o.insert("readOnly".into(), json!(true));
        }
        if q.is_null() { None } else { Some(q) }
    });
    let layiso = !doc.header.str("AAD_LAYISO", "").is_empty();
    let cecolor = Color::from_aci(i16::try_from(doc.header.i64("CECOLOR", 256)).unwrap_or(256));
    json!({
        "uid": state.uid,
        "revision": state.revision,
        "selection": selection,
        "hover": hover_handle.map(|h| h.hex()),
        "focusLayer": focus_layer,
        "focusBlock": focus_block,
        "layiso": layiso,
        "cecolor": cecolor.name(),
        "celtype": doc.header.str("CELTYPE", "ByLayer"),
        "properties": properties(doc, &state.selection),
        "scope": {
            "sheetSet": doc.active_sheet_ref().map(|s| s.name.clone()),
            "all": show_all,
            "layersShown": layers.len(),
            "layersTotal": doc.layers.len(),
            "blocksShown": blocks.len(),
            "blocksTotal": blocks_total,
        },
        "layers": layers,
        "layersTruncated": layers_truncated,
        "blocks": blocks,
        "blocksTruncated": blocks_truncated,
        // Smart blocks that can be inserted: the active tab's own ones in a sheet view, all in design.
        // The reinforced concrete blocks are tools on the Architecture bar (Architecture > RCC Structure), not entries of the library.
        "library": aad_engine::cmd::smartblock::library_entries(doc, scope)
            .iter()
            .filter(|b| b["discipline"] != "rcc")
            .take(200)
            .map(|b| json!({ "id": b["id"], "title": b["title"], "discipline": b["discipline"], "layer": b["layer"], "description": b["description"] }))
            .collect::<Vec<_>>(),
        "takeoffPhases": aad_engine::cmd::takeoff_phases_json(doc),
        "quick": quick(doc, &state.selection),
        "hoverQuick": hover_quick,
    })
    .to_string()
}

fn focus_entity<'a>(doc: &'a aad_engine::doc::Drawing, selection: &[Handle], hover: Option<Handle>) -> Option<&'a Entity> {
    let handle = selection.first().copied().or(hover)?;
    doc.entity(handle).map(|e| e.as_ref())
}

fn properties(doc: &aad_engine::doc::Drawing, selection: &[Handle]) -> Vec<Value> {
    let header = &doc.header;
    if selection.is_empty() {
        let color = Color::from_aci(i16::try_from(header.i64("CECOLOR", 256)).unwrap_or(256));
        let weight = Lineweight::from_dxf(i16::try_from(header.i64("CELWEIGHT", -1)).unwrap_or(-1));
        let scale = header.f64("CELTSCALE", 1.0);
        let scale = if scale.is_finite() { format!("{scale:.4}") } else { "0".into() };
        return vec![
            row("Selection", "No selection"),
            row("Color", &color.name()),
            row("Layer", &header.str("CLAYER", "0")),
            row("Linetype", &header.str("CELTYPE", "ByLayer")),
            row("Lineweight", &weight.name()),
            row("Linetype scale", &scale),
        ];
    }
    let count = selection.len();
    let mut kinds: Vec<&str> = Vec::new();
    let mut layer = String::new();
    for handle in selection.iter().copied().take(32) {
        let Some(entity) = doc.entity(handle) else { continue };
        let kind = entity.kind.type_name();
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
        if layer.is_empty() {
            layer = entity.common.layer.clone();
        }
    }
    let kind = match kinds.as_slice() {
        [] => "Unknown",
        [one] => one,
        _ => "Mixed",
    };
    let mut rows = vec![row("Selection", &format!("{count} selected")), row("Type", kind), row("Layer", &layer)];
    // A single object the layer mapper moved keeps the name of the layer it came from.
    if let [one] = selection
        && let Some(source) = aad_engine::cmd::layermap::source_layer_of(doc, *one)
    {
        rows.push(row("Source layer", &source));
    }
    rows
}

fn row(label: &str, value: &str) -> Value {
    json!({ "label": label, "value": value })
}

/// The Quick Properties palette: common fields for the selection, and geometry when one object is selected.
fn quick(doc: &aad_engine::doc::Drawing, selection: &[Handle]) -> Value {
    if selection.is_empty() {
        return Value::Null;
    }
    let ents: Vec<_> = selection.iter().take(200).filter_map(|handle| doc.entity(*handle)).collect();
    let Some(first) = ents.first() else { return Value::Null };
    let count = selection.len();
    let type_name = first.kind.type_name();
    let same_type = ents.iter().all(|entity| entity.kind.type_name() == type_name);
    let title = if count == 1 {
        type_name.to_string()
    } else if same_type {
        format!("{type_name} ({count})")
    } else {
        format!("All ({count})")
    };
    let handles: Vec<String> = selection.iter().take(200).map(|handle| handle.hex()).collect();
    let layer = shared(ents.iter().map(|entity| entity.common.layer.as_str()));
    let color = shared_owned(ents.iter().map(|entity| entity.common.color.name()));
    let linetype = shared(ents.iter().map(|entity| entity.common.linetype.as_str()));
    let lineweight = shared_owned(ents.iter().map(|entity| entity.common.lineweight.name()));
    let transparency = shared_owned(ents.iter().map(|entity| entity.common.transparency.name()));
    let ltscale = shared_owned(ents.iter().map(|entity| format!("{:.4}", entity.common.ltscale)));
    let mut linetypes = vec!["ByLayer".to_string(), "ByBlock".to_string()];
    for pattern in doc.linetypes.iter().take(64) {
        if pattern.name != "ByLayer" && pattern.name != "ByBlock" {
            linetypes.push(pattern.name.clone());
        }
    }
    let mut fields = Vec::new();
    let kind = shared_owned(ents.iter().map(|entity| {
        entity
            .common
            .building
            .as_ref()
            .map(|b| b.kind.as_str().to_string())
            .or_else(|| match &entity.kind {
                EntityKind::Insert(insert) => doc.blocks.get(&insert.block).and_then(|b| b.building.as_ref()).map(|b| b.kind.as_str().to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }));
    let ifc = shared_owned(ents.iter().map(|entity| {
        entity
            .common
            .building
            .as_ref()
            .and_then(|b| b.ifc_type.clone())
            .or_else(|| match &entity.kind {
                EntityKind::Insert(insert) => doc.blocks.get(&insert.block).and_then(|b| b.building.as_ref()).and_then(|b| b.ifc_type.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }));
    if !kind.is_empty() || count == 1 {
        text_field(&mut fields, "buildingKind", "Building kind", &kind);
        text_field(&mut fields, "ifcType", "IFC type", &ifc);
    }
    let openings: Vec<_> = ents
        .iter()
        .filter_map(|entity| match &entity.kind {
            EntityKind::Insert(insert) if insert.is_opening() => Some(insert),
            _ => None,
        })
        .collect();
    if !openings.is_empty() && openings.len() == ents.len() {
        opening_fields(&mut fields, doc, &openings, count == 1);
    } else if count == 1 {
        match &first.kind {
            EntityKind::Line(line) => {
                let delta = line.b.xy() - line.a.xy();
                number_field(&mut fields, "length", "Length", delta.len());
                number_field(&mut fields, "angle", "Angle", delta.angle().to_degrees());
            }
            EntityKind::Circle(circle) => {
                number_field(&mut fields, "centerX", "Center X", circle.center.x);
                number_field(&mut fields, "centerY", "Center Y", circle.center.y);
                number_field(&mut fields, "radius", "Radius", circle.radius);
                number_field(&mut fields, "diameter", "Diameter", circle.radius * 2.0);
            }
            EntityKind::Arc(arc) => {
                number_field(&mut fields, "radius", "Radius", arc.radius);
                number_field(&mut fields, "startAngle", "Start angle", arc.start.to_degrees());
                number_field(&mut fields, "endAngle", "End angle", arc.end.to_degrees());
            }
            EntityKind::LwPolyline(poly) => {
                number_field(&mut fields, "width", "Global width", poly.const_width);
            }
            EntityKind::Text(text) => {
                text_field(&mut fields, "text", "Contents", &text.value);
                number_field(&mut fields, "height", "Height", text.height);
                number_field(&mut fields, "rotation", "Rotation", text.rotation.to_degrees());
            }
            EntityKind::MText(text) => {
                text_field(&mut fields, "text", "Contents", &text.contents);
                number_field(&mut fields, "height", "Text height", text.height);
            }
            EntityKind::Insert(insert) if !insert.is_opening() => {
                fields.push(json!({ "label": "Name", "value": insert.block, "text": true }));
                number_field(&mut fields, "rotation", "Rotation", insert.rotation.to_degrees());
                number_field(&mut fields, "scale", "Scale", insert.scale.x);
                fields.extend(aad_engine::smart_block_fields(doc, insert));
            }
            EntityKind::Dimension(dim) => {
                text_field(&mut fields, "textOverride", "Text override", &dim.text);
            }
            EntityKind::Hatch(hatch) => {
                text_field(&mut fields, "pattern", "Pattern", &hatch.pattern);
                number_field(&mut fields, "scale", "Scale", hatch.scale);
                number_field(&mut fields, "angle", "Angle", hatch.angle.to_degrees());
            }
            _ => {}
        }
    }
    // Hatch pattern choices when every selected object is a hatch (combo in the shell).
    let patterns = if same_type && type_name == "Hatch" {
        aad_engine::doc::library::standard_patterns().into_iter().map(|p| p.name.to_string()).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let pattern = if same_type && type_name == "Hatch" {
        shared_owned(ents.iter().filter_map(|entity| match &entity.kind {
            EntityKind::Hatch(h) => Some(h.pattern.clone()),
            _ => None,
        }))
    } else {
        String::new()
    };
    json!({
        "title": title,
        "handles": handles,
        "layer": layer,
        "color": color,
        "linetype": linetype,
        "lineweight": lineweight,
        "transparency": transparency,
        "ltscale": ltscale,
        "linetypes": linetypes,
        "layers": doc.layers.iter().take(400).map(|layer| layer.name.clone()).collect::<Vec<_>>(),
        "patterns": patterns,
        "pattern": pattern,
        "fields": fields,
    })
}

fn shared<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut first: Option<&str> = None;
    for value in values {
        match first {
            None => first = Some(value),
            Some(seen) if seen != value => return "*VARIES*".into(),
            _ => {}
        }
    }
    first.unwrap_or("").to_string()
}

fn shared_owned(values: impl Iterator<Item = String>) -> String {
    let mut first: Option<String> = None;
    for value in values {
        match &first {
            None => first = Some(value),
            Some(seen) if seen != &value => return "*VARIES*".into(),
            _ => {}
        }
    }
    first.unwrap_or_default()
}

fn opening_fields(fields: &mut Vec<Value>, doc: &aad_engine::doc::Drawing, openings: &[&aad_engine::doc::Insert], one: bool) {
    let kind = shared(openings.iter().map(|i| if i.attrib_value("OKIND") == Some("door") { "Door" } else { "Window" }));
    fields.push(json!({ "label": "Kind", "value": kind }));
    let door = openings.iter().all(|i| i.attrib_value("OKIND") == Some("door"));
    let window = openings.iter().all(|i| i.attrib_value("OKIND") == Some("window"));
    if door || window {
        let choices: &[&str] = if door { &["single", "double", "sliding", "folding"] } else { &["fixed", "casement", "sliding", "ventilator"] };
        let current = shared(openings.iter().map(|i| i.attrib_value("OTYPE").unwrap_or("")));
        choice_field(fields, "openingType", "Type", &current, choices);
    } else {
        fields.push(json!({ "label": "Type", "value": "*VARIES*" }));
    }
    let widths: Vec<f64> = openings.iter().filter_map(|i| i.opening_width()).collect();
    match shared_f64(&widths) {
        Some(w) if widths.len() == openings.len() => number_field(fields, "openingWidth", "Width", w),
        _ => varies_field(fields, "openingWidth", "Width"),
    }
    push_metres(fields, "sill", "Sill height (m)", openings, "OSILL");
    let mode = shared(
        openings.iter().map(|i| if i.attrib_value("OLINTEL").is_some_and(|m| m.eq_ignore_ascii_case("custom")) { "custom" } else { "standard" }),
    );
    choice_field(fields, "lintelMode", "Lintel lock", &mode, &["standard", "custom"]);
    let lintel = aad_engine::standard_lintel(doc);
    if mode == "standard" {
        let fitted: Vec<f64> = openings
            .iter()
            .map(|i| {
                let sill = i.attrib_value("OSILL").and_then(|v| v.parse().ok()).filter(|n: &f64| n.is_finite()).unwrap_or(0.0);
                lintel - sill
            })
            .collect();
        match shared_f64(&fitted) {
            Some(n) if n > 0.0 => number_field(fields, "openingHeight", "Opening height (m)", n),
            _ => varies_field(fields, "openingHeight", "Opening height (m)"),
        }
    } else {
        push_metres(fields, "openingHeight", "Opening height (m)", openings, "OHEIGHT");
    }
    if mode == "standard" {
        fields.push(json!({ "label": "Head height (m)", "value": format!("{lintel:.4}") }));
    } else if mode == "custom" {
        push_metres(fields, "head", "Head height (m)", openings, "OHEAD");
    } else {
        fields.push(json!({ "label": "Head height (m)", "value": "*VARIES*" }));
    }
    if one && let Some(i) = openings.first() {
        number_field(fields, "rotation", "Rotation", i.rotation.to_degrees());
    }
}

fn push_metres(fields: &mut Vec<Value>, key: &str, label: &str, openings: &[&aad_engine::doc::Insert], tag: &str) {
    let values: Vec<f64> = openings.iter().filter_map(|i| i.attrib_value(tag).and_then(|v| v.parse().ok())).filter(|n: &f64| n.is_finite()).collect();
    match shared_f64(&values) {
        Some(n) if values.len() == openings.len() => number_field(fields, key, label, n),
        _ => varies_field(fields, key, label),
    }
}

fn varies_field(fields: &mut Vec<Value>, key: &str, label: &str) {
    fields.push(json!({ "key": key, "label": label, "value": "*VARIES*", "text": false }));
}

fn shared_f64(values: &[f64]) -> Option<f64> {
    let first = *values.first()?;
    values.iter().all(|v| (*v - first).abs() < 1e-4).then_some(first)
}

fn choice_field(fields: &mut Vec<Value>, key: &str, label: &str, value: &str, choices: &[&str]) {
    fields.push(json!({ "key": key, "label": label, "value": value, "text": true, "choices": choices }));
}

fn number_field(fields: &mut Vec<Value>, key: &str, label: &str, value: f64) {
    if value.is_finite() {
        fields.push(json!({ "key": key, "label": label, "value": format!("{value:.4}"), "text": false }));
    }
}

fn text_field(fields: &mut Vec<Value>, key: &str, label: &str, value: &str) {
    let value: String = value.chars().take(4_096).collect();
    fields.push(json!({ "key": key, "label": label, "value": value, "text": true }));
}

/// Count of inserts and up to [`MAX_BLOCK_REFS`] handles per block name.
fn reference_handles(doc: &aad_engine::doc::Drawing) -> std::collections::BTreeMap<String, (u32, Vec<Handle>)> {
    let mut refs = std::collections::BTreeMap::new();
    let entities = std::iter::once(&doc.model).chain(doc.layouts.iter().map(|layout| &layout.entities)).flat_map(|space| space.iter());
    for (seen, entity) in entities.enumerate() {
        if seen == MAX_SCAN {
            break;
        }
        if let EntityKind::Insert(insert) = &entity.kind {
            let entry = refs.entry(insert.block.clone()).or_insert_with(|| (0u32, Vec::new()));
            entry.0 = entry.0.saturating_add(1);
            if entry.1.len() < MAX_BLOCK_REFS {
                entry.1.push(entity.handle);
            }
        }
    }
    refs
}

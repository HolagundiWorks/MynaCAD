//! More tools from the Holgundi Consulting Works AutoLISP toolkit in the AccelDraw repository:
//! VHLAYERS (the Indian-architecture layer set), LAYERMAP (move everything from one layer onto
//! another and purge the old one), DELETEAREATEXT (remove "Area: ..." labels) and POLYAREA
//! (number closed polylines and draw an area table with running totals).

use aad_color::Color;
use aad_doc::{Drawing, EntityKind, HAlign, Handle, Layer, Lineweight, VAlign};
use aad_geom::{Polyline, Vec2};
use serde_json::{Value, json};

use super::draw::make_text;
use super::hcw::{add_on, drawing_units, ensure_layer};
use super::helpers::{lwpoly, rect_vertices};
use super::*;
use crate::{Result, Session};

/// The VH layer set: name, ACI colour, linetype, lineweight (hundredths of mm), plots.
pub(crate) const VH_LAYERS: &[(&str, i16, &str, u16, bool)] = &[
    ("A_WALL_CUT", 7, "Continuous", 50, true),
    ("A_WALL_LOW", 8, "Continuous", 25, true),
    ("A_WALL_EXIST", 9, "Continuous", 18, true),
    ("A_WALL_DEMO", 1, "DASHED", 18, true),
    ("A_WALL_BEYOND", 8, "HIDDEN", 13, true),
    ("A_DOOR", 3, "Continuous", 25, true),
    ("A_WINDOW", 4, "Continuous", 18, true),
    ("A_GLASS", 151, "Continuous", 13, true),
    ("A_STAIRS", 2, "Continuous", 25, true),
    ("A_RAILING", 8, "Continuous", 18, true),
    ("A_FLOOR_PATTERN", 254, "Continuous", 9, true),
    ("A_CEILING", 253, "Continuous", 9, true),
    ("A_FURNITURE", 30, "Continuous", 13, true),
    ("A_JOINERY", 32, "Continuous", 18, true),
    ("A_SANITARY", 140, "Continuous", 18, true),
    ("A_HATCH_CUT", 250, "Continuous", 9, true),
    ("A_HATCH_SURFACE", 253, "Continuous", 5, true),
    ("A_SITE", 7, "Continuous", 35, true),
    ("A_SETBACK", 1, "DASHED", 13, true),
    ("A_PARKING", 6, "Continuous", 18, true),
    ("A_LANDSCAPE", 94, "Continuous", 13, true),
    ("A_VAASTU", 210, "DASHED", 9, true),
    ("A_FSI_FAR", 5, "Continuous", 13, true),
    ("A_RAINWATER", 150, "Continuous", 18, true),
    ("A_SEPTIC_STP", 145, "Continuous", 18, true),
    ("A_SUMP_OHT", 141, "Continuous", 18, true),
    ("A_SERVICES_SHAFT", 6, "Continuous", 18, true),
    ("A_SECURITY", 1, "Continuous", 18, true),
    ("A_SIGNAGE", 30, "Continuous", 18, true),
    ("S_COLUMN", 7, "Continuous", 50, true),
    ("S_BEAM", 8, "Continuous", 35, true),
    ("S_SLAB", 9, "Continuous", 25, true),
    ("S_FOOTING", 30, "Continuous", 35, true),
    ("S_REBAR", 1, "Continuous", 18, true),
    ("S_GRID", 2, "CENTER", 13, true),
    ("S_LEVEL", 6, "Continuous", 13, true),
    ("E_LIGHT", 2, "Continuous", 18, true),
    ("E_SWITCH", 40, "Continuous", 13, true),
    ("E_POWER", 30, "Continuous", 18, true),
    ("E_CONDUIT", 2, "DASHED", 13, true),
    ("P_WATER_SUPPLY", 5, "Continuous", 18, true),
    ("P_DRAINAGE", 4, "DASHED", 18, true),
    ("P_FIXTURE", 140, "Continuous", 18, true),
    ("F_FIRE", 1, "Continuous", 25, true),
    ("H_HVAC", 151, "Continuous", 18, true),
    ("H_DRAIN", 5, "DASHED", 13, true),
    ("AN_DIM", 7, "Continuous", 13, true),
    ("AN_TEXT", 7, "Continuous", 13, true),
    ("AN_ROOM_NAME", 7, "Continuous", 18, true),
    ("AN_LEVEL", 6, "Continuous", 13, true),
    ("AN_TAG_DOOR", 3, "Continuous", 13, true),
    ("AN_TAG_WINDOW", 4, "Continuous", 13, true),
    ("AN_GRID", 2, "Continuous", 13, true),
    ("AN_REVISION", 1, "Continuous", 18, true),
    ("AN_AREA", 5, "Continuous", 13, true),
    ("AN_TITLE", 7, "Continuous", 25, true),
    ("P_SHADOW", 251, "Continuous", 5, true),
    ("P_POCHE", 252, "Continuous", 9, true),
    ("P_CONTEXT", 253, "Continuous", 5, true),
    ("P_TREE", 94, "Continuous", 9, true),
    ("P_PEOPLE", 252, "Continuous", 9, true),
    ("P_CAR", 252, "Continuous", 9, true),
    ("P_TEXTURE", 253, "Continuous", 5, true),
    ("P_SCREEN", 254, "Continuous", 5, true),
    ("A_DETAIL_2D", 7, "Continuous", 18, true),
    ("A_DETAIL_3D", 8, "Continuous", 13, true),
    ("A_WATERPROOF", 5, "Continuous", 18, true),
    ("A_FINISH", 30, "Continuous", 13, true),
    ("A_EXPANDED", 6, "Continuous", 13, true),
    ("X_GUIDE", 8, "Continuous", 5, false),
    ("X_REFERENCE", 9, "Continuous", 5, false),
    ("X_TRACE", 251, "Continuous", 5, false),
    ("X_RENDER", 253, "Continuous", 5, false),
    ("X_COORDINATION", 1, "Continuous", 13, false),
    ("X_CHECK", 1, "DASHED", 13, false),
];

const MAX_MAPPINGS: usize = 10_000;
const MAX_POLYLINES: usize = 100_000;
const TABLE_LAYER: (&str, i16) = ("AREA_TABLE", 4);
const SQ_FT_PER_SQ_M: f64 = 10.763_910_416_709_722;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("vhlayers", "VH Layer Set", run_vhlayers)
            .menu(&["Architecture", "VH Layers", "Create VH Layer Set"])
            .params("{} → {created, updated} (75 layers: A_ architecture, S_ structure, E_ electrical, P_ F_ H_ services, AN_ annotation, P_ presentation, X_ non-plotting working layers; existing layers get the standard look; current layer A_WALL_CUT)"),
        CommandSpec::new("layermap", "Map Layers", run_layermap)
            .menu(&["Format", "Layer Tools", "Map Layers"])
            .params("{suggest?: bool (list candidate maps for the non-standard layers, change nothing), map: {\"WALL-A\": \"A-WALL\", ...} (old layer -> target layer), create?: bool (make a missing target), purge?: bool (remove each old layer left empty)} → {moved, purged, skippedLocked, mapped: [{from, to, moved}]} (objects on locked layers stay)"),
        CommandSpec::new("deleteareatext", "Delete Area Text", run_delete_area_text)
            .menu(&["Architecture", "Text Tools", "Delete Area Text"])
            .params("{handles? (default: all text in the current space)} → {deleted} (text and multiline text that read \"Area: ...\")"),
        CommandSpec::new("polyarea", "Number Polylines and Area Table", run_polyarea)
            .menu(&["Architecture", "Rooms and Areas", "Polyline Area Table"])
            .params("{handles?, prefix?, textHeight?, at?: [x, y] (draws the table when given), unit?: sqm|sqft (default sqm), units?, close?: bool (default true: open polylines are closed first)} → {rows: [{label, area}], total, unit, closed, table}"),
    ]
}

/// Make `name` exist with the given look, or update it. Returns whether it was created.
pub(crate) fn apply_layer(d: &mut Drawing, name: &str, aci: i16, linetype: &str, lw: u16, plot: bool) -> bool {
    if d.linetype(linetype).is_none()
        && let Some(lt) = aad_doc::library::standard_linetypes_for(d).into_iter().find(|l| l.name.eq_ignore_ascii_case(linetype))
    {
        d.linetypes.push(lt);
    }
    let lt = d.linetype(linetype).map(|t| t.name.clone()).unwrap_or_else(|| "Continuous".into());
    let created = d.layer(name).is_none();
    if created {
        d.layers.push(Layer::new(name));
    }
    if let Some(l) = d.layers.iter_mut().find(|l| l.name == name) {
        l.color = Color::from_aci(aci);
        l.linetype = lt;
        l.lineweight = Lineweight::Mm100(lw);
        l.plot = plot;
    }
    created
}

fn run_vhlayers(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    let (mut created, mut updated) = (0, 0);
    for (name, aci, lt, lw, plot) in VH_LAYERS {
        if apply_layer(d, name, *aci, lt, *lw, *plot) {
            created += 1;
        } else {
            updated += 1;
        }
    }
    d.header.set_str("CLAYER", "A_WALL_CUT");
    Ok(json!({ "created": created, "updated": updated }))
}

/// Whether anything still sits on `layer`: an object in any space or a block definition.
fn layer_in_use(d: &Drawing, layer: &str) -> bool {
    let stores = d.model_stores().chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    stores.flat_map(|st| st.iter()).any(|e| e.common.layer == layer)
}

fn run_layermap(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "suggest", false) {
        // Candidates only: nothing moves. The person edits the list and sends it back as `map`.
        let d = s.doc()?;
        return Ok(json!({ "origin": super::geodb::origin_of(d), "suggestions": super::geodb::suggest_layer_map(d) }));
    }
    let map = p
        .get("map")
        .and_then(Value::as_object)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| bad("layermap", "`map` is required: {\"old layer\": \"target layer\", ...}"))?;
    if map.len() > MAX_MAPPINGS {
        return Err(bad("layermap", format!("at most {MAX_MAPPINGS} mappings")));
    }
    let create = bool_or(p, "create", false);
    let purge = bool_or(p, "purge", false);
    // Check everything before moving anything.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (from, to) in map {
        let to = to.as_str().filter(|t| !t.is_empty()).ok_or_else(|| bad("layermap", format!("the target for \"{from}\" must be a layer name")))?;
        if s.doc()?.layer(from).is_none() {
            return Err(bad("layermap", format!("no layer \"{from}\"")));
        }
        if s.doc()?.layer(to).is_none() && !create {
            return Err(bad("layermap", format!("no target layer \"{to}\"; pass create: true to make it")));
        }
        if from != to {
            pairs.push((from.clone(), to.to_string()));
        }
    }
    let (mut total, mut skipped, mut purged) = (0usize, 0usize, 0usize);
    let mut mapped = Vec::new();
    for (from, to) in pairs {
        if create {
            s.doc_mut()?.ensure_layer(&to);
        }
        let d = s.doc_mut()?;
        let hs: Vec<Handle> = std::iter::once(&d.model)
            .chain(d.layouts.iter().map(|l| &l.entities))
            .flat_map(|st| st.iter())
            .filter(|e| e.common.layer == from)
            .map(|e| e.handle)
            .collect();
        let source_locked = d.layer(&from).is_some_and(|l| l.locked);
        let mut moved = 0usize;
        if source_locked {
            skipped += hs.len();
        } else {
            for h in hs {
                if d.modify_entity(h, |e| e.common.layer = to.clone()).is_ok() {
                    moved += 1;
                }
            }
        }
        total += moved;
        mapped.push(json!({ "from": from, "to": to, "moved": moved }));
        let current = d.header.str("CLAYER", "0");
        if purge && !source_locked && from != "0" && from != current && !layer_in_use(d, &from) {
            d.layers.retain(|l| l.name != from);
            purged += 1;
        }
    }
    if total > 0 {
        // A foreign drawing that went through the mapper is no longer waiting for it (geodb.apply checks).
        s.doc_mut()?.project.insert(super::geodb::LAYER_MAP_DONE_KEY.into(), "1".into());
    }
    Ok(json!({ "moved": total, "purged": purged, "skippedLocked": skipped, "mapped": mapped }))
}

/// `Area: 12.50 m2`, in any case, with anything after the number; multiline text with its
/// formatting codes removed.
fn is_area_text(kind: &EntityKind) -> bool {
    let raw = match kind {
        EntityKind::Text(t) => t.value.clone(),
        EntityKind::MText(m) => m.contents.replace("\\P", "").replace("\\p", "").replace(['{', '}'], ""),
        _ => return false,
    };
    raw.trim_start().get(..6).is_some_and(|p| p.eq_ignore_ascii_case("area: "))
}

fn run_delete_area_text(s: &mut Session, p: &Value) -> Result<Value> {
    let explicit = p.get("handles").is_some_and(|v| !v.is_null());
    let mut hs = targets(s, p)?;
    if hs.is_empty() && !explicit {
        let space = s.space();
        hs = s.doc()?.space(&space).map(|st| st.iter().map(|e| e.handle).collect()).unwrap_or_default();
    }
    let doomed: Vec<Handle> = {
        let d = s.doc()?;
        hs.into_iter().filter(|h| d.entity(*h).is_some_and(|e| is_area_text(&e.kind))).filter(|h| !curves::is_locked(s, *h)).collect()
    };
    let d = s.doc_mut()?;
    let deleted = doomed.into_iter().filter(|h| d.remove_entity(*h).is_some()).count();
    Ok(json!({ "deleted": deleted }))
}

/// The centre of mass of a closed polygon, or the middle of its bounds when it has no area.
fn centre_of(vertices: &[Vec2]) -> Vec2 {
    let n = vertices.len();
    let (mut a2, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (p, q) = (vertices[i], vertices[(i + 1) % n]);
        let cross = p.x * q.y - q.x * p.y;
        a2 += cross;
        cx += (p.x + q.x) * cross;
        cy += (p.y + q.y) * cross;
    }
    if a2.abs() > 1e-12 && a2.is_finite() {
        return Vec2::new(cx / (3.0 * a2), cy / (3.0 * a2));
    }
    let b = Polyline { vertices: vertices.iter().map(|v| aad_geom::PolyVertex::new(*v)).collect(), closed: true }.bounds();
    Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0)
}

fn coloured_text(s: &mut Session, layer: &str, at: Vec2, text: &str, height: f64, aci: i16) -> Result<()> {
    let kind = make_text(s, at, text, height, 0.0, (HAlign::Center, VAlign::Middle));
    let h = add_on(s, layer, kind)?;
    s.doc_mut()?.modify_entity(h, |e| e.common.color = Color::from_aci(aci))?;
    Ok(())
}

fn run_polyarea(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() || hs.len() > MAX_POLYLINES {
        return Err(bad("polyarea", format!("select between 1 and {MAX_POLYLINES} polylines")));
    }
    let unit_key = str_param(p, "unit").unwrap_or("sqm").to_ascii_lowercase();
    let (unit, to_unit) = match unit_key.as_str() {
        "sqm" => ("sq m", 1.0),
        "sqft" => ("sq ft", SQ_FT_PER_SQ_M),
        _ => return Err(bad("polyarea", "`unit` must be sqm or sqft")),
    };
    let u = drawing_units(s.doc()?, "polyarea", p)?;
    let k = u.metres() * u.metres() * to_unit;
    let height = f64_or(p, "textHeight", s.doc()?.header.f64("TEXTSIZE", 2.5));
    if !(height.is_finite() && height > 0.0) {
        return Err(bad("polyarea", "`textHeight` must be positive"));
    }
    let prefix = str_param(p, "prefix").unwrap_or("").chars().take(60).collect::<String>();
    let close = bool_or(p, "close", true);
    let polys: Vec<Handle> = {
        let d = s.doc()?;
        hs.into_iter().filter(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::LwPolyline(_)))).collect()
    };
    if polys.is_empty() {
        return Err(bad("polyarea", "select lightweight polylines"));
    }
    let mut closed = 0usize;
    if close {
        for h in &polys {
            if curves::is_locked(s, *h) {
                continue;
            }
            s.doc_mut()?.modify_entity(*h, |e| {
                if let EntityKind::LwPolyline(pl) = &mut e.kind
                    && !pl.closed
                {
                    pl.closed = true;
                    closed += 1;
                }
            })?;
        }
    }
    // (label, area in the output unit, centre)
    let mut rows: Vec<(String, f64, Vec2)> = Vec::new();
    for (i, h) in polys.iter().enumerate() {
        let Some(EntityKind::LwPolyline(pl)) = s.doc()?.entity(*h).map(|e| e.kind.clone()) else { continue };
        let poly = Polyline { vertices: pl.vertices.clone(), closed: true };
        let area = poly.area().abs() * k;
        let at = centre_of(&pl.vertices.iter().map(|v| v.p).collect::<Vec<_>>());
        if area.is_finite() && at.x.is_finite() && at.y.is_finite() {
            rows.push((format!("{prefix}{}", i + 1), area, at));
        }
    }
    let total: f64 = rows.iter().map(|r| r.1).sum();
    let layer = s.doc()?.header.str("CLAYER", "0");
    for (label, _, at) in &rows {
        let kind = make_text(s, *at, label, height, 0.0, (HAlign::Center, VAlign::Middle));
        add_on(s, &layer, kind)?;
    }
    let mut table = false;
    if p.get("at").is_some_and(|v| !v.is_null()) {
        let at = point_req("polyarea", p, "at")?;
        ensure_layer(s.doc_mut()?, TABLE_LAYER.0, TABLE_LAYER.1, "Continuous", 18, "Polyline area table");
        draw_table(s, at, height, &prefix, &rows, total, unit, u.metres())?;
        table = true;
    }
    s.echo(format!("POLYAREA: {} polylines, total {total:.4} {unit}.", rows.len()));
    Ok(json!({
        "rows": rows.iter().map(|r| json!({ "label": r.0, "area": r.1 })).collect::<Vec<_>>(),
        "total": total,
        "unit": unit,
        "closed": closed,
        "table": table,
    }))
}

#[allow(clippy::too_many_arguments)]
fn draw_table(s: &mut Session, at: Vec2, h: f64, prefix: &str, rows: &[(String, f64, Vec2)], total: f64, unit: &str, metres: f64) -> Result<()> {
    let longest = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(1) as f64;
    let col = (h * 14.0).max(longest * h * 0.8);
    let (row_h, head_h) = (h * 2.8, h * 3.5);
    let (x0, x1, x2, x3) = (at.x, at.x + col, at.x + col * 2.0, at.x + col * 3.0);
    let layer = TABLE_LAYER.0;
    let rect = |s: &mut Session, xa: f64, ya: f64, xb: f64, yb: f64| {
        add_on(s, layer, lwpoly(rect_vertices(Vec2::new(xa, ya), Vec2::new(xb, yb)), true)).map(|_| ())
    };
    let mid = |a: f64, b: f64| (a + b) / 2.0;
    let mut top = at.y;
    let mut bottom = top - head_h;
    rect(s, x0, top, x3, bottom)?;
    coloured_text(s, layer, Vec2::new(mid(x0, x3), mid(top, bottom)), "POLYLINE AREA TABLE", h * 1.4, 4)?;
    top = bottom;
    bottom = top - row_h * 0.9;
    rect(s, x0, top, x3, bottom)?;
    let units_name = if (metres - 0.001).abs() < 1e-12 {
        "Millimetres"
    } else if (metres - 0.01).abs() < 1e-12 {
        "Centimetres"
    } else if (metres - 1.0).abs() < 1e-12 {
        "Metres"
    } else if (metres - 0.3048).abs() < 1e-12 {
        "Feet"
    } else {
        "Inches"
    };
    let sub = format!(
        "Drawing units: {units_name}   |   Areas in: {unit}{}",
        if prefix.is_empty() { String::new() } else { format!("   |   Prefix: {prefix}") }
    );
    coloured_text(s, layer, Vec2::new(mid(x0, x3), mid(top, bottom)), &sub, h * 0.75, 3)?;
    top = bottom;
    bottom = top - row_h;
    for (a, b) in [(x0, x1), (x1, x2), (x2, x3)] {
        rect(s, a, top, b, bottom)?;
    }
    coloured_text(s, layer, Vec2::new(mid(x0, x1), mid(top, bottom)), "LABEL", h, 4)?;
    coloured_text(s, layer, Vec2::new(mid(x1, x2), mid(top, bottom)), &format!("AREA ({unit})"), h, 4)?;
    coloured_text(s, layer, Vec2::new(mid(x2, x3), mid(top, bottom)), &format!("CUM. SUM ({unit})"), h, 4)?;
    let mut running = 0.0;
    for (label, area, _) in rows {
        running += area;
        top = bottom;
        bottom = top - row_h;
        for (a, b) in [(x0, x1), (x1, x2), (x2, x3)] {
            rect(s, a, top, b, bottom)?;
        }
        coloured_text(s, layer, Vec2::new(mid(x0, x1), mid(top, bottom)), label, h, 7)?;
        coloured_text(s, layer, Vec2::new(mid(x1, x2), mid(top, bottom)), &format!("{area:.4}"), h, 7)?;
        coloured_text(s, layer, Vec2::new(mid(x2, x3), mid(top, bottom)), &format!("{running:.4}"), h, 7)?;
    }
    top = bottom;
    bottom = top - row_h * 1.4;
    rect(s, x0, top, x3, bottom)?;
    rect(s, x0, top, x2, bottom)?;
    coloured_text(s, layer, Vec2::new(mid(x0, x2), mid(top, bottom)), "TOTAL AREA", h * 1.1, 1)?;
    coloured_text(s, layer, Vec2::new(mid(x2, x3), mid(top, bottom)), &format!("{total:.4} {unit}"), h * 1.1, 1)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> String {
        let r = s.execute("rectang", &json!({ "p1": [x, y], "p2": [x + w, y + h] })).unwrap();
        r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
    }

    fn on_layer(s: &mut Session, h: &str, layer: &str) {
        let d = s.doc_mut().unwrap();
        d.ensure_layer(layer);
        d.modify_entity(Handle::parse_hex(h).unwrap(), |e| e.common.layer = layer.to_string()).unwrap();
    }

    #[test]
    fn the_vh_table_is_75_unique_layers_and_six_do_not_plot() {
        assert_eq!(VH_LAYERS.len(), 75);
        assert_eq!(VH_LAYERS.iter().filter(|l| !l.4).count(), 6);
        let mut names: Vec<_> = VH_LAYERS.iter().map(|l| l.0).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 75);
    }

    #[test]
    fn vhlayers_creates_then_repairs_and_makes_the_wall_layer_current() {
        let mut s = Session::new();
        let r = s.execute("vhlayers", &json!({})).unwrap();
        assert_eq!(r["created"], json!(75));
        assert!(!s.doc().unwrap().layer("X_GUIDE").unwrap().plot);
        s.doc_mut().unwrap().layers.iter_mut().find(|l| l.name == "A_DOOR").unwrap().color = Color::from_aci(9);
        s.execute("vhlayers", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().layer("A_DOOR").unwrap().color, Color::from_aci(3));
        assert_eq!(s.doc().unwrap().header.str("CLAYER", ""), "A_WALL_CUT");
    }

    #[test]
    fn layermap_moves_objects_skips_locked_and_purges_empty_sources() {
        let mut s = Session::new();
        s.execute("hcwlayers", &json!({})).unwrap();
        let a = rect(&mut s, 0.0, 0.0, 1.0, 1.0);
        let b = rect(&mut s, 5.0, 0.0, 1.0, 1.0);
        on_layer(&mut s, &a, "WALL-A");
        on_layer(&mut s, &b, "WALL-EXT");
        assert!(s.execute("layermap", &json!({})).is_err());
        assert!(s.execute("layermap", &json!({ "map": { "NOPE": "A-WALL" } })).is_err());
        assert!(s.execute("layermap", &json!({ "map": { "WALL-A": "NO-TARGET" } })).is_err(), "a missing target needs create");
        s.doc_mut().unwrap().layers.iter_mut().find(|l| l.name == "WALL-EXT").unwrap().locked = true;
        let r = s.execute("layermap", &json!({ "map": { "WALL-A": "A-WALL", "WALL-EXT": "A-WALL" }, "purge": true })).unwrap();
        assert_eq!(r["moved"], json!(1));
        assert_eq!(r["skippedLocked"], json!(1));
        assert_eq!(r["purged"], json!(1));
        assert!(s.doc().unwrap().layer("WALL-A").is_none());
        assert!(s.doc().unwrap().layer("WALL-EXT").is_some(), "a locked source stays");
        let r = s.execute("layermap", &json!({ "map": { "WALL-EXT": "NEW-ONE" }, "create": true })).unwrap();
        assert_eq!(r["moved"], json!(0), "the locked layer's objects still do not move");
        assert!(s.doc().unwrap().layer("NEW-ONE").is_some());
    }

    #[test]
    fn delete_area_text_removes_only_area_labels() {
        let mut s = Session::new();
        s.execute("text", &json!({ "at": [0, 0], "text": "Area: 12.50 m2", "height": 1 })).unwrap();
        s.execute("text", &json!({ "at": [0, 5], "text": "area: 3.00 sq ft", "height": 1 })).unwrap();
        s.execute("text", &json!({ "at": [0, 9], "text": "BEDROOM", "height": 1 })).unwrap();
        s.execute("mtext", &json!({ "at": [0, 12], "text": "Area: 9 m2", "height": 1 })).unwrap();
        let r = s.execute("deleteareatext", &json!({})).unwrap();
        assert_eq!(r["deleted"], json!(3));
        assert_eq!(s.doc().unwrap().model.iter().count(), 1);
        assert_eq!(s.execute("deleteareatext", &json!({ "handles": [] })).unwrap()["deleted"], json!(0));
    }

    #[test]
    fn polyarea_numbers_closes_open_polylines_and_totals() {
        let mut s = Session::new();
        s.doc_mut().unwrap().header.set_i64("INSUNITS", 6);
        let a = rect(&mut s, 0.0, 0.0, 3.0, 2.0);
        let b = rect(&mut s, 10.0, 0.0, 2.0, 2.0);
        s.doc_mut()
            .unwrap()
            .modify_entity(Handle::parse_hex(&b).unwrap(), |e| {
                if let EntityKind::LwPolyline(pl) = &mut e.kind {
                    pl.closed = false;
                }
            })
            .unwrap();
        assert!(s.execute("polyarea", &json!({ "handles": [] })).is_err());
        assert!(s.execute("polyarea", &json!({ "handles": [a], "unit": "acres" })).is_err());
        let before = s.doc().unwrap().model.iter().count();
        let r = s.execute("polyarea", &json!({ "handles": [a, b], "prefix": "R-", "textHeight": 0.3 })).unwrap();
        assert_eq!(r["closed"], json!(1));
        assert!((r["total"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(r["rows"][1]["label"], json!("R-2"));
        assert_eq!(s.doc().unwrap().model.iter().count(), before + 2, "two labels, no table without a point");
        let ft = s.execute("polyarea", &json!({ "handles": [a], "unit": "sqft", "textHeight": 0.3, "at": [0, -5] })).unwrap();
        assert!((ft["total"].as_f64().unwrap() - 6.0 * SQ_FT_PER_SQ_M).abs() < 1e-6);
        assert_eq!(ft["table"], json!(true));
        assert!(s.doc().unwrap().layer("AREA_TABLE").is_some());
    }

    #[test]
    fn centre_of_a_square_and_of_a_degenerate_polygon() {
        let sq = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)];
        let c = centre_of(&sq);
        assert!((c.x - 2.0).abs() < 1e-9 && (c.y - 2.0).abs() < 1e-9);
        let line = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 2.0)];
        let c = centre_of(&line);
        assert!((c.x - 2.0).abs() < 1e-9 && (c.y - 1.0).abs() < 1e-9);
    }
}

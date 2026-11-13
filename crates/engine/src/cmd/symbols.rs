//! Levels, section and elevation markers, north and slope arrows (the kit's HCWLEVEL,
//! HCWLEVELSCHED, HCWSECTION, HCWELEV, HCWNORTH and HCWSLOPE).
//!
//! Level marks keep their value in the project data so `level.schedule` can list them. Not
//! implemented: drawing a building section from the plan (HCWSECTIONDRAW).

use aad_doc::{HAlign, VAlign};
use aad_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::draw::make_text;
use super::hcw::{add_on, ensure_layer};
use super::helpers::{circle, line, lwpoly};
use super::takeoff::{table, table_ctx};
use super::*;
use crate::annot::{self as ua};
use crate::{Result, Session};

const LEVEL: &str = "S-LEVEL";
const SECTION: &str = "BP-SECTION";
const ELEV: &str = "BP-ELEVATION";
const SYMB: &str = "AN-SYMB";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("level.mark", "Level Mark", run_level)
            .menu(&["Architecture", "Levels and Sections", "Level Mark"])
            .alias(&["hcwlevel"])
            .params("{at: [x,y], value?: metres | floor?: floor name (its finished level), offset?: metres} → {value, label}"),
        CommandSpec::new("level.schedule", "Level Schedule", run_level_schedule)
            .menu(&["Architecture", "Levels and Sections", "Level Schedule"])
            .alias(&["hcwlevelsched"])
            .params("{at?: [x,y]} → {levels}"),
        CommandSpec::new("section.marker", "Section Marker", run_section)
            .menu(&["Architecture", "Levels and Sections", "Section Marker"])
            .alias(&["section", "hcwsection"])
            .params("{p1, p2, label?: \"A\", side?: left|right (the way the section looks)}"),
        CommandSpec::new("elevation.marker", "Elevation Marker", run_elevation)
            .menu(&["Architecture", "Levels and Sections", "Elevation Marker"])
            .alias(&["elevation", "hcwelev"])
            .params("{at: [x,y], number: text, sheet?: text, angle?: degrees (the way you look)}"),
        CommandSpec::new("north.arrow", "North Arrow", run_north)
            .menu(&["Architecture", "Symbols", "North Arrow"])
            .alias(&["hcwnorth"])
            .params("{at: [x,y], angle?: degrees (default 90, up the sheet), size?}"),
        CommandSpec::new("slope.arrow", "Slope Arrow", run_slope)
            .menu(&["Architecture", "Symbols", "Slope Arrow"])
            .alias(&["hcwslope"])
            .params("{p1, p2, text: \"1:100\" | \"2%\"}"),
    ]
}

fn mm(s: &Session) -> f64 {
    s.doc().ok().and_then(ua::drawing_unit).map_or(1.0, |u| 0.001 / u.metres())
}

fn put(s: &mut Session, layer: &str, at: Vec2, t: &str, h: f64, j: (HAlign, VAlign), rot: f64) -> Result<()> {
    let kind = make_text(s, at, t, h, rot, j);
    add_on(s, layer, kind)?;
    Ok(())
}

fn fmt_level(v: f64) -> String {
    if v.abs() < 5e-4 { "±0.000".to_string() } else { format!("{v:+.3}") }
}

pub(super) fn run_level(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("level.mark", p, "at")?;
    let offset = f64_or(p, "offset", 0.0);
    let value = match (p.get("value").and_then(Value::as_f64), str_param(p, "floor")) {
        (Some(v), _) => v + offset,
        (None, Some(name)) => {
            let lv = project::get(s.doc()?, "floors");
            let mut level = 0.0;
            let mut found = None;
            for f in lv.as_array().into_iter().flatten() {
                if f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                    found = Some(level);
                    break;
                }
                level += f["floorToFloor"].as_f64().unwrap_or(0.0);
            }
            found.ok_or_else(|| bad("level.mark", format!("no floor named `{name}`")))? + offset
        }
        (None, None) => return Err(bad("level.mark", "give `value` or `floor`")),
    };
    if !value.is_finite() {
        return Err(bad("level.mark", "the level must be a number"));
    }
    let m = mm(s);
    let h = 125.0 * m;
    ensure_layer(s.doc_mut()?, LEVEL, 2, "Continuous", 18, "Spot levels");
    // An inverted triangle on a short datum line, with the value beside it.
    let w = h * 1.4;
    add_on(
        s,
        LEVEL,
        lwpoly(
            vec![PolyVertex::new(Vec2::new(at.x - w / 2.0, at.y + w)), PolyVertex::new(Vec2::new(at.x + w / 2.0, at.y + w)), PolyVertex::new(at)],
            true,
        ),
    )?;
    add_on(s, LEVEL, line(Vec2::new(at.x - w * 1.2, at.y + w), Vec2::new(at.x + w * 3.5, at.y + w)))?;
    let label = fmt_level(value);
    put(s, LEVEL, Vec2::new(at.x + w * 0.8, at.y + w + h * 0.3), &label, h, (HAlign::Left, VAlign::Bottom), 0.0)?;
    let mut list = match project::get(s.doc()?, "levelMarks") {
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    list.push(json!({ "at": [at.x, at.y], "value": value, "label": label }));
    project::set(s.doc_mut()?, "levelMarks", &Value::Array(list));
    Ok(json!({ "value": value, "label": label }))
}

fn run_level_schedule(s: &mut Session, p: &Value) -> Result<Value> {
    let list = match project::get(s.doc()?, "levelMarks") {
        Value::Array(a) if !a.is_empty() => a,
        _ => return Err(bad("level.schedule", "no level marks: place some with `level.mark`")),
    };
    let mut rows: Vec<(f64, Vec<String>)> =
        list.iter().filter_map(|l| Some((l["value"].as_f64()?, vec![l["label"].as_str()?.to_string()]))).collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (_, mut r))| {
            r.insert(0, format!("L{}", i + 1));
            r
        })
        .collect();
    let m = mm(s);
    let ext = s.doc()?.extents(&aad_doc::Space::Model);
    let at = point_param(p, "at").unwrap_or(if ext.is_empty() { Vec2::ZERO } else { Vec2::new(ext.max.x + 1000.0 * m, ext.max.y) });
    table(s, &table_ctx(125.0 * m), at, "LEVEL SCHEDULE", &["Mark".to_string(), "Level (m)".to_string()], &rows)?;
    Ok(json!({ "levels": rows }))
}

fn head(s: &mut Session, layer: &str, at: Vec2, label: &str, r: f64) -> Result<()> {
    add_on(s, layer, circle(at, r))?;
    put(s, layer, at, label, r * 1.1, (HAlign::Center, VAlign::Middle), 0.0)
}

pub(super) fn run_section(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, b) = (point_req("section.marker", p, "p1")?, point_req("section.marker", p, "p2")?);
    if a.near(b, 1e-9) {
        return Err(bad("section.marker", "the two points coincide"));
    }
    let label = str_param(p, "label").unwrap_or("A");
    if label.chars().count() > 3 {
        return Err(bad("section.marker", "`label` is at most three characters"));
    }
    let look = match str_param(p, "side") {
        None | Some("left") => 1.0,
        Some("right") => -1.0,
        Some(_) => return Err(bad("section.marker", "`side` must be left or right")),
    };
    let m = mm(s);
    let r = 300.0 * m;
    ensure_layer(s.doc_mut()?, SECTION, 30, "PHANTOM2", 35, "Section markers");
    let marker = add_on(s, SECTION, line(a, b))?;
    let d = (b - a).normalized();
    let n = Vec2::new(-d.y, d.x) * look;
    for (end, out) in [(a, d * -1.0), (b, d)] {
        let c = end + out * r;
        head(s, SECTION, c, label, r)?;
        // The arrow shows the way the section looks.
        add_on(s, SECTION, line(end, end + n * (r * 2.5)))?;
        add_on(
            s,
            SECTION,
            lwpoly(
                vec![
                    PolyVertex::new(end + n * (r * 2.5)),
                    PolyVertex::new(end + n * (r * 1.8) + d * (r * 0.3)),
                    PolyVertex::new(end + n * (r * 1.8) - d * (r * 0.3)),
                ],
                true,
            ),
        )?;
    }
    Ok(json!({ "label": label, "handle": marker.hex() }))
}

pub(super) fn run_elevation(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("elevation.marker", p, "at")?;
    let number = str_param(p, "number").ok_or_else(|| bad("elevation.marker", "`number` is required"))?;
    let sheet = str_param(p, "sheet").unwrap_or("-");
    let a = f64_or(p, "angle", 90.0).to_radians();
    let m = mm(s);
    let r = 300.0 * m;
    ensure_layer(s.doc_mut()?, ELEV, 30, "Continuous", 25, "Elevation markers");
    let marker = add_on(s, ELEV, circle(at, r))?;
    add_on(s, ELEV, line(at - Vec2::new(r, 0.0), at + Vec2::new(r, 0.0)))?;
    put(s, ELEV, at + Vec2::new(0.0, r * 0.5), number, r * 0.7, (HAlign::Center, VAlign::Middle), 0.0)?;
    put(s, ELEV, at - Vec2::new(0.0, r * 0.5), sheet, r * 0.7, (HAlign::Center, VAlign::Middle), 0.0)?;
    // The pointer: a triangle on the circle pointing the way you look.
    let d = Vec2::new(a.cos(), a.sin());
    let nrm = Vec2::new(-d.y, d.x);
    add_on(
        s,
        ELEV,
        lwpoly(
            vec![PolyVertex::new(at + d * (r * 1.8)), PolyVertex::new(at + d * r + nrm * (r * 0.4)), PolyVertex::new(at + d * r - nrm * (r * 0.4))],
            true,
        ),
    )?;
    Ok(json!({ "number": number, "sheet": sheet, "handle": marker.hex() }))
}

fn run_north(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("north.arrow", p, "at")?;
    let a = f64_or(p, "angle", 90.0).to_radians();
    let size = f64_or(p, "size", 1500.0 * mm(s));
    if !(size.is_finite() && size > 0.0) {
        return Err(bad("north.arrow", "`size` must be positive"));
    }
    ensure_layer(s.doc_mut()?, SYMB, 7, "Continuous", 18, "North point and symbols");
    let d = Vec2::new(a.cos(), a.sin());
    let n = Vec2::new(-d.y, d.x);
    add_on(s, SYMB, circle(at, size / 2.0))?;
    add_on(
        s,
        SYMB,
        lwpoly(
            vec![
                PolyVertex::new(at + d * size * 0.9),
                PolyVertex::new(at - d * size * 0.35 + n * size * 0.25),
                PolyVertex::new(at - d * size * 0.1),
                PolyVertex::new(at - d * size * 0.35 - n * size * 0.25),
            ],
            true,
        ),
    )?;
    put(s, SYMB, at + d * size * 1.2, "N", size * 0.35, (HAlign::Center, VAlign::Middle), 0.0)?;
    Ok(json!({ "angle": a.to_degrees() }))
}

fn run_slope(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, b) = (point_req("slope.arrow", p, "p1")?, point_req("slope.arrow", p, "p2")?);
    let t = str_param(p, "text").ok_or_else(|| bad("slope.arrow", "`text` is required (for example 1:100 or 2%)"))?;
    if a.near(b, 1e-9) {
        return Err(bad("slope.arrow", "the two points coincide"));
    }
    let m = mm(s);
    ensure_layer(s.doc_mut()?, SYMB, 7, "Continuous", 18, "North point and symbols");
    add_on(s, SYMB, line(a, b))?;
    let d = (b - a).normalized();
    let n = Vec2::new(-d.y, d.x);
    let head = 250.0 * m;
    add_on(
        s,
        SYMB,
        lwpoly(
            vec![PolyVertex::new(b), PolyVertex::new(b - d * head * 2.0 + n * head * 0.5), PolyVertex::new(b - d * head * 2.0 - n * head * 0.5)],
            true,
        ),
    )?;
    let mid = (a + b) * 0.5 + n * (125.0 * m);
    let mut rot = d.y.atan2(d.x);
    if rot.abs() > std::f64::consts::FRAC_PI_2 {
        rot += std::f64::consts::PI;
    }
    put(s, SYMB, mid, t, 125.0 * m, (HAlign::Center, VAlign::Bottom), rot)?;
    Ok(json!({ "text": t }))
}

//! PDF underlay for take-off: `underlay.attach`, `underlay.calibrate`, `underlay.list`, `underlay.remove`.
//!
//! A PDF plan is read through the application's PDF reader (vector paths only) and drawn as ordinary polylines on its own
//! layer, so the existing snaps, selection and `takeoff.*` tools work on it with no new entity type, and it saves and
//! reopens with the drawing. The PDF has no known scale, so it arrives at `scale` (drawing millimetres per paper
//! millimetre, default 1) and `underlay.calibrate` fixes it from two picked points and the real distance between them.
//! Text, images and scanned (raster-only) PDFs are not drawn; the result says what was left out.

use aad_doc::{Common, EntityKind, Handle, Space};
use aad_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::project;
use super::*;
use crate::{Result, Session};

/// The project-data key that lists the attached underlays.
const KEY: &str = "underlay";
const DEFAULT_LAYER: &str = "PDF-UNDERLAY";
/// Most underlay polylines one attach draws, and the largest PDF file read.
const MAX_ENTITIES: usize = 200_000;
const MAX_FILE: u64 = 256 * 1024 * 1024;
/// Millimetres in one PDF point.
const MM_PER_PT: f64 = 25.4 / 72.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("underlay.attach", "Attach PDF Underlay", run_attach)
            .menu(&["Architecture", "Take-off", "Attach PDF Underlay"])
            .params("{path: PDF file, page?: 1, at?: [x, y] (lower left of the page, default [0, 0]), scale?: drawing mm per paper mm (default 1), layer?: \"PDF-UNDERLAY\"} → {layer, page, paths, widthMm, heightMm, skipped, truncated, warnings}"),
        CommandSpec::new("underlay.calibrate", "Calibrate PDF Underlay", run_calibrate)
            .menu(&["Architecture", "Take-off", "Calibrate PDF Underlay"])
            .params("{p1: [x, y], p2: [x, y] (two points on the underlay a known distance apart), length: the real distance, unit?: \"mm\"|\"m\"|\"cm\"|\"ft\"|\"in\" (default mm), layer?} → {layer, factor, scaleDenominator}"),
        CommandSpec::new("underlay.list", "List PDF Underlays", run_list).menu(&["Architecture", "Take-off", "List PDF Underlays"]).params("{} → {underlays: [{layer, file, page, scale, calibrated, entities}]}").noundo(),
        CommandSpec::new("underlay.remove", "Remove PDF Underlay", run_remove)
            .menu(&["Architecture", "Take-off", "Remove PDF Underlay"])
            .params("{layer?: \"PDF-UNDERLAY\"} → {layer, removed}"),
    ]
}

fn layer_param(p: &Value) -> String {
    str_param(p, "layer")
        .map(|s| s.trim().chars().take(255).collect::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_LAYER.to_string())
}

/// Millimetres in one drawing unit.
pub(super) fn unit_mm(s: &Session, cmd: &str) -> Result<f64> {
    let u = super::hcw::drawing_units(s.doc()?, cmd, &Value::Null)?.metres() * 1000.0;
    if u.is_finite() && u > 0.0 { Ok(u) } else { Err(bad(cmd, "the drawing units are not set; run `drawing.units` first")) }
}

fn handles_on(s: &Session, layer: &str) -> Result<Vec<Handle>> {
    Ok(s.doc()?.model.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(layer)).map(|e| e.handle).collect())
}

pub(super) fn records(s: &Session) -> Vec<Value> {
    s.doc().map(|d| project::get(d, KEY)).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

pub(super) fn save_records(s: &mut Session, list: Vec<Value>) -> Result<()> {
    project::set(s.doc_mut()?, KEY, &Value::Array(list));
    Ok(())
}

fn run_attach(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("underlay.attach", "PDF files cannot be read on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let reader = super::file::pdf_reader().ok_or_else(|| bad("underlay.attach", "PDF reading is not available in this build"))?;
        let path = str_param(p, "path").ok_or_else(|| bad("underlay.attach", "`path` is required"))?;
        let page = match p.get("page") {
            None => 1,
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n >= 1)
                .ok_or_else(|| bad("underlay.attach", "`page` must be a whole number from 1"))?,
        };
        let scale = f64_or(p, "scale", 1.0);
        if !(scale.is_finite() && (1e-6..=1e6).contains(&scale)) {
            return Err(bad("underlay.attach", "`scale` must be between 0.000001 and 1000000"));
        }
        let at = match p.get("at") {
            None => Vec2::new(0.0, 0.0),
            Some(_) => {
                point_param(p, "at").filter(|v| v.x.is_finite() && v.y.is_finite()).ok_or_else(|| bad("underlay.attach", "`at` must be [x, y]"))?
            }
        };
        let layer = layer_param(p);
        if !handles_on(s, &layer)?.is_empty() {
            return Err(bad(
                "underlay.attach",
                format!("layer `{layer}` already holds an underlay: give another `layer`, or run `underlay.remove` first"),
            ));
        }
        let unit = unit_mm(s, "underlay.attach")?;
        let len = std::fs::metadata(path).map_err(|e| bad("underlay.attach", format!("{path}: {e}")))?.len();
        if len > MAX_FILE {
            return Err(bad("underlay.attach", format!("{path} is larger than {MAX_FILE} bytes")));
        }
        let bytes = std::fs::read(path).map_err(|e| bad("underlay.attach", format!("{path}: {e}")))?;
        let data = reader(&bytes, page).map_err(|e| bad("underlay.attach", e))?;

        // Points to drawing units: a point is MM_PER_PT paper millimetres, shown at `scale`.
        let k = MM_PER_PT * scale / unit;
        let width_pt = data["width_pt"].as_f64().unwrap_or(0.0);
        let height_pt = data["height_pt"].as_f64().unwrap_or(0.0);
        let mut polys: Vec<(Vec<PolyVertex>, bool)> = Vec::new();
        let mut dropped = 0usize;
        for path_v in data["paths"].as_array().into_iter().flatten() {
            let pts: Vec<Vec2> = path_v["pts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|pt| Some(Vec2::new(at.x + pt.get(0)?.as_f64()? * k, at.y + pt.get(1)?.as_f64()? * k)))
                .filter(|v| v.x.is_finite() && v.y.is_finite())
                .collect();
            let rgb: Vec<u64> = path_v["rgb"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
            let white = rgb.len() == 3 && rgb.iter().all(|c| *c >= 250);
            let (bw, bh) = bounds(&pts);
            let page_fill = path_v["filled"].as_bool() == Some(true) && bw * k.recip() >= width_pt * 0.95 && bh * k.recip() >= height_pt * 0.95;
            // A white path and a page-sized fill are the paper, not the drawing.
            if pts.len() < 2 || (white && path_v["stroked"].as_bool() != Some(true)) || page_fill || (bw == 0.0 && bh == 0.0) {
                dropped += 1;
                continue;
            }
            if polys.len() >= MAX_ENTITIES {
                dropped += 1;
                continue;
            }
            polys.push((pts.into_iter().map(PolyVertex::new).collect(), path_v["closed"].as_bool() == Some(true)));
        }
        if polys.is_empty() {
            return Err(bad(
                "underlay.attach",
                "the page has no vector lines to draw (a scanned PDF, or only text and images); a take-off needs a vector PDF exported from CAD",
            ));
        }
        let n = polys.len();
        let d = s.doc_mut()?;
        d.ensure_layer(&layer);
        if let Some(l) = d.layer_mut(&layer) {
            l.color = aad_color::Color::Index(8);
        }
        for (vs, closed) in polys {
            let common = Common { layer: layer.clone(), ..Common::default() };
            d.add(&Space::Model, common, super::helpers::lwpoly(vs, closed)).map_err(|e| bad("underlay.attach", e.to_string()))?;
        }
        let mut list = records(s);
        list.retain(|r| r["layer"].as_str() != Some(layer.as_str()));
        let file = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string();
        list.push(json!({ "layer": layer, "file": file, "page": page, "scale": scale, "calibrated": false, "at": [at.x, at.y] }));
        save_records(s, list)?;
        let skipped = data["skipped"].clone();
        let mut warnings = Vec::new();
        if data["truncated"].as_bool() == Some(true) {
            warnings.push("the page is very large and was cut: the drawing is incomplete".to_string());
        }
        if skipped["text"].as_u64().unwrap_or(0) + skipped["images"].as_u64().unwrap_or(0) > 0 {
            warnings.push(format!("{} text and {} image items are not drawn", skipped["text"], skipped["images"]));
        }
        warnings.push("the scale is not known: run `underlay.calibrate` with two points a known distance apart before measuring".to_string());
        s.echo(format!("PDF underlay attached on layer {layer}: {n} lines. Calibrate the scale before measuring."));
        Ok(json!({
            "layer": layer, "page": page, "paths": n, "dropped": dropped, "widthMm": width_pt * MM_PER_PT * scale, "heightMm": height_pt * MM_PER_PT * scale,
            "pages": data["pages"], "skipped": skipped, "truncated": data["truncated"], "warnings": warnings,
        }))
    }
}

fn bounds(pts: &[Vec2]) -> (f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if pts.is_empty() { (0.0, 0.0) } else { (x1 - x0, y1 - y0) }
}

/// Millimetres in one `unit`.
fn to_mm(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" | "inch" | "inches" => 25.4,
        "ft" | "foot" | "feet" => 304.8,
        _ => return None,
    })
}

fn run_calibrate(s: &mut Session, p: &Value) -> Result<Value> {
    let layer = layer_param(p);
    let mut list = records(s);
    let idx = list
        .iter()
        .position(|r| r["layer"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(&layer)))
        .ok_or_else(|| bad("underlay.calibrate", format!("no underlay on layer `{layer}`: run `underlay.attach` first")))?;
    let p1 = point_param(p, "p1").filter(|v| v.x.is_finite() && v.y.is_finite()).ok_or_else(|| bad("underlay.calibrate", "`p1` must be [x, y]"))?;
    let p2 = point_param(p, "p2").filter(|v| v.x.is_finite() && v.y.is_finite()).ok_or_else(|| bad("underlay.calibrate", "`p2` must be [x, y]"))?;
    let unit_name = str_param(p, "unit").unwrap_or("mm");
    let per_unit = to_mm(unit_name).ok_or_else(|| {
        bad("underlay.calibrate", format!("unknown unit `{}`; use mm, cm, m, in or ft", unit_name.chars().take(20).collect::<String>()))
    })?;
    let length = f64_or(p, "length", f64::NAN);
    if !(length.is_finite() && length > 0.0) {
        return Err(bad("underlay.calibrate", "`length` must be the real distance between the two points, greater than zero"));
    }
    let drawn = p1.dist(p2);
    if !(drawn.is_finite() && drawn > 0.0) {
        return Err(bad("underlay.calibrate", "the two points are the same point"));
    }
    let unit = unit_mm(s, "underlay.calibrate")?;
    let factor = (length * per_unit / unit) / drawn;
    if !(factor.is_finite() && (1e-6..=1e6).contains(&factor)) {
        return Err(bad("underlay.calibrate", "that distance would scale the underlay by more than a million: check the two points and the length"));
    }
    let handles = handles_on(s, &layer)?;
    let d = s.doc_mut()?;
    for h in &handles {
        d.modify_entity(*h, |e| {
            if let EntityKind::LwPolyline(pl) = &mut e.kind {
                for v in &mut pl.vertices {
                    v.p = Vec2::new(p1.x + (v.p.x - p1.x) * factor, p1.y + (v.p.y - p1.y) * factor);
                }
            }
        })
        .map_err(|e| bad("underlay.calibrate", e.to_string()))?;
    }
    // The page scale now: drawing millimetres per paper millimetre.
    let old = list[idx]["scale"].as_f64().unwrap_or(1.0);
    let scale = old * factor;
    list[idx]["scale"] = json!(scale);
    list[idx]["calibrated"] = json!(true);
    save_records(s, list)?;
    s.echo(format!("Underlay calibrated: scaled by {factor:.6}, about 1:{:.1}.", scale));
    Ok(json!({ "layer": layer, "factor": factor, "scaleDenominator": scale, "entities": handles.len() }))
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut out = Vec::new();
    for mut r in records(s) {
        let n = r["layer"].as_str().map(|l| handles_on(s, l).map(|h| h.len())).transpose()?.unwrap_or(0);
        r["entities"] = json!(n);
        out.push(r);
    }
    Ok(json!({ "underlays": out }))
}

fn run_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let layer = layer_param(p);
    let mut list = records(s);
    let before = list.len();
    list.retain(|r| !r["layer"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(&layer)));
    let handles = handles_on(s, &layer)?;
    if before == list.len() && handles.is_empty() {
        return Err(bad("underlay.remove", format!("no underlay on layer `{layer}`")));
    }
    let d = s.doc_mut()?;
    for h in &handles {
        d.remove_entity(*h);
    }
    save_records(s, list)?;
    Ok(json!({ "layer": layer, "removed": handles.len() }))
}

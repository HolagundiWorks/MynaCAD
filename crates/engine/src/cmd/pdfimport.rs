//! `pdf.import`: a PDF page as editable drawing geometry.
//!
//! Where `underlay.attach` draws a PDF's paths as grey polylines to measure over, this brings the page in as
//! drawing objects: lines, polylines, circles, solid hatches for filled shapes and text, each with the colour,
//! line weight and layer the PDF gave it. A PDF written by a CAD program keeps its layers as optional-content
//! groups (O-Draft's own PDF export does), and those become drawing layers; any other path goes on a layer named for
//! its colour (`PDF-FF0000`), which sorts a plan by pen colour. The result is ordinary geometry that snaps, edits and
//! saves, but a PDF holds no scale, units or object types: the page arrives at `scale` (default 1: a paper
//! millimetre is a drawing millimetre) and `underlay.calibrate`-style rescaling is the user's `scale` command.
//!
//! Curves are cut into line segments by the reader, so arcs and splines become polylines; a closed round shape is
//! recognised and drawn as a circle. Images and shadings are not read, and text is read only for fonts whose codes
//! map to characters (the result counts the rest). Native builds only.

use aad_color::{Color, Rgb};
use aad_doc::{Common, EntityKind, Lineweight, Space};
use aad_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Most objects one import adds, most distinct layers it creates, and the largest PDF file read.
const MAX_ENTITIES: usize = 500_000;
const MAX_LAYERS: usize = 500;
const MAX_FILE: u64 = 256 * 1024 * 1024;
/// Millimetres in one PDF point.
const MM_PER_PT: f64 = 25.4 / 72.0;
/// Height of a capital letter as a fraction of the font's em size.
const CAP_HEIGHT: f64 = 0.72;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("pdf.import", "Import PDF as Geometry", run_import)
            .menu(&["Insert", "PDF as Geometry..."])
            .params("{path: PDF file, page?: 1, at?: [x, y] (lower left of the page, default [0, 0]), scale?: drawing mm per paper mm (default 1), layers?: \"pdf\" (the PDF's own layers, else by colour; default) | \"colour\" | \"single\", layer?: name for \"single\" (default \"PDF-IMPORT\"), text?: bool (default true), fills?: bool (solid hatches for filled shapes, default true), lineweights?: bool (default true), curves?: bool (recognise circles, default true)} → {page, objects: {lines, polylines, circles, hatches, texts}, layers, widthMm, heightMm, skipped, warnings}. One undo step."),
    ]
}

/// A layer name safe for the drawing: no characters DXF forbids, not empty, at most 255 characters.
fn clean_layer(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || "<>/\\\":;?*|=`,".contains(c) { '_' } else { c }).take(255).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "PDF".to_string() } else { s }
}

fn rgb_of(v: &Value) -> [u8; 3] {
    let c = |i: usize| v.get(i).and_then(Value::as_u64).map_or(0, |n| n.min(255) as u8);
    [c(0), c(1), c(2)]
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Black and white are the drawing's colour 7 (they print black); anything else is kept as a true colour.
fn color_of(c: [u8; 3]) -> Color {
    if c == [0, 0, 0] || c == [255, 255, 255] { Color::Index(7) } else { Color::True(Rgb(c[0], c[1], c[2])) }
}

fn lineweight_of(width_pt: f64) -> Lineweight {
    let hundredths = width_pt * MM_PER_PT * 100.0;
    if !hundredths.is_finite() || hundredths < 2.0 {
        return Lineweight::ByLayer;
    }
    let nearest = Lineweight::STANDARD.iter().copied().min_by_key(|w| (f64::from(*w) - hundredths).abs() as i64).unwrap_or(25);
    Lineweight::Mm100(nearest)
}

/// The circle through `pts` when they are a round closed shape of at least `min_points` points: centred on the
/// bounding box, equally far from the centre within 1%, and as wide as high.
fn circle_of(pts: &[Vec2], min_points: usize) -> Option<(Vec2, f64)> {
    if pts.len() < min_points {
        return None;
    }
    let b = aad_geom::Bounds2::from_points(pts.iter().copied());
    let (w, h) = (b.width(), b.height());
    if !(w > 0.0 && h > 0.0) || (w - h).abs() > 0.01 * w.max(h) {
        return None;
    }
    let c = b.center();
    let r = pts.iter().map(|p| p.dist(c)).sum::<f64>() / pts.len() as f64;
    (r > 0.0 && pts.iter().all(|p| (p.dist(c) - r).abs() <= 0.01 * r)).then_some((c, r))
}

/// `pts` without consecutive repeats, and without a last point that repeats the first.
fn tidy(pts: Vec<Vec2>, closed: bool) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_none_or(|l| l.dist(p) > 1e-9) {
            out.push(p);
        }
    }
    if closed && out.len() > 2 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-9) {
        out.pop();
    }
    out
}

#[derive(Default)]
struct Counts {
    lines: usize,
    polylines: usize,
    circles: usize,
    hatches: usize,
    texts: usize,
}

pub(super) fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("pdf.import", "PDF files cannot be read on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        const CMD: &str = "pdf.import";
        if s.block_edit.is_some() {
            return Err(bad(CMD, "close the block editor first"));
        }
        let reader = super::file::pdf_reader().ok_or_else(|| bad(CMD, "PDF reading is not available in this build"))?;
        let path = str_param(p, "path")
            .filter(|t| !t.trim().is_empty() && t.len() < 1024 && !t.contains('\0'))
            .ok_or_else(|| bad(CMD, "`path` is required"))?;
        let page = match p.get("page") {
            None | Some(Value::Null) => 1,
            Some(v) => {
                v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n >= 1).ok_or_else(|| bad(CMD, "`page` must be a whole number from 1"))?
            }
        };
        let scale = f64_or(p, "scale", 1.0);
        if !(scale.is_finite() && (1e-6..=1e6).contains(&scale)) {
            return Err(bad(CMD, "`scale` must be between 0.000001 and 1000000"));
        }
        let at = match p.get("at") {
            None | Some(Value::Null) => Vec2::new(0.0, 0.0),
            Some(_) => point_param(p, "at").filter(|v| v.x.is_finite() && v.y.is_finite()).ok_or_else(|| bad(CMD, "`at` must be [x, y]"))?,
        };
        let layers_mode = match str_param(p, "layers").map(str::to_ascii_lowercase).as_deref() {
            None | Some("pdf") => 0,
            Some("colour" | "color") => 1,
            Some("single") => 2,
            Some(_) => return Err(bad(CMD, "`layers` is `pdf`, `colour` or `single`")),
        };
        let single = clean_layer(str_param(p, "layer").unwrap_or("PDF-IMPORT"));
        let (want_text, want_fills, want_weights, want_curves) =
            (bool_or(p, "text", true), bool_or(p, "fills", true), bool_or(p, "lineweights", true), bool_or(p, "curves", true));
        let unit_mm = {
            let u = super::hcw::drawing_units(s.doc()?, CMD, &Value::Null)?.metres() * 1000.0;
            if u.is_finite() && u > 0.0 { u } else { return Err(bad(CMD, "the drawing units are not set; run `drawing.units` first")) }
        };
        let len = std::fs::metadata(path).map_err(|e| bad(CMD, format!("{path}: {e}")))?.len();
        if len > MAX_FILE {
            return Err(bad(CMD, format!("{path} is larger than {MAX_FILE} bytes")));
        }
        let bytes = std::fs::read(path).map_err(|e| bad(CMD, format!("{path}: {e}")))?;
        let data = reader(&bytes, page).map_err(|e| bad(CMD, e))?;

        // Points to drawing units.
        let k = MM_PER_PT * scale / unit_mm;
        let (width_pt, height_pt) = (data["width_pt"].as_f64().unwrap_or(0.0), data["height_pt"].as_f64().unwrap_or(0.0));
        let place = |x: f64, y: f64| Vec2::new(at.x + x * k, at.y + y * k);

        let mut layer_names: Vec<String> = Vec::new();
        let mut layer_of = |name: String| -> String {
            if layer_names.iter().any(|l| l.eq_ignore_ascii_case(&name)) {
                return name;
            }
            if layer_names.len() >= MAX_LAYERS {
                return "PDF-OTHER".to_string();
            }
            layer_names.push(name.clone());
            name
        };
        let mut objects: Vec<(Common, EntityKind)> = Vec::new();
        let mut counts = Counts::default();
        let mut dropped = 0usize;
        let style = s.doc()?.header.str("TEXTSTYLE", "Standard");

        for path_v in data["paths"].as_array().into_iter().flatten() {
            if objects.len() >= MAX_ENTITIES {
                dropped += 1;
                continue;
            }
            let pts: Vec<Vec2> = path_v["pts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|pt| Some(place(pt.get(0)?.as_f64()?, pt.get(1)?.as_f64()?)))
                .filter(|v| v.x.is_finite() && v.y.is_finite())
                .collect();
            let (stroked, filled) = (path_v["stroked"].as_bool() == Some(true), path_v["filled"].as_bool() == Some(true));
            let closed = path_v["closed"].as_bool() == Some(true);
            let (stroke, fill) = (rgb_of(&path_v["rgb"]), rgb_of(&path_v["fill_rgb"]));
            let (b0, b1) = (
                pts.iter().fold(Vec2::new(f64::MAX, f64::MAX), |a, p| Vec2::new(a.x.min(p.x), a.y.min(p.y))),
                pts.iter().fold(Vec2::new(f64::MIN, f64::MIN), |a, p| Vec2::new(a.x.max(p.x), a.y.max(p.y))),
            );
            // The paper itself (a page-sized white fill) is not drawing.
            let page_fill =
                filled && width_pt > 0.0 && height_pt > 0.0 && (b1.x - b0.x) >= width_pt * k * 0.95 && (b1.y - b0.y) >= height_pt * k * 0.95;
            let white_fill = filled && !stroked && fill.iter().all(|c| *c >= 250);
            if pts.len() < 2 || page_fill || white_fill {
                dropped += 1;
                continue;
            }
            let layer_name = match (layers_mode, path_v["layer"].as_str()) {
                (2, _) => single.clone(),
                (0, Some(l)) => clean_layer(l),
                _ => format!("PDF-{}", hex(if stroked { stroke } else { fill })),
            };
            let layer = layer_of(layer_name);
            let weight = if want_weights && stroked { lineweight_of(path_v["width_pt"].as_f64().unwrap_or(0.0)) } else { Lineweight::ByLayer };
            let curved = path_v["curved"].as_bool() == Some(true);
            // A path that comes back to where it started is a closed shape, whether or not it said so (a writer
            // that flattens circles draws them as a run of lines ending on the first point).
            let ends_meet = pts.len() >= 4 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) <= 0.01 * k);
            let closed = closed || ends_meet;
            let pts = tidy(pts, closed || filled);
            if pts.len() < 2 {
                dropped += 1;
                continue;
            }
            let ring = closed || filled;
            if filled && want_fills && pts.len() >= 3 {
                let vertices: Vec<PolyVertex> = pts.iter().copied().map(PolyVertex::new).collect();
                let common = Common { layer: layer.clone(), color: color_of(fill), ..Common::default() };
                objects.push((
                    common,
                    EntityKind::Hatch(aad_doc::Hatch {
                        pattern: "SOLID".into(),
                        solid: true,
                        loops: vec![aad_doc::HatchLoop { vertices, outer: true }],
                        scale: 1.0,
                        angle: 0.0,
                        associative: false,
                        style: 0,
                        elevation: 0.0,
                        gradient: None,
                        origin: Vec2::ZERO,
                        background: None,
                        pattern_lines: Vec::new(),
                    }),
                ));
                counts.hatches += 1;
            }
            if stroked {
                let common = Common { layer: layer.clone(), color: color_of(stroke), lineweight: weight, ..Common::default() };
                // A shape cut from curves needs a dozen points to be called round; one already made of short lines (a
                // PDF written by a program that flattens circles) needs two dozen.
                let kind = if want_curves
                    && ring
                    && let Some((c, r)) = circle_of(&pts, if curved { 12 } else { 24 })
                {
                    counts.circles += 1;
                    EntityKind::Circle(aad_doc::Circle { center: c.to3(0.0), radius: r })
                } else if pts.len() == 2 && !ring {
                    counts.lines += 1;
                    EntityKind::Line(aad_doc::Line { a: pts[0].to3(0.0), b: pts[1].to3(0.0) })
                } else {
                    counts.polylines += 1;
                    super::helpers::lwpoly(pts.iter().copied().map(PolyVertex::new).collect(), ring)
                };
                objects.push((common, kind));
            }
        }

        if want_text {
            for t in data["texts"].as_array().into_iter().flatten() {
                if objects.len() >= MAX_ENTITIES {
                    dropped += 1;
                    continue;
                }
                let (Some(text), Some(x), Some(y), Some(size)) = (t["text"].as_str(), t["x"].as_f64(), t["y"].as_f64(), t["size_pt"].as_f64()) else {
                    continue;
                };
                let height = size * CAP_HEIGHT * k;
                let pos = place(x, y);
                if !(height.is_finite() && height > 0.0 && pos.x.is_finite() && pos.y.is_finite()) {
                    continue;
                }
                let rgb = rgb_of(&t["rgb"]);
                let layer_name = match (layers_mode, t["layer"].as_str()) {
                    (2, _) => single.clone(),
                    (0, Some(l)) => clean_layer(l),
                    _ => "PDF-TEXT".to_string(),
                };
                let common = Common { layer: layer_of(layer_name), color: color_of(rgb), ..Common::default() };
                objects.push((
                    common,
                    EntityKind::Text(aad_doc::Text {
                        insert: pos.to3(0.0),
                        align_pt: None,
                        height,
                        value: text.to_string(),
                        rotation: t["angle"].as_f64().filter(|a| a.is_finite()).unwrap_or(0.0),
                        width_factor: 1.0,
                        oblique: 0.0,
                        style: style.clone(),
                        halign: aad_doc::HAlign::Left,
                        valign: aad_doc::VAlign::Baseline,
                    }),
                ));
                counts.texts += 1;
            }
        }

        if objects.is_empty() {
            return Err(bad(
                CMD,
                "the page has nothing to import (a scanned PDF, or only images); a PDF exported from CAD has vector paths and text",
            ));
        }
        let total = objects.len();
        let d = s.doc_mut()?;
        for name in &layer_names {
            d.ensure_layer(name);
        }
        for (common, kind) in objects {
            d.add(&Space::Model, common, kind).map_err(|e| bad(CMD, e.to_string()))?;
        }
        let skipped = data["skipped"].clone();
        let mut warnings = Vec::new();
        if data["truncated"].as_bool() == Some(true) || dropped > 0 && total >= MAX_ENTITIES {
            warnings.push("the page is very large and was cut: the drawing is incomplete".to_string());
        }
        if skipped["text"].as_u64().unwrap_or(0) > 0 {
            warnings.push(format!("{} text item(s) use a font with no character map and were not imported", skipped["text"]));
        }
        if skipped["images"].as_u64().unwrap_or(0) + skipped["shadings"].as_u64().unwrap_or(0) > 0 {
            warnings.push(format!("{} image and {} shading item(s) are not imported", skipped["images"], skipped["shadings"]));
        }
        warnings
            .push("a PDF holds no real-world scale: check a known dimension and rescale with `scale` if the page was not printed 1:1".to_string());
        s.echo(format!("Imported PDF page {page}: {total} objects on {} layer(s).", layer_names.len()));
        Ok(json!({
            "page": page, "pages": data["pages"], "layers": layer_names,
            "objects": { "lines": counts.lines, "polylines": counts.polylines, "circles": counts.circles, "hatches": counts.hatches, "texts": counts.texts },
            "dropped": dropped, "widthMm": width_pt * MM_PER_PT * scale, "heightMm": height_pt * MM_PER_PT * scale,
            "skipped": skipped, "warnings": warnings,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_of_points_is_a_circle_and_an_oval_or_square_is_not() {
        let ring = |rx: f64, ry: f64| -> Vec<Vec2> {
            (0..32)
                .map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / 32.0;
                    Vec2::new(5.0 + rx * a.cos(), 7.0 + ry * a.sin())
                })
                .collect()
        };
        let (c, r) = circle_of(&ring(3.0, 3.0), 12).unwrap();
        assert!(c.dist(Vec2::new(5.0, 7.0)) < 1e-9 && (r - 3.0).abs() < 1e-9);
        assert!(circle_of(&ring(3.0, 2.0), 12).is_none(), "an ellipse");
        assert!(circle_of(&ring(3.0, 3.0), 40).is_none(), "fewer points than asked for");
        assert!(circle_of(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)], 12).is_none(), "too few points");
    }

    #[test]
    fn layer_names_and_line_weights_are_cleaned() {
        assert_eq!(clean_layer("A/B<C>:\"D\"\n"), "A_B_C___D__");
        assert_eq!(clean_layer("   "), "PDF");
        assert_eq!(clean_layer(&"x".repeat(400)).len(), 255);
        assert_eq!(lineweight_of(0.0), Lineweight::ByLayer);
        assert_eq!(lineweight_of(f64::NAN), Lineweight::ByLayer);
        // 1 pt = 0.353 mm: the nearest standard weight is 0.35.
        assert_eq!(lineweight_of(1.0), Lineweight::Mm100(35));
        assert_eq!(lineweight_of(1.0e9), Lineweight::Mm100(211));
        assert_eq!(color_of([0, 0, 0]), Color::Index(7));
        assert_eq!(color_of([255, 0, 0]), Color::True(Rgb(255, 0, 0)));
    }
}

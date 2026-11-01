//! `import`: bring a file into the open drawing, whatever its kind. One command, chosen by the file extension:
//!
//! | Kind | Extensions | What arrives |
//! |---|---|---|
//! | Drawing | `.dxf`, `.dwg` | the file's objects, layers and blocks, merged at their own coordinates (names this drawing already has keep this drawing's definition) |
//! | PDF | `.pdf` | the page's vector paths, text and fills as geometry (the same as `pdf.import`) |
//! | SVG | `.svg` | shapes and paths as polylines at the size the SVG states |
//! | Raster | `.png`, `.jpg`, `.jpeg`, `.tif`, `.tiff` | the dark lines of the picture, traced to polylines on their own layer; the picture's own pixels are not placed in the drawing |
//!
//! SVG and raster arrive on a layer of their own and are registered like an underlay, so `underlay.calibrate` fixes the scale from two picked points and a real
//! distance, and `underlay.remove` takes them out again. Everything is one undo step.

use aad_doc::{Common, Space};
use aad_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::underlay::{records, save_records, unit_mm};
use super::*;
use crate::{Result, Session};

/// Most polylines one raster gives, and the largest picture side kept (larger pictures are reduced first).
const MAX_POLYS: usize = 100_000;
const MAX_SIDE: usize = 2000;
const MAX_FILE: u64 = 256 * 1024 * 1024;
/// Millimetres in one inch.
const MM_PER_IN: f64 = 25.4;

/// The kinds `import` reads, as the file dialog lists them.
pub const EXTENSIONS: &[&str] = &["dxf", "dwg", "pdf", "svg", "png", "jpg", "jpeg", "tif", "tiff"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("import", "Import...", run_import)
            .menu(&["File", "Import..."])
            .params("{path: a .dxf .dwg .pdf .svg .png .jpg .jpeg .tif or .tiff file, at?: [x, y] (lower left of the imported page or picture, default [0, 0]; not used by dxf and dwg), scale?: drawing mm per paper mm (svg, pdf; default 1), page?: 1 (pdf), layer?: name (svg and raster; default \"SVG-IMPORT\" or \"RASTER-IMPORT\"), dpi?: pixels per inch of a raster (default 96), threshold?: 0-255, pixels darker than this are lines (raster, default 128), invert?: bool (raster: light lines on a dark ground)} → {kind, layer?, objects, widthMm, heightMm, warnings}. One undo step."),
    ]
}

fn kind_of(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match ext.as_str() {
        "dxf" | "dwg" => "drawing",
        "pdf" => "pdf",
        "svg" => "svg",
        "png" | "jpg" | "jpeg" | "tif" | "tiff" => "raster",
        _ => return None,
    })
}

fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad("import", "files cannot be imported by path on the web"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = str_param(p, "path").ok_or_else(|| bad("import", "`path` is required"))?;
        let kind = kind_of(path).ok_or_else(|| {
            bad("import", format!("{path}: this kind of file is not imported; the kinds are {}", EXTENSIONS.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" ")))
        })?;
        let len = std::fs::metadata(path).map_err(|e| bad("import", format!("{path}: {e}")))?.len();
        if len > MAX_FILE {
            return Err(bad("import", format!("{path} is larger than {MAX_FILE} bytes")));
        }
        s.doc()?;
        match kind {
            "drawing" => import_drawing(s, path),
            "pdf" => {
                let mut r = super::pdfimport::run_import(s, p)?;
                r["kind"] = json!("pdf");
                Ok(r)
            }
            "svg" => import_svg(s, path, p),
            _ => import_raster(s, path, p),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn import_drawing(s: &mut Session, path: &str) -> Result<Value> {
    let mut src = super::blocks::read_drawing(path)?;
    let report = crate::flatten::flatten_drawing(&mut src);
    let mut d = s.doc()?.clone();
    super::blocks::merge_tables(&mut d, &src);
    let renames = super::blocks::block_names(&d, &src, "");
    for (from, to) in &renames {
        let Some(b) = src.blocks.get(from) else { continue };
        let mut nb = (**b).clone();
        nb.name = to.clone();
        nb.entities = Default::default();
        for e in b.entities.iter() {
            let e = super::blocks::adopt(&mut d, e, &renames);
            nb.entities.push(e);
        }
        d.blocks.insert(to.clone(), std::sync::Arc::new(nb));
    }
    let mut n = 0usize;
    for e in src.model.iter() {
        let e = super::blocks::adopt(&mut d, e, &renames);
        d.ensure_layer(&e.common.layer);
        d.model.push(e);
        n += 1;
    }
    *s.doc_mut()? = d;
    let mut warnings = Vec::new();
    if report.has_findings() {
        warnings.push(report.message());
    }
    if n == 0 {
        warnings.push("the file has no objects in its model space".to_string());
    }
    s.echo(format!("Imported {n} objects from {}.", file_name(path)));
    Ok(json!({ "kind": "drawing", "objects": n, "warnings": warnings }))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

fn layer_or(p: &Value, default: &str) -> String {
    str_param(p, "layer").map(|s| s.trim().chars().take(255).collect::<String>()).filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
}

/// Draw `polys` (millimetres on the paper, y up) at `at` and `scale` on `layer`, grey, and register the layer like an underlay.
#[allow(clippy::too_many_arguments)]
fn place(s: &mut Session, cmd: &str, polys: Vec<(Vec<(f64, f64)>, bool)>, at: Vec2, scale: f64, layer: &str, file: &str, calibrated: bool) -> Result<usize> {
    let unit = unit_mm(s, cmd)?;
    let k = scale / unit;
    let n = polys.len();
    let d = s.doc_mut()?;
    d.ensure_layer(layer);
    if let Some(l) = d.layer_mut(layer) {
        l.color = aad_color::Color::Index(8);
    }
    for (pts, closed) in polys {
        let vs: Vec<PolyVertex> = pts.into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(at.x + x * k, at.y + y * k))).collect();
        let common = Common { layer: layer.to_string(), ..Common::default() };
        d.add(&Space::Model, common, super::helpers::lwpoly(vs, closed)).map_err(|e| bad(cmd, e.to_string()))?;
    }
    let mut list = records(s);
    list.retain(|r| r["layer"].as_str() != Some(layer));
    list.push(json!({ "layer": layer, "file": file, "page": 1, "scale": scale, "calibrated": calibrated, "at": [at.x, at.y] }));
    save_records(s, list)?;
    Ok(n)
}

fn common_params(s: &Session, p: &Value, default_layer: &str) -> Result<(Vec2, f64, String)> {
    let scale = f64_or(p, "scale", 1.0);
    if !(scale.is_finite() && (1e-6..=1e6).contains(&scale)) {
        return Err(bad("import", "`scale` must be between 0.000001 and 1000000"));
    }
    let at = match p.get("at") {
        None => Vec2::new(0.0, 0.0),
        Some(_) => point_param(p, "at").filter(|v| v.x.is_finite() && v.y.is_finite()).ok_or_else(|| bad("import", "`at` must be [x, y]"))?,
    };
    let layer = layer_or(p, default_layer);
    let busy = s.doc()?.model.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&layer));
    if busy {
        return Err(bad("import", format!("layer `{layer}` already holds objects: give another `layer`, or remove them first (`underlay.remove`)")));
    }
    Ok((at, scale, layer))
}

#[cfg(not(target_arch = "wasm32"))]
fn import_svg(s: &mut Session, path: &str, p: &Value) -> Result<Value> {
    let (at, scale, layer) = common_params(s, p, "SVG-IMPORT")?;
    let text = std::fs::read_to_string(path).map_err(|e| bad("import", format!("{path}: {e}")))?;
    let svg = super::svgimport::read(&text).map_err(|e| bad("import", format!("{path}: {e}")))?;
    if svg.polys.is_empty() {
        return Err(bad("import", format!("{path}: the SVG has no lines or shapes to draw (text and pictures inside it are not imported)")));
    }
    let n = place(s, "import", svg.polys, at, scale, &layer, &file_name(path), true)?;
    let mut warnings = Vec::new();
    if svg.skipped > 0 {
        warnings.push(format!("{} text, picture or reference elements are not drawn", svg.skipped));
    }
    if svg.truncated {
        warnings.push("the SVG is very large and was cut: the drawing is incomplete".to_string());
    }
    s.echo(format!("SVG imported on layer {layer}: {n} polylines, {:.0} x {:.0} mm.", svg.width_mm * scale, svg.height_mm * scale));
    Ok(json!({ "kind": "svg", "layer": layer, "objects": n, "widthMm": svg.width_mm * scale, "heightMm": svg.height_mm * scale, "skipped": svg.skipped, "warnings": warnings }))
}

#[cfg(not(target_arch = "wasm32"))]
fn import_raster(s: &mut Session, path: &str, p: &Value) -> Result<Value> {
    let reader = super::file::raster_reader().ok_or_else(|| bad("import", "picture reading is not available in this build"))?;
    let (at, scale, layer) = common_params(s, p, "RASTER-IMPORT")?;
    let dpi = f64_or(p, "dpi", 96.0);
    if !(dpi.is_finite() && (1.0..=10_000.0).contains(&dpi)) {
        return Err(bad("import", "`dpi` must be between 1 and 10000"));
    }
    let threshold = f64_or(p, "threshold", 128.0).clamp(1.0, 255.0) as u8;
    let invert = p.get("invert").and_then(Value::as_bool).unwrap_or(false);
    let bytes = std::fs::read(path).map_err(|e| bad("import", format!("{path}: {e}")))?;
    let pic = reader(&bytes).map_err(|e| bad("import", format!("{path}: {e}")))?;
    if pic.width == 0 || pic.height == 0 || pic.gray.len() != pic.width * pic.height {
        return Err(bad("import", format!("{path}: the picture is empty")));
    }
    let (w0, h0) = (pic.width, pic.height);
    let (gray, w, h, factor) = reduce(&pic.gray, w0, h0);
    let mask: Vec<bool> = gray.iter().map(|g| (*g < threshold) != invert).collect();
    let dark = mask.iter().filter(|b| **b).count();
    if dark == 0 || dark == mask.len() {
        return Err(bad(
            "import",
            format!("{path}: no lines found at threshold {threshold}: the picture is {}; change `threshold` (1-255) or `invert`", if dark == 0 { "all light" } else { "all dark" }),
        ));
    }
    let tol = 0.75;
    let loops = trace(&mask, w, h, tol, MAX_POLYS + 1);
    if loops.len() > MAX_POLYS {
        return Err(bad("import", format!("{path}: the picture is too detailed to trace ({MAX_POLYS}+ outlines): it is a photograph, not a line drawing; lower `threshold` or use a cleaner scan")));
    }
    // pixels to paper mm: one source pixel is 25.4 / dpi mm; `factor` source pixels make one traced pixel; y runs up
    let mm = MM_PER_IN / dpi * factor;
    let height_px = h as f64;
    let polys: Vec<(Vec<(f64, f64)>, bool)> =
        loops.into_iter().map(|pts| (pts.into_iter().map(|(x, y)| (x * mm, (height_px - y) * mm)).collect(), true)).collect();
    if polys.is_empty() {
        return Err(bad("import", format!("{path}: no lines found")));
    }
    let n = place(s, "import", polys, at, scale, &layer, &file_name(path), false)?;
    let width_mm = w0 as f64 * MM_PER_IN / dpi * scale;
    let height_mm = h0 as f64 * MM_PER_IN / dpi * scale;
    let mut warnings = vec![
        "the picture was traced to outlines; its pixels are not placed in the drawing".to_string(),
        "the scale is not known: run `underlay.calibrate` with two points a known distance apart before measuring".to_string(),
    ];
    if factor > 1.0 {
        warnings.push(format!("the picture is {w0} x {h0} pixels and was reduced {factor:.1} times before tracing"));
    }
    s.echo(format!("Picture traced on layer {layer}: {n} outlines. Calibrate the scale before measuring."));
    Ok(json!({ "kind": "raster", "layer": layer, "objects": n, "widthMm": width_mm, "heightMm": height_mm, "pixels": [w0, h0], "threshold": threshold, "warnings": warnings }))
}

/// The picture reduced (mean of blocks) until its longer side is at most [`MAX_SIDE`]. Returns the pixels, the new size and the reduction.
fn reduce(gray: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize, f64) {
    let f = w.max(h).div_ceil(MAX_SIDE).max(1);
    if f == 1 {
        return (gray.to_vec(), w, h, 1.0);
    }
    let (nw, nh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![255u8; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let (mut sum, mut cnt) = (0u32, 0u32);
            for yy in y * f..((y + 1) * f).min(h) {
                for xx in x * f..((x + 1) * f).min(w) {
                    sum += u32::from(gray[yy * w + xx]);
                    cnt += 1;
                }
            }
            out[y * nw + x] = (sum / cnt.max(1)) as u8;
        }
    }
    (out, nw, nh, f as f64)
}

/// The outlines of the set pixels of `mask` (`w` x `h`, row 0 on top): every boundary between a set and an unset pixel is an edge, edges are chained into
/// loops, and each loop is simplified within `tol` pixels. Holes are loops too. Stops after `limit` loops.
pub(crate) fn trace(mask: &[bool], w: usize, h: usize, tol: f64, limit: usize) -> Vec<Vec<(f64, f64)>> {
    let at = |x: isize, y: isize| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && mask[y as usize * w + x as usize];
    let vw = w + 1;
    // outgoing edges per vertex: up to two (where two pixels touch at a corner); an edge is (dx, dy) with the set pixel on its right
    let mut out: std::collections::HashMap<usize, Vec<(i8, i8)>> = std::collections::HashMap::new();
    let mut add = |x: usize, y: usize, d: (i8, i8)| out.entry(y * vw + x).or_default().push(d);
    for y in 0..h as isize {
        for x in 0..w as isize {
            if !at(x, y) {
                continue;
            }
            let (ux, uy) = (x as usize, y as usize);
            if !at(x, y - 1) {
                add(ux, uy, (1, 0));
            }
            if !at(x + 1, y) {
                add(ux + 1, uy, (0, 1));
            }
            if !at(x, y + 1) {
                add(ux + 1, uy + 1, (-1, 0));
            }
            if !at(x - 1, y) {
                add(ux, uy + 1, (0, -1));
            }
        }
    }
    let mut loops = Vec::new();
    let keys: Vec<usize> = {
        let mut k: Vec<usize> = out.keys().copied().collect();
        k.sort_unstable();
        k
    };
    for start in keys {
        while let Some(first) = out.get_mut(&start).and_then(|v| v.pop()) {
            let mut pts = vec![(start % vw, start / vw)];
            let (mut cur, mut dir) = (start, first);
            loop {
                let (cx, cy) = (cur % vw, cur / vw);
                let nx = (cx as isize + isize::from(dir.0)) as usize;
                let ny = (cy as isize + isize::from(dir.1)) as usize;
                cur = ny * vw + nx;
                if cur == start {
                    break;
                }
                pts.push((nx, ny));
                let Some(opts) = out.get_mut(&cur) else { break };
                if opts.is_empty() {
                    break;
                }
                // where two edges leave, turn right (the set pixel stays on the right): the loops stay simple
                let pick = if opts.len() > 1 {
                    let right = (-dir.1, dir.0);
                    opts.iter().position(|d| *d == right).or_else(|| opts.iter().position(|d| *d == dir)).unwrap_or(0)
                } else {
                    0
                };
                dir = opts.swap_remove(pick);
            }
            let poly = simplify(&pts.into_iter().map(|(x, y)| (x as f64, y as f64)).collect::<Vec<_>>(), tol);
            if poly.len() >= 3 {
                loops.push(poly);
                if loops.len() >= limit {
                    return loops;
                }
            }
        }
    }
    loops
}

/// Douglas-Peucker on a closed loop: split at the two points farthest apart, simplify both halves within `tol`.
fn simplify(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    if pts.len() <= 4 {
        return pts.to_vec();
    }
    let a = 0;
    let mut b = 0;
    let mut best = -1.0;
    for (i, p) in pts.iter().enumerate() {
        let d = (p.0 - pts[a].0).powi(2) + (p.1 - pts[a].1).powi(2);
        if d > best {
            best = d;
            b = i;
        }
    }
    let first = dp(&pts[a..=b], tol);
    let mut second_src: Vec<(f64, f64)> = pts[b..].to_vec();
    second_src.push(pts[a]);
    let second = dp(&second_src, tol);
    let mut out = first;
    out.pop();
    out.extend(second);
    out.pop();
    out
}

fn dp(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    let (mut idx, mut worst) = (0, 0.0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = if len == 0.0 { ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt() } else { ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len };
        if d > worst {
            worst = d;
            idx = i;
        }
    }
    if worst <= tol {
        return vec![a, b];
    }
    let mut left = dp(&pts[..=idx], tol);
    let right = dp(&pts[idx..], tol);
    left.pop();
    left.extend(right);
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filled_block_traces_to_one_rectangle() {
        let (w, h) = (10, 8);
        let mut mask = vec![false; w * h];
        for y in 2..6 {
            for x in 3..8 {
                mask[y * w + x] = true;
            }
        }
        let loops = trace(&mask, w, h, 0.75, 100);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 4, "{:?}", loops[0]);
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &loops[0] {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        assert_eq!((x0, y0, x1, y1), (3.0, 2.0, 8.0, 6.0));
    }

    #[test]
    fn a_ring_traces_to_an_outline_and_a_hole() {
        let (w, h) = (9, 9);
        let mut mask = vec![false; w * h];
        for y in 1..8 {
            for x in 1..8 {
                mask[y * w + x] = !((3..6).contains(&x) && (3..6).contains(&y));
            }
        }
        assert_eq!(trace(&mask, w, h, 0.75, 100).len(), 2);
    }

    #[test]
    fn two_blocks_touching_at_a_corner_are_two_loops_and_the_limit_stops() {
        let (w, h) = (4, 4);
        let mut mask = vec![false; w * h];
        mask[0] = true;
        mask[w + 1] = true;
        let loops = trace(&mask, w, h, 0.75, 100);
        assert_eq!(loops.len(), 2);
        assert_eq!(trace(&mask, w, h, 0.75, 1).len(), 1);
    }

    #[test]
    fn a_large_picture_is_reduced_and_extensions_are_recognised() {
        let (g, w, h, f) = reduce(&vec![0u8; 4000 * 1000], 4000, 1000);
        assert_eq!((w, h, f), (2000, 500, 2.0));
        assert_eq!(g.len(), w * h);
        assert_eq!(kind_of("C:\\a\\Plan.TIFF"), Some("raster"));
        assert_eq!(kind_of("a.dwg"), Some("drawing"));
        assert_eq!(kind_of("a.doc"), None);
    }
}

//! Vector PDF plotting (PLOT / EXPORTPDF): a minimal PDF 1.4 writer of our own.
//!
//! One page per plot: model-space or layout extents fitted to a sheet by default (`scale_to_fit`),
//! or a fixed scale when fit is off
//! (paper units are millimetres in metric drawings, inches otherwise). Lines are stroked paths
//! (`m`/`l`/`S`), fills are `f` paths, colours are RGB with colour 7 printing black on white
//! paper, and line widths come from lineweights when enabled. The page setup's plot style
//! table (or the `plotStyleTable` option) sets the printed colours, linetypes and lineweights.

use std::fmt::Write as _;

use aad_color::Rgb;
use aad_doc::{Drawing, PageSetup, Space};
use aad_geom::{Bounds2, Vec2};
use aad_render::{Kind, Sheet, clip, paper};
use serde_json::Value;

use crate::{IoError, Result};

/// Most one-page plots joined into one file.
const MAX_MERGE: usize = 256;

/// PDF points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Largest sheet side we accept, in millimetres (PDF viewers cap pages at 200 inches anyway).
const MAX_SHEET_MM: f64 = 5080.0;

/// Plot settings. `None` fields fall back to the layout's page setup (or sensible defaults for
/// model space).
#[derive(Clone, Debug, Default)]
pub struct PdfOptions {
    /// Plot style table name, replacing the page setup's (`"None"` plots without one).
    pub plot_style_table: Option<String>,
    /// Paper name from [`aad_render::PAPER_SIZES`] (`"A4"`, `"Letter"`, `"ANSI B"`…).
    pub paper: Option<String>,
    /// Custom paper size in millimetres (portrait width, height).
    pub paper_mm: Option<(f64, f64)>,
    pub landscape: Option<bool>,
    /// Fit the drawing to the printable area. Default: the page setup's `scale_to_fit` (on for new
    /// drawings), and always on for a model-space plot window.
    pub fit: Option<bool>,
    /// Plot scale in paper units per drawing unit (when not fitting).
    pub scale: Option<f64>,
    /// Plot object lineweights. Default: the layout's setting; on for model space.
    pub lineweights: Option<bool>,
    /// Flate-compress the content stream.
    pub compress: bool,
    pub title: String,
    /// Model space only: plot this rectangle (drawing units), fitted to the printable area,
    /// instead of the extents. Ignored for layouts.
    pub window: Option<Bounds2>,
    /// Write each drawing layer as a PDF optional-content group (a layer the viewer can show or
    /// hide). Default on.
    pub layers: Option<bool>,
}

impl PdfOptions {
    /// Parse `{paper?, width?, height?, landscape?, fit?, scale?, lineweights?, compress?, title?, window?}`.
    pub fn from_json(v: &Value) -> PdfOptions {
        let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        let paper_mm = match (num("width"), num("height")) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        };
        PdfOptions {
            plot_style_table: v.get("plotStyleTable").and_then(Value::as_str).map(|s| s.chars().take(260).collect()),
            paper: v.get("paper").and_then(Value::as_str).map(str::to_string),
            paper_mm,
            landscape: v.get("landscape").and_then(Value::as_bool),
            fit: v.get("fit").and_then(Value::as_bool),
            scale: num("scale").filter(|s| *s > 0.0),
            lineweights: v.get("lineweights").and_then(Value::as_bool),
            compress: v.get("compress").and_then(Value::as_bool).unwrap_or(true),
            title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            window: window_from_json(v),
            layers: v.get("layers").and_then(Value::as_bool),
        }
    }
}

/// Plot with JSON options (the engine's `plot` hook).
pub fn plot(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    pdf(d, space, &PdfOptions::from_json(opts))
}

/// The page setup a plot of `space` uses, with overrides applied.
pub fn page_for(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<PageSetup> {
    let mut page = match space {
        Space::Paper(n) => d.layout(n).map(|l| l.page.clone()).ok_or_else(|| IoError::Format(format!("no layout `{n}`")))?,
        Space::Model => {
            let mut p = PageSetup::default();
            if paper::paper_unit_mm(d) == 1.0
                && let Some(a4) = paper::paper_size("A4")
            {
                p.paper = a4.name.into();
                p.width_mm = a4.width_mm;
                p.height_mm = a4.height_mm;
            }
            p
        }
    };
    if let Some(name) = &o.paper {
        let ps = paper::paper_size(name).ok_or_else(|| IoError::Format(format!("unknown paper size `{name}`")))?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    if let Some((w, h)) = o.paper_mm {
        page.paper = format!("User ({w:.2} x {h:.2} MM)");
        page.width_mm = w.min(MAX_SHEET_MM);
        page.height_mm = h.min(MAX_SHEET_MM);
    }
    if let Some(l) = o.landscape {
        page.landscape = l;
    }
    if let Some(t) = &o.plot_style_table {
        page.plot_style_table = t.clone();
    }
    page.width_mm = page.width_mm.clamp(1.0, MAX_SHEET_MM);
    page.height_mm = page.height_mm.clamp(1.0, MAX_SHEET_MM);
    Ok(page)
}

/// World → paper-unit mapping: `p' = (p - from) * s + to`.
#[derive(Clone, Copy)]
pub(crate) struct Map {
    pub from: Vec2,
    pub s: f64,
    pub to: Vec2,
    /// Points per paper unit.
    pub k: f64,
}

impl Map {
    pub fn pt(&self, p: Vec2) -> Vec2 {
        ((p - self.from) * self.s + self.to) * self.k
    }
}

/// What a plot of one space needs, whatever the output format: the display list, where it lands on
/// the sheet, and the sheet itself. Shared by the PDF, SVG and PNG writers so they fit, scale and
/// clip alike.
pub(crate) struct Prepared {
    pub list: aad_render::DisplayList,
    pub map: Map,
    /// The sheet in points.
    pub media: Vec2,
    /// Where the drawing may be drawn, in points.
    pub clip_pt: Bounds2,
    pub title: String,
}

pub(crate) fn prepare(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<Prepared> {
    let page = page_for(d, space, o)?;
    let unit_mm = paper::paper_unit_mm(d);
    let sheet = Sheet::from_page(&page, unit_mm);
    let lineweights = o.lineweights.unwrap_or(match space {
        Space::Paper(_) => page.lineweights,
        Space::Model => true,
    });
    let plot_style_table = aad_doc::plot_style_table(d, &page.plot_style_table).map(std::sync::Arc::new);
    let ropts = aad_render::Options {
        tolerance: 0.001,
        min_dash: 0.0,
        text: true,
        fill: true,
        lineweights,
        view_height: 0.0,
        plot_style_table,
        ..Default::default()
    };
    let k = unit_mm * PT_PER_MM;
    let window = o.window.filter(|w| matches!(space, Space::Model) && !w.is_empty());
    let fit = window.is_some() || o.fit.unwrap_or(page.scale_to_fit || matches!(space, Space::Model));
    let scale = o.scale.filter(|s| s.is_finite() && *s > 0.0).or_else(|| (!fit && page.scale.is_finite() && page.scale > 0.0).then_some(page.scale));
    // Chord tolerance: about 0.05 mm on paper.
    let est = plot_scale(&window.unwrap_or_else(|| d.extents(space)), &sheet, fit, scale);
    let tol = 0.05 / unit_mm / est.max(1e-300);
    let tolerance = if tol.is_finite() && tol > 0.0 { tol } else { ropts.tolerance };
    // Relative point sizes follow the plotted window (else the extents).
    let view_height = window.map(|w| w.height()).unwrap_or(0.0);
    let list = aad_render::build_plot(d, space, &aad_render::Options { tolerance, view_height, ..ropts });
    let b = window.unwrap_or(list.bounds);
    let s = plot_scale(&b, &sheet, fit, scale);
    let map = if fit || matches!(space, Space::Model) {
        let from = if b.is_empty() { Vec2::ZERO } else { b.center() };
        Map { from, s, to: sheet.printable.center(), k }
    } else {
        // Layout at the page scale from the paper origin (1:1 when scale is 1).
        Map { from: Vec2::ZERO, s, to: Vec2::ZERO, k }
    };
    let media = sheet.size * k;
    let clip_pt = if fit || matches!(space, Space::Model) {
        Bounds2::new(sheet.printable.min * k, sheet.printable.max * k)
    } else {
        Bounds2::new(Vec2::ZERO, media)
    };
    let title = if o.title.is_empty() {
        match space {
            Space::Model => "Model".to_string(),
            Space::Paper(n) => n.clone(),
        }
    } else {
        o.title.clone()
    };
    Ok(Prepared { list, map, media, clip_pt, title })
}

/// Write a one-page vector PDF of a space.
pub fn pdf(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<Vec<u8>> {
    let pr = prepare(d, space, o)?;
    let layers = o.layers.unwrap_or(true).then(|| crate::layers::index(d, &pr.list));
    let content = content_stream(&pr.list, &pr.map, &pr.clip_pt, layers.as_ref());
    let names: &[String] = layers.as_ref().map_or(&[], |l| l.names.as_slice());
    Ok(assemble(pr.media, content.as_bytes(), o.compress, &pr.title, names))
}

/// Paper units per drawing unit for a plot.
fn plot_scale(b: &Bounds2, sheet: &Sheet, fit: bool, scale: Option<f64>) -> f64 {
    if let Some(s) = scale.filter(|s| s.is_finite() && *s > 0.0)
        && !fit
    {
        return s;
    }
    if !fit || b.is_empty() {
        return 1.0;
    }
    let pw = sheet.printable.width();
    let ph = sheet.printable.height();
    let s = (pw / b.width().max(1e-12)).min(ph / b.height().max(1e-12));
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// A number for a content stream: short, finite, no exponent, at most three decimals, no trailing zeros. Written straight into `out`: a heavy plot
/// writes tens of millions of these, so this does not allocate.
fn num(out: &mut String, v: f64) {
    let v = if v.is_finite() { v.clamp(-1.0e7, 1.0e7) } else { 0.0 };
    // Thousandths, rounded; the clamp keeps this far inside u64.
    let thousandths = (v.abs() * 1000.0).round() as u64;
    if thousandths == 0 {
        out.push('0');
        return;
    }
    if v < 0.0 {
        out.push('-');
    }
    push_u64(out, thousandths / 1000);
    let frac = thousandths % 1000;
    if frac != 0 {
        out.push('.');
        let digits = [b'0' + (frac / 100) as u8, b'0' + (frac / 10 % 10) as u8, b'0' + (frac % 10) as u8];
        let keep = if digits[2] != b'0' {
            3
        } else if digits[1] != b'0' {
            2
        } else {
            1
        };
        for d in &digits[..keep] {
            out.push(char::from(*d));
        }
    }
}

fn push_u64(out: &mut String, mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for d in &buf[i..] {
        out.push(char::from(*d));
    }
}

fn pt(out: &mut String, p: Vec2) {
    num(out, p.x);
    out.push(' ');
    num(out, p.y);
}

fn color(out: &mut String, c: Rgb, op: &str) {
    for v in [c.0, c.1, c.2] {
        num(out, f64::from(v) / 255.0);
        out.push(' ');
    }
    out.push_str(op);
    out.push('\n');
}

/// One drawn primitive in sheet points, ready for any writer.
pub(crate) enum Shape {
    /// An open polyline (lines, tessellated arcs and ellipses, clipped infinite lines).
    Stroke { pts: Vec<Vec2>, rgb: Rgb, width_pt: f64 },
    /// Filled triangles, three points each (solid fills; a wipeout's mask is paper white).
    Fill { pts: Vec<Vec2>, rgb: Rgb },
    /// A point marker: its centre and full size.
    Dot { at: Vec2, size: f64, rgb: Rgb },
}

/// The shape of primitive `p` on the sheet; `None` for one that draws nothing (not finite, fewer
/// than two points, off the clip).
pub(crate) fn shape(p: &aad_render::DPrim, list: &aad_render::DisplayList, map: &Map, clip_pt: &Bounds2) -> Option<Shape> {
    let white = Rgb(255, 255, 255);
    let rgb = p.display_rgb(white);
    let raw = list.points(p);
    if raw.iter().any(|q| !q.is_finite()) {
        return None;
    }
    let w = f64::from(p.lw) * PT_PER_MM;
    let width_pt = if w.is_finite() && w > 0.0 { w.min(100.0) } else { 0.0 };
    match p.kind {
        Kind::Polyline | Kind::Arc { .. } | Kind::Ellipse { .. } | Kind::Infinite { .. } => {
            let pts: Vec<Vec2> = match p.kind {
                Kind::Infinite { ray } => {
                    let (base, dir) = (raw.first()?, raw.get(1)?);
                    let a = map.pt(*base);
                    let dir_pt = map.pt(*base + *dir) - a;
                    let (x, y) = clip::clip_infinite(a, dir_pt, ray, clip_pt)?;
                    vec![x, y]
                }
                Kind::Arc { sweep } => {
                    let (center, rim) = (raw.first()?, raw.get(1)?);
                    let radius = center.dist(*rim);
                    let start = center.angle_to(*rim);
                    let mut tess = Vec::new();
                    aad_geom::Arc::new(*center, radius, start, start + aad_render::sweep_radians(sweep))
                        .tessellate((radius * 1e-3).max(1e-6), &mut tess);
                    tess.iter().map(|q| map.pt(*q)).collect()
                }
                Kind::Ellipse { start, sweep } => {
                    let (center, u_tip, v_tip) = (raw.first()?, raw.get(1)?, raw.get(2)?);
                    let u = *u_tip - *center;
                    let v = *v_tip - *center;
                    let sw = aad_render::sweep_radians(sweep);
                    let a0 = f64::from(start);
                    let n = 32usize.max(((sw.abs() / 0.05) as usize).min(512));
                    (0..=n)
                        .map(|i| {
                            let t = a0 + sw * i as f64 / n as f64;
                            map.pt(*center + u * t.cos() + v * t.sin())
                        })
                        .collect()
                }
                _ => raw.iter().map(|q| map.pt(*q)).collect(),
            };
            (pts.len() >= 2).then_some(Shape::Stroke { pts, rgb, width_pt })
        }
        Kind::Tris | Kind::Mask => {
            // A wipeout's mask is paper white.
            let rgb = if p.kind == Kind::Mask { white } else { rgb };
            let pts: Vec<Vec2> = raw.as_chunks::<3>().0.iter().flatten().map(|q| map.pt(*q)).collect();
            (!pts.is_empty()).then_some(Shape::Fill { pts, rgb })
        }
        Kind::Point => {
            let q = raw.first()?;
            Some(Shape::Dot { at: map.pt(*q), size: width_pt.max(0.5), rgb })
        }
    }
}

fn content_stream(list: &aad_render::DisplayList, map: &Map, clip_pt: &Bounds2, layers: Option<&crate::layers::LayerIndex>) -> String {
    let mut c = String::new();
    c.push_str("q\n1 J 1 j\n");
    // Clip to the plot area.
    pt(&mut c, clip_pt.min);
    c.push(' ');
    num(&mut c, clip_pt.width());
    c.push(' ');
    num(&mut c, clip_pt.height());
    c.push_str(" re W n\n");
    let mut stroke: Option<Rgb> = None;
    let mut fill: Option<Rgb> = None;
    let mut width: Option<f64> = None;
    let mut pending = false;
    let mut open_group: Option<usize> = None;
    let flush = |c: &mut String, pending: &mut bool| {
        if *pending {
            c.push_str("S\n");
            *pending = false;
        }
    };
    for (i, p) in list.prims.iter().enumerate() {
        let Some(shape) = shape(p, list, map, clip_pt) else { continue };
        if let Some(ix) = layers {
            // Each run of one layer is one marked-content section, so the draw order is kept.
            let g = ix.of_prim.get(i).copied().unwrap_or(0);
            if open_group != Some(g) {
                flush(&mut c, &mut pending);
                if open_group.is_some() {
                    c.push_str("EMC\n");
                }
                let _ = writeln!(c, "/OC /L{g} BDC");
                open_group = Some(g);
            }
        }
        match shape {
            Shape::Stroke { pts, rgb, width_pt } => {
                if stroke != Some(rgb) || width != Some(width_pt) {
                    flush(&mut c, &mut pending);
                    if stroke != Some(rgb) {
                        color(&mut c, rgb, "RG");
                        stroke = Some(rgb);
                    }
                    if width != Some(width_pt) {
                        num(&mut c, width_pt);
                        c.push_str(" w\n");
                        width = Some(width_pt);
                    }
                }
                for (i, q) in pts.iter().enumerate() {
                    pt(&mut c, *q);
                    c.push_str(if i == 0 { " m\n" } else { " l\n" });
                }
                pending = true;
            }
            Shape::Fill { pts, rgb } => {
                flush(&mut c, &mut pending);
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                for t in pts.chunks(3) {
                    for (i, q) in t.iter().enumerate() {
                        pt(&mut c, *q);
                        c.push_str(if i == 0 { " m\n" } else { " l\n" });
                    }
                    c.push_str("h\n");
                }
                c.push_str("f\n");
            }
            Shape::Dot { at, size, rgb } => {
                flush(&mut c, &mut pending);
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                let half = size / 2.0;
                pt(&mut c, at - Vec2::new(half, half));
                c.push(' ');
                num(&mut c, half * 2.0);
                c.push(' ');
                num(&mut c, half * 2.0);
                c.push_str(" re f\n");
            }
        }
    }
    flush(&mut c, &mut pending);
    if open_group.is_some() {
        c.push_str("EMC\n");
    }
    c.push_str("Q\n");
    c
}

/// A PDF literal string with `(`, `)` and `\` escaped; non-ASCII becomes `?`.
fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for ch in s.chars().take(200) {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// A PDF text string (UTF-16BE with a byte-order mark when it is not plain ASCII), for layer names.
fn pdf_text(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(120).collect();
    if s.is_ascii() {
        return pdf_string(&s);
    }
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out.push('>');
    out
}

/// Content bigger than this is compressed in independent chunks on several threads.
const PARALLEL_DEFLATE_FROM: usize = 8 << 20;
/// Size of one independently compressed chunk. Large enough that resetting the dictionary costs next to nothing.
const DEFLATE_CHUNK: usize = 4 << 20;

/// `data` as one zlib stream. A heavy plot's content stream is hundreds of megabytes, and compressing it on one thread was most of the export time, so big
/// input is cut into chunks that are compressed at the same time. Each chunk is ended with a full flush (an empty stored block, dictionary reset), which
/// makes the chunks join into one valid deflate stream; the checksum of the pieces is combined.
fn zlib(data: &[u8], level: u8) -> Vec<u8> {
    #[cfg(not(target_arch = "wasm32"))]
    if data.len() >= PARALLEL_DEFLATE_FROM {
        return zlib_parallel(data, level);
    }
    miniz_oxide::deflate::compress_to_vec_zlib(data, level)
}

#[cfg(not(target_arch = "wasm32"))]
fn zlib_parallel(data: &[u8], level: u8) -> Vec<u8> {
    use miniz_oxide::deflate::core::{CompressorOxide, TDEFLFlush, compress_to_output, create_comp_flags_from_zip_params};
    let chunks: Vec<&[u8]> = data.chunks(DEFLATE_CHUNK).collect();
    let n = chunks.len();
    let workers = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get).clamp(1, 16).min(n);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done: Vec<(usize, Vec<u8>, u32)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(chunk) = chunks.get(i) else { break };
                        // Raw deflate (no zlib wrapper) per chunk: window bits 0.
                        let mut c = CompressorOxide::new(create_comp_flags_from_zip_params(i32::from(level), 0, 0));
                        let mut out = Vec::with_capacity(chunk.len() / 6 + 64);
                        let flush = if i + 1 == n { TDEFLFlush::Finish } else { TDEFLFlush::Full };
                        compress_to_output(&mut c, chunk, flush, |b| {
                            out.extend_from_slice(b);
                            true
                        });
                        mine.push((i, out, adler32(chunk)));
                    }
                    mine
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    let mut parts = done;
    parts.sort_by_key(|(i, _, _)| *i);
    // The two header bytes the library writes for this level.
    let head = miniz_oxide::deflate::compress_to_vec_zlib(&[], level);
    let mut out = Vec::with_capacity(parts.iter().map(|(_, b, _)| b.len()).sum::<usize>() + 6);
    out.extend_from_slice(&head[..2]);
    let mut sum = 1u32;
    for (i, bytes, a) in &parts {
        out.extend_from_slice(bytes);
        sum = adler32_combine(sum, *a, chunks[*i].len());
    }
    out.extend_from_slice(&sum.to_be_bytes());
    out
}

const ADLER_BASE: u32 = 65_521;

/// The Adler-32 checksum of `data`.
#[cfg(not(target_arch = "wasm32"))]
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for block in data.chunks(5552) {
        for &x in block {
            a += u32::from(x);
            b += a;
        }
        a %= ADLER_BASE;
        b %= ADLER_BASE;
    }
    (b << 16) | a
}

/// The Adler-32 of two pieces joined, from the checksum of each and the length of the second.
#[cfg(not(target_arch = "wasm32"))]
fn adler32_combine(adler1: u32, adler2: u32, len2: usize) -> u32 {
    let rem = (len2 % ADLER_BASE as usize) as u32;
    let mut sum1 = adler1 & 0xffff;
    let mut sum2 = (rem * sum1) % ADLER_BASE;
    sum1 += (adler2 & 0xffff) + ADLER_BASE - 1;
    sum2 += ((adler1 >> 16) & 0xffff) + ((adler2 >> 16) & 0xffff) + ADLER_BASE - rem;
    if sum1 >= ADLER_BASE {
        sum1 -= ADLER_BASE;
    }
    if sum1 >= ADLER_BASE {
        sum1 -= ADLER_BASE;
    }
    if sum2 >= (ADLER_BASE << 1) {
        sum2 -= ADLER_BASE << 1;
    }
    if sum2 >= ADLER_BASE {
        sum2 -= ADLER_BASE;
    }
    sum1 | (sum2 << 16)
}

/// Assemble the file: header, the catalog, the page tree, the page, its content, the info
/// dictionary, one optional-content group per layer, then the cross-reference table and trailer.
fn assemble(media: Vec2, content: &[u8], compress: bool, title: &str, layers: &[String]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(content.len() + 1024);
    // Optional content needs PDF 1.5.
    out.extend_from_slice(if layers.is_empty() { b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n" } else { b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n" });
    let mut offsets: Vec<usize> = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // Objects 1-5 are fixed; the groups follow as 6, 7, ...
    let group_refs: Vec<String> = (0..layers.len()).map(|i| format!("{} 0 R", 6 + i)).collect();
    if layers.is_empty() {
        obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    } else {
        let all = group_refs.join(" ");
        obj(&mut out, format!("<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [{all}] /D << /Order [{all}] /ON [{all}] >> >> >>").as_bytes());
    }
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let mut mb = String::new();
    num(&mut mb, media.x);
    mb.push(' ');
    num(&mut mb, media.y);
    let resources = if layers.is_empty() {
        "<< >>".to_string()
    } else {
        let props: String = group_refs.iter().enumerate().map(|(i, r)| format!("/L{i} {r} ")).collect();
        format!("<< /Properties << {props}>> >>")
    };
    obj(&mut out, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {mb}] /Resources {resources} /Contents 4 0 R >>").as_bytes());
    let (data, filter) = if compress { (zlib(content, 6), " /Filter /FlateDecode") } else { (content.to_vec(), "") };
    let mut stream = format!("<< /Length {}{filter} >>\nstream\n", data.len()).into_bytes();
    stream.extend_from_slice(&data);
    stream.extend_from_slice(b"\nendstream");
    obj(&mut out, &stream);
    obj(&mut out, format!("<< /Producer (AAD) /Creator (AAD) /Title {} >>", pdf_string(title)).as_bytes());
    for name in layers {
        obj(&mut out, format!("<< /Type /OCG /Name {} >>", pdf_text(name)).as_bytes());
    }
    let xref = out.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(x, "{o:010} 00000 n ");
    }
    let _ = write!(x, "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out.extend_from_slice(x.as_bytes());
    out
}

/// Join one-page plot PDFs into one file, in order. Each part must be a PDF this plotter
/// (or any reader lopdf accepts) can open. Page parents are rewritten onto the first file's page tree.
pub fn merge(parts: &[Vec<u8>]) -> Result<Vec<u8>> {
    if parts.is_empty() {
        return Err(IoError::Format("nothing to merge".into()));
    }
    if parts.len() > MAX_MERGE {
        return Err(IoError::Format(format!("at most {MAX_MERGE} pages can be joined")));
    }
    if parts.len() == 1 {
        return Ok(parts[0].clone());
    }
    join(parts)
}

/// The web build has no PDF reader (`lopdf` is native only), so joining several plots is not available there.
#[cfg(target_arch = "wasm32")]
fn join(_parts: &[Vec<u8>]) -> Result<Vec<u8>> {
    Err(IoError::Format("joining PDFs is not available in the web build".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn join(parts: &[Vec<u8>]) -> Result<Vec<u8>> {
    let mut base = lopdf::Document::load_mem(&parts[0]).map_err(|e| IoError::Format(format!("not a readable PDF: {e}")))?;
    for part in parts.iter().skip(1) {
        append_pdf(&mut base, part)?;
    }
    let mut buf = Vec::new();
    base.save_to(&mut buf).map_err(|e| IoError::Format(format!("could not write the joined PDF: {e}")))?;
    Ok(buf)
}

#[cfg(not(target_arch = "wasm32"))]
fn append_pdf(base: &mut lopdf::Document, bytes: &[u8]) -> Result<()> {
    let mut extra = lopdf::Document::load_mem(bytes).map_err(|e| IoError::Format(format!("not a readable PDF: {e}")))?;
    let start = base.max_id.saturating_add(1);
    extra.renumber_objects_with(start);
    let pages: Vec<lopdf::ObjectId> = extra.get_pages().into_values().collect();
    if pages.is_empty() {
        return Err(IoError::Format("a plot PDF has no pages".into()));
    }
    base.max_id = base.max_id.max(extra.max_id);
    for (id, object) in extra.objects {
        base.objects.insert(id, object);
    }
    let tree = page_tree_id(base)?;
    {
        let dict = base.get_object_mut(tree).and_then(|o| o.as_dict_mut()).map_err(|e| IoError::Format(e.to_string()))?;
        let kids = dict.get_mut(b"Kids").and_then(|o| o.as_array_mut()).map_err(|e| IoError::Format(format!("page tree has no Kids array: {e}")))?;
        for id in &pages {
            kids.push(lopdf::Object::Reference(*id));
        }
    }
    {
        let dict = base.get_object_mut(tree).and_then(|o| o.as_dict_mut()).map_err(|e| IoError::Format(e.to_string()))?;
        let count = dict.get(b"Count").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
        dict.set("Count", count.saturating_add(pages.len() as i64));
    }
    for id in pages {
        if let Ok(page) = base.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
            page.set("Parent", lopdf::Object::Reference(tree));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn page_tree_id(doc: &lopdf::Document) -> Result<lopdf::ObjectId> {
    let catalog = doc.catalog().map_err(|e| IoError::Format(e.to_string()))?;
    catalog.get(b"Pages").and_then(lopdf::Object::as_reference).map_err(|e| IoError::Format(format!("PDF has no page tree: {e}")))
}

/// `window: [x0, y0, x1, y1]` as a rectangle (any corner order); `None` when absent or not four
/// finite numbers with an area.
fn window_from_json(v: &Value) -> Option<Bounds2> {
    let a: Vec<f64> = v.get("window")?.as_array()?.iter().filter_map(Value::as_f64).collect();
    match a[..] {
        [x0, y0, x1, y1] if a.iter().all(|n| n.is_finite() && n.abs() < 1e12) && x0 != x1 && y0 != y1 => {
            Some(Bounds2::new(Vec2::new(x0.min(x1), y0.min(y1)), Vec2::new(x0.max(x1), y0.max(y1))))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use aad_doc::{Drawing, Space};

    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn two_plots_join_into_one_pdf_with_two_pages() {
        let drawing = Drawing::new_metric();
        let first = pdf(&drawing, &Space::Model, &PdfOptions::default()).unwrap();
        let second = pdf(&drawing, &Space::Model, &PdfOptions::default()).unwrap();
        let joined = merge(&[first, second]).unwrap();
        let doc = lopdf::Document::load_mem(&joined).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        assert!(merge(&[]).is_err());
        assert!(merge(&vec![vec![b'x'; 8]; 257]).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn numbers_are_short_finite_and_have_no_exponent() {
        let s = |v: f64| {
            let mut o = String::new();
            num(&mut o, v);
            o
        };
        assert_eq!(s(0.0), "0");
        assert_eq!(s(-0.0), "0");
        assert_eq!(s(-0.0004), "0");
        assert_eq!(s(1.0), "1");
        assert_eq!(s(1.5), "1.5");
        assert_eq!(s(-2.25), "-2.25");
        assert_eq!(s(0.125), "0.125");
        assert_eq!(s(0.0006), "0.001");
        assert_eq!(s(123456.7894), "123456.789");
        assert_eq!(s(f64::NAN), "0");
        assert_eq!(s(f64::INFINITY), "0");
        assert_eq!(s(1e30), "10000000");
        assert_eq!(s(-1e30), "-10000000");
        assert_eq!(s(100.0), "100");
        assert_eq!(s(0.1), "0.1");
    }

    /// A big stream is compressed in chunks on several threads; it must read back as exactly the input, with a correct zlib checksum.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_big_stream_compressed_in_chunks_reads_back_unchanged() {
        use std::io::Write as _;
        let mut data = Vec::new();
        let mut x = 12345u64;
        while data.len() < PARALLEL_DEFLATE_FROM + 3 * DEFLATE_CHUNK + 777 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let _ = writeln!(&mut data, "{} {} m\n{} {} l", (x >> 40) % 5000, (x >> 20) % 3000, (x >> 8) % 4000, x % 2000);
        }
        let packed = zlib(&data, 6);
        assert!(packed.len() < data.len() / 2, "it compresses");
        assert_eq!(miniz_oxide::inflate::decompress_to_vec_zlib(&packed).unwrap(), data);
        // The combined checksum equals one pass over the whole input.
        let whole = adler32(&data);
        assert_eq!(packed[packed.len() - 4..], whole.to_be_bytes());
        // A small stream still takes the single-thread path.
        let small = zlib(&data[..1000], 6);
        assert_eq!(miniz_oxide::inflate::decompress_to_vec_zlib(&small).unwrap(), &data[..1000]);
    }

    /// Where the time of a plot goes, on a real drawing: `AAD_PROBE_FILE=<dwg or dxf> cargo test -p aad-io --release -- --ignored --nocapture where_the_plot_time_goes`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "a measurement, needs AAD_PROBE_FILE"]
    fn where_the_plot_time_goes() {
        use std::time::Instant;
        let Ok(path) = std::env::var("AAD_PROBE_FILE") else { return };
        let bytes = std::fs::read(&path).unwrap();
        let drawing = crate::read(&bytes, &path).unwrap();
        let o = PdfOptions::default();
        let t = Instant::now();
        let pr = prepare(&drawing, &Space::Model, &o).unwrap();
        let t_prepare = t.elapsed();
        let t = Instant::now();
        let layers = crate::layers::index(&drawing, &pr.list);
        let t_layers = t.elapsed();
        let t = Instant::now();
        let content = content_stream(&pr.list, &pr.map, &pr.clip_pt, Some(&layers));
        let t_content = t.elapsed();
        let t = Instant::now();
        let packed = assemble(pr.media, content.as_bytes(), true, &pr.title, &layers.names);
        let t_deflate = t.elapsed();
        let t = Instant::now();
        let plain = assemble(pr.media, content.as_bytes(), false, &pr.title, &layers.names);
        let t_plain = t.elapsed();
        println!(
            "prims {} | prepare {:?} | layers {:?} | content_stream {:?} ({} bytes) | assemble+deflate {:?} ({} bytes) | assemble no compression {:?} ({} bytes)",
            pr.list.prims.len(),
            t_prepare,
            t_layers,
            t_content,
            content.len(),
            t_deflate,
            packed.len(),
            t_plain,
            plain.len()
        );
    }
}

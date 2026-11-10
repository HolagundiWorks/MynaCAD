//! AAD file formats: DXF read/write, SVG, PNG and PDF export (plotting).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod dxf_ext;
mod dxf_image;
mod dxf_read;
mod dxf_write;
mod layers;
mod levels_file;
pub mod pdf;
#[cfg(not(target_arch = "wasm32"))]
pub mod pdfread;
#[cfg(not(target_arch = "wasm32"))]
pub mod raster;
pub mod svg;

use aad_doc::{Drawing, Space};
use aad_geom::Bounds2;

pub use pdf::{PdfOptions, pdf};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Format(String),
    #[error("unsupported file type `{0}`")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

fn ext(name: &str) -> String {
    std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Read a drawing; the format comes from the content (DXF ASCII/binary) or the name.
pub fn read(bytes: &[u8], name: &str) -> Result<Drawing> {
    if aad_dwg::is_dwg(bytes) {
        let dxf = aad_dwg::dwg_to_dxf(bytes).map_err(IoError::Format)?;
        return dxf_read::read(&dxf).map(levels_file::from_file).map_err(|e| IoError::Format(format!("{e} (in the DXF converted from this DWG)")));
    }
    match ext(name).as_str() {
        "dxf" | "" => dxf_read::read(bytes).map(levels_file::from_file),
        e if bytes.len() > 4 => dxf_read::read(bytes).map(levels_file::from_file).map_err(|_| IoError::Unsupported(e.to_string())),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// Write a drawing in the format chosen by the name's extension. Images (PNG, SVG, PDF) show
/// model space fitted to its extents.
pub fn write(d: &Drawing, name: &str) -> Result<Vec<u8>> {
    write_framed(d, name, None)
}

/// Like [`write`], but image formats (PNG, SVG, PDF) show `window` (a model-space rectangle in
/// drawing units, centred and scaled to fit the image without distortion) instead of the
/// extents. Formats that are not images (DXF, DWG) ignore the window. The window is checked with
/// [`check_window`].
pub fn write_framed(d: &Drawing, name: &str, window: Option<Bounds2>) -> Result<Vec<u8>> {
    if let Some(w) = &window
        && is_image(name)
    {
        check_window(w)?;
    }
    match ext(name).as_str() {
        "dxf" | "" => Ok(dxf_write::write(&levels_file::to_file(d)).into_bytes()),
        "dwg" => aad_dwg::dxf_to_dwg(dxf_write::write(&levels_file::to_file(d)).as_bytes()).map_err(IoError::Format),
        "svg" => Ok(svg::export_window(d, &Space::Model, window).into_bytes()),
        "png" => png_framed(d, &Space::Model, 2400, 1600, window),
        "pdf" => pdf::pdf(d, &Space::Model, &PdfOptions { compress: true, window, ..PdfOptions::default() }),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// Whether `name` is written as an image (PNG, SVG, PDF), i.e. whether framing applies to it.
pub fn is_image(name: &str) -> bool {
    matches!(ext(name).as_str(), "png" | "svg" | "pdf")
}

/// Largest coordinate magnitude accepted in an export window.
pub const MAX_WINDOW_COORD: f64 = 1.0e12;

/// Validate an export window: finite corners within ±[`MAX_WINDOW_COORD`], and a width and height
/// that are positive and not vanishingly small next to the coordinates (at least 1e-9 of the
/// largest coordinate magnitude, and at least 1e-9 drawing units).
pub fn check_window(w: &Bounds2) -> Result<()> {
    let coords = [w.min.x, w.min.y, w.max.x, w.max.y];
    if coords.iter().any(|c| !c.is_finite()) {
        return Err(IoError::Format("export window: coordinates must be finite numbers".into()));
    }
    let mag = coords.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    if mag > MAX_WINDOW_COORD {
        return Err(IoError::Format(format!("export window: coordinates must lie within ±{MAX_WINDOW_COORD:e}")));
    }
    let min_side = mag.max(1.0) * 1e-9;
    let (width, height) = (w.max.x - w.min.x, w.max.y - w.min.y);
    if !(width >= min_side && height >= min_side) {
        return Err(IoError::Format(format!("export window: width and height must be positive (got {width} x {height})")));
    }
    Ok(())
}

/// Plot a space with JSON options (the engine's `plot` hook). `format` picks the output: `pdf`
/// (default), `svg` or `png` (with `dpi`, default 150). All three lay the sheet out alike
/// (`paper`, `landscape`, `fit`, `scale`, `window`, `lineweights`, `plotStyleTable`); `layers`
/// (default on) keeps the drawing layers as PDF optional-content groups or SVG layer groups.
pub fn plot(d: &Drawing, space: &Space, opts: &serde_json::Value) -> Result<Vec<u8>> {
    let o = PdfOptions::from_json(opts);
    match opts.get("format").and_then(serde_json::Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("pdf") => pdf::pdf(d, space, &o),
        Some("svg") => svg::plot(d, space, &o).map(String::into_bytes),
        Some("png") => {
            let dpi = match opts.get("dpi") {
                None | Some(serde_json::Value::Null) => 150.0,
                Some(v) => v.as_f64().ok_or_else(|| IoError::Format("`dpi` must be a number".into()))?,
            };
            png_at_dpi(d, space, &o, dpi)
        }
        Some(other) => Err(IoError::Format(format!("unknown plot format `{other}` (pdf, svg, png)"))),
    }
}

/// Lowest and highest dots per inch a raster export accepts.
pub const DPI_RANGE: (f64, f64) = (36.0, 1200.0);
/// Most pixels along one side of a raster export (the rasteriser's limit).
const MAX_SIDE_PX: f64 = 16384.0;
/// Most pixels in a raster export (about 400 MB of RGBA).
const MAX_PIXELS: f64 = 100_000_000.0;

/// Render a plot of `space` (a layout at its paper size, or model space fitted to a sheet) to PNG at
/// `dpi` dots per inch: the image is the whole sheet, `paper size / 25.4 * dpi` pixels, drawn with
/// the printed colours on white and the lineweights at their true width. The file records the
/// resolution (`pHYs`), so it opens at its paper size in viewers and page-layout programs.
pub fn png_at_dpi(d: &Drawing, space: &Space, o: &PdfOptions, dpi: f64) -> Result<Vec<u8>> {
    if !dpi.is_finite() || dpi < DPI_RANGE.0 || dpi > DPI_RANGE.1 {
        return Err(IoError::Format(format!("`dpi` must be between {} and {}", DPI_RANGE.0, DPI_RANGE.1)));
    }
    let pr = pdf::prepare(d, space, o)?;
    // Sheet points -> pixels.
    let px_per_pt = dpi / 72.0;
    let (w, h) = ((pr.media.x * px_per_pt).round(), (pr.media.y * px_per_pt).round());
    if !(w >= 1.0 && h >= 1.0) || w > MAX_SIDE_PX || h > MAX_SIDE_PX || w * h > MAX_PIXELS {
        let side = pr.media.x.max(pr.media.y) / 72.0;
        let max_dpi = (MAX_SIDE_PX / side.max(1e-9)).min((MAX_PIXELS / (pr.media.x * pr.media.y / 5184.0).max(1e-9)).sqrt());
        return Err(IoError::Format(format!(
            "a {:.0} x {:.0} px image is too large ({:.0} megapixels at most, {:.0} px per side); at most {:.0} dpi for this sheet",
            w,
            h,
            MAX_PIXELS / 1e6,
            MAX_SIDE_PX,
            max_dpi.floor().max(DPI_RANGE.0)
        )));
    }
    let (w, h) = (w as u32, h as u32);
    // The world point at the image centre and pixels per drawing unit, from the plot's own mapping.
    let m = pr.map;
    let center = m.from + (pr.media / 2.0 / m.k - m.to) / m.s;
    let view = aad_render::raster::View { center, scale: m.s * m.k * px_per_pt, width: w, height: h };
    let px_per_mm = (dpi / 25.4) as f32;
    let opts = aad_render::raster::RasterOptions {
        background: aad_color::Rgb(255, 255, 255),
        hairline: (px_per_mm * 0.1).max(1.0),
        px_per_mm,
        antialias: true,
    };
    let png = aad_render::raster::render_png(&pr.list, &view, &opts).ok_or_else(|| IoError::Format("render failed".into()))?;
    Ok(with_resolution(png, dpi))
}

/// `png` with a `pHYs` chunk giving `dpi` dots per inch (inserted after the header chunk); the bytes
/// are returned unchanged when they are not a PNG.
fn with_resolution(png: Vec<u8>, dpi: f64) -> Vec<u8> {
    const SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    // Signature (8) + IHDR chunk (4 length + 4 type + 13 data + 4 crc).
    const AFTER_IHDR: usize = 8 + 25;
    if png.len() < AFTER_IHDR || !png.starts_with(SIGNATURE) || &png[12..16] != b"IHDR" {
        return png;
    }
    let per_metre = (dpi / 0.0254).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&per_metre.to_be_bytes());
    chunk.extend_from_slice(&per_metre.to_be_bytes());
    chunk.push(1); // metres
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..AFTER_IHDR]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[AFTER_IHDR..]);
    out
}

/// CRC-32 (the PNG chunk checksum).
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Render a space to PNG, fitted to its extents.
pub fn png(d: &Drawing, space: &Space, width: u32, height: u32) -> Result<Vec<u8>> {
    png_framed(d, space, width, height, None)
}

/// Render a space to PNG showing `window` (centred, fitted without margin), or fitted to its
/// extents (5% margin) when `window` is `None`.
pub fn png_framed(d: &Drawing, space: &Space, width: u32, height: u32, window: Option<Bounds2>) -> Result<Vec<u8>> {
    // Draw at the detail the picture can show: curves flattened to a quarter of a pixel, text that is a few pixels tall as a bar and under a pixel not at
    // all, hatches finer than a pixel as a tint. Without this a heavy drawing is triangulated at 0.001 drawing units and the picture is mostly work nobody can see.
    let px = 1.0 / png_view(&window.unwrap_or_else(|| d.extents(space)), width, height, window).scale;
    let opts = if px.is_finite() && px > 0.0 {
        aad_render::Options { tolerance: px * 0.25, screen_px: px, ..aad_render::Options::default() }
    } else {
        aad_render::Options::default()
    };
    let list = aad_render::build(d, space, &opts);
    let view = png_view(&list.bounds, width, height, window);
    aad_render::raster::render_png(&list, &view, &aad_render::raster::RasterOptions::default()).ok_or_else(|| IoError::Format("render failed".into()))
}

/// The raster view a PNG export uses.
pub fn png_view(extents: &Bounds2, width: u32, height: u32, window: Option<Bounds2>) -> aad_render::raster::View {
    match window {
        Some(w) => aad_render::raster::View::fit(&w, width, height, 0.0),
        None => aad_render::raster::View::fit(extents, width, height, 0.05),
    }
}

pub fn read_dxf(bytes: &[u8]) -> Result<Drawing> {
    dxf_read::read(bytes).map(levels_file::from_file)
}
pub fn write_dxf(d: &Drawing) -> String {
    dxf_write::write(&levels_file::to_file(d))
}

#[cfg(test)]
mod tests;

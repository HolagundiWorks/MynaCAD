//! Optional Vello quality spike for paper-space / plot-preview PNG comparison.
//!
//! **Prototype / proposed** until sheet quality is measured against the custom `tiny-skia`
//! path ([docs/canvas-smoothness.md](../../../docs/canvas-smoothness.md) Phase 3 #12).
//!
//! - Env flag: [`ENV_CANVAS`] (`AAD_CANVAS`). Value `vello` (case-insensitive) selects this path;
//!   unset or any other value keeps the custom mesh / `aad_render::raster` path.
//! - Layering: this crate is L5 only. It must never move into `aad-render` (L2).
//! - The interactive model-space canvas stays on the custom mesh/wgpu presenters; this crate
//!   does not touch GPU present.
//! - Implementation uses Linebender [`vello_cpu`] (Apache-2.0 OR MIT), not GPL code and not
//!   proprietary third-party CAD assets. Full GPU `vello` remains a possible follow-up if CPU compare wins on sheets.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use aad_color::Rgb;
use aad_geom::{Bounds2, Vec2};
use aad_render::raster::{RasterOptions, View};
use aad_render::{DPrim, DisplayList, Kind};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{BezPath, Rect, Stroke};
use vello_cpu::{Pixmap, RenderContext, Resources};

/// Environment variable that selects the canvas / preview raster backend.
///
/// | Value | Behaviour |
/// |---|---|
/// | unset / empty / other | Custom path (`aad_render::raster` + mesh/wgpu interactive) |
/// | `vello` (any case) | This crate’s `vello_cpu` PNG path for control/`ui.render` / plot-preview compare |
pub const ENV_CANVAS: &str = "AAD_CANVAS";

/// Which PNG / preview backend is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasBackend {
    /// Default: `aad_render::raster` (tiny-skia) and the custom mesh interactive canvas.
    Custom,
    /// Experimental `vello_cpu` paper/plot PNG path (this crate).
    Vello,
}

impl CanvasBackend {
    /// Parse a raw env value the same way [`from_env`] does.
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            Some(v) if v.eq_ignore_ascii_case("vello") => Self::Vello,
            _ => Self::Custom,
        }
    }

    /// Read [`ENV_CANVAS`] from the process environment.
    pub fn from_env() -> Self {
        Self::parse(std::env::var(ENV_CANVAS).ok().as_deref())
    }

    pub fn is_vello(self) -> bool {
        matches!(self, Self::Vello)
    }
}

/// PNG for control `ui.render` / plot-preview compare: dispatches on [`CanvasBackend::from_env`].
///
/// Default (flag off) is identical to `aad_render::raster::render_png`.
pub fn preview_png(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Vec<u8>> {
    preview_png_with(CanvasBackend::from_env(), list, view, o)
}

/// Same as [`preview_png`] with an explicit backend (tests and tooling).
pub fn preview_png_with(backend: CanvasBackend, list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Vec<u8>> {
    match backend {
        CanvasBackend::Custom => aad_render::raster::render_png(list, view, o),
        CanvasBackend::Vello => render_png(list, view, o),
    }
}

/// Rasterise a display list with `vello_cpu` and encode PNG.
///
/// Returns `None` for zero/absurd sizes, encode failure, or if `vello_cpu` panics
/// (caught — never-crash). Does not consult [`ENV_CANVAS`]; callers that want the flag
/// should use [`preview_png`].
pub fn render_png(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Vec<u8>> {
    let pm = render(list, view, o)?;
    pm.into_png().ok()
}

/// Same contract as [`aad_render::raster::render`], but via `vello_cpu`.
pub fn render(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Pixmap> {
    if view.width == 0 || view.height == 0 || view.width > 16384 || view.height > 16384 {
        return None;
    }
    let w = u16::try_from(view.width).ok()?;
    let h = u16::try_from(view.height).ok()?;
    // vello_cpu documents panics for unsupported filter graphs; we never push filters, but
    // catch_unwind still keeps a hostile path from aborting the host.
    catch_unwind(AssertUnwindSafe(|| render_inner(list, view, o, w, h))).ok().flatten()
}

fn render_inner(list: &DisplayList, view: &View, o: &RasterOptions, w: u16, h: u16) -> Option<Pixmap> {
    let mut ctx = RenderContext::new(w, h);
    let mut resources = Resources::new();
    let bg = solid(o.background);
    ctx.set_paint(bg);
    ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));

    let vis = view.world_bounds();
    for p in &list.prims {
        let c = p.display_rgb(o.background);
        ctx.set_paint(solid(c));
        let pts = list.points(p);
        match p.kind {
            Kind::Polyline => {
                let bb = Bounds2::from_points(pts.iter().copied());
                if !bb.intersects(&vis) {
                    continue;
                }
                stroke_points(&mut ctx, view, pts, p, o);
            }
            Kind::Arc { sweep } => {
                let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) else { continue };
                let radius = center.dist(*rim);
                let start = center.angle_to(*rim);
                let mut tess = Vec::new();
                aad_geom::Arc::new(*center, radius, start, start + aad_render::sweep_radians(sweep)).tessellate((radius * 1e-3).max(1e-6), &mut tess);
                stroke_points(&mut ctx, view, &tess, p, o);
            }
            Kind::Ellipse { start, sweep } => {
                let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) else {
                    continue;
                };
                let u = *u_tip - *center;
                let v = *v_tip - *center;
                let mut tess = Vec::new();
                let sw = aad_render::sweep_radians(sweep);
                let a0 = f64::from(start);
                let n = 32usize.max(((sw.abs() / 0.05) as usize).min(512));
                for i in 0..=n {
                    let t = a0 + sw * i as f64 / n as f64;
                    tess.push(*center + u * t.cos() + v * t.sin());
                }
                stroke_points(&mut ctx, view, &tess, p, o);
            }
            Kind::Tris | Kind::Mask => {
                if p.kind == Kind::Mask {
                    ctx.set_paint(bg);
                }
                fill_tris(&mut ctx, view, pts);
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    let (x, y) = view.to_screen(*q);
                    ctx.fill_rect(&Rect::new(f64::from(x) - 0.5, f64::from(y) - 0.5, f64::from(x) + 1.0, f64::from(y) + 1.0));
                }
            }
            Kind::Infinite { ray } => {
                let (Some(base), Some(dir)) = (pts.first(), pts.get(1)) else { continue };
                let far = (vis.width() + vis.height()) * 2.0 + base.dist(vis.center());
                let a = if ray { *base } else { *base - *dir * far };
                let b = *base + *dir * far;
                stroke_points(&mut ctx, view, &[a, b], p, o);
            }
        }
    }

    ctx.flush();
    let mut target = Pixmap::new(w, h);
    ctx.render(&mut target, &mut resources);
    Some(target)
}

fn solid(c: Rgb) -> AlphaColor<vello_cpu::color::Srgb> {
    AlphaColor::from_rgba8(c.0, c.1, c.2, 255)
}

fn stroke_width(p: &DPrim, o: &RasterOptions) -> f64 {
    let w = if o.px_per_mm > 0.0 && p.lw > 0.0 { (p.lw * o.px_per_mm).max(o.hairline) } else { o.hairline };
    f64::from(w)
}

fn stroke_points(ctx: &mut RenderContext, view: &View, pts: &[Vec2], p: &DPrim, o: &RasterOptions) {
    if pts.len() < 2 {
        return;
    }
    let mut path = BezPath::new();
    for (i, q) in pts.iter().enumerate() {
        let (x, y) = view.to_screen(*q);
        if i == 0 {
            path.move_to((f64::from(x), f64::from(y)));
        } else {
            path.line_to((f64::from(x), f64::from(y)));
        }
    }
    ctx.set_stroke(Stroke::new(stroke_width(p, o)));
    ctx.stroke_path(&path);
}

fn fill_tris(ctx: &mut RenderContext, view: &View, pts: &[Vec2]) {
    for t in pts.chunks(3) {
        let [a, b, c] = t else { continue };
        let (ax, ay) = view.to_screen(*a);
        let (bx, by) = view.to_screen(*b);
        let (cx, cy) = view.to_screen(*c);
        let mut path = BezPath::new();
        path.move_to((f64::from(ax), f64::from(ay)));
        path.line_to((f64::from(bx), f64::from(by)));
        path.line_to((f64::from(cx), f64::from(cy)));
        path.close_path();
        ctx.fill_path(&path);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use aad_doc::{Common, Drawing, EntityKind, Line, Space};
    use aad_geom::Vec3;
    use aad_render::Options;

    fn line_list() -> DisplayList {
        let mut d = Drawing::new_imperial();
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) }))
            .expect("add line");
        aad_render::build(&d, &Space::Model, &Options::default())
    }

    #[test]
    fn canvas_backend_defaults_to_custom() {
        assert_eq!(CanvasBackend::parse(None), CanvasBackend::Custom);
        assert_eq!(CanvasBackend::parse(Some("")), CanvasBackend::Custom);
        assert_eq!(CanvasBackend::parse(Some("mesh")), CanvasBackend::Custom);
        assert_eq!(CanvasBackend::parse(Some("custom")), CanvasBackend::Custom);
        assert_eq!(CanvasBackend::parse(Some("vello")), CanvasBackend::Vello);
        assert_eq!(CanvasBackend::parse(Some("VELLO")), CanvasBackend::Vello);
        assert_eq!(CanvasBackend::parse(Some(" Vello ")), CanvasBackend::Vello);
    }

    #[test]
    fn from_env_matches_parse_of_process_var() {
        // Does not mutate the environment (crate forbids unsafe). Equivalent to parse(var).
        let raw = std::env::var(ENV_CANVAS).ok();
        assert_eq!(CanvasBackend::from_env(), CanvasBackend::parse(raw.as_deref()));
    }

    #[test]
    fn preview_png_custom_matches_tiny_skia() {
        let list = line_list();
        let view = View::fit(&list.bounds, 64, 64, 0.05);
        let opts = RasterOptions::default();
        let custom = aad_render::raster::render_png(&list, &view, &opts);
        let via = preview_png_with(CanvasBackend::Custom, &list, &view, &opts);
        assert!(custom.is_some());
        assert_eq!(via, custom, "Custom backend must keep the tiny-skia path");
    }

    #[test]
    fn vello_path_renders_a_line_png() {
        let list = line_list();
        let view = View::fit(&list.bounds, 64, 64, 0.05);
        let png = render_png(&list, &view, &RasterOptions::default());
        assert!(png.as_ref().is_some_and(|b| b.starts_with(b"\x89PNG") && b.len() > 32));
    }

    #[test]
    fn vello_rejects_absurd_sizes() {
        let list = line_list();
        let bad = View { center: Vec2::ZERO, scale: 1.0, width: 0, height: 10 };
        assert!(render_png(&list, &bad, &RasterOptions::default()).is_none());
    }

    #[test]
    fn preview_png_vello_matches_render_png() {
        let list = line_list();
        let view = View::fit(&list.bounds, 48, 48, 0.05);
        let opts = RasterOptions::default();
        let via = preview_png_with(CanvasBackend::Vello, &list, &view, &opts);
        let vello = render_png(&list, &view, &opts);
        assert!(via.is_some() && vello.is_some());
        assert_eq!(via, vello, "Vello backend must select vello_cpu");
    }
}

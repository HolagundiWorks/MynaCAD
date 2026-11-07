//! AAD rendering.
//!
//! [`build`] turns a drawing space into a [`DisplayList`]: world-space polylines, filled
//! triangles and points, each tagged with its top-level entity handle and resolved colour.
//! Linetypes, hatches, text, block references and dimensions are expanded here. [`mesh`] packs
//! the list for the GPU canvas; [`raster`] draws it on the CPU (PNG export, plot preview, tests).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod clip;
mod dim;
mod fill;
pub mod glyph_mesh;
mod hatch;
mod linetype;
mod linetype_text;
pub mod mesh;
pub mod paper;
mod plotstyle;
mod point;
pub mod raster;
pub mod scene;
mod trace;
pub mod units;
mod wide;

use std::sync::Arc;

use aad_color::{Color, Rgb};
use aad_doc::{Drawing, Entity, EntityKind, Handle, Lineweight, Prim, Space};
use aad_geom::{Bounds2, Mat3, Polyline, TAU, Vec2};

pub use dim::{ArrowGeom, Arrowhead, DimGeometry, DimText, LineRole, arrowhead, dimension_geometry, dimension_geometry_with, format_linear_value};
pub use fill::{triangulate_evenodd, triangulate_outline};
pub use glyph_mesh::{GlyphFillKey, MAX_GLYPH_MESH_ENTRIES, filled_tris, glyph_mesh_cache_clear, glyph_mesh_cache_len};
pub use hatch::{MAX_HATCH_PATTERN_ENTRIES, hatch_pattern_cache_clear, hatch_pattern_cache_len, pattern_lines};
pub use paper::{PAPER_SIZES, PaperSize, Sheet, paper_size, sheet};
pub use scene::{EntInfo, Scene, SceneTile, build_scene, build_scene_if_viable, flatten_scene, rebuild_scene, scene_tile_edge, scene_tiling_viable};
pub use trace::{build_with_traces, stylize_trace};

/// Handle carried by model-space geometry drawn inside a paper-space viewport (it belongs to no
/// single paper-space entity, so selection highlighting never matches it).
pub const VIEWPORT_CONTENT: Handle = Handle(0);

/// What a display-list primitive draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `verts[start..start+len]` as a connected polyline.
    Polyline,
    /// Circular stroke: `verts[start]` = centre, `verts[start+1]` = rim point (radius).
    /// `sweep` is the CCW span in radians from the rim angle; ≥ 2π − ε is a full circle.
    Arc { sweep: f32 },
    /// Elliptical stroke: `verts[start]` = centre, `verts[start+1]` = centre+U,
    /// `verts[start+2]` = centre+V. Parametric `P(t) = C + U·cos(t) + V·sin(t)` for
    /// `t ∈ [start, start+sweep]`. U and V are conjugate diameters (perpendicular for DXF
    /// ellipses; any linearly independent pair after a non-uniform transform). Continuous
    /// linetype only — dashed styles stay polylines.
    Ellipse { start: f32, sweep: f32 },
    /// `tris[start..start+len]`, three vertices per triangle.
    Tris,
    /// A wipeout: triangles laid out like [`Kind::Tris`], filled with the background colour
    /// (paper white when plotting, the canvas colour on screen) so they hide what was drawn
    /// before them.
    Mask,
    /// A single point marker at `verts[start]`.
    Point,
    /// An infinite line or ray: `verts[start]` = base, `verts[start+1]` = unit direction.
    Infinite { ray: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DPrim {
    pub handle: Handle,
    pub color: Rgb,
    /// Colour 7 (directly, by layer or by block): drawn white on a dark background and black on
    /// a light one, see [`DPrim::display_rgb`]. Other colours keep their RGB.
    pub aci7: bool,
    /// Lineweight in mm (0 = thinnest).
    pub lw: f32,
    pub kind: Kind,
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub prims: Vec<DPrim>,
    pub verts: Vec<Vec2>,
    pub tris: Vec<Vec2>,
    pub bounds: Bounds2,
    /// The paper sheet when the list was built for a layout (paper space); the UI draws the
    /// white sheet, its shadow and the printable-area outline from it.
    pub sheet: Option<Sheet>,
    /// Block contents expanded while building (see [`MAX_BLOCK_EXPANSION`]).
    pub expanded: usize,
}

impl DPrim {
    /// The colour to draw with on `background`.
    pub fn display_rgb(&self, background: Rgb) -> Rgb {
        aad_color::display_rgb(self.color, self.aci7, background)
    }
}

/// A resolved colour: its RGB, whether it is colour 7, and its index colour (0 = a true
/// colour), which picks the style of a colour-dependent plot style table.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ink {
    rgb: Rgb,
    aci7: bool,
    aci: u8,
}

/// Resolve `c` like [`Color::resolve`], noting whether it comes out as colour 7.
fn ink(c: Color, layer: Color, block: Color) -> Ink {
    // ByLayer / ByBlock left unresolved (layer or block colour itself logical) mean colour 7.
    let is7 = |c: Color| matches!(c, Color::ByLayer | Color::ByBlock | Color::Index(7));
    let aci7 = match c {
        Color::ByLayer => is7(layer),
        Color::ByBlock => is7(block),
        c => c == Color::Index(7),
    };
    let aci = match c {
        Color::ByLayer => layer,
        Color::ByBlock => block,
        c => c,
    };
    let aci = match aci {
        Color::Index(i) => i,
        Color::True(_) => 0,
        Color::ByLayer | Color::ByBlock => 7,
    };
    Ink { rgb: c.resolve(layer, block), aci7, aci }
}

impl DisplayList {
    pub fn segment_count(&self) -> usize {
        self.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| p.len.saturating_sub(1) as usize).sum()
    }
    /// The polyline points of a primitive.
    pub fn points(&self, p: &DPrim) -> &[Vec2] {
        match p.kind {
            Kind::Tris | Kind::Mask => self.tris.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
            _ => self.verts.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
        }
    }
}

/// Tessellation cache band for a given world-units-per-pixel size (`floor(log2(px))`).
pub fn tessellation_band(world_per_pixel: f64) -> i32 {
    world_per_pixel.max(1e-300).log2().floor() as i32
}

/// Chord error for [`tessellation_band`]: about a fifth of a screen pixel.
///
/// Tight enough that arcs and circles read as smooth on the GPU path without exploding
/// segment counts (each zoom octave still shares one display-list cache key).
pub fn tessellation_tolerance(band: i32) -> f64 {
    2f64.powi(band) * 0.2
}

/// Build options.
#[derive(Clone, Debug)]
pub struct Options {
    /// Chord tolerance in world units (about a fifth of a pixel at the current zoom).
    pub tolerance: f64,
    /// Dashes shorter than this (world units) are drawn as continuous lines.
    pub min_dash: f64,
    /// Draw text (off for very coarse previews).
    pub text: bool,
    /// Filled hatches and solid fills (off when features are sub-pixel on screen).
    pub fill: bool,
    /// Far-zoom text: grey bars instead of glyphs (avoids pop-in).
    pub text_greek: bool,
    /// Far-zoom pattern hatches: solid tint instead of pattern lines (avoids moiré).
    pub fill_tint: bool,
    pub lineweights: bool,
    /// Height of the visible area in world units of the space being built. Relative point
    /// sizes (`PDSIZE` <= 0) are a percentage of it; 0 = the height of the space's extents.
    pub view_height: f64,
    /// When set with a positive [`Self::view_height`], model-space builds skip entities whose
    /// bounds miss an expanded view window (zoomed-in frustum cull). Ignored for paper space
    /// and when plotting.
    pub view_center: Option<Vec2>,
    /// Plot style table to draw with. When `None`, [`build`] of a layout whose page setup has
    /// "Display plot styles" on uses the layout's table; [`build_plot`] uses none.
    pub plot_style_table: Option<std::sync::Arc<aad_doc::PlotStyleTable>>,
    /// World units per screen pixel at the current zoom (0 = unknown, for plots and tools that have no screen). With it, each piece of text
    /// is drawn at the detail its own size on screen deserves: full glyphs when it is legible, a grey bar when it is a few pixels tall, and
    /// nothing when it is under a pixel. Without it only the zoom-wide [`Self::text_greek`] switch applies.
    pub screen_px: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tolerance: 0.001,
            min_dash: 0.0,
            text: true,
            fill: true,
            text_greek: false,
            fill_tint: false,
            lineweights: false,
            view_height: 0.0,
            view_center: None,
            plot_style_table: None,
            screen_px: 0.0,
        }
    }
}

/// Screen-pixel size of one world unit for a view of `view_height` drawn into `pixel_height`.
pub fn world_per_pixel(view_height: f64, pixel_height: f64) -> f64 {
    if !(view_height.is_finite() && view_height > 0.0 && pixel_height.is_finite() && pixel_height > 0.0) {
        return 0.0;
    }
    view_height / pixel_height
}

/// LOD for interactive display: text on/off, fill on/off, greek text, tinted hatches.
///
/// Instead of hard cut-offs that pop text and patterns in and out, mid-zoom ranges keep a cheap
/// stand-in (grey text bars; solid hatch tint) so far views stay light without moiré or flicker.
///
/// Thresholds are world units per pixel. Metric floor plans sit around 5–20 wpp at a whole-building
/// view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodFlags {
    pub text: bool,
    pub fill: bool,
    pub text_greek: bool,
    pub fill_tint: bool,
}

/// LOD flags for interactive display: `(draw_text, draw_fill)`.
/// Prefer [`lod_flags_for_view`] when greek/tint stand-ins are wanted.
pub fn lod_for_view(view_height: f64, pixel_height: f64) -> (bool, bool) {
    let l = lod_flags_for_view(view_height, pixel_height);
    (l.text, l.fill)
}

/// Full LOD including density stand-ins for text and pattern hatches.
pub fn lod_flags_for_view(view_height: f64, pixel_height: f64) -> LodFlags {
    let wpp = world_per_pixel(view_height, pixel_height);
    if wpp <= 0.0 {
        return LodFlags { text: true, fill: true, text_greek: false, fill_tint: false };
    }
    // Text: full → greek bars → off. Fill: full patterns → solid tint → off.
    let text = wpp < 100.0;
    let text_greek = text && wpp >= 35.0;
    let fill = wpp < 50.0;
    let fill_tint = fill && wpp >= 12.0;
    LodFlags { text, fill, text_greek, fill_tint }
}

/// Text shorter than this on screen is not drawn; shorter than [`TEXT_GREEK_PX`] it is a grey bar instead of glyphs.
const TEXT_SKIP_PX: f64 = 1.5;
const TEXT_GREEK_PX: f64 = 6.0;

/// World-space half-extent of the mesh origin snap cell ([`mesh::choose_origin`] uses `64 × h`).
pub const MODEL_ORIGIN_CELL: f64 = 64.0;

fn model_cull_window(opts: &Options) -> Option<Bounds2> {
    let center = opts.view_center?;
    let h = opts.view_height;
    if !(h.is_finite() && h > 0.0 && center.is_finite()) {
        return None;
    }
    // Cull around the snapped mesh origin cell so a cached display list still covers every pan
    // that keeps the same origin. A window the size of one screen left geometry missing after a
    // deep zoom + far pan until the next zoom band or edit rebuilt the list.
    let step = h * MODEL_ORIGIN_CELL;
    let origin = Vec2::new((center.x / step).round() * step, (center.y / step).round() * step);
    let half = Vec2::new(step * 0.5 + h * 2.0, step * 0.5 + h * 2.0);
    Some(Bounds2::new(origin - half, origin + half))
}

/// Coarse pan cell for model-space display-list caching (matches [`model_cull_window`]).
pub fn model_pan_cell(center: Vec2, view_height: f64) -> (i64, i64) {
    if !(view_height.is_finite() && view_height > 0.0 && center.is_finite()) {
        return (0, 0);
    }
    let step = view_height * MODEL_ORIGIN_CELL;
    ((center.x / step).round() as i64, (center.y / step).round() as i64)
}

fn entity_in_cull(d: &Drawing, e: &Entity, win: &Bounds2) -> bool {
    if matches!(e.kind, EntityKind::Ray(_) | EntityKind::XLine(_)) {
        return true;
    }
    let eb = aad_doc::entity_bounds(d, e, 0);
    eb.is_empty() || eb.intersects(win)
}

/// Graphics state inherited through block references.
#[derive(Clone)]
struct Ctx<'a> {
    d: &'a Drawing,
    xf: Mat3,
    block_color: Color,
    block_layer: Option<String>,
    block_lw: Lineweight,
    block_ltype: String,
    depth: usize,
    top: Handle,
    /// Layers frozen in the viewport being drawn.
    frozen: &'a [String],
    /// Per-viewport layer colour overrides of the viewport being drawn.
    vp_colors: &'a [(String, Color)],
    /// Extra linetype scale for model space seen through a viewport: 1 / viewport scale when
    /// `PSLTSCALE` is on (dashes keep their paper-space length at any viewport scale), else 1.
    lt_factor: f64,
    /// Named plot style (table index) of the block reference being expanded, for `ByBlock`.
    block_pstyle: Option<usize>,
}

impl Ctx<'_> {
    /// A layer's colour, honouring the viewport's override.
    fn layer_color(&self, name: &str) -> Option<Color> {
        if let Some((_, c)) = self.vp_colors.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            return Some(*c);
        }
        self.d.layer(name).map(|l| l.color)
    }
}

struct Builder<'a> {
    list: DisplayList,
    opts: &'a Options,
    /// Plotting: skip layers marked "do not plot".
    plotting: bool,
    /// The plot style table drawn with.
    styles: Option<&'a plotstyle::Styler>,
    /// Named plot style (table index) of the entity being drawn.
    named: Option<usize>,
    /// Block-expansion budget shared by builders running side by side (None: `list.expanded` is it).
    budget: Option<&'a std::sync::atomic::AtomicUsize>,
}

/// Upper bound on the block contents drawn per display list (each block reference, MINSERT copy
/// and entity drawn inside a block counts one). Nested, self-referencing or arrayed block
/// references in a hostile file would otherwise expand exponentially (`MAX_BLOCK_DEPTH` only
/// bounds the depth); past the limit the remaining block contents are not drawn.
pub const MAX_BLOCK_EXPANSION: usize = 2_000_000;

impl Builder<'_> {
    /// `color` and `lw` as the plot style table prints them.
    fn styled(&self, color: Ink, lw: f32) -> (Ink, f32) {
        match self.styles {
            Some(s) => s.apply(color, self.named, lw, self.opts.lineweights),
            None => (color, lw),
        }
    }
    fn polyline(&mut self, ctx: &Ctx, color: Ink, lw: f32, pts: &[Vec2]) {
        if pts.len() < 2 {
            return;
        }
        let (color, lw) = self.styled(color, lw);
        let start = self.list.verts.len() as u32;
        for p in pts {
            let q = ctx.xf.apply(*p);
            self.list.bounds.add(q);
            self.list.verts.push(q);
        }
        self.list.prims.push(DPrim { handle: ctx.top, color: color.rgb, aci7: color.aci7, lw, kind: Kind::Polyline, start, len: pts.len() as u32 });
    }
    /// Emit a GPU distance-field arc when `ctx.xf` is a similarity; otherwise an elliptical
    /// ring when the affine image is a non-degenerate ellipse. Returns false so the caller can
    /// tessellate (dashed linetypes, degenerate transforms).
    fn try_arc(&mut self, ctx: &Ctx, color: Ink, lw: f32, center: Vec2, radius: f64, start: f64, sweep: f64) -> bool {
        if let Some((c, r, a0, sw)) = map_similarity_arc(ctx.xf, center, radius, start, sweep) {
            if !(r.is_finite() && r > 1e-12 && sw.is_finite() && sw > 0.0) {
                return false;
            }
            let (color, lw) = self.styled(color, lw);
            let rim = Vec2::polar(c, r, a0);
            let at = self.list.verts.len() as u32;
            self.list.bounds.add(c - Vec2::new(r, r));
            self.list.bounds.add(c + Vec2::new(r, r));
            self.list.verts.push(c);
            self.list.verts.push(rim);
            self.list.prims.push(DPrim {
                handle: ctx.top,
                color: color.rgb,
                aci7: color.aci7,
                lw,
                kind: Kind::Arc { sweep: sw as f32 },
                start: at,
                len: 2,
            });
            return true;
        }
        let Some((c, u, v, a0, sw)) = map_affine_arc(ctx.xf, center, radius, start, sweep) else {
            return false;
        };
        self.push_ellipse(ctx, color, lw, c, u, v, a0, sw)
    }

    /// Emit a GPU elliptical ring when continuous; returns false for degenerate axes.
    fn try_ellipse(&mut self, ctx: &Ctx, color: Ink, lw: f32, el: &aad_geom::Ellipse) -> bool {
        let Some((c, u, v, a0, sw)) = map_affine_ellipse(ctx.xf, el) else {
            return false;
        };
        self.push_ellipse(ctx, color, lw, c, u, v, a0, sw)
    }

    fn push_ellipse(&mut self, ctx: &Ctx, color: Ink, lw: f32, c: Vec2, u: Vec2, v: Vec2, start: f64, sweep: f64) -> bool {
        if !(c.is_finite() && u.is_finite() && v.is_finite() && start.is_finite() && sweep.is_finite()) {
            return false;
        }
        if sweep <= 0.0 || u.len() <= 1e-12 || v.len() <= 1e-12 || (u.x * v.y - u.y * v.x).abs() <= 1e-18 {
            return false;
        }
        let (color, lw) = self.styled(color, lw);
        let bb = ellipse_world_bounds(c, u, v, start as f32, sweep as f32);
        self.list.bounds.add(bb.min);
        self.list.bounds.add(bb.max);
        let at = self.list.verts.len() as u32;
        self.list.verts.push(c);
        self.list.verts.push(c + u);
        self.list.verts.push(c + v);
        self.list.prims.push(DPrim {
            handle: ctx.top,
            color: color.rgb,
            aci7: color.aci7,
            lw,
            kind: Kind::Ellipse { start: start as f32, sweep: sweep as f32 },
            start: at,
            len: 3,
        });
        true
    }
    fn tris(&mut self, ctx: &Ctx, color: Ink, tris: &[Vec2]) {
        if tris.len() < 3 {
            return;
        }
        let (color, _) = self.styled(color, 0.0);
        let start = self.list.tris.len() as u32;
        for p in tris {
            let q = ctx.xf.apply(*p);
            self.list.bounds.add(q);
            self.list.tris.push(q);
        }
        self.list.prims.push(DPrim { handle: ctx.top, color: color.rgb, aci7: color.aci7, lw: 0.0, kind: Kind::Tris, start, len: tris.len() as u32 });
    }
    /// Shaped text: strokes as polylines; TrueType glyphs filled (TEXTFILL) or outlined.
    ///
    /// TrueType fill follows `TEXTFILL` and whether text is drawn at all — not the hatch/solid
    /// fill LOD (`opts.fill`). AutoCAD keeps TEXTFILL independent of FILLMODE; tying them made
    /// glyphs turn into hollow outlines whenever hatch fill was dropped for zoom performance.
    /// Far zoom with [`Options::text_greek`] draws a grey bar instead of glyphs.
    fn shaped(&mut self, ctx: &Ctx, color: Ink, lw: f32, sh: &aad_fonts::Shaped) {
        if self.opts.text_greek {
            self.greek_text(ctx, sh);
            return;
        }
        if self.opts.screen_px > 0.0 {
            // Detail by size on screen. A line of text a few pixels tall is a smudge: its glyph triangles (thousands of vertices a line)
            // cost far more than they show.
            let px_h = self.text_pixel_height(ctx, sh);
            if px_h < TEXT_SKIP_PX {
                return;
            }
            if px_h < TEXT_GREEK_PX {
                self.greek_text(ctx, sh);
                return;
            }
        }
        for s in &sh.strokes {
            self.polyline(ctx, color, lw, s);
        }
        if sh.glyphs.is_empty() {
            return;
        }
        let fill = self.opts.text && ctx.d.header.i64("TEXTFILL", 1) != 0;
        for (i, g) in sh.glyphs.iter().enumerate() {
            if fill {
                let tris = glyph_mesh::filled_tris(sh.glyph_keys.get(i).copied().flatten(), sh.glyph_places.get(i).copied().flatten(), sh.post_xf, g);
                self.tris(ctx, color, &tris);
            } else {
                for c in g {
                    self.polyline(ctx, color, lw, c);
                }
            }
        }
    }
    /// Height of this shaped text on screen, in pixels (its vertical extent through the block transform, over world units per pixel).
    fn text_pixel_height(&self, ctx: &Ctx, sh: &aad_fonts::Shaped) -> f64 {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut note = |p: &Vec2| {
            let q = ctx.xf.apply(sh.post_xf.apply(*p));
            lo = lo.min(q.y);
            hi = hi.max(q.y);
        };
        for s in &sh.strokes {
            s.iter().for_each(&mut note);
        }
        for g in &sh.glyphs {
            for c in g {
                c.iter().for_each(&mut note);
            }
        }
        if hi < lo {
            return f64::INFINITY; // nothing to measure: leave it to the normal path
        }
        (hi - lo) / self.opts.screen_px
    }
    /// Cheap stand-in for far-zoom text: a mid-grey bar over the shaped bounds.
    fn greek_text(&mut self, ctx: &Ctx, sh: &aad_fonts::Shaped) {
        let mut bb = Bounds2::default();
        for s in &sh.strokes {
            for p in s {
                bb.add(*p);
            }
        }
        for g in &sh.glyphs {
            for c in g {
                for p in c {
                    bb.add(*p);
                }
            }
        }
        if bb.is_empty() {
            if sh.width > 0.0 {
                bb = Bounds2::new(Vec2::ZERO, Vec2::new(sh.width, 1.0));
            } else {
                return;
            }
        }
        let h = bb.height().max(bb.width() * 0.02);
        let pad = h * 0.2;
        let y0 = bb.min.y + pad;
        let y1 = (bb.max.y - pad).max(y0 + h * 0.35);
        let x0 = bb.min.x;
        let x1 = bb.max.x.max(x0 + h * 0.5);
        let grey = Ink { rgb: Rgb(168, 168, 168), aci7: false, aci: 0 };
        self.tris(ctx, grey, &[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
    }
    fn point(&mut self, ctx: &Ctx, color: Ink, p: Vec2) {
        let (color, _) = self.styled(color, 0.0);
        let q = ctx.xf.apply(p);
        self.list.bounds.add(q);
        let start = self.list.verts.len() as u32;
        self.list.verts.push(q);
        self.list.prims.push(DPrim { handle: ctx.top, color: color.rgb, aci7: color.aci7, lw: 0.0, kind: Kind::Point, start, len: 1 });
    }
    fn infinite(&mut self, ctx: &Ctx, color: Ink, base: Vec2, dir: Vec2, ray: bool) {
        let (color, _) = self.styled(color, 0.0);
        let start = self.list.verts.len() as u32;
        self.list.verts.push(ctx.xf.apply(base));
        self.list.verts.push(ctx.xf.apply_vec(dir).normalized());
        self.list.prims.push(DPrim { handle: ctx.top, color: color.rgb, aci7: color.aci7, lw: 0.0, kind: Kind::Infinite { ray }, start, len: 2 });
    }
    /// Take one unit of the block-expansion budget; false once it is spent.
    fn expand(&mut self) -> bool {
        self.list.expanded = self.list.expanded.saturating_add(1);
        match self.budget {
            Some(shared) => shared.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < MAX_BLOCK_EXPANSION,
            None => self.list.expanded <= MAX_BLOCK_EXPANSION,
        }
    }
}

/// Build the display list for a space. For a layout (paper space) this includes the model-space
/// geometry seen through each viewport, clipped to the viewport, and [`DisplayList::sheet`].
pub fn build(d: &Drawing, space: &Space, opts: &Options) -> DisplayList {
    build_space(d, space, opts, false)
}

/// Like [`build`] but for plotting: layers marked "do not plot" (such as `Defpoints`) are left out.
pub fn build_plot(d: &Drawing, space: &Space, opts: &Options) -> DisplayList {
    build_space(d, space, opts, true)
}

fn top_ctx<'a>(d: &'a Drawing, xf: Mat3, top: Handle, frozen: &'a [String], vp_colors: &'a [(String, Color)]) -> Ctx<'a> {
    Ctx {
        d,
        xf,
        block_color: Color::Index(7),
        block_layer: None,
        block_lw: Lineweight::Default,
        block_ltype: "Continuous".into(),
        depth: 0,
        top,
        frozen,
        vp_colors,
        lt_factor: 1.0,
        block_pstyle: None,
    }
}

fn build_space(d: &Drawing, space: &Space, opts: &Options, plotting: bool) -> DisplayList {
    aad_fonts::with_world_chord_tol(opts.tolerance, || build_space_inner(d, space, opts, plotting))
}

fn build_space_inner(d: &Drawing, space: &Space, opts: &Options, plotting: bool) -> DisplayList {
    let opts = &with_point_view(d, space, opts);
    let table = opts.plot_style_table.clone().or_else(|| match space {
        Space::Paper(n) if !plotting => d
            .layout(n)
            .filter(|l| l.page.show_plot_styles)
            .and_then(|l| aad_doc::plot_style_table(d, &l.page.plot_style_table))
            .map(std::sync::Arc::new),
        _ => None,
    });
    let styler = table.map(|t| plotstyle::Styler::new(d, t));
    let mut b = Builder { list: DisplayList::default(), opts, plotting, styles: styler.as_ref(), named: None, budget: None };
    if let Space::Paper(name) = space {
        b.list.sheet = paper::sheet(d, name);
    }
    if let Some(store) = d.space(space) {
        let paper = matches!(space, Space::Paper(_));
        let cull = if paper || plotting { None } else { model_cull_window(opts) };
        let ents: Vec<&std::sync::Arc<Entity>> = store.iter().filter(|e| cull.as_ref().is_none_or(|w| entity_in_cull(d, e, w))).collect();
        if !paper && ents.len() >= PARALLEL_MIN {
            // Model space: the entities are independent, so chunks are built on all cores and
            // appended in draw order.
            use rayon::prelude::*;
            let budget = std::sync::atomic::AtomicUsize::new(0);
            let tol = opts.tolerance;
            let parts: Vec<DisplayList> = ents
                .par_chunks(PARALLEL_CHUNK)
                .map(|chunk| {
                    aad_fonts::with_world_chord_tol(tol, || {
                        let mut sub =
                            Builder { list: DisplayList::default(), opts, plotting, styles: styler.as_ref(), named: None, budget: Some(&budget) };
                        for e in chunk {
                            entity(&mut sub, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
                        }
                        sub.list
                    })
                })
                .collect();
            for part in parts {
                append(&mut b.list, part);
            }
            return b.list;
        }
        let mut viewports = 0usize;
        for e in ents {
            if paper && let EntityKind::Viewport(vp) = &e.kind {
                if viewports < MAX_VIEWPORTS {
                    viewports += 1;
                    viewport(&mut b, d, e, vp);
                }
                continue;
            }
            entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
        }
    }
    b.list
}

/// Fewer entities than this are built on one thread (the split costs more than it saves).
const PARALLEL_MIN: usize = 4096;
/// Entities per unit of parallel work.
pub(crate) const PARALLEL_CHUNK: usize = 256;

/// Append `part` (built separately) to `dst`, keeping draw order: its primitives' starts move past
/// what `dst` already holds.
fn append(dst: &mut DisplayList, part: DisplayList) {
    let (vo, to) = (dst.verts.len() as u32, dst.tris.len() as u32);
    dst.prims.extend(part.prims.into_iter().map(|mut p| {
        p.start += if matches!(p.kind, Kind::Tris | Kind::Mask) { to } else { vo };
        p
    }));
    dst.verts.extend(part.verts);
    dst.tris.extend(part.tris);
    dst.bounds = dst.bounds.union(&part.bounds);
    dst.expanded = dst.expanded.saturating_add(part.expanded);
}

/// Whether everything an entity's drawing depends on, other than the entity itself, is the same in
/// both drawings (header, tables, styles, block definitions).
pub(crate) fn same_environment(a: &Drawing, b: &Drawing) -> bool {
    a.header == b.header
        && a.layers == b.layers
        && a.linetypes == b.linetypes
        && a.text_styles == b.text_styles
        && a.dim_styles == b.dim_styles
        && a.mleader_styles == b.mleader_styles
        && a.table_styles == b.table_styles
        && a.plot_style_tables == b.plot_style_tables
        && a.blocks.len() == b.blocks.len()
        && a.blocks.iter().zip(b.blocks.iter()).all(|((ka, va), (kb, vb))| ka == kb && (std::sync::Arc::ptr_eq(va, vb) || va == vb))
}

/// One stretch of the new list: primitives taken from the old list, or entities to draw afresh.
enum Segment<'a> {
    Keep(usize, usize),
    Draw(Vec<&'a Entity>),
}

/// Update the model-space display `old` (built from `old_doc` with the same `opts`) to match
/// `new_doc`: entities that are the same objects as before keep their primitives, only changed or
/// new ones are drawn. `None` when that is not safe or not worth it (the tables, styles or blocks
/// differ, points are sized to the view, most entities changed): build the list from scratch then.
pub fn rebuild(old: &DisplayList, old_doc: &Drawing, new_doc: &Drawing, opts: &Options) -> Option<DisplayList> {
    use std::collections::HashMap;
    use std::sync::Arc;
    if old.sheet.is_some() || old.expanded >= MAX_BLOCK_EXPANSION / 2 || !same_environment(old_doc, new_doc) {
        return None;
    }
    // Points sized to the view depend on the extents of everything.
    if new_doc.header.f64("PDSIZE", 0.0) <= 0.0 && point::sized(new_doc.header.i64("PDMODE", 0)) {
        return None;
    }
    // Where each entity's primitives are in the old list (an entity's are one contiguous run).
    let mut ranges: HashMap<Handle, (usize, usize)> = HashMap::new();
    let mut i = 0;
    while i < old.prims.len() {
        let h = old.prims.get(i)?.handle;
        let mut j = i + 1;
        while old.prims.get(j).is_some_and(|p| p.handle == h) {
            j += 1;
        }
        if ranges.insert(h, (i, j)).is_some() {
            return None;
        }
        i = j;
    }
    let mut segments: Vec<Segment> = Vec::new();
    let mut fresh = 0usize;
    for e in new_doc.model.iter() {
        let same = old_doc.model.get(e.handle).is_some_and(|o| Arc::ptr_eq(o, e));
        if same {
            if let Some(&(a, b)) = ranges.get(&e.handle) {
                match segments.last_mut() {
                    Some(Segment::Keep(_, end)) if *end == a => *end = b,
                    _ => segments.push(Segment::Keep(a, b)),
                }
            }
        } else {
            fresh += 1;
            match segments.last_mut() {
                Some(Segment::Draw(v)) => v.push(e),
                _ => segments.push(Segment::Draw(vec![e])),
            }
        }
    }
    if fresh > new_doc.model.len() / 4 + 64 {
        return None;
    }
    let mut out = DisplayList { expanded: old.expanded, ..DisplayList::default() };
    out.prims.reserve(old.prims.len() + 64);
    out.verts.reserve(old.verts.len() + 256);
    out.tris.reserve(old.tris.len() + 256);
    for seg in segments {
        match seg {
            Segment::Keep(a, b) => {
                let prims = old.prims.get(a..b)?;
                let is_tris = |p: &DPrim| matches!(p.kind, Kind::Tris | Kind::Mask);
                let span = |tris: bool| {
                    let mut it = prims.iter().filter(|p| is_tris(p) == tris);
                    let first = it.next()?.start;
                    let end = prims.iter().filter(|p| is_tris(p) == tris).map(|p| p.start + p.len).max()?;
                    Some((first, end))
                };
                let (vo, to) = (out.verts.len() as i64, out.tris.len() as i64);
                let vspan = span(false);
                let tspan = span(true);
                if let Some((s, e)) = vspan {
                    out.verts.extend_from_slice(old.verts.get(s as usize..e as usize)?);
                }
                if let Some((s, e)) = tspan {
                    out.tris.extend_from_slice(old.tris.get(s as usize..e as usize)?);
                }
                for p in prims {
                    let mut p = *p;
                    let (base, off) = if is_tris(&p) { (tspan?.0, to) } else { (vspan?.0, vo) };
                    p.start = u32::try_from(i64::from(p.start) - i64::from(base) + off).ok()?;
                    out.prims.push(p);
                }
            }
            Segment::Draw(ents) => {
                let mut b = Builder { list: DisplayList::default(), opts, plotting: false, styles: None, named: None, budget: None };
                for e in ents {
                    entity(&mut b, &top_ctx(new_doc, Mat3::IDENTITY, e.handle, &[], &[]), e);
                }
                append(&mut out, b.list);
            }
        }
    }
    let mut bounds = Bounds2::default();
    for p in &out.prims {
        match p.kind {
            Kind::Infinite { .. } => continue,
            Kind::Arc { sweep } => {
                let pts = out.points(p);
                if let (Some(c), Some(rim)) = (pts.first(), pts.get(1)) {
                    let b = arc_world_bounds(*c, *rim, sweep);
                    bounds.add(b.min);
                    bounds.add(b.max);
                }
            }
            Kind::Ellipse { start, sweep } => {
                let pts = out.points(p);
                if let (Some(c), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) {
                    let b = ellipse_world_bounds(*c, *u_tip - *c, *v_tip - *c, start, sweep);
                    bounds.add(b.min);
                    bounds.add(b.max);
                }
            }
            Kind::Tris | Kind::Mask => {
                for q in out.points(p) {
                    bounds.add(*q);
                }
            }
            _ => {
                for q in out.points(p) {
                    bounds.add(*q);
                }
            }
        }
    }
    out.bounds = bounds;
    Some(out)
}

/// `opts` with a view height for relative point sizes: the extents height when none is given
/// and points draw as figures sized relative to the view.
fn with_point_view(d: &Drawing, space: &Space, opts: &Options) -> Options {
    let mut o = opts.clone();
    let relative = d.header.f64("PDSIZE", 0.0) <= 0.0 && point::sized(d.header.i64("PDMODE", 0));
    if !(o.view_height.is_finite() && o.view_height > 0.0) && relative {
        let ext = d.extents(space);
        o.view_height = if ext.is_empty() { 0.0 } else { ext.height().max(ext.width() * 1e-3) };
    }
    o
}

/// Upper bound on viewports drawn per layout (hostile files).
const MAX_VIEWPORTS: usize = 256;

/// Identity of model-space content + viewport scale + a coarse pan cell (fine pans reuse the
/// tessellation and only re-transform / clip).
fn model_tess_key(
    d: &Drawing,
    opts: &Options,
    lt_factor: f64,
    frozen: &[String],
    colors: &[(String, Color)],
    pan_cell: (i64, i64),
    view_height_bits: u64,
    sources: &[trace::Source<'_>],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    trace::hash_sources(sources, &mut h);
    for l in &d.layers {
        l.name.hash(&mut h);
        l.on.hash(&mut h);
        l.frozen.hash(&mut h);
        format!("{:?}", l.color).hash(&mut h);
        l.linetype.hash(&mut h);
        format!("{:?}", l.lineweight).hash(&mut h);
    }
    opts.tolerance.to_bits().hash(&mut h);
    opts.min_dash.to_bits().hash(&mut h);
    opts.text.hash(&mut h);
    opts.fill.hash(&mut h);
    opts.text_greek.hash(&mut h);
    opts.fill_tint.hash(&mut h);
    opts.lineweights.hash(&mut h);
    lt_factor.to_bits().hash(&mut h);
    view_height_bits.hash(&mut h);
    pan_cell.hash(&mut h);
    frozen.hash(&mut h);
    for (n, c) in colors {
        n.hash(&mut h);
        format!("{c:?}").hash(&mut h);
    }
    h.finish()
}

/// How many neighbouring pan cells to keep (most-recent first).
const VP_MODEL_LRU: usize = 6;
/// Model units of pan before the tessellation cell changes (`N × view_height`).
const VP_PAN_CELL_MULT: f64 = 8.0;

/// Process-wide layout-viewport model tessellation LRU (Phase C: no longer `thread_local`, so
/// display-list worker threads share the same tiles as the UI thread).
fn vp_model_cache() -> &'static std::sync::Mutex<Vec<(u64, Arc<DisplayList>)>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<(u64, Arc<DisplayList>)>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub(crate) fn vp_model_get(key: u64) -> Option<Arc<DisplayList>> {
    let mut guard = match vp_model_cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(i) = guard.iter().position(|(k, _)| *k == key) {
        let entry = guard.remove(i);
        let list = Arc::clone(&entry.1);
        guard.insert(0, entry);
        return Some(list);
    }
    None
}

pub(crate) fn vp_model_put(key: u64, list: Arc<DisplayList>) {
    let mut guard = match vp_model_cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.retain(|(k, _)| *k != key);
    guard.insert(0, (key, list));
    if guard.len() > VP_MODEL_LRU {
        guard.truncate(VP_MODEL_LRU);
    }
}

/// Map a circular arc through a similarity transform. Non-uniform scales return `None` (ellipse).
fn map_similarity_arc(xf: Mat3, center: Vec2, radius: f64, start: f64, sweep: f64) -> Option<(Vec2, f64, f64, f64)> {
    let u = xf.apply_vec(Vec2::X);
    let v = xf.apply_vec(Vec2::Y);
    let (lu, lv) = (u.len(), v.len());
    let tol = 1e-9 * lu.max(lv);
    if !((lu - lv).abs() <= tol && u.dot(v).abs() <= tol * lu.max(lv) && lu > 0.0) {
        return None;
    }
    let c = xf.apply(center);
    let r = radius * lu;
    let p0 = xf.apply(Vec2::polar(center, radius, start));
    let p1 = xf.apply(Vec2::polar(center, radius, start + sweep));
    let (a0, a1) = (c.angle_to(p0), c.angle_to(p1));
    let (a0, a1) = if xf.is_mirroring() { (a1, a0) } else { (a0, a1) };
    let sw = if sweep >= TAU - 1e-9 { TAU } else { aad_geom::ccw_sweep(a0, a1) };
    Some((c, r, a0, sw))
}

/// Map a circular arc through a general affine transform to conjugate-diameter form.
/// `P(t) = C + U·cos(t) + V·sin(t)` with the same parametric `[start, start+sweep]`.
fn map_affine_arc(xf: Mat3, center: Vec2, radius: f64, start: f64, sweep: f64) -> Option<(Vec2, Vec2, Vec2, f64, f64)> {
    if !(radius.is_finite() && radius > 1e-12 && start.is_finite() && sweep.is_finite() && sweep > 0.0) {
        return None;
    }
    let c = xf.apply(center);
    let u = xf.apply_vec(Vec2::new(radius, 0.0));
    let v = xf.apply_vec(Vec2::new(0.0, radius));
    if !c.is_finite() || !u.is_finite() || !v.is_finite() {
        return None;
    }
    if u.len() <= 1e-12 || v.len() <= 1e-12 || (u.x * v.y - u.y * v.x).abs() <= 1e-18 {
        return None;
    }
    let (a0, sw, v) = if xf.is_mirroring() {
        // Reflection reverses parametric orientation; flip V and start from the far end.
        (start + sweep, sweep.min(TAU), Vec2::new(-v.x, -v.y))
    } else {
        (start, if sweep >= TAU - 1e-9 { TAU } else { sweep }, v)
    };
    Some((c, u, v, a0, sw))
}

/// Map a DXF-style ellipse through `xf` to conjugate diameters (still an ellipse).
fn map_affine_ellipse(xf: Mat3, el: &aad_geom::Ellipse) -> Option<(Vec2, Vec2, Vec2, f64, f64)> {
    let sweep = el.sweep();
    if !(el.major.is_finite() && el.ratio.is_finite() && sweep.is_finite() && sweep > 0.0) {
        return None;
    }
    if el.major.len() <= 1e-12 || el.ratio <= 1e-12 {
        return None;
    }
    let c = xf.apply(el.center);
    let u = xf.apply_vec(el.major);
    let v = xf.apply_vec(el.minor());
    if !c.is_finite() || !u.is_finite() || !v.is_finite() {
        return None;
    }
    if u.len() <= 1e-12 || v.len() <= 1e-12 || (u.x * v.y - u.y * v.x).abs() <= 1e-18 {
        return None;
    }
    let (a0, sw, v) = if xf.is_mirroring() {
        (el.start + sweep, sweep.min(TAU), Vec2::new(-v.x, -v.y))
    } else {
        (el.start, if sweep >= TAU - 1e-9 { TAU } else { sweep }, v)
    };
    Some((c, u, v, a0, sw))
}

/// Expand a stored `f32` sweep to `f64` radians. `f32::TAU` rounds *above* `f64::TAU`; feeding
/// that into [`aad_geom::ccw_sweep`] collapses a full circle to a near-zero arc.
pub fn sweep_radians(sweep: f32) -> f64 {
    if !sweep.is_finite() || sweep <= 0.0 {
        return 0.0;
    }
    if sweep >= std::f32::consts::TAU - 1e-4 { TAU } else { f64::from(sweep).min(TAU) }
}

fn arc_world_bounds(center: Vec2, rim: Vec2, sweep: f32) -> Bounds2 {
    let radius = center.dist(rim);
    let start = center.angle_to(rim);
    aad_geom::Arc::new(center, radius, start, start + sweep_radians(sweep)).bounds()
}

/// Axis-aligned bounds of `C + U·cos(t) + V·sin(t)` over a parametric sweep.
fn ellipse_world_bounds(center: Vec2, u: Vec2, v: Vec2, start: f32, sweep: f32) -> Bounds2 {
    let start = f64::from(start);
    let sweep = sweep_radians(sweep);
    let at = |t: f64| center + u * t.cos() + v * t.sin();
    let mut b = Bounds2::from_points([at(start), at(start + sweep)]);
    // Coordinate extrema of the supporting full ellipse; keep those that fall in the sweep.
    for angle in [v.x.atan2(u.x), v.x.atan2(u.x) + std::f64::consts::PI, v.y.atan2(u.y), v.y.atan2(u.y) + std::f64::consts::PI] {
        let mut a = angle - start;
        a -= (a / TAU).floor() * TAU;
        if a < 0.0 {
            a += TAU;
        }
        if sweep >= TAU - 1e-9 || a <= sweep + 1e-9 {
            b.add(at(angle));
        }
    }
    b
}

/// Map every point of `src` by `xf` (infinite lines map base and direction).
fn transform_list(src: &DisplayList, xf: Mat3) -> DisplayList {
    let mut out = DisplayList { expanded: src.expanded, ..DisplayList::default() };
    out.prims.reserve(src.prims.len());
    out.verts.reserve(src.verts.len());
    out.tris.reserve(src.tris.len());
    let map = |p: Vec2| xf.apply(p);
    for p in &src.prims {
        let pts = src.points(p);
        match p.kind {
            Kind::Infinite { ray } => {
                if let (Some(base), Some(dir)) = (pts.first(), pts.get(1)) {
                    let b = map(*base);
                    let d = xf.apply_vec(*dir).normalized();
                    let start = out.verts.len() as u32;
                    out.verts.push(b);
                    out.verts.push(if d.x.is_finite() && d.y.is_finite() { d } else { Vec2::new(1.0, 0.0) });
                    out.prims.push(DPrim { start, len: 2, kind: Kind::Infinite { ray }, ..*p });
                }
            }
            Kind::Arc { sweep } => {
                if let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) {
                    let radius = center.dist(*rim);
                    let start_a = center.angle_to(*rim);
                    let sw0 = sweep_radians(sweep);
                    if let Some((c, r, a0, sw)) = map_similarity_arc(xf, *center, radius, start_a, sw0) {
                        let start = out.verts.len() as u32;
                        out.verts.push(c);
                        out.verts.push(Vec2::polar(c, r, a0));
                        out.prims.push(DPrim { start, len: 2, kind: Kind::Arc { sweep: sw as f32 }, ..*p });
                    } else if let Some((c, u, v, a0, sw)) = map_affine_arc(xf, *center, radius, start_a, sw0) {
                        let start = out.verts.len() as u32;
                        out.verts.push(c);
                        out.verts.push(c + u);
                        out.verts.push(c + v);
                        out.prims.push(DPrim { start, len: 3, kind: Kind::Ellipse { start: a0 as f32, sweep: sw as f32 }, ..*p });
                    } else {
                        let mut tess = Vec::new();
                        aad_geom::Arc::new(*center, radius, start_a, start_a + sw0).tessellate((radius * 1e-3).max(1e-6), &mut tess);
                        let start = out.verts.len() as u32;
                        out.verts.extend(tess.iter().copied().map(map));
                        out.prims.push(DPrim { start, len: tess.len() as u32, kind: Kind::Polyline, ..*p });
                    }
                }
            }
            Kind::Ellipse { start: a0, sweep } => {
                if let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) {
                    // U/V may not be perpendicular after earlier maps; transform axes directly.
                    let c = map(*center);
                    let u = xf.apply_vec(*u_tip - *center);
                    let v0 = xf.apply_vec(*v_tip - *center);
                    let sw0 = sweep_radians(sweep);
                    let a0f = f64::from(a0);
                    let (a0t, sw, v) = if xf.is_mirroring() { (a0f + sw0, sw0, Vec2::new(-v0.x, -v0.y)) } else { (a0f, sw0, v0) };
                    if u.len() > 1e-12 && v.len() > 1e-12 && (u.x * v.y - u.y * v.x).abs() > 1e-18 {
                        let start = out.verts.len() as u32;
                        out.verts.push(c);
                        out.verts.push(c + u);
                        out.verts.push(c + v);
                        out.prims.push(DPrim { start, len: 3, kind: Kind::Ellipse { start: a0t as f32, sweep: sw as f32 }, ..*p });
                    } else {
                        let mut tess = Vec::new();
                        let n = 32usize.max((sw0.abs() / 0.1) as usize);
                        let u0 = *u_tip - *center;
                        let v0 = *v_tip - *center;
                        for i in 0..=n {
                            let ti = a0f + sw0 * i as f64 / n as f64;
                            tess.push(map(*center + u0 * ti.cos() + v0 * ti.sin()));
                        }
                        let start = out.verts.len() as u32;
                        let len = tess.len() as u32;
                        out.verts.extend(tess);
                        out.prims.push(DPrim { start, len, kind: Kind::Polyline, ..*p });
                    }
                }
            }
            Kind::Tris | Kind::Mask => {
                let start = out.tris.len() as u32;
                out.tris.extend(pts.iter().copied().map(map));
                out.prims.push(DPrim { start, len: p.len, ..*p });
            }
            _ => {
                let start = out.verts.len() as u32;
                out.verts.extend(pts.iter().copied().map(map));
                out.prims.push(DPrim { start, len: p.len, ..*p });
            }
        }
    }
    let mut bounds = Bounds2::default();
    for prim in &out.prims {
        match prim.kind {
            Kind::Arc { sweep } => {
                let pts = out.points(prim);
                if let (Some(c), Some(rim)) = (pts.first(), pts.get(1)) {
                    let b = arc_world_bounds(*c, *rim, sweep);
                    bounds.add(b.min);
                    bounds.add(b.max);
                }
            }
            Kind::Ellipse { start, sweep } => {
                let pts = out.points(prim);
                if let (Some(c), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) {
                    let b = ellipse_world_bounds(*c, *u_tip - *c, *v_tip - *c, start, sweep);
                    bounds.add(b.min);
                    bounds.add(b.max);
                }
            }
            Kind::Infinite { .. } => {}
            _ => {
                for q in out.points(prim) {
                    bounds.add(*q);
                }
            }
        }
    }
    out.bounds = bounds;
    out
}

/// A paper-space viewport: its border (as an ordinary entity) and model space seen through it.
/// Viewport id 1 is the paper-space view itself and is not drawn.
fn viewport(b: &mut Builder, d: &Drawing, e: &Entity, vp: &aad_doc::Viewport) {
    if vp.id == 1 || !e.common.visible {
        return;
    }
    // The viewport's layer frozen hides its contents too (layer off hides only the border).
    if d.layer(&e.common.layer).is_some_and(|l| l.frozen) {
        return;
    }
    // Border (on the viewport's layer; layer off hides only the border).
    entity(b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
    if b.list.expanded >= MAX_BLOCK_EXPANSION {
        return;
    }
    let center = vp.center.xy();
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !(ok(vp.width) && ok(vp.height) && ok(vp.view_height) && center.is_finite() && vp.view_center.is_finite()) {
        return;
    }
    let s = vp.height / vp.view_height;
    if !ok(s) {
        return;
    }
    let half = Vec2::new(vp.width / 2.0, vp.height / 2.0);
    let default_rect = Bounds2::new(center - half, center + half);
    let poly_clip = clip::is_poly_clip(&vp.clip);
    let rect = if poly_clip {
        let c = Bounds2::from_points(vp.clip.iter().copied());
        if c.is_empty() { default_rect } else { c }
    } else {
        default_rect
    };
    let xf = Mat3::translate(center).then_before(Mat3::scale(s, s)).then_before(Mat3::translate(-vp.view_center));
    let mhalf = half / s;
    // Model window with slack: entity bounds are approximate (text, dimensions).
    let win = Bounds2::new(vp.view_center - mhalf, vp.view_center + mhalf).expand(mhalf.x.max(mhalf.y) * 0.1);
    // PSLTSCALE on: model-space dashes are scaled so they measure the same on paper as in
    // paper space, whatever the viewport scale.
    let lt_factor = if d.header.i64("PSLTSCALE", 1) != 0 { 1.0 / s } else { 1.0 };
    // Tessellate a generous band around the view; pans within the cell only re-transform.
    let cell = vp.view_height.max(1e-9) * VP_PAN_CELL_MULT;
    let pan_cell = ((vp.view_center.x / cell).round() as i64, (vp.view_center.y / cell).round() as i64);
    let sources = trace::viewport_sources(d, vp);
    let key = model_tess_key(d, b.opts, lt_factor, &vp.frozen_layers, &vp.layer_colors, pan_cell, vp.view_height.to_bits(), &sources);
    // Cover the whole cell plus the visible half-window (entity bounds are approximate).
    let tess_win = {
        let c = Vec2::new(pan_cell.0 as f64 * cell, pan_cell.1 as f64 * cell);
        let half_cell = cell * 0.5;
        let pad = half_cell + mhalf.x.max(mhalf.y) * 1.2;
        Bounds2::new(c - Vec2::new(pad, pad), c + Vec2::new(pad, pad))
    };
    let world = if let Some(list) = vp_model_get(key) {
        list
    } else {
        // Tessellate in model/world coordinates; pan within the cell only changes the transform.
        let list = Arc::new(trace::tessellate_sources(b, d, vp, &sources, lt_factor, tess_win));
        vp_model_put(key, Arc::clone(&list));
        list
    };
    // Cull to the current view before transforming.
    let mut culled = DisplayList { expanded: world.expanded, ..DisplayList::default() };
    for p in &world.prims {
        let pts = world.points(p);
        if matches!(p.kind, Kind::Infinite { .. }) {
            push_raw(&mut culled, p, p.kind, pts);
            continue;
        }
        let bb = match p.kind {
            Kind::Arc { sweep } => {
                if let (Some(c), Some(rim)) = (pts.first(), pts.get(1)) {
                    arc_world_bounds(*c, *rim, sweep)
                } else {
                    Bounds2::default()
                }
            }
            Kind::Ellipse { start, sweep } => {
                if let (Some(c), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) {
                    ellipse_world_bounds(*c, *u_tip - *c, *v_tip - *c, start, sweep)
                } else {
                    Bounds2::default()
                }
            }
            _ => Bounds2::from_points(pts.iter().copied()),
        };
        if !bb.is_empty() && !bb.intersects(&win) {
            continue;
        }
        push_raw(&mut culled, p, p.kind, pts);
    }
    let paper = transform_list(&culled, xf);
    b.list.expanded = b.list.expanded.saturating_add(paper.expanded);
    if poly_clip {
        append_clipped_poly(&mut b.list, &paper, &vp.clip);
    } else {
        append_clipped(&mut b.list, &paper, &rect);
    }
}

/// Append `src` to `dst`, clipped to `rect`.
fn append_clipped(dst: &mut DisplayList, src: &DisplayList, rect: &Bounds2) {
    for p in &src.prims {
        let pts = src.points(p);
        match p.kind {
            Kind::Polyline => {
                for piece in clip::clip_polyline(pts, rect) {
                    push_raw(dst, p, Kind::Polyline, &piece);
                }
            }
            Kind::Arc { sweep } => append_clipped_arc(dst, p, pts, sweep, rect, None),
            Kind::Ellipse { start, sweep } => append_clipped_ellipse(dst, p, pts, start, sweep, rect, None),
            Kind::Tris | Kind::Mask => {
                let t = clip::clip_triangles(pts, rect);
                push_raw(dst, p, p.kind, &t);
            }
            Kind::Point => {
                if let Some(q) = pts.first()
                    && rect.contains(*q)
                {
                    push_raw(dst, p, Kind::Point, &[*q]);
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(base), Some(dir)) = (pts.first(), pts.get(1))
                    && let Some((a, c)) = clip::clip_infinite(*base, *dir, ray, rect)
                {
                    push_raw(dst, p, Kind::Polyline, &[a, c]);
                }
            }
        }
    }
}

/// Keep a GPU arc when its bounds sit inside the clip; otherwise flatten and clip as a polyline.
fn append_clipped_arc(dst: &mut DisplayList, p: &DPrim, pts: &[Vec2], sweep: f32, rect: &Bounds2, poly: Option<&[Vec2]>) {
    let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) else { return };
    let bb = arc_world_bounds(*center, *rim, sweep);
    if !clip_bounds_hit(&bb, rect, poly) {
        return;
    }
    if clip_bounds_inside(&bb, rect, poly) {
        push_raw(dst, p, Kind::Arc { sweep }, pts);
        return;
    }
    let radius = center.dist(*rim);
    let start = center.angle_to(*rim);
    let mut tess = Vec::new();
    aad_geom::Arc::new(*center, radius, start, start + sweep_radians(sweep)).tessellate((radius * 1e-3).max(1e-6), &mut tess);
    append_clipped_tess(dst, p, &tess, rect, poly);
}

/// Keep a GPU ellipse when fully inside the clip; otherwise flatten and clip as a polyline.
fn append_clipped_ellipse(dst: &mut DisplayList, p: &DPrim, pts: &[Vec2], start: f32, sweep: f32, rect: &Bounds2, poly: Option<&[Vec2]>) {
    let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) else { return };
    let u = *u_tip - *center;
    let v = *v_tip - *center;
    let bb = ellipse_world_bounds(*center, u, v, start, sweep);
    if !clip_bounds_hit(&bb, rect, poly) {
        return;
    }
    if clip_bounds_inside(&bb, rect, poly) {
        push_raw(dst, p, Kind::Ellipse { start, sweep }, pts);
        return;
    }
    let mut tess = Vec::new();
    let sw = sweep_radians(sweep);
    let a0 = f64::from(start);
    let n = 32usize.max(((sw.abs() / 0.05) as usize).min(512));
    for i in 0..=n {
        let t = a0 + sw * i as f64 / n as f64;
        tess.push(*center + u * t.cos() + v * t.sin());
    }
    append_clipped_tess(dst, p, &tess, rect, poly);
}

fn clip_bounds_hit(bb: &Bounds2, rect: &Bounds2, poly: Option<&[Vec2]>) -> bool {
    match poly {
        Some(poly) => bb.intersects(&Bounds2::from_points(poly.iter().copied())),
        None => bb.intersects(rect),
    }
}

fn clip_bounds_inside(bb: &Bounds2, rect: &Bounds2, poly: Option<&[Vec2]>) -> bool {
    match poly {
        Some(poly) => {
            let corners = [bb.min, Vec2::new(bb.max.x, bb.min.y), bb.max, Vec2::new(bb.min.x, bb.max.y)];
            corners.iter().all(|q| aad_geom::point_in_polygon(poly, *q))
        }
        None => rect.contains(bb.min) && rect.contains(bb.max),
    }
}

fn append_clipped_tess(dst: &mut DisplayList, p: &DPrim, tess: &[Vec2], rect: &Bounds2, poly: Option<&[Vec2]>) {
    match poly {
        Some(poly) => {
            for piece in clip::clip_polyline_poly(tess, poly) {
                push_raw(dst, p, Kind::Polyline, &piece);
            }
        }
        None => {
            for piece in clip::clip_polyline(tess, rect) {
                push_raw(dst, p, Kind::Polyline, &piece);
            }
        }
    }
}

/// Append `src` to `dst`, clipped to an arbitrary polygon (even-odd). Falls back to the polygon's
/// axis-aligned bounds when the ring is degenerate.
fn append_clipped_poly(dst: &mut DisplayList, src: &DisplayList, poly: &[Vec2]) {
    if !clip::is_poly_clip(poly) {
        let rect = Bounds2::from_points(poly.iter().copied());
        if !rect.is_empty() {
            append_clipped(dst, src, &rect);
        }
        return;
    }
    for p in &src.prims {
        let pts = src.points(p);
        match p.kind {
            Kind::Polyline => {
                for piece in clip::clip_polyline_poly(pts, poly) {
                    push_raw(dst, p, Kind::Polyline, &piece);
                }
            }
            Kind::Arc { sweep } => {
                let rect = Bounds2::from_points(poly.iter().copied());
                append_clipped_arc(dst, p, pts, sweep, &rect, Some(poly));
            }
            Kind::Ellipse { start, sweep } => {
                let rect = Bounds2::from_points(poly.iter().copied());
                append_clipped_ellipse(dst, p, pts, start, sweep, &rect, Some(poly));
            }
            Kind::Tris | Kind::Mask => {
                let t = clip::clip_triangles_poly(pts, poly);
                push_raw(dst, p, p.kind, &t);
            }
            Kind::Point => {
                if let Some(q) = pts.first()
                    && aad_geom::point_in_polygon(poly, *q)
                {
                    push_raw(dst, p, Kind::Point, &[*q]);
                }
            }
            Kind::Infinite { ray } => {
                let rect = Bounds2::from_points(poly.iter().copied());
                if let (Some(base), Some(dir)) = (pts.first(), pts.get(1))
                    && let Some((a, c)) = clip::clip_infinite(*base, *dir, ray, &rect)
                {
                    for piece in clip::clip_polyline_poly(&[a, c], poly) {
                        push_raw(dst, p, Kind::Polyline, &piece);
                    }
                }
            }
        }
    }
}

fn push_raw(dst: &mut DisplayList, p: &DPrim, kind: Kind, pts: &[Vec2]) {
    let min = match kind {
        Kind::Polyline | Kind::Arc { .. } => 2,
        Kind::Ellipse { .. } | Kind::Tris | Kind::Mask => 3,
        _ => 1,
    };
    if pts.len() < min {
        return;
    }
    let tris = matches!(kind, Kind::Tris | Kind::Mask);
    let start = if tris { dst.tris.len() } else { dst.verts.len() } as u32;
    for q in pts {
        dst.bounds.add(*q);
    }
    if tris {
        dst.tris.extend_from_slice(pts);
    } else {
        dst.verts.extend_from_slice(pts);
    }
    dst.prims.push(DPrim { handle: p.handle, color: p.color, aci7: p.aci7, lw: p.lw, kind, start, len: pts.len() as u32 });
}

/// Build the display list for a set of loose entities (previews, rubber bands).
pub fn build_entities<'a, I: IntoIterator<Item = &'a Entity>>(d: &Drawing, ents: I, opts: &Options) -> DisplayList {
    aad_fonts::with_world_chord_tol(opts.tolerance, || {
        let ents: Vec<&Entity> = ents.into_iter().collect();
        // Only previews holding points need the model extents for relative point sizes.
        let with_view;
        let opts = if ents.iter().any(|e| matches!(e.kind, EntityKind::Point(_))) {
            with_view = with_point_view(d, &Space::Model, opts);
            &with_view
        } else {
            opts
        };
        let mut b = Builder { list: DisplayList::default(), opts, plotting: false, styles: None, named: None, budget: None };
        for e in ents {
            entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
        }
        b.list
    })
}

fn resolve(ctx: &Ctx, e: &Entity, plotting: bool) -> (Ink, f32, Option<aad_doc::Linetype>, f64, bool) {
    resolve_common(ctx, &e.common, plotting)
}

/// [`resolve`] for an object's properties `c`.
fn resolve_common(ctx: &Ctx, c: &aad_doc::Common, plotting: bool) -> (Ink, f32, Option<aad_doc::Linetype>, f64, bool) {
    let d = ctx.d;
    // Layer "0" inside a block takes the insert's layer.
    let layer_name = if c.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { c.layer.as_str() };
    let layer = d.layer(layer_name);
    let visible =
        c.visible && layer.is_none_or(|l| l.visible() && (!plotting || l.plot)) && !ctx.frozen.iter().any(|f| f.eq_ignore_ascii_case(layer_name));
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let rgb = ink(c.color, layer_color, ctx.block_color);
    let lw = match c.lineweight {
        Lineweight::ByLayer => layer.map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        Lineweight::ByBlock => ctx.block_lw,
        x => x,
    };
    let lw_mm = match lw {
        Lineweight::Mm100(v) => f32::from(v) / 100.0,
        _ => default_lineweight(d),
    };
    let lt_name = match c.linetype.to_ascii_lowercase().as_str() {
        "bylayer" => layer.map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        "byblock" => ctx.block_ltype.clone(),
        _ => c.linetype.clone(),
    };
    let lt = d.linetype(&lt_name).filter(|l| !l.pattern.is_empty()).cloned();
    let scale = d.header.f64("LTSCALE", 1.0) * c.ltscale * ctx.lt_factor;
    (rgb, lw_mm, lt, scale, visible)
}

/// The width in mm that the "Default" lineweight stands for: `LWDEFAULT` (hundredths of a mm,
/// 0 to 211), 0.25 mm when it is unset or out of range.
fn default_lineweight(d: &Drawing) -> f32 {
    match d.header.i64("LWDEFAULT", 25) {
        v @ 0..=211 => v as f32 / 100.0,
        _ => 0.25,
    }
}

fn entity(b: &mut Builder, ctx: &Ctx, e: &Entity) {
    // The entity's named plot style, for everything it draws (block contents set their own).
    let saved = b.named;
    b.named = b.styles.and_then(|s| s.named_index(ctx, &e.common));
    entity_parts(b, ctx, e);
    b.named = saved;
}

fn entity_parts(b: &mut Builder, ctx: &Ctx, e: &Entity) {
    let (rgb, lw, lt, ltscale, visible) = resolve(ctx, e, b.plotting);
    if !visible {
        return;
    }
    // A plot style's linetype replaces the object's.
    let lt = match b.styles.and_then(|s| s.linetype(rgb, b.named)) {
        Some(over) => over,
        None => lt,
    };
    let tol = b.opts.tolerance / ctx.xf.scale_factor().max(1e-12);
    let lw = if b.opts.lineweights { lw } else { 0.0 };
    // Stroke a polyline with the entity's linetype.
    let stroke = |b: &mut Builder, pts: &[Vec2]| match &lt {
        Some(lt) => {
            let min = b.opts.min_dash / ctx.xf.scale_factor().max(1e-12);
            for dash in linetype::apply(pts, lt, ltscale, min) {
                if dash.len() == 1 {
                    if let Some(p) = dash.first() {
                        b.point(ctx, rgb, *p);
                    }
                } else {
                    b.polyline(ctx, rgb, lw, &dash);
                }
            }
            // Text embedded in the linetype (shapes need SHX files and are not drawn).
            if b.opts.text {
                for sh in linetype_text::texts(ctx.d, pts, lt, ltscale, min) {
                    b.shaped(ctx, rgb, lw, &sh);
                }
            }
        }
        None => b.polyline(ctx, rgb, lw, pts),
    };
    match &e.kind {
        EntityKind::Text(t) => {
            if !b.opts.text {
                return;
            }
            let (sh, _) = place_text_entity(ctx.d, t, &t.value);
            b.shaped(ctx, rgb, lw, &sh);
        }
        EntityKind::AttDef(a) => {
            let (sh, _) = place_text_entity(ctx.d, &a.text, &a.tag);
            b.shaped(ctx, rgb, lw, &sh);
        }
        EntityKind::MText(t) => {
            if !b.opts.text {
                return;
            }
            mtext(b, ctx, t, rgb, lw);
        }
        EntityKind::Insert(ins) => insert(b, ctx, e, ins, rgb),
        EntityKind::Dimension(dm) => {
            if let Some(blk) = dm.block.as_ref().and_then(|n| ctx.d.block(n))
                && ctx.depth < aad_doc::MAX_BLOCK_DEPTH
            {
                let sub = Ctx { block_pstyle: b.named, ..sub_ctx(ctx, e, Mat3::IDENTITY) };
                for be in blk.entities.iter() {
                    if !b.expand() {
                        break;
                    }
                    entity(b, &sub, be);
                }
            } else {
                dimension(b, ctx, e, dm, lw);
            }
        }
        EntityKind::Hatch(h) => hatch::draw(b, ctx, e, h, rgb, lw, tol),
        EntityKind::LwPolyline(p) if p.const_width > 0.0 || p.vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) => {
            if b.opts.fill && ctx.d.header.i64("FILLMODE", 1) != 0 {
                match &lt {
                    // Dashed: each dash a filled piece of the band; dots a line across it.
                    Some(lt) => {
                        let min = b.opts.min_dash / ctx.xf.scale_factor().max(1e-12);
                        let (tris, ticks) = wide::dashed(p, tol, lt, ltscale, min);
                        b.tris(ctx, rgb, &tris);
                        for t in &ticks {
                            match t.as_slice() {
                                [q] => b.point(ctx, rgb, *q),
                                _ => b.polyline(ctx, rgb, lw, t),
                            }
                        }
                    }
                    None => b.tris(ctx, rgb, &fill::wide_polyline(p, tol)),
                }
            } else {
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                stroke(b, &pl.tessellate(tol));
            }
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            if p.plinegen || lt.is_none() {
                stroke(b, &pl.tessellate(tol));
            } else {
                for s in pl.segments() {
                    let mut pts = Vec::new();
                    s.tessellate(tol, &mut pts);
                    stroke(b, &pts);
                }
            }
        }
        EntityKind::Solid(s) | EntityKind::Trace(s) => {
            let c = &s.corners;
            let q = [c[0].xy(), c[1].xy(), c[3].xy(), c[2].xy()];
            if b.opts.fill && ctx.d.header.i64("FILLMODE", 1) != 0 {
                b.tris(ctx, rgb, &[q[0], q[1], q[2], q[0], q[2], q[3]]);
            } else {
                b.polyline(ctx, rgb, lw, &[q[0], q[1], q[2], q[3], q[0]]);
            }
        }
        EntityKind::Wipeout(w) => {
            // Hide what was drawn before it (draw order), then the frame per WIPEOUTFRAME:
            // 0 = none, 1 = shown and plotted, 2 = shown but not plotted.
            if b.opts.fill {
                let n = b.list.prims.len();
                b.tris(ctx, rgb, &fill::triangulate_evenodd(std::slice::from_ref(&w.boundary)));
                if let Some(p) = b.list.prims.get_mut(n) {
                    p.kind = Kind::Mask;
                }
            }
            let frame = match ctx.d.header.i64("WIPEOUTFRAME", 1) {
                0 => false,
                2 => !b.plotting,
                _ => true,
            };
            if frame {
                let mut pts = w.boundary.clone();
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                b.polyline(ctx, rgb, lw, &pts);
            }
        }
        EntityKind::Image(i) => {
            let o = i.insert.xy();
            let u = i.u.xy() * i.size.x;
            let v = i.v.xy() * i.size.y;
            b.polyline(ctx, rgb, lw, &[o, o + u, o + u + v, o + v, o]);
            b.polyline(ctx, rgb, lw, &[o, o + u + v]);
            b.polyline(ctx, rgb, lw, &[o + u, o + v]);
        }
        EntityKind::Table(t) => table(b, ctx, t, rgb, lw),
        EntityKind::Leader(l) => {
            let pts: Vec<Vec2> = l.vertices.iter().map(|v| v.xy()).collect();
            stroke(b, &pts);
            if l.arrow
                && let (Some(a), Some(n)) = (pts.first(), pts.get(1))
            {
                let st = ctx.d.dim_style(&l.style);
                // DIMASZ × DIMSCALE of the leader's style (the drawing's DIMSCALE when it is 0).
                let size = st.map(|s| s.arrow_size).unwrap_or(0.18) * leader_scale(ctx.d, st);
                let kind = dim::Arrowhead::parse(st.map(|s| s.arrow_block.as_str()).unwrap_or(""));
                let g = dim::arrowhead(kind, *a, (*a - *n).normalized(), size);
                for l in &g.lines {
                    b.polyline(ctx, rgb, lw, l);
                }
                b.tris(ctx, rgb, &g.tris);
            }
        }
        EntityKind::MLeader(m) => {
            // Straight or spline leader lines; block content draws like a block reference.
            for pts in m.leader_paths() {
                b.polyline(ctx, rgb, lw, &pts);
                if let (Some(a), Some(n)) = (pts.first(), pts.get(1)) {
                    b.tris(ctx, rgb, &dim::arrow(*a, (*a - *n).normalized(), m.arrow_size));
                }
            }
            if let Some(t) = &m.text {
                let land = m.landing.xy();
                let dir = if t.insert.x >= land.x { 1.0 } else { -1.0 };
                b.polyline(ctx, rgb, lw, &[land, land + Vec2::new(m.dogleg * dir, 0.0)]);
                mtext(b, ctx, t, rgb, lw);
            }
            if let Some(ins) = &m.block {
                if m.dogleg > 0.0 && !m.leaders.is_empty() {
                    let land = m.landing.xy();
                    let dir = if ins.insert.x >= land.x { 1.0 } else { -1.0 };
                    b.polyline(ctx, rgb, lw, &[land, land + Vec2::new(m.dogleg * dir, 0.0)]);
                }
                insert(b, ctx, e, ins, rgb);
            }
        }
        EntityKind::Point(p) => {
            // Relative sizes follow the view: undo the block / viewport scale.
            let view = b.opts.view_height / ctx.xf.scale_factor().max(1e-12);
            let size = point::size(ctx.d.header.f64("PDSIZE", 0.0), view);
            let fig = point::figure(ctx.d.header.i64("PDMODE", 0), size, p.p.xy(), p.angle, tol);
            if fig.dot {
                b.point(ctx, rgb, p.p.xy());
            }
            for l in &fig.lines {
                b.polyline(ctx, rgb, lw, l);
            }
        }
        kind => {
            for prim in kind.prims() {
                match prim {
                    Prim::Seg(aad_geom::Segment::Arc { arc, .. })
                        if lt.is_none() && b.try_arc(ctx, rgb, lw, arc.center, arc.radius, arc.start, arc.sweep()) => {}
                    Prim::Seg(s) => {
                        let mut pts = Vec::new();
                        s.tessellate(tol, &mut pts);
                        stroke(b, &pts);
                    }
                    Prim::Circle(c) if lt.is_none() && b.try_arc(ctx, rgb, lw, c.center, c.radius, 0.0, TAU) => {}
                    Prim::Circle(c) => {
                        let mut pts = Vec::new();
                        c.tessellate(tol, &mut pts);
                        stroke(b, &pts);
                    }
                    Prim::Ellipse(el) if lt.is_none() && b.try_ellipse(ctx, rgb, lw, &el) => {}
                    Prim::Ellipse(el) => {
                        let mut pts = Vec::new();
                        el.tessellate(tol, &mut pts);
                        stroke(b, &pts);
                    }
                    Prim::Spline(s) => stroke(b, &s.tessellate(tol)),
                    Prim::Point(p) => b.point(ctx, rgb, p),
                    Prim::Infinite { base, dir, ray } => b.infinite(ctx, rgb, base, dir, ray),
                    Prim::Fill(pts) => {
                        let tris = fill::triangulate_evenodd(&[pts]);
                        b.tris(ctx, rgb, &tris);
                    }
                }
            }
        }
    }
}

/// The text style of an entity (Standard defaults when missing).
fn text_style(d: &Drawing, name: &str) -> aad_doc::TextStyle {
    d.text_style(name).cloned().unwrap_or_default()
}

/// Shape a TEXT / ATTRIB / ATTDEF with its style's font and generation flags.
pub fn place_text_entity(d: &Drawing, t: &aad_doc::Text, value: &str) -> (aad_fonts::Shaped, Bounds2) {
    let ts = text_style(d, &t.style);
    let font = aad_fonts::TextFont::resolve(&ts.font);
    let (h, v) = align(t.halign, t.valign);
    let p = aad_fonts::TextParams {
        insert: t.insert.xy(),
        align_pt: t.align_pt.map(|p| p.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: t.width_factor,
        oblique: t.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    aad_fonts::place(&font, value, &p)
}

/// Lay out an MTEXT with its style's font.
pub fn layout_mtext_entity(d: &Drawing, t: &aad_doc::MText) -> aad_fonts::MTextLayout {
    let ts = text_style(d, &t.style);
    let params = aad_fonts::MTextParams {
        insert: t.insert.xy(),
        height: t.height,
        width: t.width,
        attach: t.attach,
        rotation: t.rotation,
        line_spacing: t.line_spacing,
        font: aad_fonts::TextFont::resolve(&ts.font),
        width_factor: ts.width_factor,
        oblique: ts.oblique,
    };
    aad_fonts::layout_mtext_with(&t.contents, &params)
}

fn mtext_color(c: Option<aad_fonts::MTextColor>, rgb: Ink) -> Ink {
    match c {
        Some(aad_fonts::MTextColor::Aci(i)) => {
            u8::try_from(i).ok().filter(|i| *i > 0).map(|i| ink(Color::Index(i), Color::Index(7), Color::Index(7))).unwrap_or(rgb)
        }
        Some(aad_fonts::MTextColor::Rgb(v)) => {
            Ink { rgb: Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8), aci7: false, aci: 0 }
        }
        None => rgb,
    }
}

fn mtext(b: &mut Builder, ctx: &Ctx, t: &aad_doc::MText, rgb: Ink, lw: f32) {
    let l = layout_mtext_entity(ctx.d, t);
    for p in &l.pieces {
        b.shaped(ctx, mtext_color(p.color, rgb), lw, &p.shaped);
    }
}

/// The font settings of a dimension style's text style.
pub fn dim_text(d: &Drawing, st: &aad_doc::DimStyle) -> DimText {
    let ts = text_style(d, &st.text_style);
    DimText { font: aad_fonts::TextFont::resolve(&ts.font), fixed_height: ts.height, width_factor: ts.width_factor, oblique: ts.oblique }
}

/// Geometry of a dimension in a drawing: its style, overrides and text font.
pub fn dimension_in(d: &Drawing, dm: &aad_doc::Dimension) -> DimGeometry {
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
    let st = style.with_overrides(&dm.overrides);
    dim::dimension_geometry_with(dm, &style, d.header.f64("DIMSCALE", 1.0), &dim_text(d, &st))
}

fn dimension(b: &mut Builder, ctx: &Ctx, e: &Entity, dm: &aad_doc::Dimension, lw: f32) {
    let d = ctx.d;
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = dimension_in(d, dm);
    // DIMCLRD / DIMCLRE / DIMCLRT: ByBlock = the dimension's own colour.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let dim_rgb = ink(style.dim_line_color, layer_color, own);
    let ext_rgb = ink(style.ext_line_color, layer_color, own);
    let txt_rgb = ink(style.text_color, layer_color, own);
    for (l, role) in g.lines.iter().zip(g.line_roles.iter().chain(std::iter::repeat(&LineRole::Dim))) {
        b.polyline(ctx, if *role == LineRole::Ext { ext_rgb } else { dim_rgb }, lw, l);
    }
    for t in &g.fills {
        b.tris(ctx, dim_rgb, t);
    }
    if b.opts.text {
        let sh = aad_fonts::Shaped { strokes: g.text.clone(), glyphs: g.text_glyphs.clone(), width: 0.0, ..Default::default() };
        b.shaped(ctx, txt_rgb, lw, &sh);
    }
}

/// Covered cells of a table (inside another cell's merge, not its anchor).
pub fn table_covered(t: &aad_doc::Table) -> Vec<Vec<bool>> {
    let rows = t.row_heights.len().min(10_000);
    let cols = t.col_widths.len().min(10_000);
    let mut cov = vec![vec![false; cols]; rows];
    for (r, row) in t.cells.iter().enumerate().take(rows) {
        for (c, cell) in row.iter().enumerate().take(cols) {
            if let Some((rs, cs)) = cell.merged {
                for rr in r..(r + rs.max(1) as usize).min(rows) {
                    for cc in c..(c + cs.max(1) as usize).min(cols) {
                        if (rr, cc) != (r, c)
                            && let Some(x) = cov.get_mut(rr).and_then(|v| v.get_mut(cc))
                        {
                            *x = true;
                        }
                    }
                }
            }
        }
    }
    cov
}

fn table(b: &mut Builder, ctx: &Ctx, t: &aad_doc::Table, rgb: Ink, lw: f32) {
    let o = t.insert.xy();
    let rows = t.row_heights.len().min(10_000);
    let cols = t.col_widths.len().min(10_000);
    let xs: Vec<f64> = std::iter::once(0.0)
        .chain(t.col_widths.iter().take(cols).scan(0.0, |a, w| {
            *a += w;
            Some(*a)
        }))
        .collect();
    let ys: Vec<f64> = std::iter::once(0.0)
        .chain(t.row_heights.iter().take(rows).scan(0.0, |a, h| {
            *a += h;
            Some(*a)
        }))
        .collect();
    let cov = table_covered(t);
    let style = ctx.d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&t.style)).cloned().unwrap_or_default();
    let ts = text_style(ctx.d, "Standard");
    let font = aad_fonts::TextFont::resolve(&ts.font);
    let x_at = |c: usize| xs.get(c).copied().unwrap_or(0.0);
    let y_at = |r: usize| ys.get(r).copied().unwrap_or(0.0);
    for r in 0..rows {
        for c in 0..cols {
            if cov.get(r).and_then(|v| v.get(c)).copied().unwrap_or(false) {
                continue;
            }
            let cell = t.cells.get(r).and_then(|row| row.get(c));
            let (rs, cs) = cell.and_then(|x| x.merged).map(|(a, b)| (a.max(1) as usize, b.max(1) as usize)).unwrap_or((1, 1));
            let (r2, c2) = ((r + rs).min(rows), (c + cs).min(cols));
            let (x0, x1, y0, y1) = (x_at(c), x_at(c2), y_at(r), y_at(r2));
            let p = |x: f64, y: f64| o + Vec2::new(x, -y);
            // Each cell draws its bottom and right edges; the outer top/left come from row 0 / col 0.
            b.polyline(ctx, rgb, lw, &[p(x0, y1), p(x1, y1), p(x1, y0)]);
            if r == 0 {
                b.polyline(ctx, rgb, lw, &[p(x0, y0), p(x1, y0)]);
            }
            if c == 0 {
                b.polyline(ctx, rgb, lw, &[p(x0, y0), p(x0, y1)]);
            }
            let Some(cell) = cell.filter(|x| !x.text.is_empty() && b.opts.text) else { continue };
            let title = t.title && r == 0;
            let header = t.header && r == usize::from(t.title);
            let h = if title { t.text_height * 1.4 } else { t.text_height };
            // Titles and headers are centred; data sits middle-left inside the cell margin.
            let params = if title || header {
                aad_fonts::TextParams::centered(p((x0 + x1) / 2.0, (y0 + y1) / 2.0), h, 0.0)
            } else {
                let at = p(x0 + style.margin.max(0.0), (y0 + y1) / 2.0);
                aad_fonts::TextParams {
                    align_pt: Some(at),
                    h: aad_fonts::Align::Left,
                    v: aad_fonts::VAlign::Middle,
                    ..aad_fonts::TextParams::new(at, h)
                }
            };
            let (sh, _) = aad_fonts::place(&font, &cell.text, &params);
            b.shaped(ctx, rgb, lw, &sh);
        }
    }
}

fn align(h: aad_doc::HAlign, v: aad_doc::VAlign) -> (aad_fonts::Align, aad_fonts::VAlign) {
    use aad_fonts::{Align as A, VAlign as V};
    let ha = match h {
        aad_doc::HAlign::Left => A::Left,
        aad_doc::HAlign::Center => A::Center,
        aad_doc::HAlign::Right => A::Right,
        aad_doc::HAlign::Aligned => A::Aligned,
        aad_doc::HAlign::Middle => A::Middle,
        aad_doc::HAlign::Fit => A::Fit,
    };
    let va = match v {
        aad_doc::VAlign::Baseline => V::Baseline,
        aad_doc::VAlign::Bottom => V::Bottom,
        aad_doc::VAlign::Middle => V::Middle,
        aad_doc::VAlign::Top => V::Top,
    };
    (ha, va)
}

fn sub_ctx<'a>(ctx: &Ctx<'a>, e: &Entity, m: Mat3) -> Ctx<'a> {
    let layer = if e.common.layer == "0" { ctx.block_layer.clone() } else { Some(e.common.layer.clone()) };
    let color = match e.common.color {
        Color::ByBlock => ctx.block_color,
        Color::ByLayer => ctx.layer_color(layer.as_deref().unwrap_or("0")).unwrap_or(Color::Index(7)),
        c => c,
    };
    let lw = match e.common.lineweight {
        Lineweight::ByBlock => ctx.block_lw,
        Lineweight::ByLayer => ctx.d.layer(layer.as_deref().unwrap_or("0")).map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        x => x,
    };
    let lt = match e.common.linetype.to_ascii_lowercase().as_str() {
        "byblock" => ctx.block_ltype.clone(),
        "bylayer" => ctx.d.layer(layer.as_deref().unwrap_or("0")).map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        _ => e.common.linetype.clone(),
    };
    Ctx {
        d: ctx.d,
        xf: ctx.xf.then_before(m),
        block_color: color,
        block_layer: layer,
        block_lw: lw,
        block_ltype: lt,
        depth: ctx.depth + 1,
        top: ctx.top,
        frozen: ctx.frozen,
        vp_colors: ctx.vp_colors,
        lt_factor: ctx.lt_factor,
        block_pstyle: ctx.block_pstyle,
    }
}

fn insert(b: &mut Builder, ctx: &Ctx, e: &Entity, ins: &aad_doc::Insert, rgb: Ink) {
    if ctx.depth >= aad_doc::MAX_BLOCK_DEPTH {
        return;
    }
    let clip_on = ins.clipping && ins.clip.len() >= 2 && ins.clip.iter().all(|p| p.is_finite());
    let mut clipped = if clip_on {
        Some(Builder { list: DisplayList::default(), opts: b.opts, plotting: b.plotting, styles: b.styles, named: b.named, budget: None })
    } else {
        None
    };
    let target: &mut Builder = clipped.as_mut().unwrap_or(b);
    if let Some(blk) = ctx.d.block(&ins.block) {
        let cols = ins.cols.clamp(1, 10_000);
        let rows = ins.rows.clamp(1, 10_000);
        'copies: for r in 0..rows {
            for c in 0..cols {
                if !target.expand() {
                    break 'copies;
                }
                let off = Vec2::new(ins.col_spacing * f64::from(c), ins.row_spacing * f64::from(r)).rotate(ins.rotation);
                let m = Mat3::translate(off).then_before(ins.transform(blk.base.xy()));
                let sub = Ctx { block_pstyle: target.named, ..sub_ctx(ctx, e, m) };
                for be in blk.entities.iter() {
                    // Constant/visible attribute definitions inside blocks are not drawn; attribs are.
                    if matches!(be.kind, EntityKind::AttDef(_)) {
                        continue;
                    }
                    if !target.expand() {
                        break 'copies;
                    }
                    entity(target, &sub, be);
                }
            }
        }
    }
    if let Some(sub) = clipped {
        b.list.expanded = b.list.expanded.saturating_add(sub.list.expanded);
        if clip::is_poly_clip(&ins.clip) {
            append_clipped_poly(&mut b.list, &sub.list, &ins.clip);
        } else {
            let rect = Bounds2::from_points(ins.clip.iter().copied());
            if !rect.is_empty() {
                append_clipped(&mut b.list, &sub.list, &rect);
            }
        }
    }
    if b.opts.text {
        // ATTMODE (ATTDISP): 0 shows no attributes, 1 the visible ones, 2 all of them.
        let attmode = ctx.d.header.i64("ATTMODE", 1);
        for a in &ins.attribs {
            if attmode == 0 || (a.invisible && attmode != 2) {
                continue;
            }
            let (sh, _) = place_text_entity(ctx.d, &a.text, &a.text.value);
            // Properties of the attribute's definition (as AutoCAD copies them to the attribute
            // on insertion), except those set on the attribute itself: ByBlock = the reference's,
            // layer 0 = the reference's layer.
            let def = ctx.d.block(&ins.block).and_then(|blk| blk.entities.iter().find(|be| attdef_tag(be, &a.tag)));
            let (ink, lw, shown) = match (def, a.props.is_empty()) {
                (None, true) => (rgb, resolve(ctx, e, b.plotting).1, true),
                (def, _) => {
                    let c = a.props.over(&def.map(|d| d.common.clone()).unwrap_or_else(by_block));
                    let (ink, lw, _, _, shown) = resolve_common(&sub_ctx(ctx, e, Mat3::IDENTITY), &c, b.plotting);
                    (ink, lw, shown)
                }
            };
            if shown {
                b.shaped(ctx, ink, if b.opts.lineweights { lw } else { 0.0 }, &sh);
            }
        }
    }
}

/// Properties that follow the block reference's (an attribute without a definition).
fn by_block() -> aad_doc::Common {
    aad_doc::Common { color: Color::ByBlock, linetype: "ByBlock".into(), lineweight: Lineweight::ByBlock, ..aad_doc::Common::default() }
}

/// The attribute definition with tag `tag` (any case).
fn attdef_tag(e: &Entity, tag: &str) -> bool {
    matches!(&e.kind, EntityKind::AttDef(d) if d.tag.eq_ignore_ascii_case(tag))
}

/// The overall scale of a leader drawn with dimension style `st`: its DIMSCALE, else the
/// drawing's.
fn leader_scale(d: &Drawing, st: Option<&aad_doc::DimStyle>) -> f64 {
    let dimscale = d.header.f64("DIMSCALE", 1.0);
    st.map(|s| s.effective_scale(dimscale)).unwrap_or_else(|| aad_doc::DimStyle::default().effective_scale(dimscale))
}

#[cfg(test)]
mod tests;

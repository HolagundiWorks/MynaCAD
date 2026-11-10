//! Screen mesh for the drawing canvas.
//!
//! [`build_mesh`] turns a [`DisplayList`] into the vertex bytes the GPU canvas uploads. The egui
//! shell and the WinUI host both use this layout and [`CANVAS_SHADER`], so there is one mesh path.
//!
//! Phase B spatial tiles: [`partition_prims`] / [`build_mesh_prims`] let hosts remesh dirty
//! fragments only; [`mesh_tiling_viable`] keeps the monolithic path when tile count is tiny or a
//! wipeout spans tiles (draw-order unsafe).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use rayon::prelude::*;

use aad_color::Rgb;
use aad_geom::{Bounds2, Vec2};

use crate::{DPrim, DisplayList, Kind};

/// Line/triangle vertex: position (2 × f32, relative to the origin) + sRGB colour (4 × u8).
pub const VERTEX_SIZE: usize = 12;
/// Wide-line / point instance: a, b (2 × f32 each) + colour (4 × u8) + width in points (f32).
pub const INSTANCE_SIZE: usize = 24;
/// Circle / ellipse ring instance: centre + conjugate axes U/V (3 × vec2) + (start, sweep, width, pad)
/// + colour (4 × u8) + pad. Circles use U=(r,0), V=(0,r).
pub const RING_INSTANCE_SIZE: usize = 48;
/// Point markers are 1.5 × 1.5 points (as on the CPU path).
const POINT_SIZE: f32 = 1.5;

/// WGSL for fills and screen-space strokes. `view.a` is offset xy, pixels per world unit,
/// and a linear-output flag. `view.b` is the canvas centre and size in device pixels.
/// `view.c.x` is device pixels per point; `view.c.y` selects true-scale lineweights (world mm).
pub const CANVAS_SHADER: &str = r#"
struct View {
    a: vec4<f32>, // offset x, offset y (world), scale (device px per unit), 1 = linear output
    b: vec4<f32>, // canvas centre x, y (device px), screen w, h (device px)
    c: vec4<f32>, // pixels per point, 1 = true-scale lineweights (width in world units)
};
@group(0) @binding(0) var<uniform> view: View;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) pa: vec2<f32>,
    @location(2) pb: vec2<f32>,
    @location(3) hw: f32,
    // 0 = segment stroke (pa/pb endpoints). >0 = ring/ellipse: pa=centre_px, pb=U_px, axis_v=V_px.
    @location(4) sweep: f32,
    @location(5) axis_v: vec2<f32>,
    @location(6) start: f32,
};

fn to_px(p: vec2<f32>) -> vec2<f32> {
    let w = (p + view.a.xy) * view.a.z;
    return vec2<f32>(view.b.x + w.x, view.b.y - w.y);
}

fn to_px_vec(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(v.x * view.a.z, -v.y * view.a.z);
}

fn px_to_clip(px: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(px.x / view.b.z * 2.0 - 1.0, 1.0 - px.y / view.b.w * 2.0, 0.0, 1.0);
}

fn out_color(c: vec4<f32>) -> vec4<f32> {
    if (view.a.w > 0.5) {
        let lo = c.rgb / 12.92;
        let hi = pow((c.rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
        return vec4<f32>(select(hi, lo, c.rgb <= vec3<f32>(0.04045)), c.a);
    }
    return c;
}

fn stroke_px_w(width: f32) -> f32 {
    let w_abs = abs(width);
    return max(select(1.5 * view.c.x, w_abs * select(view.c.x, view.a.z, view.c.y > 0.5), w_abs > 1e-6), 1.0);
}

fn solid(pos: vec4<f32>, color: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = pos;
    o.color = color;
    o.pa = vec2<f32>(0.0, 0.0);
    o.pb = vec2<f32>(0.0, 0.0);
    o.hw = 0.0;
    o.sweep = 0.0;
    o.axis_v = vec2<f32>(0.0, 0.0);
    o.start = 0.0;
    return o;
}

@vertex
fn vs_line(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VOut {
    return solid(px_to_clip(to_px(pos)), out_color(color));
}

@vertex
fn vs_quad(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
) -> VOut {
    var pa = to_px(a);
    var pb = to_px(b);
    // A hairline is one logical pixel. Snap a thin axis-aligned stroke onto a pixel centre
    // so it fills one row or column instead of splitting across two.
    // Negative width = butt caps (joined strip edges). Positive = round end extensions.
    let butt = width < 0.0;
    // Screen: width is points × pixels-per-point. True scale (layouts): width is world mm × scale.
    // Width ~0 is a hairline sentinel (always screen points) so default strokes stay thin on paper.
    let px_w = stroke_px_w(width);
    let dlt = pb - pa;
    if (px_w <= 3.5 && abs(dlt.y) <= 0.75 && abs(dlt.x) >= abs(dlt.y)) {
        let y = floor((pa.y + pb.y) * 0.5) + 0.5;
        pa.y = y;
        pb.y = y;
    } else if (px_w <= 3.5 && abs(dlt.x) <= 0.75 && abs(dlt.y) > abs(dlt.x)) {
        let x = floor((pa.x + pb.x) * 0.5) + 0.5;
        pa.x = x;
        pb.x = x;
    }
    let delta = pb - pa;
    let len = length(delta);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 0.0001) {
        dir = delta / len;
    }
    let n = vec2<f32>(-dir.y, dir.x);
    let hw = 0.5 * px_w;
    let t = select(0.0, 1.0, vi == 1u || vi == 4u || vi == 5u);
    let s = select(-1.0, 1.0, vi == 2u || vi == 3u || vi == 5u);
    let extend = select(hw, 0.0, butt);
    let along = mix(pa - dir * extend, pb + dir * extend, t);
    var o: VOut;
    o.pos = px_to_clip(along + n * hw * s);
    o.color = out_color(color);
    o.pa = pa;
    o.pb = pb;
    o.hw = hw;
    o.sweep = 0.0;
    o.axis_v = vec2<f32>(0.0, 0.0);
    o.start = 0.0;
    return o;
}

@vertex
fn vs_ring(
    @builtin(vertex_index) vi: u32,
    @location(0) center: vec2<f32>,
    @location(1) u: vec2<f32>,
    @location(2) v_axis: vec2<f32>,
    @location(3) params: vec4<f32>, // start, sweep, width, pad
    @location(4) color: vec4<f32>,
) -> VOut {
    let start = params.x;
    let sweep = params.y;
    let width = params.z;
    let px_w = stroke_px_w(width);
    let hw = 0.5 * px_w;
    let c = to_px(center);
    let u_px = to_px_vec(u);
    let v_px = to_px_vec(v_axis);
    let pad = hw + 1.5;
    let ext = vec2<f32>(abs(u_px.x) + abs(v_px.x), abs(u_px.y) + abs(v_px.y)) + vec2<f32>(pad, pad);
    let t = select(0.0, 1.0, vi == 1u || vi == 4u || vi == 5u);
    let s = select(-1.0, 1.0, vi == 2u || vi == 3u || vi == 5u);
    let px = c + vec2<f32>((2.0 * t - 1.0) * ext.x, (2.0 * s - 1.0) * ext.y);
    var o: VOut;
    o.pos = px_to_clip(px);
    o.color = out_color(color);
    o.pa = c;
    o.pb = u_px;
    o.hw = hw;
    o.sweep = max(sweep, 1e-6);
    o.axis_v = v_px;
    o.start = start;
    return o;
}

@fragment
fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    let p = v.pos.xy;
    var d: f32;
    if (v.sweep > 0.0) {
        let c = v.pa;
        let u = v.pb;
        let vv = v.axis_v;
        let dxy = p - c;
        let det = u.x * vv.y - u.y * vv.x;
        let tau = 6.28318530718;
        if (abs(det) < 1e-12) {
            d = length(dxy);
        } else {
            // Solve dxy = cosθ·U + sinθ·V for (cosθ, sinθ) in unit-circle parameter space.
            let cos_t = (dxy.x * vv.y - dxy.y * vv.x) / det;
            let sin_t = (u.x * dxy.y - u.y * dxy.x) / det;
            let r = sqrt(cos_t * cos_t + sin_t * sin_t);
            let d_unit = abs(r - 1.0);
            let inv_r = select(0.0, 1.0 / max(r, 1e-6), r > 1e-6);
            let dir = vec2<f32>(cos_t, sin_t) * inv_r;
            // Radial Jacobian so stroke width stays near-constant in screen pixels.
            let jac = length(u * dir.x + vv * dir.y);
            let d_ring = d_unit * max(jac, 1e-6);
            let full = v.sweep >= (tau - 1e-3);
            if (full) {
                d = d_ring;
            } else {
                let ang = atan2(sin_t, cos_t);
                var a = ang - v.start;
                a = a - floor(a / tau) * tau;
                if (a < 0.0) {
                    a = a + tau;
                }
                if (a <= v.sweep) {
                    d = d_ring;
                } else {
                    let a0 = v.start;
                    let a1 = v.start + v.sweep;
                    let p0 = c + u * cos(a0) + vv * sin(a0);
                    let p1 = c + u * cos(a1) + vv * sin(a1);
                    d = min(length(p - p0), length(p - p1));
                }
            }
        }
    } else {
        let ab = v.pb - v.pa;
        let lab = dot(ab, ab);
        let t = select(0.0, clamp(dot(p - v.pa, ab) / max(lab, 0.000001), 0.0, 1.0), lab > 0.0001);
        d = length(p - (v.pa + ab * t));
    }
    // Fills (hw = 0) stay solid. A stroke covers its width and fades across one pixel at the edge.
    var cover = select(1.0, clamp(v.hw + 0.5 - d, 0.0, 1.0), v.hw > 0.0);
    // Linear-light premultiplied blend thins dark-on-white edges; raise coverage toward display
    // gamma so a black hairline keeps its visual weight on a white sheet.
    if (v.hw > 0.0 && view.a.w > 0.5) {
        cover = pow(cover, 0.454545);
    }
    let a = v.color.a * cover;
    return vec4<f32>(v.color.rgb * a, a);
}
"#;

/// CPU-side vertex data for one display list, waiting to be uploaded.
#[derive(Default)]
pub struct MeshData {
    pub lines: Vec<u8>,
    pub tris: Vec<u8>,
    pub quads: Vec<u8>,
    /// Distance-field circle / ellipse stroke instances ([`RING_INSTANCE_SIZE`] each).
    pub rings: Vec<u8>,
    /// Draw-order breaks, one before each wipeout mask: fill / line / quad / ring counts drawn
    /// before it. Each group is drawn fills, lines, quads, rings in turn.
    pub breaks: Vec<[usize; 4]>,
}

impl MeshData {
    pub fn line_vertices(&self) -> usize {
        self.lines.len() / VERTEX_SIZE
    }
    pub fn tri_vertices(&self) -> usize {
        self.tris.len() / VERTEX_SIZE
    }
    pub fn quad_instances(&self) -> usize {
        self.quads.len() / INSTANCE_SIZE
    }
    pub fn ring_instances(&self) -> usize {
        self.rings.len() / RING_INSTANCE_SIZE
    }
}

fn rel(p: Vec2, origin: Vec2) -> [f32; 2] {
    [(p.x - origin.x) as f32, (p.y - origin.y) as f32]
}

fn push_vertex(out: &mut Vec<u8>, p: [f32; 2], c: [u8; 4]) {
    out.extend_from_slice(&p[0].to_le_bytes());
    out.extend_from_slice(&p[1].to_le_bytes());
    out.extend_from_slice(&c);
}

/// Append a filled axis-aligned-ish quad (two triangles) with explicit RGBA.
///
/// Used for translucent selection-window fills on the overlay; [`build_mesh`] always writes opaque colours.
pub fn append_fill_quad(mesh: &mut MeshData, origin: Vec2, corners: [Vec2; 4], rgba: [u8; 4]) {
    if !corners.iter().all(|p| p.is_finite()) {
        return;
    }
    for p in [corners[0], corners[1], corners[2], corners[0], corners[2], corners[3]] {
        push_vertex(&mut mesh.tris, rel(p, origin), rgba);
    }
}

fn push_instance(out: &mut Vec<u8>, a: [f32; 2], b: [f32; 2], c: [u8; 4], width: f32) {
    for v in [a[0], a[1], b[0], b[1]] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&c);
    out.extend_from_slice(&width.to_le_bytes());
}

fn push_ring(out: &mut Vec<u8>, center: [f32; 2], u: [f32; 2], v: [f32; 2], start: f32, sweep: f32, width: f32, c: [u8; 4]) {
    for x in [center[0], center[1], u[0], u[1], v[0], v[1], start, sweep, width, 0.0] {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out.extend_from_slice(&c);
    out.extend_from_slice(&[0u8; 4]);
}

/// Append one distance-field circle/arc stroke (world coords relative to `origin`).
pub fn stroke_arc(out: &mut Vec<u8>, center: Vec2, radius: f64, start: f64, sweep: f64, origin: Vec2, col: [u8; 4], width: f32) {
    if !(center.is_finite() && radius.is_finite() && sweep.is_finite()) || radius <= 1e-12 || sweep <= 0.0 {
        return;
    }
    stroke_ellipse(out, center, Vec2::new(radius, 0.0), Vec2::new(0.0, radius), start, sweep, origin, col, width);
}

/// Append one distance-field elliptical arc: `P(t) = center + u·cos(t) + v·sin(t)`.
pub fn stroke_ellipse(out: &mut Vec<u8>, center: Vec2, u: Vec2, v: Vec2, start: f64, sweep: f64, origin: Vec2, col: [u8; 4], width: f32) {
    if !(center.is_finite() && u.is_finite() && v.is_finite() && start.is_finite() && sweep.is_finite()) {
        return;
    }
    if sweep <= 0.0 || u.len() <= 1e-12 || v.len() <= 1e-12 {
        return;
    }
    // Degenerate (parallel axes) cannot form an ellipse.
    if (u.x * v.y - u.y * v.x).abs() <= 1e-18 {
        return;
    }
    let width = if width.is_finite() && width > 0.0 { width } else { HAIRLINE_POINTS };
    push_ring(out, rel(center, origin), [u.x as f32, u.y as f32], [v.x as f32, v.y as f32], start as f32, sweep as f32, width, col);
}

/// Screen-space stroke for a polyline: butt-ended edges plus round joins/caps.
///
/// One round-capped quad per segment double-blends soft edges at every joint (arcs look beaded).
/// Joined strips use butt edges (`width` stored negative) and a round disc (a == b) at each join
/// and open end so coverage meets once.
pub fn stroke_polyline(out: &mut Vec<u8>, pts: &[Vec2], origin: Vec2, col: [u8; 4], width: f32) {
    let width = if width.is_finite() && width > 0.0 { width } else { HAIRLINE_POINTS };
    let mut clean: Vec<Vec2> = Vec::with_capacity(pts.len());
    for &p in pts {
        if !p.is_finite() {
            continue;
        }
        if clean.last().is_some_and(|q| q.dist(p) < 1e-12) {
            continue;
        }
        clean.push(p);
    }
    let n = clean.len();
    if n < 2 {
        return;
    }
    let closed = n >= 3 && clean[0].dist(clean[n - 1]) < 1e-9;
    if closed {
        clean.pop();
    }
    let n = clean.len();
    if n < 2 {
        return;
    }
    if n == 2 && !closed {
        // Lone segment: one round-capped quad (same as the old path).
        push_instance(out, rel(clean[0], origin), rel(clean[1], origin), col, width);
        return;
    }
    let last = if closed { n - 1 } else { n - 2 };
    for i in 0..=last {
        let j = if i + 1 < n { i + 1 } else { 0 };
        push_instance(out, rel(clean[i], origin), rel(clean[j], origin), col, -width);
    }
    if closed {
        for p in &clean {
            let r = rel(*p, origin);
            push_instance(out, r, r, col, width);
        }
    } else {
        let start = rel(clean[0], origin);
        let end = rel(clean[n - 1], origin);
        push_instance(out, start, start, col, width);
        push_instance(out, end, end, col, width);
        for p in &clean[1..n - 1] {
            let r = rel(*p, origin);
            push_instance(out, r, r, col, width);
        }
    }
}

/// How object lineweights map to stroke width when LWDISPLAY is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineweightMode {
    /// Fixed screen pixels (model space): `lw × 3.78` points, floored at [`HAIRLINE_POINTS`].
    Screen,
    /// Paper millimetres (layouts): instance width stays in world units; the shader × scale.
    TrueScale,
}

/// Lineweight width for a primitive, or `None` for a hairline.
///
/// Screen mode: millimetres → points (`lw × 3.78`), never thinner than [`HAIRLINE_POINTS`].
/// True scale: returns `lw` as world millimetres for the shader to scale with the view.
pub fn wide_width(lw: f32, mode: Option<LineweightMode>) -> Option<f32> {
    let mode = mode?;
    if lw <= 0.0 {
        return None;
    }
    match mode {
        LineweightMode::Screen => Some((lw * 3.78).clamp(HAIRLINE_POINTS, 12.0)),
        LineweightMode::TrueScale => Some(lw),
    }
}

/// Convert a display list into vertex data relative to `origin`, colours resolved against `bg`.
pub fn build_mesh(list: &DisplayList, origin: Vec2, bg: Rgb, lwdisplay: bool) -> MeshData {
    build_mesh_mode(list, origin, bg, lwdisplay.then_some(LineweightMode::Screen))
}

/// Like [`build_mesh`], with an explicit lineweight display mode (screen vs true scale).
pub fn build_mesh_mode(list: &DisplayList, origin: Vec2, bg: Rgb, lw: Option<LineweightMode>) -> MeshData {
    let n = list.prims.len();
    if n > MAX_MESH_PRIMS {
        return MeshData::default();
    }
    let indices: Vec<usize> = (0..n).collect();
    build_mesh_prims(list, &indices, origin, bg, lw)
}

/// World-space mesh tile identity on a uniform grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileId {
    pub x: i32,
    pub y: i32,
}

impl TileId {
    /// Pack for hash maps / GPU cache keys (checked; non-finite grids never produce a tile).
    pub fn to_bits(self) -> u64 {
        ((self.x as u32 as u64) << 32) | (self.y as u32 as u64)
    }
}

/// Tile edge as a multiple of the current view height (same spirit as `MODEL_ORIGIN_CELL`).
///
/// A typical monitor then shows O(4–16) tiles of model content.
pub const MESH_TILE_VIEW_MULT: f64 = 4.0;

/// Prefer the monolithic mesh when fewer than this many non-empty tiles would be produced.
pub const MIN_MESH_TILES: usize = 2;

/// Cap retained mesh tiles; above this, fall back to a single mesh (hostile / huge extents).
pub const MAX_MESH_TILES: usize = 256;

/// Cap prims considered for tiling / mesh build (hostile input).
pub const MAX_MESH_PRIMS: usize = 2_000_000;

/// World-space edge length for mesh tiles at the current view height.
pub fn mesh_tile_edge(view_height: f64) -> f64 {
    if !view_height.is_finite() || view_height <= 0.0 {
        return 1.0;
    }
    let edge = quantize_view_height(view_height) * MESH_TILE_VIEW_MULT;
    if edge.is_finite() && edge > 0.0 { edge } else { 1.0 }
}

/// Lists with at least this many primitives get finer tiles than the view alone would give (see [`mesh_tile_edge_for`]).
pub const FINE_TILES_FROM_PRIMS: usize = 30_000;

/// Primitives a tile should hold, about, when the list is big: the tiles of a big drawing are meshed, fingerprinted and merged one tile per thread,
/// so a handful of huge tiles leaves most cores idle (a 232k-entity drawing at the fitted view fell into four tiles).
pub const TARGET_PRIMS_PER_TILE: usize = 6_000;

/// Most tiles the finer edge aims for: well under [`MAX_MESH_TILES`], with room for the empty-to-full spread of a real drawing.
const FINE_TILE_TARGET_MAX: usize = 64;

/// [`mesh_tile_edge`], halved while a big list would otherwise fall into too few tiles to work on in parallel. Small lists, and a list whose bounds
/// are unknown, get the plain edge. The result only depends on the list's size and bounds and the view height.
pub fn mesh_tile_edge_for(list: &DisplayList, view_height: f64) -> f64 {
    let mut edge = mesh_tile_edge(view_height);
    let n = list.prims.len();
    let b = list.bounds;
    if n < FINE_TILES_FROM_PRIMS || b.is_empty() || !(b.width().is_finite() && b.height().is_finite()) {
        return edge;
    }
    let target = (n / TARGET_PRIMS_PER_TILE).clamp(8, FINE_TILE_TARGET_MAX) as f64;
    for _ in 0..4 {
        let est = (b.width() / edge).ceil().max(1.0) * (b.height() / edge).ceil().max(1.0);
        // Stop when there are enough tiles, or when halving again would overshoot the cap.
        if est >= target || est * 4.0 > 220.0 {
            break;
        }
        edge *= 0.5;
    }
    edge
}

/// `view_height` rounded down to a power of two. The mesh origin step and the tile edge are derived
/// from this instead of the exact height, so a wheel-zoom tick inside the same factor-of-two range
/// keeps the origin, the tile keys and therefore the cached meshes (before, every tick changed
/// them and remeshed the whole drawing). The result is within a factor of two below the input, so
/// the origin-to-view offset stays below `32 x view_height` and `f32` precision is unaffected.
pub fn quantize_view_height(view_height: f64) -> f64 {
    if !(view_height.is_finite() && view_height > 0.0) {
        return view_height;
    }
    let q = 2f64.powi(view_height.log2().floor() as i32);
    if q.is_finite() && q > 0.0 { q } else { view_height }
}

/// Grid cell containing `p` for a positive finite `edge`.
pub fn tile_id_at(p: Vec2, edge: f64) -> Option<TileId> {
    if !(p.is_finite() && edge.is_finite() && edge > 0.0) {
        return None;
    }
    let x = (p.x / edge).floor();
    let y = (p.y / edge).floor();
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    // Reject values outside i32 before casting.
    if x < f64::from(i32::MIN) || x > f64::from(i32::MAX) || y < f64::from(i32::MIN) || y > f64::from(i32::MAX) {
        return None;
    }
    Some(TileId { x: x as i32, y: y as i32 })
}

/// Axis-aligned bounds of one display-list primitive (empty for infinites / missing verts).
pub fn prim_bounds(list: &DisplayList, prim: &DPrim) -> Bounds2 {
    let pts = list.points(prim);
    match prim.kind {
        Kind::Infinite { .. } => Bounds2::default(),
        Kind::Arc { .. } => {
            if let (Some(c), Some(rim)) = (pts.first(), pts.get(1)) {
                let r = c.dist(*rim);
                if !r.is_finite() || r <= 0.0 {
                    return Bounds2::default();
                }
                // Conservative circle bounds (sweep ignored): safe for tile assignment.
                Bounds2::new(*c - Vec2::new(r, r), *c + Vec2::new(r, r))
            } else {
                Bounds2::default()
            }
        }
        Kind::Ellipse { .. } => {
            if let (Some(c), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) {
                let u = *u_tip - *c;
                let v = *v_tip - *c;
                let rx = u.x.abs().max(v.x.abs());
                let ry = u.y.abs().max(v.y.abs());
                if !(rx.is_finite() && ry.is_finite()) {
                    return Bounds2::default();
                }
                Bounds2::new(*c - Vec2::new(rx, ry), *c + Vec2::new(rx, ry))
            } else {
                Bounds2::default()
            }
        }
        _ => Bounds2::from_points(pts.iter().copied()),
    }
}

fn prim_centroid(list: &DisplayList, prim: &DPrim) -> Option<Vec2> {
    let b = prim_bounds(list, prim);
    if b.is_empty() { None } else { Some(b.center()) }
}

/// True when a wipeout mask's bounds cover more than one mesh tile (draw order across tiles is unsafe).
pub fn wipeout_spans_tiles(list: &DisplayList, edge: f64) -> bool {
    if !(edge.is_finite() && edge > 0.0) {
        return true;
    }
    for prim in &list.prims {
        if prim.kind != Kind::Mask {
            continue;
        }
        let b = prim_bounds(list, prim);
        if b.is_empty() {
            continue;
        }
        let Some(a) = tile_id_at(b.min, edge) else { return true };
        let Some(c) = tile_id_at(b.max, edge) else { return true };
        if a != c {
            return true;
        }
    }
    false
}

/// Whether hosts should use per-tile mesh retention for this list and tile edge.
///
/// Returns false when tiling is not worthwhile (too few tiles), hostile (too many), or unsafe
/// (wipeout spans tiles). Infinites are ignored (hosts append them after tiling).
pub fn mesh_tiling_viable(list: &DisplayList, edge: f64) -> bool {
    if list.prims.len() > MAX_MESH_PRIMS {
        return false;
    }
    if wipeout_spans_tiles(list, edge) {
        return false;
    }
    let parts = partition_prims(list, edge);
    (MIN_MESH_TILES..=MAX_MESH_TILES).contains(&parts.len())
}

/// Assign each finite primitive to one tile by its bounds centroid. Order within each tile follows
/// display-list order. `Kind::Infinite` prims are omitted (view-clipped by the host each upload).
pub fn partition_prims(list: &DisplayList, edge: f64) -> Vec<(TileId, Vec<usize>)> {
    let mut map: BTreeMap<TileId, Vec<usize>> = BTreeMap::new();
    if !(edge.is_finite() && edge > 0.0) || list.prims.len() > MAX_MESH_PRIMS {
        return Vec::new();
    }
    // The tile of each primitive is worked out on all cores (the centroid of its points is most of the cost); grouping them is cheap and stays in order.
    let ids: Vec<Option<TileId>> = list
        .prims
        .par_iter()
        .map(|prim| {
            if matches!(prim.kind, Kind::Infinite { .. }) {
                return None;
            }
            prim_centroid(list, prim).and_then(|c| tile_id_at(c, edge))
        })
        .collect();
    for (i, id) in ids.into_iter().enumerate() {
        let Some(id) = id else { continue };
        map.entry(id).or_default().push(i);
        if map.len() > MAX_MESH_TILES {
            return Vec::new();
        }
    }
    map.into_iter().collect()
}

/// The tiles of `list` at `edge` when per-tile retention is worthwhile (the checks of [`mesh_tiling_viable`]), partitioned once so the caller does not
/// partition again. `None` when it is not worthwhile, hostile, or unsafe.
pub fn tiling_plan(list: &DisplayList, edge: f64) -> Option<Vec<(TileId, Vec<usize>)>> {
    if list.prims.len() > MAX_MESH_PRIMS || wipeout_spans_tiles(list, edge) {
        return None;
    }
    let parts = partition_prims(list, edge);
    (MIN_MESH_TILES..=MAX_MESH_TILES).contains(&parts.len()).then_some(parts)
}

/// A small fast hasher for fingerprints (multiply and rotate, like rustc's FxHasher). The fingerprint of a big drawing hashes tens of millions of
/// coordinates every time the zoom changes, and the standard SipHash was the larger part of that. A fingerprint only has to differ when the content differs
/// within one run, so it needs no protection against crafted input.
#[derive(Default)]
struct FastHasher(u64);

impl FastHasher {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    #[inline]
    fn add(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(Self::K);
    }
}

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.add(u64::from_le_bytes(*w));
        }
        for &b in rest {
            self.add(u64::from(b));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

/// Fingerprint of the primitives in a tile (geometry + style). Used to skip remesh when unchanged.
pub fn tile_content_fingerprint(list: &DisplayList, prim_indices: &[usize]) -> u64 {
    let mut h = FastHasher::default();
    prim_indices.len().hash(&mut h);
    for &i in prim_indices.iter().take(MAX_MESH_PRIMS) {
        let Some(prim) = list.prims.get(i) else {
            i.hash(&mut h);
            continue;
        };
        prim.handle.0.hash(&mut h);
        prim.aci7.hash(&mut h);
        prim.lw.to_bits().hash(&mut h);
        prim.color.0.hash(&mut h);
        prim.color.1.hash(&mut h);
        prim.color.2.hash(&mut h);
        match prim.kind {
            Kind::Polyline => 0u8.hash(&mut h),
            Kind::Arc { sweep } => {
                1u8.hash(&mut h);
                sweep.to_bits().hash(&mut h);
            }
            Kind::Ellipse { start, sweep } => {
                2u8.hash(&mut h);
                start.to_bits().hash(&mut h);
                sweep.to_bits().hash(&mut h);
            }
            Kind::Tris => 3u8.hash(&mut h),
            Kind::Mask => 4u8.hash(&mut h),
            Kind::Point => 5u8.hash(&mut h),
            Kind::Infinite { ray } => {
                6u8.hash(&mut h);
                ray.hash(&mut h);
            }
        }
        for p in list.points(prim) {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// Build mesh data for a subset of primitives (tile fragment / list slice), preserving index order.
pub fn build_mesh_prims(list: &DisplayList, prim_indices: &[usize], origin: Vec2, bg: Rgb, lw: Option<LineweightMode>) -> MeshData {
    let mut m = MeshData { lines: Vec::new(), tris: Vec::new(), quads: Vec::new(), rings: Vec::new(), breaks: Vec::new() };
    if prim_indices.len() > MAX_MESH_PRIMS || !origin.is_finite() {
        return m;
    }
    for &i in prim_indices {
        let Some(prim) = list.prims.get(i) else { continue };
        append_prim(&mut m, list, prim, origin, bg, lw);
    }
    m
}

fn append_prim(m: &mut MeshData, list: &DisplayList, prim: &DPrim, origin: Vec2, bg: Rgb, lw: Option<LineweightMode>) {
    let c = prim.display_rgb(bg);
    let col = [c.0, c.1, c.2, 255];
    let pts = list.points(prim);
    let hair = if matches!(lw, Some(LineweightMode::TrueScale)) { 0.0 } else { HAIRLINE_POINTS };
    let w = wide_width(prim.lw, lw).unwrap_or(hair);
    match prim.kind {
        Kind::Polyline => {
            stroke_polyline(&mut m.quads, pts, origin, col, w);
        }
        Kind::Arc { sweep } => {
            if let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) {
                let radius = center.dist(*rim);
                let start = center.angle_to(*rim);
                stroke_arc(&mut m.rings, *center, radius, start, crate::sweep_radians(sweep), origin, col, w);
            }
        }
        Kind::Ellipse { start, sweep } => {
            if let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) {
                let u = *u_tip - *center;
                let v = *v_tip - *center;
                stroke_ellipse(&mut m.rings, *center, u, v, f64::from(start), crate::sweep_radians(sweep), origin, col, w);
            }
        }
        Kind::Tris => {
            for t in pts.as_chunks::<3>().0 {
                for q in t {
                    push_vertex(&mut m.tris, rel(*q, origin), col);
                }
            }
        }
        Kind::Mask => {
            m.breaks.push([m.tri_vertices(), m.line_vertices(), m.quad_instances(), m.ring_instances()]);
            for t in pts.as_chunks::<3>().0 {
                for q in t {
                    push_vertex(&mut m.tris, rel(*q, origin), [bg.0, bg.1, bg.2, 255]);
                }
            }
        }
        Kind::Point => {
            if let Some(q) = pts.first() {
                let p = rel(*q, origin);
                push_instance(&mut m.quads, p, p, col, POINT_SIZE);
            }
        }
        // Drawn by the shell each frame (clipped to the view).
        Kind::Infinite { .. } => {}
    }
}

/// Concatenate tile meshes in order, adjusting wipeout break offsets.
pub fn merge_meshes(parts: &[&MeshData]) -> MeshData {
    let mut out = MeshData::default();
    if parts.len() > MAX_MESH_TILES {
        return out;
    }
    for part in parts {
        let tri0 = out.tri_vertices();
        let line0 = out.line_vertices();
        let quad0 = out.quad_instances();
        let ring0 = out.ring_instances();
        for b in &part.breaks {
            out.breaks.push([b[0].saturating_add(tri0), b[1].saturating_add(line0), b[2].saturating_add(quad0), b[3].saturating_add(ring0)]);
        }
        out.lines.extend_from_slice(&part.lines);
        out.tris.extend_from_slice(&part.tris);
        out.quads.extend_from_slice(&part.quads);
        out.rings.extend_from_slice(&part.rings);
    }
    out
}

/// Clip construction lines and rays to the view and append them as hairline segments.
///
/// [`build_mesh`] leaves `Kind::Infinite` out. The host calls this before [`expand_hairlines`].
pub fn append_infinite(mesh: &mut MeshData, list: &DisplayList, origin: Vec2, center: Vec2, width: f64, height: f64, bg: Rgb) {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 && center.is_finite()) {
        return;
    }
    let vis = Bounds2::from_points([
        Vec2::new(center.x - width * 0.5, center.y - height * 0.5),
        Vec2::new(center.x + width * 0.5, center.y + height * 0.5),
    ]);
    for prim in &list.prims {
        let Kind::Infinite { ray } = prim.kind else { continue };
        let pts = list.points(prim);
        let (Some(base), Some(dir)) = (pts.first(), pts.get(1)) else { continue };
        if !dir.is_finite() || dir.len() < 1e-12 {
            continue;
        }
        let far = (vis.width() + vis.height()) * 2.0 + base.dist(vis.center());
        if !far.is_finite() {
            continue;
        }
        let a = if ray { *base } else { *base - *dir * far };
        let b = *base + *dir * far;
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        let rgb = prim.display_rgb(bg);
        let col = [rgb.0, rgb.1, rgb.2, 255];
        push_vertex(&mut mesh.lines, rel(a, origin), col);
        push_vertex(&mut mesh.lines, rel(b, origin), col);
    }
}

/// Default document hairline width in points (logical pixels ≈ CSS px at 96 DPI).
///
/// AutoCAD model-space hairlines are about one device pixel; on HiDPI that reads as frail.
/// 1.5 px is the default displayed weight after pixel-centre snap when LWDISPLAY is off.
pub const HAIRLINE_POINTS: f32 = 1.5;

/// Turn hairline segments into screen-space quads, in the same draw order.
///
/// Hardware line lists leave gaps on diagonals and draw a single device pixel, which is thinner
/// than one logical pixel on a scaled display. The quad shader strokes these in screen space.
pub fn expand_hairlines(mesh: &mut MeshData) {
    expand_hairlines_width(mesh, HAIRLINE_POINTS);
}

/// Like [`expand_hairlines`] for a true-scale lineweight mesh: width `0` is the shader hairline sentinel.
pub fn expand_hairlines_true_scale(mesh: &mut MeshData) {
    expand_hairlines_width(mesh, 0.0);
}

/// Like [`expand_hairlines`], with an explicit stroke width in points.
///
/// Overlay hairlines (crosshair, rubber band) use a half-point width so they stay near one device
/// pixel on common display scales, matching AutoCAD-style thin crosshairs.
/// Width `0` is kept (true-scale hairline sentinel for the canvas shader).
pub fn expand_hairlines_width(mesh: &mut MeshData, width: f32) {
    if mesh.line_vertices() < 2 {
        return;
    }
    let width = if width.is_finite() && width >= 0.0 { width } else { 1.0 };
    let line_n = mesh.line_vertices();
    let quad_n = mesh.quad_instances();
    let ring_n = mesh.ring_instances();
    let mut quads = Vec::with_capacity(mesh.quads.len() + (line_n / 2) * INSTANCE_SIZE);
    let mut breaks = Vec::with_capacity(mesh.breaks.len());
    let mut line_from = 0usize;
    let mut quad_from = 0usize;
    let end = [mesh.tri_vertices(), line_n, quad_n, ring_n];
    for [tri, line_to, quad_to, ring_to] in mesh.breaks.iter().copied().chain(std::iter::once(end)) {
        let mut i = line_from;
        while i + 1 < line_to {
            if let (Some((a, color)), Some((b, _))) = (read_vertex(&mesh.lines, i), read_vertex(&mesh.lines, i + 1)) {
                push_instance(&mut quads, a, b, color, width);
            }
            i += 2;
        }
        let lo = quad_from.saturating_mul(INSTANCE_SIZE);
        let hi = quad_to.saturating_mul(INSTANCE_SIZE).min(mesh.quads.len());
        if lo < hi {
            quads.extend_from_slice(&mesh.quads[lo..hi]);
        }
        breaks.push([tri, 0, quads.len() / INSTANCE_SIZE, ring_to]);
        line_from = line_to;
        quad_from = quad_to;
    }
    breaks.pop();
    mesh.lines.clear();
    mesh.quads = quads;
    mesh.breaks = breaks;
}

fn read_vertex(bytes: &[u8], index: usize) -> Option<([f32; 2], [u8; 4])> {
    let at = index.checked_mul(VERTEX_SIZE)?;
    let b = bytes.get(at..at + VERTEX_SIZE)?;
    let x = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let y = f32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    Some(([x, y], [b[8], b[9], b[10], b[11]]))
}

/// The vertex origin for a display list seen at `view_center` / `view_height`: the list's centre
/// normally; at deep zoom (the view much smaller than the drawing) a point near the view, snapped
/// to a coarse grid so panning re-uploads only every few dozen screens.
pub fn choose_origin(bounds: &Bounds2, view_center: Vec2, view_height: f64) -> Vec2 {
    let size = bounds.width().max(bounds.height());
    if bounds.is_empty()
        || !view_center.is_finite()
        || view_height.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        || view_height * 2000.0 >= size
    {
        return if bounds.is_empty() { Vec2::ZERO } else { bounds.center() };
    }
    let step = quantize_view_height(view_height) * 64.0;
    Vec2::new((view_center.x / step).round() * step, (view_center.y / step).round() * step)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn zoom_ticks_inside_one_octave_keep_origin_and_tile_edge() {
        let b = Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(1.0e6, 1.0e6));
        let c = Vec2::new(123_456.0, 654_321.0);
        let o = choose_origin(&b, c, 100.0);
        for h in [100.0, 110.0, 125.0, 127.9] {
            assert_eq!(choose_origin(&b, c, h), o, "height {h}");
            assert_eq!(mesh_tile_edge(h), mesh_tile_edge(100.0), "height {h}");
        }
        assert_ne!(mesh_tile_edge(130.0), mesh_tile_edge(100.0));
        assert!(quantize_view_height(100.0) <= 100.0 && quantize_view_height(100.0) > 50.0);
        assert!(quantize_view_height(f64::NAN).is_nan());
        assert_eq!(quantize_view_height(0.0), 0.0);
    }

    #[test]
    fn hairlines_become_quads_and_keep_wipeout_order() {
        let mut mesh = MeshData::default();
        push_vertex(&mut mesh.lines, [0.0, 0.0], [1, 2, 3, 255]);
        push_vertex(&mut mesh.lines, [2.0, 0.0], [1, 2, 3, 255]);
        push_instance(&mut mesh.quads, [0.0, 0.0], [1.0, 0.0], [9, 9, 9, 255], 2.0);
        push_vertex(&mut mesh.lines, [0.0, 1.0], [4, 5, 6, 255]);
        push_vertex(&mut mesh.lines, [0.0, 3.0], [4, 5, 6, 255]);
        mesh.breaks.push([0, 2, 1, 0]);
        expand_hairlines(&mut mesh);
        assert_eq!(mesh.line_vertices(), 0);
        assert_eq!(mesh.quad_instances(), 3);
        assert_eq!(mesh.breaks, vec![[0, 0, 2, 0]]);
        let width = f32::from_le_bytes(mesh.quads[20..24].try_into().expect("width bytes"));
        assert_eq!(width, HAIRLINE_POINTS);
    }

    #[test]
    fn gpu_arc_emits_one_ring_instance() {
        let mut list = DisplayList::default();
        list.verts.extend([Vec2::new(10.0, 20.0), Vec2::new(15.0, 20.0)]);
        list.prims.push(crate::DPrim {
            handle: aad_doc::Handle(1),
            color: Rgb(0, 0, 0),
            aci7: false,
            lw: 0.0,
            kind: Kind::Arc { sweep: std::f32::consts::TAU },
            start: 0,
            len: 2,
        });
        let m = build_mesh(&list, Vec2::ZERO, Rgb(255, 255, 255), false);
        assert_eq!(m.ring_instances(), 1);
        assert_eq!(m.quad_instances(), 0);
        // Circle packs as U=(r,0), V=(0,r) at bytes 8..24.
        let ux = f32::from_le_bytes(m.rings[8..12].try_into().expect("ux"));
        let uy = f32::from_le_bytes(m.rings[12..16].try_into().expect("uy"));
        let vx = f32::from_le_bytes(m.rings[16..20].try_into().expect("vx"));
        let vy = f32::from_le_bytes(m.rings[20..24].try_into().expect("vy"));
        assert!((ux - 5.0).abs() < 1e-5 && uy.abs() < 1e-5, "U=({ux},{uy})");
        assert!(vx.abs() < 1e-5 && (vy - 5.0).abs() < 1e-5, "V=({vx},{vy})");
    }

    #[test]
    fn gpu_ellipse_emits_one_ring_instance() {
        let mut list = DisplayList::default();
        let c = Vec2::new(0.0, 0.0);
        list.verts.extend([c, c + Vec2::new(10.0, 0.0), c + Vec2::new(0.0, 4.0)]);
        list.prims.push(crate::DPrim {
            handle: aad_doc::Handle(2),
            color: Rgb(0, 0, 0),
            aci7: false,
            lw: 0.0,
            kind: Kind::Ellipse { start: 0.0, sweep: std::f32::consts::TAU },
            start: 0,
            len: 3,
        });
        let m = build_mesh(&list, Vec2::ZERO, Rgb(255, 255, 255), false);
        assert_eq!(m.ring_instances(), 1);
        assert_eq!(m.quad_instances(), 0);
        let ux = f32::from_le_bytes(m.rings[8..12].try_into().expect("ux"));
        let vy = f32::from_le_bytes(m.rings[20..24].try_into().expect("vy"));
        assert!((ux - 10.0).abs() < 1e-5, "ux {ux}");
        assert!((vy - 4.0).abs() < 1e-5, "vy {vy}");
    }

    #[test]
    fn dashed_style_keeps_ellipse_out_of_rings() {
        // Mesh only sees Kind::Ellipse; dashed entities tessellate to polylines in the list builder.
        let mut list = DisplayList::default();
        list.verts.extend([Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)]);
        list.prims.push(crate::DPrim {
            handle: aad_doc::Handle(3),
            color: Rgb(0, 0, 0),
            aci7: false,
            lw: 0.0,
            kind: Kind::Polyline,
            start: 0,
            len: 4,
        });
        let m = build_mesh(&list, Vec2::ZERO, Rgb(255, 255, 255), false);
        assert_eq!(m.ring_instances(), 0);
        assert!(m.quad_instances() > 0);
    }

    #[test]
    fn displayed_lineweights_are_monotonic_and_at_least_hairline() {
        assert!(wide_width(0.25, None).is_none());
        let mut prev = 0.0_f32;
        for &hundredths in &aad_doc::Lineweight::STANDARD {
            let mm = f32::from(hundredths) / 100.0;
            let Some(w) = wide_width(mm, Some(LineweightMode::Screen)) else {
                // 0.00 mm stays a hairline expander path.
                assert_eq!(hundredths, 0);
                continue;
            };
            assert!(w >= HAIRLINE_POINTS, "{mm} mm → {w} pt below hairline");
            assert!(w + 1e-5 >= prev, "{mm} mm ({w} pt) thinner than previous ({prev} pt)");
            prev = w;
        }
        assert!(prev > HAIRLINE_POINTS, "heavier standard weights must exceed the hairline floor");
    }

    #[test]
    fn true_scale_lineweights_keep_world_millimetres() {
        assert_eq!(wide_width(0.5, Some(LineweightMode::TrueScale)), Some(0.5));
        assert_eq!(wide_width(0.25, Some(LineweightMode::TrueScale)), Some(0.25));
        assert!(wide_width(0.0, Some(LineweightMode::TrueScale)).is_none());
        let mut list = DisplayList::default();
        list.prims.push(crate::DPrim {
            handle: aad_doc::Handle(1),
            color: Rgb(0, 0, 0),
            aci7: false,
            lw: 0.5,
            kind: Kind::Polyline,
            start: 0,
            len: 2,
        });
        list.verts.extend([Vec2::ZERO, Vec2::new(10.0, 0.0)]);
        list.bounds.add(Vec2::ZERO);
        list.bounds.add(Vec2::new(10.0, 0.0));
        let screen = build_mesh_mode(&list, Vec2::ZERO, Rgb(255, 255, 255), Some(LineweightMode::Screen));
        let paper = build_mesh_mode(&list, Vec2::ZERO, Rgb(255, 255, 255), Some(LineweightMode::TrueScale));
        let sw = instance_width(&screen.quads, 0).abs();
        let pw = instance_width(&paper.quads, 0).abs();
        assert!((sw - (0.5_f32 * 3.78).clamp(HAIRLINE_POINTS, 12.0)).abs() < 1e-4);
        assert!((pw - 0.5).abs() < 1e-4, "true-scale keeps millimetres, got {pw}");
    }

    fn instance_width(quads: &[u8], index: usize) -> f32 {
        let at = index * INSTANCE_SIZE + 20;
        f32::from_le_bytes(quads[at..at + 4].try_into().expect("width"))
    }

    fn instance_is_point(quads: &[u8], index: usize) -> bool {
        let at = index * INSTANCE_SIZE;
        let a = f32::from_le_bytes(quads[at..at + 4].try_into().unwrap());
        let b = f32::from_le_bytes(quads[at + 4..at + 8].try_into().unwrap());
        let c = f32::from_le_bytes(quads[at + 8..at + 12].try_into().unwrap());
        let d = f32::from_le_bytes(quads[at + 12..at + 16].try_into().unwrap());
        (a - c).abs() < 1e-6 && (b - d).abs() < 1e-6
    }

    #[test]
    fn polyline_strip_uses_butt_edges_and_round_joins() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let mut quads = Vec::new();
        stroke_polyline(&mut quads, &pts, Vec2::ZERO, [0, 0, 0, 255], 2.0);
        // 3 butt edges + 2 end caps + 2 interior joins = 7
        assert_eq!(quads.len() / INSTANCE_SIZE, 7);
        let mut butts = 0;
        let mut discs = 0;
        for i in 0..7 {
            let w = instance_width(&quads, i);
            if instance_is_point(&quads, i) {
                assert!(w > 0.0, "join/cap discs use round (positive) width");
                discs += 1;
            } else {
                assert!(w < 0.0, "strip edges use butt (negative) width");
                butts += 1;
            }
        }
        assert_eq!(butts, 3);
        assert_eq!(discs, 4);
    }

    #[test]
    fn closed_polyline_strip_has_joins_at_every_vertex() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 0.0)];
        let mut quads = Vec::new();
        stroke_polyline(&mut quads, &pts, Vec2::ZERO, [0, 0, 0, 255], 1.5);
        // 3 edges + 3 vertex discs
        assert_eq!(quads.len() / INSTANCE_SIZE, 6);
        assert_eq!((0..6).filter(|&i| instance_is_point(&quads, i)).count(), 3);
    }

    #[test]
    fn two_point_polyline_stays_one_round_quad() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(5.0, 0.0)];
        let mut quads = Vec::new();
        stroke_polyline(&mut quads, &pts, Vec2::ZERO, [0, 0, 0, 255], HAIRLINE_POINTS);
        assert_eq!(quads.len() / INSTANCE_SIZE, 1);
        assert!(instance_width(&quads, 0) > 0.0);
    }

    fn line_prim(handle: u64, a: Vec2, b: Vec2) -> (crate::DPrim, [Vec2; 2]) {
        (crate::DPrim { handle: aad_doc::Handle(handle), color: Rgb(0, 0, 0), aci7: false, lw: 0.0, kind: Kind::Polyline, start: 0, len: 2 }, [a, b])
    }

    #[test]
    fn build_mesh_prims_matches_full_list_for_all_indices() {
        let mut list = DisplayList::default();
        let (p0, pts0) = line_prim(1, Vec2::ZERO, Vec2::new(10.0, 0.0));
        list.verts.extend(pts0);
        list.prims.push(p0);
        let (mut p1, pts1) = line_prim(2, Vec2::new(100.0, 0.0), Vec2::new(110.0, 0.0));
        p1.start = 2;
        list.verts.extend(pts1);
        list.prims.push(p1);
        let full = build_mesh_mode(&list, Vec2::ZERO, Rgb(255, 255, 255), None);
        let slice = build_mesh_prims(&list, &[0, 1], Vec2::ZERO, Rgb(255, 255, 255), None);
        assert_eq!(full.quads, slice.quads);
        assert_eq!(full.rings, slice.rings);
        assert_eq!(full.tris, slice.tris);
        let one = build_mesh_prims(&list, &[1], Vec2::ZERO, Rgb(255, 255, 255), None);
        assert!(one.quad_instances() > 0);
        assert!(one.quad_instances() < full.quad_instances());
    }

    #[test]
    fn partition_splits_distant_prims_and_wipeout_span_is_detected() {
        let mut list = DisplayList::default();
        let (p0, pts0) = line_prim(1, Vec2::ZERO, Vec2::new(1.0, 0.0));
        list.verts.extend(pts0);
        list.prims.push(p0);
        let (mut p1, pts1) = line_prim(2, Vec2::new(1000.0, 0.0), Vec2::new(1001.0, 0.0));
        p1.start = 2;
        list.verts.extend(pts1);
        list.prims.push(p1);
        let edge = 100.0;
        let parts = partition_prims(&list, edge);
        assert_eq!(parts.len(), 2, "distant lines land in different tiles");
        assert!(mesh_tiling_viable(&list, edge));

        // A wipeout covering both tiles makes tiling unsafe.
        let mut wipe = list.clone();
        wipe.tris.extend([
            Vec2::new(-1.0, -1.0),
            Vec2::new(1002.0, -1.0),
            Vec2::new(1002.0, 1.0),
            Vec2::new(-1.0, -1.0),
            Vec2::new(1002.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ]);
        wipe.prims.push(crate::DPrim { handle: aad_doc::Handle(9), color: Rgb(0, 0, 0), aci7: false, lw: 0.0, kind: Kind::Mask, start: 0, len: 6 });
        assert!(wipeout_spans_tiles(&wipe, edge));
        assert!(!mesh_tiling_viable(&wipe, edge));
    }

    #[test]
    fn merge_meshes_preserves_counts_and_break_offsets() {
        let a = build_mesh_prims(
            &{
                let mut list = DisplayList::default();
                let (p, pts) = line_prim(1, Vec2::ZERO, Vec2::new(1.0, 0.0));
                list.verts.extend(pts);
                list.prims.push(p);
                list
            },
            &[0],
            Vec2::ZERO,
            Rgb(255, 255, 255),
            None,
        );
        let mut b = MeshData::default();
        b.breaks.push([0, 0, 0, 0]);
        push_vertex(&mut b.tris, [0.0, 0.0], [1, 2, 3, 255]);
        push_vertex(&mut b.tris, [1.0, 0.0], [1, 2, 3, 255]);
        push_vertex(&mut b.tris, [0.0, 1.0], [1, 2, 3, 255]);
        let merged = merge_meshes(&[&a, &b]);
        assert_eq!(merged.quad_instances(), a.quad_instances());
        assert_eq!(merged.tri_vertices(), 3);
        assert_eq!(merged.breaks, vec![[a.tri_vertices(), a.line_vertices(), a.quad_instances(), a.ring_instances()]]);
    }

    #[test]
    fn tile_fingerprint_changes_only_when_content_changes() {
        let mut list = DisplayList::default();
        let (p0, pts0) = line_prim(1, Vec2::ZERO, Vec2::new(1.0, 0.0));
        list.verts.extend(pts0);
        list.prims.push(p0);
        let fp = tile_content_fingerprint(&list, &[0]);
        assert_eq!(fp, tile_content_fingerprint(&list, &[0]));
        list.verts[1] = Vec2::new(2.0, 0.0);
        assert_ne!(fp, tile_content_fingerprint(&list, &[0]));
    }
}

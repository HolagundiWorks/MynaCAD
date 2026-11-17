//! The drawing area: grid, entities, selection highlighting, grips, rubber bands, object-snap
//! markers, crosshair cursor, UCS icon and mouse handling.

use std::collections::HashMap;

use aad_color::Rgb;
use aad_doc::Handle;
use aad_engine::Input;
use aad_engine::snap::{self, SnapHit};
use aad_geom::{Bounds2, Vec2};
use aad_render::mesh::{MeshData, TileId};
use aad_render::{DisplayList, Kind};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// World ↔ screen transform for the canvas rect.
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub rect: Rect,
    pub center: Vec2,
    /// Pixels (points) per drawing unit.
    pub scale: f64,
}

impl Xf {
    pub fn to_screen(&self, p: Vec2) -> Pos2 {
        let c = self.rect.center();
        pos2(c.x + ((p.x - self.center.x) * self.scale) as f32, c.y - ((p.y - self.center.y) * self.scale) as f32)
    }
    pub fn to_world(&self, p: Pos2) -> Vec2 {
        let c = self.rect.center();
        Vec2::new(self.center.x + f64::from(p.x - c.x) / self.scale, self.center.y - f64::from(p.y - c.y) / self.scale)
    }
    pub fn world_bounds(&self) -> Bounds2 {
        Bounds2::new(self.to_world(self.rect.left_bottom()), self.to_world(self.rect.right_top()))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HotGrip {
    pub handle: Handle,
    pub index: usize,
    pub base: Vec2,
    pub mode: aad_engine::grips::GripMode,
}

impl HotGrip {
    pub fn label(&self) -> &'static str {
        use aad_engine::grips::GripMode::*;
        match self.mode {
            Stretch => "** STRETCH ** Specify stretch point or [Base point/Copy/Undo/eXit]:",
            Move => "** MOVE ** Specify move point or [Base point/Copy/Undo/eXit]:",
            Rotate => "** ROTATE ** Specify rotation angle or [Base point/Copy/Undo/Reference/eXit]:",
            Scale => "** SCALE ** Specify scale factor or [Base point/Copy/Undo/Reference/eXit]:",
            Mirror => "** MIRROR ** Specify second point or [Base point/Copy/Undo/eXit]:",
        }
    }
    pub fn next_mode(&mut self) {
        use aad_engine::grips::GripMode::*;
        self.mode = match self.mode {
            Stretch => Move,
            Move => Rotate,
            Rotate => Scale,
            Scale => Mirror,
            Mirror => Stretch,
        };
    }
}

/// Apply a hot grip at `to` (one undo step) and clear it.
pub fn apply_hot_grip(app: &mut CadApp, to: Vec2) {
    if let Some(g) = app.canvas.hot_grip.take()
        && let Err(e) = app.session.grip_edit(g.handle, g.index, to, g.mode)
    {
        app.session.echo(e.to_string());
    }
}

#[derive(Default)]
pub struct CanvasState {
    pub list: Option<DisplayList>,
    key: ListKey,
    /// A display list for a new zoom band being built on a worker thread.
    pending: Option<PendingList>,
    /// The drawing `list` was built from (the next edit updates the list from it, see `aad_render::rebuild`).
    list_doc: Option<std::sync::Arc<aad_doc::Drawing>>,
    /// Bumped whenever `list` is rebuilt (identifies it for [`PrimIndex`]).
    list_gen: u64,
    prim_index: PrimIndex,
    /// Set when the app runs on wgpu: entities are drawn by [`crate::gpu`]; otherwise on the CPU.
    pub gpu: Option<crate::gpu::GpuTarget>,
    /// The mesh last handed to the GPU (see [`crate::gpu::CanvasCallback::key`]).
    mesh_key: Option<u64>,
    mesh_slot: crate::gpu::MeshSlot,
    mesh_origin: Vec2,
    /// Retained per-tile meshes (Phase B); remeshed only when content fingerprints change.
    mesh_tiles: HashMap<TileId, (u64, MeshData)>,
    mesh_basis: Option<u64>,
    /// Indices of infinite-line primitives in `list` (drawn on the CPU in GPU mode).
    infinite: Vec<usize>,
    /// Milliseconds spent converting the display list to vertex data (last rebuild).
    pub mesh_ms: f64,
    /// Ring instances in the last GPU mesh (for `ui.inspect` perf).
    pub mesh_rings: usize,
    /// Byte size of the last GPU mesh buffers (for `ui.inspect` perf).
    pub mesh_bytes: usize,
    pub rect: Option<Rect>,
    pub xf: Option<Xf>,
    pub hover: Option<Handle>,
    pub snap: Option<SnapHit>,
    /// Effective cursor point after snaps/ortho/polar.
    pub cursor: Option<Vec2>,
    /// The polar or object snap tracking path(s) the cursor is on.
    pub track: Option<snap::tracking::Track>,
    pan_last: Option<Pos2>,
    /// True when the latest scroll came from a trackpad (or other precise-scrolling device):
    /// scrolling then pans and pinch zooms. A notched mouse wheel clears it so the wheel zooms.
    scroll_pans: bool,
    /// A hot (clicked) grip being dragged: entity, grip index, grip position, mode.
    pub hot_grip: Option<HotGrip>,
    /// Zoom to extents once the canvas size is known (after opening a drawing).
    pub zoom_pending: bool,
    pub build_ms: f64,
    pub draw_ms: f64,
    hover_at: Option<Pos2>,
    /// A selection window opened by a press-and-drag, to close where the button is let go.
    drag_window: bool,
    /// The right-click that opened the shortcut menu held Shift or Ctrl: it is the object snap
    /// menu ([`crate::context_menu`]).
    pub snap_menu: bool,
    /// Tab was pressed over a snap marker: show the next candidate ([`SnapCycle`]).
    pub snap_tab: bool,
    pub snap_cycle: Option<SnapCycle>,
}

/// Tab cycling through the candidate snaps under the cursor: where it started and which
/// candidate shows. Moving the cursor out of the aperture ends it.
#[derive(Clone, Copy, Debug)]
pub struct SnapCycle {
    anchor: Vec2,
    index: usize,
}

/// Primitives of `list` sorted by entity handle, built on the first highlight after each list
/// rebuild. Highlighting the hovered entity used to scan every primitive on every frame: ~2.5 ms
/// per frame on a 270k-entity drawing, whether or not anything changed.
#[derive(Default)]
struct PrimIndex {
    list_gen: Option<u64>,
    by_handle: Vec<(Handle, usize)>,
}

impl PrimIndex {
    /// Indices of the primitives drawn for `handles` (in list order).
    fn prims(&mut self, list_gen: u64, list: &DisplayList, handles: &[Handle]) -> Vec<usize> {
        if handles.is_empty() {
            return Vec::new();
        }
        if self.list_gen != Some(list_gen) {
            self.by_handle = list.prims.iter().enumerate().map(|(i, p)| (p.handle, i)).collect();
            self.by_handle.sort_unstable();
            self.list_gen = Some(list_gen);
        }
        let mut out = Vec::new();
        for h in handles {
            let from = self.by_handle.partition_point(|(k, _)| k < h);
            out.extend(self.by_handle.get(from..).unwrap_or(&[]).iter().take_while(|(k, _)| k == h).map(|(_, i)| *i));
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Indices of the primitives in `list` drawn for `handles` (a scan; for small lists).
fn prims_of(list: &DisplayList, handles: &[Handle]) -> Vec<usize> {
    if handles.is_empty() {
        return Vec::new();
    }
    let set: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    list.prims.iter().enumerate().filter(|(_, pr)| set.contains(&pr.handle)).map(|(i, _)| i).collect()
}

fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// The state a display list was built for: document id, revision, zoom band, line weight display,
/// document identity.
type ListKey = (u64, u64, i32, bool, usize);

/// A display list being rebuilt on a worker thread for a new zoom band.
pub struct PendingList {
    key: ListKey,
    rx: std::sync::mpsc::Receiver<DisplayList>,
    t0: f64,
}

fn list_options(app: &CadApp, st: &aad_engine::DocState, band: i32, px: f64) -> aad_render::Options {
    let view = st.view();
    // `px` is already world units per screen pixel.
    let lod = aad_render::lod_flags_for_view(px.max(1e-300), 1.0);
    aad_render::Options {
        tolerance: aad_render::tessellation_tolerance(band),
        min_dash: px * 2.0,
        text: lod.text,
        fill: lod.fill,
        text_greek: lod.text_greek,
        fill_tint: lod.fill_tint,
        lineweights: app.session.settings.lwdisplay,
        view_height: view.height,
        view_center: matches!(st.space, aad_doc::Space::Model).then_some(view.center),
        plot_style_table: None,
        screen_px: 0.0,
    }
}

/// Make `app.canvas.list` match the drawing and the zoom. A change to the drawing rebuilds at once
/// (the picture must show the edit). Crossing into another zoom band only refines tessellation, so
/// on a big drawing it is built on a worker thread while the current list stays on screen: the
/// build of a 25,000-entity drawing takes over a second and used to freeze every band change.
fn ensure_list(app: &mut CadApp, ctx: &egui::Context, px: f64) {
    let Ok(st) = app.session.state() else { return };
    // Take a finished background build first.
    if let Some(p) = &app.canvas.pending {
        match p.rx.try_recv() {
            Ok(list) => {
                if let Some(p) = app.canvas.pending.take() {
                    // Still the same drawing? A list built for an older revision is dropped.
                    if (p.key.0, p.key.1, p.key.3, p.key.4)
                        == (st.uid, st.revision, app.session.settings.lwdisplay, std::sync::Arc::as_ptr(&st.doc) as usize)
                    {
                        app.canvas.list = Some(list);
                        app.canvas.key = p.key;
                        app.canvas.list_doc = Some(st.doc.clone());
                        app.canvas.list_gen = app.canvas.list_gen.wrapping_add(1);
                        app.canvas.build_ms = crate::now_ms() - p.t0;
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(30)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => app.canvas.pending = None,
        }
    }
    // Rebuild when the drawing changes or the zoom moves by more than 2x (tessellation band).
    let band = aad_render::tessellation_band(px);
    let key: ListKey = (st.uid, st.revision, band, app.session.settings.lwdisplay, std::sync::Arc::as_ptr(&st.doc) as usize);
    if app.canvas.list.is_some() && app.canvas.key == key {
        return;
    }
    let space = st.space.clone();
    let same_drawing = (app.canvas.key.0, app.canvas.key.1, app.canvas.key.3, app.canvas.key.4) == (key.0, key.1, key.3, key.4);
    #[cfg(not(target_arch = "wasm32"))]
    if app.canvas.list.is_some() && same_drawing {
        if app.canvas.pending.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let (doc, opts, repaint, space) = (st.doc.clone(), list_options(app, st, band, px), ctx.clone(), space.clone());
            let spawned = std::thread::Builder::new().name("aad-display-list".into()).spawn(move || {
                let _ = tx.send(aad_render::build(&doc, &space, &opts));
                repaint.request_repaint();
            });
            if spawned.is_ok() {
                app.canvas.pending = Some(PendingList { key, rx, t0: crate::now_ms() });
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
                return;
            }
        } else {
            return; // one background build at a time; the next band is started when it lands
        }
    }
    let _ = same_drawing;
    app.canvas.pending = None;
    let t0 = crate::now_ms();
    let opts = list_options(app, st, band, px);
    // Picking and snapping need the spatial index: build it off the UI thread, alongside the display list, so the first pick does not.
    aad_engine::spatial::warm(st.doc.clone(), space.clone());
    // An edit at the same zoom: keep what did not change and draw only the changed entities.
    let incremental = match (&app.canvas.list, &app.canvas.list_doc) {
        (Some(old), Some(old_doc))
            if matches!(space, aad_doc::Space::Model) && (app.canvas.key.0, app.canvas.key.2, app.canvas.key.3) == (key.0, key.2, key.3) =>
        {
            aad_render::rebuild(old, old_doc, &st.doc, &opts)
        }
        _ => None,
    };
    app.canvas.list = Some(incremental.unwrap_or_else(|| aad_render::build(&st.doc, &space, &opts)));
    app.canvas.key = key;
    app.canvas.list_doc = Some(st.doc.clone());
    app.canvas.list_gen = app.canvas.list_gen.wrapping_add(1);
    app.canvas.build_ms = crate::now_ms() - t0;
}

/// Full-viewport axes through the shared site / world base (default `(0, 0)`).
fn draw_axes(p: &egui::Painter, xf: &Xf, site: Vec2) {
    let t = Tokens::get();
    let o = xf.to_screen(site);
    if xf.rect.y_range().contains(o.y) {
        p.line_segment([pos2(xf.rect.left(), o.y), pos2(xf.rect.right(), o.y)], Stroke::new(1.0, t.axis_x));
    }
    if xf.rect.x_range().contains(o.x) {
        p.line_segment([pos2(o.x, xf.rect.top()), pos2(o.x, xf.rect.bottom())], Stroke::new(1.0, t.axis_y));
    }
}

fn clip_infinite(xf: &Xf, base: Vec2, dir: Vec2, ray: bool) -> Option<[Pos2; 2]> {
    let wb = xf.world_bounds();
    let far = (wb.width() + wb.height()) * 2.0 + base.dist(wb.center());
    let a = if ray { base } else { base - dir * far };
    let b = base + dir * far;
    Some([xf.to_screen(a), xf.to_screen(b)])
}

/// Draw a display list. `highlight`: draw these handles brighter (hover) or dashed (selected).
fn draw_list(p: &egui::Painter, xf: &Xf, list: &DisplayList, bg: Rgb, lwdisplay: bool) {
    let vis = xf.world_bounds();
    let mut shapes: Vec<Shape> = Vec::with_capacity(list.prims.len());
    let px = 1.0 / xf.scale;
    for prim in &list.prims {
        let col = color32(prim.display_rgb(bg));
        let pts = list.points(prim);
        match prim.kind {
            Kind::Polyline => {
                let bb = Bounds2::from_points(pts.iter().copied());
                if !bb.intersects(&vis) {
                    continue;
                }
                let w = if lwdisplay && prim.lw > 0.0 { (prim.lw * 3.78).clamp(1.0, 12.0) } else { 1.0 };
                // Skip sub-pixel objects except as a dot.
                if bb.width() < px && bb.height() < px {
                    shapes.push(Shape::rect_filled(Rect::from_center_size(xf.to_screen(bb.center()), vec2(1.0, 1.0)), 0.0, col));
                    continue;
                }
                let screen: Vec<Pos2> = pts.iter().map(|q| xf.to_screen(*q)).collect();
                if screen.len() == 2 {
                    if let [a, b] = screen.as_slice() {
                        shapes.push(Shape::line_segment([*a, *b], Stroke::new(w, col)));
                    }
                } else {
                    shapes.push(Shape::line(screen, Stroke::new(w, col)));
                }
            }
            Kind::Arc { sweep } => {
                let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) else { continue };
                let radius = center.dist(*rim);
                let start = center.angle_to(*rim);
                let mut tess = Vec::new();
                aad_geom::Arc::new(*center, radius, start, start + aad_render::sweep_radians(sweep)).tessellate(px.max(1e-9), &mut tess);
                let bb = Bounds2::from_points(tess.iter().copied());
                if !bb.intersects(&vis) {
                    continue;
                }
                let w = if lwdisplay && prim.lw > 0.0 { (prim.lw * 3.78).clamp(1.0, 12.0) } else { 1.0 };
                let screen: Vec<Pos2> = tess.iter().map(|q| xf.to_screen(*q)).collect();
                if screen.len() >= 2 {
                    shapes.push(Shape::line(screen, Stroke::new(w, col)));
                }
            }
            Kind::Ellipse { start, sweep } => {
                let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) else { continue };
                let u = *u_tip - *center;
                let v = *v_tip - *center;
                let mut tess = Vec::new();
                let sw = aad_render::sweep_radians(sweep);
                let a0 = f64::from(start);
                let n = 32usize.max(((sw.abs() / px.max(1e-9)).ceil() as usize).min(512));
                for i in 0..=n {
                    let t = a0 + sw * i as f64 / n as f64;
                    tess.push(*center + u * t.cos() + v * t.sin());
                }
                let bb = Bounds2::from_points(tess.iter().copied());
                if !bb.intersects(&vis) {
                    continue;
                }
                let w = if lwdisplay && prim.lw > 0.0 { (prim.lw * 3.78).clamp(1.0, 12.0) } else { 1.0 };
                let screen: Vec<Pos2> = tess.iter().map(|q| xf.to_screen(*q)).collect();
                if screen.len() >= 2 {
                    shapes.push(Shape::line(screen, Stroke::new(w, col)));
                }
            }
            Kind::Tris | Kind::Mask => {
                // A wipeout's mask hides what was drawn before it with the background colour.
                let col = if prim.kind == Kind::Mask { color32(bg) } else { col };
                let mut mesh = egui::Mesh::default();
                for tri in pts.chunks(3) {
                    if let [a, b, c] = tri {
                        let i = mesh.vertices.len() as u32;
                        for q in [a, b, c] {
                            mesh.colored_vertex(xf.to_screen(*q), col);
                        }
                        mesh.add_triangle(i, i + 1, i + 2);
                    }
                }
                if !mesh.vertices.is_empty() {
                    shapes.push(Shape::mesh(mesh));
                }
            }
            Kind::Point => {
                if let Some(q) = pts.first()
                    && vis.contains(*q)
                {
                    shapes.push(Shape::rect_filled(Rect::from_center_size(xf.to_screen(*q), vec2(1.5, 1.5)), 0.0, col));
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(b), Some(d)) = (pts.first(), pts.get(1))
                    && let Some(seg) = clip_infinite(xf, *b, *d, ray)
                {
                    shapes.push(Shape::line_segment(seg, Stroke::new(1.0, col)));
                }
            }
        }
    }
    p.extend(shapes);
}

/// Draw the display list through the GPU paint callback (uploading it when it changed), plus
/// the infinite lines, which are clipped to the view on the CPU every frame.
fn draw_list_gpu(c: &mut CanvasState, p: &egui::Painter, xf: &Xf, bg: Rgb, lwdisplay: bool, true_scale: bool) {
    let Some(list) = &c.list else { return };
    let view_height = f64::from(xf.rect.height()) / xf.scale.max(1e-300);
    let origin = crate::gpu::choose_origin(&list.bounds, xf.center, view_height);
    let lw_mode =
        lwdisplay.then_some(if true_scale { aad_render::mesh::LineweightMode::TrueScale } else { aad_render::mesh::LineweightMode::Screen });
    let tint = u64::from(bg.0) << 16 | u64::from(bg.1) << 8 | u64::from(bg.2);
    let mesh_basis = origin.x.to_bits()
        ^ origin.y.to_bits().rotate_left(1)
        ^ tint.rotate_left(21)
        ^ u64::from(lwdisplay).rotate_left(40)
        ^ u64::from(true_scale).rotate_left(48);
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (c.key, mesh_basis).hash(&mut h);
        h.finish()
    };
    if c.mesh_key != Some(key) {
        let t0 = crate::now_ms();
        let edge = aad_render::mesh::mesh_tile_edge(view_height);
        let mesh = if aad_render::mesh::mesh_tiling_viable(list, edge) {
            if c.mesh_basis != Some(mesh_basis) {
                c.mesh_tiles.clear();
            }
            let parts = aad_render::mesh::partition_prims(list, edge);
            let keep: std::collections::HashSet<TileId> = parts.iter().map(|(id, _)| *id).collect();
            c.mesh_tiles.retain(|id, _| keep.contains(id));
            for (id, indices) in &parts {
                let content = aad_render::mesh::tile_content_fingerprint(list, indices);
                let tile_key = content ^ mesh_basis;
                let needs = c.mesh_tiles.get(id).is_none_or(|(k, _)| *k != tile_key);
                if needs {
                    let tile_mesh = aad_render::mesh::build_mesh_prims(list, indices, origin, bg, lw_mode);
                    c.mesh_tiles.insert(*id, (tile_key, tile_mesh));
                }
            }
            let refs: Vec<&MeshData> = parts.iter().filter_map(|(id, _)| c.mesh_tiles.get(id).map(|(_, m)| m)).collect();
            let mut merged = aad_render::mesh::merge_meshes(&refs);
            if true_scale {
                aad_render::mesh::expand_hairlines_true_scale(&mut merged);
            } else {
                crate::gpu::expand_hairlines(&mut merged);
            }
            c.mesh_basis = Some(mesh_basis);
            merged
        } else {
            c.mesh_tiles.clear();
            c.mesh_basis = Some(mesh_basis);
            let mut mono = aad_render::mesh::build_mesh_mode(list, origin, bg, lw_mode);
            if true_scale {
                aad_render::mesh::expand_hairlines_true_scale(&mut mono);
            } else {
                crate::gpu::expand_hairlines(&mut mono);
            }
            mono
        };
        c.mesh_rings = mesh.ring_instances();
        c.mesh_bytes = mesh.lines.len() + mesh.tris.len() + mesh.quads.len() + mesh.rings.len();
        if let Ok(mut slot) = c.mesh_slot.lock() {
            *slot = Some(mesh);
        }
        c.infinite = list.prims.iter().enumerate().filter(|(_, pr)| matches!(pr.kind, Kind::Infinite { .. })).map(|(i, _)| i).collect();
        c.mesh_key = Some(key);
        c.mesh_origin = origin;
        c.mesh_ms = crate::now_ms() - t0;
    }
    let center = xf.rect.center();
    p.add(egui_wgpu::Callback::new_paint_callback(
        xf.rect,
        crate::gpu::CanvasCallback {
            key,
            slot: c.mesh_slot.clone(),
            offset: [(c.mesh_origin.x - xf.center.x) as f32, (c.mesh_origin.y - xf.center.y) as f32],
            scale: xf.scale as f32,
            center: [center.x, center.y],
            true_scale,
        },
    ));
    let mut shapes = Vec::new();
    for prim in c.infinite.iter().filter_map(|i| list.prims.get(*i)) {
        let pts = list.points(prim);
        if let (Kind::Infinite { ray }, Some(b), Some(d)) = (prim.kind, pts.first(), pts.get(1))
            && let Some(seg) = clip_infinite(xf, *b, *d, ray)
        {
            shapes.push(Shape::line_segment(seg, Stroke::new(1.0, color32(prim.display_rgb(bg)))));
        }
    }
    p.extend(shapes);
}

/// Draw selected/hovered entities as highlight overlays: `prims` are indices into `list` (from
/// [`PrimIndex::prims`] or [`prims_of`]).
fn draw_highlight(p: &egui::Painter, xf: &Xf, list: &DisplayList, prims: &[usize], color: Color32, width: f32, dashed: bool) {
    let mut shapes = Vec::new();
    for prim in prims.iter().filter_map(|&i| list.prims.get(i)) {
        let pts = list.points(prim);
        match prim.kind {
            Kind::Polyline => {
                let screen: Vec<Pos2> = pts.iter().map(|q| xf.to_screen(*q)).collect();
                if dashed {
                    shapes.extend(Shape::dashed_line(&screen, Stroke::new(width, color), 6.0, 4.0));
                } else {
                    shapes.push(Shape::line(screen, Stroke::new(width, color)));
                }
            }
            Kind::Arc { sweep } => {
                let (Some(center), Some(rim)) = (pts.first(), pts.get(1)) else { continue };
                let radius = center.dist(*rim);
                let start = center.angle_to(*rim);
                let mut tess = Vec::new();
                aad_geom::Arc::new(*center, radius, start, start + aad_render::sweep_radians(sweep)).tessellate((radius * 1e-3).max(1e-6), &mut tess);
                let screen: Vec<Pos2> = tess.iter().map(|q| xf.to_screen(*q)).collect();
                if dashed {
                    shapes.extend(Shape::dashed_line(&screen, Stroke::new(width, color), 6.0, 4.0));
                } else if screen.len() >= 2 {
                    shapes.push(Shape::line(screen, Stroke::new(width, color)));
                }
            }
            Kind::Ellipse { start, sweep } => {
                let (Some(center), Some(u_tip), Some(v_tip)) = (pts.first(), pts.get(1), pts.get(2)) else { continue };
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
                let screen: Vec<Pos2> = tess.iter().map(|q| xf.to_screen(*q)).collect();
                if dashed {
                    shapes.extend(Shape::dashed_line(&screen, Stroke::new(width, color), 6.0, 4.0));
                } else if screen.len() >= 2 {
                    shapes.push(Shape::line(screen, Stroke::new(width, color)));
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(b), Some(d)) = (pts.first(), pts.get(1))
                    && let Some(seg) = clip_infinite(xf, *b, *d, ray)
                {
                    if dashed {
                        shapes.extend(Shape::dashed_line(&seg, Stroke::new(width, color), 6.0, 4.0));
                    } else {
                        shapes.push(Shape::line_segment(seg, Stroke::new(width, color)));
                    }
                }
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    shapes.push(Shape::circle_stroke(xf.to_screen(*q), 3.0, Stroke::new(1.0, color)));
                }
            }
            Kind::Tris | Kind::Mask => {
                // Outline triangles' bounding region lightly.
                let bb = Bounds2::from_points(pts.iter().copied());
                let r = Rect::from_two_pos(xf.to_screen(bb.min), xf.to_screen(bb.max));
                shapes.push(Shape::rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle));
            }
        }
    }
    p.extend(shapes);
}

fn draw_snap_marker(p: &egui::Painter, at: Pos2, hit: &SnapHit) {
    let t = Tokens::get();
    let s = 5.5;
    let st = Stroke::new(2.0, t.snap);
    use snap::mode;
    match hit.mode {
        mode::END => {
            p.rect_stroke(Rect::from_center_size(at, vec2(2.0 * s, 2.0 * s)), 0.0, st, egui::StrokeKind::Middle);
        }
        mode::MID => {
            p.add(Shape::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, s * 0.8), at + vec2(-s, s * 0.8)], st));
        }
        mode::CEN | mode::GCEN => {
            p.circle_stroke(at, s, st);
        }
        mode::QUA => {
            p.add(Shape::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, 0.0), at + vec2(0.0, s), at + vec2(-s, 0.0)], st));
        }
        mode::INT | mode::APP => {
            p.line_segment([at + vec2(-s, -s), at + vec2(s, s)], st);
            p.line_segment([at + vec2(s, -s), at + vec2(-s, s)], st);
            if hit.mode == mode::APP {
                p.rect_stroke(Rect::from_center_size(at, vec2(2.0 * s, 2.0 * s)), 0.0, st, egui::StrokeKind::Middle);
            }
        }
        mode::PER => {
            p.line(vec![at + vec2(-s, -s), at + vec2(-s, s), at + vec2(s, s)], st);
            p.line(vec![at + vec2(-s, 0.0), at + vec2(0.0, 0.0), at + vec2(0.0, s)], st);
        }
        mode::TAN => {
            p.circle_stroke(at, s * 0.8, st);
            p.line_segment([at + vec2(-s, -s), at + vec2(s, -s)], st);
        }
        mode::NOD => {
            p.circle_stroke(at, s, st);
            p.line_segment([at + vec2(-s, -s), at + vec2(s, s)], st);
            p.line_segment([at + vec2(s, -s), at + vec2(-s, s)], st);
        }
        mode::INS => {
            p.line(
                vec![
                    at + vec2(-s, -s),
                    at + vec2(0.0, -s),
                    at + vec2(0.0, 0.0),
                    at + vec2(s, 0.0),
                    at + vec2(s, s),
                    at + vec2(-s, s),
                    at + vec2(-s, -s),
                ],
                st,
            );
        }
        _ => {
            p.line(vec![at + vec2(-s, -s), at + vec2(s, -s), at + vec2(-s, s), at + vec2(s, s), at + vec2(-s, -s)], st);
        }
    }
    p.text(at + vec2(12.0, 12.0), egui::Align2::LEFT_TOP, crate::i18n::t(hit.name), crate::theme::small(), Color32::BLACK);
    let galley = p.layout_no_wrap(crate::i18n::t(hit.name).to_string(), crate::theme::small(), Color32::BLACK);
    let r = Rect::from_min_size(at + vec2(10.0, 10.0), galley.size() + vec2(6.0, 2.0));
    p.rect_filled(r, 2.0, Color32::from_rgb(0xff, 0xff, 0xe1));
    p.galley(r.min + vec2(3.0, 1.0), galley, Color32::BLACK);
}

/// Acquired tracking points (small `+`, `//` for a Parallel line), the dotted paths the cursor
/// is on and the tooltip naming them ("Endpoint: 5.0000 < 90°").
fn draw_tracking(p: &egui::Painter, xf: &Xf, acquired: &[snap::tracking::Acquired], track: Option<&snap::tracking::Track>, far: f64) {
    use snap::tracking::{Acquired, PathShape};
    let t = Tokens::get();
    let st = Stroke::new(1.0, t.snap);
    for a in acquired {
        let c = xf.to_screen(a.at());
        if let Acquired::Parallel { .. } = a {
            p.line_segment([c + vec2(-5.0, 3.0), c + vec2(-1.0, -5.0)], st);
            p.line_segment([c + vec2(1.0, 3.0), c + vec2(5.0, -5.0)], st);
        } else {
            p.line_segment([c + vec2(-4.0, 0.0), c + vec2(4.0, 0.0)], st);
            p.line_segment([c + vec2(0.0, -4.0), c + vec2(0.0, 4.0)], st);
        }
    }
    let Some(tr) = track else { return };
    let mut tips = Vec::new();
    for path in &tr.paths {
        // Around the cursor only (a path's origin may be far off screen).
        let pts: Vec<Pos2> = match path.shape {
            PathShape::Ray { origin, dir } => {
                let back = (tr.point - origin).dot(dir).clamp(0.0, far);
                vec![xf.to_screen(tr.point - dir * back), xf.to_screen(tr.point + dir * far)]
            }
            PathShape::Line { dir, .. } => vec![xf.to_screen(tr.point - dir * far), xf.to_screen(tr.point + dir * far)],
            PathShape::Circle { center, radius, .. } => {
                let (a0, span) = (center.angle_to(tr.point), (far / radius).min(std::f64::consts::PI));
                (0..=96).map(|i| xf.to_screen(Vec2::polar(center, radius, a0 - span + span * f64::from(i) / 48.0))).collect()
            }
        };
        p.extend(Shape::dotted_line(&pts, t.snap, 6.0, 1.0));
        let (distance, angle) = (
            aad_engine::units::format_distance(path.from.dist(tr.point), 2, 4),
            (path.from.angle_to(tr.point).to_degrees().round() as i64).rem_euclid(360),
        );
        tips.push(if path.name == "Polar" {
            crate::tf!("Polar: {distance} < {angle}°", distance = distance, angle = angle)
        } else {
            crate::tf!("{name}: {distance} < {angle}°", name = crate::i18n::t(path.name), distance = distance, angle = angle)
        });
    }
    if let Some(o) = tr.object {
        tips.push(crate::i18n::t(o).to_string());
    }
    let at = xf.to_screen(tr.point);
    if tr.paths.len() > 1 || tr.object.is_some() {
        p.line_segment([at + vec2(-4.0, -4.0), at + vec2(4.0, 4.0)], st);
        p.line_segment([at + vec2(4.0, -4.0), at + vec2(-4.0, 4.0)], st);
    }
    tooltip(p, at + vec2(16.0, 18.0), &tips.join(", "));
}

/// The rubber-band and snap-adjusted cursor point for the current prompt (the engine's
/// `Session::snap_cursor`: object snaps, acquiring tracking points, grid, tracking paths, ortho).
fn effective_point(app: &mut CadApp, raw: Vec2, xf: &Xf, ctx: &egui::Context) -> Vec2 {
    let prompt = app.session.current_prompt();
    let hot = app.canvas.hot_grip;
    let wants_point = hot.is_some() || prompt.as_ref().is_some_and(|p| p.accept.point && !p.accept.select);
    let base = hot.map(|g| g.base).or_else(|| prompt.as_ref().and_then(|p| p.base));
    let deferred = hot.is_none() && prompt.as_ref().is_some_and(|p| p.deferred);
    app.canvas.snap = None;
    app.canvas.track = None;
    if !wants_point || app.session.pending_window.is_some() {
        if !wants_point {
            app.session.tracking.clear();
        }
        return raw;
    }
    let (now, shift) = ctx.input(|i| (i.time, i.modifiers.shift));
    let q = snap::tracking::CursorQuery { base, deferred, aperture: app.session.settings.aperture / xf.scale, tol: 6.0 / xf.scale, now, shift };
    let r = app.session.snap_cursor(raw, &q);
    if r.wait {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(snap::tracking::DWELL / 4.0));
    }
    app.canvas.snap = r.snap;
    app.canvas.track = r.track;
    r.point
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let rect = ui.available_rect_before_wrap();
    let resp = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    app.canvas.rect = Some(rect);
    app.session.viewport_px = (f64::from(rect.width()).max(1.0), f64::from(rect.height()).max(1.0));
    if app.canvas.zoom_pending {
        app.canvas.zoom_pending = false;
        let _ = app.session.zoom_extents();
    }
    let Ok(st) = app.session.state() else { return };
    let view = st.view();
    let scale = f64::from(rect.height()) / view.height.max(1e-12);
    let xf = Xf { rect, center: view.center, scale };
    app.canvas.xf = Some(xf);

    // ---------- input ----------
    let (hover_pos, scroll, mods, middle_down, pressed_primary, pressed_secondary, dbl_middle, dbl_primary) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.smooth_scroll_delta,
            i.modifiers,
            i.pointer.middle_down(),
            i.pointer.primary_clicked(),
            i.pointer.secondary_clicked(),
            i.pointer.button_double_clicked(egui::PointerButton::Middle),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
        )
    });
    let inside = hover_pos.is_some_and(|p| rect.contains(p)) && resp.hovered();
    // Which device is scrolling? On macOS, trackpads (and Magic Mouse) report precise point
    // deltas; notched mouse wheels report lines. The answer is sticky because egui spreads a wheel
    // notch over several frames. Elsewhere (browsers, Wayland smooth-scroll mice) ordinary wheels
    // also report points, so the wheel keeps zooming there.
    ui.input(|i| {
        for e in &i.raw.events {
            if let egui::Event::MouseWheel { unit, .. } = e {
                app.canvas.scroll_pans = cfg!(target_os = "macos") && *unit == egui::MouseWheelUnit::Point;
            }
        }
    });
    if inside && scroll != egui::Vec2::ZERO {
        if app.canvas.scroll_pans {
            // Two-finger swipe pans: the drawing follows the fingers, like a middle-drag.
            // Pinch (and Cmd+scroll) still zoom via `zoom_delta` below.
            if let Ok(st) = app.session.state_mut() {
                let v = st.view();
                st.set_view_quiet(aad_engine::View {
                    center: v.center - Vec2::new(f64::from(scroll.x) / scale, -f64::from(scroll.y) / scale),
                    height: v.height,
                });
            }
        } else if scroll.y.abs() > 0.0
            && let Some(hp) = hover_pos
        {
            // Zoom with the mouse wheel about the cursor (~1.22× per 120-unit notch).
            let f = (f64::from(scroll.y) / 600.0).exp();
            let about = xf.to_world(hp);
            let _ = app.session.zoom_about(f, about);
        }
    }
    let zoom_pinch = ui.input(|i| i.zoom_delta());
    if inside
        && (zoom_pinch - 1.0).abs() > 1e-4
        && let Some(hp) = hover_pos
    {
        let _ = app.session.zoom_about(f64::from(zoom_pinch), xf.to_world(hp));
    }
    // Pan with the middle button (or Shift+right-drag / two-finger drag on trackpads is scroll).
    let middle_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle));
    let focused = ui.input(|i| i.focused);
    if middle_down
        && focused
        && (app.canvas.pan_last.is_some() || (middle_pressed && inside))
        && let Some(hp) = hover_pos
    {
        if let Some(last) = app.canvas.pan_last {
            let d = hp - last;
            if let Ok(st) = app.session.state_mut() {
                let v = st.view();
                st.set_view_quiet(aad_engine::View {
                    center: v.center - Vec2::new(f64::from(d.x) / scale, -f64::from(d.y) / scale),
                    height: v.height,
                });
            }
        }
        app.canvas.pan_last = Some(hp);
    } else {
        app.canvas.pan_last = None;
    }
    if dbl_middle && inside {
        let _ = app.session.zoom_extents();
    }
    // Re-read the view after navigation.
    let view = app.session.state().map(|s| s.view()).unwrap_or(view);
    let scale = f64::from(rect.height()) / view.height.max(1e-12);
    let xf = Xf { rect, center: view.center, scale };
    app.canvas.xf = Some(xf);
    // The paper (display) transform; equal to `xf` except inside an MSPACE viewport.
    let (xf_paper, active_vp) = match app.session.state() {
        Ok(st) => {
            let pv = st.paper_view();
            let xp = Xf { rect, center: pv.center, scale: f64::from(rect.height()) / pv.height.max(1e-12) };
            (if st.mspace.is_some() { xp } else { xf }, st.active_viewport().map(|(_, v)| v))
        }
        Err(_) => (xf, None),
    };
    let raw_world = hover_pos.filter(|_| inside).map(|p| xf.to_world(p));
    let dyn_frame = crate::cmdline::dyn_frame(app);
    if let Some(w) = raw_world {
        let mut eff = effective_point(app, w, &xf, ui.ctx());
        // A locked Dynamic Input value (`40,` or `40<` typed) holds the cursor and overrides snaps.
        if let Some(f) = dyn_frame
            && let Some(e) = crate::dyninput::parse(&app.cmd.buffer, &f)
            && e.first_locked().is_some()
        {
            eff = crate::dyninput::constrain(&e, &f, eff);
            app.canvas.snap = None;
            app.canvas.track = None;
        }
        app.canvas.cursor = Some(eff);
        app.session.cursor = eff;
        app.session.cursor_deferred = app.canvas.snap.and_then(|h| h.deferred);
        // Typed or menu snap overrides (END, NON…) and Tab cycling; a locked Dynamic Input value wins.
        if dyn_frame.and_then(|f| crate::dyninput::parse(&app.cmd.buffer, &f)).is_none_or(|e| e.first_locked().is_none()) {
            override_cursor(app, w, &xf);
        }
    } else {
        app.canvas.cursor = None;
        app.canvas.snap = None;
        app.session.cursor_deferred = None;
    }

    // Clicks.
    if inside
        && pressed_primary
        && !middle_down
        && let Some(p) = app.canvas.cursor
    {
        if app.session.running.is_some() {
            app.canvas.hot_grip = None;
            // Clicking places the point; a value typed into the Dynamic Input boxes is used up.
            if let Some(f) = dyn_frame
                && crate::dyninput::parse(&app.cmd.buffer, &f).is_some()
            {
                app.cmd.buffer.clear();
            }
            // A deferred tangent/perpendicular goes to the command as such.
            let input = app.canvas.snap.filter(|h| h.deferred.is_some()).map_or(Input::Point(p), |h| h.input());
            if let Err(e) = app.session.input(input) {
                app.session.echo(e.to_string());
            }
        } else if app.canvas.hot_grip.is_some() {
            apply_hot_grip(app, p);
        } else if let Some(g) = grip_at(app, &xf, hover_pos) {
            app.canvas.hot_grip = Some(g);
            app.session.echo(g.label());
        } else if let Err(e) = app.session.idle_click(raw_world.unwrap_or(p), mods.shift) {
            app.session.echo(e.to_string());
        }
    }
    // Double-click on text starts TEXTEDIT on it (the first click of the pair selected it).
    if inside
        && dbl_primary
        && app.session.running.is_none()
        && let Some(w) = raw_world
    {
        edit_selected_text_at(app, w);
    }
    // Press-and-drag selection window (AutoCAD's PICKDRAG = 2; click-click still works): a drag
    // that starts while objects are being selected opens a window where the button went down,
    // and letting go closes it. Drags on grips and drags while drawing are left alone.
    if resp.drag_started_by(egui::PointerButton::Primary)
        && !middle_down
        && app.canvas.hot_grip.is_none()
        && app.session.pending_window.is_none()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && rect.contains(origin)
        && grip_at(app, &xf, Some(origin)).is_none()
    {
        app.canvas.drag_window = app.session.begin_window(xf.to_world(origin));
    }
    if app.canvas.drag_window && resp.drag_stopped_by(egui::PointerButton::Primary) {
        app.canvas.drag_window = false;
        if app.session.pending_window.is_some()
            && let Some(end) = ui.input(|i| i.pointer.interact_pos())
        {
            let end = xf.to_world(end);
            let done = if app.session.running.is_some() { app.session.input(Input::Point(end)) } else { app.session.idle_click(end, mods.shift) };
            if let Err(e) = done {
                app.session.echo(e.to_string());
            }
        }
    }
    // Right-click acts as Enter while a command runs (the classic CAD default); otherwise it opens
    // the shortcut menu (Repeat, Clipboard, …). Shift/Ctrl+right-click opens the object snap menu.
    let idle = app.session.running.is_none();
    if inside && pressed_secondary {
        app.canvas.snap_menu = mods.shift || mods.ctrl;
    }
    if inside && pressed_secondary && !idle && !app.canvas.snap_menu {
        let _ = app.session.input(Input::Enter);
    }
    if idle || app.canvas.snap_menu {
        crate::context_menu::show(app, &resp);
    }

    // Hover highlight (throttled to cursor movement).
    // The cursor picks objects (pick box, rollover highlight): "Select objects" and single-object
    // prompts, or no command with PICKFIRST on.
    let selecting = app.session.picking_objects();
    if inside && selecting && app.session.pending_window.is_none() {
        if hover_pos != app.canvas.hover_at
            && let (Some(w), Ok(st)) = (raw_world, app.session.state())
        {
            app.canvas.hover = aad_engine::select::pick(&st.doc, &st.edit_space(), w, app.session.settings.pickbox.max(1.0) * 1.5 / scale);
            app.canvas.hover_at = hover_pos;
        }
    } else {
        app.canvas.hover = None;
        app.canvas.hover_at = None;
    }

    // ---------- paint ----------
    painter.rect_filled(rect, 0.0, t.canvas);
    let t0 = crate::now_ms();
    ensure_list(app, &painter.ctx().clone(), 1.0 / xf_paper.scale);
    let sheet = app.canvas.list.as_ref().and_then(|l| l.sheet);
    let bg = match sheet {
        Some(sh) => {
            // Paper space: grey surround, the sheet with a shadow, the printable area dashed.
            painter.rect_filled(rect, 0.0, Color32::from_rgb(0x50, 0x57, 0x63));
            let a = xf_paper.to_screen(Vec2::new(0.0, sh.size.y));
            let b = xf_paper.to_screen(Vec2::new(sh.size.x, 0.0));
            let paper = Rect::from_two_pos(a, b);
            painter.rect_filled(paper.translate(vec2(5.0, 5.0)), 0.0, Color32::from_black_alpha(110));
            painter.rect_filled(paper, 0.0, Color32::WHITE);
            let pa = Rect::from_two_pos(
                xf_paper.to_screen(Vec2::new(sh.printable.min.x, sh.printable.max.y)),
                xf_paper.to_screen(Vec2::new(sh.printable.max.x, sh.printable.min.y)),
            );
            let pts = [pa.left_top(), pa.right_top(), pa.right_bottom(), pa.left_bottom(), pa.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, Color32::from_gray(150)), 4.0, 4.0));
            Rgb(255, 255, 255)
        }
        None => {
            let site = app.session.doc().map(|d| d.site_origin()).unwrap_or(Vec2::ZERO);
            draw_axes(&painter, &xf, site);
            Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b())
        }
    };
    let sel = app.session.selection();
    let true_scale = sheet.is_some() && app.session.settings.lwdisplay;
    if app.canvas.gpu.is_some() {
        draw_list_gpu(&mut app.canvas, &painter, &xf_paper, bg, app.session.settings.lwdisplay, true_scale);
    } else if let Some(list) = &app.canvas.list {
        draw_list(&painter, &xf_paper, list, bg, app.session.settings.lwdisplay);
    }
    if let Some(vp) = &active_vp {
        // The active viewport: a heavy border, highlights in model units clipped to it.
        let r = Rect::from_two_pos(
            xf_paper.to_screen(Vec2::new(vp.center.x - vp.width / 2.0, vp.center.y + vp.height / 2.0)),
            xf_paper.to_screen(Vec2::new(vp.center.x + vp.width / 2.0, vp.center.y - vp.height / 2.0)),
        );
        painter.rect_stroke(r, 0.0, Stroke::new(3.0, Color32::from_rgb(0x2a, 0x2a, 0x2a)), egui::StrokeKind::Middle);
        if let Ok(st) = app.session.state() {
            let mut hs = sel.clone();
            hs.extend(app.canvas.hover.filter(|h| !sel.contains(h)));
            let ents: Vec<_> = hs.iter().filter_map(|h| st.doc.model.get(*h)).map(|e| e.as_ref()).collect();
            if !ents.is_empty() {
                let list = aad_render::build_entities(&st.doc, ents, &aad_render::Options { tolerance: 0.2 / scale, ..Default::default() });
                let clipped = painter.with_clip_rect(r.intersect(rect));
                if let Some(h) = app.canvas.hover
                    && !sel.contains(&h)
                {
                    draw_highlight(&clipped, &xf, &list, &prims_of(&list, &[h]), t.hover, 2.0, false);
                }
                draw_highlight(&clipped, &xf, &list, &prims_of(&list, &sel), t.selection, 1.5, true);
            }
        }
    } else if let CanvasState { list: Some(list), list_gen, prim_index, hover, .. } = &mut app.canvas {
        if let Some(h) = *hover
            && !sel.contains(&h)
        {
            draw_highlight(&painter, &xf, list, &prim_index.prims(*list_gen, list, &[h]), t.hover, 2.0, false);
        }
        draw_highlight(&painter, &xf, list, &prim_index.prims(*list_gen, list, &sel), t.selection, 1.5, true);
    }
    app.canvas.draw_ms = crate::now_ms() - t0;
    // Grips on selected objects (when idle).
    if app.session.running.is_none()
        && let Ok(st) = app.session.state()
    {
        let gs = app.session.settings.gripsize as f32;
        for h in sel.iter().take(200) {
            if let Some(e) = st.doc.entity(*h) {
                for (gi, g) in aad_engine::grips::grips(&st.doc, e).into_iter().enumerate() {
                    let sp = xf.to_screen(g);
                    if rect.contains(sp) {
                        let r = Rect::from_center_size(sp, vec2(gs * 2.0, gs * 2.0));
                        let hot = app.canvas.hot_grip.is_some_and(|hg| hg.handle == *h && hg.index == gi);
                        let warm = !hot && hover_pos.is_some_and(|hp| r.expand(2.0).contains(hp));
                        painter.rect_filled(
                            r,
                            0.0,
                            if hot {
                                t.grip_hot
                            } else if warm {
                                Color32::from_rgb(0xff, 0x7f, 0x9f)
                            } else {
                                t.grip
                            },
                        );
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80)), egui::StrokeKind::Middle);
                    }
                }
            }
        }
    }
    // Hot grip preview.
    if let (Some(g), Some(c), Ok(st)) = (app.canvas.hot_grip, app.canvas.cursor, app.session.state())
        && let Some(e) = st.doc.entity(g.handle)
    {
        let k = match aad_engine::grips::mode_matrix(g.mode, g.base, c) {
            Some(m) => {
                let mut k = e.kind.clone();
                k.transform(&m);
                Some(k)
            }
            None if g.mode == aad_engine::grips::GripMode::Stretch => aad_engine::grips::stretch_grip(&e.kind, g.index, c),
            None => None,
        };
        if let Some(k) = k {
            let ent = aad_doc::Entity { handle: Handle(0), common: e.common.clone(), kind: k };
            let list = aad_render::build_entities(
                &st.doc,
                std::iter::once(&ent),
                &aad_render::Options { tolerance: 0.2 / scale, min_dash: 2.0 / scale, ..Default::default() },
            );
            draw_list(&painter, &xf, &list, bg, false);
        }
        painter.extend(Shape::dashed_line(&[xf.to_screen(g.base), xf.to_screen(c)], Stroke::new(0.5, Color32::from_rgb(0x00, 0xe6, 0xff)), 4.0, 3.0));
    }
    // Rubber band preview of the active command (cyan highlight, thinner than committed geometry).
    if let (Some(c), true) = (app.canvas.cursor, app.session.running.is_some()) {
        let ents = app.session.preview(c);
        if !ents.is_empty()
            && let Ok(st) = app.session.state()
        {
            let mut list = aad_render::build_entities(
                &st.doc,
                ents.iter(),
                &aad_render::Options { tolerance: 0.2 / scale, min_dash: 2.0 / scale, ..Default::default() },
            );
            for prim in &mut list.prims {
                prim.color = aad_color::Rgb(0x00, 0xe6, 0xff);
                prim.aci7 = false;
            }
            draw_list(&painter, &xf, &list, bg, false);
        }
    }
    // Pending selection window.
    if let (Some(pw), Some(c)) = (app.session.pending_window, raw_world) {
        let a = xf.to_screen(pw.corner);
        let b = xf.to_screen(c);
        let r = Rect::from_two_pos(a, b);
        let crossing = c.x < pw.corner.x;
        painter.rect_filled(r, 0.0, if crossing { t.crossing_fill } else { t.window_fill });
        let st = Stroke::new(1.0, if crossing { t.crossing_stroke } else { t.window_stroke });
        if crossing {
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&pts, st, 5.0, 3.0));
        } else {
            painter.rect_stroke(r, 0.0, st, egui::StrokeKind::Middle);
        }
    }
    // Acquired tracking points and the polar / tracking paths under the cursor.
    if app.canvas.cursor.is_some() {
        draw_tracking(&painter, &xf, &app.session.tracking.acquired, app.canvas.track.as_ref(), view.height * 4.0);
    }
    if let (Some(hit), Some(_)) = (app.canvas.snap, app.canvas.cursor) {
        draw_snap_marker(&painter, xf.to_screen(hit.point), &hit);
    }
    // UCS icon and viewport label.
    if app.ui.show_ucs_icon {
        draw_ucs_icon(&painter, rect);
        crate::ucsicon::ucs_icon(app, ui, rect);
    }
    if app.ui.show_pomodoro {
        crate::widgets::pomodoro_overlay(app, ui, rect);
    }
    viewport_label(&painter, rect);
    // Command line overlay.
    if app.ui.show_command_line {
        crate::cmdline::show(app, ui, rect);
    }
    // Crosshair cursor.
    if inside {
        // Hide the OS cursor under the drawn crosshair, unless assistive tech needs it (#39).
        ui.ctx().set_cursor_icon(if app.ui.system_cursor { egui::CursorIcon::Crosshair } else { egui::CursorIcon::None });
        if let Some(hp) = hover_pos {
            draw_crosshair(app, &painter, rect, hp, app.session.cursor_shape());
        }
        if app.session.settings.dynmode
            && let Some(hp) = hover_pos
        {
            dynamic_input(app, &painter, hp);
        }
    }
    // No continuous repaint while hovering: input events repaint by themselves. Repainting every
    // frame kept a vsync swapchain's queue full (every frame waited behind queued ones) and would
    // spin the CPU with the low-latency present mode (`gpu::surface_config`).
}

fn tooltip(p: &egui::Painter, at: Pos2, text: &str) {
    let galley = p.layout_no_wrap(text.to_string(), crate::theme::small(), Color32::from_rgb(0x20, 0x20, 0x20));
    let r = Rect::from_min_size(at, galley.size() + vec2(8.0, 4.0));
    p.rect_filled(r, 2.0, Color32::from_rgb(0xf4, 0xf4, 0xf4));
    p.galley(r.min + vec2(4.0, 2.0), galley, Color32::BLACK);
}

/// Half the side of the pick box: the pick aperture (PICKBOX × 1.5 pixels, as `select::pick` is
/// called with), so the square shows what a click hits. `None` when PICKBOX is 0 (no box).
/// The candidate Tab has cycled to, if cycling: a Tab press starts or advances it, and the
/// cursor leaving the aperture where it started ends it.
fn snap_cycle(app: &mut CadApp, raw: Vec2, ap: f64) -> Option<usize> {
    let tab = std::mem::take(&mut app.canvas.snap_tab);
    let c = &mut app.canvas.snap_cycle;
    if c.is_some_and(|c| c.anchor.dist(raw) > ap) {
        *c = None;
    }
    if tab {
        *c = Some(c.map_or(SnapCycle { anchor: raw, index: 1 }, |c| SnapCycle { index: c.index + 1, ..c }));
    }
    c.map(|c| c.index)
}

/// The cursor under a one-pick object snap override (`Session::snap_override`: END, NON…) or
/// while Tab cycles the candidates: the snap of that mode (or the cycled candidate) replaces
/// what the running snaps found; with none, a running snap no longer holds the cursor.
fn override_cursor(app: &mut CadApp, raw: Vec2, xf: &Xf) {
    let over = app.session.snap_override();
    let ap = app.session.settings.aperture / xf.scale;
    let cycle = snap_cycle(app, raw, ap);
    let prompt = app.session.current_prompt();
    let wants_point = prompt.as_ref().is_some_and(|p| p.accept.point && !p.accept.select);
    if (over.is_none() && cycle.is_none()) || !wants_point || app.canvas.hot_grip.is_some() || app.session.pending_window.is_some() {
        return;
    }
    let base = prompt.as_ref().and_then(|p| p.base);
    let deferred = prompt.as_ref().is_some_and(|p| p.deferred);
    let s = &app.session.settings;
    let osmode = over.unwrap_or(s.osmode) | if s.osnaphatch { snap::mode::HATCH } else { 0 };
    let Ok(st) = app.session.state() else { return };
    let hit = match cycle {
        Some(i) => {
            let cands = aad_engine::pointmod::snap_candidates(&st.doc, &st.edit_space(), raw, ap, osmode, base, deferred);
            cands.get(i % cands.len().max(1)).copied()
        }
        None => snap::osnap(&st.doc, &st.edit_space(), raw, ap, osmode, base, deferred),
    };
    let p = match (hit, app.canvas.snap) {
        (Some(h), _) => {
            app.canvas.snap = Some(h);
            h.point
        }
        (None, Some(_)) => {
            app.canvas.snap = None;
            raw
        }
        (None, None) => return,
    };
    app.canvas.cursor = Some(p);
    app.session.cursor = p;
    app.session.cursor_deferred = app.canvas.snap.and_then(|h| h.deferred);
}

fn pickbox_half(pickbox: f64) -> Option<f32> {
    let half = (pickbox * 1.5) as f32;
    (half.is_finite() && half >= 1.0).then_some(half)
}

fn draw_crosshair(app: &CadApp, p: &egui::Painter, rect: Rect, at: Pos2, shape: aad_engine::CursorShape) {
    // AutoCAD: idle = crosshairs+pickbox; point prompt = crosshairs; select prompt = pickbox only.
    let st = Stroke::new(0.5, Tokens::get().crosshair);
    let pb = pickbox_half(app.session.settings.pickbox);
    if shape == aad_engine::CursorShape::Pickbox {
        if let Some(half) = pb {
            p.rect_stroke(Rect::from_center_size(at, vec2(half * 2.0, half * 2.0)), 0.0, st, egui::StrokeKind::Middle);
        }
        return;
    }
    let len = (rect.height() * app.session.settings.cursorsize as f32 / 100.0).max(12.0);
    let gap = if shape == aad_engine::CursorShape::CrosshairsAndPickbox { pb.unwrap_or(0.0) } else { 0.0 };
    p.line_segment([pos2(at.x - len, at.y), pos2(at.x - gap, at.y)], st);
    p.line_segment([pos2(at.x + gap, at.y), pos2(at.x + len, at.y)], st);
    p.line_segment([pos2(at.x, at.y - len), pos2(at.x, at.y - gap)], st);
    p.line_segment([pos2(at.x, at.y + gap), pos2(at.x, at.y + len)], st);
    if shape == aad_engine::CursorShape::CrosshairsAndPickbox
        && let Some(half) = pb
    {
        p.rect_stroke(Rect::from_center_size(at, vec2(half * 2.0, half * 2.0)), 0.0, st, egui::StrokeKind::Middle);
    }
}

/// The Dynamic Input tooltip beside the cursor, styled like the command line. At a point prompt
/// it shows the pair of value boxes, a view of the command-line text (see [`crate::dyninput`]);
/// otherwise the prompt with what is being typed, or the live distance and angle.
fn dynamic_input(app: &CadApp, p: &egui::Painter, at: Pos2) {
    let Some(prompt) = app.session.current_prompt() else { return };
    if let Some(f) = crate::cmdline::dyn_frame(app)
        && let Some(e) = crate::dyninput::parse(&app.cmd.buffer, &f)
        && let Some(c) = app.canvas.cursor
    {
        dynamic_input_boxes(p, at, crate::i18n::t(&prompt.message), &f, &e, c);
        return;
    }
    let mut text = crate::i18n::t(&prompt.message).to_string();
    if !app.cmd.buffer.is_empty() {
        text = format!("{}: {}", crate::i18n::t(&prompt.message), app.cmd.buffer);
    } else if let (Some(base), Some(c)) = (prompt.base, app.canvas.cursor) {
        text = format!(
            "{}   {}  <  {}°",
            crate::i18n::t(&prompt.message),
            aad_engine::units::format_distance(base.dist(c), 2, 4),
            (base.angle_to(c).to_degrees() * 10.0).round() / 10.0
        );
    } else if let Some(c) = app.canvas.cursor {
        text = format!(
            "{}   {}, {}",
            crate::i18n::t(&prompt.message),
            aad_engine::units::format_distance(c.x, 2, 4),
            aad_engine::units::format_distance(c.y, 2, 4)
        );
    }
    let t = Tokens::get();
    let galley = p.layout_no_wrap(text, crate::theme::small(), t.text);
    let r = tooltip_frame(p, at, galley.size() + vec2(12.0, 8.0));
    p.galley(r.min + vec2(6.0, 4.0), galley, t.text);
}

/// The tooltip background, matching the command line bar; returns its rectangle.
fn tooltip_frame(p: &egui::Painter, at: Pos2, size: egui::Vec2) -> Rect {
    let t = Tokens::get();
    let r = Rect::from_min_size(at + vec2(18.0, -size.y - 14.0), size);
    p.rect_filled(r, 3.0, t.cmd_bg);
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    r
}

/// The Dynamic Input value boxes: X and Y, or distance and angle. Typed values show in full
/// text colour, a locked value (a separator typed after it) gets a lock, the active box gets the
/// accent outline, and open boxes show the cursor's live value, dimmed. Values measured from the
/// last point are marked with `@`.
fn dynamic_input_boxes(p: &egui::Painter, at: Pos2, message: &str, f: &crate::dyninput::Frame, e: &crate::dyninput::Entry<'_>, cursor: Vec2) {
    use crate::dyninput::Format;
    let t = Tokens::get();
    let font = crate::theme::small();
    let dist = |v: f64| aad_engine::units::format_distance(v, 2, 4);
    let (live_first, live_second) = f.live(e.format, cursor);
    let live_second = match e.format {
        Format::Cartesian => dist(live_second),
        Format::Polar => format!("{}°", (live_second.to_degrees() * 10.0).round() / 10.0),
    };
    let (label_first, label_second) = match e.format {
        Format::Cartesian => ("X", "Y"),
        Format::Polar => ("", "<"),
    };
    // (label, text, typed, locked, active)
    let cells = [
        (
            label_first,
            if e.first.is_empty() { dist(live_first) } else { e.first.to_owned() },
            !e.first.is_empty(),
            e.first_locked().is_some(),
            !e.split,
        ),
        (label_second, if e.second.is_empty() { live_second } else { e.second.to_owned() }, !e.second.is_empty(), false, e.split),
    ];

    let msg = p.layout_no_wrap(message.to_owned(), font.clone(), t.text_dim);
    let marker = p.layout_no_wrap(if f.relative() { "@".to_owned() } else { String::new() }, font.clone(), t.text_faint);
    let lock_w = 10.0;
    let laid: Vec<_> = cells
        .into_iter()
        .map(|(label, text, typed, locked, active)| {
            let label = p.layout_no_wrap(label.to_owned(), font.clone(), t.text_faint);
            let value = p.layout_no_wrap(text, font.clone(), if typed { t.text } else { t.text_faint });
            let w = (value.size().x + 10.0 + if locked { lock_w } else { 0.0 }).max(48.0);
            (label, value, w, locked, active)
        })
        .collect();

    // Measure, then draw.
    let pad = 6.0;
    let gap = 6.0;
    let box_h = msg.size().y + 6.0;
    let mut w = pad + msg.size().x + gap + marker.size().x;
    for (label, _, bw, _, _) in &laid {
        w += gap + label.size().x + 3.0 + bw;
    }
    let r = tooltip_frame(p, at, vec2(w + pad, box_h + 2.0 * pad - 2.0));
    let cy = r.center().y;
    let mut x = r.left() + pad;
    let (msg_w, msg_h) = (msg.size().x, msg.size().y);
    p.galley(pos2(x, cy - msg_h / 2.0), msg, t.text_dim);
    x += msg_w + gap;
    let mw = marker.size().x;
    p.galley(pos2(x, cy - marker.size().y / 2.0), marker, t.text_faint);
    x += mw;
    for (label, value, bw, locked, active) in laid {
        x += gap;
        let lw = label.size().x;
        p.galley(pos2(x, cy - label.size().y / 2.0), label, t.text_faint);
        x += lw + 3.0;
        let b = Rect::from_min_size(pos2(x, cy - box_h / 2.0), vec2(bw, box_h));
        p.rect_filled(b, 2.0, t.chrome_dark);
        let stroke = if active { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.border) };
        p.rect_stroke(b, 2.0, stroke, egui::StrokeKind::Inside);
        let vw = value.size().x;
        p.galley(pos2(b.left() + 5.0, cy - value.size().y / 2.0), value, t.text);
        if active {
            let cx = b.left() + 6.0 + vw;
            p.line_segment([pos2(cx, b.top() + 3.0), pos2(cx, b.bottom() - 3.0)], Stroke::new(1.0, t.text));
        }
        if locked {
            draw_lock(p, pos2(b.right() - lock_w / 2.0 - 3.0, cy), t.accent);
        }
        x += bw;
    }
}

/// A small padlock, drawn in code: body plus shackle.
fn draw_lock(p: &egui::Painter, c: Pos2, color: Color32) {
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, 1.5), vec2(7.0, 5.0)), 1.0, color);
    p.add(Shape::line(
        vec![pos2(c.x - 2.0, c.y - 1.0), pos2(c.x - 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 1.0)],
        Stroke::new(1.2, color),
    ));
}

fn draw_ucs_icon(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    let o = pos2(rect.left() + 36.0, rect.bottom() - 34.0);
    let st = Stroke::new(1.0, t.canvas_ink);
    p.line_segment([o, o + vec2(60.0, 0.0)], st);
    p.line_segment([o, o + vec2(0.0, -60.0)], st);
    p.rect_stroke(Rect::from_center_size(o, vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
    let f = egui::FontId::proportional(13.0);
    p.text(o + vec2(70.0, 0.0), egui::Align2::LEFT_CENTER, crate::tl!("X"), f.clone(), t.canvas_ink);
    p.text(o + vec2(0.0, -70.0), egui::Align2::CENTER_BOTTOM, crate::tl!("Y"), f, t.canvas_ink);
}

fn viewport_label(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    let at = pos2(rect.left() + 10.0, rect.top() + 8.0);
    p.text(at, egui::Align2::LEFT_TOP, crate::tl!("+  |  Top  |  2D Wireframe"), crate::theme::small(), t.canvas_ink);
}

/// The grip of a selected object under the cursor, if any.
fn grip_at(app: &CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<HotGrip> {
    let hp = hover?;
    let st = app.session.state().ok()?;
    let gs = app.session.settings.gripsize as f32 + 3.0;
    for h in app.session.selection().iter().take(200) {
        let e = st.doc.entity(*h)?;
        if st.doc.layer(&e.common.layer).is_some_and(|l| l.locked) {
            continue;
        }
        for (i, g) in aad_engine::grips::grips(&st.doc, e).into_iter().enumerate() {
            let sp = xf.to_screen(g);
            if (sp.x - hp.x).abs() <= gs && (sp.y - hp.y).abs() <= gs {
                return Some(HotGrip { handle: *h, index: i, base: g, mode: aad_engine::grips::GripMode::Stretch });
            }
        }
    }
    None
}

/// Start TEXTEDIT on the selected object when it is a single text, mtext, attribute definition or
/// dimension, picking it at world point `w`. Returns whether the command started.
pub fn edit_selected_text_at(app: &mut CadApp, w: Vec2) -> bool {
    let editable = app.session.state().ok().and_then(|st| match st.selection.as_slice() {
        [h] => st.doc.entity(*h).map(|e| {
            matches!(
                e.kind,
                aad_doc::EntityKind::Text(_) | aad_doc::EntityKind::MText(_) | aad_doc::EntityKind::AttDef(_) | aad_doc::EntityKind::Dimension(_)
            )
        }),
        _ => None,
    });
    if editable != Some(true) {
        return false;
    }
    app.start("textedit");
    if let Err(e) = app.session.input(Input::Point(w)) {
        app.session.echo(e.to_string());
    }
    app.session.running.is_some()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use aad_render::DPrim;

    fn list(handles: &[u64]) -> DisplayList {
        let prim = |h| DPrim { handle: Handle(h), color: Rgb(255, 255, 255), aci7: true, lw: 0.0, kind: Kind::Polyline, start: 0, len: 0 };
        DisplayList { prims: handles.iter().map(|&h| prim(h)).collect(), ..Default::default() }
    }

    fn hs(v: &[u64]) -> Vec<Handle> {
        v.iter().map(|&h| Handle(h)).collect()
    }

    #[test]
    fn prim_index_matches_a_scan_and_follows_list_rebuilds() {
        let a = list(&[3, 1, 3, 2, 7, 3]);
        let mut ix = PrimIndex::default();
        assert_eq!(ix.prims(1, &a, &hs(&[3])), vec![0, 2, 5]);
        assert_eq!(ix.prims(1, &a, &hs(&[3])), prims_of(&a, &hs(&[3])));
        assert_eq!(ix.prims(1, &a, &hs(&[2, 1, 2])), vec![1, 3]);
        assert!(ix.prims(1, &a, &hs(&[9])).is_empty());
        assert!(ix.prims(1, &a, &[]).is_empty());
        // A rebuilt list (new generation) is re-indexed.
        let b = list(&[3, 9]);
        assert_eq!(ix.prims(2, &b, &hs(&[9, 3])), vec![0, 1]);
    }
}

//! Windows shell host.
//!
//! The WinUI app calls the C ABI in [`ffi`]. This crate owns the [`aad_engine::Session`], packs
//! the display list with [`aad_render::mesh`], and on Windows paints that mesh into a child
//! window. It exposes the registry menu, the command line, form descriptions and grip editing.
//!
//! `unsafe` is denied except in the ABI, the child window, and the swap-chain surface.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod assistant;
mod autosave;
mod bar;
mod chrome;
mod control;
mod ffi;
mod forms;
#[cfg(windows)]
mod gpu;
mod osas_ai;
mod overlay;
mod panels;
#[cfg(windows)]
mod surface;
#[cfg(windows)]
mod window;

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
#[cfg(windows)]
use std::sync::atomic::Ordering;

use aad_color::Rgb;
use aad_engine::Session;
use aad_engine::doc::{Drawing, Space};
use aad_geom::Vec2;
use aad_render::DisplayList;
use aad_render::mesh::{self, MeshData, TileId};
use rayon::prelude::*;
use serde_json::Value;

/// uid, revision, tessellation band, lwdisplay, space, packed LOD bits, packed model pan cell.
/// LOD bits: text | fill<<1 | text_greek<<2 | fill_tint<<3.
type ListKey = (u64, u64, i32, bool, u64, u8, i64);

fn lod_pack(l: aad_render::LodFlags) -> u8 {
    u8::from(l.text) | (u8::from(l.fill) << 1) | (u8::from(l.text_greek) << 2) | (u8::from(l.fill_tint) << 3)
}

/// Display list being rebuilt on a worker for a new zoom / LOD band or pan cell, with its scene (so edits after the swap stay on the delta
/// path) and, when the list is tiled, its mesh tiles: the swap on the UI thread then only installs them.
struct PendingList {
    key: ListKey,
    rx: std::sync::mpsc::Receiver<(DisplayList, Option<aad_render::Scene>, Option<PreMesh>)>,
}

/// Mesh tiles of a list built on the worker (see [`premesh`]), for the view the list was asked for.
struct PreMesh {
    basis: u64,
    origin: Vec2,
    edge: f64,
    tiles: Vec<(TileId, u64, MeshData)>,
}

/// What identifies the vertices of a mesh tile besides its content: the origin they are relative to, the paper tint, the tessellation band of
/// the list they come from, and true-scale lineweights.
fn mesh_basis_of(origin: Vec2, paper: Rgb, band: i32, true_scale: bool) -> u64 {
    let tint = u64::from(paper.0) << 16 | u64::from(paper.1) << 8 | u64::from(paper.2);
    origin.x.to_bits()
        ^ origin.y.to_bits().rotate_left(1)
        ^ tint.rotate_left(21)
        ^ (band as u64).rotate_left(40)
        ^ u64::from(true_scale).rotate_left(48)
}

/// The tile edge and partition for a list: finer tiles for big lists, the plain view edge when those are not usable. `None`: not tiled.
fn plan_tiles(list: &DisplayList, view_height: f64) -> Option<(f64, Vec<(TileId, Vec<usize>)>)> {
    let base_edge = mesh::mesh_tile_edge(view_height);
    let fine_edge = mesh::mesh_tile_edge_for(list, view_height);
    mesh::tiling_plan(list, fine_edge)
        .map(|p| (fine_edge, p))
        .or_else(|| if fine_edge == base_edge { None } else { mesh::tiling_plan(list, base_edge).map(|p| (base_edge, p)) })
}

/// Partition, fingerprint and mesh every tile of a new list on the worker that built it, for the view it was asked for.
fn premesh(list: &DisplayList, center: Vec2, height: f64, paper: Rgb, lw_mode: Option<mesh::LineweightMode>, band: i32) -> Option<PreMesh> {
    let origin = mesh::choose_origin(&list.bounds, center, height);
    let basis = mesh_basis_of(origin, paper, band, matches!(lw_mode, Some(mesh::LineweightMode::TrueScale)));
    let (edge, parts) = plan_tiles(list, height)?;
    let tiles = parts
        .par_iter()
        .map(|(id, indices)| {
            (*id, mesh::tile_content_fingerprint(list, indices) ^ basis, mesh::build_mesh_prims(list, indices, origin, paper, lw_mode))
        })
        .collect();
    Some(PreMesh { basis, origin, edge, tiles })
}

/// First display list of a freshly opened large drawing, built on a worker so the window stays alive.
struct FirstBuild {
    key: ListKey,
    doc: std::sync::Arc<Drawing>,
    rx: std::sync::mpsc::Receiver<(DisplayList, Option<aad_render::Scene>)>,
}

/// Returned by `mesh()` while the first display list is still being built.
const LOADING: &str = "display list loading";
/// Drawings with at least this many entities get their first display list built off the UI thread.
const ASYNC_FIRST_BUILD_ENTITIES: usize = 3000;

/// Eased wheel zoom in progress (~120 ms). Fast notches accumulate onto `target`.
struct ZoomAnim {
    from: aad_engine::View,
    target: aad_engine::View,
    t0: std::time::Instant,
}

impl ZoomAnim {
    fn sample(&self, now: std::time::Instant) -> aad_engine::View {
        let ms = now.duration_since(self.t0).as_millis();
        if ms >= ZOOM_ANIM_MS {
            return self.target;
        }
        let t = ease_out_cubic(ms as f64 / ZOOM_ANIM_MS as f64);
        let ln_h = self.from.height.ln() + (self.target.height.ln() - self.from.height.ln()) * t;
        aad_engine::View {
            center: Vec2::new(
                self.from.center.x + (self.target.center.x - self.from.center.x) * t,
                self.from.center.y + (self.target.center.y - self.from.center.y) * t,
            ),
            height: ln_h.exp(),
        }
    }
}

const ZOOM_ANIM_MS: u128 = 120;

fn zoomed_view(v: aad_engine::View, factor: f64, about: Vec2) -> aad_engine::View {
    let h = (v.height / factor).clamp(1e-9, 1e12);
    let k = h / v.height;
    aad_engine::View { center: about + (v.center - about) * k, height: h }
}

fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

/// One retained mesh tile (Phase B): remeshed only when its content fingerprint changes.
struct TileMesh {
    /// `tile_content_fingerprint` xor'd with the mesh basis (origin / tint / band / lw).
    key: u64,
    mesh: MeshData,
}

/// Cached display list / GPU mesh so pans and small edits do not rebuild everything every frame.
struct FrameCache {
    list_key: ListKey,
    list: Option<DisplayList>,
    list_doc: Option<std::sync::Arc<Drawing>>,
    /// Phase C model-space scene (Arc tiles); `None` for paper, traces, or monolithic builds.
    scene: Option<aad_render::Scene>,
    mesh_key: Option<u64>,
    /// Composed mesh (monolithic or merged tiles) for inspect / present fallback.
    mesh: Option<MeshData>,
    mesh_origin: Vec2,
    /// Per-tile meshes when tiling is active; empty on the monolithic path.
    tiles: HashMap<TileId, TileMesh>,
    /// Mesh basis last used for tile vertices (origin + tint + band + true-scale). Cleared tiles when this changes.
    mesh_basis: Option<u64>,
    /// Tiles remeshed on the last mesh rebuild (0 on fine pan; tests assert dirty edits).
    tiles_remeshed: usize,
    /// Non-empty tiles retained after the last tiled rebuild (0 when monolithic).
    tiles_total: usize,
    /// True when the last mesh rebuild used the spatial tile path.
    mesh_tiled: bool,
    pending: Option<PendingList>,
    first: Option<FirstBuild>,
    /// Last synchronous display-list build time (ms). Worker rebuilds do not update this.
    list_build_ms: f64,
    /// Last mesh pack time (ms).
    mesh_ms: f64,
    /// The tile edge of the list on screen, with the band it was chosen for: kept while the band is the same, so zooming inside a band does
    /// not re-partition (and remesh) the drawing.
    tile_edge: Option<(i32, f64)>,
}

/// Identity of the last GPU present: document mesh, overlay, view uniform, and clear colour.
#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct PaintStamp {
    mesh_key: u64,
    over_key: u64,
    uniform: [u32; 12],
    clear: (u8, u8, u8),
}

impl Default for FrameCache {
    fn default() -> Self {
        FrameCache {
            list_key: (0, 0, 0, false, 0, 0b11, 0),
            list: None,
            list_doc: None,
            scene: None,
            mesh_key: None,
            mesh: None,
            mesh_origin: Vec2::ZERO,
            tiles: HashMap::new(),
            mesh_basis: None,
            tiles_remeshed: 0,
            tiles_total: 0,
            mesh_tiled: false,
            pending: None,
            first: None,
            list_build_ms: 0.0,
            mesh_ms: 0.0,
            tile_edge: None,
        }
    }
}

impl FrameCache {
    /// Drop the composed mesh so the next [`Host::mesh`] rebuilds it. Retains per-tile meshes so
    /// an edit can remesh only dirty tiles (Phase B).
    fn invalidate_composed_mesh(&mut self) {
        self.mesh_key = None;
        self.mesh = None;
    }

    /// Drop composed and tiled meshes (origin / LOD basis change, or tiling abandoned).
    /// Ring instances and mesh bytes of what is painted: the sum of the tiles when the mesh is tiled (no composed mesh is kept then), else the one mesh.
    fn mesh_totals(&self) -> (usize, usize) {
        let one = |m: &MeshData| (m.ring_instances(), m.lines.len() + m.tris.len() + m.quads.len() + m.rings.len());
        if self.mesh_tiled {
            return self.tiles.values().map(|t| one(&t.mesh)).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        }
        self.mesh.as_ref().map_or((0, 0), one)
    }

    fn clear_mesh(&mut self) {
        self.invalidate_composed_mesh();
        self.tiles.clear();
        self.mesh_basis = None;
        self.tiles_remeshed = 0;
        self.tiles_total = 0;
        self.mesh_tiled = false;
        self.scene = None;
    }
}

/// Whether model space is drawn with other stores around it: traces of the level being edited,
/// or the level shown for reference in a sheet view.
fn has_traces(doc: &Drawing) -> bool {
    doc.active_sheet.is_some() || doc.active_view.is_some() || doc.active_level_ref().is_some_and(|l| !l.traces.is_empty())
}

/// Opt-in timing for the host: with `AAD_PERF=1` a redraw that takes over 20 ms is appended to `%LOCALAPPDATA%\OSAS\odraft-perf.log` with the time of each step.
mod perf {
    use std::sync::OnceLock;
    use std::time::Instant;

    fn on() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var("AAD_PERF").is_ok_and(|v| v == "1"))
    }
    pub fn start() -> Option<Instant> {
        on().then(Instant::now)
    }
    /// Milliseconds since `start` (0 when timing is off).
    pub fn lap(start: &Option<Instant>) -> f64 {
        start.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0)
    }
    /// Log a line whatever the time (only when timing is on).
    pub fn force(line: &str) {
        let Some(dir) = std::env::var_os("LOCALAPPDATA") else { return };
        let path = std::path::Path::new(&dir).join("OSAS").join("odraft-perf.log");
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "host: {line}");
        }
    }
    /// A named milestone with the wall-clock time (milliseconds since the Unix epoch), so a script can subtract the process start time.
    /// Only when timing is on. The first-list and first-frame markers below are what the open-time audit reads.
    pub fn mark(name: &str) {
        if !on() {
            return;
        }
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        force(&format!("marker {name} epoch_ms={ms}"));
    }
    /// The first frame that paints a drawing with geometry in it (once per process), when timing is on.
    pub fn first_geometry_frame(entities: usize) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static DONE: AtomicBool = AtomicBool::new(false);
        if entities > 0 && on() && !DONE.swap(true, Ordering::Relaxed) {
            mark(&format!("first geometry frame entities={entities}"));
        }
    }
    pub fn note(start: &Option<Instant>, line: &str) {
        let Some(t) = start else { return };
        let total = t.elapsed().as_secs_f64() * 1000.0;
        if total < 20.0 {
            return;
        }
        let Some(dir) = std::env::var_os("LOCALAPPDATA") else { return };
        let path = std::path::Path::new(&dir).join("OSAS").join("odraft-perf.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "host {total:.0} ms total: {line}");
        }
    }
}

/// The display list of `space`; model space of a level with traces is built with its traces,
/// faded towards `background`.
/// The object types a save left out, from the save result's `notSaved` map (`{"MULTILEADER": 164}`), as `164 MULTILEADER`.
fn not_saved_list(result: &Value) -> String {
    let Some(map) = result.get("notSaved").and_then(Value::as_object) else { return String::new() };
    map.iter().filter_map(|(kind, n)| Some(format!("{} {kind}", n.as_u64()?))).collect::<Vec<_>>().join(", ")
}

/// Like [`build_list`], also returning a Phase C [`aad_render::Scene`] when model-space tiling applies.
fn build_list_and_scene(doc: &Drawing, space: &Space, opts: &aad_render::Options, background: Rgb) -> (DisplayList, Option<aad_render::Scene>) {
    if matches!(space, Space::Model) && has_traces(doc) {
        let guides = aad_engine::sectionguide::active_guides(doc);
        return (aad_render::build_with_guides(doc, opts, background, &guides), None);
    }
    if matches!(space, Space::Model)
        && let Some(scene) = aad_render::build_scene_if_viable(doc, opts)
    {
        // One measuring pass decides whether to tile and builds the tiles (asking first and then building measured twice).
        let list = aad_render::flatten_scene(&scene);
        return (list, Some(scene));
    }
    (aad_render::build(doc, space, opts), None)
}

fn space_fp(space: &Space) -> u64 {
    match space {
        Space::Model => 0,
        Space::Paper(n) => {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            n.hash(&mut h);
            h.finish()
        }
    }
}

/// Wheel notch. Positive zooms in, about the pointer.
pub const POINTER_WHEEL: u32 = 1;
/// Left button pressed. Places a point, or picks when no command is running.
pub const POINTER_LEFT_DOWN: u32 = 2;
/// Left button released. Closes a selection window after a drag.
pub const POINTER_LEFT_UP: u32 = 3;
/// Pointer moved. Pans while the middle button is down, and opens a selection window on a drag.
pub const POINTER_MOVE: u32 = 4;
/// Middle button pressed. The next moves pan the view.
pub const POINTER_MIDDLE_DOWN: u32 = 5;
/// Middle button released.
pub const POINTER_MIDDLE_UP: u32 = 6;
/// Right button. Enter while a command is running.
pub const POINTER_RIGHT: u32 = 7;
/// Left button double-click. Edits text, or enters a layout viewport.
pub const POINTER_LEFT_DOUBLE: u32 = 8;
/// Middle button double-click. Zooms to the drawing extents.
pub const POINTER_MIDDLE_DOUBLE: u32 = 9;
/// Shift is held.
pub const MOD_SHIFT: u32 = 1;
/// Ctrl is held.
pub const MOD_CTRL: u32 = 2;

/// One pointer event. Screen pixels are relative to the drawing surface, origin top-left.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub x: f64,
    pub y: f64,
    pub action: u32,
    pub wheel: f32,
    /// [`MOD_SHIFT`] when that key is held. Ignored for the wheel.
    pub mods: u32,
}

/// Counts the WinUI status line reads back.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub entities: u32,
    pub _pad: u32,
    pub view_height: f64,
    pub width: u32,
    pub height: u32,
}

/// Wheel events posted by the child window, applied on the next [`Host::pump`].
#[cfg(windows)]
pub struct Bridge {
    pending: Mutex<Vec<Pointer>>,
}

#[cfg(windows)]
impl Bridge {
    fn new() -> Self {
        Bridge { pending: Mutex::new(Vec::new()) }
    }

    fn push_wheel(&self, x: f64, y: f64, notches: f32) {
        self.push(Pointer { x, y, action: POINTER_WHEEL, wheel: notches, mods: 0 });
    }

    fn push(&self, ev: Pointer) {
        let mut queue = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if ev.action == POINTER_MOVE
            && let Some(last) = queue.last_mut()
            && last.action == POINTER_MOVE
        {
            *last = ev;
            return;
        }
        if queue.len() == 64 {
            queue.remove(0);
        }
        queue.push(ev);
    }

    fn drain(&self) -> Vec<Pointer> {
        let mut queue = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *queue)
    }
}

/// The values beside the cursor. The command line already shows the prompt.
/// Kept for the overlay test; the WinUI fields use [`dyn_boxes`].
#[cfg(test)]
fn dyn_line(prompt: &aad_engine::Prompt, frame: Option<&aad_engine::dyninput::Frame>, draft: &str, aim: Vec2) -> String {
    let line = if let Some(frame) = frame
        && let Some(entry) = aad_engine::dyninput::parse(draft, frame)
    {
        let held = aad_engine::dyninput::constrain(&entry, frame, aim);
        let (first_live, second_live) = frame.live(entry.format, held);
        let dist = |v: f64| aad_engine::units::format_distance(v, 2, 4);
        let first = if entry.first.is_empty() { dist(first_live) } else { entry.first.to_string() };
        let second = if entry.second.is_empty() {
            match entry.format {
                aad_engine::dyninput::Format::Cartesian => dist(second_live),
                aad_engine::dyninput::Format::Polar => format!("{:.1}", second_live.to_degrees()),
            }
        } else {
            entry.second.to_string()
        };
        let mark = if frame.relative() { "@ " } else { "" };
        match entry.format {
            aad_engine::dyninput::Format::Cartesian => format!("{mark}{first} , {second}"),
            aad_engine::dyninput::Format::Polar => format!("{mark}{first} < {second}"),
        }
    } else if !draft.is_empty() {
        draft.to_string()
    } else if let Some(base) = prompt.base.filter(|base| base.is_finite() && aim.is_finite()) {
        let deg = (base.angle_to(aim).to_degrees() * 10.0).round() / 10.0;
        format!("{} < {deg}", aad_engine::units::format_distance(base.dist(aim), 2, 4))
    } else if aim.is_finite() {
        format!("{}, {}", aad_engine::units::format_distance(aim.x, 2, 4), aad_engine::units::format_distance(aim.y, 2, 4))
    } else {
        String::new()
    };
    line.chars().take(48).collect()
}

/// The two Dynamic Input boxes. The command-line text stays the source of truth; this is only the view.
fn dyn_boxes(prompt: &aad_engine::Prompt, frame: &aad_engine::dyninput::Frame, draft: &str, aim: Vec2) -> (String, String, String, String, i32) {
    let dist = |v: f64| aad_engine::units::format_distance(v, 2, 4);
    let clip = |text: String| text.chars().take(24).collect();
    // The first point of a command, nothing typed: there is no distance to show, and the cursor's
    // own X and Y are not shown.
    if draft.trim().is_empty() && frame.base.is_none() {
        return (String::new(), String::new(), String::new(), String::new(), 0);
    }
    if let Some(entry) = aad_engine::dyninput::parse(draft, frame) {
        let held = aad_engine::dyninput::constrain(&entry, frame, aim);
        let (first_live, second_live) = frame.live(entry.format, held);
        let first = if entry.first.is_empty() { dist(first_live) } else { entry.first.to_string() };
        let second = if entry.second.is_empty() {
            match entry.format {
                aad_engine::dyninput::Format::Cartesian => dist(second_live),
                aad_engine::dyninput::Format::Polar => format!("{:.1}°", second_live.to_degrees()),
            }
        } else {
            entry.second.to_string()
        };
        let (a_label, b_label) = match entry.format {
            aad_engine::dyninput::Format::Cartesian => ("X", "Y"),
            aad_engine::dyninput::Format::Polar => ("Length", "Angle"),
        };
        let active = if entry.split { 1 } else { 0 };
        return (a_label.into(), b_label.into(), clip(first), clip(second), active);
    }
    if !draft.is_empty() {
        return ("Input".into(), String::new(), clip(draft.to_string()), String::new(), 0);
    }
    if let Some(base) = prompt.base.filter(|base| base.is_finite() && aim.is_finite()) {
        let deg = (base.angle_to(aim).to_degrees() * 10.0).round() / 10.0;
        return ("Length".into(), "Angle".into(), dist(base.dist(aim)), format!("{deg:.1}°"), 0);
    }
    // No live X and Y readout: the cursor shows what is being entered, not where it is.
    (String::new(), String::new(), String::new(), String::new(), 0)
}

/// What is typed while no point is being asked for: the command name or the answer, near the cursor.
fn draft_box(draft: &str, running: bool) -> (String, String, String, String, i32) {
    let label = if running { "Input" } else { "Command" };
    (label.into(), String::new(), draft.chars().take(24).collect(), String::new(), 0)
}

/// One editing session and, on Windows, its drawing surface.
pub struct Host {
    session: Session,
    /// The view the drawing was last fitted to (centre x, y and height). While the view is still
    /// exactly this one nobody has navigated, so a change of the drawing area refits it.
    fitted: Option<(f64, f64, f64)>,
    width: u32,
    height: u32,
    error: String,
    /// What the last completed save could not write, as `164 MULTILEADER, 2 MLINE`; empty when the file keeps everything.
    save_notice: String,
    result: String,
    /// Shortcut or snap menu requested by a right-click, as JSON. Empty when none is waiting.
    menu_request: String,
    #[cfg(windows)]
    bridge: Arc<Bridge>,
    #[cfg(windows)]
    hwnd: isize,
    #[cfg(windows)]
    gpu: Option<gpu::Gpu>,
    /// Last presented frame identity; when unchanged, `redraw` skips GPU work.
    #[cfg(windows)]
    paint: Option<PaintStamp>,
    /// Set when the drawing must be presented (commands, resize, async list ready). Cleared after a paint attempt.
    #[cfg(windows)]
    paint_dirty: bool,
    /// `embed`, `move_child`, and `pump` present on the UI thread. A nested call from inside
    /// `Present` must not touch the surface again.
    #[cfg(windows)]
    in_call: bool,
    /// Command lines that arrived while `in_call` was set. Run after the frame, not dropped.
    #[cfg(windows)]
    pending_submit: Vec<String>,
    /// Screen point where the left or middle button went down.
    press: Option<Vec2>,
    panning: bool,
    dragged: bool,
    dark: bool,
    /// Device pixels per logical point. Hairlines are one point wide.
    pixels_per_point: f32,
    clock: std::time::Instant,
    /// Last pointer, in world units, before snapping.
    raw: Option<Vec2>,
    /// The point a click will use.
    aim: Vec2,
    screen: Option<Vec2>,
    snap: Option<aad_engine::snap::SnapHit>,
    /// Polar / object-snap tracking result at the aim (drawn on the overlay).
    track: Option<aad_engine::snap::tracking::Track>,
    /// Entity under the pick box while selecting (rollover highlight).
    hover: Option<aad_engine::doc::Handle>,
    hot: Option<overlay::HotGrip>,
    /// Command-line text that has not been submitted. Dynamic Input reads it.
    draft: String,
    /// Tab cycling: the raw point where it started, and how many times Tab has advanced.
    snap_cycle: Option<(Vec2, usize)>,
    show_ucs: bool,
    canvas_visible: bool,
    assistant: assistant::Desk,
    control_rx: Option<std::sync::mpsc::Receiver<control::Job>>,
    control_port: u16,
    control_stop: Option<Arc<AtomicBool>>,
    /// Set by `app.quit` on the control channel; the shell closes when chrome reports it.
    quit_requested: bool,
    /// The last panels JSON with the state it was built for. The side tabs ask for it about eight times a second, and building it
    /// scans every block reference in the drawing, so an unchanged drawing, selection and hover reuse the text.
    panels_cache: std::sync::Mutex<Option<(String, String)>>,
    /// A drawing being read on a worker thread: the path asked for and the channel the result arrives on.
    loading: Option<(String, std::sync::mpsc::Receiver<Result<OpenedDoc, String>>)>,
    /// What the open worker is doing now, one line for the status bar.
    open_progress: Arc<std::sync::Mutex<String>>,
    /// What the last completed open did, as JSON ([`aad_engine::nativefile::OpenInfo`]); empty before the first one.
    open_info: String,
    /// A drawing being written on a worker thread.
    saving: Option<(Arc<aad_engine::cmd::file::SaveJob>, std::sync::mpsc::Receiver<Result<usize, String>>)>,
    /// A plot being built on a worker thread.
    plotting: Option<std::sync::mpsc::Receiver<Result<Value, String>>>,
    /// Display list and mesh reused across pans / small edits.
    frame: FrameCache,
    /// Control-channel `app.open` / `app.save` waiting for a worker poll to finish.
    control_io: Option<(control::Job, ControlIoKind)>,
    /// Wheel zoom easing; advanced on [`Host::pump`] / present.
    zoom_anim: Option<ZoomAnim>,
    /// Recovery snapshots of drawings with unsaved changes, and the save status line (`autosave.rs`).
    autosave: autosave::Autosave,
    /// A `geodb.build` running on a worker; its model goes back into the session on a later pump.
    geodb_job: Option<std::sync::mpsc::Receiver<aad_engine::cmd::geodb::BuildDone>>,
}

/// A drawing read by the open worker: flattened and ready, with the project file it is edited in.
struct OpenedDoc {
    prepared: aad_engine::cmd::file::Prepared,
    /// The file the document is tied to: the project file itself, or the working copy of a DWG or DXF.
    path: String,
    info: aad_engine::nativefile::OpenInfo,
}

/// Which file job a deferred control reply is waiting on.
#[derive(Clone, Copy)]
enum ControlIoKind {
    Open,
    Save,
}

/// DXF and DWG read and write. The engine stays free of file formats; the host installs them
/// once. DWG is converted by a worker process when that program is beside the shell.
fn install_formats() {
    aad_engine::cmd::file::set_io(aad_engine::cmd::file::IoHooks {
        read: |bytes, name| aad_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |drawing, name| aad_io::write(drawing, name).map_err(|e| e.to_string()),
        plot: Some(|drawing, space, opts| aad_io::plot(drawing, space, opts).map_err(|e| e.to_string())),
    });
    aad_engine::cmd::file::set_container(aad_engine::nativefile::container_hooks());
    aad_engine::cmd::file::set_pdf_reader(|bytes, page| aad_io::pdfread::read_page(bytes, page).map_err(|e| e.to_string()));
    aad_engine::cmd::file::set_pdf_merge(|parts| aad_io::pdf::merge(parts).map_err(|e| e.to_string()));
    aad_engine::cmd::file::set_raster_reader(|bytes| {
        aad_io::raster::read_gray(bytes).map(|g| aad_engine::cmd::file::Raster { width: g.width, height: g.height, gray: g.gray })
    });
    if let Some(worker) = dwg_worker() {
        let _ = aad_dwg::isolated::enable_with(worker);
    }
}

fn dwg_worker() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let names = ["aad-dwg-worker", "aad", "aad-cli"];
    for name in names {
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        for path in [dir.join(&file), dir.join("..").join(&file), dir.join("../debug").join(&file)] {
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// The demo drawing that `--sample` opens instead of the built-in one: the file named by
/// `AAD_SAMPLE_FILE`, or `sample/DEMO.dwg` (then `.dxf`) beside the program. Not part of the
/// repository: a drawing made with other software is not committed here.
fn local_sample_path() -> Option<String> {
    let exists = |p: std::path::PathBuf| p.is_file().then(|| p.to_string_lossy().into_owned());
    if let Some(v) = std::env::var_os("AAD_SAMPLE_FILE").filter(|v| !v.is_empty()) {
        return exists(std::path::PathBuf::from(v));
    }
    let dir = std::env::current_exe().ok()?.parent()?.join("sample");
    exists(dir.join("DEMO.dwg")).or_else(|| exists(dir.join("DEMO.dxf")))
}

impl Host {
    /// `sample` opens the built-in sample drawing. Otherwise the session starts empty.
    pub fn create(sample: bool) -> Self {
        install_formats();
        let mut session = Session::empty();
        if sample {
            // A drawing the user keeps for demos replaces the built-in sample; it is opened as a copy.
            let local = local_sample_path().and_then(|path| {
                let prepared = aad_engine::cmd::file::read_drawing_file(&path).ok()?;
                aad_engine::cmd::file::finish_open_copy(&mut session, prepared, &path).ok()
            });
            if local.is_none() {
                session.open_drawing(aad_engine::sample::default_sample(), "Sample", None);
            }
        } else {
            session.new_drawing(false);
        }
        session.viewport_px = (1200.0, 800.0);
        let _ = session.zoom_extents();
        Host {
            session,
            fitted: None,
            width: 1200,
            height: 800,
            error: String::new(),
            save_notice: String::new(),
            result: String::new(),
            menu_request: String::new(),
            #[cfg(windows)]
            bridge: Arc::new(Bridge::new()),
            #[cfg(windows)]
            hwnd: 0,
            #[cfg(windows)]
            gpu: None,
            #[cfg(windows)]
            paint: None,
            #[cfg(windows)]
            paint_dirty: true,
            #[cfg(windows)]
            in_call: false,
            #[cfg(windows)]
            pending_submit: Vec::new(),
            press: None,
            panning: false,
            dragged: false,
            dark: true,
            pixels_per_point: 1.0,
            clock: std::time::Instant::now(),
            raw: None,
            aim: Vec2::ZERO,
            screen: None,
            snap: None,
            track: None,
            hover: None,
            hot: None,
            draft: String::new(),
            snap_cycle: None,
            show_ucs: true,
            canvas_visible: true,
            assistant: assistant::Desk::new(),
            control_rx: None,
            control_port: 0,
            control_stop: None,
            quit_requested: false,
            panels_cache: std::sync::Mutex::new(None),
            loading: None,
            open_progress: Arc::new(std::sync::Mutex::new(String::new())),
            open_info: String::new(),
            saving: None,
            plotting: None,
            frame: FrameCache::default(),
            control_io: None,
            zoom_anim: None,
            autosave: autosave::Autosave::new(),
            geodb_job: None,
        }
    }

    /// Start reading a drawing file on a worker thread. The window stays responsive; the shell
    /// calls [`Host::open_poll`] until it reports the result.
    pub fn open_start(&mut self, path: &str) -> Result<(), String> {
        self.open_start_with(path, aad_engine::nativefile::OnChanged::Auto)
    }

    /// [`Host::open_start`] with the choice for a source that changed since its working copy was made. A project file opens as it is; a DWG
    /// or DXF is converted once into its working copy (a project file) on the worker and edited there, so the original is never written.
    pub fn open_start_with(&mut self, path: &str, on_changed: aad_engine::nativefile::OnChanged) -> Result<(), String> {
        if self.loading.is_some() {
            return Err("a drawing is already being opened".into());
        }
        if path.is_empty() || path.len() > 4096 {
            return Err("missing path".into());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let owned = path.to_string();
        let progress = self.open_progress.clone();
        if let Ok(mut line) = progress.lock() {
            line.clear();
        }
        std::thread::Builder::new()
            .name("aad-open".into())
            .spawn(move || {
                let say = |line: &str| {
                    if let Ok(mut slot) = progress.lock() {
                        *slot = line.to_string();
                    }
                };
                let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let opened = aad_engine::nativefile::open_for_edit(&owned, on_changed, &say)?;
                    say("Preparing the drawing");
                    Ok(OpenedDoc { prepared: aad_engine::cmd::file::prepare(opened.drawing), path: opened.path, info: opened.info })
                }))
                .unwrap_or_else(|_| Err("internal error while reading the file".into()));
                let _ = tx.send(read);
            })
            .map_err(|e| e.to_string())?;
        self.loading = Some((path.to_string(), rx));
        Ok(())
    }

    /// What the open worker is doing now (one line; empty when nothing is being opened).
    pub fn open_progress(&self) -> String {
        if self.loading.is_none() {
            return String::new();
        }
        self.open_progress.lock().map(|line| line.clone()).unwrap_or_default()
    }

    /// What the last completed open did, as JSON: the working copy, the source, whether it was converted or reused, whether the source has
    /// changed, the time of each stage and one line for the status bar. Empty before the first open.
    pub fn open_info(&self) -> &str {
        &self.open_info
    }

    /// `Ok(0)` nothing is loading, `Ok(1)` still reading, `Ok(2)` the drawing is open and fitted.
    /// A failed read or import is the error; the drawing that was open is untouched.
    pub fn open_poll(&mut self) -> Result<i32, String> {
        use std::sync::mpsc::TryRecvError;
        let Some((path, rx)) = self.loading.as_ref() else {
            return Ok(0);
        };
        let read = match rx.try_recv() {
            Ok(read) => read,
            Err(TryRecvError::Empty) => return Ok(1),
            Err(TryRecvError::Disconnected) => Err("the file reader stopped".into()),
        };
        let _ = path;
        self.loading = None;
        let imported = read.and_then(|opened| {
            let OpenedDoc { prepared, path, info } = opened;
            let index = aad_engine::cmd::file::finish_open(&mut self.session, prepared, &path).map_err(|e| e.to_string())?;
            if let Some(state) = self.session.docs.get_mut(index) {
                state.source = info.source.clone();
            }
            if !info.message.is_empty() {
                self.session.echo(info.message.clone());
            }
            self.open_info = serde_json::to_string(&info).unwrap_or_default();
            Ok(index)
        });
        match imported {
            Ok(_) => {
                self.fit_view();
                #[cfg(windows)]
                {
                    self.paint = None;
                    self.paint_dirty = true;
                }
                Ok(2)
            }
            Err(e) => {
                self.session.echo(e.clone());
                Err(e)
            }
        }
    }

    /// Start writing the active drawing on a worker thread: to `path`, or to its own file when
    /// `None`. The shell calls [`Host::save_poll`] until it reports the result.
    pub fn save_start(&mut self, path: Option<&str>) -> Result<(), String> {
        if self.saving.is_some() {
            return Err("a drawing is already being saved".into());
        }
        if path.is_some_and(|p| p.is_empty() || p.len() > 4096) {
            return Err("missing path".into());
        }
        self.save_notice.clear();
        let job = Arc::new(aad_engine::cmd::file::begin_save(&mut self.session, path).map_err(|e| e.to_string())?);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = job.clone();
        std::thread::Builder::new()
            .name("aad-save".into())
            .spawn(move || {
                let written = std::panic::catch_unwind(|| worker.run()).unwrap_or_else(|_| Err("internal error while saving the file".into()));
                let _ = tx.send(written);
            })
            .map_err(|e| e.to_string())?;
        self.saving = Some((job, rx));
        Ok(())
    }

    /// `Ok(0)` nothing is saving, `Ok(1)` still writing, `Ok(2)` saved. A failed save is the error
    /// and the drawing keeps its unsaved changes.
    pub fn save_poll(&mut self) -> Result<i32, String> {
        use std::sync::mpsc::TryRecvError;
        let Some((_, rx)) = self.saving.as_ref() else {
            return Ok(0);
        };
        let written = match rx.try_recv() {
            Ok(written) => written,
            Err(TryRecvError::Empty) => return Ok(1),
            Err(TryRecvError::Disconnected) => Err("the file writer stopped".into()),
        };
        let Some((job, _)) = self.saving.take() else {
            return Ok(0);
        };
        let done = written.and_then(|bytes| aad_engine::cmd::file::finish_save(&mut self.session, &job, bytes).map_err(|e| e.to_string()));
        match done {
            Ok(result) => {
                self.save_notice = not_saved_list(&result);
                Ok(2)
            }
            Err(e) => {
                // Never silent: the command history, the error of the save, and the status line.
                self.session.echo(format!("Save failed: {e}"));
                let title = self.session.docs.get(job.index()).map(|d| d.title.clone()).unwrap_or_default();
                self.autosave.note_save_failed(&title, &e);
                Err(e)
            }
        }
    }

    /// What the last completed save could not write, as `164 MULTILEADER, 2 MLINE`, or empty when the file keeps everything.
    /// The engine also echoes this to the command history, which a user who saved from the menu never sees, so the shell
    /// shows it after the save.
    pub fn save_notice(&self) -> &str {
        &self.save_notice
    }

    /// `geodb.build` runs on a worker (the incremental update of a 232k-entity drawing takes about a second): `Some` with the reply when the
    /// command was taken here, `None` to run it as usual. `{wait: true}` keeps the old synchronous behaviour. While a build runs, the other
    /// `geodb.*` commands are refused, since the drawing's model is with the worker.
    fn geodb_gate(&mut self, command: &str, params: &Value) -> Option<Result<Value, String>> {
        let id = command.to_ascii_lowercase();
        if !id.starts_with("geodb.") {
            return None;
        }
        if self.geodb_job.is_some() {
            let message = "the geometry database is being built; try again when it is done".to_string();
            self.session.echo(message.clone());
            return Some(Err(message));
        }
        if id != "geodb.build" || params.get("wait").and_then(Value::as_bool).unwrap_or(false) {
            return None;
        }
        let params = if params.is_null() { serde_json::json!({}) } else { params.clone() };
        let job = match aad_engine::cmd::geodb::begin_build(&mut self.session, &params) {
            Ok(job) => job,
            Err(e) => {
                let message = e.to_string();
                self.session.echo(message.clone());
                return Some(Err(message));
            }
        };
        let revision = self.session.state().map(|s| s.revision).unwrap_or(0);
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("aad-geodb".into()).spawn(move || {
            let _ = tx.send(job.run());
        });
        if let Err(e) = spawned {
            return Some(Err(e.to_string()));
        }
        self.geodb_job = Some(rx);
        self.session.echo("Building the geometry database in the background...".to_string());
        Some(Ok(serde_json::json!({ "started": true, "revision": revision })))
    }

    /// Take a finished `geodb.build` from its worker (on every pump).
    fn poll_geodb(&mut self) {
        let Some(rx) = &self.geodb_job else { return };
        match rx.try_recv() {
            Ok(done) => {
                self.geodb_job = None;
                if let Err(e) = aad_engine::cmd::geodb::finish_build(&mut self.session, done) {
                    self.session.echo(e.to_string());
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.geodb_job = None;
                self.session.echo("The geometry database build stopped (internal error); run geodb.build again.".to_string());
            }
        }
    }

    /// Application methods for working copies, autosave, crash recovery and backups (`docs/NATIVE-OPEN.md`, `docs/AUTOSAVE-BACKUP.md`): the
    /// shell calls them through `aad_host_app`, scripts through the control channel.
    pub fn app(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let text = |key: &str| params.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
        let active_file = |host: &Host| host.session.state().ok().and_then(|st| st.path.clone()).filter(|p| aad_engine::cmd::file::is_container(p));
        match method {
            "app.peek" => {
                let path = text("path").ok_or("missing path")?;
                serde_json::to_value(aad_engine::nativefile::peek(&path)?).map_err(|e| e.to_string())
            }
            "autosave.status" => Ok(self.autosave.status_json(autosave_interval())),
            "autosave.now" => {
                self.autosave.force = true;
                self.autosave.tick(&self.session, autosave_interval());
                if params.get("wait").and_then(Value::as_bool).unwrap_or(false) {
                    self.autosave.wait(&self.session, std::time::Duration::from_secs(300));
                }
                Ok(self.autosave.status_json(autosave_interval()))
            }
            "recovery.list" => {
                let found = aad_engine::recovery::scan(self.autosave.root());
                // A snapshot not newer than its file (the file was saved after it) has nothing to offer: it goes.
                let (offer, stale): (Vec<_>, Vec<_>) = found.into_iter().partition(|f| f.newer_than_file);
                for f in &stale {
                    let _ = aad_engine::recovery::discard(self.autosave.root(), &f.id);
                }
                Ok(serde_json::json!({ "snapshots": offer, "removed": stale.len() }))
            }
            "recovery.restore" => {
                let id = text("id").ok_or("missing id")?;
                let (manifest, drawing) = aad_engine::recovery::load(self.autosave.root(), &id)?;
                // Tied to the file it was being edited in, so Save writes there; a file that is gone makes it a new drawing.
                let path = manifest.path.clone().filter(|p| std::path::Path::new(p).is_file());
                let title = if manifest.title.is_empty() { "Recovered".to_string() } else { manifest.title.clone() };
                let index = self.session.open_drawing(drawing, &title, path);
                let state = self.session.docs.get_mut(index).ok_or("the restored drawing was not opened")?;
                // Unsaved: the restored state differs from the file.
                state.doc = Arc::new((*state.doc).clone());
                state.source = manifest.source.clone();
                let (uid, doc) = (state.uid, state.doc.clone());
                self.autosave.adopt(uid, &doc, id.clone());
                self.session.echo(format!("Restored the unsaved changes of {title} from {}. Save to keep them.", manifest.saved_utc));
                self.fit_view();
                #[cfg(windows)]
                {
                    self.paint = None;
                    self.paint_dirty = true;
                }
                Ok(serde_json::json!({ "index": index, "title": title, "path": manifest.path, "entities": manifest.entities }))
            }
            "recovery.discard" => {
                let id = text("id").ok_or("missing id")?;
                aad_engine::recovery::discard(self.autosave.root(), &id)?;
                Ok(Value::Null)
            }
            "backup.list" => {
                let file =
                    text("path").or_else(|| active_file(self)).ok_or("the drawing is not saved in a project file (.osp), so it has no backups")?;
                let path = std::path::Path::new(&file);
                Ok(serde_json::json!({
                    "file": file,
                    "folder": aad_engine::backup::folder(path).to_string_lossy(),
                    "backups": aad_engine::backup::list(path),
                }))
            }
            "backup.open" => {
                let backup = text("backup").ok_or("missing backup")?;
                let read = aad_engine::nativefile::read_project(&backup)?;
                let name = std::path::Path::new(&backup).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let index = aad_engine::cmd::file::finish_open_copy(&mut self.session, aad_engine::cmd::file::prepare(read.drawing), &name)
                    .map_err(|e| e.to_string())?;
                self.fit_view();
                #[cfg(windows)]
                {
                    self.paint = None;
                    self.paint_dirty = true;
                }
                Ok(serde_json::json!({ "index": index }))
            }
            "backup.restore" => {
                let backup = text("backup").ok_or("missing backup")?;
                let file = text("path").or_else(|| active_file(self)).ok_or("missing path")?;
                let path = std::path::Path::new(&file);
                if !aad_engine::backup::list(path).iter().any(|b| std::path::Path::new(&b.path) == std::path::Path::new(&backup)) {
                    return Err(format!("{backup} is not a backup of {file}"));
                }
                let kept = aad_engine::backup::restore(path, std::path::Path::new(&backup), &aad_engine::backup::Policy::current())?;
                Ok(serde_json::json!({ "file": file, "kept": kept.to_string_lossy() }))
            }
            other => Err(format!("unknown method {other}")),
        }
    }

    /// Start a plot (`plot` or `exportpdf` with its parameters, `path` included) on a worker
    /// thread. The shell calls [`Host::plot_poll`] until it reports the result.
    pub fn plot_start(&mut self, command: &str, params: &Value) -> Result<(), String> {
        if self.plotting.is_some() {
            return Err("a plot is already running".into());
        }
        if command != "plot" && command != "exportpdf" {
            return Err("unknown plot command".into());
        }
        let job = aad_engine::cmd::layout::begin_plot(&self.session, params, command).map_err(|e| e.to_string())?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("aad-plot".into())
            .spawn(move || {
                let done = std::panic::catch_unwind(|| job.run()).unwrap_or_else(|_| Err("internal error while plotting".into()));
                let _ = tx.send(done);
            })
            .map_err(|e| e.to_string())?;
        self.plotting = Some(rx);
        Ok(())
    }

    /// `Ok(0)` no plot is running, `Ok(1)` still building, `Ok(2)` the file is written. A failure
    /// is the error.
    pub fn plot_poll(&mut self) -> Result<i32, String> {
        use std::sync::mpsc::TryRecvError;
        let Some(rx) = self.plotting.as_ref() else {
            return Ok(0);
        };
        let done = match rx.try_recv() {
            Ok(done) => done,
            Err(TryRecvError::Empty) => return Ok(1),
            Err(TryRecvError::Disconnected) => Err("the plot worker stopped".into()),
        };
        self.plotting = None;
        match done {
            Ok(_) => Ok(2),
            Err(e) => {
                self.session.echo(e.clone());
                Err(e)
            }
        }
    }

    pub fn execute(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        if command.is_empty() || command.len() > 128 {
            return Err("missing command".into());
        }
        self.result.clear();
        if let Some(started) = self.geodb_gate(command, params) {
            let value = started?;
            self.result = value.to_string();
            return Ok(value);
        }
        let value = match self.session.execute(command, params) {
            Ok(value) => value,
            Err(e) => {
                let message = e.to_string();
                self.session.echo(message.clone());
                if matches!(command, "qsave" | "save" | "saveas") {
                    let title = self.session.state().map(|d| d.title.clone()).unwrap_or_default();
                    self.autosave.note_save_failed(&title, &message);
                }
                return Err(message);
            }
        };
        let text = value.to_string();
        if text.len() > 8 * 1_024 * 1_024 {
            return Err("result is too large".into());
        }
        self.result = text;
        // Overlay may change without a document revision (e.g. companion.preview ghost).
        #[cfg(windows)]
        {
            self.paint_dirty = true;
        }
        Ok(value)
    }

    /// Run one command-line line, the same path as typing it and pressing Enter.
    pub fn submit(&mut self, text: &str) -> Result<(), String> {
        #[cfg(windows)]
        if self.in_call {
            if self.pending_submit.len() < 8 {
                self.pending_submit.push(text.to_string());
                return Ok(());
            }
            self.session.echo("command was not run".to_string());
            return Err("the drawing is busy".into());
        }
        let result = self.exclusive(|host| host.submit_inner(text));
        #[cfg(windows)]
        self.flush_submit();
        result
    }

    /// Lines queued while a frame was being presented.
    #[cfg(windows)]
    fn flush_submit(&mut self) {
        let mut rounds = 0;
        while rounds < 8 && !self.in_call && !self.pending_submit.is_empty() {
            let pending = std::mem::take(&mut self.pending_submit);
            for text in pending {
                let _ = self.exclusive(|host| host.submit_inner(&text));
            }
            rounds += 1;
        }
    }

    /// Escape. A hot grip is cleared; otherwise the running command is cancelled.
    pub fn cancel_command(&mut self) -> Result<(), String> {
        self.exclusive(|host| {
            host.draft.clear();
            if host.hot.take().is_some() {
                host.session.echo("*Cancel*");
                return host.present_if_ready();
            }
            if host.assistant.busy() {
                host.assistant.stop(&mut host.session);
                return host.present_if_ready();
            }
            host.session.cancel();
            host.present_if_ready()
        })
    }

    pub fn menu_json(&self) -> String {
        chrome::menu_json(&self.session)
    }

    pub fn chrome_json(&self) -> String {
        let cursor = self.screen.filter(|point| point.is_finite()).map(|_| self.aim);
        let mut value: Value = serde_json::from_str(&chrome::chrome_json(&self.session, cursor, &self.draft)).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("assistant".into(), Value::String(self.assistant.phase().to_string()));
            object.insert("assistantPrompt".into(), Value::String(self.assistant.prompt().to_string()));
            object.insert("assistantPartial".into(), Value::String(self.assistant.partial().to_string()));
            object.insert("assistantModel".into(), Value::String(self.assistant.model_status().to_string()));
            object.insert("controlPort".into(), Value::from(self.control_port));
            object.insert("quit".into(), Value::from(self.quit_requested));
            object.insert("ucs".into(), Value::from(self.show_ucs));
            object.insert("dyn".into(), self.dyn_chrome());
        }
        value.to_string()
    }

    /// Cursor parking for the CAD prompt, plus Length/Angle when a dyn frame applies.
    ///
    /// `on` is true whenever the pointer is known (so WinUI can park the CAD field). Length/Angle
    /// (`dims`) still require Dynamic Input mode. Assistant `/` drafts stay off the cursor chrome.
    fn dyn_chrome(&self) -> Value {
        if self.hot.is_some() || self.session.pending_window.is_some() {
            return serde_json::json!({ "on": false });
        }
        let screen = self
            .screen
            .filter(|point| point.is_finite())
            .unwrap_or_else(|| Vec2::new(f64::from(self.width.max(1)) * 0.5, f64::from(self.height.max(1)) * 0.5));
        let running = self.session.running.is_some();
        // Slash assistant drafts belong on the fixed AI bar, not beside the cursor.
        if !running && matches!(bar::classify(&self.draft), bar::Kind::Reference) {
            return serde_json::json!({ "on": false });
        }
        let prompt = self.session.prompt_text();
        let dynmode = self.session.settings.dynmode;
        let drawn = self.session.doc().ok().and_then(aad_engine::annot::drawing_unit);
        let (a_label, b_label, a, b, active, dims) = if !dynmode {
            (String::new(), String::new(), String::new(), String::new(), 0, false)
        } else {
            aad_engine::units::with_drawn_unit(drawn, || match (self.session.current_prompt(), self.dyn_frame()) {
                (Some(prompt), Some(frame)) => {
                    let (a_label, b_label, a, b, active) = dyn_boxes(&prompt, &frame, &self.draft, self.aim);
                    let dims = !(a.is_empty() && b.is_empty());
                    (a_label, b_label, a, b, active, dims)
                }
                _ if !self.draft.trim().is_empty() => {
                    let (a_label, b_label, a, b, active) = draft_box(self.draft.trim_start(), running);
                    let dims = !(a.is_empty() && b.is_empty());
                    (a_label, b_label, a, b, active, dims)
                }
                _ => (String::new(), String::new(), String::new(), String::new(), 0, false),
            })
        };
        serde_json::json!({
            "on": true,
            "x": screen.x,
            "y": screen.y,
            "prompt": prompt,
            "dims": dims,
            "aLabel": a_label,
            "a": a,
            "bLabel": b_label,
            "b": b,
            "active": active,
        })
    }

    /// Natural-language ask from the fixed AI prompt (no CAD classify / cmdline path).
    pub fn ask(&mut self, text: &str) -> Result<(), String> {
        if text.len() > 1_048_576 {
            return Err("text is too long".into());
        }
        self.exclusive(|host| {
            host.draft.clear();
            host.ask_assistant(text)
        })
    }

    /// Drop the command-line log only. The drawing, the running command and the registry stay.
    pub fn clear_log(&mut self) -> Result<(), String> {
        self.session.log.clear();
        Ok(())
    }

    /// Properties, layers and blocks, tagged with the drawing id and revision.
    /// Includes the hovered entity so the side tabs can show selection or rollover.
    pub fn panels_json(&self) -> String {
        let key = self.session.state().ok().map(|state| {
            format!(
                "{}:{}:{}:{}:{}:{}:{}:{}:{}",
                state.uid,
                state.revision,
                state.selection.len(),
                state.selection.first().map(|h| h.hex()).unwrap_or_default(),
                state.selection.last().map(|h| h.hex()).unwrap_or_default(),
                self.hover.map(|h| h.hex()).unwrap_or_default(),
                state.doc.layers.len(),
                state.doc.blocks.len(),
                state.doc.active_sheet_ref().map(|sheet| sheet.name.as_str()).unwrap_or_default(),
            )
        });
        if let Some(key) = &key
            && let Ok(cache) = self.panels_cache.lock()
            && let Some((cached_key, text)) = cache.as_ref()
            && cached_key == key
        {
            return text.clone();
        }
        let text = panels::panels_json(&self.session, self.hover);
        if let (Some(key), Ok(mut cache)) = (key, self.panels_cache.lock()) {
            *cache = Some((key, text.clone()));
        }
        text
    }

    /// Remember what is typed at the command line, so Dynamic Input can follow it.
    pub fn set_draft(&mut self, text: &str) -> Result<(), String> {
        if text.len() > 4_096 {
            return Err("text is too long".into());
        }
        self.draft = text.to_string();
        #[cfg(windows)]
        if self.in_call {
            return Ok(());
        }
        if let Some(screen) = self.screen {
            self.place_cursor(screen.x, screen.y, false)?;
        }
        self.present_if_ready()
    }

    /// Tab at a point prompt: the separator for the format on show (`40` becomes `40<`).
    pub fn dyn_tab(&self, text: &str) -> Option<String> {
        if text.len() > 4_096 {
            return None;
        }
        let frame = self.dyn_frame()?;
        let sep = aad_engine::dyninput::tab_separator(text, &frame)?;
        let mut next = text.to_string();
        next.push(sep);
        Some(next)
    }

    fn submit_inner(&mut self, text: &str) -> Result<(), String> {
        if text.len() > 1_048_576 {
            let message = "text is too long".to_string();
            self.session.echo(message.clone());
            return Err(message);
        }
        // `geodb.build` typed (or chosen from a menu) without options runs on a worker like the JSON form.
        if self.session.running.is_none()
            && text.trim().eq_ignore_ascii_case("geodb.build")
            && let Some(started) = self.geodb_gate("geodb.build", &Value::Null)
        {
            return started.map(|_| ());
        }
        if self.assistant.confirming() {
            let word = text.trim().to_ascii_lowercase();
            let accept = matches!(word.as_str(), "y" | "yes" | "allow");
            let decline = matches!(word.as_str(), "n" | "no" | "decline");
            self.draft.clear();
            if accept || decline {
                self.assistant.confirm(&mut self.session, accept);
                return self.present_if_ready();
            }
            let message = "Allow or Decline".to_string();
            self.session.echo(message.clone());
            return Err(message);
        }
        if self.session.running.is_none() && self.hot.is_none() {
            // Assistant requests need a leading `/` (or a `/reference` word). Plain sentences stay
            // on the command line so Space/Enter run CAD commands.
            if matches!(bar::classify(text), bar::Kind::Reference) {
                return self.ask_assistant(text);
            }
        }
        let finished = self.finish_dyn(text);
        self.draft.clear();
        let result = self.session.cmdline(&finished);
        if let Err(e) = &result {
            self.session.echo(e.to_string());
        }
        let result = result.map_err(|e| e.to_string());
        self.present_if_ready()?;
        result
    }

    fn present_if_ready(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.paint_dirty = true;
            if self.gpu.is_some() {
                let result = self.redraw();
                if result.is_ok() {
                    self.paint_dirty = false;
                }
                return result;
            }
        }
        Ok(())
    }

    fn exclusive(&mut self, body: impl FnOnce(&mut Self) -> Result<(), String>) -> Result<(), String> {
        #[cfg(windows)]
        if self.in_call {
            return Ok(());
        }
        #[cfg(windows)]
        {
            self.in_call = true;
        }
        let result = body(self);
        #[cfg(windows)]
        {
            self.in_call = false;
        }
        result
    }

    pub fn pointer(&mut self, ev: Pointer) -> Result<(), String> {
        if !ev.x.is_finite() || !ev.y.is_finite() || !ev.wheel.is_finite() {
            return Err("pointer values must be finite".into());
        }
        match ev.action {
            POINTER_WHEEL => self.wheel(ev),
            POINTER_LEFT_DOWN => self.left_down(ev),
            POINTER_LEFT_UP => self.left_up(ev),
            POINTER_MOVE => self.moved(ev),
            POINTER_MIDDLE_DOWN => {
                self.panning = true;
                self.press = Some(Vec2::new(ev.x, ev.y));
                Ok(())
            }
            POINTER_MIDDLE_UP => {
                self.panning = false;
                self.press = None;
                Ok(())
            }
            POINTER_RIGHT => self.right_click(ev),
            POINTER_LEFT_DOUBLE => self.left_double(ev),
            POINTER_MIDDLE_DOUBLE => self.middle_double(),
            _ => Err("unsupported pointer action".into()),
        }
    }

    /// Black drawing area, or white. Colour 7 follows the background.
    /// One-page PDF of the layout on screen, fitted to the sheet.
    pub fn pdf_bytes(&mut self) -> Result<Vec<u8>, String> {
        let mut produced: Option<Result<Vec<u8>, String>> = None;
        self.exclusive(|host| {
            produced = Some(host.plot_pdf());
            Ok(())
        })?;
        produced.unwrap_or_else(|| Err("the preview is busy".into()))
    }

    fn plot_pdf(&self) -> Result<Vec<u8>, String> {
        let state = self.session.state().map_err(|e| e.to_string())?;
        let opts = aad_io::PdfOptions { compress: true, fit: Some(true), ..aad_io::PdfOptions::default() };
        let bytes = aad_io::pdf(&state.doc, &state.space, &opts).map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("the PDF is too large to preview".into());
        }
        if !bytes.starts_with(b"%PDF") {
            return Err("the plot was not a PDF".into());
        }
        Ok(bytes)
    }

    pub fn set_dark(&mut self, dark: bool) -> Result<(), String> {
        // Always store the preference — exclusive() can no-op while a redraw is in flight,
        // which previously left the GPU paper black under a light WinUI shell.
        if self.dark != dark && self.session.doc().is_ok_and(has_traces) {
            // Traces are faded towards the background colour, so the list must be rebuilt.
            self.frame.list = None;
        }
        self.dark = dark;
        self.exclusive(|host| host.present_if_ready())
    }

    pub fn set_ucs(&mut self, on: bool) -> Result<(), String> {
        self.exclusive(|host| {
            host.show_ucs = on;
            host.present_if_ready()
        })
    }

    /// Hide the child drawing window so a XAML page can use that cell. `pump` will not raise it
    /// until it is shown again.
    pub fn set_canvas_visible(&mut self, visible: bool) -> Result<(), String> {
        self.canvas_visible = visible;
        #[cfg(windows)]
        if !visible && self.hwnd != 0 {
            window::hide(self.hwnd);
        }
        Ok(())
    }

    /// Listen on `127.0.0.1`. Port `0` picks a free port. A second call returns the port already bound.
    pub fn listen(&mut self, port: u16) -> Result<u16, String> {
        if self.control_port != 0 {
            return Ok(self.control_port);
        }
        let (bound, rx, stop) = control::start(port)?;
        self.control_port = bound;
        self.control_rx = Some(rx);
        self.control_stop = Some(stop);
        Ok(bound)
    }

    pub fn assistant_reply(&mut self, accept: bool) -> Result<(), String> {
        self.exclusive(|host| {
            if !host.assistant.confirming() {
                return Err("the assistant is not waiting".into());
            }
            host.assistant.confirm(&mut host.session, accept);
            host.present_if_ready()
        })
    }

    fn ask_assistant(&mut self, text: &str) -> Result<(), String> {
        self.assistant.ask(&mut self.session, text)?;
        self.present_if_ready()
    }

    fn ucs_hit(&self, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let scale = f64::from(self.pixels_per_point.max(1.0));
        let x0 = 16.0 * scale;
        let x1 = 130.0 * scale;
        let y1 = f64::from(self.height) - 8.0 * scale;
        let y0 = y1 - 96.0 * scale;
        x >= x0 && x <= x1 && y >= y0 && y <= y1
    }

    fn drain_control(&mut self) {
        self.poll_control_io();
        let Some(rx) = &self.control_rx else { return };
        let jobs: Vec<control::Job> = std::iter::from_fn(|| rx.try_recv().ok()).take(32).collect();
        for job in jobs {
            if !job.begin() {
                continue;
            }
            if matches!(job.method.as_str(), "app.open" | "app.save") {
                // queue_control_io replies itself on every path that does not start the worker.
                let _ = self.queue_control_io(job);
                continue;
            }
            let value = self.control_one(&job.method, &job.params);
            let _ = job.reply.send(value);
        }
    }

    /// Start a worker open/save and hold the control reply until [`Self::poll_control_io`].
    /// Returns `Some(job)` only when the job was not stored (caller must reply) — unused; always replies or stores.
    fn queue_control_io(&mut self, job: control::Job) -> Option<()> {
        if self.control_io.is_some() {
            let _ = job.reply.send(json_err("a file operation is already in progress"));
            return None;
        }
        match job.method.as_str() {
            "app.open" => {
                let Some(path) = job.params.get("path").and_then(Value::as_str) else {
                    let _ = job.reply.send(json_err("missing path"));
                    return None;
                };
                let Some(on_changed) = aad_engine::nativefile::OnChanged::parse(job.params.get("onChanged").and_then(Value::as_str).unwrap_or(""))
                else {
                    let _ = job.reply.send(json_err("`onChanged` is auto, reimport or keep"));
                    return None;
                };
                if let Err(e) = self.open_start_with(path, on_changed) {
                    let _ = job.reply.send(json_err(e));
                    return None;
                }
                self.control_io = Some((job, ControlIoKind::Open));
            }
            "app.save" => {
                let path = job.params.get("path").and_then(Value::as_str);
                if let Err(e) = self.save_start(path) {
                    let _ = job.reply.send(json_err(e));
                    return None;
                }
                self.control_io = Some((job, ControlIoKind::Save));
            }
            _ => {
                let _ = job.reply.send(json_err("internal: not a file method"));
            }
        }
        None
    }

    fn poll_control_io(&mut self) {
        let Some(kind) = self.control_io.as_ref().map(|(_, k)| *k) else {
            return;
        };
        let status = match kind {
            ControlIoKind::Open => self.open_poll(),
            ControlIoKind::Save => self.save_poll(),
        };
        match status {
            Ok(1) => {}
            Ok(2) => {
                if let Some((job, _)) = self.control_io.take() {
                    let result = match kind {
                        ControlIoKind::Open => serde_json::from_str(&self.open_info).unwrap_or(Value::Null),
                        ControlIoKind::Save => Value::Null,
                    };
                    let _ = job.reply.send(json_ok(result));
                }
            }
            Ok(_) => {
                if let Some((job, _)) = self.control_io.take() {
                    let _ = job.reply.send(json_err("file operation ended unexpectedly"));
                }
            }
            Err(e) => {
                if let Some((job, _)) = self.control_io.take() {
                    let _ = job.reply.send(json_err(e));
                }
            }
        }
    }

    fn control_one(&mut self, method: &str, params: &Value) -> Value {
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or("");
        match method {
            "engine.execute" | "command" => {
                let Some(id) = params.get("command").and_then(Value::as_str).or_else(|| params.get("id").and_then(Value::as_str)) else {
                    return json_err("missing `command`");
                };
                let body = params.get("params").cloned().unwrap_or(serde_json::json!({}));
                let body = if body.is_null() { serde_json::json!({}) } else { body };
                match self.execute(id, &body) {
                    Ok(value) => json_ok(value),
                    Err(e) => json_err(e),
                }
            }
            "cmdline.input" => {
                let before = self.session.log.len();
                let error = self.submit(text("text")).err();
                json_ok(self.cmdline_state(before, error))
            }
            "cmdline.script" => {
                let before = self.session.log.len();
                match self.session.script(text("text")) {
                    Ok(()) => json_ok(serde_json::json!({"output": log_since(&self.session, before), "state": self.cmdline_state(before, None)})),
                    Err(e) => json_err(e.to_string()),
                }
            }
            "cmdline.key" => {
                let before = self.session.log.len();
                let error = match text("key").to_ascii_lowercase().as_str() {
                    "escape" | "esc" => self.cancel_command().err(),
                    _ => self.session.input(aad_engine::Input::Enter).err().map(|e| e.to_string()),
                };
                if let Some(message) = &error {
                    self.session.echo(message.clone());
                }
                json_ok(self.cmdline_state(before, error))
            }
            "cmdline.state" => json_ok(self.cmdline_state(self.session.log.len(), None)),
            "engine.commands" => {
                let list: Vec<Value> = aad_engine::command_specs()
                    .iter()
                    .map(|spec| serde_json::to_value(spec.info(&self.session)).unwrap_or(Value::Null))
                    .filter(|value| !value.is_null())
                    .collect();
                json_ok(Value::Array(list))
            }
            "drawing.inspect" | "document.inspect" => match self.execute("drawing.inspect", params) {
                Ok(value) => json_ok(value),
                Err(e) => json_err(e),
            },
            "ui.menu.invoke" => {
                let Some(id) = params.get("command").and_then(Value::as_str).or_else(|| params.get("id").and_then(Value::as_str)) else {
                    return json_err("missing `command`");
                };
                let before = self.session.log.len();
                let error = self.submit(id).err();
                json_ok(self.cmdline_state(before, error))
            }
            "ui.menu.list" => self.control_menu_list(),
            "ui.inspect" => self.control_inspect(),
            "ui.pointer" => self.control_pointer(params),
            "ui.screenshot" => self.control_screenshot(params),
            "app.open" => {
                // Direct callers (tests): worker + poll without holding the paint loop via the queue.
                let Some(path) = params.get("path").and_then(Value::as_str) else {
                    return json_err("missing path");
                };
                let Some(on_changed) = aad_engine::nativefile::OnChanged::parse(text("onChanged")) else {
                    return json_err("`onChanged` is auto, reimport or keep");
                };
                if let Err(e) = self.open_start_with(path, on_changed) {
                    return json_err(e);
                }
                loop {
                    match self.open_poll() {
                        Ok(1) => std::thread::sleep(std::time::Duration::from_millis(5)),
                        Ok(2) => return json_ok(serde_json::from_str(&self.open_info).unwrap_or(Value::Null)),
                        Ok(_) => return json_err("open ended unexpectedly"),
                        Err(e) => return json_err(e),
                    }
                }
            }
            "app.save" => {
                let path = params.get("path").and_then(Value::as_str);
                if let Err(e) = self.save_start(path) {
                    return json_err(e);
                }
                loop {
                    match self.save_poll() {
                        Ok(1) => std::thread::sleep(std::time::Duration::from_millis(5)),
                        Ok(2) => return json_ok(Value::Null),
                        Ok(_) => return json_err("save ended unexpectedly"),
                        Err(e) => return json_err(e),
                    }
                }
            }
            "app.quit" => {
                // Programmatic calls never open dialogs: quit without the unsaved-changes prompt.
                self.quit_requested = true;
                json_ok(Value::Null)
            }
            "ui.render" => self.control_render(params),
            m if is_app_method(m) => match self.app(m, params) {
                Ok(value) => json_ok(value),
                Err(e) => json_err(e),
            },
            other if other.starts_with("ui.") => {
                json_err(format!("{other} is egui chrome automation; use --egui, or ui.pointer / ui.render / ui.screenshot on WinUI"))
            }
            _ => json_err("unknown method"),
        }
    }

    fn control_menu_list(&self) -> Value {
        match serde_json::from_str::<Value>(&self.menu_json()) {
            Ok(Value::Array(items)) => {
                let menus: Vec<Value> = items.into_iter().map(|node| menu_node_for_control(&node)).collect();
                json_ok(Value::Array(menus))
            }
            Ok(_) | Err(_) => json_err("menu tree is unavailable"),
        }
    }

    fn control_inspect(&self) -> Value {
        let view = self.session.state().map(|state| state.view()).unwrap_or(aad_engine::View { center: Vec2::ZERO, height: 1.0 });
        let settings = &self.session.settings;
        let (rings, mesh_bytes) = self.frame.mesh_totals();
        let prims = self.frame.list.as_ref().map(|l| l.prims.len()).unwrap_or(0);
        json_ok(serde_json::json!({
            "shell": "winui",
            "canvas": { "width": self.width, "height": self.height, "visible": self.canvas_visible },
            "view": { "center": [view.center.x, view.center.y], "height": view.height },
            "cursor": [self.aim.x, self.aim.y],
            "dark": self.dark,
            "ucs": self.show_ucs,
            "osnap": settings.osmode != 0,
            "ortho": settings.orthomode,
            "polar": settings.polarmode,
            "dynmode": settings.dynmode,
            "controlPort": self.control_port,
            "assistant": self.assistant.phase().to_string(),
            "closePrompt": Value::Null,
            "focus": Value::Null,
            "perf": {
                "buildMs": self.frame.list_build_ms,
                "meshMs": self.frame.mesh_ms,
                "prims": prims,
                "rings": rings,
                "meshBytes": mesh_bytes,
                "renderer": "gpu",
            },
        }))
    }

    fn control_pointer(&mut self, params: &Value) -> Value {
        let Some(x) = params.get("x").and_then(Value::as_f64) else {
            return json_err("missing x/y");
        };
        let Some(y) = params.get("y").and_then(Value::as_f64) else {
            return json_err("missing x/y");
        };
        if !x.is_finite() || !y.is_finite() {
            return json_err("x/y must be finite");
        }
        let world = if params.get("space").and_then(Value::as_str).unwrap_or("world") == "screen" {
            match self.screen_to_world(x, y) {
                Ok(point) => point,
                Err(e) => return json_err(e),
            }
        } else {
            Vec2::new(x, y)
        };
        self.aim = world;
        self.session.cursor = world;
        self.session.cursor_deferred = None;
        let action = params.get("action").and_then(Value::as_str).unwrap_or("click");
        if action == "move" {
            return json_ok(serde_json::json!({ "world": [world.x, world.y] }));
        }
        let shift = params.get("shift").and_then(Value::as_bool).unwrap_or(false);
        let before = self.session.log.len();
        let result = match params.get("button").and_then(Value::as_str).unwrap_or("left") {
            "right" => self.session.input(aad_engine::Input::Enter),
            _ if self.session.running.is_some() => self.session.input(aad_engine::Input::Point(world)),
            _ => self.session.idle_click(world, shift),
        };
        match result {
            Ok(()) => {
                let _ = self.present_if_ready();
                json_ok(serde_json::json!({
                    "world": [world.x, world.y],
                    "state": self.cmdline_state(before, None),
                }))
            }
            Err(e) => json_err(e.to_string()),
        }
    }

    /// Drawing PNG (not WinUI chrome). Same raster path as `ui.render`; default path is the temp dir.
    fn control_screenshot(&self, params: &Value) -> Value {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| std::env::temp_dir().join("aad-shot.png").to_string_lossy().into_owned());
        let mut body = params.clone();
        if let Some(object) = body.as_object_mut() {
            object.insert("path".into(), Value::String(path.clone()));
            object.entry("width".to_string()).or_insert_with(|| Value::from(self.width.max(16)));
            object.entry("height".to_string()).or_insert_with(|| Value::from(self.height.max(16)));
            object.entry("fit".to_string()).or_insert(Value::Bool(false));
        }
        let rendered = self.control_render(&body);
        if rendered.get("ok") != Some(&Value::Bool(true)) {
            return rendered;
        }
        let result = rendered.get("result").cloned().unwrap_or(Value::Null);
        json_ok(serde_json::json!({
            "path": result.get("path").cloned().unwrap_or(Value::String(path)),
            "width": result.get("width").cloned().unwrap_or(Value::from(self.width)),
            "height": result.get("height").cloned().unwrap_or(Value::from(self.height)),
            "drawing": true,
        }))
    }

    /// Headless PNG of the drawing (same contract as egui `ui.render`). No window chrome.
    fn control_render(&self, params: &Value) -> Value {
        let width = params.get("width").and_then(Value::as_f64).unwrap_or(1600.0).clamp(16.0, 8192.0) as u32;
        let height = params.get("height").and_then(Value::as_f64).unwrap_or(1000.0).clamp(16.0, 8192.0) as u32;
        let fit = params.get("fit").and_then(Value::as_bool).unwrap_or(true);
        let Ok(state) = self.session.state() else {
            return json_err("no drawing");
        };
        let list = aad_render::build(&state.doc, &state.space, &aad_render::Options::default());
        let view = state.view();
        let raster = if fit {
            aad_render::raster::View::fit(&list.bounds, width, height, 0.05)
        } else {
            aad_render::raster::View { center: view.center, scale: f64::from(height) / view.height.max(1e-12), width, height }
        };
        let Some(png) = aad_render::raster::render_png(&list, &raster, &aad_render::raster::RasterOptions::default()) else {
            return json_err("render failed");
        };
        if let Some(path) = params.get("path").and_then(Value::as_str) {
            if let Err(e) = std::fs::write(path, &png) {
                return json_err(e.to_string());
            }
            return json_ok(serde_json::json!({
                "pngBase64": aad_engine::cmd::file::base64_encode(&png),
                "width": width,
                "height": height,
                "bytes": png.len(),
                "path": path,
            }));
        }
        json_ok(serde_json::json!({
            "pngBase64": aad_engine::cmd::file::base64_encode(&png),
            "width": width,
            "height": height,
            "bytes": png.len(),
        }))
    }

    fn cmdline_state(&self, before: usize, error: Option<String>) -> Value {
        let prompt = self.session.current_prompt();
        serde_json::json!({
            "prompt": self.session.prompt_text(),
            "running": self.session.running.as_ref().map(|running| running.id.clone()),
            "keywords": prompt.as_ref().map(|prompt| prompt.keywords.clone()).unwrap_or_default(),
            "accept": prompt.as_ref().map(|prompt| serde_json::to_value(prompt.accept).unwrap_or(Value::Null)).unwrap_or(Value::Null),
            "buffer": self.draft,
            "history": self.session.log.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev().cloned().collect::<Vec<_>>(),
            "output": log_since(&self.session, before),
            "error": error,
        })
    }

    fn wheel(&mut self, ev: Pointer) -> Result<(), String> {
        if ev.wheel == 0.0 {
            return Ok(());
        }
        let about = self.screen_to_world(ev.x, ev.y)?;
        // One notch is 120 wheel units. Divisor 600 → ~1.22× per notch (was 300 → ~1.49×).
        let factor = (f64::from(ev.wheel) * 120.0 / 600.0).exp();
        let now = std::time::Instant::now();
        let from = match &self.zoom_anim {
            Some(anim) => anim.sample(now),
            None => self.session.state().map_err(|e| e.to_string())?.view(),
        };
        let target = zoomed_view(from, factor, about);
        self.zoom_anim = Some(ZoomAnim { from, target, t0: now });
        self.fitted = None;
        // Unit tests and hosts without a GPU pump loop need the final view immediately.
        #[cfg(windows)]
        let live = self.gpu.is_some();
        #[cfg(not(windows))]
        let live = false;
        if live {
            self.apply_zoom_sample(now)?;
            #[cfg(windows)]
            {
                self.paint_dirty = true;
            }
        } else {
            self.finish_zoom()?;
        }
        Ok(())
    }

    fn finish_zoom(&mut self) -> Result<(), String> {
        if let Some(anim) = self.zoom_anim.take() {
            self.session.state_mut().map_err(|e| e.to_string())?.set_view_quiet(anim.target);
        }
        Ok(())
    }

    fn apply_zoom_sample(&mut self, now: std::time::Instant) -> Result<(), String> {
        let Some(anim) = &self.zoom_anim else {
            return Ok(());
        };
        let view = anim.sample(now);
        let done = now.duration_since(anim.t0).as_millis() >= ZOOM_ANIM_MS;
        self.session.state_mut().map_err(|e| e.to_string())?.set_view_quiet(view);
        if done {
            self.zoom_anim = None;
        }
        Ok(())
    }

    /// Advance an in-flight wheel zoom; keep presenting while it runs.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tick_zoom(&mut self) -> Result<bool, String> {
        if self.zoom_anim.is_none() {
            return Ok(false);
        }
        self.apply_zoom_sample(std::time::Instant::now())?;
        Ok(self.zoom_anim.is_some())
    }

    fn left_down(&mut self, ev: Pointer) -> Result<(), String> {
        self.place_cursor(ev.x, ev.y, ev.mods & MOD_SHIFT != 0)?;
        let shift = ev.mods & MOD_SHIFT != 0;
        if self.session.running.is_some() {
            self.hot = None;
            self.dragged = false;
            self.press = Some(Vec2::new(ev.x, ev.y));
            let input = match self.snap.filter(|hit| hit.deferred.is_some()) {
                Some(hit) => hit.input(),
                None => aad_engine::Input::Point(self.aim),
            };
            return self.session.input(input).map_err(|e| e.to_string());
        }
        if let Some(grip) = self.hot.take() {
            return self.session.grip_edit(grip.handle, grip.index, self.aim, grip.mode).map_err(|e| e.to_string());
        }
        if let Some(grip) = self.grip_at(ev.x, ev.y) {
            self.session.echo(grip.label().to_string());
            self.hot = Some(grip);
            return Ok(());
        }
        if self.show_ucs && self.ucs_hit(ev.x, ev.y) {
            let (x, y) = self.screen_point(ev.x, ev.y);
            self.menu_request = chrome::ucs_popup(x, y);
            return Ok(());
        }
        self.dragged = false;
        self.press = Some(Vec2::new(ev.x, ev.y));
        let world = self.raw.unwrap_or(self.aim);
        self.session.idle_click(world, shift).map_err(|e| e.to_string())
    }

    fn left_up(&mut self, ev: Pointer) -> Result<(), String> {
        let dragged = self.dragged;
        self.press = None;
        self.dragged = false;
        if !dragged || self.session.pending_window.is_none() {
            return Ok(());
        }
        let world = self.screen_to_world(ev.x, ev.y)?;
        let shift = ev.mods & MOD_SHIFT != 0;
        if self.session.running.is_some() {
            self.session.input(aad_engine::Input::Point(world)).map_err(|e| e.to_string())
        } else {
            self.session.idle_click(world, shift).map_err(|e| e.to_string())
        }
    }

    fn moved(&mut self, ev: Pointer) -> Result<(), String> {
        self.place_cursor(ev.x, ev.y, ev.mods & MOD_SHIFT != 0)?;
        if self.hot.is_some() {
            return Ok(());
        }
        if self.panning {
            let Some(last) = self.press else { return Ok(()) };
            self.press = Some(Vec2::new(ev.x, ev.y));
            return self.pan_by(ev.x - last.x, ev.y - last.y);
        }
        let Some(origin) = self.press else { return Ok(()) };
        let dx = ev.x - origin.x;
        let dy = ev.y - origin.y;
        if !self.dragged && dx * dx + dy * dy > 16.0 {
            self.dragged = true;
            if self.session.pending_window.is_none() {
                let world = self.screen_to_world(origin.x, origin.y)?;
                self.session.begin_window(world);
            }
        }
        Ok(())
    }

    fn left_double(&mut self, ev: Pointer) -> Result<(), String> {
        if self.session.running.is_some() {
            return Ok(());
        }
        let (on_paper, paper, inside, had_viewport) = {
            let state = self.session.state().map_err(|e| e.to_string())?;
            let on_paper = matches!(state.space, aad_engine::doc::Space::Paper(_));
            let paper = self.screen_on(ev.x, ev.y, state.paper_view());
            let viewport = state.active_viewport();
            let inside = viewport
                .as_ref()
                .is_some_and(|(_, view)| (paper.x - view.center.x).abs() <= view.width / 2.0 && (paper.y - view.center.y).abs() <= view.height / 2.0);
            (on_paper, paper, inside, viewport.is_some())
        };
        if on_paper && !inside {
            self.hot = None;
            let at = serde_json::json!({ "at": [paper.x, paper.y] });
            if self.session.execute("mspace", &at).is_err() && had_viewport {
                let _ = self.session.execute("pspace", &serde_json::json!({}));
            }
            return Ok(());
        }
        let world = self.screen_to_world(ev.x, ev.y)?;
        self.edit_text_at(world)
    }

    fn edit_text_at(&mut self, world: Vec2) -> Result<(), String> {
        let editable = self.session.state().ok().and_then(|state| match state.selection.as_slice() {
            [handle] => state.doc.entity(*handle).map(|entity| {
                matches!(
                    entity.kind,
                    aad_engine::doc::EntityKind::Text(_)
                        | aad_engine::doc::EntityKind::MText(_)
                        | aad_engine::doc::EntityKind::AttDef(_)
                        | aad_engine::doc::EntityKind::Dimension(_)
                )
            }),
            _ => None,
        });
        if editable != Some(true) {
            return Ok(());
        }
        self.session.start("textedit").map_err(|e| e.to_string())?;
        self.session.input(aad_engine::Input::Point(world)).map_err(|e| e.to_string())
    }

    fn middle_double(&mut self) -> Result<(), String> {
        self.panning = false;
        self.press = None;
        self.session.zoom_extents().map_err(|e| e.to_string())
    }

    fn screen_on(&self, x: f64, y: f64, view: aad_engine::View) -> Vec2 {
        let height = f64::from(self.height.max(1));
        let scale = height / view.height.max(1e-12);
        let cx = f64::from(self.width.max(1)) * 0.5;
        let cy = height * 0.5;
        Vec2::new(view.center.x + (x - cx) / scale, view.center.y - (y - cy) / scale)
    }

    fn right_click(&mut self, ev: Pointer) -> Result<(), String> {
        let snap = ev.mods & (MOD_SHIFT | MOD_CTRL) != 0;
        if self.session.running.is_some() && !snap {
            self.menu_request.clear();
            return self.session.input(aad_engine::Input::Enter).map_err(|e| e.to_string());
        }
        let (x, y) = self.screen_point(ev.x, ev.y);
        self.menu_request = chrome::popup_json(&self.session, snap, x, y);
        Ok(())
    }

    fn screen_point(&self, x: f64, y: f64) -> (i32, i32) {
        #[cfg(windows)]
        {
            window::client_to_screen(self.hwnd, x, y)
        }
        #[cfg(not(windows))]
        {
            let _ = self;
            (i32::try_from(x as i64).unwrap_or(0), i32::try_from(y as i64).unwrap_or(0))
        }
    }

    /// The pending right-click menu, then none.
    pub fn take_popup(&mut self) -> String {
        std::mem::take(&mut self.menu_request)
    }

    /// JSON for a finished `plot.window` / `plotl` / `plotp` request. Empty when nothing is waiting.
    /// `window` null means Extents; `layout` is the nameplate; `preview` asks for on-screen PDF.
    pub fn plot_request_json(&self) -> String {
        match &self.session.plot_request {
            None => String::new(),
            Some(req) => serde_json::json!({
                "window": req.window,
                "layout": req.layout,
                "preview": req.preview,
            })
            .to_string(),
        }
    }

    /// Clear a finished `plot.window` request after the shell has copied it.
    pub fn clear_plot_request(&mut self) {
        self.session.plot_request = None;
    }

    /// The drawing clipboard as DXF text, or empty when there is nothing to put on the system clipboard.
    pub fn clipboard_text(&self) -> String {
        aad_engine::cmd::clipboard::system_text(&self.session).unwrap_or_default()
    }

    /// Replace the drawing clipboard with DXF text from the system clipboard.
    pub fn load_clipboard(&mut self, text: &str) -> bool {
        if text.len() > 8 * 1_024 * 1_024 {
            return false;
        }
        aad_engine::cmd::clipboard::load_system_text(&mut self.session, text)
    }

    fn pan_by(&mut self, dx: f64, dy: f64) -> Result<(), String> {
        let state = self.session.state_mut().map_err(|e| e.to_string())?;
        let view = state.view();
        let scale = f64::from(self.height.max(1)) / view.height.max(1e-12);
        state.set_view_quiet(aad_engine::View { center: view.center - Vec2::new(dx / scale, -dy / scale), height: view.height });
        Ok(())
    }

    pub fn stats(&self) -> Stats {
        let entities = self.session.doc().map(|d| d.entity_count()).unwrap_or(0);
        let view_height = self.session.state().map(|s| s.view().height).unwrap_or(0.0);
        Stats { entities: u32::try_from(entities).unwrap_or(u32::MAX), _pad: 0, view_height, width: self.width, height: self.height }
    }

    fn place_cursor(&mut self, x: f64, y: f64, shift: bool) -> Result<(), String> {
        let raw = self.screen_to_world(x, y)?;
        self.raw = Some(raw);
        self.screen = Some(Vec2::new(x, y));
        let prompt = self.session.current_prompt();
        let wants = self.hot.is_some() || prompt.as_ref().is_some_and(|p| p.accept.point && !p.accept.select);
        let scale = self.pixels_per_unit();
        self.update_hover(raw, scale);
        if !wants || self.session.pending_window.is_some() {
            if !wants {
                self.session.tracking.clear();
            }
            self.aim = raw;
            self.snap = None;
            self.track = None;
            self.session.cursor = raw;
            self.session.cursor_deferred = None;
            return Ok(());
        }
        let base = self.hot.map(|grip| grip.base).or_else(|| prompt.as_ref().and_then(|p| p.base));
        let deferred = self.hot.is_none() && prompt.as_ref().is_some_and(|p| p.deferred);
        let query = aad_engine::snap::tracking::CursorQuery {
            base,
            deferred,
            aperture: self.session.settings.aperture / scale,
            tol: 6.0 / scale,
            now: self.clock.elapsed().as_secs_f64(),
            shift,
        };
        let snapped = self.session.snap_cursor(raw, &query);
        self.aim = snapped.point;
        self.snap = snapped.snap;
        self.track = snapped.track;
        self.apply_snap_cycle(raw, self.session.settings.aperture / scale);
        if let Some(frame) = self.dyn_frame() {
            let drawn = self.session.doc().ok().and_then(aad_engine::annot::drawing_unit);
            if let Some(entry) = aad_engine::units::with_drawn_unit(drawn, || aad_engine::dyninput::parse(&self.draft, &frame)) {
                self.aim = aad_engine::dyninput::constrain(&entry, &frame, self.aim);
            }
        }
        self.session.cursor = self.aim;
        self.session.cursor_deferred = self.snap.and_then(|hit| hit.deferred);
        Ok(())
    }

    fn update_hover(&mut self, raw: Vec2, scale: f64) {
        if !self.session.picking_objects() || self.session.pending_window.is_some() || self.hot.is_some() {
            self.hover = None;
            return;
        }
        let aperture = self.session.settings.pickbox.max(1.0) * 1.5 / scale.max(1e-12);
        self.hover = self.session.state().ok().and_then(|st| aad_engine::select::pick(&st.doc, &st.edit_space(), raw, aperture));
    }

    /// Enter or Space while a grip is hot. An empty line is left for [`Self::grip_cycle`].
    /// A point (`x,y`, `@dx,dy`, `@d<a`) applies the grip and clears it. Anything else is reported
    /// and still consumed, so it is not run as a command.
    pub fn grip_point(&mut self, text: &str) -> bool {
        if self.hot.is_none() {
            return false;
        }
        if text.len() > 1_048_576 {
            self.session.echo("text is too long".to_string());
            let _ = self.present_if_ready();
            return true;
        }
        let typed = text.trim();
        if typed.is_empty() {
            return false;
        }
        let Some(grip) = self.hot else { return false };
        match aad_engine::prompt::parse_point_with(typed, grip.base, &self.session.angle_settings()) {
            Some(point) => {
                self.hot = None;
                if let Err(error) = self.session.grip_edit(grip.handle, grip.index, point, grip.mode) {
                    self.session.echo(error.to_string());
                }
            }
            None => self.session.echo("Requires a point (x,y, @dx,dy or @d<a)."),
        }
        let _ = self.present_if_ready();
        true
    }

    /// Space or Enter with nothing typed: the hot grip's next mode (stretch, move, rotate, scale, mirror).
    pub fn grip_cycle(&mut self) -> bool {
        let label = {
            let Some(grip) = self.hot.as_mut() else { return false };
            grip.next_mode();
            grip.label().to_string()
        };
        self.session.echo(label);
        let _ = self.present_if_ready();
        true
    }

    /// Move the drafting cursor by arrow keys. `dx`/`dy` are steps on screen (positive `dy` is down).
    /// Ctrl pans the view instead. Shift uses a larger step.
    pub fn key_nudge(&mut self, dx: i32, dy: i32, ctrl: bool, shift: bool) -> Result<(), String> {
        self.exclusive(|host| {
            let dx = dx.clamp(-64, 64);
            let dy = dy.clamp(-64, 64);
            let step = if shift { 40.0 } else { 8.0 };
            let sx = f64::from(dx) * step;
            let sy = f64::from(dy) * step;
            if ctrl {
                host.pan_by(sx, sy)?;
                if let Some(screen) = host.screen {
                    host.place_cursor(screen.x, screen.y, false)?;
                }
                return host.present_if_ready();
            }
            let screen = host.screen.unwrap_or_else(|| Vec2::new(f64::from(host.width.max(1)) * 0.5, f64::from(host.height.max(1)) * 0.5));
            let max_x = f64::from(host.width.max(1));
            let max_y = f64::from(host.height.max(1));
            let x = (screen.x + sx).clamp(0.0, max_x);
            let y = (screen.y + sy).clamp(0.0, max_y);
            host.place_cursor(x, y, shift)?;
            host.present_if_ready()
        })
    }

    /// Place a point, pick or grip, or close a selection window at the keyboard cursor (Insert).
    pub fn key_activate(&mut self, shift: bool) -> Result<(), String> {
        self.exclusive(|host| {
            let screen = match host.screen {
                Some(point) if point.is_finite() => point,
                _ => host
                    .world_to_screen(host.aim)
                    .unwrap_or_else(|_| Vec2::new(f64::from(host.width.max(1)) * 0.5, f64::from(host.height.max(1)) * 0.5)),
            };
            if !screen.x.is_finite() || !screen.y.is_finite() {
                return Err("cursor is not on the drawing".into());
            }
            let mods = if shift { MOD_SHIFT } else { 0 };
            host.left_down(Pointer { x: screen.x, y: screen.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods })?;
            host.present_if_ready()
        })
    }

    /// Tab with nothing for Dynamic Input to split: the next object snap under the cursor.
    pub fn snap_tab(&mut self) -> Result<(), String> {
        let Some(raw) = self.raw else { return Ok(()) };
        let aperture = self.session.settings.aperture / self.pixels_per_unit();
        let (anchor, index) = self.snap_cycle.unwrap_or((raw, 0));
        let anchor = if anchor.dist(raw) > aperture { raw } else { anchor };
        self.snap_cycle = Some((anchor, index.saturating_add(1)));
        if let Some(screen) = self.screen {
            self.place_cursor(screen.x, screen.y, false)?;
        }
        self.present_if_ready()
    }

    fn apply_snap_cycle(&mut self, raw: Vec2, aperture: f64) {
        let Some((anchor, index)) = self.snap_cycle else { return };
        if !aperture.is_finite() || anchor.dist(raw) > aperture {
            self.snap_cycle = None;
            return;
        }
        let Some(state) = self.session.state().ok() else { return };
        let prompt = self.session.current_prompt();
        let candidates = aad_engine::pointmod::snap_candidates(
            &state.doc,
            &state.edit_space(),
            raw,
            aperture,
            self.session.settings.osmode,
            prompt.as_ref().and_then(|prompt| prompt.base),
            prompt.as_ref().is_some_and(|prompt| prompt.deferred),
        );
        let Some(hit) = candidates.get(index % candidates.len().max(1)).copied() else { return };
        self.aim = hit.point;
        self.snap = Some(hit);
    }

    /// The dialog description for `id`, when the command has settings to ask before any points.
    pub fn form_json(&self, id: &str) -> Option<String> {
        forms::form_json(&self.session, id)
    }

    /// Start `id` with the dialog's answers. Points still missing are picked on the drawing.
    pub fn start_form(&mut self, id: &str, preset: &Value) -> Result<(), String> {
        self.session.start_form(id, preset.clone()).map_err(|e| e.to_string())?;
        self.draft.clear();
        self.present_if_ready()
    }

    /// The Dynamic Input frame, when the value boxes are live.
    fn dyn_frame(&self) -> Option<aad_engine::dyninput::Frame> {
        if !self.session.settings.dynmode || self.hot.is_some() || self.session.pending_window.is_some() {
            return None;
        }
        let prompt = self.session.current_prompt().filter(|prompt| prompt.accept.point && !prompt.accept.select)?;
        Some(aad_engine::dyninput::Frame {
            base: prompt.base,
            cartesian: self.session.settings.dynpi_cartesian,
            absolute: self.session.settings.dynpi_absolute,
        })
    }

    /// A split entry (`40,30`, `40<90`) is completed from the cursor before it is submitted.
    fn finish_dyn(&self, text: &str) -> String {
        let Some(frame) = self.dyn_frame() else { return text.to_string() };
        let drawn = self.session.doc().ok().and_then(aad_engine::annot::drawing_unit);
        aad_engine::units::with_drawn_unit(drawn, || {
            aad_engine::dyninput::parse(text, &frame)
                .and_then(|entry| aad_engine::dyninput::complete(&entry, &frame, self.aim, self.session.last_point))
                .unwrap_or_else(|| text.to_string())
        })
    }

    /// The line drawn beside the cursor while a command is running.
    /// The WinUI shell draws these fields itself; this string is what the overlay test strokes.
    #[cfg(test)]
    fn dyn_tip(&self) -> Option<String> {
        if self.screen.is_none() || self.session.pending_window.is_some() {
            return None;
        }
        if let Some(hot) = self.hot {
            return Some(hot.label().to_string());
        }
        if !self.session.settings.dynmode {
            return None;
        }
        let prompt = self.session.current_prompt()?;
        Some(dyn_line(&prompt, self.dyn_frame().as_ref(), &self.draft, self.aim))
    }

    fn grip_at(&self, x: f64, y: f64) -> Option<overlay::HotGrip> {
        if self.session.running.is_some() {
            return None;
        }
        let state = self.session.state().ok()?;
        let reach = self.session.settings.gripsize.max(1.0) + 3.0;
        for handle in self.session.selection().iter().take(200) {
            let Some(entity) = state.doc.entity(*handle) else { continue };
            if state.doc.layer(&entity.common.layer).is_some_and(|layer| layer.locked) {
                continue;
            }
            for (index, grip) in aad_engine::grips::grips(&state.doc, entity).into_iter().enumerate() {
                let Ok(screen) = self.world_to_screen(grip) else { continue };
                if (screen.x - x).abs() <= reach && (screen.y - y).abs() <= reach {
                    return Some(overlay::HotGrip { handle: *handle, index, base: grip, mode: aad_engine::grips::GripMode::Stretch });
                }
            }
        }
        None
    }

    fn pixels_per_unit(&self) -> f64 {
        let height = self.session.state().map(|state| state.view().height).unwrap_or(1.0);
        f64::from(self.height.max(1)) / height.max(1e-12)
    }

    fn world_to_screen(&self, point: Vec2) -> Result<Vec2, String> {
        let view = self.session.state().map_err(|e| e.to_string())?.view();
        let scale = self.pixels_per_unit();
        let cx = f64::from(self.width.max(1)) * 0.5;
        let cy = f64::from(self.height.max(1)) * 0.5;
        Ok(Vec2::new(cx + (point.x - view.center.x) * scale, cy - (point.y - view.center.y) * scale))
    }

    fn paper(&self) -> Rgb {
        if self.dark { Rgb(0, 0, 0) } else { Rgb(255, 255, 255) }
    }

    fn screen_to_world(&self, x: f64, y: f64) -> Result<Vec2, String> {
        let view = self.session.state().map_err(|e| e.to_string())?.view();
        let height = f64::from(self.height.max(1));
        let scale = height / view.height.max(1e-12);
        let cx = f64::from(self.width.max(1)) * 0.5;
        let cy = height * 0.5;
        Ok(Vec2::new(view.center.x + (x - cx) / scale, view.center.y - (y - cy) / scale))
    }

    /// Ensure the cached display list and mesh match the session; return GPU key, uniforms and origin.
    fn mesh(&mut self) -> Result<(u64, [f32; 12], Vec2), String> {
        let t_pre = perf::start();
        let lw = self.session.settings.lwdisplay;
        let state = self.session.state().map_err(|e| e.to_string())?;
        let lap_state = perf::lap(&t_pre);
        let view = state.view();
        let px = view.height / f64::from(self.height.max(1));
        let band = aad_render::tessellation_band(px);
        let lod = aad_render::lod_flags_for_view(px.max(1e-300), 1.0);
        let on_paper = matches!(state.space, Space::Paper(_));
        let opts = aad_render::Options {
            tolerance: aad_render::tessellation_tolerance(band),
            min_dash: px * 2.0,
            text: lod.text,
            fill: lod.fill,
            text_greek: lod.text_greek,
            fill_tint: lod.fill_tint,
            lineweights: lw,
            view_height: view.height,
            view_center: matches!(state.space, Space::Model).then_some(view.center),
            plot_style_table: None,
            screen_px: px,
        };
        let pan_cell = if matches!(state.space, Space::Model) {
            let (cx, cy) = aad_render::model_pan_cell(view.center, view.height);
            ((cx as i32 as u32 as i64) << 32) | (cy as i32 as u32 as i64)
        } else {
            0
        };
        let lod_bits = lod_pack(lod);
        let list_key: ListKey = (state.uid, state.revision, band, lw, space_fp(&state.space), lod_bits, pan_cell);
        let background = self.paper();
        // Take a finished background rebuild (band, LOD or pan cell) first: the list, its scene and its tiles go in together.
        if let Some(p) = &self.frame.pending {
            match p.rx.try_recv() {
                Ok((list, scene, pre)) => {
                    if let Some(p) = self.frame.pending.take()
                        && (p.key.0, p.key.1, p.key.3, p.key.4) == (list_key.0, list_key.1, list_key.3, list_key.4)
                    {
                        // The replaced list, scene and tiles are large on a big drawing: freeing them is left to a thread.
                        let old =
                            (self.frame.list.replace(list), std::mem::replace(&mut self.frame.scene, scene), std::mem::take(&mut self.frame.tiles));
                        let _ = std::thread::Builder::new().name("aad-free".into()).spawn(move || drop(old));
                        // The scene stays with the list, so an edit after a zoom is still a delta rebuild.
                        self.frame.list_key = p.key;
                        self.frame.list_doc = Some(state.doc.clone());
                        self.frame.invalidate_composed_mesh();
                        self.frame.tile_edge = None;
                        match pre {
                            Some(pre) => {
                                self.frame.tiles = pre.tiles.into_iter().map(|(id, key, mesh)| (id, TileMesh { key, mesh })).collect();
                                self.frame.tiles_total = self.frame.tiles.len();
                                self.frame.tiles_remeshed = 0;
                                self.frame.mesh_tiled = true;
                                self.frame.mesh_basis = Some(pre.basis);
                                self.frame.tile_edge = Some((p.key.2, pre.edge));
                                // Nothing to do on this thread when the view still has the origin the worker meshed for.
                                self.frame.mesh_key = Some(p.key.1 ^ pre.basis);
                                self.frame.mesh = Some(MeshData::default());
                                self.frame.mesh_origin = pre.origin;
                            }
                            None => {
                                self.frame.tiles.clear();
                                self.frame.mesh_basis = None;
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.frame.pending = None,
            }
        }
        // First build of a large drawing: finished on a worker, or still running (nothing to paint yet).
        if let Some(f) = &self.frame.first {
            match f.rx.try_recv() {
                Ok((list, scene)) => {
                    perf::mark("first list ready");
                    if let Some(f) = self.frame.first.take()
                        && f.key.0 == list_key.0
                    {
                        self.frame.clear_mesh();
                        self.frame.list = Some(list);
                        self.frame.scene = scene;
                        self.frame.list_key = f.key;
                        self.frame.list_doc = Some(f.doc);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Err(LOADING.to_string()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.frame.first = None,
            }
        }
        if self.frame.list_key != list_key || self.frame.list.is_none() {
            // The same drawing and revision: only the zoom band, the level of detail or the pan cell changed.
            let same_drawing = self.frame.list.is_some()
                && (self.frame.list_key.0, self.frame.list_key.1, self.frame.list_key.3, self.frame.list_key.4)
                    == (list_key.0, list_key.1, list_key.3, list_key.4);
            // Keep the last list on screen and rebuild on a worker, with its scene and its mesh tiles.
            if same_drawing && self.frame.pending.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let (doc, space, opts) = (state.doc.clone(), state.space.clone(), opts.clone());
                let (center, height) = (view.center, view.height);
                let lw_mode = lw.then_some(if on_paper { mesh::LineweightMode::TrueScale } else { mesh::LineweightMode::Screen });
                if std::thread::Builder::new()
                    .name("aad-display-list".into())
                    .spawn(move || {
                        let (list, scene) = build_list_and_scene(&doc, &space, &opts, background);
                        let pre = premesh(&list, center, height, background, lw_mode, band);
                        let _ = tx.send((list, scene, pre));
                    })
                    .is_ok()
                {
                    self.frame.pending = Some(PendingList { key: list_key, rx });
                }
            } else if !same_drawing {
                let fresh = self.frame.list.is_none() || self.frame.list_key.0 != list_key.0;
                if fresh && self.gpu.is_some() && state.doc.entity_count() >= ASYNC_FIRST_BUILD_ENTITIES {
                    if self.frame.first.as_ref().is_none_or(|f| f.key.0 != list_key.0) {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let (doc, space, opts) = (state.doc.clone(), state.space.clone(), opts.clone());
                        let doc_keep = doc.clone();
                        perf::mark(&format!("first list build started entities={}", doc.entity_count()));
                        if std::thread::Builder::new()
                            .name("aad-first-list".into())
                            .spawn(move || {
                                aad_engine::spatial::warm(doc.clone(), space.clone());
                                let _ = tx.send(build_list_and_scene(&doc, &space, &opts, background));
                            })
                            .is_ok()
                        {
                            self.frame.first = Some(FirstBuild { key: list_key, doc: doc_keep, rx });
                        }
                    }
                    if self.frame.first.is_some() {
                        self.frame.list = None;
                        self.frame.scene = None;
                        self.frame.pending = None;
                        return Err(LOADING.to_string());
                    }
                }
                if perf::start().is_some() {
                    perf::force(&format!("sync list build: old key {:?} new key {:?}", self.frame.list_key, list_key));
                }
                self.frame.pending = None;
                aad_engine::spatial::warm(state.doc.clone(), state.space.clone());
                let t0 = std::time::Instant::now();
                let can_incremental = matches!(state.space, Space::Model)
                    && !has_traces(&state.doc)
                    && self.frame.list_doc.as_ref().is_some_and(|old_doc| !has_traces(old_doc))
                    && self.frame.list_key.0 == list_key.0
                    && self.frame.list_key.2 == list_key.2
                    && self.frame.list_key.3 == list_key.3
                    && self.frame.list_key.5 == list_key.5
                    && self.frame.list_key.6 == list_key.6;
                let (list, scene) = if can_incremental {
                    if let (Some(old_scene), Some(old_doc)) = (&self.frame.scene, &self.frame.list_doc) {
                        if let Some(next) = aad_render::rebuild_scene(old_scene, old_doc, &state.doc, &opts) {
                            let list = aad_render::flatten_scene(&next);
                            (list, Some(next))
                        } else if let (Some(old), Some(old_doc)) = (&self.frame.list, &self.frame.list_doc) {
                            match aad_render::rebuild(old, old_doc, &state.doc, &opts) {
                                Some(list) => (list, None),
                                None => build_list_and_scene(&state.doc, &state.space, &opts, background),
                            }
                        } else {
                            build_list_and_scene(&state.doc, &state.space, &opts, background)
                        }
                    } else if let (Some(old), Some(old_doc)) = (&self.frame.list, &self.frame.list_doc) {
                        match aad_render::rebuild(old, old_doc, &state.doc, &opts) {
                            Some(list) => (list, None),
                            None => build_list_and_scene(&state.doc, &state.space, &opts, background),
                        }
                    } else {
                        build_list_and_scene(&state.doc, &state.space, &opts, background)
                    }
                } else {
                    build_list_and_scene(&state.doc, &state.space, &opts, background)
                };
                self.frame.list = Some(list);
                self.frame.scene = scene;
                self.frame.list_build_ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.frame.list_key = list_key;
                self.frame.list_doc = Some(state.doc.clone());
                // Keep tile meshes: dirty tiles remesh via content fingerprint on the next pack.
                self.frame.invalidate_composed_mesh();
            }
            // same_drawing with pending still building: keep showing the previous list.
            if self.frame.list.is_none() {
                aad_engine::spatial::warm(state.doc.clone(), state.space.clone());
                let t0 = std::time::Instant::now();
                let (list, scene) = build_list_and_scene(&state.doc, &state.space, &opts, background);
                self.frame.list = Some(list);
                self.frame.scene = scene;
                self.frame.list_build_ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.frame.list_key = list_key;
                self.frame.list_doc = Some(state.doc.clone());
                self.frame.invalidate_composed_mesh();
            }
        }
        let lap_list = perf::lap(&t_pre);
        let list = self.frame.list.as_ref().ok_or_else(|| "display list missing".to_string())?;
        let origin = mesh::choose_origin(&list.bounds, view.center, view.height);
        let paper = self.paper();
        let lw_mode = lw.then_some(if on_paper { mesh::LineweightMode::TrueScale } else { mesh::LineweightMode::Screen });
        let true_scale = matches!(lw_mode, Some(mesh::LineweightMode::TrueScale));
        // Origin is snapped to a coarse grid while panning, so the mesh key stays stable across
        // fine pans and the GPU keeps the last upload (uniform carries the view offset). The band is the one of the list on screen, not the
        // one asked for: while a worker builds the next band, the tiles of the list shown stay valid.
        let list_band = self.frame.list_key.2;
        let mesh_basis = mesh_basis_of(origin, paper, list_band, true_scale);
        let mesh_key = state.revision ^ mesh_basis;
        if t_pre.is_some() && (self.frame.mesh_key != Some(mesh_key) || self.frame.mesh.is_none()) {
            perf::force(&format!(
                "mesh() before the mesh step: session.state() {lap_state:.1}, until the display list is settled {lap_list:.1}, until here {:.1} ms",
                perf::lap(&t_pre)
            ));
        }
        if self.frame.mesh_key != Some(mesh_key) || self.frame.mesh.is_none() {
            let t0 = std::time::Instant::now();
            let world_width = view.height * f64::from(self.width.max(1)) / f64::from(self.height.max(1));
            // Big drawings get finer tiles so the tiles can be worked on by every core; if that edge is not usable (a wipeout across tiles, too many
            // tiles) the plain edge is tried. The partition is made once and reused below.
            let t_plan = perf::start();
            // The edge is chosen once per list band (from the list's density), not per view octave, so tile ids stay put while zooming.
            let plan = match self.frame.tile_edge {
                Some((b, edge)) if b == list_band => mesh::tiling_plan(list, edge).map(|p| (edge, p)),
                _ => plan_tiles(list, view.height),
            };
            self.frame.tile_edge = plan.as_ref().map(|(edge, _)| (list_band, *edge));
            let plan = plan.map(|(_, p)| p);
            let lap_plan = perf::lap(&t_plan);
            let (built, remeshed, tiles_total, mesh_tiled) = if let Some(parts) = plan {
                if self.frame.mesh_basis != Some(mesh_basis) {
                    self.frame.tiles.clear();
                }
                let t_mb = perf::start();
                let keep: std::collections::HashSet<TileId> = parts.iter().map(|(id, _)| *id).collect();
                self.frame.tiles.retain(|id, _| keep.contains(id));
                // Tiles are independent: fingerprint them all, then remesh the ones whose content changed, each on its own thread.
                let keys: Vec<u64> = parts.par_iter().map(|(_, indices)| mesh::tile_content_fingerprint(list, indices) ^ mesh_basis).collect();
                let lap_keys = perf::lap(&t_mb);
                let stale: Vec<usize> = parts
                    .iter()
                    .zip(&keys)
                    .enumerate()
                    .filter(|(_, ((id, _), key))| self.frame.tiles.get(id).is_none_or(|t| t.key != **key))
                    .map(|(i, _)| i)
                    .collect();
                let rebuilt: Vec<(usize, MeshData)> = stale
                    .par_iter()
                    .filter_map(|&i| parts.get(i).map(|(_, indices)| (i, mesh::build_mesh_prims(list, indices, origin, paper, lw_mode))))
                    .collect();
                let remeshed = rebuilt.len();
                let lap_remesh = perf::lap(&t_mb);
                for (i, mesh) in rebuilt {
                    if let (Some((id, _)), Some(key)) = (parts.get(i), keys.get(i)) {
                        self.frame.tiles.insert(*id, TileMesh { key: *key, mesh });
                    }
                }
                let tiles_total = parts.len();
                // The tiles are what is painted (`present_tiles`); the view-clipped infinite lines are added at each present. The composed mesh, one copy of
                // every tile joined, was only read by `ui.inspect` and tests, and cost about 20 ms of every zoom step on a big drawing: it is not built.
                let lap_merge = perf::lap(&t_mb);
                if t_mb.is_some() {
                    perf::force(&format!(
                        "mesh rebuild: plan {lap_plan:.1}, fingerprints until {lap_keys:.1}, remesh until {lap_remesh:.1} ({remeshed} of {tiles_total} tiles), no merge {lap_merge:.1} ms"
                    ));
                }
                (MeshData::default(), remeshed, tiles_total, true)
            } else {
                self.frame.tiles.clear();
                let mut mono = mesh::build_mesh_mode(list, origin, paper, lw_mode);
                mesh::append_infinite(&mut mono, list, origin, view.center, world_width, view.height, paper);
                if true_scale {
                    mesh::expand_hairlines_true_scale(&mut mono);
                } else {
                    mesh::expand_hairlines(&mut mono);
                }
                (mono, 1usize, 0usize, false)
            };
            self.frame.tiles_remeshed = remeshed;
            self.frame.tiles_total = tiles_total;
            self.frame.mesh_tiled = mesh_tiled;
            self.frame.mesh_basis = Some(mesh_basis);
            self.frame.mesh_ms = t0.elapsed().as_secs_f64() * 1000.0;
            self.frame.mesh = Some(built);
            self.frame.mesh_key = Some(mesh_key);
            self.frame.mesh_origin = origin;
        }
        let origin = self.frame.mesh_origin;
        let scale = f64::from(self.height.max(1)) / view.height.max(1e-12);
        let uniform = [
            (origin.x - view.center.x) as f32,
            (origin.y - view.center.y) as f32,
            scale as f32,
            0.0,
            f64::from(self.width.max(1)) as f32 * 0.5,
            f64::from(self.height.max(1)) as f32 * 0.5,
            f64::from(self.width.max(1)) as f32,
            f64::from(self.height.max(1)) as f32,
            self.pixels_per_point,
            if true_scale { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ];
        Ok((mesh_key, uniform, origin))
    }

    /// Vertices in the current frame. Used by tests; the window paints the same mesh.
    pub fn mesh_vertices(&mut self) -> Result<usize, String> {
        let _ = self.mesh()?;
        let count = |m: &MeshData| m.line_vertices() + m.tri_vertices() + m.quad_instances() + m.ring_instances();
        if self.frame.mesh_tiled {
            return Ok(self.frame.tiles.values().map(|t| count(&t.mesh)).sum());
        }
        let built = self.frame.mesh.as_ref().ok_or_else(|| "mesh missing".to_string())?;
        Ok(count(built))
    }

    /// Zoom to the extents and remember the view that gave, so a later change of the drawing area
    /// (the window settling, a panel resized) refits it while nobody has navigated.
    fn fit_view(&mut self) {
        let _ = self.session.zoom_extents();
        self.fitted = self.session.state().ok().map(|s| {
            let v = s.view();
            (v.center.x, v.center.y, v.height)
        });
    }

    fn apply_size(&mut self, width: u32, height: u32) {
        let before = (self.width, self.height);
        self.width = width.clamp(1, 8192);
        self.height = height.clamp(1, 8192);
        self.session.viewport_px = (f64::from(self.width), f64::from(self.height));
        // Nobody has navigated since the drawing was fitted: fit it to the new drawing area.
        let untouched = self.session.state().ok().map(|s| s.view()).zip(self.fitted).is_some_and(|(v, f)| (v.center.x, v.center.y, v.height) == f);
        if untouched && (self.width, self.height) != before {
            self.fit_view();
        }
    }
}

#[cfg(windows)]
impl Host {
    /// Create the child drawing window inside `parent` and fit the sample to it.
    pub fn embed(&mut self, parent: isize, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        if self.in_call {
            return Ok(());
        }
        self.in_call = true;
        let result = self.embed_inner(parent, x, y, width, height);
        self.in_call = false;
        result
    }

    fn embed_inner(&mut self, parent: isize, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        if self.hwnd != 0 {
            return self.move_child_inner(x, y, width, height);
        }
        let hwnd = window::create(parent, x, y, width, height, Arc::as_ptr(&self.bridge))?;
        self.hwnd = hwnd;
        self.apply_size(width, height);
        self.fit_view();
        match gpu::Gpu::open(hwnd, self.width, self.height) {
            Ok(gpu) => {
                self.gpu = Some(gpu);
                self.paint = None;
                self.paint_dirty = true;
            }
            Err(e) => {
                window::destroy(hwnd);
                self.hwnd = 0;
                return Err(e);
            }
        }
        self.redraw()
    }

    pub fn move_child(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        if self.in_call {
            return Ok(());
        }
        self.in_call = true;
        let result = self.move_child_inner(x, y, width, height);
        self.in_call = false;
        result
    }

    fn move_child_inner(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        if self.hwnd == 0 {
            return Err("no drawing window".into());
        }
        window::move_to(self.hwnd, x, y, width, height, self.canvas_visible)?;
        self.apply_size(width, height);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.resize(self.width, self.height)?;
        }
        self.paint = None;
        self.paint_dirty = true;
        self.redraw()
    }

    /// Apply queued pointer events and present when the canvas is dirty.
    pub fn pump(&mut self) -> Result<(), String> {
        if self.in_call {
            return Ok(());
        }
        let t_pump = perf::start();
        self.drain_control();
        self.assistant.poll(&mut self.session);
        self.poll_geodb();
        self.autosave.tick(&self.session, autosave_interval());
        let t_ctl = perf::lap(&t_pump);
        self.in_call = true;
        let result = (|| {
            if self.canvas_visible {
                window::raise(self.hwnd);
            }
            let events = self.bridge.drain();
            let had_events = !events.is_empty();
            let n_events = events.len();
            for ev in events {
                let t_ev = perf::start();
                self.pointer(ev)?;
                perf::note(&t_ev, &format!("pump: one pointer event took long (events this pump {n_events})"));
            }
            let t_events = perf::lap(&t_pump);
            let zooming = self.tick_zoom()?;
            let t_zoom = perf::lap(&t_pump);
            // Idle pumps skip the GPU path: document/overlay only change on input, commands
            // (`present_if_ready`), resize, a zoom animation, or a background display-list band finishing.
            let need = had_events || self.paint_dirty || zooming || self.frame.pending.is_some() || self.frame.first.is_some();
            if self.gpu.is_some() && need {
                self.redraw()?;
                self.paint_dirty = zooming;
            }
            perf::note(
                &t_pump,
                &format!(
                    "pump: control {t_ctl:.1}, events {n_events} until {t_events:.1}, zoom tick until {t_zoom:.1}, total {:.1} ms (zooming {zooming}, pending {}, first {})",
                    perf::lap(&t_pump),
                    self.frame.pending.is_some(),
                    self.frame.first.is_some()
                ),
            );
            Ok(())
        })();
        self.in_call = false;
        self.flush_submit();
        result
    }

    pub fn redraw(&mut self) -> Result<(), String> {
        let t_all = perf::start();
        self.pixels_per_point = window::dpi_scale(self.hwnd);
        let (key, uniform, origin) = match self.mesh() {
            Err(e) if e == LOADING => return Ok(()),
            other => other?,
        };
        let t_mesh = perf::lap(&t_all);
        let paper = self.paper();
        let warm_grip = self.screen.and_then(|s| self.grip_at(s.x, s.y)).map(|g| (g.handle, g.index));
        let cursor = self.screen.is_some().then_some(overlay::Cursor {
            aim: self.aim,
            raw: self.raw,
            snap: self.snap,
            track: self.track.clone(),
            hot: self.hot,
            hover: self.hover,
            warm_grip,
        });
        let tip = overlay::cursor_tip(self.snap.as_ref(), self.track.as_ref(), self.hot);
        let overlay = overlay::mesh(
            &self.session,
            cursor.as_ref(),
            paper,
            self.height,
            origin,
            tip.as_deref(),
            self.width,
            self.show_ucs,
            f64::from(self.pixels_per_point),
        );
        let t_overlay = perf::lap(&t_all);
        let over_key = gpu::Gpu::overlay_key(&overlay);
        let stamp =
            PaintStamp { mesh_key: key, over_key, uniform: std::array::from_fn(|i| uniform[i].to_bits()), clear: (paper.0, paper.1, paper.2) };
        if self.paint.as_ref() == Some(&stamp) {
            return Ok(());
        }
        let tiled = self.frame.mesh_tiled && !self.frame.tiles.is_empty();
        let gpu = self.gpu.as_mut().ok_or("no drawing surface")?;
        gpu.set_clear(paper);
        gpu.set_overlay(&overlay);
        if tiled {
            let mut items: Vec<(u64, u64, &MeshData)> = self.frame.tiles.iter().map(|(id, t)| (id.to_bits(), t.key, &t.mesh)).collect();
            items.sort_by_key(|(id, _, _)| *id);
            // Infinite lines are view-clipped and not stored in tiles; rebuild that small mesh each present.
            let mut extra = MeshData::default();
            if let (Some(list), Ok(state)) = (self.frame.list.as_ref(), self.session.state()) {
                let view = state.view();
                let world_width = view.height * f64::from(self.width.max(1)) / f64::from(self.height.max(1));
                let lw = self.session.settings.lwdisplay;
                let on_paper = matches!(state.space, Space::Paper(_));
                let true_scale = lw && on_paper;
                mesh::append_infinite(&mut extra, list, origin, view.center, world_width, view.height, paper);
                if true_scale {
                    mesh::expand_hairlines_true_scale(&mut extra);
                } else {
                    mesh::expand_hairlines(&mut extra);
                }
            }
            let has_extra = extra.line_vertices() + extra.tri_vertices() + extra.quad_instances() + extra.ring_instances() > 0;
            gpu.present_tiles(key, &items, has_extra.then_some(&extra), &uniform)?;
        } else {
            let built = self.frame.mesh.as_ref().ok_or("mesh missing")?;
            gpu.present(key, built, &uniform)?;
        }
        self.paint = Some(stamp);
        if perf::start().is_some() {
            perf::first_geometry_frame(self.session.state().map_or(0, |s| s.doc.entity_count()));
        }
        perf::note(
            &t_all,
            &format!("redraw: mesh {t_mesh:.1}, cursor+overlay {:.1}, gpu present {:.1} ms", t_overlay - t_mesh, perf::lap(&t_all) - t_overlay),
        );
        Ok(())
    }
}

/// Headless pump: drain the control channel without a WinUI surface (Linux / CI).
#[cfg(not(windows))]
impl Host {
    pub fn pump(&mut self) -> Result<(), String> {
        self.drain_control();
        self.assistant.poll(&mut self.session);
        self.poll_geodb();
        self.autosave.tick(&self.session, autosave_interval());
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for Host {
    fn drop(&mut self) {
        if let Some(stop) = &self.control_stop {
            stop.store(true, Ordering::Relaxed);
        }
        self.gpu.take();
        if self.hwnd != 0 {
            window::destroy(self.hwnd);
            self.hwnd = 0;
        }
    }
}

fn log_since(session: &Session, before: usize) -> Vec<String> {
    session.log.iter().skip(before).cloned().collect()
}

/// Shape the host menu tree like egui `ui.menu.list` (`label` / `children` / optional `id`).
fn menu_node_for_control(node: &Value) -> Value {
    let label = node.get("title").and_then(Value::as_str).unwrap_or("");
    let children: Vec<Value> =
        node.get("items").and_then(Value::as_array).map(|items| items.iter().map(menu_node_for_control).collect()).unwrap_or_default();
    let mut out = serde_json::json!({ "label": label, "children": children });
    if let (Some(id), Some(object)) = (node.get("id").and_then(Value::as_str), out.as_object_mut())
        && !id.is_empty()
    {
        object.insert("id".into(), Value::String(id.to_string()));
    }
    if let (Some(enabled), Some(object)) = (node.get("enabled"), out.as_object_mut()) {
        object.insert("enabled".into(), enabled.clone());
    }
    out
}

/// The configured time between recovery snapshots (`AUTOSAVESECS`; zero turns the timer off).
fn autosave_interval() -> std::time::Duration {
    std::time::Duration::from_secs(u64::from(aad_engine::sysvars::AUTOSAVE_SECS.load(std::sync::atomic::Ordering::Relaxed)))
}

/// Methods of [`Host::app`] (FFI `aad_host_app` and the control channel).
fn is_app_method(method: &str) -> bool {
    matches!(
        method,
        "app.peek"
            | "autosave.status"
            | "autosave.now"
            | "recovery.list"
            | "recovery.restore"
            | "recovery.discard"
            | "backup.list"
            | "backup.open"
            | "backup.restore"
    )
}

fn json_ok(value: Value) -> Value {
    serde_json::json!({"ok": true, "result": value})
}

fn json_err(error: impl std::fmt::Display) -> Value {
    serde_json::json!({"ok": false, "error": error.to_string()})
}

#[cfg(test)]
#[allow(clippy::unwrap_used, unsafe_code)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_resize_refits_the_drawing_until_the_user_navigates() {
        let mut host = Host::create(true);
        host.apply_size(600, 600);
        host.fit_view();
        let narrow = host.stats().view_height;
        host.apply_size(1600, 600);
        let wide = host.stats().view_height;
        assert!(wide < narrow, "a wider drawing area fits the sheet at a smaller view height ({wide} < {narrow})");
        // Zoomed in by the user: a later resize leaves the view alone.
        let _ = host.session.zoom_about(0.5, aad_engine::geom::Vec2::new(8.0, 3.0));
        let zoomed = host.stats().view_height;
        host.apply_size(900, 600);
        assert_eq!(host.stats().view_height, zoomed, "the user's view is kept");
    }

    #[test]
    fn sample_line_and_wheel_zoom() {
        let mut host = Host::create(true);
        let before = host.stats().entities;
        assert!(before > 10, "sample drawing");
        assert!(host.mesh_vertices().unwrap() > 0, "mesh path");
        host.execute("line", &json!({ "points": [[0.0, 0.0], [2.0, 0.0]] })).unwrap();
        assert_eq!(host.stats().entities, before + 1);
        let height = host.stats().view_height;
        host.pointer(Pointer { x: 600.0, y: 400.0, action: POINTER_WHEEL, wheel: 1.0, mods: 0 }).unwrap();
        let after = host.stats().view_height;
        assert!(after < height, "wheel zooms in");
        let step = height / after;
        assert!((1.15..=1.30).contains(&step), "wheel step ≈1.22×, got {step}");
        host.pointer(Pointer { x: 600.0, y: 400.0, action: POINTER_WHEEL, wheel: -1.0, mods: 0 }).unwrap();
        assert!((host.stats().view_height - height).abs() / height < 1e-6);
    }

    #[test]
    fn zoom_anim_eases_and_accumulates_notches() {
        let start = aad_engine::View { center: Vec2::new(10.0, 5.0), height: 100.0 };
        let about = Vec2::new(10.0, 5.0);
        let mid = zoomed_view(start, (0.2_f64).exp(), about);
        let end = zoomed_view(mid, (0.2_f64).exp(), about);
        let t0 = std::time::Instant::now();
        let anim = ZoomAnim { from: start, target: end, t0 };
        let at_start = anim.sample(t0);
        assert!((at_start.height - start.height).abs() < 1e-9);
        let half = anim.sample(t0 + std::time::Duration::from_millis(60));
        assert!(half.height < start.height && half.height > end.height);
        let done = anim.sample(t0 + std::time::Duration::from_millis(ZOOM_ANIM_MS as u64));
        assert!((done.height - end.height).abs() < 1e-9);
    }

    #[test]
    fn menu_and_command_line() {
        let mut host = Host::create(false);
        let menu: serde_json::Value = serde_json::from_str(&host.menu_json()).unwrap();
        let draw = menu.as_array().unwrap().iter().find(|m| m["title"] == "Draw").unwrap();
        let line = draw["items"].as_array().unwrap().iter().find(|i| i["id"] == "line").unwrap();
        assert_eq!(line["enabled"], true);
        assert!(!host.menu_json().contains("Cmd+"));
        assert!(host.menu_json().contains("Ctrl+N"));
        let before = host.stats().entities;
        host.submit("LINE 0,0 4,0").unwrap();
        assert_eq!(host.stats().entities, before + 1);
        let chrome: serde_json::Value = serde_json::from_str(&host.chrome_json()).unwrap();
        assert!(chrome["prompt"].as_str().unwrap().contains("Specify next point"));
        assert_eq!(chrome["command"], "line");
        assert_eq!(chrome["literalSpace"], false);
        assert_eq!(chrome["space"], "Model");
        assert_eq!(chrome["osnap"], true);
        assert_eq!(chrome["ortho"], false);
        assert_eq!(chrome["documents"][0]["active"], true);
        assert_eq!(chrome["documents"][0]["dirty"], true);
        assert!((chrome["polarang"].as_f64().unwrap() - 90.0).abs() < 1e-6);
        assert_eq!(chrome["osmode"], 4133);
        host.cancel_command().unwrap();
        let chrome: serde_json::Value = serde_json::from_str(&host.chrome_json()).unwrap();
        assert_eq!(chrome["prompt"], "Command:");
        assert_eq!(chrome["command"], "");
        assert!(host.submit("not-a-command").is_err());
        assert_eq!(host.stats().entities, before + 1);
        assert!(host.chrome_json().contains("unknown command"));
    }

    #[test]
    fn panels_list_block_reference_handles() {
        let mut host = Host::create(false);
        let line = host.execute("line", &json!({ "points": [[0.0, 0.0], [1.0, 0.0]] })).unwrap();
        let handle = line
            .get("handles")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .cloned()
            .or_else(|| line.get("handle").cloned())
            .unwrap_or(json!("1"));
        host.execute("block", &json!({ "name": "DOOR", "base": [0.0, 0.0], "handles": [handle], "keep": "delete" })).unwrap();
        host.execute("insert", &json!({ "name": "DOOR", "at": [2.0, 0.0] })).unwrap();
        let panels: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        let blocks = panels["blocks"].as_array().unwrap();
        let door = blocks.iter().find(|b| b["name"] == "DOOR").expect("DOOR block");
        assert!(door["references"].as_u64().unwrap_or(0) >= 1);
        assert!(!door["handles"].as_array().unwrap().is_empty());
    }

    #[test]
    fn panels_carry_the_take_off_phases_for_the_pickers() {
        let mut host = Host::create(false);
        let before: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        assert_eq!(before["takeoffPhases"]["estimation"][0], "Foundation");
        assert_eq!(before["takeoffPhases"]["current"]["estimation"], serde_json::Value::Null);
        host.execute("takeoff.phases", &json!({ "current": { "estimation": "Plinth", "billing": "Phase 2" } })).unwrap();
        let after: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        assert_eq!(after["takeoffPhases"]["current"], json!({ "estimation": "Plinth", "billing": "Phase 2" }));
        assert_ne!(before["revision"], after["revision"], "the panels refresh when the choice changes");
    }

    #[test]
    fn panels_follow_the_drawing_revision() {
        let mut host = Host::create(false);
        let first: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        assert_eq!(first["properties"][0]["value"], "No selection");
        assert!(first["quick"].is_null());
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        host.execute("selectall", &json!({})).unwrap();
        let selected: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        assert_eq!(selected["quick"]["title"], "Line");
        assert_eq!(selected["quick"]["lineweight"], "ByLayer");
        assert_eq!(selected["quick"]["transparency"], "ByLayer");
        assert!(selected["quick"]["fields"].as_array().unwrap().iter().any(|field| field["key"] == "length"));
        assert_eq!(first["layers"][0]["name"], "0");
        assert_eq!(first["layers"][0]["current"], true);
        let revision = first["revision"].as_u64().unwrap();
        host.execute("layer.set", &json!({ "name": "0", "color": 1 })).unwrap();
        let next: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        assert!(next["revision"].as_u64().unwrap() > revision);
        assert_eq!(next["layers"][0]["colorName"], "Red");
        assert!(host.execute("layer.set", &json!({ "name": "0", "frozen": true })).is_err());
        let kept: serde_json::Value = serde_json::from_str(&host.panels_json()).unwrap();
        assert_eq!(kept["layers"][0]["frozen"], false);
        host.submit("insert").unwrap();
        host.submit("NO-SUCH-BLOCK").unwrap();
        assert!(host.chrome_json().contains("not found"));
        assert_eq!(host.stats().entities, 1);
    }

    #[test]
    fn an_open_started_on_a_worker_finishes_by_polling_and_a_bad_file_fails_cleanly() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let dxf = std::env::temp_dir().join(format!("aad-host-async-{}.dxf", std::process::id()));
        host.execute("saveas", &json!({ "path": dxf.to_string_lossy() })).unwrap();
        let wait = |h: &mut Host| {
            for _ in 0..2000 {
                match h.open_poll() {
                    Ok(1) => std::thread::sleep(std::time::Duration::from_millis(5)),
                    other => return other,
                }
            }
            Err("timed out".into())
        };
        let mut opened = Host::create(false);
        assert_eq!(opened.open_poll(), Ok(0));
        opened.open_start(&dxf.to_string_lossy()).unwrap();
        assert!(opened.open_start(&dxf.to_string_lossy()).is_err(), "one open at a time");
        assert_eq!(wait(&mut opened), Ok(2));
        assert!(opened.stats().entities >= 1);
        assert_eq!(opened.open_poll(), Ok(0));
        let before = opened.stats().entities;
        opened.open_start("Z:/no/such/file.dxf").unwrap();
        assert!(wait(&mut opened).is_err());
        assert_eq!(opened.stats().entities, before, "a failed open leaves the drawing alone");
        assert!(opened.open_start("").is_err());
        let _ = std::fs::remove_file(dxf);
    }

    #[test]
    fn a_save_started_on_a_worker_writes_the_file_and_a_bad_path_fails_cleanly() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let dxf = std::env::temp_dir().join(format!("aad-host-save-async-{}.dxf", std::process::id()));
        let wait = |h: &mut Host| {
            for _ in 0..2000 {
                match h.save_poll() {
                    Ok(1) => std::thread::sleep(std::time::Duration::from_millis(5)),
                    other => return other,
                }
            }
            Err("timed out".into())
        };
        assert_eq!(host.save_poll(), Ok(0));
        assert!(host.save_start(None).is_err(), "a new drawing has no file name");
        host.save_start(Some(&dxf.to_string_lossy())).unwrap();
        assert!(host.save_start(Some(&dxf.to_string_lossy())).is_err(), "one save at a time");
        assert_eq!(wait(&mut host), Ok(2));
        let mut reopened = Host::create(false);
        reopened.execute("open", &json!({ "path": dxf.to_string_lossy() })).unwrap();
        assert!(reopened.stats().entities >= 1);
        // The drawing now has a file name, so a quick save needs no path.
        host.save_start(None).unwrap();
        assert_eq!(wait(&mut host), Ok(2));
        host.save_start(Some("Z:/no/such/dir/x.dxf")).unwrap();
        assert!(wait(&mut host).is_err());
        assert_eq!(host.save_poll(), Ok(0));
        let _ = std::fs::remove_file(dxf);
    }

    #[test]
    fn saving_records_a_drawing_revision_and_the_command_notices_a_later_change() {
        let mut host = Host::create(false);
        let dir = std::env::temp_dir().join(format!("aad-host-rev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plan.dxf").to_string_lossy().to_string();
        assert!(host.execute("drawing.revision", &json!({})).is_err(), "no file yet");
        host.execute("line", &json!({ "points": [[0.0, 0.0], [5.0, 0.0]] })).unwrap();
        host.execute("saveas", &json!({ "path": path })).unwrap();
        let r = host.execute("drawing.revision", &json!({})).unwrap();
        assert_eq!(r["status"], json!("current"));
        assert_eq!(r["recorded"]["entities"], json!(1));
        assert_eq!(r["recorded"]["file"], json!("plan.dxf"));
        // The worker-thread save records one as well.
        host.execute("line", &json!({ "points": [[0.0, 1.0], [5.0, 1.0]] })).unwrap();
        host.save_start(None).unwrap();
        for _ in 0..2000 {
            if host.save_poll() != Ok(1) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let r = host.execute("drawing.revision", &json!({})).unwrap();
        assert_eq!(r["status"], json!("current"));
        assert_eq!(r["recorded"]["entities"], json!(2));
        std::fs::write(&path, b"edited elsewhere").unwrap();
        assert_eq!(host.execute("drawing.revision", &json!({})).unwrap()["status"], json!("changed"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_reads_a_saved_dwg() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let dwg = std::env::temp_dir().join(format!("aad-host-open-{}.dwg", std::process::id()));
        let dxf = std::env::temp_dir().join(format!("aad-host-open-{}.dxf", std::process::id()));
        host.execute("saveas", &json!({ "path": dwg.to_string_lossy() })).unwrap();
        host.execute("saveas", &json!({ "path": dxf.to_string_lossy() })).unwrap();
        let mut opened = Host::create(false);
        opened.execute("open", &json!({ "path": dwg.to_string_lossy() })).unwrap();
        assert!(opened.stats().entities >= 1);
        opened.execute("open", &json!({ "path": dxf.to_string_lossy() })).unwrap();
        assert!(opened.stats().entities >= 1);
        let _ = std::fs::remove_file(&dwg);
        let _ = std::fs::remove_file(&dxf);
    }

    #[test]
    fn the_shortcut_menu_moves_the_selection_to_another_level_where_that_can_work() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [100.0, 0.0]] })).unwrap();
        let handle = host.session.doc().unwrap().model.iter().next().unwrap().handle;
        let menu = |h: &mut Host| -> serde_json::Value {
            h.pointer(Pointer { x: 12.0, y: 12.0, action: POINTER_RIGHT, wheel: 0.0, mods: 0 }).unwrap();
            serde_json::from_str(&h.take_popup()).unwrap()
        };
        // Nested under "Send to Level" → "Move to Level" (copy is a sibling).
        let move_entry = |m: &serde_json::Value| -> Option<serde_json::Value> {
            let send = m["groups"].as_array()?.iter().flat_map(|g| g.as_array().into_iter().flatten()).find(|i| i["label"] == "Send to Level")?;
            send["items"].as_array()?.iter().find(|i| i["label"] == "Move to Level").cloned()
        };
        host.session.set_selection(vec![handle]);
        assert!(move_entry(&menu(&mut host)).is_none(), "a drawing with one level has nowhere to move to");
        host.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
        host.execute("level.new", &json!({ "name": "Second" })).unwrap();
        host.session.set_selection(vec![handle]);
        let m = move_entry(&menu(&mut host)).expect("two other levels");
        let labels: Vec<&str> = m["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["First", "Second"], "every level but the active one, in tab order");
        assert!(m["items"].as_array().unwrap().iter().all(|i| i["run"] == "levelmove"));
        host.session.set_selection(Vec::new());
        assert!(move_entry(&menu(&mut host)).is_none(), "nothing selected, nothing to move");
        // Choosing an entry is `level.moveto` with that id: the line leaves Ground for First.
        host.session.set_selection(vec![handle]);
        let id: u64 = m["items"][0]["id"].as_str().unwrap().parse().unwrap();
        host.execute("level.moveto", &json!({ "level": id })).unwrap();
        assert_eq!(host.session.doc().unwrap().model.len(), 0);
        host.execute("level.activate", &json!({ "level": "First" })).unwrap();
        assert_eq!(host.session.doc().unwrap().model.len(), 1);
        // A sheet view holds annotation, not level geometry: no entry there.
        host.execute("sheetset.new", &json!({ "name": "Electrical", "layerPrefix": "E-" })).unwrap();
        host.execute("sheetset.activate", &json!({ "set": "Electrical" })).unwrap();
        host.execute("text", &json!({ "at": [1.0, 1.0], "text": "x", "height": 1.0 })).unwrap();
        let note = host.session.doc().unwrap().model.iter().next().unwrap().handle;
        host.session.set_selection(vec![note]);
        assert!(move_entry(&menu(&mut host)).is_none());
    }

    #[test]
    fn clicks_draw_and_pan_moves_the_view() {
        let mut host = Host::create(false);
        host.submit("line").unwrap();
        host.pointer(Pointer { x: 100.0, y: 200.0, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 100.0, y: 200.0, action: POINTER_LEFT_UP, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 400.0, y: 200.0, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        assert_eq!(host.stats().entities, 1);
        host.pointer(Pointer { x: 0.0, y: 0.0, action: POINTER_RIGHT, wheel: 0.0, mods: 0 }).unwrap();
        assert!(host.take_popup().is_empty(), "a right-click while a command runs is Enter");
        assert!(host.chrome_json().contains("Command:"));
        host.pointer(Pointer { x: 12.0, y: 12.0, action: POINTER_RIGHT, wheel: 0.0, mods: 0 }).unwrap();
        let menu: serde_json::Value = serde_json::from_str(&host.take_popup()).unwrap();
        assert_eq!(menu["kind"], "command");
        host.submit("line").unwrap();
        host.pointer(Pointer { x: 12.0, y: 12.0, action: POINTER_RIGHT, wheel: 0.0, mods: MOD_SHIFT }).unwrap();
        let snaps: serde_json::Value = serde_json::from_str(&host.take_popup()).unwrap();
        assert_eq!(snaps["kind"], "snap");
        assert!(host.chrome_json().contains("Specify"));
        let before = host.screen_to_world(400.0, 200.0).unwrap();
        host.pointer(Pointer { x: 10.0, y: 10.0, action: POINTER_MIDDLE_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 80.0, y: 10.0, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 80.0, y: 10.0, action: POINTER_MIDDLE_UP, wheel: 0.0, mods: 0 }).unwrap();
        let after = host.screen_to_world(400.0, 200.0).unwrap();
        assert!(before.dist(after) > 1e-6, "middle drag pans");
        assert_eq!(host.stats().entities, 1);
        host.set_dark(false).unwrap();
        host.set_dark(true).unwrap();
    }

    #[test]
    fn panning_a_level_with_traces_stays_on_the_cached_list_and_mesh() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [100.0, 0.0]] })).unwrap();
        host.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
        host.execute("level.activate", &json!({ "level": "First" })).unwrap();
        host.execute("line", &json!({ "points": [[0.0, 50.0], [100.0, 50.0]] })).unwrap();
        host.execute("level.trace", &json!({ "add": "Ground" })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        let (key_a, uniform_a, _) = host.mesh().unwrap();
        let (list_key, prims) = (host.frame.list_key, host.frame.list.as_ref().map(|l| l.prims.len()));
        host.pointer(Pointer { x: 200.0, y: 200.0, action: POINTER_MIDDLE_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 208.0, y: 200.0, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 208.0, y: 200.0, action: POINTER_MIDDLE_UP, wheel: 0.0, mods: 0 }).unwrap();
        let (key_b, uniform_b, _) = host.mesh().unwrap();
        assert_eq!(host.frame.list_key, list_key, "a pan must not rebuild the list, traces included");
        assert_eq!(host.frame.list.as_ref().map(|l| l.prims.len()), prims);
        assert_eq!(key_b, key_a, "a fine pan keeps the mesh upload");
        assert_ne!(uniform_a[0], uniform_b[0]);
    }

    #[test]
    fn fine_pan_keeps_the_cached_mesh() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [100.0, 0.0]] })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        let (key_a, uniform_a, _) = host.mesh().unwrap();
        let list_key = host.frame.list_key;
        // Small middle-drag: origin snap should keep the GPU mesh; only the view uniform moves.
        host.pointer(Pointer { x: 200.0, y: 200.0, action: POINTER_MIDDLE_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 208.0, y: 200.0, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 208.0, y: 200.0, action: POINTER_MIDDLE_UP, wheel: 0.0, mods: 0 }).unwrap();
        let (key_b, uniform_b, _) = host.mesh().unwrap();
        assert_eq!(host.frame.list_key, list_key, "pan must not rebuild the display list");
        assert_eq!(key_b, key_a, "fine pan must keep the mesh upload key");
        assert_ne!(uniform_a[0], uniform_b[0], "the view offset uniform must change");
    }

    #[test]
    fn edit_only_remeshes_dirty_tiles() {
        let mut host = Host::create(false);
        // Fixed anchors keep list bounds (and mesh origin) stable when the middle cluster moves.
        // view_height 100 → tile edge 400; all geometry stays inside the model cull window.
        host.execute("line", &json!({ "points": [[-100.0, 0.0], [-90.0, 0.0]] })).unwrap();
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let mid = host.execute("line", &json!({ "points": [[800.0, 0.0], [810.0, 0.0]] })).unwrap();
        host.execute("line", &json!({ "points": [[900.0, 0.0], [910.0, 0.0]] })).unwrap();
        let mid_handles = mid.get("handles").cloned().unwrap_or(Value::Null);
        host.execute("zoom", &json!({ "mode": "center", "center": [5.0, 0.0], "height": 100.0 })).unwrap();
        let (key_a, _, _) = host.mesh().unwrap();
        assert!(host.frame.mesh_tiled, "separated clusters must use the tiled mesh path");
        assert!(host.frame.tiles_total >= 2, "expected ≥2 tiles, got {}", host.frame.tiles_total);
        let total = host.frame.tiles_total;
        assert_eq!(host.frame.tiles_remeshed, total, "cold build remeshes every tile");
        let mesh_bytes_a = host.frame.mesh_totals().1;

        host.execute("move", &json!({ "handles": mid_handles, "delta": [20.0, 0.0] })).unwrap();
        let (key_b, _, _) = host.mesh().unwrap();
        assert_ne!(key_b, key_a, "an edit changes the mesh scene key");
        assert!(host.frame.mesh_tiled);
        assert_eq!(host.frame.tiles_total, total, "tile count stays stable for an in-tile edit");
        assert_eq!(host.frame.tiles_remeshed, 1, "only the dirty tile remeshes, got {}", host.frame.tiles_remeshed);
        let mesh_bytes_b = host.frame.mesh_totals().1;
        assert_eq!(mesh_bytes_a, mesh_bytes_b, "byte size stays stable when moving a stroke within a tile");

        // Fine pan after a tiled edit still keeps the composed upload key.
        let list_key = host.frame.list_key;
        host.pointer(Pointer { x: 200.0, y: 200.0, action: POINTER_MIDDLE_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 208.0, y: 200.0, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 208.0, y: 200.0, action: POINTER_MIDDLE_UP, wheel: 0.0, mods: 0 }).unwrap();
        let (key_c, _, _) = host.mesh().unwrap();
        assert_eq!(host.frame.list_key, list_key);
        assert_eq!(key_c, key_b);
        assert_eq!(host.frame.tiles_remeshed, 1, "fine pan must not remesh tiles again");
    }

    #[test]
    fn traced_levels_are_in_the_cached_list_and_survive_an_edit() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [100.0, 0.0]] })).unwrap();
        host.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
        host.execute("level.activate", &json!({ "level": "First" })).unwrap();
        host.execute("line", &json!({ "points": [[0.0, 50.0], [100.0, 50.0]] })).unwrap();
        host.execute("level.trace", &json!({ "add": "Ground", "fade": 60 })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        host.mesh().unwrap();
        let traced = |h: &Host| h.frame.list.as_ref().map(|l| l.prims.iter().filter(|p| p.handle == aad_render::VIEWPORT_CONTENT).count());
        assert_eq!(traced(&host), Some(1), "the traced level's line is in the list");
        assert_eq!(host.frame.list.as_ref().map(|l| l.prims.len()), Some(2));

        // An edit takes the incremental path when there are no traces; with traces it must still show them.
        host.execute("line", &json!({ "points": [[0.0, 60.0], [100.0, 60.0]] })).unwrap();
        host.mesh().unwrap();
        assert_eq!(traced(&host), Some(1), "the trace survives an edit");
        assert_eq!(host.frame.list.as_ref().map(|l| l.prims.len()), Some(3));

        // Removing the trace removes it from the list.
        host.execute("level.trace", &json!({ "remove": "Ground" })).unwrap();
        host.mesh().unwrap();
        assert_eq!(traced(&host), Some(0));
    }

    #[test]
    fn the_status_bar_data_lists_section_and_elevation_tabs_and_the_open_view_draws_guides() {
        let mut host = Host::create(false);
        let views = |h: &Host| -> Vec<Value> {
            let v: Value = serde_json::from_str(&crate::chrome::chrome_json(&h.session, None, "")).unwrap();
            v["views"].as_array().cloned().unwrap_or_default()
        };
        assert!(views(&host).is_empty());
        host.execute("drawing.units", &json!({ "units": "mm", "mode": "metadata" })).unwrap();
        host.execute("wall.draw", &json!({ "points": [[4115.0, -3000.0], [4115.0, 3000.0]], "thickness": 230.0 })).unwrap();
        host.execute("level.new", &json!({ "name": "First", "first": "Ground", "elevation": 3150.0, "height": 3000.0 })).unwrap();
        host.execute("level.set", &json!({ "level": "Ground", "height": 3000.0 })).unwrap();
        host.execute("section.new", &json!({ "p1": [0.0, 0.0], "p2": [10000.0, 0.0], "depth": 5000.0 })).unwrap();
        host.execute("elevation.new", &json!({ "at": [4000.0, -6000.0], "angle": 90.0 })).unwrap();
        let tabs = views(&host);
        assert_eq!(
            tabs.iter().map(|t| (t["name"].as_str().unwrap(), t["kind"].as_str().unwrap())).collect::<Vec<_>>(),
            [("Section A-A", "section"), ("Elevation A", "elevation")]
        );
        assert!(tabs.iter().all(|t| t["active"] == false));
        // The plan has the wall; the open section has nothing drafted, yet draws its guides.
        host.mesh().unwrap();
        let plan = host.frame.list.as_ref().map(|l| l.prims.len()).unwrap_or(0);
        host.execute("view.activate", &json!({ "view": "Section A-A" })).unwrap();
        host.mesh().unwrap();
        assert_eq!(views(&host).iter().filter(|t| t["active"] == true).count(), 1);
        let open = host.frame.list.as_ref().map(|l| l.prims.len()).unwrap_or(0);
        assert!(open > 0, "the guides are drawn in an empty view (plan had {plan})");
        host.execute("level.activate", &json!({ "level": "Ground" })).unwrap();
        host.mesh().unwrap();
        assert!(views(&host).iter().all(|t| t["active"] == false));
    }

    #[test]
    fn the_status_bar_data_lists_level_tabs_with_the_active_and_traced_marks() {
        let mut host = Host::create(false);
        let levels = |h: &Host| -> Vec<Value> {
            let v: Value = serde_json::from_str(&crate::chrome::chrome_json(&h.session, None, "")).unwrap();
            v["levels"].as_array().cloned().unwrap_or_default()
        };
        assert!(levels(&host).is_empty(), "a drawing with one implicit level has no level tabs");
        host.execute("level.new", &json!({ "name": "First", "first": "Ground" })).unwrap();
        host.execute("level.new", &json!({ "name": "Terrace" })).unwrap();
        host.execute("level.activate", &json!({ "level": "First" })).unwrap();
        host.execute("level.trace", &json!({ "add": "Ground" })).unwrap();
        let tabs = levels(&host);
        let row = |n: &str| tabs.iter().find(|t| t["name"] == n).cloned().unwrap();
        assert_eq!(tabs.iter().map(|t| t["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Ground", "First", "Terrace"]);
        assert_eq!((row("First")["active"].as_bool(), row("Ground")["active"].as_bool()), (Some(true), Some(false)));
        assert_eq!((row("Ground")["traced"].as_bool(), row("Terrace")["traced"].as_bool()), (Some(true), Some(false)));
        // The settings of a trace travel with its tab; an untraced level has none.
        assert_eq!(row("Ground")["trace"], json!({ "fade": 60, "colour": "grey", "order": "below", "snap": false, "layers": null }));
        assert!(row("Terrace")["trace"].is_null());
        host.execute("level.trace", &json!({ "add": "Ground", "fade": 30, "order": "above", "snap": true, "layers": ["A-WALL"] })).unwrap();
        assert_eq!(
            levels(&host).iter().find(|t| t["name"] == "Ground").unwrap()["trace"],
            json!({ "fade": 30, "colour": "grey", "order": "above", "snap": true, "layers": ["A-WALL"] })
        );
    }

    #[test]
    fn a_sheet_view_draws_the_level_for_reference_and_the_rail_data_lists_the_sets() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [100.0, 0.0]] })).unwrap();
        host.execute("sheetset.new", &json!({ "preset": true })).unwrap();
        let rail = |h: &Host| -> Value { serde_json::from_str(&crate::chrome::chrome_json(&h.session, None, "")).unwrap() };
        let v = rail(&host);
        assert_eq!(v["sheetSets"].as_array().map(Vec::len), Some(5));
        assert_eq!(v["designMode"], true);
        host.execute("sheetset.activate", &json!({ "set": "Electrical" })).unwrap();
        host.execute("text", &json!({ "at": [10.0, 10.0], "text": "DB", "height": 5.0 })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        host.mesh().unwrap();
        let list = host.frame.list.as_ref().unwrap();
        let reference = list.prims.iter().filter(|p| p.handle == aad_render::VIEWPORT_CONTENT).count();
        assert_eq!(reference, 1, "the level's line is drawn for reference");
        assert!(list.prims.len() > 1, "and the annotation over it");
        let v = rail(&host);
        assert_eq!(v["designMode"], false);
        assert!(v["sheetSets"].as_array().unwrap().iter().any(|s| s["name"] == "Electrical" && s["active"] == true));
        host.execute("sheetset.activate", &json!({ "set": null })).unwrap();
        host.mesh().unwrap();
        assert_eq!(host.frame.list.as_ref().unwrap().prims.iter().filter(|p| p.handle == aad_render::VIEWPORT_CONTENT).count(), 0);
    }

    #[test]
    fn the_blocks_panel_lists_the_smart_blocks_of_the_active_tab() {
        let mut host = Host::create(false);
        let library = |h: &Host| -> Vec<String> {
            let v: Value = serde_json::from_str(&crate::panels::panels_json(&h.session, None)).unwrap();
            v["library"].as_array().unwrap().iter().map(|b| b["id"].as_str().unwrap().to_string()).collect()
        };
        let design = library(&host);
        assert!(design.len() > 25, "design lists every smart block: {}", design.len());
        assert!(design.iter().all(|id| !id.starts_with("rcc.")), "the RCC blocks are toolbar tools, not library entries: {design:?}");
        host.execute("sheetset.standard", &json!({})).unwrap();
        host.execute("sheetset.activate", &json!({ "set": "Electrical" })).unwrap();
        let electrical = library(&host);
        assert!(!electrical.is_empty() && electrical.iter().all(|id| id.starts_with("elec.")), "{electrical:?}");
        host.execute("layer.scope", &json!({ "mode": "all" })).unwrap();
        assert_eq!(library(&host).len(), design.len(), "all layers shows every smart block");
    }

    #[test]
    fn the_layers_panel_can_list_only_the_layers_used_on_the_active_level() {
        let mut host = Host::create(false);
        let names = |h: &Host| -> Vec<String> {
            let v: Value = serde_json::from_str(&crate::panels::panels_json(&h.session, None)).unwrap();
            v["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
        };
        host.execute("sheetset.standard", &json!({})).unwrap();
        host.execute("layer.current", &json!({"name": "A-WALL"})).unwrap();
        host.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
        host.execute("layer.current", &json!({"name": "0"})).unwrap();
        let all = names(&host);
        assert!(all.len() > 10, "{all:?}");
        host.execute("layer.scope", &json!({"mode": "level"})).unwrap();
        let used = names(&host);
        assert!(used.iter().any(|n| n == "A-WALL") && used.len() < all.len(), "{used:?}");
        assert!(!used.iter().any(|n| n == "P-PIPE"), "{used:?}");
    }

    #[test]
    fn double_click_edits_text_and_middle_double_zooms_extents() {
        let mut host = Host::create(false);
        host.execute("text", &json!({ "at": [0.0, 0.0], "text": "Hi", "height": 1.0 })).unwrap();
        host.execute("selectall", &json!({})).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        let at = host.world_to_screen(Vec2::ZERO).unwrap();
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_DOUBLE, wheel: 0.0, mods: 0 }).unwrap();
        assert!(host.chrome_json().contains("new text"), "a double-click on text starts TEXTEDIT");
        host.cancel_command().unwrap();
        let fitted = host.stats().view_height;
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_WHEEL, wheel: 2.0, mods: 0 }).unwrap();
        assert!(host.stats().view_height < fitted);
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_MIDDLE_DOUBLE, wheel: 0.0, mods: 0 }).unwrap();
        assert!((host.stats().view_height - fitted).abs() / fitted < 1e-6, "a middle double-click zooms to the extents");
    }

    fn mesh_has_rgb(mesh: &MeshData, rgb: [u8; 3]) -> bool {
        let hit = |bytes: &[u8], stride: usize, at: usize| {
            bytes.chunks(stride).any(|v| v.len() >= at + 3 && v[at] == rgb[0] && v[at + 1] == rgb[1] && v[at + 2] == rgb[2])
        };
        hit(&mesh.lines, mesh::VERTEX_SIZE, 8)
            || hit(&mesh.tris, mesh::VERTEX_SIZE, 8)
            || hit(&mesh.quads, mesh::INSTANCE_SIZE, 16)
            || hit(&mesh.rings, mesh::RING_INSTANCE_SIZE, 24)
    }

    #[test]
    fn light_paper_draws_colour_7_black() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let (dark_key, _, _) = host.mesh().unwrap();
        assert!(mesh_has_rgb(host.frame.mesh.as_ref().unwrap(), [255, 255, 255]), "colour 7 is white on the black drawing area");
        host.dark = false;
        let (light_key, _, _) = host.mesh().unwrap();
        assert_ne!(light_key, dark_key, "the canvas reuploads when the sheet colour changes");
        assert!(mesh_has_rgb(host.frame.mesh.as_ref().unwrap(), [0, 0, 0]), "colour 7 is black on the white drawing area");
        assert!(!mesh_has_rgb(host.frame.mesh.as_ref().unwrap(), [255, 255, 255]), "the white stroke is replaced");
    }

    #[test]
    fn a_ray_is_drawn_across_the_view() {
        let mut host = Host::create(false);
        let empty = host.mesh_vertices().unwrap();
        host.execute("ray", &json!({ "base": [0.0, 0.0], "through": [10.0, 0.0] })).unwrap();
        assert!(host.mesh_vertices().unwrap() > empty, "a ray is clipped into the canvas mesh");
    }

    #[test]
    fn rejected_input_leaves_the_drawing() {
        let mut host = Host::create(false);
        let before = host.stats().entities;
        let height = host.stats().view_height;
        assert!(host.execute("", &json!({})).is_err());
        assert!(host.execute("no-such-command", &json!({})).is_err());
        assert!(host.execute("line", &json!({ "points": "nope" })).is_err());
        assert!(host.pointer(Pointer { x: f64::NAN, y: 0.0, action: POINTER_WHEEL, wheel: 1.0, mods: 0 }).is_err());
        assert_eq!(host.stats().entities, before);
        assert_eq!(host.stats().view_height, height);
    }

    #[test]
    fn ffi_executes_a_line() {
        let host = ffi::create(1);
        assert!(!host.is_null());
        let rc = unsafe { ffi::execute(host, c"line".as_ptr(), c"{\"points\":[[0,0],[1,0]]}".as_ptr()) };
        assert_eq!(rc, 0, "{}", ffi::error_text(host));
        let mut stats = Stats { entities: 0, _pad: 0, view_height: 0.0, width: 0, height: 0 };
        assert_eq!(unsafe { ffi::stats(host, &mut stats) }, 0);
        assert!(stats.entities > 1);
        unsafe { ffi::destroy(host) };
    }

    /// The structural details go into the project file with the drawing, in the same revision, and again only when they change. MynaOps reads them
    /// and draws them read only. `UPDATE_FIXTURES=1` also writes `tests/fixtures/details.osp` (for MynaOps's tests of the same file).
    #[test]
    fn saving_into_a_project_file_records_the_structural_details() {
        let dir = std::env::temp_dir().join(format!("aad-details-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("house.osp");
        let _ = std::fs::remove_file(&path);
        let mut host = Host::create(false);
        host.execute("rcc.beam", &json!({ "at": [0, 0], "params": "width=300, nbot=4, barbot=20, stsp=125" })).unwrap();
        host.execute("rcc.column", &json!({ "at": [5000, 0], "params": "nx=3, ny=4" })).unwrap();
        host.execute("rcc.slab", &json!({ "at": [0, 4000] })).unwrap();
        host.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();

        let odp = aad_odp::Odp::open(&path).unwrap();
        let details = odp.details().unwrap();
        let kinds: Vec<&str> = details.iter().map(|d| d.kind.as_str()).collect();
        assert_eq!(details.len(), 3, "{kinds:?}");
        let beam = details.iter().find(|d| d.kind == "beams").unwrap();
        assert_eq!(beam.block, "rcc.beam.section");
        assert_eq!(beam.params["width"], 300.0);
        assert_eq!(beam.params["barbot"], 20.0);
        assert_eq!(beam.params["stsp"], 125.0);
        assert!(beam.id.starts_with(&format!("{}/", beam.drawing)), "{}", beam.id);
        let revision = odp.revision().unwrap();
        drop(odp);

        // Saved again with nothing changed: no new revision, so a program watching the file is not told to reload.
        host.execute("qsave", &json!({})).unwrap();
        assert_eq!(aad_odp::Odp::open(&path).unwrap().revision().unwrap(), revision);
        // Change a bar: the details change with the drawing, in one revision.
        host.execute("smartblock.edit", &json!({ "handle": beam.handle, "params": "nbot=2" })).unwrap();
        host.execute("qsave", &json!({})).unwrap();
        let odp = aad_odp::Odp::open(&path).unwrap();
        assert_eq!(odp.revision().unwrap(), revision + 1);
        assert_eq!(odp.details().unwrap().iter().find(|d| d.kind == "beams").unwrap().params["nbot"], 2.0);
        drop(odp);

        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/details.osp");
            std::fs::create_dir_all(out.parent().unwrap()).unwrap();
            std::fs::copy(&path, out).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn endpoint_snap_places_the_line() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        host.submit("line").unwrap();
        let near = host.world_to_screen(Vec2::new(0.02, 0.02)).unwrap();
        host.pointer(Pointer { x: near.x, y: near.y, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        assert!(host.session.cursor.dist(Vec2::ZERO) < 1e-6, "the cursor snaps to the endpoint");
        host.pointer(Pointer { x: near.x, y: near.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        let far = host.world_to_screen(Vec2::new(4.0, 2.0)).unwrap();
        host.pointer(Pointer { x: far.x, y: far.y, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        let rubber = overlay::mesh(
            &host.session,
            Some(&overlay::Cursor {
                aim: host.aim,
                raw: host.raw,
                snap: host.snap,
                track: host.track.clone(),
                hot: host.hot,
                hover: host.hover,
                warm_grip: None,
            }),
            host.paper(),
            host.height,
            Vec2::ZERO,
            None,
            host.width,
            host.show_ucs,
            f64::from(host.pixels_per_point),
        );
        assert!(rubber.quad_instances() > 4, "the rubber band and crosshair are drawn");
        host.pointer(Pointer { x: far.x, y: far.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: 0.0, y: 0.0, action: POINTER_RIGHT, wheel: 0.0, mods: 0 }).unwrap();
        let doc = host.session.doc().unwrap();
        let space = doc.space(&aad_engine::doc::Space::Model).unwrap();
        let snapped = space.iter().any(|entity| match &entity.kind {
            aad_engine::doc::EntityKind::Line(line) => line.a.xy().dist(Vec2::ZERO) < 1e-6 && line.b.xy().dist(Vec2::new(4.0, 2.0)) < 1.0,
            _ => false,
        });
        assert!(snapped, "the new line starts on the snapped endpoint");
    }

    #[test]
    fn dynamic_input_locks_the_distance_and_places_the_point() {
        let mut host = Host::create(false);
        host.submit("line").unwrap();
        host.submit("0,0").unwrap();
        assert_eq!(host.dyn_tab("40").as_deref(), Some("40<"));
        host.set_draft("5<").unwrap();
        let up = host.world_to_screen(Vec2::new(0.0, 20.0)).unwrap();
        host.pointer(Pointer { x: up.x, y: up.y, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        assert!((host.aim - Vec2::new(0.0, 5.0)).len() < 1e-6, "a locked distance holds the cursor");
        let tip = host.dyn_tip().unwrap();
        assert!(tip.contains("5"), "{tip}");
        let drawn = overlay::mesh(
            &host.session,
            Some(&overlay::Cursor { aim: host.aim, raw: host.raw, snap: None, track: None, hot: None, hover: None, warm_grip: None }),
            host.paper(),
            host.height,
            Vec2::ZERO,
            Some(&tip),
            host.width,
            host.show_ucs,
            f64::from(host.pixels_per_point),
        );
        assert!(drawn.quad_instances() > 0, "the tooltip is stroked");
        host.submit("5<90").unwrap();
        let doc = host.session.doc().unwrap();
        let space = doc.space(&aad_engine::doc::Space::Model).unwrap();
        let placed = space.iter().any(|entity| match &entity.kind {
            aad_engine::doc::EntityKind::Line(line) => line.a.xy().dist(Vec2::ZERO) < 1e-6 && line.b.xy().dist(Vec2::new(0.0, 5.0)) < 1e-6,
            _ => false,
        });
        assert!(placed, "Enter places the locked polar point");
    }

    #[test]
    fn grip_cycle_moves_the_selected_line() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        let at = host.world_to_screen(Vec2::ZERO).unwrap();
        let click = |host: &mut Host, at: Vec2| {
            host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
            host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_UP, wheel: 0.0, mods: 0 }).unwrap();
        };
        click(&mut host, at);
        click(&mut host, at);
        assert!(host.hot.is_some(), "the second click picks the grip");
        assert!(host.grip_cycle());
        assert_eq!(host.hot.unwrap().mode, aad_engine::grips::GripMode::Move);
        let to = host.world_to_screen(Vec2::new(0.0, 5.0)).unwrap();
        click(&mut host, to);
        let doc = host.session.doc().unwrap();
        let space = doc.space(&aad_engine::doc::Space::Model).unwrap();
        let moved = space.iter().any(|entity| match &entity.kind {
            aad_engine::doc::EntityKind::Line(line) => line.a.xy().dist(Vec2::new(0.0, 5.0)) < 1e-6 && line.b.xy().dist(Vec2::new(10.0, 5.0)) < 1e-6,
            _ => false,
        });
        assert!(moved, "move mode shifts the whole line");
    }

    #[test]
    fn keyboard_nudge_and_activate_select_without_a_pointer() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        let at = host.world_to_screen(Vec2::ZERO).unwrap();
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        let before = host.screen.expect("cursor on screen");
        host.key_nudge(1, 0, false, false).unwrap();
        let after = host.screen.expect("cursor on screen");
        assert!(after.x > before.x, "arrow keys move the screen cursor");
        let on_line = host.world_to_screen(Vec2::new(5.0, 0.0)).unwrap();
        host.pointer(Pointer { x: on_line.x, y: on_line.y, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        host.key_activate(false).unwrap();
        let selected = host.session.state().unwrap().selection.len();
        assert!(selected >= 1, "Insert picks the entity under the keyboard cursor");
        let idle = overlay::mesh(
            &host.session,
            Some(&overlay::Cursor { aim: host.aim, raw: host.raw, snap: None, track: None, hot: None, hover: host.hover, warm_grip: None }),
            host.paper(),
            host.height,
            Vec2::ZERO,
            None,
            host.width,
            false,
            f64::from(host.pixels_per_point),
        );
        assert!(idle.quad_instances() > 8, "selected geometry is dashed-highlighted with grips and crosshair");
        let center = host.session.state().unwrap().view().center;
        host.key_nudge(2, 0, true, false).unwrap();
        let moved = host.session.state().unwrap().view().center;
        assert!(center.dist(moved) > 1e-9, "Ctrl+arrow pans");
    }

    #[test]
    fn a_typed_point_applies_the_hot_grip_and_escape_clears_it() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        let at = host.world_to_screen(Vec2::ZERO).unwrap();
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_UP, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: at.x, y: at.y, action: POINTER_LEFT_UP, wheel: 0.0, mods: 0 }).unwrap();
        assert!(host.hot.is_some());
        assert!(!host.grip_point("   "));
        assert!(host.grip_point("nope"));
        assert!(host.hot.is_some(), "a non-point leaves the grip hot");
        assert!(host.chrome_json().contains("Requires a point"));
        assert!(host.grip_point("@0,5"));
        assert!(host.hot.is_none());
        let doc = host.session.doc().unwrap();
        let space = doc.space(&aad_engine::doc::Space::Model).unwrap();
        let stretched = space.iter().any(|entity| match &entity.kind {
            aad_engine::doc::EntityKind::Line(line) => line.a.xy().dist(Vec2::new(0.0, 5.0)) < 1e-6 && line.b.xy().dist(Vec2::new(10.0, 0.0)) < 1e-6,
            _ => false,
        });
        assert!(stretched, "stretch moves the picked endpoint");
        let moved = host.world_to_screen(Vec2::new(0.0, 5.0)).unwrap();
        host.pointer(Pointer { x: moved.x, y: moved.y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        host.pointer(Pointer { x: moved.x, y: moved.y, action: POINTER_LEFT_UP, wheel: 0.0, mods: 0 }).unwrap();
        assert!(host.hot.is_some(), "the endpoint is hot again");
        host.cancel_command().unwrap();
        assert!(host.hot.is_none());
        assert!(host.chrome_json().contains("*Cancel*"));
        assert!(!host.grip_point("1,0"));
    }

    #[test]
    fn tab_cycles_object_snaps_and_a_form_is_json() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        host.execute("line", &json!({ "points": [[0.2, 0.2], [0.2, 8.0]] })).unwrap();
        host.execute("zoom.extents", &json!({})).unwrap();
        host.submit("line").unwrap();
        let near = host.world_to_screen(Vec2::new(0.1, 0.1)).unwrap();
        host.pointer(Pointer { x: near.x, y: near.y, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        host.snap_tab().unwrap();
        assert!(host.aim.is_finite());
        assert!(host.snap_cycle.is_some());
        let form = host.form_json("electrical.schedule").unwrap();
        assert!(form.contains("Electrical Schedule"));
        assert!(host.form_json("line").is_none());
    }

    #[test]
    fn layer_query_is_the_authoritative_table() {
        let host = ffi::create(0);
        let run = |command: &str, params: &str| {
            let command = std::ffi::CString::new(command).unwrap();
            let params = std::ffi::CString::new(params).unwrap();
            unsafe { ffi::execute(host, command.as_ptr(), params.as_ptr()) }
        };
        assert_eq!(run("layer.new", r#"{"name":"Walls","plot":false,"transparency":40,"description":"exterior"}"#), 0);
        assert_eq!(run("layer", "{}"), 0);
        let table: serde_json::Value = serde_json::from_str(&ffi::result_text(host)).unwrap();
        let walls = table["layers"].as_array().unwrap().iter().find(|layer| layer["name"] == "Walls").unwrap();
        assert_eq!(walls["plot"], false);
        assert_eq!(walls["transparency"], 40);
        assert_eq!(walls["description"], "exterior");
        assert_eq!(walls["used"], false);
        assert!(table["linetypes"].as_array().unwrap().iter().any(|name| name == "Continuous"));
        assert_eq!(run("layer.set", r#"{"name":"0","frozen":true}"#), 1);
        assert!(ffi::result_text(host).is_empty(), "a rejected command clears the previous result");
        assert!(ffi::error_text(host).contains("cannot freeze"));
        assert_eq!(run("layer.delete", r#"{"name":"0"}"#), 1);
        assert!(ffi::error_text(host).contains("cannot delete"));
        unsafe { ffi::destroy(host) };
    }

    #[test]
    fn a_plot_started_on_a_worker_writes_the_pdf_and_bad_arguments_fail_cleanly() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let pdf = std::env::temp_dir().join(format!("aad-host-plot-async-{}.pdf", std::process::id()));
        let wait = |h: &mut Host| {
            for _ in 0..2000 {
                match h.plot_poll() {
                    Ok(1) => std::thread::sleep(std::time::Duration::from_millis(5)),
                    other => return other,
                }
            }
            Err("timed out".into())
        };
        assert_eq!(host.plot_poll(), Ok(0));
        assert!(host.plot_start("line", &json!({})).is_err(), "only plotting commands");
        assert!(host.plot_start("plot", &json!({ "window": [0.0, 0.0, 1.0] })).is_err(), "a malformed window is refused up front");
        host.plot_start("plot", &json!({ "path": pdf.to_string_lossy() })).unwrap();
        assert!(host.plot_start("plot", &json!({})).is_err(), "one plot at a time");
        assert_eq!(wait(&mut host), Ok(2));
        let bytes = std::fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        host.plot_start("plot", &json!({ "path": "Z:/no/such/dir/x.pdf" })).unwrap();
        assert!(wait(&mut host).is_err());
        assert_eq!(host.plot_poll(), Ok(0));
        let _ = std::fs::remove_file(pdf);
    }

    #[test]
    fn plot_preview_is_a_pdf() {
        let mut host = Host::create(true);
        let pdf = host.pdf_bytes().unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.windows(4).any(|w| w == b"%%EOF") || pdf.ends_with(b"%%EOF\n") || pdf.ends_with(b"%%EOF"));
    }

    #[test]
    fn a_slash_request_reports_when_the_assistant_runtime_is_absent() {
        let mut host = Host::create(false);
        if host.assistant.available() {
            assert!(host.submit("/ layers").is_ok());
            assert!(host.assistant.busy(), "slash ask should leave the assistant working");
            host.assistant.stop(&mut host.session);
        } else {
            let err = host.submit("/ layers").expect_err("assistant runtime absent");
            let lower = err.to_ascii_lowercase();
            assert!(
                lower.contains("assistant")
                    || lower.contains("not included")
                    || lower.contains("not ready")
                    || lower.contains("olmo")
                    || lower.contains("token")
                    || lower.contains("model"),
                "{err}"
            );
            assert_eq!(host.assistant.phase(), "idle");
        }
    }

    #[test]
    fn the_ucs_icon_click_opens_its_menu() {
        let mut host = Host::create(false);
        let y = f64::from(host.stats().height) - 40.0;
        host.pointer(Pointer { x: 40.0, y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        let popup = host.take_popup();
        assert!(popup.contains("Hide UCS / site mark"), "{popup}");
        host.set_ucs(false).unwrap();
        host.pointer(Pointer { x: 40.0, y, action: POINTER_LEFT_DOWN, wheel: 0.0, mods: 0 }).unwrap();
        assert!(host.take_popup().is_empty());
    }

    #[test]
    fn plot_window_request_json_is_exposed_to_the_shell() {
        let mut host = Host::create(false);
        assert!(host.plot_request_json().is_empty());
        host.session.plot_request = Some(aad_engine::PlotRequest { window: None, layout: String::new(), preview: false });
        assert!(host.plot_request_json().contains("null"), "{}", host.plot_request_json());
        host.session.plot_request = Some(aad_engine::PlotRequest { window: Some([1.0, 2.0, 3.0, 4.0]), layout: "Nameplate L".into(), preview: true });
        let json = host.plot_request_json();
        assert!(json.contains('1') && json.contains('4'), "{json}");
        host.clear_plot_request();
        assert!(host.plot_request_json().is_empty());
        assert!(host.session.plot_request.is_none());
    }

    #[test]
    fn ui_inspect_menu_list_pointer_and_screenshot_work_on_the_control_channel() {
        let mut host = Host::create(false);
        host.apply_size(640, 480);
        let inspect = host.control_one("ui.inspect", &json!({}));
        assert_eq!(inspect["ok"], true, "{inspect}");
        assert_eq!(inspect["result"]["shell"], "winui");
        assert_eq!(inspect["result"]["canvas"]["width"], 640);
        let menus = host.control_one("ui.menu.list", &json!({}));
        assert_eq!(menus["ok"], true, "{menus}");
        assert!(menus["result"].as_array().unwrap().iter().any(|m| m["label"] == "File"));
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let click = host.control_one("ui.pointer", &json!({ "x": 5.0, "y": 0.0, "space": "world" }));
        assert_eq!(click["ok"], true, "{click}");
        assert_eq!(host.session.selection().len(), 1);
        let path = std::env::temp_dir().join(format!("aad-host-ui-shot-{}.png", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let shot = host.control_one("ui.screenshot", &json!({ "path": path.to_string_lossy() }));
        assert_eq!(shot["ok"], true, "{shot}");
        assert_eq!(shot["result"]["drawing"], true);
        assert!(path.is_file(), "missing {}", path.display());
        let _ = std::fs::remove_file(&path);
        let chrome = host.control_one("ui.click", &json!({ "x": 1.0, "y": 1.0 }));
        assert_eq!(chrome["ok"], false);
        assert!(chrome["error"].as_str().unwrap_or("").contains("egui"));
    }

    #[test]
    fn ui_render_writes_a_png_on_the_control_channel() {
        let path = std::env::temp_dir().join(format!("aad-host-ui-render-{}.png", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let reply = host.control_one("ui.render", &json!({ "path": path.to_string_lossy(), "width": 64, "height": 48 }));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["width"], 64);
        assert_eq!(reply["result"]["height"], 48);
        assert!(reply["result"]["bytes"].as_u64().unwrap_or(0) > 0);
        assert!(path.is_file(), "missing {}", path.display());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn app_open_save_and_quit_work_on_the_control_channel() {
        let dir = std::env::temp_dir().join(format!("aad-host-app-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plan.dxf");
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [3.0, 0.0]] })).unwrap();
        host.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
        let source = std::fs::read(&path).unwrap();
        let mut opened = Host::create(false);
        let open = opened.control_one("app.open", &json!({ "path": path.to_string_lossy() }));
        assert_eq!(open["ok"], true, "{open}");
        assert!(opened.stats().entities >= 1);
        // A DXF is edited in its working copy, a project file beside it; the reply and the chrome say which file that is and where it came from.
        let native = dir.join("plan.osp");
        assert_eq!(open["result"]["action"], "imported", "{open}");
        assert_eq!(open["result"]["native"], native.to_string_lossy().as_ref());
        assert!(native.is_file());
        // The shell publishes the open file to the other suite apps from the chrome state, whoever opened it.
        let chrome: serde_json::Value = serde_json::from_str(&opened.chrome_json()).unwrap();
        let active = chrome["documents"].as_array().unwrap().iter().find(|d| d["active"] == true).unwrap();
        assert_eq!(active["path"], native.to_string_lossy().as_ref(), "{chrome}");
        assert_eq!(active["source"], path.to_string_lossy().as_ref(), "{chrome}");
        assert_eq!(active["title"], "plan.osp");
        opened.execute("circle", &json!({ "center": [1.0, 1.0], "radius": 1.0 })).unwrap();
        let save = opened.control_one("app.save", &json!({}));
        assert_eq!(save["ok"], true, "{save}");
        assert_eq!(std::fs::read(&path).unwrap(), source, "saving writes the working copy, never the source");
        // The next open of the same DXF reads the saved working copy from its native model.
        let mut again = Host::create(false);
        let open = again.control_one("app.open", &json!({ "path": path.to_string_lossy() }));
        assert_eq!((&open["result"]["action"], &open["result"]["fromNative"]), (&json!("reused"), &json!(true)), "{open}");
        assert_eq!(again.stats().entities, opened.stats().entities);
        assert_eq!(again.control_one("app.open", &json!({ "path": path.to_string_lossy(), "onChanged": "sometimes" }))["ok"], false);
        assert!(again.open_progress().is_empty());
        let missing = opened.control_one("app.open", &json!({}));
        assert_eq!(missing["ok"], false);
        assert!(missing["error"].as_str().unwrap_or("").contains("missing path"));
        let quit = opened.control_one("app.quit", &json!({}));
        assert_eq!(quit["ok"], true, "{quit}");
        let chrome: serde_json::Value = serde_json::from_str(&opened.chrome_json()).unwrap();
        assert_eq!(chrome["quit"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_control_channel_runs_a_command_on_the_session_thread() {
        let mut host = Host::create(false);
        let port = host.listen(0).unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        use std::io::Write;
        writeln!(stream, r#"{{"id":7,"method":"engine.execute","params":{{"command":"line","params":{{"points":[[0,0],[2,0]]}}}}}}"#).unwrap();
        stream.set_nonblocking(true).unwrap();
        let start = std::time::Instant::now();
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        while start.elapsed() < std::time::Duration::from_secs(3) && !got.contains(&b'\n') {
            host.pump().unwrap();
            match std::io::Read::read(&mut stream, &mut buf) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(e) => panic!("{e}"),
            }
        }
        let text = String::from_utf8(got).unwrap();
        assert!(text.contains("\"ok\":true") && text.contains("\"id\":7"), "{text}");
        assert!(host.stats().entities >= 1);
    }

    #[test]
    fn the_control_channel_waits_for_a_line_written_after_accept() {
        let mut host = Host::create(false);
        let port = host.listen(0).unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(80));
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        use std::io::Write;
        writeln!(stream, r#"{{"id":4,"method":"cmdline.input","params":{{"text":"line"}}}}"#).unwrap();
        stream.set_nonblocking(true).unwrap();
        let start = std::time::Instant::now();
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        while start.elapsed() < std::time::Duration::from_secs(3) && !got.contains(&b'\n') {
            host.pump().unwrap();
            match std::io::Read::read(&mut stream, &mut buf) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(e) => panic!("{e}"),
            }
        }
        let text = String::from_utf8(got).unwrap();
        assert!(text.contains("\"ok\":true") && text.contains("\"id\":4"), "{text}");
        assert_eq!(host.session.running.as_ref().map(|running| running.id.as_str()), Some("line"));
    }

    #[test]
    fn dyn_chrome_reports_the_point_prompt_fields() {
        let mut host = Host::create(false);
        host.submit("LINE 0,0 4,0").unwrap();
        host.pointer(Pointer { x: 100.0, y: 40.0, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        let chrome: serde_json::Value = serde_json::from_str(&host.chrome_json()).unwrap();
        assert_eq!(chrome["dyn"]["on"], true);
        assert_eq!(chrome["dyn"]["x"], 100.0);
        assert_eq!(chrome["dyn"]["aLabel"], "Length");
        assert_eq!(chrome["dyn"]["bLabel"], "Angle");
        assert!(!chrome["dyn"]["a"].as_str().unwrap_or("").is_empty());
    }

    #[test]
    fn dyn_chrome_shows_the_typed_command_not_the_coordinates_and_leaves_assistant_requests_to_the_bar() {
        let mut host = Host::create(false);
        let mv = |h: &mut Host| h.pointer(Pointer { x: 100.0, y: 40.0, action: POINTER_MOVE, wheel: 0.0, mods: 0 }).unwrap();
        let dynamic = |h: &Host| -> serde_json::Value { serde_json::from_str::<serde_json::Value>(&h.chrome_json()).unwrap()["dyn"].clone() };
        // Idle with nothing typed: CAD parking is on; Length/Angle dims stay off.
        mv(&mut host);
        let idle = dynamic(&host);
        assert_eq!(idle["on"], true);
        assert_eq!(idle["dims"], false);
        assert_eq!(idle["x"], 100.0);
        // A command being typed shows beside the cursor.
        host.set_draft("lin").unwrap();
        let d = dynamic(&host);
        assert_eq!(
            (d["on"].clone(), d["dims"].clone(), d["aLabel"].clone(), d["a"].clone(), d["bLabel"].clone()),
            (json!(true), json!(true), json!("Command"), json!("lin"), json!(""))
        );
        assert_eq!(d["x"], 100.0);
        // A slash assistant draft stays off the cursor chrome (fixed AI bar).
        host.set_draft("/ what layers are there").unwrap();
        assert_eq!(dynamic(&host)["on"], false);
        // Plain natural text may park at the cursor (CAD channel); AI field is separate.
        host.set_draft("what is in this drawing").unwrap();
        let natural = dynamic(&host);
        assert_eq!(natural["on"], true);
        assert_eq!(natural["dims"], true);
        host.set_draft("").unwrap();
        // While a command waits for a point with no base point yet: parking on, no Length/Angle.
        host.submit("LINE").unwrap();
        mv(&mut host);
        let waiting = dynamic(&host);
        assert_eq!(waiting["on"], true);
        assert_eq!(waiting["dims"], false);
        assert!(waiting["prompt"].as_str().unwrap_or("").contains("Specify"));
        // A typed answer shows as input, and the same words are not taken for a request while a command runs.
        host.set_draft("what is this").unwrap();
        let d = dynamic(&host);
        assert_eq!((d["on"].clone(), d["a"].clone()), (json!(true), json!("what is this")));
    }

    #[test]
    fn ask_routes_natural_language_without_cad_cmdline() {
        let mut host = Host::create(false);
        let before = host.stats().entities;
        if host.assistant.available() {
            // Llama or OSAS AI Core is up — ask starts work without touching the CAD cmdline.
            assert!(host.ask("what layers are locked").is_ok());
            assert!(host.assistant.busy(), "ask should leave the assistant working");
            host.assistant.stop(&mut host.session);
        } else {
            let err = host.ask("what layers are locked").expect_err("no assistant runtime");
            let lower = err.to_ascii_lowercase();
            assert!(
                lower.contains("assistant")
                    || lower.contains("not included")
                    || lower.contains("not ready")
                    || lower.contains("olmo")
                    || lower.contains("token"),
                "{err}"
            );
            assert_eq!(host.assistant.phase(), "idle");
        }
        assert_eq!(host.stats().entities, before, "ask must not change the drawing");
        assert!(host.session.running.is_none());
        // Without a leading `/`, submit stays on the CAD path (unknown command), not ask.
        let cad = host.submit("what layers are locked");
        assert!(cad.is_err(), "plain sentence is CAD, not an assistant ask");
        assert!(host.session.running.is_none());
        assert_eq!(host.stats().entities, before);
    }

    #[test]
    fn slash_transfer_helpers_match_idle_contract() {
        use crate::bar::{slash_transfer_eligible, slash_transfer_payload};
        let session = Session::new();
        assert!(slash_transfer_eligible(&session, "/ create a room"));
        assert_eq!(slash_transfer_payload("/ create a room"), "create a room");
        assert!(!slash_transfer_eligible(&session, "LINE"));
    }

    #[test]
    fn clear_log_keeps_the_drawing_and_the_running_command() {
        let mut host = Host::create(false);
        host.submit("LINE 0,0 4,0").unwrap();
        let entities = host.stats().entities;
        assert!(entities >= 1);
        assert!(host.session.running.is_some());
        assert!(!host.session.log.is_empty());
        host.clear_log().unwrap();
        assert!(host.session.log.is_empty());
        assert_eq!(host.stats().entities, entities);
        assert!(host.session.running.is_some());
        let chrome: serde_json::Value = serde_json::from_str(&host.chrome_json()).unwrap();
        assert!(chrome["history"].as_array().is_some_and(|rows| rows.is_empty()));
        assert_eq!(chrome["command"], "line");
    }

    #[test]
    fn high_value_commands_appear_on_the_menu() {
        let host = Host::create(false);
        let menu = host.menu_json();
        for label in ["Deselect All", "Measure Geometry", "New Layer", "Inspect Drawing", "Switch"] {
            assert!(menu.contains(label), "menu should include {label}");
        }
        assert!(menu.contains("\"id\":\"ai_deselect\"") || menu.contains("ai_deselect"));
        assert!(menu.contains("measuregeom") || menu.contains("\"id\":\"measuregeom\""));
    }
    /// Timing probe, not a pass/fail test: `AAD_PERF_FILE=<drawing> cargo test --release -p aad-host --lib edit_latency -- --ignored --nocapture`.
    /// Splits the DWG read: the DWG reader and DXF writer (aad_dwg, in this process), then our DXF reader.
    #[test]
    #[ignore]
    fn dwg_read_pipeline_stages() {
        let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let bytes = std::fs::read(&path).unwrap();
        let t = std::time::Instant::now();
        let doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(&bytes[..])).read().unwrap();
        println!("acadrust DWG read: {:.0} ms", ms(t));
        let t = std::time::Instant::now();
        let mut out = Vec::new();
        acadrust::DxfWriter::new(&doc).write_to_writer(&mut out).unwrap();
        println!("acadrust DXF write: {:.0} ms, {} MB of DXF", ms(t), out.len() >> 20);
        drop(doc);
        let t = std::time::Instant::now();
        let tags = aad_dxf::parse(&out).unwrap();
        println!("  tag parse only (aad_dxf::parse): {:.0} ms, {} tags", ms(t), tags.len());
        drop(tags);
        let t = std::time::Instant::now();
        let drawing = aad_io::read(&out, "x.dxf").unwrap();
        println!("our DXF read: {:.0} ms, entities {}", ms(t), drawing.entity_count());
    }

    /// Where the time goes when a large drawing file is opened, stage by stage (AAD_PERF_FILE=path, --ignored --nocapture).
    #[test]
    #[ignore]
    fn open_cost_by_stage() {
        let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
        let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("x.dwg").to_string();
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut host = Host::create(false);
        let t = std::time::Instant::now();
        let bytes = std::fs::read(&path).unwrap();
        println!("read file ({} MB): {:.0} ms", bytes.len() >> 20, ms(t));
        let t = std::time::Instant::now();
        let drawing = aad_io::read(&bytes, &name).unwrap();
        println!("parse (aad_io::read): {:.0} ms, entities {}", ms(t), drawing.entity_count());
        let t = std::time::Instant::now();
        let copy = drawing.clone();
        println!("  clone of the drawing alone: {:.0} ms", ms(t));
        drop(copy);
        let t = std::time::Instant::now();
        let prepared = aad_engine::cmd::file::prepare(drawing);
        println!("prepare (meta, classify, clone, flatten): {:.0} ms", ms(t));
        let t = std::time::Instant::now();
        aad_engine::cmd::file::finish_open(&mut host.session, prepared, &name).unwrap();
        println!("finish_open: {:.0} ms", ms(t));
        let t = std::time::Instant::now();
        host.fit_view();
        println!("fit_view: {:.0} ms", ms(t));
        let t = std::time::Instant::now();
        host.mesh().unwrap();
        println!("first frame (display list + mesh): {:.0} ms, tiles {}", ms(t), host.frame.scene.as_ref().map_or(0, |s| s.tiles.len()));
    }

    /// Opens the drawing in a headless host and times what the UI thread does after each edit and each zoom step: the display
    /// list (retained scene), the tile meshes and the composed mesh, i.e. `Host::mesh`, everything but the GPU upload.
    #[test]
    #[ignore]
    fn edit_latency_on_a_real_drawing() {
        let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
        let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("x.dwg").to_string();
        let mut host = Host::create(false);
        let drawing = aad_io::read(&std::fs::read(&path).unwrap(), &name).unwrap();
        host.session.open_drawing(drawing, &name, None);
        host.execute("zoom.extents", &json!({})).unwrap();
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let t = std::time::Instant::now();
        host.mesh().unwrap();
        println!(
            "first frame (list + mesh): {:.1} ms, tiles {} (mesh tiles {})",
            ms(t),
            host.frame.scene.as_ref().map_or(0, |s| s.tiles.len()),
            host.frame.tiles_total
        );
        let handles: Vec<String> = host.session.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
        println!("{:<14} {:>9} {:>10} {:>16} {:>14}", "edit", "engine", "frame", "mesh tiles redone", "list build ms");
        for op in ["move", "copy"] {
            for n in [1usize, 100, 2_000, handles.len()] {
                let sel: Vec<&String> = handles.iter().take(n).collect();
                let t = std::time::Instant::now();
                host.execute(op, &json!({ "handles": sel, "delta": [5000.0, 0.0] })).unwrap();
                let engine = ms(t);
                let t = std::time::Instant::now();
                host.mesh().unwrap();
                println!(
                    "{:<14} {:>7.1}ms {:>8.1}ms {:>9}/{:<6} {:>12.1}",
                    format!("{op} {n}"),
                    engine,
                    ms(t),
                    host.frame.tiles_remeshed,
                    host.frame.tiles_total,
                    host.frame.list_build_ms
                );
                host.execute("undo", &json!({})).unwrap();
                host.mesh().unwrap();
            }
        }
        // Erase and the undo that follows it, then selecting everything: the selection overlay is drawn on top of the
        // drawing on every frame, so its cost counts as much as the mesh.
        println!("{:<14} {:>9} {:>10} {:>14}", "operation", "engine", "frame", "overlay");
        let overlay_ms = |host: &mut Host| {
            let t = std::time::Instant::now();
            let origin = host.frame.mesh_origin;
            let paper = host.paper();
            // With a cursor, as the real redraw has: the selection highlight and the grips are drawn only then.
            let cursor = overlay::Cursor {
                aim: host.aim,
                raw: host.raw,
                snap: host.snap,
                track: host.track.clone(),
                hot: host.hot,
                hover: host.hover,
                warm_grip: None,
            };
            let _ = overlay::mesh(
                &host.session,
                Some(&cursor),
                paper,
                host.height,
                origin,
                None,
                host.width,
                host.show_ucs,
                f64::from(host.pixels_per_point),
            );
            t.elapsed().as_secs_f64() * 1000.0
        };
        for n in [1usize, 100, 2_000, handles.len()] {
            let sel: Vec<&String> = handles.iter().take(n).collect();
            let t = std::time::Instant::now();
            host.execute("erase", &json!({ "handles": sel })).unwrap();
            let engine = ms(t);
            let t = std::time::Instant::now();
            host.mesh().unwrap();
            let frame = ms(t);
            println!("{:<14} {:>7.1}ms {:>8.1}ms {:>12.1}ms", format!("erase {n}"), engine, frame, overlay_ms(&mut host));
            let t = std::time::Instant::now();
            host.execute("undo", &json!({})).unwrap();
            let engine = ms(t);
            let t = std::time::Instant::now();
            host.mesh().unwrap();
            let frame = ms(t);
            println!("{:<14} {:>7.1}ms {:>8.1}ms {:>12.1}ms", format!("undo erase {n}"), engine, frame, overlay_ms(&mut host));
        }
        for n in [100usize, 2_000, handles.len()] {
            let sel: Vec<&String> = handles.iter().take(n).collect();
            let t = std::time::Instant::now();
            host.execute("select", &json!({ "handles": sel })).unwrap_or_else(|_| json!({}));
            let engine = ms(t);
            let t = std::time::Instant::now();
            host.mesh().unwrap();
            let frame = ms(t);
            println!(
                "{:<14} {:>7.1}ms {:>8.1}ms {:>12.1}ms  ({} selected)",
                format!("select {n}"),
                engine,
                frame,
                overlay_ms(&mut host),
                host.session.selection().len()
            );
            host.execute("deselect", &json!({})).ok();
        }
        let t = std::time::Instant::now();
        host.execute("selectall", &json!({})).unwrap();
        let engine = ms(t);
        let t = std::time::Instant::now();
        host.mesh().unwrap();
        let frame = ms(t);
        println!(
            "{:<14} {:>7.1}ms {:>8.1}ms {:>12.1}ms  ({} selected)",
            "select all",
            engine,
            frame,
            overlay_ms(&mut host),
            host.session.selection().len()
        );
        host.execute("deselect", &json!({})).ok();
        // The ghost shown while a copy is being placed is rebuilt on every mouse move (the command previews at most 500 entities).
        println!("{:<24} {:>12} {:>12}", "copy ghost", "mean/move", "worst");
        let full_view = host.session.state().unwrap().view();
        for (label, divisor) in [("at extents", 1.0f64), ("zoomed in x12", 12.0)] {
            host.execute(
                "zoom",
                &json!({ "mode": "center", "center": [full_view.center.x, full_view.center.y], "height": full_view.height / divisor }),
            )
            .unwrap();
            host.mesh().unwrap();
            println!("  {label}");
            for n in [100usize, 2_000, handles.len()] {
                let sel: Vec<&String> = handles.iter().take(n).collect();
                host.execute("select", &json!({ "handles": sel })).unwrap();
                host.submit("copy").ok();
                host.submit("0,0").ok();
                let (mut total, mut worst) = (0f64, 0f64);
                let moves = 20;
                for i in 0..moves {
                    host.aim = aad_geom::Vec2::new(f64::from(i) * 400.0, 100.0);
                    let d = overlay_ms(&mut host);
                    total += d;
                    worst = worst.max(d);
                }
                println!(
                    "{:<24} {:>10.1}ms {:>10.1}ms  (running: {})",
                    format!("{n} selected"),
                    total / f64::from(moves),
                    worst,
                    host.session.running.is_some()
                );
                host.cancel_command().ok();
                host.execute("deselect", &json!({})).ok();
            }
        }
        host.execute("zoom", &json!({ "mode": "center", "center": [full_view.center.x, full_view.center.y], "height": full_view.height })).unwrap();
        // A zoom tick is a view change: no new document revision, so this is the pan/zoom path.
        let view = host.session.state().unwrap().view();
        let mut worst = 0f64;
        let mut total = 0f64;
        for i in 0..12 {
            let h = view.height * 0.86f64.powi(i + 1);
            host.execute("zoom", &json!({ "mode": "center", "center": [view.center.x, view.center.y], "height": h })).unwrap();
            let t = std::time::Instant::now();
            host.mesh().unwrap();
            let d = ms(t);
            worst = worst.max(d);
            total += d;
        }
        println!("12 zoom steps: worst {worst:.1} ms, mean {:.1} ms", total / 12.0);
    }
    /// Making the host must not wait on the AI agent: with no MynaOps hosting it, its health check can take the whole timeout, and it
    /// used to run on the UI thread before the window could appear (2 s on every start).
    #[test]
    fn creating_the_host_does_not_wait_for_the_ai_core() {
        // Nothing listens here, and a connection that is dropped rather than refused is what makes a check slow.
        unsafe { std::env::set_var("OSTUDIO_AI_PORT", "9") };
        unsafe { std::env::set_var("OSTUDIO_AI_TOKEN", "test-token") };
        let start = std::time::Instant::now();
        let host = Host::create(false);
        let status = host.assistant.model_status();
        assert!(start.elapsed() < std::time::Duration::from_millis(500), "host creation waited {:?}", start.elapsed());
        assert!(!status.is_empty());
    }

    /// Where the time goes when the host is created (run with --ignored --nocapture).
    #[test]
    #[ignore]
    fn create_cost_by_part() {
        let t = std::time::Instant::now();
        install_formats();
        eprintln!("install_formats {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = aad_llm::available();
        eprintln!("llm available {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = aad_llm::find_model();
        eprintln!("llm find_model {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = osas_ai::status_label();
        eprintln!("osas_ai status {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = assistant::Desk::new();
        eprintln!("Desk::new {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = Host::create(false);
        eprintln!("Host::create {:?}", t.elapsed());
    }

    /// The side tabs ask for the panels about eight times a second. An unchanged drawing answers from the cache; a different
    /// drawing never gets the old answer.
    #[test]
    fn the_panels_text_is_reused_until_the_drawing_changes() {
        use aad_engine::doc::{Common, Drawing, EntityKind, Space, Unknown};
        let mut host = Host::create(false);
        let first = host.panels_json();
        assert_eq!(host.panels_json(), first);
        let mut d = Drawing::new_metric();
        d.add(&Space::Model, Common::default(), EntityKind::Unknown(Unknown { dxf_type: "MLINE".into(), tags: vec![] })).unwrap();
        host.session.open_drawing(d, "other.dxf", None);
        let second = host.panels_json();
        assert_ne!(second, first, "a new drawing must rebuild the panels");
        assert_eq!(host.panels_json(), second);
    }

    /// A save that leaves objects out says which, where the shell can show it (the command history is not enough: a user
    /// who saves from the menu never sees it).
    #[test]
    fn a_save_reports_what_the_file_could_not_keep() {
        use aad_engine::doc::{Common, Drawing, EntityKind, RawTag, Space, Unknown};
        let unknown = |kind: &str| EntityKind::Unknown(Unknown { dxf_type: kind.into(), tags: vec![RawTag { code: 8, value: "0".into() }] });
        let save = |host: &mut Host, name: &str| {
            let path = std::env::temp_dir().join(format!("aad-save-notice-{}-{name}.dxf", std::process::id()));
            host.save_start(Some(path.to_str().unwrap())).unwrap();
            let state = loop {
                let state = host.save_poll().unwrap();
                if state != 1 {
                    break state;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            };
            let _ = std::fs::remove_file(&path);
            state
        };
        let mut host = Host::create(false);
        let mut d = Drawing::new_metric();
        for kind in ["MULTILEADER", "MLINE", "MULTILEADER", "MULTILEADER"] {
            d.add(&Space::Model, Common::default(), unknown(kind)).unwrap();
        }
        host.session.open_drawing(d, "foreign.dxf", None);
        assert_eq!(save(&mut host, "a"), 2);
        assert_eq!(host.save_notice(), "1 MLINE, 3 MULTILEADER");
        // The same text reaches the shell through the FFI.
        let mut buf = [0u8; 128];
        let n = unsafe { ffi::aad_host_save_notice(&mut host as *mut Host, buf.as_mut_ptr(), buf.len() as u32) };
        assert_eq!(std::str::from_utf8(&buf[..usize::try_from(n).unwrap()]).unwrap(), "1 MLINE, 3 MULTILEADER");
        // A buffer that is too small is reported, not truncated.
        let mut tiny = [0u8; 4];
        assert!(unsafe { ffi::aad_host_save_notice(&mut host as *mut Host, tiny.as_mut_ptr(), tiny.len() as u32) } < 0);
        // A drawing the file keeps completely clears the notice, and so does starting the next save.
        host.session.open_drawing(Drawing::new_metric(), "plain.dxf", None);
        assert_eq!(save(&mut host, "b"), 2);
        assert_eq!(host.save_notice(), "");
    }

    /// A fresh folder for one test, under the temporary directory.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aad-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A small DXF made by MynaCAD: one line.
    fn small_dxf(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("plan.dxf");
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[0.0, 0.0], [3.0, 0.0]] })).unwrap();
        host.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
        path
    }

    #[test]
    fn the_status_line_reports_saves_autosaves_and_failures() {
        let dir = scratch("save-status");
        let dxf = small_dxf(&dir);
        let mut host = Host::create(false);
        host.autosave.set_root(dir.join("recovery"));
        assert_eq!(host.control_one("app.open", &json!({ "path": dxf.to_string_lossy() }))["ok"], true);
        host.pump().unwrap();
        assert_eq!(host.app("autosave.status", &json!({})).unwrap()["status"]["kind"], "");
        host.execute("circle", &json!({ "center": [1.0, 1.0], "radius": 1.0 })).unwrap();
        let now = host.app("autosave.now", &json!({ "wait": true })).unwrap();
        assert_eq!((&now["status"]["kind"], &now["snapshots"]), (&json!("autosaved"), &json!(1)), "{now}");
        // The snapshot is in the recovery folder, not beside the drawing, and the project file did not move.
        let native = dir.join("plan.osp");
        let revision = aad_odp::Odp::open(&native).unwrap().revision().unwrap();
        assert_eq!(std::fs::read_dir(dir.join("recovery")).unwrap().count(), 1);
        assert_eq!(host.control_one("app.save", &json!({}))["ok"], true);
        host.pump().unwrap();
        let status = host.app("autosave.status", &json!({})).unwrap();
        assert_eq!(status["status"]["kind"], "saved", "{status}");
        assert!(aad_odp::Odp::open(&native).unwrap().revision().unwrap() > revision);
        assert_eq!(std::fs::read_dir(dir.join("recovery")).unwrap().count(), 0, "a save removes the snapshot");
        // A save that cannot be written is never silent.
        host.execute("circle", &json!({ "center": [4.0, 1.0], "radius": 1.0 })).unwrap();
        let blocked = dir.join("not-a-folder.txt");
        std::fs::write(&blocked, b"x").unwrap();
        let failed = host.control_one("app.save", &json!({ "path": blocked.join("x.dxf").to_string_lossy() }));
        assert_eq!(failed["ok"], false);
        let status = host.app("autosave.status", &json!({})).unwrap();
        assert_eq!(status["status"]["kind"], "failed", "{status}");
        assert!(host.session.log.iter().any(|l| l.starts_with("Save failed:")));
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backups_are_listed_and_restored_through_the_app_methods() {
        let dir = scratch("backups");
        let dxf = small_dxf(&dir);
        let mut host = Host::create(false);
        assert_eq!(host.control_one("app.open", &json!({ "path": dxf.to_string_lossy() }))["ok"], true);
        let native = dir.join("plan.osp");
        let entities = |host: &Host| host.session.state().unwrap().doc.entity_count();
        let first = entities(&host);
        host.execute("circle", &json!({ "center": [1.0, 1.0], "radius": 1.0 })).unwrap();
        assert_eq!(host.control_one("app.save", &json!({}))["ok"], true);
        host.execute("circle", &json!({ "center": [2.0, 1.0], "radius": 1.0 })).unwrap();
        assert_eq!(host.control_one("app.save", &json!({}))["ok"], true);
        let list = host.app("backup.list", &json!({})).unwrap();
        let backups = list["backups"].as_array().unwrap();
        assert_eq!(backups.len(), 2, "{list}");
        // The oldest backup is the file as it was imported (one line).
        let oldest = backups.last().unwrap()["path"].as_str().unwrap().to_string();
        assert!(host.app("backup.restore", &json!({ "backup": dxf.to_string_lossy() })).is_err(), "only a backup of this file");
        let opened = host.app("backup.open", &json!({ "backup": oldest })).unwrap();
        assert_eq!(entities(&host), first, "{opened}");
        assert!(host.session.state().unwrap().path.is_none(), "a backup opens as a copy");
        let restored = host.app("backup.restore", &json!({ "backup": oldest, "path": native.to_string_lossy() })).unwrap();
        assert!(std::path::Path::new(restored["kept"].as_str().unwrap()).is_file());
        let back = aad_engine::nativefile::read_project(&native.to_string_lossy()).unwrap();
        assert_eq!(back.drawing.entity_count(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The child half of [`a_killed_host_leaves_a_snapshot_the_next_start_restores`]: a host that opens a DXF, edits, takes snapshots, edits
    /// again and waits to be killed. Does nothing unless the parent test started it.
    #[test]
    #[ignore = "started as a separate process by a_killed_host_leaves_a_snapshot_the_next_start_restores"]
    fn recovery_child_process() {
        use std::io::Write;
        let Some(dxf) = std::env::var_os("AAD_RECOVERY_CHILD_DXF") else { return };
        let mut host = Host::create(false);
        let open = host.control_one("app.open", &json!({ "path": dxf.to_string_lossy() }));
        assert_eq!(open["ok"], true, "{open}");
        host.execute("circle", &json!({ "center": [1.0, 1.0], "radius": 1.0 })).unwrap();
        host.app("autosave.now", &json!({ "wait": true })).unwrap();
        host.execute("circle", &json!({ "center": [5.0, 1.0], "radius": 2.0 })).unwrap();
        let now = host.app("autosave.now", &json!({ "wait": true })).unwrap();
        assert_eq!(now["snapshots"], 2, "{now}");
        // An edit after the last snapshot: what a crash loses.
        host.execute("line", &json!({ "points": [[0.0, 9.0], [9.0, 9.0]] })).unwrap();
        println!("CHILD-READY {}", host.session.state().unwrap().doc.entity_count());
        let _ = std::io::stdout().flush();
        std::thread::sleep(std::time::Duration::from_secs(300));
    }

    #[test]
    fn a_killed_host_leaves_a_snapshot_the_next_start_restores() {
        use std::io::BufRead;
        let dir = scratch("killed");
        let dxf = small_dxf(&dir);
        let source = std::fs::read(&dxf).unwrap();
        let root = dir.join("recovery");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::recovery_child_process", "--ignored", "--nocapture", "--test-threads=1"])
            .env("AAD_RECOVERY_CHILD_DXF", &dxf)
            .env(aad_engine::recovery::ROOT_ENV, &root)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                // The test harness prints its own "test ... " before the line, on the same line.
                if let Some(n) = line.split("CHILD-READY ").nth(1) {
                    let _ = tx.send(n.trim().parse::<usize>().unwrap_or(0));
                }
            }
        });
        let in_memory = rx.recv_timeout(std::time::Duration::from_secs(120));
        // Killed in the middle of editing: no clean shutdown, the snapshot folder stays.
        child.kill().unwrap();
        let _ = child.wait();
        let in_memory = in_memory.unwrap();
        let native = dir.join("plan.osp");
        let revision = aad_odp::Odp::open(&native).unwrap().revision().unwrap();

        let mut host = Host::create(false);
        host.autosave.set_root(root.clone());
        let list = host.app("recovery.list", &json!({})).unwrap();
        let snapshots = list["snapshots"].as_array().unwrap();
        assert_eq!(snapshots.len(), 1, "{list}");
        let snap = snapshots.first().unwrap();
        assert_eq!(snap["newerThanFile"], true);
        assert_eq!(snap["manifest"]["path"], native.to_string_lossy().as_ref());
        let restored = host.app("recovery.restore", &json!({ "id": snap["id"] })).unwrap();
        let state = host.session.state().unwrap();
        // The two snapshotted circles are back; the line drawn after the last snapshot is lost.
        assert_eq!(state.doc.entity_count(), in_memory - 1, "{restored}");
        assert!(state.is_dirty() && state.path.as_deref() == native.to_str());
        assert_eq!(std::fs::read(&dxf).unwrap(), source, "the source is untouched");
        assert_eq!(aad_odp::Odp::open(&native).unwrap().revision().unwrap(), revision, "nothing is written to the file until a save");
        assert_eq!(host.control_one("app.save", &json!({}))["ok"], true);
        assert!(aad_odp::Odp::open(&native).unwrap().revision().unwrap() > revision);
        assert_eq!(aad_engine::nativefile::read_project(&native.to_string_lossy()).unwrap().drawing.entity_count(), in_memory - 1);
        host.pump().unwrap();
        host.autosave.wait(&host.session, std::time::Duration::from_secs(60));
        host.pump().unwrap();
        assert!(host.app("recovery.list", &json!({})).unwrap()["snapshots"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0, "the restored snapshot is gone after the save");
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Timing probe for the native working copy, autosave and backups (docs/NATIVE-OPEN.md, docs/AUTOSAVE-BACKUP.md), not a pass/fail test:
    /// `AAD_PERF_FILE=<drawing.dwg> AAD_PERF_DIR=<scratch folder> cargo test --release -p aad-host --lib native_open_timing -- --ignored --nocapture`.
    /// The drawing is copied into the scratch folder first, so its working copy is made there and nothing is written beside the original.
    #[test]
    #[ignore]
    fn native_open_timing() {
        let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let src = std::path::Path::new(&path);
        let base = std::env::var_os("AAD_PERF_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("native-open-timing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join(src.file_name().unwrap());
        std::fs::copy(src, &copy).unwrap();
        let copy_s = copy.to_string_lossy().into_owned();
        let size = std::fs::metadata(&copy).unwrap().len();
        let t = std::time::Instant::now();
        let bytes = std::fs::read(&copy).unwrap();
        let read_ms = ms(t);
        let t = std::time::Instant::now();
        let _ = aad_odp::sha256_hex(&bytes);
        println!("source {} MB: read {read_ms:.0} ms, SHA-256 {:.0} ms", size >> 20, ms(t));
        drop(bytes);

        // The old path: parse the DWG or DXF on every open.
        let mut direct = Host::create(false);
        let t = std::time::Instant::now();
        let prepared = aad_engine::cmd::file::read_drawing_file(&copy_s).unwrap();
        aad_engine::cmd::file::finish_open_copy(&mut direct.session, prepared, &copy_s).unwrap();
        direct.fit_view();
        let open_direct = ms(t);
        let t = std::time::Instant::now();
        direct.mesh().unwrap();
        println!("direct (parse every time): open {open_direct:.0} ms + first frame {:.0} ms = {:.0} ms", ms(t), open_direct + ms(t));
        drop(direct);

        for round in ["cold (convert)", "warm (working copy)"] {
            let mut host = Host::create(false);
            host.autosave.set_root(dir.join("recovery"));
            let t = std::time::Instant::now();
            let open = host.control_one("app.open", &json!({ "path": copy_s }));
            let open_ms = ms(t);
            assert_eq!(open["ok"], true, "{open}");
            let t = std::time::Instant::now();
            host.mesh().unwrap();
            let frame_ms = ms(t);
            println!(
                "{round}: open {open_ms:.0} ms + first frame {frame_ms:.0} ms = {:.0} ms; action {}, fromNative {}, stages {}",
                open_ms + frame_ms,
                open["result"]["action"],
                open["result"]["fromNative"],
                open["result"]["ms"]
            );
        }
        let native = copy.with_extension("osp");
        println!("working copy {}: {:.1} MB", native.display(), std::fs::metadata(&native).unwrap().len() as f64 / 1048576.0);

        // Autosave: the cost on the UI thread (start the snapshot) and on the worker (write it), first and after a small edit.
        let mut host = Host::create(false);
        host.autosave.set_root(dir.join("recovery"));
        assert_eq!(host.control_one("app.open", &json!({ "path": copy_s }))["ok"], true);
        let first: Vec<String> = host.session.doc().unwrap().model.iter().take(1).map(|e| e.handle.hex()).collect();
        for label in ["first snapshot", "after a one-entity edit"] {
            host.execute("move", &json!({ "handles": first, "delta": [10.0, 0.0] })).unwrap();
            let t = std::time::Instant::now();
            host.autosave.force = true;
            host.autosave.tick(&host.session, autosave_interval());
            let ui = ms(t);
            host.autosave.wait(&host.session, std::time::Duration::from_secs(600));
            let s = host.app("autosave.status", &json!({})).unwrap();
            println!("autosave {label}: UI thread {ui:.2} ms, worker {}", s["last"]);
        }
        let recovery_bytes: u64 = std::fs::read_dir(dir.join("recovery"))
            .unwrap()
            .flatten()
            .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
            .filter_map(|f| f.metadata().ok())
            .map(|m| m.len())
            .sum();
        println!("recovery folder: {:.1} MB", recovery_bytes as f64 / 1048576.0);
        // Save into the working copy: the backup, the DWG or DXF bytes and the native model.
        let t = std::time::Instant::now();
        let backup = aad_engine::backup::keep_copy(&native, "probe", &aad_engine::backup::Policy::default()).unwrap();
        println!("backup copy alone: {:.0} ms, {:.1} MB", ms(t), std::fs::metadata(&backup).unwrap().len() as f64 / 1048576.0);
        let t = std::time::Instant::now();
        let saved = host.control_one("app.save", &json!({}));
        println!("save into the working copy (backup + drawing + native model): {:.0} ms, ok {}", ms(t), saved["ok"]);
        if saved["ok"] != true {
            println!("  {saved}");
        }
        println!("working copy after save: {:.1} MB", std::fs::metadata(&native).unwrap().len() as f64 / 1048576.0);
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Zoom and pan sweep on a real drawing, headless: what the UI thread spends in `Host::mesh` per step (the GPU upload and present are not
    /// included), and when a worker rebuilds the list, the step that swaps it in. Not a pass/fail test:
    /// `AAD_PERF_FILE=<drawing> cargo test --release -p aad-host --lib zoom_sweep_timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn zoom_sweep_timing() {
        let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
        let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("x.dwg").to_string();
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut host = Host::create(false);
        let drawing = aad_io::read(&std::fs::read(&path).unwrap(), &name).unwrap();
        host.session.open_drawing(drawing, &name, None);
        host.execute("zoom.extents", &json!({})).unwrap();
        let t = std::time::Instant::now();
        host.mesh().unwrap();
        let (_, bytes) = host.frame.mesh_totals();
        println!("first frame {:.0} ms, {} tiles, CPU mesh {:.1} MB", ms(t), host.frame.tiles_total, bytes as f64 / 1048576.0);
        let full = host.session.state().unwrap().view();
        // Steps: the UI-thread time of the step itself, then (when a worker list build was started) of the step that swaps it in.
        let mut steps: Vec<f64> = Vec::new();
        let mut swaps: Vec<f64> = Vec::new();
        let mut sync_builds = 0usize;
        let step = |host: &mut Host, center: Vec2, height: f64, steps: &mut Vec<f64>, swaps: &mut Vec<f64>, sync_builds: &mut usize| {
            host.execute("zoom", &json!({ "mode": "center", "center": [center.x, center.y], "height": height })).unwrap();
            let before = host.frame.list_build_ms;
            let t = std::time::Instant::now();
            host.mesh().unwrap();
            steps.push(ms(t));
            if host.frame.list_build_ms != before {
                *sync_builds += 1;
            }
            let mut waited = 0;
            while host.frame.pending.is_some() && waited < 20_000 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                waited += 2;
                let t = std::time::Instant::now();
                host.mesh().unwrap();
                if host.frame.pending.is_none() {
                    swaps.push(ms(t));
                }
            }
        };
        // Zoom in on a point a third of the way across, 24 notches, then back out.
        let aim = Vec2::new(full.center.x + full.height * 0.3, full.center.y + full.height * 0.1);
        let mut h = full.height;
        let mut c = full.center;
        for _ in 0..24 {
            h *= 0.8;
            c = aim + (c - aim) * 0.8;
            step(&mut host, c, h, &mut steps, &mut swaps, &mut sync_builds);
        }
        for _ in 0..24 {
            h /= 0.8;
            c = aim + (c - aim) / 0.8;
            step(&mut host, c, h, &mut steps, &mut swaps, &mut sync_builds);
        }
        // One drag at a mid zoom: 20 moves of 5 % of the view width each.
        let h = full.height / 16.0;
        let mut c = aim;
        step(&mut host, c, h, &mut steps, &mut swaps, &mut sync_builds);
        for _ in 0..20 {
            c.x += h * 1.5 * 0.05;
            step(&mut host, c, h, &mut steps, &mut swaps, &mut sync_builds);
        }
        let stats = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let at = |q: f64| v.get(((v.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
            format!("n {} p50 {:.1} p95 {:.1} max {:.1} ms", v.len(), at(0.5), at(0.95), at(1.0))
        };
        println!("sweep steps (UI thread): {}; synchronous list builds {sync_builds}", stats(&mut steps));
        println!("worker list swapped in (UI thread): {}", stats(&mut swaps));
        // Edits after a zoom: move one entity, then undo, ten times.
        let handle = host.session.doc().unwrap().model.iter().next().map(|e| e.handle.hex()).unwrap();
        let mut edits = Vec::new();
        for _ in 0..10 {
            for cmd in ["move", "undo"] {
                let t = std::time::Instant::now();
                if cmd == "move" {
                    host.execute("move", &json!({ "handles": [handle], "delta": [10.0, 0.0] })).unwrap();
                } else {
                    host.execute("undo", &json!({})).unwrap();
                }
                host.mesh().unwrap();
                edits.push(ms(t));
            }
        }
        println!("edit + undo x10 after the sweep (engine + mesh): {}", stats(&mut edits));
        let (_, bytes) = host.frame.mesh_totals();
        println!("CPU mesh at the end: {:.1} MB in {} tiles", bytes as f64 / 1048576.0, host.frame.tiles_total);
    }

    /// Wait for a worker list build and swap it in (what the next pumps do).
    fn settle(host: &mut Host) {
        for _ in 0..5000 {
            if host.frame.pending.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            host.mesh().unwrap();
        }
        panic!("the worker list build did not finish");
    }

    #[test]
    fn zoom_band_and_pan_cell_changes_build_on_the_worker_and_keep_the_scene() {
        let mut host = Host::create(false);
        host.execute("line", &json!({ "points": [[-100.0, 0.0], [-90.0, 0.0]] })).unwrap();
        host.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).unwrap();
        let mid = host.execute("line", &json!({ "points": [[800.0, 0.0], [810.0, 0.0]] })).unwrap();
        host.execute("line", &json!({ "points": [[900.0, 0.0], [910.0, 0.0]] })).unwrap();
        host.execute("zoom", &json!({ "mode": "center", "center": [5.0, 0.0], "height": 100.0 })).unwrap();
        host.mesh().unwrap();
        assert!(host.frame.mesh_tiled && host.frame.scene.is_some());
        let built = host.frame.list_build_ms;
        // Another zoom band: the list on screen stays, the next one is built on the worker with its tiles.
        host.execute("zoom", &json!({ "mode": "center", "center": [5.0, 0.0], "height": 30.0 })).unwrap();
        host.mesh().unwrap();
        assert!(host.frame.pending.is_some(), "a band change goes to the worker");
        settle(&mut host);
        assert_eq!(host.frame.list_build_ms, built, "no list was built on the UI thread");
        assert!(host.frame.scene.is_some(), "the scene survives the band change");
        assert_eq!(host.frame.list_key.2, aad_render::tessellation_band(30.0 / f64::from(host.height)));
        assert!(host.frame.tile_edge.is_some_and(|(band, _)| band == host.frame.list_key.2));
        let key = host.mesh().unwrap().0;
        assert_eq!(host.frame.tiles_remeshed, 0, "the tiles came from the worker");
        assert_eq!(host.mesh().unwrap().0, key);
        // A pan into another cell: a worker build too, never a synchronous one.
        host.execute("zoom", &json!({ "mode": "center", "center": [600.0, 0.0], "height": 30.0 })).unwrap();
        host.mesh().unwrap();
        assert_eq!(host.frame.list_build_ms, built, "a pan cell change does not build on the UI thread");
        settle(&mut host);
        assert_eq!(host.frame.list_build_ms, built);
        // An edit after the zoom and pan is a delta rebuild from the kept scene: one tile remeshed.
        let handles = mid.get("handles").cloned().unwrap_or(Value::Null);
        host.execute("move", &json!({ "handles": handles, "delta": [2.0, 0.0] })).unwrap();
        host.mesh().unwrap();
        assert!(host.frame.scene.is_some());
        assert_eq!(host.frame.tiles_remeshed, 1, "only the edited tile");
    }

    /// The geometry database on a real drawing: a full build, then the incremental update after a one-entity edit, as `geodb.build` runs them
    /// on the session thread (not persisted). Not a pass/fail test:
    /// `AAD_PERF_FILE=<drawing> cargo test --release -p aad-host --lib geodb_timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn geodb_timing() {
        let Ok(path) = std::env::var("AAD_PERF_FILE") else { return };
        let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("x.dwg").to_string();
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut host = Host::create(false);
        let drawing = aad_io::read(&std::fs::read(&path).unwrap(), &name).unwrap();
        host.session.open_drawing(drawing, &name, None);
        let t = std::time::Instant::now();
        let full = host.execute("geodb.build", &json!({ "persist": false, "units": "mm", "wait": true })).unwrap();
        println!("geodb.build full: {:.0} ms, mode {}, timings {}", ms(t), full["mode"], full["summary"]["timings"]);
        let handle = host.session.doc().unwrap().model.iter().next().map(|e| e.handle.hex()).unwrap();
        for _ in 0..3 {
            host.execute("move", &json!({ "handles": [handle], "delta": [10.0, 0.0] })).unwrap();
            let t = std::time::Instant::now();
            host.execute("geodb.build", &json!({ "persist": false })).unwrap();
            let ui = ms(t);
            while host.geodb_job.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(1));
                host.poll_geodb();
            }
            let line = host.session.log.last().cloned().unwrap_or_default();
            println!("geodb.build after a one-entity move: UI thread {ui:.1} ms, done after {:.0} ms: {line}", ms(t));
        }
    }

    #[test]
    fn geodb_build_runs_on_a_worker_and_other_geodb_commands_wait_for_it() {
        let mut host = Host::create(false);
        for i in 0..4 {
            let x = f64::from(i) * 3000.0;
            host.execute("line", &json!({ "points": [[x, 0.0], [x + 2000.0, 0.0]] })).unwrap();
            host.execute("line", &json!({ "points": [[x, 230.0], [x + 2000.0, 230.0]] })).unwrap();
        }
        let started = host.execute("geodb.build", &json!({ "persist": false, "units": "mm" })).unwrap();
        assert_eq!(started["started"], true, "{started}");
        assert!(host.execute("geodb.report", &json!({})).is_err(), "the model is with the worker");
        for _ in 0..10_000 {
            if host.geodb_job.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            host.pump().unwrap();
        }
        assert!(host.geodb_job.is_none(), "the build finished");
        assert!(host.session.log.iter().any(|l| l.starts_with("Geometry database (full")), "{:?}", host.session.log);
        assert!(host.execute("geodb.report", &json!({})).is_ok());
        // The incremental update after an edit, and the synchronous form for scripts.
        host.execute("line", &json!({ "points": [[0.0, 900.0], [500.0, 900.0]] })).unwrap();
        let sync = host.execute("geodb.build", &json!({ "persist": false, "wait": true })).unwrap();
        assert_eq!(sync["mode"], "incremental", "{sync}");
    }
}

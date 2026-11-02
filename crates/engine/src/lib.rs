//! The AAD engine.
//!
//! Every user-visible action is a command with a stable id (command names in lower
//! case: `line`, `circle`, `zoom`, `layer`…, plus dotted ids for UI operations). Commands run two
//! ways: programmatically with JSON parameters ([`Session::execute`], no dialogs, used by the
//! control channel, MCP, CLI and scripts) and interactively through prompts ([`Session::start`],
//! [`Session::input`], [`Session::cmdline`]) exactly like typing at command line.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod annot;
pub mod assoc;
pub mod audit;
pub mod calc;
pub mod cmd;
pub mod dyninput;
mod finite;
pub mod flatten;
pub mod forms;
pub mod grips;
mod guard;
pub mod pointmod;
pub mod pomodoro;
pub mod projectplan;
pub mod prompt;
mod protect;
pub mod revision;
pub mod sample;
pub mod select;
pub mod snap;
pub mod spatial;
pub mod sysvars;
pub mod units;

use std::sync::Arc;

use crate::cmd::Factory;

use aad_doc::{Common, Drawing, Entity, EntityKind, Handle, Space};
use aad_geom::{Bounds2, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use aad_doc as doc;
pub use aad_geom as geom;
pub use aad_render as render;
pub use cmd::smartblock::property_fields as smart_block_fields;
pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};

/// Lintel height, in metres, of the active floor. Drawings with no floors use 2.1 m.
pub fn standard_lintel(doc: &doc::Drawing) -> f64 {
    cmd::project::standard_lintel(doc)
}
pub use prompt::{Accept, Input, Interactive, Prompt, Step};

/// Drawn cursor in the drawing area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// Specify a point — full crosshairs, no pickbox.
    Crosshairs,
    /// Select objects — small square pickbox only (`PICKBOX`).
    Pickbox,
    /// No command active — crosshairs with pickbox at the intersection.
    CrosshairsAndPickbox,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no active document")]
    NoDocument,
    #[error("{0}")]
    Other(String),
    /// A command panicked; the guard kept the drawing as it was (a bug: please report it).
    #[error("internal error in `{0}` (the drawing was kept as it was): {1}")]
    Internal(String, String),
}

impl From<aad_doc::DocError> for EngineError {
    fn from(e: aad_doc::DocError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// A 2D view of a space: world centre and visible height in drawing units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub center: Vec2,
    pub height: f64,
}

impl Default for View {
    fn default() -> Self {
        View { center: Vec2::new(6.0, 4.5), height: 10.0 }
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub label: String,
    pub doc: Arc<Drawing>,
    pub selection: Vec<Handle>,
}

/// One open drawing.
#[derive(Clone, Debug)]
pub struct DocState {
    pub doc: Arc<Drawing>,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    pub selection: Vec<Handle>,
    pub previous_selection: Vec<Handle>,
    pub path: Option<String>,
    pub title: String,
    pub saved: Arc<Drawing>,
    pub revision: u64,
    pub space: Space,
    /// MSPACE: the layout viewport whose model space is being edited (None = paper space).
    pub mspace: Option<Handle>,
    pub views: Vec<(Space, View)>,
    pub view_history: Vec<View>,
    pub uid: u64,
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl DocState {
    pub fn new(doc: Drawing, title: &str, path: Option<String>) -> Self {
        let doc = Arc::new(doc);
        DocState {
            saved: doc.clone(),
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            selection: Vec::new(),
            previous_selection: Vec::new(),
            path,
            title: title.into(),
            revision: 1,
            space: Space::Model,
            mspace: None,
            views: Vec::new(),
            view_history: Vec::new(),
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.doc, &self.saved)
    }
    /// After undo/redo replaced the drawing (`before` is the one shown until then): if the current
    /// layout no longer exists, follow it to its other name when it was renamed (same tab order,
    /// a name `before` did not have), else go to the Model tab.
    fn follow_layout(&mut self, before: &Drawing) {
        let Space::Paper(name) = &self.space else { return };
        if self.doc.layouts.iter().any(|l| l.name == *name) {
            return;
        }
        let renamed = before
            .layouts
            .iter()
            .find(|l| l.name == *name)
            .and_then(|old| self.doc.layouts.iter().find(|l| l.tab_order == old.tab_order && !before.layouts.iter().any(|b| b.name == l.name)));
        match renamed.map(|l| Space::Paper(l.name.clone())) {
            Some(to) => {
                let from = self.space.clone();
                for (sp, _) in self.views.iter_mut() {
                    if *sp == from {
                        *sp = to.clone();
                    }
                }
                self.space = to;
            }
            None => {
                self.space = Space::Model;
                self.mspace = None;
            }
        }
    }
    /// The space edits, picks and snaps act on: model space inside an active viewport.
    pub fn edit_space(&self) -> Space {
        if self.active_viewport().is_some() { Space::Model } else { self.space.clone() }
    }
    /// The active MSPACE viewport, if it still exists in the current layout.
    pub fn active_viewport(&self) -> Option<(Handle, aad_doc::Viewport)> {
        let h = self.mspace?;
        let e = self.doc.space(&self.space)?.get(h)?;
        match &e.kind {
            aad_doc::EntityKind::Viewport(v) if v.height > 1e-12 && v.view_height > 1e-12 => Some((h, v.clone())),
            _ => None,
        }
    }
    pub fn paper_view(&self) -> View {
        self.views.iter().find(|(s, _)| *s == self.space).map(|(_, v)| *v).unwrap_or_default()
    }
    /// The screen view. Inside an MSPACE viewport this is the model-space view that maps the
    /// screen consistently with the paper view, so picks, snaps and previews work in model units.
    pub fn view(&self) -> View {
        let pv = self.paper_view();
        match self.active_viewport() {
            Some((_, vp)) => {
                let k = vp.view_height / vp.height;
                View { center: vp.view_center + (pv.center - vp.center.xy()) * k, height: pv.height * k }
            }
            None => pv,
        }
    }
    /// Map a requested screen view to the paper view, or (in MSPACE) to the viewport's model view.
    fn store_view(&mut self, v: View) {
        if let Some((h, vp)) = self.active_viewport() {
            let pv = self.paper_view();
            let k = v.height / pv.height.max(1e-12);
            if vp.locked {
                // A locked viewport keeps its scale: pan/zoom the sheet instead.
                let k0 = vp.view_height / vp.height;
                let pc = vp.center.xy() + (v.center - vp.view_center) / k0;
                self.put_paper_view(View { center: pc, height: v.height / k0 });
            } else {
                let center = v.center - (pv.center - vp.center.xy()) * k;
                let height = vp.height * k;
                let doc = Arc::make_mut(&mut self.doc);
                if let Some(store) = doc.space_mut(&self.space.clone()) {
                    store.modify(h, |e| {
                        if let aad_doc::EntityKind::Viewport(x) = &mut e.kind {
                            x.view_center = center;
                            x.view_height = height;
                        }
                    });
                }
                // The viewport shows the model through this view, so the drawn content changes: redraw
                // it now. (Without this the display list kept the old view until another edit.)
                self.revision += 1;
            }
            return;
        }
        self.put_paper_view(v);
    }
    fn put_paper_view(&mut self, v: View) {
        match self.views.iter_mut().find(|(s, _)| *s == self.space) {
            Some((_, slot)) => *slot = v,
            None => self.views.push((self.space.clone(), v)),
        }
    }
    pub fn set_view(&mut self, v: View) {
        if !(v.center.is_finite() && v.height.is_finite() && v.height > 1e-12) {
            return;
        }
        let old = self.view();
        if old != v {
            self.view_history.push(old);
            if self.view_history.len() > 50 {
                self.view_history.remove(0);
            }
        }
        self.store_view(v);
    }
    /// The active MSPACE viewport when ZOOM works on its own view: extents and
    /// windows fit the viewport, and zooms keep its centre. A locked viewport keeps its scale, so
    /// zooming it moves the sheet instead (see `store_view`).
    pub fn zoom_viewport(&self) -> Option<aad_doc::Viewport> {
        self.active_viewport().map(|(_, v)| v).filter(|v| !v.locked && v.width > 1e-12)
    }
    /// The view ZOOM starts from: the model view of the active unlocked viewport, else [`Self::view`].
    pub fn zoom_frame(&self) -> View {
        match self.zoom_viewport() {
            Some(v) => View { center: v.view_center, height: v.view_height },
            None => self.view(),
        }
    }
    /// Width / height of the area ZOOM fits into: the active unlocked viewport, else `screen`.
    pub fn zoom_aspect(&self, screen: f64) -> f64 {
        self.zoom_viewport().map_or(screen, |v| v.width / v.height)
    }
    /// Set the view ZOOM works on (see [`Self::zoom_frame`]), recording view history.
    pub fn set_zoom_frame(&mut self, f: View) {
        let v = match self.zoom_viewport() {
            // The screen view that `store_view` maps to this model view of the viewport.
            Some(vp) => {
                let pv = self.paper_view();
                let k = f.height / vp.height;
                View { center: f.center + (pv.center - vp.center.xy()) * k, height: pv.height * k }
            }
            None => f,
        };
        self.set_view(v);
    }
    /// Set without recording view history (realtime pan/zoom frames).
    pub fn set_view_quiet(&mut self, v: View) {
        if !(v.center.is_finite() && v.height.is_finite() && v.height > 1e-12) {
            return;
        }
        self.store_view(v);
    }
}

/// Session-level drafting settings (the system variables stored in the registry/profile).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub osmode: u32,
    pub orthomode: bool,
    pub polarmode: bool,
    /// POLARANG (radians).
    pub polarang: f64,
    /// OSNAPHATCH: object snaps find hatch objects (off by default).
    pub osnaphatch: bool,
    pub dynmode: bool,
    /// DYNPIFORMAT = 1: Dynamic Input shows second and next points as Cartesian `x,y` instead of
    /// polar `distance<angle` (the default).
    pub dynpi_cartesian: bool,
    /// DYNPICOORDS = 1: second and next points typed into Dynamic Input are absolute instead of
    /// relative to the last point (the default).
    pub dynpi_absolute: bool,
    pub lwdisplay: bool,
    pub transparency_display: bool,
    pub selection_cycling: bool,
    pub otrack: bool,
    /// PICKBOX in pixels.
    pub pickbox: f64,
    /// APERTURE in pixels.
    pub aperture: f64,
    pub pickfirst: bool,
    pub pickadd: bool,
    pub gripsize: f64,
    pub cursorsize: f64,
    /// MAXARRAY: the most objects one array command creates (items × selected objects).
    pub maxarray: u64,
    pub isodraft: bool,
    pub annoallvisible: bool,
    pub annoautoscale: bool,
    /// QPMODE: show the Quick Properties palette when objects are selected.
    pub qpmode: bool,
    /// POLARMODE bits ([`snap::tracking::polarmode`]); polar tracking itself is on while
    /// `polarmode` is (AUTOSNAP bit 8, F10).
    pub polar_flags: u32,
    /// POLARADDANG: additional polar tracking angles (radians), used with POLARMODE bit 4.
    pub polaraddang: Vec<f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            osmode: 4133,
            orthomode: false,
            polarmode: true,
            polarang: std::f64::consts::FRAC_PI_2,
            osnaphatch: false,
            dynmode: true,
            dynpi_cartesian: false,
            dynpi_absolute: false,
            lwdisplay: false,
            transparency_display: false,
            selection_cycling: false,
            otrack: true,
            pickbox: 3.0,
            aperture: 10.0,
            pickfirst: true,
            pickadd: true,
            gripsize: 5.0,
            cursorsize: 5.0,
            maxarray: 100_000,
            isodraft: false,
            annoallvisible: true,
            annoautoscale: false,
            qpmode: false,
            polar_flags: 0,
            polaraddang: Vec::new(),
        }
    }
}

/// An interactive command in progress.
pub struct Running {
    pub id: String,
    pub machine: Box<dyn Interactive>,
    pub before: Arc<Drawing>,
    pub selection_before: Vec<Handle>,
}

/// A pending window/crossing selection started by a click on empty space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PendingWindow {
    pub corner: Vec2,
    pub during_command: bool,
}

/// Options and values the interactive commands remember for the rest of the session, offered as
/// the `<default>` the next time they ask (as does). Values keeps in a drawing
/// system variable (HPNAME, OFFSETDIST, FILLETRAD, CHAMFERA, POLYSIDES…) live in the drawing
/// header instead. JSON calls never read these: their explicit parameters and documented defaults
/// stay authoritative.
#[derive(Clone, Debug, PartialEq)]
pub struct LastUsed {
    /// HATCH/GRADIENT starts at "Select objects" instead of "Pick internal point".
    pub hatch_select: bool,
    /// POLYGON: circumscribed about the circle (the `C` option) instead of inscribed.
    pub polygon_circumscribed: bool,
    /// ROTATE angle (radians).
    pub rotate_angle: f64,
    /// SCALE factor.
    pub scale_factor: f64,
    /// MLEADER placement order and Options.
    pub mleader: MLeaderOptions,
    /// MLEADER's block content name (Options > Content type > Block).
    pub mleader_block: String,
}

impl Default for LastUsed {
    fn default() -> Self {
        LastUsed {
            hatch_select: false,
            polygon_circumscribed: false,
            rotate_angle: 0.0,
            scale_factor: 1.0,
            mleader: MLeaderOptions::default(),
            mleader_block: String::new(),
        }
    }
}

/// Which part of a multileader MLEADER places first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaderOrder {
    #[default]
    Arrowhead,
    Landing,
    Content,
}

/// The MLEADER placement order and the values set at its Options prompt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MLeaderOptions {
    pub order: LeaderOrder,
    /// Maximum points of the leader line (2..=64).
    pub max_points: usize,
    /// First and second angle constraints (radians; 0 = free).
    pub angles: [f64; 2],
    /// Draw the landing line (dogleg).
    pub landing: bool,
    /// Ask for multiline text content (Content type None turns it off).
    pub content: bool,
    /// Leader lines: straight, spline, or none (content only).
    pub leader_type: LeaderType,
    /// The content is a block (`LastUsed::mleader_block`) instead of multiline text.
    pub block: bool,
    /// The block's extents centre (not its insertion point) sits at the end of the landing.
    pub block_center: bool,
}

/// MLEADER's leader line type (Options > Leader type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaderType {
    #[default]
    Straight,
    Spline,
    None,
}

impl Default for MLeaderOptions {
    fn default() -> Self {
        MLeaderOptions {
            order: LeaderOrder::Arrowhead,
            max_points: 2,
            angles: [0.0; 2],
            landing: true,
            content: true,
            leader_type: LeaderType::Straight,
            block: false,
            block_center: true,
        }
    }
}

/// The editor session: open drawings, settings, the running command and the command log.
pub struct Session {
    pub docs: Vec<DocState>,
    pub active: usize,
    pub settings: Settings,
    pub running: Option<Running>,
    pub log: Vec<String>,
    pub last_command: Option<String>,
    /// The form (and values already given) the next form-driven command starts with.
    pub(crate) form_preset: Option<(&'static forms::Form, Value)>,
    /// LASTPOINT.
    pub last_point: Vec2,
    /// Point modifiers pending at the current point prompt (`FROM`, `M2P`, `.x`, `<a`…).
    pub point_mods: Vec<pointmod::Frame>,
    /// Cursor position in world coordinates (from the UI; used for direct distance entry).
    pub cursor: Vec2,
    /// The deferred tangent/perpendicular snap under the cursor, if any (from the UI; lets the
    /// rubber band show the line it resolves to).
    pub cursor_deferred: Option<snap::Deferred>,
    /// Object snap tracking: acquired points and the path under the cursor (from the UI's
    /// [`Session::snap_cursor`] calls; direct distance entry follows the path).
    pub tracking: snap::tracking::Tracker,
    /// Viewport size in pixels (from the UI; used for zoom and pick apertures).
    pub viewport_px: (f64, f64),
    pub clipboard: Vec<Entity>,
    pub clipboard_base: Vec2,
    pub pending_window: Option<PendingWindow>,
    /// What flattening did to the drawing opened last (see [`flatten`]).
    pub last_flatten: Option<flatten::FlattenReport>,
    pub untitled_counter: u32,
    /// The last dimension created (DIMCONTINUE / DIMBASELINE).
    pub last_dim: Option<Handle>,
    /// Options remembered between invocations of interactive commands.
    pub last_used: LastUsed,
    /// Where `clipboard` came from and the layers, styles and blocks its objects need.
    pub clipboard_source: Option<cmd::clipboard::ClipSource>,
    /// The link to the OSAS company file, once `companyfile` has opened it. Read-only.
    pub(crate) office: Option<aad_office::Link>,
    /// A plot window chosen by `plot.window` for the application to finish (it asks where to save,
    /// then calls `plot`). Taken with [`Session::take_plot_request`].
    pub plot_request: Option<PlotRequest>,
    /// Node results of the last parametric evaluation (`param.*`); disposable, never saved.
    pub param_cache: aad_param::Cache,
    /// O-Dynamo companion pipe state (`companion.*`); disposable, never saved.
    pub companion: cmd::companion::CompanionState,
    /// History milestone preview overlay (`history.preview`); disposable, never saved.
    pub history_preview: Option<cmd::history::HistoryPreviewState>,
    /// Open 2D block editor (`bedit` / `refedit`); `None` when not editing.
    pub block_edit: Option<cmd::bedit::BlockEdit>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

/// Split a typed line at spaces, keeping a number together with a unit typed after a space: `5 m,0`
/// is the one token `5m,0`, and `line 0,0 2 ft<45` ends in `2ft<45`.
fn split_tokens(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in line.split_whitespace() {
        let ends_in_number = out.last().is_some_and(|prev| prev.ends_with(|c: char| c.is_ascii_digit() || c == '.'));
        if ends_in_number && units::starts_with_unit_word(tok) {
            if let Some(prev) = out.last_mut() {
                prev.push_str(tok);
            }
        } else {
            out.push(tok.to_string());
        }
    }
    out
}

impl Session {
    pub fn new() -> Self {
        let mut s = Session::empty();
        s.new_drawing(false);
        s
    }
    /// A session with no open drawing (the Start tab).
    pub fn empty() -> Self {
        Session {
            docs: Vec::new(),
            active: 0,
            settings: Settings::default(),
            running: None,
            log: Vec::new(),
            last_command: None,
            last_point: Vec2::ZERO,
            point_mods: Vec::new(),
            cursor: Vec2::ZERO,
            cursor_deferred: None,
            tracking: snap::tracking::Tracker::default(),
            viewport_px: (1200.0, 800.0),
            clipboard: Vec::new(),
            clipboard_base: Vec2::ZERO,
            pending_window: None,
            form_preset: None,
            last_flatten: None,
            untitled_counter: 0,
            last_dim: None,
            last_used: LastUsed::default(),
            clipboard_source: None,
            office: None,
            plot_request: None,
            param_cache: aad_param::Cache::default(),
            companion: cmd::companion::CompanionState::default(),
            history_preview: None,
            block_edit: None,
        }
    }
    pub fn new_drawing(&mut self, metric: bool) -> usize {
        self.untitled_counter += 1;
        let d = if metric { Drawing::new_metric() } else { Drawing::new_imperial() };
        let mut st = DocState::new(d, &format!("Drawing{}", self.untitled_counter), None);
        st.set_view_quiet(if metric { View { center: Vec2::new(210.0, 148.5), height: 297.0 } } else { View::default() });
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.active
    }
    pub fn open_drawing(&mut self, d: Drawing, title: &str, path: Option<String>) -> usize {
        let mut st = DocState::new(d, title, path);
        let ext = st.doc.extents(&Space::Model);
        if !ext.is_empty() {
            st.set_view_quiet(View {
                center: ext.center(),
                height: (ext.height().max(ext.width() * self.viewport_px.1 / self.viewport_px.0.max(1.0)) * 1.1).max(1e-6),
            });
        }
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.active
    }

    pub fn state(&self) -> Result<&DocState> {
        self.docs.get(self.active).ok_or(EngineError::NoDocument)
    }
    pub fn state_mut(&mut self) -> Result<&mut DocState> {
        self.docs.get_mut(self.active).ok_or(EngineError::NoDocument)
    }
    pub fn doc(&self) -> Result<&Drawing> {
        Ok(&self.state()?.doc)
    }
    /// Copy-on-write access to the drawing.
    pub fn doc_mut(&mut self) -> Result<&mut Drawing> {
        Ok(Arc::make_mut(&mut self.state_mut()?.doc))
    }
    /// The space commands act on (model space while working inside a layout viewport).
    pub fn space(&self) -> Space {
        self.state().map(|s| s.edit_space()).unwrap_or_default()
    }
    /// The tab being displayed: Model or a layout, regardless of MSPACE.
    pub fn layout_space(&self) -> Space {
        self.state().map(|s| s.space.clone()).unwrap_or_default()
    }
    pub fn selection(&self) -> Vec<Handle> {
        self.state().map(|s| s.selection.clone()).unwrap_or_default()
    }
    pub fn set_selection(&mut self, sel: Vec<Handle>) {
        if let Ok(st) = self.state_mut() {
            let mut seen = std::collections::HashSet::new();
            st.selection = sel.into_iter().filter(|h| seen.insert(*h)).collect();
        }
    }

    /// Message on the command line history.
    pub fn echo(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        log::debug!("{m}");
        self.log.push(m);
        if self.log.len() > 5000 {
            self.log.drain(..1000);
        }
    }

    /// Common properties for new objects (CLAYER, CECOLOR, CELTYPE, CELWEIGHT, CELTSCALE).
    pub fn current_common(&self) -> Common {
        let Ok(d) = self.doc() else { return Common::default() };
        let h = &d.header;
        Common {
            layer: h.str("CLAYER", "0"),
            color: aad_color::Color::from_aci(h.i64("CECOLOR", 256) as i16),
            linetype: h.str("CELTYPE", "ByLayer"),
            lineweight: aad_doc::Lineweight::from_dxf(h.i64("CELWEIGHT", -1) as i16),
            ltscale: h.f64("CELTSCALE", 1.0),
            plot_style: h.str("CPLOTSTYLE", "ByLayer"),
            ..Common::default()
        }
    }

    /// Add an entity to the current space with current properties.
    pub fn add_entity(&mut self, kind: EntityKind) -> Result<Handle> {
        let common = self.current_common();
        let space = self.space();
        let true_color = self.doc()?.header.get("CECOLOR_RGB").and_then(|v| v.as_i64());
        let mut common = common;
        if let Some(rgb) = true_color {
            common.color = aad_color::Color::True(aad_color::Rgb::from_u32(rgb as u32));
        }
        let h = self.doc_mut()?.add(&space, common, kind)?;
        Ok(h)
    }

    /// World units per pixel at the current view.
    pub fn pixel_size(&self) -> f64 {
        let v = self.state().map(|s| s.view()).unwrap_or_default();
        v.height / self.viewport_px.1.max(1.0)
    }

    // ---------------- programmatic execution ----------------

    /// Run a command with JSON parameters (no dialogs). Records undo when the drawing changes.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        (spec.enabled)(self).map_err(|m| EngineError::Disabled(spec.id.into(), m))?;
        let before = self.state().ok().map(|s| (s.doc.clone(), s.selection.clone(), s.uid));
        let run = spec.run;
        // Lengths typed with a unit (`"230mm"`, `"3'"`) become numbers in the field's unit first.
        let converted = forms::normalize_lengths(spec.id, params, self.doc().ok().and_then(annot::drawing_unit));
        let params = converted.as_ref().unwrap_or(params);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params)));
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg =
                    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                // Restore the drawing as it was.
                if let Some((doc, sel, uid)) = &before
                    && let Some(st) = self.docs.iter_mut().find(|d| d.uid == *uid)
                {
                    st.doc = doc.clone();
                    st.selection = sel.clone();
                }
                return Err(EngineError::Internal(id.into(), msg));
            }
        };
        cmd::groups::after_command(self, before.as_ref().map(|b| &b.0));
        assoc::after_command(self, before.as_ref().map(|b| &b.0), before.as_ref().map(|b| b.2));
        let result = match (result, &before) {
            (Ok(v), Some((doc, sel, uid))) if spec.undoable => {
                self.refuse_non_finite(Some(*uid), doc, sel, spec.id).and_then(|()| self.refuse_protected(Some(*uid), doc, sel, spec.id)).map(|()| v)
            }
            (r, _) => r,
        };
        if spec.undoable
            && let Some((doc, sel, uid)) = before
            && let Some(st) = self.docs.iter_mut().find(|d| d.uid == uid)
            && !Arc::ptr_eq(&doc, &st.doc)
        {
            st.undo.push(Snapshot { label: spec.label.to_string(), doc, selection: sel });
            st.redo.clear();
            st.revision += 1;
            if st.undo.len() > 2000 {
                st.undo.remove(0);
            }
        }
        result
    }

    /// Bump the revision without an undo step (view-only or settings changes that redraw).
    pub fn touch(&mut self) {
        if let Ok(st) = self.state_mut() {
            st.revision += 1;
        }
    }

    // ---------------- interactive commands ----------------

    /// Start a command by name or alias as if typed at the command line.
    pub fn start(&mut self, name: &str) -> Result<()> {
        self.start_inner(name, None)
    }

    /// Start `id` through its input form with `preset` already answered (a dialog collected it).
    /// Only the points still missing are asked on the drawing.
    pub fn start_form(&mut self, id: &str, preset: Value) -> Result<()> {
        self.start_inner(id, Some(preset))
    }

    fn start_inner(&mut self, name: &str, force: Option<Value>) -> Result<()> {
        let name = name.trim();
        let (transparent, name) = match name.strip_prefix('\'') {
            Some(n) => (true, n),
            None => (false, name),
        };
        let lower = name.trim_start_matches(['_', '.', '-']).to_ascii_lowercase();
        // A command-line form with an id of its own (-ATTEDIT) is not the command it prefixes.
        let dashed = cmd::resolve_alias(&name.trim_start_matches(['_', '.']).to_ascii_lowercase());
        let id = if find_command(&dashed).is_some() { dashed } else { cmd::resolve_alias(&lower) };
        let spec = find_command(&id).ok_or_else(|| EngineError::UnknownCommand(name.to_string()))?;
        if transparent && spec.transparent && self.running.is_some() {
            // Transparent commands (zoom, pan…) run without cancelling the active one.
            if spec.interactive.is_none() {
                self.echo(format!(">>{}", spec.label));
                return self.execute(spec.id, &Value::Null).map(|_| ());
            }
        }
        if self.running.is_some() {
            self.cancel();
        }
        (spec.enabled)(self).map_err(|m| EngineError::Disabled(spec.id.into(), m))?;
        self.last_command = Some(spec.id.to_string());
        self.echo(format!("Command: {}", spec.id.to_ascii_uppercase()));
        // A form (see `forms`) drives the command when asked to, or when the command has no
        // prompts of its own and its form needs no table (tables are edited in a dialog).
        let form = forms::get(spec.id)
            .filter(|f| force.is_some() || (spec.interactive.is_none() && !f.fields.iter().any(|x| matches!(x.kind, forms::Kind::Table(_)))));
        let factory: Option<Factory> = match form {
            Some(f) => {
                self.form_preset = Some((f, force.unwrap_or(Value::Null)));
                Some(form_factory)
            }
            None => spec.interactive,
        };
        match factory {
            Some(factory) => {
                let machine = guard::Guarded::create(spec.id, factory, self)?;
                let st = self.state()?;
                let before = st.doc.clone();
                let selection_before = st.selection.clone();
                self.running = Some(Running { id: spec.id.to_string(), machine, before, selection_before });
                self.pending_window = None;
                // Some commands complete immediately (e.g. ERASE with a pickfirst selection).
                self.feed(None)?;
                Ok(())
            }
            None => {
                let r = self.execute(spec.id, &Value::Null)?;
                if let Some(s) = r.as_str() {
                    self.echo(s.to_string());
                } else if !r.is_null()
                    && let Some(msg) = r.get("message").and_then(Value::as_str)
                {
                    for l in msg.lines() {
                        self.echo(l.to_string());
                    }
                }
                Ok(())
            }
        }
    }

    /// Give an input to the running command. With no command running, `Enter` repeats the last
    /// command (by convention) and points do nothing.
    pub fn input(&mut self, input: Input) -> Result<()> {
        let drawn = self.doc().ok().and_then(annot::drawing_unit);
        units::with_drawn_unit(drawn, || self.input_inner(input))
    }

    fn input_inner(&mut self, input: Input) -> Result<()> {
        if self.running.is_none() {
            if input == Input::Enter
                && let Some(last) = self.last_command.clone()
            {
                return self.start(&last);
            }
            return Ok(());
        }
        if !self.point_mods.is_empty() {
            return self.modifier_input(input);
        }
        // A deferred snap is only meaningful where the prompt resolves it; elsewhere it is the
        // point it was picked at.
        let input = match input {
            Input::Deferred(d) if !self.current_prompt().is_some_and(|p| p.deferred) => Input::Point(d.at),
            other => other,
        };
        // Generic selection handling during "Select objects:" prompts.
        let input = self.preprocess_selection(input)?;
        match input {
            Some(i) => self.feed(Some(i)),
            None => Ok(()),
        }
    }

    fn preprocess_selection(&mut self, input: Input) -> Result<Option<Input>> {
        let Some(prompt) = self.current_prompt() else { return Ok(Some(input)) };
        if !prompt.accept.select {
            return Ok(Some(input));
        }
        let space = self.space();
        match input {
            Input::Point(p) => {
                let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
                if let Some(pw) = self.pending_window.take() {
                    let crossing = p.x < pw.corner.x;
                    let hs = select::select_window(self.doc()?, &space, Bounds2::new(pw.corner, p), crossing);
                    return Ok(Some(Input::Pick(hs)));
                }
                match select::pick(self.doc()?, &space, p, ap) {
                    Some(h) => Ok(Some(Input::Pick(vec![h]))),
                    None => {
                        self.pending_window = Some(PendingWindow { corner: p, during_command: true });
                        Ok(None)
                    }
                }
            }
            Input::Text(ref t) | Input::Keyword(ref t) => {
                let tl = t.trim().to_ascii_lowercase();
                let d = self.doc()?;
                let store = d.space(&space);
                let picked = match tl.as_str() {
                    "all" => store.map(|s| s.iter().filter(|e| d.is_visible(e)).map(|e| e.handle).collect()),
                    "l" | "last" => store.and_then(|s| s.last()).map(|e| vec![e.handle]),
                    "p" | "previous" => Some(self.state()?.previous_selection.clone()),
                    _ => None,
                };
                match picked {
                    Some(hs) => Ok(Some(Input::Pick(hs))),
                    None => Ok(Some(input)),
                }
            }
            Input::Cancel => {
                self.pending_window = None;
                Ok(Some(Input::Cancel))
            }
            other => Ok(Some(other)),
        }
    }

    fn feed(&mut self, input: Option<Input>) -> Result<()> {
        let Some(mut run) = self.running.take() else { return Ok(()) };
        let step = match input {
            Some(Input::Cancel) => Ok(Step::Cancel),
            Some(i) => {
                if let Input::Point(p) = &i {
                    self.last_point = *p;
                }
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run.machine.input(self, i)));
                match r {
                    Ok(r) => r,
                    Err(_) => Err(EngineError::Internal(run.id.clone(), "panic".into())),
                }
            }
            // A command starts with no modifiers pending.
            None => {
                self.point_mods.clear();
                run.machine.begin(self)
            }
        };
        // Redraw what the command has added so far (LINE adds a segment per point), not only when it ends.
        if let Ok(st) = self.state_mut()
            && !Arc::ptr_eq(&run.before, &st.doc)
        {
            st.revision += 1;
        }
        match step {
            Ok(Step::Continue) => {
                self.running = Some(run);
                Ok(())
            }
            Ok(Step::Done) => {
                self.finish(run, false);
                Ok(())
            }
            Ok(Step::Cancel) => {
                self.echo("*Cancel*");
                self.finish(run, true);
                Ok(())
            }
            Err(EngineError::Internal(id, m)) => {
                // Restore and end the command.
                if let Ok(st) = self.state_mut() {
                    st.doc = run.before.clone();
                }
                self.echo(format!("Internal error in {id}: {m}"));
                Err(EngineError::Internal(id, m))
            }
            Err(e) => {
                // Invalid input: report and keep prompting (re-prompts).
                self.echo(e.to_string());
                self.running = Some(run);
                Ok(())
            }
        }
    }

    fn finish(&mut self, run: Running, cancelled: bool) {
        if find_command(&run.id).is_some_and(|c| c.undoable) {
            let refused = self
                .refuse_non_finite(None, &run.before, &run.selection_before, &run.id)
                .and_then(|()| self.refuse_protected(None, &run.before, &run.selection_before, &run.id));
            if let Err(e) = refused {
                self.echo(e.to_string());
            }
        }
        // A command outside undo that changed the drawing (UNDO, which swapped it for an earlier
        // one) records no undo step, as in `execute`.
        if find_command(&run.id).is_some_and(|c| !c.undoable) && self.state().is_ok_and(|st| !Arc::ptr_eq(&run.before, &st.doc)) {
            self.pending_window = None;
            return;
        }
        self.pending_window = None;
        let label = find_command(&run.id).map(|c| c.label).unwrap_or("Command");
        let _ = cancelled;
        cmd::groups::after_command(self, Some(&run.before));
        assoc::after_command(self, Some(&run.before), None);
        if let Ok(st) = self.state_mut()
            && !Arc::ptr_eq(&run.before, &st.doc)
        {
            st.undo.push(Snapshot { label: label.to_string(), doc: run.before, selection: run.selection_before });
            st.redo.clear();
            st.revision += 1;
        }
    }

    /// Cancel the running command (Esc). With no command, clears the selection and history preview.
    pub fn cancel(&mut self) {
        self.point_mods.clear();
        if self.running.is_some() {
            let _ = self.feed(Some(Input::Cancel));
        } else if self.history_preview.is_some() {
            self.history_preview = None;
            self.echo("History preview ended.");
        } else {
            self.pending_window = None;
            self.set_selection(Vec::new());
        }
    }

    pub fn current_prompt(&self) -> Option<Prompt> {
        self.running.as_ref().map(|r| r.machine.prompt(self))
    }

    /// Whether a click picks objects right now (hit-test / hover): select prompts, and idle when
    /// PICKFIRST is on (noun-verb selection). Distinct from [`Self::cursor_shape`], which follows
    /// drawn cursor (idle always shows the pickbox even when PICKFIRST is off).
    pub fn picking_objects(&self) -> bool {
        match self.current_prompt() {
            Some(p) => p.picks_objects(),
            None => self.settings.pickfirst,
        }
    }

    /// drawing-area cursor: idle = crosshairs+pickbox; point prompt = crosshairs only;
    /// select-objects prompt = pickbox only.
    pub fn cursor_shape(&self) -> CursorShape {
        match self.current_prompt() {
            Some(p) if p.picks_objects() => CursorShape::Pickbox,
            Some(_) => CursorShape::Crosshairs,
            None => CursorShape::CrosshairsAndPickbox,
        }
    }

    /// The text shown on the command line: the active prompt, or "Command:".
    pub fn prompt_text(&self) -> String {
        match &self.running {
            Some(r) => format!("{} {}", r.id.to_ascii_uppercase(), r.machine.prompt(self).display()),
            None => "Command:".into(),
        }
    }

    /// Rubber-band preview entities for the cursor.
    pub fn preview(&self, cursor: Vec2) -> Vec<Entity> {
        let Some(r) = &self.running else { return Vec::new() };
        let common = self.current_common();
        r.machine.preview(self, cursor).into_iter().map(|k| Entity { handle: Handle(0), common: common.clone(), kind: k }).collect()
    }

    /// Companion graph preview for the canvas ghost overlay (not selectable).
    pub fn companion_ghost(&self) -> Vec<Entity> {
        cmd::companion::preview_entities(&self.companion)
    }

    /// History milestone diff overlay (added / removed / changed); not selectable.
    pub fn history_ghost(&self) -> cmd::history::HistoryGhostEntities {
        match &self.history_preview {
            Some(p) => cmd::history::preview_entities(p),
            None => cmd::history::HistoryGhostEntities::default(),
        }
    }

    /// Process one line typed at the command line (Enter pressed).
    pub fn cmdline(&mut self, text: &str) -> Result<()> {
        let t = text.trim_end_matches(['\r', '\n']);
        match &self.running {
            None => {
                let t = t.trim();
                if t.is_empty() {
                    return self.input(Input::Enter);
                }
                // `cmd {json}`: programmatic call with parameters.
                if let Some((name, json)) = t.split_once(' ')
                    && json.trim_start().starts_with('{')
                {
                    let params: Value =
                        serde_json::from_str(json.trim()).map_err(|e| EngineError::BadParams { cmd: name.into(), msg: e.to_string() })?;
                    let id = cmd::resolve_alias(&name.to_ascii_lowercase());
                    let r = self.execute(&id, &params)?;
                    if !r.is_null() {
                        self.echo(r.to_string());
                    }
                    return Ok(());
                }
                // Expressions like "LINE 0,0 5,5" in one line: first token is the command.
                let mut parts = split_tokens(t).into_iter();
                let name = parts.next().unwrap_or_default();
                self.start(&name)?;
                for rest in parts {
                    if self.running.is_none() {
                        break;
                    }
                    self.typed(&rest)?;
                }
                Ok(())
            }
            Some(_) => {
                // Space acts as Enter except where the prompt wants free text.
                let text_prompt = self.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number);
                if text_prompt || !t.contains(' ') {
                    return self.typed(t);
                }
                for tok in split_tokens(t) {
                    if self.running.is_none() {
                        break;
                    }
                    self.typed(&tok)?;
                }
                Ok(())
            }
        }
    }

    /// One token typed while a command runs.
    fn typed(&mut self, t: &str) -> Result<()> {
        let drawn = self.doc().ok().and_then(annot::drawing_unit);
        units::with_drawn_unit(drawn, || self.typed_inner(t))
    }

    fn typed_inner(&mut self, t: &str) -> Result<()> {
        let Some(prompt) = self.current_prompt() else { return Ok(()) };
        let tt = t.trim();
        if tt.is_empty() {
            return self.input_inner(Input::Enter);
        }
        // FROM, M2P, point filters, `<a` (before `'`: `'_from` is a modifier).
        if let Some(r) = self.typed_modifier(&prompt, tt) {
            return r;
        }
        // Transparent command.
        if tt.starts_with('\'') {
            return self.start(tt);
        }
        // At "Select objects" prompts the selection modes (Previous, Last, ALL) win over a command
        // keyword sharing the letter (HATCH's "picK internal point" against `P`).
        let selection_mode = prompt.accept.select && matches!(tt.to_ascii_lowercase().as_str(), "all" | "l" | "last" | "p" | "previous");
        if !selection_mode
            && let Some(k) = prompt.match_keyword(tt)
            && !(prompt.accept.number && tt.parse::<f64>().is_ok())
        {
            return self.input_inner(Input::Keyword(k));
        }
        if prompt.accept.point {
            if let Some(p) = prompt::parse_point_with(tt, self.last_point, &self.angle_settings()) {
                return self.input_inner(Input::Point(p));
            }
            // Direct distance entry along the rubber band.
            if let (Some(base), Some(dist)) = (prompt.base, units::parse_distance(tt))
                && !prompt.accept.number
            {
                // Along the tracking path the cursor is on, from the point it comes from.
                if let Some((from, dir)) = self.tracking.last.as_ref().filter(|t| t.point.near(self.cursor, 1e-9)).and_then(|t| t.along()) {
                    return self.input_inner(Input::Point(from + dir * dist));
                }
                let mut c = self.cursor;
                if self.settings.orthomode {
                    c = snap::ortho(base, c);
                }
                let dir = (c - base).normalized();
                let dir = if dir == Vec2::ZERO { Vec2::X } else { dir };
                return self.input_inner(Input::Point(base + dir * dist));
            }
        }
        self.input_inner(Input::Text(tt.to_string()))
    }

    /// Run a script: one input per line; within a line, spaces separate inputs except at text
    /// prompts (as scripts do).
    pub fn script(&mut self, text: &str) -> Result<()> {
        for line in text.lines() {
            let line = line.trim_end();
            if line.trim_start().starts_with(';') {
                continue;
            }
            if line.is_empty() {
                self.cmdline("")?;
                continue;
            }
            let mut rest = line;
            loop {
                // Free text only when the prompt has no keywords; otherwise split so `I 15` reaches the keyword and the value.
                let text_prompt = self.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && p.keywords.is_empty());
                if text_prompt || self.running.is_none() && rest.contains('{') {
                    self.cmdline(rest)?;
                    break;
                }
                match rest.split_once(' ') {
                    Some((tok, r)) => {
                        // A unit typed after a space belongs to the number before it: `5 m,0`.
                        let mut tok = tok.to_string();
                        let mut r = r;
                        let mut joined = false;
                        loop {
                            let (next, after) = r.trim_start().split_once(' ').unwrap_or((r.trim_start(), ""));
                            if tok.ends_with(|c: char| c.is_ascii_digit() || c == '.') && units::starts_with_unit_word(next) {
                                tok.push_str(next);
                                r = after;
                                joined = true;
                            } else {
                                break;
                            }
                        }
                        if self.running.is_none() {
                            self.start(&tok)?;
                        } else {
                            self.typed(&tok)?;
                        }
                        // The unit was the last word of the line: nothing more to type (an empty token would be Enter).
                        if joined && r.is_empty() {
                            break;
                        }
                        rest = r;
                    }
                    None => {
                        if self.running.is_none() {
                            self.start(rest)?;
                        } else {
                            self.typed(rest)?;
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    // ---------------- undo ----------------

    pub fn undo(&mut self) -> Result<Option<String>> {
        let st = self.state_mut()?;
        let Some(snap) = st.undo.pop() else { return Ok(None) };
        let cur = Snapshot { label: snap.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() };
        st.doc = snap.doc;
        st.follow_layout(&cur.doc);
        st.selection = Vec::new();
        st.redo.push(cur);
        st.revision += 1;
        Ok(Some(snap.label))
    }
    pub fn redo(&mut self) -> Result<Option<String>> {
        let st = self.state_mut()?;
        let Some(snap) = st.redo.pop() else { return Ok(None) };
        let cur = Snapshot { label: snap.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() };
        st.doc = snap.doc;
        st.follow_layout(&cur.doc);
        st.selection = Vec::new();
        st.undo.push(cur);
        st.revision += 1;
        Ok(Some(snap.label))
    }

    // ---------------- picking without a command ----------------

    /// Open a selection window with one corner at `corner`, for press-and-drag selection
    /// (PICKDRAG = 2: a drag opens the window wherever it starts, even over an object;
    /// the next point, where the drag ends, closes it). Only while objects are being selected: no
    /// command running, or a command asking for objects. Returns whether a window was opened.
    pub fn begin_window(&mut self, corner: Vec2) -> bool {
        let during_command = self.running.is_some();
        if !corner.is_finite() || (during_command && !self.current_prompt().is_some_and(|p| p.accept.select)) {
            return false;
        }
        self.pending_window = Some(PendingWindow { corner, during_command });
        true
    }

    /// A click with no command running: pick/toggle objects or start a selection window.
    ///
    /// PICKADD on (the default): picks add to the selection and Shift removes. PICKADD off: each pick
    /// replaces the selection and Shift adds (Shift+pick on a selected object removes it).
    pub fn idle_click(&mut self, p: Vec2, shift: bool) -> Result<()> {
        let space = self.space();
        let pickadd = self.settings.pickadd;
        if let Some(pw) = self.pending_window.take() {
            let crossing = p.x < pw.corner.x;
            let hs = select::select_window(self.doc()?, &space, Bounds2::new(pw.corner, p), crossing);
            let mut sel = if pickadd || shift { self.selection() } else { Vec::new() };
            if shift && pickadd {
                sel.retain(|h| !hs.contains(h));
            } else {
                sel.extend(hs);
            }
            self.set_selection(sel);
            return Ok(());
        }
        let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
        match select::pick(self.doc()?, &space, p, ap) {
            Some(h) => {
                let mut sel = if pickadd || shift { self.selection() } else { Vec::new() };
                let had = sel.contains(&h);
                if shift && (pickadd || had) {
                    sel.retain(|x| *x != h);
                } else if !had {
                    sel.push(h);
                }
                self.set_selection(sel);
            }
            None => self.pending_window = Some(PendingWindow { corner: p, during_command: false }),
        }
        Ok(())
    }

    /// Remember the selection used by a command as "Previous".
    pub fn remember_selection(&mut self, sel: &[Handle]) {
        if let Ok(st) = self.state_mut() {
            st.previous_selection = sel.to_vec();
        }
    }

    // ---------------- views ----------------

    /// The limits of the space being edited: the sheet in paper space, else LIMMIN/LIMMAX.
    pub fn zoom_limits(&self) -> Result<Bounds2> {
        let d = self.doc()?;
        if let Space::Paper(name) = self.space()
            && let Some(sheet) = aad_render::sheet(d, &name)
        {
            return Ok(sheet.bounds());
        }
        let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
        let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
        Ok(Bounds2::new(lo, hi))
    }

    pub fn zoom_extents(&mut self) -> Result<()> {
        let space = self.space();
        let ext = self.doc()?.extents(&space);
        let (w, h) = self.viewport_px;
        let ext = if ext.is_empty() { self.zoom_limits()? } else { ext };
        let st = self.state_mut()?;
        let aspect = st.zoom_aspect(w / h.max(1.0));
        let height = ext.height().max(ext.width() / aspect.max(1e-6)).max(1e-6) * 1.05;
        st.set_zoom_frame(View { center: ext.center(), height });
        Ok(())
    }

    /// Zoom by `factor` (> 1 zooms in) keeping `about` fixed on screen.
    pub fn zoom_about(&mut self, factor: f64, about: Vec2) -> Result<()> {
        if !(factor.is_finite() && factor > 0.0) {
            return Ok(());
        }
        let st = self.state_mut()?;
        let v = st.view();
        let h = (v.height / factor).clamp(1e-9, 1e12);
        let k = h / v.height;
        let c = about + (v.center - about) * k;
        st.set_view_quiet(View { center: c, height: h });
        Ok(())
    }

    /// Serializable summary for inspect/automation.
    pub fn summary(&self) -> Value {
        let docs: Vec<Value> = self
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                serde_json::json!({
                    "index": i, "title": d.title, "path": d.path, "dirty": d.is_dirty(),
                    "entities": d.doc.entity_count(), "active": i == self.active, "revision": d.revision,
                })
            })
            .collect();
        serde_json::json!({
            "documents": docs,
            "prompt": self.prompt_text(),
            "running": self.running.as_ref().map(|r| r.id.clone()),
            "selection": self.selection().iter().map(|h| h.hex()).collect::<Vec<_>>(),
            "settings": self.settings,
        })
    }
}

#[cfg(test)]
mod tests;

/// The factory for form-driven commands: reads the form [`Session::start_form`] or `start` chose.
fn form_factory(s: &Session) -> Result<Box<dyn Interactive>> {
    let (form, preset) = s.form_preset.clone().ok_or_else(|| EngineError::UnknownCommand("form".into()))?;
    Ok(Box::new(forms::FormRun::new(form, &preset)))
}

/// What `plot.window` / `plotl` / `plotp` asked for. The application fits `window` into the
/// nameplate (`layout`) and either saves a PDF or shows a preview.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotRequest {
    /// Whole drawing when `None`; otherwise a model-space rectangle `[x0, y0, x1, y1]`.
    pub window: Option<[f64; 4]>,
    /// Nameplate layout to plot through (`Nameplate L` / `Nameplate P`). Empty = model-space plot.
    pub layout: String,
    /// When true, the shell shows a PDF preview instead of a save dialog.
    pub preview: bool,
}

impl Session {
    /// The plot request waiting for the application, if any (cleared by taking it).
    pub fn take_plot_request(&mut self) -> Option<PlotRequest> {
        self.plot_request.take()
    }
}

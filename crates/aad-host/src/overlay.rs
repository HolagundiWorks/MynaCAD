//! Cursor overlay drawn on top of the drawing: the command preview, grips, the snap marker,
//! the selection window and the crosshair.
//!
//! These are screen-space strokes in world coordinates, packed with the same mesh as the drawing.

use aad_color::Rgb;
use aad_engine::Session;
use aad_engine::doc::{Entity, Handle};
use aad_engine::grips;
use aad_engine::snap::tracking::{Acquired, PathShape, Track};
use aad_engine::snap::{self, SnapHit};
use aad_geom::Vec2;
use aad_render::mesh::{self, MeshData};
use aad_render::{DPrim, DisplayList, Kind};

/// Selection highlight (matches egui `Tokens::selection`).
const SELECTION_INK: Rgb = Rgb(0x4a, 0x8f, 0xff);
/// Rollover / hover highlight (matches egui `Tokens::hover`).
const HOVER_INK: Rgb = Rgb(0x9c, 0xc6, 0xff);
/// Rubber-band / command preview: cyan so the active geometry stands out from committed entities.
const ACTIVE_INK: Rgb = Rgb(0x00, 0xe6, 0xff);
/// Companion graph ghost (faded cyan; not selectable).
const COMPANION_INK: Rgb = Rgb(0x5a, 0xb8, 0xd4);
/// History preview: added (blue), removed (red), changed (amber).
const HISTORY_ADDED: Rgb = Rgb(0x4a, 0x8f, 0xff);
const HISTORY_REMOVED: Rgb = Rgb(0xe0, 0x55, 0x55);
const HISTORY_CHANGED: Rgb = Rgb(0xe0, 0xa8, 0x30);
/// Object-snap and tracking ink (matches egui `Tokens::snap`).
const SNAP_INK: Rgb = Rgb(0x2f, 0xd0, 0x62);
/// Overlay hairline width in points: near one device pixel on typical DPI (AutoCAD-thin).
const OVERLAY_HAIRLINE: f32 = 0.5;

/// A grip the pointer has picked and not yet placed.
#[derive(Clone, Copy)]
pub struct HotGrip {
    pub handle: Handle,
    pub index: usize,
    pub base: Vec2,
    pub mode: grips::GripMode,
}

impl HotGrip {
    pub fn label(self) -> &'static str {
        use grips::GripMode::*;
        match self.mode {
            Stretch => "** STRETCH **",
            Move => "** MOVE **",
            Rotate => "** ROTATE **",
            Scale => "** SCALE **",
            Mirror => "** MIRROR **",
        }
    }

    pub fn next_mode(&mut self) {
        use grips::GripMode::*;
        self.mode = match self.mode {
            Stretch => Move,
            Move => Rotate,
            Rotate => Scale,
            Scale => Mirror,
            Mirror => Stretch,
        };
    }
}

/// What the pointer is doing, besides the drawing itself.
pub struct Cursor {
    pub aim: Vec2,
    pub raw: Option<Vec2>,
    pub snap: Option<SnapHit>,
    pub track: Option<Track>,
    pub hot: Option<HotGrip>,
    /// Entity under the pick box while selecting (rollover).
    pub hover: Option<Handle>,
    /// Grip under the pointer while idle (warm highlight).
    pub warm_grip: Option<(Handle, usize)>,
}

pub fn mesh(
    session: &Session,
    cursor: Option<&Cursor>,
    paper: Rgb,
    height: u32,
    origin: Vec2,
    tip: Option<&str>,
    _width: u32,
    show_ucs: bool,
    icon_px: f64,
) -> MeshData {
    let Ok(state) = session.state() else { return MeshData::default() };
    let view = state.view();
    let scale = f64::from(height.max(1)) / view.height.max(1e-12);
    let site = state.doc.site_origin();
    let mut list = DisplayList::default();
    let mut window_fill: Option<([Vec2; 4], [u8; 4])> = None;
    if let Some(cursor) = cursor {
        if let Some(hot) = cursor.hot {
            if let Some(entity) = state.doc.entity(hot.handle)
                && let Some(kind) = grip_ghost(&entity.kind, hot, cursor.aim)
            {
                let ghost = Entity { handle: Handle(0), common: entity.common.clone(), kind };
                list = aad_render::build_entities(
                    &state.doc,
                    std::iter::once(&ghost),
                    &aad_render::Options {
                        tolerance: 0.2 / scale,
                        min_dash: 2.0 / scale,
                        text: false,
                        fill: false,
                        lineweights: false,
                        view_height: view.height,
                        ..Default::default()
                    },
                );
                tint(&mut list, ACTIVE_INK);
            }
            add_dashed(&mut list, &[hot.base, cursor.aim], ACTIVE_INK, 4.0 / scale, 3.0 / scale);
        } else if session.running.is_some() {
            let ents = session.preview(cursor.aim);
            if !ents.is_empty() {
                list = aad_render::build_entities(
                    &state.doc,
                    ents.iter(),
                    &aad_render::Options {
                        tolerance: 0.2 / scale,
                        min_dash: 2.0 / scale,
                        text: true,
                        fill: true,
                        lineweights: false,
                        view_height: view.height,
                        ..Default::default()
                    },
                );
                tint(&mut list, ACTIVE_INK);
            }
        }
        // Companion evaluate/subscribe preview: faded, never selectable (Handle 0).
        let ghost = session.companion_ghost();
        if !ghost.is_empty() {
            let mut ghost_list = aad_render::build_entities(
                &state.doc,
                ghost.iter(),
                &aad_render::Options {
                    tolerance: 0.2 / scale,
                    min_dash: 2.0 / scale,
                    text: false,
                    fill: false,
                    lineweights: false,
                    view_height: view.height,
                    ..Default::default()
                },
            );
            tint(&mut ghost_list, COMPANION_INK);
            append_display(&mut list, ghost_list);
        }
        // History milestone preview: added blue, removed red, changed amber.
        let hist = session.history_ghost();
        if !hist.is_empty() {
            let opts = aad_render::Options {
                tolerance: 0.2 / scale,
                min_dash: 2.0 / scale,
                text: false,
                fill: false,
                lineweights: false,
                view_height: view.height,
                ..Default::default()
            };
            for (ents, color) in
                [(hist.added.as_slice(), HISTORY_ADDED), (hist.removed.as_slice(), HISTORY_REMOVED), (hist.changed.as_slice(), HISTORY_CHANGED)]
            {
                if ents.is_empty() {
                    continue;
                }
                let mut ghost_list = aad_render::build_entities(&state.doc, ents.iter(), &opts);
                tint(&mut ghost_list, color);
                append_display(&mut list, ghost_list);
            }
        }
        if cursor.hot.is_none() {
            if let Some(hover) = cursor.hover.filter(|h| !session.selection().iter().any(|s| s == h)) {
                add_entity_outline(&mut list, &state.doc, &[hover], scale, view.height, HOVER_INK, false);
            }
            add_selection_highlight(&mut list, session, &state.doc, scale, view.height);
        }
        if let (Some(pw), Some(raw)) = (session.pending_window, cursor.raw) {
            let crossing = raw.x < pw.corner.x;
            let color = if crossing { Rgb(80, 220, 120) } else { Rgb(80, 160, 255) };
            let a = pw.corner;
            let b = raw;
            let corners = [Vec2::new(a.x, a.y), Vec2::new(b.x, a.y), Vec2::new(b.x, b.y), Vec2::new(a.x, b.y)];
            let fill = if crossing { [0x18, 0x48, 0x18, 0x50] } else { [0x18, 0x30, 0x60, 0x50] };
            window_fill = Some((corners, fill));
            let ring = [corners[0], corners[1], corners[2], corners[3], corners[0]];
            if crossing {
                add_dashed(&mut list, &ring, color, 5.0 / scale, 3.0 / scale);
            } else {
                add_poly(&mut list, &ring, color, false);
            }
        }
        if session.running.is_none() {
            let half = session.settings.gripsize.max(1.0) / scale;
            for handle in session.selection().iter().take(200) {
                let Some(entity) = state.doc.entity(*handle) else { continue };
                if state.doc.layer(&entity.common.layer).is_some_and(|layer| layer.locked) {
                    continue;
                }
                for (index, grip) in aad_engine::grips::grips(&state.doc, entity).into_iter().enumerate() {
                    let hot = cursor.hot.is_some_and(|h| h.handle == *handle && h.index == index);
                    let warm = cursor.warm_grip.is_some_and(|(h, i)| h == *handle && i == index);
                    let color = if hot {
                        Rgb(255, 40, 40)
                    } else if warm {
                        Rgb(255, 120, 160)
                    } else {
                        Rgb(0, 120, 255)
                    };
                    add_poly(&mut list, &square(grip, half), color, false);
                }
            }
        }
        add_tracking(&mut list, &session.tracking.acquired, cursor.track.as_ref(), cursor.aim, scale, view.height);
        if let Some(hit) = cursor.snap {
            add_snap_marker(&mut list, hit, scale);
        }
        add_crosshair(&mut list, cursor.aim, scale, height, session.settings.cursorsize, session.cursor_shape(), session.settings.pickbox, paper);
        if let Some(tip) = tip {
            add_tip(&mut list, cursor.aim, scale, tip, paper);
        }
    }
    if show_ucs {
        // Screen-corner orientation chrome, plus the shared world site base (same for every level).
        add_site_origin(&mut list, site, scale, icon_px, paper);
        // A drawing tab shows its sheet: paper frame, margin and a title strip, at the tab's scale.
        if let Some(set) = state.doc.active_sheet_ref() {
            add_sheet_frame(&mut list, &state.doc, &set.name, site, scale, icon_px, paper);
        }
    }
    let mut built = mesh::build_mesh(&list, origin, paper, false);
    if let Some((corners, rgba)) = window_fill {
        mesh::append_fill_quad(&mut built, origin, corners, rgba);
    }
    mesh::expand_hairlines_width(&mut built, OVERLAY_HAIRLINE);
    built
}

/// Tooltip text for snap / tracking / grip (caller picks priority).
pub fn cursor_tip(snap: Option<&SnapHit>, track: Option<&Track>, hot: Option<HotGrip>) -> Option<String> {
    if let Some(hot) = hot {
        return Some(hot.label().to_string());
    }
    let mut tips = Vec::new();
    if let Some(hit) = snap {
        tips.push(hit.name.to_string());
    }
    if let Some(tr) = track {
        for path in &tr.paths {
            let distance = aad_engine::units::format_distance(path.from.dist(tr.point), 2, 4);
            let angle = (path.from.angle_to(tr.point).to_degrees().round() as i64).rem_euclid(360);
            tips.push(if path.name == "Polar" { format!("Polar: {distance} < {angle}°") } else { format!("{}: {distance} < {angle}°", path.name) });
        }
        if let Some(o) = tr.object {
            tips.push(o.to_string());
        }
    }
    (!tips.is_empty()).then_some(tips.join(", "))
}

fn tint(list: &mut DisplayList, color: Rgb) {
    for prim in &mut list.prims {
        prim.color = color;
        prim.aci7 = false;
    }
}

fn append_display(dst: &mut DisplayList, src: DisplayList) {
    let v0 = u32::try_from(dst.verts.len()).unwrap_or(u32::MAX);
    let t0 = u32::try_from(dst.tris.len()).unwrap_or(u32::MAX);
    if v0 == u32::MAX || t0 == u32::MAX {
        return;
    }
    dst.verts.extend_from_slice(&src.verts);
    dst.tris.extend_from_slice(&src.tris);
    for mut prim in src.prims {
        let base = match prim.kind {
            Kind::Tris | Kind::Mask => t0,
            _ => v0,
        };
        prim.start = prim.start.saturating_add(base);
        dst.prims.push(prim);
    }
}

fn add_selection_highlight(list: &mut DisplayList, session: &Session, doc: &aad_engine::doc::Drawing, scale: f64, view_height: f64) {
    let handles: Vec<Handle> = session.selection().iter().copied().take(200).collect();
    add_entity_outline(list, doc, &handles, scale, view_height, SELECTION_INK, true);
}

fn add_entity_outline(
    list: &mut DisplayList,
    doc: &aad_engine::doc::Drawing,
    handles: &[Handle],
    scale: f64,
    view_height: f64,
    color: Rgb,
    dashed: bool,
) {
    let ents: Vec<&aad_engine::doc::Entity> = handles.iter().filter_map(|handle| doc.entity(*handle).map(|e| e.as_ref())).collect();
    if ents.is_empty() {
        return;
    }
    let built = aad_render::build_entities(
        doc,
        ents,
        &aad_render::Options {
            tolerance: 0.2 / scale,
            min_dash: 2.0 / scale,
            text: true,
            fill: false,
            lineweights: false,
            view_height,
            ..Default::default()
        },
    );
    let on = (6.0 / scale).max(1e-9);
    let off = (4.0 / scale).max(0.0);
    for prim in &built.prims {
        if matches!(prim.kind, Kind::Polyline) {
            let pts = built.points(prim);
            if dashed {
                add_dashed(list, pts, color, on, off);
            } else {
                add_poly(list, pts, color, false);
            }
        }
    }
}

fn add_snap_marker(list: &mut DisplayList, hit: SnapHit, scale: f64) {
    if !hit.point.is_finite() || scale.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return;
    }
    let s = 5.5 / scale;
    let at = hit.point;
    use snap::mode;
    match hit.mode {
        mode::END => add_poly(list, &square(at, s), SNAP_INK, false),
        mode::MID => add_poly(
            list,
            &[at + Vec2::new(0.0, -s), at + Vec2::new(s, s * 0.8), at + Vec2::new(-s, s * 0.8), at + Vec2::new(0.0, -s)],
            SNAP_INK,
            false,
        ),
        mode::CEN | mode::GCEN => add_circle(list, at, s, SNAP_INK),
        mode::QUA => add_poly(
            list,
            &[at + Vec2::new(0.0, -s), at + Vec2::new(s, 0.0), at + Vec2::new(0.0, s), at + Vec2::new(-s, 0.0), at + Vec2::new(0.0, -s)],
            SNAP_INK,
            false,
        ),
        mode::INT | mode::APP => {
            add_poly(list, &[at + Vec2::new(-s, -s), at + Vec2::new(s, s)], SNAP_INK, false);
            add_poly(list, &[at + Vec2::new(s, -s), at + Vec2::new(-s, s)], SNAP_INK, false);
            if hit.mode == mode::APP {
                add_poly(list, &square(at, s), SNAP_INK, false);
            }
        }
        mode::PER => {
            add_poly(list, &[at + Vec2::new(-s, -s), at + Vec2::new(-s, s), at + Vec2::new(s, s)], SNAP_INK, false);
            add_poly(list, &[at + Vec2::new(-s, 0.0), at, at + Vec2::new(0.0, s)], SNAP_INK, false);
        }
        mode::TAN => {
            add_circle(list, at, s * 0.8, SNAP_INK);
            add_poly(list, &[at + Vec2::new(-s, -s), at + Vec2::new(s, -s)], SNAP_INK, false);
        }
        mode::NOD => {
            add_circle(list, at, s, SNAP_INK);
            add_poly(list, &[at + Vec2::new(-s, -s), at + Vec2::new(s, s)], SNAP_INK, false);
            add_poly(list, &[at + Vec2::new(s, -s), at + Vec2::new(-s, s)], SNAP_INK, false);
        }
        mode::INS => add_poly(
            list,
            &[
                at + Vec2::new(-s, -s),
                at + Vec2::new(0.0, -s),
                at,
                at + Vec2::new(s, 0.0),
                at + Vec2::new(s, s),
                at + Vec2::new(-s, s),
                at + Vec2::new(-s, -s),
            ],
            SNAP_INK,
            false,
        ),
        _ => add_poly(
            list,
            &[at + Vec2::new(-s, -s), at + Vec2::new(s, -s), at + Vec2::new(-s, s), at + Vec2::new(s, s), at + Vec2::new(-s, -s)],
            SNAP_INK,
            false,
        ),
    }
}

fn add_circle(list: &mut DisplayList, center: Vec2, radius: f64, color: Rgb) {
    if !(center.is_finite() && radius.is_finite() && radius > 0.0) {
        return;
    }
    let n = 24u32;
    let mut pts = Vec::with_capacity(n as usize + 1);
    for i in 0..=n {
        let a = std::f64::consts::TAU * f64::from(i) / f64::from(n);
        pts.push(Vec2::new(center.x + radius * a.cos(), center.y + radius * a.sin()));
    }
    add_poly(list, &pts, color, false);
}

fn add_tracking(list: &mut DisplayList, acquired: &[Acquired], track: Option<&Track>, aim: Vec2, scale: f64, view_height: f64) {
    let far = view_height * 4.0;
    let mark = 4.0 / scale;
    for a in acquired {
        let c = a.at();
        if !c.is_finite() {
            continue;
        }
        if matches!(a, Acquired::Parallel { .. }) {
            add_poly(list, &[c + Vec2::new(-5.0 / scale, 3.0 / scale), c + Vec2::new(-1.0 / scale, -5.0 / scale)], SNAP_INK, false);
            add_poly(list, &[c + Vec2::new(1.0 / scale, 3.0 / scale), c + Vec2::new(5.0 / scale, -5.0 / scale)], SNAP_INK, false);
        } else {
            add_poly(list, &[c + Vec2::new(-mark, 0.0), c + Vec2::new(mark, 0.0)], SNAP_INK, false);
            add_poly(list, &[c + Vec2::new(0.0, -mark), c + Vec2::new(0.0, mark)], SNAP_INK, false);
        }
    }
    let Some(tr) = track else { return };
    let on = (2.0 / scale).max(1e-9);
    let off = (4.0 / scale).max(0.0);
    for path in &tr.paths {
        let pts: Vec<Vec2> = match path.shape {
            PathShape::Ray { origin, dir } => {
                let back = (tr.point - origin).dot(dir).clamp(0.0, far);
                vec![tr.point - dir * back, tr.point + dir * far]
            }
            PathShape::Line { dir, .. } => vec![tr.point - dir * far, tr.point + dir * far],
            PathShape::Circle { center, radius, .. } => {
                let (a0, span) = (center.angle_to(tr.point), (far / radius.max(1e-12)).min(std::f64::consts::PI));
                (0..=48).map(|i| Vec2::polar(center, radius, a0 - span + span * f64::from(i) / 24.0)).collect()
            }
        };
        if pts.iter().all(|p| p.is_finite()) {
            add_dashed(list, &pts, SNAP_INK, on, off);
        }
    }
    if tr.paths.len() > 1 || tr.object.is_some() {
        let at = if aim.is_finite() { aim } else { tr.point };
        add_poly(list, &[at + Vec2::new(-mark, -mark), at + Vec2::new(mark, mark)], SNAP_INK, false);
        add_poly(list, &[at + Vec2::new(mark, -mark), at + Vec2::new(-mark, mark)], SNAP_INK, false);
    }
}

fn add_dashed(list: &mut DisplayList, pts: &[Vec2], color: Rgb, on: f64, off: f64) {
    if pts.len() < 2 || !(on.is_finite() && on > 0.0 && off.is_finite() && off >= 0.0) {
        return;
    }
    for window in pts.windows(2) {
        let (a, b) = (window[0], window[1]);
        if ![a.x, a.y, b.x, b.y].into_iter().all(f64::is_finite) {
            continue;
        }
        let len = a.dist(b);
        if !(len.is_finite() && len > 1e-12) {
            continue;
        }
        let dir = (b - a) / len;
        let mut t = 0.0;
        let mut draw = true;
        let mut guard = 0u32;
        while t < len && guard < 100_000 {
            guard = guard.saturating_add(1);
            let span = if draw { on } else { off };
            let t1 = (t + span).min(len);
            if draw && t1 > t {
                add_poly(list, &[a + dir * t, a + dir * t1], color, false);
            }
            if t1 <= t {
                break;
            }
            t = t1;
            draw = !draw;
        }
    }
}

fn grip_ghost(kind: &aad_engine::doc::EntityKind, hot: HotGrip, aim: Vec2) -> Option<aad_engine::doc::EntityKind> {
    if let Some(matrix) = grips::mode_matrix(hot.mode, hot.base, aim) {
        let mut ghost = kind.clone();
        ghost.transform(&matrix);
        return Some(ghost);
    }
    if hot.mode == grips::GripMode::Stretch { grips::stretch_grip(kind, hot.index, aim) } else { None }
}

/// Overlay ink contrasted against the paper/canvas clear colour (egui Tokens crosshair).
fn overlay_ink(paper: Rgb) -> Rgb {
    if paper.0 < 128 && paper.1 < 128 && paper.2 < 128 {
        Rgb(0xe8, 0xe8, 0xe8) // dark canvas → light crosshair
    } else {
        Rgb(0x1a, 0x23, 0x32) // light canvas → navy crosshair
    }
}

/// The sheet of the active drawing tab, drawn in the drawing: its paper size at the tab's scale (so 1:50 on A3 is 21 m by 14.85 m
/// of drawing), a 10 mm margin, and a title strip with the tab's name, paper, orientation and scale. The lower-left corner sits at
/// the site origin; nothing here is stored in the drawing.
fn add_sheet_frame(list: &mut DisplayList, doc: &aad_engine::doc::Drawing, tab: &str, at: Vec2, scale: f64, icon_px: f64, paper: Rgb) {
    let page = aad_engine::cmd::pagesetup::tab_page(doc, tab);
    // Drawing units in one millimetre of paper at 1:1 (a drawing without units is read as millimetres).
    let per_mm = aad_engine::annot::drawing_unit(doc).map_or(1.0, |u| 0.001 / u.metres());
    let k = per_mm * page.denominator;
    let (w, h) = (page.width_mm * k, page.height_mm * k);
    if !at.is_finite() || !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) || !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let ink = overlay_ink(paper);
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        [
            Vec2::new(at.x + x0, at.y + y0),
            Vec2::new(at.x + x1, at.y + y0),
            Vec2::new(at.x + x1, at.y + y1),
            Vec2::new(at.x + x0, at.y + y1),
            Vec2::new(at.x + x0, at.y + y0),
        ]
    };
    add_poly(list, &rect(0.0, 0.0, w, h), ink, true);
    let m = 10.0 * k;
    if w > 3.0 * m && h > 3.0 * m {
        add_dashed(list, &rect(m, m, w - m, h - m), ink, 6.0 / scale, 4.0 / scale);
        // The title strip: 180 x 40 mm of paper in the lower-right corner of the margin.
        let (tw, th) = ((180.0 * k).min(w - 2.0 * m), (40.0 * k).min(h - 2.0 * m));
        add_poly(list, &rect(w - m - tw, m, w - m, m + th), ink, true);
        let label = format!("{}  {} {}  1:{}", tab, page.paper, if page.landscape { "landscape" } else { "portrait" }, page.denominator.round());
        let text_h = (12.0 * icon_px / scale).min(th * 0.3).max(1e-9);
        add_ui_text(list, Vec2::new(at.x + w - m - tw + 0.04 * tw, at.y + m + th * 0.4), text_h, &label, ink);
    }
}

/// World-space site base shared by every level (not screen chrome). Arms stay ~48 DIP long.
fn add_site_origin(list: &mut DisplayList, at: Vec2, scale: f64, icon_px: f64, paper: Rgb) {
    if !at.is_finite() || !scale.is_finite() || scale <= 0.0 || !icon_px.is_finite() || icon_px <= 0.0 {
        return;
    }
    // A plain + at the origin (the point the cursor snaps to), and the letter O beside it.
    let plus = 8.0 * icon_px / scale;
    let ink = overlay_ink(paper);
    add_poly(list, &[Vec2::new(at.x - plus, at.y), Vec2::new(at.x + plus, at.y)], ink, true);
    add_poly(list, &[Vec2::new(at.x, at.y - plus), Vec2::new(at.x, at.y + plus)], ink, true);
    let label_at = Vec2::new(at.x + 10.0 * icon_px / scale, at.y - 14.0 * icon_px / scale);
    if label_at.is_finite() {
        add_label(list, label_at, scale, icon_px, "O", paper);
    }
}

fn add_label(list: &mut DisplayList, at: Vec2, scale: f64, icon_px: f64, text: &str, paper: Rgb) {
    if !at.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let height = 12.0 * icon_px / scale;
    add_ui_text(list, at, height, text, overlay_ink(paper));
}

fn add_crosshair(
    list: &mut DisplayList,
    at: Vec2,
    scale: f64,
    height: u32,
    cursorsize: f64,
    shape: aad_engine::CursorShape,
    pickbox: f64,
    paper: Rgb,
) {
    if !at.is_finite() || scale <= 0.0 {
        return;
    }
    // PICKBOX is device pixels (AutoCAD); floor so the square stays visible on HiDPI.
    let half = (pickbox.max(0.0) * 1.5).max(4.0) / scale;
    let ink = overlay_ink(paper);
    // Select-objects prompt: pickbox only (AutoCAD).
    if shape == aad_engine::CursorShape::Pickbox {
        add_poly(list, &square(at, half), ink, true);
        return;
    }
    let len = (f64::from(height) * cursorsize.max(1.0) / 100.0).max(12.0) / scale;
    let gap = if shape == aad_engine::CursorShape::CrosshairsAndPickbox { half } else { 0.0 };
    let x = |dx: f64| Vec2::new(at.x + dx, at.y);
    let y = |dy: f64| Vec2::new(at.x, at.y + dy);
    add_poly(list, &[x(-len), x(-gap)], ink, true);
    add_poly(list, &[x(gap), x(len)], ink, true);
    add_poly(list, &[y(-len), y(-gap)], ink, true);
    add_poly(list, &[y(gap), y(len)], ink, true);
    if gap > 0.0 {
        add_poly(list, &square(at, gap), ink, true);
    }
}

fn square(c: Vec2, half: f64) -> [Vec2; 5] {
    [
        Vec2::new(c.x - half, c.y - half),
        Vec2::new(c.x + half, c.y - half),
        Vec2::new(c.x + half, c.y + half),
        Vec2::new(c.x - half, c.y + half),
        Vec2::new(c.x - half, c.y - half),
    ]
}

fn add_tip(list: &mut DisplayList, at: Vec2, scale: f64, text: &str, paper: Rgb) {
    if !at.is_finite() || !scale.is_finite() || scale <= 0.0 || text.is_empty() {
        return;
    }
    // Half of the previous 12 DIP body size, filled (not stroked outlines).
    let height = 6.0 / scale;
    let shaped = aad_fonts::shape_line(&tip_font(), text, height, 1.0, 0.0);
    let pad = 2.0 / scale;
    // Sit clear of the crosshair: baseline above the cursor, glyphs grow upward.
    let origin = Vec2::new(at.x + 12.0 / scale, at.y + 14.0 / scale);
    let x0 = origin.x - pad;
    let y0 = origin.y - pad;
    let x1 = origin.x + shaped.width + pad;
    let y1 = origin.y + height + pad;
    if ![x0, y0, x1, y1].into_iter().all(f64::is_finite) {
        return;
    }
    add_mask(list, &[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
    add_shaped(list, origin, &shaped, overlay_ink(paper));
}

/// Shell UI face for cursor tips / UCS labels (same family as WinUI `FontFamilyUi`).
fn tip_font() -> aad_fonts::TextFont {
    for name in ["Segoe UI", "Segoe UI Variable", "Arial"] {
        if let aad_fonts::TextFont::Outline(face) = aad_fonts::TextFont::resolve(name) {
            return aad_fonts::TextFont::Outline(face);
        }
    }
    aad_fonts::TextFont::Stroke
}

fn add_ui_text(list: &mut DisplayList, at: Vec2, height: f64, text: &str, color: Rgb) {
    let shaped = aad_fonts::shape_line(&tip_font(), text, height, 1.0, 0.0);
    add_shaped(list, at, &shaped, color);
}

fn add_shaped(list: &mut DisplayList, at: Vec2, shaped: &aad_fonts::Shaped, color: Rgb) {
    for stroke in &shaped.strokes {
        let pts: Vec<Vec2> = stroke.iter().map(|point| at + *point).collect();
        add_poly(list, &pts, color, true);
    }
    for glyph in &shaped.glyphs {
        let local = aad_render::triangulate_outline(glyph);
        if local.len() < 3 {
            for contour in glyph {
                let pts: Vec<Vec2> = contour.iter().map(|point| at + *point).collect();
                add_poly(list, &pts, color, true);
            }
            continue;
        }
        let world: Vec<Vec2> = local.into_iter().map(|point| at + point).collect();
        add_tris(list, &world, color);
    }
}

fn add_tris(list: &mut DisplayList, pts: &[Vec2], color: Rgb) {
    if pts.len() < 3 {
        return;
    }
    let Ok(start) = u32::try_from(list.tris.len()) else { return };
    let Ok(len) = u32::try_from(pts.len()) else { return };
    list.tris.extend_from_slice(pts);
    list.prims.push(DPrim { handle: Handle(0), color, aci7: false, lw: 0.0, kind: Kind::Tris, start, len });
}

fn add_mask(list: &mut DisplayList, corners: &[Vec2; 4]) {
    let Ok(start) = u32::try_from(list.tris.len()) else { return };
    list.tris.extend_from_slice(&[corners[0], corners[1], corners[2], corners[0], corners[2], corners[3]]);
    list.prims.push(DPrim { handle: Handle(0), color: Rgb(0, 0, 0), aci7: false, lw: 0.0, kind: Kind::Mask, start, len: 6 });
}

fn add_poly(list: &mut DisplayList, pts: &[Vec2], color: Rgb, aci7: bool) {
    if pts.len() < 2 {
        return;
    }
    let Ok(start) = u32::try_from(list.verts.len()) else { return };
    let Ok(len) = u32::try_from(pts.len()) else { return };
    list.verts.extend_from_slice(pts);
    list.prims.push(DPrim { handle: Handle(0), color, aci7, lw: 0.0, kind: Kind::Polyline, start, len });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use aad_engine::snap::SnapHit;

    #[test]
    fn site_origin_mark_adds_overlay_geometry_at_world_point() {
        let mut session = Session::new();
        session.execute("site.origin", &serde_json::json!({ "point": [50.0, -20.0] })).unwrap();
        session.execute("zoom.extents", &serde_json::json!({})).unwrap();
        let with = mesh(&session, None, Rgb(0x20, 0x20, 0x20), 480, Vec2::ZERO, None, 640, true, 1.0);
        let without = mesh(&session, None, Rgb(0x20, 0x20, 0x20), 480, Vec2::ZERO, None, 640, false, 1.0);
        assert!(with.quad_instances() > without.quad_instances(), "UCS + site mark stroke when show_ucs");
        assert!(with.quad_instances() > 0);
    }

    #[test]
    fn cursor_tip_prefers_grip_then_snap_name() {
        let hit = SnapHit { point: Vec2::new(1.0, 2.0), mode: 1, name: "Endpoint", deferred: None };
        assert_eq!(cursor_tip(Some(&hit), None, None).as_deref(), Some("Endpoint"));
        let hot = HotGrip { handle: Handle(1), index: 0, base: Vec2::new(0.0, 0.0), mode: grips::GripMode::Move };
        assert_eq!(cursor_tip(Some(&hit), None, Some(hot)).as_deref(), Some("** MOVE **"));
        assert!(cursor_tip(None, None, None).is_none());
    }

    #[test]
    fn overlay_mesh_draws_crosshair_and_snap_marker() {
        let session = Session::new();
        let cursor = Cursor {
            aim: Vec2::new(10.0, 10.0),
            raw: Some(Vec2::new(10.0, 10.0)),
            snap: Some(SnapHit { point: Vec2::new(10.0, 10.0), mode: 1, name: "Endpoint", deferred: None }),
            track: None,
            hot: None,
            hover: None,
            warm_grip: None,
        };
        let built = mesh(&session, Some(&cursor), Rgb(0, 0, 0), 600, Vec2::ZERO, Some("Endpoint"), 800, false, 24.0);
        assert!(built.line_vertices() > 0 || built.quad_instances() > 0 || built.tri_vertices() > 0, "crosshair and snap marker must emit geometry");
    }

    #[test]
    fn companion_ghost_adds_overlay_geometry() {
        let mut session = Session::new();
        session
            .execute(
                "companion.preview",
                &serde_json::json!({
                    "result": {
                        "lines": [{"x0": 0.0, "y0": 0.0, "x1": 100.0, "y1": 0.0, "layer": "A-WALL"}],
                        "circles": [],
                        "arcs": []
                    }
                }),
            )
            .unwrap();
        assert_eq!(session.companion_ghost().len(), 1);
        let cursor = Cursor { aim: Vec2::ZERO, raw: Some(Vec2::ZERO), snap: None, track: None, hot: None, hover: None, warm_grip: None };
        let with_ghost = mesh(&session, Some(&cursor), Rgb(0, 0, 0), 600, Vec2::ZERO, None, 800, false, 24.0);
        session.execute("companion.preview", &serde_json::json!({ "result": null })).unwrap();
        let without = mesh(&session, Some(&cursor), Rgb(0, 0, 0), 600, Vec2::ZERO, None, 800, false, 24.0);
        assert!(
            with_ghost.line_vertices() + with_ghost.quad_instances() + with_ghost.tri_vertices()
                > without.line_vertices() + without.quad_instances() + without.tri_vertices(),
            "ghost preview must add overlay strokes"
        );
    }
}

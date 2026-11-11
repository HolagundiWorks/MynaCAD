//! Quantity take-off from entity take-off marks, with a fallback to the `MEASURE-*` layers
//! (the kit's TOSTART, MLIN, MBRK, MBML, MCOL, MREC, MPAINT, MCEIL, MFLOOR, MAREA, MSLAB, MCLEAR
//! and MEXPORT) for drawings that have not been mapped yet.
//!
//! Every quantity is computed here from the drawing geometry and units, never by the caller:
//! lengths of lines and polyline segments, areas of closed outlines, openings deducted where a
//! deduct mark (or a line on `MEASURE-DEDUCT`) lies on a wall (or sits inside a block that carries
//! such a line). Lengths in metric drawings round to 1 cm; in imperial drawings to 1/8 inch. Each
//! command labels the elements on `MEASURE-LABELS`, draws a summary table on `MEASURE-TABLE`, and
//! saves the result in the drawing as `takeoff:<name>` so it can be exported later.
//!
//! New tools stamp [`aad_doc::TakeoffMark`] on the entity. `takeoff.maplayers` copies the old
//! layer convention onto those marks. DXF does not store the mark on the entity, so it is also
//! kept in the project entry `takeoffMeta` (handle hex → mark) and restored on open.

use aad_doc::{Drawing, EntityKind, HAlign, Handle, Space, TakeoffKind, TakeoffMark, VAlign};
use aad_geom::{Bounds2, Polyline, Segment, Vec2, point_in_polygon};
use serde_json::{Value, json};

use super::draw::make_text;
use super::hcw::{add_on, ensure_layer};
use super::helpers::line;
use super::*;
use crate::annot::{self, Unit};
use crate::{Result, Session};

pub const L_LINEAR: &str = "MEASURE-LINEAR";
pub const L_FULL: &str = "MEASURE-FULLBRICK";
pub const L_HALF: &str = "MEASURE-HALFBRICK";
pub const L_BEAM: &str = "MEASURE-BEAM";
pub const L_LINTEL: &str = "MEASURE-LINTEL";
pub const L_COLUMN: &str = "MEASURE-COLUMN";
pub const L_COUNT: &str = "MEASURE-COUNT";
pub const L_DEDUCT: &str = "MEASURE-DEDUCT";
pub const L_CEILING: &str = "MEASURE-CEILING";
pub const L_FLOOR: &str = "MEASURE-FLOOR";
pub const L_AREA: &str = "MEASURE-AREA";
pub const L_SLAB: &str = "MEASURE-SLAB";
pub const L_SLAB_DEDUCT: &str = "MEASURE-SLAB-DEDUCT";
pub const L_LABELS: &str = "MEASURE-LABELS";
pub const L_TABLE: &str = "MEASURE-TABLE";

/// Layer, ACI colour, description.
const LAYERS: &[(&str, i16, &str)] = &[
    (L_LINEAR, 5, "Take-off: linear measurement"),
    (L_FULL, 1, "Take-off: full brick walls"),
    (L_HALF, 30, "Take-off: half brick walls"),
    (L_BEAM, 3, "Take-off: concrete beams"),
    (L_LINTEL, 4, "Take-off: concrete lintels"),
    (L_COLUMN, 6, "Take-off: concrete columns"),
    (L_COUNT, 2, "Take-off: rectangles to count"),
    (L_DEDUCT, 40, "Take-off: openings to deduct"),
    (L_CEILING, 142, "Take-off: ceiling outlines"),
    (L_FLOOR, 94, "Take-off: floor outlines"),
    (L_AREA, 150, "Take-off: areas"),
    (L_SLAB, 52, "Take-off: slab outlines"),
    (L_SLAB_DEDUCT, 41, "Take-off: slab openings"),
    (L_LABELS, 7, "Take-off labels"),
    (L_TABLE, 7, "Take-off tables"),
];

pub fn specs() -> Vec<CommandSpec> {
    let p = "{units?: mm|cm|m|in|ft (default the drawing's), at?: [x,y] (table corner), table?: bool (default true), label?: bool (default true), tolerance? (drawing units)}";
    vec![
        CommandSpec::new("takeoff.start", "Start Take-off", run_start)
            .menu(&["Architecture", "Take-off", "Start"])
            .alias(&["tostart", "msetup"])
            .params("{} → {created} (creates the MEASURE-* layers)"),
        CommandSpec::new("takeoff.linear", "Linear", run_linear).menu(&["Architecture", "Take-off", "Linear"]).alias(&["mlin"]).params(p),
        CommandSpec::new("takeoff.brick", "Brickwork", run_brick).menu(&["Architecture", "Take-off", "Brickwork"]).alias(&["mbrk"]).params(p),
        CommandSpec::new("takeoff.beams", "Beams and Lintels", run_beams).menu(&["Architecture", "Take-off", "Beams"]).alias(&["mbml"]).params(p),
        CommandSpec::new("takeoff.columns", "Columns", run_columns)
            .menu(&["Architecture", "Take-off", "Columns"])
            .alias(&["mcol"])
            .params("{layer?: default MEASURE-COLUMN, units?, at?, table?, label?}"),
        CommandSpec::new("takeoff.rects", "Rectangles", run_rects).menu(&["Architecture", "Take-off", "Rectangles"]).alias(&["mrec"]).params(p),
        CommandSpec::new("takeoff.paint", "Wall Paint", run_paint)
            .menu(&["Architecture", "Take-off", "Wall Paint"])
            .alias(&["mpaint"])
            .params("{floor?: floor name (default the first), units?, at?, table?, label?, tolerance?}"),
        CommandSpec::new("takeoff.ceiling", "Ceiling", run_ceiling).menu(&["Architecture", "Take-off", "Ceiling"]).alias(&["mceil"]).params(p),
        CommandSpec::new("takeoff.floor", "Floor", run_floor).menu(&["Architecture", "Take-off", "Floor"]).alias(&["mfloor"]).params(p),
        CommandSpec::new("takeoff.area", "Area", run_area).menu(&["Architecture", "Take-off", "Area"]).alias(&["marea", "mare"]).params(p),
        CommandSpec::new("takeoff.slab", "Slab", run_slab).menu(&["Architecture", "Take-off", "Slab"]).alias(&["mslab", "mslb"]).params(p),
        CommandSpec::new("takeoff.clear", "Clear Take-off Labels", run_clear)
            .menu(&["Architecture", "Take-off", "Clear Labels"])
            .alias(&["mclear"])
            .params("{} → {erased} (erases the objects on MEASURE-LABELS and MEASURE-TABLE only)"),
        CommandSpec::new("takeoff.list", "List Take-offs", run_list)
            .menu(&["Architecture", "Take-off", "List"])
            .alias(&["tolist", "mlist"])
            .params("{name?} → {takeoffs: [name] | the saved result}")
            .noundo(),
        CommandSpec::new("takeoff.export", "Export Take-off", run_export)
            .menu(&["Architecture", "Take-off", "Export CSV"])
            .alias(&["mexport"])
            .params("{path? (file, or a folder), name? (one take-off; default every saved take-off, one CSV each)} → {files}")
            .noundo(),
        CommandSpec::new("takeoff.restore", "Restore Take-off Tables", run_restore)
            .menu(&["Architecture", "Take-off", "Restore Tables"])
            .alias(&["mrestore"])
            .params(
                "{name?: one take-off (default every saved take-off), at?: [x,y], clear?: bool (default true; erase MEASURE-TABLE first), \
                 path?: project file (.osp) from takeoff.record, project?: true for the open project} \
                 → {restored: [name], tables, source: drawing|project}",
            ),
        CommandSpec::new("takeoff.draw", "Draw Take-off", run_draw).menu(&["Architecture", "Take-off", "Draw"]).alias(&["mdraw"]).params(
            "{kind: extWall|intWall|beam|lintel|linear|floorArea|ceilingArea|area|column|count|deduct|slab|slabDeduct, \
                 thicknessMm?, points: [[x,y],...], closed?: bool} → {handles}. Stamps take-off metadata; the MEASURE-* layer is display only.",
        ),
        CommandSpec::new("takeoff.tag", "Tag Take-off", run_tag)
            .menu(&["Architecture", "Take-off", "Tag Selection"])
            .alias(&["mtag"])
            .params("{kind, thicknessMm?, handles? (default the selection)} → {tagged}"),
        CommandSpec::new("takeoff.maplayers", "Map Measure Layers", run_maplayers)
            .menu(&["Architecture", "Take-off", "Map Layers to Metadata"])
            .alias(&["mmap"])
            .params(
                "{force?: bool (default false; leave entities that already have a mark)} → {tagged, skipped, byKind}. \
                 MEASURE-FULLBRICK → extWall 230 mm, MEASURE-HALFBRICK → intWall 115 mm, and the other MEASURE-* layers to their kinds. \
                 MEASURE-LABELS and MEASURE-TABLE are left alone.",
            ),
        CommandSpec::new("column.deduct", "Column Slab Deducts", run_column_deduct)
            .menu(&["Architecture", "Take-off", "Column Slab Deducts"])
            .params("{} → {added}. Copies column outlines onto slab-deduct marks where none already cover them."),
    ]
}

// ---------------------------------------------------------------------------------------------
// Context, units and rounding

pub(crate) struct Ctx {
    unit: Unit,
    /// Label text height in drawing units (about 125 mm of real size).
    th: f64,
    /// Matching distance in drawing units (about 10 mm of real size).
    tol: f64,
}

impl Ctx {
    fn metric(&self) -> bool {
        !matches!(self.unit, Unit::In | Unit::Ft | Unit::FtIn)
    }
    fn len_unit(&self) -> &'static str {
        if self.metric() { "m" } else { "ft" }
    }
    fn area_unit(&self) -> &'static str {
        if self.metric() { "m2" } else { "sq ft" }
    }
    /// A length in drawing units as metres (1 cm steps) or feet (1/8 inch steps).
    fn len(&self, du: f64) -> f64 {
        let m = du * self.unit.metres();
        if self.metric() {
            (m * 100.0).round() / 100.0
        } else {
            let inches = m / 0.0254;
            (inches * 8.0).round() / 8.0 / 12.0
        }
    }
    /// An area in drawing units squared as square metres or square feet (0.01 steps).
    fn area(&self, du2: f64) -> f64 {
        let m2 = du2 * self.unit.metres() * self.unit.metres();
        let v = if self.metric() { m2 } else { m2 / (0.3048 * 0.3048) };
        (v * 100.0).round() / 100.0
    }
}

fn ctx(s: &Session, p: &Value, cmd: &str) -> Result<Ctx> {
    let d = s.doc()?;
    let unit = match str_param(p, "units") {
        Some(u) => Unit::parse(u).ok_or_else(|| bad(cmd, "`units` must be mm, cm, m, in or ft"))?,
        None => annot::drawing_unit(d).ok_or_else(|| bad(cmd, "the drawing's units are not set: set them with `drawing.units` or pass `units`"))?,
    };
    let per_mm = 0.001 / unit.metres();
    let tol = f64_or(p, "tolerance", 10.0 * per_mm);
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(bad(cmd, "`tolerance` must be zero or positive"));
    }
    Ok(Ctx { unit, th: 125.0 * per_mm, tol })
}

// ---------------------------------------------------------------------------------------------
// Elements

fn on_layer(e: &aad_doc::Entity, layer: &str) -> bool {
    e.common.layer.eq_ignore_ascii_case(layer)
}

#[derive(Clone)]
struct Wall {
    layer: String,
    seg: Segment,
    handle: Handle,
    kind: TakeoffKind,
    thickness_mm: Option<f64>,
    /// The mark carries a thickness the user set; otherwise `thickness_mm` is only what the kind or layer implies.
    thickness_set: bool,
}

/// Project key that mirrors take-off marks so a DXF round-trip (which does not store the entity field) keeps them.
pub(crate) const META_KEY: &str = "takeoffMeta";

/// Display layer for a take-off kind. Colour only; measurement reads the mark first.
pub(crate) fn layer_for_kind(kind: TakeoffKind) -> &'static str {
    match kind {
        TakeoffKind::ExtWall => L_FULL,
        TakeoffKind::IntWall => L_HALF,
        TakeoffKind::Beam => L_BEAM,
        TakeoffKind::Lintel => L_LINTEL,
        TakeoffKind::Linear => L_LINEAR,
        TakeoffKind::FloorArea => L_FLOOR,
        TakeoffKind::CeilingArea => L_CEILING,
        TakeoffKind::Area => L_AREA,
        TakeoffKind::Column => L_COLUMN,
        TakeoffKind::Count => L_COUNT,
        TakeoffKind::Deduct => L_DEDUCT,
        TakeoffKind::Slab => L_SLAB,
        TakeoffKind::SlabDeduct => L_SLAB_DEDUCT,
    }
}

fn kind_for_layer(layer: &str) -> Option<(TakeoffKind, Option<f64>)> {
    let (kind, thick) = if layer.eq_ignore_ascii_case(L_FULL) {
        (TakeoffKind::ExtWall, Some(230.0))
    } else if layer.eq_ignore_ascii_case(L_HALF) {
        (TakeoffKind::IntWall, Some(115.0))
    } else if layer.eq_ignore_ascii_case(L_BEAM) {
        (TakeoffKind::Beam, None)
    } else if layer.eq_ignore_ascii_case(L_LINTEL) {
        (TakeoffKind::Lintel, None)
    } else if layer.eq_ignore_ascii_case(L_LINEAR) {
        (TakeoffKind::Linear, None)
    } else if layer.eq_ignore_ascii_case(L_COLUMN) {
        (TakeoffKind::Column, None)
    } else if layer.eq_ignore_ascii_case(L_COUNT) {
        (TakeoffKind::Count, None)
    } else if layer.eq_ignore_ascii_case(L_DEDUCT) {
        (TakeoffKind::Deduct, None)
    } else if layer.eq_ignore_ascii_case(L_CEILING) {
        (TakeoffKind::CeilingArea, None)
    } else if layer.eq_ignore_ascii_case(L_FLOOR) {
        (TakeoffKind::FloorArea, None)
    } else if layer.eq_ignore_ascii_case(L_AREA) {
        (TakeoffKind::Area, None)
    } else if layer.eq_ignore_ascii_case(L_SLAB) {
        (TakeoffKind::Slab, None)
    } else if layer.eq_ignore_ascii_case(L_SLAB_DEDUCT) {
        (TakeoffKind::SlabDeduct, None)
    } else {
        return None;
    };
    Some((kind, thick))
}

fn mark_matches(mark: &TakeoffMark, layer: &str) -> bool {
    layer_for_kind(mark.kind).eq_ignore_ascii_case(layer)
}

/// A marked entity matches by its kind. An unmarked entity matches by its MEASURE layer.
fn matches_measure(e: &aad_doc::Entity, layer: &str) -> bool {
    match &e.common.takeoff {
        Some(mark) => mark_matches(mark, layer),
        None => on_layer(e, layer),
    }
}

fn kind_of_wall(e: &aad_doc::Entity, layer: &str) -> (TakeoffKind, Option<f64>, bool) {
    if let Some(mark) = &e.common.takeoff {
        let thick = mark.thickness_mm.or_else(|| mark.kind.default_thickness_mm());
        return (mark.kind, thick, mark.thickness_mm.is_some());
    }
    let (kind, thick) = kind_for_layer(layer).unwrap_or((TakeoffKind::Linear, None));
    (kind, thick, false)
}

/// Copy marks from `takeoffMeta` onto entities that lost them (DXF load). Existing marks stay.
pub(crate) fn apply_meta(d: &mut Drawing) {
    let meta = project::get(d, META_KEY);
    let Some(obj) = meta.as_object() else { return };
    let pairs: Vec<(Handle, TakeoffMark)> = obj
        .iter()
        .filter_map(|(k, v)| {
            let h = Handle::parse_hex(k)?;
            let mark = serde_json::from_value(v.clone()).ok()?;
            Some((h, mark))
        })
        .collect();
    for (h, mark) in pairs {
        let _ = d.modify_entity(h, |e| {
            if e.common.takeoff.is_none() {
                e.common.takeoff = Some(mark);
            }
        });
    }
}

/// Write every entity mark into `takeoffMeta` so the next DXF save keeps them.
pub(crate) fn sync_meta(d: &mut Drawing) {
    let mut m = serde_json::Map::new();
    // Every level and sheet annotation, not only the active one: a mark on an inactive level must survive a save.
    let stores: Vec<&aad_doc::EntityStore> = d.model_stores().chain(d.layouts.iter().map(|l| &l.entities)).collect();
    for store in stores {
        for e in store.iter() {
            if let Some(mark) = &e.common.takeoff
                && let Ok(v) = serde_json::to_value(mark)
            {
                m.insert(e.handle.hex(), v);
            }
        }
    }
    if m.is_empty() {
        d.project.remove(META_KEY);
    } else {
        project::set(d, META_KEY, &serde_json::Value::Object(m));
    }
}

fn parse_kind(cmd: &str, p: &Value) -> Result<TakeoffKind> {
    let raw = str_param(p, "kind").ok_or_else(|| bad(cmd, "`kind` is required"))?;
    TakeoffKind::parse(raw).ok_or_else(|| bad(cmd, format!("unknown take-off kind `{raw}`")))
}

fn thickness_param(cmd: &str, p: &Value, kind: TakeoffKind) -> Result<Option<f64>> {
    match p.get("thicknessMm") {
        None | Some(Value::Null) => Ok(kind.default_thickness_mm()),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| bad(cmd, "`thicknessMm` must be a number"))?;
            if !(n.is_finite() && n > 0.0) {
                return Err(bad(cmd, "`thicknessMm` must be positive"));
            }
            Ok(Some(n))
        }
    }
}

/// The take-off layers a person may draw on or map to: name, ACI colour, description. The label and
/// table layers are output only and are left out.
pub(crate) fn measure_layers() -> Vec<(&'static str, i16, &'static str)> {
    LAYERS.iter().filter(|(n, _, _)| *n != L_LABELS && *n != L_TABLE).copied().collect()
}

fn ensure_measure_layer(d: &mut Drawing, layer: &str) {
    if let Some((name, aci, desc)) = LAYERS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(layer)) {
        ensure_layer(d, name, *aci, "Continuous", 18, desc);
    }
}

pub(crate) fn segments_of(kind: &EntityKind) -> Vec<Segment> {
    match kind {
        EntityKind::Line(l) => vec![Segment::Line(aad_geom::Line::new(l.a.xy(), l.b.xy()))],
        EntityKind::Arc(a) => {
            let arc = aad_geom::Arc { center: a.center.xy(), radius: a.radius, start: a.start, end: a.end };
            vec![Segment::Arc { arc, ccw: true }]
        }
        EntityKind::LwPolyline(p) => Polyline { vertices: p.vertices.clone(), closed: p.closed }.segments(),
        _ => Vec::new(),
    }
}

fn walls(d: &Drawing, space: &Space, layers: &[&str]) -> Vec<Wall> {
    let mut out = Vec::new();
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| d.is_visible(e)) {
            if let Some(layer) = layers.iter().copied().find(|l| matches_measure(e, l)) {
                let (kind, thickness_mm, thickness_set) = kind_of_wall(e, layer);
                for seg in segments_of(&e.kind) {
                    if seg.len() > 1e-12 {
                        out.push(Wall { layer: layer.to_string(), seg, handle: e.handle, kind, thickness_mm, thickness_set });
                    }
                }
            }
        }
    }
    // Left to right, then bottom to top.
    out.sort_by(|a, b| {
        let (ma, mb) = (a.seg.mid(), b.seg.mid());
        ma.x.total_cmp(&mb.x).then(ma.y.total_cmp(&mb.y)).then(a.handle.0.cmp(&b.handle.0))
    });
    out
}

/// Why objects on the take-off layers `layers` are not in a result: hidden by their layer, of a kind the
/// measurement does not read, or on another level. One sentence each, so an empty result explains itself.
fn unmeasured_notes(d: &Drawing, space: &Space, layers: &[&str], measurable: impl Fn(&EntityKind) -> bool, what: &str) -> Vec<String> {
    let on_layers = |e: &aad_doc::Entity| layers.iter().any(|l| matches_measure(e, l));
    let (mut hidden, mut other_kind) = (0usize, 0usize);
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| on_layers(e)) {
            if !d.is_visible(e) {
                hidden += 1;
            } else if !measurable(&e.kind) {
                other_kind += 1;
            }
        }
    }
    let mut notes = Vec::new();
    let on = layers.join(", ");
    if hidden > 0 {
        notes.push(format!(
            "{hidden} object(s) on {on} are hidden (the layer is off or frozen) and were not measured; turn the layer on to include them"
        ));
    }
    if other_kind > 0 {
        notes.push(format!("{other_kind} object(s) on {on} are not {what} and were not measured"));
    }
    if matches!(space, Space::Model) {
        for l in d.levels.iter().filter(|l| Some(l.id) != d.active_level) {
            let n = d.level_store(l.id).map_or(0, |st| st.iter().filter(|e| on_layers(e)).count());
            if n > 0 {
                notes.push(format!(
                    "{n} object(s) on {on} are on level `{}`; take-off measures the active level, so activate that level to measure them",
                    l.name
                ));
            }
        }
    }
    notes
}

struct Shape {
    handle: Handle,
    pts: Vec<Vec2>,
    area: f64,
    perimeter: f64,
    centroid: Vec2,
}

fn shape_of(kind: &EntityKind) -> Option<Vec<Vec2>> {
    match kind {
        EntityKind::LwPolyline(p) if p.closed && p.vertices.len() >= 3 => {
            Some(Polyline { vertices: p.vertices.clone(), closed: true }.tessellate(1e-3))
        }
        EntityKind::Circle(c) => {
            let n = 96;
            Some(
                (0..n)
                    .map(|i| {
                        c.center.xy()
                            + Vec2::new(
                                c.radius * (std::f64::consts::TAU * f64::from(i) / f64::from(n)).cos(),
                                c.radius * (std::f64::consts::TAU * f64::from(i) / f64::from(n)).sin(),
                            )
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

fn polygon_area_centroid(pts: &[Vec2]) -> (f64, Vec2) {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let o = pts.first().copied().unwrap_or(Vec2::ZERO);
    for w in pts.windows(2).chain(std::iter::once(&[pts.last().copied().unwrap_or(o), o][..])) {
        let (p, q) = (w[0] - o, w[1] - o);
        let cr = p.x * q.y - q.x * p.y;
        a += cr;
        cx += (p.x + q.x) * cr;
        cy += (p.y + q.y) * cr;
    }
    if a.abs() < 1e-18 {
        return (0.0, o);
    }
    (a.abs() / 2.0, Vec2::new(o.x + cx / (3.0 * a), o.y + cy / (3.0 * a)))
}

fn shapes(d: &Drawing, space: &Space, layer: &str) -> Vec<Shape> {
    let mut out = Vec::new();
    if let Some(store) = d.space(space) {
        for e in store.iter().filter(|e| d.is_visible(e) && matches_measure(e, layer)) {
            let Some(pts) = shape_of(&e.kind) else { continue };
            let (area, centroid) = polygon_area_centroid(&pts);
            let perimeter: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>() + pts.first().zip(pts.last()).map_or(0.0, |(a, b)| a.dist(*b));
            out.push(Shape { handle: e.handle, pts, area, perimeter, centroid });
        }
    }
    out.sort_by(|a, b| a.centroid.x.total_cmp(&b.centroid.x).then(a.centroid.y.total_cmp(&b.centroid.y)).then(a.handle.0.cmp(&b.handle.0)));
    out
}

/// Wall marks in sorted order: a prefix per layer and a running two-digit number (`FB01`, `HB01`).
fn wall_names(ws: &[Wall], sets: &[(&str, &str)]) -> Vec<String> {
    let mut counters: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    ws.iter()
        .map(|w| {
            let prefix = sets.iter().find(|(l, _)| *l == w.layer).map_or("W", |(_, pfx)| *pfx);
            let n = counters.entry(prefix).or_insert(0);
            *n += 1;
            format!("{prefix}{:02}", *n)
        })
        .collect()
}

struct Deduction {
    a: Vec2,
    b: Vec2,
    from_block: bool,
}

impl Deduction {
    fn len(&self) -> f64 {
        self.a.dist(self.b)
    }
    fn mid(&self) -> Vec2 {
        (self.a + self.b) * 0.5
    }
}

/// Lines on the deduction layer, including the ones inside block references (transformed to
/// drawing coordinates).
fn deductions(d: &Drawing, space: &Space) -> Vec<Deduction> {
    let mut out = Vec::new();
    let Some(store) = d.space(space) else { return out };
    for e in store.iter().filter(|e| d.is_visible(e)) {
        match &e.kind {
            EntityKind::Line(l) if matches_measure(e, L_DEDUCT) => out.push(Deduction { a: l.a.xy(), b: l.b.xy(), from_block: false }),
            EntityKind::LwPolyline(p) if matches_measure(e, L_DEDUCT) => {
                for seg in (Polyline { vertices: p.vertices.clone(), closed: p.closed }).segments() {
                    out.push(Deduction { a: seg.start(), b: seg.end(), from_block: false });
                }
            }
            EntityKind::Insert(i) => {
                let Some(b) = d.block(&i.block) else { continue };
                let (sin, cos) = i.rotation.sin_cos();
                let tf = |p: aad_geom::Vec3| {
                    let (x, y) = ((p.x - b.base.x) * i.scale.x, (p.y - b.base.y) * i.scale.y);
                    Vec2::new(i.insert.x + x * cos - y * sin, i.insert.y + x * sin + y * cos)
                };
                for be in b.entities.iter() {
                    if let EntityKind::Line(l) = &be.kind
                        && matches_measure(be, L_DEDUCT)
                    {
                        out.push(Deduction { a: tf(l.a), b: tf(l.b), from_block: true });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// A deduction matched to a wall: its position along the wall orders the openings.
struct Matched {
    wall: usize,
    len: f64,
    t: f64,
}

/// Match each deduction to the nearest wall within `tol`. A standalone deduction that touches no
/// wall is returned in the second list; a block deduction that touches none belongs to another wall
/// and is ignored.
fn match_deductions(walls: &[Wall], deds: &[Deduction], tol: f64) -> (Vec<Matched>, Vec<usize>) {
    let mut matched = Vec::new();
    let mut orphans = Vec::new();
    for (di, dd) in deds.iter().enumerate() {
        let m = dd.mid();
        let best = walls
            .iter()
            .enumerate()
            .map(|(i, w)| (i, w.seg.closest(m).dist(m)))
            .filter(|(_, dist)| *dist <= tol)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        match best {
            Some((wi, _)) => {
                let w = &walls[wi];
                let (s0, s1) = (w.seg.start(), w.seg.end());
                let dir = s1 - s0;
                let t = if dir.len() > 1e-12 { (m - s0).dot(dir) / dir.len() } else { 0.0 };
                matched.push(Matched { wall: wi, len: dd.len(), t });
            }
            None if !dd.from_block => orphans.push(di),
            None => {}
        }
    }
    (matched, orphans)
}

/// Deduction midpoints that live inside an opening insert's block, keyed by the insert handle.
fn opening_block_deductions(d: &Drawing, space: &Space) -> Vec<(Handle, Vec2)> {
    let mut out = Vec::new();
    let Some(store) = d.space(space) else { return out };
    for e in store.iter().filter(|e| d.is_visible(e)) {
        let EntityKind::Insert(i) = &e.kind else { continue };
        let Some(b) = d.block(&i.block) else { continue };
        let (sin, cos) = i.rotation.sin_cos();
        let tf = |p: aad_geom::Vec3| {
            let (x, y) = ((p.x - b.base.x) * i.scale.x, (p.y - b.base.y) * i.scale.y);
            Vec2::new(i.insert.x + x * cos - y * sin, i.insert.y + x * sin + y * cos)
        };
        for be in b.entities.iter() {
            if let EntityKind::Line(l) = &be.kind
                && on_layer(be, L_DEDUCT)
            {
                let a = tf(l.a);
                let bpt = tf(l.b);
                out.push((e.handle, (a + bpt) * 0.5));
            }
        }
    }
    out
}

/// For each opening insert whose `MEASURE-DEDUCT` line sits on a measured wall, the wall handle.
/// Prefer this over insert-point proximity when writing `wall_ref` (same match `takeoff.brick` uses).
pub(crate) fn opening_wall_by_deduct(d: &Drawing, space: &Space, wall_handles: &[Handle], tol: f64) -> std::collections::HashMap<u64, Handle> {
    let layers = [L_FULL, L_HALF];
    let all = walls(d, space, &layers);
    let ws: Vec<Wall> = all.into_iter().filter(|w| wall_handles.contains(&w.handle)).collect();
    let mut out = std::collections::HashMap::new();
    for (ins, mid) in opening_block_deductions(d, space) {
        if out.contains_key(&ins.0) {
            continue;
        }
        let best = ws
            .iter()
            .map(|w| (w.handle, w.seg.closest(mid).dist(mid)))
            .filter(|(_, dist)| *dist <= tol)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.0.cmp(&b.0.0)));
        if let Some((h, _)) = best {
            out.insert(ins.0, h);
        }
    }
    out
}

/// Thickness in millimetres from parallel `A-WALL` face spacing beside a MEASURE centre line.
/// Returns `None` when faces are missing or ambiguous so the caller can keep the layer nominal.
pub(crate) fn thickness_from_faces_mm(d: &Drawing, space: &Space, measure: Handle, unit_mm: f64) -> Option<f64> {
    if !(unit_mm.is_finite() && unit_mm > 0.0) {
        return None;
    }
    let e = d.entity(measure)?;
    let segs = segments_of(&e.kind);
    let measure_seg = segs.into_iter().max_by(|a, b| a.len().total_cmp(&b.len()))?;
    if measure_seg.len() < 1e-9 {
        return None;
    }
    let (ms, me) = (measure_seg.start(), measure_seg.end());
    let mdir = me - ms;
    let mlen = mdir.len();
    if mlen < 1e-9 {
        return None;
    }
    let mu = mdir * (1.0 / mlen);
    let mid = measure_seg.mid();
    // Search band ≈ half a metre each side of the centre line.
    let reach = 500.0 / unit_mm;
    let mut offsets: Vec<f64> = Vec::new();
    let store = d.space(space)?;
    for face in store.iter().filter(|e| d.is_visible(e) && on_layer(e, super::walls::WALL)) {
        for seg in segments_of(&face.kind) {
            if seg.len() < 1e-9 {
                continue;
            }
            let (a, b) = (seg.start(), seg.end());
            let dir = b - a;
            let len = dir.len();
            if len < 1e-9 {
                continue;
            }
            let u = dir * (1.0 / len);
            // Parallel (or anti-parallel) to the measure line.
            if u.dot(mu).abs() < 0.98 {
                continue;
            }
            let closest = measure_seg.closest(seg.mid());
            if closest.dist(seg.mid()) > reach {
                continue;
            }
            // Along-measure overlap: face midpoint projects onto the measure segment.
            let t = (seg.mid() - ms).dot(mu);
            if t < -1e-6 || t > mlen + 1e-6 {
                continue;
            }
            let off = (seg.mid() - mid).x * (-mu.y) + (seg.mid() - mid).y * mu.x;
            if off.is_finite() && off.abs() > 1e-9 {
                offsets.push(off);
            }
        }
    }
    if offsets.len() < 2 {
        return None;
    }
    let (mut pos, mut neg): (Vec<f64>, Vec<f64>) = offsets.into_iter().partition(|o| *o > 0.0);
    if pos.is_empty() || neg.is_empty() {
        return None;
    }
    pos.sort_by(|a, b| a.total_cmp(b));
    neg.sort_by(|a, b| a.total_cmp(b));
    let med = |v: &[f64]| v[v.len() / 2];
    let thick_du = med(&pos) - med(&neg);
    if !(thick_du.is_finite() && thick_du > 0.0) {
        return None;
    }
    let mm = (thick_du * unit_mm * 10.0).round() / 10.0;
    (75.0..=600.0).contains(&mm).then_some(mm)
}

// ---------------------------------------------------------------------------------------------
// Output: labels, tables, saved results

fn group_letter(i: usize) -> String {
    let mut n = i;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}

fn label(s: &mut Session, c: &Ctx, at: Vec2, text: &str) -> Result<Handle> {
    ensure_layer(s.doc_mut()?, L_LABELS, 7, "Continuous", 18, "Take-off labels");
    let kind = make_text(s, at, text, c.th, 0.0, (HAlign::Center, VAlign::Middle));
    add_on(s, L_LABELS, kind)
}

/// Draw a table (title, header, rows) with its top-left corner at `at`, on `MEASURE-TABLE`.
fn draw_table(s: &mut Session, c: &Ctx, at: Vec2, title: &str, header: &[String], rows: &[Vec<String>]) -> Result<()> {
    ensure_layer(s.doc_mut()?, L_TABLE, 7, "Continuous", 18, "Take-off tables");
    let th = c.th;
    let ncols = header.len();
    let mut widths: Vec<f64> = header.iter().map(|h| h.chars().count() as f64).collect();
    for r in rows {
        for (i, cell) in r.iter().enumerate().take(ncols) {
            if let Some(w) = widths.get_mut(i) {
                *w = w.max(cell.chars().count() as f64);
            }
        }
    }
    let widths: Vec<f64> = widths.iter().map(|w| w * th * 0.75 + th * 1.2).collect();
    let total: f64 = widths.iter().sum();
    let rh = th * 1.8;
    let mut y = at.y;
    // Title row, then the header and body rows.
    let kind = make_text(s, Vec2::new(at.x + th * 0.4, y - rh * 0.5), title, th * 1.2, 0.0, (HAlign::Left, VAlign::Middle));
    add_on(s, L_TABLE, kind)?;
    y -= rh;
    for (ri, row) in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)).enumerate() {
        let mut x = at.x;
        for (ci, w) in widths.iter().enumerate() {
            let text = row.get(ci).map_or("", String::as_str);
            let kind =
                make_text(s, Vec2::new(x + th * 0.4, y - rh * 0.5), text, if ri == 0 { th } else { th * 0.9 }, 0.0, (HAlign::Left, VAlign::Middle));
            add_on(s, L_TABLE, kind)?;
            x += w;
        }
        add_on(s, L_TABLE, line(Vec2::new(at.x, y - rh), Vec2::new(at.x + total, y - rh)))?;
        y -= rh;
    }
    let (top, bottom) = (at.y - rh, y);
    add_on(s, L_TABLE, line(Vec2::new(at.x, at.y), Vec2::new(at.x + total, at.y)))?;
    let mut x = at.x;
    for w in std::iter::once(&0.0).chain(widths.iter()) {
        x += w;
        add_on(s, L_TABLE, line(Vec2::new(x, top), Vec2::new(x, bottom)))?;
    }
    add_on(s, L_TABLE, line(Vec2::new(at.x, at.y), Vec2::new(at.x, bottom)))?;
    Ok(())
}

/// Where the next table goes: `at`, else below the drawing's extents.
fn table_origin(s: &Session, c: &Ctx, p: &Value) -> Vec2 {
    if let Some(at) = point_param(p, "at") {
        return at;
    }
    let ext = s.doc().map(|d| d.extents(&Space::Model)).unwrap_or(Bounds2::EMPTY);
    if ext.is_empty() { Vec2::ZERO } else { Vec2::new(ext.min.x, ext.min.y - 4.0 * c.th) }
}

struct Report {
    name: &'static str,
    title: String,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    /// Per-element details for the caller.
    items: Vec<Value>,
    totals: Value,
    warnings: Vec<String>,
}

fn finish(s: &mut Session, c: &Ctx, p: &Value, r: Report) -> Result<Value> {
    if bool_or(p, "table", true) && !r.rows.is_empty() {
        let at = table_origin(s, c, p);
        draw_table(s, c, at, &r.title, &r.header, &r.rows)?;
    }
    let result = json!({
        "name": r.name, "title": r.title, "lengthUnit": c.len_unit(), "areaUnit": c.area_unit(),
        "columns": r.header, "rows": r.rows, "items": r.items, "totals": r.totals, "warnings": r.warnings,
    });
    project::set(s.doc_mut()?, &format!("takeoff:{}", r.name), &result);
    Ok(result)
}

fn run_start(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    let mut created = 0;
    for (name, aci, desc) in LAYERS {
        if ensure_layer(d, name, *aci, "Continuous", 18, desc) {
            created += 1;
        }
    }
    Ok(json!({ "created": created, "layers": LAYERS.iter().map(|l| l.0).collect::<Vec<_>>() }))
}

// ---------------------------------------------------------------------------------------------
// Wall measurements

fn measure_walls(s: &mut Session, p: &Value, cmd: &str, name: &'static str, title: &str, sets: &[(&str, &str)]) -> Result<Value> {
    let c = ctx(s, p, cmd)?;
    let space = s.space();
    let layers: Vec<&str> = sets.iter().map(|(l, _)| *l).collect();
    let (ws, ded, tol) = {
        let d = s.doc()?;
        (walls(d, &space, &layers), deductions(d, &space), c.tol)
    };
    let (matched, orphans) = match_deductions(&ws, &ded, tol);
    // Numbering per prefix in sorted order; openings numbered along each wall.
    let names = wall_names(&ws, sets);
    let mut groups: Vec<(String, f64)> = Vec::new();
    struct Row {
        group: String,
        len: f64,
        ded: f64,
    }
    let mut rowsv = Vec::new();
    let mut items = Vec::new();
    for (i, w) in ws.iter().enumerate() {
        let len = c.len(w.seg.len());
        let prefix = sets.iter().find(|(l, _)| *l == w.layer).map_or("W", |(_, pfx)| *pfx);
        let key = format!("{prefix}-{len}");
        let gi = match groups.iter().position(|(k, _)| *k == key) {
            Some(g) => g,
            None => {
                groups.push((key, len));
                groups.len() - 1
            }
        };
        let letter =
            group_letter(groups.iter().filter(|(k, _)| k.starts_with(&format!("{prefix}-"))).position(|(k, _)| *k == groups[gi].0).unwrap_or(0));
        let mut mine: Vec<&Matched> = matched.iter().filter(|m| m.wall == i).collect();
        mine.sort_by(|a, b| a.t.total_cmp(&b.t));
        let ded_names: Vec<(String, f64)> = mine.iter().enumerate().map(|(k, m)| (format!("{}-D{}", names[i], k + 1), c.len(m.len))).collect();
        let ded_total: f64 = ded_names.iter().fold(0.0, |a, (_, l)| a + l);
        let mid = w.seg.mid();
        label(s, &c, Vec2::new(mid.x, mid.y + c.th * 1.2), &names[i])?;
        for (k, m) in mine.iter().enumerate() {
            let wm = w.seg.closest(w.seg.mid());
            let _ = wm;
            let at = w.seg.start() + (w.seg.end() - w.seg.start()).normalized() * m.t;
            label(s, &c, Vec2::new(at.x, at.y - c.th * 1.2), &ded_names[k].0)?;
        }
        items.push(json!({
            "mark": names[i], "group": format!("{prefix}-{letter}"), "handle": w.handle.hex(),
            "length": len, "wallType": w.kind.wall_type(w.thickness_mm), "thicknessMm": w.thickness_mm.filter(|_| w.thickness_set),
            "openings": ded_names.iter().map(|(n, l)| json!({"name": n, "length": l})).collect::<Vec<_>>(),
            "net": (len - ded_total).max(0.0),
        }));
        rowsv.push(Row { group: format!("{prefix}-{letter}"), len, ded: ded_total });
    }
    for di in &orphans {
        let m = ded[*di].mid();
        label(s, &c, Vec2::new(m.x, m.y - c.th * 1.2), "D? NO LINE")?;
    }
    // Group summary: same group, same length.
    let mut order: Vec<String> = Vec::new();
    for r in &rowsv {
        if !order.contains(&r.group) {
            order.push(r.group.clone());
        }
    }
    let mut rows = Vec::new();
    let (mut tl, mut td, mut tn) = (0.0, 0.0, 0.0);
    for g in &order {
        let members: Vec<&Row> = rowsv.iter().filter(|r| r.group == *g).collect();
        let each = members.first().map_or(0.0, |r| r.len);
        let total: f64 = members.iter().fold(0.0, |a, r| a + r.len);
        let ded: f64 = members.iter().fold(0.0, |a, r| a + r.ded);
        let net = (total - ded).max(0.0);
        tl += total;
        td += ded;
        tn += net;
        rows.push(vec![g.clone(), members.len().to_string(), format!("{each:.2}"), format!("{total:.2}"), format!("{ded:.2}"), format!("{net:.2}")]);
    }
    if !rows.is_empty() {
        rows.push(vec!["TOTAL".into(), rowsv.len().to_string(), String::new(), format!("{tl:.2}"), format!("{td:.2}"), format!("{tn:.2}")]);
    }
    let mut warnings = Vec::new();
    if ws.is_empty() {
        warnings.push(format!("nothing found on {}", layers.join(", ")));
    }
    warnings.extend(unmeasured_notes(s.doc()?, &space, &layers, |k| !segments_of(k).is_empty(), "lines, arcs or polylines"));
    if !orphans.is_empty() {
        warnings.push(format!("{} deduction line(s) lie on no wall (labelled D? NO LINE) and are not counted", orphans.len()));
    }
    let header =
        ["Name", "Count", &format!("Length ({})", c.len_unit()), "Total", "Openings", "Net total"].iter().map(|x| (*x).to_string()).collect();
    finish(
        s,
        &c,
        p,
        Report {
            name,
            title: title.into(),
            header,
            rows,
            items,
            totals: json!({ "count": rowsv.len(), "length": (tl * 100.0).round() / 100.0, "openings": (td * 100.0).round() / 100.0, "net": (tn * 100.0).round() / 100.0 }),
            warnings,
        },
    )
}

fn run_linear(s: &mut Session, p: &Value) -> Result<Value> {
    measure_walls(s, p, "takeoff.linear", "Linear", "LINEAR TAKE-OFF", &[(L_LINEAR, "L")])
}

fn run_brick(s: &mut Session, p: &Value) -> Result<Value> {
    measure_walls(s, p, "takeoff.brick", "Bricks", "BRICKWORK", &[(L_FULL, "FB"), (L_HALF, "HB")])
}

fn run_beams(s: &mut Session, p: &Value) -> Result<Value> {
    measure_walls(s, p, "takeoff.beams", "Beams", "CONCRETE BEAMS AND LINTELS", &[(L_BEAM, "BM"), (L_LINTEL, "LT")])
}

// ---------------------------------------------------------------------------------------------
// Closed outlines

/// Group shapes of equal rounded `key`; returns the table rows and per-item details.
fn area_groups(s: &mut Session, c: &Ctx, shapes: &[Shape], prefix: &str, with_perimeter: bool) -> Result<(Vec<Vec<String>>, Vec<Value>, f64)> {
    let mut keys: Vec<f64> = Vec::new();
    let mut items = Vec::new();
    let mut members: Vec<Vec<f64>> = Vec::new();
    for (i, sh) in shapes.iter().enumerate() {
        let a = c.area(sh.area);
        let g = match keys.iter().position(|k| (*k - a).abs() < 1e-9) {
            Some(g) => g,
            None => {
                keys.push(a);
                members.push(Vec::new());
                keys.len() - 1
            }
        };
        members[g].push(a);
        let name = format!("{prefix}-{}", group_letter(g));
        label(s, c, sh.centroid, &name)?;
        items.push(
            json!({ "mark": format!("{prefix}{:02}", i + 1), "group": name, "handle": sh.handle.hex(), "area": a, "perimeter": c.len(sh.perimeter) }),
        );
    }
    let mut rows = Vec::new();
    let mut grand = 0.0;
    for (g, k) in keys.iter().enumerate() {
        let n = members[g].len();
        let total = (k * n as f64 * 100.0).round() / 100.0;
        grand += total;
        let per: f64 = shapes.iter().filter(|sh| (c.area(sh.area) - k).abs() < 1e-9).map(|sh| c.len(sh.perimeter)).next().unwrap_or(0.0);
        let mut row = vec![format!("{prefix}-{}", group_letter(g)), n.to_string(), format!("{k:.2}"), format!("{total:.2}")];
        if with_perimeter {
            row.push(format!("{per:.2}"));
        }
        rows.push(row);
    }
    if !rows.is_empty() {
        let mut row = vec!["TOTAL".into(), shapes.len().to_string(), String::new(), format!("{grand:.2}")];
        if with_perimeter {
            row.push(String::new());
        }
        rows.push(row);
    }
    Ok((rows, items, (grand * 100.0).round() / 100.0))
}

fn area_command(s: &mut Session, p: &Value, cmd: &str, layer: &str, name: &'static str, title: &str, prefix: &str) -> Result<Value> {
    let c = ctx(s, p, cmd)?;
    let space = s.space();
    let sh = shapes(s.doc()?, &space, layer);
    let (rows, items, grand) = area_groups(s, &c, &sh, prefix, false)?;
    let mut warnings = if sh.is_empty() { vec![format!("no closed outlines on {layer}")] } else { Vec::new() };
    warnings.extend(unmeasured_notes(s.doc()?, &space, &[layer], |k| shape_of(k).is_some(), "closed polylines or circles"));
    let header = ["Name", "Count", &format!("Area ({})", c.area_unit()), "Total"].iter().map(|x| (*x).to_string()).collect();
    finish(s, &c, p, Report { name, title: title.into(), header, rows, items, totals: json!({ "count": sh.len(), "area": grand }), warnings })
}

fn run_ceiling(s: &mut Session, p: &Value) -> Result<Value> {
    area_command(s, p, "takeoff.ceiling", L_CEILING, "Ceiling", "CEILING PAINT", "CP")
}

fn run_floor(s: &mut Session, p: &Value) -> Result<Value> {
    area_command(s, p, "takeoff.floor", L_FLOOR, "Floor", "FLOOR AREA", "FL")
}

fn run_area(s: &mut Session, p: &Value) -> Result<Value> {
    let c = ctx(s, p, "takeoff.area")?;
    let space = s.space();
    let sh = shapes(s.doc()?, &space, L_AREA);
    let mut rows = Vec::new();
    let mut items = Vec::new();
    let mut total = 0.0;
    for (i, x) in sh.iter().enumerate() {
        let (a, per) = (c.area(x.area), c.len(x.perimeter));
        total += a;
        label(s, &c, x.centroid, &format!("A{}", i + 1))?;
        rows.push(vec![format!("A{}", i + 1), format!("{a:.2}"), format!("{per:.2}")]);
        items.push(json!({ "mark": format!("A{}", i + 1), "handle": x.handle.hex(), "area": a, "perimeter": per }));
    }
    if !rows.is_empty() {
        rows.push(vec!["TOTAL".into(), format!("{:.2}", (total * 100.0).round() / 100.0), String::new()]);
    }
    let header = ["Name", &format!("Area ({})", c.area_unit()), &format!("Perimeter ({})", c.len_unit())].iter().map(|x| (*x).to_string()).collect();
    let mut warnings = if sh.is_empty() { vec![format!("no closed outlines on {L_AREA}")] } else { Vec::new() };
    warnings.extend(unmeasured_notes(s.doc()?, &space, &[L_AREA], |k| shape_of(k).is_some(), "closed polylines or circles"));
    finish(
        s,
        &c,
        p,
        Report {
            name: "Areas",
            title: "AREAS".into(),
            header,
            rows,
            items,
            totals: json!({ "count": sh.len(), "area": (total * 100.0).round() / 100.0 }),
            warnings,
        },
    )
}

fn run_slab(s: &mut Session, p: &Value) -> Result<Value> {
    let c = ctx(s, p, "takeoff.slab")?;
    let space = s.space();
    let (slabs, holes) = {
        let d = s.doc()?;
        (shapes(d, &space, L_SLAB), shapes(d, &space, L_SLAB_DEDUCT))
    };
    let mut rows = Vec::new();
    let mut items = Vec::new();
    let mut grand = 0.0;
    let mut warnings = Vec::new();
    let mut used = vec![false; holes.len()];
    for (i, sl) in slabs.iter().enumerate() {
        let mut ded = 0.0;
        for (hi, h) in holes.iter().enumerate() {
            if point_in_polygon(&sl.pts, h.centroid) {
                ded += h.area;
                used[hi] = true;
            }
        }
        let (gross, d2, net) = (c.area(sl.area), c.area(ded), c.area((sl.area - ded).max(0.0)));
        grand += net;
        label(s, &c, sl.centroid, &format!("S{}", i + 1))?;
        rows.push(vec![format!("S{}", i + 1), format!("{gross:.2}"), format!("{d2:.2}"), format!("{net:.2}")]);
        items.push(json!({ "mark": format!("S{}", i + 1), "handle": sl.handle.hex(), "gross": gross, "openings": d2, "net": net }));
    }
    for (hi, u) in used.iter().enumerate() {
        if !u {
            warnings.push(format!("opening {} lies in no slab outline and is not counted", holes[hi].handle.hex()));
        }
    }
    if slabs.is_empty() {
        warnings.push(format!("no closed outlines on {L_SLAB}"));
    } else {
        rows.push(vec!["TOTAL".into(), String::new(), String::new(), format!("{:.2}", (grand * 100.0).round() / 100.0)]);
    }
    let header = ["Name", &format!("Gross ({})", c.area_unit()), "Openings", "Net"].iter().map(|x| (*x).to_string()).collect();
    finish(
        s,
        &c,
        p,
        Report {
            name: "Slab",
            title: "SLABS".into(),
            header,
            rows,
            items,
            totals: json!({ "count": slabs.len(), "net": (grand * 100.0).round() / 100.0 }),
            warnings,
        },
    )
}

/// Width and breadth of a four-sided outline (longer side first), or `None`.
fn rect_size(sh: &Shape) -> Option<(f64, f64)> {
    let p = &sh.pts;
    let n = if p.len() > 1 && p.first().zip(p.last()).is_some_and(|(a, b)| a.near(*b, 1e-9)) { p.len() - 1 } else { p.len() };
    if n != 4 {
        return None;
    }
    let (a, b) = (p[0].dist(p[1]), p[1].dist(p[2]));
    let ok = (p[0].dist(p[2]) - p[1].dist(p[3])).abs() <= 1e-6 * (a + b);
    ok.then_some(if a >= b { (a, b) } else { (b, a) })
}

fn run_columns(s: &mut Session, p: &Value) -> Result<Value> {
    let c = ctx(s, p, "takeoff.columns")?;
    let space = s.space();
    let layer = str_param(p, "layer").unwrap_or(L_COLUMN).to_string();
    let all = shapes(s.doc()?, &space, &layer);
    let mut sizes: Vec<(f64, f64)> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for (i, sh) in all.iter().enumerate() {
        let Some((l, b)) = rect_size(sh) else {
            warnings.push(format!("{} is not a four-sided outline and is not counted", sh.handle.hex()));
            continue;
        };
        let key = (c.len(l), c.len(b));
        let g = match sizes.iter().position(|k| *k == key) {
            Some(g) => g,
            None => {
                sizes.push(key);
                counts.push(0);
                sizes.len() - 1
            }
        };
        counts[g] += 1;
        label(s, &c, sh.centroid, &format!("C{}", g + 1))?;
        items.push(json!({ "mark": format!("C{}", g + 1), "handle": sh.handle.hex(), "length": key.0, "breadth": key.1, "index": i }));
    }
    let mut rows: Vec<Vec<String>> =
        sizes.iter().enumerate().map(|(g, (l, b))| vec![format!("C{}", g + 1), format!("{l:.2} x {b:.2}"), counts[g].to_string()]).collect();
    if !rows.is_empty() {
        rows.push(vec!["TOTAL".into(), String::new(), counts.iter().sum::<usize>().to_string()]);
    }
    let header = ["Mark", &format!("Size ({})", c.len_unit()), "Count"].iter().map(|x| (*x).to_string()).collect();
    finish(
        s,
        &c,
        p,
        Report {
            name: "Columns",
            title: "COLUMN SCHEDULE".into(),
            header,
            rows,
            items,
            totals: json!({ "count": counts.iter().sum::<usize>(), "marks": sizes.len() }),
            warnings,
        },
    )
}

fn run_rects(s: &mut Session, p: &Value) -> Result<Value> {
    let c = ctx(s, p, "takeoff.rects")?;
    let space = s.space();
    let all = shapes(s.doc()?, &space, L_COUNT);
    let mut keys: Vec<(f64, f64)> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for sh in &all {
        let Some((l, b)) = rect_size(sh) else {
            warnings.push(format!("{} is not a four-sided outline and is not counted", sh.handle.hex()));
            continue;
        };
        let key = (c.len(l), c.len(b));
        let g = match keys.iter().position(|k| *k == key) {
            Some(g) => g,
            None => {
                keys.push(key);
                counts.push(0);
                keys.len() - 1
            }
        };
        counts[g] += 1;
        label(s, &c, sh.centroid, &format!("R-{}", group_letter(g)))?;
        items.push(json!({ "mark": format!("R-{}", group_letter(g)), "handle": sh.handle.hex() }));
    }
    let mut rows: Vec<Vec<String>> = keys
        .iter()
        .enumerate()
        .map(|(g, (l, b))| {
            let area = ((l * b) * 100.0).round() / 100.0;
            vec![
                format!("R-{}", group_letter(g)),
                format!("{l:.2}"),
                format!("{b:.2}"),
                counts[g].to_string(),
                format!("{area:.2}"),
                format!("{:.2}", 2.0 * (l + b)),
            ]
        })
        .collect();
    if !rows.is_empty() {
        rows.push(vec!["TOTAL".into(), String::new(), String::new(), counts.iter().sum::<usize>().to_string(), String::new(), String::new()]);
    }
    let header = ["Name", "Length", "Breadth", "Count", "Area", "Perimeter"].iter().map(|x| (*x).to_string()).collect();
    finish(
        s,
        &c,
        p,
        Report {
            name: "Rectangles",
            title: "RECTANGLES".into(),
            header,
            rows,
            items,
            totals: json!({ "count": counts.iter().sum::<usize>() }),
            warnings,
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Wall paint

fn run_paint(s: &mut Session, p: &Value) -> Result<Value> {
    let c = ctx(s, p, "takeoff.paint")?;
    let space = s.space();
    let floors = project::floors(s.doc()?);
    let floor = match str_param(p, "floor") {
        Some(n) => floors
            .iter()
            .find(|f| f["name"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(n)))
            .ok_or_else(|| bad("takeoff.paint", format!("no floor named `{n}`")))?,
        None => floors.first().ok_or_else(|| bad("takeoff.paint", "no floors"))?,
    };
    // Heights are metres in the project data; convert to drawing units for the length x height product.
    let height_m = floor["ceiling"].as_f64().unwrap_or(2.85);
    let sched = project::get(s.doc()?, "schedule");
    let (ws, ded) = {
        let d = s.doc()?;
        (walls(d, &space, &[L_FULL, L_HALF, L_LINEAR]), deductions(d, &space))
    };
    let (matched, orphans) = match_deductions(&ws, &ded, c.tol);
    let pnames = wall_names(&ws, &[(L_FULL, "FB"), (L_HALF, "HB"), (L_LINEAR, "L")]);
    let to_m = c.unit.metres();
    let mut total_wall = 0.0;
    let mut total_open = 0.0;
    let mut total_ext = 0.0;
    let mut total_int = 0.0;
    let mut items = Vec::new();
    let mut rows = Vec::new();
    for (i, w) in ws.iter().enumerate() {
        let len_m = w.seg.len() * to_m;
        let one_face = len_m * height_m;
        // External walls: one external face and one internal face. Internal walls: two internal faces.
        let external = w.kind.wall_type(w.thickness_mm) == "external";
        let (ext_faces, int_faces) = if external { (1.0, 1.0) } else { (0.0, 2.0) };
        let faces = ext_faces + int_faces;
        let mut open_one = 0.0;
        let mut mine: Vec<&Matched> = matched.iter().filter(|m| m.wall == i).collect();
        mine.sort_by(|a, b| a.t.total_cmp(&b.t));
        for (k, m) in mine.iter().enumerate() {
            let measured = m.len * to_m;
            let label = format!("{}-D{}", pnames[i], k + 1);
            let mapped = sched["map"][&label]
                .as_str()
                .and_then(|n| sched["openings"].as_array()?.iter().find(|o| o["name"].as_str() == Some(n)))
                .and_then(|o| Some(o["length"].as_f64()? * o["height"].as_f64()?));
            open_one += mapped.unwrap_or(measured * height_m);
        }
        let net_one = (one_face - open_one).max(0.0);
        let ext_paint = net_one * ext_faces;
        let int_paint = net_one * int_faces;
        total_wall += one_face * faces;
        total_open += open_one * faces;
        total_ext += ext_paint;
        total_int += int_paint;
        items.push(json!({
            "handle": w.handle.hex(), "length": len_m, "height": height_m, "wallType": w.kind.wall_type(w.thickness_mm),
            "thicknessMm": w.thickness_mm.filter(|_| w.thickness_set),
            "wallArea": one_face * faces, "openings": open_one * faces, "net": ext_paint + int_paint,
            "externalPaint": ext_paint, "internalPaint": int_paint,
        }));
        let mid = w.seg.mid();
        label(s, &c, Vec2::new(mid.x, mid.y + c.th * 1.2), &format!("{:.2}", (((ext_paint + int_paint) * 100.0).round()) / 100.0))?;
    }
    let conv = |m2: f64| if c.metric() { m2 } else { m2 / (0.3048 * 0.3048) };
    let (tw, to, tn, te, ti) = (conv(total_wall), conv(total_open), conv(total_ext + total_int), conv(total_ext), conv(total_int));
    if !ws.is_empty() {
        rows.push(vec![
            ws.len().to_string(),
            format!("{height_m:.2}"),
            format!("{tw:.2}"),
            format!("{to:.2}"),
            format!("{te:.2}"),
            format!("{ti:.2}"),
            format!("{tn:.2}"),
        ]);
    }
    let header = ["Walls", "Height (m)", &format!("Gross ({})", c.area_unit()), "Openings", "External", "Internal", "Net paint"]
        .iter()
        .map(|x| (*x).to_string())
        .collect();
    let mut warnings = Vec::new();
    if ws.is_empty() {
        warnings.push("no walls found on the brick or linear layers, or tagged as walls".to_string());
    }
    if !orphans.is_empty() {
        warnings.push(format!("{} deduction line(s) lie on no wall and are not counted", orphans.len()));
    }
    finish(
        s,
        &c,
        p,
        Report {
            name: "WallPaint",
            title: "WALL PAINT".into(),
            header,
            rows,
            items,
            totals: json!({
                "wallArea": (tw * 100.0).round() / 100.0,
                "openings": (to * 100.0).round() / 100.0,
                "externalPaint": (te * 100.0).round() / 100.0,
                "internalPaint": (ti * 100.0).round() / 100.0,
                "net": (tn * 100.0).round() / 100.0,
                "height": height_m,
            }),
            warnings,
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Clear, list, export

fn run_clear(s: &mut Session, _p: &Value) -> Result<Value> {
    let hs: Vec<Handle> = {
        let d = s.doc()?;
        d.model.iter().filter(|e| on_layer(e, L_LABELS) || on_layer(e, L_TABLE)).map(|e| e.handle).collect()
    };
    let d = s.doc_mut()?;
    for h in &hs {
        d.remove_entity(*h);
    }
    Ok(json!({ "erased": hs.len() }))
}

/// Snapshot of every `takeoff:*` project entry (tables for the project file / restore).
pub(crate) fn snapshot_tables(d: &Drawing) -> Value {
    let mut m = serde_json::Map::new();
    for k in d.project.keys() {
        if let Some(name) = k.strip_prefix("takeoff:") {
            let v = project::get(d, k);
            if !v.is_null() {
                m.insert(name.to_string(), v);
            }
        }
    }
    Value::Object(m)
}

/// Write `takeoff:*` project entries from a `takeoff_tables` object (name → saved result).
pub(crate) fn apply_tables(d: &mut Drawing, tables: &Value) -> usize {
    let Some(obj) = tables.as_object() else { return 0 };
    let mut n = 0;
    for (name, v) in obj {
        if v.is_null() {
            continue;
        }
        project::set(d, &format!("takeoff:{name}"), v);
        n += 1;
    }
    n
}

/// Redraw MEASURE-TABLE from saved `takeoff:*` results. Returns the names redrawn.
pub(crate) fn redraw_tables(s: &mut Session, p: &Value, names: &[String]) -> Result<Vec<String>> {
    if bool_or(p, "clear", true) {
        let hs: Vec<Handle> = {
            let d = s.doc()?;
            d.model.iter().filter(|e| on_layer(e, L_TABLE)).map(|e| e.handle).collect()
        };
        let d = s.doc_mut()?;
        for h in &hs {
            d.remove_entity(*h);
        }
    }
    let c = match ctx(s, p, "takeoff.restore") {
        Ok(c) => c,
        Err(_) => table_ctx(s.doc()?.header.f64("TEXTSIZE", 0.2).max(0.05) * 2.5),
    };
    let mut at = table_origin(s, &c, p);
    let mut restored = Vec::new();
    for name in names {
        let v = project::get(s.doc()?, &format!("takeoff:{name}"));
        if v.is_null() {
            continue;
        }
        let title = v.get("title").and_then(Value::as_str).unwrap_or(name).to_string();
        let header: Vec<String> =
            v.get("columns").and_then(Value::as_array).into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
        let rows: Vec<Vec<String>> = v
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()))
            .collect();
        if header.is_empty() || rows.is_empty() {
            continue;
        }
        draw_table(s, &c, at, &title, &header, &rows)?;
        // Stack the next table below this one (title + header + rows + gap).
        let rh = c.th * 1.8;
        at.y -= rh * (rows.len() as f64 + 2.5);
        restored.push(name.clone());
    }
    Ok(restored)
}

fn run_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let from_db = p.get("path").is_some() || p.get("project").and_then(Value::as_bool).unwrap_or(false);
    let source = if from_db {
        let tables = super::takeoffrec::load_tables(s, p)?;
        let n = apply_tables(s.doc_mut()?, &tables);
        if n == 0 {
            return Err(bad(
                "takeoff.restore",
                "the project file's take-off header has no `takeoff_tables`; re-run takeoff then takeoff.record so tables are stored",
            ));
        }
        "project"
    } else {
        "drawing"
    };
    let names: Vec<String> = match str_param(p, "name") {
        Some(n) => vec![n.to_string()],
        None => s.doc()?.project.keys().filter_map(|k| k.strip_prefix("takeoff:").map(str::to_string)).collect(),
    };
    if names.is_empty() {
        return Err(bad("takeoff.restore", "no saved take-offs; run takeoff.brick / takeoff.beams / … first"));
    }
    let restored = redraw_tables(s, p, &names)?;
    s.echo(format!("Restored {} take-off table(s) from {source}.", restored.len()));
    Ok(json!({ "restored": restored, "tables": restored.len(), "source": source }))
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    match str_param(p, "name") {
        Some(n) => {
            let v = project::get(d, &format!("takeoff:{n}"));
            if v.is_null() { Err(bad("takeoff.list", format!("no saved take-off `{n}`"))) } else { Ok(v) }
        }
        None => Ok(json!({ "takeoffs": d.project.keys().filter_map(|k| k.strip_prefix("takeoff:")).collect::<Vec<_>>() })),
    }
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// A saved take-off as CSV text.
pub(crate) fn to_csv(v: &Value) -> String {
    let mut out = String::new();
    let row = |cells: Vec<String>| cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",") + "\n";
    out += &row(vec![v["title"].as_str().unwrap_or("").to_string()]);
    out += &row(v["columns"].as_array().map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default());
    for r in v["rows"].as_array().into_iter().flatten() {
        out += &row(r.as_array().map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default());
    }
    out
}

fn run_export(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let names: Vec<String> = match str_param(p, "name") {
        Some(n) => vec![n.to_string()],
        None => d.project.keys().filter_map(|k| k.strip_prefix("takeoff:")).map(str::to_string).collect(),
    };
    if names.is_empty() {
        return Err(bad("takeoff.export", "no saved take-off: run a take-off command first"));
    }
    let docs: Vec<(String, String)> = names
        .iter()
        .map(|n| {
            let v = project::get(d, &format!("takeoff:{n}"));
            if v.is_null() { Err(bad("takeoff.export", format!("no saved take-off `{n}`"))) } else { Ok((n.clone(), to_csv(&v))) }
        })
        .collect::<Result<_>>()?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = docs;
        Err(bad("takeoff.export", "files cannot be written on the web; read the result with `takeoff.list`"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let base = s.state()?.path.clone();
        let target = match str_param(p, "path") {
            Some(t) => std::path::PathBuf::from(t),
            None => match &base {
                Some(b) => std::path::Path::new(b).with_extension(""),
                None => std::env::temp_dir().join("aad-takeoff"),
            },
        };
        let (dir, single) = if target.extension().is_some() && docs.len() == 1 {
            (target.parent().map(std::path::Path::to_path_buf).unwrap_or_default(), Some(target.clone()))
        } else {
            (target.clone(), None)
        };
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&dir).map_err(|e| bad("takeoff.export", format!("{}: {e}", dir.display())))?;
        }
        let stem = base
            .as_deref()
            .and_then(|b| std::path::Path::new(b).file_stem().map(|x| x.to_string_lossy().to_string()))
            .unwrap_or_else(|| "drawing".into());
        let mut files = Vec::new();
        for (n, csv) in docs {
            let file = single.clone().unwrap_or_else(|| dir.join(format!("{stem}-{n}.csv")));
            std::fs::write(&file, csv).map_err(|e| bad("takeoff.export", format!("{}: {e}", file.display())))?;
            files.push(file.to_string_lossy().to_string());
        }
        Ok(json!({ "files": files }))
    }
}

/// A context for drawing tables only (text height `th`, no units).
pub(crate) fn table_ctx(th: f64) -> Ctx {
    Ctx { unit: Unit::M, th, tol: 0.0 }
}

/// Draw a table with its top-left corner at `at` on `MEASURE-TABLE`.
pub(crate) fn table(s: &mut Session, c: &Ctx, at: Vec2, title: &str, header: &[String], rows: &[Vec<String>]) -> Result<()> {
    draw_table(s, c, at, title, header, rows)
}

fn add_marked(s: &mut Session, geom: EntityKind, mark: TakeoffMark) -> Result<Handle> {
    let layer = layer_for_kind(mark.kind);
    ensure_measure_layer(s.doc_mut()?, layer);
    let mut common = s.current_common();
    common.layer = layer.to_string();
    common.takeoff = Some(mark);
    let space = s.space();
    s.doc_mut()?.add(&space, common, geom).map_err(|e| bad("takeoff.draw", e.to_string()))
}

fn run_draw(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = parse_kind("takeoff.draw", p)?;
    let thickness_mm = thickness_param("takeoff.draw", p, kind)?;
    let pts = points_param(p, "points").ok_or_else(|| bad("takeoff.draw", "`points` [[x,y],...] (2 or more) is required"))?;
    if pts.len() < 2 {
        return Err(bad("takeoff.draw", "need at least 2 points"));
    }
    let closed = match p.get("closed") {
        Some(Value::Bool(b)) => *b,
        _ => kind.is_outline(),
    };
    let (phase, bill_phase) = super::takeoffphase::for_new_mark("takeoff.draw", s.doc()?, p)?;
    let mark = TakeoffMark::new(kind, thickness_mm).in_phases(phase.clone(), bill_phase.clone());
    let mut handles = Vec::new();
    if closed || kind.is_outline() {
        if pts.len() < 3 {
            return Err(bad("takeoff.draw", "a closed outline needs at least 3 points"));
        }
        let vs: Vec<aad_geom::PolyVertex> = pts.iter().copied().map(aad_geom::PolyVertex::new).collect();
        handles.push(add_marked(s, super::helpers::lwpoly(vs, true), mark)?.hex());
    } else {
        for w in pts.windows(2) {
            if let [a, b] = w {
                if a.near(*b, 1e-12) {
                    continue;
                }
                handles.push(add_marked(s, line(*a, *b), mark.clone())?.hex());
            }
        }
        if handles.is_empty() {
            return Err(bad("takeoff.draw", "need at least 2 different points"));
        }
    }
    sync_meta(s.doc_mut()?);
    Ok(json!({ "handles": handles, "kind": kind.as_str(), "thicknessMm": thickness_mm, "phase": phase, "billPhase": bill_phase }))
}

fn run_tag(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = parse_kind("takeoff.tag", p)?;
    let thickness_mm = thickness_param("takeoff.tag", p, kind)?;
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("takeoff.tag", "nothing selected"));
    }
    let (phase, bill_phase) = super::takeoffphase::for_new_mark("takeoff.tag", s.doc()?, p)?;
    let mark = TakeoffMark::new(kind, thickness_mm).in_phases(phase.clone(), bill_phase.clone());
    let mut tagged = 0;
    for h in hs {
        if s.doc_mut()?.modify_entity(h, |e| e.common.takeoff = Some(mark.clone())).is_ok() {
            tagged += 1;
        }
    }
    sync_meta(s.doc_mut()?);
    Ok(json!({ "tagged": tagged, "kind": kind.as_str(), "thicknessMm": thickness_mm, "phase": phase, "billPhase": bill_phase }))
}

fn run_maplayers(s: &mut Session, p: &Value) -> Result<Value> {
    let force = bool_or(p, "force", false);
    let (phase, bill_phase) = super::takeoffphase::for_new_mark("takeoff.maplayers", s.doc()?, p)?;
    let handles: Vec<Handle> = {
        let d = s.doc()?;
        d.model_stores().flat_map(|st| st.iter()).chain(d.layouts.iter().flat_map(|l| l.entities.iter())).map(|e| e.handle).collect()
    };
    let mut tagged = 0usize;
    let mut skipped = 0usize;
    let mut by_kind: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
    for h in handles {
        let Some(e) = s.doc()?.entity(h).cloned() else { continue };
        let Some((kind, thick)) = kind_for_layer(&e.common.layer) else { continue };
        if e.common.takeoff.is_some() && !force {
            skipped += 1;
            continue;
        }
        let mark = TakeoffMark::new(kind, thick.or_else(|| kind.default_thickness_mm())).in_phases(phase.clone(), bill_phase.clone());
        if s.doc_mut()?.modify_entity(h, |ent| ent.common.takeoff = Some(mark)).is_ok() {
            tagged += 1;
            *by_kind.entry(kind.as_str()).or_insert(0) += 1;
        }
    }
    sync_meta(s.doc_mut()?);
    Ok(json!({ "tagged": tagged, "skipped": skipped, "byKind": by_kind }))
}

fn run_column_deduct(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.space();
    let (cols, holes) = {
        let d = s.doc()?;
        (shapes(d, &space, L_COLUMN), shapes(d, &space, L_SLAB_DEDUCT))
    };
    let mut covered: Vec<Vec<Vec2>> = holes.into_iter().map(|h| h.pts).collect();
    let mut added = 0usize;
    for col in &cols {
        if col.pts.len() < 3 {
            continue;
        }
        if covered.iter().any(|pts| point_in_polygon(pts, col.centroid)) {
            continue;
        }
        let vs: Vec<aad_geom::PolyVertex> = col.pts.iter().copied().map(aad_geom::PolyVertex::new).collect();
        add_marked(s, super::helpers::lwpoly(vs, true), TakeoffMark::new(TakeoffKind::SlabDeduct, None))?;
        covered.push(col.pts.clone());
        added += 1;
    }
    sync_meta(s.doc_mut()?);
    Ok(json!({ "added": added }))
}

//! Walls, doors and windows (the kit's HCWWALL, HCWDOOR, HCWWINDOW and HCWCLEAN).
//!
//! * `wall.draw` draws the two faces of a wall (and the end caps) as lines on `A-WALL` along a
//!   centre line given by points or by selected lines and polylines, with mitred corners.
//! * `wall.door` and `wall.window` cut an opening in the wall lines at a point and place a block
//!   (`AADOPEN-…`) that holds the frame, leaf and swing or glass lines and a deduction line on
//!   `MEASURE-DEDUCT`, so take-off and the opening schedule see it. Each opening keeps its own
//!   block. Width grips and the properties panel (`openingType`, `openingWidth`, `openingHeight`,
//!   `sill`, `lintelMode`, `head`) rebuild that symbol; plan sizes are drawing units, and sill /
//!   height / head are entered in drawing units (stored as metres on the insert).
//! * `wall.clean` erases zero-length and duplicate lines and joins lines that overlap on one line.

use std::sync::Arc;

use aad_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, entity_bounds};
use aad_geom::{Line, Vec2, Vec3};
use serde_json::{Value, json};

use super::hcw::{add_on, ensure_layer};
use super::helpers::{line, v3};
use super::*;
use crate::annot::{self as ua};
use crate::{Result, Session};

pub(crate) const WALL: &str = "A-WALL";
const DOOR: &str = "A-DOOR";
const WINDOW: &str = "A-WIND";
const DEDUCT: &str = "MEASURE-DEDUCT";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("wall.draw", "Draw Wall", run_wall)
            .alias(&["wall", "hcwwall"])
            .params("{points?: [[x,y],...] | handles?: [lines, polylines], thickness? (drawing units; default ≈230 mm), closed?: bool, layer?: default A-WALL} → {lines}"),
        CommandSpec::new("wall.door", "Door", run_door)
            .menu(&["Architecture", "Walls and Openings", "Door"])
            .alias(&["door", "hcwdoor"])
            .params("{at: [x,y], block? (plan block; width from its extents), angle?: degrees (default: the nearest wall line), width? (drawing units; default ≈900 mm) | name? (schedule entry), thickness? (drawing units), swing?: left|right, side?: up|down, openingType?: single|double|sliding|folding, openingHeight? (drawing units), sill? (drawing units), lintelMode?: standard|custom, head? (drawing units, custom), layer?, deduct?: bool} → {block, cut}"),
        CommandSpec::new("wall.window", "Window", run_window)
            .menu(&["Architecture", "Walls and Openings", "Window"])
            .alias(&["window", "hcwwindow"])
            .params("{at: [x,y], block?, angle?, width? (drawing units; default ≈1200 mm) | name?, thickness? (drawing units), openingType?: fixed|casement|sliding|ventilator, openingHeight? (drawing units), sill? (drawing units), lintelMode?: standard|custom, head? (drawing units), layer?, deduct?: bool} → {block, cut}"),
        CommandSpec::new("wall.clean", "Clean Lines", run_clean)
            .menu(&["Architecture", "Walls and Openings", "Clean Lines"])
            .alias(&["hcwclean"])
            .params("{handles?, layer?, tolerance?} → {erased, joined} (zero-length and duplicate lines go; lines that overlap on one line become one)"),
    ]
}

fn per_mm(s: &Session) -> f64 {
    s.doc().ok().and_then(ua::drawing_unit).map_or(1.0, |u| 0.001 / u.metres())
}

// ---------------------------------------------------------------------------------------------
// Wall faces

fn normal(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let l = d.len();
    if l < 1e-12 { Vec2::ZERO } else { Vec2::new(-d.y / l, d.x / l) }
}

/// The offset of a centre line by `h` to the left (negative: right), with mitred corners.
pub(crate) fn offset_path(pts: &[Vec2], closed: bool, h: f64) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let prev = if i > 0 {
            Some(pts[i - 1])
        } else if closed {
            Some(pts[n - 1])
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(pts[i + 1])
        } else if closed {
            Some(pts[0])
        } else {
            None
        };
        let p = pts[i];
        let n1 = prev.map(|q| normal(q, p));
        let n2 = next.map(|q| normal(p, q));
        let off = match (n1, n2) {
            (Some(a), Some(b)) => {
                let dot = a.x * b.x + a.y * b.y;
                if 1.0 + dot < 0.1 { a * h } else { (a + b) * (h / (1.0 + dot)) }
            }
            (Some(a), None) => a * h,
            (None, Some(b)) => b * h,
            (None, None) => Vec2::ZERO,
        };
        out.push(p + off);
    }
    out
}

fn centre_paths(s: &Session, p: &Value) -> Result<Vec<(Vec<Vec2>, bool)>> {
    if let Some(pts) = points_param(p, "points") {
        if pts.len() < 2 {
            return Err(bad("wall.draw", "`points` needs two or more points"));
        }
        return Ok(vec![(pts, bool_or(p, "closed", false))]);
    }
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("wall.draw", "give `points`, or select lines and polylines"));
    }
    let d = s.doc()?;
    let mut out = Vec::new();
    for h in hs {
        let Some(e) = d.entity(h) else { continue };
        match &e.kind {
            EntityKind::Line(l) => out.push((vec![l.a.xy(), l.b.xy()], false)),
            EntityKind::LwPolyline(pl) => {
                let mut v: Vec<Vec2> = pl.vertices.iter().map(|x| x.p).collect();
                v.dedup_by(|a, b| a.near(*b, 1e-9));
                if v.len() >= 2 {
                    out.push((v, pl.closed));
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err(bad("wall.draw", "the selection has no lines or polylines"));
    }
    Ok(out)
}

fn run_wall(s: &mut Session, p: &Value) -> Result<Value> {
    let thick = f64_or(p, "thickness", 230.0 * per_mm(s));
    if !(thick.is_finite() && thick > 0.0) {
        return Err(bad("wall.draw", "`thickness` must be positive"));
    }
    let layer = str_param(p, "layer").unwrap_or(WALL).to_string();
    let paths = centre_paths(s, p)?;
    ensure_layer(s.doc_mut()?, &layer, 1, "Continuous", 35, "Walls");
    let mut made = Vec::new();
    for (pts, closed) in paths {
        let left = offset_path(&pts, closed, thick / 2.0);
        let right = offset_path(&pts, closed, -thick / 2.0);
        let n = pts.len();
        let count = if closed { n } else { n - 1 };
        for i in 0..count {
            let j = (i + 1) % n;
            for face in [&left, &right] {
                made.push(add_on(s, &layer, line(face[i], face[j]))?.hex());
            }
        }
        if !closed {
            made.push(add_on(s, &layer, line(left[0], right[0]))?.hex());
            made.push(add_on(s, &layer, line(left[n - 1], right[n - 1]))?.hex());
        }
    }
    Ok(json!({ "lines": made, "thickness": thick }))
}

// ---------------------------------------------------------------------------------------------
// Openings

struct Frame {
    origin: Vec2,
    u: Vec2,
    v: Vec2,
}

impl Frame {
    fn to_local(&self, p: Vec2) -> Vec2 {
        let d = p - self.origin;
        Vec2::new(d.x * self.u.x + d.y * self.u.y, d.x * self.v.x + d.y * self.v.y)
    }
    fn to_world(&self, q: Vec2) -> Vec2 {
        self.origin + self.u * q.x + self.v * q.y
    }
}

/// The direction of the wall line nearest to `at`, in radians.
fn nearest_wall_angle(d: &Drawing, layer: &str, at: Vec2, reach: f64) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for e in d.model.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(layer)) {
        if let EntityKind::Line(l) = &e.kind {
            let seg = Line::new(l.a.xy(), l.b.xy());
            let dist = seg.closest(at).dist(at);
            if dist <= reach && best.is_none_or(|(bd, _)| dist < bd) {
                best = Some((dist, (l.b.y - l.a.y).atan2(l.b.x - l.a.x)));
            }
        }
    }
    best.map(|(_, a)| a)
}

/// Cut `[-w/2, w/2]` out of the wall lines that run along `fr.u` through the band of half
/// thickness `reach`, returning how many lines were cut and the face offsets found.
fn cut_wall_lines(s: &mut Session, layer: &str, fr: &Frame, w: f64, reach: f64) -> Result<(usize, Vec<f64>)> {
    let hits: Vec<(Handle, Vec2, Vec2, Common)> = s
        .doc()?
        .model
        .iter()
        .filter(|e| e.common.layer.eq_ignore_ascii_case(layer))
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((e.handle, l.a.xy(), l.b.xy(), e.common.clone())),
            _ => None,
        })
        .collect();
    let mut cut = 0;
    let mut faces = Vec::new();
    for (h, a, b, common) in hits {
        let (la, lb) = (fr.to_local(a), fr.to_local(b));
        // Parallel to the wall, close to the opening's centre line, and overlapping its span.
        if (la.y - lb.y).abs() > 1e-6 * (1.0 + (la.x - lb.x).abs()) || la.y.abs() > reach + 1e-9 {
            continue;
        }
        let (lo, hi) = (la.x.min(lb.x), la.x.max(lb.x));
        let (c0, c1) = (-w / 2.0, w / 2.0);
        if hi <= c0 + 1e-9 || lo >= c1 - 1e-9 {
            continue;
        }
        faces.push(la.y);
        let y = la.y;
        s.doc_mut()?.remove_entity(h);
        let space = s.space();
        for (x0, x1) in [(lo, lo.max(c0).min(hi)), (hi.min(c1).max(lo), hi)] {
            if x1 - x0 > 1e-9 && ((x1 <= c0 + 1e-9) || (x0 >= c1 - 1e-9)) {
                let (p0, p1) = (fr.to_world(Vec2::new(x0, y)), fr.to_world(Vec2::new(x1, y)));
                s.doc_mut()?.add(&space, common.clone(), line(p0, p1))?;
            }
        }
        cut += 1;
    }
    Ok((cut, faces))
}

fn push(d: &mut Drawing, b: &mut Block, layer: &str, kind: EntityKind) {
    d.ensure_layer(layer);
    let h = d.new_handle();
    b.entities.push(Entity { handle: h, common: Common { layer: layer.into(), ..Common::default() }, kind });
}

const DOOR_TYPES: &[&str] = &["single", "double", "sliding", "folding"];
const WINDOW_TYPES: &[&str] = &["fixed", "casement", "sliding", "ventilator"];

fn type_list(door: bool) -> &'static [&'static str] {
    if door { DOOR_TYPES } else { WINDOW_TYPES }
}

fn canonical_type(door: bool, raw: &str) -> Option<&'static str> {
    type_list(door).iter().copied().find(|t| t.eq_ignore_ascii_case(raw.trim()))
}

fn lintel_mode_name(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "standard" | "standard lintel" => Some("standard"),
        "custom" | "custom height" => Some("custom"),
        _ => None,
    }
}

fn plain(v: f64) -> String {
    let mut s = format!("{v:.6}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s.is_empty() || s == "-" { "0".into() } else { s }
}

fn num_flex(p: &Value, key: &str) -> Option<f64> {
    let v = p.get(key)?;
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).filter(|n| n.is_finite())
}

fn parse_attr(i: &Insert, tag: &str) -> Option<f64> {
    i.attrib_value(tag).and_then(|v| v.parse().ok()).filter(|n: &f64| n.is_finite())
}

struct Opening {
    door: bool,
    otype: String,
    width: f64,
    thick: f64,
    height: f64,
    sill: f64,
    mode: String,
    head: f64,
    deduct: bool,
}

fn read_opening(i: &Insert) -> Result<Opening> {
    let door = i.attrib_value("OKIND") == Some("door");
    let fallback = if door { "single" } else { "fixed" };
    let otype = canonical_type(door, i.attrib_value("OTYPE").unwrap_or(fallback)).unwrap_or(fallback).to_string();
    let width = i.opening_width().ok_or_else(|| bad("properties.set", "the opening has no width"))?;
    let thick = parse_attr(i, "OTHICK").filter(|t| *t > 0.0).unwrap_or(width * 0.2);
    let sill = parse_attr(i, "OSILL").filter(|s| *s >= 0.0).unwrap_or(0.0);
    let height = parse_attr(i, "OHEIGHT").filter(|h| *h > 0.0).unwrap_or(2.1);
    let mode = if i.attrib_value("OLINTEL").is_some_and(|m| m.eq_ignore_ascii_case("custom")) { "custom" } else { "standard" };
    let head = parse_attr(i, "OHEAD").unwrap_or(sill + height);
    let deduct = i.attrib_value("ODEDUCT") != Some("0");
    Ok(Opening { door, otype, width, thick, height, sill, mode: mode.into(), head, deduct })
}

fn store_params(i: &mut Insert, op: &Opening) {
    i.set_attrib("OKIND", if op.door { "door" } else { "window" });
    i.set_attrib("OTYPE", &op.otype);
    i.set_attrib("OWIDTH", &plain(op.width));
    i.set_attrib("OTHICK", &plain(op.thick));
    i.set_attrib("OHEIGHT", &plain(op.height));
    i.set_attrib("OSILL", &plain(op.sill));
    i.set_attrib("OLINTEL", &op.mode);
    i.set_attrib("OHEAD", &plain(op.head));
    i.set_attrib("ODEDUCT", if op.deduct { "1" } else { "0" });
}

/// Metres per drawing unit (imperial inch default when unset).
fn metres_per_du(s: &Session) -> f64 {
    s.doc().ok().and_then(ua::drawing_unit).map_or(0.0254, |u| u.metres())
}

/// Apply a properties edit. Under a standard lintel, height and sill share the floor head.
/// `to_m` converts drawing-unit sill / height / head into metres for storage.
fn apply_opening(op: &mut Opening, p: &Value, standard: f64, cmd: &str, to_m: f64) -> Result<()> {
    if let Some(t) = str_param(p, "openingType") {
        let kind = if op.door { "door" } else { "window" };
        op.otype = canonical_type(op.door, t)
            .ok_or_else(|| bad(cmd, format!("type `{t}` is not a {kind} type ({})", type_list(op.door).join(", "))))?
            .to_string();
    }
    if p.get("openingWidth").is_some() {
        let w = num_flex(p, "openingWidth").ok_or_else(|| bad(cmd, "width must be a number"))?;
        if !(w.is_finite() && w > 0.0) {
            return Err(bad(cmd, "width must be positive"));
        }
        op.width = w;
    }
    let set_height = p.get("openingHeight").is_some();
    let set_sill = p.get("sill").is_some();
    let set_head = p.get("head").is_some();
    if set_sill {
        let sill = num_flex(p, "sill").ok_or_else(|| bad(cmd, "sill must be a number (drawing units)"))?;
        if !(sill.is_finite() && sill >= 0.0) {
            return Err(bad(cmd, "sill must be zero or positive"));
        }
        op.sill = sill * to_m;
    }
    if set_height {
        let h = num_flex(p, "openingHeight").ok_or_else(|| bad(cmd, "opening height must be a number (drawing units)"))?;
        if !(h.is_finite() && h > 0.0) {
            return Err(bad(cmd, "opening height must be positive"));
        }
        op.height = h * to_m;
    }
    if set_head {
        let h = num_flex(p, "head").ok_or_else(|| bad(cmd, "head must be a number (drawing units)"))?;
        op.head = h * to_m;
    }
    if let Some(mode) = str_param(p, "lintelMode") {
        op.mode = lintel_mode_name(mode).ok_or_else(|| bad(cmd, "lintel must be standard or custom"))?.to_string();
    }
    if op.mode == "standard" {
        if set_height && set_sill && (op.sill + op.height - standard).abs() > 1e-3 {
            return Err(bad(cmd, "with a standard lintel, sill plus opening height must meet the lintel"));
        } else if set_height && !set_sill {
            op.sill = standard - op.height;
        } else {
            op.height = standard - op.sill;
        }
        if op.sill < -1e-6 || op.height <= 1e-6 {
            return Err(bad(cmd, "under the standard lintel, the sill and the opening height have to fit below it"));
        }
        op.sill = op.sill.max(0.0);
        op.head = standard;
        op.height = standard - op.sill;
    } else {
        if !set_head && op.head + 1e-6 < op.sill + op.height {
            op.head = op.sill + op.height;
        }
        if !(op.head.is_finite() && op.head > op.sill) {
            return Err(bad(cmd, "custom head must be above the sill"));
        }
        if op.sill + op.height > op.head + 1e-3 {
            return Err(bad(cmd, "sill plus opening height rises above the custom head"));
        }
    }
    Ok(())
}

fn arc_at(center: Vec2, radius: f64, start: f64, end: f64) -> EntityKind {
    EntityKind::Arc(aad_doc::Arc { center: Vec3::new(center.x, center.y, 0.0), radius, start, end })
}

/// Replace the opening's block with plan geometry for its type and width.
fn write_symbol(d: &mut Drawing, name: &str, door: bool, otype: &str, width: f64, thick: f64, deduct: bool) {
    let mut b = Block::new(name);
    let (hw, ht) = (width / 2.0, thick / 2.0);
    let layer = if door { DOOR } else { WINDOW };
    push(d, &mut b, layer, line(Vec2::new(-hw, -ht), Vec2::new(-hw, ht)));
    push(d, &mut b, layer, line(Vec2::new(hw, -ht), Vec2::new(hw, ht)));
    let quarter = std::f64::consts::FRAC_PI_2;
    match (door, otype) {
        (true, "double") => {
            let leaf = hw.max(1e-6);
            push(d, &mut b, DOOR, line(Vec2::new(-hw, ht), Vec2::new(-hw, ht + leaf)));
            push(d, &mut b, DOOR, arc_at(Vec2::new(-hw, ht), leaf, 0.0, quarter));
            push(d, &mut b, DOOR, line(Vec2::new(hw, ht), Vec2::new(hw, ht + leaf)));
            push(d, &mut b, DOOR, arc_at(Vec2::new(hw, ht), leaf, quarter, std::f64::consts::PI));
        }
        (true, "sliding") => {
            let y1 = ht * 0.35;
            let y2 = -ht * 0.35;
            push(d, &mut b, DOOR, line(Vec2::new(-hw, y1), Vec2::new(hw * 0.15, y1)));
            push(d, &mut b, DOOR, line(Vec2::new(-hw * 0.15, y2), Vec2::new(hw, y2)));
        }
        (true, "folding") => {
            let out = width * 0.35;
            push(d, &mut b, DOOR, line(Vec2::new(-hw, ht), Vec2::new(-hw * 0.45, ht + out)));
            push(d, &mut b, DOOR, line(Vec2::new(-hw * 0.45, ht + out), Vec2::new(0.0, ht)));
            push(d, &mut b, DOOR, line(Vec2::new(hw, ht), Vec2::new(hw * 0.45, ht + out)));
            push(d, &mut b, DOOR, line(Vec2::new(hw * 0.45, ht + out), Vec2::new(0.0, ht)));
        }
        (true, _) => {
            push(d, &mut b, DOOR, line(Vec2::new(-hw, ht), Vec2::new(-hw, ht + width)));
            push(d, &mut b, DOOR, arc_at(Vec2::new(-hw, ht), width, 0.0, quarter));
        }
        (false, "casement") => {
            push(d, &mut b, WINDOW, line(Vec2::new(-hw, -ht), Vec2::new(hw, -ht)));
            push(d, &mut b, WINDOW, line(Vec2::new(-hw, ht), Vec2::new(hw, ht)));
            push(d, &mut b, WINDOW, line(Vec2::new(-hw, ht), Vec2::new(-hw, ht + width)));
            push(d, &mut b, WINDOW, arc_at(Vec2::new(-hw, ht), width, 0.0, quarter));
        }
        (false, "sliding") => {
            push(d, &mut b, WINDOW, line(Vec2::new(-hw, -ht), Vec2::new(hw, -ht)));
            push(d, &mut b, WINDOW, line(Vec2::new(-hw, ht), Vec2::new(hw, ht)));
            push(d, &mut b, WINDOW, line(Vec2::new(0.0, -ht), Vec2::new(0.0, ht)));
            push(d, &mut b, WINDOW, line(Vec2::new(-hw, 0.0), Vec2::new(0.0, 0.0)));
            push(d, &mut b, WINDOW, line(Vec2::new(0.0, ht * 0.35), Vec2::new(hw, ht * 0.35)));
        }
        (false, "ventilator") => {
            for v in [-ht, 0.0, ht] {
                push(d, &mut b, WINDOW, line(Vec2::new(-hw, v), Vec2::new(hw, v)));
            }
            let r = (width * 0.45).max(ht);
            push(d, &mut b, WINDOW, arc_at(Vec2::new(-hw, ht), r, 0.0, quarter));
        }
        (false, _) => {
            for v in [-ht, -ht / 3.0, ht / 3.0, ht] {
                push(d, &mut b, WINDOW, line(Vec2::new(-hw, v), Vec2::new(hw, v)));
            }
        }
    }
    if deduct {
        d.ensure_layer(DEDUCT);
        push(d, &mut b, DEDUCT, line(Vec2::new(-hw, 0.0), Vec2::new(hw, 0.0)));
    }
    d.blocks.insert(name.to_string(), Arc::new(b));
}

fn block_refs(d: &Drawing, name: &str) -> usize {
    let mut n = 0usize;
    let mut scan = |store: &aad_doc::EntityStore| {
        for e in store.iter() {
            if let EntityKind::Insert(i) = &e.kind
                && i.block.eq_ignore_ascii_case(name)
            {
                n += 1;
            }
        }
    };
    scan(&d.model);
    for lay in &d.layouts {
        scan(&lay.entities);
    }
    n
}

fn exclusive_name(s: &mut Session, h: Handle, current: &str) -> Result<String> {
    let refs = block_refs(s.doc()?, current);
    if current.starts_with("AADOPEN-") && refs <= 1 {
        return Ok(current.to_string());
    }
    let name = format!("AADOPEN-{}", h.hex());
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block.clone_from(&name);
        }
    })?;
    Ok(name)
}

fn opening_corners(ins: &Insert, width: f64, thick: f64) -> [Vec2; 4] {
    let hw = width / 2.0;
    let ht = thick / 2.0;
    let m = ins.transform(Vec2::ZERO);
    [Vec2::new(-hw, -ht), Vec2::new(-hw, ht), Vec2::new(hw, -ht), Vec2::new(hw, ht)].map(|p| m.apply(p))
}

fn nearest_corner(pts: &[Vec2; 4], p: Vec2, tol: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, q) in pts.iter().enumerate() {
        let dist = q.dist(p);
        if dist <= tol && best.is_none_or(|(_, bd)| dist < bd) {
            best = Some((i, dist));
        }
    }
    best.map(|(i, _)| i)
}

/// Move wall-line ends that sit on the old jambs out to the new width.
fn shift_jambs(s: &mut Session, ins: &Insert, old_w: f64, new_w: f64, thick: f64) -> Result<()> {
    if (old_w - new_w).abs() < 1e-6 {
        return Ok(());
    }
    let old_pts = opening_corners(ins, old_w, thick);
    let new_pts = opening_corners(ins, new_w, thick);
    let tol = (thick * 1e-3).max(1e-4);
    let mut edits: Vec<(Handle, Vec2, Vec2)> = Vec::new();
    let consider = |edits: &mut Vec<(Handle, Vec2, Vec2)>, h: Handle, a: Vec2, b: Vec2| {
        let mut a2 = a;
        let mut b2 = b;
        let mut moved = false;
        if let Some(i) = nearest_corner(&old_pts, a, tol) {
            a2 = new_pts[i];
            moved = true;
        }
        if let Some(i) = nearest_corner(&old_pts, b, tol) {
            b2 = new_pts[i];
            moved = true;
        }
        if moved {
            edits.push((h, a2, b2));
        }
    };
    {
        let d = s.doc()?;
        for e in d.model.iter() {
            if let EntityKind::Line(l) = &e.kind {
                consider(&mut edits, e.handle, l.a.xy(), l.b.xy());
            }
        }
        for lay in &d.layouts {
            for e in lay.entities.iter() {
                if let EntityKind::Line(l) = &e.kind {
                    consider(&mut edits, e.handle, l.a.xy(), l.b.xy());
                }
            }
        }
    }
    let d = s.doc_mut()?;
    for (h, a, b) in edits {
        d.modify_entity(h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.a = v3(a);
                l.b = v3(b);
            }
        })?;
    }
    Ok(())
}

fn is_generated_opening(name: &str) -> bool {
    name.to_ascii_uppercase().starts_with("AADOPEN-")
}

/// Plan width of a named block from its extents (X span in block coordinates).
fn block_plan_width(d: &Drawing, name: &str) -> Option<f64> {
    let b = d.block(name)?;
    let mut bb = aad_geom::Bounds2::EMPTY;
    for e in b.entities.iter() {
        bb = bb.union(&entity_bounds(d, e, 0));
    }
    if bb.is_empty() {
        return None;
    }
    let w = bb.max.x - bb.min.x;
    (w.is_finite() && w > 1e-9).then_some(w)
}

/// Push the opening's plan width (metres) onto a schedule mark, and link the block name.
fn apply_width_to_schedule(s: &mut Session, mark: &str, width_m: f64, block: &str) -> Result<()> {
    if mark.is_empty() || !(width_m.is_finite() && width_m > 0.0) {
        return Ok(());
    }
    let mut sc = project::get(s.doc()?, "schedule");
    let Some(list) = sc["openings"].as_array_mut() else { return Ok(()) };
    let Some(o) = list.iter_mut().find(|o| o["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(mark))) else {
        return Ok(());
    };
    o["length"] = json!((width_m * 1000.0).round() / 1000.0);
    if !block.is_empty() {
        let cur = o["block"].as_str().unwrap_or("").to_string();
        if !cur.split(';').any(|b| b.trim().eq_ignore_ascii_case(block)) {
            o["block"] = json!(if cur.is_empty() { block.to_string() } else { format!("{cur};{block}") });
        }
    }
    project::set(s.doc_mut()?, "schedule", &sc);
    Ok(())
}

fn schedule_mark_for_block(d: &Drawing, block: &str) -> Option<String> {
    let sched = project::get(d, "schedule");
    sched["openings"].as_array()?.iter().find_map(|o| {
        let linked = o["block"].as_str().unwrap_or("");
        if linked.split(';').any(|b| b.trim().eq_ignore_ascii_case(block)) { o["name"].as_str().map(str::to_string) } else { None }
    })
}

fn commit_opening(s: &mut Session, h: Handle, block: &str, old_w: f64, op: &Opening) -> Result<()> {
    let insert = {
        let e = s.doc()?.entity(h).ok_or_else(|| bad("properties.set", "no such opening"))?;
        let EntityKind::Insert(i) = &e.kind else { return Err(bad("properties.set", "no such opening")) };
        i.clone()
    };
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            store_params(i, op);
            // User blocks keep their geometry; stretch X scale when the opening width changes.
            if !is_generated_opening(&i.block) && old_w > 1e-9 {
                let sx = (i.scale.x.abs() * (op.width / old_w)).copysign(i.scale.x);
                i.scale.x = sx;
            }
        }
    })?;
    let name = if is_generated_opening(block) {
        let name = exclusive_name(s, h, block)?;
        write_symbol(s.doc_mut()?, &name, op.door, &op.otype, op.width, op.thick, op.deduct);
        name
    } else {
        block.to_string()
    };
    shift_jambs(s, &insert, old_w, op.width, op.thick)?;
    let to_m = metres_per_du(s);
    if let Some(mark) = schedule_mark_for_block(s.doc()?, &name) {
        apply_width_to_schedule(s, &mark, op.width * to_m, &name)?;
    }
    Ok(())
}

/// Rebuild an opening after a width grip. `old_width` is the plan width before the grip moved.
pub(crate) fn refit_opening(s: &mut Session, h: Handle, old_width: f64) -> Result<()> {
    let (block, op) = {
        let e = s.doc()?.entity(h).ok_or_else(|| bad("grip.stretch", "no such opening"))?;
        let EntityKind::Insert(i) = &e.kind else { return Ok(()) };
        if !i.is_opening() {
            return Ok(());
        }
        (i.block.clone(), read_opening(i)?)
    };
    if (op.width - old_width).abs() < 1e-6 {
        return Ok(());
    }
    commit_opening(s, h, &block, old_width, &op)
}

/// Properties-panel edit of one or more dynamic doors or windows.
pub(crate) fn edit_openings(s: &mut Session, hs: &[Handle], p: &Value) -> Result<Value> {
    if hs.is_empty() {
        return Err(bad("properties.set", "nothing selected"));
    }
    let standard = super::project::standard_lintel(s.doc()?);
    let to_m = metres_per_du(s);
    let mut plan = Vec::new();
    for h in hs {
        let e = s.doc()?.entity(*h).ok_or_else(|| bad("properties.set", "no such object"))?;
        let EntityKind::Insert(i) = &e.kind else {
            return Err(bad("properties.set", "opening properties apply to a door or a window"));
        };
        if !i.is_opening() {
            return Err(bad("properties.set", "opening properties apply to a door or a window"));
        }
        let mut op = read_opening(i)?;
        let old_w = op.width;
        let block = i.block.clone();
        apply_opening(&mut op, p, standard, "properties.set", to_m)?;
        plan.push((*h, block, old_w, op));
    }
    for (h, block, old_w, op) in &plan {
        commit_opening(s, *h, block, *old_w, op)?;
    }
    Ok(json!({ "changed": plan.len() }))
}

pub(crate) fn opening_width_of(kind: &EntityKind) -> Option<f64> {
    match kind {
        EntityKind::Insert(i) => i.opening_width(),
        _ => None,
    }
}

fn opening(s: &mut Session, p: &Value, cmd: &str, door: bool) -> Result<Value> {
    // Two picks, one on each side of the opening (`p1`, `p2`), give its width and place it; `at` gives only the centre.
    let jambs = match (point_param(p, "p1"), point_param(p, "p2")) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() => Some((a, b)),
        _ => None,
    };
    let at = match jambs {
        Some((a, b)) => (a + b) * 0.5,
        None => point_req(cmd, p, "at")?,
    };
    let mm = per_mm(s);
    let layer = str_param(p, "layer").unwrap_or(WALL).to_string();
    let user_block = str_param(p, "block").map(str::to_string).filter(|n| !n.is_empty());
    if let Some(name) = &user_block {
        let key = s.doc()?.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
        if key.is_none() {
            return Err(bad(cmd, format!("no block `{name}` — pick one in the Blocks panel")));
        }
    }
    let resolved_block = user_block.as_ref().and_then(|n| s.doc().ok()?.blocks.keys().find(|k| k.eq_ignore_ascii_case(n)).cloned());
    // Width: explicit override, else the grabbed block's plan width, else schedule mark, else default.
    let sched = project::get(s.doc()?, "schedule");
    let entry = str_param(p, "name")
        .and_then(|n| sched["openings"].as_array()?.iter().find(|o| o["name"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(n))))
        .cloned();
    if str_param(p, "name").is_some() && entry.is_none() {
        return Err(bad(cmd, "no such entry in the opening schedule"));
    }
    let to_m = metres_per_du(s);
    let to_du = |m: f64| m / to_m;
    let block_w = resolved_block.as_ref().and_then(|n| block_plan_width(s.doc().ok()?, n));
    let width = match (p.get("width").and_then(Value::as_f64), block_w, &entry) {
        // The two picks decide the width; it is projected on the wall below.
        _ if jambs.is_some() => jambs.map_or(0.0, |(a, b)| a.dist(b)),
        (Some(w), _, _) => w,
        (None, Some(w), _) => w,
        (None, None, Some(e)) => to_du(e["length"].as_f64().unwrap_or(0.9)),
        (None, None, None) => (if door { 900.0 } else { 1200.0 }) * mm,
    };
    if !(width.is_finite() && width > 0.0) {
        return Err(bad(cmd, "`width` must be positive"));
    }
    let thick_param = p.get("thickness").and_then(Value::as_f64);
    let angle = match p.get("angle").and_then(Value::as_f64) {
        Some(a) => a.to_radians(),
        None => match nearest_wall_angle(s.doc()?, &layer, at, 400.0 * mm) {
            Some(a) => a,
            // No wall near the picks: the line through them is the direction of the opening.
            None => match jambs {
                Some((a, b)) if a.dist(b) > 1e-9 => (b - a).y.atan2((b - a).x),
                _ => return Err(bad(cmd, "no wall line near `at`: give `angle`")),
            },
        },
    };
    let u = Vec2::new(angle.cos(), angle.sin());
    // The width is the distance between the picks along the wall, however the picks sit across it.
    let width = match jambs {
        Some((a, b)) => (b - a).dot(u).abs(),
        None => width,
    };
    if !(width.is_finite() && width > 0.0) {
        return Err(bad(cmd, "the two picks are on the same spot along the wall: pick the two sides of the opening"));
    }
    let mut fr = Frame { origin: at, u, v: Vec2::new(-u.y, u.x) };
    let reach = thick_param.map_or(400.0 * mm, |t| t / 2.0 + 1e-6);
    let (cut, faces) = cut_wall_lines(s, &layer, &fr, width, reach)?;
    let (face_lo, face_hi) = faces.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), f| (l.min(*f), h.max(*f)));
    let thick = thick_param.unwrap_or(if face_hi > face_lo { face_hi - face_lo } else { 230.0 * mm });
    // Picks made along one face of the wall: the opening still sits on the wall's centre line.
    let at = if jambs.is_some() && face_hi > face_lo {
        fr.origin = at + fr.v * ((face_lo + face_hi) * 0.5);
        fr.origin
    } else {
        at
    };
    let standard = super::project::standard_lintel(s.doc()?);
    let mut op = Opening {
        door,
        otype: (if door { "single" } else { "casement" }).into(),
        width,
        thick,
        height: if door { standard } else { 1.2 },
        sill: if door { 0.0 } else { 0.9 },
        mode: "standard".into(),
        head: standard,
        deduct: bool_or(p, "deduct", true),
    };
    if let Some(e) = &entry {
        if !door && e["kind"].as_str().is_some_and(|k| k.eq_ignore_ascii_case("ventilator")) {
            op.otype = "ventilator".into();
        }
        if let Some(t) = e["type"].as_str().and_then(|t| canonical_type(door, t)) {
            op.otype = t.into();
        }
        if let Some(sl) = e["sill"].as_f64().filter(|n| n.is_finite() && *n >= 0.0) {
            op.sill = sl;
        }
        if let Some(hgt) = e["height"].as_f64().filter(|n| n.is_finite() && *n > 0.0) {
            op.height = hgt;
        }
        if let Some(l) = e["lintel"].as_f64().filter(|n| n.is_finite() && (*n - standard).abs() > 1e-3) {
            op.mode = "custom".into();
            op.head = l;
        }
    }
    apply_opening(&mut op, p, standard, cmd, to_m)?;
    let block = if let Some(name) = resolved_block {
        name
    } else {
        let token = s.doc_mut()?.new_handle();
        let block = format!("AADOPEN-{}", token.hex());
        write_symbol(s.doc_mut()?, &block, op.door, &op.otype, op.width, op.thick, op.deduct);
        block
    };
    // Insert: mirrored for a right-hand swing, turned half a turn to swing to the other side.
    let mut rot = angle;
    if str_param(p, "side") == Some("down") {
        rot += std::f64::consts::PI;
    }
    let sx = if door && str_param(p, "swing") == Some("right") { -1.0 } else { 1.0 };
    let mut placed = Insert { block: block.clone(), insert: v3(at), scale: Vec3::new(sx, 1.0, 1.0), rotation: rot, ..Insert::default() };
    store_params(&mut placed, &op);
    let ins = EntityKind::Insert(placed);
    let space = s.space();
    let mut common = s.current_common();
    common.layer = (if door { DOOR } else { WINDOW }).to_string();
    let h = s.doc_mut()?.add(&space, common, ins)?;
    // Schedule width follows the opening on the drawing.
    if let Some(e) = &entry {
        let name = e["name"].as_str().unwrap_or("");
        apply_width_to_schedule(s, name, op.width * to_m, &block)?;
    }
    Ok(json!({
        "block": block,
        "handle": h.hex(),
        "cut": cut,
        "thickness": op.thick,
        "width": op.width,
        "openingType": op.otype,
        "sill": to_du(op.sill),
        "openingHeight": to_du(op.height),
        "lintelMode": op.mode,
        "head": to_du(op.head),
    }))
}

fn run_door(s: &mut Session, p: &Value) -> Result<Value> {
    opening(s, p, "wall.door", true)
}

fn run_window(s: &mut Session, p: &Value) -> Result<Value> {
    opening(s, p, "wall.window", false)
}

// ---------------------------------------------------------------------------------------------
// Clean

fn run_clean(s: &mut Session, p: &Value) -> Result<Value> {
    let tol = f64_or(p, "tolerance", 1e-6 * per_mm(s).max(1e-9)).max(1e-12);
    let layer = str_param(p, "layer").map(str::to_string);
    let only: Option<Vec<Handle>> = if p.get("handles").is_some() { Some(targets(s, p)?) } else { None };
    let lines: Vec<(Handle, Vec2, Vec2, String)> = s
        .doc()?
        .model
        .iter()
        .filter(|e| layer.as_deref().is_none_or(|l| e.common.layer.eq_ignore_ascii_case(l)) && only.as_ref().is_none_or(|v| v.contains(&e.handle)))
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((e.handle, l.a.xy(), l.b.xy(), e.common.layer.clone())),
            _ => None,
        })
        .collect();
    let mut erased: Vec<Handle> = Vec::new();
    let mut alive: Vec<(Handle, Vec2, Vec2, String)> = Vec::new();
    for (h, a, b, l) in lines {
        if a.dist(b) <= tol {
            erased.push(h);
            continue;
        }
        let dup = alive.iter().any(|(_, c, d, ly)| *ly == l && ((c.near(a, tol) && d.near(b, tol)) || (c.near(b, tol) && d.near(a, tol))));
        if dup {
            erased.push(h);
        } else {
            alive.push((h, a, b, l));
        }
    }
    // Join lines that overlap or touch on one straight line (same layer).
    let mut joined = 0;
    let mut i = 0;
    while i < alive.len() {
        let mut merged = false;
        let mut j = i + 1;
        while j < alive.len() {
            let (_, a, b, l) = alive[i].clone();
            let (_, c, d, l2) = alive[j].clone();
            let dir = (b - a).normalized();
            let col = |p: Vec2| (p - a).cross(dir).abs() <= tol * 10.0;
            if l == l2 && col(c) && col(d) {
                let t = |p: Vec2| (p - a).dot(dir);
                let (lo1, hi1) = (t(a).min(t(b)), t(a).max(t(b)));
                let (lo2, hi2) = (t(c).min(t(d)), t(c).max(t(d)));
                if lo2 <= hi1 + tol && lo1 <= hi2 + tol {
                    let (lo, hi) = (lo1.min(lo2), hi1.max(hi2));
                    let (na, nb) = (a + dir * lo, a + dir * hi);
                    erased.push(alive[j].0);
                    alive[i].1 = na;
                    alive[i].2 = nb;
                    alive.remove(j);
                    joined += 1;
                    merged = true;
                    continue;
                }
            }
            j += 1;
        }
        if merged {
            continue;
        }
        i += 1;
    }
    let d = s.doc_mut()?;
    for h in &erased {
        d.remove_entity(*h);
    }
    for (h, a, b, _) in &alive {
        d.modify_entity(*h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.a = v3(*a);
                l.b = v3(*b);
            }
        })?;
    }
    Ok(json!({ "erased": erased.len(), "joined": joined }))
}

#[cfg(test)]
mod tests {
    use aad_doc::{EntityKind, Handle};
    use serde_json::json;

    use crate::Session;

    fn place_wall(s: &mut Session) {
        s.execute("drawing.units", &json!({ "units": "mm", "mode": "metadata" })).unwrap();
        s.execute("wall.draw", &json!({ "points": [[0.0, 0.0], [4000.0, 0.0]], "thickness": 200.0 })).unwrap();
    }

    #[test]
    fn door_jamb_grip_widens_the_symbol_and_the_wall() {
        let mut s = Session::new();
        place_wall(&mut s);
        let placed = s
            .execute(
                "wall.door",
                &json!({
                    "at": [2000.0, 0.0], "angle": 0.0, "width": 900.0, "thickness": 200.0,
                    "openingType": "single", "sill": 0.0, "openingHeight": 2100.0, "lintelMode": "standard"
                }),
            )
            .unwrap();
        let hex = placed["handle"].as_str().unwrap();
        let h = Handle::parse_hex(hex).unwrap();
        s.execute("grip.stretch", &json!({ "handle": hex, "index": 2, "to": [2600.0, 0.0] })).unwrap();
        let doc = s.doc().unwrap();
        let kind = &doc.entity(h).unwrap().kind;
        let EntityKind::Insert(i) = kind else {
            panic!("door is an insert");
        };
        let w: f64 = i.attrib_value("OWIDTH").unwrap().parse().unwrap();
        assert!((w - 1200.0).abs() < 1e-3, "{w}");
        assert!((i.attrib_value("OHEIGHT").unwrap().parse::<f64>().unwrap() - 2.1).abs() < 1e-6);
        let leaf = doc
            .block(&i.block)
            .unwrap()
            .entities
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some(l.a.xy().dist(l.b.xy())),
                _ => None,
            })
            .fold(0.0_f64, f64::max);
        assert!((leaf - 1200.0).abs() < 1e-3, "{leaf}");
        let xs: Vec<f64> = doc
            .model
            .iter()
            .filter(|e| e.common.layer.eq_ignore_ascii_case("A-WALL"))
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some([l.a.x, l.b.x]),
                _ => None,
            })
            .flatten()
            .collect();
        assert!(xs.iter().any(|x| (*x - 1400.0).abs() < 1e-2), "{xs:?}");
        assert!(xs.iter().any(|x| (*x - 2600.0).abs() < 1e-2), "{xs:?}");
    }

    #[test]
    fn window_properties_edit_type_sill_and_lintel_lock() {
        let mut s = Session::new();
        place_wall(&mut s);
        let placed = s.execute("wall.window", &json!({ "at": [2000.0, 0.0], "angle": 0.0, "width": 1200.0, "thickness": 200.0 })).unwrap();
        assert_eq!(placed["openingType"], "casement");
        assert_eq!(placed["lintelMode"], "standard");
        assert!((placed["sill"].as_f64().unwrap() - 900.0).abs() < 1e-6);
        assert!((placed["openingHeight"].as_f64().unwrap() - 1200.0).abs() < 1e-6);
        let hex = placed["handle"].as_str().unwrap();
        let h = Handle::parse_hex(hex).unwrap();
        s.execute(
            "properties.set",
            &json!({ "handles": [hex], "openingType": "sliding", "lintelMode": "custom", "head": 2400.0, "sill": 800.0, "openingHeight": 1400.0 }),
        )
        .unwrap();
        let doc = s.doc().unwrap();
        let EntityKind::Insert(i) = &doc.entity(h).unwrap().kind else {
            panic!("window is an insert");
        };
        assert_eq!(i.attrib_value("OTYPE"), Some("sliding"));
        assert_eq!(i.attrib_value("OLINTEL"), Some("custom"));
        assert!((i.attrib_value("OSILL").unwrap().parse::<f64>().unwrap() - 0.8).abs() < 1e-6);
        assert!((i.attrib_value("OHEIGHT").unwrap().parse::<f64>().unwrap() - 1.4).abs() < 1e-6);
        assert!((i.attrib_value("OHEAD").unwrap().parse::<f64>().unwrap() - 2.4).abs() < 1e-6);
        let arcs = doc.block(&i.block).unwrap().entities.iter().filter(|e| matches!(e.kind, EntityKind::Arc(_))).count();
        assert_eq!(arcs, 0, "a sliding window has no swing arc");
        let err = s.execute("properties.set", &json!({ "handles": [hex], "lintelMode": "standard", "sill": 0.0, "openingHeight": 3000.0 }));
        assert!(err.is_err(), "a 3 m opening does not fit a 2.1 m lintel");
        s.execute("properties.set", &json!({ "handles": [hex], "lintelMode": "standard", "sill": 300.0 })).unwrap();
        let doc = s.doc().unwrap();
        let EntityKind::Insert(i) = &doc.entity(h).unwrap().kind else { return };
        assert_eq!(i.attrib_value("OLINTEL"), Some("standard"));
        assert!((i.attrib_value("OHEIGHT").unwrap().parse::<f64>().unwrap() - 1.8).abs() < 1e-6);
        assert!((i.attrib_value("OHEAD").unwrap().parse::<f64>().unwrap() - 2.1).abs() < 1e-6);
    }

    fn wall_session() -> Session {
        let mut s = Session::new();
        s.execute("drawing.units", &json!({"units": "mm", "mode": "metadata"})).unwrap();
        s.execute("wall.draw", &json!({ "points": [[0.0, 0.0], [4000.0, 0.0]], "thickness": 200.0 })).unwrap();
        s
    }

    fn insert_of(s: &Session, hex: &str) -> (aad_geom::Vec2, f64, f64) {
        let h = Handle::parse_hex(hex).unwrap();
        let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("an opening is an insert") };
        (i.insert.xy(), i.rotation, i.attrib_value("OWIDTH").unwrap().parse().unwrap())
    }

    #[test]
    fn two_picks_give_the_width_and_the_place() {
        // Picks on the near face of a 200 mm wall: the width is the distance along the wall and the door sits on the centre line.
        let mut s = wall_session();
        let r = s.execute("wall.door", &json!({ "p1": [1000.0, 100.0], "p2": [1900.0, 100.0] })).unwrap();
        let (at, rot, w) = insert_of(&s, r["handle"].as_str().unwrap());
        assert!((w - 900.0).abs() < 1e-3, "{w}");
        assert!((at.x - 1450.0).abs() < 1e-3 && at.y.abs() < 1e-3, "{at:?}");
        assert!(rot.abs() < 1e-6, "{rot}");
        // Picks made in the other order, one on each face: the width is still along the wall.
        let r = s.execute("wall.window", &json!({ "p1": [3100.0, -100.0], "p2": [2300.0, 100.0] })).unwrap();
        let (at, _, w) = insert_of(&s, r["handle"].as_str().unwrap());
        assert!((w - 800.0).abs() < 1e-3, "{w}");
        assert!((at.x - 2700.0).abs() < 1e-3 && at.y.abs() < 1e-3, "{at:?}");
    }

    #[test]
    fn two_picks_on_the_same_spot_are_refused() {
        let mut s = wall_session();
        assert!(s.execute("wall.door", &json!({ "p1": [1000.0, 100.0], "p2": [1000.0, -100.0] })).is_err());
    }
}

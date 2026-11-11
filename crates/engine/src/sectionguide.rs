//! Projection guides for sections and elevations (design: `docs/levels-and-sheet-sets.md`).
//!
//! Plain 2D maths: plan geometry of the covered levels (wall faces on `A-WALL`, columns on `A-COL`,
//! opening inserts with `OSILL` / `OHEAD` / `OKIND`) is projected onto the marker line. In a view
//! space X is the distance along the marker line from its first point and Y is the height in
//! drawing units above datum 0. Guides are computed on demand, never saved, and the hidden
//! drawing is never changed. Nothing is occluded: an elevation shows every wall face and opening in
//! its depth band. Curved walls (polyline bulges), roofs, slabs and stairs give no guide.

use aad_doc::{Drawing, Entity, EntityKind, Insert, LevelId, ViewKind, ViewRecord};
use aad_geom::Vec2;

/// Most guide lines one computation returns; more are dropped and `truncated` is set.
pub const MAX_GUIDES: usize = 100_000;
/// Most plan segments read from one level (hostile drawings).
const MAX_SEGMENTS: usize = 2_000_000;
/// Wall layer and column layer the guides read.
pub const WALL_LAYER: &str = "A-WALL";
pub const COLUMN_LAYER: &str = "A-COL";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuideKind {
    /// A level's floor line.
    Floor,
    /// A level's top-of-storey line.
    Top,
    /// A wall face cut by the section line, or a wall face seen in an elevation.
    Wall,
    /// A column face.
    Column,
    /// Door or window jamb.
    Jamb,
    Sill,
    Head,
}

impl GuideKind {
    pub fn label(self) -> &'static str {
        match self {
            GuideKind::Floor => "floor",
            GuideKind::Top => "top",
            GuideKind::Wall => "wall",
            GuideKind::Column => "column",
            GuideKind::Jamb => "jamb",
            GuideKind::Sill => "sill",
            GuideKind::Head => "head",
        }
    }
}

/// One guide line in view coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub a: Vec2,
    pub b: Vec2,
    pub kind: GuideKind,
    pub level: Option<LevelId>,
    /// Text for level lines (the level name).
    pub label: Option<String>,
}

/// The guides of a view.
#[derive(Clone, Debug, Default)]
pub struct Guides {
    pub lines: Vec<Guide>,
    /// More guides than [`MAX_GUIDES`] were found and the rest were dropped.
    pub truncated: bool,
    /// Plan segments were skipped because a level held too many.
    pub segments_capped: bool,
}

/// Plan point to view coordinates: (along the marker line, depth in front of it).
fn project(rec: &ViewRecord, p: Vec2) -> (f64, f64) {
    let rel = p - rec.line.0;
    (rel.dot(rec.along()), rel.dot(rec.facing()))
}

fn metres_per_unit(d: &Drawing) -> f64 {
    crate::annot::drawing_unit(d).map_or(0.0254, |u| u.metres()).max(1e-9)
}

struct Out<'a> {
    g: &'a mut Guides,
    seen: std::collections::HashSet<(GuideKind, i64, i64, i64, i64)>,
    quantum: f64,
}

impl Out<'_> {
    fn push(&mut self, kind: GuideKind, level: Option<LevelId>, a: Vec2, b: Vec2, label: Option<String>) {
        if !(a.is_finite() && b.is_finite()) || a.near(b, 1e-9) {
            return;
        }
        let q = |v: f64| (v / self.quantum).round() as i64;
        let key = (kind, q(a.x), q(a.y), q(b.x), q(b.y));
        if !self.seen.insert(key) {
            return;
        }
        if self.g.lines.len() >= MAX_GUIDES {
            self.g.truncated = true;
            return;
        }
        self.g.lines.push(Guide { a, b, kind, level, label });
    }
}

/// Plan segments of the level's store on `layer`: straight edges of lines and polylines.
fn segments(store: &aad_doc::EntityStore, layer: &str, capped: &mut bool) -> Vec<(Vec2, Vec2)> {
    let mut out = Vec::new();
    for e in store.iter() {
        if !e.common.layer.eq_ignore_ascii_case(layer) {
            continue;
        }
        match &e.kind {
            EntityKind::Line(l) => out.push((l.a.xy(), l.b.xy())),
            EntityKind::LwPolyline(pl) => {
                let n = pl.vertices.len();
                for i in 0..n {
                    let last = i + 1 == n;
                    if last && !pl.closed {
                        break;
                    }
                    let (Some(a), Some(b)) = (pl.vertices.get(i), pl.vertices.get((i + 1) % n.max(1))) else { break };
                    if a.bulge.abs() > 1e-9 {
                        continue;
                    }
                    out.push((a.p, b.p));
                    if out.len() >= MAX_SEGMENTS {
                        *capped = true;
                        return out;
                    }
                }
            }
            _ => {}
        }
        if out.len() >= MAX_SEGMENTS {
            *capped = true;
            break;
        }
    }
    out
}

fn is_opening(e: &Entity) -> Option<&Insert> {
    match &e.kind {
        EntityKind::Insert(i) if i.is_opening() => Some(i),
        _ => None,
    }
}

fn attr(i: &Insert, tag: &str) -> Option<f64> {
    i.attrib_value(tag).and_then(|v| v.trim().parse().ok()).filter(|n: &f64| n.is_finite())
}

/// Part of a segment with 0 <= depth <= `max` (depth is the second coordinate), as projected points.
fn clip_band(a: (f64, f64), b: (f64, f64), max: f64) -> Option<((f64, f64), (f64, f64))> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let dz = b.1 - a.1;
    for (p, q) in [(a.1, dz), (max - a.1, -dz)] {
        // Constraint: p + q * t >= 0.
        if q.abs() < 1e-12 {
            if p < 0.0 {
                return None;
            }
        } else {
            let t = -p / q;
            if q > 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + dz * t);
    Some((at(t0), at(t1)))
}

/// The guides of `rec` computed from the levels it covers.
pub fn compute(d: &Drawing, rec: &ViewRecord) -> Guides {
    let mut g = Guides::default();
    if !rec.is_valid() {
        return g;
    }
    let len = rec.length();
    let mpu = metres_per_unit(d);
    let quantum = (len.max(1.0) * 1e-7).max(1e-6);
    let mut out = Out { g: &mut g, seen: std::collections::HashSet::new(), quantum };
    let default_height = 3.0 / mpu;
    let thick_default = 0.23 / mpu;
    let mut capped = false;

    let covered: Vec<&aad_doc::Level> = d.levels.iter().filter(|l| rec.covers.as_ref().is_none_or(|c| c.contains(&l.id))).collect();
    for l in covered {
        let floor = l.elevation;
        let height = if l.height > 0.0 { l.height } else { default_height };
        let top = floor + height;
        if !(floor.is_finite() && top.is_finite()) {
            continue;
        }
        out.push(GuideKind::Floor, Some(l.id), Vec2::new(0.0, floor), Vec2::new(len, floor), Some(l.name.clone()));
        if l.height > 0.0 {
            out.push(GuideKind::Top, Some(l.id), Vec2::new(0.0, top), Vec2::new(len, top), Some(format!("{} top", l.name)));
        }
        let Some(store) = d.level_store(l.id) else { continue };
        let walls = segments(store, WALL_LAYER, &mut capped);
        let cols = segments(store, COLUMN_LAYER, &mut capped);
        let mut openings: Vec<(&Insert, f64, f64, f64)> = Vec::new();
        for e in store.iter() {
            if let Some(i) = is_opening(e) {
                let sill = attr(i, "OSILL").filter(|s| *s >= 0.0).unwrap_or(0.0);
                let head = attr(i, "OHEAD").unwrap_or(sill + attr(i, "OHEIGHT").filter(|h| *h > 0.0).unwrap_or(2.1));
                let (s_y, h_y) = (floor + sill / mpu, floor + head / mpu);
                if s_y.is_finite() && h_y.is_finite() && h_y > s_y {
                    openings.push((i, s_y, h_y, attr(i, "OTHICK").filter(|t| *t > 0.0).unwrap_or(thick_default)));
                }
            }
            if openings.len() >= 100_000 {
                break;
            }
        }
        match rec.kind {
            ViewKind::Section => section_level(&mut out, rec, l.id, (floor, top), len, &walls, &cols, &openings),
            ViewKind::Elevation => elevation_level(&mut out, rec, l.id, (floor, top), len, &walls, &cols, &openings),
        }
    }
    g.segments_capped = capped;
    g
}

type Opening<'a> = (&'a Insert, f64, f64, f64);

/// A face crossing the cut plane: its X along the line.
fn crossings(rec: &ViewRecord, len: f64, segs: &[(Vec2, Vec2)]) -> Vec<f64> {
    let mut xs = Vec::new();
    for (p, q) in segs {
        let (pa, pb) = (project(rec, *p), project(rec, *q));
        if (pa.1 > 0.0) == (pb.1 > 0.0) && pa.1 != 0.0 && pb.1 != 0.0 {
            continue;
        }
        if (pa.1 - pb.1).abs() < 1e-12 {
            // Lies in the cut plane: neither a face nor a void.
            continue;
        }
        let t = pa.1 / (pa.1 - pb.1);
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        let x = pa.0 + (pb.0 - pa.0) * t;
        if x >= -1e-9 && x <= len + 1e-9 {
            xs.push(x.clamp(0.0, len));
        }
    }
    xs
}

#[allow(clippy::too_many_arguments)]
fn section_level(
    out: &mut Out<'_>,
    rec: &ViewRecord,
    lv: LevelId,
    (floor, top): (f64, f64),
    len: f64,
    walls: &[(Vec2, Vec2)],
    cols: &[(Vec2, Vec2)],
    openings: &[Opening<'_>],
) {
    // Openings the cut passes through: their centre-line position along the line.
    let mut cut_openings: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (i, s_y, h_y, th) in openings {
        let w = i.opening_width().unwrap_or(0.0);
        let dir = Vec2::new(i.rotation.cos(), i.rotation.sin());
        let c = i.insert.xy();
        let (a, b) = (c - dir * (w / 2.0), c + dir * (w / 2.0));
        for x in crossings(rec, len, &[(a, b)]) {
            cut_openings.push((x, *s_y, *h_y, *th));
        }
    }
    for x in crossings(rec, len, walls) {
        // A wall face is broken between sill and head where an opening is cut.
        let mut spans = vec![(floor, top)];
        for (ox, s_y, h_y, th) in &cut_openings {
            if (x - ox).abs() <= th / 2.0 + 1e-6 {
                spans = spans
                    .into_iter()
                    .flat_map(|(lo, hi)| {
                        let mut v = Vec::new();
                        if *s_y > lo {
                            v.push((lo, s_y.min(hi)));
                        }
                        if *h_y < hi {
                            v.push((h_y.max(lo), hi));
                        }
                        v
                    })
                    .collect();
            }
        }
        for (lo, hi) in spans {
            out.push(GuideKind::Wall, Some(lv), Vec2::new(x, lo), Vec2::new(x, hi), None);
        }
    }
    for x in crossings(rec, len, cols) {
        out.push(GuideKind::Column, Some(lv), Vec2::new(x, floor), Vec2::new(x, top), None);
    }
    for (x, s_y, h_y, th) in cut_openings {
        let (x0, x1) = ((x - th / 2.0).max(0.0), (x + th / 2.0).min(len));
        out.push(GuideKind::Sill, Some(lv), Vec2::new(x0, s_y), Vec2::new(x1, s_y), None);
        out.push(GuideKind::Head, Some(lv), Vec2::new(x0, h_y), Vec2::new(x1, h_y), None);
    }
}

#[allow(clippy::too_many_arguments)]
fn elevation_level(
    out: &mut Out<'_>,
    rec: &ViewRecord,
    lv: LevelId,
    (floor, top): (f64, f64),
    len: f64,
    walls: &[(Vec2, Vec2)],
    cols: &[(Vec2, Vec2)],
    openings: &[Opening<'_>],
) {
    let seen_edges = |segs: &[(Vec2, Vec2)]| -> Vec<f64> {
        let mut xs = Vec::new();
        for (p, q) in segs {
            if let Some((a, b)) = clip_band(project(rec, *p), project(rec, *q), rec.depth) {
                for x in [a.0, b.0] {
                    if x >= -1e-9 && x <= len + 1e-9 {
                        xs.push(x.clamp(0.0, len));
                    }
                }
            }
        }
        xs
    };
    for x in seen_edges(walls) {
        out.push(GuideKind::Wall, Some(lv), Vec2::new(x, floor), Vec2::new(x, top), None);
    }
    for x in seen_edges(cols) {
        out.push(GuideKind::Column, Some(lv), Vec2::new(x, floor), Vec2::new(x, top), None);
    }
    for (i, s_y, h_y, _) in openings {
        let w = i.opening_width().unwrap_or(0.0);
        let dir = Vec2::new(i.rotation.cos(), i.rotation.sin());
        let c = i.insert.xy();
        let (a, b) = (c - dir * (w / 2.0), c + dir * (w / 2.0));
        let (pa, pb) = (project(rec, a), project(rec, b));
        // The opening is seen when its centre is within the depth band and its span overlaps the line.
        let depth_mid = (pa.1 + pb.1) / 2.0;
        if !(0.0..=rec.depth).contains(&depth_mid) {
            continue;
        }
        let (x0, x1) = (pa.0.min(pb.0), pa.0.max(pb.0));
        if x1 < 0.0 || x0 > len {
            continue;
        }
        let (cx0, cx1) = (x0.max(0.0), x1.min(len));
        out.push(GuideKind::Sill, Some(lv), Vec2::new(cx0, *s_y), Vec2::new(cx1, *s_y), None);
        out.push(GuideKind::Head, Some(lv), Vec2::new(cx0, *h_y), Vec2::new(cx1, *h_y), None);
        if x0 >= 0.0 {
            out.push(GuideKind::Jamb, Some(lv), Vec2::new(x0, *s_y), Vec2::new(x0, *h_y), None);
        }
        if x1 <= len {
            out.push(GuideKind::Jamb, Some(lv), Vec2::new(x1, *s_y), Vec2::new(x1, *h_y), None);
        }
    }
}

/// Convert a guide back to plan coordinates is not defined (a guide is a projection); view
/// coordinates are what the view space draws in.
#[cfg(test)]
mod tests {
    use super::*;
    use aad_doc::{Common, Entity, ViewLook};
    use aad_geom::Vec3;

    fn wall(d: &mut Drawing, a: Vec2, b: Vec2) {
        let h = d.new_handle();
        let common = Common { layer: WALL_LAYER.into(), ..Common::default() };
        d.model.push(Entity {
            handle: h,
            common,
            kind: EntityKind::Line(aad_doc::Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) }),
        });
    }

    fn rec(kind: ViewKind, depth: f64) -> ViewRecord {
        ViewRecord {
            id: aad_doc::SpaceId(50),
            kind,
            name: "S".into(),
            level: LevelId(1),
            marker: None,
            line: (Vec2::new(0.0, 0.0), Vec2::new(10_000.0, 0.0)),
            look: ViewLook::Left,
            depth,
            covers: None,
            traces: Vec::new(),
            view: None,
        }
    }

    fn two_floor_drawing() -> Drawing {
        let mut d = Drawing::new_metric();
        d.header.set_f64("INSUNITS", 4.0);
        d.materialize_levels("Ground", 0.0, 3000.0).unwrap();
        d.add_level("First", 3150.0, 3000.0, None).unwrap();
        d
    }

    #[test]
    fn floor_and_top_lines_come_from_the_levels() {
        let d = two_floor_drawing();
        let g = compute(&d, &rec(ViewKind::Section, 5000.0));
        let ys: Vec<f64> = g.lines.iter().filter(|l| matches!(l.kind, GuideKind::Floor | GuideKind::Top)).map(|l| l.a.y).collect();
        assert!(ys.contains(&0.0) && ys.contains(&3000.0) && ys.contains(&3150.0) && ys.contains(&6150.0), "{ys:?}");
        assert!(g.lines.iter().any(|l| l.label.as_deref() == Some("First")));
    }

    #[test]
    fn a_wall_crossing_the_section_line_gives_two_cut_faces_and_one_beside_it_none() {
        let mut d = two_floor_drawing();
        wall(&mut d, Vec2::new(4000.0, -3000.0), Vec2::new(4000.0, 3000.0));
        wall(&mut d, Vec2::new(4230.0, -3000.0), Vec2::new(4230.0, 3000.0));
        wall(&mut d, Vec2::new(8000.0, -3000.0), Vec2::new(8000.0, -100.0));
        let g = compute(&d, &rec(ViewKind::Section, 5000.0));
        let xs: Vec<f64> = g.lines.iter().filter(|l| l.kind == GuideKind::Wall).map(|l| l.a.x).collect();
        assert_eq!(xs, vec![4000.0, 4230.0]);
        assert_eq!(
            (g.lines.iter().find(|l| l.kind == GuideKind::Wall).unwrap().a.y, g.lines.iter().find(|l| l.kind == GuideKind::Wall).unwrap().b.y),
            (0.0, 3000.0)
        );
    }

    #[test]
    fn moving_a_wall_moves_its_guide() {
        let mut d = two_floor_drawing();
        wall(&mut d, Vec2::new(4000.0, -3000.0), Vec2::new(4000.0, 3000.0));
        let before = compute(&d, &rec(ViewKind::Section, 5000.0));
        let h = d.model.iter().next().unwrap().handle;
        d.modify_entity(h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.a.x += 150.0;
                l.b.x += 150.0;
            }
        })
        .unwrap();
        let after = compute(&d, &rec(ViewKind::Section, 5000.0));
        let x = |g: &Guides| g.lines.iter().find(|l| l.kind == GuideKind::Wall).map(|l| l.a.x);
        assert_eq!((x(&before), x(&after)), (Some(4000.0), Some(4150.0)));
    }

    #[test]
    fn look_right_flips_which_side_is_seen_in_an_elevation() {
        let mut d = two_floor_drawing();
        wall(&mut d, Vec2::new(2000.0, 2000.0), Vec2::new(6000.0, 2000.0));
        let left = compute(&d, &rec(ViewKind::Elevation, 3000.0));
        assert!(left.lines.iter().any(|l| l.kind == GuideKind::Wall && l.a.x == 2000.0));
        let mut r = rec(ViewKind::Elevation, 3000.0);
        r.look = ViewLook::Right;
        assert!(!compute(&d, &r).lines.iter().any(|l| l.kind == GuideKind::Wall));
        // Beyond the depth: not seen.
        assert!(!compute(&d, &rec(ViewKind::Elevation, 1000.0)).lines.iter().any(|l| l.kind == GuideKind::Wall));
    }

    #[test]
    fn hostile_input_gives_no_guides_and_never_panics() {
        let d = two_floor_drawing();
        let mut r = rec(ViewKind::Section, 0.0);
        assert!(compute(&d, &r).lines.is_empty(), "depth 0");
        r.depth = f64::NAN;
        assert!(compute(&d, &r).lines.is_empty());
        r.depth = 100.0;
        r.line = (Vec2::ZERO, Vec2::ZERO);
        assert!(compute(&d, &r).lines.is_empty(), "zero-length line");
        r.line = (Vec2::new(0.0, 0.0), Vec2::new(f64::INFINITY, 0.0));
        assert!(compute(&d, &r).lines.is_empty());
    }

    #[test]
    fn nan_heights_and_huge_wall_counts_are_survived_and_capped() {
        let mut d = two_floor_drawing();
        d.levels[0].elevation = f64::NAN;
        d.levels[1].height = f64::INFINITY;
        wall(&mut d, Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0));
        let _ = compute(&d, &rec(ViewKind::Section, 100.0));
        let _ = compute(&d, &rec(ViewKind::Elevation, 100.0));
        let mut d = two_floor_drawing();
        for i in 0..2_000 {
            wall(&mut d, Vec2::new(i as f64 * 5.0 + 1.0, -10.0), Vec2::new(i as f64 * 5.0 + 1.0, 10.0));
        }
        let g = compute(&d, &rec(ViewKind::Section, 100.0));
        assert!(g.lines.len() <= MAX_GUIDES);
        assert!(g.lines.iter().filter(|l| l.kind == GuideKind::Wall).count() > 1000, "within the line");
    }

    #[test]
    fn clipping_a_segment_to_the_depth_band() {
        assert_eq!(clip_band((0.0, -5.0), (10.0, 5.0), 10.0).map(|(a, _)| a.0), Some(5.0));
        assert!(clip_band((0.0, 20.0), (10.0, 30.0), 10.0).is_none());
        assert!(clip_band((0.0, 3.0), (10.0, 3.0), 10.0).is_some());
        assert!(clip_band((f64::NAN, 3.0), (10.0, 3.0), 10.0).is_none());
    }
}

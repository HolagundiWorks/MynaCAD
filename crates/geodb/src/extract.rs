//! Stage 1: walk one entity (expanding blocks with their transforms) into table rows in millimetres.
//!
//! Everything here treats the drawing as hostile input: nesting is bounded (`MAX_BLOCK_DEPTH`),
//! arrays and the rows one entity may produce are capped, and non-finite numbers are skipped.

use std::collections::HashMap;
use std::sync::Arc;

use aad_doc::{Drawing, Entity, EntityKind, Insert, MAX_BLOCK_DEPTH};
use aad_geom::{Mat3, Polyline, Segment, Vec2};

use crate::model::*;

/// Most rows one top-level entity may expand into (a block array or a block bomb stops here).
pub const MAX_ROWS_PER_ENTITY: usize = 50_000;
/// Most copies of a block one array insert makes.
pub const MAX_ARRAY: u64 = 10_000;

/// What the walk skipped, by entity type, so the report can say what it did not look at.
#[derive(Default, Clone, Debug)]
pub struct Skipped(pub HashMap<String, u32>);

impl Skipped {
    pub fn add(&mut self, what: &str) {
        *self.0.entry(what.to_string()).or_insert(0) += 1;
    }
    pub fn merge(&mut self, o: &Skipped) {
        for (k, v) in &o.0 {
            *self.0.entry(k.clone()).or_insert(0) += v;
        }
    }
}

/// Shared state of one extraction pass.
pub struct Extractor<'a> {
    pub d: &'a Drawing,
    /// Millimetres per drawing unit.
    pub scale: f64,
    pub next_id: u64,
    pub intern: Interner,
    pub skipped: Skipped,
    shapes: HashMap<String, u64>,
}

impl<'a> Extractor<'a> {
    pub fn new(d: &'a Drawing, scale: f64, first_id: u64) -> Self {
        Extractor {
            d,
            scale: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 },
            next_id: first_id,
            intern: Interner::default(),
            skipped: Skipped::default(),
            shapes: HashMap::new(),
        }
    }

    fn id(&mut self) -> u64 {
        let i = self.next_id;
        self.next_id += 1;
        i
    }

    /// Rows for one top-level entity.
    pub fn entity(&mut self, e: &Entity) -> Extracted {
        let mut out = Extracted::default();
        let root = Mat3::scale(self.scale, self.scale);
        let layer = self.intern.get(&e.common.layer);
        let mut w = Walk { x: self, out: &mut out, handle: e.handle.0, budget: MAX_ROWS_PER_ENTITY };
        w.entity(e, &root, 0, &layer, "");
        out
    }

    /// Hash of a block's own geometry, independent of its name and of where it is placed.
    fn shape_hash(&mut self, name: &str) -> u64 {
        if let Some(h) = self.shapes.get(name) {
            return *h;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: i64| {
            for b in v.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        if let Some(b) = self.d.blocks.get(name) {
            let base = b.base;
            let mut items: Vec<[i64; 5]> = Vec::new();
            for e in b.entities.iter() {
                let rel = |p: aad_geom::Vec3| (q(p.x - base.x), q(p.y - base.y));
                match &e.kind {
                    EntityKind::Line(l) => {
                        let (a, c) = (rel(l.a), rel(l.b));
                        let (a, c) = if a <= c { (a, c) } else { (c, a) };
                        items.push([1, a.0, a.1, c.0, c.1]);
                    }
                    EntityKind::Circle(c) => {
                        let o = rel(c.center);
                        items.push([2, o.0, o.1, q(c.radius), 0]);
                    }
                    EntityKind::Arc(a) => {
                        let o = rel(a.center);
                        items.push([3, o.0, o.1, q(a.radius), q(a.start.to_degrees() * 10.0) ^ q(a.end.to_degrees() * 10.0)]);
                    }
                    EntityKind::LwPolyline(p) => {
                        let s: i64 = p
                            .vertices
                            .iter()
                            .fold(0i64, |acc, v| acc.wrapping_add(q(v.p.x - base.x)).wrapping_add(q(v.p.y - base.y).wrapping_mul(31)));
                        items.push([4, p.vertices.len() as i64, s, i64::from(p.closed), 0]);
                    }
                    _ => {}
                }
            }
            items.sort_unstable();
            for it in items {
                for v in it {
                    mix(v);
                }
            }
        }
        self.shapes.insert(name.to_string(), h);
        h
    }
}

struct Walk<'x, 'a> {
    x: &'x mut Extractor<'a>,
    out: &'x mut Extracted,
    handle: u64,
    budget: usize,
}

fn finite(p: Vec2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn deg(r: f64) -> f64 {
    r.to_degrees().rem_euclid(360.0)
}

fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

impl Walk<'_, '_> {
    fn new_prim(&mut self, e: &Entity, kind: PrimKind, closed: bool, layer: &Arc<str>, block_path: &str) -> usize {
        let id = self.x.id();
        let color = match e.common.color {
            aad_doc::color::Color::Index(i) => i32::from(i),
            _ => -1,
        };
        let lw = match e.common.lineweight {
            aad_doc::Lineweight::Mm100(v) => i32::from(v),
            _ => -1,
        };
        let p = Prim {
            id,
            handle: self.handle,
            kind,
            layer: layer.clone(),
            color,
            linetype: self.x.intern.get(&e.common.linetype),
            lineweight: lw,
            closed,
            block_path: self.x.intern.get(block_path),
            bbox: [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        };
        self.out.prims.push(p);
        self.out.prims.len() - 1
    }

    fn grow(&mut self, prim: usize, b: [f64; 4]) {
        if let Some(p) = self.out.prims.get_mut(prim) {
            p.bbox = union(p.bbox, b);
        }
    }

    fn seg(&mut self, prim: usize, seq: u32, a: Vec2, b: Vec2) {
        if !finite(a) || !finite(b) || self.budget == 0 {
            return;
        }
        let Some(pid) = self.out.prims.get(prim).map(|p| p.id) else { return };
        let id = self.x.id();
        let s = Seg::new(id, pid, seq, [a.x, a.y], [b.x, b.y]);
        if s.len < 1e-6 {
            return;
        }
        self.budget -= 1;
        let bb = s.bbox();
        self.out.segs.push(s);
        self.grow(prim, bb);
    }

    fn arc(&mut self, prim: usize, c: Vec2, r: f64, a0: f64, a1: f64) {
        if !finite(c) || !r.is_finite() || r <= 1e-6 || self.budget == 0 {
            return;
        }
        let Some(pid) = self.out.prims.get(prim).map(|p| p.id) else { return };
        let id = self.x.id();
        self.budget -= 1;
        self.out.arcs.push(ArcRow { id, prim: pid, cx: c.x, cy: c.y, r, a0, a1 });
        self.grow(prim, [c.x - r, c.y - r, c.x + r, c.y + r]);
    }

    /// An arc entity through a transform (a mirror swaps its ends).
    fn xf_arc(m: &Mat3, c: Vec2, r: f64, start: f64, end: f64) -> (Vec2, f64, f64, f64) {
        let sp = m.apply(Vec2::polar(c, r, start));
        let ep = m.apply(Vec2::polar(c, r, end));
        let c2 = m.apply(c);
        let r2 = r * m.scale_factor();
        let (s, e) = if m.is_mirroring() { (c2.angle_to(ep), c2.angle_to(sp)) } else { (c2.angle_to(sp), c2.angle_to(ep)) };
        (c2, r2, deg(s), deg(e))
    }

    fn entity(&mut self, e: &Entity, m: &Mat3, depth: usize, owner_layer: &Arc<str>, path: &str) {
        if !e.common.visible || self.budget == 0 {
            return;
        }
        // Layer "0" inside a block takes the layer of the insert that placed it.
        let layer = if depth > 0 && e.common.layer == "0" { owner_layer.clone() } else { self.x.intern.get(&e.common.layer) };
        match &e.kind {
            EntityKind::Line(l) => {
                let p = self.new_prim(e, PrimKind::Line, false, &layer, path);
                self.seg(p, 0, m.apply(l.a.xy()), m.apply(l.b.xy()));
            }
            EntityKind::Arc(a) => {
                let (c, r, s, en) = Self::xf_arc(m, a.center.xy(), a.radius, a.start, a.end);
                let p = self.new_prim(e, PrimKind::Arc, false, &layer, path);
                self.arc(p, c, r, s, en);
            }
            EntityKind::Circle(c) => {
                let p = self.new_prim(e, PrimKind::Circle, true, &layer, path);
                self.arc(p, m.apply(c.center.xy()), c.radius * m.scale_factor(), 0.0, 360.0);
            }
            EntityKind::LwPolyline(pl) => {
                let mirror = m.is_mirroring();
                let mut line = Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
                if line.vertices.len() > MAX_ROWS_PER_ENTITY {
                    self.x.skipped.add("oversized polyline");
                    return;
                }
                for v in &mut line.vertices {
                    v.p = m.apply(v.p);
                    if mirror {
                        v.bulge = -v.bulge;
                    }
                }
                let p = self.new_prim(e, PrimKind::Poly, pl.closed, &layer, path);
                for (i, s) in line.segments().into_iter().enumerate() {
                    match s {
                        Segment::Line(l) => self.seg(p, i as u32, l.a, l.b),
                        Segment::Arc { arc, .. } => self.arc(p, arc.center, arc.radius, deg(arc.start), deg(arc.end)),
                    }
                }
            }
            EntityKind::Point(pt) => {
                let p = self.new_prim(e, PrimKind::Point, false, &layer, path);
                let q = m.apply(pt.p.xy());
                if finite(q) {
                    self.grow(p, [q.x, q.y, q.x, q.y]);
                }
            }
            EntityKind::Text(t) => self.text(e, &layer, path, m, t.insert.xy(), t.height, t.rotation, &t.value),
            EntityKind::MText(t) => self.text(e, &layer, path, m, t.insert.xy(), t.height, t.rotation, &t.contents),
            EntityKind::Hatch(h) => {
                let p = self.new_prim(e, PrimKind::Hatch, true, &layer, path);
                let mut loops = Vec::new();
                for l in h.loops.iter().take(64) {
                    let pts: Vec<[f64; 2]> =
                        l.vertices.iter().take(MAX_ROWS_PER_ENTITY).map(|v| m.apply(v.p)).filter(|v| finite(*v)).map(|v| [v.x, v.y]).collect();
                    loops.push(pts);
                }
                self.hatch(p, h.pattern.clone(), h.solid, loops);
            }
            EntityKind::Solid(s) | EntityKind::Trace(s) => {
                let c = &s.corners;
                let pts: Vec<[f64; 2]> =
                    [c[0], c[1], c[3], c[2]].iter().map(|v| m.apply(v.xy())).filter(|v| finite(*v)).map(|v| [v.x, v.y]).collect();
                let p = self.new_prim(e, PrimKind::Hatch, true, &layer, path);
                self.hatch(p, "SOLID".into(), true, vec![pts]);
            }
            EntityKind::Insert(ins) => self.insert(e, ins, m, depth, &layer, path),
            EntityKind::AttDef(_) => {}
            other => self.x.skipped.add(other.dxf_name()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, e: &Entity, layer: &Arc<str>, path: &str, m: &Mat3, at: Vec2, h: f64, rot: f64, value: &str) {
        let p = self.new_prim(e, PrimKind::Text, false, layer, path);
        let a = m.apply(at);
        if !finite(a) || self.budget == 0 {
            return;
        }
        let Some(pid) = self.out.prims.get(p).map(|p| p.id) else { return };
        let id = self.x.id();
        self.budget -= 1;
        let height = if h.is_finite() { h * m.scale_factor() } else { 0.0 };
        let clipped: String = value.chars().take(512).collect();
        self.out.texts.push(TextRow {
            id,
            prim: pid,
            text: clipped,
            x: a.x,
            y: a.y,
            height,
            rot: deg(if rot.is_finite() { rot + m.rotation() } else { 0.0 }),
        });
        self.grow(p, [a.x, a.y, a.x, a.y]);
    }

    fn hatch(&mut self, prim: usize, pattern: String, solid: bool, loops: Vec<Vec<[f64; 2]>>) {
        let Some(pid) = self.out.prims.get(prim).map(|p| p.id) else { return };
        let id = self.x.id();
        let mut area = 0.0;
        for (i, l) in loops.iter().enumerate() {
            let a = shoelace(l).abs();
            // The first loop is the outer boundary; the rest are islands.
            area += if i == 0 { a } else { -a };
            for p in l {
                self.grow(prim, [p[0], p[1], p[0], p[1]]);
            }
        }
        self.out.hatches.push(HatchRow { id, prim: pid, pattern, solid, area: area.max(0.0), loops });
    }

    fn insert(&mut self, e: &Entity, ins: &Insert, m: &Mat3, depth: usize, layer: &Arc<str>, path: &str) {
        let at = m.apply(ins.insert.xy());
        let p = self.new_prim(e, PrimKind::Insert, false, layer, path);
        if !finite(at) {
            return;
        }
        self.grow(p, [at.x, at.y, at.x, at.y]);
        let Some(block) = self.x.d.blocks.get(&ins.block).cloned() else {
            self.x.skipped.add("insert of a missing block");
            return;
        };
        if depth + 1 > MAX_BLOCK_DEPTH {
            self.x.skipped.add("block nested too deep");
            return;
        }
        let Some(pid) = self.out.prims.get(p).map(|p| p.id) else { return };
        let inner_path = if path.is_empty() { ins.block.clone() } else { format!("{path}/{}", ins.block) };
        let first_row = self.out.prims.len();
        let cols = u64::from(ins.cols.max(1));
        let rows = u64::from(ins.rows.max(1));
        let copies = (cols.saturating_mul(rows)).min(MAX_ARRAY);
        let local = ins.transform(block.base.xy());
        let rot = Mat3::rotate(ins.rotation);
        for k in 0..copies {
            let (c, r) = (k % cols, k / cols);
            let off = rot.apply_vec(Vec2::new(c as f64 * ins.col_spacing, r as f64 * ins.row_spacing));
            let mk = m.then_before(Mat3::translate(off)).then_before(local);
            for be in block.entities.iter() {
                if matches!(be.kind, EntityKind::AttDef(_)) {
                    continue;
                }
                self.entity(be, &mk, depth + 1, layer, &inner_path);
                if self.budget == 0 {
                    self.x.skipped.add("entity truncated at the row cap");
                    break;
                }
            }
        }
        // Footprint of what the insert drew, and its row.
        let mut bb = [at.x, at.y, at.x, at.y];
        for pr in self.out.prims.iter().skip(first_row) {
            if pr.bbox[0] <= pr.bbox[2] {
                bb = union(bb, pr.bbox);
            }
        }
        self.grow(p, bb);
        let id = self.x.id();
        let shape_hash = self.x.shape_hash(&ins.block);
        let attrs = ins.attribs.iter().map(|a| (a.tag.to_ascii_uppercase(), a.text.value.clone())).collect();
        let name = self.x.intern.get(&ins.block);
        let s = m.scale_factor();
        self.out.inserts.push(InsertRow {
            id,
            prim: pid,
            block: name,
            x: at.x,
            y: at.y,
            rot: deg(ins.rotation + m.rotation()),
            sx: ins.scale.x * s,
            sy: ins.scale.y * s,
            width: bb[2] - bb[0],
            depth: bb[3] - bb[1],
            shape_hash,
            attrs,
            nested: depth > 0,
        });
    }
}

pub fn shoelace(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use aad_doc::{Block, Handle, Line};
    use aad_geom::Vec3;

    fn line(h: u64, a: (f64, f64), b: (f64, f64)) -> Entity {
        Entity::new(Handle(h), EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }))
    }

    #[test]
    fn units_scale_to_millimetres_and_blocks_expand_with_their_transform() {
        let mut d = Drawing::new_metric();
        let mut blk = Block::new("B");
        blk.entities.push(line(0x10, (0.0, 0.0), (1.0, 0.0)));
        d.blocks.insert("B".into(), Arc::new(blk));
        let ins = Insert {
            block: "B".into(),
            insert: Vec3::new(2.0, 3.0, 0.0),
            rotation: std::f64::consts::FRAC_PI_2,
            scale: Vec3::new(2.0, 2.0, 1.0),
            ..Insert::default()
        };
        let e = Entity::new(Handle(7), EntityKind::Insert(ins));
        let mut x = Extractor::new(&d, 1000.0, 1);
        let out = x.entity(&e);
        assert_eq!(out.inserts.len(), 1);
        let s = out.segs.first().unwrap();
        // Block (0,0)-(1,0) scaled 2, turned 90 degrees, moved to (2,3), in metres -> mm.
        assert!((s.x1 - 2000.0).abs() < 1e-6 && (s.y1 - 3000.0).abs() < 1e-6, "{s:?}");
        assert!((s.x2 - 2000.0).abs() < 1e-6 && (s.y2 - 5000.0).abs() < 1e-6, "{s:?}");
        assert!((s.len - 2000.0).abs() < 1e-6 && (s.angle - 90.0).abs() < 1e-6);
        assert!(out.prims.iter().all(|p| p.handle == 7), "block content belongs to the insert");
        let bp = out.prims.iter().find(|p| p.kind == PrimKind::Line).unwrap();
        assert_eq!(&*bp.block_path, "B");
    }

    #[test]
    fn a_cyclic_block_stops_at_the_depth_limit() {
        let mut d = Drawing::new_metric();
        let mut blk = Block::new("LOOP");
        blk.entities.push(line(1, (0.0, 0.0), (1.0, 0.0)));
        blk.entities.push(Entity::new(Handle(2), EntityKind::Insert(Insert { block: "LOOP".into(), ..Insert::default() })));
        d.blocks.insert("LOOP".into(), Arc::new(blk));
        let e = Entity::new(Handle(9), EntityKind::Insert(Insert { block: "LOOP".into(), ..Insert::default() }));
        let mut x = Extractor::new(&d, 1.0, 1);
        let out = x.entity(&e);
        assert!(out.segs.len() <= MAX_BLOCK_DEPTH + 1, "{}", out.segs.len());
        assert!(x.skipped.0.contains_key("block nested too deep"));
    }

    #[test]
    fn polyline_edges_become_segments_and_bulges_become_arcs() {
        let d = Drawing::new_metric();
        let mut pl = aad_doc::LwPolyline { vertices: vec![], closed: true, const_width: 0.0, elevation: 0.0, plinegen: false };
        for (p, b) in [((0.0, 0.0), 0.0), ((100.0, 0.0), 1.0), ((100.0, 100.0), 0.0), ((0.0, 100.0), 0.0)] {
            pl.vertices.push(aad_geom::PolyVertex { p: Vec2::new(p.0, p.1), bulge: b, start_width: 0.0, end_width: 0.0 });
        }
        let e = Entity::new(Handle(3), EntityKind::LwPolyline(pl));
        let mut x = Extractor::new(&d, 1.0, 1);
        let out = x.entity(&e);
        assert_eq!(out.segs.len(), 3);
        assert_eq!(out.arcs.len(), 1);
        let a = out.arcs.first().unwrap();
        assert!((a.r - 50.0).abs() < 1e-6, "bulge 1 is a half circle over a 100 chord: {a:?}");
    }
}

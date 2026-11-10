//! Stage 3: rules turn pairs, arcs and blocks into features with a confidence.
//!
//! Walls are always two lines. A pair of parallel faces whose gap matches a wall type becomes a
//! pair; collinear pairs on one axis merge into a wall run (centre line, thickness, type, length,
//! corner and T joins). Doors are swing arcs whose hinge sits on a wall, or door blocks. Windows
//! are blocks or the thin lines that cross a wall opening. Nothing here reads a picture and
//! nothing moves anything: the result is rows.

use std::collections::{HashMap, HashSet};

use rstar::{AABB, RTree, RTreeObject};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::PairRow;
use crate::derive::{PairParams, PairStats, SegRef, find_pairs};
use crate::model::{ArcRow, Feature, InsertRow, Member, Prim, q};
use crate::rules::{Rules, glob_any};
use crate::units::UnitSystem;

pub struct Input<'a> {
    pub segs: Vec<SegRef<'a>>,
    pub arcs: Vec<(&'a ArcRow, &'a Prim)>,
    pub inserts: Vec<(&'a InsertRow, &'a Prim)>,
}

pub struct Params<'a> {
    pub rules: &'a Rules,
    pub system: UnitSystem,
    /// Millimetres per drawing unit (for attribute values written in drawing units).
    pub scale: f64,
}

#[derive(Default, Clone, Debug)]
pub struct ClassifyStats {
    pub pair: PairStats,
    pub pairs: usize,
    pub runs: usize,
}

#[derive(Clone, Copy)]
struct Boxed {
    idx: usize,
    b: [f64; 4],
}

impl RTreeObject for Boxed {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.b[0], self.b[1]], [self.b[2], self.b[3]])
    }
}

fn window(t: &RTree<Boxed>, x0: f64, y0: f64, x1: f64, y1: f64) -> impl Iterator<Item = usize> + '_ {
    t.locate_in_envelope_intersecting(&AABB::from_corners([x0, y0], [x1, y1])).map(|b| b.idx)
}

/// A wall run kept for the later steps (joins, openings).
struct Run {
    feat: usize,
    u: [f64; 2],
    n: [f64; 2],
    d: f64,
    t0: f64,
    t1: f64,
    half: f64,
    theta: f64,
}

impl Run {
    /// Axial position and lateral offset of a point against this run's axis.
    fn locate(&self, p: [f64; 2]) -> (f64, f64) {
        (p[0] * self.u[0] + p[1] * self.u[1], p[0] * self.n[0] + p[1] * self.n[1] - self.d)
    }
    fn contains(&self, p: [f64; 2], tol: f64) -> bool {
        let (s, o) = self.locate(p);
        o.abs() <= self.half + tol && s >= self.t0 - tol && s <= self.t1 + tol
    }
}

fn is_dashed(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    ["DASH", "HIDDEN", "CENTER", "PHANTOM", "DOT"].iter().any(|k| u.contains(k))
}

fn feature_key(kind: &str, sub: &str, a: [f64; 2], b: [f64; 2], w: f64) -> String {
    let (qa, qb) = ((q(a[0]), q(a[1])), (q(b[0]), q(b[1])));
    let (lo, hi) = if qa <= qb { (qa, qb) } else { (qb, qa) };
    let mut h = Sha256::new();
    h.update(format!("{kind}|{sub}|{}|{}|{}|{}|{}", lo.0, lo.1, hi.0, hi.1, q(w)));
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn members_of(segs: &[SegRef], idx: &[usize], role: &str) -> Vec<Member> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &i in idx {
        if let Some(s) = segs.get(i)
            && seen.insert(s.prim.id)
        {
            out.push(Member { prim: s.prim.id, handle: s.prim.handle, role: role.to_string() });
        }
    }
    out
}

pub fn classify(inp_all: &Input, par: &Params, next_id: &mut u64) -> (Vec<Feature>, ClassifyStats, Vec<PairRow>) {
    let rules = par.rules;
    let mut stats = ClassifyStats::default();
    let mut feats: Vec<Feature> = Vec::new();
    // Columns first: the sides of a small closed rectangle pair up like a thin wall, so their lines leave the wall candidates.
    let owners = opening_owners(inp_all, par);
    let cols = crate::columns::find(inp_all, par, &|p: &Prim| in_symbol_block(p, par, &owners), next_id);
    // Only a rectangle the rules are fairly sure of leaves the wall candidates; a doubtful one may still be a short wall piece.
    let exclude: HashSet<u64> = {
        let sure: HashSet<u64> =
            cols.features.iter().filter(|f| f.confidence >= rules.evidence.classify_min).flat_map(|f| f.members.iter().map(|m| m.prim)).collect();
        cols.prims.intersection(&sure).copied().collect()
    };
    let filtered = Input {
        segs: inp_all.segs.iter().filter(|s| !exclude.contains(&s.prim.id)).copied().collect(),
        arcs: inp_all.arcs.clone(),
        inserts: inp_all.inserts.clone(),
    };
    let inp = &filtered;
    feats.extend(cols.features);
    let bands = rules.bands(par.system);
    let pp = PairParams {
        angle_tol_deg: rules.pair.angle_tol_deg,
        bands: &bands,
        min_overlap: rules.pair.min_overlap_mm,
        min_ratio: rules.pair.min_overlap_ratio,
        min_seg: rules.pair.min_segment_mm,
        max_steps: 4000,
    };
    let (pairs, ps) = find_pairs(&inp.segs, &pp);
    stats.pair = ps;
    stats.pairs = pairs.len();

    let mut runs = wall_runs(inp, par, &pairs, &mut feats, next_id);
    stats.runs = runs.len();
    wall_joins(par, &runs, &mut feats);
    let mut seg_tree_items: Vec<Boxed> = Vec::with_capacity(inp.segs.len());
    for (i, s) in inp.segs.iter().enumerate() {
        seg_tree_items.push(Boxed { idx: i, b: s.seg.bbox() });
    }
    let seg_tree = RTree::bulk_load(seg_tree_items);

    wall_caps(inp, par, &runs, &mut feats, &seg_tree);
    let run_tree = RTree::bulk_load(runs.iter().enumerate().map(|(i, r)| Boxed { idx: i, b: feats[r.feat].bbox() }).collect());
    let mut openings: Vec<Feature> = Vec::new();
    doors_by_block(inp, par, &mut openings, next_id);
    windows_by_block(inp, par, &mut openings, next_id);
    doors_by_swing(inp, par, &runs, &run_tree, &feats, &seg_tree, &owners, &mut openings, next_id);
    windows_by_lines(inp, par, &runs, &run_tree, &feats, &seg_tree, &owners, &mut openings, next_id);
    dedupe(&mut openings);
    feats.extend(openings);
    runs.clear();
    let rows = pairs
        .iter()
        .map(|p| {
            let id = *next_id;
            *next_id += 1;
            PairRow {
                id,
                seg_a: inp.segs[p.a].seg.id,
                seg_b: inp.segs[p.b].seg.id,
                gap: p.gap,
                overlap: p.overlap,
                overlap_ratio: p.overlap_ratio,
                axis: p.axis,
                band: p.band,
                ladder: p.ladder,
            }
        })
        .collect();
    (feats, stats, rows)
}

/// Two features with the same key are one thing found twice (a window seen from both of its ends).
fn dedupe(v: &mut Vec<Feature>) {
    let mut seen = HashSet::new();
    v.retain(|f| {
        let mut ids: Vec<u64> = f.members.iter().map(|m| m.prim).collect();
        ids.sort_unstable();
        seen.insert((f.kind.clone(), f.subkind.clone(), ids))
    });
}

// ---- walls ----

struct Iv {
    band: usize,
    theta: f64,
    pair: usize,
}

fn canon_theta(a: f64) -> f64 {
    // Near-horizontal lines from both sides of 0/180 get one canonical angle in [-0.5, 179.5).
    if a >= 179.5 { a - 180.0 } else { a }
}

fn wall_runs(inp: &Input, par: &Params, pairs: &[crate::derive::Pair], feats: &mut Vec<Feature>, next_id: &mut u64) -> Vec<Run> {
    let rules = par.rules;
    let mut ivs: Vec<Iv> = pairs
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let s = inp.segs[p.a].seg;
            Iv { band: p.band, theta: canon_theta(s.angle), pair: i }
        })
        .collect();
    ivs.sort_by(|a, b| a.band.cmp(&b.band).then(a.theta.total_cmp(&b.theta)));
    let mut runs: Vec<Run> = Vec::new();
    let mut i = 0;
    let cluster_tol = (rules.pair.tolerance_mm * 0.75).max(4.0);
    while i < ivs.len() {
        // A group of pairs with the same band and (within half a degree) the same direction.
        let mut j = i + 1;
        while j < ivs.len() && ivs[j].band == ivs[i].band && (ivs[j].theta - ivs[j - 1].theta) <= 0.5 {
            j += 1;
        }
        let group = &ivs[i..j];
        let th = group.iter().map(|g| g.theta).sum::<f64>() / group.len() as f64;
        let (st, ct) = th.to_radians().sin_cos();
        let (u, n) = ([ct, st], [-st, ct]);
        // Per pair: lateral position and axial interval of its centre line.
        let mut rows: Vec<(f64, f64, f64, usize)> = group
            .iter()
            .map(|g| {
                let a = pairs[g.pair].axis;
                let d = ((a[0] + a[2]) * n[0] + (a[1] + a[3]) * n[1]) / 2.0;
                let (t0, t1) = (a[0] * u[0] + a[1] * u[1], a[2] * u[0] + a[3] * u[1]);
                (d, t0.min(t1), t0.max(t1), g.pair)
            })
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut k = 0;
        while k < rows.len() {
            let mut m = k + 1;
            while m < rows.len() && rows[m].0 - rows[m - 1].0 <= cluster_tol {
                m += 1;
            }
            let mut lane: Vec<&(f64, f64, f64, usize)> = rows[k..m].iter().collect();
            lane.sort_by(|a, b| a.1.total_cmp(&b.1));
            let mut c = 0;
            while c < lane.len() {
                let (mut t0, mut t1) = (lane[c].1, lane[c].2);
                let mut members = vec![lane[c]];
                let mut e = c + 1;
                while e < lane.len() && lane[e].1 <= t1 + rules.pair.join_gap_mm {
                    t1 = t1.max(lane[e].2);
                    members.push(lane[e]);
                    e += 1;
                }
                t0 = t0.min(members.iter().map(|m| m.1).fold(f64::INFINITY, f64::min));
                make_run(inp, par, pairs, &members, ivs[i].band, th, u, n, t0, t1, feats, &mut runs, next_id);
                c = e;
            }
            k = m;
        }
        i = j;
    }
    runs
}

#[allow(clippy::too_many_arguments)]
fn make_run(
    inp: &Input,
    par: &Params,
    pairs: &[crate::derive::Pair],
    members: &[&(f64, f64, f64, usize)],
    band: usize,
    theta: f64,
    u: [f64; 2],
    n: [f64; 2],
    t0: f64,
    t1: f64,
    feats: &mut Vec<Feature>,
    runs: &mut Vec<Run>,
    next_id: &mut u64,
) {
    let rules = par.rules;
    let Some(wt) = rules.wall_types.get(band) else { return };
    let length = t1 - t0;
    if length < rules.pair.min_run_mm {
        return;
    }
    let wsum: f64 = members.iter().map(|m| (m.2 - m.1).max(1.0)).sum();
    let d = members.iter().map(|m| m.0 * (m.2 - m.1).max(1.0)).sum::<f64>() / wsum;
    let gap = members.iter().map(|m| pairs[m.3].gap * (m.2 - m.1).max(1.0)).sum::<f64>() / wsum;
    let nominal = wt.thickness(par.system);
    let mut face_idx: Vec<usize> = Vec::new();
    for m in members {
        face_idx.push(pairs[m.3].a);
        face_idx.push(pairs[m.3].b);
    }
    face_idx.sort_unstable();
    face_idx.dedup();
    let ev = &rules.evidence;
    let mut conf = ev.base_wall;
    let named = face_idx.iter().filter(|&&i| glob_any(&ev.layer_wall, &inp.segs[i].prim.layer)).count();
    conf += ev.layer_match * named as f64 / face_idx.len().max(1) as f64;
    if length >= ev.long_run * nominal {
        conf += ev.long_run_factor;
    } else {
        conf -= ev.short_penalty;
    }
    let ladder = members.iter().filter(|m| pairs[m.3].ladder).count();
    if ladder * 2 > members.len() {
        conf -= ev.ladder_penalty;
    }
    if face_idx.iter().any(|&i| inp.segs[i].prim.in_block()) {
        conf -= ev.block_penalty;
    }
    // Both faces are sides of the same small closed rectangle: a column section, not a wall.
    let prims: HashSet<u64> = face_idx.iter().map(|&i| inp.segs[i].prim.id).collect();
    if prims.len() == 1 && length < ev.column_max_mm && face_idx.iter().all(|&i| inp.segs[i].prim.closed) {
        conf -= ev.column_penalty;
    }
    let dashed = face_idx.iter().filter(|&&i| is_dashed(&inp.segs[i].prim.linetype)).count();
    if dashed * 2 > face_idx.len() {
        conf -= ev.dashed_penalty;
    }
    let p0 = [u[0] * t0 + n[0] * d, u[1] * t0 + n[1] * d];
    let p1 = [u[0] * t1 + n[0] * d, u[1] * t1 + n[1] * d];
    let key = feature_key("wall", &wt.id, p0, p1, nominal);
    let id = *next_id;
    *next_id += 1;
    let mut mem = members_of(&inp.segs, &face_idx, "face");
    mem.sort_by_key(|m| m.prim);
    feats.push(Feature {
        id,
        kind: "wall".into(),
        subkind: wt.id.clone(),
        confidence: conf.clamp(0.0, 1.0),
        rule: format!("wall.{}", wt.id),
        rules_version: rules.version,
        x1: p0[0],
        y1: p0[1],
        x2: p1[0],
        y2: p1[1],
        width: nominal,
        depth: gap,
        length,
        props: json!({ "measured_thickness_mm": gap, "pairs": members.len(), "ladder_pairs": ladder, "joins": [], "dashed": dashed * 2 > face_idx.len(), "beam_named": face_idx.iter().any(|&i| glob_any(&rules.beam.layer_patterns, &inp.segs[i].prim.layer)), "in_block": face_idx.iter().any(|&i| inp.segs[i].prim.in_block()) }),
        members: mem,
        key,
    });
    runs.push(Run { feat: feats.len() - 1, u, n, d, t0, t1, half: gap / 2.0, theta });
}

/// Corners and T joins: where the end of a run meets another run.
fn wall_joins(par: &Params, runs: &[Run], feats: &mut [Feature]) {
    let tol = par.rules.pair.node_tol_mm;
    let cell = 3000.0;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let cell_of = |v: f64| (v / cell).floor() as i64;
    for (i, r) in runs.iter().enumerate() {
        let b = feats[r.feat].bbox();
        for cx in cell_of(b[0] - tol)..=cell_of(b[2] + tol) {
            for cy in cell_of(b[1] - tol)..=cell_of(b[3] + tol) {
                grid.entry((cx, cy)).or_default().push(i);
            }
        }
    }
    let mut joins: Vec<Vec<serde_json::Value>> = vec![Vec::new(); runs.len()];
    let mut joined_ends = vec![0u8; runs.len()];
    for (i, r) in runs.iter().enumerate() {
        for (end, s) in [(0u8, r.t0), (1u8, r.t1)] {
            let p = [r.u[0] * s + r.n[0] * r.d, r.u[1] * s + r.n[1] * r.d];
            let Some(cands) = grid.get(&(cell_of(p[0]), cell_of(p[1]))) else { continue };
            let mut best: Option<(f64, &'static str, usize)> = None;
            for &j in cands {
                if j == i {
                    continue;
                }
                let o = &runs[j];
                let dth = (r.theta - o.theta).abs();
                let dth = dth.min(180.0 - dth);
                if dth < 5.0 {
                    continue; // collinear continuation or a parallel neighbour, not a join
                }
                let (s2, lat) = o.locate(p);
                if lat.abs() > o.half + tol + r.half {
                    continue;
                }
                if s2 < o.t0 - tol - r.half || s2 > o.t1 + tol + r.half {
                    continue;
                }
                let near_end = (s2 - o.t0).abs().min((s2 - o.t1).abs()) <= o.half + r.half + tol;
                let kind = if near_end { "corner" } else { "T" };
                let score = lat.abs();
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, kind, j));
                }
            }
            if let Some((_, kind, j)) = best {
                joins[i].push(json!({ "end": end, "kind": kind, "with": feats[runs[j].feat].key }));
                joined_ends[i] += 1;
            }
        }
    }
    // A run lying wholly inside a thicker, parallel run is not a wall of its own (window sill lines).
    let mut nested = vec![false; runs.len()];
    for (i, r) in runs.iter().enumerate() {
        let (mid_s, mid_o) = ((r.t0 + r.t1) / 2.0, r.d);
        let p = [r.u[0] * mid_s + r.n[0] * mid_o, r.u[1] * mid_s + r.n[1] * mid_o];
        let Some(cands) = grid.get(&(cell_of(p[0]), cell_of(p[1]))) else { continue };
        for &j in cands {
            let o = &runs[j];
            if j == i || o.half <= r.half + 1.0 {
                continue;
            }
            let dth = (r.theta - o.theta).abs();
            if dth.min(180.0 - dth) > 2.0 {
                continue;
            }
            let (s0, l0) = o.locate([r.u[0] * r.t0 + r.n[0] * r.d, r.u[1] * r.t0 + r.n[1] * r.d]);
            let (s1, l1) = o.locate([r.u[0] * r.t1 + r.n[0] * r.d, r.u[1] * r.t1 + r.n[1] * r.d]);
            let (lo, hi) = (s0.min(s1), s0.max(s1));
            if l0.abs() <= o.half && l1.abs() <= o.half && lo >= o.t0 - tol && hi <= o.t1 + tol {
                nested[i] = true;
                break;
            }
        }
    }
    for (i, r) in runs.iter().enumerate() {
        let f = &mut feats[r.feat];
        if nested[i] {
            f.confidence = (f.confidence - par.rules.evidence.nested_penalty).max(0.0);
            f.props["nested"] = json!(true);
        }
        if let Some(a) = f.props.get_mut("joins") {
            *a = serde_json::Value::Array(std::mem::take(&mut joins[i]));
        }
        let bonus = par.rules.evidence.joined * f64::from(joined_ends[i]) / 2.0;
        f.confidence = (f.confidence + bonus).clamp(0.0, 1.0);
    }
}

/// Lines across a wall (end caps and jambs): as long as the wall is thick, square to it, with both ends on its faces.
fn wall_caps(inp: &Input, par: &Params, runs: &[Run], feats: &mut [Feature], seg_tree: &RTree<Boxed>) {
    let tol = par.rules.pair.tolerance_mm;
    let min = par.rules.evidence.classify_min;
    for r in runs {
        if feats[r.feat].confidence < min {
            continue;
        }
        let t = feats[r.feat].depth;
        let b = feats[r.feat].bbox();
        let have: HashSet<u64> = feats[r.feat].members.iter().map(|m| m.prim).collect();
        let mut add: Vec<Member> = Vec::new();
        for si in window(seg_tree, b[0] - tol, b[1] - tol, b[2] + tol, b[3] + tol) {
            let sr = &inp.segs[si];
            let s = sr.seg;
            if sr.prim.in_block() || (s.len - t).abs() > tol * 1.5 || have.contains(&sr.prim.id) {
                continue;
            }
            let dth = (s.angle - r.theta).abs();
            if (dth.min(180.0 - dth) - 90.0).abs() > 2.0 {
                continue;
            }
            let (sa, oa) = r.locate([s.x1, s.y1]);
            let (sb, ob) = r.locate([s.x2, s.y2]);
            let faces = (oa.abs() - r.half).abs() <= tol && (ob.abs() - r.half).abs() <= tol && oa.signum() != ob.signum();
            let along = (sa - sb).abs() <= tol && sa >= r.t0 - tol && sa <= r.t1 + tol;
            if faces && along && !add.iter().any(|m| m.prim == sr.prim.id) {
                add.push(Member { prim: sr.prim.id, handle: sr.prim.handle, role: "cap".into() });
            }
        }
        feats[r.feat].members.extend(add);
    }
}

// ---- openings ----

fn block_of(prim: &Prim) -> &str {
    prim.block_path.split('/').next().unwrap_or("")
}

fn okind(ins: &InsertRow) -> Option<&str> {
    ins.attrs.get("OKIND").map(String::as_str)
}

fn doors_by_block(inp: &Input, par: &Params, out: &mut Vec<Feature>, next_id: &mut u64) {
    let rules = par.rules;
    for (ins, prim) in &inp.inserts {
        let named = glob_any(&rules.door.block_patterns, &ins.block);
        let kind = okind(ins);
        if !(kind == Some("door") || (named && kind != Some("window"))) {
            continue;
        }
        let w = ins
            .attrs
            .get("OWIDTH")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v * par.scale)
            .unwrap_or(ins.width.max(ins.depth));
        let conf = if kind == Some("door") { 0.95 } else { 0.85 };
        out.push(opening(
            "door",
            "block",
            "door.block",
            rules.version,
            [ins.x, ins.y],
            [ins.x, ins.y],
            w,
            0.0,
            conf,
            vec![Member { prim: prim.id, handle: prim.handle, role: "block".into() }],
            json!({ "block": &*ins.block }),
        ));
        bump_id(out, next_id);
    }
}

fn windows_by_block(inp: &Input, par: &Params, out: &mut Vec<Feature>, next_id: &mut u64) {
    let rules = par.rules;
    for (ins, prim) in &inp.inserts {
        let named = glob_any(&rules.window.block_patterns, &ins.block);
        let kind = okind(ins);
        if !(kind == Some("window") || (named && kind != Some("door"))) {
            continue;
        }
        let w = ins
            .attrs
            .get("OWIDTH")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v * par.scale)
            .unwrap_or(ins.width.max(ins.depth));
        let conf = if kind == Some("window") { 0.95 } else { 0.85 };
        out.push(opening(
            "window",
            "block",
            "window.block",
            rules.version,
            [ins.x, ins.y],
            [ins.x, ins.y],
            w,
            0.0,
            conf,
            vec![Member { prim: prim.id, handle: prim.handle, role: "block".into() }],
            json!({ "block": &*ins.block }),
        ));
        bump_id(out, next_id);
    }
}

fn bump_id(out: &mut [Feature], next_id: &mut u64) {
    if let Some(f) = out.last_mut() {
        f.id = *next_id;
        *next_id += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn opening(
    kind: &str,
    sub: &str,
    rule: &str,
    version: u32,
    a: [f64; 2],
    b: [f64; 2],
    width: f64,
    depth: f64,
    conf: f64,
    members: Vec<Member>,
    props: serde_json::Value,
) -> Feature {
    Feature {
        id: 0,
        kind: kind.into(),
        subkind: sub.into(),
        confidence: conf.clamp(0.0, 1.0),
        rule: rule.into(),
        rules_version: version,
        x1: a[0],
        y1: a[1],
        x2: b[0],
        y2: b[1],
        width,
        depth,
        length: width,
        props,
        members,
        key: feature_key(kind, sub, a, b, width),
    }
}

/// Top-level entities that are door or window symbols (by attribute or by block name): their contents are the
/// symbol, not walls or separate openings.
fn opening_owners(inp: &Input, par: &Params) -> HashSet<u64> {
    inp.inserts
        .iter()
        .filter(|(i, _)| {
            !i.nested
                && (matches!(okind(i), Some("door") | Some("window"))
                    || glob_any(&par.rules.door.block_patterns, &i.block)
                    || glob_any(&par.rules.window.block_patterns, &i.block))
        })
        .map(|(_, p)| p.handle)
        .collect()
}

fn in_symbol_block(prim: &Prim, par: &Params, owners: &HashSet<u64>) -> bool {
    if !prim.in_block() {
        return false;
    }
    if owners.contains(&prim.handle) {
        return true;
    }
    let b = block_of(prim);
    glob_any(&par.rules.door.block_patterns, b) || glob_any(&par.rules.window.block_patterns, b)
}

#[allow(clippy::too_many_arguments)]
fn doors_by_swing(
    inp: &Input,
    par: &Params,
    runs: &[Run],
    run_tree: &RTree<Boxed>,
    feats: &[Feature],
    seg_tree: &RTree<Boxed>,
    owners: &HashSet<u64>,
    out: &mut Vec<Feature>,
    next_id: &mut u64,
) {
    let rules = par.rules;
    let dr = &rules.door;
    let tol = dr.hinge_tol_mm;
    let min_conf = rules.evidence.classify_min;
    let on_wall = |p: [f64; 2], slack: f64| -> Option<usize> {
        window(run_tree, p[0] - 400.0, p[1] - 400.0, p[0] + 400.0, p[1] + 400.0)
            .filter(|&ri| feats[runs[ri].feat].confidence >= min_conf && runs[ri].contains(p, slack))
            .min_by(|&a, &b| runs[a].locate(p).1.abs().total_cmp(&runs[b].locate(p).1.abs()))
    };
    for (arc, prim) in &inp.arcs {
        let sweep = arc.sweep();
        if (sweep - 90.0).abs() > dr.sweep_tol_deg || arc.r < dr.radius_min_mm || arc.r > dr.radius_max_mm {
            continue;
        }
        if in_symbol_block(prim, par, owners) {
            continue; // the block is the door
        }
        let hinge = [arc.cx, arc.cy];
        let (e0, e1) = (arc.point_at(arc.a0), arc.point_at(arc.a1));
        let wall = on_wall(hinge, tol);
        let end_on = on_wall(e0, tol * 2.0).or_else(|| on_wall(e1, tol * 2.0));
        // The leaf: a line from the hinge to one end of the swing, as long as the radius.
        let mut leaf: Option<usize> = None;
        for si in window(seg_tree, hinge[0] - tol, hinge[1] - tol, hinge[0] + tol, hinge[1] + tol) {
            let s = inp.segs[si].seg;
            if (s.len - arc.r).abs() > arc.r * 0.08 {
                continue;
            }
            let near = |x: f64, y: f64| (x - hinge[0]).hypot(y - hinge[1]) <= tol;
            if near(s.x1, s.y1) || near(s.x2, s.y2) {
                leaf = Some(si);
                break;
            }
        }
        let mut conf = 0.25 + if leaf.is_some() { 0.1 } else { 0.0 };
        if wall.is_some() {
            conf = 0.40 + 0.30 + if end_on.is_some() { 0.20 } else { 0.0 } + if leaf.is_some() { 0.10 } else { 0.0 };
        }
        if glob_any(&rules.evidence.layer_door, &prim.layer) {
            conf += rules.evidence.layer_match;
        }
        let mut members = vec![Member { prim: prim.id, handle: prim.handle, role: "swing".into() }];
        if let Some(l) = leaf {
            let p = inp.segs[l].prim;
            members.push(Member { prim: p.id, handle: p.handle, role: "leaf".into() });
        }
        let mut props = json!({ "radius_mm": arc.r, "sweep_deg": sweep });
        let mut depth = 0.0;
        if let Some(ri) = wall {
            let r = &runs[ri];
            let (s, _) = r.locate(hinge);
            depth = feats[r.feat].depth;
            props["wall"] = json!(feats[r.feat].key);
            props["offset_from_run_end_mm"] = json!((s - r.t0).abs().min((r.t1 - s).abs()));
        }
        out.push(opening("door", "swing", "door.swing", rules.version, hinge, e1, arc.r, depth, conf, members, props));
        bump_id(out, next_id);
    }
}

#[allow(clippy::too_many_arguments)]
fn windows_by_lines(
    inp: &Input,
    par: &Params,
    runs: &[Run],
    _run_tree: &RTree<Boxed>,
    feats: &[Feature],
    seg_tree: &RTree<Boxed>,
    owners: &HashSet<u64>,
    out: &mut Vec<Feature>,
    next_id: &mut u64,
) {
    let rules = par.rules;
    let wr = &rules.window;
    let min_conf = rules.evidence.classify_min;
    let angle_tol = rules.pair.angle_tol_deg;
    // Door hinges, so a closed door leaf is not read as a window.
    let hinges: Vec<[f64; 2]> = out.iter().filter(|f| f.kind == "door").map(|f| [f.x1, f.y1]).collect();
    for r in runs {
        let wf = &feats[r.feat];
        if wf.confidence < min_conf {
            continue;
        }
        let t = wf.depth;
        let b = wf.bbox();
        let pad = wr.width_max_mm + t;
        // Thin lines parallel to the run, inside its thickness, at its length or just beyond its ends.
        // (start along the axis, end along the axis, lateral offset, segment index)
        let mut lines: Vec<(f64, f64, f64, usize)> = Vec::new();
        for si in window(seg_tree, b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad) {
            let s = inp.segs[si].seg;
            let dth = (s.angle - r.theta).abs();
            if dth.min(180.0 - dth) > angle_tol * 2.0
                || s.len < wr.width_min_mm * 0.8
                || s.len > wr.width_max_mm * 1.1
                || in_symbol_block(inp.segs[si].prim, par, owners)
            {
                continue;
            }
            let (sa, oa) = r.locate([s.x1, s.y1]);
            let (sb, ob) = r.locate([s.x2, s.y2]);
            let o = (oa + ob) / 2.0;
            if o.abs() > t / 2.0 + rules.pair.tolerance_mm || (oa - ob).abs() > rules.pair.tolerance_mm {
                continue;
            }
            let (a, e) = (sa.min(sb), sa.max(sb));
            if a < r.t0 - pad || e > r.t1 + pad {
                continue;
            }
            lines.push((a, e, o, si));
        }
        if lines.len() < wr.lines_min {
            continue;
        }
        lines.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        // Cluster the lines that start and stop at the same places.
        let mut used = vec![false; lines.len()];
        for i in 0..lines.len() {
            if used[i] {
                continue;
            }
            let mut group = vec![i];
            used[i] = true;
            for j in (i + 1)..lines.len() {
                if lines[j].0 - lines[i].0 > 60.0 {
                    break;
                }
                if !used[j] && (lines[j].1 - lines[i].1).abs() <= 60.0 {
                    group.push(j);
                    used[j] = true;
                }
            }
            let g: Vec<&(f64, f64, f64, usize)> = group.iter().map(|&k| &lines[k]).collect();
            let (a0, e0) = (g.iter().map(|l| l.0).sum::<f64>() / g.len() as f64, g.iter().map(|l| l.1).sum::<f64>() / g.len() as f64);
            let width = e0 - a0;
            if width < wr.width_min_mm || width > wr.width_max_mm {
                continue;
            }
            let mut offs: Vec<f64> = g.iter().map(|l| l.2).collect();
            offs.sort_by(|a, b| a.total_cmp(b));
            offs.dedup_by(|a, b| (*a - *b).abs() < 12.0);
            if offs.len() < wr.lines_min || offs.len() > wr.lines_max {
                continue;
            }
            let span = offs.last().copied().unwrap_or(0.0) - offs.first().copied().unwrap_or(0.0);
            if span < wr.span_ratio * t {
                continue;
            }
            // A sill line between the two faces is what a window has and a wall does not.
            let interior = offs.iter().filter(|o| o.abs() < t / 2.0 - 20.0).count();
            // Or the faces stop here and the lines fill the gap between two wall ends.
            let mid = (a0 + e0) / 2.0;
            let in_gap = (mid < r.t0 || mid > r.t1) && ((a0 - r.t1).abs() < 80.0 || (e0 - r.t0).abs() < 80.0);
            if interior == 0 && !in_gap {
                continue;
            }
            let at = |s: f64, o: f64| [r.u[0] * s + r.n[0] * (r.d + o), r.u[1] * s + r.n[1] * (r.d + o)];
            let (pa, pb) = (at(a0, 0.0), at(e0, 0.0));
            if hinges.iter().any(|h| {
                let (s, o) = r.locate(*h);
                o.abs() <= t && s >= a0 - 100.0 && s <= e0 + 100.0
            }) {
                continue; // a door leaf in a door opening
            }
            // Wall (a run end, or the run itself) on both sides of the opening.
            let ends_ok = |x: f64| {
                runs.iter().any(|o| {
                    let dth = (o.theta - r.theta).abs();
                    dth.min(180.0 - dth) < 2.0
                        && (o.d - r.d).abs() < rules.pair.tolerance_mm * 2.0
                        && ((o.t0 - x).abs() < 80.0 || (o.t1 - x).abs() < 80.0)
                })
            };
            let framed = (ends_ok(a0) || a0 > r.t0 + 80.0) && (ends_ok(e0) || e0 < r.t1 - 80.0);
            let mut conf = 0.5 + if framed { 0.2 } else { 0.0 } + if offs.len() >= 3 { 0.1 } else { 0.0 };
            if g.iter().any(|l| glob_any(&rules.evidence.layer_window, &inp.segs[l.3].prim.layer)) {
                conf += rules.evidence.layer_match;
            }
            let idx: Vec<usize> = g.iter().map(|l| l.3).collect();
            out.push(opening(
                "window",
                "lines",
                "window.lines",
                rules.version,
                pa,
                pb,
                width,
                t,
                conf,
                members_of(&inp.segs, &idx, "sill"),
                json!({ "wall": wf.key, "lines": offs.len(), "interior_lines": interior }),
            ));
            bump_id(out, next_id);
        }
    }
}

//! Stage 4: context. A pair of parallel lines is a wall, a beam, a furniture edge or part of an
//! elevation; nothing in the pair itself says which. This stage looks at what surrounds it.
//!
//! 1. The sheet is clustered into **regions** (drawing views): geometry closer than `region.gap_mm`
//!    belongs to one view.
//! 2. Each region's wall runs are drawn onto a raster (door and window openings are closed where a
//!    free wall end faces the next wall within `room.bridge_mm`). The empty cells not connected to
//!    the border are **rooms**; the filled outline of walls and rooms is the **slab**.
//! 3. A region is a **plan** when its walls close into rooms and it has door swings; when the
//!    sheet has a plan, walls in the other regions (elevations, details) lose confidence.
//! 4. In a plan, a wall that bounds a room gains confidence, one that touches no room or divides
//!    nothing loses it, and a run that is dashed, floats, lies in or across other walls is a **beam**.
//! 5. The raster is drawn again without the beams and doubtful runs, giving the final rooms and slab;
//!    hatches are assigned to the room, wall or slab they sit in.
//!
//! The input is the base features (`classify`) and the geometry extents; the output replaces the
//! base features. Nothing is stored between runs, so an incremental update equals a full rebuild.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::model::{Feature, q};
use crate::rules::Rules;

/// Geometry of the sheet for region clustering.
pub enum Item {
    /// x1, y1, x2, y2 of a straight piece.
    Seg([f64; 4]),
    /// Extent of anything else (arc, text, insert, hatch).
    Bbox([f64; 4]),
}

pub struct HatchIn {
    pub id: u64,
    pub prim: u64,
    pub handle: u64,
    pub centre: [f64; 2],
}

pub struct CtxInput {
    pub items: Vec<Item>,
    pub hatches: Vec<HatchIn>,
}

#[derive(Clone, Debug)]
pub struct RegionRow {
    pub id: u64,
    pub bbox: [f64; 4],
    /// `plan`, `structure` (a structural layout: columns and a grid of pairs, no openings), `other` (judged not a plan because the sheet has one), `unjudged` or `empty` (no wall runs).
    pub kind: String,
    pub score: f64,
    pub walls: usize,
    pub wall_len_mm: f64,
    pub doors: usize,
    pub windows: usize,
    pub columns: usize,
    pub rooms: usize,
    pub closure: f64,
    pub items: usize,
}

#[derive(Clone, Debug)]
pub struct RoomRow {
    pub feature_id: u64,
    pub key: String,
    /// `room` or `slab`.
    pub kind: String,
    pub region: u64,
    pub area_mm2: f64,
    pub perimeter_mm: f64,
    pub outline: Vec<[f64; 2]>,
    pub hatches: usize,
}

#[derive(Clone, Debug)]
pub struct HatchLink {
    pub hatch: u64,
    pub prim: u64,
    pub handle: u64,
    /// `room`, `slab` or `wall`.
    pub kind: String,
    pub feature_key: String,
}

#[derive(Default, Clone, Debug)]
pub struct ContextStats {
    pub regions: usize,
    pub plan_regions: usize,
    pub other_regions: usize,
    pub rooms: usize,
    pub slabs: usize,
    pub beams: usize,
    pub walls_boosted: usize,
    pub walls_penalised: usize,
    pub walls_in_other: usize,
    pub hatches_assigned: usize,
    pub hatches_unassigned: usize,
    pub cells: usize,
    pub ms: u128,
}

pub struct CtxOut {
    pub features: Vec<Feature>,
    pub rooms: Vec<RoomRow>,
    pub regions: Vec<RegionRow>,
    pub links: Vec<HatchLink>,
    pub stats: ContextStats,
}

// ---- regions ----

struct Regions {
    cell: f64,
    cells: HashMap<(i64, i64), usize>,
    boxes: Vec<[f64; 4]>,
    items: Vec<usize>,
}

impl Regions {
    fn of(&self, p: [f64; 2]) -> Option<usize> {
        if !p[0].is_finite() || !p[1].is_finite() {
            return None;
        }
        let (cx, cy) = ((p[0] / self.cell).floor() as i64, (p[1] / self.cell).floor() as i64);
        if let Some(r) = self.cells.get(&(cx, cy)) {
            return Some(*r);
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(r) = self.cells.get(&(cx + dx, cy + dy)) {
                    return Some(*r);
                }
            }
        }
        None
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn item_box(it: &Item) -> [f64; 4] {
    match it {
        Item::Seg(s) => [s[0].min(s[2]), s[1].min(s[3]), s[0].max(s[2]), s[1].max(s[3])],
        Item::Bbox(b) => *b,
    }
}

fn build_regions(items: &[Item], rules: &Rules) -> Regions {
    let rr = &rules.region;
    let cell = rr.gap_mm.max(100.0);
    let mut ext = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for it in items {
        let b = item_box(it);
        if b.iter().all(|v| v.is_finite() && v.abs() < 1.0e9) {
            ext = [ext[0].min(b[0]), ext[1].min(b[1]), ext[2].max(b[2]), ext[3].max(b[3])];
        }
    }
    let empty = Regions { cell, cells: HashMap::new(), boxes: Vec::new(), items: Vec::new() };
    if ext[0] > ext[2] {
        return empty;
    }
    let (tw, th) = (ext[2] - ext[0], ext[3] - ext[1]);
    let diag = tw.hypot(th);
    // Occupied cells; a frame or ruler (a very long line, a box over most of the sheet) would join every view, so it is left out.
    let mut occupied: HashMap<(i64, i64), usize> = HashMap::new();
    let mut keys: Vec<(i64, i64)> = Vec::new();
    let mut mark = |cx: i64, cy: i64, occupied: &mut HashMap<(i64, i64), usize>| {
        occupied.entry((cx, cy)).or_insert_with(|| {
            keys.push((cx, cy));
            keys.len() - 1
        });
    };
    let cl = |v: f64| (v / cell).floor() as i64;
    for it in items {
        let b = item_box(it);
        if !b.iter().all(|v| v.is_finite() && v.abs() < 1.0e9) {
            continue;
        }
        match it {
            Item::Seg(s) => {
                let len = (s[2] - s[0]).hypot(s[3] - s[1]);
                if (len > rr.frame_share * diag && len > rr.frame_min_mm) || len / cell > 4096.0 {
                    continue;
                }
                let n = ((len / cell) * 2.0).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let t = k as f64 / n as f64;
                    mark(cl(s[0] + (s[2] - s[0]) * t), cl(s[1] + (s[3] - s[1]) * t), &mut occupied);
                }
            }
            Item::Bbox(_) => {
                if (b[2] - b[0]) > rr.frame_share * tw && (b[3] - b[1]) > rr.frame_share * th && (b[2] - b[0]).max(b[3] - b[1]) > rr.frame_min_mm {
                    continue;
                }
                let (nx, ny) = (cl(b[2]) - cl(b[0]) + 1, cl(b[3]) - cl(b[1]) + 1);
                if nx * ny > 4096 {
                    continue;
                }
                for cx in cl(b[0])..=cl(b[2]) {
                    for cy in cl(b[1])..=cl(b[3]) {
                        mark(cx, cy, &mut occupied);
                    }
                }
            }
        }
    }
    if keys.is_empty() {
        return empty;
    }
    let mut parent: Vec<usize> = (0..keys.len()).collect();
    for (i, &(cx, cy)) in keys.iter().enumerate() {
        for (dx, dy) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
            if let Some(&j) = occupied.get(&(cx + dx, cy + dy)) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    // Component boxes from the cells, ordered by position so ids do not depend on hashing.
    let mut comp: BTreeMap<usize, [i64; 4]> = BTreeMap::new();
    for (i, &(cx, cy)) in keys.iter().enumerate() {
        let r = find(&mut parent, i);
        let e = comp.entry(r).or_insert([cx, cy, cx, cy]);
        *e = [e[0].min(cx), e[1].min(cy), e[2].max(cx), e[3].max(cy)];
    }
    let mut order: Vec<(usize, [i64; 4])> = comp.into_iter().collect();
    order.sort_by_key(|(_, b)| (b[0], b[1], b[2], b[3]));
    order.truncate(rr.max_regions);
    let index: HashMap<usize, usize> = order.iter().enumerate().map(|(n, (r, _))| (*r, n)).collect();
    let mut cells = HashMap::new();
    for (i, &k) in keys.iter().enumerate() {
        let r = find(&mut parent, i);
        if let Some(&n) = index.get(&r) {
            cells.insert(k, n);
        }
    }
    let boxes: Vec<[f64; 4]> =
        order.iter().map(|(_, b)| [b[0] as f64 * cell, b[1] as f64 * cell, (b[2] + 1) as f64 * cell, (b[3] + 1) as f64 * cell]).collect();
    let mut counts = vec![0usize; boxes.len()];
    let reg = Regions { cell, cells, boxes, items: Vec::new() };
    for it in items {
        let b = item_box(it);
        if b.iter().all(|v| v.is_finite())
            && let Some(r) = reg.of([(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0])
        {
            counts[r] += 1;
        }
    }
    Regions { items: counts, ..reg }
}

// ---- raster ----

const WALL: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct WallGeom {
    p0: [f64; 2],
    p1: [f64; 2],
    u: [f64; 2],
    half: f64,
    len: f64,
}

fn wall_geom(f: &Feature) -> Option<WallGeom> {
    let (dx, dy) = (f.x2 - f.x1, f.y2 - f.y1);
    let len = dx.hypot(dy);
    if !(len.is_finite() && len > 0.0 && f.depth.is_finite() && f.depth > 0.0) {
        return None;
    }
    Some(WallGeom { p0: [f.x1, f.y1], p1: [f.x2, f.y2], u: [dx / len, dy / len], half: f.depth / 2.0, len })
}

struct Grid {
    x0: f64,
    y0: f64,
    cell: f64,
    w: usize,
    h: usize,
    wall: Vec<bool>,
}

impl Grid {
    fn ij(&self, p: [f64; 2]) -> Option<(usize, usize)> {
        let (i, j) = (((p[0] - self.x0) / self.cell).floor(), ((p[1] - self.y0) / self.cell).floor());
        if i < 0.0 || j < 0.0 || i >= self.w as f64 || j >= self.h as f64 || i.is_nan() || j.is_nan() {
            return None;
        }
        Some((i as usize, j as usize))
    }
    fn at(&self, i: usize, j: usize) -> usize {
        j * self.w + i
    }
    /// Fill the cells whose centres lie inside a convex quad.
    fn fill_quad(&mut self, q: [[f64; 2]; 4]) {
        let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &q {
            ymin = ymin.min(p[1]);
            ymax = ymax.max(p[1]);
        }
        let j0 = (((ymin - self.y0) / self.cell).floor().max(0.0)) as usize;
        let j1 = ((((ymax - self.y0) / self.cell).ceil()).min(self.h as f64 - 1.0).max(0.0)) as usize;
        for j in j0..=j1 {
            let yc = self.y0 + (j as f64 + 0.5) * self.cell;
            let (mut xl, mut xr) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..4 {
                let (a, b) = (q[k], q[(k + 1) % 4]);
                if (a[1] <= yc && b[1] > yc) || (b[1] <= yc && a[1] > yc) {
                    let t = (yc - a[1]) / (b[1] - a[1]);
                    let x = a[0] + (b[0] - a[0]) * t;
                    xl = xl.min(x);
                    xr = xr.max(x);
                }
            }
            if xl > xr {
                continue;
            }
            let i0 = (((xl - self.x0) / self.cell - 0.5).ceil().max(0.0)) as usize;
            let i1f = ((xr - self.x0) / self.cell - 0.5).floor();
            if i1f < 0.0 {
                continue;
            }
            let i1 = (i1f as usize).min(self.w - 1);
            for i in i0..=i1 {
                let n = self.at(i, j);
                self.wall[n] = true;
            }
        }
    }
    fn fill_band(&mut self, a: [f64; 2], b: [f64; 2], u: [f64; 2], half: f64) {
        let n = [-u[1], u[0]];
        self.fill_quad([
            [a[0] + n[0] * half, a[1] + n[1] * half],
            [b[0] + n[0] * half, b[1] + n[1] * half],
            [b[0] - n[0] * half, b[1] - n[1] * half],
            [a[0] - n[0] * half, a[1] - n[1] * half],
        ]);
    }
}

fn rasterize(walls: &[WallGeom], rr: &crate::rules::RoomRule) -> Option<Grid> {
    if walls.is_empty() {
        return None;
    }
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for w in walls {
        for p in [w.p0, w.p1] {
            b = [b[0].min(p[0] - w.half), b[1].min(p[1] - w.half), b[2].max(p[0] + w.half), b[3].max(p[1] + w.half)];
        }
    }
    if !b.iter().all(|v| v.is_finite() && v.abs() < 1.0e9) {
        return None;
    }
    let margin = rr.cell_mm * 3.0;
    let (wmm, hmm) = (b[2] - b[0] + 2.0 * margin, b[3] - b[1] + 2.0 * margin);
    let mut cell = rr.cell_mm;
    let cells = (wmm / cell) * (hmm / cell);
    if cells > rr.max_cells as f64 {
        cell *= (cells / rr.max_cells as f64).sqrt();
    }
    let (w, h) = ((wmm / cell).ceil() as usize + 1, (hmm / cell).ceil() as usize + 1);
    if w.saturating_mul(h) > rr.max_cells.saturating_mul(2) {
        return None;
    }
    let mut g = Grid { x0: b[0] - margin, y0: b[1] - margin, cell, w, h, wall: vec![false; w * h] };
    let infl = cell * 0.5;
    for wl in walls {
        // Each end is padded by the half thickness, which fills the corner where two runs meet.
        let pad = wl.half + infl;
        let a = [wl.p0[0] - wl.u[0] * pad, wl.p0[1] - wl.u[1] * pad];
        let e = [wl.p1[0] + wl.u[0] * pad, wl.p1[1] + wl.u[1] * pad];
        g.fill_band(a, e, wl.u, wl.half + infl);
    }
    // Close openings: from each free end, look along the wall for the next wall within the bridge distance.
    let mut bridges: Vec<([f64; 2], [f64; 2], [f64; 2], f64)> = Vec::new();
    let step = cell * 0.5;
    for wl in walls {
        for (p, dir) in [(wl.p1, 1.0), (wl.p0, -1.0)] {
            let u = [wl.u[0] * dir, wl.u[1] * dir];
            let start = wl.half + cell;
            let mut s = start;
            let limit = wl.half + rr.bridge_mm;
            let mut hit: Option<f64> = None;
            while s <= limit {
                let q = [p[0] + u[0] * s, p[1] + u[1] * s];
                match g.ij(q) {
                    Some((i, j)) => {
                        if g.wall[g.at(i, j)] {
                            hit = Some(s);
                            break;
                        }
                    }
                    None => break,
                }
                s += step;
            }
            if let Some(hs) = hit
                && hs > start
            {
                bridges.push(([p[0] + u[0] * wl.half, p[1] + u[1] * wl.half], [p[0] + u[0] * (hs + cell), p[1] + u[1] * (hs + cell)], u, wl.half));
            }
        }
    }
    for (a, b, u, half) in bridges {
        g.fill_band(a, b, u, half + infl);
    }
    Some(g)
}

struct Comp {
    cells: usize,
    edges: usize,
    bbox: [usize; 4],
    sum: [f64; 2],
    valid: bool,
}

struct Analysis {
    /// 0: reaches the border (outside), WALL: wall cell, otherwise 1-based component id.
    label: Vec<u32>,
    rooms: Vec<Comp>,
    /// Component id of the filled shape (walls and enclosed rooms), 0 for outside.
    filled: Vec<u32>,
    slabs: Vec<Comp>,
}

fn analyse(g: &Grid, rr: &crate::rules::RoomRule) -> Analysis {
    let (w, h) = (g.w, g.h);
    let mut label = vec![0u32; w * h];
    for (n, wl) in g.wall.iter().enumerate() {
        if *wl {
            label[n] = WALL;
        }
    }
    // Outside: empty cells connected to the border.
    let mut stack: Vec<usize> = Vec::new();
    let mut seen = vec![false; w * h];
    for i in 0..w {
        for j in [0, h - 1] {
            let n = g.at(i, j);
            if !g.wall[n] && !seen[n] {
                seen[n] = true;
                stack.push(n);
            }
        }
    }
    for j in 0..h {
        for i in [0, w - 1] {
            let n = g.at(i, j);
            if !g.wall[n] && !seen[n] {
                seen[n] = true;
                stack.push(n);
            }
        }
    }
    let neighbours = |n: usize| -> [Option<usize>; 4] {
        let (i, j) = (n % w, n / w);
        [
            if i > 0 { Some(n - 1) } else { None },
            if i + 1 < w { Some(n + 1) } else { None },
            if j > 0 { Some(n - w) } else { None },
            if j + 1 < h { Some(n + w) } else { None },
        ]
    };
    while let Some(n) = stack.pop() {
        for m in neighbours(n).into_iter().flatten() {
            if !g.wall[m] && !seen[m] {
                seen[m] = true;
                stack.push(m);
            }
        }
    }
    // Rooms: the remaining empty cells, one component each.
    let cell2 = g.cell * g.cell;
    let mut rooms: Vec<Comp> = Vec::new();
    for start in 0..w * h {
        if seen[start] || g.wall[start] {
            continue;
        }
        let id = rooms.len() as u32 + 1;
        let mut c = Comp { cells: 0, edges: 0, bbox: [usize::MAX, usize::MAX, 0, 0], sum: [0.0, 0.0], valid: false };
        seen[start] = true;
        stack.push(start);
        while let Some(n) = stack.pop() {
            label[n] = id;
            let (i, j) = (n % w, n / w);
            c.cells += 1;
            c.bbox = [c.bbox[0].min(i), c.bbox[1].min(j), c.bbox[2].max(i), c.bbox[3].max(j)];
            c.sum[0] += i as f64 + 0.5;
            c.sum[1] += j as f64 + 0.5;
            for m in neighbours(n) {
                match m {
                    Some(m) if !g.wall[m] => {
                        if !seen[m] {
                            seen[m] = true;
                            stack.push(m);
                        }
                    }
                    _ => c.edges += 1,
                }
            }
        }
        let area = c.cells as f64 * cell2;
        c.valid = area >= rr.min_area_mm2 && area <= rr.max_area_mm2;
        rooms.push(c);
    }
    // Slab: the filled shape (everything not outside), one component each.
    let mut filled = vec![0u32; w * h];
    let mut slabs: Vec<Comp> = Vec::new();
    let mut vis = vec![false; w * h];
    for start in 0..w * h {
        if vis[start] || label[start] == 0 {
            continue;
        }
        let id = slabs.len() as u32 + 1;
        let mut c = Comp { cells: 0, edges: 0, bbox: [usize::MAX, usize::MAX, 0, 0], sum: [0.0, 0.0], valid: false };
        vis[start] = true;
        stack.push(start);
        let mut has_room = false;
        while let Some(n) = stack.pop() {
            filled[n] = id;
            let (i, j) = (n % w, n / w);
            c.cells += 1;
            c.bbox = [c.bbox[0].min(i), c.bbox[1].min(j), c.bbox[2].max(i), c.bbox[3].max(j)];
            if label[n] != WALL && rooms.get(label[n] as usize - 1).is_some_and(|r| r.valid) {
                has_room = true;
            }
            for m in neighbours(n) {
                match m {
                    Some(m) if label[m] != 0 => {
                        if !vis[m] {
                            vis[m] = true;
                            stack.push(m);
                        }
                    }
                    _ => c.edges += 1,
                }
            }
        }
        c.valid = has_room && c.cells as f64 * cell2 >= rr.slab_min_area_mm2;
        slabs.push(c);
    }
    Analysis { label, rooms, filled, slabs }
}

/// Boundary of a set of cells as a closed polygon in millimetres (the largest loop), simplified to about one cell.
fn outline(g: &Grid, member: &dyn Fn(usize) -> bool, bbox: [usize; 4]) -> Vec<[f64; 2]> {
    type V = (usize, usize);
    let mut edges: Vec<(V, V)> = Vec::new();
    for j in bbox[1]..=bbox[3] {
        for i in bbox[0]..=bbox[2] {
            if !member(g.at(i, j)) {
                continue;
            }
            let on = |di: i64, dj: i64| -> bool {
                let (ni, nj) = (i as i64 + di, j as i64 + dj);
                ni >= 0 && nj >= 0 && (ni as usize) < g.w && (nj as usize) < g.h && member(g.at(ni as usize, nj as usize))
            };
            if !on(0, -1) {
                edges.push(((i, j), (i + 1, j)));
            }
            if !on(1, 0) {
                edges.push(((i + 1, j), (i + 1, j + 1)));
            }
            if !on(0, 1) {
                edges.push(((i + 1, j + 1), (i, j + 1)));
            }
            if !on(-1, 0) {
                edges.push(((i, j + 1), (i, j)));
            }
        }
    }
    let mut from: HashMap<V, Vec<usize>> = HashMap::new();
    for (k, e) in edges.iter().enumerate() {
        from.entry(e.0).or_default().push(k);
    }
    let mut used = vec![false; edges.len()];
    let mut best: Vec<V> = Vec::new();
    let mut best_area = 0.0f64;
    for s in 0..edges.len() {
        if used[s] {
            continue;
        }
        let mut ring: Vec<V> = Vec::new();
        let mut k = s;
        loop {
            used[k] = true;
            ring.push(edges[k].0);
            let end = edges[k].1;
            let next = from.get(&end).and_then(|v| v.iter().copied().find(|&x| !used[x]));
            match next {
                Some(n) if n != s => k = n,
                _ => break,
            }
            if ring.len() > edges.len() {
                break;
            }
        }
        let area: f64 = (0..ring.len())
            .map(|a| {
                let (p, r) = (ring[a], ring[(a + 1) % ring.len()]);
                p.0 as f64 * r.1 as f64 - r.0 as f64 * p.1 as f64
            })
            .sum::<f64>()
            / 2.0;
        if area.abs() > best_area {
            best_area = area.abs();
            best = ring;
        }
    }
    let pts: Vec<[f64; 2]> = best.iter().map(|v| [g.x0 + v.0 as f64 * g.cell, g.y0 + v.1 as f64 * g.cell]).collect();
    simplify_ring(&pts, g.cell)
}

fn simplify_ring(pts: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    // Drop collinear points first (the rectilinear raster has many).
    let n = pts.len();
    if n < 4 {
        return pts.to_vec();
    }
    let mut v: Vec<[f64; 2]> = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross.abs() > 1e-9 {
            v.push(b);
        }
    }
    if v.len() <= 4 {
        return v;
    }
    // Douglas-Peucker on the ring, split at the point farthest from the first.
    let far = (1..v.len())
        .max_by(|&a, &b| {
            let da = (v[a][0] - v[0][0]).hypot(v[a][1] - v[0][1]);
            let db = (v[b][0] - v[0][0]).hypot(v[b][1] - v[0][1]);
            da.total_cmp(&db)
        })
        .unwrap_or(1);
    let mut keep = vec![false; v.len()];
    keep[0] = true;
    keep[far] = true;
    let mut stack = vec![(0usize, far), (far, v.len())];
    while let Some((a, b)) = stack.pop() {
        let pa = v[a];
        let pb = v[b % v.len()];
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let l = dx.hypot(dy);
        let mut worst = (0.0, a);
        for (k, &p) in v.iter().enumerate().take(b).skip(a + 1) {
            let d = if l > 0.0 { ((p[0] - pa[0]) * dy - (p[1] - pa[1]) * dx).abs() / l } else { (p[0] - pa[0]).hypot(p[1] - pa[1]) };
            if d > worst.0 {
                worst = (d, k);
            }
        }
        if worst.0 > tol && worst.1 > a {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    v.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// What the cells on both sides of a wall say about it.
#[derive(Default, Clone, Copy)]
struct Sides {
    valid: usize,
    separating: usize,
    touches_room: usize,
}

fn sample_sides(g: &Grid, an: &Analysis, w: &WallGeom) -> Sides {
    let mut s = Sides::default();
    let n = [-w.u[1], w.u[0]];
    let off = w.half + g.cell * 1.5;
    let step = (g.cell * 2.0).max(100.0);
    let count = ((w.len / step).floor() as usize).clamp(1, 400);
    for k in 0..count {
        let t = (k as f64 + 0.5) * w.len / count as f64;
        let c = [w.p0[0] + w.u[0] * t, w.p0[1] + w.u[1] * t];
        let side = |sign: f64| -> Option<u32> {
            let p = [c[0] + n[0] * off * sign, c[1] + n[1] * off * sign];
            match g.ij(p) {
                None => Some(0),
                Some((i, j)) => {
                    let l = an.label[g.at(i, j)];
                    if l == WALL { None } else { Some(l) }
                }
            }
        };
        let (a, b) = (side(1.0), side(-1.0));
        let (Some(a), Some(b)) = (a, b) else { continue };
        s.valid += 1;
        if a != b {
            s.separating += 1;
        }
        let room = |l: u32| l != 0 && an.rooms.get(l as usize - 1).is_some_and(|r| r.valid);
        if room(a) || room(b) {
            s.touches_room += 1;
        }
    }
    s
}

fn key_of(kind: &str, sub: &str, b: [f64; 4], area: f64) -> String {
    let mut h = Sha256::new();
    h.update(format!("{kind}|{sub}|{}|{}|{}|{}|{}", q(b[0]), q(b[1]), q(b[2]), q(b[3]), q(area)));
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn wall_key(kind: &str, sub: &str, a: [f64; 2], b: [f64; 2], w: f64) -> String {
    let (qa, qb) = ((q(a[0]), q(a[1])), (q(b[0]), q(b[1])));
    let (lo, hi) = if qa <= qb { (qa, qb) } else { (qb, qa) };
    let mut h = Sha256::new();
    h.update(format!("{kind}|{sub}|{}|{}|{}|{}|{}", lo.0, lo.1, hi.0, hi.1, q(w)));
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn mid(f: &Feature) -> [f64; 2] {
    [(f.x1 + f.x2) / 2.0, (f.y1 + f.y2) / 2.0]
}

fn num(f: &Feature, k: &str) -> usize {
    f.props.get(k).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)
}

fn set_prop(f: &mut Feature, k: &str, v: serde_json::Value) {
    if let Some(o) = f.props.as_object_mut() {
        o.insert(k.to_string(), v);
    }
}

fn flag(f: &Feature, k: &str) -> bool {
    f.props.get(k).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Does `a` cross wall `b` right through (both faces of `b`, with `a` going on beyond on both sides)?
fn crosses(a: &WallGeom, b: &WallGeom, tol: f64) -> bool {
    let cross = a.u[0] * b.u[1] - a.u[1] * b.u[0];
    if cross.abs() < 0.95 {
        return false; // not near perpendicular
    }
    let (dx, dy) = (b.p0[0] - a.p0[0], b.p0[1] - a.p0[1]);
    let sa = (dx * b.u[1] - dy * b.u[0]) / cross;
    let sb = (dx * a.u[1] - dy * a.u[0]) / cross;
    sa - b.half >= tol && a.len - sa - b.half >= tol && sb >= -tol && sb <= b.len + tol
}

pub fn apply(base: &[Feature], inp: &CtxInput, rules: &Rules, next_id: &mut u64) -> CtxOut {
    let t0 = std::time::Instant::now();
    let min = rules.evidence.classify_min;
    let rr = &rules.room;
    let mut stats = ContextStats::default();
    // A fixed order so the result does not depend on how the base features were produced.
    let mut order: Vec<usize> = (0..base.len()).collect();
    order.sort_by(|&a, &b| base[a].key.cmp(&base[b].key).then(base[a].kind.cmp(&base[b].kind)));
    let reg = build_regions(&inp.items, rules);
    // The last bucket takes whatever no region claims (features whose own lines were left out as a frame).
    let nreg = reg.boxes.len() + 1;
    let region_of = |p: [f64; 2]| -> usize { reg.of(p).unwrap_or(nreg - 1) };
    // Features per region: walls with a usable axis.
    let mut walls_in: Vec<Vec<usize>> = vec![Vec::new(); nreg];
    let mut doors_in = vec![0usize; nreg];
    let mut windows_in = vec![0usize; nreg];
    let mut cols_in = vec![0usize; nreg];
    for &i in &order {
        let f = &base[i];
        match f.kind.as_str() {
            "wall" if wall_geom(f).is_some() => walls_in[region_of(mid(f))].push(i),
            "door" if f.confidence >= min => doors_in[region_of([f.x1, f.y1])] += 1,
            "window" if f.confidence >= min => windows_in[region_of(mid(f))] += 1,
            "column" if f.confidence >= rules.region.structure_column_conf => cols_in[region_of(mid(f))] += 1,
            _ => {}
        }
    }
    // Pass 1: rooms from every wall candidate that is not dashed (dashed runs are beams above, not walls).
    struct Pass1 {
        sides: HashMap<usize, Sides>,
        rooms: usize,
    }
    let mut p1: Vec<Option<Pass1>> = (0..nreg).map(|_| None).collect();
    let mut region_rows: Vec<RegionRow> = Vec::new();
    for r in 0..nreg {
        let ids = &walls_in[r];
        let cand: Vec<usize> = ids.iter().copied().filter(|&i| base[i].confidence >= min && !flag(&base[i], "dashed")).collect();
        let geoms: Vec<WallGeom> = cand.iter().filter_map(|&i| wall_geom(&base[i])).collect();
        let wall_len: f64 = geoms.iter().map(|g| g.len).sum();
        let bbox = reg.boxes.get(r).copied().unwrap_or([0.0; 4]);
        let mut row = RegionRow {
            id: 0,
            bbox,
            kind: if ids.is_empty() { "empty".into() } else { "unjudged".into() },
            score: 0.0,
            walls: cand.len(),
            wall_len_mm: wall_len,
            doors: doors_in[r],
            windows: windows_in[r],
            columns: cols_in[r],
            rooms: 0,
            closure: 0.0,
            items: reg.items.get(r).copied().unwrap_or(0),
        };
        if let Some(g) = rasterize(&geoms, rr) {
            stats.cells += g.w * g.h;
            let an = analyse(&g, rr);
            let rooms = an.rooms.iter().filter(|c| c.valid).count();
            let mut sides = HashMap::new();
            let mut closed = 0.0;
            for (k, &i) in cand.iter().enumerate() {
                if let Some(w) = geoms.get(k) {
                    let s = sample_sides(&g, &an, w);
                    if s.valid > 0 && s.touches_room * 5 >= s.valid {
                        closed += w.len;
                    }
                    sides.insert(i, s);
                }
            }
            row.rooms = rooms;
            row.closure = if wall_len > 0.0 { closed / wall_len } else { 0.0 };
            row.score = row.closure + rules.region.door_weight * (row.doors.min(2) as f64) + rules.region.room_weight * (rooms.min(3) as f64);
            let openings = row.doors;
            if rooms > 0
                && wall_len >= rules.region.min_wall_mm
                && row.score >= rules.region.plan_min_score
                && openings >= rules.region.plan_min_doors
            {
                row.kind = "plan".into();
            } else if openings == 0 && row.columns >= rules.region.structure_min_columns && wall_len >= rules.region.min_wall_mm {
                row.kind = "structure".into();
            }
            p1[r] = Some(Pass1 { sides, rooms });
        }
        region_rows.push(row);
    }
    let any_plan = region_rows.iter().any(|r| r.kind == "plan");
    if any_plan {
        for r in region_rows.iter_mut() {
            if r.kind == "unjudged" && r.walls > 0 && r.wall_len_mm >= rules.region.min_wall_mm {
                r.kind = "other".into();
            }
        }
    }
    for (n, r) in region_rows.iter_mut().enumerate() {
        r.id = n as u64 + 1;
    }
    let kinds: Vec<String> = region_rows.iter().map(|r| r.kind.clone()).collect();
    let kind_of = |r: usize| kinds.get(r).map(String::as_str).unwrap_or("empty");

    // Wall runs by coarse grid for the "across" test.
    let mut geom_of: HashMap<usize, WallGeom> = HashMap::new();
    let cellg = 3000.0;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for ids in &walls_in {
        for &i in ids {
            if let Some(g) = wall_geom(&base[i]) {
                geom_of.insert(i, g);
                let b = base[i].bbox();
                for cx in (b[0] / cellg).floor() as i64..=(b[2] / cellg).floor() as i64 {
                    for cy in (b[1] / cellg).floor() as i64..=(b[3] / cellg).floor() as i64 {
                        grid.entry((cx, cy)).or_default().push(i);
                    }
                }
            }
        }
    }

    // Walls with a hatch (poche) inside their outline.
    let mut hatched: HashSet<usize> = HashSet::new();
    if !inp.hatches.is_empty() && rr.wall_hatch_bonus != 0.0 {
        let mut hg: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (k, h) in inp.hatches.iter().enumerate() {
            hg.entry(((h.centre[0] / cellg).floor() as i64, (h.centre[1] / cellg).floor() as i64)).or_default().push(k);
        }
        for (&i, g) in geom_of.iter() {
            let bb = base[i].bbox();
            'h: for cx in (bb[0] / cellg).floor() as i64..=(bb[2] / cellg).floor() as i64 {
                for cy in (bb[1] / cellg).floor() as i64..=(bb[3] / cellg).floor() as i64 {
                    for &k in hg.get(&(cx, cy)).map(Vec::as_slice).unwrap_or(&[]) {
                        let c = inp.hatches[k].centre;
                        let (dx, dy) = (c[0] - g.p0[0], c[1] - g.p0[1]);
                        let (s, o) = (dx * g.u[0] + dy * g.u[1], -dx * g.u[1] + dy * g.u[0]);
                        if s >= -rules.pair.tolerance_mm && s <= g.len + rules.pair.tolerance_mm && o.abs() <= g.half {
                            hatched.insert(i);
                            break 'h;
                        }
                    }
                }
            }
        }
    }

    // Verdict for every base feature.
    let mut out: Vec<Feature> = Vec::with_capacity(base.len());
    let mut keep_wall: HashMap<usize, bool> = HashMap::new(); // wall index -> still a wall for the second raster
    for &i in &order {
        let f = &base[i];
        let mut nf = f.clone();
        match f.kind.as_str() {
            "wall" => {
                let r = region_of(mid(f));
                let kind = kind_of(r);
                set_prop(&mut nf, "region", json!(kind));
                let mut conf = f.confidence;
                let mut is_beam = false;
                let mut score_extra = 0.0;
                if kind == "structure" {
                    score_extra = rules.beam.structure;
                }
                if kind == "other" {
                    conf -= rules.region.non_plan_penalty;
                    stats.walls_in_other += 1;
                    keep_wall.insert(i, false);
                } else {
                    let sides = p1[r].as_ref().and_then(|p| p.sides.get(&i)).copied();
                    let have_rooms = p1[r].as_ref().is_some_and(|p| p.rooms > 0);
                    let mut score = score_extra;
                    let b = &rules.beam;
                    let mut parts: Vec<&str> = Vec::new();
                    if score_extra > 0.0 {
                        parts.push("structure_plan");
                    }
                    if flag(f, "dashed") {
                        score += b.dashed;
                        parts.push("dashed");
                    }
                    if flag(f, "beam_named") {
                        score += b.layer_match;
                        parts.push("layer");
                    }
                    if flag(f, "nested") {
                        score += b.over_wall;
                        parts.push("over_wall");
                    }
                    if let Some(g) = geom_of.get(&i) {
                        let bb = f.bbox();
                        let mut seen: HashSet<usize> = HashSet::new();
                        let mut crossed = false;
                        'g: for cx in (bb[0] / cellg).floor() as i64..=(bb[2] / cellg).floor() as i64 {
                            for cy in (bb[1] / cellg).floor() as i64..=(bb[3] / cellg).floor() as i64 {
                                for &o in grid.get(&(cx, cy)).map(Vec::as_slice).unwrap_or(&[]) {
                                    if o != i
                                        && seen.insert(o)
                                        && base[o].confidence >= min
                                        && geom_of.get(&o).is_some_and(|og| crosses(g, og, rules.pair.tolerance_mm))
                                    {
                                        crossed = true;
                                        break 'g;
                                    }
                                }
                            }
                        }
                        if crossed {
                            score += b.across_wall;
                            parts.push("across_wall");
                        }
                    }
                    let mut touches = true;
                    if let (Some(s), true) = (sides, have_rooms)
                        && s.valid >= 3
                    {
                        let sep = s.separating as f64 / s.valid as f64;
                        let in_room = s.touches_room * 5 >= s.valid;
                        let joined = num(f, "joins") > 0;
                        if sep >= 0.3 {
                            // It divides one room from another room or from the outside: it bounds a room.
                            if in_room {
                                conf += rr.room_bonus;
                                stats.walls_boosted += 1;
                            } else {
                                conf -= rr.isolated_penalty;
                                stats.walls_penalised += 1;
                            }
                        } else {
                            // The same room (or the open sheet) on both sides: it divides nothing.
                            score += b.non_separating;
                            parts.push("non_separating");
                            touches = joined;
                            if !joined {
                                score += b.floating;
                                parts.push("floating");
                                conf -= rr.floating_penalty;
                                stats.walls_penalised += 1;
                            }
                            if !in_room {
                                conf -= rr.isolated_penalty;
                            }
                        }
                    }
                    if hatched.contains(&i) {
                        conf += rr.wall_hatch_bonus;
                        set_prop(&mut nf, "hatched", json!(true));
                    }
                    set_prop(&mut nf, "beam_score", json!(score));
                    if score >= b.min_score && f.length >= b.min_length_mm {
                        is_beam = true;
                        nf.kind = "beam".into();
                        nf.rule = "beam.pair".into();
                        nf.confidence = score.min(1.0);
                        set_prop(&mut nf, "beam_evidence", json!(parts));
                        nf.key = wall_key("beam", &f.subkind, [f.x1, f.y1], [f.x2, f.y2], f.width);
                        for m in nf.members.iter_mut() {
                            m.role = "face".into();
                        }
                        stats.beams += 1;
                    }
                    keep_wall.insert(i, !is_beam && touches && conf >= min);
                }
                if !is_beam {
                    nf.confidence = conf.clamp(0.0, 1.0);
                }
            }
            "column" | "door" | "window" => {
                let r = region_of(mid(f));
                if kind_of(r) == "other" {
                    nf.confidence = (nf.confidence - rules.region.non_plan_penalty).clamp(0.0, 1.0);
                    set_prop(&mut nf, "region", json!("other"));
                }
            }
            _ => {}
        }
        out.push(nf);
    }
    // Columns: beside a wall, filled with a hatch, or in a structural layout are more likely columns.
    let col_region: Vec<usize> = out.iter().map(|f| if f.kind == "column" { region_of(mid(f)) } else { 0 }).collect();
    let mut fill_idx: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, h) in inp.hatches.iter().enumerate() {
        fill_idx.entry(((h.centre[0] / cellg).floor() as i64, (h.centre[1] / cellg).floor() as i64)).or_default().push(k);
    }
    for (n, f) in out.iter_mut().enumerate() {
        if f.kind != "column" {
            continue;
        }
        if kind_of(col_region[n]) == "structure" {
            f.confidence = (f.confidence + rules.column.structure_bonus).clamp(0.0, 1.0);
        }
        let reach = f.depth.min(f.width) / 2.0;
        let (cx, cy) = ((f.x1 / cellg).floor() as i64, (f.y1 / cellg).floor() as i64);
        let filled = fill_idx
            .get(&(cx, cy))
            .is_some_and(|v| v.iter().any(|&k| (inp.hatches[k].centre[0] - f.x1).hypot(inp.hatches[k].centre[1] - f.y1) <= reach));
        if filled {
            f.confidence = (f.confidence + rules.column.fill_bonus).clamp(0.0, 1.0);
            set_prop(f, "filled", json!(true));
        }
    }
    let wall_boxes: Vec<(WallGeom, usize)> =
        order.iter().filter_map(|&i| geom_of.get(&i).map(|g| (*g, i))).filter(|(_, i)| base[*i].confidence >= min).collect();
    for f in out.iter_mut().filter(|f| f.kind == "column") {
        let c = [f.x1, f.y1];
        let reach = f.width / 2.0 * 1.5;
        let near = wall_boxes.iter().any(|(g, _)| {
            let (dx, dy) = (c[0] - g.p0[0], c[1] - g.p0[1]);
            let s = dx * g.u[0] + dy * g.u[1];
            let o = -dx * g.u[1] + dy * g.u[0];
            s >= -reach && s <= g.len + reach && o.abs() <= g.half + reach + rules.pair.tolerance_mm
        });
        if near {
            f.confidence = (f.confidence + rules.column.near_wall_bonus).clamp(0.0, 1.0);
            set_prop(f, "near_wall", json!(true));
        }
    }

    // Pass 2: the final rooms and slab from the walls that stayed walls.
    let mut rooms_out: Vec<RoomRow> = Vec::new();
    let mut links: Vec<HatchLink> = Vec::new();
    let mut hatch_by_region: Vec<Vec<usize>> = vec![Vec::new(); nreg];
    for (k, h) in inp.hatches.iter().enumerate() {
        if let Some(r) = reg.of(h.centre) {
            hatch_by_region[r].push(k);
        }
    }
    for r in 0..nreg {
        if matches!(kind_of(r), "other" | "empty" | "structure") {
            continue;
        }
        let geoms: Vec<WallGeom> =
            walls_in[r].iter().filter(|i| keep_wall.get(i).copied().unwrap_or(false)).filter_map(|i| geom_of.get(i).copied()).collect();
        let Some(g) = rasterize(&geoms, rr) else { continue };
        let an = analyse(&g, rr);
        let cell2 = g.cell * g.cell;
        let region_id = r as u64 + 1;
        let mut room_keys: HashMap<u32, String> = HashMap::new();
        let mut slab_keys: HashMap<u32, String> = HashMap::new();
        let mut room_feats: Vec<(usize, Feature, RoomRow)> = Vec::new();
        for (k, c) in an.rooms.iter().enumerate() {
            if !c.valid {
                continue;
            }
            let lab = k as u32 + 1;
            let area = c.cells as f64 * cell2;
            let bb = [
                g.x0 + c.bbox[0] as f64 * g.cell,
                g.y0 + c.bbox[1] as f64 * g.cell,
                g.x0 + (c.bbox[2] + 1) as f64 * g.cell,
                g.y0 + (c.bbox[3] + 1) as f64 * g.cell,
            ];
            let key = key_of("room", "closed", bb, area);
            let poly = outline(&g, &|n| an.label[n] == lab, c.bbox);
            let per = c.edges as f64 * g.cell;
            let id = *next_id;
            *next_id += 1;
            let conf = if area < 2.0e6 { rr.room_confidence - 0.2 } else { rr.room_confidence };
            let f = Feature {
                id,
                kind: "room".into(),
                subkind: "closed".into(),
                confidence: conf.clamp(0.0, 1.0),
                rule: "room.loop".into(),
                rules_version: rules.version,
                x1: bb[0],
                y1: bb[1],
                x2: bb[2],
                y2: bb[3],
                width: 0.0,
                depth: 0.0,
                length: per,
                props: json!({ "area_mm2": area, "perimeter_mm": per, "region": region_id, "centroid": [g.x0 + c.sum[0] / c.cells as f64 * g.cell, g.y0 + c.sum[1] / c.cells as f64 * g.cell] }),
                members: Vec::new(),
                key: key.clone(),
            };
            room_keys.insert(lab, key.clone());
            room_feats.push((
                k,
                f,
                RoomRow { feature_id: id, key, kind: "room".into(), region: region_id, area_mm2: area, perimeter_mm: per, outline: poly, hatches: 0 },
            ));
        }
        let mut slab_feats: Vec<(Feature, RoomRow)> = Vec::new();
        for (k, c) in an.slabs.iter().enumerate() {
            if !c.valid {
                continue;
            }
            let lab = k as u32 + 1;
            let area = c.cells as f64 * cell2;
            let bb = [
                g.x0 + c.bbox[0] as f64 * g.cell,
                g.y0 + c.bbox[1] as f64 * g.cell,
                g.x0 + (c.bbox[2] + 1) as f64 * g.cell,
                g.y0 + (c.bbox[3] + 1) as f64 * g.cell,
            ];
            let key = key_of("slab", "outer", bb, area);
            let poly = outline(&g, &|n| an.filled[n] == lab, c.bbox);
            let per = c.edges as f64 * g.cell;
            let id = *next_id;
            *next_id += 1;
            let rooms_in = room_feats
                .iter()
                .filter(|(rk, _, _)| {
                    an.rooms[*rk].bbox[0] >= c.bbox[0]
                        && an.rooms[*rk].bbox[2] <= c.bbox[2]
                        && an.rooms[*rk].bbox[1] >= c.bbox[1]
                        && an.rooms[*rk].bbox[3] <= c.bbox[3]
                })
                .count();
            let f = Feature {
                id,
                kind: "slab".into(),
                subkind: "outer".into(),
                confidence: rr.room_confidence.clamp(0.0, 1.0),
                rule: "slab.outer".into(),
                rules_version: rules.version,
                x1: bb[0],
                y1: bb[1],
                x2: bb[2],
                y2: bb[3],
                width: 0.0,
                depth: 0.0,
                length: per,
                props: json!({ "area_mm2": area, "perimeter_mm": per, "region": region_id, "rooms": rooms_in }),
                members: Vec::new(),
                key: key.clone(),
            };
            slab_keys.insert(lab, key.clone());
            slab_feats.push((
                f,
                RoomRow { feature_id: id, key, kind: "slab".into(), region: region_id, area_mm2: area, perimeter_mm: per, outline: poly, hatches: 0 },
            ));
        }
        // Hatches in this region.
        for &k in &hatch_by_region[r] {
            let h = &inp.hatches[k];
            let Some((i, j)) = g.ij(h.centre) else { continue };
            let n = g.at(i, j);
            let (kind, key) = if an.label[n] == WALL {
                ("wall", String::new())
            } else if an.label[n] != 0 && room_keys.contains_key(&an.label[n]) {
                ("room", room_keys.get(&an.label[n]).cloned().unwrap_or_default())
            } else if an.filled[n] != 0 && slab_keys.contains_key(&an.filled[n]) {
                ("slab", slab_keys.get(&an.filled[n]).cloned().unwrap_or_default())
            } else {
                continue;
            };
            links.push(HatchLink { hatch: h.id, prim: h.prim, handle: h.handle, kind: kind.into(), feature_key: key.clone() });
            if kind == "room" {
                if let Some((_, _, rw)) = room_feats.iter_mut().find(|(_, f, _)| f.key == key) {
                    rw.hatches += 1;
                }
            } else if kind == "slab"
                && let Some((_, rw)) = slab_feats.iter_mut().find(|(f, _)| f.key == key)
            {
                rw.hatches += 1;
            }
        }
        for (_, f, rw) in room_feats {
            stats.rooms += 1;
            let mut f = f;
            set_prop(&mut f, "hatches", json!(rw.hatches));
            out.push(f);
            rooms_out.push(rw);
        }
        for (f, rw) in slab_feats {
            stats.slabs += 1;
            let mut f = f;
            set_prop(&mut f, "hatches", json!(rw.hatches));
            out.push(f);
            rooms_out.push(rw);
        }
        // Final room count per region row.
        if let Some(row) = region_rows.get_mut(r) {
            row.rooms = an.rooms.iter().filter(|c| c.valid).count();
        }
    }
    stats.hatches_assigned = links.len();
    stats.hatches_unassigned = inp.hatches.len().saturating_sub(links.len());
    stats.regions = region_rows.iter().filter(|r| r.kind != "empty").count();
    stats.plan_regions = region_rows.iter().filter(|r| r.kind == "plan").count();
    stats.other_regions = region_rows.iter().filter(|r| r.kind == "other").count();
    stats.ms = t0.elapsed().as_millis();
    CtxOut { features: out, rooms: rooms_out, regions: region_rows, links, stats }
}

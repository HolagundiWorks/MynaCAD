//! Columns: small closed rectangles (a closed polyline or four lines that meet at square corners) and
//! circles. Their sides are two parallel lines the right distance apart, which is what a wall pair
//! looks like, so they are found first and their lines are kept out of the wall candidates.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::json;

use crate::classify::{Input, Params};
use crate::model::{Feature, Member, Prim, PrimKind, q};
use crate::rules::glob_any;

pub struct Columns {
    pub features: Vec<Feature>,
    /// Primitives that belong to a column rectangle (kept out of the wall candidates).
    pub prims: HashSet<u64>,
}

fn dir_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(180.0 - d)
}

type Pt = [f64; 2];

struct Node {
    seg: usize,
    /// 0: the segment starts here, 1: it ends here.
    end: u8,
}

fn feature_key(sub: &str, c: Pt, w: f64, d: f64) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(format!("column|{sub}|{}|{}|{}|{}", q(c[0]), q(c[1]), q(w), q(d)));
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn find(inp: &Input, par: &Params, skip: &dyn Fn(&Prim) -> bool, next_id: &mut u64) -> Columns {
    let rules = par.rules;
    let cr = &rules.column;
    let mut out = Columns { features: Vec::new(), prims: HashSet::new() };
    if !cr.min_side_mm.is_finite() || cr.max_side_mm <= 0.0 {
        return out;
    }
    let tol = cr.corner_tol_mm.max(0.5);
    // Candidate sides: straight pieces of the length of a column side.
    let cand: Vec<usize> = (0..inp.segs.len())
        .filter(|&i| {
            let s = &inp.segs[i];
            s.seg.len >= cr.min_side_mm * 0.9 && s.seg.len <= cr.max_side_mm * 1.1 && !skip(s.prim)
        })
        .collect();
    let cell = tol * 4.0;
    let key = |p: Pt| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<Node>> = HashMap::new();
    let ends = |i: usize| -> [Pt; 2] {
        let s = inp.segs[i].seg;
        [[s.x1, s.y1], [s.x2, s.y2]]
    };
    for &i in &cand {
        for (e, p) in ends(i).iter().enumerate() {
            grid.entry(key(*p)).or_default().push(Node { seg: i, end: e as u8 });
        }
    }
    // Segments with an end at this point, other than `me`.
    let near = |p: Pt, me: usize| -> Vec<(usize, u8)> {
        let (kx, ky) = key(p);
        let mut v = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(nodes) = grid.get(&(kx + dx, ky + dy)) {
                    for n in nodes {
                        if n.seg != me {
                            let e = ends(n.seg)[n.end as usize];
                            if (e[0] - p[0]).hypot(e[1] - p[1]) <= tol && v.len() < 8 {
                                v.push((n.seg, n.end));
                            }
                        }
                    }
                }
            }
        }
        v
    };
    let mut seen: BTreeSet<[usize; 4]> = BTreeSet::new();
    let mut rects: Vec<([usize; 4], [Pt; 4])> = Vec::new();
    'seg: for &i in &cand {
        if rects.len() >= 200_000 {
            break;
        }
        let a_seg = inp.segs[i].seg;
        for start in 0..2usize {
            // Walk i from `a` to `b`, then j from b to c, k from c to d, l from d back to a.
            let a = ends(i)[start];
            let b = ends(i)[1 - start];
            for (j, jf) in near(b, i) {
                let c = ends(j)[1 - jf as usize];
                if (dir_diff(a_seg.angle, inp.segs[j].seg.angle) - 90.0).abs() > 2.0 {
                    continue;
                }
                for (k, kf) in near(c, j) {
                    if k == i || dir_diff(a_seg.angle, inp.segs[k].seg.angle) > 2.0 {
                        continue;
                    }
                    let d = ends(k)[1 - kf as usize];
                    for (l, lf) in near(d, k) {
                        if l == i || l == j {
                            continue;
                        }
                        let back = ends(l)[1 - lf as usize];
                        if (back[0] - a[0]).hypot(back[1] - a[1]) > tol * 2.0 || dir_diff(inp.segs[j].seg.angle, inp.segs[l].seg.angle) > 2.0 {
                            continue;
                        }
                        let mut id = [i, j, k, l];
                        id.sort_unstable();
                        if seen.insert(id) {
                            rects.push(([i, j, k, l], [a, b, c, d]));
                        }
                        continue 'seg;
                    }
                }
            }
        }
    }
    let pat = &cr.layer_patterns;
    let mut used: HashSet<usize> = HashSet::new();
    for (ids, [a, b, c, d]) in rects {
        if ids.iter().any(|i| used.contains(i)) {
            continue;
        }
        let (w1, w2) = ((b[0] - a[0]).hypot(b[1] - a[1]), (c[0] - b[0]).hypot(c[1] - b[1]));
        let (long, short) = (w1.max(w2), w1.min(w2));
        if short < cr.min_side_mm || long > cr.max_side_mm || long > short * cr.max_aspect {
            continue;
        }
        // Opposite sides equal.
        let (w3, w4) = ((d[0] - c[0]).hypot(d[1] - c[1]), (a[0] - d[0]).hypot(a[1] - d[1]));
        if (w1 - w3).abs() > tol * 3.0 || (w2 - w4).abs() > tol * 3.0 {
            continue;
        }
        for i in ids {
            used.insert(i);
        }
        let centre = [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0];
        let prims: BTreeSet<u64> = ids.iter().map(|&i| inp.segs[i].prim.id).collect();
        let named = ids.iter().any(|&i| glob_any(pat, &inp.segs[i].prim.layer));
        let typical = short >= 200.0 && long <= 600.0 && long <= short * 1.5;
        let mut conf = cr.base;
        if named {
            conf += cr.layer_match;
        }
        if typical {
            conf += cr.typical_bonus;
        }
        let angle = {
            let s = inp.segs[ids[0]].seg;
            s.angle
        };
        let mut members: Vec<Member> = Vec::new();
        for &i in &ids {
            let p = inp.segs[i].prim;
            if !members.iter().any(|m| m.prim == p.id) {
                members.push(Member { prim: p.id, handle: p.handle, role: "section".into() });
            }
        }
        members.sort_by_key(|m| m.prim);
        out.prims.extend(prims);
        let id = *next_id;
        *next_id += 1;
        out.features.push(Feature {
            id,
            kind: "column".into(),
            subkind: "rect".into(),
            confidence: conf.clamp(0.0, 1.0),
            rule: "column.rect".into(),
            rules_version: rules.version,
            x1: centre[0],
            y1: centre[1],
            x2: centre[0],
            y2: centre[1],
            width: long,
            depth: short,
            length: long,
            props: json!({ "angle_deg": angle, "area_mm2": long * short, "layer_named": named }),
            members,
            key: feature_key("rect", centre, long, short),
        });
    }
    // Circles.
    for (arc, prim) in &inp.arcs {
        if prim.kind != PrimKind::Circle || arc.r < cr.round_r_min_mm || arc.r > cr.round_r_max_mm || skip(prim) {
            continue;
        }
        let named = glob_any(pat, &prim.layer);
        let mut conf = cr.base - cr.round_penalty;
        if named {
            conf += cr.layer_match;
        }
        if arc.r >= 100.0 && arc.r <= 350.0 {
            conf += cr.typical_bonus;
        }
        let id = *next_id;
        *next_id += 1;
        out.features.push(Feature {
            id,
            kind: "column".into(),
            subkind: "round".into(),
            confidence: conf.clamp(0.0, 1.0),
            rule: "column.round".into(),
            rules_version: rules.version,
            x1: arc.cx,
            y1: arc.cy,
            x2: arc.cx,
            y2: arc.cy,
            width: arc.r * 2.0,
            depth: arc.r * 2.0,
            length: arc.r * 2.0,
            props: json!({ "radius_mm": arc.r, "area_mm2": std::f64::consts::PI * arc.r * arc.r, "layer_named": named }),
            members: vec![Member { prim: prim.id, handle: prim.handle, role: "section".into() }],
            key: feature_key("round", [arc.cx, arc.cy], arc.r * 2.0, arc.r * 2.0),
        });
    }
    out.features.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

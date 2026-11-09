//! Stage 2: parallel pairs without comparing everything with everything.
//!
//! 1. Bucket segments by folded angle (buckets as wide as the angle tolerance).
//! 2. Within a bucket and its upper neighbour, project each segment midpoint onto one reference
//!    normal and sort by that offset.
//! 3. Slide a window along the sorted list: only segments whose offsets differ by no more than the
//!    widest wall band (plus the drift two nearly parallel lines can have) are candidates.
//! 4. Check each candidate exactly: angle, shared length along the axis, perpendicular gap at both
//!    ends and the middle of the shared part.
//!
//! That is O(n log n) plus the candidates found.

use crate::model::{Prim, Seg};

/// A segment with the primitive it came from.
#[derive(Clone, Copy)]
pub struct SegRef<'a> {
    pub seg: &'a Seg,
    pub prim: &'a Prim,
}

#[derive(Clone, Debug)]
pub struct Pair {
    /// Indices into the `SegRef` slice.
    pub a: usize,
    pub b: usize,
    pub gap: f64,
    pub overlap: f64,
    pub overlap_ratio: f64,
    /// Centre line of the shared part.
    pub axis: [f64; 4],
    /// Which gap band matched (index into the band list).
    pub band: usize,
    /// One face of this pair also pairs on its other side at the same gap: stair treads or hatch, not a wall.
    pub ladder: bool,
}

pub struct PairParams<'a> {
    pub angle_tol_deg: f64,
    pub bands: &'a [(f64, f64)],
    pub min_overlap: f64,
    pub min_ratio: f64,
    pub min_seg: f64,
    /// Candidate steps one segment may take before the window gives up (a guard against hatch fields).
    pub max_steps: usize,
}

#[derive(Default, Clone, Debug)]
pub struct PairStats {
    pub considered: usize,
    pub candidates: u64,
    pub capped: u32,
}

fn fold_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(180.0 - d)
}

pub fn find_pairs(segs: &[SegRef], p: &PairParams) -> (Vec<Pair>, PairStats) {
    let mut stats = PairStats::default();
    let mut pairs = Vec::new();
    if p.bands.is_empty() || p.angle_tol_deg.is_nan() || p.angle_tol_deg <= 0.0 {
        return (pairs, stats);
    }
    let hi_max = p.bands.iter().map(|b| b.1).fold(0.0, f64::max) + (p.bands.iter().map(|b| b.1 - b.0).fold(0.0, f64::max));
    let tol = p.angle_tol_deg.clamp(0.01, 10.0);
    let nb = ((180.0 / tol).ceil() as usize).max(1);
    let sin_tol = tol.to_radians().sin();
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); nb];
    for (i, s) in segs.iter().enumerate() {
        let sg = s.seg;
        if sg.len < p.min_seg || !sg.len.is_finite() {
            continue;
        }
        stats.considered += 1;
        let b = ((sg.angle / tol) as usize).min(nb - 1);
        if let Some(v) = buckets.get_mut(b) {
            v.push(i);
        }
    }
    let mut items: Vec<(f64, usize, bool)> = Vec::new();
    for g in 0..nb {
        let upper = (g + 1) % nb;
        let (Some(lo), Some(up)) = (buckets.get(g), buckets.get(upper)) else { continue };
        if lo.is_empty() {
            continue;
        }
        let theta = ((g + 1) as f64 * tol).to_radians();
        let (nx, ny) = (-theta.sin(), theta.cos());
        items.clear();
        let mut lmax: f64 = 0.0;
        for &i in lo {
            let s = segs[i].seg;
            items.push((((s.x1 + s.x2) * nx + (s.y1 + s.y2) * ny) / 2.0, i, true));
            lmax = lmax.max(s.len);
        }
        if upper != g {
            for &i in up {
                let s = segs[i].seg;
                items.push((((s.x1 + s.x2) * nx + (s.y1 + s.y2) * ny) / 2.0, i, false));
                lmax = lmax.max(s.len);
            }
        }
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        for x in 0..items.len() {
            let (dx, ia, lower_a) = items[x];
            let reach = hi_max + (segs[ia].seg.len + lmax) / 2.0 * sin_tol + 1.0;
            let mut steps = 0usize;
            for &(dy, ib, lower_b) in items.iter().skip(x + 1) {
                if dy - dx > reach {
                    break;
                }
                steps += 1;
                if steps > p.max_steps {
                    stats.capped += 1;
                    break;
                }
                if !lower_a && !lower_b {
                    continue;
                }
                stats.candidates += 1;
                if let Some(pr) = check(segs, ia, ib, p, tol) {
                    pairs.push(pr);
                }
            }
        }
    }
    mark_ladders(segs.len(), &mut pairs, segs);
    (pairs, stats)
}

/// Exact test of one candidate pair.
fn check(segs: &[SegRef], ia: usize, ib: usize, p: &PairParams, tol_deg: f64) -> Option<Pair> {
    let (mut a, mut b) = (ia, ib);
    if segs[a].seg.len < segs[b].seg.len {
        std::mem::swap(&mut a, &mut b);
    }
    let (sa, sb) = (segs[a].seg, segs[b].seg);
    if fold_diff(sa.angle, sb.angle) > tol_deg {
        return None;
    }
    // Two parts of one block (a door frame, a piece of furniture) are not the two faces of a wall.
    if segs[a].prim.in_block() && segs[b].prim.in_block() && segs[a].prim.handle == segs[b].prim.handle {
        return None;
    }
    let (ux, uy) = ((sa.x2 - sa.x1) / sa.len, (sa.y2 - sa.y1) / sa.len);
    let (nx, ny) = (-uy, ux);
    let rel = |x: f64, y: f64| -> (f64, f64) {
        let (dx, dy) = (x - sa.x1, y - sa.y1);
        (dx * ux + dy * uy, dx * nx + dy * ny)
    };
    let (t1, s1) = rel(sb.x1, sb.y1);
    let (t2, s2) = rel(sb.x2, sb.y2);
    let (tb0, tb1) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
    let lo = tb0.max(0.0);
    let hi = tb1.min(sa.len);
    let overlap = hi - lo;
    if overlap < p.min_overlap {
        return None;
    }
    let ratio = overlap / sb.len.min(sa.len);
    if ratio < p.min_ratio {
        return None;
    }
    let dt = t2 - t1;
    if dt.abs() < 1e-9 {
        return None;
    }
    let s_at = |t: f64| s1 + (s2 - s1) * (t - t1) / dt;
    let (g_lo, g_mid, g_hi) = (s_at(lo), s_at((lo + hi) / 2.0), s_at(hi));
    if g_lo.signum() != g_mid.signum() || g_hi.signum() != g_mid.signum() {
        return None;
    }
    let gap = g_mid.abs();
    let (band, (blo, bhi)) = p.bands.iter().copied().enumerate().find(|(_, (l, h))| gap >= *l && gap <= *h)?;
    let slack = (bhi - blo) / 2.0;
    if g_lo.abs() < blo - slack || g_lo.abs() > bhi + slack || g_hi.abs() < blo - slack || g_hi.abs() > bhi + slack {
        return None;
    }
    let c = |t: f64| -> [f64; 2] {
        let s = s_at(t) / 2.0;
        [sa.x1 + ux * t + nx * s, sa.y1 + uy * t + ny * s]
    };
    let (c0, c1) = (c(lo), c(hi));
    Some(Pair { a, b, gap, overlap, overlap_ratio: ratio, axis: [c0[0], c0[1], c1[0], c1[1]], band, ladder: false })
}

/// A face with partners of the same band on both of its sides belongs to a ladder (stair treads,
/// evenly spaced parallel lines), not to a wall.
fn mark_ladders(n: usize, pairs: &mut [Pair], segs: &[SegRef]) {
    // Per segment: bitmask of (band, side). Bands are few (a dozen at most), two sides each.
    let mut left = vec![0u32; n];
    let mut right = vec![0u32; n];
    let side_of = |from: usize, to: usize| -> f64 {
        let (f, t) = (segs[from].seg, segs[to].seg);
        let (ux, uy) = ((f.x2 - f.x1) / f.len, (f.y2 - f.y1) / f.len);
        let (mx, my) = ((t.x1 + t.x2) / 2.0 - f.x1, (t.y1 + t.y2) / 2.0 - f.y1);
        ux * my - uy * mx
    };
    for pr in pairs.iter() {
        let bit = 1u32 << (pr.band.min(31));
        if side_of(pr.a, pr.b) > 0.0 {
            left[pr.a] |= bit;
        } else {
            right[pr.a] |= bit;
        }
        if side_of(pr.b, pr.a) > 0.0 {
            left[pr.b] |= bit;
        } else {
            right[pr.b] |= bit;
        }
    }
    for pr in pairs.iter_mut() {
        let bit = 1u32 << (pr.band.min(31));
        pr.ladder = (left[pr.a] & right[pr.a] & bit) != 0 || (left[pr.b] & right[pr.b] & bit) != 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PrimKind;
    use std::sync::Arc;

    fn prim(id: u64) -> Prim {
        Prim {
            id,
            handle: id,
            kind: PrimKind::Line,
            layer: Arc::from("0"),
            color: -1,
            linetype: Arc::from("ByLayer"),
            lineweight: -1,
            closed: false,
            block_path: Arc::from(""),
            bbox: [0.0; 4],
        }
    }

    fn run(lines: &[([f64; 2], [f64; 2])], bands: &[(f64, f64)]) -> Vec<Pair> {
        let segs: Vec<Seg> = lines.iter().enumerate().map(|(i, l)| Seg::new(i as u64, i as u64, 0, l.0, l.1)).collect();
        let prims: Vec<Prim> = (0..segs.len()).map(|i| prim(i as u64)).collect();
        let refs: Vec<SegRef> = segs.iter().zip(&prims).map(|(seg, prim)| SegRef { seg, prim }).collect();
        let p = PairParams { angle_tol_deg: 1.0, bands, min_overlap: 600.0, min_ratio: 0.6, min_seg: 100.0, max_steps: 5000 };
        find_pairs(&refs, &p).0
    }

    #[test]
    fn two_lines_230_apart_are_a_pair_and_a_third_far_away_is_not() {
        let b = [(222.0, 238.0)];
        let pairs = run(&[([0.0, 0.0], [4000.0, 0.0]), ([0.0, 230.0], [4000.0, 230.0]), ([0.0, 900.0], [4000.0, 900.0])], &b);
        assert_eq!(pairs.len(), 1);
        assert!((pairs[0].gap - 230.0).abs() < 1e-9 && (pairs[0].overlap - 4000.0).abs() < 1e-9);
        assert!((pairs[0].axis[1] - 115.0).abs() < 1e-9, "the centre line is half way");
    }

    #[test]
    fn works_across_the_zero_degree_wrap_and_for_reversed_lines() {
        let b = [(222.0, 238.0)];
        // One line drawn left to right, its partner right to left, slightly off horizontal on either side of 0/180.
        let pairs = run(&[([0.0, 0.0], [4000.0, 0.5]), ([4000.0, 230.5], [0.0, 230.0])], &b);
        assert_eq!(pairs.len(), 1, "{pairs:?}");
        // Vertical.
        assert_eq!(run(&[([0.0, 0.0], [0.0, 3000.0]), ([-230.0, 3000.0], [-230.0, 0.0])], &b).len(), 1);
        // Diagonal.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let d = 230.0 * s;
        assert_eq!(run(&[([0.0, 0.0], [3000.0 * s, 3000.0 * s]), ([-d, d], [3000.0 * s - d, 3000.0 * s + d])], &b).len(), 1);
    }

    #[test]
    fn short_overlap_and_wrong_gap_are_rejected() {
        let b = [(222.0, 238.0)];
        assert!(run(&[([0.0, 0.0], [4000.0, 0.0]), ([3800.0, 230.0], [8000.0, 230.0])], &b).is_empty(), "200 mm shared");
        assert!(run(&[([0.0, 0.0], [4000.0, 0.0]), ([0.0, 250.0], [4000.0, 250.0])], &b).is_empty(), "250 is not 230");
    }

    #[test]
    fn evenly_spaced_lines_are_marked_as_a_ladder() {
        let b = [(292.0, 308.0)];
        let lines: Vec<_> = (0..5).map(|i| ([0.0, 300.0 * i as f64], [1200.0, 300.0 * i as f64])).collect();
        let pairs = run(&lines, &b);
        assert_eq!(pairs.len(), 4);
        assert!(pairs.iter().filter(|p| p.ladder).count() >= 3, "{pairs:?}");
        let two = run(&lines[..2], &b);
        assert!(!two[0].ladder, "two lines are a wall, not a ladder");
    }

    #[test]
    fn many_segments_stay_fast() {
        // 40,000 horizontal segments in 400 rows; the window must not turn this quadratic.
        let b = [(222.0, 238.0)];
        let mut lines = Vec::new();
        for r in 0..400 {
            for c in 0..100 {
                lines.push(([c as f64 * 1000.0, r as f64 * 1000.0], [c as f64 * 1000.0 + 800.0, r as f64 * 1000.0]));
            }
        }
        let t = std::time::Instant::now();
        assert!(run(&lines, &b).is_empty());
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    }
}

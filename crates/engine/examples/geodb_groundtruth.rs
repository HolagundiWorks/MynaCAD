#![allow(clippy::collapsible_if)]
//! Ground-truth harness: take a drawing whose layers are already right, remember each object's
//! layer, set every layer to `0`, classify from geometry alone and score the result against the
//! remembered layers.
//!
//! `cargo run --release -p aad-engine --example geodb_groundtruth -- <file.dwg|.dxf|.osp> [--units mm] [--misses N] [--edit]`
//!
//! The truth classes come from layer names (`*WALL*`, `*DOOR*`, `*WIND*`); the rules never see them,
//! because every layer is `0` when the model is built.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use aad_doc::{Drawing, EntityKind, Handle};
use aad_geodb::rules::glob;
use aad_geodb::{BuildOptions, GeoModel};

fn load(path: &str) -> Result<Drawing, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".osp") {
        let odp = aad_odp::Odp::open(std::path::Path::new(path)).map_err(|e| e.to_string())?;
        let names = odp.drawing_names().map_err(|e| e.to_string())?;
        let name = names.first().ok_or("the project has no drawing")?;
        let dr = odp.drawing(name).map_err(|e| e.to_string())?.ok_or("drawing missing")?;
        return aad_io::read(&dr.bytes, name).map_err(|e| e.to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    aad_io::read(&bytes, path).map_err(|e| e.to_string())
}

const NOT_GEOMETRY: [&str; 11] = ["HATCH", "TEXT", "DIM", "TAG", "FINISH", "PATT", "ANNO", "NOTE", "PLAST", "TILE", "FENCE"];

fn is_wall_layer(l: &str) -> bool {
    glob("*WALL*", l) && !NOT_GEOMETRY.iter().any(|k| l.to_ascii_uppercase().contains(k))
}
fn is_door_layer(l: &str) -> bool {
    glob("*DOOR*", l) && !NOT_GEOMETRY.iter().any(|k| l.to_ascii_uppercase().contains(k))
}
fn is_window_layer(l: &str) -> bool {
    (glob("*WINDOW*", l) || glob("*WIND*", l) || glob("*GLAZ*", l)) && !NOT_GEOMETRY.iter().any(|k| l.to_ascii_uppercase().contains(k))
}

struct Score {
    tp: usize,
    fp: usize,
    fnn: usize,
}
impl Score {
    fn p(&self) -> f64 {
        if self.tp + self.fp == 0 { f64::NAN } else { self.tp as f64 / (self.tp + self.fp) as f64 }
    }
    fn r(&self) -> f64 {
        if self.tp + self.fnn == 0 { f64::NAN } else { self.tp as f64 / (self.tp + self.fnn) as f64 }
    }
}

fn score(pred: &HashSet<u64>, truth: &HashSet<u64>) -> Score {
    Score { tp: pred.intersection(truth).count(), fp: pred.difference(truth).count(), fnn: truth.difference(pred).count() }
}

fn is_geom(k: Option<&&'static str>) -> bool {
    matches!(k, Some(&"Line") | Some(&"Polyline") | Some(&"Arc") | Some(&"Circle"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: groundtruth <file> [--units mm|cm|m|in|ft] [--misses N] [--edit]");
        return;
    };
    let units = args.iter().position(|a| a == "--units").and_then(|i| args.get(i + 1)).cloned();
    let misses: usize = args.iter().position(|a| a == "--misses").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
    let t = Instant::now();
    let mut d = match load(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return;
        }
    };
    println!("file: {path}");
    println!(
        "read: {} ms, {} model entities, {} blocks, INSUNITS={} MEASUREMENT={}",
        t.elapsed().as_millis(),
        d.model.len(),
        d.blocks.len(),
        d.header.i64("INSUNITS", -1),
        d.header.i64("MEASUREMENT", -1)
    );

    // Truth from the layers as drawn, then every layer to 0.
    let mut layer_of: HashMap<u64, String> = HashMap::new();
    let mut kind_of: HashMap<u64, &'static str> = HashMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in d.model.iter() {
        layer_of.insert(e.handle.0, e.common.layer.clone());
        kind_of.insert(e.handle.0, e.kind.type_name());
        *counts.entry(e.common.layer.clone()).or_default() += 1;
    }
    let mut top: Vec<_> = counts.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1));
    println!("layers ({}): {}", counts.len(), top.iter().take(14).map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "));
    let original = d.clone();
    if args.iter().any(|a| a == "--keep-linetype") {
        // Keep how the lines look: a by-layer linetype becomes the entity's own before the layer goes.
        let lts: HashMap<String, String> = d.layers.iter().map(|l| (l.name.clone(), l.linetype.clone())).collect();
        for h in d.model.handles() {
            let _ = d.modify_entity(h, |e| {
                if e.common.linetype.eq_ignore_ascii_case("ByLayer") {
                    e.common.linetype = lts.get(&e.common.layer).cloned().unwrap_or_else(|| "Continuous".into());
                }
            });
        }
    }
    for h in d.model.handles() {
        let _ = d.modify_entity(h, |e| e.common.layer = "0".into());
    }
    let names: Vec<String> = d.blocks.keys().cloned().collect();
    for n in names {
        if let Some(b) = d.blocks.get_mut(&n) {
            let b = std::sync::Arc::make_mut(b);
            let hs = b.entities.handles();
            for h in hs {
                b.entities.modify(h, |e| e.common.layer = "0".into());
            }
        }
    }

    let mut rules = aad_geodb::Rules::default();
    for (flag, key, field) in [
        ("--tol", "pair", "tolerance_mm"),
        ("--min-run", "pair", "min_run_mm"),
        ("--min-overlap", "pair", "min_overlap_mm"),
        ("--min-ratio", "pair", "min_overlap_ratio"),
    ] {
        if let Some(v) = args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f64>().ok()) {
            let _ = rules.set_section(key, &serde_json::json!({ field: v }));
        }
    }
    let t = Instant::now();
    let mut m = match GeoModel::build(&d, &BuildOptions { units, rules: Some(rules) }) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("build failed: {e}");
            return;
        }
    };
    let build_ms = t.elapsed().as_millis();
    let sm = m.summary();
    println!(
        "build: {build_ms} ms  (extract {} ms, scale check {} ms, classify {} ms)",
        m.timings.extract_ms, m.timings.scale_check_ms, m.timings.classify_ms
    );
    println!("units: {} ({:?}, confirmed {})  rows: {}", m.units.unit, m.units.source, m.units.confirmed, sm["rows"]);
    println!("pairs: {} candidates: {} capped: {}  warnings: {:?}", m.stats.pairs, m.stats.pair.candidates, m.stats.pair.capped, m.warnings);
    println!("scale check: {:?}", m.scale_check.as_ref().map(|c| (&c.scores, &c.best, c.ambiguous, c.median_gap_mm)));
    println!("features: {}", sm["features"]);

    let min = m.rules.evidence.classify_min;
    let handles_of = |kind: &str| -> HashSet<u64> {
        m.features.iter().filter(|f| f.kind == kind && f.confidence >= min).flat_map(|f| f.members.iter().map(|x| x.handle)).collect()
    };
    let (pw, pd, pwin) = (handles_of("wall"), handles_of("door"), handles_of("window"));
    // A handle claimed by both a wall and an opening is the opening's (it is the more specific).
    let openings: HashSet<u64> = pd.union(&pwin).copied().collect();
    let pw: HashSet<u64> = pw.difference(&openings).copied().collect();
    let truth = |f: fn(&str) -> bool, only_geom: bool| -> HashSet<u64> {
        layer_of.iter().filter(|(h, l)| f(l) && (!only_geom || is_geom(kind_of.get(h)))).map(|(h, _)| *h).collect()
    };
    for (name, pred, f) in [("wall", &pw, is_wall_layer as fn(&str) -> bool), ("door", &pd, is_door_layer), ("window", &pwin, is_window_layer)] {
        for (label, only_geom) in [("line/arc/poly objects", true), ("all objects", false)] {
            let tr = truth(f, only_geom);
            let pr: HashSet<u64> = if only_geom { pred.iter().copied().filter(|h| is_geom(kind_of.get(h))).collect() } else { pred.clone() };
            let s = score(&pr, &tr);
            println!(
                "{name:>6} [{label:<21}] truth {:>7}  predicted {:>7}  tp {:>7}  precision {:>6.1}%  recall {:>6.1}%",
                tr.len(),
                pr.len(),
                s.tp,
                s.p() * 100.0,
                s.r() * 100.0
            );
        }
    }
    // Door and window features: how many have at least one member on a matching true layer.
    for (kind, f) in [("door", is_door_layer as fn(&str) -> bool), ("window", is_window_layer)] {
        let fs: Vec<_> = m.features.iter().filter(|x| x.kind == kind && x.confidence >= min).collect();
        let hit = fs.iter().filter(|x| x.members.iter().any(|mm| layer_of.get(&mm.handle).is_some_and(|l| f(l)))).count();
        println!(
            "{kind} features: {} classified, {} with a member on a true {kind} layer ({:.1}%)",
            fs.len(),
            hit,
            if fs.is_empty() { f64::NAN } else { hit as f64 * 100.0 / fs.len() as f64 }
        );
    }
    // Where the wall truth is not found, by entity type; and where the wall predictions are wrong, by true layer.
    let truth_w = truth(is_wall_layer, false);
    let mut missed: BTreeMap<&str, usize> = BTreeMap::new();
    for h in truth_w.difference(&pw) {
        *missed.entry(kind_of.get(h).copied().unwrap_or("?")).or_default() += 1;
    }
    println!("wall truth not found, by type: {missed:?}");
    let mut wrong: BTreeMap<String, usize> = BTreeMap::new();
    for h in pw.difference(&truth_w) {
        *wrong.entry(layer_of.get(h).cloned().unwrap_or_else(|| "(inside a block)".into())).or_default() += 1;
    }
    let mut wv: Vec<_> = wrong.into_iter().collect();
    wv.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("wall predictions on other layers (top): {:?}", wv.iter().take(10).collect::<Vec<_>>());
    if misses > 0 {
        for h in truth_w.difference(&pw).filter(|h| matches!(kind_of.get(h), Some(&"Line") | Some(&"Polyline"))).take(misses) {
            println!(
                "  missed {} ({}) on {}",
                Handle(*h).hex(),
                kind_of.get(h).copied().unwrap_or("?"),
                layer_of.get(h).cloned().unwrap_or_default()
            );
        }
    }

    if args.iter().any(|a| a == "--sweep") {
        let lenient = |l: &str| is_wall_layer(l) || glob("*BEAM*", l) || glob("*COL*", l);
        let tw = truth(is_wall_layer, true);
        let tl: HashSet<u64> = layer_of.iter().filter(|(h, l)| lenient(l) && is_geom(kind_of.get(h))).map(|(h, _)| *h).collect();
        for thr in [0.5, 0.6, 0.7, 0.8] {
            let p: HashSet<u64> = m
                .features
                .iter()
                .filter(|f| f.kind == "wall" && f.confidence >= thr)
                .flat_map(|f| f.members.iter().map(|x| x.handle))
                .filter(|h| is_geom(kind_of.get(h)) && !openings.contains(h))
                .collect();
            let (s1, s2) = (score(&p, &tw), score(&p, &tl));
            println!(
                "sweep conf>={thr}: strict P {:.1}% R {:.1}%   lenient (walls+beams+columns) P {:.1}% R {:.1}%   predicted {}",
                s1.p() * 100.0,
                s1.r() * 100.0,
                s2.p() * 100.0,
                s2.r() * 100.0,
                p.len()
            );
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        // A picture of the neighbourhood of the n-th missed wall line (drawing units), from the original layers.
        let path = args.get(i + 1).cloned().unwrap_or_else(|| "shot.png".into());
        let nth: usize = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(0);
        let half: f64 = args.get(i + 3).and_then(|v| v.parse().ok()).unwrap_or(200.0);
        let mut miss: Vec<u64> = truth_w.difference(&pw).filter(|h| matches!(kind_of.get(h), Some(&"Line"))).copied().collect();
        miss.sort_unstable();
        if let Some(h) = miss.get(nth * (miss.len() / 8).max(1)) {
            if let Some(e) = original.entity(Handle(*h)) {
                if let EntityKind::Line(l) = &e.kind {
                    let w = aad_geom::Bounds2::new(aad_geom::Vec2::new(l.a.x - half, l.a.y - half), aad_geom::Vec2::new(l.a.x + half, l.a.y + half));
                    match aad_io::write_framed(&original, "x.png", Some(w)) {
                        Ok(b) => {
                            let _ = std::fs::write(&path, b);
                            println!("wrote {path} around {:.0},{:.0}", l.a.x, l.a.y);
                        }
                        Err(e) => println!("shot failed: {e}"),
                    }
                }
            }
        }
    }
    if args.iter().any(|a| a == "--diag") {
        // For wall lines nobody paired: the gap to the nearest parallel line that overlaps them, and how long they are.
        let segs: Vec<(&aad_geodb::model::Seg, u64)> = m.owners.iter().flat_map(|(h, ex)| ex.segs.iter().map(move |s| (s, *h))).collect();
        let mut gaps: BTreeMap<i64, usize> = BTreeMap::new();
        let mut lens: BTreeMap<i64, usize> = BTreeMap::new();
        let mut n = 0;
        for (s, h) in segs.iter().filter(|(_, h)| truth_w.contains(h) && !pw.contains(h)).take(3000) {
            n += 1;
            *lens.entry((s.len / 100.0).floor().min(30.0) as i64 * 100).or_default() += 1;
            let mut best: Option<f64> = None;
            for (o, oh) in &segs {
                if oh == h && o.id == s.id {
                    continue;
                }
                let dth = (o.angle - s.angle).abs();
                if dth.min(180.0 - dth) > 1.0 {
                    continue;
                }
                let (ux, uy) = ((s.x2 - s.x1) / s.len, (s.y2 - s.y1) / s.len);
                let rel = |x: f64, y: f64| ((x - s.x1) * ux + (y - s.y1) * uy, -(x - s.x1) * uy + (y - s.y1) * ux);
                let (t1, g1) = rel(o.x1, o.y1);
                let (t2, _) = rel(o.x2, o.y2);
                let ov = t1.max(t2).min(s.len) - t1.min(t2).max(0.0);
                if ov < 50.0 || g1.abs() < 1.0 || g1.abs() > 800.0 {
                    continue;
                }
                if best.is_none_or(|b| g1.abs() < b) {
                    best = Some(g1.abs());
                }
            }
            *gaps.entry(best.map_or(-1, |g| (g / 10.0).floor() as i64 * 10)).or_default() += 1;
        }
        println!("diag over {n} unpaired true-wall lines; nearest parallel gap (mm, -1 none): {gaps:?}");
        println!("diag length histogram (mm): {lens:?}");
    }

    if let Some(i) = args.iter().position(|a| a == "--near") {
        // Unpaired true-wall lines near a point (drawing units), with their length and angle, and predicted ones on other layers.
        let f = |k: usize| args.get(i + k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let (cx, cy, half) = (f(1), f(2), f(3).max(1.0));
        let k = m.units.to_mm;
        let mut n = 0;
        for (h, ex) in &m.owners {
            for sg in &ex.segs {
                let (x, y) = (sg.x1 / k, sg.y1 / k);
                if (x - cx).abs() > half || (y - cy).abs() > half || n > 60 {
                    continue;
                }
                let tw = truth_w.contains(h);
                let pr = pw.contains(h);
                if tw != pr {
                    n += 1;
                    println!(
                        "  {} h={:X} layer={} ({:.1},{:.1})-({:.1},{:.1}) len_mm={:.0} angle={:.2}",
                        if tw { "MISSED" } else { "EXTRA " },
                        h,
                        layer_of.get(h).cloned().unwrap_or_default(),
                        x,
                        y,
                        sg.x2 / k,
                        sg.y2 / k,
                        sg.len,
                        sg.angle
                    );
                }
            }
        }
    }
    if args.iter().any(|a| a == "--edit") {
        // One edit: move the end of one wall line a little, then update incrementally.
        if let Some(h) = pw.iter().copied().find(|h| matches!(kind_of.get(h), Some(&"Line"))) {
            let _ = d.modify_entity(Handle(h), |e| {
                if let EntityKind::Line(l) = &mut e.kind {
                    l.b.x += 10.0;
                }
            });
            let t = Instant::now();
            let (st, _) = m.update(&d);
            println!(
                "incremental update after one edit: {} ms (changed {}, removed {}, features -{} +{}, region {:?})",
                t.elapsed().as_millis(),
                st.changed,
                st.removed,
                st.features_removed,
                st.features_added,
                st.region
            );
            let t = Instant::now();
            let (st, _) = m.update(&d);
            println!("update with nothing changed: {} ms ({} changed)", t.elapsed().as_millis(), st.changed);
        }
    }
}

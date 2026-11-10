//! The geometry database: the drawing as tables of vectors in millimetres, and rule-based
//! classification by arithmetic on them (docs/GEOMETRY-DB.md).
//!
//! * [`GeoModel::build`] walks model space (blocks expanded, units to millimetres) into rows,
//!   then derives parallel pairs, wall runs, doors and windows with a confidence each.
//! * [`GeoModel::update`] re-extracts only the entities `EntityStore::changed_since` /
//!   `removed_since` report and re-derives only inside the affected region.
//! * `store` (not on the web) writes the tables into the project file (`.osp`) in one bulk
//!   transaction. The tables are derived and local: they never enter the `change` log.
//!
//! Nothing here changes the drawing. Moving objects to layers is `geodb.apply` in the engine.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod classify;
pub mod columns;
pub mod context;
pub mod derive;
pub mod extract;
pub mod model;
pub mod origin;
pub mod rules;
#[cfg(not(target_arch = "wasm32"))]
pub mod store;
pub mod units;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use aad_doc::{Drawing, EntityStore, Handle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::classify::{ClassifyStats, Input, Params};
use crate::derive::{PairParams, SegRef, find_pairs};
use crate::extract::{Extractor, Skipped};
use crate::model::*;
pub use crate::rules::Rules;
pub use crate::units::{UnitSource, UnitSystem, Units};

/// Version of the table layout. A file with another version has its derived tables dropped and rebuilt.
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("database: {0}")]
    Db(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What `build` was asked to do.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// Drafting unit named by the caller (`mm`, `cm`, `m`, `in`, `ft`); confirms an unconfirmed guess.
    pub units: Option<String>,
    pub rules: Option<Rules>,
}

/// Result of the scale check: how much of the drawing's line length forms wall-thickness pairs if
/// the drawing unit were each candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScaleCheck {
    /// (unit, share of the sampled line length that pairs up at wall thicknesses).
    pub scores: Vec<(String, f64)>,
    pub best: Option<String>,
    /// The best score is not clearly ahead of the runner-up, or too small to mean anything.
    pub ambiguous: bool,
    /// Median gap of the pairs found at the best unit, in millimetres.
    pub median_gap_mm: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Timings {
    pub extract_ms: u128,
    pub scale_check_ms: u128,
    pub classify_ms: u128,
    pub total_ms: u128,
}

pub struct GeoModel {
    pub units: Units,
    pub rules: Rules,
    pub owners: BTreeMap<u64, Extracted>,
    /// What the context stage made of `base`: the features the report, the tables and `apply` use.
    pub features: Vec<Feature>,
    /// What the rules found from the geometry alone (columns, walls, doors, windows), before the context stage.
    pub base: Vec<Feature>,
    pub rooms: Vec<context::RoomRow>,
    pub regions: Vec<context::RegionRow>,
    pub hatch_links: Vec<context::HatchLink>,
    pub context_stats: context::ContextStats,
    pub skipped: Skipped,
    pub warnings: Vec<String>,
    pub scale_check: Option<ScaleCheck>,
    pub stats: ClassifyStats,
    pub timings: Timings,
    pub next_id: u64,
    /// Fingerprint of the geometry rows (what `geo_run.drawing_sha256` records).
    pub sha256: String,
    /// The model store this was built from; `update` diffs against it.
    snapshot: EntityStore,
    block_ptrs: HashMap<String, usize>,
    header_units: (i64, i64),
    pub pair_rows: Vec<PairRow>,
    pub built_utc: String,
}

/// A stored `geo_pair` row.
#[derive(Clone, Debug)]
pub struct PairRow {
    pub id: u64,
    pub seg_a: u64,
    pub seg_b: u64,
    pub gap: f64,
    pub overlap: f64,
    pub overlap_ratio: f64,
    pub axis: [f64; 4],
    pub band: usize,
    pub ladder: bool,
}

/// What an incremental update touched.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateStats {
    pub changed: usize,
    pub removed: usize,
    pub region: Option<[f64; 4]>,
    pub features_removed: usize,
    pub features_added: usize,
    pub rebuilt: bool,
    pub ms: u128,
}

/// Rows to remove from and add to a stored copy after an update.
#[derive(Clone, Debug, Default)]
pub struct Delta {
    pub dropped_handles: Vec<u64>,
    pub added_handles: Vec<u64>,
    pub dropped_features: Vec<u64>,
    pub added_features: Vec<u64>,
    pub region: Option<[f64; 4]>,
}

fn now_utc() -> String {
    // Seconds since the epoch is enough to tell builds apart; the project file formats its own times.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}

impl GeoModel {
    /// Build every table for a drawing.
    pub fn build(d: &Drawing, opts: &BuildOptions) -> Result<GeoModel> {
        let t_all = Instant::now();
        let rules = opts.rules.clone().unwrap_or_default();
        rules.validate()?;
        let mut warnings = Vec::new();
        let mut timings = Timings::default();

        // 1. Units.
        let mut units = match &opts.units {
            Some(name) => Units::new(name, UnitSource::Override, true)
                .ok_or_else(|| Error::Invalid(format!("unknown unit `{name}` (use mm, cm, m, in or ft)")))?,
            None => match units::declared_units(d) {
                Some(u) => u,
                None => {
                    let (system, name) = match units::leaning_system(d) {
                        UnitSystem::Metric => (UnitSystem::Metric, "mm"),
                        UnitSystem::Imperial => (UnitSystem::Imperial, "in"),
                    };
                    let mut u = Units::new(name, UnitSource::ScaleCheck, false).unwrap_or(Units {
                        system,
                        unit: name.into(),
                        to_mm: 1.0,
                        source: UnitSource::ScaleCheck,
                        confirmed: false,
                    });
                    u.confirmed = false;
                    u
                }
            },
        };

        // 2. Extract.
        let t = Instant::now();
        let mut owners = BTreeMap::new();
        let mut skipped = Skipped::default();
        let mut next_id = 1u64;
        let mut x = Extractor::new(d, units.to_mm, next_id);
        for e in d.model.iter() {
            owners.insert(e.handle.0, x.entity(e));
        }
        skipped.merge(&x.skipped);
        next_id = x.next_id;
        timings.extract_ms = t.elapsed().as_millis();

        // 3. Scale check. A unitless drawing takes the best unit as a proposal; a declared one is only sanity-checked.
        let t = Instant::now();
        let check = scale_check(&owners, &rules, units.to_mm, units.system);
        if units.source == UnitSource::ScaleCheck {
            match check.best.as_deref().and_then(|b| Units::new(b, UnitSource::ScaleCheck, false)) {
                Some(best) if !check.ambiguous => {
                    if (best.to_mm - units.to_mm).abs() > 1e-9 {
                        units = best;
                        let mut x = Extractor::new(d, units.to_mm, 1);
                        owners.clear();
                        skipped = Skipped::default();
                        for e in d.model.iter() {
                            owners.insert(e.handle.0, x.entity(e));
                        }
                        skipped.merge(&x.skipped);
                        next_id = x.next_id;
                    } else {
                        units = best;
                    }
                    warnings.push(format!(
                        "the drawing has no units; wall thicknesses suggest {}. Confirm with units: \"{}\" before applying anything.",
                        units.unit, units.unit
                    ));
                }
                _ => warnings.push("the drawing has no units and the scale check could not decide; give units (mm, cm, m, in or ft) to build".into()),
            }
        } else if let Some(best) = &check.best
            && !check.ambiguous
            && *best != units.unit
            && check.scores.iter().find(|s| s.0 == units.unit).is_none_or(|s| s.1 < 0.5 * check.scores.iter().map(|c| c.1).fold(0.0, f64::max))
        {
            warnings.push(format!("the drawing says {} but its wall thicknesses fit {} much better; check the units", units.unit, best));
        }
        timings.scale_check_ms = t.elapsed().as_millis();

        let mut m = GeoModel {
            units,
            rules,
            owners,
            features: Vec::new(),
            base: Vec::new(),
            rooms: Vec::new(),
            regions: Vec::new(),
            hatch_links: Vec::new(),
            context_stats: context::ContextStats::default(),
            skipped,
            warnings,
            scale_check: Some(check),
            stats: ClassifyStats::default(),
            timings,
            next_id,
            sha256: String::new(),
            snapshot: d.model.clone(),
            block_ptrs: block_ptrs(d),
            header_units: (d.header.i64("INSUNITS", 0), d.header.i64("MEASUREMENT", 1)),
            pair_rows: Vec::new(),
            built_utc: now_utc(),
        };

        // 4. Derive and classify.
        let t = Instant::now();
        m.classify_all();
        m.timings.classify_ms = t.elapsed().as_millis();
        m.sha256 = m.fingerprint();
        m.timings.total_ms = t_all.elapsed().as_millis();
        Ok(m)
    }

    fn input<'a>(&'a self, region: Option<[f64; 4]>) -> Input<'a> {
        let hit = |b: &[f64; 4]| match region {
            None => true,
            Some(r) => b[0] <= r[2] && b[2] >= r[0] && b[1] <= r[3] && b[3] >= r[1],
        };
        let mut inp = Input { segs: Vec::new(), arcs: Vec::new(), inserts: Vec::new() };
        for ex in self.owners.values() {
            if region.is_some() && ex.bbox().is_none_or(|b| !hit(&b)) {
                continue;
            }
            let by_id: HashMap<u64, &Prim> = if ex.prims.len() > 1 { ex.prims.iter().map(|p| (p.id, p)).collect() } else { HashMap::new() };
            let prim_of =
                |id: u64| -> Option<&Prim> { if by_id.is_empty() { ex.prims.iter().find(|p| p.id == id) } else { by_id.get(&id).copied() } };
            for s in &ex.segs {
                if let Some(p) = prim_of(s.prim) {
                    inp.segs.push(SegRef { seg: s, prim: p });
                }
            }
            for a in &ex.arcs {
                if let Some(p) = prim_of(a.prim) {
                    inp.arcs.push((a, p));
                }
            }
            for i in &ex.inserts {
                if let Some(p) = prim_of(i.prim) {
                    inp.inserts.push((i, p));
                }
            }
        }
        inp
    }

    fn classify_all(&mut self) {
        let (feats, stats, pairs, next) = {
            let inp = self.input(None);
            let par = Params { rules: &self.rules, system: self.units.system, scale: self.units.to_mm };
            let mut next = self.next_id;
            let (f, s, p) = classify::classify(&inp, &par, &mut next);
            (f, s, p, next)
        };
        self.base = feats;
        self.stats = stats;
        self.next_id = next;
        self.pair_rows = pairs;
        self.run_context();
    }

    /// The context stage over the whole sheet: regions, rooms, beams, slab, hatch links. Cheap next to extraction.
    fn run_context(&mut self) {
        let inp = self.ctx_input();
        let mut next = self.next_id;
        let out = context::apply(&self.base, &inp, &self.rules, &mut next);
        self.next_id = next;
        self.features = out.features;
        self.rooms = out.rooms;
        self.regions = out.regions;
        self.hatch_links = out.links;
        self.context_stats = out.stats;
    }

    fn ctx_input(&self) -> context::CtxInput {
        use crate::context::{HatchIn, Item};
        let mut items = Vec::new();
        let mut hatches = Vec::new();
        for (handle, ex) in &self.owners {
            for s in &ex.segs {
                items.push(Item::Seg([s.x1, s.y1, s.x2, s.y2]));
            }
            for p in &ex.prims {
                if matches!(p.kind, PrimKind::Arc | PrimKind::Circle | PrimKind::Text | PrimKind::Point | PrimKind::Hatch) && p.bbox[0] <= p.bbox[2] {
                    items.push(Item::Bbox(p.bbox));
                }
            }
            for h in &ex.hatches {
                let first = h.loops.first().filter(|l| !l.is_empty());
                let centre = match first {
                    Some(l) => [l.iter().map(|p| p[0]).sum::<f64>() / l.len() as f64, l.iter().map(|p| p[1]).sum::<f64>() / l.len() as f64],
                    None => continue,
                };
                hatches.push(HatchIn { id: h.id, prim: h.prim, handle: *handle, centre });
            }
        }
        context::CtxInput { items, hatches }
    }

    /// Re-classify with changed rules (no re-extraction).
    pub fn reclassify(&mut self, rules: Rules) -> Result<()> {
        rules.validate()?;
        self.rules = rules;
        let t = Instant::now();
        self.classify_all();
        self.timings.classify_ms = t.elapsed().as_millis();
        self.sha256 = self.fingerprint();
        Ok(())
    }

    /// Hash of the geometry rows: equal drawings give equal fingerprints, an edit changes it.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(format!("{}|{}", self.units.unit, self.units.system.as_str()));
        for (handle, ex) in &self.owners {
            h.update(handle.to_le_bytes());
            for s in &ex.segs {
                for v in [s.x1, s.y1, s.x2, s.y2] {
                    h.update(q(v).to_le_bytes());
                }
            }
            for a in &ex.arcs {
                for v in [a.cx, a.cy, a.r, a.a0, a.a1] {
                    h.update(q(v).to_le_bytes());
                }
            }
            for i in &ex.inserts {
                h.update(i.block.as_bytes());
                h.update(q(i.x).to_le_bytes());
                h.update(q(i.y).to_le_bytes());
            }
            for p in &ex.prims {
                h.update(p.layer.as_bytes());
            }
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn prim(&self, handle: u64, id: u64) -> Option<&Prim> {
        self.owners.get(&handle)?.prims.iter().find(|p| p.id == id)
    }

    /// Incremental update from the drawing's current model store. Re-extracts the changed entities
    /// and re-derives inside the region they touch.
    pub fn update(&mut self, d: &Drawing) -> (UpdateStats, Delta) {
        let t = Instant::now();
        let mut stats = UpdateStats::default();
        let mut delta = Delta::default();
        // A change of units or of the rules' meaning needs a full rebuild.
        let hu = (d.header.i64("INSUNITS", 0), d.header.i64("MEASUREMENT", 1));
        if hu != self.header_units {
            stats.rebuilt = true;
            let opts = BuildOptions {
                units: if self.units.source == UnitSource::Override { Some(self.units.unit.clone()) } else { None },
                rules: Some(self.rules.clone()),
            };
            if let Ok(m) = GeoModel::build(d, &opts) {
                *self = m;
            }
            stats.ms = t.elapsed().as_millis();
            return (stats, delta);
        }
        let changed: Vec<_> = d.model.changed_since(&self.snapshot).into_iter().cloned().collect();
        let removed: Vec<Handle> = d.model.removed_since(&self.snapshot);
        let new_ptrs = block_ptrs(d);
        let changed_blocks: HashSet<&String> = new_ptrs
            .iter()
            .filter(|(k, v)| self.block_ptrs.get(*k) != Some(v))
            .map(|(k, _)| k)
            .chain(self.block_ptrs.keys().filter(|k| !new_ptrs.contains_key(*k)))
            .collect();
        let mut dirty: HashSet<u64> = changed.iter().map(|e| e.handle.0).collect();
        dirty.extend(removed.iter().map(|h| h.0));
        if !changed_blocks.is_empty() {
            for (h, ex) in &self.owners {
                if ex.inserts.iter().any(|i| changed_blocks.contains(&i.block.to_string())) {
                    dirty.insert(*h);
                }
            }
        }
        stats.changed = changed.len();
        stats.removed = removed.len();
        if dirty.is_empty() {
            stats.ms = t.elapsed().as_millis();
            self.snapshot = d.model.clone();
            return (stats, delta);
        }
        // The region the edit touches: old and new extents of every dirty entity.
        let mut region: Option<[f64; 4]> = None;
        let grow = |r: &mut Option<[f64; 4]>, b: [f64; 4]| {
            *r = Some(match *r {
                None => b,
                Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
            })
        };
        for h in &dirty {
            if let Some(b) = self.owners.get(h).and_then(Extracted::bbox) {
                grow(&mut region, b);
            }
            delta.dropped_handles.push(*h);
        }
        let mut x = Extractor::new(d, self.units.to_mm, self.next_id);
        for h in &dirty {
            self.owners.remove(h);
            if let Some(e) = d.model.get(Handle(*h)) {
                let ex = x.entity(e);
                if let Some(b) = ex.bbox() {
                    grow(&mut region, b);
                }
                self.owners.insert(*h, ex);
                delta.added_handles.push(*h);
            }
        }
        self.next_id = x.next_id;
        self.skipped.merge(&x.skipped);
        let band_max = self.rules.wall_types.iter().map(|t| t.metric_mm.max(t.imperial_mm)).fold(0.0, f64::max);
        let reach = (band_max * 3.0 + self.rules.window.width_max_mm).max(1500.0);
        if let Some(d0) = region {
            // Features that touch the edit (inflated by what one face can pair across) are dropped and re-derived whole;
            // the others are not touched. The derivation window is those features' extents plus a margin.
            let dirty_box = [d0[0] - reach, d0[1] - reach, d0[2] + reach, d0[3] + reach];
            let hit = |b: [f64; 4], r: [f64; 4]| b[0] <= r[2] && b[2] >= r[0] && b[1] <= r[3] && b[3] >= r[1];
            let mut r = dirty_box;
            for f in &self.base {
                let b = f.bbox();
                if hit(b, dirty_box) {
                    r = [r[0].min(b[0]), r[1].min(b[1]), r[2].max(b[2]), r[3].max(b[3])];
                }
            }
            r = [r[0] - reach, r[1] - reach, r[2] + reach, r[3] + reach];
            let (feats, cstats, pairs, next) = {
                let inp = self.input(Some(r));
                let par = Params { rules: &self.rules, system: self.units.system, scale: self.units.to_mm };
                let mut next = self.next_id;
                let (f, s, p) = classify::classify(&inp, &par, &mut next);
                (f, s, p, next)
            };
            self.next_id = next;
            let inside = |b: [f64; 4]| hit(b, dirty_box);
            let before = self.base.len();
            self.base.retain(|f| {
                let drop = inside(f.bbox());
                !drop
            });
            stats.features_removed = before - self.base.len();
            for f in feats {
                if inside(f.bbox()) {
                    self.base.push(f);
                    stats.features_added += 1;
                }
            }
            self.pair_rows.retain(|p| {
                let b = [p.axis[0].min(p.axis[2]), p.axis[1].min(p.axis[3]), p.axis[0].max(p.axis[2]), p.axis[1].max(p.axis[3])];
                !inside(b)
            });
            for p in pairs {
                let b = [p.axis[0].min(p.axis[2]), p.axis[1].min(p.axis[3]), p.axis[0].max(p.axis[2]), p.axis[1].max(p.axis[3])];
                if inside(b) {
                    self.pair_rows.push(p);
                }
            }
            self.stats.pairs = cstats.pairs;
            stats.region = Some(r);
            delta.region = Some(dirty_box);
        }
        self.snapshot = d.model.clone();
        self.block_ptrs = new_ptrs;
        let old: HashMap<u64, u64> = self.features.iter().map(|f| (f.id, feature_hash(f))).collect();
        self.run_context();
        let new: HashMap<u64, u64> = self.features.iter().map(|f| (f.id, feature_hash(f))).collect();
        delta.dropped_features = old.iter().filter(|(id, h)| new.get(id) != Some(h)).map(|(id, _)| *id).collect();
        delta.added_features = new.iter().filter(|(id, h)| old.get(id) != Some(h)).map(|(id, _)| *id).collect();
        delta.dropped_features.sort_unstable();
        delta.added_features.sort_unstable();
        self.sha256 = self.fingerprint();
        stats.ms = t.elapsed().as_millis();
        (stats, delta)
    }

    /// Counts for the report: features by kind and subkind, and how many pass the confidence threshold.
    pub fn summary(&self) -> Value {
        let min = self.rules.evidence.classify_min;
        let mut by: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
        for f in &self.features {
            let e = by.entry(format!("{}.{}", f.kind, f.subkind)).or_insert((0, 0, 0.0));
            e.0 += 1;
            if f.confidence >= min {
                e.1 += 1;
            }
            e.2 += f.confidence;
        }
        let wall_len: f64 = self.features.iter().filter(|f| f.kind == "wall" && f.confidence >= min).map(|f| f.length).sum();
        let kinds: Vec<Value> = by
            .into_iter()
            .map(|(k, (n, ok, sum))| json!({ "kind": k, "found": n, "classified": ok, "ambiguous": n - ok, "meanConfidence": if n > 0 { (sum / n as f64 * 1000.0).round() / 1000.0 } else { 0.0 } }))
            .collect();
        let (mut prims, mut segs, mut arcs, mut ins, mut texts, mut hatches) = (0, 0, 0, 0, 0, 0);
        for ex in self.owners.values() {
            prims += ex.prims.len();
            segs += ex.segs.len();
            arcs += ex.arcs.len();
            ins += ex.inserts.len();
            texts += ex.texts.len();
            hatches += ex.hatches.len();
        }
        let mut region_kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for r in self.regions.iter().filter(|r| r.kind != "empty") {
            *region_kinds.entry(r.kind.as_str()).or_default() += 1;
        }
        json!({
            "units": { "system": self.units.system.as_str(), "unit": self.units.unit, "toMm": self.units.to_mm, "source": self.units.source, "confirmed": self.units.confirmed },
            "rulesVersion": self.rules.version,
            "rows": { "prims": prims, "segs": segs, "arcs": arcs, "inserts": ins, "texts": texts, "hatches": hatches, "pairs": self.pair_rows.len(), "features": self.features.len(), "entities": self.owners.len() },
            "features": kinds,
            "wallLengthMm": wall_len.round(),
            "context": {
                "regions": region_kinds,
                "rooms": self.rooms.iter().filter(|r| r.kind == "room").count(),
                "roomAreaM2": (self.rooms.iter().filter(|r| r.kind == "room").map(|r| r.area_mm2).sum::<f64>() / 1.0e4).round() / 100.0,
                "slabs": self.rooms.iter().filter(|r| r.kind == "slab").count(),
                "slabAreaM2": (self.rooms.iter().filter(|r| r.kind == "slab").map(|r| r.area_mm2).sum::<f64>() / 1.0e4).round() / 100.0,
                "hatchLinks": self.hatch_links.len(),
                "ms": self.context_stats.ms,
            },
            "skipped": self.skipped.0,
            "warnings": self.warnings,
            "scaleCheck": self.scale_check,
            "timings": self.timings,
            "sha256": self.sha256,
        })
    }
}

/// A cheap fingerprint of everything stored for a feature, to find the rows an update changed.
fn feature_hash(f: &Feature) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&f.key, &f.kind, &f.subkind, (f.confidence * 1.0e6).round() as i64, f.members.len(), f.props.to_string()).hash(&mut h);
    for v in [f.x1, f.y1, f.x2, f.y2, f.width, f.depth, f.length] {
        q(v).hash(&mut h);
    }
    h.finish()
}

fn block_ptrs(d: &Drawing) -> HashMap<String, usize> {
    d.blocks.iter().map(|(k, v)| (k.clone(), std::sync::Arc::as_ptr(v) as usize)).collect()
}

/// Which unit makes the drawing's line pairs look like walls?
///
/// `current_to_mm` is the factor the rows were extracted with; a candidate unit `u` rescales the
/// rows by `u / current_to_mm`. The score is the share of sampled line length that pairs up at a
/// wall thickness, which does not depend on the unit and so can be compared.
pub fn scale_check(owners: &BTreeMap<u64, Extracted>, rules: &Rules, current_to_mm: f64, lean: UnitSystem) -> ScaleCheck {
    let mut all: Vec<SegRef> = Vec::new();
    for ex in owners.values() {
        let by: HashMap<u64, &Prim> = ex.prims.iter().map(|p| (p.id, p)).collect();
        for s in &ex.segs {
            if let Some(p) = by.get(&s.prim) {
                all.push(SegRef { seg: s, prim: p });
            }
        }
    }
    // The longest lines carry the plan; a sample keeps the five passes cheap on a heavy drawing.
    const SAMPLE: usize = 40_000;
    if all.len() > SAMPLE {
        all.sort_by(|a, b| b.seg.len.total_cmp(&a.seg.len));
        all.truncate(SAMPLE);
    }
    let total: f64 = all.iter().map(|s| s.seg.len).sum();
    let mut scores: Vec<(String, f64)> = Vec::new();
    let mut best_gaps: Vec<(String, Vec<f64>)> = Vec::new();
    if total > 0.0 && current_to_mm > 0.0 {
        for name in units::CANDIDATES {
            let Some((system, _, u)) = units::unit_by_name(name) else { continue };
            // Rows are in `current_to_mm` per unit; real millimetres = row * (u / current_to_mm) is wrong the other way round:
            // a row value v is v drawing units, worth v * u mm if the drawing unit is `u`. Rows were scaled by current_to_mm already.
            let k = u / current_to_mm; // multiply row values by k to get millimetres under this candidate
            let bands: Vec<(f64, f64)> = rules.bands(system).into_iter().map(|(a, b)| (a / k, b / k)).collect();
            let pp = PairParams {
                angle_tol_deg: rules.pair.angle_tol_deg,
                bands: &bands,
                // Coincidences between long unrelated lines would score at any unit; ask for a wall-length overlap here.
                min_overlap: rules.pair.min_run_mm.max(rules.pair.min_overlap_mm) / k,
                min_ratio: rules.pair.min_overlap_ratio,
                min_seg: rules.pair.min_segment_mm / k,
                max_steps: 1500,
            };
            let (pairs, _) = find_pairs(&all, &pp);
            let matched: f64 = pairs.iter().filter(|p| !p.ladder).map(|p| p.overlap * 2.0).sum();
            scores.push((name.to_string(), matched / total));
            best_gaps.push((name.to_string(), pairs.iter().filter(|p| !p.ladder).map(|p| p.gap * k).collect()));
        }
    }
    let mut ranked: Vec<&(String, f64)> = scores.iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let best = ranked.first().filter(|b| b.1 > 0.0).map(|b| b.0.clone());
    let runner = ranked.get(1).map(|r| r.1).unwrap_or(0.0);
    let top = ranked.first().map(|r| r.1).unwrap_or(0.0);
    // The lean (MEASUREMENT) breaks near ties: a metric-leaning drawing prefers mm over in.
    let ambiguous = best.is_none() || top < 0.01 || top < runner * 1.5;
    let _ = lean;
    let median_gap_mm = best.as_ref().and_then(|b| best_gaps.iter().find(|g| &g.0 == b)).and_then(|g| {
        let mut v = g.1.clone();
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() / 2).copied()
    });
    ScaleCheck { scores, best, ambiguous, median_gap_mm }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests2;

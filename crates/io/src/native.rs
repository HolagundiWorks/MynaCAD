//! The native drawing model as bytes: what MynaCAD keeps in a project file (`.osp`) so a drawing opens without being parsed from DXF or DWG again.
//!
//! A drawing becomes a list of [`Part`]s. Part 0 is the head (header variables, tables, block and layout records, levels, sheet sets, project
//! data: everything except entities). Every later part is a batch of entities for named stores (`model`, `block:<n>`, `layout:<n>`, `level:<id>`,
//! `sheet:<set>:<level>`). Model space is cut at the entity store's own chunk boundaries, so an edit changes the bytes of the parts it touched and
//! no others; a caller that stores parts by content hash then writes only those.
//!
//! Each part is MessagePack with field names (so a field added later with a default still reads old files), deflated, behind a one-byte codec
//! tag. Decoding checks sizes before allocating: the bytes may come from a damaged or hostile file.

use std::collections::BTreeMap;
use std::sync::Arc;

use aad_doc::{
    Block, ComposeView, DimStyle, Drawing, Entity, EntityStore, Group, Header, Layer, LayerState, Layout, Level, LevelId, Linetype, MLeaderStyle,
    MasterLayout, NamedView, PlotStyleTable, PublisherSet, SheetSet, SheetSetId, TableStyle, TextStyle, Trace, Ucs, ViewRecord, ViewSpace,
};
use aad_geom::Vec2;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{IoError, Result};

/// Version of the part layout. A reader refuses any other version; the caller then reads the drawing from its DXF or DWG bytes instead.
pub const FORMAT_VERSION: u32 = 1;

/// Entities per part for the stores that are batched together (blocks, layouts, inactive levels).
const BATCH: usize = 1024;
/// A part is closed once its entities weigh about this many bytes ([`weight`]), so a few very large entities (an embedded OLE object is
/// megabytes of text) become parts of their own and are packed and unpacked on separate cores.
const PART_WEIGHT: usize = 1 << 20;
/// Largest part after inflating (a damaged length cannot make the reader allocate more).
const MAX_PART_BYTES: usize = 1 << 30;
/// Most parts in one drawing.
pub const MAX_PARTS: usize = 4_000_000;

const CODEC_RAW: u8 = 0;
const CODEC_DEFLATE: u8 = 1;

/// One piece of an encoded drawing. `bytes` is self-contained; parts are read back in the order they were written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub bytes: Vec<u8>,
    /// Entities in the part (0 for the head).
    pub entities: usize,
    /// SHA-256 of `bytes`, lower-case hex: the name a store keeps the part under.
    pub hash: String,
}

impl Part {
    fn new(bytes: Vec<u8>, entities: usize) -> Part {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        Part { bytes, entities, hash }
    }
}

fn err(m: impl Into<String>) -> IoError {
    IoError::Format(m.into())
}

#[derive(Serialize)]
struct LevelOut<'a> {
    id: LevelId,
    name: &'a str,
    elevation: f64,
    height: f64,
    traces: &'a [Trace],
    view: Option<(Vec2, f64)>,
}

#[derive(Deserialize)]
struct LevelIn {
    id: LevelId,
    name: String,
    elevation: f64,
    height: f64,
    #[serde(default)]
    traces: Vec<Trace>,
    #[serde(default)]
    view: Option<(Vec2, f64)>,
}

#[derive(Serialize)]
struct SheetOut<'a> {
    id: SheetSetId,
    name: &'a str,
    layer_prefix: &'a str,
    space: &'a Option<String>,
    dim_style: &'a Option<String>,
    text_style: &'a Option<String>,
    layers: &'a [String],
    blocks: &'a [String],
    /// The levels that have an annotation store (an empty store is still a slot).
    annotation_levels: Vec<LevelId>,
}

#[derive(Deserialize)]
struct SheetIn {
    id: SheetSetId,
    name: String,
    #[serde(default)]
    layer_prefix: String,
    #[serde(default)]
    space: Option<String>,
    #[serde(default)]
    dim_style: Option<String>,
    #[serde(default)]
    text_style: Option<String>,
    #[serde(default)]
    layers: Vec<String>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    annotation_levels: Vec<LevelId>,
}

/// Everything in a drawing except its entities.
#[derive(Serialize)]
struct HeadOut<'a> {
    format: u32,
    header: &'a Header,
    layers: &'a [Layer],
    linetypes: &'a [Linetype],
    text_styles: &'a [TextStyle],
    dim_styles: &'a [DimStyle],
    mleader_styles: &'a [MLeaderStyle],
    table_styles: &'a [TableStyle],
    /// Block records in name order; block `n` owns the store `block:<n>`.
    blocks: Vec<&'a Block>,
    levels: Vec<LevelOut<'a>>,
    active_level: Option<LevelId>,
    level_seed: u32,
    sheet_sets: Vec<SheetOut<'a>>,
    active_sheet: Option<SheetSetId>,
    sheet_seed: u32,
    /// Sections and elevations (additive: older files have none); view `n` owns the store `view:<id>`.
    view_spaces: Vec<&'a ViewRecord>,
    active_view: Option<aad_doc::SpaceId>,
    /// Layout records in order; layout `n` owns the store `layout:<n>`.
    layouts: &'a [Layout],
    views: &'a [NamedView],
    ucss: &'a [Ucs],
    layer_states: &'a [LayerState],
    compose_views: &'a [ComposeView],
    compose_view_seed: u32,
    master_layouts: &'a [MasterLayout],
    master_layout_seed: u32,
    publisher_sets: &'a [PublisherSet],
    project: &'a BTreeMap<String, String>,
    groups: &'a [Group],
    plot_style_tables: &'a [PlotStyleTable],
    handseed: u64,
}

#[derive(Deserialize)]
struct HeadIn {
    format: u32,
    header: Header,
    layers: Vec<Layer>,
    linetypes: Vec<Linetype>,
    text_styles: Vec<TextStyle>,
    dim_styles: Vec<DimStyle>,
    #[serde(default)]
    mleader_styles: Vec<MLeaderStyle>,
    #[serde(default)]
    table_styles: Vec<TableStyle>,
    #[serde(default)]
    blocks: Vec<Block>,
    #[serde(default)]
    levels: Vec<LevelIn>,
    #[serde(default)]
    active_level: Option<LevelId>,
    #[serde(default)]
    level_seed: u32,
    #[serde(default)]
    sheet_sets: Vec<SheetIn>,
    #[serde(default)]
    active_sheet: Option<SheetSetId>,
    #[serde(default)]
    sheet_seed: u32,
    #[serde(default)]
    view_spaces: Vec<ViewRecord>,
    #[serde(default)]
    active_view: Option<aad_doc::SpaceId>,
    #[serde(default)]
    layouts: Vec<Layout>,
    #[serde(default)]
    views: Vec<NamedView>,
    #[serde(default)]
    ucss: Vec<Ucs>,
    #[serde(default)]
    layer_states: Vec<LayerState>,
    #[serde(default)]
    compose_views: Vec<ComposeView>,
    #[serde(default)]
    compose_view_seed: u32,
    #[serde(default)]
    master_layouts: Vec<MasterLayout>,
    #[serde(default)]
    master_layout_seed: u32,
    #[serde(default)]
    publisher_sets: Vec<PublisherSet>,
    #[serde(default)]
    project: BTreeMap<String, String>,
    #[serde(default)]
    groups: Vec<Group>,
    #[serde(default)]
    plot_style_tables: Vec<PlotStyleTable>,
    handseed: u64,
}

/// A batch of entities: (store key, entities in draw order), appended to the named stores in part order.
type BatchOut<'a> = Vec<(&'a str, Vec<&'a Entity>)>;
type BatchIn = Vec<(String, Vec<Entity>)>;

fn pack<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut plain = Vec::new();
    let mut ser = rmp_serde::Serializer::new(&mut plain).with_struct_map();
    value.serialize(&mut ser).map_err(|e| err(format!("native model: {e}")))?;
    let packed = miniz_oxide::deflate::compress_to_vec(&plain, 1);
    let mut out = Vec::with_capacity(packed.len().min(plain.len()) + 1);
    if packed.len() < plain.len() {
        out.push(CODEC_DEFLATE);
        out.extend_from_slice(&packed);
    } else {
        out.push(CODEC_RAW);
        out.extend_from_slice(&plain);
    }
    Ok(out)
}

fn unpack<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let (codec, body) = bytes.split_first().ok_or_else(|| err("native model: an empty part"))?;
    let inflated;
    let plain: &[u8] = match *codec {
        CODEC_RAW => body,
        CODEC_DEFLATE => {
            inflated =
                miniz_oxide::inflate::decompress_to_vec_with_limit(body, MAX_PART_BYTES).map_err(|_| err("native model: a part is damaged"))?;
            &inflated
        }
        other => return Err(err(format!("native model: unknown part codec {other}"))),
    };
    rmp_serde::from_slice(plain).map_err(|e| err(format!("native model: {e}")))
}

/// A cheap estimate of an entity's encoded size: exact enough to keep huge entities apart, and a pure function of the entity, so the same
/// chunk is always cut the same way.
fn weight(e: &Entity) -> usize {
    match &e.kind {
        aad_doc::EntityKind::Unknown(u) => u.tags.iter().fold(64usize, |n, t| n.saturating_add(t.value.len()).saturating_add(16)),
        _ => 256,
    }
}

/// Cut one store chunk into batches of about [`PART_WEIGHT`].
fn cut<'a>(key: &'a str, items: &'a [Arc<Entity>], out: &mut Vec<BatchOut<'a>>) {
    let mut run: Vec<&Entity> = Vec::new();
    let mut heavy = 0usize;
    for e in items {
        let w = weight(e);
        if !run.is_empty() && heavy.saturating_add(w) > PART_WEIGHT {
            out.push(vec![(key, std::mem::take(&mut run))]);
            heavy = 0;
        }
        run.push(e.as_ref());
        heavy = heavy.saturating_add(w);
    }
    if !run.is_empty() {
        out.push(vec![(key, run)]);
    }
}

/// The stores other than model space, with their keys, in a fixed order.
fn side_stores(d: &Drawing) -> Vec<(String, &EntityStore)> {
    let mut out = Vec::new();
    for (n, b) in d.blocks.values().enumerate() {
        out.push((format!("block:{n}"), &b.entities));
    }
    for (n, l) in d.layouts.iter().enumerate() {
        out.push((format!("layout:{n}"), &l.entities));
    }
    for l in &d.levels {
        out.push((format!("level:{}", l.id.0), &l.entities));
    }
    for s in &d.sheet_sets {
        for (level, store) in &s.annotations {
            out.push((format!("sheet:{}:{}", s.id.0, level.0), store));
        }
    }
    for v in &d.view_spaces {
        out.push((format!("view:{}", v.rec.id.0), &v.entities));
    }
    out
}

/// Encode a drawing. Parts are built on every core; the result is the same bytes for the same drawing.
pub fn encode(d: &Drawing) -> Result<Vec<Part>> {
    Ok(Encoder::default().encode(d)?.into_iter().map(Arc::unwrap_or_clone).collect())
}

/// Encodes one drawing again and again as it is edited (a save, then an autosave, then a save) and reuses the parts of everything that did not
/// change: a model-space chunk an edit did not touch is recognised by identity and not encoded again, so the cost follows the edit, not the
/// drawing. The result is always exactly what [`encode`] gives.
#[derive(Default)]
pub struct Encoder {
    /// The last drawing encoded. Holding it keeps its chunks alive, so a chunk identity in the cache can only ever mean that same chunk.
    keep: Option<Drawing>,
    model: std::collections::HashMap<usize, Vec<Arc<Part>>>,
    side_key: Vec<(String, usize)>,
    side: Vec<Arc<Part>>,
    /// Parts taken from the cache, and parts built, by the last [`Encoder::encode`].
    pub reused: usize,
    pub built: usize,
}

impl Encoder {
    pub fn encode(&mut self, d: &Drawing) -> Result<Vec<Arc<Part>>> {
        let head = Arc::new(Part::new(pack(&head_of(d))?, 0));
        self.reused = 0;
        self.built = 1;
        // Model space: one or more parts per store chunk, so an unchanged chunk gives unchanged bytes.
        let chunks: Vec<(usize, &[Arc<Entity>])> = d.model.chunk_slices().collect();
        let mut fresh: Vec<(usize, BatchOut<'_>)> = Vec::new();
        for (n, (id, items)) in chunks.iter().enumerate() {
            if !self.model.contains_key(id) {
                let mut cuts = Vec::new();
                cut("model", items, &mut cuts);
                fresh.extend(cuts.into_iter().map(|b| (n, b)));
            }
        }
        // The other stores are usually many and small (block definitions): several share a part. They are encoded again as a set when any changed.
        let side = side_stores(d);
        let side_key: Vec<(String, usize)> =
            side.iter().flat_map(|(key, store)| store.chunk_slices().map(move |(id, _)| (key.clone(), id))).collect();
        let side_same = self.keep.is_some() && side_key == self.side_key;
        let mut side_batches: Vec<BatchOut<'_>> = Vec::new();
        if !side_same {
            let mut open: BatchOut<'_> = Vec::new();
            let mut open_len = 0usize;
            for (key, store) in &side {
                for (_, items) in store.chunk_slices() {
                    let heavy = items.iter().map(|e| weight(e)).fold(0usize, usize::saturating_add);
                    if heavy > PART_WEIGHT {
                        // A store with very large entities is cut like model space, apart from the shared batches.
                        cut(key.as_str(), items, &mut side_batches);
                        continue;
                    }
                    open.push((key.as_str(), items.iter().map(Arc::as_ref).collect()));
                    open_len = open_len.saturating_add(items.len());
                    if open_len >= BATCH {
                        side_batches.push(std::mem::take(&mut open));
                        open_len = 0;
                    }
                }
            }
            if !open.is_empty() {
                side_batches.push(open);
            }
        }
        let build = |b: &BatchOut<'_>| -> Result<Arc<Part>> { Ok(Arc::new(Part::new(pack(b)?, b.iter().map(|(_, items)| items.len()).sum()))) };
        let (model_built, side_built): (Vec<Result<Arc<Part>>>, Vec<Result<Arc<Part>>>) =
            rayon::join(|| fresh.par_iter().map(|(_, b)| build(b)).collect(), || side_batches.par_iter().map(build).collect());
        let mut by_chunk: std::collections::HashMap<usize, Vec<Arc<Part>>> = std::collections::HashMap::new();
        for ((n, _), part) in fresh.iter().zip(model_built) {
            let Some((id, _)) = chunks.get(*n) else { continue };
            by_chunk.entry(*id).or_default().push(part?);
            self.built += 1;
        }
        let mut parts = vec![head];
        let mut model: std::collections::HashMap<usize, Vec<Arc<Part>>> = std::collections::HashMap::with_capacity(chunks.len());
        for (id, _) in &chunks {
            let of_chunk = match by_chunk.remove(id) {
                Some(new) => new,
                None => {
                    let old = self.model.get(id).cloned().unwrap_or_default();
                    self.reused += old.len();
                    old
                }
            };
            parts.extend(of_chunk.iter().cloned());
            model.insert(*id, of_chunk);
        }
        if side_same {
            self.reused += self.side.len();
        } else {
            let mut built = Vec::with_capacity(side_built.len());
            for part in side_built {
                built.push(part?);
            }
            self.built += built.len();
            self.side = built;
            self.side_key = side_key;
        }
        parts.extend(self.side.iter().cloned());
        self.model = model;
        self.keep = Some(d.clone());
        Ok(parts)
    }
}

fn head_of(d: &Drawing) -> HeadOut<'_> {
    HeadOut {
        format: FORMAT_VERSION,
        header: &d.header,
        layers: &d.layers,
        linetypes: &d.linetypes,
        text_styles: &d.text_styles,
        dim_styles: &d.dim_styles,
        mleader_styles: &d.mleader_styles,
        table_styles: &d.table_styles,
        blocks: d.blocks.values().map(Arc::as_ref).collect(),
        levels: d
            .levels
            .iter()
            .map(|l| LevelOut { id: l.id, name: &l.name, elevation: l.elevation, height: l.height, traces: &l.traces, view: l.view })
            .collect(),
        active_level: d.active_level,
        level_seed: d.level_seed,
        sheet_sets: d
            .sheet_sets
            .iter()
            .map(|s| SheetOut {
                id: s.id,
                name: &s.name,
                layer_prefix: &s.layer_prefix,
                space: &s.space,
                dim_style: &s.dim_style,
                text_style: &s.text_style,
                layers: &s.layers,
                blocks: &s.blocks,
                annotation_levels: s.annotations.keys().copied().collect(),
            })
            .collect(),
        active_sheet: d.active_sheet,
        sheet_seed: d.sheet_seed,
        view_spaces: d.view_spaces.iter().map(|v| &v.rec).collect(),
        active_view: d.active_view,
        layouts: &d.layouts,
        views: &d.views,
        ucss: &d.ucss,
        layer_states: &d.layer_states,
        compose_views: &d.compose_views,
        compose_view_seed: d.compose_view_seed,
        master_layouts: &d.master_layouts,
        master_layout_seed: d.master_layout_seed,
        publisher_sets: &d.publisher_sets,
        project: &d.project,
        groups: &d.groups,
        plot_style_tables: &d.plot_style_tables,
        handseed: d.handseed,
    }
}

fn store_index(key: &str, prefix: &str) -> Option<usize> {
    key.strip_prefix(prefix)?.parse().ok()
}

/// Decode parts written by [`encode`], in the same order. A damaged, truncated or newer encoding is an error, never a partial drawing.
pub fn decode<B: AsRef<[u8]> + Sync>(parts: &[B]) -> Result<Drawing> {
    if parts.len() > MAX_PARTS {
        return Err(err("native model: too many parts"));
    }
    let (first, rest) = parts.split_first().ok_or_else(|| err("native model: no parts"))?;
    let head: HeadIn = unpack(first.as_ref())?;
    if head.format != FORMAT_VERSION {
        return Err(err(format!("native model: format {} (this program reads {FORMAT_VERSION})", head.format)));
    }
    let batches: Vec<Result<BatchIn>> = rest.par_iter().map(|p| unpack::<BatchIn>(p.as_ref())).collect();

    let mut d = Drawing::new_imperial();
    d.header = head.header;
    d.layers = head.layers;
    d.linetypes = head.linetypes;
    d.text_styles = head.text_styles;
    d.dim_styles = head.dim_styles;
    d.mleader_styles = head.mleader_styles;
    d.table_styles = head.table_styles;
    d.active_level = head.active_level;
    d.level_seed = head.level_seed;
    d.active_sheet = head.active_sheet;
    d.sheet_seed = head.sheet_seed;
    d.active_view = head.active_view;
    if head.view_spaces.len() > aad_doc::MAX_VIEW_SPACES || head.view_spaces.iter().any(|v| !v.is_valid()) {
        return Err(err("native model: a section or elevation record is not valid"));
    }
    d.view_spaces = head.view_spaces.into_iter().map(|rec| ViewSpace { rec, entities: EntityStore::new() }).collect();
    d.layouts = head.layouts;
    d.views = head.views;
    d.ucss = head.ucss;
    d.layer_states = head.layer_states;
    d.compose_views = head.compose_views;
    d.compose_view_seed = head.compose_view_seed;
    d.master_layouts = head.master_layouts;
    d.master_layout_seed = head.master_layout_seed;
    d.publisher_sets = head.publisher_sets;
    d.project = head.project;
    d.groups = head.groups;
    d.plot_style_tables = head.plot_style_tables;
    d.handseed = head.handseed;
    d.model = EntityStore::new();
    d.levels = head
        .levels
        .into_iter()
        .map(|l| Level {
            id: l.id,
            name: l.name,
            elevation: l.elevation,
            height: l.height,
            traces: l.traces,
            view: l.view,
            entities: EntityStore::new(),
        })
        .collect();
    d.sheet_sets = head
        .sheet_sets
        .into_iter()
        .map(|s| SheetSet {
            id: s.id,
            name: s.name,
            layer_prefix: s.layer_prefix,
            space: s.space,
            dim_style: s.dim_style,
            text_style: s.text_style,
            layers: s.layers,
            blocks: s.blocks,
            annotations: s.annotation_levels.into_iter().map(|l| (l, EntityStore::new())).collect(),
        })
        .collect();
    let mut blocks: Vec<Block> = head.blocks;

    for batch in batches {
        for (key, entities) in batch? {
            let store: &mut EntityStore = if key == "model" {
                &mut d.model
            } else if let Some(n) = store_index(&key, "block:") {
                &mut blocks.get_mut(n).ok_or_else(|| err("native model: entities for a block that is not in the file"))?.entities
            } else if let Some(n) = store_index(&key, "layout:") {
                &mut d.layouts.get_mut(n).ok_or_else(|| err("native model: entities for a layout that is not in the file"))?.entities
            } else if let Some(id) = key.strip_prefix("level:").and_then(|v| v.parse::<u32>().ok()) {
                &mut d
                    .levels
                    .iter_mut()
                    .find(|l| l.id.0 == id)
                    .ok_or_else(|| err("native model: entities for a level that is not in the file"))?
                    .entities
            } else if let Some((set, level)) = key.strip_prefix("sheet:").and_then(|v| v.split_once(':')) {
                let (set, level): (u32, u32) = (
                    set.parse().map_err(|_| err("native model: a bad store key"))?,
                    level.parse().map_err(|_| err("native model: a bad store key"))?,
                );
                d.sheet_sets
                    .iter_mut()
                    .find(|s| s.id.0 == set)
                    .and_then(|s| s.annotations.get_mut(&LevelId(level)))
                    .ok_or_else(|| err("native model: entities for a sheet set that is not in the file"))?
            } else if let Some(id) = key.strip_prefix("view:").and_then(|v| v.parse::<u32>().ok()) {
                &mut d
                    .view_spaces
                    .iter_mut()
                    .find(|v| v.rec.id.0 == id)
                    .ok_or_else(|| err("native model: entities for a view that is not in the file"))?
                    .entities
            } else {
                return Err(err(format!("native model: unknown store `{key}`")));
            };
            for e in entities {
                store.push(e);
            }
        }
    }
    d.blocks = blocks.into_iter().map(|b| (b.name.clone(), Arc::new(b))).collect();
    Ok(d)
}

#[cfg(test)]
mod tests {
    use aad_doc::{EntityKind, Handle, Line};
    use aad_geom::Vec3;

    use super::*;

    fn line(h: u64, x: f64) -> Entity {
        Entity::new(Handle(h), EntityKind::Line(Line { a: Vec3 { x, y: 0.0, z: 0.0 }, b: Vec3 { x, y: 10.0, z: 2.5 } }))
    }

    fn sample() -> Drawing {
        let mut d = Drawing::new_metric();
        for h in 1..=3000u64 {
            d.model.push(line(h, h as f64));
        }
        let mut b = Block::new("DOOR");
        b.entities.push(line(4001, 1.0));
        b.entities.push(line(4002, 2.0));
        d.blocks.insert(b.name.clone(), Arc::new(b));
        d.blocks.insert("EMPTY".into(), Arc::new(Block::new("EMPTY")));
        let mut l = Layout::new("Sheet 1", 1);
        l.entities.push(line(5001, 3.0));
        d.layouts.push(l);
        d.project.insert("k".into(), "{\"a\":1}".into());
        d.header.set_f64("LTSCALE", 2.5);
        d.header.set_str("NAN", "x");
        d.handseed = 6000;
        d
    }

    #[test]
    fn a_drawing_round_trips_exactly() {
        let d = sample();
        let parts = encode(&d).unwrap();
        assert!(parts.len() >= 4, "head, three model chunks, the side stores");
        assert_eq!(parts.iter().map(|p| p.entities).sum::<usize>(), 3003);
        let bytes: Vec<&[u8]> = parts.iter().map(|p| p.bytes.as_slice()).collect();
        let back = decode(&bytes).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.blocks.get("DOOR").unwrap().entities.len(), 2);
    }

    #[test]
    fn levels_and_sheet_sets_keep_their_stores() {
        let mut d = sample();
        let mut store = EntityStore::new();
        store.push(line(7001, 9.0));
        d.levels.push(Level {
            id: LevelId(3),
            name: "First".into(),
            elevation: 3000.0,
            height: 3000.0,
            traces: Vec::new(),
            view: None,
            entities: store.clone(),
        });
        d.level_seed = 3;
        d.sheet_sets.push(SheetSet {
            id: SheetSetId(2),
            name: "Electrical".into(),
            layer_prefix: "E-".into(),
            space: None,
            dim_style: None,
            text_style: Some("Standard".into()),
            layers: vec!["E-*".into()],
            blocks: Vec::new(),
            annotations: [(LevelId(3), store), (LevelId(9), EntityStore::new())].into_iter().collect(),
        });
        let parts = encode(&d).unwrap();
        let back = decode(&parts.iter().map(|p| p.bytes.as_slice()).collect::<Vec<_>>()).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.sheet_sets.first().unwrap().annotations.len(), 2);
    }

    #[test]
    fn views_keep_their_records_and_stores_and_the_active_view() {
        let mut d = sample();
        d.materialize_levels("Ground", 0.0, 3000.0).unwrap();
        let g = d.active_level.unwrap();
        let v = d
            .add_view_space(
                aad_doc::ViewKind::Elevation,
                "North",
                g,
                (Vec2::new(0.0, 0.0), Vec2::new(8000.0, 0.0)),
                aad_doc::ViewLook::Right,
                4000.0,
                Some(Handle(77)),
            )
            .unwrap();
        d.enter_view_space(v).unwrap();
        d.model.push(line(8001, 4.0));
        let parts = encode(&d).unwrap();
        let back = decode(&parts.iter().map(|p| p.bytes.as_slice()).collect::<Vec<_>>()).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.active_view, Some(v));
        assert_eq!(back.view_spaces.first().unwrap().rec.marker, Some(Handle(77)));
        // A damaged record is refused, not half-loaded.
        let mut bad = d.clone();
        bad.view_spaces[0].rec.depth = f64::NAN;
        let parts = encode(&bad).unwrap();
        assert!(decode(&parts.iter().map(|p| p.bytes.as_slice()).collect::<Vec<_>>()).is_err());
    }

    #[test]
    fn non_finite_numbers_survive() {
        let mut d = Drawing::new_metric();
        d.model.push(line(1, f64::NAN));
        d.model.push(line(2, f64::INFINITY));
        let parts = encode(&d).unwrap();
        let back = decode(&parts.iter().map(|p| p.bytes.as_slice()).collect::<Vec<_>>()).unwrap();
        let xs: Vec<f64> = back
            .model
            .iter()
            .map(|e| match &e.kind {
                EntityKind::Line(l) => l.a.x,
                _ => 0.0,
            })
            .collect();
        assert!(xs.first().is_some_and(|x| x.is_nan()) && xs.get(1) == Some(&f64::INFINITY));
    }

    #[test]
    fn an_edit_changes_only_the_parts_it_touched() {
        let d = sample();
        let before = encode(&d).unwrap();
        let mut e = d.clone();
        e.model.modify(Handle(5), |x| x.common.layer = "A-WALL".into());
        let after = encode(&e).unwrap();
        assert_eq!(before.len(), after.len());
        let changed = before.iter().zip(&after).filter(|(a, b)| a.bytes != b.bytes).count();
        assert_eq!(changed, 1, "one model chunk");
    }

    #[test]
    fn an_encoder_reuses_what_did_not_change_and_gives_the_same_bytes() {
        let d = sample();
        let mut enc = Encoder::default();
        let first = enc.encode(&d).unwrap();
        assert_eq!(first.iter().map(|p| (*p).as_ref().clone()).collect::<Vec<_>>(), encode(&d).unwrap());
        assert_eq!(enc.reused, 0);
        // Nothing changed: everything but the head comes from the cache.
        let again = enc.encode(&d).unwrap();
        assert_eq!((enc.built, enc.reused), (1, first.len() - 1));
        assert_eq!(again, first);
        // An edit in model space: one chunk is encoded (entity 2500 and the appended line are both in the last chunk of 1024).
        let mut e = d.clone();
        e.model.modify(Handle(2500), |x| x.common.layer = "A-WALL".into());
        e.model.push(line(9000, 1.0));
        let after = enc.encode(&e).unwrap();
        assert_eq!(enc.built, 2, "the head and the one chunk touched");
        assert_eq!(after.iter().map(|p| (*p).as_ref().clone()).collect::<Vec<_>>(), encode(&e).unwrap());
        // An edit in the first chunk: that chunk is encoded, the others come from the cache.
        let mut e = e.clone();
        e.model.modify(Handle(3), |x| x.common.layer = "A-DOOR".into());
        let after_first = enc.encode(&e).unwrap();
        assert_eq!((enc.built, enc.reused), (2, after_first.len() - 2));
        assert_eq!(after_first.iter().map(|p| (*p).as_ref().clone()).collect::<Vec<_>>(), encode(&e).unwrap());
        // An edit in a block: the side stores are encoded again, model space is not.
        let mut f = e.clone();
        let mut b = Block::new("WINDOW");
        b.entities.push(line(9100, 1.0));
        f.blocks.insert(b.name.clone(), Arc::new(b));
        let after = enc.encode(&f).unwrap();
        assert!(enc.reused >= 3 && enc.built >= 2);
        let fresh = encode(&f).unwrap();
        assert_eq!(after.iter().map(|p| (*p).as_ref().clone()).collect::<Vec<_>>(), fresh);
        assert_eq!(decode(&fresh.iter().map(|p| p.bytes.as_slice()).collect::<Vec<_>>()).unwrap(), f);
        assert!(fresh.iter().all(|p| p.hash.len() == 64));
    }

    #[test]
    fn damaged_or_hostile_parts_are_errors_not_panics() {
        let d = sample();
        let parts = encode(&d).unwrap();
        let bytes: Vec<Vec<u8>> = parts.iter().map(|p| p.bytes.clone()).collect();
        assert!(decode::<Vec<u8>>(&[]).is_err());
        assert!(decode(&[Vec::<u8>::new()]).is_err());
        assert!(decode(&[vec![9u8, 1, 2, 3]]).is_err());
        // Truncated parts, flipped bytes, parts in the wrong place.
        for cut in [1usize, 2, 7, 40] {
            let mut b = bytes.clone();
            if let Some(p) = b.get_mut(1) {
                p.truncate(cut);
            }
            assert!(decode(&b).is_err(), "cut {cut}");
        }
        let mut swapped = bytes.clone();
        swapped.swap(0, 1);
        assert!(decode(&swapped).is_err());
        let mut seed = 0x1234_5678_9ABC_DEF1u64;
        for _ in 0..200 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mut b = bytes.clone();
            let n = b.len();
            if let Some(p) = b.get_mut((seed as usize) % n) {
                let at = ((seed >> 16) as usize) % p.len();
                if let Some(x) = p.get_mut(at) {
                    *x ^= (seed >> 40) as u8 | 1;
                }
            }
            let _ = decode(&b); // an error or a drawing: never a panic
        }
        // A deflate stream that would inflate past the limit is refused by size, not by running out of memory.
        let bomb = miniz_oxide::deflate::compress_to_vec(&vec![0u8; 8 << 20], 6);
        let mut part = vec![CODEC_DEFLATE];
        part.extend_from_slice(&bomb);
        assert!(decode(&[part]).is_err());
    }
}

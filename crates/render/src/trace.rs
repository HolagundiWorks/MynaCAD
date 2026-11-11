//! Level traces: another level shown read-only, faded, under or over the level being edited.
//!
//! A trace is a display list of the traced level's visible objects whose colours are greyed and
//! blended towards the background, whose lines are drawn at the thinnest weight, and whose
//! primitives carry [`VIEWPORT_CONTENT`] as their handle so selection highlighting never matches
//! them. The traces are merged around the main list in draw order: traces marked Below, the edit
//! target, traces marked Above. The result is one ordinary [`DisplayList`], so the GPU mesh, the
//! raster and the egui canvas need no change. The fade bakes in the background colour, so a list
//! built here must be rebuilt when the theme changes.

use aad_color::Rgb;
use aad_doc::{Drawing, Space, Trace, TraceColour, TraceOrder};

use crate::{DPrim, DisplayList, Kind, Options, VIEWPORT_CONTENT, build, build_entities};

/// Grey and fade the primitives of a trace list in place. Wipeouts are dropped (they would hide
/// the level being edited), lineweights go to the thinnest, and colour 7 is resolved against
/// `background` first so the result no longer depends on it.
pub fn stylize_trace(list: &mut DisplayList, fade: u8, true_colour: bool, background: Rgb) {
    let f = f32::from(fade.min(90)) / 100.0;
    let mix = |a: u8, b: u8| (f32::from(a) * (1.0 - f) + f32::from(b) * f).round().clamp(0.0, 255.0) as u8;
    list.prims.retain(|p| p.kind != Kind::Mask);
    for p in &mut list.prims {
        let c = p.display_rgb(background);
        let c = if true_colour {
            c
        } else {
            let l = (0.299 * f32::from(c.0) + 0.587 * f32::from(c.1) + 0.114 * f32::from(c.2)).round().clamp(0.0, 255.0) as u8;
            Rgb(l, l, l)
        };
        p.color = Rgb(mix(c.0, background.0), mix(c.1, background.1), mix(c.2, background.2));
        p.aci7 = false;
        p.lw = 0.0;
        p.handle = VIEWPORT_CONTENT;
    }
}

/// Append `src` after `dst`'s primitives, moving its vertex references.
pub(crate) fn append(dst: &mut DisplayList, src: DisplayList) {
    let (vo, to) = (dst.verts.len() as u32, dst.tris.len() as u32);
    dst.prims.extend(src.prims.into_iter().map(|p: DPrim| match p.kind {
        Kind::Tris | Kind::Mask => DPrim { start: p.start + to, ..p },
        _ => DPrim { start: p.start + vo, ..p },
    }));
    dst.verts.extend(src.verts);
    dst.tris.extend(src.tris);
    dst.bounds = dst.bounds.union(&src.bounds);
    dst.expanded = dst.expanded.saturating_add(src.expanded);
}

/// The display list of one trace: the visible objects of the traced level, restricted to the
/// trace's layer list when it has one.
fn trace_list(d: &Drawing, t: &Trace, opts: &Options, background: Rgb) -> Option<DisplayList> {
    let store = d.level_store(t.level)?;
    let keep = |e: &aad_doc::Entity| d.is_visible(e) && t.layers.as_ref().is_none_or(|ls| ls.iter().any(|n| n.eq_ignore_ascii_case(&e.common.layer)));
    let mut list = build_entities(d, store.iter().map(|e| e.as_ref()).filter(|e| keep(e)), opts);
    stylize_trace(&mut list, t.fade, t.colour == TraceColour::TrueColour, background);
    Some(list)
}

/// How faded the level shown for reference in a sheet view is (percent).
pub const REFERENCE_FADE: u8 = 35;

/// The level shown for reference in a sheet view: its visible objects in their own colours,
/// lightly faded, so the annotation drawn over it stands out.
fn reference_list(d: &Drawing, opts: &Options, background: Rgb) -> Option<DisplayList> {
    let store = d.reference_store()?;
    let mut list = build_entities(d, store.iter().map(|e| e.as_ref()).filter(|e| d.is_visible(e)), opts);
    stylize_trace(&mut list, REFERENCE_FADE, true, background);
    Some(list)
}

/// How faded the projection guides of a section or elevation are (percent).
pub const GUIDE_FADE: u8 = 55;

/// Model space of the edit target with what is shown around it: Below traces, the level shown
/// for reference (in a sheet view), the edit target, Above traces. A drawing without levels, or
/// a level without traces or sheet view, gives exactly [`build`].
pub fn build_with_traces(d: &Drawing, opts: &Options, background: Rgb) -> DisplayList {
    build_with_guides(d, opts, background, &[])
}

/// Like [`build_with_traces`], also drawing `guides` (transient read-only line objects such as the
/// projection guides of a section) and, while a section or elevation is open, the other view
/// spaces it traces, all faded under the edit target. They carry [`VIEWPORT_CONTENT`] as their
/// handle, so they are never selected.
pub fn build_with_guides(d: &Drawing, opts: &Options, background: Rgb, guides: &[aad_doc::Entity]) -> DisplayList {
    let main = build(d, &Space::Model, opts);
    let reference = reference_list(d, opts, background);
    let traces: &[Trace] = if d.active_view.is_some() { &[] } else { d.active_level_ref().map(|l| l.traces.as_slice()).unwrap_or(&[]) };
    let mut under: Vec<DisplayList> = Vec::new();
    if let Some(v) = d.active_view_ref() {
        for id in &v.rec.traces {
            if let Some(store) = d.view_store(*id) {
                let mut l = build_entities(d, store.iter().map(|e| e.as_ref()).filter(|e| d.is_visible(e)), opts);
                stylize_trace(&mut l, 60, false, background);
                under.push(l);
            }
        }
    }
    if !guides.is_empty() {
        let mut l = build_entities(d, guides.iter(), opts);
        stylize_trace(&mut l, GUIDE_FADE, false, background);
        under.push(l);
    }
    if reference.is_none() && traces.is_empty() && under.is_empty() {
        return main;
    }
    let (mut below, mut above) = (Vec::new(), Vec::new());
    for t in traces {
        if let Some(l) = trace_list(d, t, opts, background) {
            if t.order == TraceOrder::Below { below.push(l) } else { above.push(l) }
        }
    }
    let mut out = DisplayList::default();
    for l in under.into_iter().chain(below) {
        append(&mut out, l);
    }
    if let Some(r) = reference {
        append(&mut out, r);
    }
    append(&mut out, main);
    for l in above {
        append(&mut out, l);
    }
    out
}

// ---------------- layout viewports ----------------

/// One store a layout viewport draws, with the trace settings when it is a trace.
pub(crate) struct Source<'a> {
    pub store: &'a aad_doc::EntityStore,
    pub trace: Option<&'a Trace>,
}

/// The stores a layout viewport draws, in draw order: its Below traces, the level it shows, that
/// level's annotation for its sheet set, its Above traces. A viewport with no binding shows the
/// first level (what every older file shows); a drawing without levels shows `model`.
pub(crate) fn viewport_sources<'a>(d: &'a Drawing, vp: &'a aad_doc::Viewport) -> Vec<Source<'a>> {
    if d.levels.is_empty() {
        return vec![Source { store: &d.model, trace: None }];
    }
    let level = vp.level.filter(|id| d.level(*id).is_some()).or_else(|| d.levels.first().map(|l| l.id));
    let Some(level) = level else { return vec![Source { store: &d.model, trace: None }] };
    let mut out = Vec::new();
    let traces = |order: TraceOrder| vp.traces.iter().filter(move |t| t.order == order && t.level != level);
    for t in traces(TraceOrder::Below) {
        if let Some(store) = d.level_store(t.level) {
            out.push(Source { store, trace: Some(t) });
        }
    }
    if let Some(store) = d.level_store(level) {
        out.push(Source { store, trace: None });
    }
    if let Some(set) = vp.sheet_set.filter(|s| d.sheet_set(*s).is_some())
        && let Some(store) = d.annotation_store(set, level)
    {
        out.push(Source { store, trace: None });
    }
    for t in traces(TraceOrder::Above) {
        if let Some(store) = d.level_store(t.level) {
            out.push(Source { store, trace: Some(t) });
        }
    }
    out
}

/// Fold the identity of the sources into a tessellation cache key.
pub(crate) fn hash_sources(sources: &[Source<'_>], h: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    for s in sources {
        s.store.len().hash(h);
        for e in s.store.iter() {
            (std::sync::Arc::as_ptr(e) as usize).hash(h);
        }
        match s.trace {
            Some(t) => {
                1u8.hash(h);
                (t.fade, t.colour as u8, t.level.0).hash(h);
                t.layers.hash(h);
            }
            None => 0u8.hash(h),
        }
    }
}

/// Tessellate the sources of a viewport in model (world) coordinates. Traces are greyed and faded
/// towards white paper. `win` limits the objects tessellated (entity bounds are approximate).
pub(crate) fn tessellate_sources(
    b: &crate::Builder<'_>,
    d: &Drawing,
    vp: &aad_doc::Viewport,
    sources: &[Source<'_>],
    lt_factor: f64,
    win: aad_geom::Bounds2,
) -> DisplayList {
    let mut out = DisplayList::default();
    for src in sources {
        let mut part =
            crate::Builder { list: DisplayList::default(), opts: b.opts, plotting: b.plotting, styles: b.styles, named: None, budget: None };
        for me in src.store.iter() {
            if matches!(me.kind, aad_doc::EntityKind::Viewport(_)) {
                continue;
            }
            if src.trace.is_some_and(|t| t.layers.as_ref().is_some_and(|ls| !ls.iter().any(|n| n.eq_ignore_ascii_case(&me.common.layer)))) {
                continue;
            }
            if !matches!(me.kind, aad_doc::EntityKind::Ray(_) | aad_doc::EntityKind::XLine(_)) {
                let eb = aad_doc::entity_bounds(d, me, 0);
                if !eb.is_empty() && !eb.intersects(&win) {
                    continue;
                }
            }
            let ctx = crate::Ctx { lt_factor, ..crate::top_ctx(d, aad_geom::Mat3::IDENTITY, VIEWPORT_CONTENT, &vp.frozen_layers, &vp.layer_colors) };
            crate::entity(&mut part, &ctx, me);
        }
        if let Some(t) = src.trace {
            stylize_trace(&mut part.list, t.fade, t.colour == TraceColour::TrueColour, Rgb(255, 255, 255));
        }
        append(&mut out, part.list);
    }
    out
}

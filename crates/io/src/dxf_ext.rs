//! DXF data shared by the reader and the writer beyond plain entity geometry:
//! dimension-variable group codes (DIMSTYLE records and the `ACAD` "DSTYLE" override xdata,
//! both per the DXF Reference), arrowhead block names, extended-data helpers and the
//! AAD-owned payloads (exact associativity, table flags, hatch gradients).
//!
//! AAD's own data lives under the registered application `AAD` (xdata) and the
//! named-object-dictionary entries `AAD_LAYERSTATES` and
//! `AAD_PLOTSTYLES` (XRECORDs). Other readers keep or ignore them.

use std::collections::HashMap;

use aad_color::Color;
use aad_doc::{AssocSnap, DimAssoc, DimStyle, Handle, LayerState, PlotStyleTable};
use aad_dxf::Tag;
use aad_render::Arrowhead;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Registered application name of AAD's extended data.
pub(crate) const APP: &str = "AAD";
/// The application name older files carry in xdata (before the rename to AAD): still read, never written.
pub(crate) const LEGACY_APP: &str = "CADCRAFT";
/// Named-object-dictionary key of the saved layer states XRECORD.
pub(crate) const LAYER_STATES_KEY: &str = "AAD_LAYERSTATES";
/// Named-object-dictionary key of the drawing's plot style tables XRECORD.
pub(crate) const PLOT_STYLES_KEY: &str = "AAD_PLOTSTYLES";
/// Named-object-dictionary key of the plot style names dictionary (DXF Reference): its entries
/// name `ACDBPLACEHOLDER` objects, and layers and objects point at them with group 390.
pub(crate) const PLOT_STYLE_NAMES_KEY: &str = "ACAD_PLOTSTYLENAME";
/// Most plot style tables and plot style names read from a file.
pub(crate) const MAX_PLOT_STYLE_TABLES: usize = 1000;
pub(crate) const MAX_PLOT_STYLE_NAMES: usize = 100_000;

/// Caps for hostile input.
pub(crate) const MAX_XDATA_ITEMS: usize = 4096;
pub(crate) const MAX_PAYLOAD: usize = 64 << 20;
pub(crate) const MAX_LAYER_STATES: usize = 100_000;

/// How a dimension variable is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum K {
    Real,
    Int,
    Bool,
    Str,
    /// ACI colour number (0 ByBlock, 256 ByLayer).
    Color,
    /// A character stored as its code (DIMDSEP).
    Char,
    /// Handle of a STYLE record (DIMTXSTY).
    TextStyle,
    /// Handle of an arrowhead BLOCK_RECORD (DIMBLK, DIMBLK1, DIMBLK2).
    Block,
}

/// [`DimStyle`] field (camelCase) → DIMSTYLE group code (DXF Reference, DIMSTYLE table). The
/// same codes identify variables in the DSTYLE override xdata.
pub(crate) const DIM_CODES: &[(&str, i32, K)] = &[
    ("post", 3, K::Str),
    ("altPost", 4, K::Str),
    ("scale", 40, K::Real),
    ("arrowSize", 41, K::Real),
    ("extOffset", 42, K::Real),
    ("baselineSpacing", 43, K::Real),
    ("extExtend", 44, K::Real),
    ("round", 45, K::Real),
    ("dimLineExtend", 46, K::Real),
    ("tolPlus", 47, K::Real),
    ("tolMinus", 48, K::Real),
    ("textHeight", 140, K::Real),
    ("centerMark", 141, K::Real),
    ("tickSize", 142, K::Real),
    ("altFactor", 143, K::Real),
    ("linearFactor", 144, K::Real),
    ("tolScale", 146, K::Real),
    ("textGap", 147, K::Real),
    ("altRound", 148, K::Real),
    ("tolerance", 71, K::Bool),
    ("limits", 72, K::Bool),
    ("textInsideHorizontal", 73, K::Bool),
    ("textOutsideHorizontal", 74, K::Bool),
    ("suppressExt1", 75, K::Bool),
    ("suppressExt2", 76, K::Bool),
    ("textAbove", 77, K::Int),
    ("zeroSuppression", 78, K::Int),
    ("alt", 170, K::Bool),
    ("altDecimals", 171, K::Int),
    ("dimLineColor", 176, K::Color),
    ("extLineColor", 177, K::Color),
    ("textColor", 178, K::Color),
    ("angularDecimals", 179, K::Int),
    ("decimals", 271, K::Int),
    ("tolDecimals", 272, K::Int),
    ("altUnit", 273, K::Int),
    ("angularUnit", 275, K::Int),
    ("fractionFormat", 276, K::Int),
    ("linearUnit", 277, K::Int),
    ("decimalSeparator", 278, K::Char),
    ("textJust", 280, K::Int),
    ("tolZeroSuppression", 284, K::Int),
    ("altZeroSuppression", 285, K::Int),
    ("textStyle", 340, K::TextStyle),
    ("arrowBlock", 342, K::Block),
    ("arrowBlock1", 343, K::Block),
    ("arrowBlock2", 344, K::Block),
];

/// DIMSAH: separate arrow blocks in use.
pub(crate) const DIMSAH: i32 = 173;

pub(crate) fn dim_code(field: &str) -> Option<(i32, K)> {
    DIM_CODES.iter().find(|(f, _, _)| *f == field).map(|(_, c, k)| (*c, *k))
}
pub(crate) fn dim_field(code: i32) -> Option<(&'static str, K)> {
    DIM_CODES.iter().find(|(_, c, _)| *c == code).map(|(f, _, k)| (*f, *k))
}

/// The canonical JSON of a dimension-variable value (as a serialised [`DimStyle`] holds it),
/// or `None` when the value is unusable for the field.
pub(crate) fn canonical(field: &str, v: &Value) -> Option<Value> {
    let field = DimStyle::field_name(field)?;
    let mut st = DimStyle::default();
    let mut m = serde_json::Map::new();
    m.insert(field.to_string(), v.clone());
    if !st.apply_fields(&m).is_empty() {
        return None;
    }
    serde_json::to_value(&st).ok()?.get(field).cloned()
}

/// A dimension variable encoded for a group: real, integer, string or handle.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DimVal {
    Real(f64),
    Int(i64),
    Str(String),
    Handle(String),
}

/// Encode a canonical field value. `style` / `block` map names to handles.
pub(crate) fn encode(kind: K, v: &Value, style: &dyn Fn(&str) -> Option<String>, block: &dyn Fn(&str) -> Option<String>) -> Option<DimVal> {
    Some(match kind {
        K::Real => DimVal::Real(v.as_f64().filter(|x| x.is_finite())?),
        K::Int => DimVal::Int(v.as_i64()?),
        K::Bool => DimVal::Int(i64::from(v.as_bool()?)),
        K::Str => DimVal::Str(v.as_str()?.to_string()),
        K::Color => DimVal::Int(i64::from(serde_json::from_value::<Color>(v.clone()).ok()?.to_aci())),
        K::Char => DimVal::Int(i64::from(u32::from(v.as_str()?.chars().next().unwrap_or('.')))),
        K::TextStyle => DimVal::Handle(style(v.as_str()?)?),
        K::Block => {
            let name = v.as_str()?;
            DimVal::Handle(if name.trim().is_empty() { "0".to_string() } else { block(name)? })
        }
    })
}

/// Decode a group value into the JSON a [`DimStyle`] field takes. `styles` / `blocks` map
/// upper-case handles to names.
pub(crate) fn decode(kind: K, t: &Tag, styles: &HashMap<String, String>, blocks: &HashMap<String, String>) -> Option<Value> {
    Some(match kind {
        K::Real => {
            let x = t.f64();
            if !x.is_finite() {
                return None;
            }
            serde_json::json!(x)
        }
        K::Int => serde_json::json!(t.i64()),
        K::Bool => Value::Bool(t.i64() != 0),
        K::Str => Value::String(t.str()),
        K::Color => serde_json::to_value(Color::from_aci(i16::try_from(t.i64()).ok()?)).ok()?,
        K::Char => Value::String(char::from_u32(u32::try_from(t.i64()).ok()?).filter(|c| !c.is_control())?.to_string()),
        K::TextStyle => Value::String(styles.get(&t.str().trim().to_ascii_uppercase())?.clone()),
        K::Block => {
            let h = t.str().trim().to_ascii_uppercase();
            if h.is_empty() || h == "0" { Value::String(String::new()) } else { Value::String(blocks.get(&h)?.clone()) }
        }
    })
}

/// Block names of the arrowheads (the DIMBLK names of the DXF Reference); geometry is ours.
pub(crate) const ARROWS: &[(Arrowhead, &str)] = &[
    (Arrowhead::ClosedFilled, "_ClosedFilled"),
    (Arrowhead::ClosedBlank, "_ClosedBlank"),
    (Arrowhead::Closed, "_Closed"),
    (Arrowhead::Dot, "_Dot"),
    (Arrowhead::DotSmall, "_DotSmall"),
    (Arrowhead::DotBlank, "_DotBlank"),
    (Arrowhead::Oblique, "_Oblique"),
    (Arrowhead::ArchTick, "_ArchTick"),
    (Arrowhead::Open, "_Open"),
    (Arrowhead::Open90, "_Open90"),
    (Arrowhead::Open30, "_Open30"),
    (Arrowhead::Origin, "_Origin"),
    (Arrowhead::Origin2, "_Origin2"),
    (Arrowhead::Small, "_Small"),
    (Arrowhead::BoxFilled, "_BoxFilled"),
    (Arrowhead::BoxBlank, "_BoxBlank"),
    (Arrowhead::DatumFilled, "_DatumFilled"),
    (Arrowhead::DatumBlank, "_DatumBlank"),
    (Arrowhead::Integral, "_Integral"),
    (Arrowhead::None, "_None"),
];

/// True for a standard arrowhead block name (case-insensitive).
pub(crate) fn is_standard_arrow(name: &str) -> bool {
    ARROWS.iter().any(|(_, n)| n.eq_ignore_ascii_case(name))
}

/// The block an arrow name is written as when no user block has that name: names starting
/// with `_` are kept (block names are case-insensitive), other recognised names become the
/// standard name; unrecognised names (which draw as closed filled) give `None`.
pub(crate) fn arrow_block_name(name: &str) -> Option<(String, Arrowhead)> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let kind = Arrowhead::parse(n);
    let norm: String = n.trim_start_matches('_').chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    if kind == Arrowhead::ClosedFilled && !matches!(norm.as_str(), "closedfilled" | "filled") {
        return None;
    }
    let std_name = ARROWS.iter().find(|(k, _)| *k == kind).map(|(_, s)| s.to_string())?;
    let keep = n.starts_with('_') && n.chars().skip(1).all(|c| c.is_ascii_alphanumeric());
    Some((if keep { n.to_string() } else { std_name }, kind))
}

/// Applications whose xdata carries layer properties: transparency and description.
pub(crate) const LAYER_TRANSPARENCY_APP: &str = "AcCmTransparency";
pub(crate) const LAYER_DESCRIPTION_APP: &str = "AcAecLayerStandard";

/// A transparency percentage (0..=90) as an `AcCmTransparency` value: the alpha with the
/// "by alpha" flag (0x02 in the top byte).
pub(crate) fn transparency_to_dxf(percent: u8) -> i64 {
    let t = u32::from(percent.min(90));
    i64::from(0x0200_0000 | ((100 - t) * 255 / 100))
}

/// The percentage of an `AcCmTransparency` value; `None` unless it is "by alpha".
pub(crate) fn transparency_from_dxf(v: i64) -> Option<u8> {
    if (v >> 24) & 0xff != 2 {
        return None;
    }
    let alpha = u32::try_from(v & 0xff).ok()?;
    let opaque = (alpha * 100 + 127) / 255;
    u8::try_from(100u32.saturating_sub(opaque).min(90)).ok()
}

/// An xdata string cut to the 255 bytes a `1000` group may hold, at a character boundary.
pub(crate) fn xdata_str(s: &str) -> &str {
    let mut end = s.len().min(255);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// The extended data of one application: the groups after its `1001` up to the next `1001`.
pub(crate) fn xdata<'a>(tags: &'a [Tag], app: &str) -> &'a [Tag] {
    let is_app =
        |t: &Tag| t.code == 1001 && (t.str().trim().eq_ignore_ascii_case(app) || (app == APP && t.str().trim().eq_ignore_ascii_case(LEGACY_APP)));
    let Some(start) = tags.iter().position(is_app) else { return &[] };
    let rest = tags.get(start + 1..).unwrap_or(&[]);
    let end = rest.iter().position(|t| t.code == 1001).unwrap_or(rest.len());
    rest.get(..end).unwrap_or(&[])
}

/// The groups inside `1000 <name>` `1002 {` … `1002 }` of an xdata run (nested braces kept).
pub(crate) fn xdata_list<'a>(x: &'a [Tag], name: &str) -> &'a [Tag] {
    let Some(at) = x.iter().position(|t| t.code == 1000 && t.str().trim().eq_ignore_ascii_case(name)) else { return &[] };
    let Some(open) = x.get(at + 1).filter(|t| t.code == 1002 && t.str().trim() == "{") else { return &[] };
    let _ = open;
    let body = x.get(at + 2..).unwrap_or(&[]);
    let mut depth = 0usize;
    for (i, t) in body.iter().enumerate() {
        if t.code == 1002 {
            match t.str().trim() {
                "{" => depth += 1,
                "}" if depth == 0 => return body.get(..i).unwrap_or(&[]),
                "}" => depth -= 1,
                _ => {}
            }
        }
    }
    body
}

/// Owner handle of a record: the first 330 outside `102 {…}` groups.
pub(crate) fn owner(tags: &[Tag]) -> Option<String> {
    let mut in_group = false;
    for t in tags {
        match t.code {
            102 => in_group = t.str().trim_start().starts_with('{'),
            330 if !in_group => return Some(t.str()),
            c if c >= 1000 => return None,
            _ => {}
        }
    }
    None
}

/// Per-dimension overrides from the `ACAD` "DSTYLE" xdata: `1070 <code>` followed by the value.
pub(crate) fn read_dstyle(tags: &[Tag], styles: &HashMap<String, String>, blocks: &HashMap<String, String>) -> serde_json::Map<String, Value> {
    let list = xdata_list(xdata(tags, "ACAD"), "DSTYLE");
    let mut out = serde_json::Map::new();
    let mut it = list.iter().take(MAX_XDATA_ITEMS * 2);
    while let Some(t) = it.next() {
        if t.code != 1070 {
            continue;
        }
        let code = t.i64();
        let Some(v) = it.next() else { break };
        let Some((field, kind)) = i32::try_from(code).ok().and_then(dim_field) else { continue };
        if let Some(val) = decode(kind, v, styles, blocks).and_then(|j| canonical(field, &j)) {
            out.insert(field.to_string(), val);
        }
    }
    out
}

/// Xdata marker of the paper-space viewport (id 1) the writer adds to layouts that have none;
/// AAD keeps that viewport implicit, so the reader drops it.
pub(crate) const PAPER_VIEW: &str = "PAPERVIEW";

/// True for a VIEWPORT record carrying the [`PAPER_VIEW`] marker as the first string of its AAD
/// xdata, where the writer puts it. Later strings are layer names (VPFROZEN, VPCOLORS), so a layer
/// called PAPERVIEW does not count.
pub(crate) fn is_paper_view(tags: &[Tag]) -> bool {
    xdata(tags, APP).iter().find(|t| t.code == 1000).is_some_and(|t| t.str().trim().eq_ignore_ascii_case(PAPER_VIEW))
}

/// Snap kinds in AAD's ASSOC xdata.
fn snap_code(s: &AssocSnap) -> i64 {
    match s {
        AssocSnap::Start => 0,
        AssocSnap::End => 1,
        AssocSnap::Mid => 2,
        AssocSnap::Center => 3,
        AssocSnap::OnCircle { .. } => 4,
        AssocSnap::Intersection { .. } => 5,
        AssocSnap::Vertex { .. } => 6,
    }
}

/// Exact associativity as AAD xdata groups: per link `1000 point`, `1005 object`,
/// `1070 snap` and the snap's argument (`1040 angle`, `1005 other object`, `1071 vertex`).
pub(crate) fn assoc_xdata(assoc: &[DimAssoc]) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "ASSOC"), Tag::s(1002, "{")];
    for a in assoc {
        v.push(Tag::s(1002, "{"));
        v.push(Tag::s(1000, a.point.clone()));
        v.push(Tag::s(1005, a.handle.hex()));
        v.push(Tag::i(1070, snap_code(&a.snap)));
        match &a.snap {
            AssocSnap::OnCircle { angle } => v.push(Tag::f(1040, *angle)),
            AssocSnap::Intersection { other } => v.push(Tag::s(1005, other.hex())),
            AssocSnap::Vertex { index } => v.push(Tag::i(1071, i64::try_from(*index).unwrap_or(i64::from(i32::MAX)).min(i64::from(i32::MAX)))),
            _ => {}
        }
        v.push(Tag::s(1002, "}"));
    }
    v.push(Tag::s(1002, "}"));
    v
}

/// An arc-length dimension as AAD xdata: `1000 ARCLEN`, `1002 {`, the arc centre as an xdata
/// point (`1010`/`1020`/`1030`), `1002 }`. The record itself stays a standard aligned dimension
/// (DXF has no arc-length DIMENSION type), so other programs still read it.
pub(crate) fn arclen_xdata(center: aad_geom::Vec3) -> Vec<Tag> {
    vec![Tag::s(1000, "ARCLEN"), Tag::s(1002, "{"), Tag::f(1010, center.x), Tag::f(1020, center.y), Tag::f(1030, center.z), Tag::s(1002, "}")]
}

/// The arc centre written by [`arclen_xdata`]; None for other dimensions and other writers.
pub(crate) fn read_arclen(tags: &[Tag]) -> Option<aad_geom::Vec3> {
    let list = xdata_list(xdata(tags, APP), "ARCLEN");
    let g = |c: i32| list.iter().find(|t| t.code == c).map(Tag::f64).filter(|v| v.is_finite());
    Some(aad_geom::Vec3::new(g(1010)?, g(1020)?, g(1030).unwrap_or(0.0)))
}

/// A viewport's frozen layer names as AAD xdata: `1000 VPFROZEN`, `1002 {`, one `1000` per layer, `1002 }`.
pub(crate) fn frozen_xdata(layers: &[String]) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "VPFROZEN"), Tag::s(1002, "{")];
    v.extend(layers.iter().map(|l| Tag::s(1000, l.clone())));
    v.push(Tag::s(1002, "}"));
    v
}

/// The frozen layer names written by [`frozen_xdata`]; empty for files from other writers.
pub(crate) fn read_frozen(tags: &[Tag]) -> Vec<String> {
    let list = xdata_list(xdata(tags, APP), "VPFROZEN");
    list.iter().take(MAX_XDATA_ITEMS).filter(|t| t.code == 1000).map(Tag::str).collect()
}

/// A viewport's layer colour overrides as AAD xdata: `1000 VPCOLORS`, `1002 {`, then per override
/// `1000 layer` and `1000 colour` (the [`Color::name`] text, which keeps true colours), `1002 }`.
pub(crate) fn layer_colors_xdata(colors: &[(String, Color)]) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "VPCOLORS"), Tag::s(1002, "{")];
    for (layer, color) in colors {
        v.push(Tag::s(1000, layer.clone()));
        v.push(Tag::s(1000, color.name()));
    }
    v.push(Tag::s(1002, "}"));
    v
}

/// The overrides written by [`layer_colors_xdata`]; empty for files from other writers, bad pairs are skipped.
pub(crate) fn read_layer_colors(tags: &[Tag]) -> Vec<(String, Color)> {
    let list = xdata_list(xdata(tags, APP), "VPCOLORS");
    let names: Vec<String> = list.iter().take(MAX_XDATA_ITEMS * 2).filter(|t| t.code == 1000).map(Tag::str).collect();
    names.as_chunks::<2>().0.iter().filter_map(|[name, color]| Some((name.clone(), Color::parse(color)?))).collect()
}

/// Most characters of a viewport's binding record (hostile files).
const MAX_VPBIND_CHARS: usize = 64 * 1024;

#[derive(Serialize, Deserialize, Default)]
struct VpBind {
    #[serde(default)]
    level: Option<aad_doc::LevelId>,
    #[serde(default)]
    sheet_set: Option<aad_doc::SheetSetId>,
    #[serde(default)]
    traces: Vec<aad_doc::Trace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<aad_doc::SpaceId>,
}

/// A viewport's level, sheet set and traces as AAD xdata: `1000 VPBIND`, `1002 {`, the record as
/// JSON in `1000` strings of at most 200 characters, `1002 }`.
pub(crate) fn vpbind_xdata(
    level: Option<aad_doc::LevelId>,
    sheet_set: Option<aad_doc::SheetSetId>,
    traces: &[aad_doc::Trace],
    view: Option<aad_doc::SpaceId>,
) -> Vec<Tag> {
    let bind = VpBind { level, sheet_set, traces: traces.iter().take(aad_doc::MAX_TRACES).cloned().collect(), view };
    let json = serde_json::to_string(&bind).unwrap_or_default();
    let chars: Vec<char> = json.chars().collect();
    let mut v = vec![Tag::s(1000, "VPBIND"), Tag::s(1002, "{")];
    v.extend(chars.chunks(200).map(|c| Tag::s(1000, c.iter().collect::<String>())));
    v.push(Tag::s(1002, "}"));
    v
}

/// The binding written by [`vpbind_xdata`]; all empty for files from other writers or a record
/// that does not parse. Traces are capped and their fade is limited to 90 percent.
pub(crate) type VpBinding = (Option<aad_doc::LevelId>, Option<aad_doc::SheetSetId>, Vec<aad_doc::Trace>, Option<aad_doc::SpaceId>);

pub(crate) fn read_vpbind(tags: &[Tag]) -> VpBinding {
    let list = xdata_list(xdata(tags, APP), "VPBIND");
    let mut json = String::new();
    for t in list.iter().filter(|t| t.code == 1000) {
        json.push_str(&t.str());
        if json.len() > MAX_VPBIND_CHARS {
            return (None, None, Vec::new(), None);
        }
    }
    // Only a JSON object is a record (serde would also read a short array as a sequence).
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else { return (None, None, Vec::new(), None) };
    let Some(mut b) = value.is_object().then(|| serde_json::from_value::<VpBind>(value).ok()).flatten() else {
        return (None, None, Vec::new(), None);
    };
    b.traces.truncate(aad_doc::MAX_TRACES);
    b.traces.retain(|t| t.fade <= 90 && t.layers.as_ref().is_none_or(|l| l.len() <= 256));
    (b.level, b.sheet_set, b.traces, b.view)
}

/// Most clip vertices kept per insert/viewport (hostile files).
pub(crate) const MAX_CLIP: usize = 10_000;

/// XCLIP / VPCLIP polygon as AAD xdata: `CLIP` then on/off and `x,y` pairs.
pub(crate) fn clip_xdata(points: &[aad_geom::Vec2], clipping: bool) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "CLIP"), Tag::s(1002, "{"), Tag::i(1070, i64::from(clipping))];
    for p in points.iter().take(MAX_CLIP) {
        if p.is_finite() {
            v.push(Tag::f(1010, p.x));
            v.push(Tag::f(1020, p.y));
        }
    }
    v.push(Tag::s(1002, "}"));
    v
}

/// `(points, clipping)` from [`clip_xdata`]; empty when absent.
pub(crate) fn read_clip(tags: &[Tag]) -> (Vec<aad_geom::Vec2>, bool) {
    let list = xdata_list(xdata(tags, APP), "CLIP");
    let clipping = list.iter().find(|t| t.code == 1070).map(|t| t.i64() != 0).unwrap_or(false);
    let mut pts = Vec::new();
    let mut x = None;
    for t in list.iter().take(MAX_CLIP * 2 + 8) {
        match t.code {
            1010 => x = Some(t.f64()),
            1020 => {
                if let Some(xv) = x.take() {
                    let p = aad_geom::Vec2::new(xv, t.f64());
                    if p.is_finite() {
                        pts.push(p);
                    }
                }
            }
            _ => {}
        }
    }
    (pts, clipping)
}

/// Viewport clip polygon only (always "on" when present).
pub(crate) fn read_vp_clip(tags: &[Tag]) -> Vec<aad_geom::Vec2> {
    read_clip(tags).0
}

const POINT_NAMES: [&str; 5] = ["defpt", "p13", "p14", "p15", "p16"];

/// Associativity links from AAD xdata; malformed links are skipped.
pub(crate) fn read_assoc(tags: &[Tag]) -> Vec<DimAssoc> {
    let list = xdata_list(xdata(tags, APP), "ASSOC");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(t) = list.get(i) {
        if out.len() >= MAX_XDATA_ITEMS {
            break;
        }
        i += 1;
        if !(t.code == 1002 && t.str().trim() == "{") {
            continue;
        }
        let end = list.get(i..).and_then(|r| r.iter().position(|x| x.code == 1002)).map(|p| i + p).unwrap_or(list.len());
        let body = list.get(i..end).unwrap_or(&[]);
        i = end + 1;
        let point = body.iter().find(|x| x.code == 1000).map(Tag::str).unwrap_or_default();
        let Some(point) = POINT_NAMES.iter().find(|p| **p == point.trim()) else { continue };
        let handles: Vec<Handle> = body.iter().filter(|x| x.code == 1005).filter_map(|x| Handle::parse_hex(&x.str())).collect();
        let Some(&handle) = handles.first() else { continue };
        let Some(code) = body.iter().find(|x| x.code == 1070).map(Tag::i64) else { continue };
        let snap = match code {
            0 => AssocSnap::Start,
            1 => AssocSnap::End,
            2 => AssocSnap::Mid,
            3 => AssocSnap::Center,
            4 => match body.iter().find(|x| x.code == 1040).map(Tag::f64).filter(|a| a.is_finite()) {
                Some(angle) => AssocSnap::OnCircle { angle },
                None => continue,
            },
            5 => match handles.get(1) {
                Some(&other) => AssocSnap::Intersection { other },
                None => continue,
            },
            6 => match body.iter().find(|x| x.code == 1071).and_then(|x| usize::try_from(x.i64()).ok()) {
                Some(index) => AssocSnap::Vertex { index },
                None => continue,
            },
            _ => continue,
        };
        out.push(DimAssoc { point: point.to_string(), handle, snap });
    }
    out
}

/// JSON text split into XRECORD strings of at most 250 characters. Backslashes are written as
/// `\u005c` so no chunk contains a DXF `\U+` escape.
fn json_chunks(json: &str) -> Vec<String> {
    let mut safe = String::with_capacity(json.len());
    let mut it = json.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('\\') => safe.push_str("\\u005c"),
                Some(n) => {
                    safe.push('\\');
                    safe.push(n);
                }
                None => safe.push('\\'),
            }
        } else {
            safe.push(c);
        }
    }
    let chars: Vec<char> = safe.chars().collect();
    chars.chunks(250).map(|c| c.iter().collect()).collect()
}

/// The saved layer states stored in the `AAD_LAYERSTATES` XRECORD.
#[derive(Serialize, Deserialize)]
struct LayerStatesPayload {
    version: u32,
    #[serde(default)]
    states: Vec<LayerState>,
}

/// JSON chunks for the layer states XRECORD, or `None` when the drawing has no saved layer
/// states.
pub(crate) fn layer_state_chunks(states: &[LayerState]) -> Option<Vec<String>> {
    if states.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&LayerStatesPayload { version: 1, states: states.to_vec() }).ok()?;
    Some(json_chunks(&json))
}

/// Parse the layer states XRECORD text; `None` when it is not a payload we understand.
pub(crate) fn parse_layer_states(text: &str) -> Option<Vec<LayerState>> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: LayerStatesPayload = serde_json::from_str(text).ok()?;
    if p.version != 1 || p.states.len() > MAX_LAYER_STATES {
        return None;
    }
    Some(p.states)
}

/// The drawing's plot style tables stored in the `AAD_PLOTSTYLES` XRECORD.
#[derive(Serialize, Deserialize)]
struct PlotStylesPayload {
    version: u32,
    #[serde(default)]
    tables: Vec<PlotStyleTable>,
}

/// JSON chunks for the plot style tables XRECORD, or `None` when the drawing keeps none.
pub(crate) fn plot_style_chunks(tables: &[PlotStyleTable]) -> Option<Vec<String>> {
    if tables.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&PlotStylesPayload { version: 1, tables: tables.to_vec() }).ok()?;
    Some(json_chunks(&json))
}

/// Parse the plot style tables XRECORD text (tables sanitized); `None` when it is not a
/// payload we understand.
pub(crate) fn parse_plot_styles(text: &str) -> Option<Vec<PlotStyleTable>> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: PlotStylesPayload = serde_json::from_str(text).ok()?;
    if p.version != 1 || p.tables.len() > MAX_PLOT_STYLE_TABLES {
        return None;
    }
    Some(p.tables.into_iter().map(PlotStyleTable::sanitized).filter(|t| !t.name.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    fn record(json: &str) -> Vec<Tag> {
        let mut v = vec![Tag::s(1001, APP), Tag::s(1000, "VPBIND"), Tag::s(1002, "{")];
        v.extend(json.chars().collect::<Vec<_>>().chunks(200).map(|c| Tag::s(1000, c.iter().collect::<String>())));
        v.push(Tag::s(1002, "}"));
        v
    }

    #[test]
    fn a_viewport_binding_is_read_back_and_hostile_records_are_bounded() {
        let t = aad_doc::Trace::new(aad_doc::LevelId(3));
        let mut tags = vec![Tag::s(1001, APP)];
        tags.extend(vpbind_xdata(Some(aad_doc::LevelId(2)), Some(aad_doc::SheetSetId(1)), std::slice::from_ref(&t), Some(aad_doc::SpaceId(7))));
        let (l, s, ts, v) = read_vpbind(&tags);
        assert_eq!(v, Some(aad_doc::SpaceId(7)));
        assert_eq!((l, s, ts), (Some(aad_doc::LevelId(2)), Some(aad_doc::SheetSetId(1)), vec![t]));

        // Not JSON, wrong shapes, over-long, too many and out-of-range traces.
        assert_eq!(read_vpbind(&record("not json")), (None, None, Vec::new(), None));
        assert_eq!(read_vpbind(&record("[1,2]")), (None, None, Vec::new(), None));
        assert_eq!(read_vpbind(&record(r#"{"level": "x"}"#)), (None, None, Vec::new(), None));
        assert_eq!(read_vpbind(&record(&"x".repeat(MAX_VPBIND_CHARS + 500))), (None, None, Vec::new(), None));
        let one = r#"{"level":9,"fade":95,"colour":"Grey","order":"Below","snap":false,"layers":null}"#;
        let (_, _, ts, _) = read_vpbind(&record(&format!(r#"{{"level":1,"traces":[{one}]}}"#)));
        assert!(ts.is_empty(), "a fade over 90 is dropped");
        let ok = r#"{"level":9,"fade":10,"colour":"Grey","order":"Below","snap":false,"layers":null}"#;
        let many = vec![ok; 40].join(",");
        let (l, _, ts, _) = read_vpbind(&record(&format!(r#"{{"level":1,"traces":[{many}]}}"#)));
        assert_eq!((l, ts.len()), (Some(aad_doc::LevelId(1)), aad_doc::MAX_TRACES), "traces are capped");
        assert_eq!(read_vpbind(&[]), (None, None, Vec::new(), None), "a viewport from another program has none");
    }
    use super::*;

    #[test]
    fn every_dimvar_has_a_code() {
        for (_, field) in aad_doc::DIMVARS {
            assert!(dim_code(field).is_some(), "{field} has no DXF group code");
        }
    }

    #[test]
    fn arrow_names() {
        assert_eq!(arrow_block_name("_DOT").map(|x| x.0).as_deref(), Some("_DOT"));
        assert_eq!(arrow_block_name("Architectural tick").map(|x| x.0).as_deref(), Some("_ArchTick"));
        assert!(arrow_block_name("").is_none());
        assert!(arrow_block_name("nonsense").is_none());
        assert!(is_standard_arrow("_dot"));
    }
}

/// Named-object-dictionary key of the project data XRECORD.
pub(crate) const PROJECT_KEY: &str = "AAD_PROJECT";
/// Most project entries read from a file.
pub(crate) const MAX_PROJECT_ENTRIES: usize = 10_000;

#[derive(Serialize, Deserialize)]
struct ProjectPayload {
    version: u32,
    items: std::collections::BTreeMap<String, String>,
}

/// JSON chunks for the project data XRECORD, or `None` when there is none.
pub(crate) fn project_chunks(items: &std::collections::BTreeMap<String, String>) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&ProjectPayload { version: 1, items: items.clone() }).ok()?;
    Some(json_chunks(&json))
}

pub(crate) fn parse_project(text: &str) -> Option<std::collections::BTreeMap<String, String>> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: ProjectPayload = serde_json::from_str(text).ok()?;
    (p.version == 1 && p.items.len() <= MAX_PROJECT_ENTRIES).then_some(p.items)
}

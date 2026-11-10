//! Sheets: the paper-space side of a drawing. A layout is a sheet; this module gives it a title
//! block with fields, lets a finished sheet be saved as a reusable template, and lists and edits
//! its viewports. There is no Layout panel; sheet setup is done through these commands.
//!
//! - **Nameplates** (`sheet.designer`): a new drawing's empty Layout1 and Layout2 become
//!   **Nameplate L** (landscape) and **Nameplate P** (portrait), each with its own title block so
//!   the two orientations can be designed separately. A legacy single `Title plate` is migrated to
//!   Nameplate L once. `landscape` / `open` choose which plate to open.
//! - **Title block** (`sheet.titleblock`): the block `AAD-TITLEBLOCK`, drawn in millimetres of real
//!   paper size and inserted at the lower right of the printable area, with a border around it. Its
//!   fields are block attributes (`PROJECT`, `CLIENT`, `TITLE`, `FLOOR`, `DRAWN`, `DATE`, `SHEET`,
//!   `SCALE`, `REV`). They start from the project details; `sheet.fields` sets them. Any block with
//!   attributes of those tags works, so a template with its own title block is filled the same way.
//! - **Templates** (`sheet.template.*`): everything on the sheet except its viewports, the blocks it
//!   uses, and the page setup, saved by name in the drawing (`scope: drawing`, travels with the
//!   file) or for the user (`scope: user`, one JSON file in the AAD settings folder). Applying a
//!   template sets the page up, places the objects, and replaces what an earlier template put on
//!   that layout, never anything the user drew.
//! - **Viewports** (`viewport.list|add|remove`): the viewports of a layout with their scale, size,
//!   lock and frozen layers.
//!
//! - **Sheet index** (`sheet.index`): lists the sheets of the drawing (nameplates excluded) with
//!   number, title, scale and revision read from each title block; `renumber` writes consecutive
//!   sheet numbers into the title blocks; `draw` places the list as a table on a layout.
//!
//! Revision tables are drawn by `revision.table` / `revision.issue` (layer `A-ANNO-REVT`).
//! Not implemented here: templates that carry viewports (viewports are added per drawing,
//! since what they show is the drawing's own).

use std::collections::BTreeMap;
use std::sync::Arc;

use aad_doc::{Attrib, Block, Common, Drawing, Entity, EntityKind, HAlign, PageSetup, Space, Text, VAlign};
use aad_geom::{Bounds2, Vec2};
use serde_json::{Map, Value, json};

use super::draw::make_text;
use super::helpers::{line, lwpoly, rect_vertices};
use super::project;
use super::*;
use crate::{Result, Session};

pub const TITLE_BLOCK: &str = "AAD-TITLEBLOCK";
/// Landscape nameplate layout (designed separately from portrait).
pub const TITLE_PLATE_L: &str = "Nameplate L";
/// Portrait nameplate layout (designed separately from landscape).
pub const TITLE_PLATE_P: &str = "Nameplate P";
/// Legacy single title-plate name (still recognised when migrating old drawings).
pub const TITLE_PLATE: &str = "Title plate";
const BORDER_LAYER: &str = "A-ANNO-BORD";
const TITLE_LAYER: &str = "A-ANNO-TTLB";
/// The fields of the standard title block, in the order the panel shows them.
pub const FIELDS: &[(&str, &str)] = &[
    ("PROJECT", "Project"),
    ("CLIENT", "Client"),
    ("TITLE", "Drawing title"),
    ("FLOOR", "Floor"),
    ("DRAWN", "Drawn by"),
    ("DATE", "Date"),
    ("SHEET", "Sheet no."),
    ("SCALE", "Scale"),
    ("REV", "Revision"),
];
const MAX_TEMPLATE_ENTITIES: usize = 20_000;
const MAX_TEMPLATE_BLOCKS: usize = 200;
const MAX_TEMPLATE_BYTES: u64 = 8_000_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("sheet.titleblock", "Title Block", run_titleblock)
            .menu(&["Architecture", "Sheets", "Title Block"])
            .params("{layout?, fields?: {PROJECT, CLIENT, TITLE, FLOOR, DRAWN, DATE, SHEET, SCALE, REV}} → {handle, fields, created}"),
        CommandSpec::new("sheet.designer", "Title Plate", run_designer)
            .menu(&["Architecture", "Sheets", "Title Plate"])
            .params("{open?: bool, landscape?: bool (default true → Nameplate L; false → Nameplate P)} → {name, layouts, created}. Ensures both nameplates; `open` switches to the chosen one."),
        CommandSpec::new("sheet.fields", "Sheet Fields", run_fields)
            .menu(&["Architecture", "Sheets", "Sheet Fields"])
            .params("{layout?, fields?: {TAG: value}} → {fields: {TAG: value}} (without `fields`: just reads them)"),
        CommandSpec::new("sheet.template.save", "Save Sheet Template", run_tpl_save)
            .menu(&["Architecture", "Sheets", "Save Sheet Template"])
            .params("{name, layout?, scope?: drawing|user, folder? (user scope: a shared folder instead of the settings folder)} → {name, entities, blocks}")
            .noundo(),
        CommandSpec::new("sheet.template.list", "Sheet Templates", run_tpl_list)
            .params("{folder?} → {templates: [{name, scope, paper, entities}]}")
            .noundo(),
        CommandSpec::new("sheet.template.apply", "Apply Sheet Template", run_tpl_apply)
            .menu(&["Architecture", "Sheets", "Apply Sheet Template"])
            .params("{name, layout?, folder?, fields?: {TAG: value}} → {placed, replaced}"),
        CommandSpec::new("sheet.template.delete", "Delete Sheet Template", run_tpl_delete)
            .params("{name, scope?: drawing|user, folder?} → {deleted}")
            .noundo(),
        CommandSpec::new("viewport.list", "List Viewports", run_vp_list)
            .menu(&["View", "Viewports", "List"])
            .alias(&["vplist"])
            .params("{layout?} → {viewports: [{handle, rect, scale, scaleText, locked, frozen}]}")
            .noundo(),
        CommandSpec::new("viewport.add", "Add Viewport", run_vp_add)
            .menu(&["View", "Viewports", "Add"])
            .alias(&["vpadd", "mview"])
            .params("{layout?, p1?, p2? (paper units; default the printable area), scale?: \"1:100\", locked?: bool} → {handle}"),
        CommandSpec::new("viewport.remove", "Remove Viewport", run_vp_remove)
            .menu(&["View", "Viewports", "Remove"])
            .alias(&["vprem"])
            .params("{handle}"),
        CommandSpec::new("sheet.index", "Sheet Index", run_index)
            .menu(&["Architecture", "Sheets", "Sheet Index"])
            .alias(&["sheetindex"])
            .params("{renumber?: {prefix?: \"A-\", start?: 101, step?: 1}, draw?: bool, layout? (where to draw; default the current layout), at? ([x, y] paper units, top-left; default inside the printable area), height? (text height in mm, default 2.5)} → {sheets: [{layout, number, title, scale, rev, titleBlock}], renumbered, drawn}. Without `renumber` and `draw` it only reads."),
        // Used by Print after the user picks a model-space window (or Extents / Display).
        CommandSpec::new("sheet.plotwindow", "Plot Window", run_plotwindow)
            .alias(&["plotwindow", "nameplate.plotwindow"])
            .params("{layout?, window?: [x0,y0,x1,y1] model region to show (default: model extents), p1?, p2? (paper units for the viewport; default above the title block), locked?: bool (default true)} → {handle, scale, scaleText, created, layout}"),
    ]
}

// ---------------- helpers ----------------

/// The layout a command acts on: `layout` or the current one (not the Model tab).
fn layout_of(s: &Session, cmd: &str, p: &Value) -> Result<String> {
    if let Some(n) = str_param(p, "layout") {
        return s.doc()?.layout(n).map(|l| l.name.clone()).ok_or_else(|| bad(cmd, format!("no layout named `{n}`")));
    }
    match s.layout_space() {
        Space::Paper(n) => Ok(n),
        Space::Model => Err(bad(cmd, "switch to a layout or pass `layout`")),
    }
}

/// Run `f` with `layout` as the current layout, then go back to where the user was.
fn in_layout<T>(s: &mut Session, layout: &str, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
    let prev = s.layout_space();
    s.execute("layout.set", &json!({ "name": layout }))?;
    let r = f(s);
    let back = match prev {
        Space::Paper(n) => n,
        Space::Model => "Model".to_string(),
    };
    let _ = s.execute("layout.set", &json!({ "name": back }));
    r
}

fn sheet_of(d: &Drawing, layout: &str, cmd: &str) -> Result<aad_render::Sheet> {
    aad_render::sheet(d, layout).ok_or_else(|| bad(cmd, format!("no layout named `{layout}`")))
}

fn scale_text(scale: f64) -> String {
    if !(scale.is_finite() && scale > 0.0) {
        return String::new();
    }
    if scale <= 1.0 {
        let n = 1.0 / scale;
        if (n - n.round()).abs() < 1e-6 * n { format!("1:{}", n.round()) } else { format!("1:{n:.2}") }
    } else if (scale - scale.round()).abs() < 1e-9 {
        format!("{}:1", scale.round())
    } else {
        format!("{scale:.2}:1")
    }
}

fn ensure_layer(d: &mut Drawing, name: &str, aci: i16, desc: &str) {
    super::hcw::ensure_layer(d, name, aci, "Continuous", 25, desc);
}

fn attdef(style: &str, tag: &str, prompt: &str, at: Vec2, height: f64) -> EntityKind {
    EntityKind::AttDef(Attrib {
        tag: tag.to_string(),
        text: Text {
            insert: aad_geom::Vec3::new(at.x, at.y, 0.0),
            align_pt: None,
            height,
            value: String::new(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: style.to_string(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        },
        invisible: false,
        constant: false,
        prompt: prompt.to_string(),
        props: Default::default(),
    })
}

/// Make the title block definition if the drawing has none. Drawn in millimetres, its insertion
/// point is the lower right corner of the panel.
fn ensure_title_block(s: &mut Session) -> Result<()> {
    if s.doc()?.block(TITLE_BLOCK).is_some() {
        return Ok(());
    }
    const W: f64 = 180.0;
    const ROW: f64 = 14.0;
    const H: f64 = ROW * 4.0;
    let style = s.doc()?.header.str("TEXTSTYLE", "Standard");
    // (tag, label, x from the left of the panel, width, row from the top)
    let cells: [(&str, &str, f64, f64, usize); 9] = [
        ("PROJECT", "PROJECT", 0.0, 180.0, 0),
        ("CLIENT", "CLIENT", 0.0, 90.0, 1),
        ("TITLE", "DRAWING TITLE", 90.0, 90.0, 1),
        ("FLOOR", "FLOOR", 0.0, 60.0, 2),
        ("DRAWN", "DRAWN BY", 60.0, 60.0, 2),
        ("DATE", "DATE", 120.0, 60.0, 2),
        ("SHEET", "SHEET", 0.0, 60.0, 3),
        ("SCALE", "SCALE", 60.0, 60.0, 3),
        ("REV", "REVISION", 120.0, 60.0, 3),
    ];
    let mut kinds: Vec<(String, EntityKind)> = Vec::new();
    let x = |v: f64| v - W;
    let y = |row: usize| H - ROW * (row as f64 + 1.0);
    kinds.push(("0".into(), lwpoly(rect_vertices(Vec2::new(x(0.0), 0.0), Vec2::new(0.0, H)), true)));
    for r in 1..4 {
        let yy = H - ROW * r as f64;
        // Rows below the first are split into cells; the first row spans the panel.
        kinds.push(("0".into(), line(Vec2::new(x(0.0), yy), Vec2::new(0.0, yy))));
    }
    for (_, _, x0, w, row) in cells.iter().filter(|c| c.1 != "PROJECT") {
        if *x0 > 0.0 {
            kinds.push(("0".into(), line(Vec2::new(x(*x0), y(*row)), Vec2::new(x(*x0), y(*row) + ROW))));
        }
        let _ = w;
    }
    for (tag, label, x0, _, row) in cells {
        let (cx, cy) = (x(x0) + 2.0, y(row));
        kinds.push(("0".into(), make_text(s, Vec2::new(cx, cy + ROW - 4.0), label, 2.0, 0.0, (HAlign::Left, VAlign::Baseline))));
        kinds.push(("0".into(), attdef(&style, tag, label, Vec2::new(cx, cy + 3.0), 4.5)));
    }
    let d = s.doc_mut()?;
    let mut b = Block::new(TITLE_BLOCK);
    b.description = "MynaCAD title block".into();
    for (layer, kind) in kinds {
        let handle = d.new_handle();
        b.entities.push(Entity { handle, common: Common { layer, ..Default::default() }, kind });
    }
    d.blocks.insert(TITLE_BLOCK.into(), Arc::new(b));
    Ok(())
}

/// The handles of the title block references (or any block with title fields) in `layout`.
pub(crate) fn title_inserts(d: &Drawing, layout: &str) -> Vec<aad_doc::Handle> {
    let Some(l) = d.layouts.iter().find(|l| l.name == layout) else { return Vec::new() };
    l.entities
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(TITLE_BLOCK) || i.attribs.iter().any(|a| FIELDS.iter().any(|(t, _)| a.tag.eq_ignore_ascii_case(t)))))
        .map(|e| e.handle)
        .collect()
}

fn read_fields(d: &Drawing, layout: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(l) = d.layouts.iter().find(|l| l.name == layout) else { return out };
    for h in title_inserts(d, layout) {
        if let Some(EntityKind::Insert(i)) = l.entities.get(h).map(|e| e.kind.clone()) {
            for a in &i.attribs {
                out.entry(a.tag.to_ascii_uppercase()).or_insert_with(|| a.text.value.clone());
            }
        }
    }
    out
}

/// Write title-block attribute values on `layout`. Used by `sheet.fields` and `companion.commit`.
pub(crate) fn write_fields(d: &mut Drawing, layout: &str, fields: &BTreeMap<String, String>) -> usize {
    let mut changed = 0;
    for h in title_inserts(d, layout) {
        let Some(l) = d.layouts.iter_mut().find(|l| l.name == layout) else { continue };
        l.entities.modify(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                for a in &mut i.attribs {
                    if let Some(v) = fields.get(&a.tag.to_ascii_uppercase())
                        && a.text.value != *v
                    {
                        a.text.value = v.clone();
                        changed += 1;
                    }
                }
            }
        });
    }
    changed
}

/// What a new title block starts with: the project's details and floor, the first viewport's scale.
fn default_fields(d: &Drawing, layout: &str) -> BTreeMap<String, String> {
    let det = project::get(d, "details");
    let text = |k: &str| det.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut m = BTreeMap::new();
    let mut project = text("title");
    let summary = text("summary");
    if !summary.is_empty() {
        if !project.is_empty() {
            project.push_str(" — ");
        }
        project.push_str(&summary);
    }
    m.insert("PROJECT".into(), project);
    m.insert("CLIENT".into(), text("owner"));
    m.insert("DRAWN".into(), if text("engineer").is_empty() { text("architect") } else { text("engineer") });
    m.insert("SHEET".into(), layout.to_string());
    if let Some(f) = project::get(d, "projectFile").get("floor").and_then(Value::as_str) {
        m.insert("FLOOR".into(), f.to_string());
    }
    if let Some(l) = d.layouts.iter().find(|l| l.name == layout)
        && let Some(v) = l.entities.iter().find_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id != 1 && v.view_height > 1e-12 => Some(v.height / v.view_height),
            _ => None,
        })
    {
        m.insert("SCALE".into(), scale_text(v));
    }
    m
}

fn string_map(v: Option<&Value>) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    if let Some(o) = v.and_then(Value::as_object) {
        if o.len() > 200 {
            return Err(bad("sheet.fields", "too many fields"));
        }
        for (k, val) in o {
            let t = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Null => String::new(),
                _ => return Err(bad("sheet.fields", format!("`{k}` must be text"))),
            };
            if t.chars().count() > 500 {
                return Err(bad("sheet.fields", format!("`{k}` is too long")));
            }
            out.insert(k.to_ascii_uppercase(), t);
        }
    }
    Ok(out)
}

// ---------------- title plate ----------------

fn is_nameplate(name: &str) -> bool {
    name.eq_ignore_ascii_case(TITLE_PLATE_L) || name.eq_ignore_ascii_case(TITLE_PLATE_P) || name.eq_ignore_ascii_case(TITLE_PLATE)
}

/// Empty Layout1/Layout2 (or existing nameplates) and nothing else.
fn only_stock_sheets(d: &Drawing) -> bool {
    !d.layouts.is_empty()
        && d.layouts.iter().all(|l| {
            let stock = l.name.eq_ignore_ascii_case("Layout1") || l.name.eq_ignore_ascii_case("Layout2");
            (stock && l.entities.is_empty()) || is_nameplate(&l.name)
        })
}

fn layout_names(s: &Session) -> Vec<String> {
    s.doc().map(|d| d.layouts.iter().map(|l| l.name.clone()).collect()).unwrap_or_default()
}

fn plate_for(landscape: bool) -> &'static str {
    if landscape { TITLE_PLATE_L } else { TITLE_PLATE_P }
}

/// Ensure Nameplate L (landscape) and Nameplate P (portrait) exist with title blocks and page setup.
fn ensure_nameplates(s: &mut Session) -> Result<(bool, bool)> {
    // Migrate a legacy single "Title plate" into Nameplate L once.
    if s.doc()?.layout(TITLE_PLATE).is_some() && s.doc()?.layout(TITLE_PLATE_L).is_none() {
        // Rename by creating L and copying is heavy; keep legacy usable as L via alias in plotwindow_layout.
    }
    let mut created_l = false;
    let mut created_p = false;
    if only_stock_sheets(s.doc()?) {
        for (name, _) in [(TITLE_PLATE_L, true), (TITLE_PLATE_P, false)] {
            if s.doc()?.layout(name).is_none() {
                s.execute("layout.new", &json!({ "name": name, "viewport": false }))?;
                if name == TITLE_PLATE_L {
                    created_l = true;
                } else {
                    created_p = true;
                }
            }
        }
        let remove: Vec<String> = s.doc()?.layouts.iter().filter(|l| !is_nameplate(&l.name)).map(|l| l.name.clone()).collect();
        for name in remove {
            s.execute("layout.delete", &json!({ "name": name }))?;
        }
    } else {
        for (name, _) in [(TITLE_PLATE_L, true), (TITLE_PLATE_P, false)] {
            if s.doc()?.layout(name).is_none() {
                s.execute("layout.new", &json!({ "name": name, "viewport": false }))?;
                if name == TITLE_PLATE_L {
                    created_l = true;
                } else {
                    created_p = true;
                }
            }
        }
    }
    for (name, landscape) in [(TITLE_PLATE_L, true), (TITLE_PLATE_P, false)] {
        if s.doc()?.layout(name).is_some() {
            let _ = s.execute("sheet.titleblock", &json!({ "layout": name }));
            let _ = s.execute("pagesetup", &json!({ "layout": name, "landscape": landscape }));
        }
    }
    // Legacy Title plate: keep title block; treat as landscape companion when L was just created from stock.
    if s.doc()?.layout(TITLE_PLATE).is_some() {
        let _ = s.execute("sheet.titleblock", &json!({ "layout": TITLE_PLATE }));
        let _ = s.execute("pagesetup", &json!({ "layout": TITLE_PLATE, "landscape": true }));
    }
    Ok((created_l, created_p))
}

/// Both nameplates (landscape + portrait). Drawings that already have other sheets keep them;
/// missing nameplates are still added. `landscape` chooses which sheet `open` switches to.
fn run_designer(s: &mut Session, p: &Value) -> Result<Value> {
    let open = bool_or(p, "open", false);
    let landscape = bool_or(p, "landscape", true);
    let target = plate_for(landscape);
    let (created_l, created_p) = ensure_nameplates(s)?;
    // Prefer the requested plate; fall back to legacy Title plate if L is missing somehow.
    let name = if s.doc()?.layout(target).is_some() {
        target
    } else if s.doc()?.layout(TITLE_PLATE).is_some() {
        TITLE_PLATE
    } else {
        return Ok(json!({ "name": Value::Null, "layouts": layout_names(s), "created": false }));
    };
    let block = s.execute("sheet.titleblock", &json!({ "layout": name }))?;
    if open {
        s.execute("layout.set", &json!({ "name": name }))?;
    }
    Ok(json!({
        "name": name,
        "landscape": landscape,
        "layouts": layout_names(s),
        "created": created_l || created_p || block.get("created").and_then(Value::as_bool).unwrap_or(false),
        "fields": block.get("fields").cloned().unwrap_or(Value::Null),
    }))
}

// ---------------- title block and fields ----------------

fn run_titleblock(s: &mut Session, p: &Value) -> Result<Value> {
    let layout = layout_of(s, "sheet.titleblock", p)?;
    let given = string_map(p.get("fields"))?;
    let existing = title_inserts(s.doc()?, &layout);
    let mut created = false;
    if existing.is_empty() {
        ensure_title_block(s)?;
        let sh = sheet_of(s.doc()?, &layout, "sheet.titleblock")?;
        let prt = sh.printable;
        let k = 1.0 / sh.unit_mm;
        let at = Vec2::new(prt.max.x, prt.min.y);
        in_layout(s, &layout, |s| {
            ensure_layer(s.doc_mut()?, BORDER_LAYER, 7, "Sheet border");
            ensure_layer(s.doc_mut()?, TITLE_LAYER, 7, "Title block");
            super::hcw::add_on(s, BORDER_LAYER, lwpoly(rect_vertices(prt.min, prt.max), true))?;
            s.execute("insert", &json!({ "name": TITLE_BLOCK, "at": [at.x, at.y], "scale": k }))?;
            Ok(())
        })?;
        created = true;
    }
    let mut fields = default_fields(s.doc()?, &layout);
    fields.extend(read_fields(s.doc()?, &layout).into_iter().filter(|(_, v)| !v.is_empty()));
    fields.extend(given);
    write_fields(s.doc_mut()?, &layout, &fields);
    let handle = title_inserts(s.doc()?, &layout).first().map(|h| h.hex());
    Ok(json!({ "handle": handle, "fields": read_fields(s.doc()?, &layout), "created": created }))
}

fn run_fields(s: &mut Session, p: &Value) -> Result<Value> {
    let layout = layout_of(s, "sheet.fields", p)?;
    let given = string_map(p.get("fields"))?;
    if !given.is_empty() {
        if title_inserts(s.doc()?, &layout).is_empty() {
            return Err(bad("sheet.fields", "this sheet has no title block: add one with sheet.titleblock or a template"));
        }
        write_fields(s.doc_mut()?, &layout, &given);
    }
    let known: Vec<Value> = FIELDS.iter().map(|(t, l)| json!({ "tag": t, "label": l })).collect();
    Ok(json!({ "fields": read_fields(s.doc()?, &layout), "known": known, "titleBlock": !title_inserts(s.doc()?, &layout).is_empty() }))
}

// ---------------- templates ----------------

fn user_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))
        })?;
        Some(base.join("AAD").join("sheet-templates"))
    }
}

/// The folder user templates live in: `folder` (an office-wide shared folder, for example) or the AAD
/// settings folder.
fn dir_of(p: &Value) -> Option<std::path::PathBuf> {
    match str_param(p, "folder") {
        Some(f) => Some(std::path::PathBuf::from(f)),
        None => user_dir(),
    }
}

fn template_file(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    dir.join(format!("{}.json", safe_name(name)))
}

fn safe_name(raw: &str) -> String {
    let s: String = raw.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '_' }).collect();
    s.trim().chars().take(60).collect()
}

fn check_name(cmd: &str, raw: &str) -> Result<String> {
    let n = safe_name(raw);
    if n.is_empty() { Err(bad(cmd, "the template needs a name")) } else { Ok(n) }
}

fn capture(d: &Drawing, layout: &str) -> Result<Value> {
    let l = d.layouts.iter().find(|l| l.name == layout).ok_or_else(|| bad("sheet.template.save", format!("no layout named `{layout}`")))?;
    let mut entities = Vec::new();
    let mut want: Vec<(String, usize)> = Vec::new();
    for e in l.entities.iter() {
        if matches!(e.kind, EntityKind::Viewport(_)) {
            continue;
        }
        if entities.len() >= MAX_TEMPLATE_ENTITIES {
            return Err(bad("sheet.template.save", "the sheet has too many objects for a template"));
        }
        if let EntityKind::Insert(i) = &e.kind {
            want.push((i.block.clone(), 0));
        }
        entities.push(serde_json::to_value(&**e).map_err(|x| bad("sheet.template.save", x.to_string()))?);
    }
    let mut blocks: Vec<Value> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    while let Some((name, depth)) = want.pop() {
        if !seen.insert(name.to_ascii_uppercase()) || blocks.len() >= MAX_TEMPLATE_BLOCKS {
            continue;
        }
        let Some(b) = d.block(&name) else { continue };
        let mut es = Vec::new();
        for e in b.entities.iter() {
            if let EntityKind::Insert(i) = &e.kind
                && depth < aad_doc::MAX_BLOCK_DEPTH
            {
                want.push((i.block.clone(), depth + 1));
            }
            es.push(serde_json::to_value(&**e).map_err(|x| bad("sheet.template.save", x.to_string()))?);
        }
        blocks.push(json!({ "name": b.name, "base": [b.base.x, b.base.y, b.base.z], "description": b.description, "entities": es }));
    }
    Ok(
        json!({ "format": "aad-sheet-template", "version": 1, "page": serde_json::to_value(&l.page).unwrap_or(Value::Null), "entities": entities, "blocks": blocks }),
    )
}

fn run_tpl_save(s: &mut Session, p: &Value) -> Result<Value> {
    let name = check_name("sheet.template.save", str_param(p, "name").unwrap_or(""))?;
    let layout = layout_of(s, "sheet.template.save", p)?;
    let tpl = capture(s.doc()?, &layout)?;
    let counts = json!({ "entities": tpl["entities"].as_array().map_or(0, Vec::len), "blocks": tpl["blocks"].as_array().map_or(0, Vec::len) });
    match str_param(p, "scope").unwrap_or("drawing") {
        "drawing" => {
            let mut all = match project::get(s.doc()?, "sheetTemplates") {
                Value::Object(o) => o,
                _ => Map::new(),
            };
            all.insert(name.clone(), tpl);
            project::set(s.doc_mut()?, "sheetTemplates", &Value::Object(all));
        }
        "user" => {
            let dir = dir_of(p).ok_or_else(|| bad("sheet.template.save", "a user template cannot be saved here"))?;
            let path = template_file(&dir, &name);
            let text = serde_json::to_string(&tpl).map_err(|e| bad("sheet.template.save", e.to_string()))?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| bad("sheet.template.save", format!("{}: {e}", dir.display())))?;
            }
            std::fs::write(&path, text).map_err(|e| bad("sheet.template.save", format!("{}: {e}", path.display())))?;
        }
        other => return Err(bad("sheet.template.save", format!("`scope` must be drawing or user, not `{other}`"))),
    }
    Ok(json!({ "name": name, "entities": counts["entities"], "blocks": counts["blocks"], "message": format!("Saved sheet template `{name}`") }))
}

fn user_templates(dir: Option<std::path::PathBuf>) -> Vec<(String, Value)> {
    let Some(dir) = dir else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten().take(200) {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") || e.metadata().map_or(true, |m| m.len() > MAX_TEMPLATE_BYTES) {
            continue;
        }
        let Some(name) = path.file_stem().map(|n| n.to_string_lossy().to_string()) else { continue };
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(v) = serde_json::from_str::<Value>(&text)
            && v["format"] == "aad-sheet-template"
        {
            out.push((name, v));
        }
    }
    out
}

fn find_template(s: &Session, name: &str, dir: Option<std::path::PathBuf>) -> Result<Value> {
    if let Some(v) = project::get(s.doc()?, "sheetTemplates").get(name) {
        return Ok(v.clone());
    }
    user_templates(dir)
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
        .ok_or_else(|| bad("sheet.template.apply", format!("no sheet template named `{name}`")))
}

fn run_tpl_list(s: &mut Session, p: &Value) -> Result<Value> {
    let mut out = Vec::new();
    if let Value::Object(o) = project::get(s.doc()?, "sheetTemplates") {
        for (n, v) in o {
            out.push(json!({ "name": n, "scope": "drawing", "paper": v["page"]["paper"], "entities": v["entities"].as_array().map_or(0, Vec::len) }));
        }
    }
    for (n, v) in user_templates(dir_of(p)) {
        out.push(json!({ "name": n, "scope": "user", "paper": v["page"]["paper"], "entities": v["entities"].as_array().map_or(0, Vec::len) }));
    }
    Ok(json!({ "templates": out }))
}

fn run_tpl_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("sheet.template.delete", "`name` is required"))?.to_string();
    match str_param(p, "scope").unwrap_or("drawing") {
        "drawing" => {
            let mut all = match project::get(s.doc()?, "sheetTemplates") {
                Value::Object(o) => o,
                _ => Map::new(),
            };
            if all.remove(&name).is_none() {
                return Err(bad("sheet.template.delete", format!("no sheet template named `{name}` in this drawing")));
            }
            project::set(s.doc_mut()?, "sheetTemplates", &Value::Object(all));
        }
        "user" => {
            let path = dir_of(p)
                .map(|d| template_file(&d, &name))
                .filter(|f| f.is_file())
                .ok_or_else(|| bad("sheet.template.delete", format!("no user template named `{name}`")))?;
            std::fs::remove_file(&path).map_err(|e| bad("sheet.template.delete", e.to_string()))?;
        }
        other => return Err(bad("sheet.template.delete", format!("`scope` must be drawing or user, not `{other}`"))),
    }
    Ok(json!({ "deleted": name }))
}

fn run_tpl_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("sheet.template.apply", "`name` is required"))?.to_string();
    let layout = layout_of(s, "sheet.template.apply", p)?;
    let tpl = find_template(s, &name, dir_of(p))?;
    let given = string_map(p.get("fields"))?;
    let page: PageSetup =
        serde_json::from_value(tpl["page"].clone()).map_err(|e| bad("sheet.template.apply", format!("the template's page setup is damaged: {e}")))?;
    let ents = tpl["entities"].as_array().ok_or_else(|| bad("sheet.template.apply", "the template has no objects"))?;
    let blocks = tpl["blocks"].as_array().cloned().unwrap_or_default();
    if ents.len() > MAX_TEMPLATE_ENTITIES || blocks.len() > MAX_TEMPLATE_BLOCKS {
        return Err(bad("sheet.template.apply", "the template is too large"));
    }
    // Decode everything first, so a damaged template changes nothing.
    let mut decoded: Vec<Entity> = Vec::new();
    for e in ents {
        let ent: Entity = serde_json::from_value(e.clone()).map_err(|x| bad("sheet.template.apply", format!("a template object is damaged: {x}")))?;
        if !matches!(ent.kind, EntityKind::Viewport(_)) {
            decoded.push(ent);
        }
    }
    let mut defs: Vec<(String, aad_geom::Vec3, String, Vec<Entity>)> = Vec::new();
    for b in &blocks {
        let n = b["name"].as_str().filter(|n| !n.is_empty()).ok_or_else(|| bad("sheet.template.apply", "a template block has no name"))?;
        let base = b["base"]
            .as_array()
            .map(|a| aad_geom::Vec3::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0), 0.0))
            .unwrap_or(aad_geom::Vec3::ZERO);
        let mut es = Vec::new();
        for e in b["entities"].as_array().into_iter().flatten().take(MAX_TEMPLATE_ENTITIES) {
            es.push(
                serde_json::from_value::<Entity>(e.clone()).map_err(|x| bad("sheet.template.apply", format!("a template block is damaged: {x}")))?,
            );
        }
        defs.push((n.to_string(), base, b["description"].as_str().unwrap_or("").to_string(), es));
    }

    // What an earlier template put on this layout goes; nothing else does.
    let owned_key = "sheetOwned";
    let mut owned = match project::get(s.doc()?, owned_key) {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    let old: Vec<String> =
        owned.get(&layout).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let d = s.doc_mut()?;
    let space = Space::Paper(layout.clone());
    let mut replaced = 0;
    for h in old {
        if let Some(hd) = aad_doc::Handle::parse_hex(&h)
            && d.space(&space).is_some_and(|st| st.get(hd).is_some())
        {
            d.remove_entity(hd);
            replaced += 1;
        }
    }
    if let Some(l) = d.layouts.iter_mut().find(|l| l.name == layout) {
        l.page = page;
    }
    for (n, base, desc, es) in defs {
        if d.block(&n).is_some() {
            continue;
        }
        let mut b = Block::new(&n);
        b.base = base;
        b.description = desc;
        for mut e in es {
            e.handle = d.new_handle();
            b.entities.push(e);
        }
        d.blocks.insert(n, Arc::new(b));
    }
    let mut placed = Vec::new();
    for e in decoded {
        let h = d.add(&space, e.common, e.kind)?;
        placed.push(json!(h.hex()));
    }
    let n = placed.len();
    owned.insert(layout.clone(), Value::Array(placed));
    project::set(d, owned_key, &Value::Object(owned));

    // Fill the title fields from the project, then from what the caller gave.
    let mut fields = default_fields(s.doc()?, &layout);
    fields.extend(read_fields(s.doc()?, &layout).into_iter().filter(|(_, v)| !v.is_empty()));
    fields.extend(given);
    write_fields(s.doc_mut()?, &layout, &fields);
    Ok(json!({ "placed": n, "replaced": replaced, "layout": layout, "message": format!("Applied sheet template `{name}` to {layout}") }))
}

// ---------------- viewports ----------------

fn viewport_json(h: aad_doc::Handle, v: &aad_doc::Viewport) -> Value {
    let scale = if v.view_height > 1e-12 { v.height / v.view_height } else { 0.0 };
    let (x0, y0) = (v.center.x - v.width / 2.0, v.center.y - v.height / 2.0);
    json!({
        "handle": h.hex(), "rect": [x0, y0, x0 + v.width, y0 + v.height], "width": v.width, "height": v.height,
        "scale": scale, "scaleText": scale_text(scale), "locked": v.locked, "frozen": v.frozen_layers,
        "viewCenter": [v.view_center.x, v.view_center.y], "viewHeight": v.view_height,
        "binding": super::levels::viewport_binding_json(v),
    })
}

fn run_vp_list(s: &mut Session, p: &Value) -> Result<Value> {
    let layout = layout_of(s, "viewport.list", p)?;
    let d = s.doc()?;
    let l = d.layouts.iter().find(|l| l.name == layout).ok_or_else(|| bad("viewport.list", "no such layout"))?;
    let vps: Vec<Value> = l
        .entities
        .iter()
        .filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { (v.id != 1).then(|| viewport_json(e.handle, v)) } else { None })
        .collect();
    Ok(json!({ "layout": layout, "viewports": vps }))
}

fn run_vp_add(s: &mut Session, p: &Value) -> Result<Value> {
    let layout = layout_of(s, "viewport.add", p)?;
    let sh = sheet_of(s.doc()?, &layout, "viewport.add")?;
    let (a, b) = match (point_param(p, "p1"), point_param(p, "p2")) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            // Above the title block when there is one, else the printable area.
            let prt: Bounds2 = sh.printable;
            let top = if title_inserts(s.doc()?, &layout).is_empty() { prt.min.y } else { prt.min.y + 56.0 / sh.unit_mm };
            (Vec2::new(prt.min.x, top), prt.max)
        }
    };
    let mut q = json!({ "layout": layout, "count": 1, "p1": [a.x, a.y], "p2": [b.x, b.y] });
    if let Some(o) = q.as_object_mut() {
        o.insert("layout".into(), json!(layout));
    }
    let r = s.execute("mview", &q)?;
    let handle = r["viewports"][0].as_str().ok_or_else(|| bad("viewport.add", "the viewport was not created"))?.to_string();
    let mut set = json!({ "handle": handle });
    if let Some(sc) = p.get("scale") {
        set["scale"] = sc.clone();
    }
    if let Some(l) = p.get("locked") {
        set["locked"] = l.clone();
    }
    for key in ["level", "sheetSet", "traces"] {
        if let Some(v) = p.get(key) {
            set[key] = v.clone();
        }
    }
    if set.as_object().is_some_and(|o| o.len() > 1) {
        s.execute("viewport.set", &set)?;
    }
    // A title block on the sheet shows the scale of the viewport just added.
    if !title_inserts(s.doc()?, &layout).is_empty()
        && let Some(EntityKind::Viewport(v)) = aad_doc::Handle::parse_hex(&handle).and_then(|h| s.doc().ok()?.entity(h).map(|e| e.kind.clone()))
        && v.view_height > 1e-12
    {
        let mut f = BTreeMap::new();
        f.insert("SCALE".to_string(), scale_text(v.height / v.view_height));
        write_fields(s.doc_mut()?, &layout, &f);
    }
    Ok(json!({ "handle": handle }))
}

fn run_vp_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let hex = str_param(p, "handle").ok_or_else(|| bad("viewport.remove", "`handle` is required"))?;
    let h = aad_doc::Handle::parse_hex(hex).ok_or_else(|| bad("viewport.remove", "`handle` is not a handle"))?;
    let d = s.doc_mut()?;
    let is_vp = d.entity(h).is_some_and(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.id != 1));
    if !is_vp {
        return Err(bad("viewport.remove", "that is not a viewport"));
    }
    d.remove_entity(h);
    Ok(json!({ "removed": hex }))
}

/// Layout for the nameplate plot window: explicit `layout`, else Nameplate L, else legacy / current paper.
fn plotwindow_layout(s: &mut Session, p: &Value) -> Result<String> {
    if let Some(n) = str_param(p, "layout") {
        let _ = s.execute("sheet.designer", &json!({ "landscape": !n.eq_ignore_ascii_case(TITLE_PLATE_P) }));
        return s.doc()?.layout(n).map(|l| l.name.clone()).ok_or_else(|| bad("sheet.plotwindow", format!("no layout named `{n}`")));
    }
    let _ = s.execute("sheet.designer", &json!({}));
    if s.doc()?.layout(TITLE_PLATE_L).is_some() {
        return Ok(TITLE_PLATE_L.to_string());
    }
    if s.doc()?.layout(TITLE_PLATE).is_some() {
        return Ok(TITLE_PLATE.to_string());
    }
    match s.layout_space() {
        Space::Paper(n) => Ok(n),
        Space::Model => Err(bad("sheet.plotwindow", "switch to a layout or open the nameplate first")),
    }
}

/// Model extents used to fit a plot window (limits when the model is empty).
fn model_fit_extents(d: &Drawing) -> Bounds2 {
    let mut ext = d.extents(&Space::Model);
    if ext.is_empty() {
        let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
        let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
        ext = Bounds2::new(lo, hi);
    }
    ext
}

/// Optional model-space rectangle from Print's window pick (`window: [x0,y0,x1,y1]`).
fn model_window_extents(d: &Drawing, p: &Value) -> Result<Bounds2> {
    let Some(v) = p.get("window").filter(|v| !v.is_null()) else {
        return Ok(model_fit_extents(d));
    };
    let a: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    match a[..] {
        [x0, y0, x1, y1] if a.iter().all(|n| n.is_finite() && n.abs() < 1e12) && x0 != x1 && y0 != y1 => {
            Ok(Bounds2::new(Vec2::new(x0.min(x1), y0.min(y1)), Vec2::new(x0.max(x1), y0.max(y1))))
        }
        _ => Err(bad("sheet.plotwindow", "`window` must be [x0, y0, x1, y1]: two different corners")),
    }
}

/// Paper units per model unit that just fit `ext` into a viewport of size `w` × `h`.
fn exact_fit_scale(ext: Bounds2, w: f64, h: f64) -> f64 {
    let aspect = (w / h.max(1e-12)).max(1e-12);
    let vh = (ext.height().max(ext.width() / aspect) * 1.02).max(1e-9);
    (h / vh).max(1e-12)
}

/// Add or update the sheet's plot window so the model fits at the nearest standard scale.
fn run_plotwindow(s: &mut Session, p: &Value) -> Result<Value> {
    let layout = plotwindow_layout(s, p)?;
    // Ensure a title block / border so the default plot area sits above it.
    let _ = s.execute("sheet.titleblock", &json!({ "layout": layout }))?;
    let existing = {
        let d = s.doc()?;
        let l = d.layouts.iter().find(|l| l.name == layout).ok_or_else(|| bad("sheet.plotwindow", "no such layout"))?;
        l.entities.iter().find_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id != 1 => Some(e.handle.hex()),
            _ => None,
        })
    };
    let (handle, created) = match existing {
        Some(h) => (h, false),
        None => {
            let mut add = json!({ "layout": layout });
            if let (Some(a), Some(b), Some(o)) = (point_param(p, "p1"), point_param(p, "p2"), add.as_object_mut()) {
                o.insert("p1".into(), json!([a.x, a.y]));
                o.insert("p2".into(), json!([b.x, b.y]));
            }
            let r = s.execute("viewport.add", &add)?;
            let h = r["handle"].as_str().ok_or_else(|| bad("sheet.plotwindow", "the plot window was not created"))?.to_string();
            (h, true)
        }
    };
    let locked = bool_or(p, "locked", true);
    let (scale_text_out, scale_num, center) = {
        let d = s.doc()?;
        let h = aad_doc::Handle::parse_hex(&handle).ok_or_else(|| bad("sheet.plotwindow", "bad viewport handle"))?;
        let EntityKind::Viewport(v) = d.entity(h).map(|e| e.kind.clone()).ok_or_else(|| bad("sheet.plotwindow", "no viewport"))? else {
            return Err(bad("sheet.plotwindow", "not a viewport"));
        };
        let ext = model_window_extents(d, p)?;
        let exact = exact_fit_scale(ext, v.width, v.height);
        let nearest = crate::annot::nearest_plot_scale(exact);
        let scale_label = if nearest.nearest.is_empty() { scale_text(exact) } else { nearest.nearest.clone() };
        let scale_num = if scale_label.contains(':') {
            let mut parts = scale_label.split(':');
            let a: f64 = parts.next().unwrap_or("1").parse().unwrap_or(1.0);
            let b: f64 = parts.next().unwrap_or("1").parse().unwrap_or(1.0);
            (a / b.max(1e-12)).max(1e-12)
        } else {
            exact
        };
        (scale_label, scale_num, ext.center())
    };
    // Unlock first so a previously locked plot window can be re-fitted.
    s.execute(
        "viewport.set",
        &json!({
            "handle": handle,
            "scale": scale_text_out,
            "center": [center.x, center.y],
            "locked": false,
        }),
    )?;
    if locked {
        s.execute("viewport.set", &json!({ "handle": handle, "locked": true }))?;
    }
    if !title_inserts(s.doc()?, &layout).is_empty() {
        let mut f = BTreeMap::new();
        f.insert("SCALE".to_string(), scale_text_out.clone());
        write_fields(s.doc_mut()?, &layout, &f);
    }
    // Show the sheet so the plot window is visible.
    s.execute("layout.set", &json!({ "name": layout }))?;
    Ok(json!({
        "handle": handle,
        "layout": layout,
        "created": created,
        "scale": scale_num,
        "scaleText": scale_text_out,
        "locked": locked,
        "message": format!("Plot window on {layout} at {scale_text_out}"),
    }))
}

// ---------------- sheet index ----------------

const MAX_INDEX_SHEETS: usize = 500;
const INDEX_LAYER: &str = "A-ANNO-SIDX";
const INDEX_OWNED: &str = "sheetIndexOwned";

/// The sheet layouts in drawing order, nameplates left out.
fn index_layouts(d: &Drawing) -> Vec<String> {
    d.layouts.iter().filter(|l| !is_nameplate(&l.name)).map(|l| l.name.clone()).take(MAX_INDEX_SHEETS).collect()
}

fn index_row(d: &Drawing, layout: &str) -> Value {
    let f = read_fields(d, layout);
    let get = |k: &str| f.get(k).map(String::as_str).unwrap_or("").trim().to_string();
    let number = match get("SHEET") {
        n if n.is_empty() => layout.to_string(),
        n => n,
    };
    let scale = match get("SCALE") {
        s if s.is_empty() => default_fields(d, layout).remove("SCALE").unwrap_or_default(),
        s => s,
    };
    json!({ "layout": layout, "number": number, "title": get("TITLE"), "scale": scale, "rev": get("REV"), "titleBlock": !title_inserts(d, layout).is_empty() })
}

fn run_index(s: &mut Session, p: &Value) -> Result<Value> {
    let mut renumbered = 0usize;
    if let Some(r) = p.get("renumber").filter(|v| !v.is_null()) {
        let prefix = r.get("prefix").and_then(Value::as_str).unwrap_or("A-");
        if prefix.chars().count() > 20 {
            return Err(bad("sheet.index", "`prefix` is too long"));
        }
        let int = |k: &str, def: i64| match r.get(k) {
            None | Some(Value::Null) => Ok(def),
            Some(v) => v.as_i64().ok_or_else(|| bad("sheet.index", format!("`{k}` must be a whole number"))),
        };
        let (start, step) = (int("start", 101)?, int("step", 1)?);
        let layouts = index_layouts(s.doc()?);
        // Work out every number first so a bad range changes nothing.
        let mut numbers = Vec::new();
        let mut n = start;
        for l in &layouts {
            if !title_inserts(s.doc()?, l).is_empty() {
                numbers.push((l.clone(), format!("{prefix}{n}")));
                n = n.checked_add(step).ok_or_else(|| bad("sheet.index", "the sheet numbers overflow"))?;
            }
        }
        for (l, num) in numbers {
            let fields = BTreeMap::from([("SHEET".to_string(), num)]);
            renumbered += write_fields(s.doc_mut()?, &l, &fields);
        }
    }
    let rows: Vec<Value> = {
        let d = s.doc()?;
        index_layouts(d).iter().map(|l| index_row(d, l)).collect()
    };
    let mut drawn = Value::Null;
    if bool_or(p, "draw", false) {
        let layout = layout_of(s, "sheet.index", p)?;
        let mm = match p.get("height") {
            None | Some(Value::Null) => 2.5,
            Some(v) => v
                .as_f64()
                .filter(|h| h.is_finite() && (0.5..=50.0).contains(h))
                .ok_or_else(|| bad("sheet.index", "`height` must be between 0.5 and 50 mm"))?,
        };
        let sh = sheet_of(s.doc()?, &layout, "sheet.index")?;
        let k = 1.0 / sh.unit_mm;
        let th = mm * k;
        let at = point_param(p, "at").unwrap_or_else(|| Vec2::new(sh.printable.min.x + 10.0 * k, sh.printable.max.y - 10.0 * k));
        let header = ["SHEET NO.", "TITLE", "SCALE", "REV"];
        let body: Vec<[String; 4]> = rows
            .iter()
            .map(|r| {
                let c = |key: &str| r.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                [c("number"), c("title"), c("scale"), c("rev")]
            })
            .collect();
        let mut widths: Vec<f64> = header.iter().map(|h| h.chars().count() as f64).collect();
        for row in &body {
            for (w, cell) in widths.iter_mut().zip(row) {
                *w = w.max(cell.chars().count() as f64);
            }
        }
        let widths: Vec<f64> = widths.iter().map(|w| w * th * 0.75 + th * 1.2).collect();
        let total: f64 = widths.iter().sum();
        let rh = th * 2.0;
        let old: Vec<aad_doc::Handle> = match project::get(s.doc()?, INDEX_OWNED).get(&layout) {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(aad_doc::Handle::parse_hex).collect(),
            _ => Vec::new(),
        };
        let space = Space::Paper(layout.clone());
        for h in old {
            if s.doc()?.space(&space).is_some_and(|st| st.get(h).is_some()) {
                s.doc_mut()?.remove_entity(h);
            }
        }
        let nrows = body.len() + 1;
        let bottom = at.y - rh * nrows as f64;
        let handles = in_layout(s, &layout, |s| {
            ensure_layer(s.doc_mut()?, INDEX_LAYER, 7, "Sheet index");
            let mut hs = Vec::new();
            let mut y = at.y;
            for (ri, row) in std::iter::once(header.map(String::from)).chain(body.iter().cloned()).enumerate() {
                let mut x = at.x;
                for (cell, w) in row.iter().zip(&widths) {
                    let kind = make_text(
                        s,
                        Vec2::new(x + th * 0.4, y - rh * 0.5),
                        cell,
                        if ri == 0 { th } else { th * 0.9 },
                        0.0,
                        (HAlign::Left, VAlign::Middle),
                    );
                    hs.push(super::hcw::add_on(s, INDEX_LAYER, kind)?);
                    x += w;
                }
                y -= rh;
            }
            for i in 0..=nrows {
                let y = at.y - rh * i as f64;
                hs.push(super::hcw::add_on(s, INDEX_LAYER, line(Vec2::new(at.x, y), Vec2::new(at.x + total, y)))?);
            }
            let mut x = at.x;
            for w in std::iter::once(&0.0).chain(widths.iter()) {
                x += w;
                hs.push(super::hcw::add_on(s, INDEX_LAYER, line(Vec2::new(x, at.y), Vec2::new(x, bottom)))?);
            }
            hs.push(super::hcw::add_on(s, INDEX_LAYER, line(Vec2::new(at.x, at.y), Vec2::new(at.x, bottom)))?);
            Ok(hs)
        })?;
        let mut owned = match project::get(s.doc()?, INDEX_OWNED) {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        let n = handles.len();
        owned.insert(layout.clone(), Value::Array(handles.iter().map(|h| json!(h.hex())).collect()));
        project::set(s.doc_mut()?, INDEX_OWNED, &Value::Object(owned));
        drawn = json!({ "layout": layout, "objects": n });
    }
    Ok(json!({ "sheets": rows, "renumbered": renumbered, "drawn": drawn }))
}

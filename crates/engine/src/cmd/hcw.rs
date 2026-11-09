//! Architectural drafting tools built into AAD, re-implemented in Rust from the hcwCAD-KIT
//! command set (MIT, Holgundi Consulting Works): the standard architectural and building-permit
//! layer sets, a presentation (lineweight) set, room labels (ROOM) and the polyline area label
//! (RAREA).
//!
//! The room and area tools read the drawing's insertion units (`INSUNITS`; a metric drawing with
//! no units set is taken as millimetres, an imperial one as inches), measure in square metres or
//! square feet, and size label text from 125 mm of real height.

use aad_color::Color;
use aad_doc::{Drawing, EntityKind, Group, HAlign, Handle, Layer, Lineweight, LwPolyline, VAlign};
use aad_geom::{Polyline, Vec2};
use serde_json::{Value, json};

use super::draw::make_text;
use super::helpers::{lwpoly, rect_vertices};
use super::machines::SelectRun;
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Project-data group description for a ROOM rectangle + its three labels.
const ROOM_GROUP_DESC: &str = "aad-room";

pub(crate) const LABEL_LAYER: (&str, i16) = ("ROOM-LABELS", 3);
pub(crate) const RECT_LAYER: (&str, i16) = ("ROOM-RECT", 8);
/// Room label text height in millimetres of real size.
const TEXT_MM: f64 = 125.0;

/// Building-permit layers (BBMP / AutoPlan naming kept for interchange): name, ACI colour,
/// linetype, lineweight (hundredths of mm).
pub(crate) const PERMIT_LAYERS: &[(&str, i16, &str, u16)] = &[
    ("BP-SITE-BOUNDARY", 1, "Continuous", 50),
    ("BP-SITE-SETBACK", 3, "DASHED2", 25),
    ("BP-SITE-SURRENDER", 3, "DASHED", 18),
    ("BP-ROAD-EDGE", 8, "Continuous", 35),
    ("BP-ROAD-CL", 8, "CENTER2", 18),
    ("BP-DRIVEWAY", 52, "Continuous", 25),
    ("BP-COMP-WALL", 9, "Continuous", 18),
    ("BP-BUILDING-CUT", 7, "Continuous", 50),
    ("BP-BUILDING-VIEW", 7, "Continuous", 25),
    ("BP-DOOR", 252, "Continuous", 18),
    ("BP-WINDOW", 252, "Continuous", 13),
    ("BP-STAIR", 142, "Continuous", 25),
    ("BP-LIFT", 141, "Continuous", 18),
    ("BP-RAMP", 140, "Continuous", 18),
    ("BP-ROOM", 153, "Continuous", 18),
    ("BP-BALCONY", 144, "Continuous", 18),
    ("BP-TERRACE", 96, "Continuous", 18),
    ("BP-PORCH", 22, "Continuous", 18),
    ("BP-LOFT", 33, "DASHED", 13),
    ("BP-MEZZANINE", 33, "Continuous", 18),
    ("BP-SHAFT", 132, "Continuous", 18),
    ("BP-OPEN-SPACE", 3, "Continuous", 18),
    ("BP-PARKING", 151, "Continuous", 18),
    ("BP-PARKING-AISLE", 151, "DASHED", 13),
    ("BP-RWH", 133, "Continuous", 18),
    ("BP-SERVICES", 134, "Continuous", 13),
    ("BP-FIRE", 1, "DASHED", 25),
    ("BP-LEVEL", 6, "Continuous", 13),
    ("BP-PLANTATION", 62, "Continuous", 13),
    ("BP-GARAGE", 152, "Continuous", 18),
    ("BP-ACCESSORY", 152, "Continuous", 13),
    ("BP-PwD", 94, "Continuous", 13),
    ("BP-FAR", 6, "Continuous", 13),
    ("BP-SURVEY-SKETCH", 9, "Continuous", 13),
    ("BP-SECTION", 30, "Continuous", 35),
    ("BP-ELEVATION", 30, "Continuous", 25),
    ("BP-STRUC-REF", 3, "Continuous", 13),
    ("BP-EXISTING", 251, "Continuous", 13),
    ("BP-DEMOLISH", 251, "DASHED", 13),
];

/// Presentation (graphic) layers ordered by lineweight for floor-plan hierarchy: cut walls
/// heaviest, furniture and notes lightest. Use with the built-in `presentation.stb` plot styles
/// for screened poche and soft overhead.
pub(crate) const PRESENTATION_LAYERS: &[(&str, i16, &str, u16, &str)] = &[
    ("G-HEAVY", 7, "Continuous", 50, "Cut / silhouette - heaviest punch"),
    ("G-MED", 7, "Continuous", 35, "Accent - stair top riser, retaining"),
    ("G-LIGHT", 8, "Continuous", 18, "Floor edges, frames, counters, fixtures"),
    ("G-FINE", 8, "Continuous", 13, "Door swings, sills, wall sub-assemblies"),
    ("G-XFINE", 9, "Continuous", 9, "Furniture and light detail"),
    ("G-HIDDEN", 8, "HIDDEN", 18, "Overhead or below (dashed)"),
    ("G-HATCH", 9, "Continuous", 10, "Poche, material and shadow hatch"),
    ("G-ANNO", 1, "Continuous", 18, "Presentation notes (distinct from geometry)"),
];

/// Standard architectural layer set (AIA-style discipline prefixes): name, ACI colour, linetype,
/// lineweight (hundredths of mm), description.
pub(crate) const STANDARD_LAYERS: &[(&str, i16, &str, u16, &str)] = &[
    ("AN-REF", 250, "Continuous", 9, "Construction lines / viewport borders"),
    ("AN-GRID", 8, "CENTER2", 13, "Grid and centre lines"),
    ("AN-SYMB", 7, "Continuous", 18, "North point / symbols / logos"),
    ("AN-TEXT", 7, "Continuous", 18, "All text - general, title block, keynotes"),
    ("AN-DIMS", 8, "Continuous", 18, "Dimensions and leaders"),
    ("AN-HATCH", 9, "Continuous", 10, "Hatching and fill patterns"),
    ("A-WALL", 1, "Continuous", 50, "Walls - new and existing (cut); heaviest punch"),
    ("A-DEMO", 1, "DASHED2", 25, "Elements to be demolished"),
    ("A-COL", 1, "Continuous", 50, "Columns (cut); heaviest punch"),
    ("A-DOOR", 2, "Continuous", 25, "Doors - frame and swing arc"),
    ("A-WIND", 4, "Continuous", 25, "Windows - frame and sill"),
    ("A-GLAZ", 4, "HIDDEN", 18, "Glazing / glass line"),
    ("A-STAIR", 2, "Continuous", 25, "Stairs - treads, stringers, ramps"),
    ("A-RAIL", 6, "Continuous", 20, "Handrails and balustrades"),
    ("A-ROOF", 1, "Continuous", 25, "Roof outline and edge"),
    ("A-FLR", 3, "Continuous", 18, "Floor finish / slab outline"),
    ("A-CLNG", 3, "Continuous", 18, "Ceiling outline and features"),
    ("A-DETAIL", 1, "Continuous", 25, "Detail bubbles and reference marks"),
    ("I-FURN", 3, "Continuous", 20, "Loose furniture"),
    ("I-JOIN", 5, "Continuous", 20, "Fixed joinery - kitchen, wardrobes, built-ins"),
    ("I-FINISH", 30, "Continuous", 18, "Finish zones and material areas"),
    ("I-DECOR", 140, "Continuous", 15, "Decorative and soft furnishing elements"),
    ("E-CEIL", 6, "Continuous", 18, "Ceiling fittings - lights, fans, AC, sensors"),
    ("E-POINT", 2, "Continuous", 18, "Wall points - power, data, switches"),
    ("E-WIRE", 30, "Continuous", 15, "Wiring runs and conduit schematic"),
    ("P-FIXT", 6, "Continuous", 18, "Plumbing fixtures - basin, WC, shower, bath"),
    ("P-PIPE", 4, "Continuous", 18, "Pipes - supply, drain and vent"),
    ("S-BOUND", 1, "PHANTOM2", 35, "Site and property boundary"),
    ("S-ROAD", 8, "Continuous", 25, "Road edge and carriageway"),
    ("S-LAND", 3, "Continuous", 18, "Landscaping and planting"),
    ("S-TREE", 140, "Continuous", 18, "Trees - canopy and trunk"),
    ("S-LEVEL", 2, "Continuous", 18, "Spot levels and contours"),
    ("S-DRAIN", 4, "Continuous", 18, "Site drainage and channels"),
    ("S-FENCE", 8, "Continuous", 20, "Fences and boundary walls"),
    ("PR-TONE", 9, "Continuous", 10, "Presentation tone - shadow, fill, material"),
    ("PR-CLOUD", 2, "Continuous", 25, "Revision clouds"),
];

/// Two-letter room codes and the room type each names.
const ROOM_TYPES: &[(&str, &str)] = &[
    ("br", "Bedroom"),
    ("di", "Dining Room"),
    ("ki", "Kitchen"),
    ("li", "Living Room"),
    ("ba", "Bathroom"),
    ("at", "Attached Toilet"),
    ("ct", "Common Toilet"),
    ("mb", "Master Bedroom"),
    ("st", "Study Room"),
    ("sr", "Store Room"),
    ("pr", "Pooja Room"),
    ("bc", "Balcony"),
    ("te", "Terrace"),
    ("sc", "Staircase"),
    ("co", "Corridor"),
    ("en", "Entrance"),
    ("ut", "Utility"),
    ("ga", "Garage"),
    ("gd", "Garden"),
    ("cy", "Courtyard"),
    ("lo", "Lobby"),
    ("of", "Office"),
    ("gr", "Guest Room"),
    ("pa", "Pantry"),
    ("la", "Laundry"),
    ("wr", "Wardrobe"),
    ("dr", "Dressing Room"),
    ("ht", "Home Theater"),
    ("gy", "Gym"),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("hcwlayers", "Architectural Layer Set", run_layers)
            .menu(&["Format", "Architectural Layer Set"])
            .alias(&["hcwlayer", "archlayers", "stdlayers"])
            .params(
                "{set?: \"standard\"|\"permit\"|\"presentation\"|\"both\"|\"all\" (aliases: hcw→standard, bp→permit; default standard)} → {created, existing, set}",
            ),
        CommandSpec::new("room", "Room Label", run_room)
            .menu(&["Architecture", "Rooms and Areas", "Room Label"])
            .alias(&["hcwroom", "roomc"])
            .params(
                "{p1, p2, name (room name or two-letter code), floorPrefix?, textHeight? (drawing units), rectangle?: bool, units?: mm|cm|m|ft|in}",
            )
            .interactive(|_| Ok(Box::new(RoomM::default()))),
        CommandSpec::new("room.settings", "Room Label Settings...", run_room_settings)
            .menu(&["Architecture", "Rooms and Areas", "Room Label Settings..."])
            .params("{units?: mm|cm|m|ft|in (\"\": the drawing's), box?: bool (draw the room rectangle), textHeight?: drawing units (null: default), font?: family (\"\": the current text style)} → the settings (saved with the drawing; the ROOM command uses them; typed or from the menu it opens the dialog)"),
        CommandSpec::new("rarea", "Room Area", run_rarea)
            .menu(&["Architecture", "Rooms and Areas", "Room Area"])
            .alias(&["hcwroomarea"])
            .params("{handles?: closed polylines, textHeight?, units?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("rarea", "RAREA", "Select a closed polyline")))),
    ]
}

// ---------------------------------------------------------------------------------------------
// Units

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Units {
    Mm,
    Cm,
    M,
    Ft,
    In,
}

impl Units {
    fn parse(s: &str) -> Option<Units> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "mm" | "millimeters" | "millimetres" => Units::Mm,
            "cm" | "centimeters" | "centimetres" => Units::Cm,
            "m" | "meters" | "metres" => Units::M,
            "ft" | "feet" => Units::Ft,
            "in" | "inches" => Units::In,
            _ => return None,
        })
    }
    /// Metres in one drawing unit.
    pub(crate) fn metres(self) -> f64 {
        match self {
            Units::Mm => 0.001,
            Units::Cm => 0.01,
            Units::M => 1.0,
            Units::Ft => 0.3048,
            Units::In => 0.0254,
        }
    }
    fn imperial(self) -> bool {
        matches!(self, Units::Ft | Units::In)
    }
}

/// The units of the drawing: `units` if given, else `INSUNITS`, else millimetres (metric drawing)
/// or inches.
pub(crate) fn drawing_units(d: &Drawing, cmd: &str, p: &Value) -> Result<Units> {
    if let Some(u) = str_param(p, "units") {
        return Units::parse(u).ok_or_else(|| bad(cmd, "`units` must be mm, cm, m, ft or in"));
    }
    let from_header = match d.header.i64("INSUNITS", 0) {
        1 => Units::In,
        2 => Units::Ft,
        4 => Units::Mm,
        5 => Units::Cm,
        6 => Units::M,
        _ if d.header.i64("MEASUREMENT", 0) == 1 => Units::Mm,
        _ => Units::In,
    };
    Ok(infer_from_extents(d, from_header))
}

/// Inches is the default of a drawing that never set its unit, so a metric plan often says inches.
/// When the header gives inches or feet but the drawing would then be longer than 500 m, take the
/// unit that makes it a building-sized drawing (millimetres, then centimetres, then metres). A unit
/// the header states as metric is trusted as it is.
fn infer_from_extents(d: &Drawing, header: Units) -> Units {
    const LARGEST_PLAUSIBLE_M: f64 = 500.0;
    const SMALLEST_PLAUSIBLE_M: f64 = 3.0;
    if !matches!(header, Units::In | Units::Ft) {
        return header;
    }
    let b = d.extents(&aad_doc::Space::Model);
    let size = (b.max.x - b.min.x).max(b.max.y - b.min.y);
    if !size.is_finite() || size <= 0.0 || size * header.metres() <= LARGEST_PLAUSIBLE_M {
        return header;
    }
    [Units::Mm, Units::Cm, Units::M]
        .into_iter()
        .find(|u| (SMALLEST_PLAUSIBLE_M..=LARGEST_PLAUSIBLE_M).contains(&(size * u.metres())))
        .unwrap_or(header)
}

/// Feet and inches of a length in inches, rounded to the inch: `12'-6"`.
fn feet_inches(inches: f64) -> String {
    let total = inches.abs().round() as i64;
    format!("{}'-{}\"", total / 12, total % 12)
}

fn format_dim(u: Units, w: f64, h: f64) -> String {
    if u.imperial() {
        let to_in = u.metres() / 0.0254;
        format!("{} x {}", feet_inches(w * to_in), feet_inches(h * to_in))
    } else {
        format!("{:.2} m x {:.2} m", w * u.metres(), h * u.metres())
    }
}

/// An area in drawing units squared as (value, unit label): square metres, or square feet in an
/// imperial drawing.
fn area_in(u: Units, area: f64) -> (f64, &'static str) {
    let sq_m = area * u.metres() * u.metres();
    if u.imperial() { (sq_m / (0.3048 * 0.3048), "sq ft") } else { (sq_m, "m2") }
}

fn text_height(u: Units, p: &Value) -> Result<f64> {
    let h = f64_or(p, "textHeight", TEXT_MM / 1000.0 / u.metres());
    if h.is_finite() && h > 0.0 { Ok(h) } else { Err(bad("room", "`textHeight` must be positive")) }
}

// ---------------------------------------------------------------------------------------------
// Layers

/// Make `name` exist with the given look; a linetype missing from the library falls back to
/// Continuous. Returns whether the layer was created.
pub(crate) fn ensure_layer(d: &mut Drawing, name: &str, aci: i16, linetype: &str, lw: u16, description: &str) -> bool {
    if d.layer(name).is_some() {
        return false;
    }
    if d.linetype(linetype).is_none()
        && let Some(lt) = aad_doc::library::standard_linetypes_for(d).into_iter().find(|l| l.name.eq_ignore_ascii_case(linetype))
    {
        d.linetypes.push(lt);
    }
    let mut l = Layer::new(name);
    l.color = Color::from_aci(aci);
    l.linetype = d.linetype(linetype).map(|t| t.name.clone()).unwrap_or_else(|| "Continuous".into());
    l.lineweight = Lineweight::Mm100(lw);
    l.description = description.into();
    d.layers.push(l);
    true
}

fn normalize_layer_set(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "standard" | "hcw" | "arch" => Some("standard"),
        "permit" | "bp" | "bplt" => Some("permit"),
        "presentation" | "graphic" | "weights" => Some("presentation"),
        "both" => Some("both"),
        "all" => Some("all"),
        _ => None,
    }
}

pub(super) fn run_layers(s: &mut Session, p: &Value) -> Result<Value> {
    let set = normalize_layer_set(str_param(p, "set").unwrap_or("standard"))
        .ok_or_else(|| bad("hcwlayers", "`set` must be standard, permit, presentation, both or all"))?;
    let d = s.doc_mut()?;
    let (mut created, mut existing) = (0, 0);
    let mut count = |made: bool| if made { created += 1 } else { existing += 1 };
    let want_standard = matches!(set, "standard" | "both" | "all");
    let want_permit = matches!(set, "permit" | "both" | "all");
    let want_presentation = matches!(set, "presentation" | "all");
    if want_standard {
        for (n, c, lt, lw, desc) in STANDARD_LAYERS {
            count(ensure_layer(d, n, *c, lt, *lw, desc));
        }
    }
    if want_permit {
        for (n, c, lt, lw) in PERMIT_LAYERS {
            count(ensure_layer(d, n, *c, lt, *lw, ""));
        }
    }
    if want_presentation {
        for (n, c, lt, lw, desc) in PRESENTATION_LAYERS {
            count(ensure_layer(d, n, *c, lt, *lw, desc));
            // Named styles from the built-in presentation.stb table (Normal if unused).
            let style = match *n {
                "G-HEAVY" => "Heavy",
                "G-MED" => "Medium",
                "G-LIGHT" => "Light",
                "G-FINE" | "G-XFINE" => "Fine",
                "G-HIDDEN" => "Screened 40%",
                "G-HATCH" => "Screened 80%",
                "G-ANNO" => "Anno",
                _ => "Normal",
            };
            if let Some(l) = d.layers.iter_mut().find(|l| l.name == *n) {
                l.plot_style = style.into();
            }
        }
    }
    Ok(json!({ "created": created, "existing": existing, "set": set }))
}

/// Add `kind` to the current space on `layer`.
pub(crate) fn add_on(s: &mut Session, layer: &str, kind: EntityKind) -> Result<Handle> {
    let mut common = s.current_common();
    common.layer = layer.into();
    let space = s.space();
    Ok(s.doc_mut()?.add(&space, common, kind)?)
}

fn ensure_room_layers(s: &mut Session) -> Result<()> {
    let d = s.doc_mut()?;
    ensure_layer(d, LABEL_LAYER.0, LABEL_LAYER.1, "Continuous", 18, "Room labels");
    ensure_layer(d, RECT_LAYER.0, RECT_LAYER.1, "Continuous", 18, "Room rectangles");
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Room label settings

const SETTINGS_KEY: &str = "roomSettings";
/// The text style the room labels use when a font is chosen for them.
const LABEL_STYLE: &str = "ROOM-LABELS";

/// Units of the size text, box, text height and font of the room labels. Saved with the drawing
/// (project data), so every room made in it follows them; a parameter of the ROOM call wins.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RoomSettings {
    /// mm, cm, m, ft or in; empty: the drawing's units.
    pub units: String,
    pub boxed: bool,
    /// Drawing units; `None`: the default height.
    pub text_height: Option<f64>,
    /// A font family; empty: the current text style.
    pub font: String,
}

pub(crate) fn room_settings(d: &Drawing) -> RoomSettings {
    let v = super::project::get(d, SETTINGS_KEY);
    RoomSettings {
        units: v["units"].as_str().filter(|u| Units::parse(u).is_some()).unwrap_or("").to_string(),
        boxed: v["box"].as_bool().unwrap_or(true),
        text_height: v["textHeight"].as_f64().filter(|h| h.is_finite() && *h > 0.0),
        font: v["font"].as_str().unwrap_or("").chars().take(120).collect(),
    }
}

fn settings_json(r: &RoomSettings) -> Value {
    json!({"units": r.units, "box": r.boxed, "textHeight": r.text_height, "font": r.font})
}

/// `room.settings {units?, box?, textHeight?, font?}`: the settings (changed first when given). A
/// `textHeight` of null (or 0) goes back to the default; `units` or `font` of "" likewise.
fn run_room_settings(s: &mut Session, p: &Value) -> Result<Value> {
    let mut r = room_settings(s.doc()?);
    let mut changed = false;
    if let Some(u) = p.get("units") {
        let u = u.as_str().ok_or_else(|| bad("room.settings", "`units` must be text"))?.trim();
        if !u.is_empty() && Units::parse(u).is_none() {
            return Err(bad("room.settings", "`units` must be mm, cm, m, ft or in (or blank for the drawing's)"));
        }
        r.units = u.to_ascii_lowercase();
        changed = true;
    }
    if let Some(b) = p.get("box") {
        r.boxed = b.as_bool().ok_or_else(|| bad("room.settings", "`box` must be true or false"))?;
        changed = true;
    }
    if let Some(h) = p.get("textHeight") {
        r.text_height = match h {
            Value::Null => None,
            v => match v.as_f64() {
                Some(0.0) => None,
                Some(x) if x.is_finite() && x > 0.0 && x < 1e9 => Some(x),
                _ => return Err(bad("room.settings", "`textHeight` must be a positive number (or null for the default)")),
            },
        };
        changed = true;
    }
    if let Some(f) = p.get("font") {
        r.font = f.as_str().ok_or_else(|| bad("room.settings", "`font` must be text"))?.trim().chars().take(120).collect();
        changed = true;
    }
    if changed {
        super::project::set(s.doc_mut()?, SETTINGS_KEY, &settings_json(&r));
        s.touch();
    }
    Ok(settings_json(&r))
}

/// Make the label text style exist with `font`, and return its name.
fn ensure_label_style(s: &mut Session, font: &str) -> Result<()> {
    let d = s.doc_mut()?;
    match d.text_styles.iter_mut().find(|t| t.name.eq_ignore_ascii_case(LABEL_STYLE)) {
        Some(t) => t.font = font.to_string(),
        None => d.text_styles.push(aad_doc::TextStyle { name: LABEL_STYLE.into(), font: font.to_string(), ..Default::default() }),
    }
    Ok(())
}

/// The call's own parameters over the saved settings: what a room is made with.
fn with_settings(s: &Session, p: &Value) -> Result<(Value, RoomSettings)> {
    let r = room_settings(s.doc()?);
    let mut o = p.as_object().cloned().unwrap_or_default();
    if !o.contains_key("units") && !r.units.is_empty() {
        o.insert("units".into(), json!(r.units));
    }
    if !o.contains_key("textHeight")
        && let Some(h) = r.text_height
    {
        o.insert("textHeight".into(), json!(h));
    }
    Ok((Value::Object(o), r))
}
// ---------------------------------------------------------------------------------------------
// ROOM

/// The room type a typed name stands for: a two-letter code names its type, anything else is
/// taken as typed.
pub(crate) fn room_name(input: &str) -> String {
    let t = input.trim();
    ROOM_TYPES.iter().find(|(code, _)| t.eq_ignore_ascii_case(code)).map(|(_, name)| (*name).to_string()).unwrap_or_else(|| t.to_string())
}

struct RoomLabel {
    name: String,
    width: f64,
    height: f64,
    handles: Vec<Handle>,
    area: f64,
    unit: &'static str,
}

fn make_room(s: &mut Session, p1: Vec2, p2: Vec2, name: &str, prefix: &str, rectangle: bool, p: &Value) -> Result<RoomLabel> {
    let (p, rs) = with_settings(s, p)?;
    let p = &p;
    let u = drawing_units(s.doc()?, "room", p)?;
    let th = text_height(u, p)?;
    let (w, h) = ((p2.x - p1.x).abs(), (p2.y - p1.y).abs());
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() {
        return Err(bad("room", "the two corners must differ in both x and y"));
    }
    let name = room_name(name);
    if name.is_empty() {
        return Err(bad("room", "`name` is required"));
    }
    let full = if prefix.trim().is_empty() { name.clone() } else { format!("{} {}", prefix.trim(), name) };
    let center = Vec2::new((p1.x + p2.x) / 2.0, (p1.y + p2.y) / 2.0);
    let (area, unit) = area_in(u, w * h);
    ensure_room_layers(s)?;
    let mut handles = Vec::new();
    if rectangle {
        handles.push(add_on(s, RECT_LAYER.0, lwpoly(rect_vertices(p1, p2), true))?);
    }
    let centered = (HAlign::Center, VAlign::Middle);
    if !rs.font.is_empty() {
        ensure_label_style(s, &rs.font)?;
    }
    let lines = [(full.to_uppercase(), 0.0, th), (format_dim(u, w, h), th * 1.5, th * 0.8), (format!("Area: {area:.2} {unit}"), th * 2.8, th * 0.7)];
    for (text, down, height) in lines {
        let mut kind = make_text(s, Vec2::new(center.x, center.y - down), &text, height, 0.0, centered);
        if !rs.font.is_empty()
            && let EntityKind::Text(t) = &mut kind
        {
            t.style = LABEL_STYLE.into();
        }
        handles.push(add_on(s, LABEL_LAYER.0, kind)?);
    }
    // Group the rectangle (if any) with its labels so grip edits can refresh size and area.
    let key = handles.first().map(|h| h.hex()).unwrap_or_else(|| "0".into());
    let d = s.doc_mut()?;
    d.groups.push(Group {
        name: format!("AAD-ROOM-{key}"),
        description: ROOM_GROUP_DESC.into(),
        selectable: true,
        members: handles.clone(),
        child_groups: Vec::new(),
    });
    Ok(RoomLabel { name: full, width: w, height: h, handles, area, unit })
}

/// After a grip stretch (or similar edit) of a room rectangle, rewrite the size and area labels.
pub(crate) fn refit_rooms(s: &mut Session, edited: &[Handle]) -> Result<()> {
    if edited.is_empty() {
        return Ok(());
    }
    let room_groups: Vec<(usize, Vec<Handle>)> = {
        let d = s.doc()?;
        let mut out = Vec::new();
        for &h in edited {
            for i in super::groups::groups_of(d, h) {
                let g = &d.groups[i];
                if g.description != ROOM_GROUP_DESC && !g.name.starts_with("AAD-ROOM-") {
                    continue;
                }
                if out.iter().any(|(j, _)| *j == i) {
                    continue;
                }
                out.push((i, g.members.clone()));
            }
        }
        out
    };
    for (_i, members) in room_groups {
        refresh_room_labels(s, &members)?;
    }
    Ok(())
}

fn refresh_room_labels(s: &mut Session, members: &[Handle]) -> Result<()> {
    let d = s.doc()?;
    let mut corners: Option<(Vec2, Vec2)> = None;
    let mut texts: Vec<(Handle, String, f64)> = Vec::new();
    for &h in members {
        let Some(e) = d.entity(h) else { continue };
        match &e.kind {
            EntityKind::LwPolyline(pl) if e.common.layer.eq_ignore_ascii_case(RECT_LAYER.0) => {
                corners = poly_corners(pl);
            }
            EntityKind::Text(t) if e.common.layer.eq_ignore_ascii_case(LABEL_LAYER.0) => {
                texts.push((h, t.value.clone(), t.height));
            }
            _ => {}
        }
    }
    let Some((p1, p2)) = corners else {
        return Ok(());
    };
    let mut name_h = None;
    let mut dim_h = None;
    let mut area_h = None;
    let mut th = TEXT_MM / 1000.0;
    let mut name_val = String::new();
    for (h, v, height) in &texts {
        if v.starts_with("Area:") {
            area_h = Some(*h);
        } else if v.contains(" x ") || v.contains('×') {
            dim_h = Some(*h);
        } else {
            name_h = Some(*h);
            name_val = v.clone();
            th = *height;
        }
    }
    let (Some(name_h), Some(dim_h), Some(area_h)) = (name_h, dim_h, area_h) else {
        return Ok(());
    };
    let u = drawing_units(d, "room", &Value::Null)?;
    let (w, h) = ((p2.x - p1.x).abs(), (p2.y - p1.y).abs());
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() {
        return Ok(());
    }
    let center = Vec2::new((p1.x + p2.x) / 2.0, (p1.y + p2.y) / 2.0);
    let (area, unit) = area_in(u, w * h);
    let dim_s = format_dim(u, w, h);
    let area_s = format!("Area: {area:.2} {unit}");
    let updates = [(name_h, name_val.as_str(), 0.0, th), (dim_h, dim_s.as_str(), th * 1.5, th * 0.8), (area_h, area_s.as_str(), th * 2.8, th * 0.7)];
    for (handle, text, down, height) in updates {
        s.doc_mut()?.modify_entity(handle, |e| {
            if let EntityKind::Text(t) = &mut e.kind {
                t.value = text.into();
                t.height = height;
                t.insert = aad_geom::Vec3::new(center.x, center.y - down, 0.0);
                t.align_pt = Some(aad_geom::Vec3::new(center.x, center.y - down, 0.0));
                t.halign = HAlign::Center;
                t.valign = VAlign::Middle;
            }
        })?;
    }
    Ok(())
}

fn poly_corners(pl: &LwPolyline) -> Option<(Vec2, Vec2)> {
    if pl.vertices.len() < 2 {
        return None;
    }
    let mut lo = pl.vertices[0].p;
    let mut hi = lo;
    for v in &pl.vertices {
        lo = Vec2::new(lo.x.min(v.p.x), lo.y.min(v.p.y));
        hi = Vec2::new(hi.x.max(v.p.x), hi.y.max(v.p.y));
    }
    Some((lo, hi))
}

fn run_room(s: &mut Session, p: &Value) -> Result<Value> {
    let p1 = point_req("room", p, "p1")?;
    let p2 = point_req("room", p, "p2")?;
    let name = str_param(p, "name").ok_or_else(|| bad("room", "`name` is required"))?;
    let default_prefix = super::project::active_floor_prefix(s.doc()?);
    let boxed = room_settings(s.doc()?).boxed;
    let r = make_room(s, p1, p2, name, str_param(p, "floorPrefix").unwrap_or(&default_prefix), bool_or(p, "rectangle", boxed), p)?;
    Ok(json!({
        "name": r.name.to_uppercase(),
        "width": r.width,
        "height": r.height,
        "area": r.area,
        "areaUnit": r.unit,
        "handles": r.handles.iter().map(|h| h.hex()).collect::<Vec<_>>(),
    }))
}

#[derive(Default)]
struct RoomM {
    name: Option<String>,
    first: Option<Vec2>,
}

impl Interactive for RoomM {
    fn name(&self) -> &'static str {
        "ROOM"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (&self.name, self.first) {
            (None, _) => Prompt::new("Select the room", Accept::TEXT)
                .options(ROOM_TYPES.iter().map(|(code, name)| ((*name).to_string(), (*code).to_string())).collect()),
            (Some(_), None) => Prompt::new("Select FIRST corner of room", Accept::POINT),
            (Some(_), Some(a)) => Prompt::new("Select OPPOSITE corner of room", Accept::POINT).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.name, self.first, i) {
            (None, _, Input::Text(t)) if !t.trim().is_empty() => self.name = Some(t),
            (Some(_), None, Input::Point(p)) => self.first = Some(p),
            (Some(name), Some(a), Input::Point(b)) => {
                // The floor chosen in the top bar numbers the room; the saved settings decide the rest.
                let prefix = super::project::active_floor_prefix(s.doc()?);
                let boxed = room_settings(s.doc()?).boxed;
                let r = make_room(s, a, b, name, &prefix, boxed, &Value::Null)?;
                s.echo(format!(
                    "{} | {} | Area: {:.2} {}",
                    r.name.to_uppercase(),
                    format_dim(drawing_units(s.doc()?, "room", &Value::Null)?, r.width, r.height),
                    r.area,
                    r.unit
                ));
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.first.map(|a| vec![lwpoly(rect_vertices(a, c), true)]).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// RAREA

fn run_rarea(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let u = drawing_units(s.doc()?, "rarea", p)?;
    let th = text_height(u, p)?;
    let mut labels = Vec::new();
    for h in hs {
        let Some(e) = s.doc()?.entity(h).cloned() else { continue };
        let EntityKind::LwPolyline(pl) = &e.kind else { continue };
        if !pl.closed {
            continue;
        }
        let poly = Polyline { vertices: pl.vertices.clone(), closed: true };
        let (area, unit) = area_in(u, poly.area().abs());
        let b = poly.bounds();
        let at = Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
        ensure_room_layers(s)?;
        let text = format!("{area:.2} {unit}");
        let kind = make_text(s, at, &text, th * 0.8, 0.0, (HAlign::Center, VAlign::Middle));
        let label = add_on(s, LABEL_LAYER.0, kind)?;
        labels.push(json!({ "handle": label.hex(), "area": area, "areaUnit": unit }));
    }
    if labels.is_empty() {
        return Err(bad("rarea", "select one or more closed polylines"));
    }
    Ok(json!({ "labels": labels }))
}

pub(crate) fn room_types() -> Vec<(&'static str, &'static str)> {
    ROOM_TYPES.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feet_and_inches_round_to_the_inch() {
        assert_eq!(feet_inches(150.0), "12'-6\"");
        assert_eq!(feet_inches(120.4), "10'-0\"");
    }

    #[test]
    fn room_codes_name_their_type() {
        assert_eq!(room_name("br"), "Bedroom");
        assert_eq!(room_name("KI"), "Kitchen");
        assert_eq!(room_name("Servant Room"), "Servant Room");
    }

    #[test]
    fn metric_room_label_reads_the_drawing_units() {
        let mut s = Session::new();
        s.execute("setvar", &json!({"name": "INSUNITS", "value": 4})).unwrap();
        let r = s.execute("room", &json!({"p1": [0, 0], "p2": [3400, 2500], "name": "br"})).unwrap();
        assert_eq!(r["name"], "BEDROOM");
        assert!((r["area"].as_f64().unwrap() - 8.5).abs() < 1e-9);
        assert_eq!(r["areaUnit"], "m2");
        let d = s.doc().unwrap();
        assert!(d.layer("ROOM-LABELS").is_some() && d.layer("ROOM-RECT").is_some());
        let texts: Vec<String> =
            d.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
        assert!(texts.contains(&"BEDROOM".to_string()), "{texts:?}");
        assert!(texts.contains(&"3.40 m x 2.50 m".to_string()), "{texts:?}");
        assert!(texts.contains(&"Area: 8.50 m2".to_string()), "{texts:?}");
    }

    #[test]
    fn room_in_feet_labels_square_feet_and_prefix() {
        let mut s = Session::new();
        s.execute("setvar", &json!({"name": "INSUNITS", "value": 2})).unwrap();
        s.execute("room", &json!({"p1": [0, 0], "p2": [12.5, 10], "name": "Kitchen", "floorPrefix": "GF", "rectangle": false})).unwrap();
        let d = s.doc().unwrap();
        let texts: Vec<String> =
            d.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
        assert!(texts.contains(&"GF KITCHEN".to_string()), "{texts:?}");
        assert!(texts.contains(&"12'-6\" x 10'-0\"".to_string()), "{texts:?}");
        assert!(texts.contains(&"Area: 125.00 sq ft".to_string()), "{texts:?}");
        assert!(!d.model.iter().any(|e| matches!(e.kind, EntityKind::LwPolyline(_))), "no rectangle");
    }

    #[test]
    fn room_command_line_asks_name_then_corners() {
        let mut s = Session::new();
        s.cmdline("room ki 0,0 4,3").unwrap();
        assert!(s.running.is_none(), "{}", s.prompt_text());
        assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "KITCHEN")));
    }

    #[test]
    fn hostile_room_params_are_errors() {
        let mut s = Session::new();
        for p in [
            json!({"p1": [0, 0], "p2": [0, 5], "name": "br"}),
            json!({"p1": [0, 0], "p2": [1, 1]}),
            json!({"p1": [0, 0], "p2": [1, 1], "name": "br", "textHeight": -1}),
            json!({"p1": [0, 0], "p2": [1e308, 1e308], "name": "br", "units": "furlongs"}),
        ] {
            assert!(s.execute("room", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn rarea_labels_a_closed_polyline() {
        let mut s = Session::new();
        s.execute("setvar", &json!({"name": "INSUNITS", "value": 6})).unwrap();
        let h = s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 2]})).unwrap();
        let r = s.execute("rarea", &json!({"handles": [h["handle"].clone()]})).unwrap();
        assert!((r["labels"][0]["area"].as_f64().unwrap() - 8.0).abs() < 1e-9);
        assert!(s.execute("rarea", &json!({"handles": []})).is_err());
    }

    #[test]
    fn layer_sets_are_created_once() {
        let mut s = Session::new();
        let a = s.execute("hcwlayers", &json!({})).unwrap();
        assert_eq!(a["created"], STANDARD_LAYERS.len());
        assert_eq!(a["set"], "standard");
        let b = s.execute("hcwlayers", &json!({"set": "both"})).unwrap();
        assert_eq!(b["created"], PERMIT_LAYERS.len());
        assert!(s.doc().unwrap().layer("A-WALL").is_some() && s.doc().unwrap().layer("BP-DOOR").is_some());
        let c = s.execute("hcwlayers", &json!({"set": "presentation"})).unwrap();
        assert_eq!(c["created"], PRESENTATION_LAYERS.len());
        assert!(s.doc().unwrap().layer("G-HEAVY").is_some());
        // Legacy aliases still resolve.
        assert_eq!(s.execute("hcwlayers", &json!({"set": "hcw"})).unwrap()["set"], "standard");
        assert_eq!(s.execute("hcwlayers", &json!({"set": "bp"})).unwrap()["set"], "permit");
        assert!(s.execute("hcwlayers", &json!({"set": "nope"})).is_err());
    }
}

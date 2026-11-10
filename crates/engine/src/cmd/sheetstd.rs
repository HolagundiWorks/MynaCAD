//! The standard mapping of the discipline sheet sets: which layers and which block names belong to
//! Working drawing, Electrical, Plumbing, Interior and False ceiling, the layers themselves (colour,
//! linetype, lineweight) and a small set of plan symbols drawn in code.
//!
//! The layer names follow the discipline-letter and element-code pattern that ISO 13567 and the
//! architectural layer set already in this program (`hcwlayers`) share: `E-POINT`, `P-FIXT`,
//! `I-FURN`, `FC-PANL`. The table is this program's own; it has not been checked against the text of
//! ISO 13567 (which is not freely available), so it is "in the style of" that standard, not a
//! certified implementation. `sheetset.standard` applies it; the mapping stays editable per sheet
//! set (`sheetset.set` with `layers` and `blocks`).

use std::sync::Arc;

use aad_doc::{Block, Common, Drawing, Entity, EntityKind};
use aad_geom::Vec3;
use serde_json::{Value, json};

use super::hcw::{STANDARD_LAYERS, ensure_layer};
use super::*;
use crate::{Result, Session};

/// One standard layer: name, ACI colour, linetype, lineweight (hundredths of mm), description.
type StdLayer = (&'static str, i16, &'static str, u16, &'static str);

/// A drawing primitive of a plan symbol, in millimetres about the symbol's centre.
#[derive(Clone, Copy)]
enum P {
    /// Line from (x0, y0) to (x1, y1).
    L(f64, f64, f64, f64),
    /// Circle at (x, y) of radius r.
    C(f64, f64, f64),
    /// Arc at (x, y) of radius r from the first angle to the second, in degrees.
    A(f64, f64, f64, f64, f64),
    /// Rectangle from (x0, y0) to (x1, y1).
    R(f64, f64, f64, f64),
}

use P::{A, C, L, R};

/// A symbol block: name, description, primitives.
type Symbol = (&'static str, &'static str, &'static [P]);

struct Tab {
    name: &'static str,
    prefix: &'static str,
    /// Layers created for the tab (besides the architectural set for Working drawing).
    layers: &'static [StdLayer],
    /// Layer name patterns the tab shows.
    layer_patterns: &'static [&'static str],
    /// Block name patterns the tab shows.
    block_patterns: &'static [&'static str],
    symbols: &'static [Symbol],
}

/// Structure drawn on the working drawing: the layers the RCC and steel smart blocks use.
const WORKING_LAYERS: &[StdLayer] =
    &[("S-RCC", 8, "Continuous", 35, "Reinforced concrete: columns, beams, slabs"), ("S-STEEL", 5, "Continuous", 25, "Steel sections")];

const ELECTRICAL_LAYERS: &[StdLayer] = &[
    ("E-CEIL", 6, "Continuous", 18, "Ceiling fittings - lights, fans, AC, sensors"),
    ("E-POINT", 2, "Continuous", 18, "Wall points - power, data, switches"),
    ("E-WIRE", 30, "Continuous", 15, "Wiring runs and conduit schematic"),
    ("E-BOARD", 1, "Continuous", 35, "Distribution boards and meters"),
    ("E-CIRC", 3, "Continuous", 13, "Circuit and point references"),
];

const PLUMBING_LAYERS: &[StdLayer] = &[
    ("P-FIXT", 6, "Continuous", 18, "Plumbing fixtures - basin, WC, shower, bath"),
    ("P-PIPE", 4, "Continuous", 18, "Pipes - supply, drain and vent"),
    ("P-WATR", 5, "Continuous", 18, "Water supply lines"),
    ("P-HOTW", 1, "Continuous", 18, "Hot water supply lines"),
    ("P-DRAN", 3, "DASHED", 18, "Drainage lines"),
    ("P-VENT", 8, "DASHED2", 13, "Vent pipes and stacks"),
    ("P-FITG", 4, "Continuous", 18, "Pipe fittings - elbows, tees, reducers"),
];

const INTERIOR_LAYERS: &[StdLayer] = &[
    ("I-FURN", 3, "Continuous", 20, "Loose furniture"),
    ("I-JOIN", 5, "Continuous", 20, "Fixed joinery - kitchen, wardrobes, built-ins"),
    ("I-FINISH", 30, "Continuous", 18, "Finish zones and material areas"),
    ("I-DECOR", 140, "Continuous", 15, "Decorative and soft furnishing elements"),
    ("I-FLOR", 9, "Continuous", 10, "Floor finish patterns"),
];

const CEILING_LAYERS: &[StdLayer] = &[
    ("FC-BOUN", 1, "Continuous", 35, "False ceiling boundary and drop line"),
    ("FC-PANL", 8, "Continuous", 13, "Ceiling panel grid"),
    ("FC-COVE", 4, "DASHED", 18, "Cove and recessed light lines"),
    ("FC-LITE", 2, "Continuous", 18, "Ceiling lights and diffusers"),
    ("FC-LEVL", 6, "Continuous", 13, "Ceiling level marks"),
    ("M-HVAC", 4, "Continuous", 18, "Ducts, diffusers and grilles"),
];

const ELECTRICAL_SYMBOLS: &[Symbol] = &[
    ("E-SW1", "One way switch (layer E-POINT)", &[C(0.0, 0.0, 50.0), L(0.0, 50.0, 0.0, 250.0), L(-45.0, 250.0, 45.0, 250.0)]),
    (
        "E-SW2",
        "Two way switch (layer E-POINT)",
        &[C(0.0, 0.0, 50.0), L(0.0, 50.0, 0.0, 250.0), L(-45.0, 250.0, 45.0, 250.0), L(-45.0, 200.0, 45.0, 200.0)],
    ),
    ("E-SOCK5", "5 amp socket (layer E-POINT)", &[C(0.0, 0.0, 90.0), L(-90.0, 0.0, 90.0, 0.0)]),
    ("E-SOCK15", "15 amp socket (layer E-POINT)", &[C(0.0, 0.0, 120.0), L(-120.0, 0.0, 120.0, 0.0), C(0.0, 0.0, 35.0)]),
    ("E-LIGHT", "Light point (layer E-CEIL)", &[C(0.0, 0.0, 150.0), L(-106.0, -106.0, 106.0, 106.0), L(-106.0, 106.0, 106.0, -106.0)]),
    (
        "E-FAN",
        "Ceiling fan point (layer E-CEIL)",
        &[
            C(0.0, 0.0, 300.0),
            C(0.0, 0.0, 40.0),
            L(40.0, 0.0, 300.0, 0.0),
            L(-40.0, 0.0, -300.0, 0.0),
            L(0.0, 40.0, 0.0, 300.0),
            L(0.0, -40.0, 0.0, -300.0),
        ],
    ),
    (
        "E-EXFAN",
        "Exhaust fan (layer E-CEIL)",
        &[C(0.0, 0.0, 150.0), C(0.0, 0.0, 25.0), L(25.0, 0.0, 150.0, 0.0), L(-12.0, 22.0, -75.0, 130.0), L(-12.0, -22.0, -75.0, -130.0)],
    ),
    (
        "E-DB",
        "Distribution board (layer E-BOARD)",
        &[R(-150.0, -200.0, 150.0, 200.0), L(-150.0, -200.0, 150.0, 200.0), L(-150.0, 200.0, 150.0, -200.0)],
    ),
    ("E-BELL", "Calling bell (layer E-POINT)", &[C(0.0, 0.0, 100.0), A(0.0, 0.0, 150.0, 30.0, 150.0), L(-100.0, -100.0, 100.0, -100.0)]),
    ("E-AC", "Air conditioner point (layer E-CEIL)", &[R(-300.0, -125.0, 300.0, 125.0), L(-300.0, 50.0, 300.0, 50.0), L(-300.0, 0.0, 300.0, 0.0)]),
    ("E-TV", "TV point (layer E-POINT)", &[R(-80.0, -50.0, 80.0, 50.0), L(-40.0, 50.0, 0.0, 110.0), L(40.0, 50.0, 0.0, 110.0)]),
];

const PLUMBING_SYMBOLS: &[Symbol] = &[
    (
        "P-WC",
        "Western water closet, plan (layer P-FIXT)",
        &[R(-250.0, 0.0, 250.0, 180.0), L(-200.0, 180.0, -200.0, 450.0), A(0.0, 450.0, 200.0, 180.0, 360.0), L(200.0, 450.0, 200.0, 180.0)],
    ),
    (
        "P-WCI",
        "Indian water closet pan, plan (layer P-FIXT)",
        &[R(-225.0, -300.0, 225.0, 300.0), C(0.0, 60.0, 110.0), R(-200.0, -250.0, -80.0, -50.0), R(80.0, -250.0, 200.0, -50.0)],
    ),
    ("P-BASIN", "Wash basin, plan (layer P-FIXT)", &[R(-275.0, -225.0, 275.0, 225.0), C(0.0, -10.0, 150.0), C(0.0, 190.0, 25.0)]),
    (
        "P-SINK",
        "Kitchen sink, plan (layer P-FIXT)",
        &[
            R(-500.0, -250.0, 500.0, 250.0),
            R(-460.0, -210.0, -40.0, 210.0),
            C(-250.0, 0.0, 30.0),
            L(0.0, -250.0, 0.0, 250.0),
            L(40.0, -150.0, 460.0, -150.0),
            L(40.0, -60.0, 460.0, -60.0),
        ],
    ),
    ("P-SHOWER", "Shower tray, plan (layer P-FIXT)", &[R(-450.0, -450.0, 450.0, 450.0), C(0.0, 0.0, 50.0), L(-450.0, 450.0, 450.0, -450.0)]),
    ("P-BATH", "Bath tub, plan (layer P-FIXT)", &[R(-850.0, -375.0, 850.0, 375.0), R(-780.0, -305.0, 780.0, 305.0), C(-700.0, 0.0, 30.0)]),
    ("P-TAP", "Tap or bib cock (layer P-FIXT)", &[C(0.0, 0.0, 50.0), L(-50.0, 0.0, 50.0, 0.0), L(0.0, -50.0, 0.0, 50.0)]),
    ("P-TRAP", "Floor trap (layer P-DRAN)", &[R(-75.0, -75.0, 75.0, 75.0), C(0.0, 0.0, 55.0), L(-55.0, 0.0, 55.0, 0.0), L(0.0, -55.0, 0.0, 55.0)]),
    ("P-GEYSER", "Water heater (layer P-FIXT)", &[C(0.0, 0.0, 200.0), C(0.0, 0.0, 140.0), L(-140.0, 0.0, 140.0, 0.0)]),
];

const INTERIOR_SYMBOLS: &[Symbol] = &[
    (
        "I-BED-D",
        "Double bed, plan (layer I-FURN)",
        &[R(-900.0, -1000.0, 900.0, 1000.0), R(-800.0, 560.0, -100.0, 940.0), R(100.0, 560.0, 800.0, 940.0), L(-900.0, 300.0, 900.0, 300.0)],
    ),
    (
        "I-SOFA3",
        "Three seat sofa, plan (layer I-FURN)",
        &[
            R(-1050.0, -450.0, 1050.0, 450.0),
            R(-1050.0, 250.0, 1050.0, 450.0),
            R(-1050.0, -450.0, -850.0, 250.0),
            R(850.0, -450.0, 1050.0, 250.0),
            L(-175.0, -450.0, -175.0, 250.0),
            L(175.0, -450.0, 175.0, 250.0),
        ],
    ),
    (
        "I-TABLE4",
        "Dining table with four chairs, plan (layer I-FURN)",
        &[
            R(-600.0, -400.0, 600.0, 400.0),
            R(-500.0, -850.0, -50.0, -450.0),
            R(50.0, -850.0, 500.0, -450.0),
            R(-500.0, 450.0, -50.0, 850.0),
            R(50.0, 450.0, 500.0, 850.0),
        ],
    ),
    ("I-CHAIR", "Chair, plan (layer I-FURN)", &[R(-225.0, -225.0, 225.0, 225.0), L(-225.0, 160.0, 225.0, 160.0)]),
    (
        "I-WARD",
        "Wardrobe, plan (layer I-JOIN)",
        &[R(-900.0, -300.0, 900.0, 300.0), L(0.0, -300.0, 0.0, 300.0), L(-900.0, -300.0, 0.0, 300.0), L(900.0, -300.0, 0.0, 300.0)],
    ),
    ("I-TVUNIT", "TV unit, plan (layer I-JOIN)", &[R(-750.0, -200.0, 750.0, 200.0), R(-400.0, -150.0, 400.0, 150.0)]),
];

const CEILING_SYMBOLS: &[Symbol] = &[
    ("FC-LIGHT", "Recessed downlight (layer FC-LITE)", &[C(0.0, 0.0, 75.0), L(-53.0, -53.0, 53.0, 53.0), L(-53.0, 53.0, 53.0, -53.0)]),
    ("FC-PANEL", "Ceiling panel 600 x 600 (layer FC-PANL)", &[R(-300.0, -300.0, 300.0, 300.0)]),
    (
        "FC-DIFF",
        "Air diffuser (layer FC-LITE)",
        &[R(-300.0, -300.0, 300.0, 300.0), R(-200.0, -200.0, 200.0, 200.0), L(-200.0, -200.0, 200.0, 200.0), L(-200.0, 200.0, 200.0, -200.0)],
    ),
];

const TABS: [Tab; 5] = [
    Tab {
        name: "Working drawing",
        prefix: "W-",
        layers: WORKING_LAYERS,
        layer_patterns: &["A-*", "AN-*", "W-*", "ROOM-LABELS", "MEASURE-*", "S-*"],
        block_patterns: &["AADOPEN-*", "DOOR*", "WIN*", "STAIR*", "NORTH*", "A-*", "SB_STRUCT_*", "SB_RCC_*"],
        symbols: &[],
    },
    Tab {
        name: "Electrical",
        prefix: "E-",
        layers: ELECTRICAL_LAYERS,
        layer_patterns: &["E-*"],
        block_patterns: &["E-*", "SW*", "SB*", "LP*", "FP*", "EF*", "SOCK*", "SB_ELEC_*"],
        symbols: ELECTRICAL_SYMBOLS,
    },
    Tab {
        name: "Plumbing",
        prefix: "P-",
        layers: PLUMBING_LAYERS,
        layer_patterns: &["P-*"],
        block_patterns: &["P-*", "WC*", "WB*", "SINK*", "SHOWER*", "TAP*", "SB_PLUMB_*"],
        symbols: PLUMBING_SYMBOLS,
    },
    Tab {
        name: "Interior",
        prefix: "I-",
        layers: INTERIOR_LAYERS,
        layer_patterns: &["I-*", "A-FLR"],
        block_patterns: &["FURN-*", "I-*", "SB_INT_*"],
        symbols: INTERIOR_SYMBOLS,
    },
    Tab {
        name: "False ceiling",
        prefix: "FC-",
        layers: CEILING_LAYERS,
        layer_patterns: &["FC-*", "A-CLNG", "E-CEIL", "M-*"],
        // The ceiling fittings of the electrical library are ceiling items too (layer E-CEIL).
        block_patterns: &["FC-*", "SB_HVAC_*", "SB_ELEC_LIGHT_*", "SB_ELEC_FAN_CEILING_*", "SB_ELEC_DOWNLIGHT_*"],
        symbols: CEILING_SYMBOLS,
    },
];

/// Whether `name` is one of the standard object layers (drawn geometry and symbols, as opposed to
/// text, dimensions and notes): the architectural set or a tab's own layers.
pub fn is_object_layer(name: &str) -> bool {
    let is = |n: &str| n.eq_ignore_ascii_case(name);
    TABS.iter().flat_map(|t| t.layers.iter()).any(|l| is(l.0)) || STANDARD_LAYERS.iter().any(|l| is(l.0) && !l.0.starts_with("AN-"))
}

/// A standard layer by name: (name, colour, linetype, lineweight, description).
fn std_layer(name: &str) -> Option<(&'static str, i16, &'static str, u16, &'static str)> {
    TABS.iter().flat_map(|t| t.layers.iter()).chain(STANDARD_LAYERS.iter()).find(|l| l.0.eq_ignore_ascii_case(name)).copied()
}

/// The standard layers a drawn layer can be mapped to: (layer, tab it belongs to, description).
pub(super) fn standard_targets() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out: Vec<(&str, &str, &str)> = Vec::new();
    for t in &TABS {
        let own = t.layers.iter().map(|l| (l.0, t.name, l.4));
        let arch = STANDARD_LAYERS
            .iter()
            .filter(|_| t.name == "Working drawing")
            .filter(|l| l.0.starts_with("A-") || l.0.starts_with("AN-"))
            .map(|l| (l.0, t.name, l.4));
        for e in own.chain(arch) {
            if !out.iter().any(|o| o.0 == e.0) {
                out.push(e);
            }
        }
    }
    out
}

/// The standard layer a layer of another drawing most likely stands for, by the words in its name
/// (`AR_WALLS` -> `A-WALL`). A layer that already has a standard name maps to itself; `0`, `Defpoints`
/// and anything not recognised have no suggestion (they are imported under their own names).
pub fn suggest_target(name: &str) -> Option<&'static str> {
    let up = name.trim().to_ascii_uppercase();
    if up.is_empty() || up == "0" || up == "DEFPOINTS" {
        return None;
    }
    if let Some(l) = std_layer(&up) {
        return Some(l.0);
    }
    // Quantity take-off layers stay as they are: the Working drawing tab already maps `MEASURE-*`.
    if up.starts_with("MEASURE") {
        return None;
    }
    let tokens: Vec<&str> = up.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    let has = |words: &[&str]| words.iter().any(|w| up.contains(w));
    // Short words only count as whole words (`DEDUCT` is not a duct, `FANCY` is not a fan).
    let word = |words: &[&str]| words.iter().any(|w| tokens.contains(w));
    let structural = up.starts_with("ST") || up.starts_with("S-") || up.starts_with("S_") || has(&["STRUCT", "RCC"]);
    let rules: [(bool, &str); 23] = [
        (structural && has(&["COLUMN", "BEAM", "SLAB", "FOOTING", "RCC"]), "S-RCC"),
        (structural && has(&["STEEL"]), "S-STEEL"),
        (has(&["WALL"]), "A-WALL"),
        (has(&["COLUMN"]), "A-COL"),
        (has(&["DOOR"]), "A-DOOR"),
        (has(&["WINDOW", "WIND", "GLAZ"]), "A-WIND"),
        (has(&["STAIR"]), "A-STAIR"),
        (has(&["RAIL"]), "A-RAIL"),
        (has(&["ROOF"]), "A-ROOF"),
        (has(&["CEIL", "CLNG"]), "A-CLNG"),
        (has(&["FURN"]), "I-FURN"),
        (has(&["JOIN", "CABINET"]), "I-JOIN"),
        (has(&["DECOR"]), "I-DECOR"),
        (has(&["FINISH"]), "I-FINISH"),
        (has(&["DIM"]), "AN-DIMS"),
        (has(&["TEXT", "NOTE", "ANNO"]) || word(&["TXT"]), "AN-TEXT"),
        (has(&["HATCH"]), "AN-HATCH"),
        (has(&["GRID", "CENTRE", "CENTER"]), "AN-GRID"),
        (has(&["HVAC"]) || word(&["DUCT", "DUCTS"]), "M-HVAC"),
        (has(&["LIGHT"]) || word(&["FAN", "FANS"]), "E-CEIL"),
        (has(&["ELEC", "SWITCH", "SOCKET", "POWER"]), "E-POINT"),
        (has(&["SANIT", "BASIN"]) || word(&["WC"]), "P-FIXT"),
        (has(&["PLUMB", "PIPE"]), "P-PIPE"),
    ];
    rules.iter().find(|r| r.0).map(|r| r.1)
}

/// Every layer the discipline tabs create (each name once), for the layer mapper's destinations.
pub(super) fn tab_layers() -> Vec<StdLayer> {
    let mut out: Vec<StdLayer> = Vec::new();
    for l in TABS.iter().flat_map(|t| t.layers.iter()) {
        if !out.iter().any(|o| o.0.eq_ignore_ascii_case(l.0)) {
            out.push(*l);
        }
    }
    out
}

/// The names and prefixes of the standard tabs, in order (the presets of `sheetset.new`).
pub(super) fn tab_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    TABS.iter().map(|t| (t.name, t.prefix))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("sheetset.standard", "Standard Sheet Sets", run_standard)
            .menu(&["Architecture", "Sheet Sets", "Apply Standard Layers and Symbols"])
            .params("{sets? (names; default all five), layers? (default true: create the standard layers), symbols? (default true: add the standard symbol blocks)} → {sets, layersCreated, blocksCreated}. Creates Working drawing, Electrical, Plumbing, Interior and False ceiling when missing, maps each to its layer and block name patterns (replacing a mapping edited before), creates the missing layers, and adds the symbol blocks that are not in the drawing yet. One undo step."),
        CommandSpec::new("layer.scope", "Layer Scope", run_scope)
            .menu(&["Format", "Layer Scope"])
            .params("{mode?: \"tab\" | \"level\" | \"all\"} → {mode}. While a sheet set is the active tab the Layers panel shows only its layers (`tab`, the default), only the layers used on the active level (`level`), or every layer (`all`); the choice is kept in the drawing. Without `mode` it only reports."),
    ]
}

/// What `apply_standard` did.
#[derive(Default)]
pub struct Report {
    pub sets: Vec<String>,
    pub layers_created: usize,
    pub blocks_created: usize,
}

fn mm_to_units(d: &Drawing) -> f64 {
    let metres = crate::annot::drawing_unit(d).map_or(0.001, |u| u.metres());
    if metres.is_finite() && metres > 0.0 { 0.001 / metres } else { 1.0 }
}

fn entity(d: &mut Drawing, kind: EntityKind) -> Entity {
    // Layer 0 and ByBlock: a symbol takes the layer and colour of the insert that places it.
    Entity { handle: d.new_handle(), common: Common::default(), kind }
}

fn symbol_block(d: &mut Drawing, name: &str, description: &str, prims: &[P]) -> Block {
    let k = mm_to_units(d);
    let v = |x: f64, y: f64| Vec3::new(x * k, y * k, 0.0);
    let mut b = Block::new(name);
    b.description = description.to_string();
    for p in prims {
        let kinds: Vec<EntityKind> = match *p {
            L(x0, y0, x1, y1) => vec![EntityKind::Line(aad_doc::Line { a: v(x0, y0), b: v(x1, y1) })],
            C(x, y, r) => vec![EntityKind::Circle(aad_doc::Circle { center: v(x, y), radius: r * k })],
            A(x, y, r, a0, a1) => {
                vec![EntityKind::Arc(aad_doc::Arc { center: v(x, y), radius: r * k, start: a0.to_radians(), end: a1.to_radians() })]
            }
            R(x0, y0, x1, y1) => [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)]
                .iter()
                .map(|&(ax, ay, bx, by)| EntityKind::Line(aad_doc::Line { a: v(ax, ay), b: v(bx, by) }))
                .collect(),
        };
        for kind in kinds {
            let e = entity(d, kind);
            b.entities.push(e);
        }
    }
    b
}

/// Create the standard tabs (or the named ones), map them, and make their layers and symbols.
pub fn apply_standard(d: &mut Drawing, only: Option<&[String]>, layers: bool, symbols: bool) -> Result<Report> {
    let mut report = Report::default();
    for tab in &TABS {
        if only.is_some_and(|names| !names.iter().any(|n| n.trim().eq_ignore_ascii_case(tab.name))) {
            continue;
        }
        if d.sheet_set_by_name(tab.name).is_none() {
            d.add_sheet_set(tab.name, tab.prefix)?;
        }
        if let Some(set) = d.sheet_sets.iter_mut().find(|s| s.name.eq_ignore_ascii_case(tab.name)) {
            set.layers = tab.layer_patterns.iter().map(|p| (*p).to_string()).collect();
            set.blocks = tab.block_patterns.iter().map(|p| (*p).to_string()).collect();
        }
        report.sets.push(tab.name.to_string());
        if layers {
            if tab.name == "Working drawing" {
                for (n, c, lt, lw, desc) in STANDARD_LAYERS.iter().filter(|l| l.0.starts_with("A-") || l.0.starts_with("AN-")) {
                    report.layers_created += usize::from(ensure_layer(d, n, *c, lt, *lw, desc));
                }
            }
            for (n, c, lt, lw, desc) in tab.layers {
                report.layers_created += usize::from(ensure_layer(d, n, *c, lt, *lw, desc));
            }
        }
        if symbols {
            for (name, description, prims) in tab.symbols {
                if d.block(name).is_none() {
                    let b = symbol_block(d, name, description, prims);
                    d.blocks.insert((*name).to_string(), Arc::new(b));
                    report.blocks_created += 1;
                }
            }
        }
    }
    Ok(report)
}

fn run_standard(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "sheetset.standard";
    if s.block_edit.is_some() {
        return Err(bad(CMD, "close the block editor first"));
    }
    let only: Option<Vec<String>> = match p.get("sets") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) => Some(t.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()),
        Some(Value::Array(a)) if a.len() <= 16 => Some(a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()),
        Some(_) => return Err(bad(CMD, "`sets` is a list of sheet set names")),
    };
    if let Some(names) = &only {
        let unknown: Vec<&String> = names.iter().filter(|n| !TABS.iter().any(|t| t.name.eq_ignore_ascii_case(n.trim()))).collect();
        if !unknown.is_empty() {
            let known = TABS.iter().map(|t| t.name).collect::<Vec<_>>().join(", ");
            return Err(bad(CMD, format!("no standard sheet set {unknown:?}; the standard ones are {known}")));
        }
    }
    let (layers, symbols) = (bool_or(p, "layers", true), bool_or(p, "symbols", true));
    // Work on a copy so a refused change (the sheet set cap) leaves the drawing as it was.
    let mut d = s.doc()?.clone();
    let r = apply_standard(&mut d, only.as_deref(), layers, symbols)?;
    *s.doc_mut()? = d;
    Ok(json!({ "sets": r.sets, "layersCreated": r.layers_created, "blocksCreated": r.blocks_created }))
}

/// The project key that keeps the Layers panel scope (project data is saved with the file).
const SCOPE_KEY: &str = "layerScope";

/// Whether the Layers and Blocks panels show every layer and block even in a sheet view.
pub fn show_all(d: &Drawing) -> bool {
    super::project::get(d, SCOPE_KEY).as_str() == Some("all")
}

/// Whether the Layers panel lists only the layers used on the active level.
pub fn show_level_only(d: &Drawing) -> bool {
    super::project::get(d, SCOPE_KEY).as_str() == Some("level")
}

fn run_scope(s: &mut Session, p: &Value) -> Result<Value> {
    match p.get("mode") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let mode = match v.as_str().map(str::to_ascii_lowercase).as_deref() {
                Some(m @ ("all" | "tab" | "level")) => m.to_string(),
                _ => return Err(bad("layer.scope", "`mode` is `tab`, `level` or `all`")),
            };
            let current = if show_all(s.doc()?) {
                "all"
            } else if show_level_only(s.doc()?) {
                "level"
            } else {
                "tab"
            };
            if current != mode {
                super::project::set(s.doc_mut()?, SCOPE_KEY, &json!(mode));
            }
        }
    }
    let d = s.doc()?;
    Ok(json!({ "mode": if show_all(d) { "all" } else if show_level_only(d) { "level" } else { "tab" } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_standard_symbol_is_named_inside_its_tabs_block_patterns() {
        for tab in &TABS {
            let set = aad_doc::SheetSet {
                id: aad_doc::SheetSetId(1),
                name: tab.name.into(),
                layer_prefix: tab.prefix.into(),
                space: None,
                dim_style: None,
                text_style: None,
                layers: tab.layer_patterns.iter().map(|p| (*p).into()).collect(),
                blocks: tab.block_patterns.iter().map(|p| (*p).into()).collect(),
                annotations: Default::default(),
            };
            for (name, _, prims) in tab.symbols {
                assert!(set.maps_block(name), "{} does not map its own symbol {name}", tab.name);
                assert!(!prims.is_empty(), "{name} draws nothing");
            }
            for (name, ..) in tab.layers {
                assert!(set.maps_layer(name), "{} does not map its own layer {name}", tab.name);
            }
        }
    }

    #[test]
    fn the_presets_and_the_standard_table_agree() {
        let table: Vec<_> = tab_names().collect();
        assert_eq!(table, super::super::sheetset::PRESETS.to_vec());
    }
}

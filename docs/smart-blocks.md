# Smart blocks and discipline block libraries

O-Draft has two block libraries for services and structure:

| Library | Where | What it is |
|---|---|---|
| Smart blocks | `assets/smartblocks/*.json`, built into the engine | O-Draft's own parametric definitions: sizes, counts and options are parameters |
| Discipline DXF blocks | `assets/blocks/<discipline>/` | 79 static CC0 symbols and details (plumbing, HVAC, electrical, structural / RCC, interior) |

Smart blocks use O-Draft's parametric format (`osas-smartblocks` v1): one definition, many sizes
and variants, with an open, documented format and engine commands. DWG and DXF files may carry
dynamic-block metadata from other programs; O-Draft opens that geometry as static blocks only.

## Commands

All are engine commands (`crates/engine/src/cmd/smartblock.rs`), so the command line, scripts,
the control channel, MCP and the agent reach the same code. Programmatic calls never open dialogs.

| Command | Undo | Parameters → result |
|---|---|---|
| `smartblock.list` | no | `{discipline?, sheetSet?}` → `{blocks: [{id, title, discipline, description, layer, params: [{name, label, unit, default, min, max, integer, choices}], stored?}]}` |
| `smartblock.insert` | yes | `{id \| definition, at: [x,y], params?, rotation?, scale?, layer?, units?}` → `{handle, block, id, params}` |
| `smartblock.get` | no | `{handle?}` (else the first selected smart block) → `{handle, id, title, block, params, spec}` |
| `smartblock.edit` | yes | `{handle? \| handles? (else the selection), params}` → `{edited, blocks}` |

- `params` is an object (`{"width": 300, "bar": 20}`) or text (`"width=300, bar=20"`). A choice
  parameter takes its value or its label (`{"kind": "RWP"}`). A value outside the range, a
  fraction for a whole-number parameter, or an unknown name is refused and nothing changes.
- Geometry is in millimetres. The reference is scaled to the drawing units (`INSUNITS`, or the
  `units` parameter): scale 1 in a millimetre drawing, 0.001 in a metre drawing, times `scale`.
- The reference goes on the definition's layer (created with its colour and lineweight when
  missing) unless `layer` is given. Block contents are on layer 0, so they follow the reference.
- Menus: parametric and ordinary blocks share one place. Insert > Block... opens the Blocks window (below);
  there is no separate Smart Block menu. Modify > Object > Block Parameters... asks
  for the parameters (text) and runs `smartblock.edit` on the selection. Aliases: `sbinsert`, `smartblock`, `sblist`, `sbedit`.
- `properties.set` takes smart block parameters as `sb:<name>` keys (`{"handles": [...], "sb:width": 300,
  "sb:bar": "20"}`) and edits them like `smartblock.edit`.

### Editing a placed smart block

- **Properties panel.** With one smart block reference selected, the Properties panel (WinUI) and
  Quick Properties (egui) list its parameters after Name, Rotation and Scale: a number field, or a
  list for a parameter with choices. A change runs `properties.set` with the `sb:<name>` key. The
  rows come from `aad_engine::smart_block_fields(doc, insert)`.
- **Grips.** A definition can declare parameter grips (see `grips` below). They follow the
  reference's own grip (the insertion point) in `Session::grips_of` and `aad_engine::grips::grips`,
  which both front ends draw. Dragging one (`grip.move` with that grip index) changes its parameter
  by the drag along the grip's axis in block millimetres times `per`, rounded to `step` (default 1)
  and kept within the parameter's range; the reference is pointed at the block for the new value,
  as `smartblock.edit` does. Rotation, scale and drawing units are taken into account. 34 of the 39
  built-in blocks have grips (for example width and depth of a column, the length of a duct, the
  sink and hob positions of a kitchen counter).

### Structural tools and IS 456:2000 checks

The reinforced concrete blocks are built-in tools: the Architecture bar has **RCC column**, **Round
column**, **RCC beam**, **Footing**, **Slab** and **Lintel** (also Architecture > RCC Structure, and
the engine commands `rcc.column`, `rcc.column.round`, `rcc.beam`, `rcc.footing`, `rcc.slab`,
`rcc.lintel`, which are `smartblock.insert` for that block with `at`). In the Windows app a tool
picks the insertion point on the drawing and places the block with its defaults; the reinforcement
is then edited in the Properties panel, as for any parametric block.

The parameters follow the code: concrete grade (M20 to M40), steel grade (Fe250 to Fe550), exposure
(IS 456 Table 3: mild to extreme), clear cover, bar and tie or stirrup diameters, numbers of bars,
and tie, stirrup and distribution bar spacing (c/c). Below the parameters, Properties lists the
**IS 456:2000** result: "Complies" or "N to review", the quantities the code limits, and one row
per broken limit with its clause. `smartblock.check` (`is456`, Architecture > RCC Structure >
Check to IS 456) returns the same as data for a placed block (`handle`, else the selection) or for
a definition with `params` before it is placed. A check never refuses a value; the blocks stay
drafting aids and the structural design is the engineer's.

| Block | Checked (clause) |
|---|---|
| Rectangular and circular column | steel 0.8 % to 6 % of the section, at least 4 bars (6 round), bars of 12 mm or more (26.5.3.1); cover 40 mm or the bar diameter, and Table 16 for the exposure (26.4.2); ties at least a quarter of the largest bar and 6 mm, at most the least side, 16 bar diameters and 300 mm apart (26.5.3.2); clear spacing of bars at least the bar diameter and 25 mm (26.3.2), at most 300 mm c/c (26.5.3.1(h)) |
| Beam section | tension steel at least 0.85 b d / fy and at most 4 % of the section, same for the top bars (26.5.1.1); clear spacing and fit of bars (26.3.2); cover (Table 16, bar diameter); stirrups at most 0.75 d and 300 mm (26.5.1.5), and the minimum shear steel 0.4 / (0.87 fy) (26.5.1.6); side-face steel for depths over 750 mm (26.5.1.3) |
| Slab section | minimum steel 0.12 % (Fe415 and above) or 0.15 % (Fe250) of the section, main and distribution (26.5.2.1); main bars at most 3 d and 300 mm, distribution at most 5 d and 450 mm (26.3.3); bar at most an eighth of the thickness (26.5.2.2); cover |
| Footing section | cover 50 mm and Table 16 (26.4.2.2); thickness at the edge at least 150 mm on soil (34.1.2); minimum mesh steel and spacing as for a slab |
| Lintel | cover (Table 16, bar diameter) |

Cover by exposure (Table 16): mild 20 mm, moderate 30, severe 45, very severe 50, extreme 75. The
defaults of every block comply at the default exposure (mild). The plan of a footing carries no
reinforcement and is not checked. The rules are in `crates/engine/src/is456.rs`, each with a test.

### Detailing style

The RCC blocks are drawn as the structural drawings in use draw them: bars coloured by diameter, ties with 135 degree hooks and cross-ties, dimensions
in feet and inches (or millimetres with `ftin` = 0), leaders with the bar notation of a schedule (`4Y20 + 6Y16`, `Ties-Y8@6"`, `Y10@5"`), PCC and boulder
soling under a footing with the development length Ld of the column bars (IS 456 cl. 26.2.1 and Table 5), the pedestal offset and the ground level.
`rcc.beam.elevation` draws a beam along its span with continuous and extra bars, their curtailment lengths, and the stirrup zones (`5 nos @ 6"`, `@ 7"`);
`rcc.stirrup.detail` the stirrup and its hook; `rcc.footing.combined` and `rcc.raft.lift` the plans of a combined footing and a lift raft. A column can have
corner bars of one diameter and the others of another (`bar`, `bar2`). The switches `label` (leaders and notes), `dims` and `ftin` are parameters of each.

### In O-CoreOp

O-CoreOp draws the RCC blocks too, read only. When O-Draft saves a drawing into a project file (`.osp`) it records every RCC block in the model, with the
parameters it was placed with, in the file's `detail` table (same revision as the drawing; see [PROJECT-FILE.md](PROJECT-FILE.md)). The Beams, Columns, Slabs,
Footings and Lintels pages of O-CoreOp list those details, draw the chosen one, and say so when there is none. O-CoreOp's own reader of the format
(`Office.Shared.SmartBlocks`, a copy of `rcc.json`) is checked against this engine: `crates/engine/tests/rcc_parity.rs` pins the parts each block makes
(`tests/fixtures/rcc_counts.json`), and O-CoreOp's tests hold a copy of that file. After a block definition changes, run that test with
`UPDATE_FIXTURES=1` and copy the file (and the new `rcc.json`) to O-CoreOp.

### Windows app

- Blocks panel: one **Block library** with a **Blocks...** button and a collapsed group per discipline
  (RCC, structural, plumbing, HVAC, electrical, interior). Each group holds the parametric blocks and the
  ready-made DXF symbols; thumbnails are made when a group is first opened. Clicking a parametric
  thumbnail places it with its default parameters; a symbol starts `insert.place` with that file, as for the furniture set.
- Blocks window (Insert > Block..., or the button): one window of thumbnails, grouped, for this drawing's
  blocks and the parametric library; filter by drawing or discipline. Click a block and pick its insertion
  point on the drawing. A parametric block goes in with its default parameters and is left selected; its
  parameters are entered in the Properties panel (or by its grips), not in the window.

Examples (control channel or `aad-cli run --cmd`):

```
smartblock.insert {"id": "rcc.column.rect", "at": [0, 0], "params": {"width": 300, "depth": 600, "ny": 4, "bar": 20}}
smartblock.edit   {"handle": "1A2", "params": "nx=3, tie=10"}
smartblock.insert {"id": "int.dining.set", "at": [5000, 0], "params": "side=3, ends=1"}
```

### How a reference stays editable

Each distinct parameter set becomes an ordinary block named `SB_<ID>_<hash>` (for example
`SB_RCC_COLUMN_RECT_1F2E3D4C`). Inserting the same parameters again reuses that block. The block's
description holds `{"osas-smartblock": 1, "id": ..., "params": {...}}`; `smartblock.get` and
`smartblock.edit` read it, so a reference stays editable after a DXF save and reload. Other
programs see a plain block. `smartblock.edit` points the reference at the block for its new
parameters; position, rotation, scale, layer and other references are unchanged. Blocks no longer
used can be removed with `purge`.

An inline `definition` is checked like a built-in one and stored in the drawing's project data
(`smartBlockDefs`, at most 500), so references made from it stay editable. A stored definition with
the id of a built-in one takes its place in that drawing.

## Built-in library

| Discipline | Ids |
|---|---|
| RCC (`S-RCC`) | `rcc.column.rect`, `rcc.column.circular`, `rcc.beam.section`, `rcc.beam.elevation`, `rcc.stirrup.detail`, `rcc.footing.plan`, `rcc.footing.section`, `rcc.footing.combined`, `rcc.raft.lift`, `rcc.lintel.section`, `rcc.slab.section` |
| Structural steel (`S-STEEL`) | `struct.ibeam`, `struct.channel`, `struct.angle`, `struct.hollow.rect`, `struct.baseplate` |
| Plumbing (`P-FIXT`, `P-PIPE`) | `plumb.wc.european`, `plumb.wc.indian`, `plumb.basin`, `plumb.sink.kitchen`, `plumb.shower`, `plumb.floor.trap`, `plumb.chamber`, `plumb.tank.round`, `plumb.riser` |
| HVAC (`M-HVAC`) | `hvac.duct.rect`, `hvac.diffuser.square`, `hvac.grille`, `hvac.split.indoor`, `hvac.split.outdoor`, `hvac.fan.exhaust` |
| Electrical (`E-POINT`, `E-CEIL`) | `elec.switchboard`, `elec.socket`, `elec.light`, `elec.fan.ceiling`, `elec.db`, `elec.downlight` |
| Interior (`I-JOIN`, `I-FURN`) | `int.wardrobe`, `int.kitchen.counter`, `int.dining.set`, `int.bed`, `int.sofa`, `int.shelf` |

`smartblock.list` gives each definition's parameters. Examples of what the parameters do: the
rectangular column takes width, depth, cover, bars per face, bar and tie diameter and draws the
bars and tie; the beam section adds top and bottom bar counts, stirrups and an optional slab; the
kitchen sink takes 1 to 3 bowls and an optional drainer; the dining set takes chairs per side and
end chairs; the switch board takes switch and socket counts. Steel sections are drawn from the
dimensions given; no section tables (ISMB, IPE, ...) are built in. Electrical symbols are generic
shapes drawn for O-Draft, not artwork copied from a standard.

These are drafting aids. Reinforcement, sizes and clearances come from the design.

## Definition format (`osas-smartblocks` version 1)

A library file is `{format: "osas-smartblocks", version: 1, discipline, note?, blocks: [Def]}`.
Unknown keys are refused.

```json
{
  "id": "my.tag", "title": "Room tag", "discipline": "interior",
  "layer": "A-TAG", "color": 7, "lineweight": 18,
  "params": [
    { "name": "size", "label": "Size", "unit": "mm", "default": 300, "min": 100, "max": 900 },
    { "name": "boxed", "default": 1, "choices": [ { "label": "Yes", "value": 1 }, { "label": "No", "value": 0 } ] }
  ],
  "vars": [ { "name": "half", "expr": "size/2" } ],
  "items": [
    { "when": "boxed", "crect": [0, 0, "size", "size"] },
    { "circle": [0, 0, "half*0.8"] },
    { "repeat": { "var": "i", "count": 3, "items": [ { "line": ["-half", "(i - 1)*size/4", "half", "(i - 1)*size/4"] } ] } },
    { "text": { "at": [0, "-half - 80"], "height": 60, "value": "{size}", "align": "c" } }
  ]
}
```

- **Parameters**: `name` (a formula name: letters, digits, `_`; not a unit or function name such
  as `mm`, `h`, `round`), `default`, optional `label`, `unit`, `min`, `max`, `integer`, `choices`
  (label and value). Defaults must satisfy the range.
- **vars**: derived values, evaluated in order after the parameters.
- **grips** (optional, at most 16): `{param, at: [x, y], axis: "x" | "y", per?, step?}`. `param`
  names a parameter that is not a list of choices; `at` is where the grip is drawn (formulas);
  dragging it by `d` mm along `axis` adds `per * d` to the parameter (`per` is a number or formula,
  default 1; use 2 for a size measured about the centre), rounded to `step` (default 1).
- **Numbers**: every coordinate, size and count is a number or a formula in the `aad-param`
  formula language (`+ - * /`, `^` with a whole exponent, parentheses, `min`, `max`, `abs`,
  `sqrt`, `round`, `floor`, `ceil`). There are no trigonometric functions; use `ring` for points
  on a circle or write the constants.
- **Items** (all optional fields: `when` formula, drawn only when greater than zero; `layer`;
  `linetype` from the standard library):

| Item | Value |
|---|---|
| `line` | `[x1, y1, x2, y2]` |
| `rect` | `[x, y, width, height]` from a corner |
| `crect` | `[cx, cy, width, height]` about a centre |
| `poly` | `{pts: [[x, y], ...], closed?}` |
| `circle` | `[cx, cy, r]` (skipped when r ≤ 0) |
| `dot` | `[cx, cy, r]` filled disc (a bar in section) |
| `arc` | `[cx, cy, r, start°, end°]` counter-clockwise |
| `text` | `{at, height, value, align?: "l" \| "c", rotation?}`; `{name}` in `value` is replaced by a parameter's value (a choice's label) or a derived value |
| `hatch` | `{pts, pattern, scale?}` with a standard pattern name or `SOLID` |
| `ring` | `{c, r, count, dot}` dots evenly on a circle |
| `dim` | `{from: [x, y], to: [x, y], off, height, text?}` a dimension along x or y (the longer way between the points): dimension line `off` away (above or to the right when positive), extension lines, ticks and the length as text, orange. Without `text` the length is written in feet and inches (`1'-6"`, to the half inch) when the block's `ftin` parameter is above zero, else in millimetres |
| `leader` | `{from, to, height, text}` a line from `from` to `to`, a short landing and the text after it (before it when it runs left), orange |

Any item can take `"color": n` (an AutoCAD colour index) to be drawn in that colour instead of the reference's; the RCC blocks use red for main bars,
magenta and blue for bars of other diameters, green and cyan for ties and cross-ties. In text, `{name:len}` writes a derived value or parameter as a
length in the unit chosen by `ftin`.
| `repeat` | `{var, count, items}`; `var` counts from 0 |

Limits (larger input is refused, never truncated): 64 KB per inline definition, 48 parameters,
64 derived values, 16 grips, 2000 items, repeats nested 4 deep and at most 1000 times, 5000 objects per
block, coordinates within 1e9 mm, text 200 characters.

## Discipline DXF blocks

`assets/blocks/{plumbing,hvac,electrical,structural,interior}/` hold 79 DXF files with a
`catalog.json` each (same shape as the furniture catalog). They are original Innerscene models
released under CC0 1.0; each item's licence was read from its page on 2026-10-27 and is recorded in
`ATTRIBUTION.md`. Insert one with `insert {name, file: "<path>", at}`. The Windows package and the
WinUI build copy them to `blocks\` beside the program, and the WinUI Blocks panel lists them
(see Windows app above). The egui window does not list them.

### Sources reviewed and not bundled

These were checked for this library and are not redistributable (or state no licence), so nothing
from them is in the repository. Users can download from them for their own drawings under each
site's terms.

| Source | Terms (as published, 2026-10-27) |
|---|---|
| [Draftsperson blocks](https://blocks.draftsperson.net/license/) | Free to use in projects; no re-hosting or redistribution of the files |
| [CAD Forum block catalog](https://www.cadforum.cz/catalog_en/) | Community uploads (some dynamic blocks); no licence stated per file |
| [CADProfi / CP-Symbols](https://novedge.com/collections/cadprofi) | Commercial |
| Manufacturer block downloads (sanitary ware, HVAC units, luminaires) | Each manufacturer's terms; usually use only, no redistribution |

No openly licensed parametric block catalog was found for these disciplines at the time the
library was built, which is why the parametric library is O-Draft's own.

## Performance

- Every formula is parsed once, when a definition is read; evaluation only walks the parsed
  expression, including inside repeats.
- The built-in library is read and checked once per process and shared (`Arc`), never copied per
  command. `smartblock.edit` looks each definition up once however many references it edits.
- Block names of inline definitions hash the definition's canonical JSON.
- A block is made once per distinct parameter set; references with the same parameters share it.

## Tests

- `crates/engine/src/cmd/smartblock.rs` (unit): the library parses, its ids match the form list,
  every definition draws at its defaults and at each range end and choice, text templates.
- `crates/engine/tests/smartblock.rs`: insert and reuse, edit and undo, choice labels, drawing-unit
  scale, every library block inserts, DXF round trip (built-in and inline), hostile definitions and
  values are refused without a panic and without changing the drawing, the insert and edit
  forms, every catalogued discipline DXF reads, every parameter grip lies on its block and dragging
  it outwards grows its parameter, a grip drag under rotation and metre units (range clamp, undo,
  bad index), the Properties rows and `properties.set` with `sb:` keys.
- `cargo xtask assets` checks every file under `assets/blocks` and `assets/smartblocks` has an
  `ATTRIBUTION.md` row.

## Not done

- The WinUI changes (Smart Blocks window, the Blocks panel library thumbnails) build in the shell
  (`dotnet build`) but were not clicked through; the Rust side they call is tested.
- Grips for count parameters (bars, chairs, shutters) and for the switch board; these are edited
  in Properties.
- Discipline DXFs and a smart block picker in the egui window.
- Import of DWG/DXF dynamic-block metadata (parameters, actions, visibility).
- Take-off rows from smart block parameters (for example column or footing concrete).

## Tabs

`smartblock.list {sheetSet: true | "<name>"}` lists only the blocks whose layer the active (or named) sheet set maps. The standard tabs (`sheetset.standard`) map them by layer: Electrical gets `elec.*`, Plumbing `plumb.*`, Interior `int.*`, Working drawing `struct.*` and `rcc.*`, False ceiling `hvac.*` plus the three ceiling electrical fittings. The host's panel snapshot carries the same list as `library`. Verified by `smart_blocks_belong_to_the_tab_whose_layers_they_use` and the host test `the_blocks_panel_lists_the_smart_blocks_of_the_active_tab`; the WinUI Blocks panel lists it under Smart blocks (a click opens the `smartblock.insert` form with that block selected; compiled, not clicked through).

## Drawing units

Block geometry is in millimetres and is scaled to the drawing's unit (`INSUNITS`; `units` overrides). A header that says inches or feet on a plan that would then be longer than 500 m is taken to be an untouched template default, and the unit that makes the plan 3-500 m long (millimetres, then centimetres, then metres) is used instead; a stated metric unit is always trusted. This also applies to the other commands that read the drawing unit (rooms, permit layers, areas). Verified by `a_metric_plan_flagged_as_inches_still_gets_blocks_at_millimetre_scale` and on a copy of the OSAS demo drawing (header inches, 57 m wide plan in mm: scale 1.0 instead of 0.039). A drawing whose size is ambiguous (a small plan flagged as inches) keeps the header's unit.

The Blocks panel shows the library as two-column thumbnails (`blocks.preview {smartblock: id}` draws the block with its default parameters without touching the drawing; every built-in block has one, tested). The thumbnails were built into the shell but not looked at on screen: the desktop was locked during the check.

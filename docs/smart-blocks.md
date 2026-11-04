# Smart blocks and discipline block libraries

O-Draft has two block libraries for services and structure:

| Library | Where | What it is |
|---|---|---|
| Smart blocks | `assets/smartblocks/*.json`, built into the engine | O-Draft's own parametric definitions: sizes, counts and options are parameters |
| Discipline DXF blocks | `assets/blocks/<discipline>/` | 79 static CC0 symbols and details (plumbing, HVAC, electrical, structural / RCC, interior) |

Smart blocks use O-Draft's parametric format (`osas-smartblocks` v1): one definition, many sizes
and variants, with an open, documented format and engine commands.

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
- Menus: Insert > Smart Block > Insert... opens a form (block, parameters as text, rotation, then
  the insertion point); Insert > Smart Block > List; Modify > Object > Smart Block... asks
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

### Windows app

- Blocks panel: a **Smart blocks...** button opens the Smart Blocks window, and a **Discipline
  blocks** section lists the five DXF sets below as collapsed groups; thumbnails are made when a
  group is first opened. Clicking a thumbnail starts `insert.place` with that file, as for the
  furniture set.
- Smart Blocks window (Insert > Smart Block > Insert..., or the button): filter by discipline,
  pick a block, fill in its parameters (lists for choices, numbers with the range as a tooltip) and
  a rotation; **Insert** starts the `smartblock.insert` form with these values and the insertion
  point is picked on the drawing.

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
| RCC (`S-RCC`) | `rcc.column.rect`, `rcc.column.circular`, `rcc.beam.section`, `rcc.footing.plan`, `rcc.footing.section`, `rcc.footing.combined`, `rcc.raft.lift`, `rcc.beam.elevation`, `rcc.lintel.section`, `rcc.slab.section` |
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

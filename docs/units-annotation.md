# Units, plot scale and annotation layout

Implemented in `crates/engine/src/annot.rs` (pure functions, no UI) and `crates/engine/src/cmd/annot.rs`
(commands). The status bar popup is `crates/ui-egui/src/units_panel.rs`; the AI tools are in
`crates/agent/src/registry.rs`. Tests: `crates/engine/tests/annotation_layout.rs`,
`flatten_import.rs`, `crates/agent/src/tests.rs`, `units_panel::tests`.

## Units

| Command | Input | Effect |
|---|---|---|
| `drawing.units` | `{units: mm\|cm\|m\|in\|ft, mode: "metadata"\|"scale"}` | Sets `INSUNITS`. **`mode` is required.** `metadata` changes only the setting and returns a warning that no geometry was scaled. `scale` multiplies every model-space object by the unit ratio (using the `scale` command, one undo step) and returns `factor` and `scaled`. Refused when the current unit is unknown (nothing to convert from). |
| `display.units` | `{units: mm\|cm\|m\|in\|ft\|ftin}` | Sets `AADDISPUNITS` and, in every dimension style, the linear factor (`DIMLFAC`), decimals, unit format and suffix. Changes dimension text only; no object changes. |

- The drawing unit is read from `INSUNITS` (`annot::drawing_unit`). When it is unset or not a
  length unit Tren knows, commands that need it fail with a message and a file import says so on the
  command line (`import_drawing`). Nothing is guessed.
- Feet-and-inches uses the architectural format with the drawing measured in inches.
- Typed distances accept multi-unit suffixes: `'` / `"` (feet and inches), `mm`, `cm`, `m`. Bare
  numbers stay in drawing units. With `INSUNITS` set, suffixes convert into that unit (`5m` → 5000
  in a millimetre drawing); without it, feet/inches stay in inches and metric suffixes stay in the
  unit written (`5m` → 5). Architecture form fields (dialogs, prompts and JSON calls) take the same suffixes
  ([input-forms.md](input-forms.md)).
- Display units apply to dimension styles. Per-dimension overrides (xdata) are not rewritten, and
  the cursor readout still follows the drawing's `LUNITS` and `INSUNITS`.

## Plot scale and annotation size

`plot.config {scale: "1:100" | 100, paper: A0..A4 | [w, h] mm, textMm?: 2.5, arrowMm?: 2.5, apply?: true}`.

Annotation size in drawing units = printed millimetres × `scale` × 0.001 ÷ (metres per drawing unit).
2.5 mm at 1:100 is 250 in a millimetre drawing and 0.25 in a metre drawing, whatever the zoom: the
size comes from paper and scale, never from screen pixels. With `apply` (default) every dimension
style gets text height, arrow size, extension offset and extension, text gap and baseline spacing
from the table below, `DIMSCALE` is set to 1, `TEXTSIZE` is set, and `AADPLOTSCALE`, `AADPAPERW`,
`AADPAPERH` are stored in the drawing header. Geometry and dimension values are not touched.

| Quantity | Printed size (mm) |
|---|---|
| Text | `textMm` (2.5) |
| Arrow | `arrowMm` (2.5) |
| Extension line offset / extension | 1.5 / 1.25 |
| Text gap | 0.9 |
| Baseline spacing | 7 |

Paper sizes are landscape: A0 1189x841, A1 841x594, A2 594x420, A3 420x297, A4 297x210, or a custom
sheet of 50 to 20000 mm per side.

`plot.validate {marginMm?: 10, minTextMm?: 1.8, maxTextMm?: 12}` reads the stored configuration and
reports `fits` (model extents at the scale against the printable area, with the smallest scale that
fits), text that would print smaller or larger than the limits (handles listed, 50 at most), and
the number of annotation conflicts, as `warnings`.

## Collisions and layout

`annotation.collisions {clearance?}` lists, in handle order, every TEXT, MTEXT or attribute
definition (`text`) that comes within `clearance` of another text (`text-text`), a line, arc,
circle, ellipse, polyline, spline or ray (`text-geometry`), a dimension (`text-dimension`), a block
reference (`text-block`) or a leader (`text-leader`). Candidates come from the spatial index
(`select_window`, crossing), so only objects near a text are tested. The default clearance is 0.5
printed millimetres once a plot scale is set, else 0.

`annotation.layout {mode: "preview"|"apply", clearance?, maxSteps?: 12, includeManual?}`:

1. Conflicting texts are taken in handle order.
2. Text in the group `AAD-MANUAL` (`annotation.lock {handles, manual}`) is skipped and reported in
   `skippedManual`, unless `includeManual`.
3. For each text, candidate positions are tried at 1..`maxSteps` text heights in a fixed order of
   eight directions (E, N, W, S, NE, NW, SW, SE). The first position clear of all other text
   (including positions already proposed) and of all geometry wins.
4. A text with no clear position is listed in `unresolved`; it is not moved, hidden or deleted.
5. `preview` returns the moves and changes nothing; `apply` translates only the text objects (not
   those on locked layers) as one undo step.

Properties: only text moves; geometry never does; the result is deterministic (the same drawing and
settings give the same moves) and stable (a second run after `apply` proposes nothing).

## Status bar popup

The status bar shows `drawing → display  1:scale`. Its popup has the Drawing units dropdown with
*Change unit only* and a two-click *Change unit and rescale*, the Display units dropdown, the plot
scale and paper with *Size annotation*, *Check the sheet*, and *Find* / *Resolve overlaps*. Every
control runs the commands above.

## AI tools

`set_drawing_units` (destructive when `mode` is `scale`), `set_display_units`,
`set_plot_configuration`, `validate_plot_layout`, `analyze_annotation_collisions`,
`preview_layout_changes`, `calculate_annotation_layout` (same as preview), `apply_layout_changes`
(destructive with `includeManual`). Changing tools need confirmation; the calculations are the
commands above. See [ai-agent.md](ai-agent.md).

## Not implemented

- A declarative, CSS-like constraint language, or a general constraint solver: layout is a
  deterministic search over candidate positions.
- Incremental layout: each run recomputes from the current drawing (candidate lookup uses the
  spatial index, so cost follows the number of texts, not the whole drawing). There is no
  scheduler that re-flows labels when an object moves.
- Interactive preview in the viewport and conflict highlighting; the preview is data
  (`moves`, `unresolved`).
- Moving dimensions, dimension text, leaders or blocks; conflicts of dimension lines with text;
  conflicts with hatches or viewport boundaries; shrinking text within limits; automatic
  re-layout when the plot scale or paper changes (run `annotation.layout` again).
- Unit-aware quantity take-off, schedules and exports: those features do not exist yet.
- Unit detection beyond reading `INSUNITS`; ambiguity is reported, not resolved by heuristics.
- A survey of external layout or collision libraries was not done; the implementation reuses the
  existing spatial index and window selection.
- Plot-level checks of rendered output (PDF) against the configured scale.

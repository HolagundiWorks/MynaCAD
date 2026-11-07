# Active Drawing Space, layer protection and the workspace modes

> Describes the optional egui window (`aad --egui`): the context bar, the Classic and Pro workspaces and the
> docked panels are egui features. The default WinUI shell has no context bar (a layer selector in the top bar, level
> tabs, drawing tabs above the drawing and a right-hand rail of panel titles instead; see [status.md](status.md)). The layer-state
> commands (`space.*`) and the layer protection in the engine apply to both.

What was found, what was built, how it behaves, and what is not done. Written against the code of
2026-10-12; every statement about behaviour has a test named below.

## 1. What existed before (inspection)

| Area | Found |
|---|---|
| Layers | `doc::Layer` with `on`, `frozen`, `locked` (independent flags), colour, linetype, lineweight, plot flags. No stable layer ID: the **name** is the only key. `layer.set newName` renames and re-points objects |
| Saved layer states | `layerstate.*` (whole `Layer` copies by name, AutoCAD-style). Kept as it is |
| Lock enforcement | Individual commands skipped locked objects by themselves (move, erase, trim, ...). No central rule: `copy` of locked objects and drawing on a locked current layer got through |
| `layer.current` | Silently **thawed** a frozen layer, and accepted a locked one |
| Floors | Two models: a drawing's `floors` list (heights, project data) and project folders (`project.aad.json`, `floor.activate` opens another file, other floors are locked overlays). No "current floor" for a single drawing; `room` needed `floorPrefix` every time |
| Project data | `Drawing.project` (string map), saved in the DXF as extension data, part of undo |
| Workspaces | Classic and Pro, one shared panel layout, one pin setting; Pro hid panels and toolbars (including the layer controls). Toolbars: Draw on the left, Modify on the **right**. Properties docked left, Layers/Blocks right |
| Top controls | Classic only: a property row (layer-of-selection, colour, linetype, lineweight); removed since, see docs/workspaces.md |

## 2. What was added

### Engine (`crates/engine`)

| File | What |
|---|---|
| `src/protect.rs` | One check after every undoable command (and every finished interactive command): if it replaced or removed an object on a **locked or frozen** layer, or put a new object on one, the drawing and selection are put back and the command fails with a message naming the layer. The layer's state **before** the command counts. Whole-command refusal, never half applied |
| `src/cmd/space.rs` | Active Drawing Space: the model and `space.*` commands (below) |
| `src/cmd/layer.rs` | `layer.current` refuses frozen and locked layers and no longer thaws; `layer.set newName` updates drawing spaces |
| `src/cmd/projfile.rs`, `project.rs`, `hcw.rs` | `floor.current` and the active floor; `room` uses the active floor's code (`GF`, `FF`, `SF`, `TF`, `F4`...) when `floorPrefix` is not given |

Exempt from the protection check, on purpose: `layer` and `layer.set` (renaming re-points objects, it
is not an edit of them) and `xref.*`, `floor.*`, `project.*` (they manage the locked overlay layers
of other drawings).

### Active Drawing Space model

Stored in the drawing's project data under `drawingSpaces` (so it travels with the file and takes
part in undo):

```text
{ version, active: id | null, next,
  spaces: [ { id: "ds-N", name, description, floor: name | null,
              layers: [ { layer, visible, frozen, locked } ],
              current: layer | null,
              unlisted: "keep" | "hide" | "lock",
              created, modified, version } ] }
```

- **Visible, frozen and locked are separate flags** and are applied to the layer table's `on`,
  `frozen`, `locked`. **Editable is derived** (`not frozen and not locked`); it is never stored, so a
  space cannot contradict itself. A space does not make every visible layer editable.
- The layer table stays the only live state. A space is a saved snapshot of three flags per layer,
  plus an optional current layer. Colour, linetype, lineweight, names and objects are never touched.
- **Layers are referenced by name.** Rename (`layer.set newName`) is followed in every space. A
  deleted layer stays in the space and is reported as `missing` on activation. A layer created after
  the space was saved is left alone, unless the space's `unlisted` policy hides or locks it.
- **Current layer rules:** a space's current layer must be visible, thawed and unlocked in the same
  space (checked when saved). On activation, if the current layer would become frozen, the current
  layer moves to the first usable layer of the space (else layer 0) and a warning says so; if there is
  none, activation is refused whole. A locked or hidden current layer produces a warning.
- Limits: 200 spaces, 10 000 layer entries each, names of 1 to 64 characters.

### Commands

| Command | Does |
|---|---|
| `space.list {floor?}` | spaces (those for the floor and for every floor), active id, whether the layers differ from the active space |
| `space.get {id\|name}` | one space with a row per layer: visible, frozen, locked, editable, exists |
| `space.save {name, description?, floor?, current?, unlisted?, layers?, activate?}` | save the current layer states (or the given ones) as a new space |
| `space.update {id\|name, newName?, description?, floor?, current?, unlisted?, layers?, remove?}` | edit the **saved** space only; the drawing is unchanged |
| `space.capture {id\|name}` | store every layer's current flags in the space |
| `space.activate {id\|name}` | apply it: layer flags, current layer; returns `changed`, `missing`, `warnings`; one undo step |
| `space.preview {id\|name}` | what activation would change; changes nothing; `many` when over 25 settings |
| `space.duplicate`, `space.delete` | as named; deleting the active space clears it |
| `space.status` | active space, `dirty`, and the differences (layer, field, saved, now) |
| `space.recover` | show, thaw and unlock every layer (the way out of a bad space) |
| `floor.current {name?}` | the current floor: project mode opens that floor (`floor.activate`); single drawing mode stores the choice. Moves no object, changes no layer, leaves the active space alone |

All are ordinary commands: the command line, scripts, the control channel, MCP and the agent reach
them, and none opens a dialog.

### Interface (`crates/ui-egui`)

| File | What |
|---|---|
| `src/contextbar.rs` | The **context bar**, a row above the drawing in **both** modes: current-layer selector (search box, colour swatch, locked/frozen/hidden badges, locked and frozen layers shown but not selectable, with the reason), floor selector and the panels pin toggle. The egui window has no drawing-space selector or editor any more (removed 2026-10-28 as unreachable code); spaces are used through the `space.*` commands |
| `src/docking.rs` | Default layout: the drawing, and **one tabbed stack on the right: Layers, Properties, Blocks** |
| `src/toolbars.rs` | Draw and Modify (with Dimension and Leader tools) as two vertical bars on the **left** |
| `src/menus.rs` | Per-mode state: `set_workspace` swaps the panel arrangement, pin state and toolbar switch of the mode being left with its own saved slot; `ui.workspace.reset` |
| `apps/aad/src/workspace.rs` | Saves the inactive mode's slot (`modeSlots`) in the workspace file |

### Workspace modes

- **Classic:** left toolbars, customisable top tool bar, context bar, right-hand tabbed panels, **pinned by
  default**. Unpin them (button in the context bar, or Window > Pin Side Panels) and they auto-collapse
  to an edge strip when the pointer has been away for 1.5 s.
- **Pro:** no toolbars, panels **collapsed by default** (auto-collapse on, edge strips bring them back),
  context bar kept. Panels open from the Window menu or the command line (not covered by a Pro-specific test).
- Each mode keeps its **own** arrangement, pin state and toolbar switch; switching never overwrites
  the other's. A saved slot that no longer reads is dropped without affecting anything else.
  `Reset This Workspace` (Window > Workspace) resets the current mode only.
- Switching modes or tabs changes neither selection, nor command state, nor the view (tests below).

## 3. Acceptance criteria

| Criterion | State | Evidence |
|---|---|---|
| Classic shows left toolbars and pinned right-side tabbed panels | Done | `workspace_modes.rs`; screenshot checked |
| Pro opens with no left toolbars and all panels collapsed | Done | `context_bar.rs::both_workspaces...`, `workspace_modes.rs` |
| Both modes have the top toolbar with drawing-context controls | Done | `context_bar.rs` |
| Layer selector changes the current layer; unsuitable layers refused | Done | `context_bar.rs::the_layer_selector...`, `layer_protection.rs`, `drawing_space.rs` |
| Floor selector changes the floor, moves nothing | Done | `context_bar.rs::choosing_a_floor...`, `drawing_space.rs::choosing_a_floor...` |
| Create, edit, save, switch spaces | Done through the `space.*` commands (no egui control) | `drawing_space.rs` (13 tests) |
| Saved layer states persist after closing and reopening | Done | `apps/aad-cli/tests/drawing_space_file.rs` (real DXF round trip) |
| Locked objects cannot be modified through any supported pathway | Done for model space and layouts, 22 commands, by handle and by selection, programmatic and interactive | `layer_protection.rs`. **Not covered:** objects inside block definitions (see below) |
| Frozen layers follow the engine's rules | Done: not editable, not offered as current, never thawed as a side effect | `layer_protection.rs`, `drawing_space.rs` |
| Switching spaces restores states without changing geometry | Done | `drawing_space.rs` (geometry compared before and after, repeated switching) |
| Missing, renamed, deleted layers handled safely | Done | `drawing_space.rs::renamed_deleted_and_new_layers_are_handled` |
| Existing commands, undo/redo, selection, save/load keep working | Done: 1 085 workspace tests pass; activation is one undo step | `cargo test --workspace` |
| Tests cover both modes, persistence, floor switching, protected-layer editing | Done | files above |

## 4. Not done, and open questions

- **Block definitions are not guarded.** Editing a block's content (BEDIT-style, `blocks` commands)
  can change objects on a locked layer inside the definition; `protect.rs` checks model space and
  layouts only. Unfinished: needs a rule for what "layer of an object in a block" means.
- **Stable layer IDs do not exist**; names are the key. A rename done outside `layer.set` (a DXF
  edited by hand) breaks the link and shows as `missing`.
- **Panels that do not exist and were not invented:** Opening Schedule, Drawing Explorer, Xrefs,
  Measurement and Quantities, Project/Floor Navigator. The spec lists them as "potential". The
  commands behind several exist (schedules, take-off, xrefs, floors) but have no panel. The
  Annotation, Hatch, Blocks and Measurement toolbar groups are the existing Draw/Block/Hatch/Text
  tools; there is no separate Measurement toolbar.
- **Configurable toolbar order and visibility per toolbar:** not implemented (one switch).
- **Export and import of drawing spaces** between drawings: not implemented (they travel with the
  file). A space can be copied by `space.get` and `space.save` with `layers`.
- **Per-floor layer state beyond the optional `floor` scope** of a space: not implemented. A space
  can be scoped to a floor and the list filters by it; activating a floor does not activate a space
  (deliberately: the active space is never overwritten by a floor change).
- **Discipline and floor metadata on layers**: layers carry none; spaces filter by layer name.
- **Pro-mode panels as temporary overlays** are the existing auto-collapse behaviour (panels dock in
  the side and collapse again); they do not float over the drawing.
- A change in the **docked/floating panel layout of the old default** (Properties on the left) is
  not migrated: saved layouts that name a panel set the code no longer has fall back to the default
  arrangement and keep the other preferences.

## 5. Conflicting instructions, and how I reconciled them (for review)

The specification says Classic panels are pinned by default. A separate instruction asked for
auto-collapse. Both hold: Classic is pinned by default, Pro collapses by default, and the pin toggle
in the context bar switches either mode.

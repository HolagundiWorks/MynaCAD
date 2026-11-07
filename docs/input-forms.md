# Input forms

Most architecture commands take JSON. A menu click has none, so each command that needs input has
a form declared as data in `crates/engine/src/forms.rs` (`FORMS`). One declaration serves three
callers:

- **Dialog** (menu and toolbar): the settings are shown in a dialog (WinUI `FormDialog`, or
  `crates/ui-egui/src/formdialog.rs`): Project Settings (details, default slab thickness and beam
  depth including slab), Floors (names), beam depth list, the opening schedule (a table), rates,
  room type and options, door/window block and width, stair rise and width, take-off options, and
  so on. OK validates the values and shows the reason if one is wrong. If the command also needs
  points, they are picked on the drawing afterwards.
- **Command line**: typing a command that has a form (and no prompts of its own) asks the missing
  values one by one, with points picked on the drawing. Enter skips an optional question; Escape
  cancels. A bad answer keeps the question.
- **Scripts, control channel, agent**: unchanged. They call the command with JSON and never see a
  dialog.

Both paths end in the same call, `forms::parse_input` turns the collected text into the command's
parameters, and a refusal from the command is shown and ends the command. Numbers must be finite;
tables are limited to 500 rows and point lists to 10 000 points.

**Lengths take any unit.** A form field that is a length (wall thickness, door width, sill, stair rise, grid bays,
column size, level height and the rest; the `drawing`, `mm` and `m` markers in `FORMS`) accepts `12mm`, `30 cm`,
`0.3m`, `1'`, `1'6"`, `18"` or a bare number. A suffix converts into the unit the field is measured in: the drawing's
unit for most fields, millimetres for the `...Mm` fields and beam depths, metres for the level fields. A bare number
stays in that unit. The same holds for JSON calls: `forms::normalize_lengths` runs in `Session::execute`, so
`{"thickness": "230mm"}` works from a script, MCP or the control channel. A list (`bays`, `rows`, `size`, `depths`)
takes one unit per entry (`4m, 5000mm, 15'`; commas separate entries when there are any). Only a string that reads
as a length is converted; anything else is left for the command to refuse. Commands that have no form (and so no
declared length fields) still take numbers in drawing units; the command line accepts units on every distance
(`units-annotation.md`). Checked by `forms::tests` and `crates/engine/tests/forms.rs`
(`architecture_commands_take_a_length_in_any_unit`, `lists_and_fixed_unit_fields_take_units_too`).

Added 2026-11-02: the length cells of the opening schedule (width, height, sill, metres) take units
(`the_openings_table_takes_a_unit_in_each_length_cell`); the dialog label says the drawing's unit where it used to
say "drawing units", and a length field's help line says that any unit can be typed; the Project Wizard (floor to floor
in m, slab and beam in mm, floor heights in m), the Smart Blocks form (into the parameter's own unit) and Room Label
Settings (text height) read a typed unit through `LengthText` (`apps/aad-winui/LengthText.cs`, the same suffixes as the
engine); `underlay.calibrate` takes `length` as text with its own unit (`"3.5m"`, `"12ft"`).

Unit words accepted after a number (command line, forms, JSON): `mm`, `cm`, `dm`, `m`, `km`, `µm`/`um`, `in`, `ft`,
`yd`, `mi` and the long forms (`inch`, `feet`, `metres`, `meters`, `kilometres`, ...); feet-inches `1'6"`; scientific
numbers (`1.5e1cm`). A unit typed after a space joins the number (`5 ft,0`, `2 cm<45`), but `m`, `in`, `mi`, `um` and
`dm` alone after a space stay keywords (`CHAMFER ... 1 M` is Multiple): type `5m` or `5 m,0`.

Forms exist for: `project`, `floors`, `beams`, `schedule`, `takeoff.rates`, `room`, `hcwlayers` (choices: standard, permit, presentation, both, all),
`takeoff.linear|brick|beams|columns|rects|paint|ceiling|floor|area|slab`, `wall.draw`, `wall.door`,
`wall.window`, `grid.axis`, `column.place`, `lintel.draw`, `stair.plan`, `lift.shaft`, `rail.draw`,
`level.mark`, `section.marker`, `elevation.marker`, `north.arrow`, `slope.arrow`,
`electrical.connect|number|schedule`, `plumbing.pipe|pipe.cold|pipe.hot|pipe.drain|connect|number|schedule` and `bridge.export`. `bridge.import` has its own file picker
and preview window ([hcw-kit-port.md](hcw-kit-port.md)).

Adding a form for a new command is one entry in `FORMS`; `forms::tests` checks that every form
names a real command and that field keys are unique.

Limits: the dialog translates field labels only when the interface string catalog has them (the
catalog does not yet cover the form labels, so they show in English); `schedule`'s dialog edits
the whole opening list, not one entry.

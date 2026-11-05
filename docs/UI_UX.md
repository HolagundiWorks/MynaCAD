# User interface

This document describes the optional egui window (`cargo build -p aad --features egui` then `aad --egui`). The default Windows shell started by `aad` is the WinUI app in `apps/aad-winui`; what it implements is in [status.md](status.md). The menu and command list is the command registry (`ui.menu.list`); the
measured behaviour is in [UI_TEST_MATRIX.md](UI_TEST_MATRIX.md)
and [UI_BASELINE.md](UI_BASELINE.md); workspace details in [workspaces.md](workspaces.md).

## Layout

| Region | Notes |
|---|---|
| Menu bar | 12 menus: File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Architecture, Window, Help (282 entries) |
| Toolbars | Classic only: Draw, Modify and Annotate as vertical tool bars on the left; the top strip has the layer box, the architecture tools and the gear (View > Customize Top Bar). The take-off tools (Ext wall, Int wall, Beam, Floor, Ceiling, Column, Deduct, Paint, Openings, Record, Map layers) appear in the top strip only while **Take-off** is switched on, the toggle in the bottom bar next to Nameplate |
| Context bar | Both workspaces: current layer (searchable), floor, drawing space, panel pin/auto-hide ([ACTIVE_DRAWING_SPACE.md](ACTIVE_DRAWING_SPACE.md)) |
| File tabs and layout tabs | Drawings along the top; Model and layout tabs along the bottom, with New layout and a layout list |
| Canvas | GPU-drawn crosshair cursor (`CURSORSIZE` / `PICKBOX`): idle = crosshairs+pickbox; point prompt = crosshairs only; select-objects prompt = pickbox only; OS cursor hidden on the WinUI drawing HWND. Cyan rubber-band preview, dashed blue selection / solid hover highlight, mode-specific snap markers with tips, polar/OTRACK paths, grips (warm under pointer), selection window fill + dashed crossing |
| Panels | Layers, Properties and Blocks (one tabbed stack on the right), Plot window; docked, floatable, closable, pin and auto-collapse |
| Command line and history | Typed commands with prompts; F2 opens the history |
| Status bar | Toggles (snap, ortho, polar, tracking, dynamic input), coordinates, units popup, messages |
| Widgets | Architect calculator and Pomodoro timer as floating overlays |

Two workspaces: **Classic** (toolbars and side panels) and **Pro** (full-screen drawing area with
command line and status bar), switched with `Ctrl+F11`. Switching changes visibility only. Two
themes, light (white drawing area) and dark (black), with no other colour customisation. The interface is English only.

WinUI secondary windows (layer manager, hatch, rooms, styles, forms, About, PDF, …) use the same
solid shell chrome as the main window via `ShellChrome` / `ShellIcon.Apply`: no Mica/acrylic
backdrop, opaque page fill, and matching title-bar colours. Theme toggles re-apply chrome to open
tool windows. Content-based sizing and close-with-shell behaviour: [WINDOW_SIZING.md](WINDOW_SIZING.md).
Shared colours, type, spacing and control sizes: [UI_TOKENS.md](UI_TOKENS.md) (`Themes/*.xaml`, `Density`).
Window templates, shared components and the hierarchy rules every window follows:
[UI_STANDARDS.md](UI_STANDARDS.md) (`Standard.cs`; enforced by `cargo xtask ui`).
WinUI visual direction: Fluent graphite + Windows blue accent; solid overlays (no glass on the
drawing child); AI vs CAD channels distinguished by accent/green edges without changing routing.


## Working with commands

Pick a tool from a menu or toolbar, or type its name or alias at the command line. The prompt
asks for points, objects or options; click on the canvas or type coordinates. Escape cancels,
Enter finishes (a polyline, for example), and right-click acts as Enter while a command runs;
otherwise it opens the shortcut menu (Shift or Ctrl with right-click opens the snap menu).
Selection before a command is supported. Scripts and the control channel run the same commands
without dialogs.

## Mouse

Click picks (Shift removes); a click on empty space or a drag opens a selection window (left to
right: inside only; right to left: crossing); the wheel zooms about the pointer; middle-drag pans;
double-click on text edits it; grips on a selected object edit it; Tab cycles snap candidates.

## Keyboard shortcuts

`Ctrl+N`, `Ctrl+O`, `Ctrl+W`, `Ctrl+S`, `Ctrl+Shift+S`, `Ctrl+P`, `Ctrl+Z`, `Ctrl+Shift+Z` / `Ctrl+Y`,
`Ctrl+X/C/V`, `Ctrl+Shift+C/V`, `Ctrl+A`, `Delete`, `Ctrl+1` Properties, `Ctrl+9` command line,
`Ctrl+Q`, `Ctrl+F11` workspace, `Ctrl+Shift+F11` reset toolbars and menus, `F1` command reference, `F2` history, `F3` object snap, `F8` ortho,
`F10` polar, `F11` object snap tracking, `F12` dynamic input (from the command registry; complete
list: `ui.menu.list` on the control channel). Other shortcuts do not fire while a text field has focus; the function keys do, including in the command line. There is no grid display or snap to grid, no isometric plane switch and no 3D object snap, so `F4`, `F5`, `F7` and `F9` do nothing.

## Dialogs

Managers (layers, text and dimension styles, plot styles, units), generated input forms for the
architecture commands ([input-forms.md](input-forms.md)), About, language, command reference, and
the "Save changes?" prompt on close.

## Window sizes

Tested: 1400x800, 1100x700, 900x560 and 1600x900 (canvas stays inside the window and picks
correctly), and 1200x760 and 900x560 at 1x and 2x scale (status labels do not overlap). No minimum
window size is defined or documented.

## Limitations

- Opening, saving and plotting run on the UI thread and have no progress display.
- Files are opened through a platform file dialog supplied by the app; there is none in headless runs.
- The Plot window has no automated UI test. Dynamic input is covered by host tests (`aad-host`: what shows beside the cursor, and that assistant requests do not).
- No screenshot baselines are approved, so visual regressions are not detected automatically.

## Automation coverage

Click-driven drawing workflow (`drawing_workflow`), a walk over every menu entry (`menu_integrity`),
docking, workspaces, context menu, dialogs, widgets, status-bar geometry, and the control channel.
See [TESTING.md](TESTING.md) and [UI_TESTING_STRATEGY.md](UI_TESTING_STRATEGY.md).

## Dynamic input beside the cursor (WinUI)

The cursor-anchored CAD field parks beside the crosshair when host `dyn.on` is true (independent of
the Dynamic Input mode toggle). It shows **typed text only** (no prompt label or Length/Angle
readouts). The field is 70×18 DIP (half the previous bar), with the same 2 DIP corner radius as a button, below the 32 DIP floor used elsewhere. Typed distances still accept SketchUp-style units
in the engine (`40`, `40<90`, `5'6"`). Slash assistant drafts stay on the fixed AI bar (`dyn.on`
false for `/…` drafts). While a CAD command runs, the Ask bar hides so it does not cover the lower
drawing (Allow/Decline stays visible). Full dual-channel contract: [COMMAND_BAR.md](COMMAND_BAR.md).

## Text size (annotative)

Text is annotative, as in AutoCAD: its height is the height it prints on the sheet times the plot scale, held in drawing units, so it zooms with the
drawing like any other geometry. Set the plot scale and paper once (Units > Plot scale and paper, `plot.config`): text, dimension text, arrows and the
default `TEXTSIZE` are then sized to print 2.5 mm (or the paper's small, medium or large rank) at that scale, and `plot.validate` lists text that would
print too small or too large. Changing the scale and applying it again resizes the annotations to the new scale. There is no screen-pixel size for text.

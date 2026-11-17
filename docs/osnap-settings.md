# Osnap settings pane

Status: implemented 2026-11-17 on branch `osnap-settings-pane` (see "What was built" for what was run and what was not).

## Why

Owner report (2026-11-17): "Osnap settings pane doesn't exist". Findings:

- The engine has 14 running object-snap modes (`snap::mode`: END, MID, CEN, GCEN, NOD, QUA, INT, EXT, INS, PER, TAN, NEA, APP, PAR), the OFF bit, the hatch bit
  (`osnaphatch`), `Settings.aperture` (pixels, `APERTURE`) and OTRACK. `osnap` is the toggle / bit-set command (F3, aliases `os`, `ddosnap`).
- The WinUI shell has only the combined **Drafting Settings** window (`DraftingWindow.cs`, polar + osnap + dynamic input + ortho), reachable
  from Tools > Drafting Settings or `dsettings`. The Shift/Ctrl+right-click override popup has an item "Osnap Settings..." that opens that
  combined window, scrolled to nothing in particular. The status-bar Osnap button has no right-click, no aperture or hatch setting is
  editable anywhere, and no snap setting survives a restart (the session starts from `Settings::default`, osmode 4133).

## What

1. **Command `osnap.settings`** (engine, `cmd/settings.rs`), aliases `osnapsettings`, `osettings`. JSON form, never opens a dialog.
   Params (all optional): `osmode` (bits, masked to the known bits), `modes` (names as for `osnap`), `all: true` (every mode on), `none: true`
   (every mode off), `on` (master switch, the OFF bit), `aperture` (pixels, clamped 1..50, non-finite ignored, like `APERTURE`), `hatch` (OSNAPHATCH),
   `tracking` (OTRACK). Returns `{ on, osmode, aperture, hatch, tracking, modes: [{ id, name, bit, enabled }] }` so a UI lists the catalogue
   from the engine, not from a copy. With no params it only reports. The shell command id is `osnap.settings`; the bare words `OSNAP`
   typed at the command line keep toggling (existing behaviour), `OSNAPSETTINGS` and `OS`-dialog entry points open the pane.
2. **WinUI pane** `OsnapSettingsWindow.cs` (template `PropertyDialog`, `StandardWindow.Present`, id `osnap`): master check box, one check box per
   catalogue mode in two columns, **Select all** / **Clear all**, aperture (NumberBox in pixels), include hatch objects, object snap tracking.
   Every change runs the engine command and the pane reloads from the reply; the running snap engine reads `Settings` live, so it applies at once.
   Dimensions come from `Density` tokens only.
3. **Entry points:** right-click (RightTapped) on the status-bar Osnap button; the override popup item "Osnap Settings..."; Tools > Osnap Settings...
   (command `osnap.settings` with a menu path); the command line words above.
4. **Persistence per user:** `shell.json` (`ShellPrefs`, `%LOCALAPPDATA%\AAD\shell.json`) gets an `osnap` object `{ osmode, aperture, hatch, tracking }`.
   The pane saves it after each change; the shell applies it once to each new session through `osnap.settings` after the host starts. Missing or
   invalid values leave engine defaults. Nothing is written into `.osp` (MynaOps shares that file).

## Areas touched

`crates/engine/src/cmd/settings.rs` (+ tests), `apps/aad-winui` (`OsnapSettingsWindow.cs`, `MainWindow.xaml(.cs)`, `ShellPrefs.cs`),
`crates/aad-host/src/chrome.rs` (override popup id), `docs/status.md`, `xtask/src/ui.rs` only if a literal baseline is needed (not expected).

## Acceptance

- `osnap.settings` with no params reports the 14 modes, defaults osmode 4133 (endpoint, centre, intersection, extension) and aperture 10.
- `all`, `none`, `modes`, `osmode`, `aperture`, `hatch`, `tracking` change `Session.settings` and the next snap uses them.
- Hostile params (huge numbers, NaN-like strings, wrong types, empty or non-ASCII mode names) never panic and never leave a bit outside the known set.
- The pane opens from the status-bar right-click and from the menu; toggling a box changes `OSMODE` as `getvar` reports it; restart keeps the choice.
- `cargo fmt --check`, clippy -D warnings on aad-engine and aad-host, tests, `cargo xtask layers/assets/ui/licenses/wasm` pass.

## Out of scope / follow-ups (the engine lacks them; not faked in the pane)

Snap marker size and colour (`AUTOSNAPSIZE`; the marker size is fixed in `overlay.rs`), magnet (`AUTOSNAP` bit 2) and tooltip (bit 4)
toggles (`AUTOSNAP` is derived, not stored), a separate one-shot override editor, snap-from and temporary tracking points, osnap on 3D.

## What was built

- Engine: `osnap.settings` (`cmd/settings.rs`; aliases `osnapsettings`, `osettings`; menu Tools > Osnap Settings...). A report-only call (no params) does not mark the drawing changed.
  `osnap` keeps its toggle behaviour. Tests: `crates/engine/tests/osnap_settings.rs` (defaults and catalogue, alias from the command line, all/none/modes/osmode/master switch,
  aperture/hatch/tracking, hostile params).
- Shell: `OsnapSettingsWindow.cs` (pane and `OsnapPrefs`), `MainWindow.xaml(.cs)` (status-bar Osnap button `ContextRequested`, i.e. right-click, command dispatch,
  apply saved choices once the host exists, pane refresh on chrome refresh), `ShellPrefs.cs` (`osnap` object in `shell.json`), `aad-host/chrome.rs`
  (the override popup's "Osnap Settings..." now opens this pane instead of Drafting Settings). Drafting Settings is unchanged.
- Run: the Osnap pane was opened in a running WinUI build (Debug, temp output) through the Tools menu with UI Automation. UIA listed the master check box,
  14 mode check boxes, Select all, Clear all, the aperture spin box, hatch and tracking check boxes; a window capture showed them laid out in the shell's theme.
  Ticking Midpoint, Clear all, Select all, Endpoint and hatch through UIA changed `OSMODE` as the engine reports it (4135, 0, 16383, 1) and `shell.json` got
  `osnap {osmode 1, aperture 10, hatch true, tracking true}`; a second start of the shell reported osmode 1 and hatch true again.
- Not verified: the right-click on the status-bar Osnap button (synthetic mouse input never reached the window in the automated run, a "Restore unsaved drawings?"
  overlay was showing); the handler is a standard `ContextRequested`. The aperture spin box was not changed in the UI (engine clamp is tested). Light theme not looked at.

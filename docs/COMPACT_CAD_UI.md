# Compact CAD UI (classic compact density)

> Density background and the original sizing pass. The current rules for windows, layout and
> components are in [UI_STANDARDS.md](UI_STANDARDS.md); where the two differ, that page wins.

Sizing and styling pass for O-Draft WinUI 3 chrome. **No CAD engine, workflow, or command changes.** Density targets compact professional CAD chrome (2007-era scale as a reference).

## Design targets (effective pixels / DIP)

| Element | Target |
|---|---|
| Menu bar | 22–24 |
| Menu item | 20–24 |
| Toolbar icon | 16×16 |
| Toolbar button | 24–28 |
| Property row | 24–26 |
| Input / dropdown | 22–24 |
| Panel header | 24–26 |
| Dialog button | 25–28 |
| Command line row | 20–22 |
| Status bar | 22–24 |
| Body text | ~12 |
| Panel padding | 4–6 |
| Dialog padding | 10–12 |

Implemented values live in `Density.cs` and `Themes/*Tokens.xaml` (keep in sync).

## Phase 1 — Inventory

### Shell

| Surface | Primary files |
|---|---|
| Menu bar / menus | `MainWindow.xaml`, menu build in `MainWindow.xaml.cs`, `ShellMenuBarStyle` / flyout styles |
| File tabs | `MainWindow.xaml` (`FileTabs`, `FileTabHeight`) |
| Tool bar (drawing tools, a divider, architecture tools; one line) | `ToolStrip`, `ArchBar` in `ToolStripHost` |
| Vertical tool strip | `ToolStrip` / `ToolStripHost`, `MakeToolButton`, `ToolIcons` |
| Side panel (Properties / Layers / Blocks) | `SideHost`, `PropertiesPanel`, `LayersPanel`, `BlocksPanel` |
| Side rail (collapsed) | Side-rail label transform in `MainWindow.xaml.cs` |
| Command / Ask bar | `CommandBar`, `AiInput`; cursor CAD in `CommandChrome.cs` |
| Status bar | `StatusRow`, chips, units, theme |
| Start / splash | `WelcomeOverlay` (New / Open / Project Wizard / recent), `BusyOverlay` (open/save progress), splash overlay |

### Dialogs and managers

| Window | File |
|---|---|
| Layer Properties Manager | `LayerManagerWindow.xaml(.cs)` |
| Hatch Editor | `HatchWindow.cs` |
| Text / Dim / MLeader / Linetype / Plot styles | `*StyleWindow.cs`, `LinetypeWindow.cs`, `PlotStyleWindow.cs` |
| Units, page setup, batch plot | `UnitsWindow.cs`, `PageSetupWindow.cs`, `BatchPlotWindow.cs` |
| Drafting settings, point style, rooms | `DraftingWindow.cs`, `PointStyleWindow.cs`, `RoomSettingsWindow.cs` |
| Engine forms | `FormDialog.cs` |
| Confirmations / about / tools / calc | `SavePrompt.cs`, `AboutWindow.cs`, `ToolsWindow.cs`, `CalcWindow.cs`, … |

### Token / style sources

| File | Role |
|---|---|
| `Themes/ControlTokens.xaml` | Heights, icon size, Fluent min overrides |
| `Themes/SpacingTokens.xaml` | Space scale, paddings |
| `Themes/TypographyTokens.xaml` | Font sizes / families |
| `Themes/ControlStyles.xaml` | Icon buttons, menus, chrome, inputs |
| `Themes/ColorTokens.xaml` | Semantic brushes (unchanged role) |
| `Density.cs` | Code-built UI constants + helpers |
| `ToolIcons.cs` | `DisplaySize = 16` |

## Phase 2–3 — What changed

- Centralized compact tokens; implicit `Button` / `TextBox` / `ComboBox` / `ListViewItem` / menu styles.
- Fluent overrides: `TextControlThemeMinHeight`, `ListViewItemMinHeight`, menu/flyout padding.
- Shell: menu 24, tabs 24, toolbar icons 16 / buttons 26, status 22, command row 22, side panel default 240.
- Panels: tighter label/value spacing; property labels use caption size.
- Dialogs: inputs at `ControlHeight` (24); primary actions at `DialogButtonHeight` (26).
- Hatch pattern swatches remain **72 DIP** so patterns stay distinguishable.

## Phase 4 — DPI notes

- All sizes are DIP; WinUI scales with Windows display scaling.
- Prefer vector `DrawnIcon` / `ToolIcons` at 16 DIP.
- Manual check at 100% / 125% / 150% / 200%: menu/tool clipping, flyout placement, dock splitter, multi-monitor.

## Phase 5 — Acceptance checklist

1. Menus and toolbars use compact density.
2. Icons aligned, not clipped.
3. Properties readable at minimum panel width.
4. Hatch / Layer / Block / style / Plot dialogs keep all controls.
5. No clipped popup content at default sizes.
6. Shortcuts, tooltips, bindings unchanged.
7. Command line and status behaviour unchanged (Ask vs CAD dyn).
8. More canvas height from thinner chrome.
9. Usable at listed scale factors.
10. Existing automated tests still pass.

## Exceptions (reference density only)

| Item | Reason |
|---|---|
| MenuBar / MenuFlyout | WinUI templates retain some Fluent padding; height capped via styles + theme keys. |
| ComboBox / TextBox | Template chrome may exceed 22 DIP even with `MinHeight` 24. |
| Hatch swatches | 56 DIP (`Density.HatchSwatch`) so patterns stay readable without the old 72 DIP tiles. |
| Cursor CAD field | Intentional transparent 105×27 DIP overlay (`CommandChrome`); not status-bar density. |
| TabView | Add-tab control and item chrome follow WinUI; height set to `FileTabHeight`. |
| Touch / pointer targets | Some controls stay ≥24 DIP for accessibility rather than classic 16–18. |

## Screenshots

Capture at a fixed window size (e.g. 1440×900) and 100% scaling:

1. Main shell (menu + arch bar + tools + properties + status).
2. Layer Manager.
3. Hatch Editor (swatch grid).
4. Text Style (or Dim Style) dialog.

No before/after screenshots are stored in the repository.

## Related

- [UI_TOKENS.md](UI_TOKENS.md)
- [WINDOW_SIZING.md](WINDOW_SIZING.md)
- [UI_UX.md](UI_UX.md)

## Consistency pass (dialog chrome)

Shared helpers: `DialogLayout.cs` (`Labeled`, `Heading`, `Field`, `Action`, `ActionRow`). Width tokens: `ManagerListWidth` 180, `FieldWidth` 120, `DialogActionMinWidth` 72. Confirmation rows are right-aligned with the primary action immediately before Cancel. Live-apply managers keep their toolbars.

| Surface | Change |
|---|---|
| Layer Manager | Token toolbar widths, tighter filter column and table columns, compact state buttons |
| Hatch Editor | Swatches 56; dropped duplicate in-content title; actions right-aligned |
| Dim / Text / MLeader / Plot / Linetype / Page setup | Shared field helpers; list column 180; persist keys where missing |
| Parameters, Batch plot, Tools, Quick properties, Forms, Commands, Panels float | `ShellFit` persist keys |
| Save prompt, clear history | Shared right-aligned action row (no stretched equal columns) |
| Pomodoro, Calculator | `DialogLayout.Action` instead of ad-hoc buttons |

Retained exceptions: Ask pill, `CommandChrome` overlays, native file pickers, platform title bars (`ExtendsContentIntoTitleBar` stays false).

Remaining: WinUI ComboBox/TextBox templates can still exceed 24 DIP; visual pass of every dialog at 100/150% scaling is manual.

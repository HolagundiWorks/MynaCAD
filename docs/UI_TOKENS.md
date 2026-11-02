# WinUI design tokens

Canonical token system for `apps/aad-winui`. Visual direction: **compact CAD density**
(AutoCAD 2007–inspired chrome sizing) with **Fluent** restraint — graphite/charcoal dark
workspace, neutral off-white light workspace, and the suite accent, Midnight (`docs/BRANDING.md`).

Window templates, components, hierarchy rules and the screen audit: [UI_STANDARDS.md](UI_STANDARDS.md).
Density background, acceptance checklist and exceptions: [COMPACT_CAD_UI.md](COMPACT_CAD_UI.md).

## Resource dictionaries

Merged from `App.xaml`:

| File | Contents |
|---|---|
| `Themes/ColorTokens.xaml` | Semantic brushes + title-bar colours (Light / Default / Dark) |
| `Themes/TypographyTokens.xaml` | Font sizes; default faces Segoe UI / Consolas |
| `Themes/SpacingTokens.xaml` | Compact space scale + paddings |
| `Themes/ControlTokens.xaml` | Heights, icon size, radii, Fluent min overrides |
| `Themes/ControlStyles.xaml` | Icon buttons, menus, status/side-panel chrome, inputs |

Code-built UI uses `Density`. At launch, `ShellFonts.Resolve()` upgrades to **Segoe UI Variable** and
**Cascadia Mono** when those font files are installed (WinUI has no CSS-style `FontFamily` fallbacks).
`ShellTheme.VerifyTokens()` logs any missing semantic brushes to `%TEMP%\aad-winui.log`.

## Colour direction

Ink is Midnight `#102C80` (secondary `#3E56A0`, muted `#546B88`: all at least 4.5 : 1 on the light surfaces); disabled is Midnight at about 40 %. The system accent in `App.xaml` (selection, toggles, focus rings) is Midnight too. Updated 2026-11-02; before that the accent was Windows blue and the ink a slate `#1A2332`.

| Role | Dark | Light |
|---|---|---|
| App / title bar | `#1B1B1B` | `#E4EAF2` |
| Elevated surface | `#2C2C2C` | `#FFFFFF` |
| Accent | `#6F8FF0` (Midnight tint) | `#102C80` (Midnight) |
| Drawing canvas | `#000000` | `#FFFFFF` (CAD preference; not shell chrome) |
| Splash text | `#FFFFFF` on splash background (`SplashForegroundBrush`) | same |
| Selection zebra | Accent at 25% alpha (`Density.SelectionFill`) | Accent at 25% alpha |
| Side panel | `SidePanelWidth` / `Density.SidePanelWidth` = **240** DIP; title rail `SideRailWidth` = **28** | same |
| AI channel edge | Accent (Midnight) | Accent (Midnight) |
| CAD channel edge | Success green | Success green |

**No Mica/acrylic on floating command chrome or the drawing HWND.** Solid fills keep overlays
readable over the canvas.

## Dual command channels (visual)

| Channel | Cue |
|---|---|
| Fixed AI bar | Top accent border + `AiChannelLabelStyle` (“Ask”) |
| Cursor CAD field | Transparent overlay + mono type (`CommandChrome`; no box) |

Behaviour: [COMMAND_BAR.md](COMMAND_BAR.md).

## Compact sizing (DIP)

| Token | Value |
|---|---|
| `MenuBarHeight` / `MenuItemHeight` | 24 / 22 |
| `FileTabHeight` / `StatusBarHeight` / `CommandLineHeight` | 24 / 22 / 22 |
| `ControlHeight` / `DialogButtonHeight` | 26 / 26 (inputs and buttons share a height) |
| `IconButtonSize` / `IconSize` | 26 / 16 |
| `PropertyRowHeight` / `PanelHeaderHeight` | 26 / 24 |
| `CornerRadiusControl` / `CornerRadiusButton` | 4 / 4 |
| `FontSizeCaption` / `FontSizeBody` / `FontSizeSection` | 11 / 12 / 13 |

## Spacing scale

4 px base scale ([UI_STANDARDS.md](UI_STANDARDS.md#tokens)).

| Token | DIP | Use |
|---|---|---|
| `Space1` … `Space6` | 4, 8, 12, 16, 20, 24 | tight, standard, group, section, major, large |
| `SpaceXs` | 2 | icon strips only |
| `SpaceSm` / `SpaceMd` / `SpaceLg` / `SpaceSection` | 4 / 8 / 12 / 16 | older names, aliases of the scale |
| Panel padding | 8×4 (`PaddingPanel`) | side panels, toolbars |
| Dialog padding | 12 (`PaddingDialog` / `Density.DialogMargin`) | dialog content |
| Footer padding | 12×8 (`PaddingFooter`) | pinned dialog footers |

## Intentionally out of scope

- CAD canvas entity colours (host renderer).
- Drawn icon geometry (paths stay the same; display size is 16).
- A permanent large chat panel (would steal drawing space).

## Related

- [UI_STANDARDS.md](UI_STANDARDS.md) — window templates, components, hierarchy, screen audit
- [COMPACT_CAD_UI.md](COMPACT_CAD_UI.md) — density background, checklist, exceptions
- [WINDOW_SIZING.md](WINDOW_SIZING.md) — content-fit + persisted tool window sizes
- [COMMAND_BAR.md](COMMAND_BAR.md)
- [UI_UX.md](UI_UX.md)

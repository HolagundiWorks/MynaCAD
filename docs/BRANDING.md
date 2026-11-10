# Brand guide: the MynaHub suite

**Canonical copy:** MynaHub `docs/BRANDING.md`. MynaOps and MynaCAD keep an identical copy; **O-UI** (`o-ui`) holds the artwork, the colour files and the tool that draws them.
**Names and terms:** [SUITE.md](SUITE.md). **Written 2026-11-01.**

The idea: **one family name, Myna, and a use-case word for each app** (MynaHub, MynaCAD, MynaOps, MynaMod; decided 2026-11-10 after the user survey). Marks: the three-circle suite mark in each app's colour. The **orb** is the **AI agent's** mark and wears all four colours; it appears inside MynaOps and MynaCAD, where the agent works. **Myna** is the suite's name; since 2026-11-11 there is no app called Myna (the launcher was removed).

## 1. The palette

From the suite's palette card. These four are the only brand colours.

| Name | HEX | RGB | CMYK | Belongs to |
|---|---|---|---|---|
| **Midnight** | `#102C80` | 16, 44, 128 | 88, 66, 0, 50 | **MynaCAD** |
| **Ember** | `#B83E16` | 184, 62, 22 | 0, 66, 88, 28 | **MynaHub** |
| **Grove** | `#166040` | 22, 96, 64 | 77, 0, 33, 62 | **MynaMod** (planned) |
| **Dusk** | `#5630A0` | 86, 48, 160 | 46, 70, 0, 37 | **MynaOps** |

Suite ink (text): **Midnight** `#102C80` on light surfaces; white `#FFFFFF` on dark. Supporting: Midnight soft `#3E56A0`; hover and pressed shades of each accent are in `o-ui` `Themes/SuiteAccent.*.xaml`; coloured *text* on white uses `SuiteAccentText` (per-app derivatives at ≥ 4.5 : 1 on white).

### Contrast

All four Light Conditions accents are dark enough for **white** text and glyphs.

| Pair | Ratio | Use |
|---|---|---|
| White on Midnight | 12.2 : 1 | text and glyph on MynaCAD's tile and dark surfaces |
| White on Dusk | 9.1 : 1 | text and glyph on MynaOps's tile |
| White on Grove | 7.6 : 1 | text and glyph on MynaMod's tile |
| White on Ember | 5.6 : 1 | text and glyph on MynaHub's tile (AA ✓) |

### Themes (2026-11-10)

Every app has two themes, **pure white** and **pure black**, and the person chooses (or follows Windows). The neutrals are not tinted: light is `#FFFFFF` surfaces with `#000000` text, dark is `#000000` surfaces with `#FFFFFF` text. Regions are told apart by edge lines, not by a change of grey. Secondary text is `#4D4D4D` on white and `#B8B8B8` on black; control edges are `#767676` on white and `#8C8C8C` on black (both over 3:1, WCAG 1.4.11); hover is `#F0F0F0` / `#1F1F1F`. The **app colour** (Ember, Midnight, Dusk, Grove) stays for fills, marks, focus and selection; in dark it is the lighter tint in `SuiteAccent.<App>.Dark.xaml`. The drawing canvas of MynaCAD is white or black on its own switch. Defined in `o-ui` `O.Ui/Themes/SuiteColors.xaml` (light) and `SuiteColors.Dark.xaml`; MynaCAD's `Themes/ColorTokens.xaml` and the egui shell (`crates/ui-egui/src/theme.rs`) carry the same values.

## 2. The mark

**One mark, three circles.** The suite mark is a half ring open to the right (the largest circle), a smaller half ring below the centre, and a small ring at the centre, all white. Every app wears it on a rounded tile in **that app's colour** (corner radius 42 of 194, about 21.5 %). The **orb** (the same mark lit by all four app colours, section 3) belongs to the **AI agent**, not to an app; the launcher that wore it until 2026-11-11 is gone. Redrawn 2026-11-10 from the pre-bird marks; the bird marks of 2026-11-07 are retired. Drawn by `o-ui` `tools/Brand` (`Program.cs`); MynaCAD's `xtask` logo command draws the same curves.

| App | Tile |
|---|---|
| MynaHub | Ember |
| MynaCAD | Midnight |
| MynaMod | Grove |
| MynaOps | Dusk |
| the AI agent (in MynaOps and MynaCAD) | the orb (all four colours) |

Rules
- The mark is not redrawn, rotated or given a colour outside its row. White on these tiles meets the contrast table above.
- Clear space round a tile: a quarter of its width. Smallest size: 16 px (the ICO has 16, 24, 32, 48, 64, 128 and 256 px).
- On a plain ground without a tile (a wide splash), the mark is white on a field of the same colour.
- Never put a second logo, a shadow or a gradient on an app tile.

### Files

`o-ui/brand/<app>/` for `studio` (MynaHub), `draft` (MynaCAD), `form` (MynaMod), `coreop` (MynaOps), and `launcher` (the retired launcher; kept as artwork only): `mark.svg`, `mark.png` (1024), `mark-256.png`, `app.ico`, `lockup.png`, and `tiles/` (the Windows tile and store sizes). `brand/brand-sheet.png` is the one-page card. Redraw everything with `dotnet run --project tools/Brand -- brand` in `o-ui`. Apps copy what they need: MynaHub `native/Adms.Desktop/Assets` (its mark and the marks of the apps it opens; `myna.ico` was removed with the launcher), MynaOps `BBSDesktop/BBSApp/Assets`, MynaCAD `assets/app-icon`, MynaMod `scripts/startup/bl_app_templates_system/oform`.

## 3. The AI agent

The AI agent has its own symbol, drawn in the design file `o-ui/brand/ai/ai-orb.html` (the animated master): a **dark sphere lit by four soft lobes**, one in each app colour (Ember upper left, Dusk upper right, Midnight lower right, Grove lower left), the **suite mark in white** with round ends on it, a halo of the same four colours behind, and **four small particles** orbiting. It wears all four app colours because it is the suite's agent. Since 2026-11-11 it is hosted by MynaOps and works in MynaOps and MynaCAD; MynaHub has no AI and does not show the orb.

| State | Look |
|---|---|
| **Running** | The sphere breathes (7.5 s, 5 %), the four halos swell and fade on their own beats (5.2, 6.8, 7.4 and 5.9 s), the white mark glows (4.8 s), and the particles orbit: Ember `#E05A28` in 11 s, Dusk `#7858D0` in 16 s (the other way), Midnight `#2858B8` in 20 s, Grove `#22A870` in 26 s (the other way) |
| **Waiting** (starting) | Still, at 60 % |
| **Not running / off** | Still, at 30 % |

With "reduce motion" on, the design file stops all animation; an app shows the still orb then.

Files: `o-ui/brand/ai/ai-orb.html` (design file, animated), `ai-orb.svg` and `ai-orb-<size>.png` (first frame). In the apps it is drawn by `O.Ui` (`SuiteDrawnIcon` kind `agent`, in MynaOps' and MynaCAD's bottom bar beside the Ask bar). The launcher's animated `AiOrb` and MynaHub's assistant bar were removed on 2026-11-11.

The company file uses a plain icon, not the orb.

## 4. Type

| Use | Typeface | Source and licence |
|---|---|---|
| **The names of the apps and the suite** (MynaHub, MynaOps, MynaCAD, MynaMod, in tiles, headers, lockups) | **Apfel Grotezk** | Collletttivo, free (SIL Open Font Licence). Chosen on 2026-11-01 as the free grotesk; it replaces the commercial Studio Grotesk (demo personal-use only), which can be swapped in later by changing one constant in `O.Ui.SuiteFonts` |
| **Display text** (page and section titles, large numbers) | **Mazius Display** | Collletttivo, free (SIL Open Font Licence). Written "Maius Display" in the brief; taken to be this font |
| Body text and controls | Segoe UI Variable | the Windows system font |

The files (`ApfelGrotezk-Regular/Mittel/Fett.otf`, `MaziusDisplay-Regular/Bold.otf`, with their licence texts) are bundled in `o-ui` `O.Ui/Assets/Fonts` and copied beside every app that references O.Ui; `O.Ui.SuiteFonts.Names` and `.Display` load them from `Assets\Fonts`, else an installed font of the same name, else the fallbacks (Segoe UI Variable Display, Sitka Display). The brand sheet is drawn in Segoe UI.

Rules: the user-visible names are MynaHub, MynaOps, MynaCAD and MynaMod (and Myna for the suite as a whole), spelled exactly that way, in the Names font; never in capitals; never AQC, AOS, AAD, ShilpiDB or OSAS in user text. Install folders and ticket ids keep the earlier names.

## 5. Using the colours in the apps

- Surfaces are neutral (white, soft grey-blue); the app colour is for the **primary action, the selection and the tile**, not for large areas.
- Ink is Midnight `#102C80` everywhere. Disabled is Midnight at 40 %.
- One accent per window: the colour of the app the window belongs to. Tiles that open another app (MynaHub's bottom bar) carry that app's mark.
- Status colours are separate from the brand and keep their meaning: green running, amber waiting, red not running, grey off. They are used only for service state.

## 6. What is done and what is not (2026-11-01)

| Part | State |
|---|---|
| Light Conditions palette (Ember / Midnight / Grove / Dusk), WCAG 2.2 AA audit | done |
| The four marks, ICO and tiles, lockups, brand sheet | done: `o-ui` `master` |
| Accent colour files per app (`SuiteAccent.Orange / Mirage / Sea / Sand`, values updated to Light Conditions) | done |
| `SuiteFonts` (Names, Display with fallbacks), `SuiteApp.Form` | done; the font files are not added |
| AI orb — four-colour sphere from the design file (`o-ui/brand/ai/ai-orb.html`) | done |
| Myna launcher: marks in tiles, MynaMod tile, Ember accent, animated AI orb | removed with the launcher (2026-11-11) |
| MynaHub: marks and `app.ico` | done; its own pages take Ember from `SuiteAccent.Orange` automatically |
| MynaOps: Dusk accent, tiles and `app.ico` | done |
| MynaCAD: app icon files (`assets/app-icon`) | done; window chrome on the Midnight accent (2026-11-02) |
| MynaMod | the mark and the colour exist; the app does not |
| Windows installer and shortcut icons from the new `.ico` | follow the next installer build |

# Brand guide: the O-Studio suite

**Canonical copy:** O-Studio `docs/BRANDING.md`. O-CoreOp and O-Draft keep an identical copy; **O-UI** (`o-ui`) holds the artwork, the colour files and the tool that draws them.
**Names and terms:** [SUITE.md](SUITE.md). **Written 2026-11-01.**

The idea: **one mark, four colours.** Every app wears the same mark. Each app has one colour of the palette. The **AI agent** wears **gold**.

## 1. The palette

From the suite's palette card. These four are the only brand colours.

| Name | HEX | RGB | CMYK | Belongs to |
|---|---|---|---|---|
| **Midnight** | `#102C80` | 16, 44, 128 | 88, 66, 0, 50 | **O-Draft** |
| **Ember** | `#B83E16` | 184, 62, 22 | 0, 66, 88, 28 | **O-Studio** |
| **Grove** | `#166040` | 22, 96, 64 | 77, 0, 33, 62 | **O-Form** (planned) |
| **Dusk** | `#5630A0` | 86, 48, 160 | 46, 70, 0, 37 | **O-CoreOp** |

Suite ink (text): **Midnight** `#102C80` on light surfaces; white `#FFFFFF` on dark. Supporting: Midnight soft `#3E56A0`; hover and pressed shades of each accent are in `o-ui` `Themes/SuiteAccent.*.xaml`; coloured *text* on white uses `SuiteAccentText` (per-app derivatives at ≥ 4.5 : 1 on white).

### Contrast

All four Light Conditions accents are dark enough for **white** text and glyphs.

| Pair | Ratio | Use |
|---|---|---|
| White on Midnight | 12.2 : 1 | text and glyph on Draft's tile and dark surfaces |
| White on Dusk | 9.1 : 1 | text and glyph on CoreOp's tile |
| White on Grove | 7.6 : 1 | text and glyph on Form's tile |
| White on Ember | 5.6 : 1 | text and glyph on Studio's tile (AA ✓) |
| Gold text `#9B6000` on white | 4.7 : 1 | AI agent accent text on white (AA ✓) |

## 2. The mark

The same in every app: three strokes round one centre on a 194 x 194 grid, centred at (107, 96):

| Stroke | Radius | Width |
|---|---|---|
| The big C: a half ring open to the right | 69 | 18 |
| The smaller bowl below the centre | 50 | 12 |
| The ring at the centre | 18.5 | 5 |

On a rounded tile (corner radius 42 of 194, about 21.5 %).

| App | Tile | Strokes |
|---|---|---|
| O-Studio | Ember | white |
| O-Draft | Midnight | white |
| O-Form | Grove | white |
| O-CoreOp | Dusk | white |

Rules
- The mark is never redrawn, rotated, outlined or recoloured outside the table. All four Light Conditions accents are dark enough for white strokes.
- Clear space round a tile: a quarter of its width. Smallest size: 16 px (the ICO has 16, 24, 32, 48, 64, 128 and 256 px).
- On a plain ground without a tile (a wide splash), use the strokes alone in white.
- Never put a second logo, a shadow or a gradient on the tile.

### Files

`o-ui/brand/<app>/` for `studio`, `draft`, `form`, `coreop`: `mark.svg`, `mark.png` (1024), `mark-256.png`, `app.ico`, `lockup.png`, and `tiles/` (the Windows tile and store sizes). `brand/brand-sheet.png` is the one-page card. Redraw everything with `dotnet run --project tools/Brand -- brand` in `o-ui`. Apps copy what they need: O-Studio `native/Adms.Desktop/Assets` (marks of all apps for the launcher), O-CoreOp `BBSDesktop/BBSApp/Assets`, O-Draft `assets/app-icon`.

## 3. The AI agent

The AI agent is the **suite logo on a molten-gold tile**. Its three circles (the big C, the bowl under it and the ring at the centre) are drawn in **white** on a gold radial-gradient disc — a distinct identity separate from the four app colours.

| State | Look |
|---|---|
| **Running** | White ring strokes on a pulsing gold disc; a warm gold halo breathes every four seconds. Gold particles orbit the disc in four variants: #F0C840, #D4980E, #E8B030, #C88010 |
| **Waiting** (starting) | Ring strokes at half white opacity, disc at 60 % gold |
| **Not running / off** | Disc and strokes at 30 % — nearly dark |

The gold gradient (radial, upper-left highlight):

| Stop | Colour | Offset |
|---|---|---|
| Bright highlight | `#F0C840` | 0 % |
| Mid gold | `#D4980E` | 35 % |
| Deep gold | `#9B6000` | 65 % |
| Dark amber | `#6B4000` | 100 % |

Files: `o-ui/brand/ai/ai-orb.svg` (static mark, animated master), `ai-orb-<size>.png` (still). In the launcher, the app bar and the Services page.

The database server and the company file use plain icons, not the orb: a database icon and a folder icon, lit (accent wash) while they run and dim grey when they do not.

## 4. Type

| Use | Typeface | Source and licence |
|---|---|---|
| **The names of the apps and the suite** (O-Studio, O-CoreOp, O-Draft, O-Form, in tiles, headers, lockups) | **Apfel Grotezk** | Collletttivo, free (SIL Open Font Licence). Chosen on 2026-11-01 as the free grotesk; it replaces the commercial Studio Grotesk (demo personal-use only), which can be swapped in later by changing one constant in `O.Ui.SuiteFonts` |
| **Display text** (page and section titles, large numbers) | **Mazius Display** | Collletttivo, free (SIL Open Font Licence). Written "Maius Display" in the brief; taken to be this font |
| Body text and controls | Segoe UI Variable | the Windows system font |

The files (`ApfelGrotezk-Regular/Mittel/Fett.otf`, `MaziusDisplay-Regular/Bold.otf`, with their licence texts) are bundled in `o-ui` `O.Ui/Assets/Fonts` and copied beside every app that references O.Ui; `O.Ui.SuiteFonts.Names` and `.Display` load them from `Assets\Fonts`, else an installed font of the same name, else the fallbacks (Segoe UI Variable Display, Sitka Display). The brand sheet is drawn in Segoe UI.

Rules: the name of an app is always written with a capital O and a hyphen (O-CoreOp, not OCoreOp, not "CoreOp" alone in the UI) and always in the Names font; never in capitals; never AQC, AOS, AAD, ShilpiDB or OSAS in user text.

## 5. Using the colours in the apps

- Surfaces are neutral (white, soft grey-blue); the app colour is for the **primary action, the selection and the tile**, not for large areas.
- Ink is Midnight `#102C80` everywhere. Disabled is Midnight at 40 %.
- One accent per window: the colour of the app the window belongs to. The launcher belongs to O-Studio (Ember); the app tiles in it carry their own app's mark.
- Status colours are separate from the brand and keep their meaning: green running, amber waiting, red not running, grey off. They are used only for service state.

## 6. What is done and what is not (2026-11-01)

| Part | State |
|---|---|
| Light Conditions palette (Ember / Midnight / Grove / Dusk), WCAG 2.2 AA audit | done |
| The four marks, ICO and tiles, lockups, brand sheet | done: `o-ui` `feat/brand` |
| Accent colour files per app (`SuiteAccent.Orange / Mirage / Sea / Sand`, values updated to Light Conditions) | done |
| `SuiteFonts` (Names, Display with fallbacks), `SuiteApp.Form` | done; the font files are not added |
| AI orb — molten-gold identity (`o-ui/brand/ai/ai-orb.svg`) | done |
| Launcher: marks in tiles, O-Form tile, Ember accent, animated AI orb, Names font for app names, Display font for titles | done |
| O-Studio: marks and `app.ico` | done; its own pages take Ember from `SuiteAccent.Orange` automatically |
| O-CoreOp: Dusk accent, tiles and `app.ico` | done |
| O-Draft: app icon files (`assets/app-icon`) | done; window chrome not yet on Midnight accent |
| O-Form | the mark and the colour exist; the app does not |
| Windows installer and shortcut icons from the new `.ico` | follow the next installer build |

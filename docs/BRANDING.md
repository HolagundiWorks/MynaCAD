# Brand guide: the O-Studio suite

**Canonical copy:** O-Studio `docs/BRANDING.md`. O-CoreOp and O-Draft keep an identical copy; **O-UI** (`o-ui`) holds the artwork, the colour files and the tool that draws them.
**Names and terms:** [SUITE.md](SUITE.md). **Written 2026-11-01.**

The idea: **one mark, four colours.** Every app wears the same mark. Each app has one colour of the palette. The **AI agent** wears all four.

## 1. The palette

From the suite's palette card. These four are the only brand colours.

| Name | HEX | RGB | CMYK | Belongs to |
|---|---|---|---|---|
| **Dark Slate Blue** (called *Mirage*) | `#16232B` | 22, 35, 43 | 49, 19, 0, 83 | **O-Draft**. Also the ink (text) of the whole suite |
| **Flame Orange** | `#FF5B04` | 255, 91, 4 | 0, 64, 98, 0 | **O-Studio** |
| **Deep Sea Green** | `#075056` | 7, 80, 86 | 92, 7, 0, 66 | **O-Form** (planned) |
| **Wild Sand** | `#E4EEF0` | 228, 238, 240 | 5, 1, 0, 6 | **O-CoreOp** |

Supporting values (not brand colours): Mirage soft ink `#4A5A63`; hover and pressed shades of each accent are in `o-ui` `Themes/SuiteAccent.*.xaml`; coloured *text* on white uses `SuiteAccentText` (Flame `#A83A00`, Wild Sand `#3B6670`).

### Contrast

| Pair | Ratio | Use |
|---|---|---|
| White on Mirage | 16.0 | text and glyph on Draft's tile and dark surfaces |
| Mirage on Wild Sand | 13.6 | text on CoreOp's colour; the CoreOp glyph |
| White on Deep Sea Green | 9.2 | Form |
| Mirage on Flame Orange | 5.2 | **text on an orange button** (white on orange is only 3.1: use white for the large glyph, never for small text) |
| Flame text `#A83A00` on white | 6.4 | orange text |
| Teal text `#3B6670` on white / on Wild Sand | 6.3 / 5.4 | text in CoreOp's colour |
| Wild Sand on Mirage, Flame on Mirage | 13.6, 5.2 | the orb's tones on its disc |

Wild Sand is a very light colour. A button filled with it must have a visible edge (CoreOp's primary button has a 1 px edge `#B3C6CB`), and coloured *text* in CoreOp uses the teal above, not Wild Sand itself.

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
| O-Studio | Flame Orange | white |
| O-Draft | Mirage | white |
| O-Form | Deep Sea Green | white |
| O-CoreOp | Wild Sand | Mirage |

Rules
- The mark is never redrawn, rotated, outlined or recoloured outside the table. Do not put the orange mark on a Flame Orange ground, or the sand mark on white without its tile.
- Clear space round a tile: a quarter of its width. Smallest size: 16 px (the ICO has 16, 24, 32, 48, 64, 128 and 256 px).
- On a plain ground without a tile (a wide splash), use the strokes alone in the tile's stroke colour.
- Never put a second logo, a shadow or a gradient on the tile.

### Files

`o-ui/brand/<app>/` for `studio`, `draft`, `form`, `coreop`: `mark.svg`, `mark.png` (1024), `mark-256.png`, `app.ico`, `lockup.png`, and `tiles/` (the Windows tile and store sizes). `brand/brand-sheet.png` is the one-page card. Redraw everything with `dotnet run --project tools/Brand -- brand` in `o-ui`. Apps copy what they need: O-Studio `native/Adms.Desktop/Assets` (marks of all apps for the launcher), O-CoreOp `BBSDesktop/BBSApp/Assets`, O-Draft `assets/app-icon`.

## 3. The AI agent

The AI agent is **the logo itself** on a round Mirage tile. Its **three circles** (the big C, the bowl under it and the ring at the centre) are each lit in a different colour of the suite, so the agent wears all four colours (Mirage is the tile).

| State | Look |
|---|---|
| **Running** | The three circles pass the colours on: every two seconds each circle takes the colour of the next one, eased, so at any moment the three circles show three of the four colours and in eight seconds every circle has worn all four. A fainter, wider copy behind each circle is the glow, and it breathes every four seconds. On the dark tile the tones are lifted so that they can be seen: Flame Orange `#FF5B04`, Wild Sand `#E4EEF0`, Deep Sea Green `#1496A0`, Mirage `#4F7C93` |
| **Waiting** (starting) | The three circles are one steady amber (`#D29922`) with a faint glow |
| **Not running / off** | The three circles are grey (`#8B949E`), no glow, the whole orb at half strength |

At rest (the still artwork) the C is Flame Orange, the bowl Wild Sand and the ring Deep Sea Green on the Mirage tile. The mark is drawn at 74 % of the tile so that the glow has room. The orb is round, never on a square tile.

Files: `o-ui/brand/ai/ai-orb.svg` (the animated master: each circle's colour cycles through the four tones in eight seconds, offset by one step from the circle before), `ai-orb-<size>.png` (still). In the launcher it is `AiOrb` (`native/Adms.LoginWidget/AiOrb.cs`), in the app bar and on the Services page.

The database server and the company file use plain icons, not the orb: a database icon and a folder icon, lit (orange wash) while they run and dim grey when they do not.

## 4. Type

| Use | Typeface | Source and licence |
|---|---|---|
| **The names of the apps and the suite** (O-Studio, O-CoreOp, O-Draft, O-Form, in tiles, headers, lockups) | **Studio Grotesk** | Jetsmax Studio. A commercial font: the free demo is for personal use only; a licence for the family is bought from the publisher (MyFonts / Creative Market) |
| **Display text** (page and section titles, large numbers) | **Mazius Display** | Collletttivo, 2018, free (SIL Open Font Licence). Written "Maius Display" in the brief: confirm that this is the font meant |
| Body text and controls | Segoe UI Variable | the Windows system font |

The files are **not** in the repositories. Put `StudioGrotesk-*.otf` / `.ttf` and `MaziusDisplay-*.otf` / `.ttf` in `Assets\Fonts` beside each app; `O.Ui.SuiteFonts.Names` and `.Display` use them from there, or the installed fonts of the same name, or the fallbacks (Segoe UI Variable Display for names, Sitka Display for display), so an app always starts. Until the files are added every screen shows the fallbacks. The brand sheet is set in Segoe UI for the same reason.

Rules: the name of an app is always written with a capital O and a hyphen (O-CoreOp, not OCoreOp, not "CoreOp" alone in the UI) and always in the Names font; never in capitals; never AQC, AOS, AAD, ShilpiDB or OSAS in user text.

## 5. Using the colours in the apps

- Surfaces are neutral (white, soft grey-blue); the app colour is for the **primary action, the selection and the tile**, not for large areas.
- Ink is Mirage everywhere. Disabled is Mirage at 40 %.
- One accent per window: the colour of the app the window belongs to. The launcher belongs to O-Studio (orange); the app tiles in it carry their own app's mark.
- Status colours are separate from the brand and keep their meaning: green running, amber waiting, red not running, grey off. They are used only for service state.

## 6. What is done and what is not (2026-11-01)

| Part | State |
|---|---|
| Palette, the four marks, ICO and tiles, lockups, brand sheet, the AI orb artwork (the logo, three circles in three colours) | done: `o-ui` `feat/brand` |
| Accent colour files per app (`SuiteAccent.Orange / Mirage / Sea / Sand`, with light, dark and navigation themes) | done |
| `SuiteFonts` (Names, Display with fallbacks), `SuiteApp.Form` | done; the font files are not added |
| Launcher: marks in tiles, O-Form tile, orange accent, animated AI orb, Names font for app names, Display font for titles | done |
| O-Studio: marks and `app.ico` | done; its own pages still take the orange from `SuiteAccent.Orange` automatically |
| O-CoreOp: Wild Sand accent, tiles and `app.ico` | done |
| O-Draft: app icon files (`assets/app-icon`) | done; the window chrome keeps its own colour tokens and is **not** yet on the Mirage accent |
| O-Form | the mark and the colour exist; the app does not |
| Windows installer and shortcut icons from the new `.ico` | follow the next installer build |

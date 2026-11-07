# Typography: font presets, roles and migration

Five coordinated font presets, a role system that the interface and drawings resolve through, and a
migration of a drawing's text styles with a preview. This page separates what exists from what is
planned ([status.md](status.md)).

## How fonts work in this repository (audit)

| Area | Finding |
|---|---|
| Rule | Font files are never committed (CLAUDE.md). They are an optional build and packaging input; code and tests work without them |
| Drawing text | `crates/fonts`: the built-in single-stroke font, or TrueType/OpenType outlines found at run time by file stem or family name (`ttf.rs`), with per-character fallback. `TextStyle.font` holds a file or family name. SHX files are not read (the stroke font or `FONTALT` is used). WOFF2 is not read |
| Interface | `theme::install_fonts`: egui's own font plus one system font (Segoe UI, Arial, SF, DejaVu) |
| Packaging | The MSI and portable zip carried no fonts |

## What is built

- **Registry** (`crates/fonts/src/registry.rs`): the five presets with their Sans, Condensed and
  Mono families, licence and upstream records and the notes about what must be verified before
  bundling; the seven roles (Primary, Condensed, Monospace, Heading, Dimension, Schedule,
  Nameplate) mapped to a face and a weight (400 to 600, nothing lighter than Regular for small
  CAD text); `Typography { preset, overrides }`; `resolve(role)` with a fixed fallback order
  (the role's face, then Sans, Condensed, Mono, then the built-in stroke font) that reports
  `substituted` instead of hiding it; `missing_glyphs(family, text)`.
- **Presets**: Engineering (IBM Plex Sans, Sans Condensed, Mono; the default), Architectural
  Editorial (Source Sans 3, Source Code Pro, optional Source Serif 4; no dedicated condensed face,
  so Condensed uses Source Sans 3 and the preset says so), Multilingual Documentation (Noto Sans,
  Noto Sans Condensed, Noto Sans Mono), Compact Interface (Roboto, Roboto Condensed, Roboto Mono),
  Drafting Utility (Fira Sans, Fira Sans Condensed, Fira Mono). Licences other than IBM Plex are
  marked "verify": each release's licence files are checked before packaging.
- **Where fonts are found**: `ttf.rs` searches `AAD_FONT_DIR`, `<exe folder>\fonts` and
  `%APPDATA%\O-Draft\fonts` before the system folders, so fonts placed by the installer or the user win.
- **Interface**: `UiState.font_preset` (saved in the workspace file, an unknown value falls back to
  the default). The preset's Sans is the Proportional font, Mono is the Monospace font (command
  bar, values) and Condensed is the egui family `aad-condensed`; a system font stays behind them for
  missing characters. Changing the preset rebuilds the fonts on the next frame; the layout does not
  move. View > Font Preset and `ui.fonts {preset?}` set and report it.
- **Commands** (`crates/engine/src/cmd/typography.rs`): `fonts.presets` (presets, roles, which
  families are installed), `fonts.resolve {preset}`, `fonts.scan` (the drawing's text styles, where
  each is used, whether its font is installed, the role it most likely has, and the styles whose
  purpose cannot be told), `fonts.migrate {preset, roles?, apply?}` (preview, then apply).
- **Migration rules**: only a text style's `font` changes; height, width factor, obliquing and the
  flags are kept, and no entity is touched. Unclassified styles are skipped until a role is given.
  A font that is not installed is never replaced by a different one: the style is skipped with the
  reason. Characters the new font lacks are warned about. Applying is one undo step.
- **Installer**: set `AAD_FONTS_DIR` to a folder with the `.ttf`/`.otf` files and their licence
  texts and `package.ps1` puts them in `<install folder>\fonts` (MSI and portable zip). It refuses
  a folder without fonts or without a licence file. Without the variable the installer carries no
  fonts and the interface keeps the system font until fonts are added.

Tests: `crates/fonts/src/registry.rs`, `crates/engine/tests/typography.rs`,
`crates/ui-egui/tests/fonts.rs`, `apps/aad/src/workspace.rs`.

## IBM Plex assets (Engineering preset)

The font files are obtained from the official IBM Plex releases and kept outside the repository
(CLAUDE.md: fonts are never committed). They were fetched with `gh release download -R IBM/plex`:

| Release | File |
|---|---|
| `@ibm/plex-sans@1.1.0` | `ibm-plex-sans.zip` |
| `@ibm/plex-sans-condensed@2.0.0` | `ibm-plex-sans-condensed.zip` |
| `@ibm/plex-mono@2.5.0` | `ibm-plex-mono.zip` |

Only the upright faces the roles use are kept (Sans Regular, Medium, SemiBold, Bold; Sans Condensed
Regular, Medium, SemiBold; Mono Regular, Medium), with each family's licence (SIL OFL 1.1,
copyright IBM Corp., Reserved Font Name "Plex") as `OFL-<family>.txt` and a `SOURCES.txt` recording
the release tags, the archive hashes and each font file's size and SHA-256. `AAD_FONTS_DIR` points
at that folder. A development run searches `AAD_FONTS_DIR` and `AAD_FONT_DIR` first, then
`<exe folder>\fonts`, `%APPDATA%\O-Draft\fonts` and the system folders; `package.ps1` copies the files,
licences and `SOURCES.txt` into the MSI and the portable zip, and refuses a folder with no font
files or no licence file.

Verified: with the fonts, `fonts.resolve engineering` gives IBM Plex Sans, Sans Condensed and Mono
with nothing substituted and every weight installed, and the interface renders IBM Plex in Classic
and Pro; without them every role reports `substituted: true` and `weightInstalled: false` and the
interface keeps the system font. The MSI built with fonts lists the 9 font files and 3 licences in
its File table, and the one built without lists neither; the portable zip built with fonts, run from
an empty environment, resolves the Engineering preset to its own `fonts` folder. A release is said
to include the fonts only when `package.ps1` printed `fonts: N file(s)` for that build.

## Added after the first version

- **Weights**: `registry::find_weighted` finds Medium, SemiBold and Bold as their own faces
  (`IBM Plex Sans SemiBold`, `IBMPlexSans-SemiBold`); `Resolved.weight_installed` says whether the
  role's weight exists, and a role whose weight is missing is drawn at Regular and reported. The
  interface Heading family (`theme::HEADING_FAMILY`) uses the Sans face at the Heading weight.
- **Role use in the interface**: `theme::small()` (compact text: toolbar labels, metadata) uses the
  Condensed family and `egui::TextStyle::Heading` uses the Heading family. They are switched on the
  frame after the fonts are installed (egui applies a new font set at the start of the next frame),
  and the named families always exist, falling back to the Proportional chain.
- **Project typography**: `ProjectConfig.typography { preset, overrides: [{role, face, weight}] }`
  (validated: known preset and roles, face sans/condensed/mono, weight 400 to 700). A generated
  drawing gets one text style per role (`AAD-Primary`, `AAD-Heading`, `AAD-Monospace` ...) naming the
  preset's family at the role's weight (the intended font, whether or not it is installed here),
  `Standard` and the leader styles on the Primary font and dimension styles on the Dimension style.
  A project copies the typography when it is created; changing the application default later does
  not change it. The application-wide default is the interface preset (`ui.fonts`, View > Font
  Preset), which new project configurations start from.
- **Project-wide migration**: `fonts.migrate {preset, root, apply}` previews and applies over the
  project's drawings (from `project.json`). Applying backs each changed drawing up to
  `backups/fonts-NNN/`, saves it, and writes `report.json` with the hash of what was written.
  `fonts.rollback {root, id}` restores those drawings byte for byte and leaves alone any drawing
  edited since. The styles Tren creates for roles are recognised by name.
- **Clipping and overlap**: for single-line text the migration measures the width in the old and the
  new font and lists text that gets more than 5% wider (`growth`: handle, text, both widths, and the
  neighbours on the same baseline it now reaches). MTEXT, rotated or non-left-aligned text,
  attributes and table cells are not measured.
- **Round trip**: `apps/aad-cli/tests/typography_project.rs` generates a project, migrates it,
  reopens the saved DXF and checks the fonts, then rolls back.

## Not built yet

| Item | State |
|---|---|
| Other presets (Source, Noto, Roboto, Fira) | Defined and selectable; their files and licences are unverified until obtained the same way |
| Measuring more text | MTEXT wrapping, rotated or centred text, block attributes, dimension text and table cells are not measured for growth |
| DWG | Not tested: this build writes DXF. External CAD software may substitute the fonts; the migration only reports what Tren itself can see |
| Saved migration report beyond JSON | The report is a JSON file; there is no report viewer |
| Template editor | Typography is part of the project configuration; there is no template editor UI for drawing-set font roles yet |

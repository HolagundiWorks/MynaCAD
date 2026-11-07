# WCAG and UI audit of the WinUI shell

Audit of `apps/aad-winui` as built on 2026-10-18, against WCAG 2.2 level AA (1.4.3 text contrast,
1.4.11 non-text contrast, 2.1.1 keyboard, 2.4.7 focus visible, 4.1.2 name and role, 2.5.8 target size)
and for plain interface faults. Only what was measured or seen is listed as a finding.

## Method

| Check | How |
|---|---|
| Names, roles, focusability, target sizes | Windows UI Automation tree of the running window (118 elements, sample drawing, dark theme) read with `System.Windows.Automation` |
| Controls built in code without a name | Source scan of every `*.cs` for text boxes, combo boxes, sliders, toggles and check boxes created without `AutomationProperties`, a header, a placeholder or content |
| Text contrast | Contrast ratios computed from the hard-coded colours (WCAG relative luminance) |
| Appearance in both themes | Window captured with `PrintWindow` (dark, then light through the theme button's Invoke pattern) |

## Findings and fixes

| # | Finding | Criterion | State |
|---|---|---|---|
| 1 | In the light theme the Properties panel labels (No selection, Color, Layer, Linetype, Lineweight) were nearly invisible: they took the secondary-text brush from the application's theme (dark), not the window's | 1.4.3 | **Fixed.** `Density.ThemeBrush` looks the brush up in the theme the shell is showing (searching the merged WinUI dictionaries). The panels are rebuilt when the theme changes. Same fix for the canvas menu, block selection and point-style window |
| 2 | Splash subtitle (90 % opacity, 4.2:1) and licence line (75 %, 3.4:1) on the blue splash | 1.4.3 | **Fixed in the working tree**: opacity removed (white on the splash blue is 4.8:1). Not committed: the splash markup belongs to the uncommitted `MainWindow.xaml` of the other session |
| 3 | Dark theme: inactive title-bar text `#9A9A9A` on the pressed-button fill was 4.0:1 | 1.4.3 | **Fixed**: `#AAAAAA` (4.9:1) |
| 4 | The command bar was half-transparent acrylic (50 % tint) over the drawing, so its text contrast depended on the drawing underneath | 1.4.3 | **Fixed**: AI bar, cursor CAD chrome, and secondary tool windows use solid fills via `ShellChrome` (`SystemBackdrop` cleared; opaque dark/light page + title bar). Contrast no longer depends on the drawing underneath |
| 5 | Two marks drawn inside tool icons (the `1.5` on Dimension, the `A` on Text) were exposed to assistive technology as separate text elements | 4.1.2 | **Fixed**: hidden from the accessibility view; the buttons keep their names |
| 6 | The side panel opened 520 px wide whenever the layer manager was merged into it, about 38 % of a 1400 px window | UI | **Fixed**: a third of the window, between 380 and 520 px |

## Passed

- Every visible interactive element in the tree has a name (buttons, menu items, tabs, combo boxes, the command line).
- No control created in code lacks a name, header, placeholder or content (source scan).
- Targets: toolbar, status and panel buttons are 32 px; the title-bar buttons 46 x 31; the tab close button is 32 x 24 and not a tab stop (Ctrl+W closes a tab).
- Dark and light body text: 16.3:1 and 15.7:1; inactive text 5.8:1 and 6.0:1.
- The window announces command prompts through a live region (`PromptLive`).

## Left as is, with reasons

- The side-panel resize grip is 8 px wide. It is also a keyboard control (focusable, Left and Right resize it, named "Resize side panel, width N"), which meets 2.5.8's equivalent-control exception.
- The Space combo box is disabled (not focusable) when the drawing defines no spaces; a disabled control is exempt from contrast and focus.
- The Pro mode edge strip and the floating windows were not examined.

## Not tested

Narrator or any screen reader; Windows high-contrast themes; 200 % text scaling and reflow at 320 px; a keyboard-only walk through every dialog; the light-theme command bar; contrast of drawing colours against the sheet colour (a drawing property, not a shell one); real function-key presses (see [UI_UX.md](UI_UX.md)). The dialogs and tool windows were checked by source scan only, not rendered.

## Reproduce

Start `aad --sample`, then read the tree:

```powershell
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$p = Get-Process Aad.WinUi | Select-Object -First 1
$root = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
$root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
```

Do not capture the window with a screen copy while another window overlaps it; `PrintWindow` with the
`PW_RENDERFULLCONTENT` flag (2) captures only that window.

## Second pass, after the Tren rebrand (2026-10-19)

Same method, plus the dialogs opened through UI Automation (Units, Linetype, Drafting Settings, Command list) and both themes captured with `PrintWindow`.

| # | Finding | Criterion | State |
|---|---|---|---|
| 7 | With the black accent, a pressed-in status-bar toggle (snap, polar, tracking, dynamic input) is filled light in the dark theme and ink in the light theme, but its drawn icon kept the old stroke colour, so the icon vanished | 1.4.11 | **Fixed.** `DrawnIcon` redraws when its toggle is checked or unchecked and draws the opposite tone on an accent fill |
| 8 | The Drafting Settings window opened 520 px high; its last section was cut off (the page scrolls, but nothing showed that) | UI | **Fixed**: 680 px |
| 9 | The Start page showed a logo and the word "Start" | UI | **Fixed**: product name, one line saying what it is, then the three buttons |
| 10 | The layer table's horizontal scroll bar was only an overlay, so a 470 px panel looked as if the table ended at Freeze | UI | **Fixed**: the bar is always visible. The columns still do not all fit; widen the panel or scroll |

Passed again: every element in the four dialogs has a name; no focusable target under 24 px other than the clipped one in 8.

Still open: the `command_bar` tests of the egui shell (six) fail because the command bar's Space rule changed (a sentence now needs a leading `/`); the tests were not updated. Narrator, high-contrast themes, 200 % scaling and the dialogs not reachable by a named menu item (Page Setup, Room Label Settings) were not examined.

## Third pass — History and Revisions panels (2026-10-27)

Source review of `HistoryWindow.cs`, `RevisionsWindow.cs`, `LevelBars.cs` and the History/Revisions side-rail entries against WCAG 2.2 AA. No Narrator run and no UIA tree capture for these panels (unlike passes 1–2).

| # | Finding | Criterion | State |
|---|---|---|---|
| 11 | History and Revisions list rows were `Border` + `PointerPressed` only | 2.1.1 | **Fixed.** Both panels use `ListView` (single selection, item click); arrows / Enter / Space work through the control |
| 12 | Rows lacked list-item role / Revisions rows unnamed | 4.1.2 | **Fixed.** `ListViewItem` with accessible names; `AutomationId` on timeline, register, actions and fields |
| 13 | Selection was colour-only; no focus ring on rows | 2.4.7, 1.4.1 | **Fixed.** ListView focus visuals plus a ✓ selection mark with “Selected” / “Not selected” name |
| 14 | Issue / milestone fields used placeholder without a visible label | 3.3.2 | **Fixed.** `DialogLayout.Form` / `Labeled` for issue fields and milestone name (panel and dialogs) |
| 15 | Register column header “Chk” | 3.1.4 | **Fixed.** Header text “Checked” |
| 16 | Day group labels were plain text | 1.3.1 | **Fixed.** Day headers are non-selectable `ListViewItem`s with `HeadingLevel` 3 |
| 17 | Status live regions, Level-2 panel titles, named actions, ContentDialogs, menus | 4.1.3, 2.4.6, 2.1.1 | **Pass** (unchanged) |

Still not tested on device: Narrator walkthrough, Windows High Contrast, 200 % scaling.

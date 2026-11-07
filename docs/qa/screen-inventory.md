# Tren WinUI screen inventory

Scope: `apps/aad-winui` (WinUI 3 shell). Engine menu paths from `crates/engine` `CommandSpec::menu()` (via `crates/aad-host/src/chrome.rs` → host `TextMenu` JSON).

**Testing status:** all rows default to `not_tested` (no pass/fail claimed). **Evidence:** — until linked from the QA campaign.

**Retired (2026-10):** ShilpiDB / `vdb.*` UI rows were removed from this inventory when take-off moved into `.osp`.

| ID | Surface | Type | Source file | How opened | Testing status | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| main.window | Tren main shell | main | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml | Application launch (`Program.cs` → `MainWindow`) | pass | screenshots/03-main-clean-drawing1.webp |
| main.splash | Startup splash overlay | overlay | apps/aad-winui/MainWindow.xaml (SplashOverlay) | Shown at load until host/canvas ready | pass | visual 2026-10-26 (dismisses ~900ms) |
| main.welcome | Welcome after splash | overlay | apps/aad-winui/MainWindow.xaml (WelcomeOverlay) + WelcomeBusy.cs | Empty launch (no path / not --sample): New, Open, Project Wizard, recent | unverified | code 2026-10-28 |
| main.busy | Open/save progress | overlay | apps/aad-winui/MainWindow.xaml (BusyOverlay) + WelcomeBusy.cs | While OpenFileAsync / SaveFileAsync (and .vdb import) run | unverified | code 2026-10-28 |
| main.menu | Menu bar (engine-driven + shell) | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (Bar); MainWindow.xaml.cs BuildMenus() | Always visible when Window ▸ Menu bar checked; populated from host TextMenu | pass | screenshots/03-main-clean-drawing1.webp |
| main.arch-bar | Tool bar: drawing tools, divider, architecture tools (one line; Layer / Color / Linetype / Floor boxes removed) | toolbar | apps/aad-winui/MainWindow.xaml (ToolStripHost) | Visible under the menu in Classic; tool buttons call StartCommand | not_tested | screenshot removed (no attribution entry) |
| main.selector.layer | Current layer combo | toolbar | — | Removed from the top bar; the current layer is set in the Layers panel | removed | 2026-10-27 |
| main.selector.space | Drawing space combo | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (SpaceBox) | Arch bar; saved view/spaces from host | pass | placeholder “No saved drawing space” |
| main.selector.floor | Current floor combo | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (FloorBox) | Arch bar; floor selection | pass | shows `Ground`; no dirty on fill |
| main.toolstrip | Left tool strip (draw/modify/annotate) | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (ToolStripHost); BuildTools() | Window ▸ Tool bar; gear opens customize window | pass | screenshots/03-main-clean-drawing1.webp |
| main.canvas | Drawing canvas host | main | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (CanvasHost) | Primary document view (native child HWND) | pass | UCS + crosshair; 0 entities |
| main.side-panel | Side panel tab host (Properties / Layer / Blocks) | panel | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (SideTabs) | Docked right; auto-hide rail when unpinned | pass | rail visible (Layer header) |
| main.panel.properties | Properties tab | panel | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (PropertiesTab, PropertiesPanel) | Side tab; ShowSidePanel(Properties); Ctrl+1; ui.panel.properties | not_tested | — |
| main.panel.layers | Layer side tab (merged palette) | panel | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (LayersTab, LayersSlot) | Side tab; layer merge embeds LayerManagerWindow body | not_tested | — |
| main.panel.blocks | Blocks tab | panel | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (BlocksTab, BlocksPanel) | Side tab; ui.dialog.blocks / ShowSidePanel(Blocks) | not_tested | — |
| main.side-rail | Collapsed side-panel rail | overlay | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (SideRail) | When side panel collapsed; tap to expand | pass | collapsed Layer rail on right |
| main.splitter | Side panel width grip | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (LeftSplitter) | Drag or arrow keys when focused | not_tested | — |
| main.status | Status bar | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (StatusRow) | Window ▸ Status bar | pass | screenshots/03-main-clean-drawing1.webp |
| main.status.drawings | Open drawing tabs + New | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (DrawingTabs) | Status bar left cluster | pass | `Drawing1` clean (no ` *`); QA-2026-003 |
| main.status.nameplate | Nameplate L / P menu | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (NameplateButton) | Status bar; flyout opens Nameplate L or Nameplate P via sheet.designer | not_tested | — |
| main.status.coords | Cursor coordinates | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (Coords) | Host chrome refresh | pass | live coords on status |
| main.status.modes | Snap / Ortho / Polar / OTrack / Dyn / LW / Transparency / Quick props toggles | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (status toggles) | Status bar right cluster | pass | toggles visible |
| main.status.units | Drafting units combo | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (UnitsBox) | Status bar; drawing.units metadata | pass | shows `in` |
| main.status.layers-btn | Open layer manager | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (LayersButton) | Status bar layers icon → OpenLayers() | not_tested | — |
| main.status.pdf | Plot preview | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (PdfButton) | Status bar → plotl/plotp window pick, then temporary nameplate PDF preview | not_tested | — |
| main.status.workspace | Classic / Pro workspace | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (WorkspaceButton) | Status bar; Ctrl+F11 toggle | not_tested | — |
| main.status.theme | Light / dark theme | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (ThemeButton) | Status bar | not_tested | — |
| main.status.ai-chip | AI assistant chip | status | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (AiChip) | Focus Ask bar (CommandChrome host window) | not_tested | — |
| main.ask-bar | Ask pill / AI prompt (hosted in separate HWND) | overlay | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/CommandChrome.cs; MainWindow.xaml (CommandBar, AskInputPill) | Ctrl+9 toggle command line area; AI chip; assistant mode | not_tested | — |
| main.ask-history | Command history list (Ask bar) | overlay | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (HistoryList) | F2 toggles command history open state | not_tested | — |
| main.ask-assistant | Allow / Decline assistant tools | overlay | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (AssistantButtons) | When host reports pending assistant action | not_tested | — |
| arch.tool.wall.draw | Architecture bar: Wall | toolbar | — | Removed from Architecture bar; `wall.draw` remains command-line / scripts | removed | 2026-10-26 |
| arch.tool.wall.door | Architecture bar: Door | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("wall.door") | not_tested | — |
| arch.tool.wall.window | Architecture bar: Window | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("wall.window") | not_tested | — |
| arch.tool.room | Architecture bar: Room | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("room") | not_tested | — |
| arch.tool.rarea | Architecture bar: Room area | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("rarea") | not_tested | — |
| arch.tool.grid.axis | Architecture bar: Axis grid | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("grid.axis") | not_tested | — |
| arch.tool.column.place | Architecture bar: Column | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("column.place") | not_tested | — |
| arch.tool.stair.plan | Architecture bar: Stair | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("stair.plan") | not_tested | — |
| arch.tool.north.arrow | Architecture bar: North | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ArchitectureTools') | Arch bar button → StartCommand("north.arrow") | not_tested | — |
| tool.line | Tool strip: Line | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("line") | not_tested | — |
| tool.pline | Tool strip: Polyline | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("pline") | not_tested | — |
| tool.circle | Tool strip: Circle | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("circle") | not_tested | — |
| tool.arc | Tool strip: Arc | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("arc") | not_tested | — |
| tool.rectang | Tool strip: Rectangle | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("rectang") | not_tested | — |
| tool.move | Tool strip: Move | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("move") | not_tested | — |
| tool.copy | Tool strip: Copy | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("copy") | not_tested | — |
| tool.rotate | Tool strip: Rotate | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("rotate") | not_tested | — |
| tool.offset | Tool strip: Offset | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("offset") | not_tested | — |
| tool.trim | Tool strip: Trim | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("trim") | not_tested | — |
| tool.erase | Tool strip: Erase | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("erase") | not_tested | — |
| tool.dimlinear | Tool strip: Dimension | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("dimlinear") | not_tested | — |
| tool.text | Tool strip: Text | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("text") | not_tested | — |
| tool.hatch | Tool strip: Hatch | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('ToolCatalog'/'BuildTools') | Left strip when enabled in customize list → StartCommand("hatch") | not_tested | — |
| tool.customize-gear | Tool strip: Customize tool bar | toolbar | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('BuildTools') | Gear tile → OpenTools() | not_tested | — |
| win.menu.shell.menu-bar | Window menu: Menu bar | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Menu bar toggle | not_tested | — |
| win.menu.shell.tool-bar | Window menu: Tool bar | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Tool bar toggle | not_tested | — |
| win.menu.shell.status-bar | Window menu: Status bar | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Status bar toggle | not_tested | — |
| win.menu.shell.command-line | Window menu: Command line | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Command line toggle | not_tested | — |
| win.menu.shell.pin-panels | Window menu: Pin side panels | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Pin side panels | not_tested | — |
| win.menu.shell.props | Window menu: Properties | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Properties | not_tested | — |
| win.menu.shell.layers-palette | Window menu: Layers palette | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Layers palette | not_tested | — |
| win.menu.shell.blocks | Window menu: Blocks | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Blocks | not_tested | — |
| win.menu.shell.parameters | Window menu: Parameters Manager | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Parameters Manager | not_tested | — |
| win.menu.shell.workspace.classic | Window menu: Classic workspace | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Classic workspace | not_tested | — |
| win.menu.shell.workspace.pro | Window menu: Pro workspace | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Pro workspace | not_tested | — |
| win.menu.shell.workspace.reset | Window menu: Reset this workspace | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Reset this workspace | not_tested | — |
| win.menu.shell.calc | Window menu: Calculator | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Calculator | not_tested | — |
| win.menu.shell.pomodoro | Window menu: Pomodoro | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Pomodoro | not_tested | — |
| win.menu.shell.customize-toolbar | Window menu: Customize tool bar | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Customize tool bar | not_tested | — |
| win.menu.shell.nameplate | Window menu: Nameplate L / P | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ Nameplate L / Nameplate P | not_tested | — |
| main.panel.project | Project settings side tab | panel | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml (ProjectTab) | Side tab; slab/beam defaults + links to project/floors forms | not_tested | — |
| win.menu.shell.ucs | Window menu: UCS icon | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureShellMenus') | Window ▸ UCS icon | not_tested | — |
| help.commands | Help: Command list | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('EnsureHelp') | Help menu or F1 → CommandList.Show | not_tested | — |
| help.about | Help: About Tren | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/AboutWindow.cs | Help menu → AboutWindow.Show | not_tested | — |
| win.layer-manager | Layer Properties Manager | manager | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LayerManagerWindow.xaml | Menu `layer`/`layers`; status layers button; `OpenLayers()`; merge mode docks body in side tab | not_tested | — |
| win.drafting | Drafting Settings | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/DraftingWindow.cs | `dsettings`/`ds`/`se`; menu/osnap shortcut menu | not_tested | — |
| win.units | Display units and plot | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/UnitsWindow.cs | `units`/`un`; Format menu | not_tested | — |
| win.text-style | Text Style | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/TextStyleWindow.cs | `style`/`st` | not_tested | — |
| win.page-setup | Page Setup Manager | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/PageSetupWindow.cs | `pagesetup` | not_tested | — |
| win.plot-style | Plot Style Manager | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/PlotStyleWindow.cs | `stylesmanager` | not_tested | — |
| win.batch-plot | Batch Plot | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/BatchPlotWindow.cs | `plot.batch` | not_tested | — |
| win.dim-style | Dimension Style | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/DimStyleWindow.cs | `dimstyle` | not_tested | — |
| win.linetype | Linetype Manager | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LinetypeWindow.cs | `linetype`/`lt`/`ltype` | not_tested | — |
| win.parameters | Parameters Manager | manager | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/ParametersWindow.cs | `ui.dialog.parameters`; Window menu | not_tested | — |
| win.hatch | Hatch Editor | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/HatchWindow.cs | `hatchedit`/`he`/`hatch.patterns` | not_tested | — |
| win.point-style | Point Style | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/PointStyleWindow.cs | `ddptype` | not_tested | — |
| win.mleader-style | Multileader Style | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MLeaderStyleWindow.cs | `mleaderstyle`/`mls` | not_tested | — |
| win.room-settings | Room Label Settings | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/RoomSettingsWindow.cs | `room.settings` | not_tested | — |
| win.quick-select | Quick Select | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/QuickSelectWindow.cs | `qselect`/`qs`/`ui.dialog.qselect` | not_tested | — |
| win.quick-props | Quick Properties | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/QuickWindow.cs | Auto when `_qpmode` and selection quick JSON from host | not_tested | — |
| win.commands | Commands (reference) | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/CommandList.cs | F1; Help; `ui.dialog.commands` | not_tested | — |
| win.about | About Tren | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/AboutWindow.cs | `ui.dialog.about`; Help menu | not_tested | — |
| win.calc | Calculator | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/CalcWindow.cs | `cal`/`calc`; Window menu | not_tested | — |
| win.pomodoro | Pomodoro | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/PomodoroWindow.cs | Window menu | not_tested | — |
| win.tools | Tool bar (customize) | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/ToolsWindow.cs | `ui.dialog.customizetop`; tool strip gear | not_tested | — |
| win.panels-float | Panels (floating side tabs) | panel | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('FloatLeft') | Side panel Float button | not_tested | — |
| win.pdf | Plot preview / PDF viewer | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('OnPlotPreview', 'ShowPdfAsync') | Status bar plot-preview button | not_tested | — |
| win.save-prompt | Save changes prompt | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/SavePrompt.cs | Close drawing / quit with dirty document | not_tested | — |
| win.clear-history | Clear command history | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs ('OnClearHistory') | Hidden UI wiring / clear history action | not_tested | — |
| win.canvas-menu | Canvas context menu window | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/CanvasMenu.cs | Host popup JSON (`ReadPopup`); right-click / snap menu | not_tested | — |
| win.form-dynamic | Engine form dialog (title from form JSON) | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs | Any command with host form spec via `ReadForm` | not_tested | — |
| chrome.ask | Ask bar top-level window | overlay | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/CommandChrome.cs ('_bar') | Hosts `CommandBar` XAML; owned by main window | not_tested | — |
| chrome.cad-input | CAD command line at cursor | overlay | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/CommandChrome.cs ('_dyn') | Ctrl+9 / F6; dynamic input at canvas | not_tested | — |
| layer.dlg.delete | Layer manager ContentDialog: Delete layers | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LayerManagerWindow.xaml.cs (CompactContentDialog, Ask) | Layer manager Delete / context Delete | not_tested | — |
| layer.dlg.rename | Layer manager ContentDialog: Rename layer | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LayerManagerWindow.xaml.cs (CompactContentDialog, Ask) | Rename / F2 / context menu | not_tested | — |
| layer.dlg.color | Layer manager ContentDialog: Layer color | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LayerManagerWindow.xaml.cs (CompactContentDialog, Ask) | Color cell edit | not_tested | — |
| layer.dlg.transparency | Layer manager ContentDialog: Transparency | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LayerManagerWindow.xaml.cs (CompactContentDialog, Ask) | Transparency cell edit | not_tested | — |
| layer.ctx-menu | Layer row context MenuFlyout | menu | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/LayerManagerWindow.xaml.cs (OnRowMenu) | Right-click layer row in manager | not_tested | — |
| picker.open | File Open (drawing) | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs | Menu File ▸ Open; Ctrl+O | not_tested | — |
| picker.save | File Save | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs | Menu File ▸ Save; Ctrl+S | not_tested | — |
| picker.save-as | File Save As | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs | Menu File ▸ Save As | not_tested | — |
| picker.plot-pdf | Plot PDF save | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs | Plot/exportpdf flow → FileSavePicker `.pdf` | not_tested | — |
| picker.pdf-open | Open PDF from preview | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/MainWindow.xaml.cs | PDF preview toolbar | not_tested | — |
| form.project | Form dialog: Project Settings | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id project) | Command project (or alias) when engine returns form JSON | not_tested | — |
| form.floors | Form dialog: Floors | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id floors) | Command floors (or alias) when engine returns form JSON | not_tested | — |
| form.beams | Form dialog: Beam Depths | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id beams) | Command beams (or alias) when engine returns form JSON | not_tested | — |
| form.schedule | Form dialog: Opening Schedule Edit | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id schedule) | Command schedule (or alias) when engine returns form JSON | not_tested | — |
| form.notes | Form dialog: Notes Edit | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id notes) | Command notes (or alias) when engine returns form JSON | not_tested | — |
| form.legend.edit | Form dialog: Legend Entries | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id legend.edit) | Command legend.edit (or alias) when engine returns form JSON | not_tested | — |
| form.symbol.desc | Form dialog: Block Description | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id symbol.desc) | Command symbol.desc (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.rates | Form dialog: Take-off Rates | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.rates) | Command takeoff.rates (or alias) when engine returns form JSON | not_tested | — |
| form.room | Form dialog: Room Label | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id room) | Command room (or alias) when engine returns form JSON | not_tested | — |
| form.xref.attach | Form dialog: Attach Xref | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id xref.attach) | Command xref.attach (or alias) when engine returns form JSON | not_tested | — |
| form.xref.detach | Form dialog: Detach Xref | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id xref.detach) | Command xref.detach (or alias) when engine returns form JSON | not_tested | — |
| form.xref.bind | Form dialog: Bind Xref | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id xref.bind) | Command xref.bind (or alias) when engine returns form JSON | not_tested | — |
| form.floor.activate | Form dialog: Activate Floor | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id floor.activate) | Command floor.activate (or alias) when engine returns form JSON | not_tested | — |
| form.floor.declare | Form dialog: Declare Floor Region | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id floor.declare) | Command floor.declare (or alias) when engine returns form JSON | not_tested | — |
| form.floor.export | Form dialog: Export Floor Drawing | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id floor.export) | Command floor.export (or alias) when engine returns form JSON | not_tested | — |
| form.floor.xref | Form dialog: Floor as Xref | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id floor.xref) | Command floor.xref (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.restore | Form dialog: Restore Take-off Tables | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.restore) | Command takeoff.restore (or alias) when engine returns form JSON | not_tested | — |
| form.hcwlayers | Form dialog: Architectural Layer Set | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id hcwlayers) | Command hcwlayers (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.linear | Form dialog: Linear Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.linear) | Command takeoff.linear (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.brick | Form dialog: Brickwork Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.brick) | Command takeoff.brick (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.beams | Form dialog: Beams and Lintels Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.beams) | Command takeoff.beams (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.columns | Form dialog: Columns Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.columns) | Command takeoff.columns (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.rects | Form dialog: Rectangles Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.rects) | Command takeoff.rects (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.paint | Form dialog: Wall Paint Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.paint) | Command takeoff.paint (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.ceiling | Form dialog: Ceiling Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.ceiling) | Command takeoff.ceiling (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.floor | Form dialog: Flooring Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.floor) | Command takeoff.floor (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.area | Form dialog: Area Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.area) | Command takeoff.area (or alias) when engine returns form JSON | not_tested | — |
| form.takeoff.slab | Form dialog: Slab Take-off | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id takeoff.slab) | Command takeoff.slab (or alias) when engine returns form JSON | not_tested | — |
| form.wall.draw | Form dialog: Draw Wall form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id wall.draw) | Command wall.draw (or alias) when engine returns form JSON | not_tested | — |
| form.wall.door | Form dialog: Door form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id wall.door) | Command wall.door (or alias) when engine returns form JSON | not_tested | — |
| form.wall.window | Form dialog: Window form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id wall.window) | Command wall.window (or alias) when engine returns form JSON | not_tested | — |
| form.grid.axis | Form dialog: Axis grid form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id grid.axis) | Command grid.axis (or alias) when engine returns form JSON | not_tested | — |
| form.column.place | Form dialog: Column form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id column.place) | Command column.place (or alias) when engine returns form JSON | not_tested | — |
| form.lintel.draw | Form dialog: Lintel form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id lintel.draw) | Command lintel.draw (or alias) when engine returns form JSON | not_tested | — |
| form.stair.plan | Form dialog: Stair form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id stair.plan) | Command stair.plan (or alias) when engine returns form JSON | not_tested | — |
| form.lift.shaft | Form dialog: Lift shaft form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id lift.shaft) | Command lift.shaft (or alias) when engine returns form JSON | not_tested | — |
| form.rail.draw | Form dialog: Rail form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id rail.draw) | Command rail.draw (or alias) when engine returns form JSON | not_tested | — |
| form.level.mark | Form dialog: Level mark form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id level.mark) | Command level.mark (or alias) when engine returns form JSON | not_tested | — |
| form.section.marker | Form dialog: Section marker form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id section.marker) | Command section.marker (or alias) when engine returns form JSON | not_tested | — |
| form.elevation.marker | Form dialog: Elevation marker form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id elevation.marker) | Command elevation.marker (or alias) when engine returns form JSON | not_tested | — |
| form.north.arrow | Form dialog: North arrow form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id north.arrow) | Command north.arrow (or alias) when engine returns form JSON | not_tested | — |
| form.slope.arrow | Form dialog: Slope arrow form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id slope.arrow) | Command slope.arrow (or alias) when engine returns form JSON | not_tested | — |
| form.electrical.connect | Form dialog: Electrical connect form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id electrical.connect) | Command electrical.connect (or alias) when engine returns form JSON | not_tested | — |
| form.electrical.number | Form dialog: Electrical numbering form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id electrical.number) | Command electrical.number (or alias) when engine returns form JSON | not_tested | — |
| form.electrical.schedule | Form dialog: Electrical Schedule form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id electrical.schedule) | Command electrical.schedule (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.pipe | Form dialog: Draw Pipe form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.pipe) | Command plumbing.pipe (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.pipe.cold | Form dialog: Cold Water Pipe form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.pipe.cold) | Command plumbing.pipe.cold (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.pipe.hot | Form dialog: Hot Water Pipe form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.pipe.hot) | Command plumbing.pipe.hot (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.pipe.drain | Form dialog: Drain Pipe form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.pipe.drain) | Command plumbing.pipe.drain (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.connect | Form dialog: Check Piping form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.connect) | Command plumbing.connect (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.number | Form dialog: Plumbing numbering form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.number) | Command plumbing.number (or alias) when engine returns form JSON | not_tested | — |
| form.plumbing.schedule | Form dialog: Plumbing Schedule form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id plumbing.schedule) | Command plumbing.schedule (or alias) when engine returns form JSON | not_tested | — |
| form.bridge.export | Form dialog: Bridge export form | dialog | D:/Work Development/Repos/O-DRAFT/apps/aad-winui/FormDialog.cs; crates/engine/src/forms.rs (id bridge.export) | Command bridge.export (or alias) when engine returns form JSON | not_tested | — |

## Engine menu bar entries

Populated at runtime from host menu JSON (`BuildMenus()`). Top-level order: File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Architecture, Window, Help (`crates/aad-host/src/chrome.rs`).

| ID | Surface | Type | Source file | How opened | Testing status | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| menu.bridge.export | Architecture > AQC Bridge > Export for AQC | menu | crates/engine/src/cmd/bridge.rs (+ host chrome) | Menu bar path; command id bridge.export via StartCommand / accelerator | not_tested | — |
| menu.bridge.import | Architecture > AQC Bridge > Import from AQC | menu | crates/engine/src/cmd/bridge.rs (+ host chrome) | Menu bar path; command id bridge.import via StartCommand / accelerator | not_tested | — |
| menu.bpltarea | Architecture > Building Permission > Area Statement | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bpltarea via StartCommand / accelerator | not_tested | — |
| menu.bpltcheck | Architecture > Building Permission > Check Before AutoPlan | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bpltcheck via StartCommand / accelerator | not_tested | — |
| menu.bpltcopy | Architecture > Building Permission > Copy to AP- Layers | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bpltcopy via StartCommand / accelerator | not_tested | — |
| menu.bpltlayers | Architecture > Building Permission > Layer Legend | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bpltlayers via StartCommand / accelerator | not_tested | — |
| menu.bpltreport | Architecture > Building Permission > Layer Report (CSV) | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bpltreport via StartCommand / accelerator | not_tested | — |
| menu.bpltstart | Architecture > Building Permission > Start (Metres > Layers > Styles) | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bpltstart via StartCommand / accelerator | not_tested | — |
| menu.bplttitleblock | Architecture > Building Permission > Title Block | menu | crates/engine/src/cmd/bplt.rs (+ host chrome) | Menu bar path; command id bplttitleblock via StartCommand / accelerator | not_tested | — |
| menu.electrical.connect | Architecture > Electrical > Check Wiring | menu | crates/engine/src/cmd/electrical.rs (+ host chrome) | Menu bar path; command id electrical.connect via StartCommand / accelerator | not_tested | — |
| menu.electrical.blocks | Architecture > Electrical > Map Blocks | menu | crates/engine/src/cmd/electrical.rs (+ host chrome) | Menu bar path; command id electrical.blocks via StartCommand / accelerator | not_tested | — |
| menu.electrical.number | Architecture > Electrical > Number Blocks | menu | crates/engine/src/cmd/electrical.rs (+ host chrome) | Menu bar path; command id electrical.number via StartCommand / accelerator | not_tested | — |
| menu.electrical.schedule | Architecture > Electrical > Schedule | menu | crates/engine/src/cmd/electrical.rs (+ host chrome) | Menu bar path; command id electrical.schedule via StartCommand / accelerator | not_tested | — |
| menu.elec.light | Architecture > Electrical > Light point | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id elec.light via StartCommand / accelerator | not_tested | — |
| menu.elec.socket | Architecture > Electrical > Socket | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id elec.socket via StartCommand / accelerator | not_tested | — |
| menu.elec.switchboard | Architecture > Electrical > Switch board | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id elec.switchboard via StartCommand / accelerator | not_tested | — |
| menu.elec.fan | Architecture > Electrical > Ceiling fan | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id elec.fan.ceiling via StartCommand / accelerator | not_tested | — |
| menu.elec.downlight | Architecture > Electrical > Downlight | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id elec.downlight via StartCommand / accelerator | not_tested | — |
| menu.elec.db | Architecture > Electrical > Distribution board | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id elec.db via StartCommand / accelerator | not_tested | — |
| menu.plumb.wc.european | Architecture > Plumbing > European WC | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.wc.european via StartCommand / accelerator | not_tested | — |
| menu.plumb.wc.indian | Architecture > Plumbing > Indian WC | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.wc.indian via StartCommand / accelerator | not_tested | — |
| menu.plumb.basin | Architecture > Plumbing > Wash basin | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.basin via StartCommand / accelerator | not_tested | — |
| menu.plumb.sink | Architecture > Plumbing > Kitchen sink | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.sink.kitchen via StartCommand / accelerator | not_tested | — |
| menu.plumb.shower | Architecture > Plumbing > Shower | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.shower via StartCommand / accelerator | not_tested | — |
| menu.plumb.trap | Architecture > Plumbing > Floor trap | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.floor.trap via StartCommand / accelerator | not_tested | — |
| menu.plumb.chamber | Architecture > Plumbing > Inspection chamber | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.chamber via StartCommand / accelerator | not_tested | — |
| menu.plumb.tank | Architecture > Plumbing > Water tank | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.tank.round via StartCommand / accelerator | not_tested | — |
| menu.plumb.riser | Architecture > Plumbing > Pipe riser | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id plumb.riser via StartCommand / accelerator | not_tested | — |
| menu.int.bed | Architecture > Interior > Bed | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id int.bed via StartCommand / accelerator | not_tested | — |
| menu.int.sofa | Architecture > Interior > Sofa | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id int.sofa via StartCommand / accelerator | not_tested | — |
| menu.int.dining | Architecture > Interior > Dining set | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id int.dining.set via StartCommand / accelerator | not_tested | — |
| menu.int.counter | Architecture > Interior > Kitchen counter | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id int.kitchen.counter via StartCommand / accelerator | not_tested | — |
| menu.int.wardrobe | Architecture > Interior > Wardrobe | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id int.wardrobe via StartCommand / accelerator | not_tested | — |
| menu.int.shelf | Architecture > Interior > Shelf | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id int.shelf via StartCommand / accelerator | not_tested | — |
| menu.hvac.diffuser | Architecture > False ceiling > Diffuser | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id hvac.diffuser.square via StartCommand / accelerator | not_tested | — |
| menu.hvac.grille | Architecture > False ceiling > Grille | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id hvac.grille via StartCommand / accelerator | not_tested | — |
| menu.hvac.duct | Architecture > False ceiling > Duct | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id hvac.duct.rect via StartCommand / accelerator | not_tested | — |
| menu.hvac.exhaust | Architecture > False ceiling > Exhaust fan | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id hvac.fan.exhaust via StartCommand / accelerator | not_tested | — |
| menu.hvac.indoor | Architecture > False ceiling > Indoor unit | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id hvac.split.indoor via StartCommand / accelerator | not_tested | — |
| menu.hvac.outdoor | Architecture > False ceiling > Outdoor unit | menu | crates/engine/src/cmd/smartblock.rs (+ host chrome) | Menu bar path; command id hvac.split.outdoor via StartCommand / accelerator | not_tested | — |
| menu.electrical.update | Architecture > Electrical > Update | menu | crates/engine/src/cmd/electrical.rs (+ host chrome) | Menu bar path; command id electrical.update via StartCommand / accelerator | not_tested | — |
| menu.electrical.layers | Architecture > Electrical > Wiring Layers | menu | crates/engine/src/cmd/electrical.rs (+ host chrome) | Menu bar path; command id electrical.layers via StartCommand / accelerator | not_tested | — |
| menu.plumbing.connect | Architecture > Plumbing > Check Piping | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.connect via StartCommand / accelerator | not_tested | — |
| menu.plumbing.blocks | Architecture > Plumbing > Map Blocks | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.blocks via StartCommand / accelerator | not_tested | — |
| menu.plumbing.number | Architecture > Plumbing > Number Blocks | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.number via StartCommand / accelerator | not_tested | — |
| menu.plumbing.schedule | Architecture > Plumbing > Schedule | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.schedule via StartCommand / accelerator | not_tested | — |
| menu.plumbing.update | Architecture > Plumbing > Update | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.update via StartCommand / accelerator | not_tested | — |
| menu.plumbing.layers | Architecture > Plumbing > Pipe Layers | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.layers via StartCommand / accelerator | not_tested | — |
| menu.plumbing.pipe | Architecture > Plumbing > Draw Pipe | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.pipe via StartCommand / accelerator | not_tested | — |
| menu.plumbing.pipe.cold | Architecture > Plumbing > Cold Water Pipe | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.pipe.cold via StartCommand / accelerator | not_tested | — |
| menu.plumbing.pipe.hot | Architecture > Plumbing > Hot Water Pipe | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.pipe.hot via StartCommand / accelerator | not_tested | — |
| menu.plumbing.pipe.drain | Architecture > Plumbing > Drain Pipe | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.pipe.drain via StartCommand / accelerator | not_tested | — |
| menu.plumbing.fittings | Architecture > Plumbing > Fittings | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.fittings via StartCommand / accelerator | not_tested | — |
| menu.plumbing.autofit | Architecture > Plumbing > Auto-add Fittings | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.autofit via StartCommand / accelerator | not_tested | — |
| menu.plumbing.autonumber | Architecture > Plumbing > Auto-number on Insert | menu | crates/engine/src/cmd/plumbing.rs (+ host chrome) | Menu bar path; command id plumbing.autonumber via StartCommand / accelerator | not_tested | — |
| menu.floor.activate | Architecture > Floors > Activate Floor | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id floor.activate via StartCommand / accelerator | not_tested | — |
| menu.floor.current | Architecture > Floors > Current Floor | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id floor.current via StartCommand / accelerator | not_tested | — |
| menu.floor.declare | Architecture > Floors > Declare Floor Region | menu | crates/engine/src/cmd/floorregion.rs (+ host chrome) | Menu bar path; command id floor.declare via StartCommand / accelerator | not_tested | — |
| menu.floor.export | Architecture > Floors > Export Floor Drawing | menu | crates/engine/src/cmd/floorregion.rs (+ host chrome) | Menu bar path; command id floor.export via StartCommand / accelerator | not_tested | — |
| menu.floor.xref | Architecture > Floors > Floor as Xref | menu | crates/engine/src/cmd/floorregion.rs (+ host chrome) | Menu bar path; command id floor.xref via StartCommand / accelerator | not_tested | — |
| menu.floor.list | Architecture > Floors > Floor List | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id floor.list via StartCommand / accelerator | not_tested | — |
| menu.floor.regions | Architecture > Floors > Floor Regions | menu | crates/engine/src/cmd/floorregion.rs (+ host chrome) | Menu bar path; command id floor.regions via StartCommand / accelerator | not_tested | — |
| menu.floor.set | Architecture > Floors > Floor Visibility | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id floor.set via StartCommand / accelerator | not_tested | — |
| menu.floor.refresh | Architecture > Floors > Refresh Floor Regions | menu | crates/engine/src/cmd/floorregion.rs (+ host chrome) | Menu bar path; command id floor.refresh via StartCommand / accelerator | not_tested | — |
| menu.floor.viewport | Architecture > Floors > Viewport Floors | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id floor.viewport via StartCommand / accelerator | not_tested | — |
| menu.elevation.marker | Architecture > Levels and Sections > Elevation Marker | menu | crates/engine/src/cmd/symbols.rs (+ host chrome) | Menu bar path; command id elevation.marker via StartCommand / accelerator | not_tested | — |
| menu.level.mark | Architecture > Levels and Sections > Level Mark | menu | crates/engine/src/cmd/symbols.rs (+ host chrome) | Menu bar path; command id level.mark via StartCommand / accelerator | not_tested | — |
| menu.level.schedule | Architecture > Levels and Sections > Level Schedule | menu | crates/engine/src/cmd/symbols.rs (+ host chrome) | Menu bar path; command id level.schedule via StartCommand / accelerator | not_tested | — |
| menu.section.marker | Architecture > Levels and Sections > Section Marker | menu | crates/engine/src/cmd/symbols.rs (+ host chrome) | Menu bar path; command id section.marker via StartCommand / accelerator | not_tested | — |
| menu.param.commit | Architecture > Parametric > Commit | menu | crates/engine/src/cmd/param.rs (+ host chrome) | Menu bar path; command id param.commit via StartCommand / accelerator | not_tested | — |
| menu.library.list | Architecture > Parametric > Component Library | menu | crates/engine/src/cmd/library.rs (+ host chrome) | Menu bar path; command id library.list via StartCommand / accelerator | not_tested | — |
| menu.param.graph | Architecture > Parametric > Graph | menu | crates/engine/src/cmd/param.rs (+ host chrome) | Menu bar path; command id param.graph via StartCommand / accelerator | not_tested | — |
| menu.param.library | Architecture > Parametric > New Wall | menu | crates/engine/src/cmd/param.rs (+ host chrome) | Menu bar path; command id param.library via StartCommand / accelerator | not_tested | — |
| menu.param.set | Architecture > Parametric > Preview | menu | crates/engine/src/cmd/param.rs (+ host chrome) | Menu bar path; command id param.set via StartCommand / accelerator | not_tested | — |
| menu.library.publish | Architecture > Parametric > Publish to Library | menu | crates/engine/src/cmd/library.rs (+ host chrome) | Menu bar path; command id library.publish via StartCommand / accelerator | not_tested | — |
| menu.param.status | Architecture > Parametric > Status | menu | crates/engine/src/cmd/param.rs (+ host chrome) | Menu bar path; command id param.status via StartCommand / accelerator | not_tested | — |
| menu.param.detach | Architecture > Parametric > Take-off Facts | menu | crates/engine/src/cmd/param.rs (+ host chrome) | Menu bar path; command id param.detach via StartCommand / accelerator | not_tested | — |
| menu.beams | Architecture > Project > Beam Depths | menu | crates/engine/src/cmd/project.rs (+ host chrome) | Menu bar path; command id beams via StartCommand / accelerator | not_tested | — |
| menu.levels | Architecture > Project > Building Levels | menu | crates/engine/src/cmd/project.rs (+ host chrome) | Menu bar path; command id levels via StartCommand / accelerator | not_tested | — |
| menu.companyfile | Architecture > Project > Company File Details | menu | crates/engine/src/cmd/companyfile.rs (+ host chrome) | Menu bar path; command id companyfile via StartCommand / accelerator | not_tested | — |
| menu.floors | Architecture > Project > Floors | menu | crates/engine/src/cmd/project.rs (+ host chrome) | Menu bar path; command id floors via StartCommand / accelerator | not_tested | — |
| menu.project | Architecture > Project > Project Settings | menu | crates/engine/src/cmd/project.rs (+ host chrome) | Menu bar path; command id project via StartCommand / accelerator | not_tested | — |
| menu.polyarea | Architecture > Rooms and Areas > Polyline Area Table | menu | crates/engine/src/cmd/legacytools.rs (+ host chrome) | Menu bar path; command id polyarea via StartCommand / accelerator | not_tested | — |
| menu.room.relabel | Architecture > Rooms and Areas > Rename Room Label | menu | crates/engine/src/cmd/roomtools.rs (+ host chrome) | Menu bar path; command id room.relabel via StartCommand / accelerator | not_tested | — |
| menu.rarea | Architecture > Rooms and Areas > Room Area | menu | crates/engine/src/cmd/hcw.rs (+ host chrome) | Menu bar path; command id rarea via StartCommand / accelerator | not_tested | — |
| menu.room.total | Architecture > Rooms and Areas > Room Area Total | menu | crates/engine/src/cmd/roomtools.rs (+ host chrome) | Menu bar path; command id room.total via StartCommand / accelerator | not_tested | — |
| menu.room | Architecture > Rooms and Areas > Room Label | menu | crates/engine/src/cmd/hcw.rs (+ host chrome) | Menu bar path; command id room via StartCommand / accelerator | not_tested | — |
| menu.room.audit | Architecture > Rooms and Areas > Room Label Audit | menu | crates/engine/src/cmd/roomtools.rs (+ host chrome) | Menu bar path; command id room.audit via StartCommand / accelerator | not_tested | — |
| menu.room.settings | Architecture > Rooms and Areas > Room Label Settings... | menu | crates/engine/src/cmd/hcw.rs (+ host chrome) | Menu bar path; command id room.settings via StartCommand / accelerator | not_tested | — |
| menu.room.log | Architecture > Rooms and Areas > Room Log (CSV) | menu | crates/engine/src/cmd/roomtools.rs (+ host chrome) | Menu bar path; command id room.log via StartCommand / accelerator | not_tested | — |
| menu.room.lighting | Architecture > Rooms and Areas > Room Lighting | menu | crates/engine/src/cmd/roomtools.rs (+ host chrome) | Menu bar path; command id room.lighting via StartCommand / accelerator | not_tested | — |
| menu.room.check | Architecture > Rooms and Areas > Room Rectangle Check | menu | crates/engine/src/cmd/roomtools.rs (+ host chrome) | Menu bar path; command id room.check via StartCommand / accelerator | not_tested | — |
| menu.sheet.template.list | Architecture > Sheets > Apply Sheet Template | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id sheet.template.list via StartCommand / accelerator | not_tested | — |
| menu.sheet.template.save | Architecture > Sheets > Save Sheet Template | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id sheet.template.save via StartCommand / accelerator | not_tested | — |
| menu.sheet.fields | Architecture > Sheets > Sheet Fields | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id sheet.fields via StartCommand / accelerator | not_tested | — |
| menu.sheet.titleblock | Architecture > Sheets > Title Block | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id sheet.titleblock via StartCommand / accelerator | not_tested | — |
| menu.sheet.designer | Architecture > Sheets > Title Plate | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id sheet.designer via StartCommand / accelerator | not_tested | — |
| menu.hcwaudit2 | Architecture > Standard Layers > Find Non-Standard Layers | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwaudit2 via StartCommand / accelerator | not_tested | — |
| menu.hcwbyblock | Architecture > Standard Layers > Find Property Overrides | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwbyblock via StartCommand / accelerator | not_tested | — |
| menu.hcwlegend | Architecture > Standard Layers > Layer Legend | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwlegend via StartCommand / accelerator | not_tested | — |
| menu.hcwschedule | Architecture > Standard Layers > Layer Schedule (CSV) | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwschedule via StartCommand / accelerator | not_tested | — |
| menu.hcwinfo | Architecture > Standard Layers > Layer Table | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwinfo via StartCommand / accelerator | not_tested | — |
| menu.hcwlock | Architecture > Standard Layers > Lock by Prefix | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwlock via StartCommand / accelerator | not_tested | — |
| menu.hcwmove | Architecture > Standard Layers > Move to Standard Layer | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwmove via StartCommand / accelerator | not_tested | — |
| menu.hcwreset | Architecture > Standard Layers > Reset to Standard | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwreset via StartCommand / accelerator | not_tested | — |
| menu.hcwunlock | Architecture > Standard Layers > Unlock by Prefix | menu | crates/engine/src/cmd/hcwlayers.rs (+ host chrome) | Menu bar path; command id hcwunlock via StartCommand / accelerator | not_tested | — |
| menu.grid.axis | Architecture > Structure > Axis Grid | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id grid.axis via StartCommand / accelerator | not_tested | — |
| menu.column.schedule | Architecture > Structure > Column Schedule | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id column.schedule via StartCommand / accelerator | not_tested | — |
| menu.lintel.draw | Architecture > Structure > Generate Lintel | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id lintel.draw via StartCommand / accelerator | not_tested | — |
| menu.rail.draw | Architecture > Structure > Handrail | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id rail.draw via StartCommand / accelerator | not_tested | — |
| menu.lift.shaft | Architecture > Structure > Lift Shaft | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id lift.shaft via StartCommand / accelerator | not_tested | — |
| menu.column.place | Architecture > Structure > Place Columns | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id column.place via StartCommand / accelerator | not_tested | — |
| menu.stair.plan | Architecture > Structure > Stair | menu | crates/engine/src/cmd/structure.rs (+ host chrome) | Menu bar path; command id stair.plan via StartCommand / accelerator | not_tested | — |
| menu.north.arrow | Architecture > Symbols > North Arrow | menu | crates/engine/src/cmd/symbols.rs (+ host chrome) | Menu bar path; command id north.arrow via StartCommand / accelerator | not_tested | — |
| menu.slope.arrow | Architecture > Symbols > Slope Arrow | menu | crates/engine/src/cmd/symbols.rs (+ host chrome) | Menu bar path; command id slope.arrow via StartCommand / accelerator | not_tested | — |
| menu.takeoff.annotate | Architecture > Take-off > Annotated Drawing (PDF) | menu | crates/engine/src/cmd/takeoffann.rs (+ host chrome) | Menu bar path; command id takeoff.annotate via StartCommand / accelerator | not_tested | — |
| menu.takeoff.area | Architecture > Take-off > Area | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.area via StartCommand / accelerator | not_tested | — |
| menu.underlay.attach | Architecture > Take-off > Attach PDF Underlay | menu | crates/engine/src/cmd/underlay.rs (+ host chrome) | Menu bar path; command id underlay.attach via StartCommand / accelerator | not_tested | — |
| menu.takeoff.beams | Architecture > Take-off > Beams | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.beams via StartCommand / accelerator | not_tested | — |
| menu.takeoff.bill | Architecture > Take-off > Bill | menu | crates/engine/src/cmd/schedule.rs (+ host chrome) | Menu bar path; command id takeoff.bill via StartCommand / accelerator | not_tested | — |
| menu.takeoff.brick | Architecture > Take-off > Brickwork | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.brick via StartCommand / accelerator | not_tested | — |
| menu.underlay.calibrate | Architecture > Take-off > Calibrate PDF Underlay | menu | crates/engine/src/cmd/underlay.rs (+ host chrome) | Menu bar path; command id underlay.calibrate via StartCommand / accelerator | not_tested | — |
| menu.takeoff.ceiling | Architecture > Take-off > Ceiling | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.ceiling via StartCommand / accelerator | not_tested | — |
| menu.takeoff.clear | Architecture > Take-off > Clear Labels | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.clear via StartCommand / accelerator | not_tested | — |
| menu.takeoff.columns | Architecture > Take-off > Columns | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.columns via StartCommand / accelerator | not_tested | — |
| menu.takeoff.export | Architecture > Take-off > Export CSV | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.export via StartCommand / accelerator | not_tested | — |
| menu.takeoff.floor | Architecture > Take-off > Floor | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.floor via StartCommand / accelerator | not_tested | — |
| menu.takeoff.linear | Architecture > Take-off > Linear | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.linear via StartCommand / accelerator | not_tested | — |
| menu.takeoff.list | Architecture > Take-off > List | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.list via StartCommand / accelerator | not_tested | — |
| menu.underlay.list | Architecture > Take-off > List PDF Underlays | menu | crates/engine/src/cmd/underlay.rs (+ host chrome) | Menu bar path; command id underlay.list via StartCommand / accelerator | not_tested | — |
| menu.takeoff.rates | Architecture > Take-off > Rates | menu | crates/engine/src/cmd/schedule.rs (+ host chrome) | Menu bar path; command id takeoff.rates via StartCommand / accelerator | not_tested | — |
| menu.takeoff.record | Architecture > Take-off > Record in project file (.osp) | menu | crates/engine/src/cmd/takeoffrec.rs (+ host chrome) | Menu bar path; command id takeoff.record via StartCommand / accelerator | not_tested | — |
| menu.takeoff.rects | Architecture > Take-off > Rectangles | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.rects via StartCommand / accelerator | not_tested | — |
| menu.underlay.remove | Architecture > Take-off > Remove PDF Underlay | menu | crates/engine/src/cmd/underlay.rs (+ host chrome) | Menu bar path; command id underlay.remove via StartCommand / accelerator | not_tested | — |
| menu.takeoff.restore | Architecture > Take-off > Restore Tables | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.restore via StartCommand / accelerator | not_tested | — |
| menu.takeoff.slab | Architecture > Take-off > Slab | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.slab via StartCommand / accelerator | not_tested | — |
| menu.takeoff.start | Architecture > Take-off > Start | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.start via StartCommand / accelerator | not_tested | — |
| menu.takeoff.paint | Architecture > Take-off > Wall Paint | menu | crates/engine/src/cmd/takeoff.rs (+ host chrome) | Menu bar path; command id takeoff.paint via StartCommand / accelerator | not_tested | — |
| menu.txtalign | Architecture > Text Tools > Align Text | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id txtalign via StartCommand / accelerator | not_tested | — |
| menu.txtaudit | Architecture > Text Tools > Audit Text Colours | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id txtaudit via StartCommand / accelerator | not_tested | — |
| menu.txtinc | Architecture > Text Tools > Copy and Increment Text | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id txtinc via StartCommand / accelerator | not_tested | — |
| menu.deleteareatext | Architecture > Text Tools > Delete Area Text | menu | crates/engine/src/cmd/legacytools.rs (+ host chrome) | Menu bar path; command id deleteareatext via StartCommand / accelerator | not_tested | — |
| menu.txtexport | Architecture > Text Tools > Export Text to CSV | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id txtexport via StartCommand / accelerator | not_tested | — |
| menu.txtdup | Architecture > Text Tools > Find Duplicate Text | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id txtdup via StartCommand / accelerator | not_tested | — |
| menu.fixtxt | Architecture > Text Tools > Fix Overlapping Text | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id fixtxt via StartCommand / accelerator | not_tested | — |
| menu.fixtxth | Architecture > Text Tools > Fix Overlapping Text (Horizontal) | menu | crates/engine/src/cmd/texttools.rs (+ host chrome) | Menu bar path; command id fixtxth via StartCommand / accelerator | not_tested | — |
| menu.vhlayers | Architecture > VH Layers > Create VH Layer Set | menu | crates/engine/src/cmd/legacytools.rs (+ host chrome) | Menu bar path; command id vhlayers via StartCommand / accelerator | not_tested | — |
| menu.wall.clean | Architecture > Walls and Openings > Clean Lines | menu | crates/engine/src/cmd/walls.rs (+ host chrome) | Menu bar path; command id wall.clean via StartCommand / accelerator | not_tested | — |
| menu.wall.door | Architecture > Walls and Openings > Door | menu | crates/engine/src/cmd/walls.rs (+ host chrome) | Menu bar path; command id wall.door via StartCommand / accelerator | not_tested | — |
| menu.wall.draw | Architecture > Walls and Openings > Draw Wall | menu | crates/engine/src/cmd/walls.rs | Menu entry removed; command id wall.draw remains for CLI / scripts | removed | 2026-10-26 |
| menu.wall.window | Architecture > Walls and Openings > Window | menu | crates/engine/src/cmd/walls.rs (+ host chrome) | Menu bar path; command id wall.window via StartCommand / accelerator | not_tested | — |
| menu.dimtedit.angle | Dimension > Align Text > Angle | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimtedit.angle via StartCommand / accelerator | not_tested | — |
| menu.dimtedit.center | Dimension > Align Text > Center | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimtedit.center via StartCommand / accelerator | not_tested | — |
| menu.dimdisassociate | Dimension > Align Text > Home | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimdisassociate via StartCommand / accelerator | not_tested | — |
| menu.dimtedit.left | Dimension > Align Text > Left | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimtedit.left via StartCommand / accelerator | not_tested | — |
| menu.dimtedit.right | Dimension > Align Text > Right | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimtedit.right via StartCommand / accelerator | not_tested | — |
| menu.dimaligned | Dimension > Aligned | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimaligned via StartCommand / accelerator | not_tested | — |
| menu.dimangular | Dimension > Angular | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimangular via StartCommand / accelerator | not_tested | — |
| menu.dimbaseline | Dimension > Baseline | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimbaseline via StartCommand / accelerator | not_tested | — |
| menu.dimcontinue | Dimension > Continue | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimcontinue via StartCommand / accelerator | not_tested | — |
| menu.dimdiameter | Dimension > Diameter | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimdiameter via StartCommand / accelerator | not_tested | — |
| menu.dim | Dimension > Dimension | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dim via StartCommand / accelerator | not_tested | — |
| menu.dimstyle.dimension | Dimension > Dimension Style... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id dimstyle.dimension via StartCommand / accelerator | not_tested | — |
| menu.dimlinear | Dimension > Linear | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimlinear via StartCommand / accelerator | not_tested | — |
| menu.mleader | Dimension > Multileader | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id mleader via StartCommand / accelerator | not_tested | — |
| menu.dimoverride | Dimension > Override | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimoverride via StartCommand / accelerator | not_tested | — |
| menu.qdim | Dimension > Quick Dimension | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id qdim via StartCommand / accelerator | not_tested | — |
| menu.dimradius | Dimension > Radius | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimradius via StartCommand / accelerator | not_tested | — |
| menu.dimreassociate | Dimension > Reassociate Dimensions | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id dimreassociate via StartCommand / accelerator | not_tested | — |
| menu.leader | Dimension > Update | menu | crates/engine/src/cmd/annotate.rs (+ host chrome) | Menu bar path; command id leader via StartCommand / accelerator | not_tested | — |
| menu.arc | Draw > Arc > 3 Points | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id arc via StartCommand / accelerator | not_tested | — |
| menu.arc.csa | Draw > Arc > Center > Start > Angle | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.csa via StartCommand / accelerator | not_tested | — |
| menu.arc.cse | Draw > Arc > Center > Start > End | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.cse via StartCommand / accelerator | not_tested | — |
| menu.arc.csl | Draw > Arc > Center > Start > Length | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.csl via StartCommand / accelerator | not_tested | — |
| menu.arc.continue | Draw > Arc > Continue | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.continue via StartCommand / accelerator | not_tested | — |
| menu.arc.sca | Draw > Arc > Start > Center > Angle | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.sca via StartCommand / accelerator | not_tested | — |
| menu.arc.sce | Draw > Arc > Start > Center > End | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.sce via StartCommand / accelerator | not_tested | — |
| menu.arc.scl | Draw > Arc > Start > Center > Length | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.scl via StartCommand / accelerator | not_tested | — |
| menu.arc.sea | Draw > Arc > Start > End > Angle | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.sea via StartCommand / accelerator | not_tested | — |
| menu.arc.sed | Draw > Arc > Start > End > Direction | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.sed via StartCommand / accelerator | not_tested | — |
| menu.arc.ser | Draw > Arc > Start > End > Radius | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id arc.ser via StartCommand / accelerator | not_tested | — |
| menu.wblock | Draw > Block > Base | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id wblock via StartCommand / accelerator | not_tested | — |
| menu.attdef | Draw > Block > Define Attributes... | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id attdef via StartCommand / accelerator | not_tested | — |
| menu.block | Draw > Block > Make... | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id block via StartCommand / accelerator | not_tested | — |
| menu.boundary | Draw > Boundary... | menu | crates/engine/src/cmd/hatch.rs (+ host chrome) | Menu bar path; command id boundary via StartCommand / accelerator | not_tested | — |
| menu.circle.2p | Draw > Circle > 2 Points | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id circle.2p via StartCommand / accelerator | not_tested | — |
| menu.circle.3p | Draw > Circle > 3 Points | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id circle.3p via StartCommand / accelerator | not_tested | — |
| menu.circle.cd | Draw > Circle > Center > Diameter | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id circle.cd via StartCommand / accelerator | not_tested | — |
| menu.circle | Draw > Circle > Center > Radius | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id circle via StartCommand / accelerator | not_tested | — |
| menu.circle.ttr | Draw > Circle > Tan > Tan > Radius | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id circle.ttr via StartCommand / accelerator | not_tested | — |
| menu.circle.ttt | Draw > Circle > Tan > Tan > Tan | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id circle.ttt via StartCommand / accelerator | not_tested | — |
| menu.xline | Draw > Construction Line | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id xline via StartCommand / accelerator | not_tested | — |
| menu.ellipse.arc | Draw > Ellipse > Arc | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id ellipse.arc via StartCommand / accelerator | not_tested | — |
| menu.ellipse.axis | Draw > Ellipse > Axis > End | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id ellipse.axis via StartCommand / accelerator | not_tested | — |
| menu.ellipse | Draw > Ellipse > Center | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id ellipse via StartCommand / accelerator | not_tested | — |
| menu.hatch | Draw > Hatch... | menu | crates/engine/src/cmd/hatch.rs (+ host chrome) | Menu bar path; command id hatch via StartCommand / accelerator | not_tested | — |
| menu.line | Draw > Line | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id line via StartCommand / accelerator | not_tested | — |
| menu.mline | Draw > Multiline | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id mline via StartCommand / accelerator | not_tested | — |
| menu.divide | Draw > Point > Divide | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id divide via StartCommand / accelerator | not_tested | — |
| menu.measure | Draw > Point > Measure | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id measure via StartCommand / accelerator | not_tested | — |
| menu.point.multiple | Draw > Point > Multiple Point | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id point.multiple via StartCommand / accelerator | not_tested | — |
| menu.point | Draw > Point > Single Point | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id point via StartCommand / accelerator | not_tested | — |
| menu.polygon | Draw > Polygon | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id polygon via StartCommand / accelerator | not_tested | — |
| menu.pline | Draw > Polyline | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id pline via StartCommand / accelerator | not_tested | — |
| menu.ray | Draw > Ray | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id ray via StartCommand / accelerator | not_tested | — |
| menu.rectang | Draw > Rectangle | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id rectang via StartCommand / accelerator | not_tested | — |
| menu.region | Draw > Region | menu | crates/engine/src/cmd/hatch.rs (+ host chrome) | Menu bar path; command id region via StartCommand / accelerator | not_tested | — |
| menu.revcloud.freehand | Draw > Revision Cloud > Freehand | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id revcloud.freehand via StartCommand / accelerator | not_tested | — |
| menu.revcloud.polygonal | Draw > Revision Cloud > Polygonal | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id revcloud.polygonal via StartCommand / accelerator | not_tested | — |
| menu.revcloud | Draw > Revision Cloud > Rectangular | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id revcloud via StartCommand / accelerator | not_tested | — |
| menu.spline.cv | Draw > Spline > Control Vertices | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id spline.cv via StartCommand / accelerator | not_tested | — |
| menu.spline | Draw > Spline > Fit Points | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id spline via StartCommand / accelerator | not_tested | — |
| menu.mtext | Draw > Text > Multiline Text... | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id mtext via StartCommand / accelerator | not_tested | — |
| menu.text | Draw > Text > Single Line Text | menu | crates/engine/src/cmd/draw.rs (+ host chrome) | Menu bar path; command id text via StartCommand / accelerator | not_tested | — |
| menu.wipeout | Draw > Wipeout | menu | crates/engine/src/cmd/draw2.rs (+ host chrome) | Menu bar path; command id wipeout via StartCommand / accelerator | not_tested | — |
| menu.erase.selection | Edit > Clear | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id erase.selection via StartCommand / accelerator | not_tested | — |
| menu.copyclip | Edit > Copy | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id copyclip via StartCommand / accelerator | not_tested | — |
| menu.copybase | Edit > Copy with Base Point | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id copybase via StartCommand / accelerator | not_tested | — |
| menu.cutclip | Edit > Cut | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id cutclip via StartCommand / accelerator | not_tested | — |
| menu.ai_deselect | Edit > Deselect All | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id ai_deselect via StartCommand / accelerator | not_tested | — |
| menu.filter | Edit > Filter... | menu | crates/engine/src/cmd/selectmore.rs (+ host chrome) | Menu bar path; command id filter via StartCommand / accelerator | not_tested | — |
| menu.cal | Edit > Find... | menu | crates/engine/src/cmd/utility.rs (+ host chrome) | Menu bar path; command id cal via StartCommand / accelerator | not_tested | — |
| menu.pasteclip | Edit > Paste | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id pasteclip via StartCommand / accelerator | not_tested | — |
| menu.pasteblock | Edit > Paste as Block | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id pasteblock via StartCommand / accelerator | not_tested | — |
| menu.pasteorig | Edit > Paste to Original Coordinates | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id pasteorig via StartCommand / accelerator | not_tested | — |
| menu.qselect | Edit > Quick Select... | menu | crates/engine/src/cmd/qselect.rs (+ host chrome) | Menu bar path; command id qselect via StartCommand / accelerator | not_tested | — |
| menu.redo | Edit > Redo | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id redo via StartCommand / accelerator | not_tested | — |
| menu.select | Edit > Select | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id select via StartCommand / accelerator | not_tested | — |
| menu.selectall | Edit > Select All | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id selectall via StartCommand / accelerator | not_tested | — |
| menu.selectsimilar | Edit > Select Similar | menu | crates/engine/src/cmd/selectmore.rs (+ host chrome) | Menu bar path; command id selectsimilar via StartCommand / accelerator | not_tested | — |
| menu.u | Edit > Undo | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id u via StartCommand / accelerator | not_tested | — |
| menu.undo | Edit > Undo Multiple | menu | crates/engine/src/cmd/edit.rs (+ host chrome) | Menu bar path; command id undo via StartCommand / accelerator | not_tested | — |
| menu.plot.batch | File > Batch Plot | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id plot.batch via StartCommand / accelerator | not_tested | — |
| menu.close | File > Close | menu | crates/engine/src/cmd/file.rs (+ host chrome) | Menu bar path; command id close via StartCommand / accelerator | not_tested | — |
| menu.drawing.revision | File > Close All | menu | crates/engine/src/cmd/file.rs (+ host chrome) | Menu bar path; command id drawing.revision via StartCommand / accelerator | not_tested | — |
| menu.audit | File > Drawing Utilities > Audit | menu | crates/engine/src/cmd/audit.rs (+ host chrome) | Menu bar path; command id audit via StartCommand / accelerator | not_tested | — |
| menu.recover | File > Drawing Utilities > Recover... | menu | crates/engine/src/cmd/audit.rs (+ host chrome) | Menu bar path; command id recover via StartCommand / accelerator | not_tested | — |
| menu.exportpdf | File > Export to PDF... | menu | crates/engine/src/cmd/layout.rs (+ host chrome) | Menu bar path; command id exportpdf via StartCommand / accelerator | not_tested | — |
| menu.export | File > Export... | menu | crates/engine/src/cmd/extract.rs (+ host chrome) | Menu bar path; command id export via StartCommand / accelerator | not_tested | — |
| menu.new | File > New Drawing... | menu | crates/engine/src/cmd/file.rs (+ host chrome) | Menu bar path; command id new via StartCommand / accelerator | not_tested | — |
| menu.project.setup | File > New Project | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id project.setup via StartCommand / accelerator | not_tested | — |
| menu.project.open | File > Open Project | menu | crates/engine/src/cmd/projfile.rs (+ host chrome) | Menu bar path; command id project.open via StartCommand / accelerator | not_tested | — |
| menu.open | File > Open... | menu | crates/engine/src/cmd/file.rs (+ host chrome) | Menu bar path; command id open via StartCommand / accelerator | not_tested | — |
| menu.layout.delete | File > Page Setup Manager... | menu | crates/engine/src/cmd/layout.rs (+ host chrome) | Menu bar path; command id layout.delete via StartCommand / accelerator | not_tested | — |
| menu.stylesmanager | File > Plot Styles... | menu | crates/engine/src/cmd/plotstyle.rs (+ host chrome) | Menu bar path; command id stylesmanager via StartCommand / accelerator | not_tested | — |
| menu.plot.window | File > Print... | menu | crates/engine/src/cmd/layout.rs (+ host chrome) | Menu bar path; command id plot.window via StartCommand / accelerator | not_tested | — |
| menu.qsave | File > Save | menu | crates/engine/src/cmd/file.rs (+ host chrome) | Menu bar path; command id qsave via StartCommand / accelerator | not_tested | — |
| menu.saveas | File > Save As... | menu | crates/engine/src/cmd/file.rs (+ host chrome) | Menu bar path; command id saveas via StartCommand / accelerator | not_tested | — |
| menu.hcwlayers | Format > Architectural Layer Set | menu | crates/engine/src/cmd/hcw.rs (+ host chrome) | Menu bar path; command id hcwlayers via StartCommand / accelerator | not_tested | — |
| menu.color | Format > Color... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id color via StartCommand / accelerator | not_tested | — |
| menu.style.list | Format > Dimension Style... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id style.list via StartCommand / accelerator | not_tested | — |
| menu.limits | Format > Drawing Limits | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id limits via StartCommand / accelerator | not_tested | — |
| menu.space.list | Format > Drawing Space | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.list via StartCommand / accelerator | not_tested | — |
| menu.space.activate | Format > Drawing Space > Activate | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.activate via StartCommand / accelerator | not_tested | — |
| menu.space.capture | Format > Drawing Space > Capture from Layers | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.capture via StartCommand / accelerator | not_tested | — |
| menu.space.delete | Format > Drawing Space > Delete | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.delete via StartCommand / accelerator | not_tested | — |
| menu.space.get | Format > Drawing Space > Details | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.get via StartCommand / accelerator | not_tested | — |
| menu.space.duplicate | Format > Drawing Space > Duplicate | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.duplicate via StartCommand / accelerator | not_tested | — |
| menu.space.update | Format > Drawing Space > Edit | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.update via StartCommand / accelerator | not_tested | — |
| menu.space.preview | Format > Drawing Space > Preview | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.preview via StartCommand / accelerator | not_tested | — |
| menu.space.save | Format > Drawing Space > Save | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.save via StartCommand / accelerator | not_tested | — |
| menu.space.recover | Format > Drawing Space > Show > Thaw and Unlock All | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.recover via StartCommand / accelerator | not_tested | — |
| menu.space.status | Format > Drawing Space > Status | menu | crates/engine/src/cmd/space.rs (+ host chrome) | Menu bar path; command id space.status via StartCommand / accelerator | not_tested | — |
| menu.hatch.patterns | Format > Hatch Editor... | menu | crates/engine/src/cmd/hatch.rs (+ host chrome) | Menu bar path; command id hatch.patterns via StartCommand / accelerator | not_tested | — |
| menu.layerstate.delete | Format > Layer States > Delete | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layerstate.delete via StartCommand / accelerator | not_tested | — |
| menu.layerstate.list | Format > Layer States > List | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layerstate.list via StartCommand / accelerator | not_tested | — |
| menu.layerstate.rename | Format > Layer States > Rename | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layerstate.rename via StartCommand / accelerator | not_tested | — |
| menu.layerstate.restore | Format > Layer States > Restore | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layerstate.restore via StartCommand / accelerator | not_tested | — |
| menu.layerstate.save | Format > Layer States Manager... | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layerstate.save via StartCommand / accelerator | not_tested | — |
| menu.laycur | Format > Layer Tools > Change to Current Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laycur via StartCommand / accelerator | not_tested | — |
| menu.layer.delete | Format > Layer Tools > Delete Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layer.delete via StartCommand / accelerator | not_tested | — |
| menu.layfrz | Format > Layer Tools > Freeze Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layfrz via StartCommand / accelerator | not_tested | — |
| menu.layiso | Format > Layer Tools > Isolate Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layiso via StartCommand / accelerator | not_tested | — |
| menu.laydel | Format > Layer Tools > Layer Delete | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laydel via StartCommand / accelerator | not_tested | — |
| menu.laymch | Format > Layer Tools > Layer Match | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laymch via StartCommand / accelerator | not_tested | — |
| menu.laymrg | Format > Layer Tools > Layer Merge | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laymrg via StartCommand / accelerator | not_tested | — |
| menu.layoff | Format > Layer Tools > Layer Off | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layoff via StartCommand / accelerator | not_tested | — |
| menu.laytrans | Format > Layer Tools > Layer Translate | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laytrans via StartCommand / accelerator | not_tested | — |
| menu.laywalk | Format > Layer Tools > Layer Walk | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laywalk via StartCommand / accelerator | not_tested | — |
| menu.laylck | Format > Layer Tools > Lock Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laylck via StartCommand / accelerator | not_tested | — |
| menu.layer.set | Format > Layer Tools > Make Current | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layer.set via StartCommand / accelerator | not_tested | — |
| menu.laymcur | Format > Layer Tools > Make Object's Layer Current | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laymcur via StartCommand / accelerator | not_tested | — |
| menu.layermap | Format > Layer Tools > Map Layers | menu | crates/engine/src/cmd/legacytools.rs (+ host chrome) | Menu bar path; command id layermap via StartCommand / accelerator | not_tested | — |
| menu.layer.new | Format > Layer Tools > New Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layer.new via StartCommand / accelerator | not_tested | — |
| menu.layerp | Format > Layer Tools > Previous Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layerp via StartCommand / accelerator | not_tested | — |
| menu.laythw | Format > Layer Tools > Thaw All Layers | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id laythw via StartCommand / accelerator | not_tested | — |
| menu.layon | Format > Layer Tools > Turn All Layers On | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layon via StartCommand / accelerator | not_tested | — |
| menu.layuniso | Format > Layer Tools > Unisolate Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layuniso via StartCommand / accelerator | not_tested | — |
| menu.layulk | Format > Layer Tools > Unlock Layer | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layulk via StartCommand / accelerator | not_tested | — |
| menu.layer | Format > Layers | menu | crates/engine/src/cmd/layer.rs (+ host chrome) | Menu bar path; command id layer via StartCommand / accelerator | not_tested | — |
| menu.symbol.desc | Format > Legend > Block Description… | menu | crates/engine/src/cmd/legend.rs (+ host chrome) | Menu bar path; command id symbol.desc via StartCommand / accelerator | not_tested | — |
| menu.legend.edit | Format > Legend > Edit Legend Entries… | menu | crates/engine/src/cmd/legend.rs (+ host chrome) | Menu bar path; command id legend.edit via StartCommand / accelerator | not_tested | — |
| menu.legend | Format > Legend > Layer Legend | menu | crates/engine/src/cmd/legend.rs (+ host chrome) | Menu bar path; command id legend via StartCommand / accelerator | not_tested | — |
| menu.symbol.legend | Format > Legend > Symbol Legend | menu | crates/engine/src/cmd/legend.rs (+ host chrome) | Menu bar path; command id symbol.legend via StartCommand / accelerator | not_tested | — |
| menu.linetype | Format > Linetype... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id linetype via StartCommand / accelerator | not_tested | — |
| menu.lweight | Format > Lineweight... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id lweight via StartCommand / accelerator | not_tested | — |
| menu.dimstyle.list | Format > Multileader Style... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id dimstyle.list via StartCommand / accelerator | not_tested | — |
| menu.notes.apply | Format > Notes > Apply Notes | menu | crates/engine/src/cmd/notes.rs (+ host chrome) | Menu bar path; command id notes.apply via StartCommand / accelerator | not_tested | — |
| menu.notes | Format > Notes > Edit Notes… | menu | crates/engine/src/cmd/notes.rs (+ host chrome) | Menu bar path; command id notes via StartCommand / accelerator | not_tested | — |
| menu.notes.list | Format > Notes > List Notes | menu | crates/engine/src/cmd/notes.rs (+ host chrome) | Menu bar path; command id notes.list via StartCommand / accelerator | not_tested | — |
| menu.notes.std | Format > Notes > Standard Notes | menu | crates/engine/src/cmd/notes.rs (+ host chrome) | Menu bar path; command id notes.std via StartCommand / accelerator | not_tested | — |
| menu.openings.table | Format > Opening Schedule > Draw Opening Schedule | menu | crates/engine/src/cmd/schedule.rs (+ host chrome) | Menu bar path; command id openings.table via StartCommand / accelerator | not_tested | — |
| menu.schedule | Format > Opening Schedule > Edit… | menu | crates/engine/src/cmd/schedule.rs (+ host chrome) | Menu bar path; command id schedule via StartCommand / accelerator | not_tested | — |
| menu.openings.std | Format > Opening Schedule > Standard Openings | menu | crates/engine/src/cmd/schedule.rs (+ host chrome) | Menu bar path; command id openings.std via StartCommand / accelerator | not_tested | — |
| menu.openings.sync | Format > Opening Schedule > Update Schedule | menu | crates/engine/src/cmd/schedule.rs (+ host chrome) | Menu bar path; command id openings.sync via StartCommand / accelerator | not_tested | — |
| menu.plotstyle | Format > Plot Style... | menu | crates/engine/src/cmd/plotstyle.rs (+ host chrome) | Menu bar path; command id plotstyle via StartCommand / accelerator | not_tested | — |
| menu.ddptype | Format > Point Style... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id ddptype via StartCommand / accelerator | not_tested | — |
| menu.rename | Format > Rename... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id rename via StartCommand / accelerator | not_tested | — |
| menu.style | Format > Text Style... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id style via StartCommand / accelerator | not_tested | — |
| menu.ltscale | Format > Units... | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id ltscale via StartCommand / accelerator | not_tested | — |
| menu.insunits | Format > Units... > Insertion Units | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id insunits via StartCommand / accelerator | not_tested | — |
| menu.blocktextedit | Insert > Block > Edit Text | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id blocktextedit via StartCommand / accelerator | not_tested | — |
| menu.purge | Insert > Block > List | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id purge via StartCommand / accelerator | not_tested | — |
| menu.blocktext | Insert > Block > List Text | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id blocktext via StartCommand / accelerator | not_tested | — |
| menu.-block | Insert > Block... | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id -block via StartCommand / accelerator | not_tested | — |
| menu.xref.attach | Insert > External Reference > Attach | menu | crates/engine/src/cmd/xref.rs (+ host chrome) | Menu bar path; command id xref.attach via StartCommand / accelerator | not_tested | — |
| menu.xref.bind | Insert > External Reference > Bind | menu | crates/engine/src/cmd/xref.rs (+ host chrome) | Menu bar path; command id xref.bind via StartCommand / accelerator | not_tested | — |
| menu.xref.detach | Insert > External Reference > Detach | menu | crates/engine/src/cmd/xref.rs (+ host chrome) | Menu bar path; command id xref.detach via StartCommand / accelerator | not_tested | — |
| menu.xref.list | Insert > External Reference > List | menu | crates/engine/src/cmd/xref.rs (+ host chrome) | Menu bar path; command id xref.list via StartCommand / accelerator | not_tested | — |
| menu.xref.reload | Insert > External Reference > Reload | menu | crates/engine/src/cmd/xref.rs (+ host chrome) | Menu bar path; command id xref.reload via StartCommand / accelerator | not_tested | — |
| menu.layout | Insert > Layout > New Layout | menu | crates/engine/src/cmd/layout.rs (+ host chrome) | Menu bar path; command id layout via StartCommand / accelerator | not_tested | — |
| menu.align | Modify > Align | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id align via StartCommand / accelerator | not_tested | — |
| menu.arraypath | Modify > Array > Path Array | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id arraypath via StartCommand / accelerator | not_tested | — |
| menu.arraypolar | Modify > Array > Polar Array | menu | crates/engine/src/cmd/array.rs (+ host chrome) | Menu bar path; command id arraypolar via StartCommand / accelerator | not_tested | — |
| menu.array | Modify > Array > Rectangular Array | menu | crates/engine/src/cmd/array.rs (+ host chrome) | Menu bar path; command id array via StartCommand / accelerator | not_tested | — |
| menu.break | Modify > Break | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id break via StartCommand / accelerator | not_tested | — |
| menu.breakatpoint | Modify > Break At Point | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id breakatpoint via StartCommand / accelerator | not_tested | — |
| menu.chamfer | Modify > Chamfer | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id chamfer via StartCommand / accelerator | not_tested | — |
| menu.reverse | Modify > Change Space | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id reverse via StartCommand / accelerator | not_tested | — |
| menu.xclip | Modify > Clip > Xref / Block | menu | crates/engine/src/cmd/clipcmd.rs (+ host chrome) | Menu bar path; command id xclip via StartCommand / accelerator | not_tested | — |
| menu.copy | Modify > Copy | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id copy via StartCommand / accelerator | not_tested | — |
| menu.overkill | Modify > Delete Duplicate Objects | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id overkill via StartCommand / accelerator | not_tested | — |
| menu.erase | Modify > Erase | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id erase via StartCommand / accelerator | not_tested | — |
| menu.explode | Modify > Explode | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id explode via StartCommand / accelerator | not_tested | — |
| menu.extend | Modify > Extend | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id extend via StartCommand / accelerator | not_tested | — |
| menu.fillet | Modify > Fillet | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id fillet via StartCommand / accelerator | not_tested | — |
| menu.flatten | Modify > Flatten Objects | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id flatten via StartCommand / accelerator | not_tested | — |
| menu.join | Modify > Join | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id join via StartCommand / accelerator | not_tested | — |
| menu.lengthen | Modify > Lengthen | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id lengthen via StartCommand / accelerator | not_tested | — |
| menu.properties.set | Modify > Match Properties | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id properties.set via StartCommand / accelerator | not_tested | — |
| menu.mirror | Modify > Mirror | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id mirror via StartCommand / accelerator | not_tested | — |
| menu.move | Modify > Move | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id move via StartCommand / accelerator | not_tested | — |
| menu.battman | Modify > Object > Attribute > Block Attribute Manager... | menu | crates/engine/src/cmd/blocks.rs (+ host chrome) | Menu bar path; command id battman via StartCommand / accelerator | not_tested | — |
| menu.-attedit | Modify > Object > Attribute > Global | menu | crates/engine/src/cmd/attedit.rs (+ host chrome) | Menu bar path; command id -attedit via StartCommand / accelerator | not_tested | — |
| menu.attedit | Modify > Object > Attribute > Single... | menu | crates/engine/src/cmd/attedit.rs (+ host chrome) | Menu bar path; command id attedit via StartCommand / accelerator | not_tested | — |
| menu.attdisp | Modify > Object > Attribute > Synchronize Attributes | menu | crates/engine/src/cmd/attribs.rs (+ host chrome) | Menu bar path; command id attdisp via StartCommand / accelerator | not_tested | — |
| menu.textedit | Modify > Object > Edit Text... | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id textedit via StartCommand / accelerator | not_tested | — |
| menu.hatchedit | Modify > Object > Hatch Editor... | menu | crates/engine/src/cmd/hatch.rs (+ host chrome) | Menu bar path; command id hatchedit via StartCommand / accelerator | not_tested | — |
| menu.pedit | Modify > Object > Polyline | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id pedit via StartCommand / accelerator | not_tested | — |
| menu.splinedit | Modify > Object > Spline | menu | crates/engine/src/cmd/modify2.rs (+ host chrome) | Menu bar path; command id splinedit via StartCommand / accelerator | not_tested | — |
| menu.offset | Modify > Offset | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id offset via StartCommand / accelerator | not_tested | — |
| menu.properties | Modify > Properties | menu | crates/engine/src/cmd/props.rs (+ host chrome) | Menu bar path; command id properties via StartCommand / accelerator | not_tested | — |
| menu.rotate | Modify > Rotate | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id rotate via StartCommand / accelerator | not_tested | — |
| menu.scale | Modify > Scale | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id scale via StartCommand / accelerator | not_tested | — |
| menu.stretch | Modify > Stretch | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id stretch via StartCommand / accelerator | not_tested | — |
| menu.trim | Modify > Trim | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id trim via StartCommand / accelerator | not_tested | — |
| menu.eattext | Tools > Attribute Extraction... | menu | crates/engine/src/cmd/extract.rs (+ host chrome) | Menu bar path; command id eattext via StartCommand / accelerator | not_tested | — |
| menu.lisp | Tools > AutoLISP | menu | crates/engine/src/cmd/scriptcmd.rs (+ host chrome) | Menu bar path; command id lisp via StartCommand / accelerator | not_tested | — |
| menu.bclose | Tools > Block Editor > Close | menu | crates/engine/src/cmd/bedit.rs (+ host chrome) | Menu bar path; command id bclose via StartCommand / accelerator | not_tested | — |
| menu.bedit | Tools > Block Editor > Open | menu | crates/engine/src/cmd/bedit.rs (+ host chrome) | Menu bar path; command id bedit via StartCommand / accelerator | not_tested | — |
| menu.bsave | Tools > Block Editor > Save | menu | crates/engine/src/cmd/bedit.rs (+ host chrome) | Menu bar path; command id bsave via StartCommand / accelerator | not_tested | — |
| menu.count | Tools > Count | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id count via StartCommand / accelerator | not_tested | — |
| menu.dataextraction | Tools > Data Extraction... | menu | crates/engine/src/cmd/extract.rs (+ host chrome) | Menu bar path; command id dataextraction via StartCommand / accelerator | not_tested | — |
| menu.adcenter | Tools > Design Center | menu | crates/engine/src/cmd/package.rs (+ host chrome) | Menu bar path; command id adcenter via StartCommand / accelerator | not_tested | — |
| menu.ortho | Tools > Drafting Settings... | menu | crates/engine/src/cmd/settings.rs (+ host chrome) | Menu bar path; command id ortho via StartCommand / accelerator | not_tested | — |
| menu.draworder.front | Tools > Draw Order > Bring to Front | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id draworder.front via StartCommand / accelerator | not_tested | — |
| menu.draworder.back | Tools > Draw Order > Send to Back | menu | crates/engine/src/cmd/modify.rs (+ host chrome) | Menu bar path; command id draworder.back via StartCommand / accelerator | not_tested | — |
| menu.dwgcompare | Tools > Drawing Compare... | menu | crates/engine/src/cmd/extract.rs (+ host chrome) | Menu bar path; command id dwgcompare via StartCommand / accelerator | not_tested | — |
| menu.refedit | Tools > Edit Reference | menu | crates/engine/src/cmd/bedit.rs (+ host chrome) | Menu bar path; command id refedit via StartCommand / accelerator | not_tested | — |
| menu.fonts.presets | Tools > Fonts > List Presets | menu | crates/engine/src/cmd/typography.rs (+ host chrome) | Menu bar path; command id fonts.presets via StartCommand / accelerator | not_tested | — |
| menu.fonts.migrate | Tools > Fonts > Migrate | menu | crates/engine/src/cmd/typography.rs (+ host chrome) | Menu bar path; command id fonts.migrate via StartCommand / accelerator | not_tested | — |
| menu.fonts.resolve | Tools > Fonts > Resolve Roles | menu | crates/engine/src/cmd/typography.rs (+ host chrome) | Menu bar path; command id fonts.resolve via StartCommand / accelerator | not_tested | — |
| menu.fonts.rollback | Tools > Fonts > Roll Back Migration | menu | crates/engine/src/cmd/typography.rs (+ host chrome) | Menu bar path; command id fonts.rollback via StartCommand / accelerator | not_tested | — |
| menu.fonts.scan | Tools > Fonts > Scan Drawing | menu | crates/engine/src/cmd/typography.rs (+ host chrome) | Menu bar path; command id fonts.scan via StartCommand / accelerator | not_tested | — |
| menu.group | Tools > Group | menu | crates/engine/src/cmd/groups.rs (+ host chrome) | Menu bar path; command id group via StartCommand / accelerator | not_tested | — |
| menu.angle.measure | Tools > Inquiry > Angle | menu | crates/engine/src/cmd/utility.rs (+ host chrome) | Menu bar path; command id angle.measure via StartCommand / accelerator | not_tested | — |
| menu.area | Tools > Inquiry > Area | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id area via StartCommand / accelerator | not_tested | — |
| menu.dist | Tools > Inquiry > Distance | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id dist via StartCommand / accelerator | not_tested | — |
| menu.id | Tools > Inquiry > ID Point | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id id via StartCommand / accelerator | not_tested | — |
| menu.drawing.inspect | Tools > Inquiry > Inspect Drawing | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id drawing.inspect via StartCommand / accelerator | not_tested | — |
| menu.list | Tools > Inquiry > List | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id list via StartCommand / accelerator | not_tested | — |
| menu.measuregeom | Tools > Inquiry > Measure Geometry | menu | crates/engine/src/cmd/utility.rs (+ host chrome) | Menu bar path; command id measuregeom via StartCommand / accelerator | not_tested | — |
| menu.entities | Tools > Inquiry > Query Entities | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id entities via StartCommand / accelerator | not_tested | — |
| menu.radius.measure | Tools > Inquiry > Radius | menu | crates/engine/src/cmd/utility.rs (+ host chrome) | Menu bar path; command id radius.measure via StartCommand / accelerator | not_tested | — |
| menu.setvar | Tools > Inquiry > Set Variable | menu | crates/engine/src/cmd/settings.rs (+ host chrome) | Menu bar path; command id setvar via StartCommand / accelerator | not_tested | — |
| menu.status | Tools > Inquiry > Status | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id status via StartCommand / accelerator | not_tested | — |
| menu.time | Tools > Inquiry > Time | menu | crates/engine/src/cmd/inquiry.rs (+ host chrome) | Menu bar path; command id time via StartCommand / accelerator | not_tested | — |
| menu.macro | Tools > Macro | menu | crates/engine/src/cmd/scriptcmd.rs (+ host chrome) | Menu bar path; command id macro via StartCommand / accelerator | not_tested | — |
| menu.script | Tools > Run Script... | menu | crates/engine/src/cmd/scriptcmd.rs (+ host chrome) | Menu bar path; command id script via StartCommand / accelerator | not_tested | — |
| menu.spatial.describe | Tools > Spatial > Describe | menu | crates/engine/src/cmd/spatialq.rs (+ host chrome) | Menu bar path; command id spatial.describe via StartCommand / accelerator | not_tested | — |
| menu.spatial.nearest | Tools > Spatial > Nearest | menu | crates/engine/src/cmd/spatialq.rs (+ host chrome) | Menu bar path; command id spatial.nearest via StartCommand / accelerator | not_tested | — |
| menu.spatial.overview | Tools > Spatial > Overview | menu | crates/engine/src/cmd/spatialq.rs (+ host chrome) | Menu bar path; command id spatial.overview via StartCommand / accelerator | not_tested | — |
| menu.spatial.query | Tools > Spatial > Query Window | menu | crates/engine/src/cmd/spatialq.rs (+ host chrome) | Menu bar path; command id spatial.query via StartCommand / accelerator | not_tested | — |
| menu.ungroup | Tools > Ungroup | menu | crates/engine/src/cmd/groups.rs (+ host chrome) | Menu bar path; command id ungroup via StartCommand / accelerator | not_tested | — |
| menu.validate | Tools > Validate | menu | crates/engine/src/cmd/extract.rs (+ host chrome) | Menu bar path; command id validate via StartCommand / accelerator | not_tested | — |
| menu.view.set | View > Layout > Switch | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id view.set via StartCommand / accelerator | not_tested | — |
| menu.pan.down | View > Pan > Down | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id pan.down via StartCommand / accelerator | not_tested | — |
| menu.pan.left | View > Pan > Left | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id pan.left via StartCommand / accelerator | not_tested | — |
| menu.pan | View > Pan > Realtime | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id pan via StartCommand / accelerator | not_tested | — |
| menu.pan.right | View > Pan > Right | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id pan.right via StartCommand / accelerator | not_tested | — |
| menu.pan.up | View > Pan > Up | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id pan.up via StartCommand / accelerator | not_tested | — |
| menu.redraw | View > Redraw | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id redraw via StartCommand / accelerator | not_tested | — |
| menu.regen | View > Regen | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id regen via StartCommand / accelerator | not_tested | — |
| menu.regenall | View > Regen All | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id regenall via StartCommand / accelerator | not_tested | — |
| menu.viewport.add | View > Viewports > Add | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id viewport.add via StartCommand / accelerator | not_tested | — |
| menu.vpclip | View > Viewports > Clip | menu | crates/engine/src/cmd/clipcmd.rs (+ host chrome) | Menu bar path; command id vpclip via StartCommand / accelerator | not_tested | — |
| menu.sheet.template.delete | View > Viewports > List | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id sheet.template.delete via StartCommand / accelerator | not_tested | — |
| menu.viewport.remove | View > Viewports > Remove | menu | crates/engine/src/cmd/sheet.rs (+ host chrome) | Menu bar path; command id viewport.remove via StartCommand / accelerator | not_tested | — |
| menu.zoom.all | View > Zoom > All | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.all via StartCommand / accelerator | not_tested | — |
| menu.zoom.extents | View > Zoom > Extents | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.extents via StartCommand / accelerator | not_tested | — |
| menu.zoom.in | View > Zoom > In | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.in via StartCommand / accelerator | not_tested | — |
| menu.zoom.object | View > Zoom > Object | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.object via StartCommand / accelerator | not_tested | — |
| menu.zoom.out | View > Zoom > Out | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.out via StartCommand / accelerator | not_tested | — |
| menu.zoom.previous | View > Zoom > Previous | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.previous via StartCommand / accelerator | not_tested | — |
| menu.zoom | View > Zoom > Realtime | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom via StartCommand / accelerator | not_tested | — |
| menu.zoom.window | View > Zoom > Window | menu | crates/engine/src/cmd/view.rs (+ host chrome) | Menu bar path; command id zoom.window via StartCommand / accelerator | not_tested | — |

## egui-only surfaces (not in WinUI shell)

Windows default desktop build uses WinUI (`apps/aad-winui`). The optional egui shell (`crates/ui-egui`, `aad --egui`) hosts additional or alternate UI:

| ID | Surface | Type | Source file | How opened | Testing status | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| egui.mtext-editor | Text Editor (MText) | dialog | crates/ui-egui/src/dialogs.rs | MText edit flow in egui | not_tested | — |
| egui.import-aqc | Import from AQC | dialog | crates/ui-egui/src/widgets.rs | egui widget window | not_tested | — |
| egui.customize-topbar | Customize Top Bar | dialog | crates/ui-egui/src/topbar.rs | View menu in egui | not_tested | — |
| egui.context-apply-space | Apply drawing space | dialog | crates/ui-egui/src/contextbar.rs | Context bar in egui | not_tested | — |
| egui.plot-style-alt | Plot style dialogs (egui layout) | manager | crates/ui-egui/src/plotstyles.rs | Parallel to WinUI `PlotStyleWindow` | not_tested | — |
| egui.layer-window | Layer manager (egui window) | manager | crates/ui-egui/src/layers.rs | Parallel to WinUI `LayerManagerWindow` | not_tested | — |
| egui.form-dialog | Form dialog (egui window) | dialog | crates/ui-egui/src/formdialog.rs | Parallel to WinUI `FormDialog` | not_tested | — |
| egui.panels.toolbars | Separate draw/modify left panels | toolbar | crates/ui-egui/src/toolbars.rs | Classic egui docking | not_tested | — |

## Scan notes

- WinUI project files scanned: `D:/Work Development/Repos/O-DRAFT/apps/aad-winui` (*.cs, *.xaml).
- Floating panels: `LayerManagerWindow` (standalone or merged), `FloatLeft` → **Panels** window.
- Managers: Layer Properties Manager, Parameters Manager, Linetype Manager, Page Setup Manager, Plot Style Manager.
- Engine menu entries enumerated: 384.

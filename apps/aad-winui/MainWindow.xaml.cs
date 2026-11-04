using System.Runtime.InteropServices.WindowsRuntime;
using System.Text;
using System.Text.Json;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.ApplicationModel.DataTransfer;
using Windows.Data.Pdf;
using Windows.Storage.Pickers;
using Windows.Storage.Streams;
using WinRT.Interop;

namespace Aad.WinUi;

public sealed partial class MainWindow : Window
{
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    private readonly LaunchArgs _launch;
    private IntPtr _host;
    private DispatcherQueueTimer? _timer;
    private int _ticks;
    private string _panelKey = "";
    private static readonly SideTab[] AllPanelTabs = [SideTab.Properties, SideTab.Layers, SideTab.Blocks, SideTab.Project];
    private readonly HashSet<SideTab> _stalePanels = new(AllPanelTabs);
    private string _docKey = "";
    private bool _buildingTabs;
    private bool _filling;
    private bool _historyOpen;
    private bool _asking;
    private bool _allowClose;
    private string _ownClipboard = "";
    private static readonly (string Id, string Label)[] ToolCatalog =
    [
        ("line", "Line"), ("pline", "Polyline"), ("circle", "Circle"), ("arc", "Arc"), ("rectang", "Rectangle"),
        ("move", "Move"), ("copy", "Copy"), ("rotate", "Rotate"), ("offset", "Offset"), ("trim", "Trim"),
        ("erase", "Erase"), ("dimlinear", "Dimension"), ("text", "Text"), ("hatch", "Hatch"),
    ];
    private List<string> _toolIds = ToolCatalog.Select(tool => tool.Id).ToList();
    private string _font = "engineering";
    private bool _showMenu = true;
    private bool _showTools = true;
    // Take-off tools are shown only while take-off mode is on (the Take-off button in the bottom bar).
    private bool _takeoffMode;
    private bool _showTabs = true;
    private bool _showStatus = true;
    private bool _showCommand = true;
    private bool _pinLeft = true;
    private bool _pinRight = true;
    private bool _mergeLayers;
    private bool _hoverLeft;
    private bool _leftCollapsed;
    private DateTime _leftIdleSince = DateTime.UtcNow;
    private double _leftWidth = Density.SidePanelWidth;
    private bool _ucs = true;
    private Window? _calc;
    private Window? _pomodoro;
    private Window? _toolsWindow;
    private Window? _leftFloat;
    private bool _classic = true;
    private readonly Dictionary<string, ShellPrefs.ModeSlot> _modeSlots = new();
    private bool _dark = SuiteAppearancePrefs.Load() == ElementTheme.Dark;
    private bool _themeSwitchSync;
    private Window? _pdfWindow;
    private Window? _formWindow;
    private LayerManagerWindow? _layersWindow;
    private Window? _unitsWindow;
    private Window? _textStyleWindow;
    private Window? _pageSetupWindow;
    private Window? _plotStyleWindow;
    private Window? _batchPlotWindow;
    private Window? _dimStyleWindow;
    private Window? _linetypeWindow;
    private Window? _parametersWindow;
    private Window? _graphWindow;
    private Window? _revisionsWindow;
    private Window? _historyWindow;
    private HistoryWindow.HistoryPanelController? _historyPanel;
    private RevisionsWindow.RevisionsPanelController? _revisionsPanel;
    private Window? _smartBlockWindow;
    private readonly HashSet<string> _openCatalogs = new(StringComparer.Ordinal);
    private Window? _hatchWindow;
    private Window? _pointStyleWindow;
    private Window? _mleaderStyleWindow;
    private Window? _roomSettingsWindow;
    private string _space = "Model";

    /// <summary>The tool column's two panels; made in code and put into the slots of the XAML.</summary>
    private readonly ToolColumnPanel ToolStrip = new();
    private readonly ToolColumnPanel ArchBar = new();
    private bool _plotBusy;
    private DraftingWindow? _draftingWindow;
    private QuickWindow? _quickWindow;
    private QuickSelectWindow? _quickSelectWindow;
    private string? _quickJson;
    private string? _hoverQuickJson;
    private string _command = "";
    private bool _qpmode;
    private bool _literalSpace;
    private bool _escapeHeld;
    private CommandChrome? _chrome;
    /// <summary>Cursor CAD TextBox (from <see cref="CommandChrome"/>).</summary>
    private TextBox CommandInput => _chrome?.CadInput ?? AiInput;
    /// <summary>CAD autocomplete list (from <see cref="CommandChrome"/>).</summary>
    private ListView? SuggestList => _chrome?.SuggestList;
    private bool _shellClosing;
    private bool _armSpace;
    private readonly List<string> _typed = new();
    private int _recall = -1;
    private string _liveDraft = "";
    private string _historyShown = "\u0001";
    private bool _placingBar;
    private int _lastBarX = int.MinValue;
    private int _lastBarY = int.MinValue;
    private int _lastBarW;
    private int _lastBarH;
    private int _lastDynX = int.MinValue;
    private int _lastDynY = int.MinValue;
    private int _lastDynW;
    private int _lastDynH;
    private bool _lastDynOn;
    private double _barDx;
    private double _barDy;
    /// <summary>Shortcut ? command id, built from the engine menu registry (same strings as the menu).</summary>
    private readonly Dictionary<(bool Ctrl, bool Shift, Windows.System.VirtualKey Key), string> _menuKeys = new();
    private long _accelAt;
    private string _accelLast = "";

    internal void HoldForm(Window? window) => _formWindow = window;

    public MainWindow(LaunchArgs? launch = null)
    {
        _launch = launch ?? LaunchArgs.Parse(Environment.GetCommandLineArgs().Skip(1));
        InitializeComponent();
        ToolStripSlot.Child = ToolStrip;
        ArchBarSlot.Child = ArchBar;
        AppWindow.Closing += OnAppClosing;
        // Sample only when asked and no drawing path was given (paths replace the empty session).
        _host = Native.Create(sample: _launch.Sample && _launch.Files.Count == 0);
        // Sync GPU paper with the suite preference before the child is embedded (default host is dark).
        if (_host != IntPtr.Zero)
        {
            Native.Theme(_host, _dark);
        }
        AppWindow.Changed += (_, args) =>
        {
            if (!_shellClosing && (args.DidPositionChange || args.DidSizeChange))
            {
                PlaceCommandBar();
            }
        };
        Closed += (_, _) =>
        {
            ActiveProjectPointer.Set(null);
            TearDownShell();
        };
        Activated += (_, _) =>
        {
            PlaceChild();
            PlaceCommandBar();
        };
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        SplashLogo.Source = ShellIcon.Logo();
        OpenCommandChrome();
        LoadShell();
        BuildMenus();
        RefreshChrome();
        RefreshPanels();
        ApplyShellTheme();
        Listen();
        _ = OpenLaunchFiles();
        ApplyAccess();
        EnsureUnitsBox();
        PlaceCommandBar();
        EnsureTimer();
        UpdateDyn();
        _chrome?.FocusCad();
        _ = HideSplashSoon();
    }

    /// <summary>Open the files given on the command line, one after the other, without blocking the window.</summary>
    private async Task OpenLaunchFiles()
    {
        var any = false;
        foreach (var path in _launch.Files)
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                continue;
            }
            // A project the launcher just made has a folder but no project file yet: the first drawing creates it.
            if (ProjectLock.IsLockedFile(path) && !File.Exists(path))
            {
                var folder = System.IO.Path.GetDirectoryName(path);
                if (string.IsNullOrEmpty(folder) || !Directory.Exists(folder))
                {
                    History.Text = "The folder of the locked project was not found: " + folder + ". Choose the project again in the launcher.";
                    continue;
                }
                Native.Execute(_host, "new", "{}");
                if (!await SaveFileAsync(path))
                {
                    History.Text = Native.LastError(_host);
                    continue;
                }
                ShellPrefs.RememberRecent(path);
                any = true;
                _docKey = "";
                continue;
            }
            if (!await OpenFileAsync(path))
            {
                History.Text = Native.LastError(_host);
                continue;
            }
            ShellPrefs.RememberRecent(path);
            any = true;
            _docKey = "";
        }
        if (any)
        {
            // One drawing per window: a launch with several files keeps the last one.
            await KeepOnlyActiveDrawingAsync();
            HideWelcome();
            AfterCommand();
        }
    }

    private void ApplyAccess()
    {
        AutomationProperties.SetLiveSetting(PromptLive, AutomationLiveSetting.Assertive);
        AutomationProperties.SetLiveSetting(PromptLabel, AutomationLiveSetting.Assertive);
        AutomationProperties.SetLiveSetting(Status, AutomationLiveSetting.Polite);
        AutomationProperties.SetLandmarkType(Bar, AutomationLandmarkType.Navigation);
        AutomationProperties.SetLandmarkType(ToolStripHost, AutomationLandmarkType.Navigation);
        BuildSideRail();
        AutomationProperties.SetLandmarkType(LeftColumn, AutomationLandmarkType.Form);
        AutomationProperties.SetLocalizedLandmarkType(LeftColumn, "complementary");
        AutomationProperties.SetLandmarkType(CanvasHost, AutomationLandmarkType.Main);
        AutomationProperties.SetLocalizedLandmarkType(StatusRow, "status");
        AnnouncePrompt(PromptLabel.Text ?? "Command:", "");
        AutomationProperties.SetName(LeftSplitter, "Resize side panel, width " + (int)_leftWidth);
    }

    private void OnCanvasSize(object sender, SizeChangedEventArgs e)
    {
        PlaceChild();
        EnsureTimer();
    }

    /// <summary>Keys on the cursor CAD prompt (not the fixed AI field).</summary>
    private void OnCadKey(object sender, KeyRoutedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (ApplyUndoRedo(e))
        {
            return;
        }
        if (KeyDown(Windows.System.VirtualKey.Control) && e.Key is Windows.System.VirtualKey.Number9 or Windows.System.VirtualKey.NumberPad9)
        {
            e.Handled = true;
            _showCommand = false;
            ApplyChrome();
            SaveShell();
            return;
        }
        var ctrl = KeyDown(Windows.System.VirtualKey.Control);
        var shift = KeyDown(Windows.System.VirtualKey.Shift);
        if (ApplyFunctionKey(e, ctrl, shift))
        {
            return;
        }
        if (ApplyDraftKeys(e, ctrl, shift, true))
        {
            return;
        }
        if (ctrl && e.Key is not Windows.System.VirtualKey.Number9 and not Windows.System.VirtualKey.NumberPad9)
        {
            _armSpace = false;
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Enter || e.Key == Windows.System.VirtualKey.Space)
        {
            // Always consume so the TextBox cannot type a space instead of activating.
            e.Handled = true;
            if (!SubmitKey(e.Key) && e.Key == Windows.System.VirtualKey.Space)
            {
                InsertCommandSpace();
            }
            return;
        }
        _armSpace = false;
        if (e.Key == Windows.System.VirtualKey.Escape)
        {
            e.Handled = true;
            _escapeHeld = true;
            if (SuggestList is { Visibility: Visibility.Visible, Items.Count: > 0 })
            {
                SuggestList.Items.Clear();
                SuggestList.Visibility = Visibility.Collapsed;
                UpdateDyn();
                return;
            }
            if (CloseOpenComboBoxes())
            {
                return;
            }
            if (!CanvasMenu.Dismiss())
            {
                CancelCommand();
            }
        }
        else if (e.Key == Windows.System.VirtualKey.Tab)
        {
            var next = Native.DynTab(_host, CommandInput.Text);
            if (next != null)
            {
                e.Handled = true;
                CommandInput.Text = next;
                CommandInput.SelectionStart = next.Length;
            }
            else if (TryAcceptTopSuggestion(run: false))
            {
                e.Handled = true;
            }
            else if (string.IsNullOrEmpty(CommandInput.Text))
            {
                e.Handled = true;
                Native.SnapTab(_host);
            }
        }
    }

    /// <summary>Keys on the fixed AI prompt. Enter asks; Space is literal; never CAD repeat.</summary>
    private void OnAiKey(object sender, KeyRoutedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (ApplyUndoRedo(e))
        {
            return;
        }
        var ctrl = KeyDown(Windows.System.VirtualKey.Control);
        var shift = KeyDown(Windows.System.VirtualKey.Shift);
        if (ApplyFunctionKey(e, ctrl, shift))
        {
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            e.Handled = true;
            SubmitAiAsk();
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Escape)
        {
            e.Handled = true;
            _escapeHeld = true;
            if (!string.IsNullOrEmpty(AiInput.Text))
            {
                AiInput.Text = "";
                return;
            }
            CancelCommand();
            return;
        }
        _armSpace = false;
    }

    /// <summary>WinUI TextBox still emits Space via CharacterReceived after PreviewKeyDown.Handled.</summary>
    private void OnCadCharacter(object sender, CharacterReceivedRoutedEventArgs e)
    {
        if (e.Character == ' ')
        {
            e.Handled = true;
        }
    }

    private void OnDraft(object sender, TextChangedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var text = CommandInput.Text ?? "";
        if (Native.SlashTransfer(_host, text))
        {
            TransferSlashToAi(text);
            return;
        }
        Native.Draft(_host, text);
        RefreshSuggestions();
    }

    private void TransferSlashToAi(string cadText)
    {
        var payload = Native.SlashPayload(cadText);
        CommandInput.Text = "";
        if (_host != IntPtr.Zero)
        {
            Native.Draft(_host, "");
        }
        if (SuggestList != null)
        {
            SuggestList.Items.Clear();
            SuggestList.Visibility = Visibility.Collapsed;
        }
        AiInput.Text = payload;
        AiInput.SelectionStart = payload.Length;
        AiInput.Focus(FocusState.Programmatic);
        UpdateDyn();
    }

    private void OnAskSend(object sender, RoutedEventArgs e) => SubmitAiAsk();

    private void SubmitAiAsk()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var text = (AiInput.Text ?? "").Trim();
        if (text.Length == 0)
        {
            return;
        }
        _armSpace = false;
        AiInput.Text = "";
        if (Native.Ask(_host, text) != 0)
        {
            History.Text = Native.LastError(_host);
            SyncHistory();
        }
        RefreshChrome();
        RefreshPanels();
        BuildMenus();
        AiInput.Focus(FocusState.Programmatic);
    }

    private void CancelCommand()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        CommandInput.Text = "";
        AiInput.Text = "";
        _armSpace = false;
        if (SuggestList != null)
        {
            SuggestList.Items.Clear();
            SuggestList.Visibility = Visibility.Collapsed;
        }
        if (Native.Cancel(_host) != 0)
        {
            Status.Text = Native.LastError(_host);
        }
        RefreshChrome();
        RefreshPanels();
        BuildMenus();
        UpdateDyn();
        _chrome?.FocusCad();
        // Escape / CAD focus must not leave the main shell with WS_VISIBLE cleared.
        KeepShellShown();
    }

    /// <summary>
    /// Activate a tool window and keep the CAD shell visible. On some builds, focusing a second
    /// WinUI window (or the owned CAD chrome) clears WS_VISIBLE on the main shell.
    /// </summary>
    private void PresentToolWindow(Window tool)
    {
        tool.Activate();
        Native.ShowWindow(WindowNative.GetWindowHandle(tool), Native.SwShowNa);
        KeepShellShown();
    }

    private void KeepShellShown()
    {
        var hwnd = WindowNative.GetWindowHandle(this);
        if (hwnd == IntPtr.Zero || Native.IsIconic(hwnd))
        {
            return;
        }
        Native.ShowWindow(hwnd, Native.SwShowNa);
    }

    /// <summary>
    /// Escape while the Win32 drawing child has focus never reaches XAML PreviewKeyDown.
    /// Edge-detect the key here so an active command or busy assistant still cancels.
    /// </summary>
    private void PollEscapeCancel()
    {
        var down = KeyDown(Windows.System.VirtualKey.Escape);
        if (down && !_escapeHeld)
        {
            if (Content is FrameworkElement root && root.XamlRoot != null)
            {
                var focused = FocusManager.GetFocusedElement(root.XamlRoot);
                if (focused is TextBox box && !ReferenceEquals(box, CommandInput) && !ReferenceEquals(box, AiInput))
                {
                    _escapeHeld = true;
                    return;
                }
                // Combo flyouts often focus a list item, not the ComboBox ? close open dropdowns first.
                if (focused is ComboBox || CloseOpenComboBoxes())
                {
                    CloseOpenComboBoxes();
                    _escapeHeld = true;
                    return;
                }
            }
            else if (CloseOpenComboBoxes())
            {
                _escapeHeld = true;
                return;
            }
            if (!CanvasMenu.Dismiss())
            {
                CancelCommand();
            }
        }
        _escapeHeld = down;
    }

    /// <summary>Closes arch-bar / status ComboBox dropdowns. Escape must dismiss these before CAD cancel.</summary>
    private bool CloseOpenComboBoxes()
    {
        var closed = false;
        foreach (var box in new ComboBox?[] { LayerBox, UnitsBox, PaperBox, OrientBox })
        {
            if (box is { IsDropDownOpen: true })
            {
                box.IsDropDownOpen = false;
                closed = true;
            }
        }
        return closed;
    }

    private void OnMenuCommand(object sender, RoutedEventArgs e)
    {
        if (_host == IntPtr.Zero || sender is not MenuFlyoutItem item || item.Tag is not string id)
        {
            return;
        }
        StartCommand(id);
    }

    private void StartCommand(string id)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (id is "open" or "qsave" or "saveas")
        {
            _ = PickFile(id);
            return;
        }
        if (id == "import")
        {
            _ = ImportFile();
            return;
        }
        if (RccTools.TryGetValue(id, out var rccBlock))
        {
            // Built-in structural tools: the block goes in with its defaults; the reinforcement is then edited in Properties (IS 456).
            InsertLibrarySmartBlock(rccBlock);
            return;
        }
        if (id == "new")
        {
            _ = NewDrawingReplacingAsync();
            return;
        }
        if (id == "page.manager")
        {
            OpenPageLayoutManager();
            return;
        }
        if (id == "notes.manager")
        {
            OpenNotesManager();
            return;
        }
        if (id == "close")
        {
            _ = CloseActive();
            return;
        }
        if (id is "layer" or "la" or "layers" or "ui.dialog.layers")
        {
            OpenLayers();
            return;
        }
        if (id == "ui.panel.layers")
        {
            ShowSidePanel(SideTab.Layers);
            return;
        }
        if (id is "ui.panel.properties" or "properties")
        {
            ShowSidePanel(SideTab.Properties);
            return;
        }
        if (id is "ui.dialog.blocks" or "blocks")
        {
            ShowSidePanel(SideTab.Blocks);
            return;
        }
        if (id is "units" or "un")
        {
            OpenUnits();
            return;
        }
        if (id is "ui.dialog.about")
        {
            AboutWindow.Show(this, VersionText());
            return;
        }
        if (id is "ui.dialog.commands")
        {
            CommandList.Show(this, Native.ReadText(_host, Native.TextMenu));
            return;
        }
        if (id is "ui.dialog.customizetop")
        {
            OpenTools();
            return;
        }
        if (id is "ui.dialog.parameters")
        {
            OpenParameters();
            return;
        }
        if (id is "revision.list" or "revision" or "revlist" or "revisions" or "ui.dialog.revisions")
        {
            OpenRevisions();
            return;
        }
        if (id is "history.list" or "hlist" or "ui.dialog.history")
        {
            OpenHistory();
            return;
        }
        if (id is "insert" or "smartblock.insert" or "sbinsert" or "smartblock" or "ui.dialog.smartblocks" or "ui.dialog.blocks")
        {
            OpenSmartBlocks();
            return;
        }
        if (id == "ui.workspace.classic")
        {
            SwitchWorkspace(classic: true);
            return;
        }
        if (id == "ui.workspace.pro")
        {
            SwitchWorkspace(classic: false);
            return;
        }
        if (id is "ui.workspace.toggle")
        {
            SwitchWorkspace(!_classic);
            return;
        }
        if (id is "ui.workspace.reset" or "resetui" or "ui.dock.reset")
        {
            ResetCurrentWorkspace();
            return;
        }
        if (id == "ui.workspace")
        {
            History.Text = _classic ? "Workspace: Classic" : "Workspace: Pro";
            return;
        }
        if (id is "style" or "st")
        {
            OpenTextStyle();
            return;
        }
        if (id is "plot" or "print" or "exportpdf")
        {
            _ = BeginPlot(id);
            return;
        }
        if (id == "pagesetup")
        {
            OpenPageSetup();
            return;
        }
        if (id == "stylesmanager")
        {
            OpenPlotStyles();
            return;
        }
        if (id == "plot.batch")
        {
            OpenBatchPlot();
            return;
        }
        if (id is "dimstyle" or "dimstyle.dimension")
        {
            OpenDimStyle();
            return;
        }
        if (id is "linetype" or "lt" or "ltype")
        {
            OpenLinetype();
            return;
        }
        if (id is "hatchedit" or "he" or "hatch.patterns")
        {
            OpenHatch();
            return;
        }
        if (id == "ddptype")
        {
            OpenPointStyle();
            return;
        }
        if (id is "mleaderstyle" or "mls")
        {
            OpenMLeaderStyle();
            return;
        }
        if (id == "project.wizard")
        {
            OpenProjectWizard();
            return;
        }
        if (id == "level.set")
        {
            OpenStorySettings();
            return;
        }
        if (id == "level.import")
        {
            _ = OpenImportLevel();
            return;
        }
        if (id == "room.settings")
        {
            OpenRoomSettings();
            return;
        }
        if (id is "dsettings" or "ds" or "se" or "ui.dialog.dsettings")
        {
            OpenDrafting();
            return;
        }
        if (id is "qselect" or "qs" or "ui.dialog.qselect")
        {
            OpenQuickSelect();
            return;
        }
        if (id is "cal" or "quickcalc" or "qc" or "calc")
        {
            OpenCalculator();
            return;
        }
        var form = Native.ReadForm(_host, id);
        if (form != null)
        {
            if ((id is "wall.door" or "wall.window") && !string.IsNullOrEmpty(_selectedBlock))
            {
                form = InjectFormField(form, "block", _selectedBlock!);
            }
            FormDialog.Show(this, _host, form, AfterCommand);
            return;
        }
        Native.CommandLine(_host, id);
        AfterCommand();
    }

    /// <summary>Set a scalar form field's value (e.g. door/window block from the Blocks panel).</summary>
    private static string InjectFormField(string formJson, string key, string value)
    {
        try
        {
            using var doc = JsonDocument.Parse(formJson);
            var root = doc.RootElement.Clone();
            using var stream = new MemoryStream();
            using (var writer = new Utf8JsonWriter(stream))
            {
                writer.WriteStartObject();
                foreach (var prop in root.EnumerateObject())
                {
                    if (prop.NameEquals("fields"))
                    {
                        writer.WritePropertyName("fields");
                        writer.WriteStartArray();
                        foreach (var field in prop.Value.EnumerateArray())
                        {
                            writer.WriteStartObject();
                            foreach (var fp in field.EnumerateObject())
                            {
                                if (fp.NameEquals("value") && field.TryGetProperty("key", out var k) && k.GetString() == key)
                                {
                                    writer.WriteString("value", value);
                                }
                                else
                                {
                                    fp.WriteTo(writer);
                                }
                            }
                            if (!field.TryGetProperty("value", out _) && field.TryGetProperty("key", out var fk) && fk.GetString() == key)
                            {
                                writer.WriteString("value", value);
                            }
                            writer.WriteEndObject();
                        }
                        writer.WriteEndArray();
                    }
                    else
                    {
                        prop.WriteTo(writer);
                    }
                }
                writer.WriteEndObject();
            }
            return Encoding.UTF8.GetString(stream.ToArray());
        }
        catch (JsonException)
        {
            return formJson;
        }
    }

    private void OnShellKey(object sender, KeyRoutedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Escape)
        {
            e.Handled = true;
            _escapeHeld = true;
            if (!CanvasMenu.Dismiss())
            {
                CancelCommand();
            }
            return;
        }
        if (ApplyUndoRedo(e))
        {
            return;
        }
        var ctrl = KeyDown(Windows.System.VirtualKey.Control);
        var shift = KeyDown(Windows.System.VirtualKey.Shift);
        if (e.Key == Windows.System.VirtualKey.F6)
        {
            e.Handled = true;
            _showCommand = true;
            ApplyChrome();
            _chrome?.FocusCad();
            return;
        }
        if (ApplyFunctionKey(e, ctrl, shift))
        {
            return;
        }
        if (Typing())
        {
            return;
        }
        if (ApplyDraftKeys(e, ctrl, shift, false))
        {
            return;
        }
        if (e.Key == Windows.System.VirtualKey.F1)
        {
            e.Handled = true;
            CommandList.Show(this, Native.ReadText(_host, Native.TextMenu));
            return;
        }
        if (e.Key == Windows.System.VirtualKey.F2)
        {
            e.Handled = true;
            SetHistoryOpen(!_historyOpen);
            return;
        }
        if (ctrl && e.Key == Windows.System.VirtualKey.F11)
        {
            e.Handled = true;
            if (shift)
            {
                ResetCurrentWorkspace();
            }
            else
            {
                SwitchWorkspace(!_classic);
            }
            return;
        }
        if (ctrl && e.Key == Windows.System.VirtualKey.W)
        {
            e.Handled = true;
            _ = CloseActive();
            return;
        }
        if (ctrl && e.Key == Windows.System.VirtualKey.Q)
        {
            e.Handled = true;
            _ = CloseAll(quit: true);
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Menu && !_showMenu)
        {
            e.Handled = true;
            _showMenu = true;
            ApplyChrome();
            SaveShell();
            return;
        }
        if (ctrl && e.Key is Windows.System.VirtualKey.Number9 or Windows.System.VirtualKey.NumberPad9)
        {
            e.Handled = true;
            _showCommand = !_showCommand;
            ApplyChrome();
            SaveShell();
            if (_showCommand)
            {
                _chrome?.FocusCad();
            }
            return;
        }
        if (TryMenuKey(ctrl, shift, e.Key, out var id))
        {
            e.Handled = true;
            InvokeAccelId(id);
            return;
        }
        if (ctrl || e.Key == Windows.System.VirtualKey.Menu)
        {
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Enter || e.Key == Windows.System.VirtualKey.Space)
        {
            e.Handled = true;
            if (!SubmitKey(e.Key) && e.Key == Windows.System.VirtualKey.Space)
            {
                InsertCommandSpace();
            }
            return;
        }
        _armSpace = false;
        if (e.Key == Windows.System.VirtualKey.Back && CommandInput.Text.Length > 0)
        {
            e.Handled = true;
            CommandInput.Text = CommandInput.Text[..^1];
            CommandInput.SelectionStart = CommandInput.Text.Length;
            CommandInput.Focus(FocusState.Programmatic);
            return;
        }
        var typed = Native.Character(e.Key);
        if (string.IsNullOrEmpty(typed))
        {
            return;
        }
        e.Handled = true;
        CommandInput.Text += typed;
        CommandInput.SelectionStart = CommandInput.Text.Length;
        CommandInput.Focus(FocusState.Programmatic);
    }

    /// <summary>
    /// Function keys work wherever the focus is, including the command line (as in other CAD
    /// programs): F1 command reference, F2 history, Ctrl+F11 workspace, and the registry's own
    /// F-key shortcuts (F3 object snap, F8 ortho, F10 polar, F11 object snap tracking, F12 dynamic
    /// input). F6 focuses the command line and is handled by the caller.
    /// </summary>
    private bool ApplyFunctionKey(KeyRoutedEventArgs e, bool ctrl, bool shift)
    {
        if (e.Key is < Windows.System.VirtualKey.F1 or > Windows.System.VirtualKey.F12 || e.Key == Windows.System.VirtualKey.F6)
        {
            return false;
        }
        if (e.Key == Windows.System.VirtualKey.F1 && !ctrl && !shift)
        {
            e.Handled = true;
            CommandList.Show(this, Native.ReadText(_host, Native.TextMenu));
            return true;
        }
        if (e.Key == Windows.System.VirtualKey.F2 && !ctrl && !shift)
        {
            e.Handled = true;
            SetHistoryOpen(!_historyOpen);
            return true;
        }
        if (ctrl && e.Key == Windows.System.VirtualKey.F11)
        {
            e.Handled = true;
            if (shift)
            {
                ResetCurrentWorkspace();
            }
            else
            {
                SwitchWorkspace(!_classic);
            }
            return true;
        }
        if (TryMenuKey(ctrl, shift, e.Key, out var id))
        {
            e.Handled = true;
            InvokeAccelId(id);
            return true;
        }
        return false;
    }

    private bool TryMenuKey(bool ctrl, bool shift, Windows.System.VirtualKey key, out string id) =>
        _menuKeys.TryGetValue((ctrl, shift, key), out id!);

    /// <summary>Run a menu/registry shortcut the same way as a menu click or accelerator.</summary>
    private void InvokeAccelId(string id)
    {
        if (id.Length == 0)
        {
            return;
        }
        var now = Environment.TickCount64;
        if (id == _accelLast && now - _accelAt < 80)
        {
            return;
        }
        _accelAt = now;
        _accelLast = id;
        if (id is "open" or "qsave" or "saveas")
        {
            _ = PickFile(id);
            return;
        }
        if (id == "import")
        {
            _ = ImportFile();
            return;
        }
        if (id is "pasteclip" or "pasteblock")
        {
            _ = Paste(id);
            return;
        }
        if (id is "copyclip" or "cutclip" or "copybase" or "u" or "redo" or "selectall" or "erase" or "erase.selection"
            or "new" or "osnap" or "ortho" or "polar" or "otrack" or "dynmode")
        {
            RunShortcut(id == "erase.selection" ? "erase" : id);
            return;
        }
        StartCommand(id);
    }

    /// <summary>Enter, or Space when it confirms. One path so the two keys cannot run a line twice.
    /// Leading `/` or an open quote: Space is literal. Idle empty: first Space arms, second repeats.
    /// While a command is running, empty Space is Enter. Non-empty: complete first suggestion then run.</summary>
    private bool SubmitKey(Windows.System.VirtualKey key)
    {
        if (key == Windows.System.VirtualKey.Space)
        {
            var text = CommandInput.Text ?? "";
            // Live buffer only — do not rely solely on cached `_literalSpace`.
            if (text.TrimStart().StartsWith('/') || (_host != IntPtr.Zero && Native.SpaceLiteral(_host, text)))
            {
                _armSpace = false;
                return false;
            }
            var trimmed = text.Trim();
            if (trimmed.Length != text.Length)
            {
                CommandInput.Text = trimmed;
                text = trimmed;
            }
            if (string.IsNullOrEmpty(text))
            {
                // Active command: Space confirms, never arms double-Space.
                if (!string.IsNullOrEmpty(_command))
                {
                    _armSpace = false;
                    SubmitLine();
                    return true;
                }
                if (!_armSpace)
                {
                    _armSpace = true;
                    return true;
                }
                _armSpace = false;
                SubmitLine();
                return true;
            }
            _armSpace = false;
            if (TryAcceptTopSuggestion(run: true))
            {
                return true;
            }
            SubmitLine();
            return true;
        }
        _armSpace = false;
        var enterText = (CommandInput.Text ?? "").Trim();
        if (enterText.Length != (CommandInput.Text ?? "").Length)
        {
            CommandInput.Text = enterText;
        }
        if (!string.IsNullOrEmpty(enterText) && TryAcceptTopSuggestion(run: true))
        {
            return true;
        }
        SubmitLine();
        return true;
    }

    private void RefreshSuggestions()
    {
        if (_host == IntPtr.Zero || SuggestList == null)
        {
            return;
        }
        if (!string.IsNullOrEmpty(_command))
        {
            SuggestList.Items.Clear();
            SuggestList.Visibility = Visibility.Collapsed;
            return;
        }
        var text = CommandInput.Text ?? "";
        List<SuggestRow> rows;
        try
        {
            rows = JsonSerializer.Deserialize<List<SuggestRow>>(Native.Suggest(_host, text), JsonOpts) ?? new();
        }
        catch (Exception)
        {
            rows = new();
        }
        SuggestList.Items.Clear();
        foreach (var row in rows.Take(8))
        {
            if (string.IsNullOrEmpty(row.Insert))
            {
                continue;
            }
            var label = string.IsNullOrEmpty(row.Label) ? row.Insert : $"{row.Insert}  —  {row.Label}";
            SuggestList.Items.Add(new SuggestRow { Insert = row.Insert, Label = label, Reference = row.Reference });
        }
        SuggestList.Visibility = SuggestList.Items.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        UpdateDyn();
    }

    private void OnSuggestPick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not SuggestRow row || string.IsNullOrEmpty(row.Insert))
        {
            return;
        }
        AcceptSuggestion(row.Insert, row.Reference);
    }

    private void AcceptSuggestion(string insert, bool reference)
    {
        var text = CommandInput.Text ?? "";
        var word = CurrentWord(text);
        var cut = text.Length - word.Length;
        var next = text[..cut] + insert + (reference ? " " : "");
        CommandInput.Text = next;
        CommandInput.SelectionStart = next.Length;
        CommandInput.Focus(FocusState.Programmatic);
        RefreshSuggestions();
    }

    private static string CurrentWord(string buffer)
    {
        for (var i = buffer.Length - 1; i >= 0; i--)
        {
            if (char.IsWhiteSpace(buffer[i]))
            {
                return buffer[(i + 1)..];
            }
        }
        return buffer;
    }

    /// <summary>Accept the first matching command (or reference). When <paramref name="run"/>, submit command rows.</summary>
    private bool TryAcceptTopSuggestion(bool run)
    {
        if (TryTopSuggestionFromHost(run))
        {
            return true;
        }
        if (SuggestList == null || SuggestList.Items.Count == 0)
        {
            return false;
        }
        if (SuggestList.Items[0] is not SuggestRow row || string.IsNullOrEmpty(row.Insert))
        {
            return false;
        }
        if (row.Reference || !run)
        {
            AcceptSuggestion(row.Insert, row.Reference);
            return true;
        }
        CommandInput.Text = row.Insert;
        CommandInput.Focus(FocusState.Programmatic);
        SubmitLine();
        return true;
    }

    private bool TryTopSuggestionFromHost(bool run)
    {
        if (_host == IntPtr.Zero || !string.IsNullOrEmpty(_command))
        {
            return false;
        }
        var text = (CommandInput.Text ?? "").Trim();
        if (text.Length == 0 || text.StartsWith('/') || text.IndexOfAny([' ', '\t']) >= 0)
        {
            return false;
        }
        List<SuggestRow> rows;
        try
        {
            rows = JsonSerializer.Deserialize<List<SuggestRow>>(Native.Suggest(_host, text), JsonOpts) ?? new();
        }
        catch (Exception)
        {
            return false;
        }
        var row = rows.Find(r => !string.IsNullOrEmpty(r.Insert));
        if (row == null)
        {
            return false;
        }
        if (row.Reference || !run)
        {
            AcceptSuggestion(row.Insert, row.Reference);
            return true;
        }
        CommandInput.Text = row.Insert;
        CommandInput.Focus(FocusState.Programmatic);
        SubmitLine();
        return true;
    }

    private sealed class SuggestRow
    {
        public string Insert { get; set; } = "";
        public string Label { get; set; } = "";
        public bool Reference { get; set; }
        public override string ToString() => string.IsNullOrEmpty(Label) ? Insert : Label;
    }

    private void InsertCommandSpace()
    {
        var text = CommandInput.Text ?? "";
        var start = CommandInput.SelectionStart;
        if (start < 0 || start > text.Length)
        {
            start = text.Length;
        }
        CommandInput.Text = text.Insert(start, " ");
        CommandInput.SelectionStart = start + 1;
        if (_host != IntPtr.Zero)
        {
            Native.Draft(_host, CommandInput.Text);
        }
    }

    private void Remember(string text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return;
        }
        if (_typed.Count == 0 || _typed[^1] != text)
        {
            _typed.Add(text);
            if (_typed.Count > 200)
            {
                _typed.RemoveAt(0);
            }
        }
        _recall = _typed.Count;
        _liveDraft = "";
    }

    private void Recall(int direction)
    {
        if (_typed.Count == 0)
        {
            return;
        }
        if (_recall < 0 || _recall > _typed.Count)
        {
            _recall = _typed.Count;
        }
        if (_recall == _typed.Count)
        {
            _liveDraft = CommandInput.Text;
        }
        var next = Math.Clamp(_recall + direction, 0, _typed.Count);
        _recall = next;
        CommandInput.Text = next == _typed.Count ? _liveDraft : _typed[next];
        CommandInput.SelectionStart = CommandInput.Text.Length;
    }

    private void SubmitLine()
    {
        var text = CommandInput.Text;
        _armSpace = false;
        if (SuggestList != null)
        {
            SuggestList.Items.Clear();
            SuggestList.Visibility = Visibility.Collapsed;
        }
        if (!string.IsNullOrWhiteSpace(text))
        {
            Remember(text);
        }
        else
        {
            _recall = _typed.Count;
        }
        if (Native.GripPoint(_host, text))
        {
            CommandInput.Text = "";
            RefreshChrome();
            RefreshPanels();
            return;
        }
        if (string.IsNullOrWhiteSpace(text) && Native.GripCycle(_host))
        {
            RefreshChrome();
            return;
        }
        CommandInput.Text = "";
        if (IsBarePlotCommand(text, out var plotId))
        {
            _ = BeginPlot(plotId);
            return;
        }
        if (IsBareDialogCommand(text, out var dialogId))
        {
            StartCommand(dialogId);
            CommandInput.Focus(FocusState.Programmatic);
            return;
        }
        if (Native.CommandLine(_host, text) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        RefreshChrome();
        RefreshPanels();
        BuildMenus();
        CommandInput.Focus(FocusState.Programmatic);
    }

    private static bool IsBareToken(string text, out string id)
    {
        id = "";
        var trimmed = text.Trim();
        if (trimmed.Length == 0)
        {
            return false;
        }
        var space = trimmed.IndexOfAny([' ', '\t']);
        var token = space < 0 ? trimmed : trimmed[..space];
        if (space >= 0 && trimmed[(space + 1)..].Trim().Length > 0)
        {
            return false;
        }
        id = token.ToLowerInvariant();
        return true;
    }

    private static bool IsBarePlotCommand(string text, out string id) =>
        IsBareToken(text, out id) && id is "plot" or "print" or "exportpdf";

    /// <summary>Bare typed names that open a WinUI dialog (same as the menu), not the JSON command defaults.</summary>
    private static bool IsBareDialogCommand(string text, out string id)
    {
        if (!IsBareToken(text, out id))
        {
            return false;
        }
        return id is "units" or "un"
            or "layer" or "la" or "layers"
            or "style" or "st"
            or "pagesetup"
            or "stylesmanager"
            or "plot.batch"
            or "dimstyle"
            or "linetype" or "lt" or "ltype"
            or "hatchedit" or "he" or "hatch.patterns"
            or "ddptype"
            or "mleaderstyle" or "mls"
            or "room.settings"
            or "dsettings" or "ds" or "se"
            or "qselect" or "qs"
            or "cal" or "quickcalc" or "qc" or "calc";
    }

    /// <summary>Pending plot command while the user picks the window (`plot` / `exportpdf`).</summary>
    private string _pendingPlotCommand = "plot";
    /// <summary>When true, finish with an on-screen PDF preview instead of a save dialog.</summary>
    private bool _pendingPlotPreview;
    /// <summary>Nameplate layout from <c>plotl</c> / <c>plotp</c> (or OrientBox).</summary>
    private string _pendingPlotLayout = "Nameplate L";

    private string PlotCommandForOrientation()
    {
        var portrait = string.Equals(OrientBox.SelectedItem as string, "Portrait", StringComparison.OrdinalIgnoreCase);
        return portrait ? "plotp" : "plotl";
    }

    private string NameplateForOrientation()
    {
        var portrait = string.Equals(OrientBox.SelectedItem as string, "Portrait", StringComparison.OrdinalIgnoreCase);
        return portrait ? "Nameplate P" : "Nameplate L";
    }

    private async Task BeginPlot(string id)
    {
        if (_host == IntPtr.Zero || _plotBusy)
        {
            return;
        }
        _pendingPlotCommand = id == "exportpdf" ? "exportpdf" : "plot";
        _pendingPlotPreview = false;
        _pendingPlotLayout = NameplateForOrientation();
        // plotl / plotp: pick window, then finish on the matching nameplate.
        if (Native.CommandLine(_host, PlotCommandForOrientation()) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        AfterCommand();
        await Task.CompletedTask;
    }

    private void OnPlotPreview(object sender, RoutedEventArgs e)
    {
        if (_host == IntPtr.Zero || _plotBusy)
        {
            return;
        }
        _pendingPlotCommand = "plot";
        _pendingPlotPreview = true;
        _pendingPlotLayout = NameplateForOrientation();
        if (Native.CommandLine(_host, PlotCommandForOrientation()) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        AfterCommand();
    }

    private async Task<bool> PlotFileAsync(string command, string parameters)
    {
        if (!Native.PlotStart(_host, command, parameters))
        {
            return false;
        }
        int state;
        while ((state = Native.PlotPoll(_host)) == 1)
        {
            await Task.Delay(30);
        }
        return state == 2;
    }

    private async Task FinishPlot(string command, double[]? window)
    {
        if (_host == IntPtr.Zero || _plotBusy)
        {
            return;
        }
        _plotBusy = true;
        try
        {
            var hwnd = WindowNative.GetWindowHandle(this);
            var save = new FileSavePicker();
            InitializeWithWindow.Initialize(save, hwnd);
            save.FileTypeChoices.Add("PDF", new List<string> { ".pdf" });
            var stem = _documents.FirstOrDefault(doc => doc.Active)?.Title ?? "";
            if (stem.Length == 0)
            {
                stem = "Drawing";
            }
            else
            {
                stem = Path.GetFileNameWithoutExtension(stem);
                if (stem.Length == 0)
                {
                    stem = "Drawing";
                }
            }
            save.SuggestedFileName = stem + ".pdf";
            var chosen = await save.PickSaveFileAsync();
            if (chosen == null)
            {
                History.Text = "*Cancel*";
                AfterCommand();
                return;
            }
            var body = new Dictionary<string, object?> { ["path"] = chosen.Path };
            if (window is { Length: 4 })
            {
                body["window"] = window;
            }
            History.Text = "Plotting...";
            if (!await PlotFileAsync(command, JsonSerializer.Serialize(body)))
            {
                History.Text = Native.LastError(_host);
                AfterCommand();
                return;
            }
            History.Text = window == null
                ? "Plotted to " + chosen.Path
                : "Plotted the window to " + chosen.Path;
            AfterCommand();
        }
        finally
        {
            _plotBusy = false;
        }
    }

    private void DrainPlotRequest()
    {
        if (_host == IntPtr.Zero || _plotBusy)
        {
            return;
        }
        var json = Native.ReadPlotRequest(_host);
        if (json.Length == 0)
        {
            return;
        }
        double[]? window = null;
        var layout = _pendingPlotLayout;
        var preview = _pendingPlotPreview;
        try
        {
            using var doc = JsonDocument.Parse(json);
            if (doc.RootElement.TryGetProperty("window", out var win) && win.ValueKind == JsonValueKind.Array && win.GetArrayLength() == 4)
            {
                window = new double[4];
                for (var i = 0; i < 4; i++)
                {
                    window[i] = win[i].GetDouble();
                }
            }
            if (doc.RootElement.TryGetProperty("layout", out var lay) && lay.GetString() is string name && name.Length > 0)
            {
                layout = name;
            }
            if (doc.RootElement.TryGetProperty("preview", out var prev) && prev.ValueKind == JsonValueKind.True)
            {
                preview = true;
            }
        }
        catch (JsonException)
        {
            History.Text = "The plot window could not be read.";
            return;
        }
        _pendingPlotPreview = false;
        _ = FinishPlotOnNameplate(_pendingPlotCommand, window, layout, preview);
    }

    private async Task FinishPlotOnNameplate(string command, double[]? window, string layout, bool preview)
    {
        if (_host == IntPtr.Zero || _plotBusy)
        {
            return;
        }
        var landscape = !layout.Equals("Nameplate P", StringComparison.OrdinalIgnoreCase);
        if (Native.Execute(_host, "sheet.designer", JsonSerializer.Serialize(new Dictionary<string, object> { ["landscape"] = landscape })) != 0)
        {
            History.Text = Native.LastError(_host);
            AfterCommand();
            return;
        }
        var fit = new Dictionary<string, object?> { ["layout"] = layout };
        if (window is { Length: 4 })
        {
            fit["window"] = window;
        }
        if (Native.Execute(_host, "sheet.plotwindow", JsonSerializer.Serialize(fit)) != 0)
        {
            History.Text = Native.LastError(_host);
            AfterCommand();
            return;
        }
        if (preview)
        {
            await PreviewPlotOnNameplate(command);
        }
        else
        {
            await FinishPlot(command, window: null);
        }
        if (_host != IntPtr.Zero)
        {
            Native.Execute(_host, "layout.set", JsonSerializer.Serialize(new Dictionary<string, object> { ["name"] = "Model" }));
            AfterCommand();
        }
    }

    private async Task PreviewPlotOnNameplate(string command)
    {
        if (_host == IntPtr.Zero || _plotBusy)
        {
            return;
        }
        _plotBusy = true;
        try
        {
            var path = Path.Combine(Path.GetTempPath(), "odraft-plot-preview-" + Guid.NewGuid().ToString("N") + ".pdf");
            History.Text = "Building plot preview...";
            if (!await PlotFileAsync(command, JsonSerializer.Serialize(new Dictionary<string, object> { ["path"] = path })))
            {
                History.Text = Native.LastError(_host);
                return;
            }
            try
            {
                var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
                await ShowPdfAsync(null, file);
                if (_pdfWindow != null)
                {
                    _pdfWindow.Title = "Plot preview";
                }
                History.Text = "Plot preview ready.";
            }
            catch (Exception ex)
            {
                History.Text = ex.Message;
            }
        }
        finally
        {
            _plotBusy = false;
        }
    }

    /// Drawing undo and redo. These win over a text field's own undo, including the command line.
    private bool ApplyUndoRedo(KeyRoutedEventArgs e)
    {
        if (!KeyDown(Windows.System.VirtualKey.Control))
        {
            return false;
        }
        var shift = KeyDown(Windows.System.VirtualKey.Shift);
        var id = (shift, e.Key) switch
        {
            (false, Windows.System.VirtualKey.Z) => "u",
            (true, Windows.System.VirtualKey.Z) => "redo",
            (false, Windows.System.VirtualKey.Y) => "redo",
            (false, Windows.System.VirtualKey.U) => "redo",
            _ => "",
        };
        if (id.Length == 0)
        {
            return false;
        }
        e.Handled = true;
        RunShortcut(id);
        return true;
    }

    private static bool KeyDown(Windows.System.VirtualKey key) =>
        (InputKeyboardSource.GetKeyStateForCurrentThread(key) & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;

    private bool Typing()
    {
        if (Content is not FrameworkElement root || root.XamlRoot == null)
        {
            return false;
        }
        return FocusManager.GetFocusedElement(root.XamlRoot) is TextBox or ComboBox;
    }

    private void RunShortcut(string id)
    {
        if (id is "copyclip" or "cutclip" or "copybase")
        {
            if (Native.Execute(_host, id, "{}") != 0)
            {
                History.Text = Native.LastError(_host);
            }
            else
            {
                PublishClipboard();
            }
            AfterCommand();
            return;
        }
        if (id is "u" or "redo" or "selectall" or "erase" or "new" or "osnap" or "ortho" or "polar" or "otrack" or "dynmode")
        {
            if (Native.Execute(_host, id, "{}") != 0)
            {
                History.Text = Native.LastError(_host);
            }
            _docKey = "";
            AfterCommand();
            return;
        }
        StartCommand(id);
    }

    private void PublishClipboard()
    {
        var text = Native.ReadClipboard(_host);
        if (text.Length == 0)
        {
            return;
        }
        _ownClipboard = text;
        var package = new DataPackage();
        package.SetText(text);
        Clipboard.SetContent(package);
    }

    private async Task Paste(string id)
    {
        try
        {
            var view = Clipboard.GetContent();
            if (view.Contains(StandardDataFormats.Text))
            {
                var text = await view.GetTextAsync();
                if (text.Length > 0 && text != _ownClipboard)
                {
                    Native.LoadClipboard(_host, text);
                }
            }
        }
        catch (Exception ex)
        {
            History.Text = ex.Message;
        }
        if (Native.Execute(_host, id, "{}") != 0)
        {
            History.Text = Native.LastError(_host);
        }
        AfterCommand();
    }

    private void OnMode(object sender, RoutedEventArgs e)
    {
        if (_host == IntPtr.Zero || sender is not ToggleButton button || button.Tag is not string id || button.IsChecked is not bool on)
        {
            return;
        }
        var json = JsonSerializer.Serialize(new Dictionary<string, object> { ["on"] = on });
        if (Native.Execute(_host, id, json) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        RefreshChrome();
    }

    private void OnNewDrawing(object sender, RoutedEventArgs e) => _ = NewDrawingReplacingAsync();

    /// <summary>A new drawing replaces the open one (after offering to save it): O-Draft holds one drawing.</summary>
    private async Task NewDrawingReplacingAsync()
    {
        if (_host == IntPtr.Zero || !await ConfirmReplaceDrawingAsync())
        {
            return;
        }
        HideWelcome();
        Native.Execute(_host, "new", "{}");
        await KeepOnlyActiveDrawingAsync();
        EnsureModelSpace();
        _docKey = "";
        AfterCommand();
    }

    private void OnNameplateMenu(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement anchor)
        {
            return;
        }
        var flyout = new MenuFlyout();
        var landscape = new MenuFlyoutItem { Text = "Nameplate L (landscape)" };
        landscape.Click += (_, _) => OpenNameplate(landscape: true);
        var portrait = new MenuFlyoutItem { Text = "Nameplate P (portrait)" };
        portrait.Click += (_, _) => OpenNameplate(landscape: false);
        flyout.Items.Add(landscape);
        flyout.Items.Add(portrait);
        flyout.ShowAt(anchor);
    }

    private void OnNameplateDesigner(object sender, RoutedEventArgs e) => OpenNameplate(landscape: true);

    private void OpenNameplate(bool landscape)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (Native.Execute(_host, "sheet.designer", JsonSerializer.Serialize(new Dictionary<string, object>
            {
                ["open"] = true,
                ["landscape"] = landscape,
            })) != 0)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        _filling = true;
        try
        {
            OrientBox.SelectedItem = landscape ? "Landscape" : "Portrait";
        }
        finally
        {
            _filling = false;
        }
        _docKey = "";
        AfterCommand();
    }

    /// <summary>The drawing tab whose page the hidden paper, orientation and scale boxes show.</summary>
    private string _pageTab = "Design";

    /// <summary>The tab that is active now.</summary>
    private string _activeTabName = "Design";

    /// <summary>
    /// O-Draft holds one drawing. Before a file is opened or a new drawing started, offer to save what is open; false when the
    /// person cancels. <see cref="KeepOnlyActiveDrawingAsync"/> then closes the old drawing once the new one is there.
    /// </summary>
    private async Task<bool> ConfirmReplaceDrawingAsync()
    {
        _docKey = "";
        RefreshChrome();
        foreach (var doc in _documents.Where(d => d.Dirty).ToList())
        {
            if (!await DiscardOrSave(doc.Title))
            {
                return false;
            }
        }
        return true;
    }

    private async Task KeepOnlyActiveDrawingAsync()
    {
        _docKey = "";
        RefreshChrome();
        var active = _documents.FirstOrDefault(d => d.Active);
        if (active == null)
        {
            return;
        }
        foreach (var other in _documents.Where(d => d.Index != active.Index).OrderByDescending(d => d.Index).ToList())
        {
            Native.Execute(_host, "close", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = other.Index }));
        }
        _docKey = "";
        RefreshChrome();
        await Task.CompletedTask;
    }

    private void EnsureModelSpace()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        Native.Execute(_host, "layout.set", JsonSerializer.Serialize(new Dictionary<string, object> { ["name"] = "Model" }));
    }

    private async Task<bool> PickFile(string id)
    {
        if (_host == IntPtr.Zero)
        {
            return false;
        }
        try
        {
            return await PickFileCore(id);
        }
        catch (Exception ex)
        {
            History.Text = ex.Message;
            return false;
        }
    }

    private async Task<bool> OpenFileAsync(string path)
    {
        var name = System.IO.Path.GetFileName(path);
        ShowBusy("Opening?", string.IsNullOrEmpty(name) ? path : name);
        try
        {
            if (!Native.OpenStart(_host, path))
            {
                return false;
            }
            int state;
            while ((state = Native.OpenPoll(_host)) == 1)
            {
                await Task.Delay(30);
            }
            if (state == 2)
            {
                RememberDrawing(path);
            }
            return state == 2;
        }
        finally
        {
            HideBusy();
        }
    }

    private async Task<bool> SaveFileAsync(string? path)
    {
        var detail = string.IsNullOrEmpty(path)
            ? "Current drawing"
            : (System.IO.Path.GetFileName(path) ?? path);
        ShowBusy("Saving?", detail);
        try
        {
            if (!Native.SaveStart(_host, path))
            {
                return false;
            }
            int state;
            while ((state = Native.SavePoll(_host)) == 1)
            {
                await Task.Delay(30);
            }
            if (state == 2 && !string.IsNullOrEmpty(path))
            {
                RememberDrawing(path);
            }
            return state == 2;
        }
        finally
        {
            HideBusy();
        }
    }

    // The saved file behind each open drawing, by the name its tab shows, so the active one can be published to O-CoreOp.
    private readonly Dictionary<string, string> _docPaths = new(StringComparer.OrdinalIgnoreCase);

    private void RememberDrawing(string path)
    {
        _docPaths[System.IO.Path.GetFileNameWithoutExtension(path)] = path;
        _docPaths[System.IO.Path.GetFileName(path)] = path;
    }

    /// <summary>Publish the active drawing so O-CoreOp can find the same project file (the pointer; a project file needs no server).</summary>
    private void PublishActiveProject(string? drawing) => ActiveProjectPointer.Set(drawing);

    private string? ActiveDrawingPath(DocTab? active) =>
        active is null ? null
        : !string.IsNullOrEmpty(active.Path) && System.IO.File.Exists(active.Path) ? active.Path
        : _docPaths.TryGetValue(active.Title, out var path) && System.IO.File.Exists(path) ? path : null;

    private async Task<bool> PickFileCore(string id)
    {
        var hwnd = WindowNative.GetWindowHandle(this);
        if (id == "open")
        {
            var picker = new FileOpenPicker();
            InitializeWithWindow.Initialize(picker, hwnd);
            picker.FileTypeFilter.Add(".osp");
            picker.FileTypeFilter.Add(".dxf");
            picker.FileTypeFilter.Add(".dwg");
            var file = await picker.PickSingleFileAsync();
            if (file == null)
            {
                return false;
            }
            if (!await ConfirmReplaceDrawingAsync())
            {
                return false;
            }
            // The file is read on a worker thread so the window keeps responding; the host fits the view.
            History.Text = "Opening " + file.Name + "...";
            var opened = await OpenFileAsync(file.Path);
            if (!opened)
            {
                History.Text = Native.LastError(_host);
                AfterCommand();
                return false;
            }
            ShellPrefs.RememberRecent(file.Path);
            await KeepOnlyActiveDrawingAsync();
            HideWelcome();
            _docKey = "";
            AfterCommand();
            return true;
        }
        if (id == "qsave")
        {
            History.Text = "Saving...";
            var saved = await SaveFileAsync(null);
            if (saved)
            {
                _docKey = "";
                AfterCommand();
                return true;
            }
            var err = Native.LastError(_host);
            if (!err.Contains("no file name", StringComparison.OrdinalIgnoreCase))
            {
                History.Text = err;
                AfterCommand();
                return false;
            }
        }
        var save = new FileSavePicker();
        InitializeWithWindow.Initialize(save, hwnd);
        save.FileTypeChoices.Add("Project (drawing and take-off)", new List<string> { ".osp" });
        save.FileTypeChoices.Add("DXF", new List<string> { ".dxf" });
        save.FileTypeChoices.Add("DWG", new List<string> { ".dwg" });
        save.SuggestedFileName = "drawing";
        var chosen = await save.PickSaveFileAsync();
        if (chosen == null)
        {
            return false;
        }
        History.Text = "Saving " + chosen.Name + "...";
        if (!await SaveFileAsync(chosen.Path))
        {
            History.Text = Native.LastError(_host);
            AfterCommand();
            return false;
        }
        ShellPrefs.RememberRecent(chosen.Path);
        HideWelcome();
        _docKey = "";
        AfterCommand();
        return true;
    }

    /// <summary>File > Import: DXF and DWG are merged into the open drawing, PDF and SVG arrive as geometry, pictures are traced to outlines (docs/import.md).</summary>
    private void OnAskAttach(object sender, RoutedEventArgs e) => _ = ImportFile();

    private async Task ImportFile()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var picker = new FileOpenPicker();
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        foreach (var ext in new[] { ".dxf", ".dwg", ".pdf", ".svg", ".png", ".jpg", ".jpeg", ".tif", ".tiff" })
        {
            picker.FileTypeFilter.Add(ext);
        }
        var file = await picker.PickSingleFileAsync();
        if (file == null)
        {
            return;
        }
        History.Text = "Importing " + file.Name + "...";
        var args = JsonSerializer.Serialize(new Dictionary<string, object> { ["path"] = file.Path });
        if (Native.Execute(_host, "import", args) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        else
        {
            HideWelcome();
        }
        _docKey = "";
        AfterCommand();
    }

    private void OnAppClosing(Microsoft.UI.Windowing.AppWindow sender, Microsoft.UI.Windowing.AppWindowClosingEventArgs args)
    {
        if (_allowClose)
        {
            return;
        }
        args.Cancel = true;
        _ = CloseAll(quit: true);
    }

    private async Task CloseActive()
    {
        if (_asking || _host == IntPtr.Zero)
        {
            return;
        }
        _asking = true;
        try
        {
            _docKey = "";
            RefreshChrome();
            var active = _documents.FirstOrDefault(doc => doc.Active) ?? _documents.FirstOrDefault();
            if (active != null && active.Dirty && !await DiscardOrSave(active.Title))
            {
                return;
            }
            var index = active?.Index ?? 0;
            if (Native.Execute(_host, "close", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = index })) != 0)
            {
                History.Text = Native.LastError(_host);
                return;
            }
            _docKey = "";
            RefreshChrome();
            if (_documents.Count == 0)
            {
                Native.Execute(_host, "new", "{}");
            }
            _panelKey = "";
            AfterCommand();
        }
        finally
        {
            _asking = false;
        }
    }

    private async Task CloseAll(bool quit)
    {
        if (_asking || _host == IntPtr.Zero)
        {
            return;
        }
        _asking = true;
        try
        {
            while (true)
            {
                _docKey = "";
                RefreshChrome();
                var dirty = _documents.FirstOrDefault(doc => doc.Dirty);
                if (dirty == null)
                {
                    break;
                }
                if (!dirty.Active)
                {
                    Native.Execute(_host, "document.switch", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = dirty.Index }));
                    _docKey = "";
                    RefreshChrome();
                }
                if (!await DiscardOrSave(dirty.Title))
                {
                    return;
                }
                if (Native.Execute(_host, "close", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = dirty.Index })) != 0)
                {
                    History.Text = Native.LastError(_host);
                    return;
                }
            }
            if (quit)
            {
                // Close owned tool / chrome windows before the shell HWND disappears.
                _chrome?.Close();
                _chrome = null;
                ShellWindows.CloseAllOwned();
                _allowClose = true;
                Close();
                return;
            }
        }
        finally
        {
            _asking = false;
        }
    }

    private async Task<bool> DiscardOrSave(string title)
    {
        var answer = await SavePrompt.Ask(this, title.Length == 0 ? "drawing" : title);
        if (answer == 0)
        {
            return false;
        }
        if (answer == 2)
        {
            return true;
        }
        return await PickFile("qsave");
    }

    private static string PathJson(string path) =>
        JsonSerializer.Serialize(new Dictionary<string, string> { ["path"] = path });

    private void OnWorkspace(object sender, RoutedEventArgs e) => SwitchWorkspace(!_classic);

    private string WorkspaceKey() => _classic ? "classic" : "pro";

    private static ShellPrefs.ModeSlot DefaultSlot(bool classic) => new()
    {
        PinLeft = false,
        ToolsVisible = true,
        LeftWidth = 280,
        LeftCollapsed = true,
    };

    private ShellPrefs.ModeSlot CaptureSlot() => new()
    {
        PinLeft = _pinLeft,
        ToolsVisible = _showTools,
        LeftWidth = _leftWidth,
        LeftCollapsed = _leftCollapsed,
    };

    private void ApplySlot(ShellPrefs.ModeSlot slot)
    {
        _pinLeft = true;                             // pinned at every start so the panel stays open; unpin from the panel header if you want it to auto-hide
        _showTools = slot.ToolsVisible;
        _leftWidth = Math.Clamp(slot.LeftWidth > 0 ? slot.LeftWidth : Density.SidePanelWidth, 180, 640);
        _leftCollapsed = false;                      // the side panel is open at every start; the rail title still closes it
        _leftIdleSince = DateTime.UtcNow;
        UpdatePinButton();
    }

    private void SwitchWorkspace(bool classic)
    {
        if (_classic == classic)
        {
            return;
        }
        _modeSlots[WorkspaceKey()] = CaptureSlot();
        _classic = classic;
        if (!_modeSlots.TryGetValue(WorkspaceKey(), out var slot))
        {
            slot = DefaultSlot(classic);
        }
        ApplySlot(slot);
        ToolTipService.SetToolTip(WorkspaceButton, _classic ? "Classic" : "Pro");
        AutomationProperties.SetName(WorkspaceButton, _classic ? "Classic workspace" : "Pro workspace");
        ApplyChrome();
        PlaceChild();
        SaveShell();
    }

    /// <summary>Resets the current workspace only; the other mode's slot is kept.</summary>
    private void ResetCurrentWorkspace()
    {
        ApplySlot(DefaultSlot(_classic));
        _showMenu = true;
        _showTabs = true;
        _showStatus = true;
        _showCommand = true;
        _modeSlots[WorkspaceKey()] = CaptureSlot();
        ApplyChrome();
        PlaceChild();
        SaveShell();
    }

    private UIElement? _dragSplit;
    private double _dragX;
    private double _dragWidth;
    private string _promptShown = "";
    private string _dynAccess = "";

    private void OnSplitPressed(object sender, PointerRoutedEventArgs e)
    {
        if (sender is not UIElement splitter)
        {
            return;
        }
        _dragSplit = splitter;
        _dragX = e.GetCurrentPoint(Content).Position.X;
        _dragWidth = _leftWidth;
        _leftCollapsed = false;
        _leftIdleSince = DateTime.UtcNow;
        splitter.CapturePointer(e.Pointer);
        e.Handled = true;
    }

    private void OnSplitMoved(object sender, PointerRoutedEventArgs e)
    {
        if (_dragSplit == null || !ReferenceEquals(sender, _dragSplit))
        {
            return;
        }
        var delta = e.GetCurrentPoint(Content).Position.X - _dragX;
        // Panel sits right of the grip ? drag left to widen.
        SetSideWidth(_dragWidth - delta);
        PlaceChild();
        e.Handled = true;
    }

    private void OnSplitReleased(object sender, PointerRoutedEventArgs e)
    {
        if (sender is UIElement splitter)
        {
            splitter.ReleasePointerCapture(e.Pointer);
        }
        EndSplitDrag();
        e.Handled = true;
    }

    private void OnSplitCaptureLost(object sender, PointerRoutedEventArgs e) => EndSplitDrag();

    private void EndSplitDrag()
    {
        if (_dragSplit == null)
        {
            return;
        }
        _dragSplit = null;
        _leftIdleSince = DateTime.UtcNow;
        SaveShell();
    }

    private void OnSplitKey(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key is not (Windows.System.VirtualKey.Left or Windows.System.VirtualKey.Right))
        {
            return;
        }
        var step = KeyDown(Windows.System.VirtualKey.Shift) ? 40 : 16;
        // Left arrow widens the right-docked panel.
        if (e.Key == Windows.System.VirtualKey.Right)
        {
            step = -step;
        }
        SetSideWidth(_leftWidth + step);
        PlaceChild();
        SaveShell();
        e.Handled = true;
    }

    private void SetSideWidth(double width)
    {
        _leftWidth = Math.Clamp(width, 180, 640);
        PropertiesColumn.Width = new GridLength(_leftWidth + Density.SideRailWidth);
        AutomationProperties.SetName(LeftSplitter, "Resize side panel, width " + (int)_leftWidth);
    }

    private void AnnouncePrompt(string prompt, string dynAccess = "")
    {
        var text = prompt;
        if (dynAccess.Length > 0)
        {
            text = prompt + " " + dynAccess;
        }
        if (text == _promptShown)
        {
            return;
        }
        _promptShown = text;
        PromptLive.Text = text;
        AutomationProperties.SetName(PromptLive, text);
        AutomationProperties.SetName(PromptLabel, text);
        AutomationProperties.SetName(
            CanvasHost,
            "Drawing. Arrows move the cursor, Insert picks or places, Ctrl+arrows pan. " + text);
        AutomationProperties.SetFullDescription(
            CanvasHost,
            "Keyboard drafting: type coordinates on the command line, Tab cycles snaps, Enter or Space cycles a hot grip.");
    }

    /// <summary>Arrow nudge / pan, Insert activate, and history recall when the line has text.</summary>
    private bool ApplyDraftKeys(KeyRoutedEventArgs e, bool ctrl, bool shift, bool fromCommand)
    {
        if (e.Key == Windows.System.VirtualKey.Insert)
        {
            e.Handled = true;
            _armSpace = false;
            if (Native.KeyActivate(_host, shift) != 0)
            {
                Status.Text = Native.LastError(_host);
            }
            else
            {
                RefreshChrome();
                RefreshPanels();
            }
            return true;
        }
        if (e.Key is not (Windows.System.VirtualKey.Left or Windows.System.VirtualKey.Right
            or Windows.System.VirtualKey.Up or Windows.System.VirtualKey.Down))
        {
            return false;
        }
        var empty = string.IsNullOrEmpty(CommandInput.Text);
        if (!ctrl && !empty && e.Key is Windows.System.VirtualKey.Up or Windows.System.VirtualKey.Down)
        {
            e.Handled = true;
            _armSpace = false;
            Recall(e.Key == Windows.System.VirtualKey.Up ? -1 : 1);
            if (!fromCommand)
            {
                CommandInput.Focus(FocusState.Programmatic);
            }
            return true;
        }
        if (!ctrl && !empty && e.Key is Windows.System.VirtualKey.Left or Windows.System.VirtualKey.Right)
        {
            return false;
        }
        if (!ctrl && !empty)
        {
            return false;
        }
        e.Handled = true;
        _armSpace = false;
        var dx = e.Key == Windows.System.VirtualKey.Left ? -1 : e.Key == Windows.System.VirtualKey.Right ? 1 : 0;
        var dy = e.Key == Windows.System.VirtualKey.Up ? -1 : e.Key == Windows.System.VirtualKey.Down ? 1 : 0;
        if (Native.KeyNudge(_host, dx, dy, ctrl, shift) != 0)
        {
            Status.Text = Native.LastError(_host);
        }
        else
        {
            RefreshChrome();
        }
        return true;
    }

    private void OnThemeButton(object sender, RoutedEventArgs e)
    {
        if (_themeSwitchSync)
            return;
        _dark = !_dark;
        SuiteAppearancePrefs.Save(_dark ? ElementTheme.Dark : ElementTheme.Light);
        if (_host != IntPtr.Zero)
            Native.Theme(_host, _dark);
        ApplyShellTheme();
    }

    private void ApplyShellTheme()
    {
        var theme = _dark ? ElementTheme.Dark : ElementTheme.Light;
        Density.Theme = theme;
        _levelKey = "";
        _sheetKey = "";
        _docKey = "";
        // The rail restyles only a button whose state changed; a new theme changes every colour, so forget what was shown.
        _railShown.Clear();
        RefreshSideRail();
        ShellChrome.Apply(this, theme);
        _chrome?.ApplyTheme(theme);
        SyncAskPills();
        CanvasHost.Background = Density.CanvasBackground();
        // Drawing child clear colour is independent of WinUI ThemeResources — keep it in sync.
        if (_host != IntPtr.Zero)
        {
            Native.Theme(_host, _dark);
        }
        ApplyThemeToToolWindows(theme);
        // Panel text takes its colours from the theme at build time, so build it again.
        _panelKey = "";
        RefreshPanels();
        BuildTools();
        SyncThemeSwitch();
    }

    private void SyncThemeSwitch()
    {
        _themeSwitchSync = true;
        try
        {
            // Show the icon for the active theme (sun = light, moon = dark).
            ThemeIcon.Kind = _dark ? "moon" : "sun";
            var tip = _dark ? "Dark theme — click for light" : "Light theme — click for dark";
            ToolTipService.SetToolTip(ThemeButton, tip);
            AutomationProperties.SetName(ThemeButton, tip);
        }
        finally
        {
            _themeSwitchSync = false;
        }
    }

    private void TearDownShell()
    {
        if (_shellClosing)
        {
            return;
        }
        _shellClosing = true;
        _timer?.Stop();
        _chrome?.Close();
        _chrome = null;
        ShellWindows.CloseAllOwned();
        _layersWindow = null;
        _calc = null;
        _pomodoro = null;
        _toolsWindow = null;
        _leftFloat = null;
        _pdfWindow = null;
        _formWindow = null;
        _unitsWindow = null;
        _textStyleWindow = null;
        _pageSetupWindow = null;
        _plotStyleWindow = null;
        _batchPlotWindow = null;
        _dimStyleWindow = null;
        _linetypeWindow = null;
        _parametersWindow = null;
        _graphWindow = null;
        _revisionsWindow = null;
        _historyWindow = null;
        _smartBlockWindow = null;
        _hatchWindow = null;
        _pointStyleWindow = null;
        _mleaderStyleWindow = null;
        _roomSettingsWindow = null;
        _draftingWindow = null;
        _quickWindow = null;
        _quickSelectWindow = null;
        if (_host != IntPtr.Zero)
        {
            Native.Destroy(_host);
            _host = IntPtr.Zero;
        }
    }

    private void ApplyThemeToToolWindows(ElementTheme theme)
    {
        _layersWindow?.ApplyTheme(theme);
        foreach (var window in new Window?[]
        {
            _calc, _pomodoro, _toolsWindow, _leftFloat, _pdfWindow, _formWindow,
            _unitsWindow, _textStyleWindow, _pageSetupWindow, _plotStyleWindow, _batchPlotWindow,
            _dimStyleWindow, _linetypeWindow, _parametersWindow, _graphWindow, _revisionsWindow, _historyWindow, _smartBlockWindow, _hatchWindow, _pointStyleWindow, _mleaderStyleWindow,
            _roomSettingsWindow, _draftingWindow, _quickWindow, _quickSelectWindow,
        })
        {
            if (window != null)
            {
                ShellChrome.Apply(window, theme);
            }
        }
    }

    private static Windows.UI.Color Color(byte r, byte g, byte b) =>
        Windows.UI.Color.FromArgb(255, r, g, b);

    private async void OnPdf(object sender, RoutedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var bytes = Native.ReadPdf(_host);
        if (bytes == null)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        try
        {
            await ShowPdfAsync(bytes, null);
        }
        catch (Exception ex)
        {
            History.Text = ex.Message;
        }
    }

    private async Task ShowPdfAsync(byte[]? bytes, Windows.Storage.StorageFile? file)
    {
        PdfDocument doc;
        IRandomAccessStream? owned = null;
        if (file != null)
        {
            doc = await PdfDocument.LoadFromFileAsync(file);
        }
        else
        {
            owned = new InMemoryRandomAccessStream();
            await owned.WriteAsync((bytes ?? Array.Empty<byte>()).AsBuffer());
            owned.Seek(0);
            doc = await PdfDocument.LoadFromStreamAsync(owned);
        }
        if (doc.PageCount == 0)
        {
            History.Text = "The PDF has no pages.";
            owned?.Dispose();
            return;
        }
        var image = new Image { Stretch = Stretch.Uniform, MaxWidth = 860 };
        var caption = new TextBlock();
        var open = new Button { Content = "Open PDF…", HorizontalAlignment = HorizontalAlignment.Left, MinHeight = Density.ControlHeight };
        Density.Name(open, "Open PDF");
        Density.Name(caption, "PDF page");
        Density.Name(image, "PDF page preview");
        var pageBox = new StackPanel { Spacing = Density.SpaceMd };
        pageBox.Children.Add(open);
        pageBox.Children.Add(caption);
        pageBox.Children.Add(new ScrollViewer
        {
            Content = image,
            MaxWidth = 880,
            MaxHeight = 640,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        });
        async Task ShowPage(uint index)
        {
            using var page = doc.GetPage(index);
            var rendered = new InMemoryRandomAccessStream();
            var options = new PdfPageRenderOptions
            {
                DestinationWidth = (uint)Math.Clamp(page.Size.Width * 2.0, 1, 4096),
                DestinationHeight = (uint)Math.Clamp(page.Size.Height * 2.0, 1, 4096),
            };
            await page.RenderToStreamAsync(rendered, options);
            rendered.Seek(0);
            var bitmap = new BitmapImage();
            await bitmap.SetSourceAsync(rendered);
            image.Source = bitmap;
            caption.Text = $"Page {index + 1} of {doc.PageCount}";
        }
        await ShowPage(0);
        open.Click += async (_, _) =>
        {
            try
            {
                var owner = _pdfWindow ?? this;
                var picker = new FileOpenPicker();
                InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(owner));
                picker.FileTypeFilter.Add(".pdf");
                var picked = await picker.PickSingleFileAsync();
                if (picked == null)
                {
                    return;
                }
                doc = await PdfDocument.LoadFromFileAsync(picked);
                if (doc.PageCount == 0)
                {
                    caption.Text = "The PDF has no pages.";
                    return;
                }
                await ShowPage(0);
            }
            catch (Exception ex)
            {
                caption.Text = ex.Message;
            }
        };
        var root = new Grid { Padding = Density.DialogMargin, Children = { pageBox } };
        if (Content is FrameworkElement shell)
        {
            root.RequestedTheme = shell.RequestedTheme;
        }
        if (_pdfWindow == null)
        {
            _pdfWindow = new Window { Title = "PDF" };
            _pdfWindow.Closed += (_, _) =>
            {
                owned?.Dispose();
                _pdfWindow = null;
            };
        }
        _pdfWindow.Content = root;
        ShellIcon.Apply(_pdfWindow, ShellChrome.ThemeOf(this));
        ShellFit.UserResizable(_pdfWindow, minWidthDip: 640, minHeightDip: 480, maxWidthDip: 1200, maxHeightDip: 900, persistKey: "pdf");
        PresentToolWindow(_pdfWindow);
    }

    private void AfterCommand()
    {
        RefreshChrome();
        RefreshPanels();
        if (_sideTab == SideTab.History)
        {
            _historyPanel?.Load();
        }
        if (_sideTab == SideTab.Revisions)
        {
            _revisionsPanel?.Load();
        }
        BuildMenus();
        CommandInput.Focus(FocusState.Programmatic);
    }

    private void RefreshAiChip(ChromeState state)
    {
        if (AiChip == null || AiChipHost == null)
        {
            return;
        }
        var busy = state.Assistant is "thinking" or "loading" or "confirm";
        var linked = state.ControlPort > 0;
        // Glow only while the assistant is actively working; address stays on hover.
        SetStatusGlow(AiChipHost, active: busy, accent: true);
        var tip = linked
            ? "127.0.0.1:" + state.ControlPort + (string.IsNullOrEmpty(state.AssistantModel) ? "" : " · " + state.AssistantModel)
            : "AI assistant";
        if (busy)
        {
            tip += " · " + state.Assistant;
        }
        ToolTipService.SetToolTip(AiChip, tip);
        AutomationProperties.SetName(AiChip, tip);
    }

    private void OnAiChip(object sender, RoutedEventArgs e)
    {
        _showCommand = true;
        PlaceCommandBar();
        _chrome?.ShowBar(true);
        AiInput.Focus(FocusState.Programmatic);
    }

    private static void SetStatusGlow(Border host, bool active, bool accent)
    {
        if (!active)
        {
            host.BorderBrush = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
            host.Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
            return;
        }
        var brush = accent
            ? Density.ThemeBrush("CommandChromeAccentBrush") ?? Density.ThemeBrush("AccentBrush")
            : Density.ThemeBrush("CadChannelBrush") ?? Density.ThemeBrush("AccentBrush");
        if (brush is SolidColorBrush solid)
        {
            var c = solid.Color;
            host.BorderBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(220, c.R, c.G, c.B));
            host.Background = new SolidColorBrush(Windows.UI.Color.FromArgb(48, c.R, c.G, c.B));
        }
        else
        {
            host.BorderBrush = brush;
            host.Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        }
    }

    private void BuildMenus()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        List<MenuNode>? menus;
        try
        {
            menus = JsonSerializer.Deserialize<List<MenuNode>>(Native.ReadText(_host, Native.TextMenu), JsonOpts);
        }
        catch (JsonException)
        {
            History.Text = "The command menu could not be read.";
            return;
        }
        Bar.Items.Clear();
        _menuKeys.Clear();
        if (menus == null)
        {
            return;
        }
        // Locked to a project by the launcher: no commands that make or open a project or drawing file.
        foreach (var menu in menus)
        {
            PruneLocked(menu);
        }
        foreach (var menu in menus)
        {
            var top = new MenuBarItem { Title = menu.Title, FontSize = Density.FontBody };
            foreach (var child in menu.Items)
            {
                top.Items.Add(MakeItem(child, 0));
            }
            Bar.Items.Add(top);
        }
        // Shell-only aliases that match common Windows habits and stay on the same command ids.
        RememberKey(true, false, Windows.System.VirtualKey.Y, "redo");
        RememberKey(true, false, Windows.System.VirtualKey.U, "redo");
        RememberKey(true, false, Windows.System.VirtualKey.Number1, "ui.panel.properties");
        RememberKey(true, false, Windows.System.VirtualKey.NumberPad1, "ui.panel.properties");
        EnsureHelp();
    }

    private static void PruneLocked(MenuNode node)
    {
        node.Items.RemoveAll(child => ProjectLock.IsHidden(child.Id));
        foreach (var child in node.Items)
        {
            PruneLocked(child);
        }
    }

    private void EnsureHelp()
    {
        MenuBarItem? help = null;
        foreach (var item in Bar.Items)
        {
            if (item is MenuBarItem top && top.Title?.ToString() == "Help")
            {
                help = top;
            }
        }
        if (help == null)
        {
            help = new MenuBarItem { Title = "Help", FontSize = Density.FontBody };
            Bar.Items.Add(help);
        }
        ClearShellItems(help);
        var commands = new MenuFlyoutItem { Text = "Command list", Tag = "shell.commands", FontSize = Density.FontBody };
        void ShowCommands() => CommandList.Show(this, Native.ReadText(_host, Native.TextMenu));
        commands.Click += (_, _) => ShowCommands();
        BindAccel(commands, "F1", ShowCommands, mapId: "");
        var updates = new MenuFlyoutItem { Text = "Check for updates?", Tag = "shell.updates", FontSize = Density.FontBody };
        updates.Click += async (_, _) => await ShowUpdateCheckAsync();
        var about = new MenuFlyoutItem { Text = "About O-Draft", Tag = "shell.about", FontSize = Density.FontBody };
        about.Click += (_, _) => AboutWindow.Show(this, VersionText());
        help.Items.Add(commands);
        help.Items.Add(updates);
        help.Items.Add(about);
        EnsureShellMenus();
    }

    private string VersionText()
    {
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadText(_host, Native.TextChrome));
            if (doc.RootElement.TryGetProperty("version", out var version) && version.GetString() is string text)
            {
                return text;
            }
        }
        catch (JsonException)
        {
        }
        return "";
    }

    private async Task ShowUpdateCheckAsync()
    {
        var fromHost = VersionText();
        var fromAsm = typeof(App).Assembly.GetName().Version?.ToString(3) ?? "0.0.0";
        var current = fromHost.Length > 0 ? fromHost : fromAsm;
        var result = await UpdateCheck.CheckAsync(current);
        var body = new StackPanel { Spacing = Density.SpaceSm, MaxWidth = 400 };
        body.Children.Add(new TextBlock { Text = result.Message, TextWrapping = TextWrapping.WrapWholeWords, FontSize = Density.FontBody });
        body.Children.Add(Density.Secondary(
            "Updates are not installed automatically. See the suite packaging notes for signing and the shared channel JSON.",
            TextWrapping.WrapWholeWords));
        var dlg = new ContentDialog
        {
            Title = result.UpdateAvailable ? "Update available" : "Check for updates",
            Content = body,
            CloseButtonText = "Close",
            XamlRoot = Content.XamlRoot,
            DefaultButton = ContentDialogButton.Close,
        };
        await dlg.ShowAsync();
    }

    private MenuFlyoutItemBase MakeItem(MenuNode node, int depth)
    {
        if (depth > 8 || string.IsNullOrEmpty(node.Id))
        {
            var sub = new MenuFlyoutSubItem { Text = node.Title, FontSize = Density.FontBody };
            if (depth <= 8)
            {
                foreach (var child in node.Items)
                {
                    sub.Items.Add(MakeItem(child, depth + 1));
                }
            }
            return sub;
        }
        var item = new MenuFlyoutItem
        {
            Text = node.Title,
            Tag = node.Id,
            IsEnabled = node.Enabled,
            FontSize = Density.FontBody,
        };
        if (!string.IsNullOrEmpty(node.Shortcut) && node.Id != null)
        {
            BindAccel(item, node.Shortcut, () => InvokeAccelId(node.Id!));
        }
        item.Click += OnMenuCommand;
        return item;
    }

    private void BindAccel(MenuFlyoutItem item, string shortcut, Action run, string? mapId = null)
    {
        if (!TryParseShortcut(shortcut, out var key, out var mods))
        {
            item.KeyboardAcceleratorTextOverride = shortcut;
            return;
        }
        item.KeyboardAccelerators.Clear();
        var accel = new KeyboardAccelerator { Key = key, Modifiers = mods };
        accel.Invoked += (_, args) =>
        {
            args.Handled = true;
            run();
        };
        item.KeyboardAccelerators.Add(accel);
        RememberKey(
            mods.HasFlag(Windows.System.VirtualKeyModifiers.Control),
            mods.HasFlag(Windows.System.VirtualKeyModifiers.Shift),
            key,
            mapId ?? item.Tag as string ?? "");
    }

    private void RememberKey(bool ctrl, bool shift, Windows.System.VirtualKey key, string id)
    {
        if (id.Length == 0 || key == Windows.System.VirtualKey.None)
        {
            return;
        }
        _menuKeys[(ctrl, shift, key)] = id;
    }

    private static bool TryParseShortcut(string text, out Windows.System.VirtualKey key, out Windows.System.VirtualKeyModifiers mods)
    {
        key = Windows.System.VirtualKey.None;
        mods = Windows.System.VirtualKeyModifiers.None;
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }
        var parts = text.Split('+', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length == 0)
        {
            return false;
        }
        for (var i = 0; i < parts.Length - 1; i++)
        {
            switch (parts[i].ToUpperInvariant())
            {
                case "CTRL":
                case "CONTROL":
                case "CMD":
                case "COMMAND":
                    mods |= Windows.System.VirtualKeyModifiers.Control;
                    break;
                case "SHIFT":
                    mods |= Windows.System.VirtualKeyModifiers.Shift;
                    break;
                case "ALT":
                case "MENU":
                    mods |= Windows.System.VirtualKeyModifiers.Menu;
                    break;
                default:
                    return false;
            }
        }
        var last = parts[^1].ToUpperInvariant();
        key = last switch
        {
            "DELETE" or "DEL" => Windows.System.VirtualKey.Delete,
            "ESCAPE" or "ESC" => Windows.System.VirtualKey.Escape,
            "ENTER" or "RETURN" => Windows.System.VirtualKey.Enter,
            "SPACE" => Windows.System.VirtualKey.Space,
            "TAB" => Windows.System.VirtualKey.Tab,
            "PAGEUP" => Windows.System.VirtualKey.PageUp,
            "PAGEDOWN" => Windows.System.VirtualKey.PageDown,
            "F1" => Windows.System.VirtualKey.F1,
            "F2" => Windows.System.VirtualKey.F2,
            "F3" => Windows.System.VirtualKey.F3,
            "F4" => Windows.System.VirtualKey.F4,
            "F5" => Windows.System.VirtualKey.F5,
            "F6" => Windows.System.VirtualKey.F6,
            "F7" => Windows.System.VirtualKey.F7,
            "F8" => Windows.System.VirtualKey.F8,
            "F9" => Windows.System.VirtualKey.F9,
            "F10" => Windows.System.VirtualKey.F10,
            "F11" => Windows.System.VirtualKey.F11,
            "F12" => Windows.System.VirtualKey.F12,
            { Length: 1 } s when s[0] is >= 'A' and <= 'Z' => (Windows.System.VirtualKey)s[0],
            { Length: 1 } s when s[0] is >= '0' and <= '9' => Windows.System.VirtualKey.Number0 + (s[0] - '0'),
            _ => Windows.System.VirtualKey.None,
        };
        return key != Windows.System.VirtualKey.None;
    }

    private static void ClearShellItems(MenuBarItem menu)
    {
        for (var i = menu.Items.Count - 1; i >= 0; i--)
        {
            if (menu.Items[i] is MenuFlyoutItem item && item.Tag is string tag && tag.StartsWith("shell.", StringComparison.Ordinal))
            {
                menu.Items.RemoveAt(i);
            }
        }
    }

    private void RefreshChrome()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        ChromeState? state = null;
        try
        {
            state = JsonSerializer.Deserialize<ChromeState>(Native.ReadText(_host, Native.TextChrome), JsonOpts);
        }
        catch (JsonException)
        {
            History.Text = Native.LastError(_host);
        }
        if (state != null)
        {
            var history = string.Join(Environment.NewLine, state.History);
            if (History.Text != history)
            {
                History.Text = history;
            }
            SyncHistory();
            // Cursor chrome is typed-text only; engine prompt is announced, Ask status stays on the AI bar.
            var aiPrompt = "Ask:";
            if (state.Assistant is "thinking" or "loading")
            {
                aiPrompt = string.IsNullOrEmpty(state.AssistantPartial) ? "Assistant…" : state.AssistantPartial;
            }
            else if (state.Assistant == "confirm" && !string.IsNullOrEmpty(state.AssistantPrompt))
            {
                aiPrompt = state.AssistantPrompt;
            }
            PromptLabel.Text = aiPrompt;
            AnnouncePrompt(state.Prompt ?? "Command:", _dynAccess);
            AssistantButtons.Visibility = state.Assistant == "confirm" ? Visibility.Visible : Visibility.Collapsed;
            if (!string.IsNullOrEmpty(state.Space))
            {
                _space = state.Space;
            }
            RefreshAiChip(state);
            if (state.ControlPort > 0)
            {
                ControlPort.Text = "127.0.0.1:" + state.ControlPort;
            }
            if (state.Quit && !_allowClose)
            {
                // Programmatic quit: no unsaved-changes prompt (same as egui app.quit).
                _allowClose = true;
                Close();
                return;
            }
            Coords.Text = state.Cursor;
            SnapButton.IsChecked = state.Osnap;
            OrthoButton.IsChecked = state.Ortho;
            PolarButton.IsChecked = state.Polar;
            TrackButton.IsChecked = state.Otrack;
            DynButton.IsChecked = state.Dynmode;
            WeightButton.IsChecked = state.Lwdisplay;
            TransparencyButton.IsChecked = state.Transparency;
            QuickButton.IsChecked = state.Qpmode;
            _qpmode = state.Qpmode;
            _command = state.Command;
            _literalSpace = state.LiteralSpace;
            SyncDraftingUnits(state.Units);
            ShowDrawingTabs(state);
            ShowLevelBars(state);
            _draftingWindow?.Reload(state.Polar, state.Osnap, state.Dynmode, state.Ortho, state.Osmode, state.Polarang, state.DynpiCartesian, state.DynpiAbsolute);
            UpdateQuick();
            PlaceCommandBar();
        }
        var stats = Native.ReadStats(_host);
        Status.Text = stats.entities + " entities";
    }

    private void RefreshPanels()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        PanelSnapshot? snapshot;
        try
        {
            var text = Native.ReadText(_host, Native.TextPanels);
            using var document = JsonDocument.Parse(text);
            _quickJson = document.RootElement.TryGetProperty("quick", out var quick) && quick.ValueKind == JsonValueKind.Object ? quick.GetRawText() : null;
            _hoverQuickJson = document.RootElement.TryGetProperty("hoverQuick", out var hoverQuick) && hoverQuick.ValueKind == JsonValueKind.Object
                ? hoverQuick.GetRawText()
                : null;
            snapshot = JsonSerializer.Deserialize<PanelSnapshot>(text, JsonOpts);
            FillTakeoffPhases(document.RootElement);
        }
        catch (JsonException)
        {
            return;
        }
        if (snapshot == null)
        {
            return;
        }
        // Include hover so Properties / Layer / Blocks update on rollover, not only on pick.
        var key = snapshot.Uid + ":" + snapshot.Revision + ":" + snapshot.Selection + ":" + (snapshot.Hover ?? "") + ":" + (snapshot.FocusLayer ?? "") + ":" + (snapshot.FocusBlock ?? "") + ":" + snapshot.Layiso;
        if (key == _panelKey)
        {
            return;
        }
        _panelKey = key;
        // Only the panel that is on screen is rebuilt. The others are marked stale and rebuilt when their title is chosen: rebuilding the
        // Blocks thumbnails and the Layers list on every hover change cost about a second per zoom or pointer move on a large drawing.
        _stalePanels.UnionWith(AllPanelTabs);
        var visible = _leftFloat != null || !_leftCollapsed ? _sideTab : (SideTab?)null;
        var w = PerfLog.Enabled ? System.Diagnostics.Stopwatch.StartNew() : null;
        if (visible == SideTab.Properties && _stalePanels.Remove(SideTab.Properties))
        {
            FillProperties(snapshot);
            Density.Apply(PropertiesPanel);
        }
        w?.Mark("  props");
        if (visible == SideTab.Layers && _stalePanels.Remove(SideTab.Layers))
        {
            FillLayers(snapshot);
            Density.Apply(LayersPanel);
        }
        w?.Mark("  layers");
        if (visible == SideTab.Blocks && _stalePanels.Remove(SideTab.Blocks))
        {
            FillBlocks(snapshot);
            Density.Apply(BlocksPanel);
        }
        w?.Mark("  blocks");
        if (visible == SideTab.Project && _stalePanels.Remove(SideTab.Project))
        {
            FillProject();
            Density.Apply(ProjectPanel);
        }
        w?.Mark("  project");
        FillContext(snapshot);
        w?.Mark("  context");
        UpdateSideTabHeaders(snapshot);
        w?.Mark("  headers");
        _layersWindow?.OnDocumentRevision(snapshot.Uid, snapshot.Revision);
        w?.Mark("  layerswin");
        UpdateQuick();
        w?.Mark("  quick");
    }

    /// <summary>Tab titles show what is selected or under the cursor.</summary>
    private void UpdateSideTabHeaders(PanelSnapshot snapshot)
    {
        var propTitle = "Properties";
        if (snapshot.Selection > 0 && _quickJson != null)
        {
            propTitle = QuickTitle(_quickJson) ?? propTitle;
        }
        else if (_hoverQuickJson != null)
        {
            var hover = QuickTitle(_hoverQuickJson);
            if (!string.IsNullOrEmpty(hover))
            {
                propTitle = "Hover · " + hover;
            }
        }
        _sideTitles[SideTab.Properties] = propTitle;
        AutomationProperties.SetName(PropertiesTab, propTitle);

        var layerName = snapshot.FocusLayer;
        if (string.IsNullOrEmpty(layerName))
        {
            layerName = snapshot.Layers.FirstOrDefault(l => l.Current)?.Name;
        }
        var layerTitle = string.IsNullOrEmpty(layerName) ? "Layer" : "Layer · " + layerName;
        _sideTitles[SideTab.Layers] = layerTitle;
        AutomationProperties.SetName(LayersTab, layerTitle);

        var blockTitle = string.IsNullOrEmpty(snapshot.FocusBlock) ? "Blocks" : "Blocks · " + snapshot.FocusBlock;
        _sideTitles[SideTab.Blocks] = blockTitle;
        AutomationProperties.SetName(BlocksTab, blockTitle);
        RefreshSideRail();
    }

    private static string? QuickTitle(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            if (doc.RootElement.TryGetProperty("title", out var title) && title.ValueKind == JsonValueKind.String)
            {
                return title.GetString();
            }
        }
        catch (JsonException)
        {
        }
        return null;
    }

    private void OpenQuickSelect()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_quickSelectWindow == null)
        {
            _quickSelectWindow = new QuickSelectWindow(Content as FrameworkElement, _host, AfterCommand);
            _quickSelectWindow.Closed += (_, _) => _quickSelectWindow = null;
        }
        PresentToolWindow(_quickSelectWindow);
    }

    private string? _quickApplied;

    private void UpdateQuick()
    {
        var wanted = _qpmode && _command.Length == 0 && _quickJson != null;
        if (!wanted)
        {
            _quickWindow?.Close();
            return;
        }
        if (_quickWindow == null)
        {
            _quickWindow = new QuickWindow(Content as FrameworkElement, (command, json) =>
            {
                if (_host == IntPtr.Zero)
                {
                    return;
                }
                if (Native.Execute(_host, command, json) != 0)
                {
                    History.Text = Native.LastError(_host);
                }
                _panelKey = "";
                AfterCommand();
            });
            _quickWindow.Closed += (_, _) => _quickWindow = null;
            _quickWindow.Place(this);
            PresentToolWindow(_quickWindow);
        }
        if (_quickJson != null && !string.Equals(_quickJson, _quickApplied, StringComparison.Ordinal))
        {
            _quickApplied = _quickJson;               // the same fields again need no rebuild of the window (it cost ~0.2 s)
            PerfLog.Note("quick " + _quickJson.Length + " " + _quickJson[..Math.Min(160, _quickJson.Length)]);
            _quickWindow.Apply(_quickJson);
        }
    }

    private void OnLayers(object sender, RoutedEventArgs e) => OpenLayers();

    private void OpenDrafting()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_draftingWindow == null)
        {
            _draftingWindow = new DraftingWindow(Content as FrameworkElement, _host, () =>
            {
                RefreshChrome();
            });
            _draftingWindow.Closed += (_, _) => _draftingWindow = null;
            RefreshChrome();
        }
        PresentToolWindow(_draftingWindow);
    }

    private static readonly string[] DraftingUnitChoices = ["mm", "cm", "m", "in", "ft"];
    private static readonly string[] PaperChoices = ["A0", "A1", "A2", "A3", "A4"];
    private static readonly string[] OrientChoices = ["Landscape", "Portrait"];

    private void EnsureUnitsBox()
    {
        if (UnitsBox.Items.Count > 0)
        {
            return;
        }
        foreach (var unit in DraftingUnitChoices)
        {
            UnitsBox.Items.Add(unit);
        }
    }

    private void EnsurePaperBox()
    {
        if (PaperBox.Items.Count > 0)
        {
            return;
        }
        foreach (var paper in PaperChoices)
        {
            PaperBox.Items.Add(paper);
        }
    }

    private void EnsureOrientBox()
    {
        if (OrientBox.Items.Count > 0)
        {
            return;
        }
        foreach (var orient in OrientChoices)
        {
            OrientBox.Items.Add(orient);
        }
    }

    private void SyncPaperSize()
    {
        EnsurePaperBox();
        EnsureOrientBox();
        _filling = true;
        try
        {
            if (PaperBox.SelectedItem == null)
            {
                PaperBox.SelectedItem = "A1";
                PaperBox.PlaceholderText = null;
            }
            if (OrientBox.SelectedItem == null)
            {
                OrientBox.SelectedItem = "Landscape";
            }
            SyncOrientFromNameplate();
        }
        finally
        {
            _filling = false;
        }
    }

    private void SyncOrientFromNameplate()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        // Prefer Nameplate L / P (or legacy Title plate) so the status bar matches the active plate.
        if (Native.Execute(_host, "pagesetup", "{}") != 0)
        {
            return;
        }
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(_host));
            if (!doc.RootElement.TryGetProperty("layouts", out var layouts) || layouts.ValueKind != JsonValueKind.Array)
            {
                return;
            }
            JsonElement? page = null;
            var preferL = !string.Equals(OrientBox.SelectedItem as string, "Portrait", StringComparison.OrdinalIgnoreCase);
            string[] prefer = preferL
                ? ["Nameplate L", "Title plate", "Nameplate P"]
                : ["Nameplate P", "Title plate", "Nameplate L"];
            foreach (var wantName in prefer)
            {
                foreach (var layout in layouts.EnumerateArray())
                {
                    var name = layout.TryGetProperty("name", out var n) ? n.GetString() : null;
                    if (string.Equals(name, wantName, StringComparison.OrdinalIgnoreCase)
                        && layout.TryGetProperty("page", out var tp))
                    {
                        page = tp;
                        break;
                    }
                }
                if (page != null)
                {
                    break;
                }
            }
            if (page == null)
            {
                foreach (var layout in layouts.EnumerateArray())
                {
                    if (layout.TryGetProperty("page", out var first))
                    {
                        page = first;
                        break;
                    }
                }
            }
            if (page is not { } p)
            {
                return;
            }
            if (p.TryGetProperty("paper", out var paperEl) && paperEl.GetString() is string paper && paper.Length > 0)
            {
                var match = PaperChoices.FirstOrDefault(c => paper.Contains(c, StringComparison.OrdinalIgnoreCase));
                if (match != null && !Equals(PaperBox.SelectedItem, match))
                {
                    PaperBox.SelectedItem = match;
                }
            }
            var landscape = !p.TryGetProperty("landscape", out var land) || land.ValueKind != JsonValueKind.False;
            var want = landscape ? "Landscape" : "Portrait";
            if (!Equals(OrientBox.SelectedItem, want))
            {
                OrientBox.SelectedItem = want;
            }
        }
        catch (JsonException)
        {
        }
    }

    /// <summary>Show the saved paper size and orientation of a tab (A1 landscape until one is chosen).</summary>
    private void FillPageFor(string tab)
    {
        if (_host == IntPtr.Zero || Native.Execute(_host, "sheet.page", JsonSerializer.Serialize(new Dictionary<string, object> { ["tab"] = tab })) != 0)
        {
            return;
        }
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(_host));
            EnsurePaperBox();
            EnsureOrientBox();
            EnsureScaleBox();
            _filling = true;
            try
            {
                var paper = doc.RootElement.TryGetProperty("paper", out var p) ? p.GetString() : "A1";
                PaperBox.SelectedItem = PaperChoices.FirstOrDefault(c => string.Equals(c, paper, StringComparison.OrdinalIgnoreCase)) ?? "A1";
                var landscape = !doc.RootElement.TryGetProperty("landscape", out var l) || l.ValueKind != JsonValueKind.False;
                OrientBox.SelectedItem = landscape ? "Landscape" : "Portrait";
                var scale = doc.RootElement.TryGetProperty("scale", out var sc) ? sc.GetString() ?? "1:100" : "1:100";
                if (!ScaleBox.Items.Contains(scale))
                {
                    ScaleBox.Items.Add(scale);
                }

                ScaleBox.SelectedItem = scale;
            }
            finally
            {
                _filling = false;
            }
        }
        catch (JsonException)
        {
        }
    }

    private static readonly string[] ScaleChoices = ["1:10", "1:20", "1:50", "1:100", "1:200", "1:500"];

    private void EnsureScaleBox()
    {
        if (ScaleBox.Items.Count > 0)
        {
            return;
        }

        foreach (var scale in ScaleChoices)
        {
            ScaleBox.Items.Add(scale);
        }
    }

    private void OnScalePicked(object sender, SelectionChangedEventArgs e)
    {
        if (_filling || _host == IntPtr.Zero || ScaleBox.SelectedItem is not string)
        {
            return;
        }

        SavePageFor(_pageTab);
        AfterCommand();
    }

    /// <summary>Remember the paper size and orientation shown for the tab being edited.</summary>
    private void SavePageFor(string tab)
    {
        if (_host == IntPtr.Zero || PaperBox.SelectedItem is not string paper || OrientBox.SelectedItem is not string orient)
        {
            return;
        }
        Native.Execute(_host, "sheet.page", JsonSerializer.Serialize(new Dictionary<string, object>
        {
            ["tab"] = tab,
            ["paper"] = paper,
            ["landscape"] = !string.Equals(orient, "Portrait", StringComparison.OrdinalIgnoreCase),
            ["scale"] = ScaleBox.SelectedItem as string ?? "1:100",
        }));
        _sheetKey = "";
    }

    private void OnPaperPicked(object sender, SelectionChangedEventArgs e)
    {
        if (_filling || _host == IntPtr.Zero || PaperBox.SelectedItem is not string paper || paper.Length == 0)
        {
            return;
        }
        SavePageFor(_pageTab);
        // The paper size is kept for the tab and set on the layouts. It does not rescale the drawing: plot.config with apply resizes every
        // dimension and text to the paper (and, with no scale set, forced 1:100), which made the arrows and text huge.
        ApplyNameplatePage();
        AfterCommand();
    }

    private void OnOrientPicked(object sender, SelectionChangedEventArgs e)
    {
        if (_filling || _host == IntPtr.Zero || OrientBox.SelectedItem is not string)
        {
            return;
        }
        SavePageFor(_pageTab);
        ApplyNameplatePage();
        AfterCommand();
    }

    /// <summary>Push paper size onto Nameplate L and Nameplate P; orientation picks which plate Orient uses.</summary>
    private void ApplyNameplatePage()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var paper = PaperBox.SelectedItem as string ?? "A1";
        var landscape = !string.Equals(OrientBox.SelectedItem as string, "Portrait", StringComparison.OrdinalIgnoreCase);
        // Choosing a paper size or orientation only sets the pages of the layouts. (sheet.designer was called here too; it draws the
        // nameplate into the drawing, which a paper choice must not do.)
        foreach (var (name, land) in new (string, bool)[] { ("Nameplate L", true), ("Nameplate P", false) })
        {
            var setup = new Dictionary<string, object>
            {
                ["layout"] = name,
                ["paper"] = paper,
                ["landscape"] = land,
            };
            Native.Execute(_host, "pagesetup", JsonSerializer.Serialize(setup));
        }
        // Legacy single Title plate, if still present.
        Native.Execute(_host, "pagesetup", JsonSerializer.Serialize(new Dictionary<string, object>
        {
            ["layout"] = "Title plate",
            ["paper"] = paper,
            ["landscape"] = landscape,
        }));
    }

    private void SyncDraftingUnits(string chromeLabel)
    {
        EnsureUnitsBox();
        var unit = ParseDraftingUnit(chromeLabel);
        _filling = true;
        if (unit.Length > 0 && DraftingUnitChoices.Contains(unit, StringComparer.OrdinalIgnoreCase))
        {
            var choice = DraftingUnitChoices.First(u => u.Equals(unit, StringComparison.OrdinalIgnoreCase));
            if (!Equals(UnitsBox.SelectedItem, choice))
            {
                UnitsBox.SelectedItem = choice;
            }
            UnitsBox.PlaceholderText = null;
        }
        else if (UnitsBox.SelectedItem != null || UnitsBox.PlaceholderText != (string.IsNullOrEmpty(chromeLabel) ? "units?" : chromeLabel))
        {
            UnitsBox.SelectedItem = null;
            UnitsBox.PlaceholderText = string.IsNullOrEmpty(chromeLabel) ? "units?" : chromeLabel;
        }
        _filling = false;
    }

    /// <summary>Accepts chrome labels like <c>mm</c> or legacy <c>in ? in  1:100</c>.</summary>
    private static string ParseDraftingUnit(string label)
    {
        if (string.IsNullOrWhiteSpace(label))
        {
            return "";
        }
        var text = label.Trim();
        var cut = text.IndexOfAny(['?', '-', ' ', '\t']);
        if (cut > 0)
        {
            text = text[..cut].Trim();
        }
        return text.Equals("units?", StringComparison.OrdinalIgnoreCase) ? "" : text;
    }

    private void OnUnitsPicked(object sender, SelectionChangedEventArgs e)
    {
        if (_filling || _host == IntPtr.Zero || UnitsBox.SelectedItem is not string unit || unit.Length == 0)
        {
            return;
        }
        // Metadata only: drafting stays in the chosen unit; geometry is not rescaled. Scale at plot.
        if (Native.Execute(_host, "drawing.units", JsonSerializer.Serialize(new Dictionary<string, string>
            {
                ["units"] = unit,
                ["mode"] = "metadata",
            })) != 0)
        {
            Status.Text = Native.LastError(_host);
            AfterCommand();
            return;
        }
        AfterCommand();
    }

    private void OpenUnits()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_unitsWindow == null)
        {
            _unitsWindow = UnitsWindow.Open(this, _host, AfterCommand);
            _unitsWindow.Closed += (_, _) => _unitsWindow = null;
        }
        PresentToolWindow(_unitsWindow);
    }

    private void OpenTextStyle()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_textStyleWindow == null)
        {
            _textStyleWindow = TextStyleWindow.Open(this, _host, AfterCommand);
            _textStyleWindow.Closed += (_, _) => _textStyleWindow = null;
        }
        PresentToolWindow(_textStyleWindow);
    }

    private void OpenPageSetup()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_pageSetupWindow == null)
        {
            _pageSetupWindow = PageSetupWindow.Open(this, _host, AfterCommand);
            _pageSetupWindow.Closed += (_, _) => _pageSetupWindow = null;
        }
        PresentToolWindow(_pageSetupWindow);
    }

    private void OpenPlotStyles()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_plotStyleWindow == null)
        {
            _plotStyleWindow = PlotStyleWindow.Open(this, _host, _space, AfterCommand);
            _plotStyleWindow.Closed += (_, _) => _plotStyleWindow = null;
        }
        PresentToolWindow(_plotStyleWindow);
    }

    private void OpenBatchPlot()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_batchPlotWindow == null)
        {
            _batchPlotWindow = BatchPlotWindow.Open(this, _host, AfterCommand);
            _batchPlotWindow.Closed += (_, _) => _batchPlotWindow = null;
        }
        PresentToolWindow(_batchPlotWindow);
    }

    private void OpenDimStyle()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_dimStyleWindow == null)
        {
            _dimStyleWindow = DimStyleWindow.Open(this, _host, AfterCommand);
            _dimStyleWindow.Closed += (_, _) => _dimStyleWindow = null;
        }
        PresentToolWindow(_dimStyleWindow);
    }

    private void OpenLinetype()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_linetypeWindow == null)
        {
            _linetypeWindow = LinetypeWindow.Open(this, _host, AfterCommand);
            _linetypeWindow.Closed += (_, _) => _linetypeWindow = null;
        }
        PresentToolWindow(_linetypeWindow);
    }

    private void OpenParameters()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_parametersWindow == null)
        {
            _parametersWindow = ParametersWindow.Open(this, _host, AfterCommand);
            _parametersWindow.Closed += (_, _) => _parametersWindow = null;
        }
        PresentToolWindow(_parametersWindow);
    }

    private void OpenGraph()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_graphWindow == null)
        {
            _graphWindow = GraphWindow.Open(this, _host, AfterCommand);
            _graphWindow.Closed += (_, _) => _graphWindow = null;
        }
        PresentToolWindow(_graphWindow);
    }

    private void OpenRevisions()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        ShowSidePanel(SideTab.Revisions);
    }

    private void OpenHistory()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        ShowSidePanel(SideTab.History);
    }

    private void EnsureHistoryPanel()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_historyPanel == null)
        {
            _historyPanel = HistoryWindow.Mount(HistoryHost, _host, Content?.XamlRoot, AfterCommand);
        }
        else
        {
            _historyPanel.Load();
        }
    }

    private void EnsureRevisionsPanel()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_revisionsPanel == null)
        {
            _revisionsPanel = RevisionsWindow.Mount(RevisionsHost, _host, AfterCommand);
        }
        else
        {
            _revisionsPanel.Load();
        }
    }

    private void OpenSmartBlocks()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_smartBlockWindow == null)
        {
            _smartBlockWindow = SmartBlockWindow.Open(this, _host, AfterCommand, InsertBlock, InsertLibrarySmartBlock, WindowPreview);
            _smartBlockWindow.Closed += (_, _) => _smartBlockWindow = null;
        }
        PresentToolWindow(_smartBlockWindow);
    }

    private void OpenHatch()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_hatchWindow == null)
        {
            _hatchWindow = HatchWindow.Open(this, _host, AfterCommand);
            _hatchWindow.Closed += (_, _) =>
            {
                _hatchWindow = null;
                KeepShellShown();
            };
        }
        PresentToolWindow(_hatchWindow);
    }

    private void OpenPointStyle()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_pointStyleWindow == null)
        {
            _pointStyleWindow = PointStyleWindow.Open(this, _host, AfterCommand);
            _pointStyleWindow.Closed += (_, _) => _pointStyleWindow = null;
        }
        PresentToolWindow(_pointStyleWindow);
    }

    private void OpenMLeaderStyle()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_mleaderStyleWindow == null)
        {
            _mleaderStyleWindow = MLeaderStyleWindow.Open(this, _host, AfterCommand);
            _mleaderStyleWindow.Closed += (_, _) => _mleaderStyleWindow = null;
        }
        PresentToolWindow(_mleaderStyleWindow);
    }

    private void OpenRoomSettings()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_roomSettingsWindow == null)
        {
            _roomSettingsWindow = RoomSettingsWindow.Open(this, _host, AfterCommand, () => UnitsBox.SelectedItem as string ?? "");
            _roomSettingsWindow.Closed += (_, _) => _roomSettingsWindow = null;
        }
        PresentToolWindow(_roomSettingsWindow);
    }

    private void OpenLayers()
    {
        // Full properties (colour, linetype, filters, states) live in the manager window.
        // The side-panel Layer tab stays the compact lock / freeze / isolate list.
        EnsureLayers();
        if (_layersWindow == null)
        {
            return;
        }
        _layersWindow.AttachBody();
        _layersWindow.ShowMerged(false);
        _layersWindow.Reload("");
        PresentToolWindow(_layersWindow);
    }

    private void OnMergeLayers(object sender, RoutedEventArgs e) => DockLayersToSidePanel();

    private void EnsureLayers()
    {
        if (_host == IntPtr.Zero || _layersWindow != null)
        {
            return;
        }
        _layersWindow = new LayerManagerWindow(Content as FrameworkElement, _host, AfterCommand);
        _layersWindow.MergeRequested += (_, _) => DockLayersToSidePanel();
        _layersWindow.AppWindow.Closing += (_, args) =>
        {
            if (_shellClosing)
            {
                return;
            }
            args.Cancel = true;
            _layersWindow.Conceal();
        };
        _layersWindow.Closed += (_, _) => _layersWindow = null;
    }

    /// <summary>Hide the Layer Manager window and show the compact Layer side tab (never embeds the manager).</summary>
    private void DockLayersToSidePanel()
    {
        _mergeLayers = false;
        EnsureLayers();
        _layersWindow?.AttachBody();
        _layersWindow?.ShowMerged(false);
        _layersWindow?.Conceal();
        _leftCollapsed = false;
        ShowSidePanel(SideTab.Layers);
        ApplyColumns();
        SaveShell();
    }

    private void ApplyLayerHost(bool showWindow)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        EnsureLayers();
        if (_layersWindow == null)
        {
            return;
        }
        // Side panel always uses LayersPanel; never host the full manager UI there.
        if (_layersWindow.Body().Parent is Panel slot && ReferenceEquals(slot, LayersSlot))
        {
            slot.Children.Remove(_layersWindow.Body());
        }
        _layersWindow.AttachBody();
        _layersWindow.ShowMerged(false);
        if (showWindow)
        {
            _layersWindow.Reload("");
            PresentToolWindow(_layersWindow);
        }
        ApplyColumns();
    }

    private void FillProperties(PanelSnapshot snapshot)
    {
        PropertiesPanel.Children.Clear();
        if (snapshot.Selection > 0)
        {
            if (_quickJson != null)
            {
                FillSelectionProperties(_quickJson, readOnly: false);
            }
            return;
        }
        if (_hoverQuickJson != null)
        {
            FillSelectionProperties(_hoverQuickJson, readOnly: true);
            return;
        }
        PropertiesPanel.Children.Add(Heading("Properties"));
        PropertiesPanel.Children.Add(FieldLabel("No selection ? hover an object to inspect it"));
        var layers = new List<string>();
        var currentLayer = "0";
        foreach (var layer in snapshot.Layers)
        {
            layers.Add(layer.Name);
            if (layer.Current)
            {
                currentLayer = layer.Name;
            }
        }
        AddColorPicker("Color", Row(snapshot, "Color"), allowByLayerByBlock: true, value =>
            RunProp("color", new Dictionary<string, object> { ["color"] = value }));
        AddChoice("Layer", layers, currentLayer, value =>
            RunProp("layer.current", new Dictionary<string, object> { ["name"] = value }));
        AddChoice("Linetype", new[] { "ByLayer", "ByBlock", "Continuous" }, Row(snapshot, "Linetype"), value =>
            RunProp("linetype", new Dictionary<string, object> { ["current"] = value }));
        AddChoice("Lineweight", new[] { "ByLayer", "ByBlock", "Default", "0.25", "0.50", "1.00" }, NormalizeWeight(Row(snapshot, "Lineweight")), value =>
        {
            if (WeightValue(value) is object weight)
            {
                RunProp("lweight", new Dictionary<string, object> { ["lineweight"] = weight });
            }
        });
    }

    private static string Row(PanelSnapshot snapshot, string label)
    {
        foreach (var row in snapshot.Properties)
        {
            if (row.Label == label)
            {
                return row.Value;
            }
        }
        return "ByLayer";
    }

    private static string NormalizeWeight(string value)
    {
        if (value.Contains("0.25", StringComparison.Ordinal))
        {
            return "0.25";
        }
        if (value.Contains("0.50", StringComparison.Ordinal) || value.Contains("0.5 ", StringComparison.Ordinal))
        {
            return "0.50";
        }
        if (value.Contains("1.00", StringComparison.Ordinal) || value.Contains("1.0 ", StringComparison.Ordinal))
        {
            return "1.00";
        }
        if (value.Contains("Default", StringComparison.OrdinalIgnoreCase))
        {
            return "Default";
        }
        if (value.Contains("ByBlock", StringComparison.OrdinalIgnoreCase))
        {
            return "ByBlock";
        }
        return value.Contains("ByLayer", StringComparison.OrdinalIgnoreCase) ? "ByLayer" : value;
    }

    private static object? WeightValue(string value)
    {
        if (value is "ByLayer" or "ByBlock" or "Default")
        {
            return value;
        }
        var token = value.Replace("mm", "", StringComparison.OrdinalIgnoreCase).Trim();
        return double.TryParse(token, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var mm) && double.IsFinite(mm)
            ? mm
            : null;
    }

    private void FillSelectionProperties(string json, bool readOnly)
    {
        JsonDocument doc;
        try
        {
            doc = JsonDocument.Parse(json);
        }
        catch (JsonException)
        {
            return;
        }
        using (doc)
        {
            var root = doc.RootElement;
            var handles = new List<string>();
            if (root.TryGetProperty("handles", out var listed) && listed.ValueKind == JsonValueKind.Array)
            {
                foreach (var handle in listed.EnumerateArray())
                {
                    if (handle.ValueKind == JsonValueKind.String && handle.GetString() is string text && text.Length > 0)
                    {
                        handles.Add(text);
                    }
                }
            }
            var title = root.TryGetProperty("title", out var heading) && heading.ValueKind == JsonValueKind.String ? heading.GetString() ?? "Properties" : "Properties";
            if (readOnly)
            {
                title = "Hover · " + title;
            }
            PropertiesPanel.Children.Add(Heading(title));
            if (readOnly)
            {
                PropertiesPanel.Children.Add(FieldLabel("Rollover ? click to select"));
                AddReadOnly("Layer", Text(root, "layer"));
                AddReadOnly("Color", Text(root, "color"));
                AddReadOnly("Linetype", Text(root, "linetype"));
                AddReadOnly("Lineweight", Text(root, "lineweight"));
                AddReadOnly("Transparency", Text(root, "transparency"));
                AddReadOnly("Linetype scale", Text(root, "ltscale"));
                if (root.TryGetProperty("fields", out var hoverRows) && hoverRows.ValueKind == JsonValueKind.Array)
                {
                    foreach (var row in hoverRows.EnumerateArray())
                    {
                        var label = row.TryGetProperty("label", out var labelValue) && labelValue.ValueKind == JsonValueKind.String ? labelValue.GetString() ?? "" : "";
                        var value = row.TryGetProperty("value", out var fieldValue) && fieldValue.ValueKind == JsonValueKind.String ? fieldValue.GetString() ?? "" : "";
                        if (label.Length > 0)
                        {
                            AddReadOnly(label, value);
                        }
                    }
                }
                return;
            }
            var layers = Names(root, "layers");
            var linetypes = Names(root, "linetypes");
            AddChoice("Layer", layers, Text(root, "layer"), value => CommitChoice(handles, "layer", value));
            AddColorPicker("Color", Text(root, "color"), allowByLayerByBlock: true, value => CommitChoice(handles, "color", value));
            AddChoice("Linetype", linetypes.Count > 0 ? linetypes : new List<string> { "ByLayer", "ByBlock", "Continuous" }, Text(root, "linetype"), value => CommitChoice(handles, "linetype", value));
            AddChoice("Lineweight", new[] { "ByLayer", "ByBlock", "Default", "0.25", "0.50", "1.00" }, NormalizeWeight(Text(root, "lineweight")), value =>
            {
                if (WeightValue(value) is object weight)
                {
                    CommitChoice(handles, "lineweight", weight);
                }
            });
            AddChoice("Transparency", new[] { "ByLayer", "ByBlock", "0", "30", "50", "70", "90" }, Text(root, "transparency"), value => CommitChoice(handles, "transparency", value));
            AddNumber(handles, "ltscale", "Linetype scale", Text(root, "ltscale"));
            var patterns = Names(root, "patterns");
            if (patterns.Count > 0)
            {
                AddChoice("Pattern", patterns, Text(root, "pattern"), value => CommitChoice(handles, "pattern", value));
            }
            var fields = new Dictionary<string, TextBox>(StringComparer.Ordinal);
            if (root.TryGetProperty("fields", out var rows) && rows.ValueKind == JsonValueKind.Array)
            {
                foreach (var row in rows.EnumerateArray())
                {
                    var key = row.TryGetProperty("key", out var keyValue) && keyValue.ValueKind == JsonValueKind.String ? keyValue.GetString() ?? "" : "";
                    var label = row.TryGetProperty("label", out var labelValue) && labelValue.ValueKind == JsonValueKind.String ? labelValue.GetString() ?? key : key;
                    var value = row.TryGetProperty("value", out var fieldValue) && fieldValue.ValueKind == JsonValueKind.String ? fieldValue.GetString() ?? "" : "";
                    var text = row.TryGetProperty("text", out var kind) && kind.ValueKind == JsonValueKind.True;
                    // Pattern is shown as a combo when the library list is present.
                    if (key == "pattern" && patterns.Count > 0)
                    {
                        continue;
                    }
                    if (key is "height" or "textHeight")
                    {
                        AddTextSizeField(handles, key, label, value);
                        continue;
                    }
                    if (row.TryGetProperty("choices", out var choiceValues) && choiceValues.ValueKind == JsonValueKind.Array && key.Length > 0)
                    {
                        var names = new List<string>();
                        foreach (var choice in choiceValues.EnumerateArray())
                        {
                            if (choice.ValueKind == JsonValueKind.String && choice.GetString() is string name && name.Length > 0)
                            {
                                names.Add(name);
                            }
                        }
                        if (names.Count > 0)
                        {
                            AddChoice(label, names, value, picked => CommitChoice(handles, key, picked));
                            continue;
                        }
                    }
                    if (key.Length == 0)
                    {
                        PropertiesPanel.Children.Add(FieldLabel(label));
                        var read = new TextBlock { Text = value, TextWrapping = TextWrapping.Wrap };
                        AutomationProperties.SetName(read, label + " " + value);
                        PropertiesPanel.Children.Add(read);
                        continue;
                    }
                    var box = new TextBox { Text = value, Tag = value, MinHeight = Density.ControlHeight };
                    AutomationProperties.SetName(box, label);
                    fields[key] = box;
                    box.LostFocus += (_, _) => CommitPropField(box, key, text, handles, fields);
                    box.KeyDown += (_, args) =>
                    {
                        if (args.Key == Windows.System.VirtualKey.Enter)
                        {
                            CommitPropField(box, key, text, handles, fields);
                            args.Handled = true;
                        }
                    };
                    PropertiesPanel.Children.Add(FieldLabel(label));
                    PropertiesPanel.Children.Add(box);
                }
            }
        }
    }

    private void AddReadOnly(string label, string value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return;
        }
        PropertiesPanel.Children.Add(FieldLabel(label));
        var read = new TextBlock { Text = value, TextWrapping = TextWrapping.Wrap };
        AutomationProperties.SetName(read, label + " " + value);
        PropertiesPanel.Children.Add(read);
    }

    private static List<string> Names(JsonElement root, string name)
    {
        var names = new List<string>();
        if (!root.TryGetProperty(name, out var listed) || listed.ValueKind != JsonValueKind.Array)
        {
            return names;
        }
        foreach (var item in listed.EnumerateArray())
        {
            if (item.ValueKind == JsonValueKind.String && item.GetString() is string text && text.Length > 0)
            {
                names.Add(text);
            }
        }
        return names;
    }

    private static string Text(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() ?? "" : "";

    private void AddChoice(string label, IEnumerable<string> names, string current, Action<string> commit)
    {
        var box = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch, MinHeight = Density.ControlHeight };
        AutomationProperties.SetName(box, label);
        _filling = true;
        foreach (var name in names)
        {
            if (name.Length == 0 || box.Items.OfType<ComboBoxItem>().Any(item => item.Tag as string == name))
            {
                continue;
            }
            box.Items.Add(new ComboBoxItem { Content = name, Tag = name });
        }
        if (current.Length > 0 && box.Items.OfType<ComboBoxItem>().All(item => item.Tag as string != current))
        {
            box.Items.Insert(0, new ComboBoxItem { Content = current, Tag = current });
        }
        for (var i = 0; i < box.Items.Count; i++)
        {
            if (box.Items[i] is ComboBoxItem item && item.Tag as string == current)
            {
                box.SelectedIndex = i;
                break;
            }
        }
        _filling = false;
        box.SelectionChanged += (_, _) =>
        {
            if (_filling || box.SelectedItem is not ComboBoxItem picked || picked.Tag is not string value || value == "*VARIES*")
            {
                return;
            }
            commit(value);
        };
        PropertiesPanel.Children.Add(FieldLabel(label));
        PropertiesPanel.Children.Add(box);
    }

    private void AddColorPicker(string label, string current, bool allowByLayerByBlock, Action<string> commit)
    {
        PropertiesPanel.Children.Add(FieldLabel(label));
        PropertiesPanel.Children.Add(IndexColorWindow.CreatePickerButton(this, current, allowByLayerByBlock, commit));
    }

    private void AddTextSizeField(List<string> handles, string key, string label, string currentHeight)
    {
        PropertiesPanel.Children.Add(FieldLabel(string.IsNullOrEmpty(label) ? "Text size" : label));
        var rank = new ComboBox
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            MinHeight = Density.ControlHeight,
        };
        AutomationProperties.SetName(rank, "Text size");
        foreach (var (tag, title) in new (string, string)[]
        {
            ("small", "Small"),
            ("medium", "Medium"),
            ("large", "Large"),
            ("custom", "Custom"),
        })
        {
            rank.Items.Add(new ComboBoxItem { Content = title, Tag = tag });
        }
        rank.SelectedIndex = 3;
        var custom = new TextBox
        {
            Text = currentHeight,
            Tag = currentHeight,
            MinHeight = Density.ControlHeight,
            PlaceholderText = "Drawing units",
        };
        AutomationProperties.SetName(custom, "Custom text height");
        void CommitRank()
        {
            if (_filling || rank.SelectedItem is not ComboBoxItem item || item.Tag is not string tag)
            {
                return;
            }
            if (tag == "custom")
            {
                custom.Visibility = Visibility.Visible;
                return;
            }
            custom.Visibility = Visibility.Collapsed;
            RunProp("properties.set", new Dictionary<string, object> { ["handles"] = handles, ["textSize"] = tag });
        }
        void CommitCustom()
        {
            if (_filling || custom.Visibility != Visibility.Visible)
            {
                return;
            }
            var text = custom.Text?.Trim() ?? "";
            if (text.Length == 0 || text == custom.Tag as string)
            {
                return;
            }
            if (!double.TryParse(text, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var height)
                || !double.IsFinite(height)
                || height <= 0)
            {
                Status.Text = "Text height must be a positive number";
                return;
            }
            custom.Tag = text;
            RunProp("properties.set", new Dictionary<string, object> { ["handles"] = handles, [key] = height });
        }
        rank.SelectionChanged += (_, _) => CommitRank();
        custom.LostFocus += (_, _) => CommitCustom();
        custom.KeyDown += (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                CommitCustom();
                args.Handled = true;
            }
        };
        PropertiesPanel.Children.Add(rank);
        PropertiesPanel.Children.Add(custom);
    }

    private void RunProp(string command, Dictionary<string, object> values)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (Native.Execute(_host, command, JsonSerializer.Serialize(values)) != 0)
        {
            Status.Text = Native.LastError(_host);
            return;
        }
        _panelKey = "";
        RefreshChrome();
        RefreshPanels();
    }

    private void AddNumber(List<string> handles, string key, string label, string value)
    {
        if (value.Length == 0)
        {
            return;
        }
        var box = new TextBox { Text = value, Tag = value, MinHeight = Density.ControlHeight };
        AutomationProperties.SetName(box, label);
        box.LostFocus += (_, _) => CommitPropField(box, key, false, handles, new Dictionary<string, TextBox>());
        box.KeyDown += (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                CommitPropField(box, key, false, handles, new Dictionary<string, TextBox>());
                args.Handled = true;
            }
        };
        PropertiesPanel.Children.Add(FieldLabel(label));
        PropertiesPanel.Children.Add(box);
    }

    private void CommitChoice(List<string> handles, string key, object value)
    {
        if (handles.Count == 0 || value is "*VARIES*")
        {
            return;
        }
        RunProp("properties.set", new Dictionary<string, object> { ["handles"] = handles, [key] = value });
    }

    private void CommitPropField(TextBox box, string key, bool text, List<string> handles, Dictionary<string, TextBox> fields)
    {
        var value = box.Text.Trim();
        if (handles.Count == 0 || value.Length == 0 || value == "*VARIES*" || value == box.Tag as string)
        {
            return;
        }
        Dictionary<string, object> values;
        if (key is "centerX" or "centerY")
        {
            if (!fields.TryGetValue("centerX", out var xBox) || !fields.TryGetValue("centerY", out var yBox)
                || !double.TryParse(xBox.Text, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var x)
                || !double.TryParse(yBox.Text, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var y)
                || !double.IsFinite(x) || !double.IsFinite(y))
            {
                Status.Text = "Enter a number.";
                return;
            }
            values = new Dictionary<string, object> { ["handles"] = handles, ["center"] = new[] { x, y } };
        }
        else if (text)
        {
            values = new Dictionary<string, object> { ["handles"] = handles, [key] = value };
        }
        else if (double.TryParse(value, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var number) && double.IsFinite(number))
        {
            values = new Dictionary<string, object> { ["handles"] = handles, [key] = number };
        }
        else
        {
            Status.Text = "Enter a number.";
            return;
        }
        box.Tag = value;
        RunProp("properties.set", values);
    }

    private void FillLayers(PanelSnapshot snapshot)
    {
        LayersPanel.Children.Clear();
        var focus = snapshot.FocusLayer;
        if (!string.IsNullOrEmpty(focus))
        {
            var why = snapshot.Selection > 0 ? "Selected object" : "Under cursor";
            LayersPanel.Children.Add(Heading("Layer · " + focus));
            LayersPanel.Children.Add(FieldLabel(why));
        }
        else
        {
            LayersPanel.Children.Add(Heading("Layer"));
        }
        LayersPanel.Children.Add(FieldLabel("Lock, freeze and isolate only. Colour and linetype are in Layer Manager."));
        LayersPanel.Children.Add(LayerPanelActions());
        LayersPanel.Children.Add(LayerHeader());
        // Put the focused layer first so selection/hover is visible without scrolling.
        var ordered = snapshot.Layers
            .Select((layer, index) => (layer, index))
            .OrderByDescending(pair => pair.layer.Focus || pair.layer.Selected || pair.layer.Hovered)
            .ThenBy(pair => pair.index)
            .ToList();
        for (var i = 0; i < ordered.Count; i++)
        {
            LayersPanel.Children.Add(LayerLine(ordered[i].layer, i, snapshot.Layiso));
        }
        if (snapshot.LayersTruncated)
        {
            LayersPanel.Children.Add(FieldLabel("Further layers are not listed."));
        }
    }

    private UIElement LayerPanelActions()
    {
        var stack = new StackPanel { Spacing = Density.SpaceSm };
        var manager = new Button
        {
            Content = "Layer Manager?",
            MinHeight = Density.ControlHeight,
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        Density.Name(manager, "Open layer manager");
        ToolTipService.SetToolTip(manager, "Colour, linetype, lineweight and layer states");
        manager.Click += (_, _) => OpenLayers();
        stack.Children.Add(manager);

        stack.Children.Add(FieldLabel("Layer set"));
        var spaceBox = new ComboBox
        {
            MinHeight = Density.ControlHeight,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            PlaceholderText = "No saved layer set",
        };
        AutomationProperties.SetName(spaceBox, "Layer set");
        ToolTipService.SetToolTip(spaceBox, "Activate a saved layer set (visible, frozen and locked layers)");
        FillLayerSetCombo(spaceBox);
        spaceBox.SelectionChanged += (_, _) =>
        {
            if (_filling || spaceBox.SelectedItem is not ComboBoxItem picked || picked.Tag is not string id || id.Length == 0)
            {
                return;
            }
            RunLayer("space.activate", new Dictionary<string, object> { ["id"] = id });
        };
        stack.Children.Add(spaceBox);

        var saveRow = new Grid { ColumnSpacing = Density.SpaceSm };
        saveRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        saveRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var nameBox = new TextBox
        {
            MinHeight = Density.ControlHeight,
            PlaceholderText = "New set name",
        };
        AutomationProperties.SetName(nameBox, "New layer set name");
        var createSet = new Button
        {
            Content = "Create",
            MinHeight = Density.ControlHeight,
            MinWidth = 72,
        };
        Density.Name(createSet, "Create layer set from current layers");
        ToolTipService.SetToolTip(createSet, "Save the current layer visibility as a named layer set");
        createSet.Click += (_, _) =>
        {
            var name = (nameBox.Text ?? "").Trim();
            if (name.Length == 0)
            {
                Status.Text = "Enter a name for the layer set";
                return;
            }
            RunLayer("space.save", new Dictionary<string, object> { ["name"] = name, ["activate"] = true });
            nameBox.Text = "";
            FillLayerSetCombo(spaceBox);
        };
        Grid.SetColumn(nameBox, 0);
        Grid.SetColumn(createSet, 1);
        saveRow.Children.Add(nameBox);
        saveRow.Children.Add(createSet);
        stack.Children.Add(saveRow);

        stack.Children.Add(FieldLabel("Architecture layers"));
        var setRow = new Grid { ColumnSpacing = Density.SpaceSm };
        setRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        setRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var setBox = new ComboBox
        {
            MinHeight = Density.ControlHeight,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            PlaceholderText = "Which set",
        };
        foreach (var (tag, label) in new (string, string)[]
        {
            ("standard", "Standard"),
            ("permit", "Permit"),
            ("presentation", "Presentation"),
            ("both", "Standard + Permit"),
            ("all", "All sets"),
        })
        {
            setBox.Items.Add(new ComboBoxItem { Content = label, Tag = tag });
        }
        setBox.SelectedIndex = 0;
        AutomationProperties.SetName(setBox, "Architectural layer set to create");
        var archCreate = new Button
        {
            Content = "Add",
            MinHeight = Density.ControlHeight,
            MinWidth = 72,
        };
        Density.Name(archCreate, "Create architectural layer set");
        ToolTipService.SetToolTip(archCreate, "Create the selected architectural layer set");
        archCreate.Click += (_, _) =>
        {
            var set = setBox.SelectedItem is ComboBoxItem item && item.Tag is string tag ? tag : "standard";
            RunLayer("hcwlayers", new Dictionary<string, object> { ["set"] = set });
        };
        Grid.SetColumn(setBox, 0);
        Grid.SetColumn(archCreate, 1);
        setRow.Children.Add(setBox);
        setRow.Children.Add(archCreate);
        stack.Children.Add(setRow);
        return stack;
    }

    private void FillLayerSetCombo(ComboBox box)
    {
        var names = new List<(string, string)>();
        string? active = null;
        if (_host != IntPtr.Zero && Native.Execute(_host, "space.list", "{}") == 0)
        {
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(_host));
                active = doc.RootElement.TryGetProperty("active", out var value) && value.ValueKind == JsonValueKind.String
                    ? value.GetString()
                    : null;
                if (doc.RootElement.TryGetProperty("spaces", out var spaces))
                {
                    foreach (var space in spaces.EnumerateArray())
                    {
                        var id = space.TryGetProperty("id", out var ident) ? ident.GetString() ?? "" : "";
                        var name = space.TryGetProperty("name", out var label) ? label.GetString() ?? id : id;
                        if (id.Length > 0)
                        {
                            names.Add((id, name));
                        }
                    }
                }
            }
            catch (JsonException)
            {
            }
        }
        var was = _filling;
        _filling = true;
        FillCombo(box, names, active);
        if (names.Count == 0)
        {
            box.PlaceholderText = "No saved layer set";
            box.SelectedIndex = -1;
        }
        _filling = was;
    }

    private static Grid LayerHeader()
    {
        var header = LayerGrid();
        LayerCell(header, 0, "Name");
        LayerCell(header, 1, "Lock");
        LayerCell(header, 2, "Freeze");
        LayerCell(header, 3, "Isolate");
        return header;
    }

    private Grid LayerLine(LayerRow layer, int index, bool layiso)
    {
        var name = layer.Name;
        var grid = LayerGrid();
        var focus = layer.Focus || layer.Selected || layer.Hovered;
        grid.Background = focus
            ? Density.SelectionFill(layer.Hovered && !layer.Selected ? (byte)40 : (byte)64)
            : layer.Current
                ? Density.SelectionFill(32)
                : index % 2 == 1
                    ? Density.ZebraFill()
                    : new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        var label = new Button
        {
            Content = new TextBlock { Text = name, FontSize = Density.FontBody, TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center },
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Left,
            MinHeight = Density.ControlHeight,
            Padding = new Thickness(4, 0, 4, 0),
            FontSize = Density.FontBody,
            FontWeight = focus || layer.Current ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
        };
        AutomationProperties.SetName(label, "Layer " + name);
        label.Click += (_, _) => RunLayer("layer.current", new Dictionary<string, object> { ["name"] = name });
        Put(grid, 0, label);
        var lockKind = layer.Locked ? "lock" : "unlock";
        var lockTip = (layer.Locked ? "Unlock layer " : "Lock layer ") + name;
        Put(grid, 1, LayerIcon(lockKind, lockTip, () => RunLayer("layer.set", new Dictionary<string, object> { ["name"] = name, ["locked"] = !layer.Locked })));
        var freezeKind = layer.Frozen ? "snowflake" : "sun";
        var freezeTip = (layer.Frozen ? "Thaw layer " : "Freeze layer ") + name;
        Put(grid, 2, LayerIcon(freezeKind, freezeTip, () => RunLayer("layer.set", new Dictionary<string, object> { ["name"] = name, ["frozen"] = !layer.Frozen })));
        // When isolation is on and this layer is still visible, the control restores all layers.
        var isolatedHere = layiso && layer.On;
        var isoKind = isolatedHere ? "unisolate" : "isolate";
        var isoTip = isolatedHere ? "Unisolate layers" : "Isolate layer " + name;
        Put(grid, 3, LayerIcon(isoKind, isoTip, () =>
        {
            if (isolatedHere)
            {
                RunLayer("layuniso", new Dictionary<string, object>());
            }
            else
            {
                RunLayer("layiso", new Dictionary<string, object> { ["name"] = name });
            }
        }));
        Density.Apply(grid);
        return grid;
    }

    private static Grid LayerGrid()
    {
        var grid = new Grid { MinHeight = Density.ControlHeight };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Density.MinTarget) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Density.MinTarget) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Density.MinTarget) });
        return grid;
    }

    private static void LayerCell(Grid grid, int column, string title)
    {
        var text = new TextBlock
        {
            Text = title,
            FontSize = Density.FontBody,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(4, 0, 0, 0),
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
        };
        Put(grid, column, text);
    }

    private static Button LayerIcon(string kind, string access, Action click)
    {
        var button = new Button
        {
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            Padding = new Thickness(0),
            Width = Density.MinTarget,
            Height = Density.MinTarget,
            HorizontalAlignment = HorizontalAlignment.Center,
            Content = new DrawnIcon { Kind = kind },
        };
        AutomationProperties.SetName(button, access);
        button.Click += (_, _) => click();
        return button;
    }

    private static void Put(Grid grid, int column, FrameworkElement child)
    {
        Grid.SetColumn(child, column);
        grid.Children.Add(child);
    }

    private string? _selectedBlock;
    private ulong _blockPreviewUid;
    private ulong _blockPreviewRevision;
    private readonly Dictionary<string, string> _blockPreviewJson = new(StringComparer.OrdinalIgnoreCase);
    /// <summary>Library DXF/DWG previews keyed by absolute path (stable across drawing revision).</summary>
    private readonly Dictionary<string, string> _filePreviewJson = new(StringComparer.OrdinalIgnoreCase);
    private bool _fillingProject;
    private string _projectPanelKey = "";

    /// <summary>The project's own details, in panel order: the engine key and the label shown.</summary>
    private static readonly (string Key, string Label)[] ProjectDetailFields =
    [
        ("title", "Project name"),
        ("owner", "Client"),
        ("location", "Location"),
        ("address", "Site address"),
        ("pid", "Property ID"),
        ("siteArea", "Site area (m2)"),
        ("plotUse", "Plot use"),
        ("authority", "Approving authority"),
        ("architect", "Architect"),
        ("engineer", "Engineer"),
        ("date", "Date"),
        ("summary", "Summary"),
    ];

    /// <summary>What the company file gave the drawing (read only here), in panel order.</summary>
    private static readonly (string Key, string Label)[] CompanyDetailFields =
    [
        ("name", "Company"),
        ("address", "Address"),
        ("phone", "Phone"),
        ("email", "Email"),
        ("architect_name", "Architect"),
        ("coa_registration", "COA registration"),
        ("pan", "PAN"),
        ("gstin", "GSTIN"),
        ("prepared_on", "Prepared on"),
    ];

    private static Dictionary<string, string> TextMap(JsonElement root, string name)
    {
        var map = new Dictionary<string, string>();
        if (root.TryGetProperty(name, out var obj) && obj.ValueKind == JsonValueKind.Object)
        {
            foreach (var prop in obj.EnumerateObject())
            {
                if (prop.Value.ValueKind == JsonValueKind.String)
                {
                    map[prop.Name] = prop.Value.GetString() ?? "";
                }
            }
        }
        return map;
    }

    private void FillProject()
    {
        if (_host == IntPtr.Zero || _fillingProject)
        {
            return;
        }
        if (Native.Execute(_host, "project", "{}") != 0)
        {
            return;
        }
        Dictionary<string, string> details;
        Dictionary<string, string> company;
        string slab = "150";
        string beam = "450";
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(_host));
            details = TextMap(doc.RootElement, "details");
            company = TextMap(doc.RootElement, "company");
            if (doc.RootElement.TryGetProperty("structure", out var st))
            {
                if (st.TryGetProperty("slabMm", out var s) && s.TryGetDouble(out var sv))
                {
                    slab = sv.ToString("0.###");
                }
                if (st.TryGetProperty("beamMm", out var b) && b.TryGetDouble(out var bv))
                {
                    beam = bv.ToString("0.###");
                }
            }
        }
        catch (JsonException)
        {
            return;
        }
        var key = string.Join("\u0001", ProjectDetailFields.Select(f => details.GetValueOrDefault(f.Key, "")))
            + "|" + string.Join("\u0001", CompanyDetailFields.Select(f => company.GetValueOrDefault(f.Key, "")))
            + "|" + slab + "|" + beam;
        if (key == _projectPanelKey && ProjectPanel.Children.Count > 0)
        {
            return;
        }
        _projectPanelKey = key;
        _fillingProject = true;
        try
        {
            ProjectPanel.Children.Clear();
            // Read only: the project is edited in the Project Wizard (File menu).
            void Row(string label, string value)
            {
                ProjectPanel.Children.Add(FieldLabel(label));
                ProjectPanel.Children.Add(new TextBlock
                {
                    Text = value.Length == 0 ? "-" : value,
                    TextWrapping = TextWrapping.Wrap,
                    IsTextSelectionEnabled = true,
                    FontSize = Density.FontBody,
                    Opacity = value.Length == 0 ? 0.55 : 1.0,
                });
            }
            var edit = Density.DialogAction("Edit project...", primary: true);
            ToolTipService.SetToolTip(edit, "Opens the Project Wizard (File menu) to change these details");
            edit.Click += (_, _) => StartCommand("project.wizard");
            ProjectPanel.Children.Add(edit);

            ProjectPanel.Children.Add(Heading("Project"));
            foreach (var (detail, label) in ProjectDetailFields)
            {
                Row(label, details.GetValueOrDefault(detail, ""));
            }
            ProjectPanel.Children.Add(Heading("Company"));
            var shown = 0;
            foreach (var (detail, label) in CompanyDetailFields)
            {
                var value = company.GetValueOrDefault(detail, "");
                if (value.Length > 0)
                {
                    shown++;
                    Row(label, value);
                }
            }
            if (shown == 0)
            {
                ProjectPanel.Children.Add(FieldLabel("No company details in this drawing yet. Open from Studio (or use Read company file in the Project Wizard) to load them."));
            }
            else
            {
                ProjectPanel.Children.Add(FieldLabel("Company identity is owned by Studio and is refresh-only here. Change it in Studio, then refresh from the company file in the Project Wizard."));
            }
            ProjectPanel.Children.Add(Heading("Structure"));
            Row("Default slab thickness (mm)", slab);
            Row("Default beam depth including slab (mm)", beam);
        }
        finally
        {
            _fillingProject = false;
        }
    }

    private void FillBlocks(PanelSnapshot snapshot)
    {
        BlocksPanel.Children.Clear();
        // Canvas selection/hover wins over the last clicked thumbnail.
        if (!string.IsNullOrEmpty(snapshot.FocusBlock))
        {
            _selectedBlock = snapshot.FocusBlock;
        }
        if (!string.IsNullOrEmpty(_selectedBlock) && snapshot.Blocks.All(b => !string.Equals(b.Name, _selectedBlock, StringComparison.OrdinalIgnoreCase)))
        {
            _selectedBlock = null;
        }

        BlocksPanel.Children.Add(Heading(string.IsNullOrEmpty(snapshot.FocusBlock) ? "Current drawing" : "Current drawing · " + snapshot.FocusBlock));
        if (!string.IsNullOrEmpty(snapshot.FocusBlock))
        {
            BlocksPanel.Children.Add(FieldLabel(snapshot.Selection > 0 ? "Selected insert" : "Under cursor"));
        }
        if (snapshot.Blocks.Count == 0)
        {
            BlocksPanel.Children.Add(FieldLabel("This drawing defines no blocks."));
        }
        else
        {
            // Blocks palette: medium thumbnails with title (click = select, double-click = insert).
            var ordered = snapshot.Blocks
                .OrderByDescending(b => b.Focus || b.Selected || b.Hovered || string.Equals(b.Name, _selectedBlock, StringComparison.OrdinalIgnoreCase))
                .ThenBy(b => b.Name, StringComparer.OrdinalIgnoreCase)
                .ToList();
            var grid = new Grid { ColumnSpacing = Density.SpaceMd, RowSpacing = Density.SpaceMd };
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            for (var i = 0; i < ordered.Count; i++)
            {
                var row = i / 2;
                while (grid.RowDefinitions.Count <= row)
                {
                    grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
                }
                var card = MakeBlockThumbnail(ordered[i], snapshot);
                Grid.SetRow(card, row);
                Grid.SetColumn(card, i % 2);
                grid.Children.Add(card);
            }
            BlocksPanel.Children.Add(grid);
        }

        var focus = snapshot.Blocks.FirstOrDefault(b => string.Equals(b.Name, _selectedBlock, StringComparison.OrdinalIgnoreCase));
        if (focus != null)
        {
            BlocksPanel.Children.Add(new Border { Height = 1, Margin = new Thickness(0, 8, 0, 8), Background = StripBrush() });
            BlocksPanel.Children.Add(FieldLabel(focus.Entities + " objects · " + focus.References + " references"));
            if (!string.IsNullOrEmpty(focus.Description))
            {
                BlocksPanel.Children.Add(FieldLabel("Description"));
                BlocksPanel.Children.Add(new TextBlock { Text = focus.Description, TextWrapping = TextWrapping.Wrap });
            }
            var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceMd };
            var insert = new Button { Content = "Insert", MinHeight = Density.ControlHeight };
            Density.Name(insert, "Insert block " + focus.Name);
            ToolTipService.SetToolTip(insert, "Place at the picked point at drawing scale (1:1)");
            insert.Click += (_, _) => InsertBlock(focus.Name);
            actions.Children.Add(insert);
            var select = new Button
            {
                Content = "Select references",
                MinHeight = Density.ControlHeight,
                IsEnabled = focus.Handles.Count > 0,
            };
            Density.Name(select, "Select references of " + focus.Name);
            select.Click += (_, _) => SelectBlockReferences(focus);
            actions.Children.Add(select);
            BlocksPanel.Children.Add(actions);
            if (focus.Attributes.Count > 0)
            {
                BlocksPanel.Children.Add(FieldLabel("Attribute definitions"));
                foreach (var attr in focus.Attributes)
                {
                    BlocksPanel.Children.Add(new TextBlock { Text = attr });
                }
                BlocksPanel.Children.Add(FieldLabel("Edit attribute values on a reference with ATTEDIT."));
            }
        }
        if (snapshot.BlocksTruncated)
        {
            BlocksPanel.Children.Add(FieldLabel("Further blocks are not listed."));
        }
        BlocksPanel.Children.Add(FieldLabel("Double-click a thumbnail to insert. External references are not supported."));
        BlocksPanel.Children.Add(new Border { Height = 1, Margin = new Thickness(0, 8, 0, 8), Background = StripBrush() });
        AddBlockLibrary(snapshot);
        AddFurnitureCatalog();
    }

    /// <summary>The one block library: per discipline, the parametric blocks (sizes and options are parameters) and the ready-made symbols,
    /// as thumbnails. The Blocks window lists the same blocks with this drawing's own.</summary>
    private void AddBlockLibrary(PanelSnapshot snapshot)
    {
        BlocksPanel.Children.Add(Heading("Block library"));
        BlocksPanel.Children.Add(FieldLabel("RCC, steel, plumbing, HVAC, electrical and interior blocks: parametric ones (edit a placed one in Properties or by its grips) and ready-made symbols (CC0, millimetres). Click a thumbnail to insert."));
        var open = new Button { Content = "Blocks...", MinHeight = Density.ControlHeight };
        Density.Name(open, "Open blocks");
        ToolTipService.SetToolTip(open, "Pick a block from this drawing or the library, set its parameters and insert it");
        open.Click += (_, _) => OpenSmartBlocks();
        BlocksPanel.Children.Add(open);

        var symbols = FurnitureCatalog.LoadDisciplines();
        var order = new List<string> { "rcc", "structural", "plumbing", "hvac", "electrical", "interior" };
        order.AddRange(snapshot.Library.Select(l => l.Discipline.ToLowerInvariant()).Where(d => d.Length > 0 && !order.Contains(d)).Distinct());
        foreach (var id in order)
        {
            var parametric = snapshot.Library.Where(l => string.Equals(l.Discipline, id, StringComparison.OrdinalIgnoreCase)).ToList();
            var set = symbols.FirstOrDefault(s => s.Id == id);
            var count = parametric.Count + (set?.Entries.Count ?? 0);
            if (count == 0)
            {
                continue;
            }
            var body = new StackPanel { Spacing = Density.SpaceSm };
            var expander = new Expander
            {
                Header = DisciplineTitle(id) + " (" + count + ")",
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Stretch,
                Content = body,
            };
            Density.Name(expander, DisciplineTitle(id));
            var built = false;
            void Build()
            {
                if (built)
                {
                    return;
                }
                built = true;
                if (parametric.Count > 0)
                {
                    body.Children.Add(FieldLabel("PARAMETRIC"));
                    AddSmartTiles(body, parametric);
                }
                if (set != null)
                {
                    body.Children.Add(FieldLabel("SYMBOLS"));
                    AddCatalogGrid(body, set.Entries);
                }
            }
            expander.Expanding += (_, _) =>
            {
                _openCatalogs.Add(id);
                Build();
            };
            expander.Collapsed += (_, _) => _openCatalogs.Remove(id);
            if (_openCatalogs.Contains(id))
            {
                Build();
                expander.IsExpanded = true;
            }
            BlocksPanel.Children.Add(expander);
        }
        BlocksPanel.Children.Add(new Border { Height = 1, Margin = new Thickness(0, 8, 0, 8), Background = StripBrush() });
    }

    private static string DisciplineTitle(string id) => id switch
    {
        "rcc" => "RCC",
        "hvac" => "HVAC",
        _ => System.Globalization.CultureInfo.InvariantCulture.TextInfo.ToTitleCase(id),
    };

    private void AddSmartTiles(Panel target, IReadOnlyList<LibraryRow> entries)
    {
        var tiles = new Grid { ColumnSpacing = Density.SpaceMd, RowSpacing = Density.SpaceMd };
        tiles.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        tiles.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        for (var i = 0; i < entries.Count; i++)
        {
            var entry = entries[i];
            var row = i / 2;
            while (tiles.RowDefinitions.Count <= row)
            {
                tiles.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            }
            var card = DesignCenterCard(false, "Insert block " + entry.Title, entry.Title + (entry.Description.Length > 0 ? ": " + entry.Description : ""));
            var stack = new StackPanel { Spacing = Density.SpaceXs };
            stack.Children.Add(PreviewCanvas("Block preview " + entry.Title, CachedSmartBlockPreviewJson(entry.Id), 72));
            stack.Children.Add(new TextBlock
            {
                Text = entry.Title,
                FontSize = Density.FontCaption,
                TextAlignment = TextAlignment.Center,
                TextWrapping = TextWrapping.Wrap,
                MaxLines = 2,
                TextTrimming = TextTrimming.CharacterEllipsis,
            });
            card.Child = stack;
            var id = entry.Id;
            card.Tapped += (_, _) => InsertLibrarySmartBlock(id);
            Grid.SetRow(card, row);
            Grid.SetColumn(card, i % 2);
            tiles.Children.Add(card);
        }
        target.Children.Add(tiles);
    }

    private readonly Dictionary<string, string> _smartPreviewJson = new();

    /// <summary>The thumbnail geometry of a smart block with its default parameters; kept for the session.</summary>
    private string? CachedSmartBlockPreviewJson(string id)
    {
        if (_smartPreviewJson.TryGetValue(id, out var cached))
        {
            return cached;
        }
        if (_host == IntPtr.Zero || Native.Execute(_host, "blocks.preview", JsonSerializer.Serialize(new Dictionary<string, object> { ["smartblock"] = id })) != 0)
        {
            return null;
        }
        var json = Native.ReadResult(_host);
        if (!string.IsNullOrEmpty(json))
        {
            _smartPreviewJson[id] = json;
        }
        return json;
    }

    /// <summary>A parametric block goes in with its default parameters; they are entered in Properties once it is placed and selected.</summary>
    private void InsertLibrarySmartBlock(string id)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var preset = JsonSerializer.Serialize(new Dictionary<string, string> { ["id"] = id, ["params"] = "", ["rotation"] = "0" });
        if (Native.StartForm(_host, "smartblock.insert", preset) != 0)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        AfterCommand();
    }

    /// <summary>The thumbnail the Blocks window shows for a parametric block (default parameters) or a block of this drawing.</summary>
    private UIElement WindowPreview(string id, bool drawing)
    {
        string? json = null;
        if (drawing && _host != IntPtr.Zero
            && Native.Execute(_host, "blocks.preview", JsonSerializer.Serialize(new Dictionary<string, object> { ["name"] = id })) == 0)
        {
            json = Native.ReadResult(_host);
        }
        else if (!drawing)
        {
            json = CachedSmartBlockPreviewJson(id);
        }
        return PreviewCanvas("Block preview " + id, json, 84);
    }

    /// <summary>Two-column thumbnail grids of <paramref name="entries"/>, one per category.</summary>
    private void AddCatalogGrid(Panel target, IReadOnlyList<FurnitureCatalog.Entry> entries)
    {
        string? lastCat = null;
        Grid? grid = null;
        var col = 0;
        var row = 0;
        foreach (var entry in entries)
        {
            if (grid == null || !string.Equals(lastCat, entry.Item.Category, StringComparison.Ordinal))
            {
                lastCat = entry.Item.Category;
                target.Children.Add(FieldLabel((lastCat ?? "").ToUpperInvariant()));
                grid = new Grid { ColumnSpacing = Density.SpaceMd, RowSpacing = Density.SpaceMd };
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                target.Children.Add(grid);
                col = 0;
                row = 0;
            }
            while (grid.RowDefinitions.Count <= row)
            {
                grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            }
            var card = MakeFurnitureThumbnail(entry);
            Grid.SetRow(card, row);
            Grid.SetColumn(card, col);
            grid.Children.Add(card);
            col++;
            if (col >= 2)
            {
                col = 0;
                row++;
            }
        }
    }

    private Border MakeBlockThumbnail(BlockRow block, PanelSnapshot snapshot)
    {
        var name = block.Name;
        var selected = string.Equals(_selectedBlock, name, StringComparison.OrdinalIgnoreCase) || block.Focus || block.Selected || block.Hovered;
        var card = DesignCenterCard(selected, "Block " + name, name + " ? click to select, double-click to insert");
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(BlockPreview(name, snapshot, 96));
        stack.Children.Add(new TextBlock
        {
            Text = name,
            FontSize = Density.FontCaption,
            FontWeight = selected ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxLines = 1,
            HorizontalAlignment = HorizontalAlignment.Center,
            TextAlignment = TextAlignment.Center,
        });
        card.Child = stack;
        card.PointerPressed += (_, e) =>
        {
            if (e.GetCurrentPoint(card).Properties.IsLeftButtonPressed)
            {
                _selectedBlock = name;
                _panelKey = "";
                RefreshPanels();
            }
        };
        card.DoubleTapped += (_, _) => InsertBlock(name);
        return card;
    }

    /// <summary>DesignCenter-style tile: quiet chrome, light-blue selection, hover wash.</summary>
    private Border DesignCenterCard(bool selected, string a11yName, string tip)
    {
        var idle = Density.ThemeBrush("CardBackgroundFillColorDefaultBrush")
            ?? new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        var hover = Density.ThemeBrush("SubtleFillColorSecondaryBrush")
            ?? new SolidColorBrush(Windows.UI.Color.FromArgb(255, 232, 232, 232));
        var picked = Density.ThemeBrush("AccentFillColorSecondaryBrush")
            ?? Density.ThemeBrush("SubtleFillColorTertiaryBrush")
            ?? new SolidColorBrush(Windows.UI.Color.FromArgb(255, 228, 232, 248));
        var card = new Border
        {
            Padding = new Thickness(4, 6, 4, 4),
            CornerRadius = new CornerRadius(2),
            BorderThickness = new Thickness(0),
            Background = selected ? picked : idle,
        };
        Density.Name(card, a11yName);
        ToolTipService.SetToolTip(card, tip);
        card.PointerEntered += (_, _) =>
        {
            if (!selected)
            {
                card.Background = hover;
            }
        };
        card.PointerExited += (_, _) =>
        {
            card.Background = selected ? picked : idle;
        };
        return card;
    }

    private string? CachedBlockPreviewJson(string name, PanelSnapshot snapshot)
    {
        if (snapshot.Uid != _blockPreviewUid || snapshot.Revision != _blockPreviewRevision)
        {
            _blockPreviewJson.Clear();
            _blockPreviewUid = snapshot.Uid;
            _blockPreviewRevision = snapshot.Revision;
        }
        if (_blockPreviewJson.TryGetValue(name, out var cached))
        {
            return cached;
        }
        if (_host == IntPtr.Zero)
        {
            return null;
        }
        if (Native.Execute(_host, "blocks.preview", JsonSerializer.Serialize(new Dictionary<string, object> { ["name"] = name })) != 0)
        {
            return null;
        }
        var json = Native.ReadResult(_host);
        if (!string.IsNullOrEmpty(json))
        {
            _blockPreviewJson[name] = json;
        }
        return json;
    }

    private string? CachedFilePreviewJson(string path)
    {
        if (_filePreviewJson.TryGetValue(path, out var cached))
        {
            return cached;
        }
        if (_host == IntPtr.Zero || string.IsNullOrWhiteSpace(path))
        {
            return null;
        }
        if (Native.Execute(_host, "blocks.preview", JsonSerializer.Serialize(new Dictionary<string, object> { ["path"] = path })) != 0)
        {
            return null;
        }
        var json = Native.ReadResult(_host);
        if (!string.IsNullOrEmpty(json))
        {
            _filePreviewJson[path] = json;
        }
        return json;
    }

    private UIElement BlockPreview(string name, PanelSnapshot snapshot, double size)
        => PreviewCanvas("Block preview " + name, CachedBlockPreviewJson(name, snapshot), size);

    private UIElement FilePreview(string path, string a11yName, double size)
        => PreviewCanvas(a11yName, CachedFilePreviewJson(path), size);

    private UIElement PreviewCanvas(string a11yName, string? json, double size)
    {
        var canvas = new Microsoft.UI.Xaml.Controls.Canvas
        {
            Width = size,
            Height = size,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        AutomationProperties.SetName(canvas, a11yName);
        // DesignCenter paper: light grey field, no heavy frame (geometry carries the edge).
        var paper = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 245, 245, 245));
        var fallbackInk = new SolidColorBrush(Microsoft.UI.Colors.Black);
        var frame = new Microsoft.UI.Xaml.Shapes.Rectangle
        {
            Width = size,
            Height = size,
            Stroke = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 210, 210, 210)),
            StrokeThickness = 0.5,
            Fill = paper,
            RadiusX = 1,
            RadiusY = 1,
        };
        Microsoft.UI.Xaml.Controls.Canvas.SetLeft(frame, 0);
        Microsoft.UI.Xaml.Controls.Canvas.SetTop(frame, 0);
        canvas.Children.Add(frame);
        if (!string.IsNullOrEmpty(json))
        {
            try
            {
                using var doc = JsonDocument.Parse(json);
                if (doc.RootElement.TryGetProperty("paper", out var paperHex) && paperHex.ValueKind == JsonValueKind.String
                    && ParsePreviewHex(paperHex.GetString()) is { } paperColor)
                {
                    frame.Fill = new SolidColorBrush(paperColor);
                }
                DrawBlockPreview(canvas, doc.RootElement, fallbackInk, size);
            }
            catch (JsonException)
            {
            }
        }
        return canvas;
    }

    private static void DrawBlockPreview(Microsoft.UI.Xaml.Controls.Canvas canvas, JsonElement preview, Brush fallbackInk, double size)
    {
        var pad = 4.0;
        var draw = size - pad * 2;
        if (preview.TryGetProperty("strokes", out var strokes) && strokes.ValueKind == JsonValueKind.Array)
        {
            foreach (var stroke in strokes.EnumerateArray())
            {
                if (stroke.ValueKind != JsonValueKind.Object)
                {
                    continue;
                }
                var ink = fallbackInk;
                if (stroke.TryGetProperty("color", out var colorEl) && colorEl.ValueKind == JsonValueKind.String
                    && ParsePreviewHex(colorEl.GetString()) is { } rgb)
                {
                    ink = new SolidColorBrush(rgb);
                }
                if (!stroke.TryGetProperty("points", out var points) || points.ValueKind != JsonValueKind.Array)
                {
                    continue;
                }
                AddPreviewStroke(canvas, points, ink, pad, draw);
            }
            return;
        }
        if (!preview.TryGetProperty("segments", out var segments) || segments.ValueKind != JsonValueKind.Array)
        {
            return;
        }
        foreach (var seg in segments.EnumerateArray())
        {
            AddPreviewStroke(canvas, seg, fallbackInk, pad, draw);
        }
    }

    private static void AddPreviewStroke(Microsoft.UI.Xaml.Controls.Canvas canvas, JsonElement points, Brush ink, double pad, double draw)
    {
        if (points.ValueKind != JsonValueKind.Array || points.GetArrayLength() == 0)
        {
            return;
        }
        if (points.GetArrayLength() == 1
            || (points.GetArrayLength() == 2
                && NearlySamePreviewPoint(points[0], points[1])))
        {
            var p = BlockPreviewPoint(points[0], pad, draw);
            var dot = new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 2.0, Height = 2.0, Fill = ink };
            Microsoft.UI.Xaml.Controls.Canvas.SetLeft(dot, p.X - 1.0);
            Microsoft.UI.Xaml.Controls.Canvas.SetTop(dot, p.Y - 1.0);
            canvas.Children.Add(dot);
            return;
        }
        var geo = new Microsoft.UI.Xaml.Media.PathGeometry();
        var fig = new Microsoft.UI.Xaml.Media.PathFigure { IsClosed = false, IsFilled = false };
        var first = true;
        foreach (var pt in points.EnumerateArray())
        {
            var p = BlockPreviewPoint(pt, pad, draw);
            if (first)
            {
                fig.StartPoint = p;
                first = false;
            }
            else
            {
                fig.Segments.Add(new Microsoft.UI.Xaml.Media.LineSegment { Point = p });
            }
        }
        geo.Figures.Add(fig);
        canvas.Children.Add(new Microsoft.UI.Xaml.Shapes.Path
        {
            Data = geo,
            Stroke = ink,
            StrokeThickness = 1.0,
            StrokeStartLineCap = Microsoft.UI.Xaml.Media.PenLineCap.Flat,
            StrokeEndLineCap = Microsoft.UI.Xaml.Media.PenLineCap.Flat,
            StrokeLineJoin = Microsoft.UI.Xaml.Media.PenLineJoin.Miter,
        });
    }

    private static bool NearlySamePreviewPoint(JsonElement a, JsonElement b)
    {
        if (a.ValueKind != JsonValueKind.Array || b.ValueKind != JsonValueKind.Array
            || a.GetArrayLength() < 2 || b.GetArrayLength() < 2)
        {
            return false;
        }
        var ax = a[0].TryGetDouble(out var axv) ? axv : 0;
        var ay = a[1].TryGetDouble(out var ayv) ? ayv : 0;
        var bx = b[0].TryGetDouble(out var bxv) ? bxv : 0;
        var by = b[1].TryGetDouble(out var byv) ? byv : 0;
        return Math.Abs(ax - bx) < 1e-9 && Math.Abs(ay - by) < 1e-9;
    }

    private static Windows.UI.Color? ParsePreviewHex(string? hex)
    {
        if (string.IsNullOrWhiteSpace(hex))
        {
            return null;
        }
        var s = hex.Trim().TrimStart('#');
        if (s.Length != 6)
        {
            return null;
        }
        try
        {
            var r = Convert.ToByte(s[..2], 16);
            var g = Convert.ToByte(s[2..4], 16);
            var b = Convert.ToByte(s[4..6], 16);
            return Windows.UI.Color.FromArgb(255, r, g, b);
        }
        catch (FormatException)
        {
            return null;
        }
        catch (OverflowException)
        {
            return null;
        }
    }

    private static Windows.Foundation.Point BlockPreviewPoint(JsonElement pt, double pad, double draw)
    {
        var x = 0.0;
        var y = 0.0;
        if (pt.ValueKind == JsonValueKind.Array && pt.GetArrayLength() >= 2)
        {
            x = pt[0].TryGetDouble(out var xv) ? xv : 0;
            y = pt[1].TryGetDouble(out var yv) ? yv : 0;
        }
        // Engine preview is Y-up; WinUI Canvas is Y-down.
        return new Windows.Foundation.Point(pad + x * draw, pad + (1.0 - y) * draw);
    }

    private void InsertBlock(string name)
    {
        if (_host == IntPtr.Zero || Native.CommandLine(_host, "insert.place") != 0)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        Native.CommandLine(_host, name);
        _panelKey = "";
        RefreshChrome();
        RefreshPanels();
    }

    /// <summary>Standard furniture set from <c>furniture/standard</c> (CC0 plan DXFs).</summary>
    private void AddFurnitureCatalog()
    {
        var entries = FurnitureCatalog.Load();
        if (entries.Count == 0)
            return;
        BlocksPanel.Children.Add(Heading(FurnitureCatalog.Name));
        BlocksPanel.Children.Add(FieldLabel("Plan blocks · millimetres · CC0 (Innerscene)"));
        BlocksPanel.Children.Add(FieldLabel("Click a thumbnail to insert."));
        AddCatalogGrid(BlocksPanel, entries);
        BlocksPanel.Children.Add(new Border { Height = 1, Margin = new Thickness(0, 8, 0, 8), Background = StripBrush() });
    }

    private Border MakeFurnitureThumbnail(FurnitureCatalog.Entry entry)
    {
        var path = entry.Path;
        var blockName = System.IO.Path.GetFileNameWithoutExtension(entry.Item.File);
        var caption = string.IsNullOrEmpty(entry.Item.SizeMm)
            ? entry.Item.Title
            : entry.Item.Title + " · " + entry.Item.SizeMm + " mm";
        var tip = string.IsNullOrEmpty(entry.Item.Note) ? caption : caption + " ? " + entry.Item.Note;
        var card = DesignCenterCard(false, "Insert furniture " + entry.Item.Title, tip);
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(FilePreview(path, "Furniture preview " + entry.Item.Title, 96));
        stack.Children.Add(new TextBlock
        {
            Text = entry.Item.Title,
            FontSize = Density.FontCaption,
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxLines = 1,
            HorizontalAlignment = HorizontalAlignment.Center,
            TextAlignment = TextAlignment.Center,
        });
        card.Child = stack;
        card.PointerPressed += (_, e) =>
        {
            if (e.GetCurrentPoint(card).Properties.IsLeftButtonPressed)
            {
                InsertFurnitureFile(blockName, path);
            }
        };
        return card;
    }

    private void InsertFurnitureFile(string blockName, string path)
    {
        if (_host == IntPtr.Zero || Native.CommandLine(_host, "insert.place") != 0)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        Native.CommandLine(_host, blockName + "=" + path);
        _panelKey = "";
        RefreshChrome();
        RefreshPanels();
    }

    private void SelectBlockReferences(BlockRow block)
    {
        if (_host == IntPtr.Zero || block.Handles.Count == 0)
        {
            return;
        }
        var json = JsonSerializer.Serialize(new Dictionary<string, object> { ["handles"] = block.Handles });
        if (Native.Execute(_host, "select", json) != 0)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        _panelKey = "";
        RefreshChrome();
        RefreshPanels();
    }

    private void RunLayer(string command, Dictionary<string, object> parameters)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var json = JsonSerializer.Serialize(parameters);
        if (Native.Execute(_host, command, json) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        _panelKey = "";
        RefreshChrome();
        RefreshPanels();
    }

    private static TextBlock Heading(string text)
    {
        var block = Density.SectionHeading(text);
        AutomationProperties.SetHeadingLevel(block, AutomationHeadingLevel.Level2);
        return block;
    }

    private static TextBlock FieldLabel(string text)
    {
        Brush? brush = null;
        if (Density.ThemeBrush("TextFillColorSecondaryBrush") is { } secondary)
        {
            brush = secondary;
        }
        var block = new TextBlock
        {
            Text = text,
            FontSize = Density.FontCaption,
            Margin = new Thickness(0, Density.SpaceSm, 0, 0),
        };
        if (brush != null)
        {
            block.Foreground = brush;
        }
        return block;
    }

    private void PlaceChild()
    {
        if (_host == IntPtr.Zero || CanvasHost.XamlRoot == null || Content is not FrameworkElement root)
        {
            return;
        }
        // The canvas is a native child window drawn over the XAML, so the welcome screen needs it hidden.
        Native.SetCanvas(_host, WelcomeOverlay.Visibility != Visibility.Visible);
        var scale = CanvasHost.XamlRoot.RasterizationScale;
        if (scale <= 0 || CanvasHost.ActualWidth < 1 || CanvasHost.ActualHeight < 1)
        {
            return;
        }
        var point = CanvasHost.TransformToVisual(root).TransformPoint(new Windows.Foundation.Point(0, 0));
        int x = (int)Math.Round(point.X * scale);
        int y = (int)Math.Round(point.Y * scale);
        int w = Math.Max(1, (int)Math.Round(CanvasHost.ActualWidth * scale));
        int h = Math.Max(1, (int)Math.Round(CanvasHost.ActualHeight * scale));
        var parent = WindowNative.GetWindowHandle(this);
        var rc = Native.IsEmbedded(_host)
            ? Native.Move(_host, x, y, (uint)w, (uint)h)
            : Native.Embed(_host, parent, x, y, (uint)w, (uint)h);
        if (rc != 0)
        {
            History.Text = Native.LastError(_host);
        }
        else
        {
            // GPU surface exists only after Embed — re-sync paper colour with the shell theme.
            Native.Theme(_host, _dark);
        }
        PlaceCommandBar();
    }

    private void FillContext(PanelSnapshot snapshot)
    {
        _filling = true;
        try
        {
            var currentLayer = snapshot.Layers.FirstOrDefault(layer => layer.Current)?.Name;
            FillCombo(LayerBox, snapshot.Layers.Select(layer => (layer.Name, layer.Name)), currentLayer);
            SyncPaperSize();
        }
        finally
        {
            _filling = false;
        }
    }

    private static void FillCombo(ComboBox box, IEnumerable<(string Tag, string Label)> items, string? selected)
    {
        var list = items.ToList();
        var sameCount = box.Items.Count == list.Count;
        var sameItems = sameCount;
        if (sameItems)
        {
            for (var i = 0; i < list.Count; i++)
            {
                if (box.Items[i] is not ComboBoxItem existing || existing.Tag as string != list[i].Tag)
                {
                    sameItems = false;
                    break;
                }
            }
        }
        if (!sameItems)
        {
            box.Items.Clear();
            foreach (var (tag, label) in list)
            {
                box.Items.Add(new ComboBoxItem { Content = label, Tag = tag });
            }
        }
        box.IsEnabled = box.Items.Count > 0;
        var want = -1;
        for (var i = 0; i < box.Items.Count; i++)
        {
            if (box.Items[i] is ComboBoxItem item && item.Tag is string tag && tag.Equals(selected, StringComparison.OrdinalIgnoreCase))
            {
                want = i;
                break;
            }
        }
        // Avoid SelectionChanged ? layer.current / floor.current when chrome already matches.
        if (box.SelectedIndex != want)
        {
            box.SelectedIndex = want;
        }
    }

    private void OnLayerPicked(object sender, SelectionChangedEventArgs e) => PickContext(LayerBox, "layer.current", "name");

    private void PickContext(ComboBox box, string command, string key)
    {
        if (_filling || box.SelectedItem is not ComboBoxItem item || item.Tag is not string value || _host == IntPtr.Zero)
        {
            return;
        }
        var json = JsonSerializer.Serialize(new Dictionary<string, object> { [key] = value });
        if (Native.Execute(_host, command, json) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        _panelKey = "";
        AfterCommand();
    }

    private void ChoosePopup(string id, string run)
    {
        if (run == "ucs")
        {
            _ucs = id != "hide";
            Native.SetUcs(_host, _ucs);
            return;
        }
        if (run is "levelmove" or "levelcopy")
        {
            // Start the form with the target level so the canvas asks for origin / matching points.
            if (long.TryParse(id, out var level))
            {
                StartLevelSend(run == "levelcopy" ? "level.copyto" : "level.moveto", level);
            }
            return;
        }
        if (run == "type")
        {
            if (Native.CommandLine(_host, id) != 0)
            {
                History.Text = Native.LastError(_host);
            }
            RefreshChrome();
            return;
        }
        StartCommand(id);
    }

    private void EnsureTimer()
    {
        if (_timer != null)
        {
            return;
        }
        var queue = DispatcherQueue.GetForCurrentThread();
        _timer = queue.CreateTimer();
        _timer.Interval = TimeSpan.FromMilliseconds(32);
        _timer.IsRepeating = true;
        _timer.Tick += (_, _) =>
        {
            if (_host == IntPtr.Zero)
            {
                return;
            }
            var perf = PerfLog.Enabled ? System.Diagnostics.Stopwatch.StartNew() : null;
            CanvasMenu.DismissOutsideClick();
            PollEscapeCancel();
            if (Native.Pump(_host) != 0)
            {
                Status.Text = Native.LastError(_host);
            }
            perf?.Mark("pump");
            var popup = Native.ReadPopup(_host);
            if (popup.Length > 0)
            {
                try
                {
                    CanvasMenu.Show(this, popup, ChoosePopup);
                }
                catch (JsonException)
                {
                    Status.Text = "The shortcut menu could not be read.";
                }
            }
            DrainPlotRequest();
            TickCollapse();
            _ticks++;
            SyncHistory();
            UpdateDyn();
            perf?.Mark("history+dyn");
            if (_ticks % 4 == 0)
            {
                RefreshChrome();
                perf?.Mark("chrome");
            }
            // Every ~4 ticks so hover rollover updates Properties / Layer / Blocks promptly.
            if (_ticks % 4 == 0)
            {
                RefreshPanels();
                perf?.Mark("panels");
            }
            perf?.Report();
        };
        _timer.Start();
    }

    private sealed class MenuNode
    {
        public string Title { get; set; } = "";
        public List<MenuNode> Items { get; set; } = new();
        public string? Id { get; set; }
        public string? Shortcut { get; set; }
        public bool Enabled { get; set; } = true;
    }

    private void BuildTools()
    {
        ToolStrip.Children.Clear();
        // Draw · Modify · Annotate on the top tool strip (horizontal groups).
        string[][] groups =
        [
            ["line", "pline", "circle", "arc", "rectang"],
            ["move", "copy", "rotate", "offset", "trim", "erase"],
            ["dimlinear", "text", "hatch"],
        ];
        var first = true;
        foreach (var group in groups)
        {
            var visible = group.Where(id => _toolIds.Contains(id)).ToList();
            if (visible.Count == 0)
            {
                continue;
            }
            if (!first)
            {
                ToolStrip.Children.Add(MakeToolColumnDivider());
            }
            first = false;
            foreach (var id in visible)
            {
                var label = ToolCatalog.First(t => t.Id == id).Label;
                var command = id;
                var button = MakeToolTile(ToolIcons.For(id, _dark), label);
                button.Click += (_, _) => StartCommand(command);
                ToolStrip.Children.Add(button);
            }
        }
        ToolStrip.Children.Add(MakeToolColumnDivider());
        var gear = MakeToolTile(ToolIcons.For("gear", _dark), "Customize tool bar");
        gear.Click += (_, _) => OpenTools();
        ToolStrip.Children.Add(gear);
        RebuildArchitectureBar();
        RebuildTakeoffBar();
    }

    private static Button MakeToolTile(UIElement icon, string name)
    {
        var button = new Button
        {
            Content = icon,
            Width = Density.IconButton,
            Height = Density.IconButton,
            MinWidth = Density.IconButton,
            MinHeight = Density.IconButton,
            Padding = new Thickness(0),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            CornerRadius = new CornerRadius(Density.CornerControl),
            HorizontalContentAlignment = HorizontalAlignment.Center,
            VerticalContentAlignment = VerticalAlignment.Center,
        };
        if (Application.Current.Resources.TryGetValue("IconButtonStyle", out var style) && style is Style s)
        {
            button.Style = s;
        }
        AutomationProperties.SetName(button, name);
        ToolTipService.SetToolTip(button, name);
        return button;
    }

    /// <summary>A thin horizontal rule between groups of the vertical tool bar.</summary>
    private static Border MakeToolRule() => new()
    {
        Height = 1,
        Margin = new Thickness(4, 2, 4, 2),
        Background = Density.ThemeBrush("BorderSubtleBrush") ?? new SolidColorBrush(Windows.UI.Color.FromArgb(120, 128, 128, 128)),
        Opacity = 0.85,
    };

    private static Border MakeToolDivider(bool horizontalBar = false)
    {
        var key = horizontalBar ? "ArchBarDividerStyle" : "ToolStripDividerStyle";
        if (Application.Current.Resources.TryGetValue(key, out var style) && style is Style s)
        {
            return new Border { Style = s };
        }
        return new Border
        {
            Width = 1,
            Height = Density.IconSize,
            Margin = new Thickness(horizontalBar ? 1 : 4, 0, horizontalBar ? 1 : 4, 0),
            VerticalAlignment = VerticalAlignment.Center,
            Background = Density.ThemeBrush("BorderSubtleBrush") ?? new SolidColorBrush(Windows.UI.Color.FromArgb(120, 128, 128, 128)),
            Opacity = 0.85,
        };
    }

    /// <summary>A thin line across the tool column (both of its columns), between the groups of tools.</summary>
    private static Border MakeToolColumnDivider()
    {
        var line = new Border
        {
            Height = 1,
            Margin = new Thickness(2, 4, 2, 4),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            Background = Density.ThemeBrush("BorderSubtleBrush") ?? new SolidColorBrush(Windows.UI.Color.FromArgb(120, 128, 128, 128)),
            Opacity = 0.85,
            Tag = "divider",
        };
        return line;
    }

    /// <summary>Drawing tools on the Architecture bar. Layer set lives under Format (not here).</summary>
    private static readonly (string Id, string Label)[] ArchitectureTools =
    [
        ("wall.door", "Door"),
        ("wall.window", "Window"),
        ("room", "Room"),
        ("rarea", "Room area"),
        ("grid.axis", "Axis grid"),
        ("column.place", "Column"),
        ("stair.plan", "Stair"),
        ("north.arrow", "North"),
        ("rcc.column", "RCC column"),
        ("rcc.column.round", "Round column"),
        ("rcc.beam", "RCC beam"),
        ("rcc.footing", "Footing"),
        ("rcc.slab", "Slab"),
        ("rcc.lintel", "Lintel"),
    ];

    /// <summary>The structural tools and the parametric block each places (IS 456 checked in Properties).</summary>
    private static readonly Dictionary<string, string> RccTools = new()
    {
        ["rcc.column"] = "rcc.column.rect",
        ["rcc.column.round"] = "rcc.column.circular",
        ["rcc.beam"] = "rcc.beam.section",
        ["rcc.footing"] = "rcc.footing.section",
        ["rcc.slab"] = "rcc.slab.section",
        ["rcc.lintel"] = "rcc.lintel.section",
    };

    private void RebuildArchitectureBar()
    {
        ArchBar.Children.Clear();
        // Openings · rooms · structure ? thin dividers between sections. Wall draw is command-line only.
        string[][] groups =
        [
            ["wall.door", "wall.window"],
            ["room", "rarea", "north.arrow"],
            ["grid.axis", "column.place", "stair.plan"],
            ["rcc.column", "rcc.column.round", "rcc.beam", "rcc.footing", "rcc.slab", "rcc.lintel"],
        ];
        var first = true;
        foreach (var group in groups)
        {
            if (!first)
            {
                ArchBar.Children.Add(MakeToolColumnDivider());
            }
            first = false;
            foreach (var id in group)
            {
                var label = ArchitectureTools.First(t => t.Id == id).Label;
                var command = id;
                var button = MakeToolTile(ToolIcons.Architecture(command, _dark), label);
                button.Click += (_, _) => StartCommand(command);
                ArchBar.Children.Add(button);
            }
        }
    }

    private void StartTakeoffDraw(string kind)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var preset = "{\"kind\":\"" + kind + "\"}";
        if (Native.StartForm(_host, "takeoff.draw", preset) != 0)
        {
            History.Text = "Take-off draw did not start.";
        }
    }

    /// <summary>Run a take-off engine command with JSON parameters and show counts in the status strip.</summary>
    private void RunTakeoffCommand(string id, string json = "{}", bool remapForce = false)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (remapForce)
        {
            json = "{\"force\":true}";
        }
        if (Native.Execute(_host, id, json) != 0)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        History.Text = SummarizeTakeoffResult(id, Native.ReadResult(_host));
        AfterCommand();
    }

    private static string SummarizeTakeoffResult(string id, string json)
    {
        if (string.IsNullOrWhiteSpace(json))
        {
            return id + " finished.";
        }
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            if (id == "takeoff.maplayers")
            {
                var tagged = root.TryGetProperty("tagged", out var t) ? t.GetInt32() : 0;
                var skipped = root.TryGetProperty("skipped", out var s) ? s.GetInt32() : 0;
                if (tagged == 0 && skipped > 0)
                {
                    return $"Map layers: 0 new ( {skipped} already had take-off metadata). Hold Shift and click Map layers to re-stamp, or draw with Ext/Int wall tools.";
                }
                return $"Map layers: {tagged} stamped, {skipped} skipped.";
            }
            if (root.TryGetProperty("written", out var w))
            {
                return $"Take-off recorded ({w.GetInt32()} records).";
            }
            if (root.TryGetProperty("totals", out _))
            {
                return id + " finished ? see command log for totals.";
            }
        }
        catch (JsonException)
        {
        }
        return id + " finished.";
    }

    private void SyncTakeoffToProject()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        RunTakeoffCommand("takeoff.record", "{\"annotate\":false}");
    }

    private void RebuildTakeoffBar()
    {
        TakeoffBar.Children.Clear();
        // Draw · compute · sync ? same section dividers as the architecture strip.
        (string icon, string label, string? kind, string? command)[][] groups =
        [
            [
                ("takeoff.ext", "Ext wall", "extWall", null),
                ("takeoff.int", "Int wall", "intWall", null),
                ("takeoff.beam", "Beam", "beam", null),
                ("takeoff.floor", "Floor", "floorArea", null),
                ("takeoff.ceiling", "Ceiling", "ceilingArea", null),
                ("takeoff.column", "Column", "column", null),
                ("takeoff.deduct", "Deduct", "deduct", null),
            ],
            [
                ("takeoff.paint", "Paint", null, "takeoff.paint"),
                ("openings.sync", "Openings", null, "openings.sync"),
                ("openings.deduct", "Fix deducts", null, "openings.deduct"),
                ("column.deduct", "Col deduct", null, "column.deduct"),
            ],
            [
                ("takeoff.sync", "Sync take-off", null, "__takeoff_sync__"),
                ("takeoff.record", "Record", null, "takeoff.record"),
                ("takeoff.maplayers", "Map layers", null, "takeoff.maplayers"),
            ],
        ];
        var first = true;
        foreach (var group in groups)
        {
            if (!first)
            {
                TakeoffBar.Children.Add(MakeToolDivider(horizontalBar: true));
            }
            first = false;
            foreach (var (icon, label, kind, command) in group)
            {
                var button = MakeTextTool(label);
                var k = kind;
                var cmd = command;
                button.Click += (_, e) =>
                {
                    if (k != null)
                    {
                        StartTakeoffDraw(k);
                    }
                    else if (cmd != null)
                    {
                        if (cmd == "__takeoff_sync__")
                        {
                            SyncTakeoffToProject();
                            return;
                        }
                        var forceRemap = cmd == "takeoff.maplayers" && (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(Windows.System.VirtualKey.Shift)
                            .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down));
                        RunTakeoffCommand(cmd, remapForce: forceRemap);
                    }
                };
                TakeoffBar.Children.Add(button);
            }
        }
        AddPhasePickers();
    }

    /// <summary>Bottom status strip: open drawings (model space only) + Nameplate button.</summary>
    private void ShowDrawingTabs(ChromeState state)
    {
        _documents.Clear();
        _documents.AddRange(state.Documents);
        var active = _documents.FirstOrDefault(doc => doc.Active);
        Title = active == null || active.Title.Length == 0 ? "O-Draft" : "O-Draft \u2014 " + active.Title;
        PublishActiveProject(ActiveDrawingPath(active));
        var key = string.Join("\n", _documents.Select(doc => doc.Index + ":" + doc.Title + ":" + doc.Dirty + ":" + doc.Active));
        if (key == _docKey)
        {
            return;
        }
        _docKey = key;
        _buildingTabs = true;
        DrawingTabs.Children.Clear();
        foreach (var doc in _documents.ToList())
        {
            var drawing = doc;
            var title = drawing.Dirty ? drawing.Title + " *" : drawing.Title;
            var current = drawing.Active;
            var tab = new Button
            {
                Content = new StackPanel
                {
                    Orientation = Orientation.Horizontal,
                    Spacing = Density.SpaceXs,
                    Children =
                    {
                        new DrawnIcon { Kind = "model" },
                        new TextBlock
                        {
                            Text = title,
                            MaxWidth = 140,
                            FontSize = Density.FontCaption,
                            TextTrimming = TextTrimming.CharacterEllipsis,
                            VerticalAlignment = VerticalAlignment.Center,
                            FontWeight = current ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
                        },
                    },
                },
                MinHeight = Density.ControlHeight,
                Padding = new Thickness(6, 2, 2, 2),
                Background = current ? (Density.ThemeBrush("SurfaceBrush") ?? new SolidColorBrush(Microsoft.UI.Colors.Transparent)) : new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                BorderThickness = new Thickness(0),
                CornerRadius = new CornerRadius(Density.Space1, 0, 0, 0),
            };
            AutomationProperties.SetName(tab, current ? "Current drawing " + drawing.Title : "Switch to " + drawing.Title);
            tab.Click += (_, _) => SwitchDrawing(drawing.Index);
            var close = new Button
            {
                Content = "×",
                Width = 22,
                Height = Density.ControlHeight,
                MinWidth = 22,
                MinHeight = Density.ControlHeight,
                Padding = new Thickness(0),
                FontSize = Density.FontBody,
                Background = current ? (Density.ThemeBrush("SurfaceBrush") ?? new SolidColorBrush(Microsoft.UI.Colors.Transparent)) : new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                CornerRadius = new CornerRadius(0, Density.Space1, 0, 0),
                BorderThickness = new Thickness(0),
                Visibility = current ? Visibility.Visible : Visibility.Collapsed,
            };
            AutomationProperties.SetName(close, "Close " + drawing.Title);
            ToolTipService.SetToolTip(close, "Close " + drawing.Title);
            close.Click += (_, _) => _ = CloseDocument(drawing.Index);
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 0, Children = { tab, close } };
            DrawingTabs.Children.Add(row);
        }
        _buildingTabs = false;
    }

    private void SwitchDrawing(int index)
    {
        if (_buildingTabs || _host == IntPtr.Zero)
        {
            return;
        }
        var doc = _documents.FirstOrDefault(item => item.Index == index);
        if (doc is { Active: true })
        {
            EnsureModelSpace();
            AfterCommand();
            return;
        }
        if (Native.Execute(_host, "document.switch", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = index })) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        EnsureModelSpace();
        _docKey = "";
        _panelKey = "";
        AfterCommand();
    }

    private async Task CloseDocument(int index)
    {
        if (_asking || _host == IntPtr.Zero)
        {
            return;
        }
        _asking = true;
        try
        {
            _docKey = "";
            RefreshChrome();
            var doc = _documents.FirstOrDefault(item => item.Index == index);
            if (doc == null)
            {
                return;
            }
            if (!doc.Active)
            {
                Native.Execute(_host, "document.switch", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = index }));
                _docKey = "";
                RefreshChrome();
            }
            if (doc.Dirty && !await DiscardOrSave(doc.Title))
            {
                return;
            }
            if (Native.Execute(_host, "close", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = index })) != 0)
            {
                History.Text = Native.LastError(_host);
                return;
            }
            _docKey = "";
            RefreshChrome();
            if (_documents.Count == 0)
            {
                Native.Execute(_host, "new", "{}");
            }
            _panelKey = "";
            AfterCommand();
        }
        finally
        {
            _asking = false;
        }
    }

    private void LoadShell()
    {
        var prefs = ShellPrefs.Load();
        if (prefs.Tools.Count > 0)
        {
            _toolIds = prefs.Tools.Where(id => ToolCatalog.Any(tool => tool.Id == id)).ToList();
        }
        _font = string.IsNullOrEmpty(prefs.Font) ? "engineering" : prefs.Font;
        _showMenu = prefs.Menu;
        _showTabs = prefs.Tabs;
        _showStatus = prefs.Status;
        _showCommand = prefs.Command;
        _pinRight = prefs.PinRight;
        _modeSlots.Clear();
        foreach (var (key, slot) in prefs.ModeSlots)
        {
            if (slot != null && (key == "classic" || key == "pro"))
            {
                _modeSlots[key] = slot;
            }
        }
        if (!_modeSlots.ContainsKey("classic"))
        {
            _modeSlots["classic"] = new ShellPrefs.ModeSlot
            {
                PinLeft = prefs.PinLeft,
                ToolsVisible = prefs.ToolsVisible,
                LeftWidth = 280,
                LeftCollapsed = false,
            };
        }
        if (!_modeSlots.ContainsKey("pro"))
        {
            _modeSlots["pro"] = DefaultSlot(classic: false);
        }
        _classic = !string.Equals(prefs.Workspace, "pro", StringComparison.OrdinalIgnoreCase);
        ApplySlot(_modeSlots.TryGetValue(WorkspaceKey(), out var current) ? current : DefaultSlot(_classic));
        ToolTipService.SetToolTip(WorkspaceButton, _classic ? "Classic" : "Pro");
        AutomationProperties.SetName(WorkspaceButton, _classic ? "Classic workspace" : "Pro workspace");
        _leftIdleSince = DateTime.UtcNow;
        // Compact Layer side tab only; full manager opens as a window (never merge filters into the dock).
        _mergeLayers = false;
        _ucs = prefs.Ucs;
        _barDx = prefs.BarDx;
        _barDy = prefs.BarDy;
        Density.Use(_font);
        Native.SetUcs(_host, _ucs);
        ApplyChrome();
        ApplyLayerHost(showWindow: false);
    }

    private void SaveShell()
    {
        _modeSlots[WorkspaceKey()] = CaptureSlot();
        ShellPrefs.Save(new ShellPrefs.Prefs
        {
            Tools = _toolIds,
            Font = _font,
            Menu = _showMenu,
            ToolsVisible = _showTools,
            Tabs = _showTabs,
            Status = _showStatus,
            Command = _showCommand,
            PinLeft = _pinLeft,
            PinRight = _pinRight,
            MergeLayers = _mergeLayers,
            Ucs = _ucs,
            BarDx = _barDx,
            BarDy = _barDy,
            Workspace = WorkspaceKey(),
            ModeSlots = new Dictionary<string, ShellPrefs.ModeSlot>(_modeSlots),
        });
    }

    private void Listen()
    {
        ushort port = _launch.ControlPort ?? 0;
        if (port == 0
            && ushort.TryParse(Environment.GetEnvironmentVariable("AAD_CONTROL_PORT"), out var parsed))
        {
            port = parsed;
        }
        if (Native.Listen(_host, port) < 0)
        {
            Status.Text = Native.LastError(_host);
        }
    }

    private void ApplyChrome()
    {
        Bar.Visibility = _showMenu ? Visibility.Visible : Visibility.Collapsed;
        StatusRow.Visibility = _showStatus ? Visibility.Visible : Visibility.Collapsed;
        PlaceCommandBar();
        ApplyColumns();
    }

    private void ApplyColumns()
    {
        // Pro hides the tool strip; Classic honours the per-mode toolbar switch.
        var tools = _classic && _showTools;
        var takeoff = tools && _takeoffMode;
        TakeoffModeButton.IsChecked = _takeoffMode;
        ToolStripHost.Visibility = tools ? Visibility.Visible : Visibility.Collapsed;
        SheetRailHost.Visibility = tools ? Visibility.Visible : Visibility.Collapsed;
        if (tools && ArchBar.Children.Count == 0)
        {
            RebuildArchitectureBar();
        }
        if (takeoff && TakeoffBar.Children.Count == 0)
        {
            RebuildTakeoffBar();
        }
        if (ArchToolsDivider != null)
        {
            ArchToolsDivider.Visibility = tools && ArchBar.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        }
        ArchBar.Visibility = tools ? Visibility.Visible : Visibility.Collapsed;
        if (TakeoffToolStripHost != null)
        {
            TakeoffToolStripHost.Visibility = takeoff && TakeoffBar.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        }
        TakeoffBar.Visibility = takeoff ? Visibility.Visible : Visibility.Collapsed;
        // A panel is open until its title is clicked again (or, unpinned, until the pointer has been
        // away for 1.5 s). The rail of titles is always there, so a collapsed dock is just the rail.
        var leftOpen = _leftFloat != null || !_leftCollapsed;
        var dockWidth = _leftFloat != null ? Math.Min(_leftWidth, 200) : _leftWidth;
        PropertiesColumn.Width = new GridLength(leftOpen ? dockWidth + Density.SideRailWidth : Density.SideRailWidth);
        SideRail.Visibility = Visibility.Visible;
        LeftColumn.Visibility = leftOpen ? Visibility.Visible : Visibility.Collapsed;
        RefreshSideRail();
        LeftSplitterColumn.Width = leftOpen && _leftFloat == null ? new GridLength(8) : new GridLength(0);
        LeftSplitter.Visibility = leftOpen && _leftFloat == null ? Visibility.Visible : Visibility.Collapsed;
        SideTabs.Visibility = leftOpen && _leftFloat == null ? Visibility.Visible : Visibility.Collapsed;
        PinLeftButton.Visibility = leftOpen ? Visibility.Visible : Visibility.Collapsed;
        FloatLeftButton.Visibility = leftOpen ? Visibility.Visible : Visibility.Collapsed;
        LeftColumn.Background = leftOpen ? null : StripBrush();
        LayersHost.Visibility = Visibility.Visible;
    }

    /// Unpinned panels stay open while the pointer is over them or a field in them has focus,
    /// then collapse to the edge strip 1.5 s after the pointer leaves.
    private void TickCollapse()
    {
        if (_leftFloat != null)
        {
            return;
        }
        var now = DateTime.UtcNow;
        if (ColumnBusy(LeftColumn) || _dragSplit != null)
        {
            _leftIdleSince = now;
        }
        var changed = false;
        if (!_pinLeft && !_hoverLeft && (now - _leftIdleSince).TotalMilliseconds >= 1500 && !_leftCollapsed)
        {
            _leftCollapsed = true;
            changed = true;
        }
        if (changed)
        {
            ApplyColumns();
        }
    }

    private static Brush StripBrush()
    {
        if (Density.ThemeBrush("SubtleFillColorSecondaryBrush") is { } brush)
        {
            return brush;
        }
        return new SolidColorBrush(Windows.UI.Color.FromArgb(255, 96, 96, 96));
    }

    private static bool ColumnBusy(DependencyObject column)
    {
        if (column is not FrameworkElement element || element.XamlRoot == null)
        {
            return false;
        }
        return FocusManager.GetFocusedElement(element.XamlRoot) is DependencyObject focused && IsDescendant(column, focused);
    }

    private static bool IsDescendant(DependencyObject root, DependencyObject node)
    {
        var current = node;
        while (current != null)
        {
            if (current == root)
            {
                return true;
            }
            current = VisualTreeHelper.GetParent(current);
        }
        return false;
    }

    private void EnsureShellMenus()
    {
        MenuBarItem? window = null;
        foreach (var item in Bar.Items)
        {
            if (item is MenuBarItem top && top.Title?.ToString() == "Window")
            {
                window = top;
            }
        }
        if (window == null)
        {
            window = new MenuBarItem { Title = "Window" };
            Bar.Items.Add(window);
        }
        ClearShellItems(window);
        CheckItem(window, "Menu bar", () => _showMenu, value => _showMenu = value);
        CheckItem(window, "Tool bar", () => _showTools, value => _showTools = value);
        CheckItem(window, "Status bar", () => _showStatus, value => _showStatus = value);
        CheckItem(window, "Command line", () => _showCommand, value => _showCommand = value);
        CheckItem(window, "Pin side panels", () => _pinLeft, value =>
        {
            _pinLeft = value;
            _leftCollapsed = false;
            _leftIdleSince = DateTime.UtcNow;
            UpdatePinButton();
        });
        CommandItem(window, "Properties", () => ShowSidePanel(SideTab.Properties));
        CommandItem(window, "Layers palette", () => ShowSidePanel(SideTab.Layers));
        CommandItem(window, "Blocks", () => ShowSidePanel(SideTab.Blocks));
        CommandItem(window, "Project settings", () => ShowSidePanel(SideTab.Project));
        CommandItem(window, "History", () => ShowSidePanel(SideTab.History));
        CommandItem(window, "Revisions", () => ShowSidePanel(SideTab.Revisions));
        CommandItem(window, "Parameters Manager", OpenParameters);
        CommandItem(window, "Graph", OpenGraph);
        CommandItem(window, "Revision register", OpenRevisions);
        CommandItem(window, "Version history", OpenHistory);
        CommandItem(window, "Classic workspace", () => SwitchWorkspace(classic: true));
        CommandItem(window, "Pro workspace", () => SwitchWorkspace(classic: false));
        CommandItem(window, "Reset this workspace", ResetCurrentWorkspace);
        CommandItem(window, "Calculator", OpenCalculator);
        CommandItem(window, "Pomodoro", OpenPomodoro);
        CommandItem(window, "Customize tool bar", OpenTools);
        CommandItem(window, "Nameplate L (landscape)", () => OpenNameplate(landscape: true));
        CommandItem(window, "Nameplate P (portrait)", () => OpenNameplate(landscape: false));
        CommandItem(window, "UCS icon", () =>
        {
            _ucs = !_ucs;
            Native.SetUcs(_host, _ucs);
            SaveShell();
        });
    }

    private enum SideTab
    {
        Properties,
        Layers,
        Blocks,
        Project,
        History,
        Revisions,
    }

    /// Expands the side column (and floats it back if needed), then selects a panel tab.
    private void ShowSidePanel(SideTab tab)
    {
        if (_leftFloat != null)
        {
            _leftFloat.Activate();
        }
        else
        {
            _leftCollapsed = false;
            _leftIdleSince = DateTime.UtcNow;
            ApplyColumns();
            PlaceChild();
        }
        var item = tab switch
        {
            SideTab.Layers => LayersTab,
            SideTab.Blocks => BlocksTab,
            SideTab.Project => ProjectTab,
            SideTab.History => HistoryTab,
            SideTab.Revisions => RevisionsTab,
            _ => PropertiesTab,
        };
        // Only the chosen panel is shown: there are no tabs inside the panel.
        _sideTab = tab;
        _panelKey = "";                                // rebuild the panel now shown at the next tick
        PropertiesTab.Visibility = item == PropertiesTab ? Visibility.Visible : Visibility.Collapsed;
        LayersTab.Visibility = item == LayersTab ? Visibility.Visible : Visibility.Collapsed;
        BlocksTab.Visibility = item == BlocksTab ? Visibility.Visible : Visibility.Collapsed;
        ProjectTab.Visibility = item == ProjectTab ? Visibility.Visible : Visibility.Collapsed;
        HistoryTab.Visibility = item == HistoryTab ? Visibility.Visible : Visibility.Collapsed;
        RevisionsTab.Visibility = item == RevisionsTab ? Visibility.Visible : Visibility.Collapsed;
        if (tab == SideTab.History)
        {
            EnsureHistoryPanel();
        }
        if (tab == SideTab.Revisions)
        {
            EnsureRevisionsPanel();
        }
        RefreshSideRail();
    }

    private void CheckItem(MenuBarItem menu, string label, Func<bool> read, Action<bool> write)
    {
        var item = new ToggleMenuFlyoutItem
        {
            Text = label,
            Tag = "shell.toggle." + label,
            IsChecked = read(),
        };
        item.Click += (_, _) =>
        {
            write(item.IsChecked);
            ApplyChrome();
            SaveShell();
        };
        menu.Items.Add(item);
    }

    private static void CommandItem(MenuBarItem menu, string label, Action run)
    {
        var item = new MenuFlyoutItem { Text = label, Tag = "shell.cmd." + label };
        item.Click += (_, _) => run();
        menu.Items.Add(item);
    }

    private void OnTakeoffMode(object sender, RoutedEventArgs e)
    {
        _takeoffMode = TakeoffModeButton.IsChecked == true;
        ApplyChrome();
    }

    private void OnPin(object sender, RoutedEventArgs e)
    {
        _pinLeft = !_pinLeft;
        _leftCollapsed = false;
        _leftIdleSince = DateTime.UtcNow;
        UpdatePinButton();
        ApplyChrome();
        SaveShell();
    }

    private void UpdatePinButton()
    {
        if (PinLeftButton.Content is DrawnIcon icon)
        {
            icon.Kind = _pinLeft ? "pin" : "unpin";
        }
        ToolTipService.SetToolTip(PinLeftButton, _pinLeft ? "Unpin side panel (auto-hide)" : "Pin side panel");
        AutomationProperties.SetName(PinLeftButton, _pinLeft ? "Unpin side panel" : "Pin side panel");
    }

    private void OnFloat(object sender, RoutedEventArgs e)
    {
        if (sender is Button button && button.Tag as string == "layers")
        {
            OpenLayers();
            return;
        }
        FloatLeft();
    }

    private void FloatLeft()
    {
        if (_leftFloat != null)
        {
            _leftFloat.Activate();
            return;
        }
        if (SideTabs.Parent is Panel dock)
        {
            dock.Children.Remove(SideTabs);
        }
        var window = new Window { Title = "Panels" };
        var host = new Grid();
        host.Children.Add(SideTabs);
        window.Content = host;
        ShellIcon.Apply(window, ShellChrome.ThemeOf(this));
        ShellFit.UserResizable(window, minWidthDip: 240, minHeightDip: 280, maxWidthDip: 480, maxHeightDip: 900, persistKey: "panels");
        window.Closed += (_, _) =>
        {
            if (SideTabs.Parent is Panel floated)
            {
                floated.Children.Remove(SideTabs);
            }
            window.Content = null;
            if (LeftColumn is Grid column && !column.Children.Contains(SideTabs))
            {
                Grid.SetRow(SideTabs, 1);
                column.Children.Add(SideTabs);
            }
            _leftFloat = null;
            _leftCollapsed = false;
            _leftIdleSince = DateTime.UtcNow;
            ApplyColumns();
            PlaceChild();
        };
        _leftFloat = window;
        Density.Apply(host);
        ApplyColumns();
        PlaceChild();
        PresentToolWindow(window);
    }

    private void OnColumnEntered(object sender, PointerRoutedEventArgs e)
    {
        // Hovering keeps an open panel from auto-collapsing; it no longer opens a collapsed one:
        // a panel opens when its title on the rail is clicked.
        _hoverLeft = true;
        _leftIdleSince = DateTime.UtcNow;
        ApplyColumns();
    }

    private void OnColumnExited(object sender, PointerRoutedEventArgs e)
    {
        if (_dragSplit != null || sender is not FrameworkElement element)
        {
            return;
        }
        // Leaving a control inside the column bubbles here. That is not leaving the column.
        if (e.OriginalSource is DependencyObject source && source != element && IsDescendant(element, source))
        {
            var point = e.GetCurrentPoint(element).Position;
            if (point.X >= 0 && point.Y >= 0 && point.X < element.ActualWidth && point.Y < element.ActualHeight)
            {
                return;
            }
        }
        _hoverLeft = false;
        _leftIdleSince = DateTime.UtcNow;
    }

    private void OnAssistantAllow(object sender, RoutedEventArgs e)
    {
        Native.Assistant(_host, true);
        RefreshChrome();
    }

    private void OnAssistantDecline(object sender, RoutedEventArgs e)
    {
        Native.Assistant(_host, false);
        RefreshChrome();
    }

    private void OpenCalculator()
    {
        if (_calc != null)
        {
            _calc.Activate();
            return;
        }
        _calc = CalcWindow.Show(this, _host);
        _calc.Closed += (_, _) => _calc = null;
    }

    private void OpenPomodoro()
    {
        if (_pomodoro != null)
        {
            _pomodoro.Activate();
            return;
        }
        _pomodoro = PomodoroWindow.Show(this);
        _pomodoro.Closed += (_, _) => _pomodoro = null;
    }

    private void OpenTools()
    {
        if (_toolsWindow != null)
        {
            _toolsWindow.Activate();
            return;
        }
        _toolsWindow = ToolsWindow.Show(this, ToolCatalog, _toolIds, _font, (ids, font) =>
        {
            if (ids.Count > 0)
            {
                _toolIds = ids.ToList();
            }
            _font = font;
            Density.Use(_font);
            Density.Apply(PropertiesPanel);
            Density.Apply(LayersPanel);
            Density.Apply(BlocksPanel);
            BuildTools();
            SaveShell();
        });
        _toolsWindow.Closed += (_, _) => _toolsWindow = null;
    }

    private void OpenCommandChrome()
    {
        if (_chrome != null)
        {
            return;
        }
        if (CommandBar.Parent is Panel panel)
        {
            panel.Children.Remove(CommandBar);
        }
        _chrome = new CommandChrome();
        _chrome.Attach(CommandBar, WindowNative.GetWindowHandle(this));
        _chrome.WireCad(OnCadKey, OnDraft, OnCadCharacter, OnSuggestPick);
        CommandBar.SizeChanged += (_, _) => PlaceCommandBar();
        StatusRow.SizeChanged += (_, _) => PlaceCommandBar();
        if (StatusRow.Parent is FrameworkElement statusBand)
        {
            statusBand.SizeChanged += (_, _) => PlaceCommandBar();
        }
    }

    private void SetHistoryOpen(bool open)
    {
        _historyOpen = open;
        HistoryList.Visibility = open ? Visibility.Visible : Visibility.Collapsed;
        HistoryGlyph.Glyph = open ? "\uE70E" : "\uE70D";
        PlaceCommandBar();
    }

    private void SyncAskPills()
    {
        // Queued / Ask tags removed ? pill input only.
    }

    private void OnHistoryToggle(object sender, RoutedEventArgs e) => SetHistoryOpen(!_historyOpen);

    private void OnHistoryPick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not string line)
        {
            return;
        }
        _armSpace = false;
        SetHistoryOpen(false);
        AiInput.Text = line;
        AiInput.SelectionStart = line.Length;
        AiInput.Focus(FocusState.Programmatic);
    }

    private void OnClearHistory(object sender, RoutedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var clear = Density.DialogAction("Clear", primary: true);
        var cancel = Density.DialogAction("Cancel");
        Density.Name(clear, "Clear history");
        var text = new TextBlock
        {
            Text = "Clear the command history? This cannot be undone. The drawing and the current command stay as they are.",
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 420,
            FontSize = Density.FontBody,
        };
        Density.Name(text, text.Text);
        var buttons = DialogLayout.ActionRow(clear, cancel);
        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, Children = { text, buttons } };
        var theme = ShellChrome.ThemeOf(this);
        root.RequestedTheme = theme;
        var window = new Window { Title = "Clear command history", Content = root };
        ShellIcon.Apply(window, theme);
        ShellFit.ToContent(window, minWidthDip: 360, minHeightDip: 120, maxWidthDip: 520, maxHeightDip: 220);
        var pos = AppWindow.Position;
        window.AppWindow.Move(new Windows.Graphics.PointInt32(pos.X + 80, pos.Y + 120));
        cancel.Click += (_, _) => window.Close();
        clear.Click += (_, _) =>
        {
            window.Close();
            if (Native.ClearLog(_host) != 0)
            {
                History.Text = Native.LastError(_host);
                SyncHistory();
                return;
            }
            _typed.Clear();
            _recall = -1;
            _liveDraft = "";
            History.Text = "";
            SyncHistory();
            RefreshChrome();
        };
        PresentToolWindow(window);
    }

    private static bool IsCommandControl(DependencyObject source)
    {
        for (DependencyObject? node = source; node != null; node = VisualTreeHelper.GetParent(node))
        {
            if (node is TextBox or Button or ListViewBase)
            {
                return true;
            }
        }
        return false;
    }

    private void SyncHistory()
    {
        var text = History.Text ?? "";
        var lines = new List<string>();
        foreach (var part in text.Split('\n'))
        {
            var line = part.TrimEnd('\r');
            if (line.Length > 0)
            {
                lines.Add(line);
            }
        }
        if (text == _historyShown)
        {
            return;
        }
        _historyShown = text;
        HistoryList.Items.Clear();
        foreach (var line in lines)
        {
            HistoryList.Items.Add(line);
        }
        if (HistoryList.Items.Count > 0)
        {
            HistoryList.ScrollIntoView(HistoryList.Items[HistoryList.Items.Count - 1]);
        }
        PlaceCommandBar();
    }

    private void UpdateDyn()
    {
        if (_chrome == null || _host == IntPtr.Zero || !_showCommand)
        {
            HideDyn();
            return;
        }
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadText(_host, Native.TextChrome));
            var root = doc.RootElement;
            if (!root.TryGetProperty("dyn", out var dyn) || !dyn.TryGetProperty("on", out var on) || on.ValueKind != JsonValueKind.True)
            {
                if (_dynAccess.Length > 0)
                {
                    _dynAccess = "";
                    AnnouncePrompt("Command:", "");
                }
                HideDyn();
                return;
            }
            if (!TryCanvas(out var origin, out var canvasW, out var canvasH, out var scale))
            {
                return;
            }
            var px = dyn.TryGetProperty("x", out var xNode) && xNode.TryGetDouble(out var x) ? x : 0;
            var py = dyn.TryGetProperty("y", out var yNode) && yNode.TryGetDouble(out var y) ? y : 0;
            var prompt = dyn.TryGetProperty("prompt", out var promptNode) ? promptNode.GetString() ?? "" : "Command:";
            var dims = dyn.TryGetProperty("dims", out var dimsNode) && dimsNode.ValueKind == JsonValueKind.True;
            var aLabel = dyn.TryGetProperty("aLabel", out var aLabelNode) ? aLabelNode.GetString() ?? "" : "";
            var a = dyn.TryGetProperty("a", out var aNode) ? aNode.GetString() ?? "" : "";
            var bLabel = dyn.TryGetProperty("bLabel", out var bLabelNode) ? bLabelNode.GetString() ?? "" : "";
            var b = dyn.TryGetProperty("b", out var bNode) ? bNode.GetString() ?? "" : "";
            var active = dyn.TryGetProperty("active", out var activeNode) && activeNode.TryGetInt32(out var activeValue) ? activeValue : 0;
            var parts = new List<string>();
            if (!string.IsNullOrEmpty(prompt))
            {
                parts.Add(prompt);
            }
            if (dims && aLabel.Length > 0)
            {
                parts.Add((active == 0 ? "active " : "") + aLabel + " " + a);
            }
            if (dims && bLabel.Length > 0)
            {
                parts.Add((active == 1 ? "active " : "") + bLabel + " " + b);
            }
            var dynAccess = string.Join(", ", parts);
            if (dynAccess != _dynAccess)
            {
                _dynAccess = dynAccess;
                AnnouncePrompt(prompt, _dynAccess);
            }
            _chrome.CadInput.PlaceholderText = "";
            var suggestH = SuggestList is { Visibility: Visibility.Visible, Items.Count: > 0 }
                ? Math.Min(54.0, SuggestList.Items.Count * CommandChrome.DynHeightDip + 2.0)
                : 0.0;
            // Cursor field is typed text only (no chrome box); 1.5× prior half-size.
            var dipW = CommandChrome.DynWidthDip;
            var dipH = CommandChrome.DynHeightDip + suggestH;
            var left = origin.X + px / scale + 18;
            var top = origin.Y + py / scale + 8;
            if (left + dipW > origin.X + canvasW - 8)
            {
                left = origin.X + px / scale - dipW - 10;
            }
            if (top + dipH > origin.Y + canvasH - 8)
            {
                top = origin.Y + py / scale - dipH - 10;
            }
            left = Math.Max(origin.X + 4, left);
            top = Math.Max(origin.Y + 4, top);
            if (!ClientToScreen(left, top, out var sx, out var sy))
            {
                return;
            }
            var pixelW = Math.Max(1, (int)Math.Round(dipW * scale));
            var pixelH = Math.Max(1, (int)Math.Round(dipH * scale));
            if (_lastDynOn && _lastDynX == sx && _lastDynY == sy && _lastDynW == pixelW && _lastDynH == pixelH)
            {
                return;
            }
            _lastDynOn = true;
            _lastDynX = sx;
            _lastDynY = sy;
            _lastDynW = pixelW;
            _lastDynH = pixelH;
            _chrome.ShowCad(true, sx, sy, pixelW, pixelH, prompt, false, "", "", "", "", 0);
        }
        catch (JsonException)
        {
            HideDyn();
        }
    }

    private void HideDyn()
    {
        if (!_lastDynOn)
        {
            return;
        }
        _lastDynOn = false;
        _lastDynX = int.MinValue;
        _chrome?.ShowCad(false, 0, 0, 0, 0, "", false, "", "", "", "", 0);
    }

    private void PlaceCommandBar()
    {
        if (_chrome == null || _placingBar)
        {
            return;
        }
        var chrome = _chrome;
        if (!_showCommand || !TryCanvas(out var origin, out var canvasW, out var canvasH, out var scale))
        {
            chrome.ShowBar(false);
            _lastBarX = int.MinValue;
            HideDyn();
            return;
        }
        // Hide Ask while a CAD command runs so it does not cover the lower drawing (UX-007).
        // Keep it when Allow/Decline is showing so the user can answer the assistant.
        if (!string.IsNullOrEmpty(_command) && AssistantButtons.Visibility != Visibility.Visible)
        {
            chrome.ShowBar(false);
            _lastBarX = int.MinValue;
            return;
        }
        _placingBar = true;
        try
        {
            var available = Math.Max(0, canvasW - 48);
            var barW = Math.Min(520, available);
            if (double.IsNaN(CommandBar.Width) || Math.Abs(CommandBar.Width - barW) > 0.5)
            {
                CommandBar.Width = barW;
            }
            // Clear prior Height so Measure is content-sized (old tall Height locked an empty card below the pill).
            CommandBar.ClearValue(FrameworkElement.HeightProperty);
            CommandBar.ClearValue(FrameworkElement.MaxHeightProperty);
            const double pillH = 40.0;
            double barH;
            if (_historyOpen && HistoryList.Visibility == Visibility.Visible)
            {
                CommandBar.Measure(new Windows.Foundation.Size(barW, double.PositiveInfinity));
                barH = Math.Clamp(Math.Ceiling(CommandBar.DesiredSize.Height) + 2, pillH, 280.0);
            }
            else
            {
                AskInputPill.Measure(new Windows.Foundation.Size(barW, double.PositiveInfinity));
                var h = AskInputPill.DesiredSize.Height;
                barH = h > 1 ? Math.Ceiling(h) : pillH;
            }
            CommandBar.Height = barH;
            CommandBar.MaxHeight = barH;
            // Pin bottom-centre of the canvas column, flush above the status strip (or canvas bottom if hidden).
            var left = origin.X + Math.Max(0, (canvasW - barW) / 2);
            left = Math.Clamp(left, origin.X + 8, Math.Max(origin.X + 8, origin.X + canvasW - barW - 8));
            var top = origin.Y + Math.Max(8, canvasH - barH - 8);
            if (Content is FrameworkElement root
                && _showStatus
                && StatusRow.Visibility == Visibility.Visible
                && StatusRow.ActualHeight > 0)
            {
                var statusBand = StatusRow.Parent as FrameworkElement ?? StatusRow;
                var statusTop = statusBand.TransformToVisual(root).TransformPoint(new Windows.Foundation.Point(0, 0)).Y;
                top = statusTop - barH - 8;
            }
            top = Math.Max(8, top);
            if (!ClientToScreen(left, top, out var sx, out var sy))
            {
                return;
            }
            var pixelW = Math.Max(1, (int)Math.Round(barW * scale));
            var pixelH = Math.Max(1, (int)Math.Round(barH * scale));
            if (_lastBarX == sx && _lastBarY == sy && _lastBarW == pixelW && _lastBarH == pixelH)
            {
                chrome.ShowBar(true);
                return;
            }
            _lastBarX = sx;
            _lastBarY = sy;
            _lastBarW = pixelW;
            _lastBarH = pixelH;
            chrome.ShowBar(true);
            chrome.PlaceBar(sx, sy, pixelW, pixelH);
        }
        finally
        {
            _placingBar = false;
        }
    }

    private bool TryCanvas(out Windows.Foundation.Point origin, out double canvasW, out double canvasH, out double scale)
    {
        origin = default;
        canvasW = 0;
        canvasH = 0;
        scale = 1;
        if (Content is not FrameworkElement root || CanvasHost.XamlRoot == null)
        {
            return false;
        }
        scale = CanvasHost.XamlRoot.RasterizationScale;
        if (scale <= 0 || CanvasHost.ActualWidth < 1 || CanvasHost.ActualHeight < 1)
        {
            return false;
        }
        origin = CanvasHost.TransformToVisual(root).TransformPoint(new Windows.Foundation.Point(0, 0));
        canvasW = CanvasHost.ActualWidth;
        canvasH = CanvasHost.ActualHeight;
        return true;
    }

    private bool ClientToScreen(double dipX, double dipY, out int x, out int y)
    {
        x = 0;
        y = 0;
        if (Content is not FrameworkElement root || root.XamlRoot == null)
        {
            return false;
        }
        var scale = root.XamlRoot.RasterizationScale;
        var point = new Native.PointI { X = (int)Math.Round(dipX * scale), Y = (int)Math.Round(dipY * scale) };
        if (!Native.ClientToScreen(WindowNative.GetWindowHandle(this), ref point))
        {
            return false;
        }
        x = point.X;
        y = point.Y;
        return true;
    }

    private sealed class ChromeState
    {
        public string Prompt { get; set; } = "Command:";
        public List<string> History { get; set; } = new();
        public string Space { get; set; } = "Model";
        public List<string> Layouts { get; set; } = new();
        public string Layer { get; set; } = "0";
        public bool Ortho { get; set; }
        public bool Polar { get; set; }
        public bool Otrack { get; set; }
        public bool Dynmode { get; set; }
        public bool Osnap { get; set; }
        public bool Lwdisplay { get; set; }
        public bool Transparency { get; set; }
        public bool Qpmode { get; set; }
        public string Command { get; set; } = "";
        public bool LiteralSpace { get; set; }
        public string Units { get; set; } = "";
        public uint Osmode { get; set; }
        public double Polarang { get; set; } = 90;
        public bool DynpiCartesian { get; set; }
        public bool DynpiAbsolute { get; set; }
        public string Cursor { get; set; } = "";
        public string Assistant { get; set; } = "idle";
        public string AssistantPrompt { get; set; } = "";
        public string AssistantPartial { get; set; } = "";
        public string AssistantModel { get; set; } = "";
        public int ControlPort { get; set; }
        public bool Quit { get; set; }
        public List<DocTab> Documents { get; set; } = new();
        public List<LevelTab> Levels { get; set; } = new();
        public List<SheetSetTab> SheetSets { get; set; } = new();
        public bool DesignMode { get; set; } = true;
    }

    private sealed class DocTab
    {
        public int Index { get; set; }
        public string Title { get; set; } = "";
        /// <summary>The saved file behind the drawing, as the engine knows it (also set for files opened by the control channel).</summary>
        public string? Path { get; set; }
        public bool Dirty { get; set; }
        public bool Active { get; set; }
    }

    private readonly List<DocTab> _documents = new();

    private sealed class PanelSnapshot
    {
        public ulong Uid { get; set; }
        public ulong Revision { get; set; }
        public int Selection { get; set; }
        public string? Hover { get; set; }
        public string FocusLayer { get; set; } = "";
        public string FocusBlock { get; set; } = "";
        public bool Layiso { get; set; }
        public string Cecolor { get; set; } = "ByLayer";
        public string Celtype { get; set; } = "ByLayer";
        public List<PropertyRow> Properties { get; set; } = new();
        public List<LayerRow> Layers { get; set; } = new();
        public bool LayersTruncated { get; set; }
        public List<BlockRow> Blocks { get; set; } = new();
        public bool BlocksTruncated { get; set; }
        public List<LibraryRow> Library { get; set; } = new();
    }

    private sealed class LibraryRow
    {
        public string Id { get; set; } = "";
        public string Title { get; set; } = "";
        public string Description { get; set; } = "";
        public string Discipline { get; set; } = "";
    }

    private sealed class PropertyRow
    {
        public string Label { get; set; } = "";
        public string Value { get; set; } = "";
    }

    private sealed class LayerRow
    {
        public string Name { get; set; } = "";
        public bool On { get; set; }
        public bool Frozen { get; set; }
        public bool Locked { get; set; }
        public string Color { get; set; } = "";
        public bool Current { get; set; }
        public bool Focus { get; set; }
        public bool Selected { get; set; }
        public bool Hovered { get; set; }
    }

    private sealed class BlockRow
    {
        public string Name { get; set; } = "";
        public string Description { get; set; } = "";
        public int Entities { get; set; }
        public int References { get; set; }
        public List<string> Handles { get; set; } = new();
        public List<string> Attributes { get; set; } = new();
        public bool Focus { get; set; }
        public bool Selected { get; set; }
        public bool Hovered { get; set; }
    }
}

/// <summary>Side-column edge. Focusable so Left/Right can change the width.</summary>
public sealed class ResizeGrip : Control
{
    public ResizeGrip()
    {
        IsTabStop = true;
        UseSystemFocusVisuals = true;
        ProtectedCursor = InputSystemCursor.Create(InputSystemCursorShape.SizeWestEast);
        // Without a template, Control.Background never paints and the grip does not hit-test.
        Template = (ControlTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(
            """
            <ControlTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
              <Border Background="{TemplateBinding Background}" />
            </ControlTemplate>
            """);
    }
}

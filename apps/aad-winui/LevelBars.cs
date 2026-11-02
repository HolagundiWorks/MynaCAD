using System.Text.Json;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;

namespace Aad.WinUi;

/// <summary>
/// Level tabs under the drawing and the sheet-set rail beside it. Both are thin views of the
/// chrome state: every click runs an engine command (<c>level.*</c>, <c>sheetset.*</c>), and the
/// forms for new level, level settings and delete level come from the engine
/// (<c>forms.rs</c>), like the other dialogs.
/// </summary>
public sealed partial class MainWindow
{
    private sealed class LevelTab
    {
        public long Id { get; set; }
        public string Name { get; set; } = "";
        public bool Active { get; set; }
        public bool Traced { get; set; }
        public double Elevation { get; set; }

        /// <summary>The settings of the trace that shows this level under or over the active one.</summary>
        public TraceSettings? Trace { get; set; }
    }

    private sealed class TraceSettings
    {
        public int Fade { get; set; } = 60;
        public string Colour { get; set; } = "grey";
        public string Order { get; set; } = "below";
        public bool Snap { get; set; }
        public List<string>? Layers { get; set; }

        public override string ToString() =>
            Fade + "% fade, " + Colour + ", " + Order + (Snap ? ", snaps" : "") + (Layers is { Count: > 0 } ? ", " + Layers.Count + " layer(s)" : "");
    }

    private sealed class SheetSetTab
    {
        public long Id { get; set; }
        public string Name { get; set; } = "";
        public bool Active { get; set; }
        public string LayerPrefix { get; set; } = "";
    }

    private const string TraceMark = "◐";

    /// <summary>
    /// How a level is written on the tabs and menus, counted from the base level: the base level is <c>L0</c>, the levels above it
    /// <c>L1</c>, <c>L2</c>..., and the levels below it <c>-L1</c>, <c>-L2</c>... The base level is the one at elevation 0 (the datum),
    /// or the one nearest it. The level's own name stays in the tooltip.
    /// </summary>
    internal static string LevelCode(int offsetFromBase) =>
        offsetFromBase == 0 ? "L0" : offsetFromBase > 0 ? "L" + offsetFromBase : "-L" + -offsetFromBase;

    private static string LevelCodeOf(LevelTab level, IReadOnlyList<LevelTab> all)
    {
        if (all.Count == 0)
        {
            return LevelCode(0);
        }

        var ordered = all.OrderBy(l => l.Elevation).ToList();
        var baseIndex = ordered.IndexOf(ordered.OrderBy(l => Math.Abs(l.Elevation)).First());
        var index = ordered.FindIndex(l => l.Id == level.Id);
        return LevelCode(index < 0 ? 0 : index - baseIndex);
    }

    private string _levelKey = "";
    private string _sheetKey = "";
    private List<LevelTab> _cachedLevels = new();

    private void ShowLevelBars(ChromeState state)
    {
        ShowLevelTabs(state);
        ShowSheetRail(state);
    }

    private void ShowLevelTabs(ChromeState state)
    {
        var key = state.DesignMode + "|" + string.Join("|", state.Levels.Select(l => l.Id + ":" + l.Name + ":" + l.Active + ":" + l.Traced + ":" + l.Elevation + ":" + l.Trace))
            + "|" + string.Join("|", state.SheetSets.Where(s => s.Active).Select(s => s.Name));
        if (key == _levelKey)
        {
            return;
        }
        _levelKey = key;
        _cachedLevels = state.Levels.ToList();
        LevelTabs.Children.Clear();
        foreach (var level in state.Levels.ToList())
        {
            LevelTabs.Children.Add(MakeLevelTab(level, state.DesignMode));
        }
        if (state.Levels.Count == 0)
        {
            // The drawing has one implicit level: show it as the tab it is, so the bar is never empty.
            var only = SeamlessTab.Create(
                new TextBlock { Text = LevelCode(0), FontSize = Density.FontCaption, FontWeight = FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center },
                true,
                TabEdge.Bottom,
                SeamlessTab.LabelPadding);
            only.MinHeight = Density.ControlHeight;
            AutomationProperties.SetName(only, "Current level Level 1");
            ToolTipService.SetToolTip(only, "The drawing's only level. Use + to add a level for each floor.");
            LevelTabs.Children.Add(only);
        }
        var sheet = state.SheetSets.FirstOrDefault(s => s.Active);
        ToolTipService.SetToolTip(LevelHint, sheet == null ? null : "The level underneath is a read-only reference: only " + sheet.Name + " annotation is edited.");
        LevelHint.Text = state.Levels.Count == 0
            ? "One level. Add a level to draw each floor on its own."
            : sheet != null
                ? "Annotating " + sheet.Name
                : "";
    }

    private Button MakeLevelTab(LevelTab level, bool design)
    {
        var label = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs };
        if (level.Traced)
        {
            label.Children.Add(new TextBlock { Text = TraceMark, FontSize = Density.FontCaption, VerticalAlignment = VerticalAlignment.Center });
        }
        label.Children.Add(new TextBlock
        {
            Text = LevelCodeOf(level, _cachedLevels),
            MaxWidth = 140,
            FontSize = Density.FontCaption,
            TextTrimming = TextTrimming.CharacterEllipsis,
            VerticalAlignment = VerticalAlignment.Center,
            FontWeight = level.Active ? FontWeights.SemiBold : FontWeights.Normal,
        });
        var tab = SeamlessTab.Create(label, level.Active, TabEdge.Bottom, SeamlessTab.LabelPadding);
        tab.MinHeight = Density.ControlHeight;
        AutomationProperties.SetName(tab, (level.Active ? "Current level " : "Switch to level ") + level.Name);
        var tip = level.Name + "  elevation " + level.Elevation.ToString("0.###", System.Globalization.CultureInfo.InvariantCulture);
        if (level.Traced)
        {
            tip += "  shown as a trace (" + (level.Trace?.ToString() ?? "default settings") + ")";
        }
        ToolTipService.SetToolTip(tab, tip + "  (right-click for more)");
        tab.Click += (_, _) =>
        {
            RunShellCommand("level.activate", new Dictionary<string, object> { ["level"] = level.Id });
        };
        // Opened by hand: the bar is at the window bottom, and a context flyout does not always honour its placement.
        tab.RightTapped += (_, e) =>
        {
            e.Handled = true;
            BuildLevelMenu(level, design).ShowAt(tab, new FlyoutShowOptions { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft });
        };
        return tab;
    }

    private MenuFlyout BuildLevelMenu(LevelTab level, bool design)
    {
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft };
        var settings = new MenuFlyoutItem { Text = "Story settings..." };
        settings.Click += (_, _) => OpenStorySettings(level.Id);
        menu.Items.Add(settings);
        if (!level.Active)
        {
            if (design)
            {
                var send = new MenuFlyoutItem { Text = "Send selection here…" };
                send.Click += (_, _) => StartLevelSend("level.moveto", level.Id);
                menu.Items.Add(send);
                var copy = new MenuFlyoutItem { Text = "Copy selection here…" };
                copy.Click += (_, _) => StartLevelSend("level.copyto", level.Id);
                menu.Items.Add(copy);
                menu.Items.Add(new MenuFlyoutSeparator());
            }
            var trace = new ToggleMenuFlyoutItem { Text = "Trace over the current level", IsChecked = level.Traced };
            trace.Click += (_, _) => RunShellCommand(
                "level.trace",
                new Dictionary<string, object> { [trace.IsChecked ? "add" : "remove"] = new[] { level.Id } });
            menu.Items.Add(trace);
            var settingsItem = new MenuFlyoutItem { Text = level.Traced ? "Trace settings..." : "Trace with settings..." };
            settingsItem.Click += (_, _) => StartTraceForm(level);
            menu.Items.Add(settingsItem);
        }
        menu.Items.Add(new MenuFlyoutSeparator());
        var saveMilestone = new MenuFlyoutItem { Text = "Save milestone..." };
        saveMilestone.Click += (_, _) => _ = SaveMilestoneAsync("level", level.Id, level.Name);
        menu.Items.Add(saveMilestone);
        var history = new MenuFlyoutItem { Text = "History" };
        history.Click += (_, _) => ShowSidePanel(SideTab.History);
        menu.Items.Add(history);
        menu.Items.Add(new MenuFlyoutSeparator());
        var delete = new MenuFlyoutItem { Text = "Delete level..." };
        delete.Click += (_, _) => StartLevelForm("level.delete", level);
        menu.Items.Add(delete);
        return menu;
    }

    private async Task SaveMilestoneAsync(string scope, long scopeId, string label)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var box = new TextBox
        {
            Text = label + " milestone",
            PlaceholderText = "e.g. Issued for tender",
            MinWidth = 260,
        };
        Density.Name(box, "Milestone name");
        var dialog = new ContentDialog
        {
            Title = "Save milestone",
            Content = DialogLayout.Labeled("Milestone name", box),
            PrimaryButtonText = "Save",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = Content?.XamlRoot,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary)
        {
            return;
        }
        var name = box.Text?.Trim() ?? "";
        if (name.Length == 0)
        {
            History.Text = "Enter a milestone name.";
            return;
        }
        var body = new Dictionary<string, object>
        {
            ["name"] = name,
            ["scope"] = scope,
        };
        if (scope == "level")
        {
            body["level"] = scopeId;
        }
        else if (scope == "sheet")
        {
            body["sheetSet"] = scopeId;
        }
        RunShellCommand("history.save", body);
        ShowSidePanel(SideTab.History);
    }

    private void ShowSheetRail(ChromeState state)
    {
        var key = state.DesignMode + "|" + string.Join("|", state.SheetSets.Select(s => s.Id + ":" + s.Name + ":" + s.Active + ":" + s.LayerPrefix));
        if (key == _sheetKey)
        {
            return;
        }
        _sheetKey = key;
        SheetRailHost.Visibility = Visibility.Collapsed;
        TabRow.Children.Clear();
        _activeTabName = state.SheetSets.FirstOrDefault(x => x.Active)?.Name ?? "Design";
        TabRow.Children.Add(MakeTopTab(
            "Design",
            "Design: draw the building. Sheet-set annotation is not shown.",
            state.DesignMode,
            () => RunShellCommand("sheetset.activate", new Dictionary<string, object>()),
            BuildDesignSheetMenu));
        foreach (var set in state.SheetSets.ToList())
        {
            var item = set;
            var tip = item.Name + ": annotation for each level" + (item.LayerPrefix.Length > 0 ? ", on layers starting " + item.LayerPrefix : "");
            TabRow.Children.Add(MakeTopTab(
                item.Name,
                tip,
                item.Active,
                () => RunShellCommand("sheetset.activate", new Dictionary<string, object> { ["set"] = item.Id }),
                () => BuildSheetSetMenu(item)));
        }

        // Without tabs the row would hold only Design: this makes the standard discipline tabs (and layers, symbols).
        if (state.SheetSets.Count == 0)
        {
            TabRow.Children.Add(MakeTopTab(
                "+ Drawing tabs",
                "Add the standard tabs: Working drawing, Electrical, Plumbing, Interior and False ceiling, each with its layers and symbols.",
                false,
                () => RunShellCommand("sheetset.standard", new Dictionary<string, object>()),
                badge: false));
        }
    }

    /// <summary>The page badge of a tab ("A1 L"): its paper size and orientation, as saved by <c>sheet.page</c>.</summary>
    private string PageBadge(string tab)
    {
        if (_host == IntPtr.Zero || Native.Execute(_host, "sheet.page", JsonSerializer.Serialize(new Dictionary<string, object> { ["tab"] = tab })) != 0)
        {
            return "";
        }

        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(_host));
            var paper = doc.RootElement.TryGetProperty("paper", out var p) ? p.GetString() : "A1";
            var landscape = !doc.RootElement.TryGetProperty("landscape", out var l) || l.ValueKind != JsonValueKind.False;
            return paper + " " + (landscape ? "L" : "P");
        }
        catch (JsonException)
        {
            return "";
        }
    }

    /// <summary>
    /// A horizontal drawing tab: the tab itself (its name, with an accent underline when active) and, beside it, a small button with the
    /// tab's paper size and orientation that opens the page size controls of that tab. Right-click on the tab opens its menu.
    /// </summary>
    private FrameworkElement MakeTopTab(string name, string tip, bool active, Action activate, Func<MenuFlyout>? menu = null, bool badge = true)
    {
        var accent = Density.ThemeBrush("AccentBrush") ?? new SolidColorBrush(Microsoft.UI.Colors.Gray);
        var clear = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        var tab = new Button
        {
            Content = new TextBlock
            {
                Text = name,
                FontSize = Density.FontBody,
                FontWeight = active ? FontWeights.SemiBold : FontWeights.Normal,
                VerticalAlignment = VerticalAlignment.Center,
            },
            MinHeight = Density.ControlHeight,
            Padding = new Thickness(Density.SpaceMd, 2, badge ? Density.SpaceXs : Density.SpaceMd, 2),
            Background = clear,
            BorderBrush = active ? accent : clear,
            BorderThickness = new Thickness(0, 0, 0, 2),
            CornerRadius = new CornerRadius(0),
        };
        AutomationProperties.SetName(tab, (active ? "Current tab: " : "Switch to ") + name);
        ToolTipService.SetToolTip(tab, tip + (menu != null ? "  (right-click for more)" : ""));
        tab.Click += (_, _) => activate();
        if (menu != null)
        {
            tab.RightTapped += (_, e) =>
            {
                e.Handled = true;
                menu().ShowAt(tab, new FlyoutShowOptions { Placement = FlyoutPlacementMode.BottomEdgeAlignedLeft });
            };
        }

        var text = badge ? PageBadge(name) : "";
        if (text.Length == 0)
        {
            return tab;
        }

        var chip = new Button
        {
            Content = new TextBlock { Text = text, FontSize = Density.FontCaption, Opacity = 0.8 },
            MinHeight = Density.ControlHeight,
            MinWidth = 0,
            Padding = new Thickness(Density.SpaceXs, 2, Density.SpaceMd, 2),
            Background = clear,
            BorderThickness = new Thickness(0),
            CornerRadius = new CornerRadius(0),
        };
        AutomationProperties.SetName(chip, "Page size of " + name + ": " + text);
        ToolTipService.SetToolTip(chip, "Paper size and orientation of " + name + ": click to change");
        chip.Click += (_, _) => ShowPageLayout(TabRowHost, name);
        return new StackPanel { Orientation = Orientation.Horizontal, Spacing = 0, Children = { tab, chip } };
    }

    private MenuFlyout BuildDesignSheetMenu()
    {
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.RightEdgeAlignedTop };
        var page = new MenuFlyoutItem { Text = "Page size..." };
        page.Click += (_, _) => ShowPageLayout(TabRowHost, "Design");
        menu.Items.Add(page);
        var history = new MenuFlyoutItem { Text = "History" };
        history.Click += (_, _) => ShowSidePanel(SideTab.History);
        menu.Items.Add(history);
        return menu;
    }

    private MenuFlyout BuildSheetSetMenu(SheetSetTab set)
    {
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.RightEdgeAlignedTop };
        var saveMilestone = new MenuFlyoutItem { Text = "Save milestone..." };
        saveMilestone.Click += (_, _) => _ = SaveMilestoneAsync("sheet", set.Id, set.Name);
        menu.Items.Add(saveMilestone);
        var page = new MenuFlyoutItem { Text = "Page size..." };
        page.Click += (_, _) => ShowPageLayout(TabRowHost, set.Name);
        menu.Items.Add(page);
        var history = new MenuFlyoutItem { Text = "History" };
        history.Click += (_, _) => ShowSidePanel(SideTab.History);
        menu.Items.Add(history);
        return menu;
    }

    /// <summary>
    /// A vertical tab: the name is rotated 90 degrees (reading upward) inside a canvas of fixed size,
    /// because a render transform does not change layout and the canvas does not clip.
    /// </summary>
    private static Button MakeRailEntry(string name, string tip, bool active, Action activate, Func<MenuFlyout>? menu = null)
    {
        const double width = 26;
        const double lineHeight = 15;
        var length = Math.Clamp(name.Length * 6.5 + 28, 72, 168);
        var text = new TextBlock
        {
            Text = name,
            Width = length,
            TextAlignment = TextAlignment.Center,
            FontSize = Density.FontCaption,
            TextTrimming = TextTrimming.CharacterEllipsis,
            FontWeight = active ? FontWeights.SemiBold : FontWeights.Normal,
            RenderTransformOrigin = new Windows.Foundation.Point(0, 0),
            RenderTransform = new RotateTransform { Angle = -90 },
        };
        Canvas.SetLeft(text, (width - lineHeight) / 2);
        Canvas.SetTop(text, length);
        var entry = SeamlessTab.Create(new Canvas { Width = width, Height = length, Children = { text } }, active, TabEdge.Left, new Thickness(0));
        entry.Width = width;
        entry.Height = length;
        entry.MinWidth = width;
        entry.MinHeight = length;
        AutomationProperties.SetName(entry, (active ? "Current: " : "Switch to ") + name);
        ToolTipService.SetToolTip(entry, tip + (menu != null ? "  (right-click for more)" : ""));
        entry.Click += (_, _) => activate();
        if (menu != null)
        {
            entry.RightTapped += (_, e) =>
            {
                e.Handled = true;
                menu().ShowAt(entry, new FlyoutShowOptions { Placement = FlyoutPlacementMode.RightEdgeAlignedTop });
            };
        }
        return entry;
    }

    private void OnLevelsWindow(object sender, RoutedEventArgs e) => OpenStorySettings();

    private void OnAddLevel(object sender, RoutedEventArgs e)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var form = Native.ReadForm(_host, "level.new");
        if (form == null)
        {
            return;
        }
        var count = 0;
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadText(_host, Native.TextChrome));
            count = doc.RootElement.TryGetProperty("levels", out var levels) && levels.ValueKind == JsonValueKind.Array ? levels.GetArrayLength() : 0;
        }
        catch (JsonException)
        {
        }
        form = InjectFormField(form, "name", "Level " + Math.Max(2, count + 1));
        FormDialog.Show(this, _host, form, AfterCommand);
    }

    private void OnMoveToLevel(object sender, RoutedEventArgs e) => BeginMoveSelectionToLevel();

    /// <summary>Open the engine form <paramref name="id"/> for one level.</summary>
    private void StartLevelForm(string id, LevelTab level)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var form = Native.ReadForm(_host, id);
        if (form == null)
        {
            return;
        }
        form = InjectFormField(form, "level", level.Name);
        if (id == "level.set")
        {
            form = InjectFormField(form, "elevation", level.Elevation.ToString("0.###", System.Globalization.CultureInfo.InvariantCulture));
        }
        FormDialog.Show(this, _host, form, AfterCommand);
    }

    /// <summary>Open the Trace Settings form for <paramref name="level"/>, filled with the trace's current settings.</summary>
    private void StartTraceForm(LevelTab level)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var form = Native.ReadForm(_host, "level.trace");
        if (form == null)
        {
            return;
        }
        form = InjectFormField(form, "add", level.Name);
        if (level.Trace is { } trace)
        {
            form = InjectFormField(form, "fade", trace.Fade.ToString(System.Globalization.CultureInfo.InvariantCulture));
            form = InjectFormField(form, "colour", trace.Colour);
            form = InjectFormField(form, "order", trace.Order);
            form = InjectFormField(form, "snap", trace.Snap ? "true" : "false");
            form = InjectFormField(form, "layers", trace.Layers is { Count: > 0 } layers ? string.Join(", ", layers) : "*");
        }
        FormDialog.Show(this, _host, form, AfterCommand);
    }

    /// <summary>Run one engine command with JSON parameters; show its error in the history.</summary>
    private void RunShellCommand(string id, Dictionary<string, object> parameters)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (Native.Execute(_host, id, JsonSerializer.Serialize(parameters)) != 0)
        {
            History.Text = Native.LastError(_host);
        }
        AfterCommand();
    }

    /// <summary>
    /// Start send/copy-to-level with the target preset; the host then asks for an origin point
    /// (and matching point) on the drawing so floors drawn offset can be aligned.
    /// </summary>
    private void StartLevelSend(string command, long levelId)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var preset = JsonSerializer.Serialize(new Dictionary<string, object> { ["level"] = levelId });
        if (Native.StartForm(_host, command, preset) != 0)
        {
            History.Text = Native.LastError(_host);
            AfterCommand();
            return;
        }
        AfterCommand();
    }

    /// <summary>Pick a target level, then base and match points on the drawing (<c>level.moveto</c> form).</summary>
    private void BeginMoveSelectionToLevel()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var targets = _cachedLevels.Where(l => !l.Active).ToList();
        if (targets.Count == 0)
        {
            History.Text = "Add another level first, or right-click a level tab and choose Send selection here.";
            return;
        }
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft };
        foreach (var level in targets)
        {
            var item = new MenuFlyoutItem { Text = LevelCodeOf(level, _cachedLevels) + "  " + level.Name };
            var id = level.Id;
            item.Click += (_, _) => StartLevelSend("level.moveto", id);
            menu.Items.Add(item);
        }
        menu.Items.Add(new MenuFlyoutSeparator());
        var copy = new MenuFlyoutItem { Text = "Copy selection to level…" };
        copy.Click += (_, _) => ShowCopyToLevelMenu(targets);
        menu.Items.Add(copy);
        menu.ShowAt(MoveToLevelButton, new FlyoutShowOptions { Placement = FlyoutPlacementMode.Top });
    }

    private void ShowCopyToLevelMenu(List<LevelTab> targets)
    {
        var menu = new MenuFlyout();
        foreach (var level in targets)
        {
            var item = new MenuFlyoutItem { Text = LevelCodeOf(level, _cachedLevels) + "  " + level.Name };
            var id = level.Id;
            item.Click += (_, _) => StartLevelSend("level.copyto", id);
            menu.Items.Add(item);
        }
        menu.ShowAt(MoveToLevelButton, new FlyoutShowOptions { Placement = FlyoutPlacementMode.Top });
    }
}

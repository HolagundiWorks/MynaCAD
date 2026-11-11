using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.Storage;
using Windows.System;

namespace Aad.WinUi;

/// <summary>
/// Layer properties for the open drawing. The layer name is the identity (the drawing has no
/// separate layer id). The revision is the document revision. Mutations go through engine commands.
/// </summary>
public sealed partial class LayerManagerWindow : Window
{
    private static readonly string[] Lineweights =
    [
        "Default", "0.00 mm", "0.05 mm", "0.09 mm", "0.13 mm", "0.15 mm", "0.18 mm", "0.20 mm",
        "0.25 mm", "0.30 mm", "0.35 mm", "0.40 mm", "0.50 mm", "0.53 mm", "0.60 mm", "0.70 mm",
        "0.80 mm", "0.90 mm", "1.00 mm", "1.06 mm", "1.20 mm", "1.40 mm", "1.58 mm", "2.00 mm", "2.11 mm",
    ];

    private static readonly (double Width, string Title)[] Columns =
    [
        (48, "Status"), (148, "Name"), (36, "On"), (48, "Freeze"), (40, "Lock"), (40, "Plot"),
        (110, "Color"), (132, "Linetype"), (100, "Lineweight"), (100, "Transparency"), (108, "New VP Freeze"), (180, "Description"),
    ];

    private readonly IntPtr _host;
    private readonly Action _changed;
    private readonly List<LayerRecord> _all = new();
    private readonly List<LayerRecord> _shown = new();
    private readonly HashSet<string> _selected = new(StringComparer.OrdinalIgnoreCase);
    private readonly List<string> _linetypes = new();
    private readonly List<string> _states = new();
    private readonly Button _nameSort = new() { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0), HorizontalContentAlignment = HorizontalAlignment.Left, Padding = Density.CellPadding };
    private string _pivot = "";
    private ulong _uid;
    private ulong _revision;
    private bool _sortDesc;
    private bool _reloading;
    private bool _acceptText;
    private int _editors;
    private bool _refreshWhenIdle;
    private string _focusName = "";

    /// <summary>The shell docks this view into the Layers column, or opens it in this window.</summary>
    public event EventHandler? MergeRequested;

    /// <summary>The shell opens the layer mapping wizard for the selected layers (the wizard lives in the main window).</summary>
    public event EventHandler<List<string>>? MapRequested;

    public LayerManagerWindow(FrameworkElement? themeSource, IntPtr host, Action changed)
    {
        InitializeComponent();
        _host = host;
        _changed = changed;
        if (Content is FrameworkElement root)
        {
            if (themeSource != null)
            {
                root.RequestedTheme = themeSource.RequestedTheme;
            }
            Density.Apply(root);
        }
        BuildHeader();
        AutomationProperties.SetLiveSetting(Status, AutomationLiveSetting.Polite);
        AutomationProperties.SetLiveSetting(Summary, AutomationLiveSetting.Polite);
        AutomationProperties.SetName(Empty, "No layers match the search.");
        _reloading = true;
        LoadPrefs();
        _reloading = false;
        Closed += (_, _) => SavePrefs();
        // The layer table needs 720 DIP before columns scroll; otherwise the Manager defaults.
        StandardWindow.Present(this, null, WindowTemplate.Manager, "layers", new WindowBounds(720, 420, 1400, 1000));
    }

    public void ApplyTheme(ElementTheme theme)
    {
        ShellChrome.Apply(this, theme);
        // When merged into the side panel, Content is null but Root still needs the shell theme.
        ShellChrome.Paint(Root, theme);
    }

    public void ShowMerged(bool merged)
    {
        // Dock returns to the compact Layer side tab; the full manager stays a separate window.
        MergeIcon.Kind = merged ? "undock" : "dock";
        var tip = merged ? "Open Layer Manager window" : "Show Layer side panel";
        ToolTipService.SetToolTip(MergeButton, tip);
        AutomationProperties.SetName(MergeButton, tip);
    }

    /// <summary>Take the view out of this window so the side panel can host it.</summary>
    public void DetachBody()
    {
        if (Content == Root)
        {
            Content = null;
        }
    }

    /// <summary>Put the view back in this window.</summary>
    public void AttachBody()
    {
        if (Root.Parent is Panel panel)
        {
            panel.Children.Remove(Root);
        }
        Content = Root;
    }

    public void Conceal() => AppWindow.Hide();

    public FrameworkElement Body() => Root;

    private void OnMergeRequested(object sender, RoutedEventArgs e) => MergeRequested?.Invoke(this, EventArgs.Empty);

    private XamlRoot? ViewRoot => Root.XamlRoot;

    public void OnDocumentRevision(ulong uid, ulong revision)
    {
        if (_reloading || (uid == _uid && revision == _revision))
        {
            return;
        }
        if (!AppWindow.IsVisible)
        {
            // A hidden manager is reloaded when it is shown (OpenLayers reloads); rebuilding its table on every change cost ~0.25 s each.
            return;
        }
        if (_editors > 0)
        {
            _refreshWhenIdle = true;
            return;
        }
        Status.Text = "The drawing changed. The list was refreshed.";
        Reload("");
    }

    public void Reload(string focusName)
    {
        _focusName = focusName;
        var selected = SelectedNames();
        if (focusName.Length > 0)
        {
            selected.Add(focusName);
        }
        _reloading = true;
        try
        {
            if (!ReadTable())
            {
                return;
            }
            Show(selected);
        }
        finally
        {
            _reloading = false;
        }
    }

    private void OnSearch(object sender, TextChangedEventArgs e) => Show(SelectedNames());

    private void OnSort(object sender, RoutedEventArgs e)
    {
        _sortDesc = !_sortDesc;
        SavePrefs();
        Show(SelectedNames());
    }

    private void OnNew(object sender, RoutedEventArgs e) => CreateLayer(false);

    private void OnNewFrozen(object sender, RoutedEventArgs e) => CreateLayer(true);

    private void OnSetCurrent(object sender, RoutedEventArgs e)
    {
        if (_selected.Count != 1)
        {
            Status.Text = "Select one layer.";
            return;
        }
        SetCurrent(_selected.First());
    }

    private void OnRefresh(object sender, RoutedEventArgs e) => Reload("");

    private void CreateLayer(bool freezeInNewViewports)
    {
        if (!EnsureCurrent())
        {
            return;
        }
        var name = NextName();
        if (name == null)
        {
            Status.Text = "Could not choose a new layer name.";
            return;
        }
        if (!Run("layer.new", new Dictionary<string, object> { ["name"] = name }))
        {
            return;
        }
        if (freezeInNewViewports && !Run("layer.set", new Dictionary<string, object> { ["name"] = name, ["newVpFreeze"] = true }))
        {
            Reload(name);
            _changed();
            return;
        }
        Status.Text = "Created " + name + ".";
        Reload(name);
        _changed();
    }

    private async void OnDelete(object sender, RoutedEventArgs e)
    {
        try
        {
            await DeleteSelection();
        }
        catch (Exception ex)
        {
            Status.Text = ex.Message;
        }
    }

    private void OnUndo(object sender, RoutedEventArgs e) => History("u");

    private void OnRedo(object sender, RoutedEventArgs e) => History("redo");

    private void OnMakeCurrent(object sender, RoutedEventArgs e)
    {
        if (RecordOf(sender) is LayerRecord row)
        {
            SetCurrent(row.Name);
        }
    }

    private void OnNameDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (RecordOf(sender) is LayerRecord row)
        {
            e.Handled = true;
            _ = Rename(row);
        }
    }

    private void OnVisibility(object sender, RoutedEventArgs e) => Toggle(sender, "on");

    private void OnFreeze(object sender, RoutedEventArgs e) => Toggle(sender, "frozen");

    private void OnLock(object sender, RoutedEventArgs e) => Toggle(sender, "locked");

    private void OnPlot(object sender, RoutedEventArgs e) => Toggle(sender, "plot");

    private void OnNewVp(object sender, RoutedEventArgs e)
    {
        if (RecordOf(sender) is LayerRecord row)
        {
            Set(row.Name, Params(row, "newVpFreeze", !row.NewVpFreeze));
        }
    }

    private async void OnColor(object sender, RoutedEventArgs e)
    {
        if (RecordOf(sender) is not LayerRecord row)
        {
            return;
        }
        var next = await IndexColorWindow.PickAsync(this, row.ColorName, allowByLayerByBlock: false);
        if (next == null || next == row.ColorName)
        {
            return;
        }
        Set(row.Name, Params(row, "color", next));
    }

    private void OnLinetype(object sender, RoutedEventArgs e)
    {
        if (sender is not Button button || RecordOf(sender) is not LayerRecord row)
        {
            return;
        }
        ShowChoices(button, _linetypes, row.Linetype, value => Set(row.Name, Params(row, "linetype", value)));
    }

    private void OnLineweight(object sender, RoutedEventArgs e)
    {
        if (sender is not Button button || RecordOf(sender) is not LayerRecord row)
        {
            return;
        }
        ShowChoices(button, Lineweights, row.Lineweight, value => Set(row.Name, Params(row, "lineweight", value)));
    }

    private async void OnTransparency(object sender, RoutedEventArgs e)
    {
        if (RecordOf(sender) is not LayerRecord row)
        {
            return;
        }
        var next = await Ask("Transparency", "Percent from 0 to 90.", row.Transparency.ToString(), "Apply");
        if (next == null)
        {
            return;
        }
        if (!int.TryParse(next.Trim(), out var percent) || percent < 0 || percent > 90)
        {
            Status.Text = "Transparency is a whole number from 0 to 90.";
            return;
        }
        if (percent == row.Transparency && TargetsOf(row).Count == 1)
        {
            return;
        }
        Set(row.Name, Params(row, "transparency", percent));
    }

    private void OnEditorFocus(object sender, RoutedEventArgs e) => _editors++;

    private void OnDescriptionLost(object sender, RoutedEventArgs e)
    {
        _editors = Math.Max(0, _editors - 1);
        if (!_reloading)
        {
            CommitDescription(sender);
        }
        if (_refreshWhenIdle && _editors == 0)
        {
            _refreshWhenIdle = false;
            Status.Text = "The drawing changed. The list was refreshed.";
            Reload("");
        }
    }

    private void OnDescriptionKey(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && sender is TextBox box)
        {
            e.Handled = true;
            box.Focus(FocusState.Programmatic);
            CommitDescription(sender);
        }
        else if (e.Key == VirtualKey.Escape && sender is TextBox editor && RecordOf(sender) is LayerRecord row)
        {
            e.Handled = true;
            editor.Text = row.Description;
            TableScroll.Focus(FocusState.Programmatic);
        }
    }

    private void OnRowMenu(object sender, RightTappedRoutedEventArgs e)
    {
        if (RecordOf(e.OriginalSource) is not LayerRecord row)
        {
            return;
        }
        if (!_selected.Contains(row.Name))
        {
            _selected.Clear();
            _selected.Add(row.Name);
            _pivot = row.Name;
            PaintSelection();
        }
        var menu = new MenuFlyout();
        // With several layers selected the actions apply to all of them; renaming and making current are for one.
        var several = _selected.Count > 1;
        menu.Items.Add(Item("Make current", () => SetCurrent(row.Name), row.CanMakeCurrent && !several));
        menu.Items.Add(Item("Rename", () => _ = Rename(row), !several));
        menu.Items.Add(Item(row.On ? "Turn off" : "Turn on", () => Set(row.Name, Params(row, "on", !row.On)), true));
        menu.Items.Add(Item(row.Frozen ? "Thaw" : "Freeze", () => Set(row.Name, Params(row, "frozen", !row.Frozen)), true));
        menu.Items.Add(Item(row.Locked ? "Unlock" : "Lock", () => Set(row.Name, Params(row, "locked", !row.Locked)), true));
        menu.Items.Add(Item("Map to built-in layers...", () => MapSelectionToBuiltIn(), true));
        menu.Items.Add(Item("Delete", () => _ = DeleteSelection(), true));
        menu.ShowAt(Rows, e.GetPosition(Rows));
        e.Handled = true;
    }

    private void OnWindowKey(object sender, KeyRoutedEventArgs e)
    {
        var control = (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        if (control && e.Key == VirtualKey.F)
        {
            e.Handled = true;
            SearchBox.Focus(FocusState.Keyboard);
            SearchBox.SelectAll();
            return;
        }
        if (e.Key == VirtualKey.F2 && _selected.Count == 1 && _shown.FirstOrDefault(item => item.Name.Equals(_selected.First(), StringComparison.OrdinalIgnoreCase)) is LayerRecord row)
        {
            e.Handled = true;
            _ = Rename(row);
            return;
        }
        if (control && e.Key == VirtualKey.A && ViewRoot is XamlRoot selectRoot && FocusManager.GetFocusedElement(selectRoot) is not TextBox)
        {
            e.Handled = true;
            _selected.Clear();
            foreach (var item in _shown)
            {
                _selected.Add(item.Name);
            }
            _pivot = _shown.FirstOrDefault()?.Name ?? "";
            PaintSelection();
            return;
        }
        if (e.Key == VirtualKey.Delete && ViewRoot is XamlRoot deleteRoot && FocusManager.GetFocusedElement(deleteRoot) is not TextBox)
        {
            e.Handled = true;
            _ = DeleteSelection();
        }
    }

    private void Toggle(object sender, string field)
    {
        if (RecordOf(sender) is not LayerRecord row)
        {
            return;
        }
        var value = field switch
        {
            "on" => !row.On,
            "frozen" => !row.Frozen,
            "locked" => !row.Locked,
            "plot" => !row.Plot,
            _ => false,
        };
        Set(row.Name, Params(row, field, value));
    }

    private void SetCurrent(string name)
    {
        if (!EnsureCurrent())
        {
            return;
        }
        if (!Run("layer.current", new Dictionary<string, object> { ["name"] = name }))
        {
            Reload(name);
            return;
        }
        Status.Text = name + " is the current layer.";
        Reload(name);
        _changed();
    }

    private void Set(string name, Dictionary<string, object> parameters)
    {
        if (!EnsureCurrent())
        {
            return;
        }
        if (!Run("layer.set", parameters))
        {
            Reload(name);
            return;
        }
        Status.Text = "";
        if (parameters.TryGetValue("names", out var many) && many is List<string> list)
        {
            var skipped = ReadSkipped();
            Status.Text = "Changed " + (list.Count - skipped.Count) + " layers." + (skipped.Count > 0 ? " Not changed (current layer): " + string.Join(", ", skipped) + "." : "");
        }
        Reload(name);
        _changed();
    }

    private void CommitDescription(object sender)
    {
        if (sender is not TextBox box || RecordOf(sender) is not LayerRecord row || box.Text == row.Description)
        {
            return;
        }
        Set(row.Name, new Dictionary<string, object> { ["name"] = row.Name, ["description"] = box.Text });
    }

    private async Task Rename(LayerRecord row)
    {
        var next = await Ask("Rename layer", "New name for " + row.Name + ".", row.Name, "Rename");
        if (next == null)
        {
            return;
        }
        next = next.Trim();
        if (next.Length == 0 || next.Equals(row.Name, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }
        Set(row.Name, new Dictionary<string, object> { ["name"] = row.Name, ["newName"] = next });
    }

    private async Task DeleteSelection()
    {
        var names = _selected.ToList();
        if (names.Count == 0)
        {
            Status.Text = "Select one or more layers.";
            return;
        }
        var prompt = names.Count == 1
            ? "Delete layer " + names[0] + "? The command keeps the layer when it still has objects."
            : "Delete " + names.Count + " layers in one undo step? A layer that still has objects, the current layer, layer 0 and Defpoints stay.";
        var dialog = CompactContentDialog(
            "Delete layers",
            new TextBlock { Text = prompt, TextWrapping = TextWrapping.Wrap, FontSize = Density.FontBody },
            "Delete",
            ContentDialogButton.Close);
        if (await dialog.ShowAsync() != ContentDialogResult.Primary || !EnsureCurrent())
        {
            return;
        }
        // One command, one undo step; the layers that cannot go (used, current, protected) are reported and kept.
        var failed = new List<string>();
        var removed = 0;
        if (Native.Execute(_host, "layer.delete", JsonSerializer.Serialize(new Dictionary<string, object> { ["names"] = names })) != 0)
        {
            failed.Add(Native.LastError(_host));
        }
        else
        {
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(_host));
                removed = doc.RootElement.GetProperty("deleted").GetArrayLength();
                foreach (var s in doc.RootElement.GetProperty("skipped").EnumerateArray())
                {
                    failed.Add(s.GetProperty("name").GetString() + ": " + s.GetProperty("reason").GetString() + ".");
                }
            }
            catch (Exception ex) when (ex is JsonException or KeyNotFoundException or InvalidOperationException)
            {
                removed = names.Count;
            }
        }
        Status.Text = failed.Count == 0
            ? "Deleted " + removed + "."
            : "Deleted " + removed + ". " + string.Join(" ", failed);
        _selected.Clear();
        Reload("");
        if (removed > 0)
        {
            _changed();
        }
    }

    private void History(string command)
    {
        if (!Run(command, new Dictionary<string, object>()))
        {
            return;
        }
        Status.Text = "";
        Reload("");
        _changed();
    }

    private bool EnsureCurrent()
    {
        if (!TryReadRevision(out var uid, out var revision))
        {
            return true;
        }
        if (uid == _uid && revision == _revision)
        {
            return true;
        }
        Status.Text = "The drawing changed. The list was refreshed.";
        Reload("");
        return false;
    }

    private bool Run(string command, Dictionary<string, object> parameters)
    {
        if (_host == IntPtr.Zero)
        {
            return false;
        }
        if (Native.Execute(_host, command, JsonSerializer.Serialize(parameters)) == 0)
        {
            return true;
        }
        Status.Text = Native.LastError(_host);
        return false;
    }

    private bool ReadTable()
    {
        if (!Run("layer", new Dictionary<string, object>()))
        {
            return false;
        }
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(_host));
            var root = doc.RootElement;
            _all.Clear();
            _linetypes.Clear();
            if (root.TryGetProperty("linetypes", out var linetypes))
            {
                foreach (var name in linetypes.EnumerateArray())
                {
                    var text = name.GetString();
                    if (!string.IsNullOrEmpty(text))
                    {
                        _linetypes.Add(text);
                    }
                }
            }
            if (root.TryGetProperty("layers", out var layers))
            {
                foreach (var layer in layers.EnumerateArray())
                {
                    _all.Add(LayerRecord.From(layer));
                }
            }
            _states.Clear();
            if (root.TryGetProperty("states", out var states))
            {
                foreach (var state in states.EnumerateArray())
                {
                    var text = state.GetString();
                    if (!string.IsNullOrEmpty(text))
                    {
                        _states.Add(text);
                    }
                }
            }
        }
        catch (JsonException)
        {
            Status.Text = "The layer table could not be read.";
            return false;
        }
        if (TryReadRevision(out var uid, out var revision))
        {
            _uid = uid;
            _revision = revision;
        }
        return true;
    }

    private bool TryReadRevision(out ulong uid, out ulong revision)
    {
        uid = 0;
        revision = 0;
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadText(_host, Native.TextPanels));
            uid = doc.RootElement.GetProperty("uid").GetUInt64();
            revision = doc.RootElement.GetProperty("revision").GetUInt64();
            return true;
        }
        catch (JsonException)
        {
            return false;
        }
    }

    private void Show(HashSet<string> selected)
    {
        var query = SearchBox.Text.Trim();
        _shown.Clear();
        _shown.AddRange(_all.Where(row => Matches(row, query)));
        _shown.Sort((a, b) =>
        {
            var order = string.Compare(a.Name, b.Name, StringComparison.OrdinalIgnoreCase);
            return _sortDesc ? -order : order;
        });
        _selected.Clear();
        foreach (var row in _shown)
        {
            if (selected.Contains(row.Name))
            {
                _selected.Add(row.Name);
            }
        }
        if (_pivot.Length == 0 || _shown.All(row => !row.Name.Equals(_pivot, StringComparison.OrdinalIgnoreCase)))
        {
            _pivot = _shown.FirstOrDefault()?.Name ?? "";
        }
        BuildRows();
        FillStates();
        Empty.Visibility = _shown.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        var current = _all.FirstOrDefault(row => row.Current)?.Name ?? "—";
        _nameSort.Content = _sortDesc ? "Name  ▼" : "Name  ▲";
        Summary.Text = "Layers displayed: " + _shown.Count + " of " + _all.Count + ".   Current layer: " + current;
        if (_focusName.Length > 0)
        {
            var focus = _focusName;
            _focusName = "";
            _selected.Clear();
            _selected.Add(focus);
            _pivot = focus;
            PaintSelection();
            DispatcherQueue.TryEnqueue(() =>
            {
                foreach (var child in Rows.Children)
                {
                    if (child is FrameworkElement element && element.DataContext is LayerRecord row && row.Name.Equals(focus, StringComparison.OrdinalIgnoreCase))
                    {
                        element.StartBringIntoView();
                        element.Focus(FocusState.Programmatic);
                        return;
                    }
                }
            });
        }
    }

    private bool Matches(LayerRecord row, string query)
    {
        if (query.Length > 0 && row.Name.IndexOf(query, StringComparison.OrdinalIgnoreCase) < 0)
        {
            return false;
        }
        return true;
    }

    private void BuildHeader()
    {
        Header.ColumnDefinitions.Clear();
        Header.Children.Clear();
        for (var i = 0; i < Columns.Length; i++)
        {
            Header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Columns[i].Width) });
            if (i == 1)
            {
                _nameSort.Content = "Name  ▲";
                _nameSort.Click += OnSort;
                AutomationProperties.SetName(_nameSort, "Sort layers by name");
                Grid.SetColumn(_nameSort, i);
                Header.Children.Add(_nameSort);
                continue;
            }
            var title = new TextBlock
            {
                Text = Columns[i].Title,
                VerticalAlignment = VerticalAlignment.Center,
                Margin = Density.CellPadding,
                FontSize = Density.FontBody,
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            };
            Grid.SetColumn(title, i);
            Header.Children.Add(title);
        }
        // An explicit width stops the grid from shrinking its columns into the viewport.
        Header.Width = Columns.Sum(column => column.Width);
        Header.HorizontalAlignment = HorizontalAlignment.Left;
        Density.Apply(Header);
    }

    private void BuildRows()
    {
        Rows.Children.Clear();
        foreach (var row in _shown)
        {
            var grid = new Grid { MinHeight = Density.PropertyRowHeight, Width = Header.Width, HorizontalAlignment = HorizontalAlignment.Left, DataContext = row, Background = RowBrush(row.Name, Rows.Children.Count) };
            AutomationProperties.SetName(grid, row.Name);
            grid.PointerPressed += OnRowPressed;
            grid.RightTapped += OnRowMenu;
            for (var i = 0; i < Columns.Length; i++)
            {
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Columns[i].Width) });
            }
            Add(grid, 0, IconButton(row.StatusKind, row.CurrentAccess, OnMakeCurrent));
            var name = new TextBlock { Text = row.Name, VerticalAlignment = VerticalAlignment.Center, Margin = Density.CellPadding };
            AutomationProperties.SetName(name, row.NameAccess);
            name.DoubleTapped += OnNameDoubleTapped;
            Add(grid, 1, name);
            Add(grid, 2, IconButton(row.OnKind, row.OnAccess, OnVisibility));
            Add(grid, 3, IconButton(row.FreezeKind, row.FreezeAccess, OnFreeze));
            Add(grid, 4, IconButton(row.LockKind, row.LockAccess, OnLock));
            Add(grid, 5, IconButton(row.PlotKind, row.PlotAccess, OnPlot));
            var color = new Button
            {
                Background = Transparent(),
                BorderThickness = new Thickness(0),
                HorizontalContentAlignment = HorizontalAlignment.Left,
                MinHeight = Density.ControlHeight,
                Padding = Density.CellPadding,
            };
            color.Content = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Spacing = Density.SpaceSm,
                Children = { new Border { Width = Density.SwatchSize, Height = Density.SwatchSize, Background = row.Swatch, VerticalAlignment = VerticalAlignment.Center }, new TextBlock { Text = row.ColorName, VerticalAlignment = VerticalAlignment.Center } },
            };
            AutomationProperties.SetName(color, row.ColorAccess);
            color.Click += OnColor;
            Add(grid, 6, color);
            Add(grid, 7, TextButton(row.Linetype, row.LinetypeAccess, OnLinetype));
            Add(grid, 8, TextButton(row.Lineweight, row.LineweightAccess, OnLineweight));
            Add(grid, 9, TextButton(row.TransparencyLabel, row.TransparencyAccess, OnTransparency));
            Add(grid, 10, IconButton(row.NewVpKind, row.NewVpAccess, OnNewVp));
            var description = new TextBox
            {
                Text = row.Description,
                Background = Transparent(),
                BorderThickness = new Thickness(0),
                MinHeight = Density.ControlHeight,
                Padding = Density.CellPadding,
                VerticalAlignment = VerticalAlignment.Center,
            };
            AutomationProperties.SetName(description, row.DescriptionAccess);
            description.GotFocus += OnEditorFocus;
            description.LostFocus += OnDescriptionLost;
            description.KeyDown += OnDescriptionKey;
            Add(grid, 11, description);
            Density.Apply(grid);
            Rows.Children.Add(grid);
        }
    }

    private void OnRowPressed(object sender, PointerRoutedEventArgs e)
    {
        if (sender is not FrameworkElement element || element.DataContext is not LayerRecord row || IsEditor(e.OriginalSource as DependencyObject))
        {
            return;
        }
        var control = Key(VirtualKey.Control);
        var shift = Key(VirtualKey.Shift);
        if (shift && _pivot.Length > 0)
        {
            var from = _shown.FindIndex(item => item.Name.Equals(_pivot, StringComparison.OrdinalIgnoreCase));
            var to = _shown.FindIndex(item => item.Name.Equals(row.Name, StringComparison.OrdinalIgnoreCase));
            if (from >= 0 && to >= 0)
            {
                if (!control)
                {
                    _selected.Clear();
                }
                var start = Math.Min(from, to);
                var end = Math.Max(from, to);
                for (var i = start; i <= end; i++)
                {
                    _selected.Add(_shown[i].Name);
                }
            }
        }
        else if (control)
        {
            if (!_selected.Remove(row.Name))
            {
                _selected.Add(row.Name);
            }
            _pivot = row.Name;
        }
        else
        {
            _selected.Clear();
            _selected.Add(row.Name);
            _pivot = row.Name;
        }
        PaintSelection();
        e.Handled = true;
    }

    private void PaintSelection()
    {
        for (var i = 0; i < Rows.Children.Count; i++)
        {
            if (Rows.Children[i] is Grid grid && grid.DataContext is LayerRecord row)
            {
                grid.Background = RowBrush(row.Name, i);
            }
        }
    }

    private Brush RowBrush(string name, int index)
    {
        if (_selected.Contains(name))
        {
            return Density.SelectionFill();
        }
        return index % 2 == 1 ? Density.ZebraFill() : Transparent();
    }

    private void FillStates()
    {
        var keep = States.SelectedItem as string;
        _reloading = true;
        try
        {
            States.Items.Clear();
            foreach (var state in _states)
            {
                States.Items.Add(state);
            }
            if (keep != null && _states.Contains(keep))
            {
                States.SelectedItem = keep;
            }
        }
        finally
        {
            _reloading = false;
        }
        var chosen = States.SelectedItem is string;
        RestoreStateButton.IsEnabled = chosen;
        DeleteStateButton.IsEnabled = chosen;
    }

    private void OnStatePicked(object sender, SelectionChangedEventArgs e)
    {
        if (_reloading)
        {
            return;
        }
        var chosen = States.SelectedItem is string;
        RestoreStateButton.IsEnabled = chosen;
        DeleteStateButton.IsEnabled = chosen;
    }

    private void OnRestoreState(object sender, RoutedEventArgs e)
    {
        if (States.SelectedItem is string name)
        {
            HistoryCommand("layerstate.restore", name);
        }
    }

    private void OnDeleteState(object sender, RoutedEventArgs e)
    {
        if (States.SelectedItem is string name)
        {
            HistoryCommand("layerstate.delete", name);
        }
    }

    private void OnSaveState(object sender, RoutedEventArgs e)
    {
        var name = StateName.Text.Trim();
        if (name.Length == 0)
        {
            Status.Text = "Type a name for the layer state.";
            return;
        }
        if (!Run("layerstate.save", new Dictionary<string, object> { ["name"] = name }))
        {
            return;
        }
        StateName.Text = "";
        Status.Text = "Saved layer state " + name + ".";
        Reload("");
        _changed();
        States.SelectedItem = name;
    }

    private void HistoryCommand(string command, string name)
    {
        if (!Run(command, new Dictionary<string, object> { ["name"] = name }))
        {
            return;
        }
        Status.Text = "";
        Reload("");
        _changed();
    }

    private static void Add(Grid grid, int column, FrameworkElement child)
    {
        Grid.SetColumn(child, column);
        grid.Children.Add(child);
    }

    private static Button IconButton(string kind, string access, RoutedEventHandler click)
    {
        var button = new Button
        {
            Background = Transparent(),
            BorderThickness = new Thickness(0),
            Padding = new Thickness(0),
            Width = Density.MinTarget,
            Height = Density.MinTarget,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        button.Content = new DrawnIcon { Kind = kind };
        AutomationProperties.SetName(button, access);
        button.Click += click;
        return button;
    }

    private static Button TextButton(string text, string access, RoutedEventHandler click)
    {
        var button = new Button
        {
            Content = text,
            Background = Transparent(),
            BorderThickness = new Thickness(0),
            HorizontalContentAlignment = HorizontalAlignment.Left,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            MinHeight = Density.ControlHeight,
            Padding = Density.CellPadding,
        };
        AutomationProperties.SetName(button, access);
        button.Click += click;
        return button;
    }

    private static SolidColorBrush Transparent() => new(Microsoft.UI.Colors.Transparent);

    private static bool IsEditor(DependencyObject? node)
    {
        while (node != null)
        {
            if (node is Button or TextBox)
            {
                return true;
            }
            node = VisualTreeHelper.GetParent(node);
        }
        return false;
    }

    private static bool Key(VirtualKey key) =>
        (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(key) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;

    private string? NextName()
    {
        for (var i = 1; i <= 10_000; i++)
        {
            var candidate = "Layer" + i;
            if (!_all.Any(row => row.Name.Equals(candidate, StringComparison.OrdinalIgnoreCase)))
            {
                return candidate;
            }
        }
        return null;
    }

    private HashSet<string> SelectedNames() => new(_selected, StringComparer.OrdinalIgnoreCase);

    private void ShowChoices(FrameworkElement anchor, IReadOnlyList<string> values, string current, Action<string> apply)
    {
        var menu = new MenuFlyout();
        if (values.Count == 0)
        {
            menu.Items.Add(new MenuFlyoutItem { Text = "None loaded", IsEnabled = false });
        }
        foreach (var value in values)
        {
            var item = new MenuFlyoutItem { Text = value, IsEnabled = !value.Equals(current, StringComparison.OrdinalIgnoreCase) };
            var chosen = value;
            item.Click += (_, _) => apply(chosen);
            menu.Items.Add(item);
        }
        menu.ShowAt(anchor);
    }

    private ContentDialog CompactContentDialog(string title, UIElement content, string primary, ContentDialogButton defaultButton)
    {
        var dialog = new ContentDialog
        {
            Title = title,
            Content = content,
            PrimaryButtonText = primary,
            CloseButtonText = "Cancel",
            DefaultButton = defaultButton,
            XamlRoot = ViewRoot,
            FontSize = Density.FontBody,
            CornerRadius = new CornerRadius(Density.CornerControl),
        };
        dialog.Resources["ContentDialogTitleFontSize"] = Density.FontSection;
        dialog.Resources["ContentDialogButtonHeight"] = Density.DialogButtonHeight;
        dialog.Resources["ContentDialogPadding"] = Density.DialogMargin;
        dialog.Resources["ContentDialogTopPadding"] = Density.SpaceLg;
        dialog.Resources["ContentDialogBottomPadding"] = Density.SpaceLg;
        dialog.Resources["ContentDialogLeftPadding"] = Density.SpaceLg;
        dialog.Resources["ContentDialogRightPadding"] = Density.SpaceLg;
        if (Density.TryFindStyle("CompactButtonStyle", out var btnStyle))
        {
            dialog.Resources["ContentDialogButtonStyle"] = btnStyle;
            dialog.Resources["AccentButtonStyle"] = btnStyle;
        }
        return dialog;
    }

    private async Task<string?> Ask(string title, string label, string value, string primary)
    {
        var box = Density.DialogField(value, label);
        var dialog = CompactContentDialog(
            title,
            new StackPanel
            {
                Spacing = Density.SpaceMd,
                Children =
                {
                    new TextBlock { Text = label, TextWrapping = TextWrapping.Wrap, FontSize = Density.FontBody },
                    box,
                },
            },
            primary,
            ContentDialogButton.Primary);
        _acceptText = false;
        box.KeyDown += (_, args) =>
        {
            if (args.Key == VirtualKey.Enter)
            {
                args.Handled = true;
                _acceptText = true;
                dialog.Hide();
            }
        };
        var result = await dialog.ShowAsync();
        if (result != ContentDialogResult.Primary && !_acceptText)
        {
            return null;
        }
        return box.Text;
    }

    private void LoadPrefs()
    {
        try
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            _sortDesc = values["aad.layers.sortDesc"] as bool? ?? false;
            _nameSort.Content = _sortDesc ? "Name  ▼" : "Name  ▲";
        }
        catch (Exception)
        {
            _nameSort.Content = "Name  ▲";
        }
    }

    private void SavePrefs()
    {
        try
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            values["aad.layers.sortDesc"] = _sortDesc;
        }
        catch (Exception)
        {
            // Preferences are optional. The drawing is unaffected.
        }
    }

    private static Dictionary<string, object> Flag(string name, string field, bool value) =>
        new() { ["name"] = name, [field] = value };

    /// <summary>The layers an action on this row applies to: the whole selection when the row is part of a multiple selection.</summary>
    private List<string> TargetsOf(LayerRecord row) =>
        _selected.Count > 1 && _selected.Contains(row.Name) ? _selected.ToList() : new List<string> { row.Name };

    /// <summary><c>layer.set</c> parameters for a property of this row: <c>names</c> for a multiple selection, <c>name</c> otherwise.</summary>
    private Dictionary<string, object> Params(LayerRecord row, string field, object value)
    {
        var targets = TargetsOf(row);
        var parameters = new Dictionary<string, object> { [field] = value };
        if (targets.Count > 1)
        {
            parameters["names"] = targets;
        }
        else
        {
            parameters["name"] = row.Name;
        }
        return parameters;
    }

    /// <summary>The layers the last <c>layer.set</c> with several names left alone (the current layer is not frozen).</summary>
    private List<string> ReadSkipped()
    {
        var skipped = new List<string>();
        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(_host));
            if (doc.RootElement.TryGetProperty("skipped", out var s) && s.ValueKind == JsonValueKind.Array)
            {
                skipped.AddRange(s.EnumerateArray().Select(x => x.GetString() ?? "").Where(x => x.Length > 0));
            }
        }
        catch (JsonException)
        {
        }
        return skipped;
    }

    private void MapSelectionToBuiltIn()
    {
        if (_selected.Count == 0)
        {
            Status.Text = "Select one or more layers.";
            return;
        }
        MapRequested?.Invoke(this, _selected.ToList());
    }

    private static LayerRecord? RecordOf(object sender)
    {
        if (sender is FrameworkElement element)
        {
            return element.DataContext as LayerRecord;
        }
        return null;
    }

    private static MenuFlyoutItem Item(string text, Action action, bool enabled)
    {
        var item = new MenuFlyoutItem { Text = text, IsEnabled = enabled };
        item.Click += (_, _) => action();
        return item;
    }
}

public sealed class LayerRecord
{
    public string Name { get; init; } = "";
    public bool On { get; init; }
    public bool Frozen { get; init; }
    public bool Locked { get; init; }
    public bool Plot { get; init; }
    public bool Current { get; init; }
    public bool Used { get; init; }
    public string ColorName { get; init; } = "";
    public string Linetype { get; init; } = "";
    public string Lineweight { get; init; } = "";
    public int Transparency { get; init; }
    public string Description { get; init; } = "";
    public bool NewVpFreeze { get; init; }
    public Brush Swatch { get; init; } = new SolidColorBrush(Microsoft.UI.Colors.Gray);

    public string StatusKind => Current ? "check" : Used ? "layers" : "layer-frozen";
    public string OnKind => On ? "bulb" : "bulb-off";
    public string FreezeKind => Frozen ? "snowflake" : "sun";
    public string LockKind => Locked ? "lock" : "unlock";
    public string PlotKind => Plot ? "print" : "print-off";
    public string NewVpKind => NewVpFreeze ? "snowflake" : "sun";
    public string NewVpAccess => (NewVpFreeze ? "Thaw " : "Freeze ") + Name + " in new viewports";

    public string CurrentLabel => Current ? "Current" : "Make current";
    public bool CanMakeCurrent => !Current && !Frozen && !Locked;
    public string CurrentAccess =>
        Current ? "Current layer " + Name
        : Frozen ? "Layer " + Name + " is frozen. Thaw it before making it current."
        : Locked ? "Layer " + Name + " is locked. Unlock it before making it current."
        : "Make " + Name + " the current layer";
    public string OnAccess => (On ? "Turn off layer " : "Turn on layer ") + Name;
    public string FreezeAccess => (Frozen ? "Thaw layer " : "Freeze layer ") + Name;
    public string LockAccess => (Locked ? "Unlock layer " : "Lock layer ") + Name;
    public string PlotAccess => (Plot ? "Do not plot layer " : "Plot layer ") + Name;
    public string NameAccess => "Layer " + Name;
    public string ColorAccess => "Layer color: " + ColorName;
    public string LinetypeAccess => "Linetype of layer " + Name + ": " + Linetype;
    public string LineweightAccess => "Lineweight of layer " + Name + ": " + Lineweight;
    public string TransparencyLabel => Transparency.ToString();
    public string TransparencyAccess => "Transparency of layer " + Name + ": " + Transparency + " percent";
    public string UsageLabel => Used ? "Used" : "Unused";
    public string UsageAccess => Used ? "Layer " + Name + " has objects" : "Layer " + Name + " has no objects";
    public string DescriptionAccess => "Description of layer " + Name;

    public static LayerRecord From(JsonElement layer)
    {
        var color = Text(layer, "colorRgb");
        return new LayerRecord
        {
            Name = Text(layer, "name"),
            On = Bool(layer, "on"),
            Frozen = Bool(layer, "frozen"),
            Locked = Bool(layer, "locked"),
            Plot = Bool(layer, "plot"),
            Current = Bool(layer, "current"),
            Used = Bool(layer, "used"),
            ColorName = Text(layer, "color"),
            Linetype = Text(layer, "linetype"),
            Lineweight = Text(layer, "lineweight"),
            Transparency = layer.TryGetProperty("transparency", out var transparency) && transparency.TryGetInt32(out var percent) ? percent : 0,
            Description = Text(layer, "description"),
            NewVpFreeze = Bool(layer, "newVpFreeze"),
            Swatch = SwatchOf(color),
        };
    }

    private static string Text(JsonElement layer, string name) =>
        layer.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() ?? "" : "";

    private static bool Bool(JsonElement layer, string name) =>
        layer.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.True;

    private static Brush SwatchOf(string hex)
    {
        if (hex.Length == 7 && hex[0] == '#' && Convert.FromHexString(hex.AsSpan(1)) is byte[] rgb && rgb.Length == 3)
        {
            return new SolidColorBrush(Windows.UI.Color.FromArgb(255, rgb[0], rgb[1], rgb[2]));
        }
        return new SolidColorBrush(Microsoft.UI.Colors.Gray);
    }
}

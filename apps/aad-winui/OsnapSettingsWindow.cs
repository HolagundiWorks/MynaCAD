using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>
/// Osnap settings pane (docs/osnap-settings.md): every running object snap, Select all / Clear all, aperture,
/// hatch objects and snap tracking. Each change runs the engine command <c>osnap.settings</c>; the pane shows
/// what the engine reports and the shell keeps the result in shell.json for the next start.
/// </summary>
internal sealed class OsnapSettingsWindow : Window
{
    private readonly IntPtr _host;
    private readonly Action _changed;
    private readonly CheckBox _on = new() { Content = "Object snap on (F3)", MinHeight = Density.ControlHeight };
    private readonly CheckBox _hatch = new() { Content = "Snap to hatch objects", MinHeight = Density.ControlHeight };
    private readonly CheckBox _tracking = new() { Content = "Object snap tracking (F11)", MinHeight = Density.ControlHeight };
    private readonly NumberBox _aperture = new() { Minimum = 1, Maximum = 50, SmallChange = 1, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact, MinHeight = Density.ControlHeight };
    private readonly Grid _grid = new() { ColumnSpacing = Density.SpaceLg, RowSpacing = Density.SpaceXs };
    private readonly Dictionary<uint, CheckBox> _boxes = new();
    private bool _filling;

    public OsnapSettingsWindow(FrameworkElement? themeSource, IntPtr host, Action changed)
    {
        _host = host;
        _changed = changed;
        _grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        _grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var all = Ui.Action("Select all");
        var none = Ui.Action("Clear all");
        all.Click += (_, _) => Apply(new { all = true });
        none.Click += (_, _) => Apply(new { none = true });
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceMd };
        buttons.Children.Add(all);
        buttons.Children.Add(none);
        Density.Name(_aperture, "Snap aperture in pixels");
        Density.Name(_grid, "Object snap modes");
        var stack = new StackPanel { Spacing = Density.SpaceSm, Margin = Density.DialogMargin };
        stack.Children.Add(DialogLayout.Heading("Object snap modes"));
        stack.Children.Add(_on);
        stack.Children.Add(_grid);
        stack.Children.Add(buttons);
        stack.Children.Add(DialogLayout.Heading("Snapping"));
        stack.Children.Add(Density.FieldRow("Aperture (pixels)", _aperture, "How close the cursor must be to an object to snap to it."));
        stack.Children.Add(_hatch);
        stack.Children.Add(_tracking);
        Density.Apply(stack);
        var theme = ShellChrome.Resolve(themeSource?.RequestedTheme ?? Density.Theme);
        Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { stack } } };
        Title = "Osnap Settings";
        StandardWindow.Present(this, null, WindowTemplate.PropertyDialog, "osnap");
        _on.Click += (_, _) => Apply(new { on = _on.IsChecked == true });
        _hatch.Click += (_, _) => Apply(new { hatch = _hatch.IsChecked == true });
        _tracking.Click += (_, _) => Apply(new { tracking = _tracking.IsChecked == true });
        _aperture.ValueChanged += (_, args) =>
        {
            if (!_filling && double.IsFinite(args.NewValue))
            {
                Apply(new { aperture = args.NewValue });
            }
        };
        Reload();
    }

    /// <summary>Show the engine's current osnap settings (also after a change made elsewhere).</summary>
    public void Reload() => Show(Query(new { }));

    private JsonElement? Query(object parameters)
    {
        if (_host == IntPtr.Zero || Native.Execute(_host, "osnap.settings", JsonSerializer.Serialize(parameters)) != 0)
        {
            return null;
        }
        try
        {
            return JsonDocument.Parse(Native.ReadResult(_host)).RootElement.Clone();
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private void Apply(object parameters)
    {
        if (_filling)
        {
            return;
        }
        var state = Query(parameters);
        Show(state);
        if (state is { } s)
        {
            OsnapPrefs.Save(s);
        }
        _changed();
    }

    private void Show(JsonElement? state)
    {
        if (state is not { ValueKind: JsonValueKind.Object } s)
        {
            return;
        }
        _filling = true;
        _on.IsChecked = s.TryGetProperty("on", out var on) && on.ValueKind == JsonValueKind.True;
        _hatch.IsChecked = s.TryGetProperty("hatch", out var h) && h.ValueKind == JsonValueKind.True;
        _tracking.IsChecked = s.TryGetProperty("tracking", out var t) && t.ValueKind == JsonValueKind.True;
        if (s.TryGetProperty("aperture", out var a) && a.TryGetDouble(out var px))
        {
            _aperture.Value = px;
        }
        if (_boxes.Count == 0 && s.TryGetProperty("modes", out var modes) && modes.ValueKind == JsonValueKind.Array)
        {
            var i = 0;
            foreach (var mode in modes.EnumerateArray())
            {
                if (!mode.TryGetProperty("bit", out var bit) || !bit.TryGetUInt32(out var mask))
                {
                    continue;
                }
                if (i % 2 == 0)
                {
                    _grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
                }
                var name = mode.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                var box = new CheckBox { Content = name, MinHeight = Density.ControlHeight };
                box.Click += (_, _) => Apply(new { modes = Enabled(mask, box.IsChecked == true) });
                _boxes[mask] = box;
                Grid.SetRow(box, i / 2);
                Grid.SetColumn(box, i % 2);
                _grid.Children.Add(box);
                i++;
            }
        }
        if (s.TryGetProperty("modes", out var current) && current.ValueKind == JsonValueKind.Array)
        {
            foreach (var mode in current.EnumerateArray())
            {
                if (mode.TryGetProperty("bit", out var bit) && bit.TryGetUInt32(out var mask) && _boxes.TryGetValue(mask, out var box))
                {
                    box.IsChecked = mode.TryGetProperty("enabled", out var e) && e.ValueKind == JsonValueKind.True;
                }
            }
        }
        _filling = false;
    }

    /// <summary>Mode names (the engine reads their first three letters) for the checked boxes after one change.</summary>
    private List<string> Enabled(uint changed, bool on)
    {
        var names = new List<string>();
        foreach (var (mask, box) in _boxes)
        {
            var checkedNow = mask == changed ? on : box.IsChecked == true;
            if (checkedNow && box.Content is string name)
            {
                names.Add(name);
            }
        }
        return names;
    }
}

/// <summary>Per-user osnap choices in shell.json (<c>osnap</c>): applied to each new session once the host exists.</summary>
internal static class OsnapPrefs
{
    public static void Save(JsonElement state)
    {
        var prefs = ShellPrefs.Load();
        var saved = new ShellPrefs.OsnapPref();
        if (state.TryGetProperty("osmode", out var m) && m.TryGetUInt32(out var osmode))
        {
            saved.Osmode = osmode;
        }
        if (state.TryGetProperty("aperture", out var a) && a.TryGetDouble(out var aperture) && double.IsFinite(aperture))
        {
            saved.Aperture = aperture;
        }
        saved.Hatch = state.TryGetProperty("hatch", out var h) && h.ValueKind == JsonValueKind.True;
        saved.Tracking = !state.TryGetProperty("tracking", out var t) || t.ValueKind != JsonValueKind.False;
        prefs.Osnap = saved;
        ShellPrefs.Save(prefs);
    }

    /// <summary>Apply the saved choices to a new session (no saved choices: the engine defaults stay).</summary>
    public static void Apply(IntPtr host)
    {
        var saved = ShellPrefs.Load().Osnap;
        if (host == IntPtr.Zero || saved is null)
        {
            return;
        }
        var json = JsonSerializer.Serialize(new { osmode = saved.Osmode, aperture = saved.Aperture, hatch = saved.Hatch, tracking = saved.Tracking, on = (saved.Osmode & 16384) == 0 });
        Native.Execute(host, "osnap.settings", json);
    }
}

using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Aad.WinUi;

/// <summary>Polar tracking, object snaps and dynamic input. Changes go through engine commands.</summary>
internal sealed class DraftingWindow : Window
{
    private static readonly (uint Bit, string Name)[] Modes =
    [
        (1, "Endpoint"), (2, "Midpoint"), (4, "Center"), (1024, "Geometric Center"),
        (8, "Node"), (16, "Quadrant"), (32, "Intersection"), (4096, "Extension"),
        (64, "Insertion"), (128, "Perpendicular"), (256, "Tangent"), (512, "Nearest"),
        (2048, "Apparent Intersection"), (8192, "Parallel"),
    ];

    private static readonly double[] Angles = [90, 45, 30, 22.5, 18, 15, 10, 5];

    private readonly IntPtr _host;
    private readonly Action _changed;
    private readonly ToggleButton _polar = new() { Content = "Polar tracking on (F10)", MinHeight = Density.ControlHeight };
    private readonly ToggleButton _snap = new() { Content = "Object snap on (F3)", MinHeight = Density.ControlHeight };
    private readonly ToggleButton _dyn = new() { Content = "Dynamic input (F12)", MinHeight = Density.ControlHeight };
    private readonly ToggleButton _ortho = new() { Content = "Ortho (F8)", MinHeight = Density.ControlHeight };
    private readonly ComboBox _angle = new() { MinWidth = 80, MinHeight = Density.ControlHeight };
    private readonly CommunityToolkit.WinUI.Controls.SegmentedItem _polarFormat = new() { Content = "Polar" };
    private readonly CommunityToolkit.WinUI.Controls.SegmentedItem _cartesian = new() { Content = "Cartesian" };
    private readonly CommunityToolkit.WinUI.Controls.SegmentedItem _relative = new() { Content = "Relative" };
    private readonly CommunityToolkit.WinUI.Controls.SegmentedItem _absolute = new() { Content = "Absolute" };
    private readonly CommunityToolkit.WinUI.Controls.Segmented _format = new() { SelectionMode = ListViewSelectionMode.Single, HorizontalAlignment = HorizontalAlignment.Left };
    private readonly CommunityToolkit.WinUI.Controls.Segmented _coords = new() { SelectionMode = ListViewSelectionMode.Single, HorizontalAlignment = HorizontalAlignment.Left };
    private readonly Dictionary<uint, CheckBox> _modes = new();
    private bool _filling;
    private uint _osmode;

    public DraftingWindow(FrameworkElement? themeSource, IntPtr host, Action changed)
    {
        _host = host;
        _changed = changed;
        var stack = new StackPanel { Spacing = Density.SpaceSm, Margin = Density.DialogMargin };
        stack.Children.Add(Heading("Polar tracking"));
        stack.Children.Add(_polar);
        var angleRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceMd };
        var angleLabel = Density.Secondary("Increment angle");
        angleLabel.VerticalAlignment = VerticalAlignment.Center;
        angleRow.Children.Add(angleLabel);
        foreach (var angle in Angles)
        {
            _angle.Items.Add(angle);
        }
        Density.Name(_angle, "Polar increment angle");
        angleRow.Children.Add(_angle);
        stack.Children.Add(angleRow);
        stack.Children.Add(Heading("Object snap"));
        stack.Children.Add(_snap);
        var grid = new Grid { ColumnSpacing = Density.SpaceLg, RowSpacing = Density.SpaceXs };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        for (var i = 0; i < Modes.Length; i++)
        {
            if (i % 2 == 0)
            {
                grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            }
            var (bit, name) = Modes[i];
            var box = new CheckBox { Content = name, MinHeight = Density.ControlHeight };
            var mode = bit;
            box.Click += (_, _) => SetMode(mode, box.IsChecked == true);
            _modes[bit] = box;
            Grid.SetRow(box, i / 2);
            Grid.SetColumn(box, i % 2);
            grid.Children.Add(box);
        }
        stack.Children.Add(grid);
        stack.Children.Add(Heading("Dynamic input"));
        stack.Children.Add(_dyn);
        _format.Items.Add(_polarFormat);
        _format.Items.Add(_cartesian);
        _coords.Items.Add(_relative);
        _coords.Items.Add(_absolute);
        Density.Name(_format, "Dynamic input format");
        Density.Name(_coords, "Dynamic input coordinates");
        stack.Children.Add(Density.FieldRow("Format", _format, "Polar: distance < angle. Cartesian: x, y."));
        stack.Children.Add(Density.FieldRow("Coordinates", _coords, "Relative to the last point, or absolute in the drawing."));
        stack.Children.Add(_ortho);
        Density.Apply(stack);
        var theme = ShellChrome.Resolve(themeSource?.RequestedTheme ?? Density.Theme);
        Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { stack } } };
        Title = "Drafting Settings";
        StandardWindow.Present(this, null, WindowTemplate.PropertyDialog, "drafting", new WindowBounds(400, 480, 720, 900));
        _polar.Click += (_, _) => Run("polar", new { on = _polar.IsChecked == true });
        _snap.Click += (_, _) => Run("osnap", new { on = _snap.IsChecked == true });
        _dyn.Click += (_, _) => Run("dynmode", new { on = _dyn.IsChecked == true });
        _ortho.Click += (_, _) => Run("ortho", new { on = _ortho.IsChecked == true });
        _angle.SelectionChanged += (_, _) =>
        {
            if (_filling || _angle.SelectedItem is not double degrees)
            {
                return;
            }
            Run("dsettings", new { polarang = degrees });
        };
        _format.SelectionChanged += (_, _) =>
        {
            if (!_filling && _format.SelectedItem != null)
            {
                RunVar("DYNPIFORMAT", ReferenceEquals(_format.SelectedItem, _cartesian));
            }
        };
        _coords.SelectionChanged += (_, _) =>
        {
            if (!_filling && _coords.SelectedItem != null)
            {
                RunVar("DYNPICOORDS", ReferenceEquals(_coords.SelectedItem, _absolute));
            }
        };
    }

    public void Reload(bool polar, bool snap, bool dyn, bool ortho, uint osmode, double polarang, bool cartesian, bool absolute)
    {
        _filling = true;
        _osmode = osmode;
        _polar.IsChecked = polar;
        _snap.IsChecked = snap;
        _dyn.IsChecked = dyn;
        _ortho.IsChecked = ortho;
        _format.SelectedItem = cartesian ? _cartesian : _polarFormat;
        _coords.SelectedItem = absolute ? _absolute : _relative;
        _format.IsEnabled = dyn;
        _coords.IsEnabled = dyn;
        foreach (var (bit, box) in _modes)
        {
            box.IsChecked = (osmode & bit) != 0;
        }
        var nearest = Angles.OrderBy(angle => Math.Abs(angle - polarang)).First();
        _angle.SelectedItem = nearest;
        _filling = false;
    }

    private void SetMode(uint bit, bool on)
    {
        if (_filling)
        {
            return;
        }
        var next = on ? _osmode | bit : _osmode & ~bit;
        Run("osnap", new { osmode = next });
    }

    private void Run(string command, object parameters)
    {
        if (_filling)
        {
            return;
        }
        if (Native.Execute(_host, command, JsonSerializer.Serialize(parameters)) != 0)
        {
            return;
        }
        _changed();
    }

    private void RunVar(string name, bool value) => Run("setvar", new { name, value });

    private static TextBlock Heading(string text)
    {
        var block = DialogLayout.Heading(text);
        block.Margin = new Thickness(0, Density.SpaceLg, 0, 0);
        return block;
    }
}

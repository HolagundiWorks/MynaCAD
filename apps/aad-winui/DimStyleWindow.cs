using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Aad.WinUi;

/// <summary>Dimension &gt; Dimension Style? ? list, set current, create, and edit common fields via <c>dimstyle</c>.</summary>
internal static class DimStyleWindow
{
    private static readonly (byte Value, string Label)[] LinearUnits =
    [
        (1, "Scientific"),
        (2, "Decimal"),
        (3, "Engineering"),
        (4, "Architectural"),
        (5, "Fractional"),
        (6, "Windows desktop"),
    ];

    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Dimension style status");

        var list = new ListBox { MinHeight = 96, MinWidth = Density.ManagerListWidth };
        Density.Name(list, "Dimension styles");
        var currentLabel = Density.Secondary("Current style: —", TextWrapping.Wrap);
        Density.Name(currentLabel, "Current dimension style");
        var setCurrent = ActionButton("Set Current");
        var newName = new TextBox { PlaceholderText = "New style name", MinHeight = Density.ControlHeight, Width = Density.FieldWidth };
        Density.Name(newName, "New style name");
        var create = ActionButton("New");
        var scale = FieldBox("Overall scale");
        var arrow = FieldBox("Arrow size");
        var textHeight = FieldBox("Text height");
        var textGap = FieldBox("Text gap");
        var decimals = FieldBox("Decimal places");
        var textStyle = new ComboBox { IsEditable = true, MinWidth = 160, MinHeight = Density.ControlHeight };
        Density.Name(textStyle, "Text style");
        var linearUnit = new ComboBox { MinWidth = 180, MinHeight = Density.ControlHeight };
        foreach (var (_, label) in LinearUnits)
        {
            linearUnit.Items.Add(label);
        }
        Density.Name(linearUnit, "Linear units");
        var propsHeading = Heading("Properties");

        var left = new StackPanel { Spacing = Density.SpaceSm, Width = Density.ManagerListWidth };
        left.Children.Add(Heading("Styles"));
        left.Children.Add(list);
        left.Children.Add(setCurrent);
        var newRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        newRow.Children.Add(newName);
        newRow.Children.Add(create);
        left.Children.Add(newRow);

        var right = new StackPanel { Spacing = Density.SpaceSm, MinWidth = 240 };
        right.Children.Add(propsHeading);
        right.Children.Add(Labeled("Overall scale", scale));
        right.Children.Add(Labeled("Arrow size", arrow));
        right.Children.Add(Labeled("Text height", textHeight));
        right.Children.Add(Labeled("Text gap", textGap));
        right.Children.Add(Labeled("Decimal places", decimals));
        right.Children.Add(Labeled("Text style", textStyle));
        right.Children.Add(Labeled("Linear units", linearUnit));

        var columns = new Grid { ColumnSpacing = Density.SpaceXl };
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(left, 0);
        Grid.SetColumn(right, 1);
        columns.Children.Add(left);
        columns.Children.Add(right);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 560 };
        root.Children.Add(currentLabel);
        root.Children.Add(columns);
        root.Children.Add(status);

        var state = new State();
        var loading = false;

        void Fail(string message) => status.Text = message;

        bool Run(string command, object body)
        {
            if (Native.Execute(host, command, JsonSerializer.Serialize(body, JsonOpts)) != 0)
            {
                Fail(Native.LastError(host));
                return false;
            }
            status.Text = "";
            changed();
            return true;
        }

        void LoadTextStyles()
        {
            if (Native.Execute(host, "style.list", "{}") != 0)
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                textStyle.Items.Clear();
                if (doc.RootElement.TryGetProperty("styles", out var styles) && styles.ValueKind == JsonValueKind.Array)
                {
                    foreach (var style in styles.EnumerateArray())
                    {
                        var name = style.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                        if (name.Length > 0)
                        {
                            textStyle.Items.Add(name);
                        }
                    }
                }
            }
            catch (JsonException)
            {
            }
        }

        void ShowProps(string? name)
        {
            loading = true;
            try
            {
                if (string.IsNullOrEmpty(name))
                {
                    propsHeading.Text = "Properties";
                    return;
                }
                propsHeading.Text = "Properties of " + name;
                if (!Run("dimstyle.list", new { name }))
                {
                    return;
                }
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (!doc.RootElement.TryGetProperty("style", out var style))
                {
                    Fail("The dimension style could not be read.");
                    return;
                }
                scale.Text = Num(style, "scale");
                arrow.Text = Num(style, "arrowSize");
                textHeight.Text = Num(style, "textHeight");
                textGap.Text = Num(style, "textGap");
                decimals.Text = style.TryGetProperty("decimals", out var dec) ? dec.ToString() : "0";
                var ts = style.TryGetProperty("textStyle", out var t) ? t.GetString() ?? "Standard" : "Standard";
                if (!textStyle.Items.Cast<object>().Any(item => string.Equals(item as string, ts, StringComparison.OrdinalIgnoreCase)))
                {
                    textStyle.Items.Insert(0, ts);
                }
                textStyle.Text = ts;
                var unit = style.TryGetProperty("linearUnit", out var u) && u.TryGetInt32(out var unitVal)
                    ? (byte)Math.Clamp(unitVal, 1, 6)
                    : (byte)2;
                var label = LinearUnits.FirstOrDefault(x => x.Value == unit).Label;
                if (string.IsNullOrEmpty(label))
                {
                    label = LinearUnits[1].Label;
                }
                linearUnit.SelectedItem = label;
            }
            catch (JsonException)
            {
                Fail("The dimension style could not be read.");
            }
            finally
            {
                loading = false;
            }
        }

        void Reload(string? prefer)
        {
            if (!Run("dimstyle.list", new { }))
            {
                return;
            }
            StyleList? payload;
            try
            {
                payload = JsonSerializer.Deserialize<StyleList>(Native.ReadResult(host), JsonOpts);
            }
            catch (JsonException)
            {
                Fail("The dimension style list could not be read.");
                return;
            }
            state.Current = payload?.Current ?? "Standard";
            state.Names = payload?.Styles ?? new List<string>();
            currentLabel.Text = "Current style: " + state.Current;
            LoadTextStyles();
            loading = true;
            try
            {
                list.Items.Clear();
                foreach (var name in state.Names)
                {
                    list.Items.Add(name);
                }
                var keep = prefer
                    ?? (list.SelectedItem as string)
                    ?? state.Names.FirstOrDefault(n => n.Equals(state.Current, StringComparison.OrdinalIgnoreCase))
                    ?? state.Names.FirstOrDefault();
                if (keep != null && state.Names.Any(n => n.Equals(keep, StringComparison.OrdinalIgnoreCase)))
                {
                    list.SelectedItem = state.Names.First(n => n.Equals(keep, StringComparison.OrdinalIgnoreCase));
                }
                else if (list.Items.Count > 0)
                {
                    list.SelectedIndex = 0;
                }
            }
            finally
            {
                loading = false;
            }
            ShowProps(list.SelectedItem as string);
        }

        void ApplyProps()
        {
            if (loading || list.SelectedItem is not string name)
            {
                return;
            }
            if (!TryNum(scale.Text, out var sc) || sc <= 0)
            {
                Fail("Overall scale must be a positive number.");
                return;
            }
            if (!TryNum(arrow.Text, out var asz) || asz < 0)
            {
                Fail("Arrow size must be zero or positive.");
                return;
            }
            if (!TryNum(textHeight.Text, out var th) || th <= 0)
            {
                Fail("Text height must be a positive number.");
                return;
            }
            if (!TryNum(textGap.Text, out var gap) || gap < 0)
            {
                Fail("Text gap must be zero or positive.");
                return;
            }
            if (!byte.TryParse((decimals.Text ?? "").Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var dec)
                && !byte.TryParse((decimals.Text ?? "").Trim(), NumberStyles.Integer, CultureInfo.CurrentCulture, out dec))
            {
                Fail("Decimal places must be 0?8.");
                return;
            }
            if (dec > 8)
            {
                Fail("Decimal places must be 0?8.");
                return;
            }
            var ts = (textStyle.Text ?? "").Trim();
            if (ts.Length == 0)
            {
                Fail("Text style is required.");
                return;
            }
            var unitLabel = linearUnit.SelectedItem as string ?? LinearUnits[1].Label;
            var unit = LinearUnits.FirstOrDefault(x => x.Label == unitLabel).Value;
            if (unit == 0)
            {
                unit = 2;
            }
            if (!Run("dimstyle", new
            {
                name,
                scale = sc,
                arrowSize = asz,
                textHeight = th,
                textGap = gap,
                decimals = dec,
                textStyle = ts,
                linearUnit = unit,
                current = false,
            }))
            {
                return;
            }
            Reload(name);
        }

        list.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                ShowProps(list.SelectedItem as string);
            }
        };
        setCurrent.Click += (_, _) =>
        {
            if (list.SelectedItem is not string name)
            {
                return;
            }
            if (Run("dimstyle", new { name }))
            {
                Reload(name);
            }
        };
        void CreateStyle()
        {
            var name = (newName.Text ?? "").Trim();
            if (name.Length == 0)
            {
                Fail("Enter a name for the new style.");
                return;
            }
            if (state.Names.Any(n => n.Equals(name, StringComparison.OrdinalIgnoreCase)))
            {
                Fail("A style with that name already exists.");
                return;
            }
            if (Run("dimstyle", new { name, current = false }))
            {
                newName.Text = "";
                Reload(name);
            }
        }
        create.Click += (_, _) => CreateStyle();
        newName.KeyDown += (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                args.Handled = true;
                CreateStyle();
            }
        };
        scale.LostFocus += (_, _) => ApplyProps();
        arrow.LostFocus += (_, _) => ApplyProps();
        textHeight.LostFocus += (_, _) => ApplyProps();
        textGap.LostFocus += (_, _) => ApplyProps();
        decimals.LostFocus += (_, _) => ApplyProps();
        textStyle.LostFocus += (_, _) => ApplyProps();
        linearUnit.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                ApplyProps();
            }
        };
        void OnEnter(object sender, KeyRoutedEventArgs args)
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                args.Handled = true;
                ApplyProps();
            }
        }
        scale.KeyDown += OnEnter;
        arrow.KeyDown += OnEnter;
        textHeight.KeyDown += OnEnter;
        textGap.KeyDown += OnEnter;
        decimals.KeyDown += OnEnter;

        Density.Apply(root);
        var window = new Window
        {
            Title = "Dimension Style",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "dimstyle", new WindowBounds(400, 240, 760, 800));
        Reload(null);
        return window;
    }

    private static Button ActionButton(string label) => DialogLayout.Action(label);

    private static TextBox FieldBox(string name) => DialogLayout.Field(name);

    private static FrameworkElement Labeled(string label, FrameworkElement field) => DialogLayout.Labeled(label, field);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);

    private static string Num(JsonElement style, string key) =>
        style.TryGetProperty(key, out var v) && v.TryGetDouble(out var n)
            ? n.ToString("0.####", CultureInfo.InvariantCulture)
            : "0";

    private static bool TryNum(string? text, out double value) =>
        double.TryParse((text ?? "").Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out value)
        || double.TryParse((text ?? "").Trim(), NumberStyles.Float, CultureInfo.CurrentCulture, out value);

    private sealed class State
    {
        public string Current { get; set; } = "Standard";
        public List<string> Names { get; set; } = new();
    }

    private sealed class StyleList
    {
        public string Current { get; set; } = "Standard";
        public List<string> Styles { get; set; } = new();
    }
}

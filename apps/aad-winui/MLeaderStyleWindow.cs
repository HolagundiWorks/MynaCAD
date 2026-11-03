using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Aad.WinUi;

/// <summary>Format &gt; Multileader Style? ? list, set current, create, edit via <c>mleaderstyle</c>.</summary>
internal static class MLeaderStyleWindow
{
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
        Density.Name(status, "Multileader style status");

        var list = new ListBox { MinHeight = 96, MinWidth = Density.ManagerListWidth };
        Density.Name(list, "Multileader styles");
        var currentLabel = Density.Secondary("Current style: —", TextWrapping.Wrap);
        Density.Name(currentLabel, "Current multileader style");
        var setCurrent = ActionButton("Set Current");
        var newName = new TextBox { PlaceholderText = "New style name", MinHeight = Density.ControlHeight, Width = Density.FieldWidth };
        Density.Name(newName, "New style name");
        var create = ActionButton("New");
        var arrow = Field("Arrowhead size");
        var textHeight = Field("Text height");
        var landingGap = Field("Landing gap");
        var dogleg = Field("Landing length");
        var textStyle = new ComboBox { IsEditable = true, MinWidth = 160, MinHeight = Density.ControlHeight };
        Density.Name(textStyle, "Text style");
        var propsHeading = Heading("Properties");

        var left = new StackPanel { Spacing = Density.SpaceSm, Width = Density.ManagerListWidth };
        left.Children.Add(Heading("Styles"));
        left.Children.Add(list);
        left.Children.Add(setCurrent);
        var newRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        newRow.Children.Add(newName);
        newRow.Children.Add(create);
        left.Children.Add(newRow);

        var right = new StackPanel { Spacing = Density.SpaceSm, MinWidth = 220 };
        right.Children.Add(propsHeading);
        right.Children.Add(Labeled("Arrowhead size", arrow));
        right.Children.Add(Labeled("Text height", textHeight));
        right.Children.Add(Labeled("Landing gap", landingGap));
        right.Children.Add(Labeled("Landing length", dogleg));
        right.Children.Add(Labeled("Text style", textStyle));

        var columns = new Grid { ColumnSpacing = Density.SpaceXl };
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(left, 0);
        Grid.SetColumn(right, 1);
        columns.Children.Add(left);
        columns.Children.Add(right);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 540 };
        root.Children.Add(currentLabel);
        root.Children.Add(columns);
        root.Children.Add(status);

        var state = new State();
        var loading = false;

        void Fail(string message) => status.Text = message;

        bool Run(object body)
        {
            if (Native.Execute(host, "mleaderstyle", JsonSerializer.Serialize(body, JsonOpts)) != 0)
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

        void Show(StyleRow? style)
        {
            loading = true;
            try
            {
                if (style == null)
                {
                    propsHeading.Text = "Properties";
                    return;
                }
                propsHeading.Text = "Properties of " + style.Name;
                arrow.Text = Fmt(style.ArrowSize);
                textHeight.Text = Fmt(style.TextHeight);
                landingGap.Text = Fmt(style.LandingGap);
                dogleg.Text = Fmt(style.Dogleg);
                if (!textStyle.Items.Cast<object>().Any(item => string.Equals(item as string, style.TextStyle, StringComparison.OrdinalIgnoreCase)))
                {
                    textStyle.Items.Insert(0, style.TextStyle);
                }
                textStyle.Text = style.TextStyle;
            }
            finally
            {
                loading = false;
            }
        }

        void Reload(string? prefer)
        {
            if (!Run(new { }))
            {
                return;
            }
            Payload? payload;
            try
            {
                payload = JsonSerializer.Deserialize<Payload>(Native.ReadResult(host), JsonOpts);
            }
            catch (JsonException)
            {
                Fail("The multileader styles could not be read.");
                return;
            }
            state.Current = payload?.Current ?? "Standard";
            state.Styles = payload?.Styles ?? new List<StyleRow>();
            if (state.Styles.Count == 0)
            {
                state.Styles.Add(new StyleRow { Name = "Standard" });
            }
            currentLabel.Text = "Current style: " + state.Current;
            LoadTextStyles();
            loading = true;
            try
            {
                list.Items.Clear();
                foreach (var style in state.Styles)
                {
                    list.Items.Add(style.Name);
                }
                var keep = prefer
                    ?? (list.SelectedItem as string)
                    ?? state.Styles.FirstOrDefault(s => s.Name.Equals(state.Current, StringComparison.OrdinalIgnoreCase))?.Name
                    ?? state.Styles.FirstOrDefault()?.Name;
                if (keep != null)
                {
                    list.SelectedItem = state.Styles.First(s => s.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)).Name;
                }
            }
            finally
            {
                loading = false;
            }
            Show(Selected());
        }

        StyleRow? Selected()
        {
            if (list.SelectedItem is not string name)
            {
                return null;
            }
            return state.Styles.FirstOrDefault(s => s.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
        }

        void Apply()
        {
            if (loading || Selected() is not { } style)
            {
                return;
            }
            if (!TryNum(arrow.Text, out var asz) || asz < 0
                || !TryNum(textHeight.Text, out var th) || th <= 0
                || !TryNum(landingGap.Text, out var gap) || gap < 0
                || !TryNum(dogleg.Text, out var dog) || dog < 0)
            {
                Fail("Check the numeric fields (text height must be positive).");
                return;
            }
            var ts = (textStyle.Text ?? "").Trim();
            if (ts.Length == 0)
            {
                Fail("Text style is required.");
                return;
            }
            if (!Run(new
            {
                name = style.Name,
                arrowSize = asz,
                textHeight = th,
                landingGap = gap,
                dogleg = dog,
                textStyle = ts,
                current = false,
            }))
            {
                return;
            }
            Reload(style.Name);
        }

        list.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                Show(Selected());
            }
        };
        setCurrent.Click += (_, _) =>
        {
            if (Selected() is not { } style)
            {
                return;
            }
            if (Run(new { name = style.Name }))
            {
                Reload(style.Name);
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
            if (Run(new { name, current = false }))
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
        foreach (var box in new[] { arrow, textHeight, landingGap, dogleg })
        {
            box.LostFocus += (_, _) => Apply();
            box.KeyDown += (_, args) =>
            {
                if (args.Key == Windows.System.VirtualKey.Enter)
                {
                    args.Handled = true;
                    Apply();
                }
            };
        }
        textStyle.LostFocus += (_, _) => Apply();

        Density.Apply(root);
        var window = new Window
        {
            Title = "Multileader Style",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "mleaderstyle", new WindowBounds(400, 240, 760, 800));
        Reload(null);
        return window;
    }

    private static Button ActionButton(string label) => DialogLayout.Action(label);

    private static TextBox Field(string name) => DialogLayout.Field(name);

    private static FrameworkElement Labeled(string label, FrameworkElement field) => DialogLayout.Labeled(label, field);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);

    private static string Fmt(double value) => value.ToString("0.####", CultureInfo.InvariantCulture);

    private static bool TryNum(string? text, out double value) =>
        double.TryParse((text ?? "").Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out value)
        || double.TryParse((text ?? "").Trim(), NumberStyles.Float, CultureInfo.CurrentCulture, out value);

    private sealed class State
    {
        public string Current { get; set; } = "Standard";
        public List<StyleRow> Styles { get; set; } = new();
    }

    private sealed class Payload
    {
        public string Current { get; set; } = "Standard";
        public List<StyleRow> Styles { get; set; } = new();
    }

    private sealed class StyleRow
    {
        public string Name { get; set; } = "Standard";
        public double ArrowSize { get; set; } = 0.18;
        public double TextHeight { get; set; } = 0.18;
        public double LandingGap { get; set; } = 0.09;
        public double Dogleg { get; set; } = 0.36;
        public string TextStyle { get; set; } = "Standard";
    }
}

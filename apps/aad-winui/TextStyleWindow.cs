using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
namespace Aad.WinUi;

/// <summary>Format &gt; Text Style? ? list, set current, create, and edit styles via <c>style</c> / <c>style.list</c>.</summary>
internal static class TextStyleWindow
{
    private static readonly string[] BaseFonts =
    [
        "AAD Stroke",
        "Segoe UI",
        "Arial",
        "Calibri",
        "Consolas",
        "Courier New",
        "Times New Roman",
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
        Density.Name(status, "Text style status");

        var list = new ListBox { MinHeight = 96, MinWidth = Density.ManagerListWidth };
        Density.Name(list, "Text styles");
        var currentLabel = Density.Secondary("Current style: —", TextWrapping.Wrap);
        Density.Name(currentLabel, "Current text style");
        var setCurrent = ActionButton("Set Current");
        var newName = new TextBox { PlaceholderText = "New style name", MinHeight = Density.ControlHeight, Width = Density.FieldWidth };
        Density.Name(newName, "New style name");
        var create = ActionButton("New");
        var font = new ComboBox { IsEditable = true, MinWidth = 200, MinHeight = Density.ControlHeight };
        Density.Name(font, "Font");
        var height = FieldBox("Height (0: ask each time)");
        var width = FieldBox("Width factor");
        var oblique = FieldBox("Oblique angle (degrees)");
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
        right.Children.Add(Labeled("Font", font));
        right.Children.Add(Labeled("Height", height));
        right.Children.Add(Labeled("Width factor", width));
        right.Children.Add(Labeled("Oblique (?)", oblique));

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

        string? ExecuteJson(string command, object body)
        {
            if (Native.Execute(host, command, JsonSerializer.Serialize(body, JsonOpts)) != 0)
            {
                Fail(Native.LastError(host));
                return null;
            }
            var json = Native.ReadResult(host);
            status.Text = "";
            return json;
        }

        bool Run(string command, object body)
        {
            if (ExecuteJson(command, body) is null)
            {
                return false;
            }
            changed();
            return true;
        }

        void FillFonts(IEnumerable<string> fromStyles)
        {
            var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            font.Items.Clear();
            foreach (var name in BaseFonts.Concat(fromStyles))
            {
                if (string.IsNullOrWhiteSpace(name) || !names.Add(name))
                {
                    continue;
                }
                font.Items.Add(name);
            }
        }

        void ShowProps(StyleRow? style)
        {
            loading = true;
            try
            {
                if (style == null)
                {
                    propsHeading.Text = "Properties";
                    font.Text = "";
                    height.Text = "";
                    width.Text = "";
                    oblique.Text = "";
                    return;
                }
                propsHeading.Text = "Properties of " + style.Name;
                var shown = string.IsNullOrWhiteSpace(style.Font) ? "AAD Stroke" : style.Font;
                if (!font.Items.Cast<object>().Any(item => string.Equals(item as string, shown, StringComparison.OrdinalIgnoreCase)))
                {
                    font.Items.Insert(0, shown);
                }
                font.Text = shown;
                height.Text = Fmt(style.Height);
                width.Text = Fmt(style.WidthFactor);
                oblique.Text = Fmt(style.Oblique);
            }
            finally
            {
                loading = false;
            }
        }

        void Reload(string? prefer)
        {
            var listJson = ExecuteJson("style.list", new { });
            if (listJson is null)
            {
                return;
            }
            StyleList? payload;
            try
            {
                payload = JsonSerializer.Deserialize<StyleList>(listJson, JsonOpts);
            }
            catch (JsonException)
            {
                Fail("The text style list could not be read.");
                return;
            }
            state.Current = payload?.Current ?? "Standard";
            state.Styles = payload?.Styles ?? new List<StyleRow>();
            currentLabel.Text = "Current style: " + state.Current;
            FillFonts(state.Styles.Select(s => s.Font));
            var keep = prefer
                ?? (list.SelectedItem as string)
                ?? state.Styles.FirstOrDefault(s => s.Name.Equals(state.Current, StringComparison.OrdinalIgnoreCase))?.Name
                ?? state.Styles.FirstOrDefault()?.Name;
            loading = true;
            try
            {
                list.Items.Clear();
                foreach (var style in state.Styles)
                {
                    list.Items.Add(style.Name);
                }
                if (keep != null && state.Styles.Any(s => s.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)))
                {
                    list.SelectedItem = state.Styles.First(s => s.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)).Name;
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
            ShowProps(SelectedStyle());
            changed();
        }

        StyleRow? SelectedStyle()
        {
            if (list.SelectedItem is not string name)
            {
                return null;
            }
            return state.Styles.FirstOrDefault(s => s.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
        }

        void ApplyProps()
        {
            if (loading || SelectedStyle() is not { } style)
            {
                return;
            }
            if (!TryNum(height.Text, out var h) || h < 0)
            {
                Fail("Height must be zero or a positive number.");
                return;
            }
            if (!TryNum(width.Text, out var w) || w <= 0 || w > 100)
            {
                Fail("Width factor must be greater than 0 and at most 100.");
                return;
            }
            if (!TryNum(oblique.Text, out var o) || Math.Abs(o) > 85)
            {
                Fail("Oblique angle must be between -85 and 85 degrees.");
                return;
            }
            var fontName = (font.Text ?? "").Trim();
            if (fontName.Length == 0)
            {
                Fail("Font is required.");
                return;
            }
            if (!Run("style", new
            {
                name = style.Name,
                font = fontName,
                height = h,
                widthFactor = w,
                oblique = o,
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
                ShowProps(SelectedStyle());
            }
        };
        setCurrent.Click += (_, _) =>
        {
            if (SelectedStyle() is not { } style)
            {
                return;
            }
            if (Run("style", new { name = style.Name }))
            {
                Reload(style.Name);
            }
        };
        font.LostFocus += (_, _) => ApplyProps();
        font.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                ApplyProps();
            }
        };
        height.LostFocus += (_, _) => ApplyProps();
        width.LostFocus += (_, _) => ApplyProps();
        oblique.LostFocus += (_, _) => ApplyProps();
        void OnEnter(object sender, KeyRoutedEventArgs args)
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                args.Handled = true;
                ApplyProps();
            }
        }
        height.KeyDown += OnEnter;
        width.KeyDown += OnEnter;
        oblique.KeyDown += OnEnter;
        void CreateStyle()
        {
            var name = (newName.Text ?? "").Trim();
            if (name.Length == 0)
            {
                Fail("Enter a name for the new style.");
                return;
            }
            if (state.Styles.Any(s => s.Name.Equals(name, StringComparison.OrdinalIgnoreCase)))
            {
                Fail("A style with that name already exists.");
                return;
            }
            if (Run("style", new { name, current = false }))
            {
                newName.Text = "";
                Reload(name);
            }
        }
        create.Click += (_, _) => CreateStyle();
        newName.KeyDown += (_, args) =>
        {
            if (args.Key != Windows.System.VirtualKey.Enter)
            {
                return;
            }
            args.Handled = true;
            CreateStyle();
        };

        Density.Apply(root);
        var window = new Window
        {
            Title = "Text Style",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "textstyle", new WindowBounds(400, 240, 760, 800));
        Reload(null);
        return window;
    }

    private static Button ActionButton(string label) => DialogLayout.Action(label);

    private static TextBox FieldBox(string name) => DialogLayout.Field(name);

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

    private sealed class StyleList
    {
        public string Current { get; set; } = "Standard";
        public List<StyleRow> Styles { get; set; } = new();
    }

    private sealed class StyleRow
    {
        public string Name { get; set; } = "";
        public string Font { get; set; } = "";
        public double Height { get; set; }
        public double WidthFactor { get; set; } = 1;
        public double Oblique { get; set; }
    }
}

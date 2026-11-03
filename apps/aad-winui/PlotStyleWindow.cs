using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Aad.WinUi;

/// <summary>File &gt; Plot Styles? ? tables and style rows via <c>stylesmanager</c> / <c>pagesetup</c>.</summary>
internal static class PlotStyleWindow
{
    private static readonly double[] LineweightsMm = [0.0, 0.09, 0.13, 0.18, 0.25, 0.3, 0.35, 0.5, 0.7, 1.0, 1.4, 2.11];

    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    public static Window Open(Window owner, IntPtr host, string currentSpace, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Plot style status");

        var tables = new ComboBox { MinWidth = 220, MinHeight = Density.ControlHeight };
        Density.Name(tables, "Plot style table");
        var copy = ActionButton("Copy");
        var newCtb = ActionButton("New CTB");
        var newStb = ActionButton("New STB");
        var delete = ActionButton("Delete");
        var useLayout = ActionButton("Use for this layout");
        var displayStyles = new CheckBox { Content = "Display plot styles", MinHeight = Density.ControlHeight };
        Density.Name(displayStyles, "Display plot styles");
        var layoutLabel = Density.Secondary("", TextWrapping.Wrap);
        Density.Name(layoutLabel, "Layout plot style");
        var desc = Density.Secondary("", TextWrapping.Wrap);
        Density.Name(desc, "Table description");
        var styles = new ListBox { MinHeight = 140, MinWidth = 180 };
        Density.Name(styles, "Plot styles");
        var objectColor = new CheckBox { Content = "Object colour", MinHeight = Density.ControlHeight };
        Density.Name(objectColor, "Object colour");
        var grayscale = new CheckBox { Content = "Grayscale", MinHeight = Density.ControlHeight };
        Density.Name(grayscale, "Grayscale");
        var screening = new TextBox { MinHeight = Density.ControlHeight, Width = 80 };
        Density.Name(screening, "Screening percent");
        var lineweight = new ComboBox { MinWidth = 180, MinHeight = Density.ControlHeight };
        lineweight.Items.Add("Use object lineweight");
        foreach (var mm in LineweightsMm)
        {
            lineweight.Items.Add(mm.ToString("0.##", CultureInfo.InvariantCulture) + " mm");
        }
        Density.Name(lineweight, "Lineweight");
        var apply = ActionButton("Apply style");
        var propsHeading = Heading("Selected style");

        var toolbar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        toolbar.Children.Add(tables);
        toolbar.Children.Add(copy);
        toolbar.Children.Add(newCtb);
        toolbar.Children.Add(newStb);
        toolbar.Children.Add(delete);

        var layoutRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceMd };
        layoutRow.Children.Add(layoutLabel);
        layoutRow.Children.Add(useLayout);
        layoutRow.Children.Add(displayStyles);

        var left = new StackPanel { Spacing = Density.SpaceSm, Width = Density.ManagerListWidth };
        left.Children.Add(Heading("Styles"));
        left.Children.Add(styles);

        var right = new StackPanel { Spacing = Density.SpaceSm, MinWidth = 240 };
        right.Children.Add(propsHeading);
        right.Children.Add(objectColor);
        right.Children.Add(grayscale);
        right.Children.Add(Labeled("Screening %", screening));
        right.Children.Add(Labeled("Lineweight", lineweight));
        right.Children.Add(apply);

        var columns = new Grid { ColumnSpacing = Density.SpaceXl };
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(left, 0);
        Grid.SetColumn(right, 1);
        columns.Children.Add(left);
        columns.Children.Add(right);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 720 };
        root.Children.Add(Labeled("Plot style table", toolbar));
        root.Children.Add(layoutRow);
        root.Children.Add(desc);
        root.Children.Add(columns);
        root.Children.Add(status);

        var state = new State { Space = currentSpace };
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

        string? SelectedTable() => tables.SelectedItem as string;

        StyleRow? SelectedStyle()
        {
            if (styles.SelectedItem is not string name)
            {
                return null;
            }
            return state.Styles.FirstOrDefault(s => s.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
        }

        void ShowStyle(StyleRow? style)
        {
            loading = true;
            try
            {
                if (style == null)
                {
                    propsHeading.Text = "Selected style";
                    return;
                }
                propsHeading.Text = style.Name;
                objectColor.IsChecked = string.IsNullOrEmpty(style.Color);
                grayscale.IsChecked = style.Grayscale;
                screening.Text = style.Screening.ToString(CultureInfo.InvariantCulture);
                if (style.Lineweight is null)
                {
                    lineweight.SelectedIndex = 0;
                }
                else
                {
                    var mm = style.Lineweight.Value / 100.0;
                    var label = mm.ToString("0.##", CultureInfo.InvariantCulture) + " mm";
                    lineweight.SelectedItem = lineweight.Items.Cast<object>().FirstOrDefault(item => item as string == label) ?? lineweight.Items[0];
                }
            }
            finally
            {
                loading = false;
            }
        }

        void LoadLayoutChrome()
        {
            var paper = !state.Space.Equals("Model", StringComparison.OrdinalIgnoreCase);
            useLayout.IsEnabled = paper;
            displayStyles.IsEnabled = paper;
            if (!paper)
            {
                layoutLabel.Text = "Model space has no layout plot style table.";
                return;
            }
            if (!Run("pagesetup", new { layout = state.Space }))
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                var page = doc.RootElement.GetProperty("page");
                var table = page.TryGetProperty("plotStyleTable", out var t) ? t.GetString() ?? "" : "";
                state.LayoutTable = table;
                var show = page.TryGetProperty("showPlotStyles", out var s) && s.ValueKind == JsonValueKind.True;
                loading = true;
                displayStyles.IsChecked = show;
                loading = false;
                layoutLabel.Text = "Layout " + state.Space + ": " + (table.Length == 0 ? "None" : table);
            }
            catch (JsonException)
            {
                layoutLabel.Text = "Layout " + state.Space;
            }
        }

        void LoadTable(string? preferStyle)
        {
            var name = SelectedTable();
            if (string.IsNullOrEmpty(name))
            {
                state.Styles.Clear();
                styles.Items.Clear();
                return;
            }
            if (!Run("stylesmanager", new { action = "get", name }))
            {
                return;
            }
            TableDetail? detail;
            try
            {
                detail = JsonSerializer.Deserialize<TableDetail>(Native.ReadResult(host), JsonOpts);
            }
            catch (JsonException)
            {
                Fail("The plot style table could not be read.");
                return;
            }
            state.InDrawing = detail?.InDrawing == true;
            state.Named = string.Equals(detail?.Kind, "named", StringComparison.OrdinalIgnoreCase);
            desc.Text = detail?.Description ?? "";
            if (!state.InDrawing)
            {
                desc.Text = (desc.Text.Length == 0 ? "" : desc.Text + " — ") + "Built-in table: editing keeps a copy in the drawing.";
            }
            delete.IsEnabled = state.InDrawing;
            state.Styles = detail?.Styles ?? new List<StyleRow>();
            loading = true;
            try
            {
                styles.Items.Clear();
                foreach (var style in state.Styles)
                {
                    styles.Items.Add(style.Name);
                }
                var keep = preferStyle
                    ?? (styles.SelectedItem as string)
                    ?? state.Styles.FirstOrDefault()?.Name;
                if (keep != null && state.Styles.Any(s => s.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)))
                {
                    styles.SelectedItem = state.Styles.First(s => s.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)).Name;
                }
                else if (styles.Items.Count > 0)
                {
                    styles.SelectedIndex = 0;
                }
            }
            finally
            {
                loading = false;
            }
            ShowStyle(SelectedStyle());
        }

        void Reload(string? preferTable, string? preferStyle)
        {
            if (!Run("stylesmanager", new { action = "list" }))
            {
                return;
            }
            TableList? list;
            try
            {
                list = JsonSerializer.Deserialize<TableList>(Native.ReadResult(host), JsonOpts);
            }
            catch (JsonException)
            {
                Fail("The plot style tables could not be read.");
                return;
            }
            var names = (list?.Tables ?? new List<TableSummary>()).Select(t => t.Name).Where(n => !string.IsNullOrEmpty(n)).ToList();
            loading = true;
            try
            {
                tables.Items.Clear();
                foreach (var name in names)
                {
                    tables.Items.Add(name);
                }
                var keep = preferTable
                    ?? (tables.SelectedItem as string)
                    ?? state.LayoutTable
                    ?? names.FirstOrDefault();
                if (keep != null && names.Any(n => n.Equals(keep, StringComparison.OrdinalIgnoreCase)))
                {
                    tables.SelectedItem = names.First(n => n.Equals(keep, StringComparison.OrdinalIgnoreCase));
                }
                else if (tables.Items.Count > 0)
                {
                    tables.SelectedIndex = 0;
                }
            }
            finally
            {
                loading = false;
            }
            LoadLayoutChrome();
            LoadTable(preferStyle);
        }

        void ApplyStyle()
        {
            if (loading || SelectedTable() is not string table || SelectedStyle() is not { } style)
            {
                return;
            }
            if (!byte.TryParse((screening.Text ?? "").Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var screen)
                && !byte.TryParse((screening.Text ?? "").Trim(), NumberStyles.Integer, CultureInfo.CurrentCulture, out screen))
            {
                Fail("Screening must be 0?100.");
                return;
            }
            if (screen > 100)
            {
                Fail("Screening must be 0?100.");
                return;
            }
            object? color = objectColor.IsChecked == true ? null : (object)(string.IsNullOrEmpty(style.Color) ? "#000000" : style.Color);
            object? lw = null;
            var lwText = lineweight.SelectedItem as string ?? "";
            if (lwText != "Use object lineweight" && lwText.EndsWith(" mm", StringComparison.Ordinal))
            {
                var num = lwText[..^3].Trim();
                if (double.TryParse(num, NumberStyles.Float, CultureInfo.InvariantCulture, out var mm))
                {
                    lw = mm;
                }
            }
            if (!Run("stylesmanager", new
            {
                action = "set",
                name = table,
                style = new
                {
                    name = style.Name,
                    color,
                    grayscale = grayscale.IsChecked == true,
                    screening = screen,
                    lineweight = lw,
                },
            }))
            {
                return;
            }
            Reload(table, style.Name);
        }

        tables.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                LoadTable(null);
            }
        };
        styles.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                ShowStyle(SelectedStyle());
            }
        };
        copy.Click += (_, _) =>
        {
            if (SelectedTable() is not string from)
            {
                return;
            }
            var stem = from.Contains('.') ? from[..from.LastIndexOf('.')] : from;
            if (!Run("stylesmanager", new { action = "new", name = stem + "-copy", from }))
            {
                return;
            }
            var created = "";
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                created = doc.RootElement.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
            }
            catch (JsonException)
            {
            }
            Reload(created.Length > 0 ? created : null, null);
        };
        newCtb.Click += (_, _) =>
        {
            if (Run("stylesmanager", new { action = "new", name = "new.ctb" }))
            {
                Reload("new.ctb", null);
            }
        };
        newStb.Click += (_, _) =>
        {
            if (Run("stylesmanager", new { action = "new", name = "new.stb" }))
            {
                Reload("new.stb", null);
            }
        };
        delete.Click += (_, _) =>
        {
            if (SelectedTable() is not string name)
            {
                return;
            }
            if (Run("stylesmanager", new { action = "delete", name }))
            {
                Reload(null, null);
            }
        };
        useLayout.Click += (_, _) =>
        {
            if (SelectedTable() is not string name || state.Space.Equals("Model", StringComparison.OrdinalIgnoreCase))
            {
                return;
            }
            if (Run("pagesetup", new { layout = state.Space, plotStyleTable = name }))
            {
                LoadLayoutChrome();
            }
        };
        displayStyles.Checked += (_, _) =>
        {
            if (!loading && !state.Space.Equals("Model", StringComparison.OrdinalIgnoreCase))
            {
                Run("pagesetup", new { layout = state.Space, displayPlotStyles = true });
            }
        };
        displayStyles.Unchecked += (_, _) =>
        {
            if (!loading && !state.Space.Equals("Model", StringComparison.OrdinalIgnoreCase))
            {
                Run("pagesetup", new { layout = state.Space, displayPlotStyles = false });
            }
        };
        apply.Click += (_, _) => ApplyStyle();
        screening.KeyDown += (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                args.Handled = true;
                ApplyStyle();
            }
        };

        Density.Apply(root);
        var window = new Window
        {
            Title = "Plot Style Manager",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "plotstyle");
        Reload(null, null);
        return window;
    }

    private static Button ActionButton(string label) => DialogLayout.Action(label);

    private static FrameworkElement Labeled(string label, FrameworkElement field) => DialogLayout.Labeled(label, field);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);

    private sealed class State
    {
        public string Space { get; set; } = "Model";
        public string? LayoutTable { get; set; }
        public bool InDrawing { get; set; }
        public bool Named { get; set; }
        public List<StyleRow> Styles { get; set; } = new();
    }

    private sealed class TableList
    {
        public List<TableSummary> Tables { get; set; } = new();
    }

    private sealed class TableSummary
    {
        public string Name { get; set; } = "";
    }

    private sealed class TableDetail
    {
        public string Name { get; set; } = "";
        public string Description { get; set; } = "";
        public string Kind { get; set; } = "";
        public bool InDrawing { get; set; }
        public List<StyleRow> Styles { get; set; } = new();
    }

    private sealed class StyleRow
    {
        public string Name { get; set; } = "";
        public string? Color { get; set; }
        public bool Grayscale { get; set; }
        public byte Screening { get; set; } = 100;
        public ushort? Lineweight { get; set; }
    }
}

using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Aad.WinUi;

/// <summary>File &gt; Page Setup Manager… — edit a layout's page setup via <c>pagesetup</c>.</summary>
internal static class PageSetupWindow
{
    private static readonly string[] PlotAreas = ["layout", "extents", "display", "limits"];

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
        Density.Name(status, "Page setup status");

        var list = new ListBox { MinHeight = 110, MinWidth = Density.ManagerListWidth };
        Density.Name(list, "Layouts");
        var paper = new ComboBox { MinWidth = 200, MinHeight = Density.ControlHeight };
        Density.Name(paper, "Paper size");
        var portrait = new CommunityToolkit.WinUI.Controls.SegmentedItem { Content = "Portrait" };
        var landscape = new CommunityToolkit.WinUI.Controls.SegmentedItem { Content = "Landscape" };
        var orient = new CommunityToolkit.WinUI.Controls.Segmented { SelectionMode = ListViewSelectionMode.Single, HorizontalAlignment = HorizontalAlignment.Left };
        orient.Items.Add(portrait);
        orient.Items.Add(landscape);
        Density.Name(orient, "Orientation");
        var plotArea = new ComboBox { MinWidth = 160, MinHeight = Density.ControlHeight };
        foreach (var area in PlotAreas)
        {
            plotArea.Items.Add(area);
        }
        Density.Name(plotArea, "Plot area");
        var fit = new CheckBox { Content = "Fit to paper", MinHeight = Density.ControlHeight, IsChecked = true };
        Density.Name(fit, "Fit to paper");
        var scale = new TextBox { MinHeight = Density.ControlHeight, Width = Density.FieldWidth };
        Density.Name(scale, "Scale");
        var nearest = Density.Secondary("", TextWrapping.Wrap);
        Density.Name(nearest, "Nearest scale");
        var center = new CheckBox { Content = "Center the plot", MinHeight = Density.ControlHeight };
        Density.Name(center, "Center the plot");
        var lineweights = new CheckBox { Content = "Plot object lineweights", MinHeight = Density.ControlHeight };
        Density.Name(lineweights, "Plot object lineweights");
        var sizeLabel = Density.Secondary("", TextWrapping.Wrap);
        Density.Name(sizeLabel, "Paper dimensions");
        var propsHeading = Heading("Page setup");

        var left = new StackPanel { Spacing = Density.SpaceSm, Width = Density.ManagerListWidth };
        left.Children.Add(Heading("Layouts"));
        left.Children.Add(list);

        var right = new StackPanel { Spacing = Density.SpaceSm, MinWidth = 260 };
        right.Children.Add(propsHeading);
        right.Children.Add(Labeled("Paper size", paper));
        right.Children.Add(Labeled("Orientation", orient));
        right.Children.Add(Labeled("Plot area", plotArea));
        right.Children.Add(fit);
        right.Children.Add(Labeled("Scale", scale));
        right.Children.Add(nearest);
        right.Children.Add(center);
        right.Children.Add(lineweights);
        right.Children.Add(sizeLabel);

        var columns = new Grid { ColumnSpacing = Density.SpaceXl };
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(left, 0);
        Grid.SetColumn(right, 1);
        columns.Children.Add(left);
        columns.Children.Add(right);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 560 };
        root.Children.Add(columns);
        root.Children.Add(status);

        var state = new State();
        var loading = false;

        void Fail(string message) => status.Text = message;

        bool Run(object body)
        {
            if (Native.Execute(host, "pagesetup", JsonSerializer.Serialize(body, JsonOpts)) != 0)
            {
                Fail(Native.LastError(host));
                return false;
            }
            status.Text = "";
            changed();
            return true;
        }

        void ShowPage(LayoutRow? layout)
        {
            loading = true;
            try
            {
                if (layout?.Page == null)
                {
                    propsHeading.Text = "Page setup";
                    nearest.Text = "";
                    return;
                }
                propsHeading.Text = "Page setup of " + layout.Name;
                var page = layout.Page;
                if (!string.IsNullOrEmpty(page.Paper) && !paper.Items.Cast<object>().Any(item => string.Equals(item as string, page.Paper, StringComparison.OrdinalIgnoreCase)))
                {
                    paper.Items.Insert(0, page.Paper);
                }
                paper.SelectedItem = paper.Items.Cast<object>().FirstOrDefault(item => string.Equals(item as string, page.Paper, StringComparison.OrdinalIgnoreCase))
                    ?? page.Paper;
                orient.SelectedItem = page.Landscape ? landscape : portrait;
                plotArea.SelectedItem = PlotAreas.FirstOrDefault(a => a.Equals(page.PlotArea, StringComparison.OrdinalIgnoreCase)) ?? "layout";
                fit.IsChecked = page.ScaleToFit;
                scale.IsEnabled = !page.ScaleToFit;
                if (page.ScaleToFit && layout.Fit != null && !string.IsNullOrEmpty(layout.Fit.Nearest))
                {
                    scale.Text = layout.Fit.Nearest;
                    nearest.Text = string.IsNullOrEmpty(layout.Fit.Label) || layout.Fit.Label == layout.Fit.Nearest
                        ? "Nearest scale that fits: " + layout.Fit.Nearest
                        : "Fits at " + layout.Fit.Label + " · nearest: " + layout.Fit.Nearest;
                }
                else
                {
                    scale.Text = page.Scale.ToString("0.####", CultureInfo.InvariantCulture);
                    nearest.Text = layout.Fit != null && !string.IsNullOrEmpty(layout.Fit.Nearest)
                        ? "Nearest scale that fits: " + layout.Fit.Nearest
                        : "";
                }
                center.IsChecked = page.Center;
                lineweights.IsChecked = page.Lineweights;
                var w = page.Landscape ? page.HeightMm : page.WidthMm;
                var h = page.Landscape ? page.WidthMm : page.HeightMm;
                sizeLabel.Text = w.ToString("0.#", CultureInfo.InvariantCulture) + " × " + h.ToString("0.#", CultureInfo.InvariantCulture) + " mm";
            }
            finally
            {
                loading = false;
            }
        }

        LayoutRow? Selected()
        {
            if (list.SelectedItem is not string name)
            {
                return null;
            }
            return state.Layouts.FirstOrDefault(l => l.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
        }

        void Reload(string? prefer)
        {
            if (!Run(new { }))
            {
                return;
            }
            Report? report;
            try
            {
                report = JsonSerializer.Deserialize<Report>(Native.ReadResult(host), JsonOpts);
            }
            catch (JsonException)
            {
                Fail("The page setups could not be read.");
                return;
            }
            state.Layouts = report?.Layouts ?? new List<LayoutRow>();
            loading = true;
            try
            {
                paper.Items.Clear();
                foreach (var name in report?.Papers ?? new List<string>())
                {
                    paper.Items.Add(name);
                }
                list.Items.Clear();
                foreach (var layout in state.Layouts)
                {
                    list.Items.Add(layout.Name);
                }
                var keep = prefer ?? (list.SelectedItem as string) ?? state.Layouts.FirstOrDefault()?.Name;
                if (keep != null && state.Layouts.Any(l => l.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)))
                {
                    list.SelectedItem = state.Layouts.First(l => l.Name.Equals(keep, StringComparison.OrdinalIgnoreCase)).Name;
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
            ShowPage(Selected());
            if (state.Layouts.Count == 0)
            {
                Fail("This drawing has no layouts.");
            }
        }

        void Apply(string key, object value)
        {
            if (loading || Selected() is not { } layout)
            {
                return;
            }
            if (!Run(new Dictionary<string, object> { ["layout"] = layout.Name, [key] = value }))
            {
                return;
            }
            Reload(layout.Name);
        }

        list.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                ShowPage(Selected());
            }
        };
        paper.SelectionChanged += (_, _) =>
        {
            if (!loading && paper.SelectedItem is string name)
            {
                Apply("paper", name);
            }
        };
        orient.SelectionChanged += (_, _) =>
        {
            if (!loading && orient.SelectedItem != null)
            {
                Apply("landscape", ReferenceEquals(orient.SelectedItem, landscape));
            }
        };
        plotArea.SelectionChanged += (_, _) =>
        {
            if (!loading && plotArea.SelectedItem is string area)
            {
                Apply("plotArea", area);
            }
        };
        fit.Checked += (_, _) => Apply("scaleToFit", true);
        fit.Unchecked += (_, _) => Apply("scaleToFit", false);
        center.Checked += (_, _) => Apply("center", true);
        center.Unchecked += (_, _) => Apply("center", false);
        lineweights.Checked += (_, _) => Apply("lineweights", true);
        lineweights.Unchecked += (_, _) => Apply("lineweights", false);
        void ApplyScale()
        {
            if (loading || Selected() is not { } layout)
            {
                return;
            }
            var text = (scale.Text ?? "").Trim();
            if (text.Length == 0)
            {
                return;
            }
            object value = double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var n)
                || double.TryParse(text, NumberStyles.Float, CultureInfo.CurrentCulture, out n)
                ? n
                : text;
            Apply("scale", value);
        }
        scale.LostFocus += (_, _) => ApplyScale();
        scale.KeyDown += (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                args.Handled = true;
                ApplyScale();
            }
        };

        Density.Apply(root);
        var window = new Window
        {
            Title = "Page Setup Manager",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "pagesetup");
        Reload(null);
        return window;
    }

    private static FrameworkElement Labeled(string label, FrameworkElement field) => DialogLayout.Labeled(label, field);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);

    private sealed class State
    {
        public List<LayoutRow> Layouts { get; set; } = new();
    }

    private sealed class Report
    {
        public List<LayoutRow> Layouts { get; set; } = new();
        public List<string> Papers { get; set; } = new();
    }

    private sealed class LayoutRow
    {
        public string Name { get; set; } = "";
        public PageRow? Page { get; set; }
        public FitRow? Fit { get; set; }
    }

    private sealed class FitRow
    {
        public double Scale { get; set; } = 1;
        public string Label { get; set; } = "";
        public string Nearest { get; set; } = "";
    }

    private sealed class PageRow
    {
        public string Paper { get; set; } = "";
        public double WidthMm { get; set; }
        public double HeightMm { get; set; }
        public bool Landscape { get; set; }
        public string PlotArea { get; set; } = "layout";
        public bool ScaleToFit { get; set; } = true;
        public double Scale { get; set; } = 1;
        public bool Center { get; set; }
        public bool Lineweights { get; set; } = true;
    }
}

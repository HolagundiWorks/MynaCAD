using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _pageLayoutWindow;

    /// <summary>The names of the drawing tabs, Design first; kept up to date by the tab row.</summary>
    private List<string> _tabNames = new() { "Design" };

    internal void OpenPageLayoutManager(string? selectTab = null)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }

        _pageLayoutWindow?.Close();
        _pageLayoutWindow = PageLayoutWindow.Open(this, _host, _tabNames.ToList(), selectTab ?? _activeTabName, AfterCommand, () => OpenNameplate(landscape: true));
        _pageLayoutWindow.Closed += (_, _) => _pageLayoutWindow = null;
        PresentToolWindow(_pageLayoutWindow);
    }
}

/// <summary>
/// View &gt; Page Layout Manager: one row per drawing tab with its paper size, orientation and scale. A choice is saved for that tab
/// at once (<c>sheet.page</c>); the sheet frame on the canvas follows.
/// </summary>
internal static class PageLayoutWindow
{
    private static readonly string[] Papers = ["A0", "A1", "A2", "A3", "A4"];
    private static readonly string[] Orientations = ["Landscape", "Portrait"];
    private static readonly string[] Scales = ["1:10", "1:20", "1:50", "1:100", "1:200", "1:500"];

    public static Window Open(Window owner, IntPtr host, List<string> tabs, string? select, Action changed, Action openNameplate)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Page layout status");

        var header = new Grid { ColumnSpacing = Density.SpaceMd, Margin = new Thickness(0, 0, 0, 4) };
        foreach (var width in new[] { 160.0, 90.0, 120.0, 90.0 })
        {
            header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(width) });
        }

        var list = new StackPanel { Spacing = Density.SpaceSm };
        var titles = new[] { "Drawing tab", "Paper", "Orientation", "Scale" };
        for (var c = 0; c < titles.Length; c++)
        {
            var title = Density.Secondary(titles[c], TextWrapping.NoWrap);
            Grid.SetColumn(title, c);
            header.Children.Add(title);
        }

        foreach (var tab in tabs)
        {
            list.Children.Add(BuildRow(host, tab, string.Equals(tab, select, StringComparison.OrdinalIgnoreCase), status, changed));
        }

        var nameplate = DialogLayout.Action("Nameplate designer...");
        nameplate.Click += (_, _) => openNameplate();
        var close = DialogLayout.Action("Close", primary: true);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin };
        root.Children.Add(DialogLayout.Heading("Page layout"));
        root.Children.Add(Density.Secondary(
            "Choose the paper, orientation and scale of each drawing tab. The tab's sheet is drawn on the canvas at that scale: 1 mm of paper is as many millimetres of the building as the scale says. Every tab keeps its own.",
            TextWrapping.Wrap));
        root.Children.Add(header);
        root.Children.Add(list);
        root.Children.Add(status);
        root.Children.Add(DialogLayout.ActionRow(nameplate, close));

        Density.Apply(root);
        var window = new Window
        {
            Title = "Page layout",
            Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { root } } },
        };
        close.Click += (_, _) => window.Close();
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "pagelayout", new WindowBounds(520, 360, 760, 700));
        return window;
    }

    private static Grid BuildRow(IntPtr host, string tab, bool selected, TextBlock status, Action changed)
    {
        var (paper, landscape, scale) = Read(host, tab);
        var row = new Grid { ColumnSpacing = Density.SpaceMd, MinHeight = 36 };
        foreach (var width in new[] { 160.0, 90.0, 120.0, 90.0 })
        {
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(width) });
        }

        var name = new TextBlock
        {
            Text = tab,
            VerticalAlignment = VerticalAlignment.Center,
            FontWeight = selected ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        row.Children.Add(name);

        ComboBox Pick(string label, IEnumerable<string> items, string value, int column)
        {
            var box = new ComboBox { MinWidth = 0, HorizontalAlignment = HorizontalAlignment.Stretch, VerticalAlignment = VerticalAlignment.Center };
            foreach (var item in items)
            {
                box.Items.Add(item);
            }

            if (!box.Items.Contains(value))
            {
                box.Items.Add(value);
            }

            box.SelectedItem = value;
            Density.Name(box, label + " of " + tab);
            Grid.SetColumn(box, column);
            row.Children.Add(box);
            return box;
        }

        var paperBox = Pick("Paper", Papers, paper, 1);
        var orientBox = Pick("Orientation", Orientations, landscape ? "Landscape" : "Portrait", 2);
        var scaleBox = Pick("Scale", Scales, scale, 3);

        void Save()
        {
            var body = new Dictionary<string, object>
            {
                ["tab"] = tab,
                ["paper"] = paperBox.SelectedItem as string ?? "A1",
                ["landscape"] = !string.Equals(orientBox.SelectedItem as string, "Portrait", StringComparison.OrdinalIgnoreCase),
                ["scale"] = scaleBox.SelectedItem as string ?? "1:100",
            };
            if (Native.Execute(host, "sheet.page", JsonSerializer.Serialize(body)) != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }

            status.Text = tab + ": " + body["paper"] + " " + (orientBox.SelectedItem as string)?.ToLowerInvariant() + ", " + body["scale"];
            changed();
        }

        paperBox.SelectionChanged += (_, _) => Save();
        orientBox.SelectionChanged += (_, _) => Save();
        scaleBox.SelectionChanged += (_, _) => Save();
        return row;
    }

    private static (string Paper, bool Landscape, string Scale) Read(IntPtr host, string tab)
    {
        if (Native.Execute(host, "sheet.page", JsonSerializer.Serialize(new Dictionary<string, object> { ["tab"] = tab })) != 0)
        {
            return ("A1", true, "1:100");
        }

        try
        {
            using var doc = JsonDocument.Parse(Native.ReadResult(host));
            var root = doc.RootElement;
            return (
                root.TryGetProperty("paper", out var p) ? p.GetString() ?? "A1" : "A1",
                !root.TryGetProperty("landscape", out var l) || l.ValueKind != JsonValueKind.False,
                root.TryGetProperty("scale", out var s) ? s.GetString() ?? "1:100" : "1:100");
        }
        catch (JsonException)
        {
            return ("A1", true, "1:100");
        }
    }
}

using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>Insert &gt; Block: the blocks of this drawing and the parametric library as thumbnails in one window. Click one and pick its
/// insertion point on the drawing. A parametric block goes in with its default parameters; they are entered in the Properties panel
/// afterwards, with the new block selected (<c>smartblock.list</c>, <c>blocks.list</c>, <c>smartblock.insert</c>, <c>insert.place</c>).</summary>
internal static class SmartBlockWindow
{
    private static readonly string[] Disciplines = { "All", "drawing", "rcc", "structural", "plumbing", "hvac", "electrical", "interior" };
    private const int Columns = 3;

    private sealed record Spec(string Id, string Title, string Discipline, string Description, bool Drawing);

    public static Window Open(
        Window owner,
        IntPtr host,
        Action changed,
        Action<string> insertDrawingBlock,
        Action<string> insertParametric,
        Func<string, bool, UIElement> preview)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Blocks status");

        var show = Density.DialogCombo("Show");
        foreach (var name in Disciplines)
        {
            show.Items.Add(new ComboBoxItem { Content = Title(name), Tag = name });
        }
        show.SelectedIndex = 0;
        var tiles = new StackPanel { Spacing = Density.SpaceMd };

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin };
        root.Children.Add(DialogLayout.Heading("Blocks"));
        root.Children.Add(Density.FieldRow("Show", show,
            "Click a block, then pick its insertion point on the drawing. A parametric block is placed with its default sizes; set them in the Properties panel, where its parameters are listed while it is selected."));
        root.Children.Add(tiles);
        root.Children.Add(status);

        var all = new List<Spec>();

        void Load()
        {
            all.Clear();
            if (Native.Execute(host, "smartblock.list", "{}") != 0)
            {
                status.Text = Native.LastError(host);
            }
            else
            {
                Read("blocks", block =>
                {
                    var id = Str(block, "id");
                    if (id.Length > 0)
                    {
                        all.Add(new Spec(id, Str(block, "title"), Str(block, "discipline"), Str(block, "description"), false));
                    }
                });
            }
            if (Native.Execute(host, "blocks.list", "{}") == 0)
            {
                Read("blocks", block =>
                {
                    var name = Str(block, "name");
                    if (name.Length > 0 && !name.StartsWith("SB_", StringComparison.Ordinal) && !name.StartsWith('*'))
                    {
                        var count = block.TryGetProperty("entities", out var e) && e.TryGetInt32(out var c) ? c : 0;
                        all.Add(new Spec(name, name, "drawing", "Defined in this drawing, " + count + " objects.", true));
                    }
                });
            }
            Fill();
        }

        void Read(string key, Action<JsonElement> each)
        {
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (doc.RootElement.TryGetProperty(key, out var list) && list.ValueKind == JsonValueKind.Array)
                {
                    foreach (var item in list.EnumerateArray().Take(500))
                    {
                        each(item);
                    }
                }
            }
            catch (JsonException ex)
            {
                status.Text = ex.Message;
            }
        }

        void Fill()
        {
            tiles.Children.Clear();
            var want = show.SelectedItem is ComboBoxItem { Tag: string tag } ? tag : "All";
            var shown = all.Where(s => want == "All" || string.Equals(s.Discipline, want, StringComparison.OrdinalIgnoreCase)).ToList();
            if (shown.Count == 0)
            {
                tiles.Children.Add(Density.Secondary("No blocks here.", TextWrapping.Wrap));
                return;
            }
            // One group per kind, so this drawing's blocks and each discipline read as sections of the same library.
            foreach (var group in shown.GroupBy(s => s.Discipline, StringComparer.OrdinalIgnoreCase))
            {
                tiles.Children.Add(Density.SectionHeading(Title(group.Key)));
                var grid = new Grid { ColumnSpacing = Density.SpaceMd, RowSpacing = Density.SpaceMd };
                for (var c = 0; c < Columns; c++)
                {
                    grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                }
                var i = 0;
                foreach (var spec in group)
                {
                    var row = i / Columns;
                    while (grid.RowDefinitions.Count <= row)
                    {
                        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
                    }
                    var card = Card(spec);
                    Grid.SetRow(card, row);
                    Grid.SetColumn(card, i % Columns);
                    grid.Children.Add(card);
                    i++;
                }
                tiles.Children.Add(grid);
            }
        }

        Button Card(Spec spec)
        {
            var stack = new StackPanel { Spacing = Density.SpaceXs };
            stack.Children.Add(preview(spec.Id, spec.Drawing));
            stack.Children.Add(new TextBlock
            {
                Text = spec.Title,
                FontSize = Density.FontCaption,
                TextAlignment = TextAlignment.Center,
                TextWrapping = TextWrapping.Wrap,
                MaxLines = 2,
                TextTrimming = TextTrimming.CharacterEllipsis,
            });
            var button = new Button
            {
                Content = stack,
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Center,
                Padding = new Thickness(Density.SpaceSm),
            };
            Density.Name(button, "Insert " + spec.Title);
            ToolTipService.SetToolTip(button, spec.Title + (spec.Description.Length > 0 ? ": " + spec.Description : ""));
            button.Click += (_, _) =>
            {
                if (spec.Drawing)
                {
                    insertDrawingBlock(spec.Id);
                    status.Text = "Pick the insertion point of " + spec.Title + " on the drawing.";
                }
                else
                {
                    insertParametric(spec.Id);
                    status.Text = "Pick the insertion point of " + spec.Title + " on the drawing, then set its parameters in Properties.";
                }
                changed();
            };
            return button;
        }

        show.SelectionChanged += (_, _) => Fill();

        var window = new Window { Title = "Blocks" };
        window.Content = new ScrollViewer { Content = root };
        root.RequestedTheme = theme;
        Density.Apply(root);
        ShellIcon.Apply(window, theme);
        ShellFit.UserResizable(window, minWidthDip: 400, minHeightDip: 300, maxWidthDip: 600, maxHeightDip: 620, persistKey: "blocks");
        Load();
        window.Activate();
        Native.ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(window), 8);
        return window;
    }

    private static string Title(string discipline) => discipline switch
    {
        "rcc" => "RCC",
        "hvac" => "HVAC",
        "All" => "All blocks",
        "drawing" => "This drawing",
        _ => CultureInfo.InvariantCulture.TextInfo.ToTitleCase(discipline),
    };

    private static string Str(JsonElement e, string key) =>
        e.TryGetProperty(key, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";
}

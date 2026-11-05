using System.Globalization;
using System.Text.Json;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace Aad.WinUi;

/// <summary>Format / Modify &gt; Hatch Editor — pattern library with standard swatches.</summary>
internal static class HatchWindow
{
    private const double SwatchDip = Density.HatchSwatch;
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Ui.Status("Hatch status");

        var grid = new GridView
        {
            MinHeight = Density.ListMinHeight,
            SelectionMode = ListViewSelectionMode.Single,
            IsItemClickEnabled = true,
        };
        Density.Name(grid, "Hatch patterns");
        var selectionLabel = Density.Secondary("Selection: —", TextWrapping.Wrap);
        Density.Name(selectionLabel, "Hatch selection");
        var scale = DialogLayout.Field("Hatch scale");
        var angle = DialogLayout.Field("Hatch angle");
        var apply = Ui.Action("Apply to selection", ActionKind.Primary);
        var setDefaults = Ui.Action("Set as defaults");

        // Manager template: the pattern grid takes the free space; properties and actions stay put.
        var layout = new Grid { Padding = Density.DialogMargin, RowSpacing = Density.Space2 };
        layout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        layout.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        layout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var properties = Ui.Section("Pattern properties", Ui.Form(("Scale", scale), ("Angle", angle)));
        layout.Children.Add(selectionLabel);
        Grid.SetRow(grid, 1);
        layout.Children.Add(grid);
        Grid.SetRow(properties, 2);
        layout.Children.Add(properties);
        var root = Ui.ManagerPage(null, layout, Ui.Footer(status, setDefaults, apply));

        var handles = new List<string>();

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

        bool SetVar(string name, object value) =>
            Run("setvar", new Dictionary<string, object> { ["name"] = name, ["value"] = value });

        string SelectedPattern() =>
            (grid.SelectedItem as FrameworkElement)?.Tag as string ?? "";

        bool ParsePositive(TextBox box, out double value)
        {
            value = 0;
            var text = (box.Text ?? "").Trim();
            if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out value)
                && !double.TryParse(text, NumberStyles.Float, CultureInfo.CurrentCulture, out value))
            {
                return false;
            }
            return value > 0 && double.IsFinite(value);
        }

        bool ParseAngle(out double degrees)
        {
            degrees = 0;
            var text = (angle.Text ?? "").Trim();
            if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out degrees)
                && !double.TryParse(text, NumberStyles.Float, CultureInfo.CurrentCulture, out degrees))
            {
                return false;
            }
            return double.IsFinite(degrees);
        }

        void Reload()
        {
            handles.Clear();
            var patternsJson = ExecuteJson("hatch.patterns", new { });
            if (patternsJson is null)
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(patternsJson);
                var pattern = "ANSI31";
                var scaleValue = 1.0;
                var angleValue = 0.0;
                if (doc.RootElement.TryGetProperty("defaults", out var defaults))
                {
                    pattern = defaults.TryGetProperty("pattern", out var p) ? p.GetString() ?? pattern : pattern;
                    if (defaults.TryGetProperty("scale", out var s) && s.TryGetDouble(out var sv) && sv > 0)
                    {
                        scaleValue = sv;
                    }
                    if (defaults.TryGetProperty("angle", out var a) && a.TryGetDouble(out var av))
                    {
                        angleValue = av;
                    }
                }
                scale.Text = scaleValue.ToString("0.####", CultureInfo.InvariantCulture);
                angle.Text = angleValue.ToString("0.####", CultureInfo.InvariantCulture);
                grid.Items.Clear();
                FrameworkElement? selected = null;
                if (doc.RootElement.TryGetProperty("patterns", out var rows) && rows.ValueKind == JsonValueKind.Array)
                {
                    foreach (var row in rows.EnumerateArray())
                    {
                        var name = row.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                        if (name.Length == 0)
                        {
                            continue;
                        }
                        var desc = row.TryGetProperty("description", out var d) ? d.GetString() ?? "" : "";
                        var preview = row.TryGetProperty("preview", out var prev) ? prev : default;
                        var tile = BuildTile(name, desc, preview);
                        grid.Items.Add(tile);
                        if (name.Equals(pattern, StringComparison.OrdinalIgnoreCase))
                        {
                            selected = tile;
                        }
                    }
                }
                if (grid.Items.Count == 0)
                {
                    Fail("No hatch patterns were returned.");
                    return;
                }
                if (selected != null)
                {
                    grid.SelectedItem = selected;
                }
            }
            catch (JsonException)
            {
                Fail("The hatch pattern list could not be read.");
                return;
            }

            var propsJson = ExecuteJson("properties", new { });
            if (propsJson is not null)
            {
                try
                {
                    using var props = JsonDocument.Parse(propsJson);
                    if (props.RootElement.TryGetProperty("objects", out var objects) && objects.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var row in objects.EnumerateArray())
                        {
                            var type = row.TryGetProperty("type", out var t) ? t.GetString() ?? "" : "";
                            var handle = row.TryGetProperty("handle", out var h) ? h.GetString() ?? "" : "";
                            if (type.Equals("Hatch", StringComparison.OrdinalIgnoreCase) && handle.Length > 0)
                            {
                                handles.Add(handle);
                            }
                        }
                    }
                }
                catch (JsonException)
                {
                }
            }
            selectionLabel.Text = handles.Count == 0
                ? "Selection: none (Apply needs a hatch selected)"
                : handles.Count == 1
                    ? "Selection: 1 hatch"
                    : "Selection: " + handles.Count + " hatches";
            changed();
        }

        apply.Click += (_, _) =>
        {
            if (handles.Count == 0)
            {
                Fail("Select one or more hatch objects in the drawing first.");
                return;
            }
            var name = SelectedPattern();
            if (name.Length == 0)
            {
                Fail("Choose a hatch pattern.");
                return;
            }
            if (!ParsePositive(scale, out var scaleValue))
            {
                Fail("Scale must be a positive number.");
                return;
            }
            if (!ParseAngle(out var angleValue))
            {
                Fail("Angle must be a number (degrees).");
                return;
            }
            if (Run("hatchedit", new { handles, pattern = name, scale = scaleValue, angle = angleValue }))
            {
                status.Text = handles.Count == 1 ? "1 hatch updated." : handles.Count + " hatches updated.";
                Reload();
            }
        };

        setDefaults.Click += (_, _) =>
        {
            var name = SelectedPattern();
            if (name.Length == 0)
            {
                Fail("Choose a hatch pattern.");
                return;
            }
            if (!ParsePositive(scale, out var scaleValue))
            {
                Fail("Scale must be a positive number.");
                return;
            }
            if (!ParseAngle(out var angleValue))
            {
                Fail("Angle must be a number (degrees).");
                return;
            }
            if (SetVar("HPNAME", name)
                && SetVar("HPSCALE", scaleValue)
                && SetVar("HPANG", angleValue * Math.PI / 180.0))
            {
                status.Text = "Defaults updated for new hatches.";
                Reload();
            }
        };

        Density.Apply(root);
        root.RequestedTheme = theme;
        var window = new Window
        {
            Title = "Hatch Editor",
            Content = root,
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "hatch", new WindowBounds(420, 300, 780, 900));
        Reload();
        ShellFit.Invalidate(window);
        return window;
    }

    private static FrameworkElement BuildTile(string name, string description, JsonElement preview)
    {
        var ink = Density.ThemeBrush("TextPrimaryBrush")
            ?? new SolidColorBrush(Colors.Black);
        var surface = Density.ThemeBrush("CanvasBackgroundBrush")
            ?? Density.ThemeBrush("SurfaceSecondaryBrush")
            ?? new SolidColorBrush(Colors.White);
        var borderBrush = Density.ThemeBrush("BorderSubtleBrush")
            ?? new SolidColorBrush(Colors.Gray);

        var canvas = new Canvas
        {
            Width = SwatchDip,
            Height = SwatchDip,
            Background = surface,
            IsHitTestVisible = false,
        };
        DrawPreview(canvas, preview, ink);

        var label = new TextBlock
        {
            Text = name,
            FontSize = Density.FontCaption,
            TextAlignment = TextAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxWidth = SwatchDip + Density.Space2,
        };
        var stack = new StackPanel
        {
            Spacing = Density.Space1,
            Margin = new Thickness(Density.Space1),
            Width = SwatchDip + Density.Space3,
            Tag = name,
            Children =
            {
                new Border
                {
                    BorderBrush = borderBrush,
                    BorderThickness = new Thickness(Density.DividerThickness),
                    Child = canvas,
                },
                label,
            },
        };
        var tip = description.Length == 0 ? name : name + " — " + description;
        AutomationProperties.SetName(stack, tip);
        ToolTipService.SetToolTip(stack, tip);
        return stack;
    }

    private static void DrawPreview(Canvas canvas, JsonElement preview, Brush ink)
    {
        var size = SwatchDip;
        var pad = 2.0;
        var draw = size - pad * 2;
        var frame = new Rectangle
        {
            Width = draw,
            Height = draw,
            Stroke = ink,
            StrokeThickness = 0.75,
            Fill = new SolidColorBrush(Colors.Transparent),
        };
        Canvas.SetLeft(frame, pad);
        Canvas.SetTop(frame, pad);
        canvas.Children.Add(frame);

        if (preview.ValueKind != JsonValueKind.Object)
        {
            return;
        }
        if (preview.TryGetProperty("solid", out var solid) && solid.ValueKind == JsonValueKind.True)
        {
            var fill = new Rectangle
            {
                Width = draw,
                Height = draw,
                Fill = ink,
                Opacity = 0.85,
            };
            Canvas.SetLeft(fill, pad);
            Canvas.SetTop(fill, pad);
            canvas.Children.Add(fill);
            return;
        }
        if (!preview.TryGetProperty("segments", out var segments) || segments.ValueKind != JsonValueKind.Array)
        {
            return;
        }
        foreach (var seg in segments.EnumerateArray())
        {
            if (seg.ValueKind != JsonValueKind.Array || seg.GetArrayLength() == 0)
            {
                continue;
            }
            if (seg.GetArrayLength() == 1)
            {
                var p = PointOf(seg[0], pad, draw);
                var dot = new Ellipse { Width = 2.5, Height = 2.5, Fill = ink };
                Canvas.SetLeft(dot, p.X - 1.25);
                Canvas.SetTop(dot, p.Y - 1.25);
                canvas.Children.Add(dot);
                continue;
            }
            var geo = new PathGeometry();
            var fig = new PathFigure { IsClosed = false, IsFilled = false };
            var first = true;
            foreach (var pt in seg.EnumerateArray())
            {
                var p = PointOf(pt, pad, draw);
                if (first)
                {
                    fig.StartPoint = p;
                    first = false;
                }
                else
                {
                    fig.Segments.Add(new LineSegment { Point = p });
                }
            }
            geo.Figures.Add(fig);
            canvas.Children.Add(new Microsoft.UI.Xaml.Shapes.Path
            {
                Data = geo,
                Stroke = ink,
                StrokeThickness = 1.0,
                StrokeStartLineCap = PenLineCap.Round,
                StrokeEndLineCap = PenLineCap.Round,
            });
        }
    }

    private static Windows.Foundation.Point PointOf(JsonElement pt, double pad, double draw)
    {
        // Engine preview is Y-up; WinUI Canvas is Y-down.
        var x = 0.0;
        var y = 0.0;
        if (pt.ValueKind == JsonValueKind.Array && pt.GetArrayLength() >= 2)
        {
            x = pt[0].TryGetDouble(out var xv) ? xv : 0;
            y = pt[1].TryGetDouble(out var yv) ? yv : 0;
        }
        return new Windows.Foundation.Point(pad + x * draw, pad + (1.0 - y) * draw);
    }
}

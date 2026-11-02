using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Aad.WinUi;

/// <summary>Architecture &gt; Room Label Settings — <c>room.settings</c>.</summary>
internal static class RoomSettingsWindow
{
    private static readonly (string Value, string Label)[] Units =
    [
        ("", "Drawing units"),
        ("mm", "mm"),
        ("cm", "cm"),
        ("m", "m"),
        ("ft", "ft"),
        ("in", "in"),
    ];

    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    public static Window Open(Window owner, IntPtr host, Action changed, Func<string>? drawingUnit = null)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Room label settings status");

        var units = new ComboBox { MinWidth = 200, MinHeight = Density.ControlHeight };
        foreach (var (_, label) in Units)
        {
            units.Items.Add(label);
        }
        Density.Name(units, "Drawing units");
        var boxed = new CheckBox { Content = "Draw the room rectangle", MinHeight = Density.ControlHeight };
        Density.Name(boxed, "Box around the room");
        var height = new TextBox { MinHeight = Density.ControlHeight, Width = 120, PlaceholderText = "Default" };
        Density.Name(height, "Text height");
        var font = new ComboBox { IsEditable = true, MinWidth = 220, MinHeight = Density.ControlHeight };
        font.Items.Add("Text style default");
        foreach (var face in new[] { "AAD Stroke", "Segoe UI", "Arial", "Calibri", "Consolas", "Times New Roman" })
        {
            font.Items.Add(face);
        }
        Density.Name(font, "Font");

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 420 };
        root.Children.Add(Heading("Room Label Settings"));
        root.Children.Add(Density.Secondary(
            "The floor in the top bar gives the number prefix; these settings are saved with the drawing.",
            TextWrapping.Wrap));
        root.Children.Add(Labeled("Drawing units (size text)", units));
        root.Children.Add(boxed);
        root.Children.Add(Labeled("Text height (in the unit above; a unit can be typed: 10in, 0.25m)", height));
        root.Children.Add(Labeled("Font", font));
        root.Children.Add(Density.Secondary("Blank text height uses the drawing default from the page-size hierarchy (ISO Small/Medium/Large).", TextWrapping.Wrap));
        root.Children.Add(status);

        var loading = false;

        void Fail(string message) => status.Text = message;

        bool Run(object body)
        {
            if (Native.Execute(host, "room.settings", JsonSerializer.Serialize(body, JsonOpts)) != 0)
            {
                Fail(Native.LastError(host));
                return false;
            }
            status.Text = "";
            changed();
            return true;
        }

        void Load()
        {
            if (!Run(new { }))
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                var rootEl = doc.RootElement;
                loading = true;
                var u = rootEl.TryGetProperty("units", out var unitsEl) ? unitsEl.GetString() ?? "" : "";
                units.SelectedItem = Units.FirstOrDefault(x => x.Value.Equals(u, StringComparison.OrdinalIgnoreCase)).Label;
                if (units.SelectedItem == null)
                {
                    units.SelectedItem = Units[0].Label;
                }
                boxed.IsChecked = !rootEl.TryGetProperty("box", out var box) || box.ValueKind != JsonValueKind.False;
                if (rootEl.TryGetProperty("textHeight", out var th) && th.ValueKind == JsonValueKind.Number && th.TryGetDouble(out var h))
                {
                    height.Text = h.ToString("0.####", CultureInfo.InvariantCulture);
                }
                else
                {
                    height.Text = "";
                }
                var f = rootEl.TryGetProperty("font", out var fontEl) ? fontEl.GetString() ?? "" : "";
                font.Text = f.Length == 0 ? "Text style default" : f;
                loading = false;
            }
            catch (JsonException)
            {
                loading = false;
                Fail("Room label settings could not be read.");
            }
        }

        units.SelectionChanged += (_, _) =>
        {
            if (loading || units.SelectedItem is not string label)
            {
                return;
            }
            var value = Units.FirstOrDefault(x => x.Label == label).Value;
            Run(new { units = value });
        };
        boxed.Checked += (_, _) =>
        {
            if (!loading)
            {
                Run(new { box = true });
            }
        };
        boxed.Unchecked += (_, _) =>
        {
            if (!loading)
            {
                Run(new { box = false });
            }
        };
        void ApplyHeight()
        {
            if (loading)
            {
                return;
            }
            var text = (height.Text ?? "").Trim();
            if (text.Length == 0)
            {
                Run(new Dictionary<string, object?> { ["textHeight"] = null });
                return;
            }
            // The height is in the unit chosen above (or the drawing's); a length typed with a unit is converted into it.
            var chosen = Units[Math.Max(0, units.SelectedIndex)].Value;
            if (!LengthText.TryParse(text, chosen.Length > 0 ? chosen : drawingUnit?.Invoke(), out var v))
            {
                Fail("Text height must be a positive length (for example 250, 10in or 0.25m), or blank.");
                return;
            }
            Run(new { textHeight = v });
        }
        height.LostFocus += (_, _) => ApplyHeight();
        height.KeyDown += (_, args) =>
        {
            if (args.Key == Windows.System.VirtualKey.Enter)
            {
                args.Handled = true;
                ApplyHeight();
            }
        };
        void ApplyFont()
        {
            if (loading)
            {
                return;
            }
            var text = (font.Text ?? "").Trim();
            if (text.Equals("Text style default", StringComparison.OrdinalIgnoreCase))
            {
                text = "";
            }
            Run(new { font = text });
        }
        font.LostFocus += (_, _) => ApplyFont();
        font.SelectionChanged += (_, _) =>
        {
            if (!loading)
            {
                ApplyFont();
            }
        };

        Density.Apply(root);
        var window = new Window
        {
            Title = "Room Label Settings",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Dialog);
        Load();
        return window;
    }

    private static FrameworkElement Labeled(string label, FrameworkElement field) => DialogLayout.Labeled(label, field);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);
}

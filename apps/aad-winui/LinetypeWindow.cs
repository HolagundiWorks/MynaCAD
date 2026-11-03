using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>Format &gt; Linetype? ? load library patterns, set current, global scale.</summary>
internal static class LinetypeWindow
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
        Density.Name(status, "Linetype status");

        var list = new ListBox { MinHeight = 120, MaxHeight = 360, MinWidth = 280 };
        Density.Name(list, "Linetypes");
        var currentLabel = Density.Secondary("Current linetype: —", TextWrapping.Wrap);
        Density.Name(currentLabel, "Current linetype");
        var load = new ComboBox { MinWidth = 140, MinHeight = Density.ControlHeight, PlaceholderText = "Load…" };
        Density.Name(load, "Load linetype");
        var setCurrent = ActionButton("Current");
        var scale = new TextBox { MinHeight = Density.ControlHeight, Width = 100 };
        Density.Name(scale, "Global linetype scale");
        var applyScale = ActionButton("Apply scale");

        var toolbar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        toolbar.Children.Add(load);
        toolbar.Children.Add(setCurrent);

        var scaleRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        scaleRow.Children.Add(new TextBlock { Text = "Global scale factor", VerticalAlignment = VerticalAlignment.Center });
        scaleRow.Children.Add(scale);
        scaleRow.Children.Add(applyScale);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 420 };
        root.Children.Add(Heading("Linetype Manager"));
        root.Children.Add(currentLabel);
        root.Children.Add(toolbar);
        root.Children.Add(list);
        root.Children.Add(scaleRow);
        root.Children.Add(status);

        var loading = false;
        var library = new List<string>();

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

        string GetVar(string name)
        {
            var json = ExecuteJson("getvar", new { name });
            if (json is null)
            {
                return "";
            }
            try
            {
                using var doc = JsonDocument.Parse(json);
                if (!doc.RootElement.TryGetProperty("value", out var value))
                {
                    return "";
                }
                return value.ValueKind switch
                {
                    JsonValueKind.String => value.GetString() ?? "",
                    JsonValueKind.Number => value.ToString(),
                    _ => value.ToString(),
                };
            }
            catch (JsonException)
            {
                return "";
            }
        }

        void Reload()
        {
            // Snapshot before AfterCommand overwrites the host result (e.g. vdb.status).
            var listJson = ExecuteJson("linetype", new { });
            if (listJson is null)
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(listJson);
                var current = GetVar("CELTYPE");
                if (current.Length == 0)
                {
                    current = "ByLayer";
                }
                currentLabel.Text = "Current linetype: " + current;
                var lts = GetVar("LTSCALE");
                loading = true;
                scale.Text = double.TryParse(lts, NumberStyles.Float, CultureInfo.InvariantCulture, out var s) && s > 0
                    ? s.ToString("0.####", CultureInfo.InvariantCulture)
                    : "1";
                list.Items.Clear();
                list.Items.Add("ByLayer");
                list.Items.Add("ByBlock");
                var loaded = new HashSet<string>(StringComparer.OrdinalIgnoreCase) { "ByLayer", "ByBlock" };
                if (doc.RootElement.TryGetProperty("loaded", out var rows) && rows.ValueKind == JsonValueKind.Array)
                {
                    foreach (var row in rows.EnumerateArray())
                    {
                        var name = row.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                        if (name.Length == 0 || !loaded.Add(name))
                        {
                            continue;
                        }
                        var desc = row.TryGetProperty("description", out var d) ? d.GetString() ?? "" : "";
                        list.Items.Add(desc.Length == 0 ? name : name + " — " + desc);
                    }
                }
                library.Clear();
                load.Items.Clear();
                load.Items.Add("All");
                if (doc.RootElement.TryGetProperty("library", out var lib) && lib.ValueKind == JsonValueKind.Array)
                {
                    foreach (var item in lib.EnumerateArray())
                    {
                        var name = item.GetString() ?? "";
                        if (name.Length == 0 || loaded.Contains(name))
                        {
                            continue;
                        }
                        library.Add(name);
                        load.Items.Add(name);
                    }
                }
                var sel = list.Items.Cast<object>().Select(o => o as string ?? "").FirstOrDefault(item =>
                    item.Equals(current, StringComparison.OrdinalIgnoreCase)
                    || item.StartsWith(current + " — ", StringComparison.OrdinalIgnoreCase));
                if (sel != null)
                {
                    list.SelectedItem = sel;
                }
                loading = false;
                changed();
            }
            catch (JsonException)
            {
                loading = false;
                Fail("The linetype list could not be read.");
            }
        }

        string SelectedName()
        {
            var item = list.SelectedItem as string ?? "";
            var sep = item.IndexOf(" — ", StringComparison.Ordinal);
            return sep < 0 ? item : item[..sep];
        }

        setCurrent.Click += (_, _) =>
        {
            var name = SelectedName();
            if (name.Length == 0)
            {
                return;
            }
            if (Run("linetype", new { current = name }))
            {
                Reload();
            }
        };
        load.SelectionChanged += (_, _) =>
        {
            if (loading || load.SelectedItem is not string pick)
            {
                return;
            }
            var arg = pick == "All" ? "*" : pick;
            if (Run("linetype", new { load = arg }))
            {
                load.SelectedItem = null;
                Reload();
            }
        };
        applyScale.Click += (_, _) =>
        {
            if (!double.TryParse((scale.Text ?? "").Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out var v)
                && !double.TryParse((scale.Text ?? "").Trim(), NumberStyles.Float, CultureInfo.CurrentCulture, out v))
            {
                Fail("Scale must be a positive number.");
                return;
            }
            if (v <= 0)
            {
                Fail("Scale must be a positive number.");
                return;
            }
            if (Run("ltscale", new { scale = v }))
            {
                Reload();
            }
        };

        Density.Apply(root);
        var window = new Window
        {
            Title = "Linetype Manager",
            Content = new ScrollViewer
            {
                Content = new Grid { RequestedTheme = theme, Children = { root } },
            },
        };
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "linetype");
        Reload();
        return window;
    }

    private static Button ActionButton(string label) => DialogLayout.Action(label);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);
}

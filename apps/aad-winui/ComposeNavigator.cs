using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _composeNavigator;

    internal void OpenComposeNavigator()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_composeNavigator == null)
        {
            _composeNavigator = ComposeNavigator.Open(this, _host, AfterCommand);
            _composeNavigator.Closed += (_, _) => _composeNavigator = null;
        }
        PresentToolWindow(_composeNavigator);
    }
}

/// <summary>
/// Architecture sheets and File publisher: Views, Masters and Publisher as one tree.
/// Publish runs the fit check, writes the set, and records the take-off.
/// </summary>
internal static class ComposeNavigator
{
    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Navigator status");

        var tree = new TreeView { SelectionMode = TreeViewSelectionMode.Single, Height = 280 };
        Density.Name(tree, "Views, masters and publisher sets");
        var views = new TreeViewNode { Content = "Views", IsExpanded = true };
        var masters = new TreeViewNode { Content = "Masters", IsExpanded = true };
        var publisher = new TreeViewNode { Content = "Publisher", IsExpanded = true };
        tree.RootNodes.Add(views);
        tree.RootNodes.Add(masters);
        tree.RootNodes.Add(publisher);

        var name = Density.DialogField("", "Name");
        Density.Name(name, "Name");
        var layouts = new ComboBox { MinHeight = Density.ControlHeight, HorizontalAlignment = HorizontalAlignment.Stretch, PlaceholderText = "Layout" };
        Density.Name(layouts, "Layout");
        var format = new ComboBox { MinHeight = Density.ControlHeight, HorizontalAlignment = HorizontalAlignment.Stretch };
        format.Items.Add("pdf");
        format.Items.Add("dwg");
        format.SelectedIndex = 0;
        Density.Name(format, "Publisher format");
        var single = new CheckBox { Content = "One file", IsChecked = true, MinHeight = Density.ControlHeight };
        Density.Name(single, "One file");

        var saveView = DialogLayout.Action("Save view");
        var applyView = DialogLayout.Action("Apply view");
        var deleteView = DialogLayout.Action("Delete view");
        var newMaster = DialogLayout.Action("New master");
        var applyMaster = DialogLayout.Action("Apply master");
        var newSet = DialogLayout.Action("New set");
        var check = DialogLayout.Action("Check fit");
        var publish = DialogLayout.Action("Publish and record take-off", primary: true);
        var close = DialogLayout.Action("Close");

        string? ChildName(TreeViewNode root)
        {
            if (tree.SelectedNodes.Count == 0)
            {
                return null;
            }
            var node = tree.SelectedNodes[0];
            if (!ReferenceEquals(node.Parent, root) || node.Content is not string text)
            {
                return null;
            }
            return text;
        }

        bool Exec(string id, string json)
        {
            if (Native.Execute(host, id, json) == 0)
            {
                return true;
            }
            status.Text = Native.LastError(host);
            return false;
        }

        void Fill(TreeViewNode root, string command, string arrayName, Func<JsonElement, string?> label)
        {
            root.Children.Clear();
            if (!Exec(command, "{}"))
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (!doc.RootElement.TryGetProperty(arrayName, out var items) || items.ValueKind != JsonValueKind.Array)
                {
                    return;
                }
                foreach (var item in items.EnumerateArray())
                {
                    var text = label(item);
                    if (!string.IsNullOrEmpty(text))
                    {
                        var node = new TreeViewNode { Content = text, IsExpanded = true };
                        if (item.TryGetProperty("layouts", out var sheets) && sheets.ValueKind == JsonValueKind.Array)
                        {
                            foreach (var sheet in sheets.EnumerateArray())
                            {
                                if (sheet.ValueKind == JsonValueKind.String)
                                {
                                    node.Children.Add(new TreeViewNode { Content = sheet.GetString() });
                                }
                            }
                        }
                        root.Children.Add(node);
                    }
                }
            }
            catch (JsonException)
            {
                status.Text = "The navigator could not read the drawing.";
            }
        }

        void Reload()
        {
            layouts.Items.Clear();
            if (Exec("layout.list", "{}"))
            {
                try
                {
                    using var doc = JsonDocument.Parse(Native.ReadResult(host));
                    if (doc.RootElement.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var item in doc.RootElement.EnumerateArray())
                        {
                            if (item.TryGetProperty("name", out var n) && n.GetString() is { Length: > 0 } layout)
                            {
                                layouts.Items.Add(layout);
                            }
                        }
                    }
                }
                catch (JsonException)
                {
                    status.Text = "The layout list could not be read.";
                }
            }
            if (layouts.Items.Count > 0 && layouts.SelectedIndex < 0)
            {
                layouts.SelectedIndex = 0;
            }
            Fill(views, "compose.view.list", "views", e => e.TryGetProperty("name", out var n) ? n.GetString() : null);
            Fill(masters, "master.list", "masters", e => e.TryGetProperty("name", out var n) ? n.GetString() : null);
            Fill(publisher, "publisher.list", "sets", e => e.TryGetProperty("name", out var n) ? n.GetString() : null);
        }

        string Arg(string key, string? value) => JsonSerializer.Serialize(new Dictionary<string, string> { [key] = value ?? "" });

        saveView.Click += (_, _) =>
        {
            if (string.IsNullOrWhiteSpace(name.Text))
            {
                status.Text = "Type a name for the view.";
                return;
            }
            if (!Exec("compose.view.save", JsonSerializer.Serialize(new Dictionary<string, string> { ["name"] = name.Text.Trim(), ["scale"] = "1:100" })))
            {
                return;
            }
            status.Text = "Saved view " + name.Text.Trim() + ".";
            changed();
            Reload();
        };
        applyView.Click += (_, _) =>
        {
            var view = ChildName(views) ?? name.Text.Trim();
            if (view.Length == 0)
            {
                status.Text = "Select a view.";
                return;
            }
            if (Exec("compose.view.apply", Arg("view", view)))
            {
                status.Text = "Applied " + view + " to the viewport.";
                changed();
            }
        };
        deleteView.Click += (_, _) =>
        {
            var view = ChildName(views);
            if (view == null)
            {
                status.Text = "Select a view.";
                return;
            }
            if (Exec("compose.view.delete", Arg("view", view)))
            {
                status.Text = "Deleted " + view + ".";
                changed();
                Reload();
            }
        };
        newMaster.Click += (_, _) =>
        {
            if (string.IsNullOrWhiteSpace(name.Text))
            {
                status.Text = "Type a name for the master.";
                return;
            }
            if (!Exec("master.new", Arg("name", name.Text.Trim())))
            {
                return;
            }
            status.Text = "Created master " + name.Text.Trim() + ".";
            changed();
            Reload();
        };
        applyMaster.Click += (_, _) =>
        {
            var master = ChildName(masters) ?? name.Text.Trim();
            var layout = layouts.SelectedItem as string;
            if (master.Length == 0 || string.IsNullOrEmpty(layout))
            {
                status.Text = "Select a master and a layout.";
                return;
            }
            if (Exec("master.apply", JsonSerializer.Serialize(new Dictionary<string, string> { ["master"] = master, ["layout"] = layout })))
            {
                status.Text = "Applied " + master + " to " + layout + ".";
                changed();
            }
        };
        newSet.Click += (_, _) =>
        {
            if (string.IsNullOrWhiteSpace(name.Text))
            {
                status.Text = "Type a name for the publisher set.";
                return;
            }
            var sheets = layouts.Items.OfType<string>().ToArray();
            var body = new Dictionary<string, object>
            {
                ["name"] = name.Text.Trim(),
                ["layouts"] = sheets,
                ["format"] = format.SelectedItem as string ?? "pdf",
                ["singleFile"] = single.IsChecked == true,
            };
            if (!Exec("publisher.new", JsonSerializer.Serialize(body)))
            {
                return;
            }
            status.Text = "Created publisher set " + name.Text.Trim() + ".";
            changed();
            Reload();
        };
        check.Click += (_, _) =>
        {
            var set = ChildName(publisher) ?? name.Text.Trim();
            if (set.Length == 0)
            {
                status.Text = "Select a publisher set.";
                return;
            }
            if (!Exec("publisher.check", Arg("set", set)))
            {
                return;
            }
            status.Text = FitText(Native.ReadResult(host));
        };
        publish.Click += (_, _) =>
        {
            var set = ChildName(publisher) ?? name.Text.Trim();
            if (set.Length == 0)
            {
                status.Text = "Select a publisher set.";
                return;
            }
            if (format.SelectedItem is string fmt)
            {
                Exec("publisher.set", JsonSerializer.Serialize(new Dictionary<string, object>
                {
                    ["set"] = set,
                    ["format"] = fmt,
                    ["singleFile"] = single.IsChecked == true,
                }));
            }
            if (!Exec("publisher.publish", JsonSerializer.Serialize(new Dictionary<string, object> { ["set"] = set, ["takeoff"] = true })))
            {
                return;
            }
            status.Text = PublishText(Native.ReadResult(host));
            changed();
        };

        void Show()
        {
            Reload();
            status.Text = "Views, masters and publisher sets for this drawing.";
        }

        var root = new StackPanel { Spacing = Density.SpaceSm, Margin = Density.DialogMargin };
        root.Children.Add(DialogLayout.Heading("Navigator"));
        root.Children.Add(Density.Secondary("Views are display recipes. Masters are paper and title blocks. Publisher plots the layouts and records the take-off.", TextWrapping.Wrap));
        root.Children.Add(tree);
        root.Children.Add(name);
        root.Children.Add(DialogLayout.Labeled("Layout", layouts));
        root.Children.Add(DialogLayout.Labeled("Format", format));
        root.Children.Add(single);
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { saveView, applyView, deleteView } });
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { newMaster, applyMaster, newSet } });
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { check, publish, close } });
        root.Children.Add(status);
        Density.Apply(root);

        var window = new Window
        {
            Title = "Navigator",
            Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { root } } },
        };
        close.Click += (_, _) => window.Close();
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "compose-navigator", new WindowBounds(520, 420, 900, 980));
        Show();
        return window;
    }

    private static string FitText(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var ok = doc.RootElement.TryGetProperty("fitOk", out var flag) && flag.ValueKind == JsonValueKind.True;
            if (ok)
            {
                return "Every sheet fits the printable area.";
            }
            if (doc.RootElement.TryGetProperty("warnings", out var warnings) && warnings.ValueKind == JsonValueKind.Array)
            {
                var lines = warnings.EnumerateArray().Select(w => w.GetString()).Where(t => !string.IsNullOrEmpty(t));
                var text = string.Join(" ", lines);
                return text.Length == 0 ? "A sheet does not fit." : text;
            }
        }
        catch (JsonException)
        {
            return "The fit check could not be read.";
        }
        return "A sheet does not fit.";
    }

    private static string PublishText(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var message = doc.RootElement.TryGetProperty("message", out var m) ? m.GetString() ?? "" : "";
            var extra = "";
            if (doc.RootElement.TryGetProperty("warnings", out var warnings) && warnings.ValueKind == JsonValueKind.Array)
            {
                extra = string.Join(" ", warnings.EnumerateArray().Select(w => w.GetString()).Where(t => !string.IsNullOrEmpty(t)));
            }
            if (doc.RootElement.TryGetProperty("takeoff", out var takeoff) && takeoff.ValueKind == JsonValueKind.Object)
            {
                extra = string.IsNullOrEmpty(extra) ? "Take-off recorded." : extra;
            }
            return string.Join(" ", new[] { message, extra }.Where(t => !string.IsNullOrWhiteSpace(t)));
        }
        catch (JsonException)
        {
            return "Published.";
        }
    }
}

using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>Window &gt; Parameters Manager — parametric graph status, library, preview and commit.</summary>
internal static class ParametersWindow
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
        Density.Name(status, "Parameters status");

        var revision = Density.Secondary("Graph revision: —", TextWrapping.Wrap);
        Density.Name(revision, "Graph revision");
        var walls = new ListView { MinHeight = 110, MinWidth = 360, SelectionMode = ListViewSelectionMode.Single };
        Density.Name(walls, "Parametric walls");
        var nodes = new ListView { MinHeight = 80, MinWidth = 360, SelectionMode = ListViewSelectionMode.None };
        Density.Name(nodes, "Library nodes");

        var refresh = ActionButton("Refresh");
        var preview = ActionButton("Preview");
        var commit = ActionButton("Commit");
        var detach = ActionButton("Detach");
        detach.IsEnabled = false;

        var toolbar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        toolbar.Children.Add(refresh);
        toolbar.Children.Add(preview);
        toolbar.Children.Add(commit);
        toolbar.Children.Add(detach);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 480 };
        root.Children.Add(Heading("Parameters Manager"));
        root.Children.Add(revision);
        root.Children.Add(toolbar);
        root.Children.Add(Density.SectionHeading("Walls in this drawing"));
        root.Children.Add(walls);
        root.Children.Add(Density.SectionHeading("Library node types"));
        root.Children.Add(nodes);
        root.Children.Add(status);

        void Fail(string message) => status.Text = message;

        string? ExecuteJson(string command, object body)
        {
            if (Native.Execute(host, command, JsonSerializer.Serialize(body, JsonOpts)) != 0)
            {
                Fail(Native.LastError(host));
                return null;
            }
            status.Text = "";
            return Native.ReadResult(host);
        }

        void Load()
        {
            walls.Items.Clear();
            nodes.Items.Clear();
            detach.IsEnabled = false;

            if (ExecuteJson("param.status", new { }) is not string statusJson)
            {
                revision.Text = "Graph revision: —";
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(statusJson);
                var rootEl = doc.RootElement;
                var rev = rootEl.TryGetProperty("revision", out var r) && r.TryGetInt64(out var n) ? n : 0;
                revision.Text = "Graph revision: " + rev.ToString(CultureInfo.InvariantCulture);
                if (rootEl.TryGetProperty("walls", out var list) && list.ValueKind == JsonValueKind.Array)
                {
                    foreach (var wall in list.EnumerateArray())
                    {
                        var id = wall.TryGetProperty("instanceId", out var idNode) ? idNode.GetString() ?? "?" : "?";
                        var state = wall.TryGetProperty("state", out var st) ? st.GetString() ?? "" : "";
                        var reasons = wall.TryGetProperty("reasons", out var rs) && rs.ValueKind == JsonValueKind.Array
                            ? string.Join("; ", rs.EnumerateArray().Select(x => x.GetString() ?? "").Where(s => s.Length > 0))
                            : "";
                        var label = state.Length == 0 ? id : id + " — " + state;
                        if (reasons.Length > 0)
                        {
                            label += " (" + reasons + ")";
                        }
                        walls.Items.Add(new WallRow(id, label));
                    }
                }
                if (walls.Items.Count == 0)
                {
                    walls.Items.Add(new WallRow("", "No parametric walls yet — use Architecture > Parametric > New Wall."));
                }
            }
            catch (JsonException ex)
            {
                Fail(ex.Message);
            }

            if (ExecuteJson("param.library", new { }) is not string libJson)
            {
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(libJson);
                if (doc.RootElement.TryGetProperty("nodes", out var nodeList) && nodeList.ValueKind == JsonValueKind.Array)
                {
                    foreach (var node in nodeList.EnumerateArray().Take(80))
                    {
                        var label = node.TryGetProperty("label", out var labelN) ? labelN.GetString() : null;
                        var typeName = node.TryGetProperty("nodeType", out var typeN) ? typeN.GetString() : null;
                        var category = node.TryGetProperty("category", out var catN) ? catN.GetString() : null;
                        var name = !string.IsNullOrWhiteSpace(label)
                            ? (string.IsNullOrWhiteSpace(category) ? label : category + " · " + label)
                            : typeName;
                        if (!string.IsNullOrWhiteSpace(name))
                        {
                            nodes.Items.Add(name);
                        }
                    }
                }
                if (nodes.Items.Count == 0)
                {
                    nodes.Items.Add("(empty library)");
                }
            }
            catch (JsonException ex)
            {
                Fail(ex.Message);
            }
        }

        walls.SelectionChanged += (_, _) =>
        {
            detach.IsEnabled = walls.SelectedItem is WallRow { Id.Length: > 0 };
        };

        refresh.Click += (_, _) => Load();
        preview.Click += (_, _) =>
        {
            if (ExecuteJson("param.evaluate", new { }) is string json)
            {
                status.Text = "Preview ready.";
                try
                {
                    using var doc = JsonDocument.Parse(json);
                    if (doc.RootElement.TryGetProperty("staged", out var staged) && staged.ValueKind == JsonValueKind.Array)
                    {
                        status.Text = "Preview: " + staged.GetArrayLength() + " staged wall(s).";
                    }
                }
                catch (JsonException)
                {
                    // status already set
                }
                changed();
            }
        };
        commit.Click += (_, _) =>
        {
            if (ExecuteJson("param.commit", new { }) != null)
            {
                status.Text = "Committed.";
                changed();
                Load();
            }
        };
        detach.Click += (_, _) =>
        {
            if (walls.SelectedItem is not WallRow { Id.Length: > 0 } row)
            {
                return;
            }
            if (ExecuteJson("param.detach", new { instance = row.Id }) != null)
            {
                status.Text = "Detached " + row.Id + ".";
                changed();
                Load();
            }
        };

        var window = new Window { Title = "Parameters Manager" };
        window.Content = new ScrollViewer { Content = root };
        root.RequestedTheme = theme;
        Density.Apply(root);
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "parameters");
        Load();
        window.Activate();
        Native.ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(window), 8);
        return window;
    }

    private static Button ActionButton(string label) => DialogLayout.Action(label);

    private static TextBlock Heading(string text) => DialogLayout.Heading(text);

    private sealed class WallRow(string id, string label)
    {
        public string Id { get; } = id;
        public override string ToString() => label;
    }
}

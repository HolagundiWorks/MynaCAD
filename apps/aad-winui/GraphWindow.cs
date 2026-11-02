using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;

#if ODRAFT_GRAPH
using ODraft.Graph;
#endif

namespace Aad.WinUi;

/// <summary>
/// Window &gt; Graph — Dynamo companion node editor (separate from the side rail).
/// Evaluate uses in-process <c>GraphEngine</c> when <c>ODRAFT_GRAPH</c> is set; Commit uses
/// <c>companion.commit</c>. Preview is stashed with <c>companion.preview</c> for the canvas ghost.
/// </summary>
internal static class GraphWindow
{
    private static readonly JsonSerializerOptions GraphJsonOpts = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        WriteIndented = true,
    };

    private const string DefaultGraphJson = """
        {
          "nodes": [
            {"id":"wall","type":"wallOutline","x":0,"y":0,"length":4000,"thickness":230,"level":"Ground","uiX":40,"uiY":40},
            {"id":"door","type":"door","wallX0":0,"wallY0":0,"wallX1":4000,"wallY1":0,"x":2000,"y":0,"width":900,"thickness":200,"level":"Ground","uiX":280,"uiY":40}
          ],
          "edges": []
        }
        """;

    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var editor = new GraphEditor(host, changed);
        var window = new Window { Title = "Graph" };
        window.Content = editor.Root;
        editor.Root.RequestedTheme = theme;
        Density.Apply(editor.Root);
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "graph", new WindowBounds(720, 520, 1400, 1000));
        editor.Seed();
        window.Activate();
        Native.ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(window), 8);
        return window;
    }

    private sealed class GraphEditor
    {
        private readonly IntPtr _host;
        private readonly Action _changed;
        private readonly TextBox _graphEditor;
        private readonly Canvas _graphCanvas;
        private readonly ScrollViewer _graphCanvasScroll;
        private readonly TextBlock _graphStatus;
        private readonly Button _modeNodes;
        private readonly Button _modeJson;

        private string? _graphPreviewJson;
        private bool _graphNodesMode = true;
        private bool _graphSyncing;
        private string? _graphSelectedId;
        private Point? _graphDragOrigin;
        private Point? _graphNodeOrigin;
        private Border? _graphDragBorder;

        public FrameworkElement Root { get; }

        public GraphEditor(IntPtr host, Action changed)
        {
            _host = host;
            _changed = changed;

            _modeNodes = DialogLayout.Action("Nodes");
            _modeJson = DialogLayout.Action("JSON");
            AutomationProperties.SetAutomationId(_modeNodes, "graph.mode.nodes");
            AutomationProperties.SetAutomationId(_modeJson, "graph.mode.json");
            var modeRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs };
            modeRow.Children.Add(_modeNodes);
            modeRow.Children.Add(_modeJson);

            var addWall = DialogLayout.Action("+ Wall");
            var addDoor = DialogLayout.Action("+ Door");
            var addNumber = DialogLayout.Action("+ Number");
            AutomationProperties.SetAutomationId(addWall, "graph.add.wall");
            AutomationProperties.SetAutomationId(addDoor, "graph.add.door");
            AutomationProperties.SetAutomationId(addNumber, "graph.add.number");
            var addRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs };
            addRow.Children.Add(addWall);
            addRow.Children.Add(addDoor);
            addRow.Children.Add(addNumber);
            var addScroll = new ScrollViewer
            {
                Content = addRow,
                HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
                VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            };

            _graphCanvas = new Canvas
            {
                Width = Density.GraphCanvasWidth,
                Height = Density.GraphCanvasHeight,
                Background = Density.ThemeBrush("SurfaceSecondaryBrush")
                    ?? new SolidColorBrush(Windows.UI.Color.FromArgb(255, 28, 28, 30)),
            };
            Density.Name(_graphCanvas, "Graph nodes");
            _graphCanvasScroll = new ScrollViewer
            {
                Content = _graphCanvas,
                HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
                VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            };

            _graphEditor = new TextBox
            {
                Visibility = Visibility.Collapsed,
                AcceptsReturn = true,
                TextWrapping = TextWrapping.Wrap,
                FontFamily = new FontFamily("Consolas"),
                FontSize = Density.FontCaption,
            };
            Density.Name(_graphEditor, "Graph JSON");
            AutomationProperties.SetAutomationId(_graphEditor, "graph.json");
            ScrollViewer.SetVerticalScrollBarVisibility(_graphEditor, ScrollBarVisibility.Auto);

            var editorHost = new Grid();
            editorHost.Children.Add(_graphCanvasScroll);
            editorHost.Children.Add(_graphEditor);

            var evaluate = DialogLayout.Action("Evaluate");
            var commit = DialogLayout.Action("Commit");
            AutomationProperties.SetAutomationId(evaluate, "graph.evaluate");
            AutomationProperties.SetAutomationId(commit, "graph.commit");
            var actionRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs };
            actionRow.Children.Add(evaluate);
            actionRow.Children.Add(commit);

            _graphStatus = Density.Secondary("", TextWrapping.Wrap);
            Density.Name(_graphStatus, "Graph status");

            var root = new Grid
            {
                Margin = Density.DialogMargin,
                RowSpacing = Density.SpaceXs,
                MinWidth = Density.WindowMinWidthWide,
                MinHeight = Density.WindowMinHeightGraph,
            };
            root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
            root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var heading = DialogLayout.Heading("Graph");
            Grid.SetRow(heading, 0);
            Grid.SetRow(modeRow, 1);
            Grid.SetRow(addScroll, 2);
            Grid.SetRow(editorHost, 3);
            Grid.SetRow(actionRow, 4);
            Grid.SetRow(_graphStatus, 5);
            root.Children.Add(heading);
            root.Children.Add(modeRow);
            root.Children.Add(addScroll);
            root.Children.Add(editorHost);
            root.Children.Add(actionRow);
            root.Children.Add(_graphStatus);
            Root = root;

            _modeNodes.Click += (_, _) =>
            {
                _graphNodesMode = true;
                SyncJsonFromCanvasPositions();
                ApplyGraphModeUi();
                RebuildGraphCanvasFromJson();
            };
            _modeJson.Click += (_, _) =>
            {
                SyncJsonFromCanvasPositions();
                _graphNodesMode = false;
                ApplyGraphModeUi();
            };
            addWall.Click += (_, _) => AddGraphNode("wallOutline", new Dictionary<string, JsonNode?>
            {
                ["x"] = JsonValue.Create(0),
                ["y"] = JsonValue.Create(0),
                ["length"] = JsonValue.Create(4000),
                ["thickness"] = JsonValue.Create(230),
                ["level"] = JsonValue.Create("Ground"),
            });
            addDoor.Click += (_, _) => AddGraphNode("door", new Dictionary<string, JsonNode?>
            {
                ["wallX0"] = JsonValue.Create(0),
                ["wallY0"] = JsonValue.Create(0),
                ["wallX1"] = JsonValue.Create(4000),
                ["wallY1"] = JsonValue.Create(0),
                ["x"] = JsonValue.Create(2000),
                ["y"] = JsonValue.Create(0),
                ["width"] = JsonValue.Create(900),
                ["thickness"] = JsonValue.Create(200),
                ["level"] = JsonValue.Create("Ground"),
            });
            addNumber.Click += (_, _) => AddGraphNode("number", new Dictionary<string, JsonNode?>
            {
                ["value"] = JsonValue.Create(4000),
            });
            _graphEditor.LostFocus += (_, _) =>
            {
                if (!_graphNodesMode)
                {
                    RebuildGraphCanvasFromJson();
                }
            };
            evaluate.Click += (_, _) => OnGraphEvaluate();
            commit.Click += (_, _) => OnGraphCommit();
        }

        public void Seed()
        {
            if (string.IsNullOrWhiteSpace(_graphEditor.Text))
            {
                _graphEditor.Text = DefaultGraphJson;
            }
            ApplyGraphModeUi();
            RebuildGraphCanvasFromJson();
#if ODRAFT_GRAPH
            _graphStatus.Text = "Nodes: edit cards, Evaluate (ghost on canvas), then Commit.";
#else
            _graphStatus.Text = "Evaluate runs through the engine (companion.evaluate). Optional: rebuild with sibling O-Dynamo for in-process GraphEngine.";
#endif
        }

        private void ApplyGraphModeUi()
        {
            _graphCanvasScroll.Visibility = _graphNodesMode ? Visibility.Visible : Visibility.Collapsed;
            _graphEditor.Visibility = _graphNodesMode ? Visibility.Collapsed : Visibility.Visible;
        }

        private void AddGraphNode(string type, Dictionary<string, JsonNode?> props)
        {
            SyncJsonFromCanvasPositions();
            JsonObject root;
            try
            {
                root = JsonNode.Parse(_graphEditor.Text)?.AsObject() ?? new JsonObject();
            }
            catch (JsonException)
            {
                root = JsonNode.Parse(DefaultGraphJson)!.AsObject();
            }
            var nodes = root["nodes"] as JsonArray ?? new JsonArray();
            root["nodes"] = nodes;
            if (root["edges"] is not JsonArray)
            {
                root["edges"] = new JsonArray();
            }
            var id = NextGraphId(nodes, type);
            var node = new JsonObject { ["id"] = id, ["type"] = type, ["uiX"] = 40 + nodes.Count * 24, ["uiY"] = 40 + nodes.Count * 16 };
            foreach (var (k, v) in props)
            {
                node[k] = v is null ? null : v.DeepClone();
            }
            nodes.Add(node);
            _graphSyncing = true;
            _graphEditor.Text = root.ToJsonString(GraphJsonOpts);
            _graphSyncing = false;
            _graphSelectedId = id;
            _graphNodesMode = true;
            ApplyGraphModeUi();
            RebuildGraphCanvasFromJson();
        }

        private static string NextGraphId(JsonArray nodes, string type)
        {
            var prefix = type.Length <= 4 ? type : type[..4];
            for (var n = 1; n < 10_000; n++)
            {
                var id = prefix + n;
                if (!nodes.Any(x => x?["id"]?.GetValue<string>() == id))
                {
                    return id;
                }
            }
            return prefix + Guid.NewGuid().ToString("N")[..6];
        }

        private void RebuildGraphCanvasFromJson()
        {
            if (_graphSyncing)
            {
                return;
            }
            _graphCanvas.Children.Clear();
            JsonObject root;
            try
            {
                root = JsonNode.Parse(string.IsNullOrWhiteSpace(_graphEditor.Text) ? DefaultGraphJson : _graphEditor.Text)?.AsObject()
                    ?? new JsonObject();
            }
            catch (JsonException ex)
            {
                _graphStatus.Text = "JSON: " + ex.Message;
                return;
            }
            var nodes = root["nodes"] as JsonArray ?? new JsonArray();
            var edges = root["edges"] as JsonArray ?? new JsonArray();
            var positions = new Dictionary<string, Point>(StringComparer.Ordinal);
            var index = 0;
            foreach (var item in nodes)
            {
                if (item is not JsonObject node)
                {
                    continue;
                }
                var id = node["id"]?.GetValue<string>() ?? "";
                if (id.Length == 0)
                {
                    continue;
                }
                var type = node["type"]?.GetValue<string>() ?? "?";
                var ux = node["uiX"]?.GetValue<double>() ?? (40 + (index % 3) * 220);
                var uy = node["uiY"]?.GetValue<double>() ?? (40 + (index / 3) * 120);
                index++;
                positions[id] = new Point(ux, uy);
                _graphCanvas.Children.Add(MakeGraphCard(id, type, node, ux, uy));
            }
            foreach (var item in edges)
            {
                if (item is not JsonObject edge)
                {
                    continue;
                }
                var from = edge["from"]?.GetValue<string>() ?? "";
                var to = edge["to"]?.GetValue<string>() ?? "";
                if (!positions.TryGetValue(from, out var a) || !positions.TryGetValue(to, out var b))
                {
                    continue;
                }
                var line = new Line
                {
                    X1 = a.X + 180,
                    Y1 = a.Y + 36,
                    X2 = b.X,
                    Y2 = b.Y + 36,
                    Stroke = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 120, 140, 160)),
                    StrokeThickness = 1.5,
                    IsHitTestVisible = false,
                };
                _graphCanvas.Children.Insert(0, line);
            }
            _graphCanvas.Width = Math.Max(720, positions.Values.Select(p => p.X).DefaultIfEmpty(0).Max() + 240);
            _graphCanvas.Height = Math.Max(520, positions.Values.Select(p => p.Y).DefaultIfEmpty(0).Max() + 140);
        }

        private Border MakeGraphCard(string id, string type, JsonObject node, double ux, double uy)
        {
            var selected = string.Equals(id, _graphSelectedId, StringComparison.Ordinal);
            var body = new StackPanel { Spacing = Density.Space1 };
            body.Children.Add(new TextBlock
            {
                Text = type,
                FontSize = Density.FontCaption,
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            });
            body.Children.Add(new TextBlock
            {
                Text = id,
                FontSize = Density.FontCaption,
                Opacity = 0.7,
            });
            var summary = GraphNodeSummary(node);
            if (summary.Length > 0)
            {
                body.Children.Add(new TextBlock
                {
                    Text = summary,
                    FontSize = Density.FontCaption,
                    TextWrapping = TextWrapping.Wrap,
                });
            }
            var card = new Border
            {
                Width = Density.ManagerListWidth,
                MinHeight = Density.DialogActionMinWidth,
                Padding = new Thickness(Density.Space2),
                CornerRadius = new CornerRadius(Density.CornerControl),
                Background = new SolidColorBrush(selected
                    ? Windows.UI.Color.FromArgb(255, 28, 42, 80)
                    : Windows.UI.Color.FromArgb(255, 32, 36, 42)),
                BorderBrush = new SolidColorBrush(selected
                    ? Windows.UI.Color.FromArgb(255, 111, 143, 240)
                    : Windows.UI.Color.FromArgb(255, 70, 78, 88)),
                BorderThickness = new Thickness(selected ? Density.SpaceXs : Density.DividerThickness),
                Child = body,
                Tag = id,
            };
            ToolTipService.SetToolTip(card, "Drag to move. Double-click to edit fields.");
            Density.Name(card, "Graph node " + type + " " + id);
            card.PointerPressed += OnGraphCardPressed;
            card.PointerMoved += OnGraphCardMoved;
            card.PointerReleased += OnGraphCardReleased;
            card.PointerCaptureLost += OnGraphCardReleased;
            card.DoubleTapped += OnGraphCardDoubleTapped;
            Canvas.SetLeft(card, ux);
            Canvas.SetTop(card, uy);
            return card;
        }

        private static string GraphNodeSummary(JsonObject node)
        {
            var parts = new List<string>();
            foreach (var key in new[] { "length", "thickness", "width", "value", "level", "text", "name" })
            {
                if (node[key] is JsonValue v)
                {
                    parts.Add($"{key}={v.ToJsonString().Trim('"')}");
                }
                if (parts.Count >= 3)
                {
                    break;
                }
            }
            return string.Join(" · ", parts);
        }

        private void OnGraphCardPressed(object sender, PointerRoutedEventArgs e)
        {
            if (sender is not Border border || border.Tag is not string id)
            {
                return;
            }
            _graphSelectedId = id;
            _graphDragBorder = border;
            _graphDragOrigin = e.GetCurrentPoint(_graphCanvas).Position;
            _graphNodeOrigin = new Point(Canvas.GetLeft(border), Canvas.GetTop(border));
            border.CapturePointer(e.Pointer);
            RebuildGraphCanvasFromJson();
            e.Handled = true;
        }

        private void OnGraphCardMoved(object sender, PointerRoutedEventArgs e)
        {
            if (_graphDragBorder is null || _graphDragOrigin is null || _graphNodeOrigin is null)
            {
                return;
            }
            var now = e.GetCurrentPoint(_graphCanvas).Position;
            var dx = now.X - _graphDragOrigin.Value.X;
            var dy = now.Y - _graphDragOrigin.Value.Y;
            Canvas.SetLeft(_graphDragBorder, Math.Max(0, _graphNodeOrigin.Value.X + dx));
            Canvas.SetTop(_graphDragBorder, Math.Max(0, _graphNodeOrigin.Value.Y + dy));
        }

        private void OnGraphCardReleased(object sender, PointerRoutedEventArgs e)
        {
            if (_graphDragBorder is not null)
            {
                try
                {
                    _graphDragBorder.ReleasePointerCapture(e.Pointer);
                }
                catch (UnauthorizedAccessException)
                {
                }
            }
            _graphDragBorder = null;
            _graphDragOrigin = null;
            _graphNodeOrigin = null;
            SyncJsonFromCanvasPositions();
            RebuildGraphCanvasFromJson();
        }

        private async void OnGraphCardDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
        {
            if (sender is not Border border || border.Tag is not string id)
            {
                return;
            }
            await EditGraphNodeAsync(id);
        }

        private async Task EditGraphNodeAsync(string id)
        {
            JsonObject root;
            try
            {
                root = JsonNode.Parse(_graphEditor.Text)?.AsObject() ?? new JsonObject();
            }
            catch (JsonException)
            {
                return;
            }
            var nodes = root["nodes"] as JsonArray;
            var node = nodes?.OfType<JsonObject>().FirstOrDefault(n => n["id"]?.GetValue<string>() == id);
            if (node is null)
            {
                return;
            }
            var box = new TextBox
            {
                AcceptsReturn = true,
                TextWrapping = TextWrapping.Wrap,
                Text = node.ToJsonString(GraphJsonOpts),
                MinHeight = Density.ListMinHeight,
                FontFamily = new FontFamily("Consolas"),
                FontSize = Density.FontCaption,
            };
            Density.Name(box, "Node JSON");
            var dialog = new ContentDialog
            {
                Title = "Edit node " + id,
                Content = DialogLayout.Labeled("Node JSON", box),
                PrimaryButtonText = "Apply",
                CloseButtonText = "Cancel",
                XamlRoot = Root.XamlRoot,
            };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary)
            {
                return;
            }
            try
            {
                var edited = JsonNode.Parse(box.Text)?.AsObject();
                if (edited is null)
                {
                    _graphStatus.Text = "Node JSON was empty.";
                    return;
                }
                edited["id"] = id;
                if (edited["type"] is null)
                {
                    edited["type"] = node["type"]?.DeepClone();
                }
                var idx = nodes!.IndexOf(node);
                nodes[idx] = edited;
                _graphSyncing = true;
                _graphEditor.Text = root.ToJsonString(GraphJsonOpts);
                _graphSyncing = false;
                RebuildGraphCanvasFromJson();
            }
            catch (JsonException ex)
            {
                _graphStatus.Text = ex.Message;
            }
        }

        private void SyncJsonFromCanvasPositions()
        {
            JsonObject root;
            try
            {
                root = JsonNode.Parse(string.IsNullOrWhiteSpace(_graphEditor.Text) ? DefaultGraphJson : _graphEditor.Text)?.AsObject()
                    ?? new JsonObject();
            }
            catch (JsonException)
            {
                return;
            }
            var nodes = root["nodes"] as JsonArray;
            if (nodes is null)
            {
                return;
            }
            foreach (var child in _graphCanvas.Children.OfType<Border>())
            {
                if (child.Tag is not string id)
                {
                    continue;
                }
                var node = nodes.OfType<JsonObject>().FirstOrDefault(n => n["id"]?.GetValue<string>() == id);
                if (node is null)
                {
                    continue;
                }
                node["uiX"] = Canvas.GetLeft(child);
                node["uiY"] = Canvas.GetTop(child);
            }
            _graphSyncing = true;
            _graphEditor.Text = root.ToJsonString(GraphJsonOpts);
            _graphSyncing = false;
        }

        private void OnGraphEvaluate()
        {
            if (_host == IntPtr.Zero)
            {
                _graphStatus.Text = "Host is not ready.";
                return;
            }
            SyncJsonFromCanvasPositions();
            var text = _graphEditor.Text?.Trim() ?? "";
            if (text.Length == 0 || text.Length > 512_000)
            {
                _graphStatus.Text = "Graph JSON is empty or too large.";
                return;
            }

#if ODRAFT_GRAPH
            try
            {
                using var doc = JsonDocument.Parse(text);
                var evaluated = GraphEngine.Evaluate(doc.RootElement, DateTimeOffset.UtcNow.ToUnixTimeMilliseconds());
                if (!evaluated.Ok)
                {
                    var err = evaluated.Diagnostics.FirstOrDefault(d => d.Severity == "error")?.Message ?? "evaluate failed";
                    _graphStatus.Text = err;
                    _graphPreviewJson = null;
                    return;
                }
                _graphPreviewJson = JsonSerializer.Serialize(
                    new
                    {
                        evaluated.Lines,
                        evaluated.Circles,
                        evaluated.Arcs,
                        evaluated.Inserts,
                        evaluated.TitleFields,
                        evaluated.Texts,
                        evaluated.Dimensions,
                    },
                    new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase });
                StashCompanionPreview(_graphPreviewJson);
                _graphStatus.Text =
                    $"OK: {evaluated.Lines.Count} lines, {evaluated.Circles.Count} circles, {evaluated.Arcs.Count} arcs — ghost on canvas.";
            }
            catch (Exception ex) when (ex is JsonException or ArgumentException or InvalidOperationException)
            {
                _graphStatus.Text = ex.Message;
                _graphPreviewJson = null;
            }
#else
            using (var graphDoc = JsonDocument.Parse(text))
            {
                var payload = JsonSerializer.Serialize(new { graph = graphDoc.RootElement });
                if (Native.Execute(_host, "companion.evaluate", payload) != 0)
                {
                    _graphStatus.Text = Native.LastError(_host);
                    _graphPreviewJson = null;
                    return;
                }
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(_host));
                if (doc.RootElement.TryGetProperty("result", out var result))
                {
                    _graphPreviewJson = result.GetRawText();
                    _graphStatus.Text = "Evaluated via companion pipe — ghost on canvas.";
                }
                else
                {
                    _graphPreviewJson = null;
                    _graphStatus.Text = "Evaluate returned no result.";
                }
            }
            catch (JsonException ex)
            {
                _graphStatus.Text = ex.Message;
                _graphPreviewJson = null;
            }
#endif
            _changed();
        }

        private void StashCompanionPreview(string resultJson)
        {
            try
            {
                using var doc = JsonDocument.Parse(resultJson);
                var payload = JsonSerializer.Serialize(new { result = doc.RootElement });
                _ = Native.Execute(_host, "companion.preview", payload);
            }
            catch (JsonException)
            {
            }
        }

        private void OnGraphCommit()
        {
            if (_host == IntPtr.Zero)
            {
                _graphStatus.Text = "Host is not ready.";
                return;
            }
            if (string.IsNullOrWhiteSpace(_graphPreviewJson))
            {
                _graphStatus.Text = "Evaluate first.";
                return;
            }
            string payload;
            try
            {
                using var preview = JsonDocument.Parse(_graphPreviewJson);
                payload = JsonSerializer.Serialize(new { result = preview.RootElement });
            }
            catch (JsonException ex)
            {
                _graphStatus.Text = ex.Message;
                return;
            }
            if (Native.Execute(_host, "companion.commit", payload) != 0)
            {
                _graphStatus.Text = Native.LastError(_host);
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(_host));
                _graphStatus.Text = doc.RootElement.GetRawText();
            }
            catch (JsonException)
            {
                _graphStatus.Text = "Committed.";
            }
            _ = Native.Execute(_host, "companion.preview", "{\"result\":null}");
            _graphPreviewJson = null;
            _changed();
        }
    }
}

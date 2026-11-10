using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _layerMapWizard;

    /// <summary>
    /// The layer mapping wizard for the open drawing. <c>layermap {suggest}</c> lists the drawing's layers that are not
    /// built in; the wizard is shown only when it says the drawing <c>needsMapping</c> (a drawing that did not come from
    /// MynaCAD, or the layers an import brought in). The engine command never opens a dialog: this window is the UI.
    /// </summary>
    /// <param name="only">Layer names to limit the wizard to (the layers of an import, or the Layer manager selection).</param>
    /// <param name="force">Show the wizard even when the drawing is not waiting for it (the Layer manager asked for it).</param>
    private void OfferLayerMapper(IReadOnlyList<string>? only = null, bool force = false)
    {
        if (_host == IntPtr.Zero || _layerMapWizard != null)
        {
            return;
        }
        var parameters = new Dictionary<string, object> { ["suggest"] = true };
        if (only is { Count: > 0 })
        {
            parameters["only"] = only;
        }
        if (Native.Execute(_host, "layermap", JsonSerializer.Serialize(parameters)) != 0)
        {
            return;
        }
        var plan = Native.ReadResult(_host);
        if (!force && !LayerMapWizard.NeedsMapping(plan))
        {
            return;
        }
        var window = LayerMapWizard.Open(this, _host, plan, only, AfterCommand);
        if (window == null)
        {
            if (force)
            {
                History.Text = "Every layer of the selection is already a built-in layer.";
            }
            return;
        }
        _layerMapWizard = window;
        window.Closed += (_, _) => _layerMapWizard = null;
        PresentToolWindow(window);
    }

    /// <summary>Map the given layers (the Layer manager selection) to built-in layers.</summary>
    internal void MapLayersToBuiltIn(IReadOnlyList<string> names) => OfferLayerMapper(names, force: true);
}

/// <summary>
/// Step one chooses a built-in layer for each layer of the drawing (several may share one; "keep" leaves a layer alone).
/// Step two is the review: the engine's <c>preview</c> lists what moves, and nothing changes until Apply.
/// The names of the mapped layers stay on record in the drawing (<c>layerSources</c>).
/// </summary>
internal static class LayerMapWizard
{
    private const string Keep = "(keep this layer as it is)";

    public static bool NeedsMapping(string planJson)
    {
        try
        {
            using var doc = JsonDocument.Parse(planJson);
            return doc.RootElement.TryGetProperty("needsMapping", out var n) && n.ValueKind == JsonValueKind.True;
        }
        catch (JsonException)
        {
            return false;
        }
    }

    public static Window? Open(Window owner, IntPtr host, string planJson, IReadOnlyList<string>? only, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Layer mapping status");

        var destNames = new List<string>();
        var destLabels = new List<string>();
        var rows = new List<(string Layer, ComboBox Target)>();
        var list = new StackPanel { Spacing = Density.SpaceXs };
        try
        {
            using var doc = JsonDocument.Parse(planJson);
            if (doc.RootElement.TryGetProperty("destinations", out var dests) && dests.ValueKind == JsonValueKind.Array)
            {
                foreach (var t in dests.EnumerateArray())
                {
                    var name = t.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                    var group = t.TryGetProperty("group", out var g) ? g.GetString() ?? "" : "";
                    if (name.Length > 0)
                    {
                        destNames.Add(name);
                        destLabels.Add(name + "  (" + group + ")");
                    }
                }
            }
            if (doc.RootElement.TryGetProperty("layers", out var layers) && layers.ValueKind == JsonValueKind.Array)
            {
                foreach (var l in layers.EnumerateArray())
                {
                    var name = l.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                    var count = l.TryGetProperty("objects", out var c) && c.TryGetInt32(out var k) ? k : 0;
                    var suggested = l.TryGetProperty("suggested", out var s) && s.ValueKind == JsonValueKind.String ? s.GetString() : null;
                    if (name.Length == 0 || count == 0)
                    {
                        continue;
                    }
                    var combo = new ComboBox { MinHeight = Density.ControlHeight, HorizontalAlignment = HorizontalAlignment.Stretch };
                    combo.Items.Add(Keep);
                    foreach (var label in destLabels)
                    {
                        combo.Items.Add(label);
                    }
                    var at = suggested == null ? -1 : destNames.IndexOf(suggested);
                    combo.SelectedIndex = at >= 0 ? at + 1 : 0;
                    Density.Name(combo, "Built-in layer for " + name);
                    var label2 = Density.Secondary(name + "  (" + count + ")", TextWrapping.NoWrap);
                    label2.VerticalAlignment = VerticalAlignment.Center;
                    var row = new Grid { ColumnSpacing = Density.SpaceSm };
                    row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                    row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                    Grid.SetColumn(combo, 1);
                    row.Children.Add(label2);
                    row.Children.Add(combo);
                    list.Children.Add(row);
                    rows.Add((name, combo));
                }
            }
        }
        catch (JsonException)
        {
            return null;
        }
        if (rows.Count == 0)
        {
            return null;
        }

        var review = DialogLayout.Action("Review", primary: true);
        var apply = DialogLayout.Action("Apply", primary: true);
        var back = DialogLayout.Action("Back");
        var cancel = DialogLayout.Action("Cancel");
        var keepAll = DialogLayout.Action("Keep my layers");
        var purge = new CheckBox { Content = "Remove the layers left empty (their names stay on record)", IsChecked = true };
        Density.Name(purge, "Remove the layers left empty");
        var reviewList = new StackPanel { Spacing = Density.SpaceXs };

        var pick = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin };
        pick.Children.Add(DialogLayout.Heading("Map layers to MynaCAD layers"));
        pick.Children.Add(Density.Secondary(
            "These layers are not MynaCAD layers. Choose the built-in layer each one goes on; several can share one. The names stay on record as the source of the objects. The number is how many objects the layer holds. Nothing changes until you review and apply.",
            TextWrapping.Wrap));
        pick.Children.Add(list);
        pick.Children.Add(status);
        pick.Children.Add(only == null ? DialogLayout.ActionRow(review, keepAll, cancel) : DialogLayout.ActionRow(review, cancel));

        var check = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, Visibility = Visibility.Collapsed };
        check.Children.Add(DialogLayout.Heading("Review the changes"));
        check.Children.Add(reviewList);
        check.Children.Add(purge);
        check.Children.Add(DialogLayout.ActionRow(apply, back, cancel));

        var page = new Grid { RequestedTheme = theme };
        page.Children.Add(pick);
        page.Children.Add(check);
        Density.Apply(pick);
        Density.Apply(check);

        var window = new Window { Title = "Map Layers", Content = new ScrollViewer { Content = page } };
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "layermapwizard");

        Dictionary<string, string> Chosen()
        {
            var map = new Dictionary<string, string>();
            foreach (var (layer, combo) in rows)
            {
                var i = combo.SelectedIndex - 1;
                if (i >= 0 && i < destNames.Count)
                {
                    map[layer] = destNames[i];
                }
            }
            return map;
        }

        review.Click += (_, _) =>
        {
            var map = Chosen();
            if (map.Count == 0)
            {
                status.Text = "Choose a built-in layer for at least one layer, or press Cancel.";
                return;
            }
            if (Native.Execute(host, "layermap", JsonSerializer.Serialize(new { map, preview = true })) != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }
            reviewList.Children.Clear();
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                foreach (var m in doc.RootElement.GetProperty("mapped").EnumerateArray())
                {
                    var from = m.GetProperty("from").GetString() ?? "";
                    var to = m.GetProperty("to").GetString() ?? "";
                    var moved = m.GetProperty("moved").GetInt32();
                    var locked = m.TryGetProperty("locked", out var lk) && lk.ValueKind == JsonValueKind.True;
                    reviewList.Children.Add(Density.Secondary(from + "  →  " + to + "  (" + (locked ? "locked, stays" : moved + " objects") + ")", TextWrapping.Wrap));
                }
                var total = doc.RootElement.GetProperty("total").GetInt32();
                reviewList.Children.Add(DialogLayout.Heading(total + " objects will move to built-in layers"));
            }
            catch (Exception ex) when (ex is JsonException or KeyNotFoundException or InvalidOperationException)
            {
                status.Text = "The review could not be read.";
                return;
            }
            pick.Visibility = Visibility.Collapsed;
            check.Visibility = Visibility.Visible;
        };
        back.Click += (_, _) =>
        {
            check.Visibility = Visibility.Collapsed;
            pick.Visibility = Visibility.Visible;
        };
        apply.Click += (_, _) =>
        {
            var json = JsonSerializer.Serialize(new { map = Chosen(), purge = purge.IsChecked == true });
            if (Native.Execute(host, "layermap", json) != 0)
            {
                status.Text = Native.LastError(host);
                check.Visibility = Visibility.Collapsed;
                pick.Visibility = Visibility.Visible;
                return;
            }
            changed();
            window.Close();
        };
        keepAll.Click += (_, _) =>
        {
            // The drawing keeps its layers and is not offered the wizard again; Format > Layer Tools > Map Layers still works.
            if (Native.Execute(host, "layermap", "{\"dismiss\":true}") == 0)
            {
                changed();
            }
            window.Close();
        };
        cancel.Click += (_, _) => window.Close();
        return window;
    }
}

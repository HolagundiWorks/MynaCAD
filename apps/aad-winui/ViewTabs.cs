using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Aad.WinUi;

/// <summary>
/// The Section / Elevation tab group in the bottom bar, after the level tabs (design:
/// <c>docs/levels-and-sheet-sets.md</c>, "Sections and elevations"). Tab data is the <c>views</c> list of the chrome JSON; every action
/// is an engine command (<c>view.activate</c>, <c>section.new</c>, <c>section.check</c>, ...). A group of more than
/// <see cref="CollapseAfter"/> views collapses into one dropdown chip.
/// </summary>
public sealed partial class MainWindow
{
    private sealed class ViewTab
    {
        public long Id { get; set; }
        public string Name { get; set; } = "";
        public string Kind { get; set; } = "section";
        public bool Active { get; set; }
        public long Level { get; set; }
        public bool HasMarker { get; set; } = true;
    }

    private const int CollapseAfter = 6;
    private string _viewKey = "";
    private List<ViewTab> _cachedViews = new();

    private void ShowViewTabs(ChromeState state)
    {
        var key = string.Join("|", state.Views.Select(v => v.Id + ":" + v.Name + ":" + v.Kind + ":" + v.Active + ":" + v.HasMarker));
        if (key == _viewKey)
        {
            return;
        }
        _viewKey = key;
        ViewTabs.Children.Clear();
        ViewTabsSeparator.Visibility = state.Views.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
        var views = state.Views.ToList();
        if (views.Count > CollapseAfter)
        {
            ViewTabs.Children.Add(MakeViewChip(views));
            return;
        }
        foreach (var v in views)
        {
            ViewTabs.Children.Add(MakeViewTab(v));
        }
    }

    private Button MakeViewTab(ViewTab view)
    {
        var label = new TextBlock
        {
            Text = view.Name,
            MaxWidth = 160,
            FontSize = Density.FontCaption,
            TextTrimming = TextTrimming.CharacterEllipsis,
            VerticalAlignment = VerticalAlignment.Center,
            FontWeight = view.Active ? FontWeights.SemiBold : FontWeights.Normal,
        };
        var tab = SeamlessTab.Create(label, view.Active, TabEdge.Bottom, SeamlessTab.LabelPadding);
        tab.MinHeight = Density.ControlHeight;
        AutomationProperties.SetName(tab, (view.Active ? "Current view " : "Open view ") + view.Name);
        ToolTipService.SetToolTip(
            tab,
            view.Name + " (" + view.Kind + ")" + (view.HasMarker ? "" : ", its marker was deleted") + "  (right-click for more)");
        tab.Click += (_, _) => OpenView(view);
        tab.RightTapped += (_, e) =>
        {
            e.Handled = true;
            BuildViewMenu(view).ShowAt(tab, new FlyoutShowOptions { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft });
        };
        return tab;
    }

    /// <summary>More than a handful of views: one chip named after the open view (else "Sections"), listing them all.</summary>
    private Button MakeViewChip(List<ViewTab> views)
    {
        var active = views.FirstOrDefault(v => v.Active);
        var tab = SeamlessTab.Create(
            new TextBlock { Text = (active?.Name ?? "Sections") + " ▾", FontSize = Density.FontCaption, VerticalAlignment = VerticalAlignment.Center },
            active != null,
            TabEdge.Bottom,
            SeamlessTab.LabelPadding);
        tab.MinHeight = Density.ControlHeight;
        AutomationProperties.SetName(tab, "Sections and elevations: " + views.Count);
        ToolTipService.SetToolTip(tab, views.Count + " sections and elevations");
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft };
        foreach (var v in views)
        {
            var item = new ToggleMenuFlyoutItem { Text = v.Name, IsChecked = v.Active };
            var captured = v;
            item.Click += (_, _) => OpenView(captured);
            menu.Items.Add(item);
        }
        tab.Click += (_, _) => menu.ShowAt(tab);
        tab.RightTapped += (_, e) =>
        {
            e.Handled = true;
            if (active != null)
            {
                BuildViewMenu(active).ShowAt(tab, new FlyoutShowOptions { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft });
            }
        };
        return tab;
    }

    private void OpenView(ViewTab view) =>
        RunShellCommand("view.activate", new Dictionary<string, object> { ["view"] = view.Id });

    private MenuFlyout BuildViewMenu(ViewTab view)
    {
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft };
        void Add(string text, string command)
        {
            var item = new MenuFlyoutItem { Text = text };
            item.Click += (_, _) => RunShellCommand(command, new Dictionary<string, object> { ["view"] = view.Id });
            menu.Items.Add(item);
        }

        Add("Open", "view.activate");
        menu.Items.Add(new MenuFlyoutSeparator());
        Add("Draft from the guides", "section.draft");
        Add("Check against the plan", "section.check");
        Add("Level marks", "section.levelmarks");
        menu.Items.Add(new MenuFlyoutSeparator());
        var trace = new MenuFlyoutSubItem { Text = "Trace another view" };
        foreach (var other in _cachedViews.Where(o => o.Id != view.Id))
        {
            var item = new MenuFlyoutItem { Text = other.Name };
            var id = other.Id;
            item.Click += (_, _) => RunShellCommand("view.trace", new Dictionary<string, object> { ["view"] = view.Id, ["add"] = id });
            trace.Items.Add(item);
        }
        if (trace.Items.Count > 0)
        {
            menu.Items.Add(trace);
        }
        menu.Items.Add(new MenuFlyoutSeparator());
        Add("Delete view", "view.delete");
        return menu;
    }

    /// <summary>The + beside the view tabs: new section or new elevation (the engine prompts for the points on the drawing).</summary>
    private void OnAddView(object sender, RoutedEventArgs e)
    {
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.TopEdgeAlignedLeft };
        var section = new MenuFlyoutItem { Text = "New section" };
        section.Click += (_, _) => StartCommand("section.new");
        menu.Items.Add(section);
        var elevation = new MenuFlyoutItem { Text = "New elevation" };
        elevation.Click += (_, _) => StartCommand("elevation.new");
        menu.Items.Add(elevation);
        if (sender is FrameworkElement fe)
        {
            menu.ShowAt(fe);
        }
    }
}

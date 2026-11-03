using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>
/// Style-dialog helpers (list column + editor). Generic pieces delegate to <see cref="Ui"/> so
/// there is one implementation of each component; tokens live in <see cref="Density"/>.
/// </summary>
internal static class DialogLayout
{
    public static Button Action(string label, bool primary = false) => Density.DialogAction(label, primary);

    public static TextBlock Heading(string text)
    {
        var block = Density.SectionHeading(text);
        AutomationProperties.SetHeadingLevel(block, AutomationHeadingLevel.Level2);
        return block;
    }

    public static FrameworkElement Labeled(string label, FrameworkElement field, string? info = null) => Ui.Labeled(label, field, info);

    public static TextBox Field(string name, double? width = null)
    {
        var box = Density.DialogField("", name);
        box.Width = width ?? Density.FieldWidth;
        box.HorizontalAlignment = HorizontalAlignment.Left;
        return box;
    }

    public static ListBox StyleList(string accessName)
    {
        var list = new ListBox
        {
            MinHeight = Density.ListMinHeight,
            MinWidth = Density.ManagerListWidth,
            HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        Density.Name(list, accessName);
        return list;
    }

    public static StackPanel LeftColumn(string heading, ListBox list, params UIElement[] extras)
    {
        var left = new StackPanel { Spacing = Density.SpaceSm, Width = Density.ManagerListWidth };
        left.Children.Add(Heading(heading));
        left.Children.Add(list);
        foreach (var extra in extras)
        {
            left.Children.Add(extra);
        }
        return left;
    }

    public static Grid Columns(FrameworkElement left, FrameworkElement right)
    {
        var columns = new Grid { ColumnSpacing = Density.Space3 };
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(left, 0);
        Grid.SetColumn(right, 1);
        columns.Children.Add(left);
        columns.Children.Add(right);
        return columns;
    }

    public static StackPanel Root(params UIElement[] children) => Ui.Stack(children);

    /// <summary>Right-aligned actions. Put the primary button immediately before Cancel.</summary>
    public static StackPanel ActionRow(params Button[] buttons)
    {
        var row = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = Density.Space2,
            HorizontalAlignment = HorizontalAlignment.Right,
        };
        foreach (var button in buttons)
        {
            row.Children.Add(button);
        }
        return row;
    }
}

using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Aad.WinUi;

/// <summary>
/// The tool column's layout: tool tiles two to a row, and a child tagged <c>divider</c> takes a whole row of its own.
/// (WinUI 3 has no <c>VariableSizedWrapGrid</c>.)
/// </summary>
public sealed class ToolColumnPanel : Panel
{
    /// <summary>The size of one tile; the column is two tiles wide.</summary>
    public double Cell { get; set; } = 26;

    private static bool IsDivider(UIElement child) => child is FrameworkElement { Tag: "divider" };

    protected override Size MeasureOverride(Size availableSize)
    {
        var y = 0.0;
        var inRow = 0;
        foreach (var child in Children)
        {
            if (IsDivider(child))
            {
                if (inRow > 0)
                {
                    y += Cell;
                    inRow = 0;
                }

                child.Measure(new Size(Cell * 2, double.PositiveInfinity));
                y += child.DesiredSize.Height;
                continue;
            }

            child.Measure(new Size(Cell, Cell));
            if (++inRow == 2)
            {
                y += Cell;
                inRow = 0;
            }
        }

        if (inRow > 0)
        {
            y += Cell;
        }

        return new Size(Cell * 2, y);
    }

    protected override Size ArrangeOverride(Size finalSize)
    {
        var y = 0.0;
        var inRow = 0;
        foreach (var child in Children)
        {
            if (IsDivider(child))
            {
                if (inRow > 0)
                {
                    y += Cell;
                    inRow = 0;
                }

                child.Arrange(new Rect(0, y, Cell * 2, child.DesiredSize.Height));
                y += child.DesiredSize.Height;
                continue;
            }

            child.Arrange(new Rect(inRow * Cell, y, Cell, Cell));
            if (++inRow == 2)
            {
                y += Cell;
                inRow = 0;
            }
        }

        return new Size(Cell * 2, y + (inRow > 0 ? Cell : 0));
    }
}

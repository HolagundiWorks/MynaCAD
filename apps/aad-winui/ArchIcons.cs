using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Aad.WinUi;

/// <summary>
/// Drawn marks for the short Architecture toolbar. Gray is geometry already there; blue is what
/// the command adds.
/// </summary>
internal static partial class ToolIcons
{
    public static UIElement Architecture(string id, bool dark)
    {
        var canvas = new Canvas { Width = 24, Height = 24 };
        DrawArchitecture(new Pen(canvas, dark), id);
        return new Viewbox { Width = ToolIcons.DisplaySize, Height = ToolIcons.DisplaySize, Child = canvas, Stretch = Stretch.Uniform };
    }

    private static void DrawArchitecture(Pen pen, string id)
    {
        switch (id)
        {
            case "wall.draw":
                pen.Base(3, 8, 21, 8);
                pen.Accent(3, 14, 21, 14);
                pen.Base(3, 8, 3, 14);
                pen.Accent(21, 8, 21, 14);
                break;
            case "wall.door":
                pen.Base(4, 6, 4, 20);
                pen.Base(16, 6, 16, 20);
                pen.Accent(4, 20, 16, 20);
                pen.Arc(4, 20, 12, 0, 90, pen.AccentBrush);
                break;
            case "wall.window":
                pen.Base(3, 7, 21, 7);
                pen.Base(3, 17, 21, 17);
                pen.Accent(6, 7, 6, 17);
                pen.Accent(12, 7, 12, 17);
                pen.Accent(18, 7, 18, 17);
                break;
            case "room":
                pen.Box(4, 5, 16, 14, pen.BaseBrush);
                pen.Label("R", 12, 13, 9, pen.AccentBrush);
                break;
            case "rarea":
                pen.Stroke(true, pen.AccentBrush, (4, 18), (6, 5), (19, 7), (20, 19));
                pen.Label("A", 12, 13, 8, pen.BaseBrush);
                break;
            case "grid.axis":
                pen.Base(4, 6, 4, 20);
                pen.Base(12, 4, 12, 20);
                pen.Accent(20, 6, 20, 20);
                pen.Base(2, 18, 22, 18);
                pen.Circle(4, 6, 1.6, pen.BaseBrush);
                pen.Circle(12, 4, 1.6, pen.BaseBrush);
                pen.Circle(20, 6, 1.6, pen.AccentBrush);
                break;
            case "column.place":
                pen.Base(4, 4, 20, 20);
                pen.Base(20, 4, 4, 20);
                pen.FillRect(9, 9, 6, 6, pen.AccentBrush);
                break;
            case "rcc.column":
                pen.Box(7, 4, 10, 16, pen.BaseBrush);
                pen.Dot(9.5, 7);
                pen.Dot(14.5, 7);
                pen.Dot(9.5, 17);
                pen.Dot(14.5, 17);
                break;
            case "rcc.column.round":
                pen.Circle(12, 12, 8, pen.BaseBrush);
                pen.Dot(12, 6);
                pen.Dot(17.2, 9);
                pen.Dot(17.2, 15);
                pen.Dot(12, 18);
                pen.Dot(6.8, 15);
                pen.Dot(6.8, 9);
                break;
            case "rcc.beam":
                pen.Box(4, 5, 16, 14, pen.BaseBrush);
                pen.Dot(8, 8);
                pen.Dot(16, 8);
                pen.Dot(8, 16);
                pen.Dot(12, 16);
                pen.Dot(16, 16);
                break;
            case "rcc.footing":
                pen.Stroke(true, pen.BaseBrush, (3, 19), (21, 19), (17, 12), (7, 12));
                pen.FillRect(10, 4, 4, 8, pen.BaseBrush);
                pen.Accent(5, 17, 19, 17);
                break;
            case "rcc.slab":
                pen.Box(2, 9, 20, 6, pen.BaseBrush);
                pen.Accent(4, 13, 20, 13);
                pen.Dot(6, 11);
                pen.Dot(12, 11);
                pen.Dot(18, 11);
                break;
            case "rcc.lintel":
                pen.Box(3, 8, 18, 6, pen.BaseBrush);
                pen.Accent(5, 12, 19, 12);
                pen.Base(3, 14, 3, 20);
                pen.Base(21, 14, 21, 20);
                break;
            case "stair.plan":
                pen.Stroke(false, pen.AccentBrush, (3, 20), (3, 16), (8, 16), (8, 12), (13, 12), (13, 8), (18, 8), (18, 4), (22, 4));
                break;
            case "north.arrow":
                pen.Circle(12, 13, 8, pen.BaseBrush);
                pen.Stroke(true, pen.AccentBrush, (12, 5), (15, 16), (12, 13), (9, 16));
                break;
            case "takeoff.ext":
                pen.Accent(3, 16, 21, 16);
                pen.Base(3, 10, 21, 10);
                pen.Label("E", 12, 8, 7, pen.AccentBrush);
                break;
            case "takeoff.int":
                pen.Accent(3, 8, 21, 8);
                pen.Accent(3, 16, 21, 16);
                pen.Label("I", 12, 13, 7, pen.BaseBrush);
                break;
            case "takeoff.beam":
                pen.Box(3, 9, 18, 6, pen.AccentBrush);
                break;
            case "takeoff.floor":
                pen.Box(4, 8, 16, 10, pen.BaseBrush);
                pen.Accent(4, 18, 20, 18);
                break;
            case "takeoff.ceiling":
                pen.Accent(4, 6, 20, 6);
                pen.Base(6, 10, 6, 18);
                pen.Base(18, 10, 18, 18);
                break;
            case "takeoff.column":
                pen.FillRect(8, 6, 8, 12, pen.AccentBrush);
                break;
            case "takeoff.deduct":
                pen.Base(4, 6, 4, 18);
                pen.Base(20, 6, 20, 18);
                pen.Accent(6, 12, 18, 12);
                break;
            case "takeoff.paint":
                pen.Box(4, 5, 16, 14, pen.BaseBrush);
                pen.Accent(8, 16, 16, 8);
                break;
            case "openings.sync":
                pen.Arc(12, 12, 7, 40, 320, pen.AccentBrush);
                pen.Arrow(16.5, 6.5, 1, -0.2);
                break;
            case "openings.deduct":
                pen.Base(5, 5, 5, 19);
                pen.Base(19, 5, 19, 19);
                pen.Accent(7, 12, 17, 12);
                break;
            case "column.deduct":
                pen.FillRect(8, 8, 8, 8, pen.BaseBrush);
                pen.Accent(4, 20, 20, 4);
                break;
            case "takeoff.record":
                pen.Circle(12, 12, 7, pen.AccentBrush);
                pen.Accent(12, 8, 12, 13);
                pen.Dot(12, 16);
                break;
            case "takeoff.maplayers":
                pen.Base(4, 7, 10, 7);
                pen.Accent(14, 7, 20, 7);
                pen.Base(4, 12, 10, 12);
                pen.Accent(14, 12, 20, 12);
                pen.Base(4, 17, 10, 17);
                pen.Accent(14, 17, 20, 17);
                break;
            default:
                for (var k = 0; k < 3; k++)
                {
                    var y = 5.0 + k * 5;
                    pen.Stroke(true, k == 0 ? pen.AccentBrush : pen.BaseBrush, (3, y + 2), (12, y - 2), (21, y + 2), (12, y + 6));
                }
                break;
        }
    }
}

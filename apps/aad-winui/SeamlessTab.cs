using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;

namespace Aad.WinUi;

/// <summary>The edge of the work area a tab hangs from; the active tab joins that area.</summary>
internal enum TabEdge
{
    /// <summary>Under the drawing (level tabs).</summary>
    Bottom,

    /// <summary>Left of the drawing (sheet-set tabs).</summary>
    Left,

    /// <summary>Right of the side panel's edge, against the open panel (panel titles).</summary>
    Right,

    /// <summary>Above the drawing (sheet tabs).</summary>
    Top,
}

/// <summary>
/// One tab look for every strip: an inactive tab is flat text on the bar; the active tab takes the
/// colour of the surface it controls (the drawing, or the side panel), so the tab and its content read
/// as one piece with no line between them. The corners away from the content are rounded and a thin
/// accent line marks the outer edge. A plain <see cref="Button"/> with a bare template: the state is
/// set from code, because the tabs are rebuilt from the engine's state.
/// </summary>
internal static class SeamlessTab
{
    private const string TemplateXaml = """
        <ControlTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" TargetType="Button">
          <Border Background="{TemplateBinding Background}" BorderBrush="{TemplateBinding BorderBrush}"
                  BorderThickness="{TemplateBinding BorderThickness}" CornerRadius="{TemplateBinding CornerRadius}"
                  Padding="{TemplateBinding Padding}">
            <ContentPresenter HorizontalAlignment="{TemplateBinding HorizontalContentAlignment}"
                              VerticalAlignment="{TemplateBinding VerticalContentAlignment}"
                              Foreground="{TemplateBinding Foreground}" FontWeight="{TemplateBinding FontWeight}" />
          </Border>
        </ControlTemplate>
        """;

    /// <summary>Side padding of a tab holding a text label.</summary>
    public static readonly Thickness LabelPadding = new(Density.Space3, 0, Density.Space3, 0);

    public static Button Create(UIElement content, bool active, TabEdge edge, Thickness padding)
    {
        var button = new Button
        {
            Content = content,
            Template = (ControlTemplate)XamlReader.Load(TemplateXaml),
            Padding = padding,
            MinWidth = 0,
            MinHeight = 0,
            HorizontalContentAlignment = HorizontalAlignment.Center,
            VerticalContentAlignment = VerticalAlignment.Center,
        };
        Apply(button, active, edge);
        button.PointerEntered += (_, _) =>
        {
            if (button.Tag is not true)
            {
                button.Background = new SolidColorBrush(Windows.UI.Color.FromArgb(30, 128, 128, 128));
            }
        };
        button.PointerExited += (_, _) => Apply(button, button.Tag is true, edge);
        return button;
    }

    /// <summary>Set the look for the active or inactive state (also after a theme change).</summary>
    public static void Apply(Button button, bool active, TabEdge edge)
    {
        button.Tag = active;
        var surface = edge == TabEdge.Right ? "SurfaceBrush" : "CanvasBackgroundBrush";
        var clear = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        button.Background = active ? Brush(surface) : clear;
        button.BorderBrush = active ? Brush("AccentBrush") : clear;
        button.Foreground = Brush(active ? "TextPrimaryBrush" : "TextSecondaryBrush");
        button.FontWeight = active ? FontWeights.SemiBold : FontWeights.Normal;
        switch (edge)
        {
            case TabEdge.Bottom:
                button.BorderThickness = new Thickness(0, 0, 0, Density.SpaceXs);
                button.CornerRadius = new CornerRadius(0, 0, Density.Space1, Density.Space1);
                break;
            case TabEdge.Left:
                button.BorderThickness = new Thickness(Density.SpaceXs, 0, 0, 0);
                button.CornerRadius = new CornerRadius(Density.Space1, 0, 0, Density.Space1);
                break;
            case TabEdge.Top:
                button.BorderThickness = new Thickness(0, Density.SpaceXs, 0, 0);
                button.CornerRadius = new CornerRadius(Density.Space1, Density.Space1, 0, 0);
                break;
            default:
                button.BorderThickness = new Thickness(0, 0, Density.SpaceXs, 0);
                button.CornerRadius = new CornerRadius(0, Density.Space1, Density.Space1, 0);
                break;
        }
    }

    private static Brush Brush(string key) => Density.ThemeBrush(key) ?? new SolidColorBrush(Microsoft.UI.Colors.Gray);
}

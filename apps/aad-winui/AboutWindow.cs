using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>About O-Draft. The version comes from the host.</summary>
internal static class AboutWindow
{
    private static Window? _window;

    public static void Show(Window owner, string version)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var logo = new Image { Width = Density.LogoSize, Height = Density.LogoSize, HorizontalAlignment = HorizontalAlignment.Left, Source = ShellIcon.Logo() };
        AutomationProperties.SetName(logo, "Tren");
        var stack = new StackPanel { Spacing = Density.SpaceSm, Margin = Density.DialogMargin };
        stack.Children.Add(logo);
        stack.Children.Add(new TextBlock
        {
            Text = "Tren",
            FontSize = Density.FontSection,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
        });
        stack.Children.Add(new TextBlock { Text = "Architectural drafting with Kivo and Falcon", FontSize = Density.FontBody });
        if (version.Length > 0)
        {
            stack.Children.Add(new TextBlock { Text = "Version " + version, FontSize = Density.FontBody });
        }
        stack.Children.Add(new TextBlock
        {
            Text = "2D CAD in Rust. Nameplates, schedules and take-off for architectural drawings.",
            TextWrapping = TextWrapping.Wrap,
            FontSize = Density.FontBody,
        });
        stack.Children.Add(Density.Secondary("MIT OR Apache-2.0.", TextWrapping.Wrap));
        Density.Apply(stack);
        _window?.Close();
        var window = new Window
        {
            Title = "About Tren",
            Content = new Grid { RequestedTheme = theme, Children = { stack } },
        };
        StandardWindow.Present(window, owner, WindowTemplate.Dialog);
        window.Closed += (_, _) =>
        {
            if (ReferenceEquals(_window, window))
            {
                _window = null;
            }
        };
        _window = window;
        window.Activate();
        Native.ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(window), 8);
    }
}

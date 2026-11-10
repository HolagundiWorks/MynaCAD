using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;

namespace Aad.WinUi;

/// <summary>The MynaCAD mark from <c>assets/app-icon</c>, copied beside the program.</summary>
internal static class ShellIcon
{
    /// <summary>Apply shell chrome (solid theme + title bar) and the window icon.</summary>
    public static void Apply(Window window)
    {
        ShellWindows.Track(window);
        ShellChrome.Apply(window);
    }

    /// <summary>Apply shell chrome using an explicit theme (owner window or dark/light toggle).</summary>
    public static void Apply(Window window, ElementTheme theme)
    {
        ShellWindows.Track(window);
        ShellChrome.Apply(window, theme);
    }

    public static void SetIcon(Window window)
    {
        var path = Path.Combine(AppContext.BaseDirectory, "aad.ico");
        if (File.Exists(path))
        {
            window.AppWindow.SetIcon(path);
        }
    }

    /// <summary>The suite lockup (mark, name, role, suite line) the welcome screen shows, as MynaHub's does.</summary>
    public static ImageSource? Lockup()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "lockup.png");
        return File.Exists(path) ? new BitmapImage(new Uri(path)) : null;
    }

    public static ImageSource? Logo()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "aad-256.png");
        if (!File.Exists(path))
        {
            return null;
        }
        return new BitmapImage(new Uri(path));
    }
}

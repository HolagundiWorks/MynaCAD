using Microsoft.UI.Xaml;
using WinRT.Interop;

namespace Aad.WinUi;

public partial class App : Application
{
    private Window? _window;

    public App()
    {
        InitializeComponent();
        RequestedTheme = SuiteAppearancePrefs.Load() == ElementTheme.Dark
            ? ApplicationTheme.Dark
            : ApplicationTheme.Light;
        UnhandledException += (_, e) =>
        {
            try
            {
                var path = Path.Combine(Path.GetTempPath(), "aad-winui.log");
                File.AppendAllText(path, e.Exception + Environment.NewLine);
            }
            catch (IOException)
            {
            }
            // Backdrop / composition races must not kill the shell.
            if (e.Exception is ArgumentException)
            {
                e.Handled = true;
            }
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // Resolve installed faces (WinUI has no CSS-style FontFamily fallbacks), then check tokens.
        PerfLog.Phase("OnLaunched");
        ShellFonts.Resolve();
        ShellTheme.VerifyTokens();
        PerfLog.Phase("fonts and theme");
        // The window must stay rooted. A local variable is collected and the window closes.
        // Splash is drawn inside MainWindow (not a second top-level window) so command chrome
        // is not torn down by a competing Activate/Close during startup.
        _window = new MainWindow(Program.Launch);
        PerfLog.Phase("MainWindow built");
        ShellWindows.SetMain(_window);
        // Match suite appearance immediately — omitting the theme forces Dark via ShellChrome.Resolve.
        ShellIcon.Apply(_window, SuiteAppearancePrefs.Load());
        _window.Closed += (_, _) =>
        {
            // Secondary windows otherwise keep the unpackaged process alive.
            ShellWindows.CloseAllOwned();
            try
            {
                Exit();
            }
            catch (Exception)
            {
            }
        };
        PerfLog.Phase("icon applied");
        _window.Activate();
        PerfLog.Phase("activated");
        // Activate does not show the window when this process cannot take the foreground.
        Native.ShowWindow(WindowNative.GetWindowHandle(_window), 8);
    }
}

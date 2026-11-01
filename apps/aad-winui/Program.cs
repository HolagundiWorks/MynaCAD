using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace Aad.WinUi;

public static class Program
{
    internal static LaunchArgs Launch { get; private set; } = new();

    [STAThread]
    public static void Main(string[] args)
    {
        Launch = LaunchArgs.Parse(args);
        // Started on its own: ask the launcher to start O-Draft once a company is signed in and a project is locked, and stop.
        // With no launcher installed the app runs as before.
        if (ProjectLock.NeedsLauncher(Launch) && ProjectLock.RequestLauncher())
        {
            return;
        }
        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            new App();
        });
    }
}

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
        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            new App();
        });
    }
}

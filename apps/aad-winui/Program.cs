using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace Aad.WinUi;

public static class Program
{
    internal static LaunchArgs Launch { get; private set; } = new();

    /// <summary>
    /// One MynaCAD per project: the first start takes the project (mutex, lease, pipe); a second start for the same project asks the first to come to the
    /// front (and to import what <c>--import</c> names) and ends without a window. Returns false when this process should end.
    /// </summary>
    private static bool HoldProject(LaunchArgs launch)
    {
        var instance = ProjectInstance.TryAcquire(launch.Project ?? "");
        if (instance.Acquired)
        {
            return true;
        }
        var project = launch.Project ?? "";
        var reply = ProjectInstance.Forward(project, "{\"request\":\"activate\"}");
        foreach (var path in launch.Imports)
        {
            var line = System.Text.Json.JsonSerializer.Serialize(new Dictionary<string, string> { ["request"] = "import", ["path"] = path });
            reply = ProjectInstance.Forward(project, line, 600000) ?? reply;
        }
        if (reply == null)
        {
            // The other MynaCAD owns the project but does not answer (still starting): say so rather than open a second window.
            Environment.ExitCode = 2;
        }
        return false;
    }

    [STAThread]
    public static void Main(string[] args)
    {
        Launch = LaunchArgs.Parse(args);
        if (Launch.Mode == LaunchMode.Managed && !HoldProject(Launch))
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

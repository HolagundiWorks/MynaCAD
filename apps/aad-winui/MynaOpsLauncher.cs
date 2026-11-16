using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace Aad.WinUi;

/// <summary>
/// Finds and starts MynaOps (<c>O-CoreOp.exe</c>), the front door of a project (docs/ops-first-workflow.md, section 2.4). The places tried are the same as
/// the ones the AI host start used: <c>OSTUDIO_AI_HOST_EXE</c>, the program the last host recorded in <c>host.json</c>, MynaOps beside MynaCAD, and the install
/// folder. Nothing is passed on the command line except the file to import: no secrets.
/// </summary>
internal static class MynaOpsLauncher
{
    private const string ProcessName = "O-CoreOp";

    public static IReadOnlyList<string> Candidates()
    {
        var list = new List<string>();
        var env = Environment.GetEnvironmentVariable("OSTUDIO_AI_HOST_EXE");
        if (!string.IsNullOrWhiteSpace(env))
        {
            list.Add(env.Trim());
        }
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var state = Environment.GetEnvironmentVariable("OSTUDIO_AI_DIR");
        // The same order as the AI client in aad-host and as MynaOps: OSTUDIO_AI_DIR, then ai under OSTUDIO_LOCAL_DIR, then the default folder.
        var scratch = Environment.GetEnvironmentVariable("OSTUDIO_LOCAL_DIR");
        var stateDir = !string.IsNullOrWhiteSpace(state) ? state
            : !string.IsNullOrWhiteSpace(scratch) ? Path.Combine(scratch, "ai")
            : Path.Combine(local, "O-Studio", "ai");
        var hostJson = Path.Combine(stateDir, "host.json");
        try
        {
            if (File.Exists(hostJson) && new FileInfo(hostJson).Length < 65536)
            {
                using var doc = JsonDocument.Parse(File.ReadAllText(hostJson));
                if (doc.RootElement.TryGetProperty("exe", out var exe) && exe.GetString() is { Length: > 0 } recorded)
                {
                    list.Add(recorded);
                }
            }
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException)
        {
        }
        var programs = Path.GetDirectoryName(Path.GetDirectoryName(AppContext.BaseDirectory.TrimEnd('\\', '/')));
        if (!string.IsNullOrEmpty(programs))
        {
            list.Add(Path.Combine(programs, "O-CoreOp", "O-CoreOp.exe"));
        }
        list.Add(Path.Combine(local, "Programs", "O-CoreOp", "O-CoreOp.exe"));
        return list;
    }

    public static string? Find() => Candidates().FirstOrDefault(File.Exists);

    /// <summary>Whether a MynaOps process is running.</summary>
    public static bool IsRunning()
    {
        var found = Process.GetProcessesByName(ProcessName);
        var any = found.Length > 0;
        foreach (var p in found)
        {
            p.Dispose();
        }
        return any;
    }

    /// <summary>
    /// Start MynaOps, or bring the running one to the front (without a file). <paramref name="file"/> is a project to open (passed as it is) or a
    /// drawing to offer for import (<c>--import-pick</c>), see <see cref="FileHandover.ArgumentsFor"/>; a running MynaOps takes the request itself
    /// (the second process forwards it, docs/ops-first-workflow.md 16.4). Returns null when it worked, otherwise what to tell the user.
    /// </summary>
    public static string? Open(string? file = null)
    {
        var handover = FileHandover.ArgumentsFor(file);
        if (handover.Count == 0 && BringToFront())
        {
            return null;
        }
        var exe = Find();
        if (exe == null)
        {
            return "MynaOps was not found. Install MynaOps and MynaCAD together, or set OSTUDIO_AI_HOST_EXE to O-CoreOp.exe.";
        }
        try
        {
            var info = new ProcessStartInfo(exe) { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(exe) ?? "" };
            foreach (var a in handover)
            {
                info.ArgumentList.Add(a);
            }
            using var started = Process.Start(info);
            return null;
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or InvalidOperationException or IOException)
        {
            return "MynaOps could not be started: " + e.Message;
        }
    }

    private static bool BringToFront()
    {
        var shown = false;
        foreach (var p in Process.GetProcessesByName(ProcessName))
        {
            try
            {
                if (p.MainWindowHandle != IntPtr.Zero)
                {
                    ShowWindow(p.MainWindowHandle, 9);
                    SetForegroundWindow(p.MainWindowHandle);
                    shown = true;
                }
            }
            catch (InvalidOperationException)
            {
            }
            finally
            {
                p.Dispose();
            }
        }
        return shown;
    }

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hwnd, int command);
}

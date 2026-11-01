using System.Diagnostics;
using Microsoft.Win32;

namespace Aad.WinUi;

/// <summary>
/// The project is chosen and locked in the launcher (or O-Studio), never in this app. The launcher starts O-Draft with
/// <c>OSTUDIO_PROJECT_OSP</c> (the project file) and <c>OSTUDIO_PROJECT_ID</c>; the drawing opens from it and the commands that open or create a
/// project file are not offered. An app started any other way asks the launcher to start it once a company is signed in and a project is locked.
/// <c>OSTUDIO_STANDALONE=1</c>, a control port, <c>--sample</c> or a file on the command line skip the check (development, automation, tests).
/// </summary>
internal static class ProjectLock
{
    internal const string EnvOsp = "OSTUDIO_PROJECT_OSP";
    internal const string EnvStandalone = "OSTUDIO_STANDALONE";
    internal const string EnvLauncher = "OSTUDIO_LAUNCHER";

    /// <summary>The commands that make or open a project or drawing file. The launcher does that now.</summary>
    private static readonly HashSet<string> Hidden = new(StringComparer.OrdinalIgnoreCase) { "new", "open", "saveas", "project.setup", "project.open" };

    internal static string? Path
    {
        get
        {
            var value = Environment.GetEnvironmentVariable(EnvOsp);
            return string.IsNullOrWhiteSpace(value) ? null : value;
        }
    }

    internal static bool Active => Path != null;

    internal static bool IsHidden(string? commandId) => Active && commandId != null && Hidden.Contains(commandId);

    internal static bool IsLockedFile(string path) =>
        Path is { } locked && string.Equals(System.IO.Path.GetFullPath(locked), System.IO.Path.GetFullPath(path), StringComparison.OrdinalIgnoreCase);

    internal static bool NeedsLauncher(LaunchArgs launch) =>
        !Active
        && Environment.GetEnvironmentVariable(EnvStandalone) != "1"
        && launch.ControlPort is null
        && !launch.Sample
        && launch.Files.Count == 0;

    internal static string? LauncherPath()
    {
        var given = Environment.GetEnvironmentVariable(EnvLauncher);
        if (!string.IsNullOrWhiteSpace(given) && File.Exists(given))
        {
            return given;
        }
        var registered = Registry.GetValue(@"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall\O-Studio", "InstallLocation", null) as string;
        var programs = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "O-Studio");
        foreach (var folder in new[] { registered, programs })
        {
            if (string.IsNullOrWhiteSpace(folder))
            {
                continue;
            }
            var exe = System.IO.Path.Combine(folder, "launcher", "O-Studio.Launcher.exe");
            if (File.Exists(exe))
            {
                return exe;
            }
        }
        return null;
    }

    /// <summary>Ask the launcher to start O-Draft when a project is locked. False when there is no launcher to ask.</summary>
    internal static bool RequestLauncher()
    {
        var exe = LauncherPath();
        if (exe == null)
        {
            return false;
        }
        try
        {
            Process.Start(new ProcessStartInfo(exe, "--launch O-Draft") { UseShellExecute = false, WorkingDirectory = System.IO.Path.GetDirectoryName(exe)! });
            return true;
        }
        catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or InvalidOperationException)
        {
            return false;
        }
    }
}

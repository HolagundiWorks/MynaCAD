using System.Diagnostics;
using Microsoft.Win32;

namespace Aad.WinUi;

/// <summary>
/// O-Draft opens its own drawing and the shared project file (<c>.osp</c>) with O-CoreOp. It does not ask the launcher or O-Studio.
/// A start that still carries <c>OSTUDIO_PROJECT_OSP</c> opens that file. Company details for title blocks come from an imported pack
/// (<c>company.pack.import</c>), not from a live company-file session.
/// </summary>
internal static class ProjectLock
{
    internal const string EnvOsp = "OSTUDIO_PROJECT_OSP";
    internal const string EnvStandalone = "OSTUDIO_STANDALONE";
    internal const string EnvLauncher = "OSTUDIO_LAUNCHER";

    internal static string? Path
    {
        get
        {
            var value = Environment.GetEnvironmentVariable(EnvOsp);
            return string.IsNullOrWhiteSpace(value) ? null : value;
        }
    }

    internal static bool Active => Path != null;

    /// <summary>O-Draft keeps New, Open and Save as. A leftover launcher lock does not hide them.</summary>
    internal static bool IsHidden(string? commandId)
    {
        _ = commandId;
        return false;
    }

    internal static bool IsLockedFile(string path) =>
        Path is { } locked && string.Equals(System.IO.Path.GetFullPath(locked), System.IO.Path.GetFullPath(path), StringComparison.OrdinalIgnoreCase);

    /// <summary>O-Draft does not hand off to the launcher.</summary>
    internal static bool NeedsLauncher(LaunchArgs launch)
    {
        _ = launch;
        return false;
    }

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

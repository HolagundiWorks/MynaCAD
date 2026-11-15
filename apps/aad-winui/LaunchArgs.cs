namespace Aad.WinUi;

/// <summary>How this process was started (docs/ops-first-workflow.md, section 2.1 and 2.4).</summary>
public enum LaunchMode
{
    /// <summary>Started by MynaOps with <c>--managed --project</c>: the project is open, configuration comes from MynaOps.</summary>
    Managed,

    /// <summary>Today's behaviour for tests, <c>aad-cli</c>, the control channel and developers (<c>--standalone</c> or <c>AAD_STANDALONE=1</c>).</summary>
    Standalone,

    /// <summary>Started with no project: the launch screen asks the user to start MynaOps.</summary>
    LaunchScreen,
}

/// <summary>
/// Command-line options forwarded from <c>o-draft.exe</c> (the <c>aad</c> cargo binary) or passed when starting Aad.WinUi directly.
/// <code>ODraft.WinUi.exe --managed --project "C:\p\house.osp" --ops-pid 1234 [--import "plan.dxf"]... [--control 47911]</code>
/// </summary>
public sealed class LaunchArgs
{
    public bool Sample { get; init; }

    public ushort? ControlPort { get; init; }

    /// <summary>Non-option arguments: files handed to the launcher (standalone: opened as before).</summary>
    public IReadOnlyList<string> Files { get; init; } = Array.Empty<string>();

    public bool Managed { get; init; }

    /// <summary>The project file (<c>--project</c>), as given.</summary>
    public string? Project { get; init; }

    /// <summary>The MynaOps process that started this one (<c>--ops-pid</c>).</summary>
    public int? OpsPid { get; init; }

    /// <summary>Files to import into the project after it opens (<c>--import</c>, repeatable).</summary>
    public IReadOnlyList<string> Imports { get; init; } = Array.Empty<string>();

    public bool Standalone { get; init; }

    /// <summary>Why a managed launch is refused (a missing or bad <c>--project</c>); null when it is fine.</summary>
    public string? Error { get; init; }

    /// <summary>Managed wins over standalone: a managed start is never silently downgraded.</summary>
    public LaunchMode Mode => Managed && Error is null ? LaunchMode.Managed : Standalone && !Managed ? LaunchMode.Standalone : LaunchMode.LaunchScreen;

    public static LaunchArgs Parse(IEnumerable<string> args)
    {
        var sample = false;
        var managed = false;
        var standalone = Environment.GetEnvironmentVariable("AAD_STANDALONE") == "1";
        string? project = null;
        int? opsPid = null;
        ushort? control = null;
        var files = new List<string>();
        var imports = new List<string>();
        using var it = args.GetEnumerator();
        while (it.MoveNext())
        {
            var a = it.Current;
            if (a is "--sample")
            {
                sample = true;
                continue;
            }
            if (a is "--managed")
            {
                managed = true;
                continue;
            }
            if (a is "--standalone")
            {
                standalone = true;
                continue;
            }
            if (a is "--project")
            {
                if (it.MoveNext())
                {
                    project = it.Current.Trim();
                }
                continue;
            }
            if (a is "--ops-pid")
            {
                if (it.MoveNext() && int.TryParse(it.Current.Trim(), out var pid) && pid > 0)
                {
                    opsPid = pid;
                }
                continue;
            }
            if (a is "--import")
            {
                if (it.MoveNext() && !string.IsNullOrWhiteSpace(it.Current))
                {
                    imports.Add(it.Current.Trim());
                }
                continue;
            }
            if (a is "--control")
            {
                if (!it.MoveNext())
                {
                    continue;
                }
                if (ushort.TryParse(it.Current.Trim(), out var port))
                {
                    control = port;
                }
                continue;
            }
            if (a.StartsWith("--", StringComparison.Ordinal))
            {
                continue;
            }
            files.Add(a);
        }
        if (control is null
            && ushort.TryParse(Environment.GetEnvironmentVariable("AAD_CONTROL_PORT"), out var fromEnv))
        {
            control = fromEnv;
        }
        string? error = null;
        if (managed)
        {
            error = CheckProject(project);
        }
        return new LaunchArgs
        {
            Sample = sample,
            ControlPort = control,
            Files = files,
            Managed = managed,
            Project = project,
            OpsPid = opsPid,
            Imports = imports,
            Standalone = standalone,
            Error = error,
        };
    }

    /// <summary>A managed start needs an absolute path of an existing project file (<c>.osp</c>); a drawing is never opened as a project.</summary>
    private static string? CheckProject(string? project)
    {
        if (string.IsNullOrWhiteSpace(project))
        {
            return "MynaCAD was started with --managed but without --project.";
        }
        if (project.Length > 4096 || project.Contains('\0') || !Path.IsPathRooted(project))
        {
            return "The project path must be an absolute path.";
        }
        if (!string.Equals(Path.GetExtension(project), ".osp", StringComparison.OrdinalIgnoreCase))
        {
            return "This is not a project file (.osp): " + Path.GetFileName(project);
        }
        if (!File.Exists(project))
        {
            return "The project file does not exist: " + project;
        }
        return null;
    }
}

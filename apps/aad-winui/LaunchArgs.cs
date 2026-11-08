namespace Aad.WinUi;

/// <summary>
/// Command-line options forwarded from <c>o-draft.exe</c> (the <c>aad</c> cargo binary) or passed when starting Aad.WinUi directly.
/// </summary>
public sealed class LaunchArgs
{
    public bool Sample { get; init; }

    public ushort? ControlPort { get; init; }

    public IReadOnlyList<string> Files { get; init; } = Array.Empty<string>();

    public static LaunchArgs Parse(IEnumerable<string> args)
    {
        var sample = false;
        ushort? control = null;
        var files = new List<string>();
        using var it = args.GetEnumerator();
        while (it.MoveNext())
        {
            var a = it.Current;
            if (a is "--sample")
            {
                sample = true;
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
        return new LaunchArgs
        {
            Sample = sample,
            ControlPort = control,
            Files = files,
        };
    }
}

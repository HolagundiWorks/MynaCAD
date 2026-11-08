using System.Diagnostics;

namespace Aad.WinUi;

/// <summary>
/// A switch-on timing log for the UI thread: with <c>AAD_PERF=1</c>, a tick of the shell's 32 ms loop that takes longer than 40 ms is written to
/// <c>%LOCALAPPDATA%\OSAS\odraft-perf.log</c> with the time of each step in it. Off by default and free when off.
/// </summary>
internal static class PerfLog
{
    internal static readonly bool Enabled = Environment.GetEnvironmentVariable("AAD_PERF") == "1";
    private static readonly string Path = System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "OSAS", "odraft-perf.log");

    private static readonly List<(string Step, double Ms)> Steps = new();
    private static readonly Dictionary<Stopwatch, double> Last = new();

    /// <summary>Record the time since the last mark (or since the tick began) under <paramref name="step"/>.</summary>
    internal static void Mark(this Stopwatch watch, string step)
    {
        var now = watch.Elapsed.TotalMilliseconds;
        Last.TryGetValue(watch, out var before);
        Steps.Add((step, now - before));
        Last[watch] = now;
    }

    /// <summary>A startup landmark: the time since the process began, so a slow launch can be split into its steps.</summary>
    internal static void Phase(string name)
    {
        if (!Enabled)
        {
            return;
        }
        var ms = (DateTime.Now - Process.GetCurrentProcess().StartTime).TotalMilliseconds;
        Note($"phase {name} at {ms:F0} ms");
    }

    /// <summary>A free-text line in the log (only when the log is on).</summary>
    internal static void Note(string text)
    {
        if (!Enabled)
        {
            return;
        }
        try
        {
            System.IO.File.AppendAllText(Path, $"{DateTime.Now:HH:mm:ss.fff} note: {text}{Environment.NewLine}");
        }
        catch (System.IO.IOException)
        {
        }
    }

    /// <summary>End of the tick: log it if it was slow.</summary>
    internal static void Report(this Stopwatch watch)
    {
        var total = watch.Elapsed.TotalMilliseconds;
        if (total > 40)
        {
            try
            {
                System.IO.Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path)!);
                var parts = string.Join(", ", Steps.Select(s => $"{s.Step} {s.Ms:F0}"));
                System.IO.File.AppendAllText(Path, $"{DateTime.Now:HH:mm:ss.fff} tick {total:F0} ms: {parts}{Environment.NewLine}");
            }
            catch (System.IO.IOException)
            {
            }
        }
        Steps.Clear();
        Last.Clear();
    }
}

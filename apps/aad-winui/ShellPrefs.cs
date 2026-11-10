using System.Text.Json;

namespace Aad.WinUi;

/// <summary>Shell choices kept beside the user profile. Passwords are never written here.</summary>
internal static class ShellPrefs
{
    private static readonly JsonSerializerOptions Opts = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

    public static Prefs Load()
    {
        try
        {
            var path = Path();
            if (!File.Exists(path))
            {
                return new Prefs();
            }
            return JsonSerializer.Deserialize<Prefs>(File.ReadAllText(path), Opts) ?? new Prefs();
        }
        catch (IOException)
        {
            return new Prefs();
        }
        catch (JsonException)
        {
            return new Prefs();
        }
        catch (UnauthorizedAccessException)
        {
            return new Prefs();
        }
    }

    public static void Save(Prefs prefs)
    {
        try
        {
            var path = Path();
            var dir = System.IO.Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir))
            {
                Directory.CreateDirectory(dir);
            }
            File.WriteAllText(path, JsonSerializer.Serialize(prefs, Opts));
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private static string Path() =>
        System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "AAD", "shell.json");

    internal sealed class Prefs
    {
        public List<string> Tools { get; set; } = new();
        public string Font { get; set; } = "engineering";
        public bool Menu { get; set; } = true;
        /// <summary>Seconds between recovery snapshots of a drawing with unsaved changes (0 turns the timer off; 10 to 3600).</summary>
        public int AutosaveSeconds { get; set; } = 120;
        /// <summary>How many earlier versions of a project file a save keeps in its backups folder (0 to 50).</summary>
        public int BackupCount { get; set; } = 5;
        public bool ToolsVisible { get; set; } = true;
        public bool Tabs { get; set; } = true;
        public bool Status { get; set; } = true;
        public bool Command { get; set; } = true;
        public bool PinLeft { get; set; } = true;
        public bool PinRight { get; set; } = true;
        public bool MergeLayers { get; set; }
        public bool Ucs { get; set; } = true;
        public double BarDx { get; set; }
        public double BarDy { get; set; }
        /// <summary><c>classic</c> or <c>pro</c>.</summary>
        public string Workspace { get; set; } = "classic";
        /// <summary>Per-mode pin, width and toolbar; keys are <c>classic</c> and <c>pro</c>.</summary>
        public Dictionary<string, ModeSlot> ModeSlots { get; set; } = new();
        /// <summary>User-resized tool window sizes (pixels), keyed by stable window id.</summary>
        public Dictionary<string, WindowSize> WindowSizes { get; set; } = new();
        /// <summary>Recently opened drawing paths (newest first), at most 12.</summary>
        public List<string> Recent { get; set; } = new();
    }

    /// <summary>Remember <paramref name="path"/> at the front of the recent list (deduped, capped).</summary>
    public static void RememberRecent(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return;
        }
        var prefs = Load();
        prefs.Recent.RemoveAll(p => string.Equals(p, path, StringComparison.OrdinalIgnoreCase));
        prefs.Recent.Insert(0, path);
        if (prefs.Recent.Count > 12)
        {
            prefs.Recent.RemoveRange(12, prefs.Recent.Count - 12);
        }
        Save(prefs);
    }

    internal sealed class ModeSlot
    {
        public bool PinLeft { get; set; } = true;
        public bool ToolsVisible { get; set; } = true;
        public double LeftWidth { get; set; } = 280;
        public bool LeftCollapsed { get; set; }
    }

    internal sealed class WindowSize
    {
        public int Width { get; set; }
        public int Height { get; set; }
    }
}

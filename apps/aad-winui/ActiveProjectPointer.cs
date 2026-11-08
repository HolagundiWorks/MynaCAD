using System.Text.Json;

namespace Aad.WinUi;

/// <summary>
/// Tells the other suite apps which drawing is open here, so Falcon can open the same project file. It keeps
/// <c>%LOCALAPPDATA%\O-Studio\active-project.json</c> current: written when the active drawing changes (opened, saved under a new name, another tab
/// chosen), deleted when no saved drawing is active or the app closes. <c>project</c> is the drawing's path when it is a project file (<c>.osp</c>), else null.
/// <code>{ "format": "studio-active-project", "version": 1, "app": "O-Draft", "drawing": "C:\p\house.osp", "project": "C:\p\house.osp", "updated_utc": "…" }</code>
/// </summary>
internal static class ActiveProjectPointer
{
    private static string? _last;

    internal static string FilePath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "O-Studio", "active-project.json");

    /// <summary>A project file (<c>.osp</c>): the drawing and its take-off are one file, so Falcon opens the same file and needs no server.</summary>
    internal static bool IsProjectFile(string path) =>
        string.Equals(System.IO.Path.GetExtension(path), ".osp", StringComparison.OrdinalIgnoreCase);

    /// <summary>Point at <paramref name="drawingPath"/>, or clear the pointer when it is null or empty. A write that changes nothing is skipped.</summary>
    internal static void Set(string? drawingPath)
    {
        var path = string.IsNullOrWhiteSpace(drawingPath) ? null : System.IO.Path.GetFullPath(drawingPath);
        var key = path;
        if (key == _last)
        {
            return;
        }
        _last = key;
        try
        {
            if (key is null)
            {
                System.IO.File.Delete(FilePath);
                return;
            }
            System.IO.Directory.CreateDirectory(System.IO.Path.GetDirectoryName(FilePath)!);
            var json = JsonSerializer.Serialize(new Dictionary<string, object?>
            {
                ["format"] = "studio-active-project",
                ["version"] = 1,
                ["app"] = "O-Draft",
                ["drawing"] = path,
                ["project"] = IsProjectFile(path!) ? path : null,
                ["updated_utc"] = DateTime.UtcNow.ToString("o"),
            });
            // write beside, then move over, so a reader never sees half a file
            var tmp = FilePath + ".tmp";
            System.IO.File.WriteAllText(tmp, json);
            System.IO.File.Move(tmp, FilePath, overwrite: true);
        }
        catch (Exception ex) when (ex is System.IO.IOException or UnauthorizedAccessException)
        {
            // the pointer is a convenience for Falcon; never let it disturb drawing work
        }
    }
}

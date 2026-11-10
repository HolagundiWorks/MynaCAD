using System.Text.Json;
using Microsoft.UI.Xaml;

namespace Aad.WinUi;

/// <summary>
/// Shared suite theme preference (<c>%LOCALAPPDATA%\O-Studio\appearance.json</c>),
/// same file as O.Ui.SuiteAppearance so MynaHub, MynaOps and MynaCAD stay in sync.
/// </summary>
internal static class SuiteAppearancePrefs
{
    public static ElementTheme Load()
    {
        try
        {
            var path = Path();
            if (!File.Exists(path))
                return ElementTheme.Dark;
            using var doc = JsonDocument.Parse(File.ReadAllText(path));
            if (doc.RootElement.TryGetProperty("theme", out var theme) &&
                theme.GetString() is { } name)
            {
                if (name.Equals("light", StringComparison.OrdinalIgnoreCase))
                    return ElementTheme.Light;
                if (name.Equals("dark", StringComparison.OrdinalIgnoreCase))
                    return ElementTheme.Dark;
            }
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
        }
        return ElementTheme.Dark;
    }

    public static void Save(ElementTheme theme)
    {
        try
        {
            var path = Path();
            var dir = System.IO.Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            var json = JsonSerializer.Serialize(
                new { theme = theme == ElementTheme.Dark ? "dark" : "light" },
                new JsonSerializerOptions { WriteIndented = true });
            File.WriteAllText(path, json);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
        }
    }

    static string Path() =>
        System.IO.Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "O-Studio",
            "appearance.json");
}

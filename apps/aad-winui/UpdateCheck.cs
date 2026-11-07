using System.Net.Http;
using System.Text.Json;

namespace Aad.WinUi;

/// <summary>
/// Check-only update channel (<c>OSTUDIO_UPDATE_CHANNEL_URL</c>). Does not download or install.
/// </summary>
internal static class UpdateCheck
{
    public const string ProductId = "o-draft";
    public const string EnvUrl = "OSTUDIO_UPDATE_CHANNEL_URL";

    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(15) };

    public sealed record Result(bool Enabled, bool UpdateAvailable, string CurrentVersion, string? LatestVersion, string Message);

    public static async Task<Result> CheckAsync(string currentVersion, CancellationToken cancellationToken = default)
    {
        var url = Environment.GetEnvironmentVariable(EnvUrl);
        if (string.IsNullOrWhiteSpace(url))
        {
            return new Result(false, false, currentVersion, null,
                "Update checks are not configured. Set OSTUDIO_UPDATE_CHANNEL_URL when a channel JSON is published.");
        }

        if (!Uri.TryCreate(url.Trim(), UriKind.Absolute, out var uri) ||
            (uri.Scheme != Uri.UriSchemeHttps && uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != "file"))
        {
            return new Result(true, false, currentVersion, null, "Update channel URL is not a usable http(s) or file URI.");
        }

        try
        {
            string json;
            if (uri.IsFile)
                json = await File.ReadAllTextAsync(uri.LocalPath, cancellationToken).ConfigureAwait(false);
            else
            {
                using var response = await Http.GetAsync(uri, cancellationToken).ConfigureAwait(false);
                response.EnsureSuccessStatusCode();
                json = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            }

            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            if (root.TryGetProperty("schemaVersion", out var schema) &&
                schema.ValueKind == JsonValueKind.String &&
                schema.GetString() is { Length: > 0 } schemaVer &&
                schemaVer != "1")
            {
                return new Result(true, false, currentVersion, null, $"Unsupported channel schemaVersion '{schemaVer}'.");
            }

            if (!root.TryGetProperty("packages", out var packages) || packages.ValueKind != JsonValueKind.Array)
                return new Result(true, false, currentVersion, null, "Channel JSON has no packages array.");

            string? latest = null;
            foreach (var p in packages.EnumerateArray())
            {
                if (!p.TryGetProperty("id", out var id) ||
                    !string.Equals(id.GetString(), ProductId, StringComparison.OrdinalIgnoreCase))
                    continue;
                latest = p.TryGetProperty("version", out var ver) ? ver.GetString() : null;
                break;
            }

            if (latest is null)
                return new Result(true, false, currentVersion, null, $"Channel has no package id '{ProductId}'.");

            var newer = CompareVersions(latest, currentVersion) > 0;
            return new Result(true, newer, currentVersion, latest,
                newer
                    ? $"A newer Tren is listed: {latest} (you have {currentVersion})."
                    : $"You have the latest listed Tren ({currentVersion}).");
        }
        catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException or JsonException or IOException or UnauthorizedAccessException)
        {
            return new Result(true, false, currentVersion, null, $"Could not read the update channel: {ex.Message}");
        }
    }

    internal static int CompareVersions(string a, string b)
    {
        static string[] Parts(string v) => v.Trim().TrimStart('v', 'V').Split('.', StringSplitOptions.RemoveEmptyEntries);
        var left = Parts(a);
        var right = Parts(b);
        var n = Math.Max(left.Length, right.Length);
        for (var i = 0; i < n; i++)
        {
            var ls = i < left.Length ? left[i] : "0";
            var rs = i < right.Length ? right[i] : "0";
            if (int.TryParse(ls, out var li) && int.TryParse(rs, out var ri))
            {
                var c = li.CompareTo(ri);
                if (c != 0) return c;
            }
            else
            {
                var c = string.Compare(ls, rs, StringComparison.OrdinalIgnoreCase);
                if (c != 0) return c;
            }
        }
        return 0;
    }
}

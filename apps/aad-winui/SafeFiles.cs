using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>
/// The shell side of working copies, autosave, crash recovery and backups (docs/NATIVE-OPEN.md, docs/AUTOSAVE-BACKUP.md): the question when a
/// DWG or DXF changed since it was imported, the "Saved hh:mm" status, the offer to restore after a crash, and File &gt; Restore from Backup.
/// All of the work is in the host; this file only asks and shows.
/// </summary>
public sealed partial class MainWindow
{
    private string _saveStateKey = "";

    /// <summary>The JSON result of an application method, or null when it failed (the reason is Native.LastError).</summary>
    private JsonElement? AppCall(string method, object? args = null)
    {
        if (_host == IntPtr.Zero)
        {
            return null;
        }
        var json = Native.App(_host, method, args == null ? "" : JsonSerializer.Serialize(args));
        if (json == null)
        {
            return null;
        }
        try
        {
            using var doc = JsonDocument.Parse(json.Length == 0 ? "null" : json);
            return doc.RootElement.Clone();
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private static string Str(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";

    private static bool Flag(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;

    /// <summary>
    /// Before a DWG or DXF is opened: when it changed since its working copy was made and the working copy has saved edits, ask which one to
    /// open. Returns the open mode (0 decide, 1 import again, 2 open the working copy), or -1 when the user cancelled.
    /// </summary>
    private async Task<int> AskSourceChangedAsync(string path)
    {
        var peek = AppCall("app.peek", new Dictionary<string, string> { ["path"] = path });
        if (peek is not { } p || Str(p, "state") != "changed" || !Flag(p, "edited"))
        {
            return 0;
        }
        var name = System.IO.Path.GetFileName(path);
        var body = new StackPanel { Spacing = Density.SpaceSm, MaxWidth = Density.DialogBodyMaxWidth };
        body.Children.Add(new TextBlock
        {
            Text = name + " has changed since MynaCAD imported it, and the working copy has edits you saved.",
            TextWrapping = TextWrapping.WrapWholeWords,
            FontSize = Density.FontBody,
        });
        body.Children.Add(Density.Secondary(
            "Import again takes the new " + name + "; the working copy as it is now is kept in its backups folder. "
            + "Open the working copy keeps your edits and ignores the change. Working copy: " + Str(p, "native"),
            TextWrapping.WrapWholeWords));
        var dlg = new ContentDialog
        {
            Title = "The drawing changed outside MynaCAD",
            Content = body,
            PrimaryButtonText = "Import again",
            SecondaryButtonText = "Open the working copy",
            CloseButtonText = "Cancel",
            XamlRoot = Content.XamlRoot,
            DefaultButton = ContentDialogButton.Secondary,
        };
        return await dlg.ShowAsync() switch
        {
            ContentDialogResult.Primary => 1,
            ContentDialogResult.Secondary => 2,
            _ => -1,
        };
    }

    /// <summary>After an open: say where the working copy is (the status bar and the command history).</summary>
    private void ShowOpenInfo()
    {
        var info = Native.OpenInfo(_host);
        if (info.Length == 0)
        {
            return;
        }
        try
        {
            using var doc = JsonDocument.Parse(info);
            var message = Str(doc.RootElement, "message");
            if (message.Length > 0)
            {
                Status.Text = message;
                History.Text = message;
            }
        }
        catch (JsonException)
        {
        }
    }

    /// <summary>The status line "Saved 14:32", "Autosaved 14:35", "Save failed: reason" or "Autosave failed: reason". Called about once a second.</summary>
    private void UpdateSaveState()
    {
        if (AppCall("autosave.status") is not { } reply || !reply.TryGetProperty("status", out var s))
        {
            return;
        }
        var kind = Str(s, "kind");
        var unix = s.TryGetProperty("unix", out var u) && u.TryGetInt64(out var secs) ? secs : 0;
        var key = kind + ":" + unix;
        if (key == _saveStateKey)
        {
            return;
        }
        _saveStateKey = key;
        var at = unix > 0 ? DateTimeOffset.FromUnixTimeSeconds(unix).ToLocalTime().ToString("HH:mm") : "";
        var message = Str(s, "message");
        SaveState.Text = kind switch
        {
            "saved" => "Saved " + at,
            "autosaved" => "Autosaved " + at,
            "failed" => "Save failed: " + message,
            "autosaveFailed" => "Autosave failed: " + message,
            _ => "",
        };
        ToolTipService.SetToolTip(SaveState, kind == "saved" ? message : kind == "autosaved" ? "A recovery snapshot of the unsaved drawing was written. Your file is not changed until you save." : null);
        SaveState.Visibility = SaveState.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (kind is "failed" or "autosaveFailed")
        {
            // A failure is announced, not only shown.
            AutomationProperties.SetLiveSetting(SaveState, AutomationLiveSetting.Assertive);
            History.Text = SaveState.Text;
        }
        else
        {
            AutomationProperties.SetLiveSetting(SaveState, AutomationLiveSetting.Polite);
        }
    }

    /// <summary>The autosave interval and the number of backups from the shell preferences (shell.json: autosaveSeconds, backupCount).</summary>
    private void ApplySaveSettings()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        var prefs = ShellPrefs.Load();
        Native.Execute(_host, "setvar", JsonSerializer.Serialize(new Dictionary<string, object> { ["name"] = "AUTOSAVESECS", ["value"] = prefs.AutosaveSeconds }));
        Native.Execute(_host, "setvar", JsonSerializer.Serialize(new Dictionary<string, object> { ["name"] = "BACKUPCOUNT", ["value"] = prefs.BackupCount }));
    }

    /// <summary>On start: when a run of MynaCAD ended without closing its drawings, offer what it had not saved.</summary>
    private async Task OfferRecoveryAsync()
    {
        if (AppCall("recovery.list") is not { } reply || !reply.TryGetProperty("snapshots", out var list) || list.ValueKind != JsonValueKind.Array || list.GetArrayLength() == 0)
        {
            return;
        }
        var body = new StackPanel { Spacing = Density.SpaceSm, MaxWidth = Density.DialogBodyMaxWidth };
        body.Children.Add(new TextBlock
        {
            Text = "MynaCAD did not close these drawings properly last time. Their unsaved changes were kept:",
            TextWrapping = TextWrapping.WrapWholeWords,
            FontSize = Density.FontBody,
        });
        var ids = new List<string>();
        foreach (var item in list.EnumerateArray())
        {
            var m = item.TryGetProperty("manifest", out var manifest) ? manifest : default;
            var unix = m.ValueKind == JsonValueKind.Object && m.TryGetProperty("savedUnix", out var u) && u.TryGetInt64(out var secs) ? secs : 0;
            var when = unix > 0 ? DateTimeOffset.FromUnixTimeSeconds(unix).ToLocalTime().ToString("yyyy-MM-dd HH:mm") : "";
            var path = Str(m, "path");
            body.Children.Add(new TextBlock
            {
                Text = "• " + Str(m, "title") + "  (autosaved " + when + ")" + (path.Length > 0 ? "\n   " + path : "\n   never saved to a file"),
                TextWrapping = TextWrapping.WrapWholeWords,
                FontSize = Density.FontBody,
            });
            ids.Add(Str(item, "id"));
        }
        body.Children.Add(Density.Secondary(
            "Restore opens them with their unsaved changes; nothing is written to your files until you save. Discard deletes the kept changes.",
            TextWrapping.WrapWholeWords));
        var dlg = new ContentDialog
        {
            Title = "Restore unsaved drawings?",
            Content = body,
            PrimaryButtonText = "Restore",
            SecondaryButtonText = "Discard",
            CloseButtonText = "Ask me next time",
            XamlRoot = Content.XamlRoot,
            DefaultButton = ContentDialogButton.Primary,
        };
        var choice = await dlg.ShowAsync();
        if (choice == ContentDialogResult.None)
        {
            return;
        }
        var failed = "";
        foreach (var id in ids)
        {
            var method = choice == ContentDialogResult.Primary ? "recovery.restore" : "recovery.discard";
            if (Native.App(_host, method, JsonSerializer.Serialize(new Dictionary<string, string> { ["id"] = id })) == null)
            {
                failed = Native.LastError(_host);
            }
        }
        if (failed.Length > 0)
        {
            History.Text = failed;
        }
        else if (choice == ContentDialogResult.Primary)
        {
            HideWelcome();
            History.Text = "Restored. Save the drawing to keep it.";
        }
        _docKey = "";
        AfterCommand();
    }

    /// <summary>File &gt; Restore from Backup: the earlier versions a save kept of the open project file; open one to look, or put one back.</summary>
    private async Task ShowBackupsAsync()
    {
        if (AppCall("backup.list") is not { } reply)
        {
            History.Text = Native.LastError(_host);
            return;
        }
        var file = Str(reply, "file");
        var picker = new ListView { SelectionMode = ListViewSelectionMode.Single, MaxHeight = Density.PickerListMaxHeight };
        AutomationProperties.SetName(picker, "Backups");
        var paths = new List<string>();
        if (reply.TryGetProperty("backups", out var backups) && backups.ValueKind == JsonValueKind.Array)
        {
            foreach (var b in backups.EnumerateArray())
            {
                var stamp = Str(b, "stamp");
                var when = DateTimeOffset.TryParseExact(stamp, "yyyyMMdd'T'HHmmss'Z'", System.Globalization.CultureInfo.InvariantCulture,
                    System.Globalization.DateTimeStyles.AssumeUniversal, out var t)
                    ? t.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss")
                    : stamp;
                var size = b.TryGetProperty("bytes", out var n) && n.TryGetInt64(out var bytes) ? (bytes / 1048576.0).ToString("0.0") + " MB" : "";
                picker.Items.Add(new TextBlock { Text = when + "   " + Str(b, "reason") + "   " + size, FontSize = Density.FontBody });
                paths.Add(Str(b, "path"));
            }
        }
        var body = new StackPanel { Spacing = Density.SpaceSm, MaxWidth = Density.DialogBodyMaxWidth };
        if (paths.Count == 0)
        {
            body.Children.Add(new TextBlock { Text = "There are no backups of this project file yet. One is kept each time you save a change.", TextWrapping = TextWrapping.WrapWholeWords, FontSize = Density.FontBody });
        }
        else
        {
            picker.SelectedIndex = 0;
            body.Children.Add(picker);
            body.Children.Add(Density.Secondary(
                "Open a copy shows the backup as a new unnamed drawing and changes nothing. Restore replaces " + System.IO.Path.GetFileName(file)
                + " with the backup; the file as it is now is kept as another backup first. Folder: " + Str(reply, "folder"),
                TextWrapping.WrapWholeWords));
        }
        var dlg = new ContentDialog
        {
            Title = "Restore from backup",
            Content = body,
            PrimaryButtonText = paths.Count > 0 ? "Restore" : "",
            SecondaryButtonText = paths.Count > 0 ? "Open a copy" : "",
            CloseButtonText = "Close",
            XamlRoot = Content.XamlRoot,
            DefaultButton = ContentDialogButton.Close,
        };
        var choice = await dlg.ShowAsync();
        if (choice == ContentDialogResult.None || picker.SelectedIndex < 0 || picker.SelectedIndex >= paths.Count)
        {
            return;
        }
        var backup = paths[picker.SelectedIndex];
        if (choice == ContentDialogResult.Secondary)
        {
            if (Native.App(_host, "backup.open", JsonSerializer.Serialize(new Dictionary<string, string> { ["backup"] = backup })) == null)
            {
                History.Text = Native.LastError(_host);
            }
            _docKey = "";
            AfterCommand();
            return;
        }
        // Restore: unsaved changes of the open drawing would be lost with the file it belongs to, so ask as for a close.
        var active = _documents.FirstOrDefault(d => d.Active);
        if (active != null && active.Dirty && !await DiscardOrSave(active.Title))
        {
            return;
        }
        if (Native.App(_host, "backup.restore", JsonSerializer.Serialize(new Dictionary<string, string> { ["backup"] = backup, ["path"] = file })) == null)
        {
            History.Text = "Restore failed: " + Native.LastError(_host);
            AfterCommand();
            return;
        }
        // The drawing in memory is the one that was replaced: close it and open the restored file.
        if (active != null)
        {
            Native.Execute(_host, "close", JsonSerializer.Serialize(new Dictionary<string, object> { ["index"] = active.Index }));
        }
        if (!await OpenFileAsync(file))
        {
            History.Text = Native.LastError(_host);
        }
        else
        {
            History.Text = "Restored the backup. The version it replaced is in the backups folder.";
        }
        _docKey = "";
        AfterCommand();
    }
}

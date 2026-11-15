using System.Diagnostics;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using WinRT.Interop;

namespace Aad.WinUi;

/// <summary>
/// Managed mode of the shell (docs/ops-first-workflow.md, package B): MynaOps started this MynaCAD with a project; the shell opens it, applies the project
/// configuration when idle, answers the activation pipe, shows what is left of the File menu, and says so when MynaOps closes. The refusals themselves
/// (open, new, save as of project files, configuration edits) are in the host (<c>crates/aad-host/src/managed.rs</c>); this file hides what the host refuses.
/// </summary>
public sealed partial class MainWindow
{
    /// <summary>Menu commands a managed shell does not offer: they open or create projects, or edit what MynaOps configures.</summary>
    private static readonly HashSet<string> ManagedHidden = new(StringComparer.Ordinal)
    {
        "new", "open", "close", "closeall", "project.setup", "project.open", "project.wizard", "company.pack.import",
        "level.new", "level.delete", "level.set", "level.setup", "level.fromfloors", "level.import", "project.merge",
        "hcwlayers", "sheetset.standard", "sheetset.new", "sheetset.delete", "project", "floors", "beams",
    };

    private string? _managedError;
    private bool _opsGone;
    private bool _managedPollBusy;
    private string _managedStateKey = "";
    private bool _managedAdoptShown;

    internal bool IsManaged => _launch.Mode == LaunchMode.Managed;

    /// <summary>The message shown when a managed shell is asked for something MynaOps does.</summary>
    private const string ManagedOpenMessage = "Projects open through MynaOps.";
    private const string ManagedConfigMessage = "This is configured in MynaOps (levels, layer sets, sheet sets, project details).";

    /// <summary>After the host exists: make it managed. Returns false (and keeps the reason) when the project cannot be used.</summary>
    private void InitManagedHost()
    {
        if (!IsManaged || _host == IntPtr.Zero)
        {
            return;
        }
        if (!Native.SetManaged(_host, _launch.Project ?? "", _launch.OpsPid ?? 0))
        {
            _managedError = Native.LastError(_host);
        }
    }

    private bool SameProject(string path)
    {
        if (string.IsNullOrEmpty(path) || string.IsNullOrEmpty(_launch.Project))
        {
            return false;
        }
        try
        {
            return string.Equals(Path.GetFullPath(path), Path.GetFullPath(_launch.Project), StringComparison.OrdinalIgnoreCase);
        }
        catch (ArgumentException)
        {
            return false;
        }
    }

    /// <summary>The menus of a managed shell: no open, new or close, no project wizard, no level or layer-set configuration; Save As becomes an export.</summary>
    private void PruneManagedMenus(List<MenuNode> menus)
    {
        if (!IsManaged)
        {
            return;
        }
        foreach (var menu in menus)
        {
            PruneItems(menu.Items);
        }
    }

    private static void PruneItems(List<MenuNode> items)
    {
        items.RemoveAll(n => n.Id != null && ManagedHidden.Contains(n.Id));
        foreach (var n in items)
        {
            if (n.Id == "saveas")
            {
                n.Title = "Export as DXF or DWG...";
            }
            else if (n.Id == "backup.restore")
            {
                n.Title = "Open a Backup Copy...";
            }
            var had = n.Items.Count;
            PruneItems(n.Items);
            if (had > 0 && n.Items.Count == 0)
            {
                n.Id = null;
                n.Title = "";
            }
        }
        items.RemoveAll(n => string.IsNullOrEmpty(n.Id) && n.Items.Count == 0 && n.Title.Length == 0);
    }

    /// <summary>True (and says why) when a managed shell does not run this menu or toolbar command.</summary>
    private bool ManagedRefuses(string id)
    {
        if (!IsManaged)
        {
            return false;
        }
        if (id is "new" or "open" or "project.setup" or "project.open" or "close" or "closeall")
        {
            History.Text = ManagedOpenMessage;
            return true;
        }
        if (ManagedHidden.Contains(id))
        {
            History.Text = ManagedConfigMessage;
            return true;
        }
        return false;
    }

    /// <summary>Hide the buttons whose work is MynaOps' (levels, project settings) once the chrome exists.</summary>
    private void ApplyManagedChrome()
    {
        if (!IsManaged)
        {
            return;
        }
        AddLevelButton.Visibility = Visibility.Collapsed;
        LevelsButton.Visibility = Visibility.Collapsed;
        Title = "MynaCAD";
    }

    /// <summary>Open the project, apply the configuration before the drawing is shown, then the imports, then serve the pipe.</summary>
    private async Task OpenManagedProjectAsync()
    {
        if (_managedError != null)
        {
            await ShowManagedProblem(_managedError);
            return;
        }
        PerfLog.Phase("opening project");
        if (!await OpenFileAsync(_launch.Project ?? "", managedProject: true))
        {
            await ShowManagedProblem("The project could not be opened: " + Native.LastError(_host));
            return;
        }
        _docKey = "";
        await KeepOnlyActiveDrawingAsync();
        HideWelcome();
        // The configuration is applied before anything else: levels, layer sets, units and details are the project's from the first frame.
        ManagedTick(first: true);
        if (_managedAdoptShown)
        {
            return;
        }
        AfterCommand();
        StartManagedServices();
        foreach (var path in _launch.Imports)
        {
            await ImportPath(path);
        }
        await OfferRecoveryAsync();
    }

    /// <summary>A managed start that cannot continue: say why, then close (there is no scratchpad to fall back to).</summary>
    private async Task ShowManagedProblem(string text)
    {
        HideWelcome();
        _managedAdoptShown = true;
        ManagedState.Text = text;
        var dlg = new ContentDialog
        {
            Title = "MynaCAD cannot open this project",
            Content = new TextBlock { Text = text, TextWrapping = TextWrapping.WrapWholeWords, FontSize = Density.FontBody, MaxWidth = Density.DialogBodyMaxWidth },
            CloseButtonText = "Close MynaCAD",
            XamlRoot = Content.XamlRoot,
        };
        AutomationProperties.SetAutomationId(dlg, "ManagedProblem");
        await dlg.ShowAsync();
        _allowClose = true;
        Close();
    }

    private void StartManagedServices()
    {
        ProjectInstance.Current?.StartServer(HandlePipeRequest);
        if (_launch.OpsPid is int pid)
        {
            try
            {
                var ops = Process.GetProcessById(pid);
                ops.EnableRaisingEvents = true;
                ops.Exited += (_, _) => DispatcherQueue.TryEnqueue(() => SetOpsGone(true));
                if (ops.HasExited)
                {
                    SetOpsGone(true);
                }
            }
            catch (Exception e) when (e is ArgumentException or InvalidOperationException or System.ComponentModel.Win32Exception)
            {
                // The process is already gone.
                SetOpsGone(true);
            }
        }
    }

    private void SetOpsGone(bool gone)
    {
        _opsGone = gone;
        ShowManagedStatus();
    }

    /// <summary>The managed status text: <c>MynaOps closed</c> beats a waiting configuration update.</summary>
    private void ShowManagedStatus()
    {
        var text = _opsGone ? "MynaOps closed" : _managedStateText;
        if (ManagedState.Text != text)
        {
            ManagedState.Text = text;
        }
        ManagedState.Visibility = text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        OpenOpsButton.Visibility = _opsGone ? Visibility.Visible : Visibility.Collapsed;
        ToolTipService.SetToolTip(ManagedState, _opsGone
            ? "MynaOps was closed. You can keep drawing and saving; configuration changes cannot arrive until MynaOps opens this project again."
            : _managedStateText.Length > 0 ? "The project configuration changes when MynaCAD is idle." : null);
        AutomationProperties.SetLiveSetting(ManagedState, AutomationLiveSetting.Assertive);
    }

    private string _managedStateText = "";

    private void OnOpenOps(object sender, RoutedEventArgs e)
    {
        var problem = MynaOpsLauncher.Open();
        if (problem != null)
        {
            History.Text = problem;
        }
    }

    /// <summary>Nothing interactive is going on in the shell: no dialog, no tool window with a form, no busy overlay, no question being asked.</summary>
    private bool ShellIdle()
    {
        if (_asking || _busyDepth > 0 || _formWindow != null || _documents.Count == 0)
        {
            return false;
        }
        var root = Content?.XamlRoot;
        return root == null || VisualTreeHelper.GetOpenPopupsForXamlRoot(root).Count == 0;
    }

    /// <summary>About every three seconds, and once after the open: read the configuration revision and apply a newer one when idle (the host checks the engine).</summary>
    private void ManagedTick(bool first = false)
    {
        if (!IsManaged || _host == IntPtr.Zero || _managedPollBusy || _managedAdoptShown && !first)
        {
            return;
        }
        _managedPollBusy = true;
        try
        {
            if (_opsGone && MynaOpsLauncher.IsRunning())
            {
                // A MynaOps is running again: its Open MynaCAD finds this MynaCAD through the pipe, so this is the re-attach.
                SetOpsGone(false);
            }
            var idle = first || ShellIdle();
            if (AppCall("project.config.poll", new Dictionary<string, object> { ["idle"] = idle }) is not { } r)
            {
                return;
            }
            var state = Str(r, "state");
            var text = Str(r, "text");
            var key = state + ":" + Str(r, "rev") + ":" + text;
            if (state == "unadopted" && !_managedAdoptShown)
            {
                _ = ShowManagedProblem("Adopt this project in MynaOps first. It has levels and settings that MynaOps has not taken over yet.");
                return;
            }
            _managedStateText = state is "waiting" or "update" or "unreadable" or "failed" ? text : "";
            ShowManagedStatus();
            if (state == "applied" && key != _managedStateKey)
            {
                _managedStateKey = key;
                _docKey = "";
                _panelKey = "";
                var detail = "";
                if (r.TryGetProperty("items", out var items) && items.ValueKind == JsonValueKind.Array)
                {
                    detail = string.Join("; ", items.EnumerateArray().Where(i => Str(i, "status") != "ok").Select(i => Str(i, "key") + ": " + Str(i, "detail")));
                }
                History.Text = "Project configuration applied (revision " + Str(r, "rev") + ")" + (detail.Length > 0 ? ". " + detail : ".");
                RefreshChrome();
                AfterCommand();
            }
            else if (state == "failed")
            {
                History.Text = "The project configuration was not applied: " + Str(r, "reason");
            }
        }
        finally
        {
            _managedPollBusy = false;
        }
    }

    /// <summary>Import a file into the project (File > Import, <c>--import</c> and the pipe): the converted copy becomes content of the project's drawing, and nothing is written beside the source.</summary>
    private async Task<string> ImportPath(string path)
    {
        var outcome = "";
        if (_host == IntPtr.Zero)
        {
            return "MynaCAD has no drawing open";
        }
        var name = System.IO.Path.GetFileName(path);
        History.Text = "Importing " + name + "...";
        ShowBusy("Importing", name);
        try
        {
            await Task.Yield();
            var args = JsonSerializer.Serialize(new Dictionary<string, object> { ["path"] = path });
            if (Native.Execute(_host, "import", args) != 0)
            {
                outcome = Native.LastError(_host);
                History.Text = outcome;
            }
            else
            {
                HideWelcome();
                var brought = ImportedLayers(Native.ReadResult(_host));
                if (brought.Count > 0)
                {
                    DispatcherQueue.TryEnqueue(() => OfferLayerMapper(brought));
                }
                outcome = "Imported " + name + " into the project. Save to keep it.";
                History.Text = outcome;
            }
        }
        finally
        {
            HideBusy();
        }
        _docKey = "";
        AfterCommand();
        return outcome;
    }

    // ------------------------------------------------------------------------------------------------ the activation pipe

    /// <summary>One request of the activation pipe (JSON line <c>{"request": ...}</c>); runs the work on the UI thread and returns the reply.</summary>
    private Task<object> HandlePipeRequest(JsonElement request)
    {
        var done = new TaskCompletionSource<object>(TaskCreationOptions.RunContinuationsAsynchronously);
        var kind = request.TryGetProperty("request", out var k) && k.ValueKind == JsonValueKind.String ? k.GetString() ?? "" : "";
        var path = request.TryGetProperty("path", out var p) && p.ValueKind == JsonValueKind.String ? p.GetString() ?? "" : "";
        var save = request.TryGetProperty("save", out var sv) && sv.ValueKind == JsonValueKind.String ? sv.GetString() ?? "ask" : "ask";
        if (!DispatcherQueue.TryEnqueue(async () =>
        {
            try
            {
                done.TrySetResult(await RunPipeRequest(kind, path, save));
            }
            catch (Exception e) when (e is IOException or InvalidOperationException or JsonException or UnauthorizedAccessException)
            {
                done.TrySetResult(new { ok = false, error = e.Message });
            }
        }))
        {
            done.TrySetResult(new { ok = false, error = "MynaCAD is closing" });
        }
        return done.Task;
    }

    private async Task<object> RunPipeRequest(string kind, string path, string save)
    {
        switch (kind)
        {
            case "activate":
                ActivateWindow();
                return new { ok = true };
            case "import":
                if (path.Length == 0 || path.Length > 4096 || path.Contains('\0') || !Path.IsPathRooted(path) || !File.Exists(path))
                {
                    return new { ok = false, error = "import needs the absolute path of an existing file" };
                }
                ActivateWindow();
                var message = await ImportPath(path);
                return message.StartsWith("Imported ", StringComparison.Ordinal) ? new { ok = true, message } : new { ok = false, error = message };
            case "close":
                if (save is not ("ask" or "save" or "discard"))
                {
                    return new { ok = false, error = "save is ask, save or discard" };
                }
                // The reply goes out first; the window closes after it.
                DispatcherQueue.TryEnqueue(async () => await CloseForOps(save));
                return new { ok = true, closing = true };
            case "status":
                return new
                {
                    ok = true,
                    project = _launch.Project,
                    managed = true,
                    dirty = _documents.Any(d => d.Dirty),
                    opsClosed = _opsGone,
                    configuration = _managedStateText,
                };
            case "sync.set":
            case "sync.now":
                return new { ok = false, error = "not available yet (work package D)" };
            default:
                return new { ok = false, error = "unknown request" };
        }
    }

    private void ActivateWindow()
    {
        var hwnd = WindowNative.GetWindowHandle(this);
        Native.ShowWindow(hwnd, 9);
        Activate();
    }

    /// <summary>ask: the usual save question, then close; save: save, then close; discard: close without saving.</summary>
    private async Task CloseForOps(string save)
    {
        if (save == "ask")
        {
            await CloseAll(quit: true);
            return;
        }
        if (save == "save" && _documents.Any(d => d.Dirty) && !await SaveFileAsync(null))
        {
            History.Text = "Not closed: " + Native.LastError(_host);
            return;
        }
        _chrome?.Close();
        _chrome = null;
        ShellWindows.CloseAllOwned();
        _allowClose = true;
        Close();
    }
}

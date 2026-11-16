using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>
/// What MynaCAD shows when it is started without a project (docs/ops-first-workflow.md, section 2.4, decision Q2: no scratchpad): it asks the user to start
/// MynaOps, hands over and exits. A project file (<c>.osp</c>) is passed to MynaOps to open and a drawing as <c>--import-pick</c> (see <see cref="FileHandover"/>); MynaCAD then exits without a window.
/// </summary>
internal sealed class LaunchWindow : Window
{
    private readonly TextBlock _message;
    private readonly Button _open;

    public LaunchWindow(LaunchArgs launch)
    {
        Title = "MynaCAD";
        var logo = new Image { Width = Density.LogoSize, Height = Density.LogoSize, HorizontalAlignment = HorizontalAlignment.Left, Source = ShellIcon.Logo() };
        AutomationProperties.SetName(logo, "MynaCAD");
        var heading = new TextBlock
        {
            Text = "MynaCAD opens projects from MynaOps",
            FontSize = Density.FontSection,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            TextWrapping = TextWrapping.Wrap,
        };
        AutomationProperties.SetAutomationId(heading, "LaunchHeading");
        var detail = new TextBlock
        {
            Text = launch.Error ?? "Start MynaOps, open or create the project there, and press Open MynaCAD.",
            FontSize = Density.FontBody,
            TextWrapping = TextWrapping.Wrap,
        };
        AutomationProperties.SetAutomationId(detail, "LaunchDetail");
        _message = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetAutomationId(_message, "LaunchStatus");
        _open = Density.DialogAction("Open MynaOps", primary: true);
        AutomationProperties.SetAutomationId(_open, "OpenMynaOps");
        _open.Click += (_, _) => Handover(launch);
        var stack = new StackPanel { Spacing = Density.SpaceSm, Margin = Density.DialogMargin };
        stack.Children.Add(logo);
        stack.Children.Add(heading);
        stack.Children.Add(detail);
        stack.Children.Add(_open);
        stack.Children.Add(_message);
        Density.Apply(stack);
        var theme = SuiteAppearancePrefs.Load();
        Content = new Grid { RequestedTheme = theme, Children = { stack } };
    }

    /// <summary>Show the window as a standard dialog (no owner: it is the only window).</summary>
    internal void Show()
    {
        StandardWindow.Present(this, null, WindowTemplate.Dialog);
        Activate();
        Native.ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(this), 8);
    }

    /// <summary>Start (or raise) MynaOps; on success MynaCAD is done, because the MynaCAD that MynaOps starts is the real one.</summary>
    internal void Handover(LaunchArgs launch)
    {
        var file = launch.Files.FirstOrDefault(f => FileHandover.Classify(f) != HandoverKind.None);
        var problem = MynaOpsLauncher.Open(file != null ? Path.GetFullPath(file) : null);
        if (problem != null)
        {
            _message.Text = problem;
            return;
        }
        Close();
    }

    internal bool HasFile(LaunchArgs launch) => launch.Files.Any(f => FileHandover.Classify(f) != HandoverKind.None);
}

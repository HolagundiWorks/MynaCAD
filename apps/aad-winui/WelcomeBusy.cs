using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>
/// Welcome screen (after splash) and open/save progress overlay over the drawing area.
/// </summary>
public sealed partial class MainWindow
{
    private bool _welcomeDismissed;
    private int _busyDepth;

    private async Task HideSplashSoon()
    {
        try
        {
            await Task.Delay(900);
        }
        catch (TaskCanceledException)
        {
        }
        SplashOverlay.Visibility = Visibility.Collapsed;
        // Launch files replace the empty session; otherwise show the welcome screen.
        if (_launch.Files.Count == 0 && !_launch.Sample && !_welcomeDismissed)
        {
            ShowWelcome();
        }
    }

    private void ShowWelcome()
    {
        if (WelcomeOverlay == null)
        {
            return;
        }
        WelcomeLogo.Source = ShellIcon.Logo();
        FillWelcomeRecent();
        WelcomeOverlay.Visibility = Visibility.Visible;
    }

    private void HideWelcome()
    {
        _welcomeDismissed = true;
        if (WelcomeOverlay != null)
        {
            WelcomeOverlay.Visibility = Visibility.Collapsed;
        }
    }

    private void FillWelcomeRecent()
    {
        if (WelcomeRecent == null)
        {
            return;
        }
        WelcomeRecent.Children.Clear();
        var recent = ShellPrefs.Load().Recent
            .Where(p => !string.IsNullOrWhiteSpace(p) && File.Exists(p))
            .Take(8)
            .ToList();
        WelcomeRecentEmpty.Visibility = recent.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        foreach (var path in recent)
        {
            var filePath = path;
            var name = System.IO.Path.GetFileName(filePath);
            var button = new Button
            {
                Content = name,
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
                MinHeight = Density.DialogButtonHeight,
                Padding = Density.RecentItemPadding,
            };
            ToolTipService.SetToolTip(button, filePath);
            AutomationProperties.SetName(button, "Open recent " + name);
            button.Click += async (_, _) =>
            {
                var ok = await OpenFileAsync(filePath);
                if (!ok)
                {
                    History.Text = Native.LastError(_host);
                    return;
                }
                ShellPrefs.RememberRecent(filePath);
                await KeepOnlyActiveDrawingAsync();
                HideWelcome();
                _docKey = "";
                AfterCommand();
            };
            WelcomeRecent.Children.Add(button);
        }
    }

    private void OnWelcomeNew(object sender, RoutedEventArgs e)
    {
        HideWelcome();
        OnNewDrawing(sender, e);
    }

    private async void OnWelcomeOpen(object sender, RoutedEventArgs e)
    {
        await PickFileCore("open");
    }

    private void OnWelcomeWizard(object sender, RoutedEventArgs e)
    {
        HideWelcome();
        OpenProjectWizard();
    }

    private void ShowBusy(string title, string detail)
    {
        _busyDepth++;
        if (BusyOverlay == null)
        {
            return;
        }
        BusyTitle.Text = title;
        BusyDetail.Text = detail;
        BusyRing.IsActive = true;
        BusyOverlay.Visibility = Visibility.Visible;
        AutomationProperties.SetName(BusyDetail, title + " " + detail);
        AutomationProperties.SetLiveSetting(BusyDetail, AutomationLiveSetting.Assertive);
    }

    private void HideBusy()
    {
        if (_busyDepth > 0)
        {
            _busyDepth--;
        }
        if (_busyDepth > 0 || BusyOverlay == null)
        {
            return;
        }
        BusyRing.IsActive = false;
        BusyOverlay.Visibility = Visibility.Collapsed;
        BusyDetail.Text = "";
    }
}

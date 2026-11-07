using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using WinRT.Interop;

namespace Aad.WinUi;

/// <summary>Save / Don't save / Cancel in its own window, so the drawing child does not cover it.</summary>
internal static class SavePrompt
{
    private static Window? _window;

    public static Task<int> Ask(Window owner, string title)
    {
        var done = new TaskCompletionSource<int>();
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;

        var prompt = new TextBlock
        {
            Text = "Save changes to " + title + "?",
            TextWrapping = TextWrapping.Wrap,
            FontSize = Density.FontBody,
        };
        Density.Name(prompt, "Save changes to " + title + "?");

        var save = Ui.Action("Save", ActionKind.Primary);
        var discard = Ui.Action("Don't save");
        var cancel = Ui.Action("Cancel");
        var root = Ui.DialogPage(prompt, Ui.Footer(null, save, discard, cancel));
        root.RequestedTheme = theme;
        Density.Apply(root);

        var window = new Window
        {
            Title = "Tren",
            Content = root,
        };
        var answered = false;
        void Finish(int answer)
        {
            if (answered)
            {
                return;
            }
            answered = true;
            done.TrySetResult(answer);
            window.Close();
        }
        save.Click += (_, _) => Finish(1);
        discard.Click += (_, _) => Finish(2);
        cancel.Click += (_, _) => Finish(0);
        window.AppWindow.Closing += (_, args) =>
        {
            if (!answered)
            {
                answered = true;
                done.TrySetResult(0);
            }
        };
        window.Closed += (_, _) =>
        {
            if (ReferenceEquals(_window, window))
            {
                _window = null;
            }
            if (!answered)
            {
                answered = true;
                done.TrySetResult(0);
            }
        };
        _window = window;
        // Modal: the shell is disabled until the question is answered (Esc = Cancel).
        StandardWindow.Present(window, owner, WindowTemplate.Dialog, modal: true);
        window.Activate();
        Native.ShowWindow(WindowNative.GetWindowHandle(window), 8);
        return done.Task;
    }
}

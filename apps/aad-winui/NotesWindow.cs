using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _notesWindow;

    internal void OpenNotesManager()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }

        if (_notesWindow == null)
        {
            _notesWindow = NotesWindow.Open(this, _host, AfterCommand, () => StartCommand("notes.apply"));
            _notesWindow.Closed += (_, _) => _notesWindow = null;
        }

        PresentToolWindow(_notesWindow);
    }
}

/// <summary>
/// Format &gt; Notes &gt; General Notes: the drawing's note sets, like a library of blocks. Pick a set, edit its notes (one per line), save it
/// under a name, delete it, or place it on the drawing as a numbered list (<c>notes.apply</c>). The sets are saved with the project.
/// </summary>
internal static class NotesWindow
{
    public static Window Open(Window owner, IntPtr host, Action changed, Action place)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Notes status");

        var sets = new SortedDictionary<string, List<string>>(StringComparer.OrdinalIgnoreCase);
        var current = "";
        var loading = false;
        Action closeWindow = () => { };

        var listBox = new ListBox { Height = Density.NotesListHeight, MinWidth = Density.NotesListMinWidth };
        Density.Name(listBox, "Note sets");
        var nameBox = new TextBox { PlaceholderText = "Name of the set", MinHeight = Density.ControlHeight };
        Density.Name(nameBox, "Name of the note set");
        var notesBox = new TextBox
        {
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            MinHeight = Density.NotesBoxMinHeight,
            PlaceholderText = "One note to a line. They are numbered when placed.",
            VerticalAlignment = VerticalAlignment.Stretch,
        };
        Density.Name(notesBox, "Notes of the set, one to a line");

        var newSet = DialogLayout.Action("New set");
        var deleteSet = DialogLayout.Action("Delete set");
        var standard = DialogLayout.Action("Standard notes");
        var save = DialogLayout.Action("Save");
        var placeButton = DialogLayout.Action("Place on drawing", primary: true);
        var close = DialogLayout.Action("Close");

        bool Read()
        {
            sets.Clear();
            if (Native.Execute(host, "notes.list", "{}") != 0)
            {
                status.Text = Native.LastError(host);
                return false;
            }

            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (doc.RootElement.TryGetProperty("sets", out var all) && all.ValueKind == JsonValueKind.Object)
                {
                    foreach (var set in all.EnumerateObject())
                    {
                        var items = new List<string>();
                        if (set.Value.TryGetProperty("items", out var arr) && arr.ValueKind == JsonValueKind.Array)
                        {
                            items.AddRange(arr.EnumerateArray().Select(i => i.GetString() ?? "").Where(i => i.Length > 0));
                        }

                        sets[set.Name] = items;
                    }
                }

                current = doc.RootElement.TryGetProperty("current", out var c) ? c.GetString() ?? "" : "";
            }
            catch (JsonException)
            {
                return false;
            }

            return true;
        }

        void Paint(string? select)
        {
            loading = true;
            listBox.Items.Clear();
            foreach (var name in sets.Keys)
            {
                listBox.Items.Add(name);
            }

            var pick = select ?? (sets.ContainsKey(current) ? current : sets.Keys.FirstOrDefault());
            listBox.SelectedItem = pick;
            nameBox.Text = pick ?? "";
            notesBox.Text = pick != null && sets.TryGetValue(pick, out var items) ? string.Join("\r\n", items) : "";
            loading = false;
        }

        List<string> Lines() =>
            notesBox.Text.Split(["\r\n", "\r", "\n"], StringSplitOptions.None).Select(l => l.Trim()).Where(l => l.Length > 0).ToList();

        bool Save()
        {
            var name = nameBox.Text.Trim();
            if (name.Length == 0)
            {
                status.Text = "Give the set a name.";
                return false;
            }

            var body = new Dictionary<string, object> { ["name"] = name, ["items"] = Lines() };
            if (Native.Execute(host, "notes", JsonSerializer.Serialize(body)) != 0)
            {
                status.Text = Native.LastError(host);
                return false;
            }

            Read();
            Paint(name);
            status.Text = "Saved " + name + " (" + Lines().Count + " notes).";
            changed();
            return true;
        }

        listBox.SelectionChanged += (_, _) =>
        {
            if (loading || listBox.SelectedItem is not string name)
            {
                return;
            }

            loading = true;
            nameBox.Text = name;
            notesBox.Text = sets.TryGetValue(name, out var items) ? string.Join("\r\n", items) : "";
            loading = false;
        };
        newSet.Click += (_, _) =>
        {
            var n = 1;
            while (sets.ContainsKey("Notes " + n))
            {
                n++;
            }

            loading = true;
            listBox.SelectedItem = null;
            nameBox.Text = "Notes " + n;
            notesBox.Text = "";
            loading = false;
            notesBox.Focus(FocusState.Programmatic);
            status.Text = "Type the notes, one to a line, then Save.";
        };
        deleteSet.Click += (_, _) =>
        {
            var name = listBox.SelectedItem as string;
            if (name == null)
            {
                status.Text = "Pick a set to delete.";
                return;
            }

            if (Native.Execute(host, "notes", JsonSerializer.Serialize(new Dictionary<string, object> { ["delete"] = name })) != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }

            Read();
            Paint(null);
            status.Text = "Deleted " + name + ".";
            changed();
        };
        standard.Click += (_, _) =>
        {
            if (Native.Execute(host, "notes.std", "{}") != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }

            Read();
            Paint("General");
            status.Text = "The General set holds typical architectural notes. Edit it and save.";
            changed();
        };
        save.Click += (_, _) => Save();
        placeButton.Click += (_, _) =>
        {
            if (!Save())
            {
                return;
            }

            var name = nameBox.Text.Trim();
            if (Native.Execute(host, "notes", JsonSerializer.Serialize(new Dictionary<string, object> { ["current"] = name })) != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }

            closeWindow();
            place();
        };

        Read();
        Paint(null);

        var left = new StackPanel { Spacing = Density.SpaceSm, MinWidth = Density.NotesLeftMinWidth };
        left.Children.Add(Density.Secondary("Note sets", TextWrapping.NoWrap));
        left.Children.Add(listBox);
        left.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm, Children = { newSet, deleteSet } });
        var right = new StackPanel { Spacing = Density.SpaceSm, MinWidth = Density.NotesRightMinWidth };
        right.Children.Add(Density.Secondary("Name", TextWrapping.NoWrap));
        right.Children.Add(nameBox);
        right.Children.Add(Density.Secondary("Notes", TextWrapping.NoWrap));
        right.Children.Add(notesBox);
        var body2 = new Grid { ColumnSpacing = Density.SpaceLg };
        body2.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        body2.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        body2.Children.Add(left);
        Grid.SetColumn(right, 1);
        body2.Children.Add(right);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin };
        root.Children.Add(DialogLayout.Heading("General notes"));
        root.Children.Add(Density.Secondary(
            "Keep sets of notes for the drawing (general, electrical, plumbing...). Edit a set and save it, or place it on the drawing: it is drawn as a numbered list on the AN-NOTES layer, where you pick its corner.",
            TextWrapping.Wrap));
        root.Children.Add(body2);
        root.Children.Add(status);
        root.Children.Add(DialogLayout.ActionRow(standard, save, placeButton, close));

        Density.Apply(root);
        // Density.Apply gives text boxes and lists the standard control height; these two need room for the notes.
        notesBox.MinHeight = Density.NotesBoxMinHeight;
        listBox.Height = Density.NotesListHeight;
        var window = new Window
        {
            Title = "General notes",
            Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { root } } },
        };
        closeWindow = window.Close;
        close.Click += (_, _) => window.Close();
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "generalnotes", new WindowBounds(560, 330, 900, 700));
        return window;
    }
}

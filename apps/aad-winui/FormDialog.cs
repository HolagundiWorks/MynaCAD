using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using WinRT.Interop;

namespace Aad.WinUi;

/// <summary>Settings dialog for an engine form. A separate window, because the drawing child covers XAML.</summary>
internal static class FormDialog
{
    public static void Show(MainWindow owner, IntPtr host, string json, Action done)
    {
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        var id = root.GetProperty("id").GetString() ?? "";
        var title = root.GetProperty("title").GetString() ?? id;
        var fields = root.GetProperty("fields");
        var rooms = root.TryGetProperty("rooms", out var roomList) ? roomList : default;

        var stack = new StackPanel { Spacing = Density.SpaceMd };
        var scalars = new Dictionary<string, TextBox>();
        var checks = new Dictionary<string, CheckBox>();
        var choices = new Dictionary<string, ComboBox>();
        var tables = new Dictionary<string, (JsonElement Columns, StackPanel Rows)>();
        var picks = false;

        foreach (var field in fields.EnumerateArray())
        {
            var key = field.GetProperty("key").GetString() ?? "";
            var label = field.GetProperty("label").GetString() ?? key;
            var kind = field.GetProperty("kind").GetString() ?? "text";
            if (field.TryGetProperty("pick", out var pick) && pick.ValueKind == JsonValueKind.True)
            {
                picks = true;
                continue;
            }
            // Label sits directly above its field (Space1); fields are separated by Space2.
            var group = new StackPanel { Spacing = Density.Space1 };
            group.Children.Add(Density.Secondary(label, TextWrapping.Wrap));
            stack.Children.Add(group);
            switch (kind)
            {
                case "bool":
                    var box = new CheckBox { IsChecked = BoolOf(field) };
                    Density.Name(box, label);
                    checks[key] = box;
                    group.Children.Add(box);
                    break;
                case "choice":
                case "room":
                    var combo = Density.DialogCombo(label);
                    if (kind == "room" && rooms.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var room in rooms.EnumerateArray())
                        {
                            var code = room.GetProperty("code").GetString() ?? "";
                            var name = room.GetProperty("name").GetString() ?? code;
                            combo.Items.Add(new ComboBoxItem { Content = code + "  " + name, Tag = code });
                        }
                    }
                    else if (field.TryGetProperty("choices", out var list))
                    {
                        foreach (var choice in list.EnumerateArray())
                        {
                            var text = choice.GetString() ?? "";
                            combo.Items.Add(new ComboBoxItem { Content = text.Length == 0 ? "(default)" : text, Tag = text });
                        }
                    }
                    Select(combo, StringOf(field));
                    choices[key] = combo;
                    group.Children.Add(combo);
                    break;
                case "table":
                    var columns = field.GetProperty("columns");
                    var rows = new StackPanel { Spacing = Density.SpaceSm };
                    if (field.TryGetProperty("value", out var value) && value.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var row in value.EnumerateArray())
                        {
                            rows.Children.Add(TableRow(columns, row));
                        }
                    }
                    if (rows.Children.Count == 0)
                    {
                        rows.Children.Add(TableRow(columns, default));
                    }
                    var add = Density.DialogAction("Add row");
                    add.HorizontalAlignment = HorizontalAlignment.Left;
                    Density.Name(add, "Add row to " + label);
                    add.Click += (_, _) =>
                    {
                        if (rows.Children.Count < 24)
                        {
                            rows.Children.Add(TableRow(columns, default));
                        }
                    };
                    tables[key] = (columns.Clone(), rows);
                    group.Children.Add(rows);
                    group.Children.Add(add);
                    break;
                default:
                    var input = Density.DialogField(StringOf(field), label);
                    scalars[key] = input;
                    group.Children.Add(input);
                    break;
            }
        }
        if (picks)
        {
            stack.Children.Add(Density.Secondary("Points are picked on the drawing after OK.", TextWrapping.Wrap));
        }
        // Suite-linked company identity: Project Settings fields from Studio are refresh-only.
        if (id == "project" && Native.Execute(host, "project", "{}") == 0)
        {
            try
            {
                using var suiteDoc = JsonDocument.Parse(Native.ReadResult(host));
                if (suiteDoc.RootElement.TryGetProperty("suite", out var suite)
                    && suite.ValueKind == JsonValueKind.Object
                    && suite.TryGetProperty("locked_details", out var locked)
                    && locked.ValueKind == JsonValueKind.Array)
                {
                    var names = new HashSet<string>(StringComparer.Ordinal);
                    foreach (var item in locked.EnumerateArray())
                    {
                        if (item.ValueKind == JsonValueKind.String && item.GetString() is { Length: > 0 } name)
                        {
                            names.Add(name);
                        }
                    }
                    foreach (var (key, box) in scalars)
                    {
                        if (names.Contains(key))
                        {
                            box.IsReadOnly = true;
                        }
                    }
                    if (names.Count > 0)
                    {
                        stack.Children.Add(Density.Secondary(
                            "Some fields come from Studio (company file) and cannot be edited here. Change them in Studio and refresh with Company File Details.",
                            TextWrapping.Wrap));
                    }
                }
            }
            catch (JsonException)
            {
            }
        }
        var error = Ui.Status("Form error");
        AutomationProperties.SetLiveSetting(error, AutomationLiveSetting.Assertive);
        var ok = Ui.Action("OK", ActionKind.Primary);
        var cancel = Ui.Action("Cancel");

        Density.Apply(stack);
        // Fields scroll; the footer with the validation line and actions stays pinned.
        var page = Ui.DialogPage(stack, Ui.Footer(error, ok, cancel));
        page.RequestedTheme = ShellChrome.ThemeOf(owner);
        var window = new Window { Title = title, Content = page };
        // PropertyDialog (user-resizable) avoids Adaptive SizeChanged↔Resize flicker.
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "form", new WindowBounds(360, 150, 720, 800));
        ok.Click += (_, _) =>
        {
            var preset = Collect(scalars, checks, choices, tables);
            if (Native.StartForm(host, id, preset) != 0)
            {
                error.Text = Native.LastError(host);
                ShellFit.Invalidate(window);
                return;
            }
            window.Close();
            done();
        };
        cancel.Click += (_, _) => window.Close();
        window.Closed += (_, _) => owner.HoldForm(null);
        owner.HoldForm(window);
        window.Activate();
        Native.ShowWindow(WindowNative.GetWindowHandle(window), 8);
    }

    private static string Collect(
        Dictionary<string, TextBox> scalars,
        Dictionary<string, CheckBox> checks,
        Dictionary<string, ComboBox> choices,
        Dictionary<string, (JsonElement Columns, StackPanel Rows)> tables)
    {
        var body = new Dictionary<string, object?>();
        foreach (var (key, box) in scalars)
        {
            body[key] = box.Text;
        }
        foreach (var (key, box) in checks)
        {
            body[key] = box.IsChecked == true;
        }
        foreach (var (key, box) in choices)
        {
            body[key] = box.SelectedItem is ComboBoxItem item ? item.Tag as string ?? "" : "";
        }
        foreach (var (key, table) in tables)
        {
            var rows = new List<List<string>>();
            foreach (var child in table.Rows.Children)
            {
                if (child is not StackPanel row)
                {
                    continue;
                }
                var cells = new List<string>();
                foreach (var cell in row.Children)
                {
                    if (cell is TextBox text)
                    {
                        cells.Add(text.Text);
                    }
                }
                rows.Add(cells);
            }
            body[key] = rows;
        }
        return JsonSerializer.Serialize(body);
    }

    private static StackPanel TableRow(JsonElement columns, JsonElement row)
    {
        var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceSm };
        var i = 0;
        foreach (var column in columns.EnumerateArray())
        {
            var text = "";
            if (row.ValueKind == JsonValueKind.Array && i < row.GetArrayLength())
            {
                text = Cell(row[i]);
            }
            else if (row.ValueKind == JsonValueKind.Object && column.TryGetProperty("key", out var key))
            {
                var name = key.GetString() ?? "";
                if (row.TryGetProperty(name, out var cell))
                {
                    text = Cell(cell);
                }
            }
            var label = column.GetProperty("label").GetString() ?? "Cell";
            var box = Density.DialogField(text, label);
            box.Width = Density.TableCellWidth;
            box.PlaceholderText = label;
            line.Children.Add(box);
            i++;
        }
        return line;
    }

    private static void Select(ComboBox combo, string value)
    {
        foreach (var item in combo.Items)
        {
            if (item is ComboBoxItem choice && choice.Tag as string == value)
            {
                combo.SelectedItem = choice;
                return;
            }
        }
        if (combo.Items.Count > 0)
        {
            combo.SelectedIndex = 0;
        }
    }

    private static bool BoolOf(JsonElement field)
    {
        if (!field.TryGetProperty("value", out var value))
        {
            return false;
        }
        if (value.ValueKind == JsonValueKind.True)
        {
            return true;
        }
        return value.ValueKind == JsonValueKind.String && value.GetString() == "true";
    }

    private static string StringOf(JsonElement field)
    {
        if (!field.TryGetProperty("value", out var value) || value.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
        {
            return field.TryGetProperty("default", out var fallback) ? fallback.GetString() ?? "" : "";
        }
        return Cell(value);
    }

    private static string Cell(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.String => value.GetString() ?? "",
        JsonValueKind.Number => value.ToString(),
        JsonValueKind.True => "true",
        JsonValueKind.False => "false",
        _ => "",
    };
}

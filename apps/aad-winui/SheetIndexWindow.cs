using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _sheetIndexWindow;

    internal void OpenSheetIndexWindow()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_sheetIndexWindow == null)
        {
            _sheetIndexWindow = SheetIndexWindow.Open(this, _host, AfterCommand);
            _sheetIndexWindow.Closed += (_, _) => _sheetIndexWindow = null;
        }
        PresentToolWindow(_sheetIndexWindow);
    }
}

/// <summary>
/// The structured sheet set: numbered sheets grouped by discipline, their title-block fields, revisions,
/// issue sets and publish by issue or discipline. Every action is an engine command
/// (<c>sheet.*</c>, <c>sheetindex.*</c>, <c>issue.*</c>); this window only shows and edits the records.
/// Design: docs/sheet-index.md.
/// </summary>
internal static class SheetIndexWindow
{
    private sealed record Entry(string Kind, string Key, string Text)
    {
        public override string ToString() => Text;
    }

    private static readonly string[] Statuses = { "draft", "review", "approved", "issued", "void" };

    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Sheet index status");

        var tree = new TreeView
        {
            SelectionMode = TreeViewSelectionMode.Single,
            Height = Density.TreeHeight,
            CanDragItems = true,
            CanReorderItems = true,
            AllowDrop = true,
        };
        Density.Name(tree, "Sheet index");
        var sheetsRoot = new TreeViewNode { Content = new Entry("root", "", "Sheets"), IsExpanded = true };
        var issuesRoot = new TreeViewNode { Content = new Entry("root", "", "Issue sets"), IsExpanded = true };
        tree.RootNodes.Add(sheetsRoot);
        tree.RootNodes.Add(issuesRoot);

        var title = Density.DialogField("", "Title");
        var number = Density.DialogField("", "Sheet number");
        var discipline = Density.DialogCombo("Discipline");
        var scale = Density.DialogField("", "Scale");
        var paper = Density.DialogCombo("Paper");
        foreach (var p in new[] { "", "A0", "A1", "A2", "A3", "A4" })
        {
            paper.Items.Add(p);
        }
        var sheetStatus = Density.DialogCombo("Sheet status");
        foreach (var st in Statuses)
        {
            sheetStatus.Items.Add(st);
        }
        var drawn = Density.DialogField("", "Drawn by");
        var checkedBy = Density.DialogField("", "Checked by");
        var approved = Density.DialogField("", "Approved by");
        var date = Density.DialogField("", "Date");
        var note = Density.DialogField("", "Revision or issue text");

        var scope = Density.DialogCombo("Publish scope");
        var format = Density.DialogCombo("Publish format");
        format.Items.Add("pdf");
        format.Items.Add("dwg");
        format.SelectedIndex = 0;
        var single = new CheckBox { Content = "One file", IsChecked = false, MinHeight = Density.ControlHeight };
        Density.Name(single, "One file");

        var add = DialogLayout.Action("Add sheet");
        var apply = DialogLayout.Action("Apply fields");
        var delete = DialogLayout.Action("Delete");
        var up = DialogLayout.Action("Move up");
        var down = DialogLayout.Action("Move down");
        var renumber = DialogLayout.Action("Renumber");
        var build = DialogLayout.Action("Build from levels");
        var table = DialogLayout.Action("Draw sheet list");
        var addRev = DialogLayout.Action("Add revision");
        var newIssue = DialogLayout.Action("New issue set");
        var publish = DialogLayout.Action("Publish", primary: true);
        var close = DialogLayout.Action("Close");

        bool Exec(string id, object body)
        {
            if (Native.Execute(host, id, JsonSerializer.Serialize(body)) == 0)
            {
                return true;
            }
            status.Text = Native.LastError(host);
            return false;
        }

        JsonDocument? Result()
        {
            try
            {
                return JsonDocument.Parse(Native.ReadResult(host));
            }
            catch (JsonException)
            {
                status.Text = "The sheet index could not be read.";
                return null;
            }
        }

        static string Str(JsonElement e, string key) =>
            e.TryGetProperty(key, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";

        Entry? Selected() => tree.SelectedNodes.Count > 0 ? tree.SelectedNodes[0].Content as Entry : null;

        var disciplineCodes = new List<string>();
        var rows = new Dictionary<string, JsonElement>();
        var sheetDocs = new List<JsonDocument>();

        void Reload()
        {
            foreach (var d in sheetDocs)
            {
                d.Dispose();
            }
            sheetDocs.Clear();
            rows.Clear();
            sheetsRoot.Children.Clear();
            issuesRoot.Children.Clear();
            disciplineCodes.Clear();
            discipline.Items.Clear();
            var keepScope = scope.SelectedItem as string;
            scope.Items.Clear();
            scope.Items.Add("All sheets");

            if (Exec("sheet.list", new { }))
            {
                var doc = Result();
                if (doc != null)
                {
                    sheetDocs.Add(doc);
                    var root = doc.RootElement;
                    var names = new Dictionary<string, string>();
                    if (root.TryGetProperty("disciplines", out var ds) && ds.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var d in ds.EnumerateArray())
                        {
                            var code = Str(d, "code");
                            names[code] = Str(d, "name");
                            disciplineCodes.Add(code);
                            discipline.Items.Add(code);
                            scope.Items.Add("Discipline " + code);
                        }
                    }
                    var groups = new Dictionary<string, TreeViewNode>();
                    foreach (var code in disciplineCodes)
                    {
                        var node = new TreeViewNode { Content = new Entry("discipline", code, code + "  " + names[code]), IsExpanded = true };
                        groups[code] = node;
                    }
                    if (root.TryGetProperty("sheets", out var sheets) && sheets.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var sh in sheets.EnumerateArray())
                        {
                            var num = Str(sh, "number");
                            rows[num] = sh;
                            var rev = Str(sh, "rev");
                            var text = $"{num}  {Str(sh, "title")}  {Str(sh, "scale")}"
                                + (rev.Length > 0 ? "  Rev " + rev : "")
                                + "  [" + Str(sh, "status") + "]"
                                + (sh.TryGetProperty("layoutExists", out var le) && le.ValueKind == JsonValueKind.True ? "" : "  (no layout)");
                            var leaf = new TreeViewNode { Content = new Entry("sheet", num, text) };
                            if (groups.TryGetValue(Str(sh, "discipline").ToUpperInvariant(), out var g))
                            {
                                g.Children.Add(leaf);
                            }
                            else
                            {
                                sheetsRoot.Children.Add(leaf);
                            }
                        }
                    }
                    foreach (var code in disciplineCodes)
                    {
                        if (groups[code].Children.Count > 0)
                        {
                            sheetsRoot.Children.Add(groups[code]);
                        }
                    }
                }
            }
            if (Exec("issue.list", new { }))
            {
                using var doc = Result();
                if (doc != null && doc.RootElement.TryGetProperty("issues", out var issues) && issues.ValueKind == JsonValueKind.Array)
                {
                    foreach (var i in issues.EnumerateArray())
                    {
                        var name = Str(i, "name");
                        var count = i.TryGetProperty("sheets", out var s) && s.ValueKind == JsonValueKind.Array ? s.GetArrayLength() : 0;
                        issuesRoot.Children.Add(new TreeViewNode { Content = new Entry("issue", name, $"{name}  Rev {Str(i, "rev")}  {Str(i, "date")}  ({count} sheets)") });
                        scope.Items.Add("Issue " + name);
                    }
                }
            }
            scope.SelectedItem = keepScope != null && scope.Items.Contains(keepScope) ? keepScope : "All sheets";
        }

        void ShowSelected()
        {
            var sel = Selected();
            if (sel?.Kind != "sheet" || !rows.TryGetValue(sel.Key, out var r))
            {
                return;
            }
            title.Text = Str(r, "title");
            number.Text = Str(r, "number");
            discipline.SelectedItem = Str(r, "discipline");
            scale.Text = Str(r, "scale");
            paper.SelectedItem = Str(r, "paper");
            sheetStatus.SelectedItem = Str(r, "status");
            drawn.Text = Str(r, "drawn");
            checkedBy.Text = Str(r, "checked");
            approved.Text = Str(r, "approved");
            date.Text = Str(r, "date");
        }
        tree.SelectionChanged += (_, _) => ShowSelected();

        void Done(string message)
        {
            status.Text = message;
            changed();
            Reload();
        }

        // A sheet's position among its discipline's sheets, from the tree order.
        bool MoveTo(string sheet, TreeViewNode leaf, bool announce)
        {
            var parent = leaf.Parent;
            if (parent?.Content is not Entry { Kind: "discipline" } group)
            {
                return false;
            }
            var pos = parent.Children.IndexOf(leaf);
            var ok = Exec("sheet.move", new Dictionary<string, object> { ["sheet"] = sheet, ["position"] = Math.Max(pos, 0), ["discipline"] = group.Key });
            if (ok && announce)
            {
                Done("Moved " + sheet + ".");
            }
            return ok;
        }

        tree.DragItemsCompleted += (_, args) =>
        {
            if (args.Items.Count == 1 && args.Items[0] is TreeViewNode { Content: Entry { Kind: "sheet" } e } leaf)
            {
                if (!MoveTo(e.Key, leaf, announce: true))
                {
                    Reload(); // an illegal drop (outside a discipline): put the tree back
                }
            }
            else
            {
                Reload();
            }
        };

        void Step(int delta)
        {
            var sel = Selected();
            if (sel?.Kind != "sheet")
            {
                status.Text = "Select a sheet.";
                return;
            }
            var node = tree.SelectedNodes[0];
            var siblings = node.Parent?.Children;
            if (siblings == null)
            {
                return;
            }
            var at = siblings.IndexOf(node) + delta;
            if (at < 0 || at >= siblings.Count)
            {
                return;
            }
            if (Exec("sheet.move", new Dictionary<string, object> { ["sheet"] = sel.Key, ["position"] = at }))
            {
                Done("Moved " + sel.Key + ".");
            }
        }
        up.Click += (_, _) => Step(-1);
        down.Click += (_, _) => Step(1);

        add.Click += (_, _) =>
        {
            if (string.IsNullOrWhiteSpace(title.Text))
            {
                status.Text = "Type a title for the new sheet.";
                return;
            }
            var body = new Dictionary<string, object> { ["title"] = title.Text.Trim() };
            if (discipline.SelectedItem is string d && d.Length > 0)
            {
                body["discipline"] = d;
            }
            if (!string.IsNullOrWhiteSpace(scale.Text))
            {
                body["scale"] = scale.Text.Trim();
            }
            if (Exec("sheet.new", body))
            {
                Done("Added a sheet.");
            }
        };
        apply.Click += (_, _) =>
        {
            var sel = Selected();
            if (sel?.Kind != "sheet")
            {
                status.Text = "Select a sheet.";
                return;
            }
            var body = new Dictionary<string, object>
            {
                ["sheet"] = sel.Key,
                ["number"] = number.Text.Trim(),
                ["title"] = title.Text.Trim(),
                ["scale"] = scale.Text.Trim(),
                ["drawn"] = drawn.Text.Trim(),
                ["checked"] = checkedBy.Text.Trim(),
                ["approved"] = approved.Text.Trim(),
                ["date"] = date.Text.Trim(),
            };
            if (discipline.SelectedItem is string d && d.Length > 0)
            {
                body["discipline"] = d;
            }
            if (paper.SelectedItem is string p)
            {
                body["paper"] = p;
            }
            if (sheetStatus.SelectedItem is string st)
            {
                body["status"] = st;
            }
            if (Exec("sheet.set", body))
            {
                Done("Updated " + number.Text.Trim() + " and its title block.");
            }
        };
        delete.Click += (_, _) =>
        {
            var sel = Selected();
            if (sel?.Kind != "sheet")
            {
                status.Text = "Select a sheet.";
                return;
            }
            if (Exec("sheet.delete", new { sheet = sel.Key }))
            {
                Done("Deleted " + sel.Key + " (its layout is kept).");
            }
        };
        renumber.Click += (_, _) =>
        {
            if (Exec("sheet.renumber", new { }))
            {
                Done("Renumbered the sheets.");
            }
        };
        build.Click += (_, _) =>
        {
            if (!Exec("sheetindex.build", new { }))
            {
                return;
            }
            using var doc = Result();
            var n = doc != null && doc.RootElement.TryGetProperty("created", out var c) ? c.GetArrayLength() : 0;
            Done(n == 0 ? "Every level already has a sheet." : $"Built {n} sheet(s).");
        };
        table.Click += (_, _) =>
        {
            if (Exec("sheetindex.table", new { }))
            {
                Done("Drew the sheet list on layout Sheet Index.");
            }
        };
        addRev.Click += (_, _) =>
        {
            var sel = Selected();
            if (sel?.Kind != "sheet" || string.IsNullOrWhiteSpace(note.Text))
            {
                status.Text = "Select a sheet and type the revision text.";
                return;
            }
            if (Exec("sheet.revision.add", new { sheet = sel.Key, description = note.Text.Trim() }))
            {
                Done("Added a revision to " + sel.Key + ".");
            }
        };
        newIssue.Click += (_, _) =>
        {
            if (string.IsNullOrWhiteSpace(note.Text))
            {
                status.Text = "Type a name for the issue set.";
                return;
            }
            var body = new Dictionary<string, object> { ["name"] = note.Text.Trim() };
            if ((scope.SelectedItem as string)?.StartsWith("Discipline ", StringComparison.Ordinal) == true)
            {
                body["discipline"] = ((string)scope.SelectedItem)["Discipline ".Length..];
            }
            if (Exec("issue.create", body))
            {
                Done("Created issue " + note.Text.Trim() + ": its sheets are frozen and marked issued.");
            }
        };
        publish.Click += (_, _) =>
        {
            var body = new Dictionary<string, object>
            {
                ["format"] = format.SelectedItem as string ?? "pdf",
                ["singleFile"] = single.IsChecked == true,
            };
            var pick = scope.SelectedItem as string ?? "All sheets";
            if (pick.StartsWith("Issue ", StringComparison.Ordinal))
            {
                body["issue"] = pick["Issue ".Length..];
            }
            else if (pick.StartsWith("Discipline ", StringComparison.Ordinal))
            {
                body["discipline"] = pick["Discipline ".Length..];
            }
            if (!Exec("sheetindex.publish", body))
            {
                return;
            }
            using var doc = Result();
            var text = doc != null && doc.RootElement.TryGetProperty("message", out var m) ? m.GetString() ?? "Published." : "Published.";
            if (doc != null && doc.RootElement.TryGetProperty("skipped", out var sk) && sk.ValueKind == JsonValueKind.Array && sk.GetArrayLength() > 0)
            {
                text += $" {sk.GetArrayLength()} sheet(s) skipped (void or no layout).";
            }
            status.Text = text;
            changed();
        };

        var root = new StackPanel { Spacing = Density.SpaceSm, Margin = Density.DialogMargin };
        root.Children.Add(DialogLayout.Heading("Sheet index"));
        root.Children.Add(Density.Secondary("Numbered sheets by discipline. Drag a sheet to reorder it; Renumber then assigns the numbers in that order.", TextWrapping.Wrap));
        root.Children.Add(tree);
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { add, delete, up, down, renumber } });
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { build, table } });
        root.Children.Add(DialogLayout.Labeled("Title", title));
        root.Children.Add(DialogLayout.Labeled("Sheet number", number));
        root.Children.Add(DialogLayout.Labeled("Discipline", discipline));
        root.Children.Add(DialogLayout.Labeled("Scale", scale));
        root.Children.Add(DialogLayout.Labeled("Paper", paper));
        root.Children.Add(DialogLayout.Labeled("Status", sheetStatus));
        root.Children.Add(DialogLayout.Labeled("Drawn by", drawn));
        root.Children.Add(DialogLayout.Labeled("Checked by", checkedBy));
        root.Children.Add(DialogLayout.Labeled("Approved by", approved));
        root.Children.Add(DialogLayout.Labeled("Date", date));
        root.Children.Add(apply);
        root.Children.Add(DialogLayout.Labeled("Revision text or issue name", note));
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { addRev, newIssue } });
        root.Children.Add(DialogLayout.Heading("Publish"));
        root.Children.Add(DialogLayout.Labeled("Sheets", scope));
        root.Children.Add(DialogLayout.Labeled("Format", format));
        root.Children.Add(single);
        root.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, Children = { publish, close } });
        root.Children.Add(status);
        Density.Apply(root);

        var window = new Window
        {
            Title = "Sheet index",
            Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { root } } },
        };
        close.Click += (_, _) => window.Close();
        window.Closed += (_, _) =>
        {
            foreach (var d in sheetDocs)
            {
                d.Dispose();
            }
        };
        StandardWindow.Present(window, owner, WindowTemplate.Manager, "sheet-index", new WindowBounds(560, 480, 1000, 1000));
        Reload();
        status.Text = "Sheets, revisions and issue sets for this drawing.";
        return window;
    }
}

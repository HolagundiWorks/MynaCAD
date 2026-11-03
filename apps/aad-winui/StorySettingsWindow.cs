using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _storySettingsWindow;

    internal void OpenStorySettings(long? selectLevelId = null)
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_storySettingsWindow == null)
        {
            _storySettingsWindow = StorySettingsWindow.Open(this, _host, selectLevelId, AfterCommand, () => UnitsBox.SelectedItem as string ?? "");
            _storySettingsWindow.Closed += (_, _) => _storySettingsWindow = null;
        }
        PresentToolWindow(_storySettingsWindow);
    }
}

/// <summary>
/// Architecture &gt; Levels &gt; Level Settings: all storeys in one grid (elevation, floor-to-floor height),
/// with Insert Above / Below and Delete — same role as BIM "Story settings".
/// </summary>
internal static class StorySettingsWindow
{
    sealed class StoryRow
    {
        public long? Id;
        /// <summary>Level id or name used in <c>level.set</c> / <c>level.delete</c> before renames.</summary>
        public string LevelRef = "";
        public string Name = "";
        public double Elevation;
        public double Height;
        public bool MarkDeleted;
        /// <summary>The level whose elevation is typed (often 0, the datum). Every other elevation follows from the heights.</summary>
        public bool Datum;
        public bool IsNew;
    }

    public static Window Open(Window owner, IntPtr host, long? selectLevelId, Action changed, Func<string>? drawingUnit = null)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        Func<string> unit = () => drawingUnit?.Invoke() ?? "";

        var model = new List<StoryRow>();
        var implicitDrawing = false;
        var listHost = new ListView { SelectionMode = ListViewSelectionMode.Single, MinHeight = 280 };
        var insertAbove = DialogLayout.Action("Insert Above");
        var insertBelow = DialogLayout.Action("Insert Below");
        var deleteStory = DialogLayout.Action("Delete Story");
        var ok = DialogLayout.Action("OK", primary: true);
        var cancel = DialogLayout.Action("Cancel");

        bool LoadFromEngine()
        {
            model.Clear();
            implicitDrawing = false;
            if (Native.Execute(host, "level.list", "{}") != 0)
            {
                status.Text = Native.LastError(host);
                return false;
            }
            using var doc = JsonDocument.Parse(Native.ReadResult(host));
            var root = doc.RootElement;
            implicitDrawing = root.TryGetProperty("implicit", out var im) && im.ValueKind == JsonValueKind.True;
            if (!root.TryGetProperty("levels", out var levels) || levels.ValueKind != JsonValueKind.Array)
            {
                status.Text = "Could not read levels from the drawing.";
                return false;
            }
            foreach (var level in levels.EnumerateArray())
            {
                var id = level.TryGetProperty("id", out var idEl) && idEl.TryGetInt64(out var lid) ? lid : (long?)null;
                var name = level.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                var elev = level.TryGetProperty("elevation", out var e) && e.TryGetDouble(out var ev) ? ev : 0;
                var ht = level.TryGetProperty("height", out var h) && h.TryGetDouble(out var hv) ? hv : 0;
                var levelRef = implicitDrawing ? name : (id?.ToString(CultureInfo.InvariantCulture) ?? name);
                model.Add(new StoryRow
                {
                    Id = implicitDrawing ? null : id,
                    LevelRef = levelRef,
                    Name = name,
                    Elevation = elev,
                    Height = ht,
                });
            }
            return model.Count > 0;
        }

        if (!LoadFromEngine())
        {
            model.Add(new StoryRow { LevelRef = "Level 1", Name = "Level 1" });
            implicitDrawing = true;
        }

        /// <summary>
        /// Elevations are not typed except at the datum. A level's height is floor to floor (finished floor level to the finished floor
        /// level above), so each level above the datum sits at the elevation below plus the height below, and each level below the datum
        /// at the elevation above minus its own height. Levels below the datum come out negative.
        /// </summary>
        void Recalc()
        {
            var rows = model.Where(r => !r.MarkDeleted).ToList();
            if (rows.Count == 0)
            {
                return;
            }

            if (!rows.Any(r => r.Datum))
            {
                // The level nearest elevation 0 is the datum until the person picks another.
                rows.OrderBy(r => Math.Abs(r.Elevation)).First().Datum = true;
            }

            foreach (var extra in rows.Where(r => r.Datum).Skip(1))
            {
                extra.Datum = false;
            }

            var d = rows.FindIndex(r => r.Datum);
            for (var j = d + 1; j < rows.Count; j++)
            {
                rows[j].Elevation = Math.Round(rows[j - 1].Elevation + Math.Max(0, rows[j - 1].Height), 6);
            }

            for (var j = d - 1; j >= 0; j--)
            {
                rows[j].Elevation = Math.Round(rows[j + 1].Elevation - Math.Max(0, rows[j].Height), 6);
            }
        }

        void PaintList(int? selectDisplayIndex = null)
        {
            Recalc();
            listHost.Items.Clear();
            var visible = VisibleByElevation(model);
            for (var i = 0; i < visible.Count; i++)
            {
                var row = visible[i];
                // visible is top first; the code counts from the datum: L0 at the datum, L1 above it, -L1 below it.
                var datumAt = visible.FindIndex(r => r.Datum);
                var storyNo = datumAt < 0 ? visible.Count - 1 - i : datumAt - i;
                listHost.Items.Add(BuildRow(row, storyNo, () => PaintList(i), unit, () => { foreach (var r in model) { r.Datum = ReferenceEquals(r, row); } row.Elevation = 0; }));
            }
            if (selectDisplayIndex is >= 0 and var si && si < listHost.Items.Count)
            {
                listHost.SelectedIndex = si;
            }
        }

        StoryRow? SelectedModelRow()
        {
            if (listHost.SelectedIndex < 0 || listHost.SelectedItem is not Grid g || g.Tag is not StoryRow row)
            {
                return null;
            }
            return row;
        }

        insertAbove.Click += (_, _) =>
        {
            var sel = SelectedModelRow();
            var visible = VisibleByElevation(model);
            var template = sel ?? visible.FirstOrDefault() ?? model[0];
            var row = new StoryRow
            {
                IsNew = true,
                LevelRef = "new:" + Guid.NewGuid().ToString("N"),
                Name = "Level " + (visible.Count + 1),
                Elevation = template.Elevation + Math.Max(0, template.Height),
                Height = template.Height > 0 ? template.Height : 3,
            };
            var modelIndex = model.IndexOf(template);
            model.Insert(modelIndex + 1, row);
            var displayIndex = visible.IndexOf(template);
            PaintList(Math.Max(0, displayIndex));
        };

        insertBelow.Click += (_, _) =>
        {
            var sel = SelectedModelRow();
            if (sel is null)
            {
                return;
            }
            var visible = VisibleByElevation(model);
            var displayIndex = visible.IndexOf(sel);
            var row = new StoryRow
            {
                IsNew = true,
                LevelRef = "new:" + Guid.NewGuid().ToString("N"),
                Name = "Level " + (visible.Count + 1),
                Elevation = sel.Elevation - (sel.Height > 0 ? sel.Height : 3),
                Height = sel.Height > 0 ? sel.Height : 3,
            };
            if (row.Elevation < -1000)
            {
                row.Elevation = sel.Elevation - 3;
            }
            model.Insert(model.IndexOf(sel), row);
            PaintList(Math.Max(0, displayIndex + 1));
        };

        deleteStory.Click += (_, _) =>
        {
            var sel = SelectedModelRow();
            if (sel is null)
            {
                return;
            }
            if (VisibleByElevation(model).Count <= 1)
            {
                status.Text = "The last level cannot be deleted.";
                return;
            }
            sel.MarkDeleted = true;
            PaintList(0);
        };

        var header = new Grid { ColumnSpacing = 8, Margin = new Thickness(0, 0, 0, 4) };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(56) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(40) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(88) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });
        AddHeaderText(header, 0, "Datum");
        AddHeaderText(header, 1, "No.");
        AddHeaderText(header, 2, "Name");
        AddHeaderText(header, 3, "Elevation (" + (unit().Length > 0 ? unit() : "units") + ")");
        AddHeaderText(header, 4, "FFL to FFL (" + (unit().Length > 0 ? unit() : "units") + ")");

        var toolbar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.Space2 };
        toolbar.Children.Add(insertAbove);
        toolbar.Children.Add(insertBelow);
        toolbar.Children.Add(deleteStory);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin };
        root.Children.Add(DialogLayout.Heading("Levels"));
        root.Children.Add(Density.Secondary(
            "Name each level and give its floor-to-floor height (finished floor to the finished floor above). Elevations are worked out from the datum level, which you can set to 0 or a negative value; levels below it come out negative. The top level is first. Numbers are in the drawing's unit; a unit can be typed (3m, 10ft, 3000mm). The tabs call the datum level L0, the levels above it L1, L2... and the levels below it -L1, -L2...",
            TextWrapping.Wrap));
        root.Children.Add(toolbar);
        root.Children.Add(header);
        root.Children.Add(listHost);
        root.Children.Add(status);
        root.Children.Add(DialogLayout.ActionRow(ok, cancel));

        Density.Apply(root);
        var window = new Window
        {
            Title = "Levels",
            Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { root } } },
        };
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "storysettings", new WindowBounds(480, 420, 720, 900));

        PaintList();
        if (selectLevelId is not null)
        {
            var visible = VisibleByElevation(model);
            for (var i = 0; i < visible.Count; i++)
            {
                if (visible[i].Id == selectLevelId)
                {
                    listHost.SelectedIndex = i;
                    break;
                }
            }
        }

        cancel.Click += (_, _) => window.Close();
        ok.Click += (_, _) =>
        {
            if (!Commit(host, model, implicitDrawing, status))
            {
                return;
            }
            changed();
            window.Close();
        };

        return window;
    }

    static List<StoryRow> VisibleByElevation(List<StoryRow> rows) =>
        rows.Where(r => !r.MarkDeleted).OrderByDescending(r => r.Elevation).ToList();

    /// <summary>A length as shown in the grid: no trailing zeros, and never "-0".</summary>
    static string Show(double v) => (Math.Abs(v) < 1e-9 ? 0 : v).ToString("0.###", CultureInfo.InvariantCulture);

    static Grid BuildRow(StoryRow row, int storyNo, Action refresh, Func<string> unit, Action pickDatum)
    {
        var grid = new Grid { ColumnSpacing = 8, MinHeight = 36, Tag = row };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(56) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(40) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(88) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });

        var datum = new RadioButton { GroupName = "datum", IsChecked = row.Datum, MinWidth = 0, Padding = new Thickness(0), VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center };
        AutomationProperties.SetName(datum, "Datum level: " + row.Name);
        ToolTipService.SetToolTip(datum, "Datum: this level's elevation is typed (0 for the datum, or a negative or positive value). The other elevations follow from the heights.");
        datum.Checked += (_, _) =>
        {
            if (!row.Datum)
            {
                pickDatum();
                refresh();
            }
        };
        grid.Children.Add(datum);

        var no = new TextBlock
        {
            Text = storyNo == 0 ? "L0" : storyNo > 0 ? "L" + storyNo.ToString(CultureInfo.InvariantCulture) : "-L" + (-storyNo).ToString(CultureInfo.InvariantCulture),
            VerticalAlignment = VerticalAlignment.Center,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
        };
        Grid.SetColumn(no, 1);
        grid.Children.Add(no);

        var name = new TextBox { Text = row.Name, VerticalAlignment = VerticalAlignment.Center, PlaceholderText = "Storey name" };
        name.LostFocus += (_, _) => row.Name = name.Text.Trim();
        Grid.SetColumn(name, 2);
        grid.Children.Add(name);

        // Heights and elevations are typed in the drawing's unit; a unit can be typed too (3m, 10ft, 3000mm).
        TextBox LengthBox(double value, Action<double> set, bool allowNegative)
        {
            var box = new TextBox { Text = Show(value), VerticalAlignment = VerticalAlignment.Center };
            ToolTipService.SetToolTip(box, "In " + (unit().Length > 0 ? unit() : "drawing units") + ". A unit can be typed: 3m, 10ft, 3000mm.");
            void Commit()
            {
                if (LengthText.TryParse(box.Text, unit(), out var v) && (allowNegative || v >= 0))
                {
                    set(v);
                    box.Text = Show(v);
                    refresh();
                }
                else
                {
                    box.Text = Show(value);
                }
            }

            box.LostFocus += (_, _) => Commit();
            box.KeyDown += (_, args) =>
            {
                if (args.Key == Windows.System.VirtualKey.Enter)
                {
                    args.Handled = true;
                    Commit();
                }
            };
            return box;
        }

        FrameworkElement elev;
        if (row.Datum)
        {
            // Negative is allowed here: a datum below the ground, or levels hung below it.
            elev = LengthBox(row.Elevation, v => row.Elevation = v, true);
        }
        else
        {
            elev = new TextBlock { Text = Show(row.Elevation), VerticalAlignment = VerticalAlignment.Center, Opacity = 0.85 };
            ToolTipService.SetToolTip(elev, "Worked out from the heights, counting from the datum level.");
        }

        Grid.SetColumn(elev, 3);
        grid.Children.Add(elev);

        var ht = LengthBox(row.Height, v => row.Height = Math.Max(0, v), false);
        Grid.SetColumn(ht, 4);
        grid.Children.Add(ht);

        return grid;
    }

    static void AddHeaderText(Grid grid, int column, string text)
    {
        var tb = Density.Secondary(text, TextWrapping.NoWrap);
        tb.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(tb, column);
        grid.Children.Add(tb);
    }

    static bool Commit(IntPtr host, List<StoryRow> rows, bool implicitDrawing, TextBlock status)
    {
        var visible = rows.Where(r => !r.MarkDeleted).ToList();
        if (visible.Count == 0)
        {
            status.Text = "Keep at least one level.";
            return false;
        }

        if (implicitDrawing && visible.Count == 1 && !visible[0].IsNew)
        {
            status.Text = "This drawing has one implicit level. Use Insert Above or Below to add a storey, then OK.";
            return false;
        }

        foreach (var row in rows.Where(r => r.MarkDeleted && !r.IsNew))
        {
            var body = JsonSerializer.Serialize(new Dictionary<string, object> { ["level"] = row.LevelRef, ["force"] = true });
            if (Native.Execute(host, "level.delete", body) != 0)
            {
                status.Text = Native.LastError(host);
                return false;
            }
        }

        foreach (var row in visible.Where(r => r.IsNew))
        {
            var payload = new Dictionary<string, object>
            {
                ["name"] = row.Name,
                ["elevation"] = row.Elevation,
                ["height"] = row.Height,
            };
            if (implicitDrawing && visible.Count(r => !r.IsNew) == 0)
            {
                payload["first"] = row.Name;
            }
            if (Native.Execute(host, "level.new", JsonSerializer.Serialize(payload)) != 0)
            {
                status.Text = Native.LastError(host);
                return false;
            }
            row.IsNew = false;
            implicitDrawing = false;
        }

        foreach (var row in visible.Where(r => !r.IsNew))
        {
            var payload = new Dictionary<string, object>
            {
                ["level"] = row.LevelRef,
                ["name"] = row.Name,
                ["elevation"] = row.Elevation,
                ["height"] = row.Height,
            };
            if (Native.Execute(host, "level.set", JsonSerializer.Serialize(payload)) != 0)
            {
                status.Text = Native.LastError(host);
                return false;
            }
            row.LevelRef = row.Name;
        }

        return true;
    }
}

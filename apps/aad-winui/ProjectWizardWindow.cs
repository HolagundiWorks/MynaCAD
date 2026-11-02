using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

public sealed partial class MainWindow
{
    private Window? _projectWizardWindow;

    private void OpenProjectWizard()
    {
        if (_host == IntPtr.Zero)
        {
            return;
        }
        if (_projectWizardWindow == null)
        {
            _projectWizardWindow = ProjectWizardWindow.Open(this, _host, AfterCommand);
            _projectWizardWindow.Closed += (_, _) => _projectWizardWindow = null;
        }
        PresentToolWindow(_projectWizardWindow);
    }
}

/// <summary>
/// File &gt; Project Wizard: the project's details, the team, the building (floors and their
/// heights), and the standards (layer sets, discipline tabs, symbols), then one call to the engine's
/// <c>project.wizard</c>. The window collects answers; the engine applies them as one undo step.
/// </summary>
internal static class ProjectWizardWindow
{
    private static readonly (string Key, string Label)[] ProjectFields =
    [
        ("title", "Project name"),
        ("owner", "Client"),
        ("location", "Location"),
        ("address", "Site address"),
        ("pid", "Property ID"),
        ("siteArea", "Site area (m2)"),
        ("plotUse", "Plot use"),
        ("authority", "Approving authority"),
        ("date", "Date"),
        ("summary", "Summary"),
    ];

    private static readonly string[] Units = ["mm", "cm", "m", "ft", "in"];

    private static readonly string[] Tabs = ["Working drawing", "Electrical", "Plumbing", "Interior", "False ceiling"];

    private static readonly string[] Ordinals = ["Ground", "First", "Second", "Third", "Fourth", "Fifth", "Sixth", "Seventh", "Eighth", "Ninth", "Tenth"];

    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Project wizard status");

        // ---- page 1: the project
        var projectBoxes = new Dictionary<string, TextBox>();
        var page1 = new StackPanel { Spacing = Density.SpaceSm };
        page1.Children.Add(DialogLayout.Heading("Project"));
        foreach (var (key, label) in ProjectFields)
        {
            var box = Density.DialogField(key == "date" ? DateTime.Today.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) : "", label);
            projectBoxes[key] = box;
            page1.Children.Add(DialogLayout.Labeled(label, box));
        }

        // ---- page 2: the team and the company
        var architect = Density.DialogField("", "Architect");
        var engineer = Density.DialogField("", "Engineer");
        var company = Density.Secondary("No company details read yet.", TextWrapping.Wrap);
        var suiteHint = Density.Secondary(
            "Company identity (name, address, contacts, registration, architect) is owned by Studio. When this drawing is linked, those fields are refresh-only — change them in Studio, then Read company file.",
            TextWrapping.Wrap);
        var readCompany = Density.DialogAction("Read company file");
        var page2 = new StackPanel { Spacing = Density.SpaceSm };
        page2.Children.Add(DialogLayout.Heading("Team and company"));
        page2.Children.Add(DialogLayout.Labeled("Architect", architect));
        page2.Children.Add(DialogLayout.Labeled("Engineer", engineer));
        page2.Children.Add(suiteHint);
        page2.Children.Add(readCompany);
        page2.Children.Add(company);

        var lockedDetails = new HashSet<string>(StringComparer.Ordinal);
        void ApplySuiteLocks(JsonElement root)
        {
            lockedDetails.Clear();
            if (root.TryGetProperty("suite", out var suite) && suite.ValueKind == JsonValueKind.Object
                && suite.TryGetProperty("locked_details", out var locked) && locked.ValueKind == JsonValueKind.Array)
            {
                foreach (var item in locked.EnumerateArray())
                {
                    if (item.ValueKind == JsonValueKind.String)
                    {
                        var name = item.GetString();
                        if (!string.IsNullOrEmpty(name))
                        {
                            lockedDetails.Add(name);
                        }
                    }
                }
            }
            foreach (var (key, box) in projectBoxes)
            {
                box.IsReadOnly = lockedDetails.Contains(key);
            }
            architect.IsReadOnly = lockedDetails.Contains("architect");
            var refreshOnly = root.TryGetProperty("suite", out var suite2)
                && suite2.ValueKind == JsonValueKind.Object
                && suite2.TryGetProperty("refresh_only", out var ro)
                && ro.ValueKind == JsonValueKind.True;
            suiteHint.Text = refreshOnly
                ? "This drawing holds company details from Studio. Locked fields above are refresh-only — edit them in Studio, then Read company file. Engineer and other local fields stay editable."
                : "Company identity (name, address, contacts, registration, architect) is owned by Studio. When this drawing is linked, those fields are refresh-only — change them in Studio, then Read company file.";
            ToolTipService.SetToolTip(readCompany, refreshOnly
                ? "Refresh company and linked project details from Studio (company file)"
                : "Read company details from O-Studio's company file (needs OSTUDIO_COMPANY and a ticket)");
        }

        // ---- page 3: the building
        var units = new ComboBox { MinHeight = Density.ControlHeight };
        foreach (var u in Units)
        {
            units.Items.Add(u);
        }
        units.SelectedIndex = 0;
        Density.Name(units, "Drawing unit");
        var floorsAbove = Density.DialogField("2", "Floors above the ground floor");
        var basements = Density.DialogField("0", "Basements");
        var terrace = new CheckBox { Content = "Terrace", IsChecked = true, MinHeight = Density.ControlHeight };
        var storey = Density.DialogField("3.0", "Floor to floor height (m)");
        var slab = Density.DialogField("150", "Slab (mm)");
        var beam = Density.DialogField("450", "Beam depth (mm)");
        const string AnyUnit = "A bare number is in the unit shown. Any unit can be typed: 3m, 10ft, 3000mm, 9'6\"";
        foreach (var lengthBox in new[] { storey, slab, beam })
        {
            ToolTipService.SetToolTip(lengthBox, AnyUnit);
        }
        var oneLevelPerFloor = new CheckBox { Content = "One level (tab) per floor", IsChecked = true, MinHeight = Density.ControlHeight };
        var floorRows = new StackPanel { Spacing = Density.SpaceXs };
        var floorBoxes = new List<(TextBox Name, TextBox Height)>();
        var page3 = new StackPanel { Spacing = Density.SpaceSm };
        page3.Children.Add(DialogLayout.Heading("Building"));
        page3.Children.Add(DialogLayout.Labeled("Drawing unit", units));
        page3.Children.Add(DialogLayout.Labeled("Floors above the ground floor", floorsAbove));
        page3.Children.Add(DialogLayout.Labeled("Basements", basements));
        page3.Children.Add(terrace);
        page3.Children.Add(DialogLayout.Labeled("Default floor to floor height (m)", storey));
        page3.Children.Add(DialogLayout.Labeled("Default slab thickness (mm)", slab));
        page3.Children.Add(DialogLayout.Labeled("Default beam depth including slab (mm)", beam));
        page3.Children.Add(oneLevelPerFloor);
        page3.Children.Add(Density.Secondary("Floors (edit a name or a height, in metres unless you type a unit such as 10ft; changing the counts above rebuilds this list):", TextWrapping.Wrap));
        page3.Children.Add(floorRows);

        static bool Number(string text, out double value) =>
            double.TryParse((text ?? "").Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out value) && double.IsFinite(value);

        void RebuildFloors()
        {
            floorRows.Children.Clear();
            floorBoxes.Clear();
            var above = Number(floorsAbove.Text, out var a) && a >= 0 && a <= 59 ? (int)a : 0;
            var below = Number(basements.Text, out var b) && b >= 0 && b <= 10 ? (int)b : 0;
            var height = LengthText.TryParse(storey.Text, "m", out var h) && h >= 2 && h <= 30 ? h : 3.0;
            var names = new List<string>();
            for (var i = below; i >= 1; i--)
            {
                names.Add("Basement " + i);
            }
            for (var i = 0; i <= above; i++)
            {
                names.Add(i < Ordinals.Length ? Ordinals[i] + " Floor" : "Floor " + i);
            }
            if (terrace.IsChecked == true)
            {
                names.Add("Terrace");
            }
            foreach (var name in names)
            {
                var n = Density.DialogField(name, "Floor name");
                var hh = Density.DialogField(height.ToString("0.###", CultureInfo.InvariantCulture), "Height of " + name);
                hh.HorizontalAlignment = HorizontalAlignment.Stretch;
                floorBoxes.Add((n, hh));
                var row = new Grid { ColumnSpacing = Density.SpaceSm };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(3, GridUnitType.Star) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                Grid.SetColumn(hh, 1);
                row.Children.Add(n);
                row.Children.Add(hh);
                floorRows.Children.Add(row);
            }
        }
        foreach (var box in new[] { floorsAbove, basements, storey })
        {
            box.LostFocus += (_, _) => RebuildFloors();
        }
        terrace.Click += (_, _) => RebuildFloors();
        RebuildFloors();

        // ---- page 4: the standards
        var layerStandard = new CheckBox { Content = "Architectural layer set (walls, doors, windows, furniture, services)", IsChecked = true, MinHeight = Density.ControlHeight };
        var layerPermit = new CheckBox { Content = "Building permit layers (site, setbacks, areas)", IsChecked = false, MinHeight = Density.ControlHeight };
        var layerPresentation = new CheckBox { Content = "Presentation layers (tone, clouds)", IsChecked = false, MinHeight = Density.ControlHeight };
        var tabBoxes = Tabs.Select(t => new CheckBox { Content = t, Tag = t, IsChecked = true, MinHeight = Density.ControlHeight }).ToList();
        var symbols = new CheckBox { Content = "Standard symbol blocks (switches, lights, fans, sanitary fixtures, furniture, ceiling)", IsChecked = true, MinHeight = Density.ControlHeight };
        var page4 = new StackPanel { Spacing = Density.SpaceSm };
        page4.Children.Add(DialogLayout.Heading("Standards"));
        page4.Children.Add(Density.Secondary("Layer sets", TextWrapping.Wrap));
        page4.Children.Add(layerStandard);
        page4.Children.Add(layerPermit);
        page4.Children.Add(layerPresentation);
        page4.Children.Add(Density.Secondary("Drawing set tabs (each is a discipline's annotation over the floors, with its own layers and blocks)", TextWrapping.Wrap));
        foreach (var box in tabBoxes)
        {
            page4.Children.Add(box);
        }
        page4.Children.Add(symbols);
        page4.Children.Add(Density.Secondary("The layer names follow the discipline-letter and element-code style of ISO 13567; they are this program's table, not a certified copy of the standard.", TextWrapping.Wrap));

        // ---- page 5: the review
        var review = new TextBlock { TextWrapping = TextWrapping.Wrap, FontSize = Density.FontBody, IsTextSelectionEnabled = true };
        var page5 = new StackPanel { Spacing = Density.SpaceSm };
        page5.Children.Add(DialogLayout.Heading("Review"));
        page5.Children.Add(review);

        // What the drawing holds already: the form opens filled in, and a drawing that has floors or
        // tabs is edited (details, structure and extra standards) instead of set up again.
        var edit = false;
        var existingFloors = new List<string>();
        var existingTabs = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (Native.Execute(host, "level.list", "{}") == 0)
        {
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                var implicitLevel = doc.RootElement.TryGetProperty("implicit", out var im) && im.ValueKind == JsonValueKind.True;
                if (!implicitLevel && doc.RootElement.TryGetProperty("levels", out var levels) && levels.ValueKind == JsonValueKind.Array)
                {
                    foreach (var level in levels.EnumerateArray())
                    {
                        existingFloors.Add(level.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "");
                    }
                }
            }
            catch (JsonException)
            {
            }
        }
        if (Native.Execute(host, "sheetset.list", "{}") == 0)
        {
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (doc.RootElement.TryGetProperty("sheetSets", out var sets) && sets.ValueKind == JsonValueKind.Array)
                {
                    foreach (var set in sets.EnumerateArray())
                    {
                        existingTabs.Add(set.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "");
                    }
                }
            }
            catch (JsonException)
            {
            }
        }
        edit = existingFloors.Count > 0 || existingTabs.Count > 0;
        if (edit)
        {
            // The default of today is for a new project; an edit shows only what the project holds.
            projectBoxes["date"].Text = "";
        }
        if (Native.Execute(host, "project", "{}") == 0)
        {
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (doc.RootElement.TryGetProperty("details", out var held) && held.ValueKind == JsonValueKind.Object)
                {
                    foreach (var prop in held.EnumerateObject())
                    {
                        var text = prop.Value.ValueKind == JsonValueKind.String ? prop.Value.GetString() ?? "" : "";
                        if (projectBoxes.TryGetValue(prop.Name, out var box))
                        {
                            box.Text = text;
                        }
                        else if (prop.Name == "architect")
                        {
                            architect.Text = text;
                        }
                        else if (prop.Name == "engineer")
                        {
                            engineer.Text = text;
                        }
                    }
                }
                if (doc.RootElement.TryGetProperty("structure", out var structure))
                {
                    if (structure.TryGetProperty("slabMm", out var sv) && sv.TryGetDouble(out var slabValue))
                    {
                        slab.Text = slabValue.ToString("0.###", CultureInfo.InvariantCulture);
                    }
                    if (structure.TryGetProperty("beamMm", out var bv) && bv.TryGetDouble(out var beamValue))
                    {
                        beam.Text = beamValue.ToString("0.###", CultureInfo.InvariantCulture);
                    }
                }
                if (doc.RootElement.TryGetProperty("company", out var held2) && held2.ValueKind == JsonValueKind.Object)
                {
                    var lines = held2.EnumerateObject()
                        .Where(x => x.Value.ValueKind == JsonValueKind.String && (x.Value.GetString() ?? "").Length > 0 && x.Name != "project_ref")
                        .Select(x => x.Name.Replace('_', ' ') + ": " + x.Value.GetString());
                    var joined = string.Join("\n", lines);
                    if (joined.Length > 0)
                    {
                        company.Text = joined;
                    }
                }
                ApplySuiteLocks(doc.RootElement);
            }
            catch (JsonException)
            {
            }
        }
        if (edit)
        {
            // Floors, levels and the unit were chosen when the project was created: they are changed
            // on the level tabs (right-click) and with Drawing Units, not here.
            for (var i = 1; i < page3.Children.Count; i++)
            {
                page3.Children[i].Visibility = Visibility.Collapsed;
            }
            page3.Children.Add(Density.Secondary(
                "Floors in this drawing: " + string.Join(", ", existingFloors) + ". Change a floor's name, elevation or height by right-clicking its tab at the bottom of the drawing.",
                TextWrapping.Wrap));
            foreach (var box in tabBoxes)
            {
                if (existingTabs.Contains((string)box.Tag))
                {
                    box.IsChecked = true;
                    box.IsEnabled = false;
                    box.Content = (string)box.Tag + " (already in this drawing)";
                }
                else
                {
                    box.IsChecked = false;
                }
            }
        }

        var pages = new FrameworkElement[] { page1, page2, page3, page4, page5 };
        var current = 0;
        var back = Density.DialogAction("Back");
        var next = Density.DialogAction("Next", primary: true);
        var create = Density.DialogAction(edit ? "Apply changes" : "Create project", primary: true);
        var cancel = Density.DialogAction("Cancel");

        Dictionary<string, object> Collect()
        {
            var details = new Dictionary<string, object>();
            foreach (var (key, box) in projectBoxes)
            {
                var text = box.Text.Trim();
                // Editing sends empty fields too, so a detail can be cleared.
                if (text.Length > 0 || edit)
                {
                    details[key] = text;
                }
            }
            if (architect.Text.Trim().Length > 0 || edit)
            {
                details["architect"] = architect.Text.Trim();
            }
            if (engineer.Text.Trim().Length > 0 || edit)
            {
                details["engineer"] = engineer.Text.Trim();
            }
            var floors = new List<object>();
            foreach (var (n, h) in floorBoxes)
            {
                floors.Add(new Dictionary<string, object> { ["name"] = n.Text.Trim(), ["height"] = LengthText.TryParse(h.Text, "m", out var v) ? v : 3.0 });
            }
            var layers = new List<string>();
            if (layerStandard.IsChecked == true)
            {
                layers.Add("standard");
            }
            if (layerPermit.IsChecked == true)
            {
                layers.Add("permit");
            }
            if (layerPresentation.IsChecked == true)
            {
                layers.Add("presentation");
            }
            var body = new Dictionary<string, object>
            {
                ["details"] = details,
                ["units"] = Units[Math.Max(0, units.SelectedIndex)],
                ["floors"] = floors,
                ["levels"] = oneLevelPerFloor.IsChecked == true,
                ["layers"] = layers,
                ["sheetSets"] = tabBoxes.Where(b => b.IsChecked == true && b.IsEnabled).Select(b => (string)b.Tag).ToList(),
                ["symbols"] = symbols.IsChecked == true,
            };
            if (edit)
            {
                body.Remove("units");
                body.Remove("floors");
                body.Remove("levels");
            }
            if (LengthText.TryParse(slab.Text, "mm", out var sm))
            {
                body["slabMm"] = sm;
            }
            if (LengthText.TryParse(beam.Text, "mm", out var bm))
            {
                body["beamMm"] = bm;
            }
            return body;
        }

        void Show(int page)
        {
            current = Math.Clamp(page, 0, pages.Length - 1);
            for (var i = 0; i < pages.Length; i++)
            {
                pages[i].Visibility = i == current ? Visibility.Visible : Visibility.Collapsed;
            }
            if (current == pages.Length - 1)
            {
                var body = Collect();
                var names = floorBoxes.Select(f => f.Name.Text.Trim()).ToList();
                var tabs = (List<string>)body["sheetSets"];
                var details = (Dictionary<string, object>)body["details"];
                review.Text =
                    (edit ? "Editing the project in this drawing.\n" : "")
                    + "Project: " + (details.TryGetValue("title", out var t) ? t : "(no name)") + "\n"
                    + "Client: " + (details.TryGetValue("owner", out var o) ? o : "-") + "\n"
                    + (edit
                        ? "Floors: " + string.Join(", ", existingFloors) + " (unchanged)\n"
                        : "Unit: " + Units[Math.Max(0, units.SelectedIndex)] + "\n"
                            + "Floors (" + names.Count + "): " + string.Join(", ", names) + "\n"
                            + "One tab per floor: " + (oneLevelPerFloor.IsChecked == true ? "yes" : "no") + "\n")
                    + "Layer sets: " + (((List<string>)body["layers"]).Count == 0 ? "none" : string.Join(", ", (List<string>)body["layers"])) + "\n"
                    + (edit ? "Tabs to add: " : "Drawing set tabs: ") + (tabs.Count == 0 ? "none" : string.Join(", ", tabs)) + "\n"
                    + "Symbol blocks: " + ((bool)body["symbols"] ? "yes" : "no");
            }
            back.IsEnabled = current > 0;
            next.Visibility = current < pages.Length - 1 ? Visibility.Visible : Visibility.Collapsed;
            create.Visibility = current == pages.Length - 1 ? Visibility.Visible : Visibility.Collapsed;
            status.Text = "";
        }

        readCompany.Click += (_, _) =>
        {
            if (Native.Execute(host, "companyfile", "{\"refresh\":true}") != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }
            if (Native.Execute(host, "project", "{}") != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (doc.RootElement.TryGetProperty("details", out var d) && d.ValueKind == JsonValueKind.Object)
                {
                    foreach (var prop in d.EnumerateObject())
                    {
                        var text = prop.Value.ValueKind == JsonValueKind.String ? prop.Value.GetString() ?? "" : "";
                        if (text.Length == 0)
                        {
                            continue;
                        }
                        if (projectBoxes.TryGetValue(prop.Name, out var box))
                        {
                            box.Text = text;
                        }
                        else if (prop.Name == "architect")
                        {
                            architect.Text = text;
                        }
                        else if (prop.Name == "engineer")
                        {
                            engineer.Text = text;
                        }
                    }
                }
                var lines = new List<string>();
                if (doc.RootElement.TryGetProperty("company", out var c) && c.ValueKind == JsonValueKind.Object)
                {
                    foreach (var prop in c.EnumerateObject())
                    {
                        if (prop.Value.ValueKind == JsonValueKind.String && (prop.Value.GetString() ?? "").Length > 0 && prop.Name != "project_ref")
                        {
                            lines.Add(prop.Name.Replace('_', ' ') + ": " + prop.Value.GetString());
                        }
                    }
                }
                company.Text = lines.Count == 0 ? "The company file gave no company details." : string.Join("\n", lines);
                ApplySuiteLocks(doc.RootElement);
            }
            catch (JsonException)
            {
                status.Text = "The company file's answer could not be read.";
            }
        };

        back.Click += (_, _) => Show(current - 1);
        next.Click += (_, _) => Show(current + 1);

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin };
        foreach (var page in pages)
        {
            root.Children.Add(page);
        }
        root.Children.Add(status);
        root.Children.Add(DialogLayout.ActionRow(back, next, create, cancel));

        Density.Apply(root);
        var window = new Window
        {
            Title = edit ? "Edit Project" : "New Project Wizard",
            Content = new ScrollViewer { Content = new Grid { RequestedTheme = theme, Children = { root } } },
        };
        StandardWindow.Present(window, owner, WindowTemplate.PropertyDialog, "projectwizard", new WindowBounds(460, 420, 640, 860));
        create.Click += (_, _) =>
        {
            var json = JsonSerializer.Serialize(Collect());
            if (Native.Execute(host, "project.wizard", json) != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }
            changed();
            window.Close();
        };
        cancel.Click += (_, _) => window.Close();
        Show(0);
        return window;
    }
}

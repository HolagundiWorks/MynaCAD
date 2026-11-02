using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace Aad.WinUi;

/// <summary>Insert &gt; Smart Block — pick a smart block, fill in its parameters, then pick the
/// insertion point on the drawing (<c>smartblock.list</c> and the <c>smartblock.insert</c> form).
/// Parameters of a placed smart block are edited in the Properties panel or by its grips.</summary>
internal static class SmartBlockWindow
{
    private static readonly string[] Disciplines = { "All", "rcc", "structural", "plumbing", "hvac", "electrical", "interior" };

    public static Window Open(Window owner, IntPtr host, Action changed)
    {
        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        var status = Density.Secondary("", TextWrapping.Wrap);
        AutomationProperties.SetLiveSetting(status, AutomationLiveSetting.Polite);
        Density.Name(status, "Smart block status");

        var discipline = Density.DialogCombo("Discipline");
        foreach (var name in Disciplines)
        {
            discipline.Items.Add(new ComboBoxItem { Content = Title(name), Tag = name });
        }
        discipline.SelectedIndex = 0;
        var blocks = new ListView { MinHeight = 160, MaxHeight = 260, MinWidth = 360, SelectionMode = ListViewSelectionMode.Single };
        Density.Name(blocks, "Smart blocks");
        var description = Density.Secondary("", TextWrapping.Wrap);
        var fields = new StackPanel { Spacing = Density.SpaceSm };
        var rotation = Density.DialogField("0", "Rotation (degrees)");
        var insert = DialogLayout.Action("Insert", primary: true);
        var defaults = DialogLayout.Action("Defaults");
        insert.IsEnabled = false;
        defaults.IsEnabled = false;

        var root = new StackPanel { Spacing = Density.SpaceMd, Margin = Density.DialogMargin, MaxWidth = 480 };
        root.Children.Add(DialogLayout.Heading("Smart Blocks"));
        root.Children.Add(Density.FieldRow("Discipline", discipline));
        root.Children.Add(blocks);
        root.Children.Add(description);
        root.Children.Add(Density.SectionHeading("Parameters"));
        root.Children.Add(fields);
        root.Children.Add(Density.FieldRow("Rotation (degrees)", rotation));
        root.Children.Add(Density.Secondary("The insertion point is picked on the drawing after Insert. Edit a placed smart block in Properties or drag its grips.", TextWrapping.Wrap));
        root.Children.Add(DialogLayout.ActionRow(insert, defaults));
        root.Children.Add(status);

        var all = new List<Spec>();
        var inputs = new List<(Param Param, FrameworkElement Input)>();

        void Load()
        {
            all.Clear();
            if (Native.Execute(host, "smartblock.list", "{}") != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }
            try
            {
                using var doc = JsonDocument.Parse(Native.ReadResult(host));
                if (doc.RootElement.TryGetProperty("blocks", out var list) && list.ValueKind == JsonValueKind.Array)
                {
                    foreach (var block in list.EnumerateArray().Take(500))
                    {
                        if (Spec.Read(block) is Spec spec)
                        {
                            all.Add(spec);
                        }
                    }
                }
            }
            catch (JsonException ex)
            {
                status.Text = ex.Message;
            }
            Filter();
        }

        void Filter()
        {
            var want = discipline.SelectedItem is ComboBoxItem { Tag: string tag } ? tag : "All";
            blocks.Items.Clear();
            foreach (var spec in all)
            {
                if (want == "All" || string.Equals(spec.Discipline, want, StringComparison.OrdinalIgnoreCase))
                {
                    blocks.Items.Add(spec);
                }
            }
            if (blocks.Items.Count > 0)
            {
                blocks.SelectedIndex = 0;
            }
            else
            {
                Show(null);
            }
        }

        void Show(Spec? spec)
        {
            fields.Children.Clear();
            inputs.Clear();
            insert.IsEnabled = spec != null;
            defaults.IsEnabled = spec != null;
            description.Text = spec?.Description ?? "";
            if (spec == null)
            {
                return;
            }
            foreach (var param in spec.Params)
            {
                FrameworkElement input;
                if (param.Choices.Count > 0)
                {
                    var combo = Density.DialogCombo(param.Label);
                    foreach (var (label, value) in param.Choices)
                    {
                        combo.Items.Add(new ComboBoxItem { Content = label, Tag = label });
                        if (value == param.Default)
                        {
                            combo.SelectedIndex = combo.Items.Count - 1;
                        }
                    }
                    input = combo;
                }
                else
                {
                    input = Density.DialogField(Number(param.Default), param.Label);
                    ToolTipService.SetToolTip(input, param.Range() + (LengthText.MetresPer(param.Unit) != null ? ". Any unit can be typed: 12in, 0.3m." : ""));
                }
                inputs.Add((param, input));
                fields.Children.Add(Density.FieldRow(param.Caption(), input));
            }
            Density.Apply(fields);
        }

        string? ParamsText()
        {
            var parts = new List<string>();
            foreach (var (param, input) in inputs)
            {
                switch (input)
                {
                    case ComboBox { SelectedItem: ComboBoxItem { Tag: string label } }:
                        parts.Add(param.Name + "=" + label);
                        break;
                    case TextBox box:
                        var text = box.Text.Trim();
                        if (text.Length == 0)
                        {
                            break;
                        }
                        // A length may carry any unit (12in, 0.3m); it is converted into the parameter's own unit.
                        if (!LengthText.TryParse(text, param.Unit, out var number))
                        {
                            status.Text = param.Label + ": enter a number (" + param.Range() + ").";
                            return null;
                        }
                        parts.Add(param.Name + "=" + Number(number));
                        break;
                }
            }
            return string.Join(", ", parts);
        }

        discipline.SelectionChanged += (_, _) => Filter();
        blocks.SelectionChanged += (_, _) => Show(blocks.SelectedItem as Spec);
        defaults.Click += (_, _) => Show(blocks.SelectedItem as Spec);
        insert.Click += (_, _) =>
        {
            if (blocks.SelectedItem is not Spec spec || ParamsText() is not string text)
            {
                return;
            }
            var turn = rotation.Text.Trim();
            if (turn.Length > 0 && !double.TryParse(turn, NumberStyles.Float, CultureInfo.InvariantCulture, out _))
            {
                status.Text = "Rotation: enter a number of degrees.";
                return;
            }
            var preset = JsonSerializer.Serialize(new Dictionary<string, string>
            {
                ["id"] = spec.Id,
                ["params"] = text,
                ["rotation"] = turn.Length == 0 ? "0" : turn,
            });
            if (Native.StartForm(host, "smartblock.insert", preset) != 0)
            {
                status.Text = Native.LastError(host);
                return;
            }
            status.Text = "Pick the insertion point of " + spec.Title + " on the drawing.";
            changed();
        };

        var window = new Window { Title = "Smart Blocks" };
        window.Content = new ScrollViewer { Content = root };
        root.RequestedTheme = theme;
        Density.Apply(root);
        ShellIcon.Apply(window, theme);
        ShellFit.UserResizable(window, minWidthDip: 380, minHeightDip: 480, maxWidthDip: 560, maxHeightDip: 860, persistKey: "smartblocks");
        Load();
        window.Activate();
        Native.ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(window), 8);
        return window;
    }

    private static string Title(string discipline) => discipline switch
    {
        "rcc" => "RCC",
        "hvac" => "HVAC",
        "All" => "All disciplines",
        _ => CultureInfo.InvariantCulture.TextInfo.ToTitleCase(discipline),
    };

    private static string Number(double value) => value.ToString("0.###", CultureInfo.InvariantCulture);

    private sealed class Param
    {
        public string Name { get; init; } = "";
        public string Label { get; init; } = "";
        public string Unit { get; init; } = "";
        public double Default { get; init; }
        public double? Min { get; init; }
        public double? Max { get; init; }
        public bool Integer { get; init; }
        public List<(string Label, double Value)> Choices { get; } = new();

        public string Caption() => Unit.Length == 0 ? Label : Label + " (" + Unit + ")";

        public string Range()
        {
            var kind = Integer ? "whole number" : "number";
            return (Min, Max) switch
            {
                (double lo, double hi) => kind + " from " + Number(lo) + " to " + Number(hi),
                (double lo, null) => kind + ", at least " + Number(lo),
                (null, double hi) => kind + ", at most " + Number(hi),
                _ => kind,
            };
        }
    }

    private sealed class Spec
    {
        public string Id { get; init; } = "";
        public string Title { get; init; } = "";
        public string Discipline { get; init; } = "";
        public string Description { get; init; } = "";
        public List<Param> Params { get; } = new();

        public override string ToString() => Title;

        public static Spec? Read(JsonElement block)
        {
            var id = Str(block, "id");
            if (id.Length == 0)
            {
                return null;
            }
            var spec = new Spec { Id = id, Title = Str(block, "title"), Discipline = Str(block, "discipline"), Description = Str(block, "description") };
            if (block.TryGetProperty("params", out var list) && list.ValueKind == JsonValueKind.Array)
            {
                foreach (var p in list.EnumerateArray().Take(48))
                {
                    var param = new Param
                    {
                        Name = Str(p, "name"),
                        Label = Str(p, "label"),
                        Unit = Str(p, "unit"),
                        Default = Num(p, "default") ?? 0,
                        Min = Num(p, "min"),
                        Max = Num(p, "max"),
                        Integer = p.TryGetProperty("integer", out var whole) && whole.ValueKind == JsonValueKind.True,
                    };
                    if (p.TryGetProperty("choices", out var choices) && choices.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var c in choices.EnumerateArray().Take(64))
                        {
                            if (Num(c, "value") is double value)
                            {
                                param.Choices.Add((Str(c, "label"), value));
                            }
                        }
                    }
                    if (param.Name.Length > 0)
                    {
                        spec.Params.Add(param);
                    }
                }
            }
            return spec;
        }

        private static string Str(JsonElement e, string key) =>
            e.TryGetProperty(key, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";

        private static double? Num(JsonElement e, string key) =>
            e.TryGetProperty(key, out var v) && v.ValueKind == JsonValueKind.Number && v.TryGetDouble(out var d) ? d : null;
    }
}

using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Aad.WinUi;

/// <summary>
/// The take-off phase pickers: the estimation phase and the running-bill phase that dimensions drawn or tagged next
/// belong to. The lists live in the drawing (<c>takeoff.phases</c>); Falcon shows the choice as table columns.
/// </summary>
public sealed partial class MainWindow
{
    private const string NoPhase = "";

    private ComboBox? _estimationPhaseBox;
    private ComboBox? _billingPhaseBox;
    private string _phaseKey = "";

    /// <summary>Two combo boxes at the end of the take-off bar.</summary>
    private void AddPhasePickers()
    {
        _estimationPhaseBox = MakePhaseBox("Estimation phase", "estimation");
        _billingPhaseBox = MakePhaseBox("Running bill phase", "billing");
        TakeoffBar.Children.Add(MakeToolDivider(horizontalBar: true));
        TakeoffBar.Children.Add(_estimationPhaseBox);
        TakeoffBar.Children.Add(_billingPhaseBox);
        _phaseKey = "";
    }

    private ComboBox MakePhaseBox(string label, string which)
    {
        var box = new ComboBox
        {
            MinWidth = Density.FieldWidth,
            MinHeight = Density.ControlHeight,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(Density.SpaceSm, 0, 0, 0),
            PlaceholderText = label,
        };
        AutomationProperties.SetName(box, label);
        ToolTipService.SetToolTip(box, label + ": dimensions you draw or tag next belong to it");
        box.SelectionChanged += (_, _) =>
        {
            if (_filling || _host == IntPtr.Zero || box.SelectedItem is not ComboBoxItem { Tag: string name })
            {
                return;
            }
            var current = new Dictionary<string, object?> { [which] = name.Length == 0 ? null : name };
            var json = JsonSerializer.Serialize(new Dictionary<string, object> { ["current"] = current });
            if (Native.Execute(_host, "takeoff.phases", json) != 0)
            {
                History.Text = "The take-off phase was not changed.";
            }
        };
        return box;
    }

    /// <summary>
    /// A text-only tool for the take-off bar: the label on a flat button, no icon. Falcon's take-off bar is drawn the
    /// same way (flat text buttons in groups with dividers), so the two read alike.
    /// </summary>
    private static Button MakeTextTool(string label)
    {
        var button = new Button
        {
            Content = new TextBlock { Text = label, FontSize = Density.FontBody, TextTrimming = TextTrimming.CharacterEllipsis },
            MinHeight = Density.ControlHeight,
            Padding = new Thickness(Density.SpaceLg, 0, Density.SpaceLg, 0),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            CornerRadius = new CornerRadius(Density.CornerControl),
            VerticalAlignment = VerticalAlignment.Center,
        };
        if (Application.Current.Resources.TryGetValue("CompactButtonStyle", out var style) && style is Style s)
        {
            button.Style = s;
        }
        AutomationProperties.SetName(button, label);
        ToolTipService.SetToolTip(button, label);
        return button;
    }

    /// <summary>Fill the pickers from the panel snapshot when the lists or the choice changed.</summary>
    private void FillTakeoffPhases(JsonElement root)
    {
        if (_estimationPhaseBox == null || _billingPhaseBox == null)
        {
            return;
        }
        if (!root.TryGetProperty("takeoffPhases", out var phases) || phases.ValueKind != JsonValueKind.Object)
        {
            return;
        }
        var key = phases.GetRawText();
        if (key == _phaseKey)
        {
            return;
        }
        _phaseKey = key;
        phases.TryGetProperty("current", out var current);
        FillPhaseBox(_estimationPhaseBox, phases, "estimation", current);
        FillPhaseBox(_billingPhaseBox, phases, "billing", current);
    }

    private void FillPhaseBox(ComboBox box, JsonElement phases, string which, JsonElement current)
    {
        _filling = true;
        try
        {
            box.Items.Clear();
            box.Items.Add(new ComboBoxItem { Content = "(no phase)", Tag = NoPhase });
            if (phases.TryGetProperty(which, out var list) && list.ValueKind == JsonValueKind.Array)
            {
                foreach (var name in list.EnumerateArray())
                {
                    if (name.ValueKind == JsonValueKind.String && name.GetString() is { Length: > 0 } text)
                    {
                        box.Items.Add(new ComboBoxItem { Content = text, Tag = text });
                    }
                }
            }
            var chosen = current.ValueKind == JsonValueKind.Object && current.TryGetProperty(which, out var value) && value.ValueKind == JsonValueKind.String
                ? value.GetString() ?? NoPhase
                : NoPhase;
            box.SelectedIndex = 0;
            for (var i = 0; i < box.Items.Count; i++)
            {
                if (box.Items[i] is ComboBoxItem { Tag: string tag } && tag == chosen)
                {
                    box.SelectedIndex = i;
                    break;
                }
            }
        }
        finally
        {
            _filling = false;
        }
    }
}

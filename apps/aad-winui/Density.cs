using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Aad.WinUi;

/// <summary>
/// Design tokens and helpers for code-built UI. XAML counterparts live in <c>Themes/*.xaml</c>.
/// Compact CAD density with native WinUI 3 controls. Targets and rules: <c>docs/UI_STANDARDS.md</c>.
/// </summary>
internal static class Density
{
    // --- Spacing: 4 px scale (Space1 tight · Space2 standard · Space3 group · Space4 section
    // · Space5 major · Space6 large) ---
    public const double Space1 = 4;
    public const double Space2 = 8;
    public const double Space3 = 12;
    public const double Space4 = 16;
    public const double Space5 = 20;
    public const double Space6 = 24;

    // Older names, snapped to the scale. SpaceXs (2) is only for icon strips.
    public const double SpaceXs = 2;
    public const double SpaceSm = Space1;
    public const double SpaceMd = 6;
    public const double SpaceLg = Space2;
    public const double SpaceXl = Space2;
    public const double SpaceSection = 10;
    public const double SpaceXxl = 10;

    public static Thickness DialogMargin { get; } = new Thickness(Space2);
    public static Thickness FooterPadding { get; } = new Thickness(Space2, Space1, Space2, Space1);
    public static Thickness ToolbarPadding { get; } = new Thickness(Space1, SpaceXs, Space1, SpaceXs);
    public static Thickness ButtonPadding { get; } = new Thickness(Space3, SpaceXs, Space3, SpaceXs);
    public static Thickness PanelPadding { get; } = new Thickness(Space2, Space1, Space2, Space1);
    public static Thickness PanelPaddingTight { get; } = new Thickness(Space2, Space1, Space2, Space1);
    public static Thickness CommandBarPadding { get; } = new Thickness(Space3, Space1, Space3, Space1);
    public static Thickness StatusPadding { get; } = new Thickness(Space2, SpaceXs, Space2, SpaceXs);
    public static Thickness LabelFieldGap { get; } = new Thickness(0, 0, 0, Space1);
    public static Thickness ControlPadding { get; } = new Thickness(Space2, SpaceXs, Space2, SpaceXs);
    public static Thickness PageMargin { get; } = new Thickness(Space4);
    /// <summary>Horizontal inset of text inside a table / list cell.</summary>
    public static Thickness CellPadding { get; } = new Thickness(Space1, 0, Space1, 0);

    // --- Control sizing (menu 24 · toolbar btn 26 · icon 16 · input/button 26 · pane 240) ---
    public const double MinTarget = 24;
    public const double ControlHeight = 24;
    public const double ControlHeightCompact = 22;
    public const double IconButton = 26;
    public const double IconSize = 16;
    public const double IconSizeSm = 16;
    /// <summary>Colour chip in a table cell or list row.</summary>
    public const double SwatchSize = 14;
    public const double IconStroke = 1.5;
    public const double MenuBarHeight = 24;
    public const double MenuItemHeight = 22;
    public const double FileTabHeight = 28;
    public const double StatusBarHeight = 22;
    public const double CommandLineHeight = 22;
    public const double PropertyRowHeight = 24;
    public const double DialogButtonHeight = 24;
    public const double SplitterWidth = 4;
    public const double DividerThickness = 1;
    /// <summary>Default docked side-panel width (matches <c>SidePanelWidth</c>).</summary>
    public const double SidePanelWidth = 240;
    /// <summary>Collapsed side-panel title rail width (matches <c>SideRailWidth</c>).</summary>
    public const double SideRailWidth = 28;
    /// <summary>Left list column in style / manager dialogs.</summary>
    public const double ManagerListWidth = 160;
    /// <summary>Smallest useful height of a list in a dialog.</summary>
    public const double ListMinHeight = 160;
    /// <summary>Tallest list in a picker dialog before it scrolls.</summary>
    public const double PickerListMaxHeight = 260;
    /// <summary>Widest drawing tab label before it is trimmed.</summary>
    public const double ViewTabMaxWidth = 160;
    /// <summary>Numeric and short text fields in dialogs.</summary>
    public const double FieldWidth = 104;
    /// <summary>Cell editor in an inline table row.</summary>
    public const double TableCellWidth = 88;
    public const double DialogActionMinWidth = 64;
    /// <summary>Layer-state / status combo in managers (matches Themes LayerStateComboWidth).</summary>
    public const double LayerStateComboWidth = 140;
    /// <summary>Search / name fields (matches Themes SearchFieldWidth).</summary>
    public const double SearchFieldWidth = 160;
    /// <summary>Hatch pattern tile (readable, tighter than 72).</summary>
    public const double HatchSwatch = 56;
    /// <summary>Application mark in About.</summary>
    public const double LogoSize = 64;
    /// <summary>Welcome overlay logo.</summary>
    public const double LogoWelcome = 72;
    /// <summary>In-window splash logo.</summary>
    public const double LogoSplash = 96;
    /// <summary>Parametric graph canvas default extent.</summary>
    public const double GraphCanvasWidth = 720;
    public const double GraphCanvasHeight = 520;
    /// <summary>Wide manager / graph window minimums.</summary>
    public const double WindowMinWidthWide = 640;
    public const double WindowMinHeightGraph = 480;
    /// <summary>History / revisions panel minimums.</summary>
    public const double HistoryPanelMinWidth = 360;
    public const double HistoryPanelMinHeight = 420;
    public const double HistoryListMinHeight = 200;
    /// <summary>Command-history popup max height in the Ask bar.</summary>
    public const double HistoryListMaxHeight = 200;
    /// <summary>Short initials / code fields on the revision register.</summary>
    public const double InitialsFieldMinWidth = 64;
    /// <summary>Status-bar page-orientation combo minimum width.</summary>
    public const double OrientComboMinWidth = 96;
    /// <summary>Layer combo upper bound on the tool strip.</summary>
    public const double LayerComboMaxWidth = 200;
    /// <summary>Ask-bar avatar chip and send button.</summary>
    public const double AskAvatarSize = 28;
    public const double AskSendButtonSize = 32;
    /// <summary>Welcome / busy overlay card.</summary>
    public const double WelcomeMaxWidth = 520;
    public const double BusyCardMinWidth = 280;
    public const double BusyRingSize = 40;
    /// <summary>Ask-bar pill inner padding (slightly tighter than SpaceMd×SpaceSm).</summary>
    public static Thickness AskPillPadding { get; } = new Thickness(SpaceSm + SpaceXs, Space1, SpaceSm + SpaceXs, Space1);
    /// <summary>Upper bound for a status / validation line beside footer actions.</summary>
    public const double StatusMaxWidth = 420;
    public const double CornerChrome = 2;
    public const double CornerControl = 4;
    /// <summary>Recent-file row on the welcome screen (horizontal Space3, vertical Space1).</summary>
    public static Thickness RecentItemPadding { get; } = new Thickness(Space3, Space1, Space3, Space1);
    /// <summary>Busy overlay card inset (Space5+SpaceXs by Space4+SpaceXs ≈ previous 28,24).</summary>
    public static Thickness BusyCardPadding { get; } = new Thickness(Space6 + Space1, Space4 + Space2, Space6 + Space1, Space4 + Space2);

    // --- Window dimensions that were literals (values unchanged; keep in sync with Themes/*.xaml) ---
    public const double ColorPreviewSize = 72;
    public const double ColorBoxWidth = 72;
    public const double RgbBoxWidth = 56;
    public const double BookPanelMinHeight = 220;
    public const double BookListMinHeight = 200;
    public const double BookListMaxHeight = 320;
    public const double SwatchChipWidth = 22;
    public const double NotesListHeight = 150;
    public const double NotesListMinWidth = 170;
    public const double NotesBoxMinHeight = 150;
    public const double NotesLeftMinWidth = 180;
    public const double NotesRightMinWidth = 380;
    public const double TreeHeight = 280;
    public const double RowMinHeight = 36;
    public const double ListMinHeightCompact = 120;
    public const double DialogBodyMaxWidth = 420;

    // --- Typography (caption 11 · body/label 12 · section 13 semibold · page title 18) ---
    public static double Font = FontBody;
    public static bool OverrideFace;
    public static string Face = "Segoe UI Variable";
    public const double FontCaption = 11;
    public const double FontBody = 12;
    public const double FontCommand = 12;
    /// <summary>Fluent glyph size inside compact icon buttons (Ask send).</summary>
    public const double FontIcon = 14;
    public const double FontSection = 13;
    public const double FontHeading = 13;
    public const double FontPageTitle = 18;
    /// <summary>Large read-out (timer, result), not a heading.</summary>
    public const double FontDisplay = 22;
    /// <summary>Preferred UI face name; use <see cref="FontUiResolved"/> after <see cref="ShellFonts.Resolve"/>.</summary>
    public const string FontUi = "Segoe UI";
    /// <summary>Preferred mono face name; use <see cref="FontMonoResolved"/> after resolve.</summary>
    public const string FontMono = "Consolas";
    public static string FontUiResolved { get; set; } = FontUi;
    public static string FontMonoResolved { get; set; } = FontMono;

    public static void Use(string preset)
    {
        OverrideFace = true;
        Font = FontBody;
        Face = preset switch
        {
            "editorial" => "Georgia",
            "drafting" => "Consolas",
            _ => "Segoe UI",
        };
    }

    public static void Apply(DependencyObject node)
    {
        if (node is Button button)
        {
            CompactDialogButton(button);
        }
        else if (node is TextBox box)
        {
            CompactDialogField(box);
        }
        else if (node is ComboBox combo)
        {
            CompactDialogCombo(combo);
        }
        else if (node is CheckBox check)
        {
            if (check.FontSize < FontSection)
            {
                check.FontSize = FontBody;
            }
        }
        else if (node is Control control)
        {
            // Normalize Fluent defaults; keep section/page titles (≥ FontSection).
            if (control.FontSize < FontSection)
            {
                control.FontSize = Font;
            }
            if (OverrideFace)
            {
                control.FontFamily = new FontFamily(Face);
            }
        }
        else if (node is TextBlock text)
        {
            // Normalize Fluent defaults (~14) to body; keep section/page titles (≥ FontSection).
            if (text.FontSize < FontSection)
            {
                text.FontSize = Font;
            }
            if (OverrideFace && text.FontFamily?.Source != FontMonoResolved)
            {
                text.FontFamily = new FontFamily(Face);
            }
            if (OverrideFace && Face == FontMono)
            {
                text.FontFamily = new FontFamily(Face);
            }
        }
        var count = VisualTreeHelper.GetChildrenCount(node);
        for (var i = 0; i < count; i++)
        {
            Apply(VisualTreeHelper.GetChild(node, i));
        }
    }

    /// <summary>Primary/secondary dialog action. Kept for older call sites; prefer <see cref="Ui.Action"/>.</summary>
    public static Button DialogAction(string label, bool primary = false) =>
        Ui.Action(label, primary ? ActionKind.Primary : ActionKind.Secondary);

    public static TextBox DialogField(string text = "", string? name = null)
    {
        var box = new TextBox
        {
            Text = text,
            MinHeight = ControlHeight,
            Padding = ControlPadding,
            FontSize = FontBody,
            CornerRadius = new CornerRadius(CornerControl),
        };
        if (TryFindStyle("DialogFieldStyle", out var style))
        {
            box.Style = style;
            box.MinHeight = ControlHeight;
            box.Padding = ControlPadding;
            box.FontSize = FontBody;
        }
        if (!string.IsNullOrEmpty(name))
        {
            Name(box, name);
        }
        return box;
    }

    public static ComboBox DialogCombo(string? name = null)
    {
        var combo = new ComboBox
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            MinHeight = ControlHeight,
            Padding = ControlPadding,
            FontSize = FontBody,
            CornerRadius = new CornerRadius(CornerControl),
        };
        if (TryFindStyle("DialogComboStyle", out var style))
        {
            combo.Style = style;
            combo.MinHeight = ControlHeight;
            combo.Padding = ControlPadding;
            combo.FontSize = FontBody;
        }
        if (!string.IsNullOrEmpty(name))
        {
            Name(combo, name);
        }
        return combo;
    }

    private static void CompactDialogButton(Button button)
    {
        if (button.FontSize < FontSection)
        {
            button.FontSize = FontBody;
        }
        // Leave explicit icon / tool-strip sizes alone; shrink Fluent defaults (~32).
        if (double.IsNaN(button.MinHeight) || button.MinHeight > DialogButtonHeight + 2)
        {
            button.MinHeight = DialogButtonHeight;
        }
        if (button.Padding.Top > 4 || button.Padding.Bottom > 4)
        {
            button.Padding = ButtonPadding;
        }
        if (OverrideFace)
        {
            button.FontFamily = new FontFamily(Face);
        }
    }

    private static void CompactDialogField(TextBox box)
    {
        if (box.FontSize < FontSection)
        {
            box.FontSize = FontBody;
        }
        if (double.IsNaN(box.MinHeight) || box.MinHeight > ControlHeight + 2)
        {
            box.MinHeight = ControlHeight;
        }
        if (box.Padding.Top > 4 || box.Padding.Bottom > 4)
        {
            box.Padding = ControlPadding;
        }
        if (OverrideFace)
        {
            box.FontFamily = new FontFamily(Face);
        }
    }

    private static void CompactDialogCombo(ComboBox combo)
    {
        if (combo.FontSize < FontSection)
        {
            combo.FontSize = FontBody;
        }
        if (double.IsNaN(combo.MinHeight) || combo.MinHeight > ControlHeight + 2)
        {
            combo.MinHeight = ControlHeight;
        }
        if (OverrideFace)
        {
            combo.FontFamily = new FontFamily(Face);
        }
    }

    internal static bool TryFindStyle(string key, out Style style)
    {
        style = null!;
        if (Application.Current?.Resources.TryGetValue(key, out var value) == true && value is Style found)
        {
            style = found;
            return true;
        }
        return false;
    }

    /// <summary>The theme the shell is showing. Brushes are looked up in this theme's dictionary.</summary>
    public static ElementTheme Theme { get; set; } = ElementTheme.Default;

    public static Brush? ThemeBrush(string key)
    {
        var name = Theme == ElementTheme.Light ? "Light" : "Default";
        if (FindThemed(Application.Current.Resources, name, key, 0) is { } brush)
        {
            return brush;
        }
        if (Theme != ElementTheme.Light
            && FindThemed(Application.Current.Resources, "Dark", key, 0) is { } darkBrush)
        {
            return darkBrush;
        }
        return Application.Current.Resources.TryGetValue(key, out var fallback) && fallback is Brush any ? any : null;
    }

    public static Windows.UI.Color? ThemeColor(string key)
    {
        var name = Theme == ElementTheme.Light ? "Light" : "Default";
        if (FindThemedObject(Application.Current.Resources, name, key, 0) is Windows.UI.Color color)
        {
            return color;
        }
        if (Theme != ElementTheme.Light
            && FindThemedObject(Application.Current.Resources, "Dark", key, 0) is Windows.UI.Color dark)
        {
            return dark;
        }
        return null;
    }

    /// <summary>App shell / page fill for the active theme.</summary>
    public static Brush AppBackground() =>
        ThemeBrush("AppBackgroundBrush")
        ?? new SolidColorBrush(Theme == ElementTheme.Light
            ? Windows.UI.Color.FromArgb(255, 0xF3, 0xF3, 0xF3)
            : Windows.UI.Color.FromArgb(255, 0x1B, 0x1B, 0x1B));

    public static Brush CanvasBackground() =>
        ThemeBrush("CanvasBackgroundBrush")
        ?? new SolidColorBrush(Theme == ElementTheme.Light
            ? Microsoft.UI.Colors.White
            : Microsoft.UI.Colors.Black);

    /// <summary>Accent colour from tokens (for selection zebra fills).</summary>
    public static Windows.UI.Color AccentColor()
    {
        if (ThemeBrush("AccentBrush") is SolidColorBrush brush)
        {
            return brush.Color;
        }
        return Theme == ElementTheme.Light
            ? Windows.UI.Color.FromArgb(255, 0x10, 0x2C, 0x80)
            : Windows.UI.Color.FromArgb(255, 0x6F, 0x8F, 0xF0);
    }

    public static Brush SelectionFill(byte alpha = 64)
    {
        var c = AccentColor();
        return new SolidColorBrush(Windows.UI.Color.FromArgb(alpha, c.R, c.G, c.B));
    }

    public static Brush ZebraFill() =>
        new SolidColorBrush(Windows.UI.Color.FromArgb(16, 128, 128, 128));

    /// <summary>Secondary body text at full opacity (avoids Opacity &lt; 1 contrast failures).</summary>
    public static TextBlock Secondary(string text, TextWrapping wrap = TextWrapping.NoWrap)
    {
        var block = new TextBlock { Text = text, TextWrapping = wrap, FontSize = FontBody };
        block.Foreground = ThemeBrush("TextSecondaryBrush")
            ?? ThemeBrush("TextFillColorSecondaryBrush");
        return block;
    }

    public static TextBlock SectionHeading(string text)
    {
        var block = new TextBlock
        {
            Text = text,
            FontSize = FontSection,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            Foreground = ThemeBrush("TextSecondaryBrush")
                ?? ThemeBrush("TextFillColorSecondaryBrush"),
        };
        Name(block, text);
        return block;
    }

    public static StackPanel DialogStack(params UIElement[] children) => Ui.Stack(children);

    public static Grid FieldRow(string label, FrameworkElement field, string? info = null) => Ui.Labeled(label, field, info);

    public static void Name(DependencyObject target, string name)
    {
        if (name.Length > 0)
        {
            AutomationProperties.SetName(target, name);
        }
    }

    private static Brush? FindThemed(ResourceDictionary dictionary, string theme, string key, int depth)
    {
        if (FindThemedObject(dictionary, theme, key, depth) is Brush brush)
        {
            return brush;
        }
        return null;
    }

    private static object? FindThemedObject(ResourceDictionary dictionary, string theme, string key, int depth)
    {
        if (depth > 8)
        {
            return null;
        }
        if (dictionary.ThemeDictionaries.TryGetValue(theme, out var themed)
            && themed is ResourceDictionary inner
            && inner.TryGetValue(key, out var value))
        {
            return value;
        }
        foreach (var merged in dictionary.MergedDictionaries)
        {
            if (FindThemedObject(merged, theme, key, depth + 1) is { } found)
            {
                return found;
            }
        }
        return null;
    }
}

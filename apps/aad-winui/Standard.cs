using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Windows.System;
using WinRT.Interop;

namespace Aad.WinUi;

/// <summary>
/// The approved window templates (docs/UI_STANDARDS.md). Every secondary window declares one
/// through <see cref="StandardWindow.Present"/>; the template decides chrome, ownership,
/// resizing, caption buttons and keyboard behaviour.
/// </summary>
internal enum WindowTemplate
{
    /// <summary>Confirmation, small form or settings: sized to content, not resizable, Esc closes.</summary>
    Dialog,
    /// <summary>Property / style dialog: content-sized first, user-resizable, size remembered, Esc closes.</summary>
    PropertyDialog,
    /// <summary>Data manager (layers, hatch, plot styles): resizable and maximizable, size remembered.</summary>
    Manager,
    /// <summary>Floating tool palette beside the canvas: resizable, no maximize, size remembered.</summary>
    Floating,
}

/// <summary>Which footer button style an action uses. One primary per footer.</summary>
internal enum ActionKind
{
    Primary,
    Secondary,
    Destructive,
}

/// <summary>Minimum and maximum client size in DIP. Use a template default unless the content needs more.</summary>
internal readonly record struct WindowBounds(double MinWidth, double MinHeight, double MaxWidth, double MaxHeight)
{
    public static WindowBounds For(WindowTemplate template) => template switch
    {
        WindowTemplate.Dialog => new WindowBounds(320, 72, 560, 640),
        WindowTemplate.PropertyDialog => new WindowBounds(400, 320, 720, 800),
        WindowTemplate.Manager => new WindowBounds(560, 360, 1400, 1000),
        _ => new WindowBounds(260, 240, 480, 900),
    };
}

/// <summary>
/// Opens secondary windows the same way: solid themed chrome with the native caption (one title per
/// window, no in-content duplicate), owned by the shell so it stays above the drawing and leaves the
/// taskbar alone, template sizing through <see cref="ShellFit"/>, and optional modality.
/// </summary>
internal static class StandardWindow
{
    public static void Present(
        Window window,
        Window? owner,
        WindowTemplate template,
        string? persistKey = null,
        WindowBounds? bounds = null,
        bool modal = false)
    {
        var size = bounds ?? WindowBounds.For(template);
        owner ??= ShellWindows.Main;
        ShellIcon.Apply(window, ShellChrome.ThemeOf(owner));
        var ownerHwnd = owner is null ? IntPtr.Zero : WindowNative.GetWindowHandle(owner);
        if (ownerHwnd != IntPtr.Zero)
        {
            Native.SetWindowLongPtr(WindowNative.GetWindowHandle(window), Native.GwlpHwndParent, ownerHwnd);
        }
        if (template == WindowTemplate.Dialog)
        {
            ShellFit.ToContent(window, size.MinWidth, size.MinHeight, size.MaxWidth, size.MaxHeight);
        }
        else
        {
            ShellFit.UserResizable(window, size.MinWidth, size.MinHeight, size.MaxWidth, size.MaxHeight, persistKey);
        }
        if (window.AppWindow.Presenter is OverlappedPresenter presenter)
        {
            // An owned window minimizes with its owner; its own minimize button would strand it.
            presenter.IsMinimizable = ownerHwnd == IntPtr.Zero;
            presenter.IsMaximizable = template == WindowTemplate.Manager;
        }
        if (template is WindowTemplate.Dialog or WindowTemplate.PropertyDialog && window.Content is UIElement root)
        {
            // Bubbling KeyDown: an open drop-down handles Esc itself first.
            root.KeyDown += (_, e) =>
            {
                if (e.Key == VirtualKey.Escape && !e.Handled)
                {
                    e.Handled = true;
                    window.Close();
                }
            };
        }
        if (modal && ownerHwnd != IntPtr.Zero)
        {
            Native.EnableWindow(ownerHwnd, false);
            window.Closed += (_, _) =>
            {
                Native.EnableWindow(ownerHwnd, true);
                owner?.Activate();
            };
        }
    }
}

/// <summary>
/// Shared components for code-built windows. All sizes come from <see cref="Density"/> tokens; window
/// files compose these instead of setting margins, paddings or font sizes themselves.
/// </summary>
internal static class Ui
{
    /// <summary>A footer or toolbar action with the shared height, padding and minimum width.</summary>
    public static Button Action(string label, ActionKind kind = ActionKind.Secondary)
    {
        var button = new Button
        {
            Content = label,
            MinWidth = Density.DialogActionMinWidth,
            MinHeight = Density.DialogButtonHeight,
            Padding = Density.ButtonPadding,
            FontSize = Density.FontBody,
        };
        var key = kind switch
        {
            ActionKind.Primary => "PrimaryActionStyle",
            ActionKind.Destructive => "DestructiveActionStyle",
            _ => "CompactButtonStyle",
        };
        if (Density.TryFindStyle(key, out var style))
        {
            button.Style = style;
        }
        Density.Name(button, label);
        return button;
    }

    /// <summary>
    /// Pinned dialog footer: an optional status / validation line on the left, actions right-aligned
    /// in the order given. Put the primary action immediately before Cancel.
    /// </summary>
    public static Grid Footer(TextBlock? status, params Button[] actions)
    {
        var footer = new Grid
        {
            Padding = Density.FooterPadding,
            ColumnSpacing = Density.Space3,
            Background = Density.ThemeBrush("SurfaceSecondaryBrush") ?? Density.AppBackground(),
            BorderBrush = Density.ThemeBrush("BorderSubtleBrush") ?? Density.ZebraFill(),
            BorderThickness = new Thickness(0, Density.DividerThickness, 0, 0),
        };
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        if (status is not null)
        {
            status.VerticalAlignment = VerticalAlignment.Center;
            footer.Children.Add(status);
        }
        var row = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = Density.Space2,
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
        };
        foreach (var action in actions)
        {
            row.Children.Add(action);
        }
        Grid.SetColumn(row, 1);
        footer.Children.Add(row);
        return footer;
    }

    /// <summary>A status / validation line: secondary text, wraps, announced to screen readers.</summary>
    public static TextBlock Status(string accessName)
    {
        var block = Density.Secondary("", TextWrapping.Wrap);
        block.MaxWidth = Density.StatusMaxWidth;
        AutomationProperties.SetLiveSetting(block, AutomationLiveSetting.Polite);
        Density.Name(block, accessName);
        return block;
    }

    /// <summary>Vertical content stack with standard row spacing and dialog padding.</summary>
    public static StackPanel Stack(params UIElement[] children)
    {
        var stack = new StackPanel { Spacing = Density.Space2, Margin = Density.DialogMargin };
        foreach (var child in children)
        {
            stack.Children.Add(child);
        }
        return stack;
    }

    /// <summary>A small (i) icon whose tooltip is <paramref name="description"/>; focusable, so the description is also reachable by keyboard.</summary>
    public static FrameworkElement Info(string description)
    {
        var icon = new FontIcon { Glyph = "", FontSize = 12, Opacity = 0.7 };
        var host = new ContentControl
        {
            Content = icon,
            IsTabStop = true,
            VerticalAlignment = VerticalAlignment.Center,
            Padding = new Thickness(2),
        };
        ToolTipService.SetToolTip(host, description);
        ToolTipService.SetPlacement(host, Microsoft.UI.Xaml.Controls.Primitives.PlacementMode.Right);
        AutomationProperties.SetName(host, "Info");
        AutomationProperties.SetHelpText(host, description);
        host.PointerEntered += (_, _) => icon.Opacity = 1;
        host.PointerExited += (_, _) => icon.Opacity = 0.7;
        return host;
    }

    /// <summary>Label beside its field, so a form row is one line tall; the label column is wide enough for short labels and wraps beyond that.</summary>
    public static Grid Labeled(string label, FrameworkElement field, string? info = null)
    {
        var row = new Grid { ColumnSpacing = Density.Space2 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(112) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var text = new TextBlock { Text = label, FontSize = Density.FontBody, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
        Density.Name(text, label);
        // A field that already carries a tooltip gets it as the description; the field's own tooltip is dropped so it is not shown twice.
        if (info is null && ToolTipService.GetToolTip(field) is string tip && tip.Length > 0)
        {
            info = tip;
            ToolTipService.SetToolTip(field, null);
        }
        if (info is { Length: > 0 })
        {
            var cell = new Grid { ColumnSpacing = 4 };
            cell.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            cell.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            var icon = Info(info);
            Grid.SetColumn(icon, 1);
            cell.Children.Add(text);
            cell.Children.Add(icon);
            row.Children.Add(cell);
        }
        else
        {
            row.Children.Add(text);
        }
        Grid.SetColumn(field, 1);
        field.VerticalAlignment = VerticalAlignment.Center;
        row.Children.Add(field);
        return row;
    }

    /// <summary>
    /// Label/field rows on a shared two-column grid, so labels and fields line up across the
    /// whole form: labels in an auto column, fields in the remaining width.
    /// </summary>
    public static Grid Form(params (string Label, FrameworkElement Field)[] rows)
    {
        var grid = new Grid { RowSpacing = Density.Space2, ColumnSpacing = Density.Space3 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        for (var i = 0; i < rows.Length; i++)
        {
            var (label, field) = rows[i];
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var text = new TextBlock { Text = label, FontSize = Density.FontBody, VerticalAlignment = VerticalAlignment.Center };
            Density.Name(text, label);
            AutomationProperties.SetName(field, AutomationProperties.GetName(field) is { Length: > 0 } named ? named : label);
            Grid.SetRow(text, i);
            Grid.SetRow(field, i);
            Grid.SetColumn(field, 1);
            grid.Children.Add(text);
            grid.Children.Add(field);
        }
        return grid;
    }

    /// <summary>A titled group: section heading (subordinate to the window title) over its content.</summary>
    public static StackPanel Section(string heading, params UIElement[] body)
    {
        var section = new StackPanel { Spacing = Density.Space2 };
        var title = Density.SectionHeading(heading);
        AutomationProperties.SetHeadingLevel(title, AutomationHeadingLevel.Level2);
        section.Children.Add(title);
        foreach (var child in body)
        {
            section.Children.Add(child);
        }
        return section;
    }

    /// <summary>Manager toolbar strip. Icon-only buttons must carry a tooltip and an accessible name.</summary>
    public static Border Toolbar(params UIElement[] items)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.SpaceXs, VerticalAlignment = VerticalAlignment.Center };
        foreach (var item in items)
        {
            row.Children.Add(item);
        }
        var strip = new Border { Child = row };
        if (Density.TryFindStyle("ManagerToolbarStyle", out var style))
        {
            strip.Style = style;
        }
        return strip;
    }

    /// <summary>Thin vertical separator between toolbar groups.</summary>
    public static Border ToolbarDivider()
    {
        var line = new Border();
        if (Density.TryFindStyle("ToolStripDividerStyle", out var style))
        {
            line.Style = style;
        }
        return line;
    }

    /// <summary>
    /// Dialog template body: scrolling content with dialog padding over a pinned footer, so the
    /// actions never scroll away and the window can size to the content.
    /// </summary>
    public static Grid DialogPage(UIElement content, Grid footer)
    {
        var page = new Grid();
        page.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var scroll = new ScrollViewer
        {
            Content = content,
            Padding = Density.DialogMargin,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };
        page.Children.Add(scroll);
        Grid.SetRow(footer, 1);
        page.Children.Add(footer);
        return page;
    }

    /// <summary>
    /// Manager template body: toolbar, then the main content taking every remaining pixel, then
    /// the footer. The content manages its own scrolling (lists, tables, grids).
    /// </summary>
    public static Grid ManagerPage(UIElement? toolbar, FrameworkElement content, Grid? footer)
    {
        var page = new Grid();
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        page.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        if (toolbar is not null)
        {
            page.Children.Add(toolbar);
        }
        Grid.SetRow(content, 1);
        page.Children.Add(content);
        if (footer is not null)
        {
            Grid.SetRow(footer, 2);
            page.Children.Add(footer);
        }
        return page;
    }
}

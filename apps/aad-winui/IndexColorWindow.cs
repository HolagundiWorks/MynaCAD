using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Aad.WinUi;

/// <summary>
/// <summary>ACI Select Color dialog: Index Color grid (256 ACI), True Color, preview, and optional ByLayer/ByBlock.</summary>
/// </summary>
internal static class IndexColorWindow
{
    private const double GridCell = 11.0;
    private const double StandardCell = 18.0;

    public static Task<string?> PickAsync(Window owner, string currentColorName, bool allowByLayerByBlock)
    {
        if (!CadColor.TryParse(currentColorName, out var initial))
        {
            initial = new CadColorSelection(CadColorKind.ByLayer);
        }
        if (!allowByLayerByBlock && initial.Kind is CadColorKind.ByLayer or CadColorKind.ByBlock)
        {
            initial = new CadColorSelection(CadColorKind.Index, 7);
        }

        var tcs = new TaskCompletionSource<string?>();
        var finished = false;
        void Finish(string? value)
        {
            if (finished)
            {
                return;
            }
            finished = true;
            tcs.TrySetResult(value);
        }

        var theme = owner.Content is FrameworkElement shell ? shell.RequestedTheme : ElementTheme.Default;
        CadColorSelection selection = initial;
        var cells = new Dictionary<byte, Border>();

        var preview = new Border
        {
            Width = 72,
            Height = 72,
            BorderBrush = Density.ThemeBrush("BorderDefaultBrush"),
            BorderThickness = new Thickness(1),
        };
        var colorName = DialogLayout.Field("Color name", 200);
        colorName.Text = initial.DisplayName;
        var indexBox = new NumberBox
        {
            Minimum = 1,
            Maximum = 255,
            SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact,
            Width = 72,
        };
        var rBox = new NumberBox { Minimum = 0, Maximum = 255, Width = 56, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
        var gBox = new NumberBox { Minimum = 0, Maximum = 255, Width = 56, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
        var bBox = new NumberBox { Minimum = 0, Maximum = 255, Width = 56, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };

        var indexPanel = new StackPanel { Spacing = Density.SpaceSm };
        var truePanel = new StackPanel { Spacing = Density.SpaceSm };
        var bookPanel = new StackPanel { Spacing = Density.SpaceSm, MinHeight = 220 };
        ListView? bookListHost = null;
        var colorPicker = new ColorPicker
        {
            IsAlphaEnabled = false,
            IsColorSpectrumVisible = true,
            IsColorPreviewVisible = false,
            IsColorSliderVisible = true,
            IsHexInputVisible = true,
        };

        void SyncFieldsFromSelection()
        {
            preview.Background = BrushOf(selection);
            colorName.Text = selection.DisplayName;
            if (selection.Kind == CadColorKind.Index)
            {
                indexBox.Value = selection.Index;
            }
            var (r, g, b) = selection.ResolveRgb();
            rBox.Value = r;
            gBox.Value = g;
            bBox.Value = b;
            foreach (var (idx, border) in cells)
            {
                var sel = selection.Kind == CadColorKind.Index && selection.Index == idx;
                border.BorderBrush = sel
                    ? new SolidColorBrush(Microsoft.UI.Colors.White)
                    : Density.ThemeBrush("BorderSubtleBrush") ?? new SolidColorBrush(Microsoft.UI.Colors.Gray);
                border.BorderThickness = sel ? new Thickness(2) : new Thickness(1);
            }
            if (selection.Kind == CadColorKind.True)
            {
                colorPicker.Color = Windows.UI.Color.FromArgb(255, selection.R, selection.G, selection.B);
            }
            UpdateBookListHighlight();
        }

        void SelectIndex(byte index)
        {
            selection = new CadColorSelection(CadColorKind.Index, index);
            SyncFieldsFromSelection();
        }

        Border MakeCell(byte index, double size)
        {
            var border = new Border
            {
                Width = size,
                Height = size,
                Background = BrushOf(new CadColorSelection(CadColorKind.Index, index)),
                BorderThickness = new Thickness(1),
                BorderBrush = Density.ThemeBrush("BorderSubtleBrush") ?? new SolidColorBrush(Microsoft.UI.Colors.Gray),
                Tag = index,
            };
            border.PointerPressed += (_, _) => SelectIndex(index);
            cells[index] = border;
            return border;
        }

        var graded = new StackPanel { Spacing = 0 };
        foreach (var shade in AciPalette.ShadeOrder)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 0 };
            for (byte hue = 0; hue < 24; hue++)
            {
                var index = (byte)(10 + hue * 10 + shade);
                row.Children.Add(MakeCell(index, GridCell));
            }
            graded.Children.Add(row);
        }

        var standardRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, Margin = new Thickness(0, 6, 0, 0) };
        for (byte i = 1; i <= 9; i++)
        {
            standardRow.Children.Add(MakeCell(i, StandardCell));
        }
        standardRow.Children.Add(new Border { Width = 12 });
        for (byte i = 250; i <= 255; i++)
        {
            standardRow.Children.Add(MakeCell(i, StandardCell));
        }

        indexPanel.Children.Add(graded);
        indexPanel.Children.Add(standardRow);

        truePanel.Children.Add(colorPicker);
        colorPicker.ColorChanged += (_, _) =>
        {
            var c = colorPicker.Color;
            selection = new CadColorSelection(CadColorKind.True, R: c.R, G: c.G, B: c.B);
            SyncFieldsFromSelection();
        };

        var books = ColorBookCatalog.All();
        var bookCombo = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch, MinHeight = Density.ControlHeight };
        Density.Name(bookCombo, "Color book");
        foreach (var book in books)
        {
            bookCombo.Items.Add(new ComboBoxItem { Content = book.Name, Tag = book });
        }
        var bookList = new ListView { SelectionMode = ListViewSelectionMode.Single, MinHeight = 200, MaxHeight = 320 };
        bookListHost = bookList;
        Density.Name(bookList, "Color book swatches");
        ColorBook? activeBook = null;

        void SelectBookEntry(ColorBook book, ColorBookEntry entry)
        {
            selection = new CadColorSelection(CadColorKind.Book, R: entry.R, G: entry.G, B: entry.B, BookName: book.Name, EntryName: entry.Name);
            SyncFieldsFromSelection();
        }

        void UpdateBookListHighlight()
        {
            if (bookListHost is null)
            {
                return;
            }
            foreach (var obj in bookListHost.Items)
            {
                if (obj is not ListViewItem item || item.Tag is not ColorBookEntry entry || !item.IsEnabled)
                {
                    continue;
                }
                var sel = selection.Kind == CadColorKind.Book
                    && selection.EntryName == entry.Name
                    && selection.R == entry.R && selection.G == entry.G && selection.B == entry.B;
                item.Background = sel
                    ? Density.ThemeBrush("ControlSelectedBrush") ?? new SolidColorBrush(Microsoft.UI.Colors.DodgerBlue)
                    : new SolidColorBrush(Microsoft.UI.Colors.Transparent);
            }
        }

        void FillBookList(ColorBook? book)
        {
            activeBook = book;
            bookList.Items.Clear();
            if (book is null)
            {
                bookList.Items.Add(new ListViewItem { IsEnabled = false, Content = Density.Secondary("No colour books found. Add unencrypted .acb files to ColorBooks beside the app or under %LOCALAPPDATA%\\O-Draft\\ColorBooks.", TextWrapping.Wrap) });
                return;
            }
            string? lastPage = null;
            foreach (var entry in book.Entries)
            {
                if (entry.PageLabel != lastPage)
                {
                    lastPage = entry.PageLabel;
                    bookList.Items.Add(new ListViewItem
                    {
                        IsEnabled = false,
                        Content = new TextBlock
                        {
                            Text = lastPage,
                            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
                            Opacity = 0.85,
                            Margin = new Thickness(0, 6, 0, 2),
                        },
                    });
                }
                var swatch = new Border
                {
                    Width = 22,
                    Height = 14,
                    Background = new SolidColorBrush(Windows.UI.Color.FromArgb(255, entry.R, entry.G, entry.B)),
                    BorderBrush = Density.ThemeBrush("BorderSubtleBrush"),
                    BorderThickness = new Thickness(1),
                    VerticalAlignment = VerticalAlignment.Center,
                };
                var row = new StackPanel
                {
                    Orientation = Orientation.Horizontal,
                    Spacing = 8,
                    Children =
                    {
                        swatch,
                        new TextBlock { Text = entry.Name, VerticalAlignment = VerticalAlignment.Center },
                    },
                };
                bookList.Items.Add(new ListViewItem { Content = row, Tag = entry, Padding = new Thickness(4, 2, 4, 2) });
            }
            UpdateBookListHighlight();
        }

        bookCombo.SelectionChanged += (_, _) =>
        {
            if (bookCombo.SelectedItem is ComboBoxItem { Tag: ColorBook book })
            {
                FillBookList(book);
            }
        };
        bookList.SelectionChanged += (_, _) =>
        {
            if (activeBook is null || bookList.SelectedItem is not ListViewItem { Tag: ColorBookEntry entry, IsEnabled: true })
            {
                return;
            }
            SelectBookEntry(activeBook, entry);
        };

        bookPanel.Children.Add(DialogLayout.Labeled("Color book", bookCombo));
        bookPanel.Children.Add(bookList);
        if (books.Count > 0)
        {
            bookCombo.SelectedIndex = 0;
        }
        else
        {
            FillBookList(null);
        }

        var tabs = new Pivot();
        var tabIndex = new PivotItem { Header = "Index Color", Content = indexPanel };
        var tabTrue = new PivotItem { Header = "True Color", Content = truePanel };
        var tabBook = new PivotItem { Header = "Color Books", Content = bookPanel };
        tabs.Items.Add(tabIndex);
        tabs.Items.Add(tabTrue);
        tabs.Items.Add(tabBook);
        tabs.SelectionChanged += (_, args) =>
        {
            if (ReferenceEquals(args.AddedItems.FirstOrDefault(), tabTrue) && selection.Kind is CadColorKind.Index or CadColorKind.Book or CadColorKind.ByLayer or CadColorKind.ByBlock)
            {
                var (r, g, b) = selection.ResolveRgb();
                selection = new CadColorSelection(CadColorKind.True, R: r, G: g, B: b);
                SyncFieldsFromSelection();
            }
        };

        indexBox.ValueChanged += (_, args) =>
        {
            if (double.IsNaN(args.NewValue))
            {
                return;
            }
            var v = (int)Math.Round(args.NewValue);
            if (v is >= 1 and <= 255)
            {
                SelectIndex((byte)v);
            }
        };

        void ApplyRgbBoxes()
        {
            if (rBox.Value is double rd && gBox.Value is double gd && bBox.Value is double bd
                && !double.IsNaN(rd) && !double.IsNaN(gd) && !double.IsNaN(bd))
            {
                selection = new CadColorSelection(
                    CadColorKind.True,
                    R: (byte)Math.Clamp(Math.Round(rd), 0, 255),
                    G: (byte)Math.Clamp(Math.Round(gd), 0, 255),
                    B: (byte)Math.Clamp(Math.Round(bd), 0, 255));
                SyncFieldsFromSelection();
            }
        }
        rBox.ValueChanged += (_, _) => ApplyRgbBoxes();
        gBox.ValueChanged += (_, _) => ApplyRgbBoxes();
        bBox.ValueChanged += (_, _) => ApplyRgbBoxes();

        colorName.LostFocus += (_, _) =>
        {
            if (CadColor.TryParse(colorName.Text, out var parsed))
            {
                if (!allowByLayerByBlock && parsed.Kind is CadColorKind.ByLayer or CadColorKind.ByBlock)
                {
                    return;
                }
                selection = parsed;
                SyncFieldsFromSelection();
            }
        };

        var byLayer = DialogLayout.Action("ByLayer");
        var byBlock = DialogLayout.Action("ByBlock");
        byLayer.Click += (_, _) =>
        {
            selection = new CadColorSelection(CadColorKind.ByLayer);
            SyncFieldsFromSelection();
        };
        byBlock.Click += (_, _) =>
        {
            selection = new CadColorSelection(CadColorKind.ByBlock);
            SyncFieldsFromSelection();
        };
        var logicalRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.Space2, Visibility = allowByLayerByBlock ? Visibility.Visible : Visibility.Collapsed };
        logicalRow.Children.Add(byLayer);
        logicalRow.Children.Add(byBlock);

        var fields = new Grid { ColumnSpacing = Density.Space3, RowSpacing = Density.Space2, Margin = new Thickness(0, Density.SpaceMd, 0, 0) };
        fields.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        fields.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var leftFields = new StackPanel { Spacing = Density.SpaceSm };
        leftFields.Children.Add(DialogLayout.Labeled("Color", colorName));
        var rgbRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Density.Space2 };
        rgbRow.Children.Add(DialogLayout.Labeled("Index color", indexBox));
        rgbRow.Children.Add(DialogLayout.Labeled("R", rBox));
        rgbRow.Children.Add(DialogLayout.Labeled("G", gBox));
        rgbRow.Children.Add(DialogLayout.Labeled("B", bBox));
        leftFields.Children.Add(rgbRow);
        leftFields.Children.Add(logicalRow);
        fields.Children.Add(leftFields);
        var previewStack = new StackPanel { Spacing = Density.Space1, VerticalAlignment = VerticalAlignment.Top };
        previewStack.Children.Add(new TextBlock { Text = "Preview", FontSize = Density.FontBody });
        previewStack.Children.Add(preview);
        Grid.SetColumn(previewStack, 1);
        fields.Children.Add(previewStack);

        var ok = DialogLayout.Action("OK", primary: true);
        var cancel = DialogLayout.Action("Cancel");
        var body = new StackPanel { Spacing = Density.SpaceSm };
        body.Children.Add(tabs);
        body.Children.Add(fields);

        var page = Ui.DialogPage(body, Ui.Footer(null, ok, cancel));
        Density.Apply(page);
        var window = new Window
        {
            Title = "Select Color",
            Content = new Grid { RequestedTheme = theme, Children = { page } },
        };
        StandardWindow.Present(
            window,
            owner,
            WindowTemplate.PropertyDialog,
            "selectcolor",
            new WindowBounds(440, 520, 620, 780),
            modal: true);

        SyncFieldsFromSelection();
        if (initial.Kind == CadColorKind.True)
        {
            tabs.SelectedItem = tabTrue;
            if (ColorBookCatalog.FindByRgb(initial.R, initial.G, initial.B) is { Book: var b, Entry: var e })
            {
                for (var i = 0; i < bookCombo.Items.Count; i++)
                {
                    if (bookCombo.Items[i] is ComboBoxItem { Tag: ColorBook cb } && cb.Name == b.Name)
                    {
                        bookCombo.SelectedIndex = i;
                        SelectBookEntry(b, e);
                        tabs.SelectedItem = tabBook;
                        break;
                    }
                }
            }
        }
        else if (initial.Kind == CadColorKind.Book && !string.IsNullOrEmpty(initial.BookName))
        {
            for (var i = 0; i < bookCombo.Items.Count; i++)
            {
                if (bookCombo.Items[i] is ComboBoxItem { Tag: ColorBook cb } && cb.Name.Equals(initial.BookName, StringComparison.OrdinalIgnoreCase))
                {
                    bookCombo.SelectedIndex = i;
                    tabs.SelectedItem = tabBook;
                    break;
                }
            }
        }

        ok.Click += (_, _) =>
        {
            if (CadColor.TryParse(colorName.Text, out var parsed)
                && (allowByLayerByBlock || parsed.Kind is not CadColorKind.ByLayer and not CadColorKind.ByBlock))
            {
                selection = parsed;
            }
            Finish(selection.ToEngineName());
            window.Close();
        };
        cancel.Click += (_, _) =>
        {
            Finish(null);
            window.Close();
        };
        window.Closed += (_, _) => Finish(null);
        window.Activate();
        return tcs.Task;
    }

    /// <summary>Swatch + name button that opens <see cref="PickAsync"/>.</summary>
    public static Button CreatePickerButton(Window owner, string current, bool allowByLayerByBlock, Action<string> onPicked)
    {
        var swatch = new Border
        {
            Width = Density.SwatchSize,
            Height = Density.SwatchSize,
            Background = CadColor.SwatchBrush(current),
            VerticalAlignment = VerticalAlignment.Center,
        };
        var label = new TextBlock { Text = current, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(6, 0, 0, 0) };
        var button = new Button
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            MinHeight = Density.ControlHeight,
            HorizontalContentAlignment = HorizontalAlignment.Left,
            Content = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Children = { swatch, label },
            },
        };
        Density.Name(button, "Color");
        button.Click += async (_, _) =>
        {
            var next = await PickAsync(owner, current, allowByLayerByBlock);
            if (next != null && !string.Equals(next, current, StringComparison.Ordinal))
            {
                label.Text = next;
                swatch.Background = CadColor.SwatchBrush(next);
                onPicked(next);
            }
        };
        return button;
    }

    private static SolidColorBrush BrushOf(CadColorSelection sel)
    {
        var (r, g, b) = sel.ResolveRgb();
        return new SolidColorBrush(Windows.UI.Color.FromArgb(255, r, g, b));
    }
}

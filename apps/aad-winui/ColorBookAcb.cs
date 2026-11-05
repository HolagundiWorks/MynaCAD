using System.Globalization;
using System.Xml.Linq;

namespace Aad.WinUi;

internal sealed record ColorBookEntry(string Name, byte R, byte G, byte B, string PageLabel);

internal sealed record ColorBook(string Name, string SourcePath, IReadOnlyList<ColorBookEntry> Entries);

/// <summary>Loads colour book files (<c>.acb</c>, unencrypted <c>RGB8</c>).</summary>
internal static class ColorBookCatalog
{
    private static IReadOnlyList<ColorBook>? _cache;

    public static IReadOnlyList<ColorBook> All()
    {
        if (_cache is not null)
        {
            return _cache;
        }
        var books = new List<ColorBook>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var dir in SearchDirectories())
        {
            if (!Directory.Exists(dir))
            {
                continue;
            }
            foreach (var path in Directory.EnumerateFiles(dir, "*.acb", SearchOption.TopDirectoryOnly).OrderBy(p => p, StringComparer.OrdinalIgnoreCase))
            {
                if (!seen.Add(path))
                {
                    continue;
                }
                if (TryLoadFile(path) is { } book)
                {
                    books.Add(book);
                }
            }
        }
        books.Sort((a, b) => string.Compare(a.Name, b.Name, StringComparison.OrdinalIgnoreCase));
        _cache = books;
        return _cache;
    }

    static IEnumerable<string> SearchDirectories()
    {
        yield return Path.Combine(AppContext.BaseDirectory, "ColorBooks");
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        yield return Path.Combine(local, "O-Draft", "ColorBooks");
    }

    internal static bool TryLoadFile(string path, out ColorBook? book)
    {
        book = TryLoadFile(path);
        return book is not null;
    }

    static ColorBook? TryLoadFile(string path)
    {
        try
        {
            var doc = XDocument.Load(path);
            var root = doc.Root;
            if (root is null || !root.Name.LocalName.Equals("colorBook", StringComparison.OrdinalIgnoreCase))
            {
                return null;
            }
            var bookName = root.Elements().FirstOrDefault(e => e.Name.LocalName.Equals("bookName", StringComparison.OrdinalIgnoreCase))?.Value.Trim();
            if (string.IsNullOrEmpty(bookName))
            {
                bookName = Path.GetFileNameWithoutExtension(path);
            }
            var entries = new List<ColorBookEntry>();
            var pageIndex = 0;
            foreach (var page in root.Elements().Where(e => e.Name.LocalName.Equals("colorPage", StringComparison.OrdinalIgnoreCase)))
            {
                pageIndex++;
                var pageRgb = ReadRgb(page.Elements().FirstOrDefault(e => e.Name.LocalName.Equals("pageColor", StringComparison.OrdinalIgnoreCase)));
                var pageLabel = pageRgb is (var pr, var pg, var pb)
                    ? string.Create(CultureInfo.InvariantCulture, $"Page {pageIndex} ({pr},{pg},{pb})")
                    : string.Create(CultureInfo.InvariantCulture, $"Page {pageIndex}");
                foreach (var entry in page.Elements().Where(e => e.Name.LocalName.Equals("colorEntry", StringComparison.OrdinalIgnoreCase)))
                {
                    var name = entry.Elements().FirstOrDefault(e => e.Name.LocalName.Equals("colorName", StringComparison.OrdinalIgnoreCase))?.Value.Trim();
                    if (string.IsNullOrEmpty(name))
                    {
                        continue;
                    }
                    if (ReadRgb(entry) is not (byte r, byte g, byte b))
                    {
                        continue;
                    }
                    entries.Add(new ColorBookEntry(name, r, g, b, pageLabel));
                }
            }
            if (entries.Count == 0)
            {
                return null;
            }
            return new ColorBook(bookName, path, entries);
        }
        catch (Exception)
        {
            return null;
        }
    }

    static (byte R, byte G, byte B)? ReadRgb(XElement? parent)
    {
        if (parent is null)
        {
            return null;
        }
        var rgb = parent.Elements().FirstOrDefault(e =>
            e.Name.LocalName.Equals("RGB8", StringComparison.OrdinalIgnoreCase));
        if (rgb is null)
        {
            return null;
        }
        if (!TryByte(rgb, "red", out var r) || !TryByte(rgb, "green", out var g) || !TryByte(rgb, "blue", out var b))
        {
            return null;
        }
        return (r, g, b);
    }

    static bool TryByte(XElement rgb, string localName, out byte value)
    {
        value = 0;
        var el = rgb.Elements().FirstOrDefault(e => e.Name.LocalName.Equals(localName, StringComparison.OrdinalIgnoreCase));
        if (el is null)
        {
            return false;
        }
        return byte.TryParse(el.Value.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out value);
    }

    /// <summary>Find a book swatch with the same RGB (for reopening the dialog on a true colour).</summary>
    public static (ColorBook Book, ColorBookEntry Entry)? FindByRgb(byte r, byte g, byte b)
    {
        foreach (var book in All())
        {
            foreach (var entry in book.Entries)
            {
                if (entry.R == r && entry.G == g && entry.B == b)
                {
                    return (book, entry);
                }
            }
        }
        return null;
    }
}

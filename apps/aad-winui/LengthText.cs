using System.Globalization;

namespace Aad.WinUi;

/// <summary>
/// A length typed in a dialog field. A bare number is in the field's own unit; a number with a unit
/// (<c>12mm</c>, <c>0.3 m</c>, <c>1'6"</c>, <c>18in</c>, <c>5 yd</c>) is converted into it, so every length
/// can be typed in any unit. The engine reads command lines and forms the same way (<c>units.rs</c>).
/// </summary>
internal static class LengthText
{
    /// <summary>Unit words a length may end in, and metres per unit. Longer words come before the words they end with.</summary>
    private static readonly (string Word, double Metres)[] Words =
    [
        ("inches", 0.0254), ("inch", 0.0254), ("feet", 0.3048), ("foot", 0.3048),
        ("yards", 0.9144), ("yard", 0.9144), ("yd", 0.9144),
        ("miles", 1609.344), ("mile", 1609.344), ("mi", 1609.344),
        ("kilometres", 1000), ("kilometers", 1000), ("kilometre", 1000), ("kilometer", 1000), ("km", 1000),
        ("millimetres", 0.001), ("millimeters", 0.001), ("millimetre", 0.001), ("millimeter", 0.001), ("mm", 0.001),
        ("centimetres", 0.01), ("centimeters", 0.01), ("centimetre", 0.01), ("centimeter", 0.01), ("cm", 0.01),
        ("decimetres", 0.1), ("decimeters", 0.1), ("decimetre", 0.1), ("decimeter", 0.1), ("dm", 0.1),
        ("micrometres", 1e-6), ("micrometers", 1e-6), ("micrometre", 1e-6), ("micrometer", 1e-6), ("µm", 1e-6), ("um", 1e-6),
        ("metres", 1), ("meters", 1), ("metre", 1), ("meter", 1),
        ("ft", 0.3048), ("in", 0.0254), ("m", 1),
    ];

    /// Metres in one <paramref name="unit"/> (<c>mm</c>, <c>cm</c>, <c>m</c>, <c>in</c>, <c>ft</c>, …), or null for something that is not a length unit.
    public static double? MetresPer(string? unit)
    {
        var u = (unit ?? "").Trim().ToLowerInvariant();
        foreach (var (word, metres) in Words)
        {
            if (word == u)
            {
                return metres;
            }
        }

        return null;
    }

    /// <summary>
    /// Read <paramref name="text"/> as a length in <paramref name="unit"/>. When <paramref name="unit"/> is not a length unit
    /// (or is blank) only plain numbers are accepted and nothing is converted.
    /// </summary>
    public static bool TryParse(string? text, string? unit, out double value)
    {
        value = 0;
        var t = (text ?? "").Trim();
        if (t.Length == 0)
        {
            return false;
        }

        if (PlainNumber(t, out value))
        {
            return double.IsFinite(value);
        }

        var target = MetresPer(unit);
        if (target is not { } per)
        {
            return false;
        }

        var neg = t.StartsWith('-');
        var body = neg ? t[1..].Trim() : t;
        var metres = Metres(body);
        if (metres is not { } m || !double.IsFinite(m))
        {
            return false;
        }

        value = (neg ? -m : m) / per;
        return double.IsFinite(value);
    }

    private static bool PlainNumber(string t, out double v) =>
        double.TryParse(t, NumberStyles.Float, CultureInfo.InvariantCulture, out v)
        || double.TryParse(t, NumberStyles.Float, CultureInfo.CurrentCulture, out v);

    /// <summary>The length in metres: <c>number + unit word</c>, or feet and inches such as <c>1'6"</c>.</summary>
    private static double? Metres(string body)
    {
        var lower = body.ToLowerInvariant();
        foreach (var (word, metres) in Words)
        {
            if (!lower.EndsWith(word, StringComparison.Ordinal))
            {
                continue;
            }

            var number = lower[..^word.Length].Trim();
            if (number.Length > 0 && !HasStrayLetters(number) && PlainNumber(number, out var n))
            {
                return n * metres;
            }
        }

        if (body.Contains('\'') || body.Contains('"'))
        {
            var inches = 0.0;
            var rest = body;
            var tick = rest.IndexOf('\'');
            if (tick >= 0)
            {
                if (!PlainNumber(rest[..tick].Trim(), out var feet))
                {
                    return null;
                }

                inches += feet * 12;
                rest = rest[(tick + 1)..].TrimStart('-').Trim();
            }

            rest = rest.TrimEnd('"').Trim();
            if (rest.Length > 0)
            {
                if (!PlainNumber(rest, out var extra))
                {
                    return null;
                }

                inches += extra;
            }
            else if (tick < 0)
            {
                return null;
            }

            return inches * 0.0254;
        }

        return null;
    }

    /// <summary>True when the text has letters that are not a scientific exponent (<c>1.5e3</c> is a number, <c>1.5x</c> is not).</summary>
    private static bool HasStrayLetters(string number)
    {
        var exponents = 0;
        for (var i = 0; i < number.Length; i++)
        {
            var c = number[i];
            if (!char.IsLetter(c))
            {
                continue;
            }

            var before = i > 0 && (char.IsAsciiDigit(number[i - 1]) || number[i - 1] == '.');
            var after = i + 1 < number.Length && (char.IsAsciiDigit(number[i + 1]) || number[i + 1] is '+' or '-');
            if (c is not ('e' or 'E') || !before || !after)
            {
                return true;
            }

            exponents++;
        }

        return exponents > 1;
    }
}

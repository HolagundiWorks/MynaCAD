namespace Aad.WinUi;

/// <summary>256-index (ACI) colour table — same algorithm as <c>aad_color</c>.</summary>
internal static class AciPalette
{
    private static readonly (byte R, byte G, byte B)[] Table = Build();

    public static (byte R, byte G, byte B) Rgb(byte index) => Table[index];

    public static Windows.UI.Color ToUiColor(byte index)
    {
        var (r, g, b) = Rgb(index);
        return Windows.UI.Color.FromArgb(255, r, g, b);
    }

    /// <summary>Grid row order for indices 10..249 (matches Layer Properties / egui palette).</summary>
    public static readonly byte[] ShadeOrder = [0, 2, 4, 6, 8, 1, 3, 5, 7, 9];

    private static (byte R, byte G, byte B)[] Build()
    {
        var p = new (byte R, byte G, byte B)[256];
        (byte r, byte g, byte b)[] baseColors =
        [
            (0, 0, 0), (255, 0, 0), (255, 255, 0), (0, 255, 0), (0, 255, 255),
            (0, 0, 255), (255, 0, 255), (255, 255, 255), (128, 128, 128), (192, 192, 192),
        ];
        for (var i = 0; i < baseColors.Length; i++)
        {
            p[i] = baseColors[i];
        }
        var values = new[] { 255.0, 165.0, 127.0, 76.0, 38.0 };
        for (var i = 10; i < 250; i++)
        {
            var hue = (i - 10) / 10;
            var sub = i % 10;
            var v = values[sub / 2];
            var half = sub % 2 == 1;
            var (fr, fg, fb) = HueFractions(hue);
            byte Ch(double f)
            {
                if (half)
                {
                    f = 0.5 + 0.5 * f;
                }
                return (byte)Math.Clamp(Math.Floor(v * f), 0, 255);
            }
            p[i] = (Ch(fr), Ch(fg), Ch(fb));
        }
        byte[] greys = [51, 91, 132, 173, 214, 255];
        for (var k = 0; k < greys.Length; k++)
        {
            var g = greys[k];
            p[250 + k] = (g, g, g);
        }
        return p;
    }

    private static (double R, double G, double B) HueFractions(int h)
    {
        var seg = h / 4;
        var t = (h % 4) / 4.0;
        return seg switch
        {
            0 => (1.0, t, 0.0),
            1 => (1.0 - t, 1.0, 0.0),
            2 => (0.0, 1.0, t),
            3 => (0.0, 1.0 - t, 1.0),
            4 => (t, 0.0, 1.0),
            _ => (1.0, 0.0, 1.0 - t),
        };
    }
}

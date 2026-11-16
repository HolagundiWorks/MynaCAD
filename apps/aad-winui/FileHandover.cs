namespace Aad.WinUi;

/// <summary>What a file argument means to a MynaCAD that was started without a project (docs/ops-first-workflow.md, 16.3).</summary>
public enum HandoverKind
{
    /// <summary>Not a file MynaCAD hands over: the launch screen stays.</summary>
    None,

    /// <summary>A project file (<c>.osp</c>): MynaOps opens it (adopting it first when needed). MynaCAD never opens it.</summary>
    Project,

    /// <summary>A drawing (<c>.dwg</c>, <c>.dxf</c>): MynaOps asks where to import it (<c>--import-pick</c>).</summary>
    Drawing,
}

public static class FileHandover
{
    private const int MaxPath = 4096;

    /// <summary>Classify a command-line file by its extension; hostile text (empty, very long, NUL) is <see cref="HandoverKind.None"/>.</summary>
    public static HandoverKind Classify(string? file)
    {
        if (string.IsNullOrWhiteSpace(file) || file.Length > MaxPath || file.Contains('\0'))
        {
            return HandoverKind.None;
        }
        var ext = file.Trim().TrimEnd('"');
        ext = ext.Length > 0 ? System.IO.Path.GetExtension(ext) : "";
        if (string.Equals(ext, ".osp", StringComparison.OrdinalIgnoreCase))
        {
            return HandoverKind.Project;
        }
        if (string.Equals(ext, ".dwg", StringComparison.OrdinalIgnoreCase) || string.Equals(ext, ".dxf", StringComparison.OrdinalIgnoreCase))
        {
            return HandoverKind.Drawing;
        }
        return HandoverKind.None;
    }

    /// <summary>The arguments MynaOps is started with for this file: the path alone for a project, <c>--import-pick &lt;path&gt;</c> for a drawing, none otherwise.</summary>
    public static IReadOnlyList<string> ArgumentsFor(string? file) => Classify(file) switch
    {
        HandoverKind.Project => new[] { System.IO.Path.GetFullPath(file!.Trim()) },
        HandoverKind.Drawing => new[] { "--import-pick", System.IO.Path.GetFullPath(file!.Trim()) },
        _ => Array.Empty<string>(),
    };
}

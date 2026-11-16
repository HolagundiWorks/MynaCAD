using Aad.WinUi;
using Xunit;

public class ProjectHashTests
{
    // The vectors are shared with MynaOps (ProjectLeaseTests.Path_hash_matches_the_shared_test_vectors): the same hash names the mutex, the pipe and both lock files.
    [Theory]
    [InlineData(@"C:\Work\House.osp", "a8f10e8331d28c52")]
    [InlineData(@"c:\work\house.osp", "a8f10e8331d28c52")]
    [InlineData(@"C:\Users\Ana\Documents\My House\Plan.osp", "0df4c7568e96ec70")]
    public void Path_hash_matches_the_shared_vectors(string path, string expected)
    {
        Assert.Equal(expected, ProjectInstance.HashOf(path));
        Assert.Equal(@"Local\MynaCAD.Project." + expected, ProjectInstance.MutexName(expected));
        Assert.Equal("MynaCAD." + expected, ProjectInstance.PipeName(expected));
    }
}

public class FileHandoverTests
{
    [Theory]
    [InlineData(@"C:\p\house.osp", HandoverKind.Project)]
    [InlineData(@"C:\p\HOUSE.OSP", HandoverKind.Project)]
    [InlineData(@"C:\p\plan.dxf", HandoverKind.Drawing)]
    [InlineData(@"C:\p\plan.DWG", HandoverKind.Drawing)]
    [InlineData(@"C:\p\notes.txt", HandoverKind.None)]
    [InlineData(@"C:\p\house.osp.bak", HandoverKind.None)]
    [InlineData("", HandoverKind.None)]
    [InlineData(null, HandoverKind.None)]
    public void A_file_is_classified_by_its_extension(string? file, HandoverKind expected) => Assert.Equal(expected, FileHandover.Classify(file));

    [Fact]
    public void Hostile_text_is_never_handed_over()
    {
        Assert.Equal(HandoverKind.None, FileHandover.Classify("a\0.osp"));
        Assert.Equal(HandoverKind.None, FileHandover.Classify(new string('x', 5000) + ".osp"));
        Assert.Empty(FileHandover.ArgumentsFor("a\0.dxf"));
    }

    [Fact]
    public void A_project_goes_as_a_path_and_a_drawing_as_import_pick()
    {
        Assert.Equal(new[] { @"C:\p\house.osp" }, FileHandover.ArgumentsFor(@"C:\p\house.osp"));
        Assert.Equal(new[] { "--import-pick", @"C:\p\plan.dxf" }, FileHandover.ArgumentsFor(@"C:\p\plan.dxf"));
        Assert.Empty(FileHandover.ArgumentsFor(@"C:\p\readme.txt"));
    }
}

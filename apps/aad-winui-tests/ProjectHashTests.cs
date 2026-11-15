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

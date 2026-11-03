using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class StartOptionsTests
{
    [Fact]
    public void NoArgumentsMeansNoHooks()
    {
        var o = StartOptions.Parse([]);
        Assert.Null(o.TestPng);
        Assert.Null(o.ExitAfterMs);
        Assert.Null(o.ReportPath);
        Assert.Null(o.Theme);
    }

    [Fact]
    public void TheTestHooksAreRead()
    {
        var png = Repo.File("tests/fixtures/plasma_alpha_512x384.png");
        var o = StartOptions.Parse(["--test-load-png", png, "--exit-after-ms", "500", "--test-report", "a.json", "--theme", "dark"]);
        Assert.Equal(png, o.TestPng);
        Assert.Equal(500, o.ExitAfterMs);
        Assert.Equal("a.json", o.ReportPath);
        Assert.Equal("dark", o.Theme);
    }

    [Theory]
    [InlineData("--wrong")]
    [InlineData("--test-load-png")]
    [InlineData("--exit-after-ms")]
    [InlineData("--theme")]
    public void AWrongOrUnfinishedArgumentFailsAndNamesIt(string argument)
    {
        var error = Assert.Throws<ArgumentException>(() => StartOptions.Parse([argument]));
        Assert.Contains(argument, error.Message);
    }

    [Fact]
    public void BadValuesFailAndNameTheArgument()
    {
        Assert.Contains("--test-load-png", Assert.Throws<ArgumentException>(() => StartOptions.Parse(["--test-load-png", "no-such-file.png"])).Message);
        Assert.Contains("--exit-after-ms", Assert.Throws<ArgumentException>(() => StartOptions.Parse(["--exit-after-ms", "soon"])).Message);
        Assert.Contains("--exit-after-ms", Assert.Throws<ArgumentException>(() => StartOptions.Parse(["--exit-after-ms", "0"])).Message);
        Assert.Contains("--theme", Assert.Throws<ArgumentException>(() => StartOptions.Parse(["--theme", "blue"])).Message);
    }

    [Fact]
    public void AReportNeedsAnEndSoTheRunCannotHang()
    {
        var error = Assert.Throws<ArgumentException>(() => StartOptions.Parse(["--test-report", "a.json"]));
        Assert.Contains("--exit-after-ms", error.Message);
    }

    [Fact]
    public void TheRealWindowCheckExpectsTheSystemCaptionAnswers()
    {
        Assert.Equal(TestReport.HtMaxButton, TestReport.Expected["maximize"]);
        Assert.Equal(TestReport.HtClose, TestReport.Expected["close"]);
        Assert.Equal(TestReport.HtMinButton, TestReport.Expected["minimize"]);
        Assert.Equal(TestReport.HtClient, TestReport.Expected["top menu"]);
        Assert.Equal(TestReport.HtCaption, TestReport.Expected["title text"]);
    }
}

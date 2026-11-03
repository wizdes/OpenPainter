// The test values in windows/tests/vectors come from the Swift tests. These tests read the
// Swift source again each run, and fail by the vector's name when the two disagree. Then they
// hold the Windows code to the values that it already has code for. Color math comes in W8.
using System.Globalization;
using OpenPainter.Canvas;
using OpenPainter.Catalog;
using OpenPainter.Harness;
using OpenPainter.Shell;
using Xunit;

namespace OpenPainter.Tests;

public sealed class VectorTests
{
    static VectorFile Recorded(string name) =>
        Vectors.FromJson(name, Repo.Read($"{Vectors.Folder}/{name}"));

    static double N(string text) => double.Parse(text, CultureInfo.InvariantCulture);

    [Theory]
    [InlineData("zoom-steps.json")]
    [InlineData("fit-math.json")]
    [InlineData("shortcut-words.json")]
    [InlineData("color-math.json")]
    public void AVectorFileMatchesItsSwiftSource(string name)
    {
        var fromSource = Vectors.FromSources(Repo.Root).Single(f => f.FileName == name);
        var recorded = Recorded(name);
        Assert.NotEmpty(recorded.Vectors);
        Assert.Equal(fromSource.Source, recorded.Source);
        Assert.Equal(fromSource.SourceFunctions, recorded.SourceFunctions);
        var differences = Vectors.Differences(recorded, fromSource);
        Assert.True(differences.Count == 0, string.Join("\n", differences));
        Assert.Equal(Vectors.ToJson(fromSource), Repo.Read($"{Vectors.Folder}/{name}"));
    }

    [Fact]
    public void AChangedSwiftValueFailsByTheVectorsName()
    {
        var swift = Repo.Read("mac/Tests/OpenPainterKitTests/ZoomTests.swift");
        var recorded = Recorded("zoom-steps.json");
        var changed = Vectors.ZoomSteps(swift.Replace("ZoomPresets.nextLarger(0.7), 1)", "ZoomPresets.nextLarger(0.7), 2)"));
        var difference = Assert.Single(Vectors.Differences(recorded, changed));
        Assert.Contains("nextLarger(0.7)", difference);
        Assert.Contains("zoom-steps.json", difference);
    }

    [Fact]
    public void ANewSwiftValueAndARemovedOneBothFailByName()
    {
        var swift = Repo.Read("mac/Tests/OpenPainterKitTests/ZoomTests.swift");
        var recorded = Recorded("zoom-steps.json");
        var added = Vectors.ZoomSteps(swift.Replace("XCTAssertEqual(ZoomPresets.label(1), \"100%\")", "XCTAssertEqual(ZoomPresets.label(1), \"100%\")\n        XCTAssertEqual(ZoomPresets.label(2), \"200%\")"));
        Assert.Contains("label(2) is in", Assert.Single(Vectors.Differences(recorded, added)));
        var removed = Vectors.ZoomSteps(swift.Replace("XCTAssertEqual(ZoomPresets.label(0.33), \"33%\")", ""));
        Assert.Contains("label(0.33) is in the file", Assert.Single(Vectors.Differences(recorded, removed)));
    }

    [Fact]
    public void ASwiftTestInANewFormFailsAndDoesNotLeaveAnEmptyFile()
    {
        var swift = Repo.Read("mac/Tests/OpenPainterKitTests/ZoomTests.swift").Replace("ZoomPresets.label(", "ZoomLabels.text(");
        Assert.Contains("gave no label", Assert.Throws<InvalidDataException>(() => Vectors.ZoomSteps(swift)).Message);
    }

    [Fact]
    public void ZoomStepsPassOnTheWindowsCode()
    {
        foreach (var v in Recorded("zoom-steps.json").Vectors)
        {
            switch (v.Op)
            {
                case "presets":
                    Assert.Equal(v.Out.Split(' ').Select(N), ZoomPresets.Values);
                    break;
                case "nextLarger":
                    Assert.True(ZoomPresets.NextLarger(N(v.In[0])) == N(v.Out), v.Name);
                    break;
                case "nextSmaller":
                    Assert.True(ZoomPresets.NextSmaller(N(v.In[0])) == N(v.Out), v.Name);
                    break;
                case "clamp":
                    Assert.True(ZoomPresets.Clamp(N(v.In[0])) == N(v.Out), v.Name);
                    break;
                case "label":
                    Assert.True(ZoomPresets.Label(N(v.In[0])) == v.Out, v.Name);
                    break;
                default:
                    throw new InvalidDataException($"zoom-steps.json: no check for {v.Op}");
            }
        }
    }

    [Fact]
    public void FitMathPassesOnTheWindowsCode()
    {
        foreach (var v in Recorded("fit-math.json").Vectors)
        {
            var got = ZoomPresets.Fit(N(v.In[0]), N(v.In[1]), N(v.In[2]), N(v.In[3]));
            Assert.True(Math.Abs(got - N(v.Out)) < 1e-12, $"{v.Name}: {got}, expected {v.Out}");
        }
    }

    [Fact]
    public void ShortcutWordsPassOnTheWindowsCode()
    {
        foreach (var v in Recorded("shortcut-words.json").Vectors)
            Assert.True(WindowsKeys.FromMac(v.In[0]) == v.Out, $"{v.Name}: {v.In[0]} became {WindowsKeys.FromMac(v.In[0])}, expected {v.Out}");
    }

    static string Bytes(DocColor c) => $"{c.R} {c.G} {c.B} {c.A}";

    [Fact]
    public void ColorMathPassesOnTheWindowsCode()
    {
        var seen = new HashSet<string>();
        foreach (var v in Recorded("color-math.json").Vectors)
        {
            seen.Add(v.Op);
            switch (v.Op)
            {
                case "hex":
                    Assert.True(DocColor.FromHex(v.In[0]) is { } hex && Bytes(hex) == v.Out, $"{v.Name}: {DocColor.FromHex(v.In[0])}, expected {v.Out}");
                    break;
                case "hexRejected":
                    Assert.True(DocColor.FromHex(v.In[0]) is null, v.Name);
                    break;
                case "hexText":
                    Assert.Equal(v.Out, new DocColor(byte.Parse(v.In[0]), byte.Parse(v.In[1]), byte.Parse(v.In[2]), byte.Parse(v.In[3])).Hex);
                    break;
                case "channel":
                    Assert.True(ColorText.Channel(v.In[0], N(v.In[1]), N(v.In[2])) == N(v.Out), v.Name);
                    break;
                case "channelRejected":
                    Assert.True(ColorText.Channel(v.In[0], N(v.In[1]), N(v.In[2])) is null, v.Name);
                    break;
                case "rgbToHsv":
                    var hsv = Hsv.Of(new DocColor(byte.Parse(v.In[0]), byte.Parse(v.In[1]), byte.Parse(v.In[2])));
                    Assert.True($"{hsv.H} {hsv.S} {hsv.V}" == v.Out, $"{v.Name}: {hsv}, expected {v.Out}");
                    break;
                case "hsvToRgb":
                    Assert.True(Bytes(new Hsv(N(v.In[0]), N(v.In[1]), N(v.In[2])).Color(byte.Parse(v.In[3]))) == v.Out, v.Name);
                    break;
                default:
                    throw new InvalidDataException($"color-math.json: no check for {v.Op}");
            }
        }
        Assert.Equal(7, seen.Count);
    }

    [Fact]
    public void EachVectorFileHoldsTheValuesW8Needs()
    {
        var ops = Recorded("color-math.json").Vectors.GroupBy(v => v.Op).ToDictionary(g => g.Key, g => g.Count());
        foreach (var op in new[] { "hex", "hexRejected", "hexText", "channel", "channelRejected", "rgbToHsv", "hsvToRgb" })
            Assert.True(ops.GetValueOrDefault(op) > 0, $"color-math.json has no {op} values");
        Assert.True(Recorded("shortcut-words.json").Vectors.Length >= 50);
    }
}

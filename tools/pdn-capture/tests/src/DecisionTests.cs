using System.Text.Json;
using OpenPainter.Capture.Driver;
using Xunit;

public sealed class DecisionTests
{
    [Theory]
    [InlineData(false, false, null, false)]
    [InlineData(false, true, null, false)]
    [InlineData(true, false, null, false)]
    [InlineData(true, true, "Catastrophic failure", true)]
    [InlineData(true, false, "Catastrophic failure", false)]
    [InlineData(true, true, "Other failure", false)]
    public void InvocationRequiresObservedOutcome(bool opened, bool closed, string? error, bool expected)
        => Assert.Equal(expected, Decisions.InvocationSucceeded("gaussian blur", opened, closed, error));

    [Fact]
    public void NoDialogOnlyForListedEffects()
    {
        Assert.True(Decisions.InvocationSucceeded("auto-level", false, false, null));
        Assert.False(Decisions.InvocationSucceeded("gaussian blur", false, false, null));
    }

    [Theory]
    [InlineData("auto-level")]
    [InlineData("black and white")]
    [InlineData("invert alpha")]
    [InlineData("invert colors")]
    public void FourCatalogNoDialogEffectsMayFinishWithoutAWindow(string name)
        => Assert.True(Decisions.InvocationSucceeded(name, false, false, null));

    [Fact]
    public void PosterizeMustOpenItsDialog()
        => Assert.False(Decisions.InvocationSucceeded("posterize", false, false, null));

    [Theory]
    [InlineData("\"parameters\":[]")]
    [InlineData("\"parameters\":null")]
    [InlineData("\"parameters\":{\"radius\":7}")]
    [InlineData("\"parameters\":{\"Radius\":\"7\"}")]
    [InlineData("\"Parameters\":{\"Radius\":7}")]
    public void RejectBadParameters(string field)
        => Assert.ThrowsAny<Exception>(() => Decisions.ValidateEffectJson($"{{\"id\":\"paintdotnet.effects.gpu/effect/blurs/gaussian blur\",{field}}}", new[] { "Radius" }));

    [Fact]
    public void RejectUnknownJobFieldAndPathCollision()
    {
        Assert.ThrowsAny<Exception>(() => Decisions.ValidateJobJson("{\"mode\":\"capture\",\"typo\":1}"));
        Assert.ThrowsAny<Exception>(() => Decisions.ValidateDistinctPaths("a", "b", "b", "d", "m"));
    }

    [Fact]
    public void DuplicateJobFieldFails()
        => Assert.ThrowsAny<Exception>(() => Decisions.ValidateJobJson("{\"mode\":\"capture\",\"mode\":\"catalog\"}"));

    [Theory]
    [InlineData("bad/id", false)]
    [InlineData("unknown/effect/blurs/gaussian blur", false)]
    [InlineData("paintdotnet.effects.gpu/effect/blurs/gaussian blur", true)]
    public void CatalogPinsWholeId(string id, bool expected)
        => Assert.Equal(expected, Decisions.CatalogContains("[[effect]]\nid = \"paintdotnet.effects.gpu/effect/blurs/gaussian blur\"", id));

    [Fact]
    public void CatalogAcceptsWindowsLineEndings()
        => Assert.True(Decisions.CatalogContains("[[effect]]\r\nid = \"paintdotnet.effects.gpu/effect/blurs/gaussian blur\"\r\n",
            "paintdotnet.effects.gpu/effect/blurs/gaussian blur"));

    [Fact]
    public void PngHeaderChecksSize()
    {
        byte[] png = [137,80,78,71,13,10,26,10, 0,0,0,13, 73,72,68,82, 0,0,0,2, 0,0,0,3, 8,6,0,0,0, 0,0,0,0];
        Assert.Equal((2,3), Decisions.PngSize(png));
        Assert.ThrowsAny<Exception>(() => Decisions.RequirePngSize(png, 2, 2));
        Assert.ThrowsAny<Exception>(() => Decisions.PngSize(System.Text.Encoding.UTF8.GetBytes("{}")));
    }

    [Fact]
    public void FinalReadbackFindsLinkedChange()
        => Assert.ThrowsAny<Exception>(() => Decisions.RequireSettings(new Dictionary<string,double>{{"Width",2},{"Height",4}}, new Dictionary<string,double>{{"Width",3},{"Height",4}}));

    [Fact]
    public void RejectedWindowMessageFails()
        => Assert.ThrowsAny<Exception>(() => Decisions.RequirePosted(false, "Radius"));

    [Fact]
    public void OtherProcessAndWrongWindowFail()
    {
        Assert.False(Decisions.ProcessIsolated(12, new[] { 12u, 13u }, 12));
        Assert.False(Decisions.ProcessIsolated(12, new[] { 12u }, 13));
        Assert.True(Decisions.ProcessIsolated(12, new[] { 12u }, 12));
    }

    [Fact]
    public void ChangedEntriesAreNamed()
    {
        var before = new Dictionary<string,string>{{"same","1"},{"edited","1"},{"gone","1"}};
        var after = new Dictionary<string,string>{{"same","1"},{"edited","2"},{"new","1"}};
        Assert.Equal(new[] { "edited", "gone", "new" }, Decisions.ChangedEntries(before, after).Order());
    }

    [Fact]
    public void OnlyNewFilesInANewSessionFolderBelongToTheRun()
    {
        var before = new Dictionary<string,string>{{@"SessionData\old\a","1"},{@"Optimization\p","1"}};
        var after = new Dictionary<string,string>{
            {@"SessionData\old\a","1"}, {@"Optimization\p","2"},
            {@"SessionData\old\added","1"}, {@"SessionData\run\b","1"}, {@"SessionData\run\deep\c","1"},
            {@"SessionData\top","1"}, {@"Other\d","1"}};
        Assert.Equal(new[] { @"SessionData\run\b", @"SessionData\run\deep\c" }, Decisions.RunSessionFiles(before, after).Order());
    }

    [Fact]
    public void SpeedCacheComesBackExactly()
    {
        var folder = Directory.CreateTempSubdirectory("pdn-cache-").FullName;
        try
        {
            var cache = Path.Combine(folder, "Optimization");
            Directory.CreateDirectory(cache);
            var kept = Path.Combine(cache, "Startup.1.profile");
            File.WriteAllText(kept, "old");
            var stamp = new DateTime(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc);
            File.SetLastWriteTimeUtc(kept, stamp);

            var saved = Decisions.SaveCache(cache);
            File.WriteAllText(kept, "rewritten by the run");
            File.WriteAllText(Path.Combine(cache, "Startup.2.profile"), "new");
            Decisions.RestoreCache(cache, saved);

            Assert.Equal("old", File.ReadAllText(kept));
            Assert.Equal(stamp, File.GetLastWriteTimeUtc(kept));
            Assert.Single(Directory.GetFiles(cache));
        }
        finally { Directory.Delete(folder, true); }
    }

    [Fact]
    public void RunFilesAreRemovedEvenWhenReadOnly()
    {
        var folder = Directory.CreateTempSubdirectory("pdn-remove-").FullName;
        try
        {
            var session = Path.Combine(folder, "SessionData", "run", "deep");
            Directory.CreateDirectory(session);
            var mine = Path.Combine(session, "a.ch.lz4");
            File.WriteAllText(mine, "x");
            File.SetAttributes(mine, FileAttributes.ReadOnly);
            var other = Path.Combine(folder, "SessionData", "keep.txt");
            File.WriteAllText(other, "x");

            Decisions.RemoveRunFiles(folder, [@"SessionData\run\deep\a.ch.lz4"]);

            Assert.False(Directory.Exists(Path.Combine(folder, "SessionData", "run")));
            Assert.True(File.Exists(other));
        }
        finally { Directory.Delete(folder, true); }
    }

    [Fact]
    public void DecodedHashKeepsLowAlphaPixelsExact()
    {
        var file = Path.GetTempFileName() + ".png";
        try
        {
            using (var bitmap = new System.Drawing.Bitmap(2, 1, System.Drawing.Imaging.PixelFormat.Format32bppArgb))
            {
                bitmap.SetPixel(0, 0, System.Drawing.Color.FromArgb(3, 200, 100, 50));
                bitmap.SetPixel(1, 0, System.Drawing.Color.FromArgb(128, 9, 8, 7));
                bitmap.Save(file, System.Drawing.Imaging.ImageFormat.Png);
            }
            byte[] straightBgra = [50, 100, 200, 3, 7, 8, 9, 128];
            var expected = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(straightBgra)).ToLowerInvariant();
            Assert.Equal(expected, Decisions.DecodedSurfaceHash(file));
        }
        finally { File.Delete(file); }
    }

    [Fact]
    public void AnEmptyNewSessionFolderBelongsToTheRun()
    {
        var before = new Dictionary<string,string>{{@"SessionData\old","dir"}};
        var after = new Dictionary<string,string>{{@"SessionData\old","dir"},{@"SessionData\run","dir"},{@"SessionData\run\deep","dir"},{@"Other","dir"}};
        Assert.Equal(new[] { @"SessionData\run", @"SessionData\run\deep" }, Decisions.RunSessionFiles(before, after).Order());
    }

    [Fact]
    public void AnEmptyRunFolderIsRemoved()
    {
        var folder = Directory.CreateTempSubdirectory("pdn-empty-").FullName;
        try
        {
            var run = Path.Combine(folder, "SessionData", "run");
            Directory.CreateDirectory(Path.Combine(run, "deep"));
            Decisions.RemoveRunFiles(folder, [@"SessionData\run", @"SessionData\run\deep"]);
            Assert.False(Directory.Exists(run));
            Assert.True(Directory.Exists(Path.Combine(folder, "SessionData")));
        }
        finally { Directory.Delete(folder, true); }
    }

    [Fact]
    public void ACaseChangeInASessionFolderNameIsNotANewFolder()
    {
        var before = new Dictionary<string,string>{{@"SessionData\Old\a","1"}};
        var after = new Dictionary<string,string>{{@"SessionData\Old\a","1"},{@"SESSIONDATA\old\b","1"}};
        Assert.Empty(Decisions.RunSessionFiles(before, after));
    }

    [Fact]
    public void DeadlineIsShared()
        => Assert.Throws<TimeoutException>(() => Decisions.RequireBeforeDeadline(TimeSpan.FromSeconds(4), TimeSpan.FromSeconds(3)));

    [Fact]
    public void SlowGpuOutputReaderTimesOut()
        => Assert.False(Decisions.ReadersFinishedWithin(new TaskCompletionSource().Task, TimeSpan.Zero));

    [Fact]
    public void ExistingFailureResultIsPreserved()
        => Assert.False(Decisions.MayWriteFailure(true));

    [Fact]
    public void CaptureResultMustBeOneLayerOfInputSize()
    {
        Decisions.RequireCaptureResult(2, 3, 1, 2, 3);
        Assert.ThrowsAny<Exception>(() => Decisions.RequireCaptureResult(2, 3, 2, 2, 3));
        Assert.ThrowsAny<Exception>(() => Decisions.RequireCaptureResult(3, 3, 1, 2, 3));
    }

    [Fact]
    public void ChangedInputHashFails()
        => Assert.ThrowsAny<Exception>(() => Decisions.RequireUnchangedHash("old", "new"));

    [Fact]
    public void DuplicateMenuItemFails()
        => Assert.ThrowsAny<Exception>(() => Decisions.RequireSingleMatch(2, "Blur"));

    private const string GoodJob = """
        {"mode":"capture","inputPath":"C:\\a\\in.png","outputPath":"C:\\a\\out.png","resultPath":"C:\\a\\r.json","donePath":"C:\\a\\d.txt","effects":[{"id":"x/y/z/w"}]}
        """;

    [Fact]
    public void JobFileFieldsAreRead()
    {
        var job = Decisions.ParseJob(GoodJob);
        Assert.Equal("capture", job.Mode);
        Assert.Equal(@"C:\a\in.png", job.InputPath);
        Assert.Equal(@"C:\a\out.png", job.OutputPath);
        Assert.Equal(@"C:\a\r.json", job.ResultPath);
        Assert.Equal(@"C:\a\d.txt", job.DonePath);
        Assert.Single(job.Effects!);
    }

    [Theory]
    [InlineData("mode")]
    [InlineData("inputPath")]
    [InlineData("outputPath")]
    [InlineData("resultPath")]
    [InlineData("donePath")]
    public void MissingJobFieldFailsByName(string field)
    {
        var node = System.Text.Json.Nodes.JsonNode.Parse(GoodJob)!.AsObject();
        node.Remove(field);
        var error = Assert.Throws<InvalidDataException>(() => Decisions.ParseJob(node.ToJsonString()));
        Assert.Contains(field, error.Message);
    }

    [Fact]
    public void StepJobWrittenForAChainReadsBack()
    {
        var job = Decisions.ParseJob(GoodJob) with { InputPath = @"C:\a\step.png" };
        Assert.Equal(@"C:\a\step.png", Decisions.ParseJob(Decisions.WriteJob(job)).InputPath);
    }

    [Fact]
    public void WrongPinHashOrVersionFails()
    {
        Assert.ThrowsAny<Exception>(() => Decisions.RequirePin("5.1", "bad", "5.1", "good"));
        Assert.ThrowsAny<Exception>(() => Decisions.RequirePin("5.2", "good", "5.1", "good"));
    }
}

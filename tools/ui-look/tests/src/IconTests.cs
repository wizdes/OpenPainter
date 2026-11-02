// Checks the icon records against the Mac source and the files on disk, and proves with
// fixtures that the checks fail on replaced art, a fake source name and unknown symbols.
using System.Security.Cryptography;
using System.Text.RegularExpressions;
using System.Xml.Linq;
using UiLook;
using Xunit;

namespace UiLookTests;

public static class IconChecks
{
    public static readonly string[] AllowedSources = ["fluentui-svg-icons@1.1.345", "own-work"];
    public static readonly string[] AllowedLicenses = ["MIT"];

    public static string Sha256(string path) => Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(path)));

    public static Dictionary<string, string> ReadUsedList(string path) =>
        File.ReadAllLines(path).Where(l => l.Length > 0 && !l.StartsWith('#')).Select(l => l.Split(' ')).ToDictionary(p => p[0], p => p[1]);

    /// Every problem with the rows. `root` is where the rows' file paths start.
    public static List<string> Problems(IEnumerable<Dictionary<string, string>> rows, string root, IDictionary<string, string> usedFluent)
    {
        var problems = new List<string>();
        foreach (var r in rows)
        {
            var name = r.GetValueOrDefault("use", "(no use)");
            var absent = new[] { "use", "mac_symbol", "used_in", "meaning", "file", "source", "source_name", "license", "sha256" }
                .Where(key => !r.TryGetValue(key, out var value) || value.Length == 0).ToList();
            if (absent.Count > 0)
            {
                problems.Add($"{name}: no {string.Join(", ", absent)}");
                continue;
            }
            var path = Path.Combine(root, r["file"]);
            if (!r["file"].StartsWith("windows/assets/icons/", StringComparison.Ordinal)) problems.Add($"{name}: file is outside windows/assets/icons");
            if (!AllowedSources.Contains(r["source"])) problems.Add($"{name}: source {r["source"]} is not allowed");
            if (!AllowedLicenses.Contains(r["license"])) problems.Add($"{name}: license {r["license"]} is not allowed");
            if (r["meaning"].Contains(" and liquid", StringComparison.Ordinal) || r["meaning"].Contains(',')) problems.Add($"{name}: meaning '{r["meaning"]}' is more than one meaning");
            if (r["source"] == "own-work")
            {
                if (!r.ContainsKey("note")) problems.Add($"{name}: an own-work row must say why in note");
            }
            else
            {
                if (!Regex.IsMatch(r["source_name"], "^[a-z0-9_]+_20_regular$")) problems.Add($"{name}: {r["source_name"]} is not a 20 px regular icon");
                if (!usedFluent.TryGetValue(r["source_name"], out var listed)) problems.Add($"{name}: source_name {r["source_name"]} is not in fluent-used.txt");
                else if (listed != r["sha256"]) problems.Add($"{name}: the row's hash is not the package file's hash for {r["source_name"]}");
            }
            if (!File.Exists(path)) { problems.Add($"{name}: {r["file"]} does not exist"); continue; }
            if (Sha256(path) != r["sha256"]) problems.Add($"{name}: {r["file"]} does not match its sha256, so the art was replaced");
            try
            {
                var svg = XDocument.Load(path).Root;
                if (svg?.Name.LocalName != "svg") problems.Add($"{name}: {r["file"]} is not an SVG");
                if (svg?.Attribute("viewBox")?.Value != "0 0 20 20") problems.Add($"{name}: {r["file"]} is not 20 by 20");
            }
            catch (System.Xml.XmlException e) { problems.Add($"{name}: {r["file"]}: {e.Message}"); }
        }
        return problems;
    }

    /// Scene icons that do not match the record for their use.
    public static List<string> SceneMismatches(IEnumerable<(string Use, string Icon)> scene, IEnumerable<Dictionary<string, string>> rows)
    {
        var byUse = rows.ToDictionary(r => r["use"]);
        var problems = new List<string>();
        foreach (var (use, icon) in scene)
        {
            if (!byUse.TryGetValue(use, out var row)) problems.Add($"{use}: no row in icons.toml");
            else if (row["file"] != $"windows/assets/icons/{icon}.svg") problems.Add($"{use}: the scene draws {icon}, the row says {row["file"]}");
        }
        return problems;
    }

    public static IEnumerable<(string Use, string Icon)> SceneIcons() =>
        Scenes.Tools.Select(t => ("tool:" + t.Case, t.Icon))
            .Concat(Scenes.History.Select(h => (h.Use, h.Icon)))
            .Concat(Scenes.LayerActions)
            .Concat(Scenes.BarCommands);
}

public sealed class IconTests
{
    static readonly string Repo = FindRepo();
    static string TablePath => Path.Combine(Repo, "docs", "windows", "icons.toml");
    static string IconFolder => Path.Combine(Repo, "windows", "assets", "icons");
    static string UsedListPath => Path.Combine(IconFolder, "fluent-used.txt");
    static List<Dictionary<string, string>> Rows() => IconTable.Read(TablePath);

    static string FindRepo()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d is not null; d = d.Parent)
            if (File.Exists(Path.Combine(d.FullName, "docs", "windows", "icons.toml"))) return d.FullName;
        throw new DirectoryNotFoundException("docs/windows/icons.toml not found above " + AppContext.BaseDirectory);
    }

    [Fact]
    public void EveryMacUseHasARow()
    {
        var uses = MacSymbols.AllUses(Repo);
        Assert.True(uses.Count > 200, $"The scan found only {uses.Count} uses, so it is broken.");
        var missing = MacSymbols.MissingRows(uses, Rows());
        Assert.True(missing.Count == 0, "No matching row in icons.toml for: " + string.Join(", ", missing));
    }

    [Fact]
    public void EveryRowIsAMacUseOrAWindowsUseAndAppearsOnce()
    {
        var uses = MacSymbols.AllUses(Repo).Keys.Select(u => u.Replace('"', '\'')).ToHashSet();
        var rows = Rows();
        var stale = rows.Where(r => !r["use"].StartsWith("windows:", StringComparison.Ordinal) && !uses.Contains(r["use"])).Select(r => r["use"]).ToList();
        Assert.True(stale.Count == 0, "Rows for uses the Mac source does not have: " + string.Join(", ", stale));
        var twice = rows.GroupBy(r => r["use"]).Where(g => g.Count() > 1).Select(g => g.Key).ToList();
        Assert.True(twice.Count == 0, "More than one row for: " + string.Join(", ", twice));
        Assert.All(rows.Where(r => r["use"].StartsWith("windows:", StringComparison.Ordinal)), r =>
        {
            Assert.Equal("none", r["mac_symbol"]);
            Assert.True(r.ContainsKey("note"), r["use"] + " must say why in note");
        });
    }

    [Fact]
    public void EveryDottedNameIsASymbolOrAnExactNonSymbol()
    {
        var symbols = MacSymbols.AllUses(Repo).Values.ToHashSet();
        var non = MacSymbols.ReadNonSymbols(Repo);
        var all = MacSymbols.SwiftFiles(Repo).SelectMany(f => MacSymbols.DottedNames(File.ReadAllText(f))).ToHashSet();
        var left = all.Where(n => !symbols.Contains(n) && !non.Contains(n)).Order(StringComparer.Ordinal).ToList();
        Assert.True(left.Count == 0, "Dotted names that are neither a found symbol nor listed in non-symbols.txt: " + string.Join(", ", left));
        var stale = non.Where(n => !all.Contains(n)).Order(StringComparer.Ordinal).ToList();
        Assert.True(stale.Count == 0, "Names in non-symbols.txt that the Mac source no longer has: " + string.Join(", ", stale));
        var both = non.Where(symbols.Contains).ToList();
        Assert.True(both.Count == 0, "Names in non-symbols.txt that are symbols: " + string.Join(", ", both));
    }

    // doc.dirty is an engine query and tool.text.begin an engine command. They look like
    // SF Symbol names because they are dotted and lowercase, but no icon draws them. They
    // must not be picked up as symbols, and they must not get a row.
    [Theory]
    [InlineData("doc.dirty")]
    [InlineData("tool.text.begin")]
    public void EngineNamesAreNotSymbols(string name)
    {
        Assert.Contains(name, MacSymbols.ReadNonSymbols(Repo));
        Assert.DoesNotContain(name, MacSymbols.AllUses(Repo).Values);
        Assert.DoesNotContain(Rows(), r => r["mac_symbol"] == name);
    }

    [Fact]
    public void TheScannerReadsEachWayTheMacNamesASymbol()
    {
        static string One(string file, string text, string use) => MacSymbols.Extract(file, text)[use];
        Assert.Equal("eye.slash", One("X.swift", "eye = ShellButton(symbol: layer.visible ? \"eye\" : \"eye.slash\", size: 24)", "site:X.swift:eye.slash"));
        Assert.Equal("printer", One("X.swift", "BarCommand(key: \"print\", symbol: \"printer\", title: \"Print\")", "bar:print"));
        Assert.Equal("=tool:magicWand", One("X.swift", "BarCommand(key: \"wand\", symbol: PaintTool.magicWand.symbol, title: x)", "bar:wand"));
        Assert.Equal("gearshape", One("X.swift", "NSImage(systemSymbolName: \"gearshape\", accessibilityDescription: nil)", "site:X.swift:gearshape"));
        Assert.Equal("gearshape", One("X.swift", "Image(systemName: \"gearshape\")", "site:X.swift:gearshape"));
        Assert.Equal("folder", One("MenuIcons.swift", "    \"File > Open…\": \"folder\",\n", "menu:File > Open…"));
        Assert.Equal("drop", One("MenuIcons.swift", "    \"effect.blur.gaussian\": \"drop\",\n", "effect:effect.blur.gaussian"));
        Assert.Equal("bold", One("TextOptionsControls.swift", "bold = toggle(\"bold\", \"Bold\", \"\")", "site:TextOptionsControls.swift:Bold"));
        Assert.Equal("trash", One("LayersPanel.swift", "    (\"layer.delete\", \"trash\", \"Delete Layer\"),", "site:LayersPanel.swift:Delete Layer"));
        var block = MacSymbols.Extract("X.swift", "static func symbol(_ command: String) -> String {\n case \"document.open\", \"b\": return \"photo\"\n}\n");
        Assert.Equal("photo", block["history:document.open"]);
        Assert.Equal("photo", block["history:b"]);
        Assert.Equal("magnifyingglass", One("ToolState.swift", "var symbol: String {\n switch self {\n case .zoom: \"magnifyingglass\"\n }\n}\n", "tool:zoom"));
    }

    [Theory]
    [InlineData("hare")]
    [InlineData("photo.fill")]
    public void AnUnknownSymbolFailsByName(string symbol)
    {
        var uses = MacSymbols.Extract("Fixture.swift", $"let a = Image(systemName: \"{symbol}\")");
        var missing = MacSymbols.MissingRows(uses, Rows());
        Assert.Contains(missing, m => m.Contains(symbol, StringComparison.Ordinal));
    }

    [Fact]
    public void AnUnknownBareDottedNameFailsByName()
    {
        var symbols = MacSymbols.AllUses(Repo).Values.ToHashSet();
        var left = MacSymbols.Unclassified("let a = \"photo.fill\"; let b = \"doc.dirty\"", symbols, MacSymbols.ReadNonSymbols(Repo));
        Assert.Equal(["photo.fill"], left);
    }

    [Fact]
    public void TheRecordMatchesTheFiles()
    {
        var problems = IconChecks.Problems(Rows(), Repo, IconChecks.ReadUsedList(UsedListPath));
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [Fact]
    public void ReplacedArtFails()
    {
        var root = Path.Combine(Path.GetTempPath(), "ui-look-fixture-" + Guid.NewGuid().ToString("N"));
        try
        {
            var row = Rows().First(r => r["use"] == "tool:lassoSelect");
            var path = Path.Combine(root, row["file"]);
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, File.ReadAllText(Path.Combine(Repo, row["file"])).Replace("<path", "<path id=\"swapped\""));
            var problems = IconChecks.Problems([row], root, IconChecks.ReadUsedList(UsedListPath));
            Assert.Contains(problems, p => p.Contains("does not match its sha256", StringComparison.Ordinal));
        }
        finally
        {
            if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void FluentArtSwappedInBothFileAndRowStillFails()
    {
        // Swapping the art and updating the row's hash must still fail against the checked-in list.
        var row = new Dictionary<string, string>(Rows().First(r => r["use"] == "tool:pan"));
        row["sha256"] = new string('0', 64);
        var problems = IconChecks.Problems([row], Repo, IconChecks.ReadUsedList(UsedListPath));
        Assert.Contains(problems, p => p.Contains("not the package file's hash", StringComparison.Ordinal));
    }

    [Fact]
    public void AFakeSourceNameFails()
    {
        var row = new Dictionary<string, string>(Rows().First(r => r["use"] == "tool:pan"));
        row["source_name"] = "made_up_icon_20_regular";
        var problems = IconChecks.Problems([row], Repo, IconChecks.ReadUsedList(UsedListPath));
        Assert.Contains(problems, p => p.Contains("made_up_icon_20_regular is not in fluent-used.txt", StringComparison.Ordinal));
    }

    [Fact]
    public void OneMeaningHasOneFileAndOneFileHasOneMeaning()
    {
        var rows = Rows();
        var problems = new List<string>();
        foreach (var g in rows.GroupBy(r => r["meaning"]))
        {
            var files = g.Select(r => r["file"]).Distinct().ToList();
            // A second file for one meaning needs a written reason in the row.
            if (files.Count > 1 && !g.All(r => r.ContainsKey("two_files_reason")))
                problems.Add($"meaning '{g.Key}' has {string.Join(", ", files)}");
        }
        foreach (var g in rows.Where(r => r["source"] != "own-work").GroupBy(r => r["source_name"]))
        {
            var files = g.Select(r => r["file"]).Distinct().ToList();
            if (files.Count > 1) problems.Add($"{g.Key} is copied to {string.Join(", ", files)}");
        }
        foreach (var g in rows.GroupBy(r => r["file"]))
        {
            var meanings = g.Select(r => r["meaning"]).Distinct().ToList();
            if (meanings.Count > 1) problems.Add($"{g.Key} has the meanings {string.Join(", ", meanings)}");
        }
        foreach (var g in Directory.GetFiles(IconFolder, "*.svg").GroupBy(IconChecks.Sha256).Where(g => g.Count() > 1))
            problems.Add("Identical art: " + string.Join(", ", g.Select(Path.GetFileName)));
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [Theory]
    [InlineData("effect:effect.blur.gaussian", "tool:paintBucket")]
    [InlineData("effect:effect.stylize.outline", "tool:ellipseSelect")]
    [InlineData("menu:Edit > Invert Selection", "tool:cloneStamp")]
    [InlineData("site:ColorsPanel.swift:Reset colors", "effect:effect.adjust.black_and_white")]
    [InlineData("effect:effect.artistic.oil_painting", "tool:recolor")]
    [InlineData("effect:effect.render.julia_fractal", "history:effect.*")]
    [InlineData("tool:lassoSelect", "tool:ellipseSelect")]
    [InlineData("tool:paintbrush", "tool:recolor")]
    [InlineData("site:LayersPanel.swift:Duplicate Layer", "site:LayersPanel.swift:Add New Layer")]
    [InlineData("menu:Edit > Paste Selection > Add (union)", "menu:Edit > Paste Selection > Invert ('xor')")]
    [InlineData("effect:effect.blur.surface", "effect:effect.blur.sketch")]
    [InlineData("effect:effect.distort.bulge", "effect:effect.blur.radial")]
    public void TheseTwoUsesHaveDifferentFiles(string a, string b)
    {
        var rows = Rows().ToDictionary(r => r["use"]);
        Assert.NotEqual(rows[a]["file"], rows[b]["file"]);
    }

    [Fact]
    public void NoIconFileIsUnused()
    {
        var used = Rows().Select(r => Path.GetFileName(r["file"])).ToHashSet();
        var unused = Directory.GetFiles(IconFolder, "*.svg").Select(Path.GetFileName).Where(f => !used.Contains(f!)).ToList();
        Assert.True(unused.Count == 0, "Files with no row: " + string.Join(", ", unused));
    }

    [Fact]
    public void ScenesDrawTheIconsTheRecordNames()
    {
        var problems = IconChecks.SceneMismatches(IconChecks.SceneIcons(), Rows());
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [Fact]
    public void SwappingLassoAndPencilInTheToolsPanelFails()
    {
        var swapped = Scenes.Tools.Select(t => t.Case switch
        {
            "lassoSelect" => ("tool:" + t.Case, "tool-pencil"),
            "pencil" => ("tool:" + t.Case, "tool-lasso"),
            _ => ("tool:" + t.Case, t.Icon),
        });
        var problems = IconChecks.SceneMismatches(swapped, Rows());
        Assert.Equal(2, problems.Count);
    }

    [Fact]
    public void TheToolsPanelKeepsTheMacsOrder()
    {
        var uses = MacSymbols.Extract("ToolState.swift", File.ReadAllText(Path.Combine(Repo, "mac", "OpenPainterKit", "Sources", "Shell", "ToolState.swift")));
        var macOrder = uses.Keys.Where(k => k.StartsWith("tool:", StringComparison.Ordinal)).Select(k => k[5..]).ToList();
        Assert.Equal(macOrder, Scenes.Tools.Select(t => t.Case).ToList());
    }

    [Fact]
    public void LicenseFileSaysWhatItMust()
    {
        var text = File.ReadAllText(Path.Combine(IconFolder, "LICENSES.md"));
        Assert.Contains("@fluentui/svg-icons", text);
        Assert.Contains("1.1.345", text);
        Assert.Contains("06f559c29c761ca979894d07a14b3a44c94a24d1", text);
        Assert.Contains("unchanged", text);
        Assert.Contains("W19", text);
        Assert.Contains("Copyright (c) 2020 Microsoft Corporation", text);
        Assert.Contains("Permission is hereby granted, free of charge", text);
        Assert.Contains("THE SOFTWARE IS PROVIDED \"AS IS\"", text);
    }
}

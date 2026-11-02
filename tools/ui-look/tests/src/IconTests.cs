// Checks the icon records against the Mac source and the files on disk.
using System.Text.RegularExpressions;
using System.Xml.Linq;
using Xunit;

namespace UiLookTests;

public sealed class IconTests
{
    static readonly string Repo = FindRepo();
    static string TablePath => Path.Combine(Repo, "docs", "windows", "icons.toml");
    static readonly string[] AllowedSources = ["fluentui-svg-icons@1.1.345", "own-work"];
    static readonly string[] AllowedLicenses = ["MIT"];

    static string FindRepo()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d is not null; d = d.Parent)
            if (File.Exists(Path.Combine(d.FullName, "docs", "windows", "icons.toml"))) return d.FullName;
        throw new DirectoryNotFoundException("docs/windows/icons.toml not found above " + AppContext.BaseDirectory);
    }

    [Fact]
    public void EveryMacSymbolHasARow()
    {
        var symbols = MacSymbols.All(Repo);
        Assert.True(symbols.Count > 100, $"The scan found only {symbols.Count} symbols, so it is broken.");
        var rows = IconTable.Read(TablePath).Select(r => r["mac_symbol"]).ToHashSet();
        var missing = symbols.Where(s => !rows.Contains(s)).Order(StringComparer.Ordinal).ToList();
        Assert.True(missing.Count == 0, "No row in icons.toml for: " + string.Join(", ", missing));
    }

    [Fact]
    public void EveryRowIsAMacSymbolAndAppearsOnce()
    {
        var symbols = MacSymbols.All(Repo);
        var rows = IconTable.Read(TablePath).Select(r => r["mac_symbol"]).ToList();
        var stale = rows.Where(r => !symbols.Contains(r)).ToList();
        Assert.True(stale.Count == 0, "Rows for names the Mac source does not use as icons: " + string.Join(", ", stale));
        var twice = rows.GroupBy(r => r).Where(g => g.Count() > 1).Select(g => g.Key).ToList();
        Assert.True(twice.Count == 0, "More than one row for: " + string.Join(", ", twice));
    }

    [Fact]
    public void NoDottedNameInTheMacSourceIsLeftUnclassified()
    {
        var left = MacSymbols.Unclassified(Repo, MacSymbols.All(Repo));
        Assert.True(left.Count == 0, "Dotted names that are neither a found symbol nor a known engine or file id: " + string.Join(", ", left));
    }

    // doc.dirty is an engine query and tool.text.begin an engine command. They look like
    // SF Symbol names because they are dotted and lowercase, but no icon draws them. They
    // must not be picked up as symbols, and they must not get a row.
    [Theory]
    [InlineData("doc.dirty")]
    [InlineData("tool.text.begin")]
    public void EngineNamesAreNotSymbols(string name)
    {
        Assert.True(MacSymbols.IsKnownNonSymbol(name));
        Assert.DoesNotContain(name, MacSymbols.All(Repo));
        Assert.DoesNotContain(IconTable.Read(TablePath), r => r["mac_symbol"] == name);
    }

    [Fact]
    public void TheScannerReadsEachWayTheMacNamesASymbol()
    {
        Assert.Contains("eye.slash", MacSymbols.Extract("X.swift", "eye = ShellButton(symbol: layer.visible ? \"eye\" : \"eye.slash\", label: x)"));
        Assert.Contains("printer", MacSymbols.Extract("X.swift", "BarCommand(key: \"print\", symbol: \"printer\", title: \"Print\")"));
        Assert.Contains("gearshape", MacSymbols.Extract("X.swift", "NSImage(systemSymbolName: \"gearshape\", accessibilityDescription: nil)"));
        Assert.Contains("folder", MacSymbols.Extract("MenuIcons.swift", "    \"File > Open…\": \"folder\",\n"));
        Assert.Contains("drop", MacSymbols.Extract("MenuIcons.swift", "    \"effect.blur.gaussian\": \"drop\",\n"));
        Assert.Contains("bold", MacSymbols.Extract("TextOptionsControls.swift", "bold = toggle(\"bold\", \"Bold\", \"\")"));
        Assert.Contains("trash", MacSymbols.Extract("LayersPanel.swift", "    (\"layer.delete\", \"trash\", \"Delete Layer\"),"));
        var block = MacSymbols.Extract("X.swift", "static func symbol(_ command: String) -> String {\n case \"document.open\": return \"photo\"\n}\n");
        Assert.Contains("photo", block);
        Assert.DoesNotContain("document.open", block);
        var property = MacSymbols.Extract("X.swift", "var symbol: String {\n switch self {\n case .zoom: \"magnifyingglass\"\n }\n}\n");
        Assert.Contains("magnifyingglass", property);
    }

    [Fact]
    public void EveryRowHasAFileThatParsesAndAnAllowedSource()
    {
        var problems = new List<string>();
        foreach (var r in IconTable.Read(TablePath))
        {
            var name = r["mac_symbol"];
            var absent = new[] { "used_in", "meaning", "file", "source", "source_name", "license" }
                .Where(key => !r.TryGetValue(key, out var value) || value.Length == 0).ToList();
            if (absent.Count > 0)
            {
                problems.Add($"{name}: no {string.Join(", ", absent)}");
                continue;
            }
            var path = Path.Combine(Repo, r["file"]);
            if (!r["file"].StartsWith("windows/assets/icons/", StringComparison.Ordinal)) problems.Add($"{name}: file is outside windows/assets/icons");
            if (!File.Exists(path)) { problems.Add($"{name}: {r["file"]} does not exist"); continue; }
            try
            {
                var root = XDocument.Load(path).Root;
                if (root?.Name.LocalName != "svg") problems.Add($"{name}: {r["file"]} is not an SVG");
                if (root?.Attribute("viewBox")?.Value != "0 0 20 20") problems.Add($"{name}: {r["file"]} is not 20 by 20");
            }
            catch (System.Xml.XmlException e) { problems.Add($"{name}: {r["file"]}: {e.Message}"); }
            if (!AllowedSources.Contains(r["source"])) problems.Add($"{name}: source {r["source"]} is not allowed");
            if (!AllowedLicenses.Contains(r["license"])) problems.Add($"{name}: license {r["license"]} is not allowed");
            if (r["source"] == "own-work" && !r.ContainsKey("note")) problems.Add($"{name}: an own-work row must say why in note");
            if (r["source"] != "own-work" && !Regex.IsMatch(r["source_name"], "^[a-z0-9_]+_20_regular$"))
                problems.Add($"{name}: {r["source_name"]} is not a 20 px regular icon");
        }
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [Fact]
    public void OneMeaningHasOneFile()
    {
        var rows = IconTable.Read(TablePath);
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
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [Fact]
    public void NoIconFileIsUnused()
    {
        var used = IconTable.Read(TablePath).Select(r => Path.GetFileName(r["file"])).ToHashSet();
        var unused = Directory.GetFiles(Path.Combine(Repo, "windows", "assets", "icons"), "*.svg")
            .Select(Path.GetFileName).Where(f => !used.Contains(f!)).ToList();
        Assert.True(unused.Count == 0, "Files with no row: " + string.Join(", ", unused));
    }

    [Fact]
    public void LicenseFileNamesThePackageAndHoldsTheMitText()
    {
        var text = File.ReadAllText(Path.Combine(Repo, "windows", "assets", "icons", "LICENSES.md"));
        Assert.Contains("@fluentui/svg-icons", text);
        Assert.Contains("1.1.345", text);
        Assert.Contains("Copyright (c) 2020 Microsoft Corporation", text);
        Assert.Contains("Permission is hereby granted, free of charge", text);
        Assert.Contains("THE SOFTWARE IS PROVIDED \"AS IS\"", text);
    }
}

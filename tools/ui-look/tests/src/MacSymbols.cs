// Finds where the Mac app names an SF Symbol, by reading its Swift source. Each find is a
// use (a place in the app) and a symbol name.
//
// What this does catch: every literal name at the code shapes in Extract, including
// systemSymbolName:, systemName: and symbol:, and any other dotted string literal, which
// must be either a found symbol or an exact entry in tests/non-symbols.txt.
//
// What this does not catch: a one-word name such as "hare" passed through a call shape that
// Extract does not list, or a name built at run time. No list of SF Symbols exists here to
// check such a string against.
using System.Text.RegularExpressions;

namespace UiLookTests;

public static partial class MacSymbols
{
    /// Folders that hold the Mac app's code. The Mac tests are left out on purpose: they
    /// name symbols only to check them.
    public static readonly string[] SourceFolders =
    [
        "mac/OpenPainterKit/Sources",
        "mac/OpenPainter",
        "mac/EngineBridge/Sources",
    ];

    public static IEnumerable<string> SwiftFiles(string repo) =>
        SourceFolders.Select(f => Path.Combine(repo, f)).Where(Directory.Exists)
            .SelectMany(f => Directory.EnumerateFiles(f, "*.swift", SearchOption.AllDirectories));

    /// The uses in one source file. A value that starts with = points at another use.
    public static Dictionary<string, string> Extract(string fileName, string text)
    {
        var found = new Dictionary<string, string>();
        if (fileName == "MenuIcons.swift")
        {
            // Dictionary rows: "File > Open…": "folder", and "effect.blur.gaussian": "drop".
            foreach (Match m in DictionaryRow().Matches(text))
            {
                var key = m.Groups["key"].Value.Replace("\\\"", "'");
                found[(key.StartsWith("effect.", StringComparison.Ordinal) ? "effect:" : "menu:") + key] = m.Groups["value"].Value;
            }
            return found;
        }
        foreach (var line in text.Split('\n'))
        {
            var bar = BarLiteral().Match(line);
            if (bar.Success) { found["bar:" + bar.Groups[1].Value] = bar.Groups[2].Value; continue; }
            var barTool = BarTool().Match(line);
            if (barTool.Success) { found["bar:" + barTool.Groups[1].Value] = "=tool:" + barTool.Groups[2].Value; continue; }

            // NSImage(systemSymbolName: "x"), Image(systemName: "x"), ShellButton(symbol: "x"),
            // and a choice such as symbol: layer.visible ? "eye" : "eye.slash".
            var label = LabelArgument().Match(line);
            foreach (Match m in LabeledSymbol().Matches(line))
                foreach (Match s in Literal().Matches(m.Groups["rest"].Value))
                    found[$"site:{fileName}:{(label.Success ? label.Groups[1].Value : s.Groups[1].Value)}"] = s.Groups[1].Value;

            if (fileName == "ColorsPanel.swift")
                foreach (Match m in ButtonCall().Matches(line)) found[$"site:{fileName}:{m.Groups[2].Value}"] = m.Groups[1].Value;
            if (fileName == "TextOptionsControls.swift")
            {
                foreach (Match m in ToggleCall().Matches(line)) found[$"site:{fileName}:{m.Groups[2].Value}"] = m.Groups[1].Value;
                foreach (Match m in AlignTuple().Matches(line)) found[$"site:{fileName}:{m.Groups[1].Value}"] = m.Groups[1].Value;
            }
            if (fileName == "LayersPanel.swift")
            {
                var m = LayerTuple().Match(line);
                if (m.Success) found[$"site:{fileName}:{m.Groups[2].Value}"] = m.Groups[1].Value;
            }
        }

        // var symbol: String { switch self { case .x: "name" } }
        var prefix = fileName switch
        {
            "PanelLayout.swift" => "panel:",
            "ToolState.swift" => "tool:",
            "ImageSizeDialogs.swift" => "anchor:",
            _ => $"site:{fileName}:",
        };
        foreach (Match m in SymbolProperty().Matches(text))
            foreach (Match c in SwitchCase().Matches(text[(m.Index + m.Length)..BlockEnd(text, m.Index + m.Length)]))
                foreach (var one in c.Groups[1].Value.Split(','))
                    found[prefix + one.Trim().TrimStart('.')] = c.Groups[2].Value;

        // HistoryStep's func symbol(_:). Its case patterns are engine command ids, each a use.
        var step = SymbolFunction().Match(text);
        if (step.Success)
            foreach (var line in text[(step.Index + step.Length)..BlockEnd(text, step.Index + step.Length)].Split('\n'))
            {
                var c = HistoryCase().Match(line);
                if (c.Success)
                {
                    foreach (Match k in Literal().Matches(c.Groups[1].Value)) found["history:" + k.Groups[1].Value] = c.Groups[2].Value;
                    continue;
                }
                var p = HistoryPrefix().Match(line);
                if (p.Success) { found["history:" + p.Groups[1].Value + "*"] = p.Groups[2].Value; continue; }
                var r = HistoryReturn().Match(line);
                if (r.Success) found["history:default"] = r.Groups[1].Value;
            }
        return found;
    }

    static int BlockEnd(string text, int start)
    {
        var depth = 1;
        var i = start;
        while (depth > 0)
        {
            depth += text[i] switch { '{' => 1, '}' => -1, _ => 0 };
            i++;
        }
        return i;
    }

    public static Dictionary<string, string> AllUses(string repo)
    {
        var all = new Dictionary<string, string>();
        foreach (var file in SwiftFiles(repo))
            foreach (var (use, symbol) in Extract(Path.GetFileName(file), File.ReadAllText(file)))
                all[use] = symbol;
        foreach (var (use, symbol) in all.ToList())
            if (symbol.StartsWith('=')) all[use] = all[symbol[1..]];
        return all;
    }

    /// Uses with no row, or whose row names another symbol. The message names each one.
    public static List<string> MissingRows(IDictionary<string, string> uses, IEnumerable<Dictionary<string, string>> rows)
    {
        var byUse = rows.Where(r => r.ContainsKey("use")).ToDictionary(r => r["use"]);
        var problems = new List<string>();
        foreach (var (use, symbol) in uses.OrderBy(u => u.Key, StringComparer.Ordinal))
        {
            var key = use.Replace('"', '\'');
            if (!byUse.TryGetValue(key, out var row)) problems.Add($"{symbol} at {use}");
            else if (row["mac_symbol"] != symbol) problems.Add($"{use}: the source says {symbol}, the row says {row["mac_symbol"]}");
        }
        return problems;
    }

    public static string NonSymbolsPath(string repo) => Path.Combine(repo, "tools", "ui-look", "tests", "non-symbols.txt");

    public static HashSet<string> ReadNonSymbols(string repo) =>
        File.ReadAllLines(NonSymbolsPath(repo)).Where(l => l.Length > 0 && !l.StartsWith('#')).ToHashSet();

    /// Dotted names in `text` that are neither known symbols nor exact non-symbols.
    public static List<string> Unclassified(string text, ISet<string> symbols, ISet<string> nonSymbols) =>
        DottedNames(text).Where(n => !symbols.Contains(n) && !nonSymbols.Contains(n)).Distinct().Order(StringComparer.Ordinal).ToList();

    public static IEnumerable<string> DottedNames(string text) => DottedLiteral().Matches(text).Select(m => m.Groups[1].Value);

    [GeneratedRegex("""^\s*"(?<key>.+)":\s*"(?<value>[^"\n]+)",\s*$""", RegexOptions.Multiline)] private static partial Regex DictionaryRow();
    [GeneratedRegex("BarCommand\\(key: \"(\\w+)\", symbol: \"([^\"]+)\"")] private static partial Regex BarLiteral();
    [GeneratedRegex("BarCommand\\(key: \"(\\w+)\", symbol: PaintTool\\.(\\w+)\\.symbol")] private static partial Regex BarTool();
    [GeneratedRegex("\\blabel: \"([^\"]+)\"")] private static partial Regex LabelArgument();
    [GeneratedRegex("""\b(?:systemSymbolName|systemName|symbol):\s*(?<rest>[^,]*)""")] private static partial Regex LabeledSymbol();
    [GeneratedRegex("\"([^\"]+)\"")] private static partial Regex Literal();
    [GeneratedRegex("\\bbutton\\(\"([^\"]+)\", \"([^\"]+)\"")] private static partial Regex ButtonCall();
    [GeneratedRegex("\\btoggle\\(\"([^\"]+)\", \"([^\"]+)\"")] private static partial Regex ToggleCall();
    [GeneratedRegex("\"(text\\.align[a-z]+)\"")] private static partial Regex AlignTuple();
    [GeneratedRegex("""^\s*\("[^"]+", "([^"]+)", "([^"]+)"\),""")] private static partial Regex LayerTuple();
    [GeneratedRegex("""var symbol: String\s*\{""")] private static partial Regex SymbolProperty();
    [GeneratedRegex("case \\.([^:\\n]+):\\s*\"([^\"]+)\"")] private static partial Regex SwitchCase();
    [GeneratedRegex("""static func symbol\(_ command: String\) -> String\s*\{""")] private static partial Regex SymbolFunction();
    [GeneratedRegex("case (.+): return \"([^\"]+)\"")] private static partial Regex HistoryCase();
    [GeneratedRegex("hasPrefix\\(\"([^\"]+)\"\\).*return \"([^\"]+)\"")] private static partial Regex HistoryPrefix();
    [GeneratedRegex("^\\s*return \"([^\"]+)\"")] private static partial Regex HistoryReturn();
    [GeneratedRegex("\"([a-zA-Z0-9_]+(?:\\.[a-zA-Z0-9_]+)+)\"")] private static partial Regex DottedLiteral();
}

/// A small reader for docs/windows/icons.toml: [[icon]] rows of key = "value" lines.
public static class IconTable
{
    public static List<Dictionary<string, string>> Read(string path)
    {
        var rows = new List<Dictionary<string, string>>();
        var number = 0;
        foreach (var raw in File.ReadAllLines(path))
        {
            number++;
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#')) continue;
            if (line == "[[icon]]") { rows.Add(new()); continue; }
            var at = line.IndexOf(" = \"", StringComparison.Ordinal);
            if (at < 0 || !line.EndsWith('"') || rows.Count == 0) throw new InvalidDataException($"{path}:{number}: cannot read: {line}");
            rows[^1][line[..at]] = line[(at + 4)..^1];
        }
        return rows;
    }
}

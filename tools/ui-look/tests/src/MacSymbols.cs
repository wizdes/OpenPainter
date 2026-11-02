// Finds every SF Symbol name the Mac app shows, by reading its Swift source.
// Symbols reach the screen through a known set of code shapes, listed in Extract below.
// A wider check, Unclassified, catches a dotted name that appears anywhere else, so a new
// way of naming a symbol fails a test by name instead of going unseen.
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

    /// The symbol names in one source file.
    public static HashSet<string> Extract(string fileName, string text)
    {
        var found = new HashSet<string>();
        if (fileName == "MenuIcons.swift")
        {
            // Dictionary rows: "File > Open…": "folder", and "effect.blur.gaussian": "drop".
            foreach (Match m in DictionaryRow().Matches(text)) found.Add(m.Groups["value"].Value);
            return found;
        }
        foreach (var line in text.Split('\n'))
        {
            // NSImage(systemSymbolName: "x"), ShellButton(symbol: "x"), and a choice such
            // as symbol: layer.visible ? "eye" : "eye.slash".
            foreach (Match m in LabeledSymbol().Matches(line))
                foreach (Match s in Literal().Matches(m.Groups["rest"].Value))
                    found.Add(s.Groups[1].Value);
            if (fileName == "ColorsPanel.swift")
                foreach (Match m in ButtonCall().Matches(line)) found.Add(m.Groups[1].Value);
            if (fileName == "TextOptionsControls.swift")
            {
                foreach (Match m in ToggleCall().Matches(line)) found.Add(m.Groups[1].Value);
                foreach (Match m in AlignTuple().Matches(line)) found.Add(m.Groups[1].Value);
            }
            if (fileName == "LayersPanel.swift")
                foreach (Match m in LayerTuple().Matches(line)) found.Add(m.Groups[1].Value);
        }
        // var symbol: String { switch ... case .x: "name" } and HistoryStep's
        // func symbol(_:) with its return "name" lines. The case patterns of that function
        // are engine command ids, so only the value after the colon or return counts.
        foreach (Match m in SymbolBlockStart().Matches(text))
        {
            var end = BlockEnd(text, m.Index + m.Length);
            foreach (Match s in BlockValue().Matches(text[(m.Index + m.Length)..end])) found.Add(s.Groups[1].Value);
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

    public static HashSet<string> All(string repo)
    {
        var all = new HashSet<string>();
        foreach (var file in SwiftFiles(repo)) all.UnionWith(Extract(Path.GetFileName(file), File.ReadAllText(file)));
        return all;
    }

    /// Not a symbol, because the name has this start. These are engine command ids
    /// (tool.text.begin), engine query ids (doc.dirty), effect ids and file type ids.
    /// They are dotted like symbols but no icon draws them, so they must not get a row.
    static readonly string[] NonSymbolStarts =
    [
        "artistic.", "blur.", "clipboard.", "color.", "com.", "commands.", "dev.", "distort.", "doc.", "document.", "edit.", "effect.", "effects.",
        "history.", "image.", "layer.", "move.", "noise.", "object.", "org.", "photo.", "preview.", "public.",
        "render.", "selection.", "stylize.", "tool.",
    ];

    /// True for a dotted name that names something other than an icon.
    public static bool IsKnownNonSymbol(string name) =>
        name.Contains('_') || name.EndsWith(".txt", StringComparison.Ordinal)
        || NonSymbolStarts.Any(s => name.StartsWith(s, StringComparison.Ordinal));

    /// Dotted names in the source that are neither found symbols nor known non-symbols.
    public static List<string> Unclassified(string repo, HashSet<string> symbols)
    {
        var left = new SortedSet<string>(StringComparer.Ordinal);
        foreach (var file in SwiftFiles(repo))
            foreach (Match m in DottedLiteral().Matches(File.ReadAllText(file)))
            {
                var name = m.Groups[1].Value;
                if (!symbols.Contains(name) && !IsKnownNonSymbol(name)) left.Add(name);
            }
        return left.ToList();
    }

    [GeneratedRegex("""^\s*"[^\n]*":\s*"(?<value>[^"\n]+)",\s*$""", RegexOptions.Multiline)] private static partial Regex DictionaryRow();
    [GeneratedRegex("""\b(?:systemSymbolName|symbol):\s*(?<rest>[^,]*)""")] private static partial Regex LabeledSymbol();
    [GeneratedRegex("\"([^\"]+)\"")] private static partial Regex Literal();
    [GeneratedRegex("\\bbutton\\(\"([^\"]+)\"")] private static partial Regex ButtonCall();
    [GeneratedRegex("\\btoggle\\(\"([^\"]+)\"")] private static partial Regex ToggleCall();
    [GeneratedRegex("\"(text\\.align[a-z]+)\"")] private static partial Regex AlignTuple();
    [GeneratedRegex("""^\s*\("[^"]+", "([^"]+)", "[^"]+"\),""")] private static partial Regex LayerTuple();
    [GeneratedRegex("""(?:var symbol: String|static func symbol\(_ command: String\) -> String)\s*\{""")] private static partial Regex SymbolBlockStart();
    [GeneratedRegex("(?:case \\.[^:\\n]+:|return)\\s*\"([^\"]+)\"")] private static partial Regex BlockValue();
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

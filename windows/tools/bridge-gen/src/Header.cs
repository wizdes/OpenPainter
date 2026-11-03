// Reads engine/op-ffi/include/openpainter.h, the C header cbindgen writes.
// It knows the few forms cbindgen uses. Anything else is an error that names the
// text, so a new form in the header cannot turn into a wrong C# call in silence.
using System.Text.RegularExpressions;

namespace BridgeGen;

public sealed record Constant(string Name, string Value);
public sealed record EnumDef(string Name, IReadOnlyList<(string Name, long Value)> Members);
public sealed record Field(string CType, string Name);
public sealed record StructDef(string Name, IReadOnlyList<Field> Fields);
public sealed record FunctionPointer(string Name, string Return, IReadOnlyList<string> Parameters);
public sealed record Function(string Name, string Return, IReadOnlyList<Field> Parameters);

public sealed class Header
{
    public List<Constant> Constants { get; } = [];
    public List<EnumDef> Enums { get; } = [];
    public List<StructDef> Structs { get; } = [];
    public List<string> Opaque { get; } = [];
    public List<FunctionPointer> FunctionPointers { get; } = [];
    public List<Function> Functions { get; } = [];

    public IEnumerable<string> ExportNames => Functions.Select(f => f.Name);

    public static Header Parse(string text)
    {
        var header = new Header();
        text = Regex.Replace(text.Replace("\r\n", "\n"), @"/\*.*?\*/", "", RegexOptions.Singleline);
        var rest = new List<string>();
        foreach (var line in text.Split('\n'))
        {
            if (!line.StartsWith('#')) { rest.Add(line); continue; }
            var define = Regex.Match(line, @"^#define\s+(\w+)\s+(.+)$");
            if (define.Success) header.Constants.Add(new Constant(define.Groups[1].Value, define.Groups[2].Value.Trim()));
            else if (!Regex.IsMatch(line, @"^#(ifndef|endif|include|define\s+\w+\s*$)")) throw new FormatException($"openpainter.h: unknown line: {line}");
        }
        // The include guard has a name and no value, so the pattern above leaves it out.
        var body = string.Join("\n", rest);
        foreach (var raw in Split(body))
        {
            var statement = Regex.Replace(raw, @"\s+", " ").Trim();
            if (statement.Length == 0) continue;
            header.Add(statement);
        }
        return header;
    }

    /// Splits at each semicolon outside braces.
    static IEnumerable<string> Split(string body)
    {
        var depth = 0;
        var start = 0;
        for (var i = 0; i < body.Length; i++)
        {
            if (body[i] == '{') depth++;
            else if (body[i] == '}') depth--;
            else if (body[i] == ';' && depth == 0)
            {
                yield return body[start..i];
                start = i + 1;
            }
        }
        var tail = body[start..].Trim();
        if (tail.Length != 0) throw new FormatException($"openpainter.h: text after the last statement: {tail}");
    }

    void Add(string s)
    {
        Match m;
        if ((m = Regex.Match(s, @"^typedef enum (\w+) \{(.*)\} (\w+)$")).Success)
        {
            if (m.Groups[1].Value != m.Groups[3].Value) throw new FormatException($"openpainter.h: enum with two names: {s}");
            var members = new List<(string, long)>();
            var next = 0L;
            foreach (var part in m.Groups[2].Value.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            {
                var pieces = part.Split('=', StringSplitOptions.TrimEntries);
                if (pieces.Length == 2) next = long.Parse(pieces[1]);
                members.Add((pieces[0], next));
                next++;
            }
            Enums.Add(new EnumDef(m.Groups[1].Value, members));
        }
        else if ((m = Regex.Match(s, @"^typedef struct (\w+) \{(.*)\} (\w+)$")).Success)
        {
            if (m.Groups[1].Value != m.Groups[3].Value) throw new FormatException($"openpainter.h: struct with two names: {s}");
            var fields = m.Groups[2].Value.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .Select(ParseField).ToList();
            Structs.Add(new StructDef(m.Groups[1].Value, fields));
        }
        else if ((m = Regex.Match(s, @"^typedef struct (\w+) (\w+)$")).Success)
        {
            if (m.Groups[1].Value != m.Groups[2].Value) throw new FormatException($"openpainter.h: opaque struct with two names: {s}");
            Opaque.Add(m.Groups[1].Value);
        }
        else if ((m = Regex.Match(s, @"^typedef (.+?) ?\(\*(\w+)\)\((.*)\)$")).Success)
        {
            var parameters = m.Groups[3].Value.Split(',', StringSplitOptions.TrimEntries).ToList();
            FunctionPointers.Add(new FunctionPointer(m.Groups[2].Value, m.Groups[1].Value, parameters));
        }
        else if ((m = Regex.Match(s, @"^(.+?[ *])(op_\w+)\((.*)\)$")).Success)
        {
            var parameters = m.Groups[3].Value == "void" || m.Groups[3].Value.Length == 0
                ? []
                : m.Groups[3].Value.Split(',', StringSplitOptions.TrimEntries).Select(ParseField).ToList();
            Functions.Add(new Function(m.Groups[2].Value, m.Groups[1].Value.Trim(), parameters));
        }
        else throw new FormatException($"openpainter.h: unknown statement: {s}");
    }

    static Field ParseField(string text)
    {
        var m = Regex.Match(text, @"^(.*[ *])(\w+)$");
        if (!m.Success) throw new FormatException($"openpainter.h: unknown field: {text}");
        return new Field(m.Groups[1].Value.Trim(), m.Groups[2].Value);
    }
}

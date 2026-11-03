// Writes the C# bridge from a parsed header. Each C export gets one C# call with
// the same name, so a search for the name finds both.
using System.Text;

namespace BridgeGen;

public static class Writer
{
    public const string Banner = "// Written by windows/tools/bridge-gen from engine/op-ffi/include/openpainter.h. Do not edit by hand.";

    public static string Write(Header header, string @namespace = "OpenPainter.Bridge")
    {
        var map = new TypeMap(header);
        var o = new StringBuilder();
        o.AppendLine(Banner);
        o.AppendLine("#nullable enable");
        o.AppendLine("using System.Runtime.CompilerServices;");
        o.AppendLine("using System.Runtime.InteropServices;");
        o.AppendLine();
        o.AppendLine($"namespace {@namespace};");
        o.AppendLine();
        foreach (var e in header.Enums)
        {
            o.AppendLine($"public enum {e.Name}");
            o.AppendLine("{");
            foreach (var (name, value) in e.Members) o.AppendLine($"    {name} = {value},");
            o.AppendLine("}");
            o.AppendLine();
        }
        foreach (var name in header.Opaque)
        {
            o.AppendLine($"public struct {name} {{ }}");
            o.AppendLine();
        }
        foreach (var s in header.Structs)
        {
            o.AppendLine("[StructLayout(LayoutKind.Sequential)]");
            o.AppendLine($"public unsafe struct {s.Name}");
            o.AppendLine("{");
            foreach (var f in s.Fields) o.AppendLine($"    public {map.Field(f.CType)} {Safe(f.Name)};");
            o.AppendLine("}");
            o.AppendLine();
        }
        o.AppendLine("public static unsafe partial class Native");
        o.AppendLine("{");
        o.AppendLine("    public const string Library = \"op_ffi\";");
        o.AppendLine();
        foreach (var c in header.Constants)
            o.AppendLine($"    public const long {c.Name} = {LongValue(c.Value)};");
        if (header.Constants.Count != 0) o.AppendLine();
        foreach (var f in header.Functions)
        {
            var parameters = string.Join(", ", f.Parameters.Select(p => $"{map.Parameter(p.CType)} {Safe(p.Name)}"));
            o.AppendLine("    [LibraryImport(Library)]");
            o.AppendLine("    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]");
            o.AppendLine($"    public static partial {map.Return(f.Return)} {f.Name}({parameters});");
            o.AppendLine();
        }
        o.AppendLine("}");
        return o.ToString().Replace("\r\n", "\n");
    }

    /// Each number becomes a long, so products over 2 GB do not overflow as C# ints.
    static string LongValue(string value) => System.Text.RegularExpressions.Regex.Replace(value, "[0-9]+", "${0}L");

    static readonly HashSet<string> CSharpWords = ["ref", "out", "in", "params", "object", "string", "base", "this", "event", "lock", "fixed", "checked", "operator"];
    static string Safe(string name) => CSharpWords.Contains(name) ? "@" + name : name;
}

/// C types to C# types. An unknown C type is an error that names it.
public sealed class TypeMap
{
    static readonly Dictionary<string, string> Plain = new()
    {
        ["void"] = "void",
        ["bool"] = "bool",
        ["char"] = "byte",
        ["uint8_t"] = "byte",
        ["int8_t"] = "sbyte",
        ["uint16_t"] = "ushort",
        ["int16_t"] = "short",
        ["uint32_t"] = "uint",
        ["int32_t"] = "int",
        ["uint64_t"] = "ulong",
        ["int64_t"] = "long",
        ["uintptr_t"] = "nuint",
        ["intptr_t"] = "nint",
        ["float"] = "float",
        ["double"] = "double",
    };

    readonly Header _header;
    readonly HashSet<string> _named;
    public TypeMap(Header header)
    {
        _header = header;
        _named = [.. header.Structs.Select(s => s.Name), .. header.Enums.Select(e => e.Name), .. header.Opaque];
    }

    public string Field(string c) => Map(c);
    public string Return(string c) => Map(c);
    public string Parameter(string c) => c == "bool" ? "[MarshalAs(UnmanagedType.U1)] bool" : Map(c);

    string Map(string c)
    {
        var text = c.Replace("const ", "").Replace("enum ", "").Replace("struct ", "").Trim();
        var stars = text.Count(ch => ch == '*');
        var name = text.Replace("*", "").Trim();
        var fp = _header.FunctionPointers.FirstOrDefault(p => p.Name == name);
        string baseType;
        if (fp is not null)
        {
            var parts = fp.Parameters.Select(p => Map(p)).Append(Map(fp.Return));
            baseType = $"delegate* unmanaged[Cdecl]<{string.Join(", ", parts)}>";
        }
        else if (Plain.TryGetValue(name, out var plain)) baseType = plain;
        else if (_named.Contains(name)) baseType = name;
        else throw new FormatException($"openpainter.h: no C# type for `{c}`");
        return baseType + new string('*', stars);
    }
}

/// A struct's size worked out from the header alone, for a test to compare with C#.
public static class CLayout
{
    public static (int Size, int Align) Of(Header header, string cType)
    {
        var text = cType.Replace("const ", "").Replace("enum ", "").Replace("struct ", "").Trim();
        if (text.Contains('*') || header.FunctionPointers.Any(p => p.Name == text)) return (8, 8);
        switch (text)
        {
            case "bool" or "uint8_t" or "int8_t": return (1, 1);
            case "uint16_t" or "int16_t": return (2, 2);
            case "uint32_t" or "int32_t" or "float": return (4, 4);
            case "uint64_t" or "int64_t" or "uintptr_t" or "intptr_t" or "double": return (8, 8);
        }
        if (header.Enums.Any(e => e.Name == text)) return (4, 4);
        var s = header.Structs.FirstOrDefault(d => d.Name == text) ?? throw new FormatException($"openpainter.h: no size for `{cType}`");
        var (at, align) = (0, 1);
        foreach (var f in s.Fields)
        {
            var (size, fieldAlign) = Of(header, f.CType);
            at = (at + fieldAlign - 1) / fieldAlign * fieldAlign + size;
            align = Math.Max(align, fieldAlign);
        }
        return ((at + align - 1) / align * align, align);
    }
}

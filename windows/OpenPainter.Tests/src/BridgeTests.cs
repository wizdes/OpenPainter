// The bridge against the header, and the header against the DLL, by name.
using System.Runtime.InteropServices;
using BridgeGen;
using OpenPainter.Bridge;
using Xunit;

namespace OpenPainter.Tests;

public sealed class BridgeTests
{
    const string HeaderPath = "engine/op-ffi/include/openpainter.h";
    const string BridgePath = "windows/OpenPainter/src/Bridge/Native.g.cs";
    const string DllPath = "engine/target/release/op_ffi.dll";

    static Header ReadHeader() => Header.Parse(Repo.Read(HeaderPath));

    [Fact]
    public void EveryExportInTheHeaderHasACSharpCall()
    {
        var missing = Checks.MissingCalls(ReadHeader(), Repo.Read(BridgePath));
        Assert.True(missing.Count == 0, $"exports with no C# call: {string.Join(", ", missing)}");
    }

    [Fact]
    public void EveryCSharpCallHasAnExportInTheHeader()
    {
        var extra = Checks.ExtraCalls(ReadHeader(), Repo.Read(BridgePath));
        Assert.True(extra.Count == 0, $"C# calls with no export: {string.Join(", ", extra)}");
    }

    [Fact]
    public void TheCheckedInBridgeIsWhatTheGeneratorWrites()
    {
        var fresh = Writer.Write(ReadHeader());
        var kept = Repo.Read(BridgePath);
        Assert.True(fresh == kept, $"{BridgePath} is out of date. Run: dotnet run --project windows/tools/bridge-gen -- --header {HeaderPath} --out {BridgePath}");
    }

    [Fact]
    public void AMissingCallFailsByName()
    {
        var header = ReadHeader();
        var bridge = Repo.Read(BridgePath);
        var without = bridge.Replace("public static partial OpStatus op_view_set(", "public static partial OpStatus other_view_set(");
        Assert.NotEqual(bridge, without);
        Assert.Equal(["op_view_set"], Checks.MissingCalls(header, without));
    }

    [Fact]
    public void AnExtraCallFailsByName()
    {
        var bridge = Repo.Read(BridgePath) + "public static partial OpStatus op_not_real(OpEngine* p);";
        Assert.Equal(["op_not_real"], Checks.ExtraCalls(ReadHeader(), bridge));
    }

    [Fact]
    public void TheDllExportsExactlyTheHeadersNames()
    {
        var path = Repo.File(DllPath);
        Assert.True(File.Exists(path), $"{DllPath} is missing. Run: cargo build -p op-ffi --release (from engine/)");
        var dll = Checks.DllExports(path);
        var header = ReadHeader().ExportNames.OrderBy(n => n, StringComparer.Ordinal).ToList();
        var notInHeader = dll.Except(header).ToList();
        var notInDll = header.Except(dll).ToList();
        Assert.True(notInHeader.Count == 0, $"DLL exports not in the header: {string.Join(", ", notInHeader)}");
        Assert.True(notInDll.Count == 0, $"header exports not in the DLL: {string.Join(", ", notInDll)}. The DLL may be older than the header.");
        Assert.True(dll.Count >= 50, $"only {dll.Count} exports");
    }

    [Fact]
    public void AFileThatIsNotADllFailsByName()
    {
        var path = Path.GetTempFileName();
        try
        {
            File.WriteAllText(path, "this is not a program");
            Assert.ThrowsAny<Exception>(() => Checks.DllExports(path));
        }
        finally
        {
            File.Delete(path);
        }
    }

    /// Each struct's C# size equals the C size worked out from the header, by name.
    [Fact]
    public void EveryStructHasTheCSizeInCSharp()
    {
        var header = ReadHeader();
        Assert.True(header.Structs.Count >= 10, $"only {header.Structs.Count} structs");
        var assembly = typeof(OpStatus).Assembly;
        foreach (var s in header.Structs)
        {
            var type = assembly.GetType($"OpenPainter.Bridge.{s.Name}");
            Assert.True(type is not null, $"{s.Name} has no C# struct");
            var c = CLayout.Of(header, s.Name).Size;
            var csharp = Marshal.SizeOf(type);
            Assert.True(c == csharp, $"{s.Name}: C size {c}, C# size {csharp}");
        }
    }

    [Fact]
    public void EnumValuesAndConstantsComeThroughFromTheHeader()
    {
        Assert.Equal(8, (int)OpStatus.OP_ERR_CANCELLED);
        Assert.Equal(6, (int)OpEventKind.OP_EVENT_RECOVERY);
        Assert.Equal(2 * 1024L * 1024 * 1024 + 16 * 1024 * 1024, Native.MAX_PNG_FILE);
    }

    [Theory]
    [InlineData("#pragma once\n", "#pragma")]
    [InlineData("typedef union OddThing { int a; } OddThing;\n", "OddThing")]
    [InlineData("int other_function(int a);\n", "other_function")]
    public void AFormTheReaderDoesNotKnowFailsAndNamesIt(string text, string name)
    {
        var error = Assert.Throws<FormatException>(() => Header.Parse(text));
        Assert.Contains(name, error.Message);
    }

    [Fact]
    public void ATypeWithNoCSharpMatchFailsAndNamesIt()
    {
        var header = Header.Parse("struct Odd *op_x(struct Odd *p);\n");
        var error = Assert.Throws<FormatException>(() => Writer.Write(header));
        Assert.Contains("Odd", error.Message);
    }

    [Fact]
    public void ABoolParameterIsOneByte()
    {
        var text = Writer.Write(Header.Parse("void op_x(bool flag);\n"));
        Assert.Contains("[MarshalAs(UnmanagedType.U1)] bool flag", text);
    }

    // ---- Each call's types against the header. Written apart from the generator's table, so a
    // wrong row there or a hand edit in the bridge shows here.

    static readonly Dictionary<string, string> CTypes = new()
    {
        ["void"] = "void", ["bool"] = "bool", ["char"] = "byte",
        ["uint8_t"] = "byte", ["int8_t"] = "sbyte", ["uint16_t"] = "ushort", ["int16_t"] = "short",
        ["uint32_t"] = "uint", ["int32_t"] = "int", ["uint64_t"] = "ulong", ["int64_t"] = "long",
        ["uintptr_t"] = "nuint", ["intptr_t"] = "nint", ["float"] = "float", ["double"] = "double",
    };

    static string WantedType(Header header, string c)
    {
        var text = c.Replace("const ", "").Replace("enum ", "").Replace("struct ", "").Trim();
        var stars = new string('*', text.Count(ch => ch == '*'));
        var name = text.Replace("*", "").Trim();
        if (header.FunctionPointers.FirstOrDefault(p => p.Name == name) is { } pointer)
        {
            var types = pointer.Parameters.Append(pointer.Return).Select(t => WantedType(header, t));
            return $"delegate* unmanaged[Cdecl]<{string.Join(", ", types)}>{stars}";
        }
        if (CTypes.TryGetValue(name, out var plain)) return plain + stars;
        Assert.True(header.Structs.Any(s => s.Name == name) || header.Enums.Any(e => e.Name == name) || header.Opaque.Contains(name), $"the header names no type {name}, in `{c}`");
        return name + stars;
    }

    /// Splits at commas that are not inside a function pointer's own type list.
    static List<string> SplitParameters(string text)
    {
        var parts = new List<string>();
        var (depth, start) = (0, 0);
        for (var i = 0; i < text.Length; i++)
        {
            if (text[i] is '<' or '[') depth++;
            else if (text[i] is '>' or ']') depth--;
            else if (text[i] == ',' && depth == 0)
            {
                parts.Add(text[start..i].Trim());
                start = i + 1;
            }
        }
        if (text[start..].Trim().Length != 0) parts.Add(text[start..].Trim());
        return parts;
    }

    /// One line per difference, by the call's name.
    static List<string> TypeDifferences(Header header, string bridge)
    {
        var calls = System.Text.RegularExpressions.Regex.Matches(bridge, @"public static partial (?<ret>[^;(]*?) (?<name>op_\w+)\((?<args>[^;]*)\);")
            .ToDictionary(m => m.Groups["name"].Value);
        var differences = new List<string>();
        foreach (var function in header.Functions)
        {
            if (!calls.TryGetValue(function.Name, out var call))
            {
                differences.Add($"{function.Name}: no C# call");
                continue;
            }
            var ret = WantedType(header, function.Return);
            if (call.Groups["ret"].Value != ret) differences.Add($"{function.Name}: returns {call.Groups["ret"].Value}, the header says {ret}");
            var got = SplitParameters(call.Groups["args"].Value);
            if (got.Count != function.Parameters.Count)
            {
                differences.Add($"{function.Name}: {got.Count} parameters, the header says {function.Parameters.Count}");
                continue;
            }
            for (var i = 0; i < got.Count; i++)
            {
                var written = got[i].Replace("[MarshalAs(UnmanagedType.U1)] ", "");
                var type = written[..written.LastIndexOf(' ')];
                var want = WantedType(header, function.Parameters[i].CType);
                if (type != want) differences.Add($"{function.Name} parameter {function.Parameters[i].Name}: {type}, the header says {want}");
                if (want == "bool" && !got[i].Contains("MarshalAs(UnmanagedType.U1)", StringComparison.Ordinal)) differences.Add($"{function.Name} parameter {function.Parameters[i].Name}: a C bool must be one byte");
            }
        }
        return differences;
    }

    [Fact]
    public void EveryCallsTypesMatchTheHeader()
    {
        var header = ReadHeader();
        Assert.True(header.Functions.Count >= 50, $"only {header.Functions.Count} functions");
        var differences = TypeDifferences(header, Repo.Read(BridgePath));
        Assert.True(differences.Count == 0, string.Join("\n", differences));
    }

    [Theory]
    [InlineData("public static partial OpStatus op_view_set(OpEngine* p, ulong view, OpViewState* state);", "public static partial OpStatus op_view_set(OpEngine* p, uint view, OpViewState* state);", "op_view_set parameter view: uint")]
    [InlineData("public static partial OpStatus op_view_set(OpEngine* p, ulong view, OpViewState* state);", "public static partial int op_view_set(OpEngine* p, ulong view, OpViewState* state);", "op_view_set: returns int")]
    [InlineData("public static partial OpStatus op_view_set(OpEngine* p, ulong view, OpViewState* state);", "public static partial OpStatus op_view_set(OpEngine* p, ulong view);", "op_view_set: 2 parameters")]
    public void AWrongTypeInACallFailsByName(string right, string wrong, string words)
    {
        var bridge = Repo.Read(BridgePath);
        Assert.Contains(right, bridge);
        Assert.Contains(TypeDifferences(ReadHeader(), bridge.Replace(right, wrong)), line => line.StartsWith(words, StringComparison.Ordinal));
    }

    [Fact]
    public void AWrongTypeInTheHeaderFailsByName()
    {
        var text = Repo.Read(HeaderPath).Replace("enum OpStatus op_view_set(struct OpEngine *p, uint64_t view,", "enum OpStatus op_view_set(struct OpEngine *p, uint32_t view,");
        Assert.NotEqual(Repo.Read(HeaderPath), text);
        Assert.Contains(TypeDifferences(Header.Parse(text), Repo.Read(BridgePath)), line => line.StartsWith("op_view_set parameter view", StringComparison.Ordinal));
    }
}

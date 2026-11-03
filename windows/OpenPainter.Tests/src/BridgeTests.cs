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
}

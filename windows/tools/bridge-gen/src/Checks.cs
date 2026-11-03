// The name checks: the header against the C# bridge, and the DLL against the header.
using System.Reflection.PortableExecutable;
using System.Text;
using System.Text.RegularExpressions;

namespace BridgeGen;

public static class Checks
{
    /// The names the C# source declares as bridge calls.
    public static IReadOnlySet<string> CallsIn(string csharp) =>
        Regex.Matches(csharp, @"public static partial [^;(]*?\b(op_\w+)\(").Select(m => m.Groups[1].Value).ToHashSet();

    /// Exports in the header with no C# call, by name.
    public static IReadOnlyList<string> MissingCalls(Header header, string csharp)
    {
        var calls = CallsIn(csharp);
        return header.ExportNames.Where(n => !calls.Contains(n)).OrderBy(n => n, StringComparer.Ordinal).ToList();
    }

    /// C# calls with no export in the header, by name.
    public static IReadOnlyList<string> ExtraCalls(Header header, string csharp)
    {
        var known = header.ExportNames.ToHashSet();
        return CallsIn(csharp).Where(n => !known.Contains(n)).OrderBy(n => n, StringComparer.Ordinal).ToList();
    }

    /// The `op_` names a DLL exports, read from its export table.
    public static IReadOnlyList<string> DllExports(string path)
    {
        using var stream = File.OpenRead(path);
        using var pe = new PEReader(stream);
        var headers = pe.PEHeaders;
        var directory = headers.PEHeader?.ExportTableDirectory ?? throw new InvalidDataException($"{path}: no PE header");
        if (directory.Size == 0) throw new InvalidDataException($"{path}: no export table");
        var image = pe.GetEntireImage();

        var bytes = image.GetContent();
        int FileOffset(int rva)
        {
            foreach (var section in headers.SectionHeaders)
                if (rva >= section.VirtualAddress && rva < section.VirtualAddress + Math.Max(section.VirtualSize, section.SizeOfRawData))
                    return rva - section.VirtualAddress + section.PointerToRawData;
            throw new InvalidDataException($"{path}: address {rva:x} is in no section");
        }
        var table = FileOffset(directory.RelativeVirtualAddress);
        var count = BitConverter.ToInt32(bytes.AsSpan(table + 24, 4));
        var names = FileOffset(BitConverter.ToInt32(bytes.AsSpan(table + 32, 4)));
        var result = new List<string>(count);
        for (var i = 0; i < count; i++)
        {
            var at = FileOffset(BitConverter.ToInt32(bytes.AsSpan(names + i * 4, 4)));
            var end = at;
            while (bytes[end] != 0) end++;
            result.Add(Encoding.ASCII.GetString(bytes.AsSpan(at, end - at)));
        }
        return result.Where(n => n.StartsWith("op_", StringComparison.Ordinal)).OrderBy(n => n, StringComparer.Ordinal).ToList();
    }
}

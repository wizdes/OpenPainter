// bridge-gen writes the C# bridge, or with --check says by name what is out of step.
//
//   dotnet run --project windows/tools/bridge-gen -- --header <openpainter.h> --out <Native.g.cs>
//   dotnet run --project windows/tools/bridge-gen -- --header <openpainter.h> --out <Native.g.cs> --check [--dll <op_ffi.dll>]
namespace BridgeGen;

public static class Program
{
    public static int Main(string[] args)
    {
        string? headerPath = null, outPath = null, dllPath = null;
        var check = false;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--header" when i + 1 < args.Length: headerPath = args[++i]; break;
                case "--out" when i + 1 < args.Length: outPath = args[++i]; break;
                case "--dll" when i + 1 < args.Length: dllPath = args[++i]; break;
                case "--check": check = true; break;
                default:
                    Console.Error.WriteLine($"bridge-gen: unknown argument {args[i]}");
                    return 2;
            }
        }
        if (headerPath is null || outPath is null)
        {
            Console.Error.WriteLine("usage: bridge-gen --header <openpainter.h> --out <Native.g.cs> [--check] [--dll <op_ffi.dll>]");
            return 2;
        }
        var header = Header.Parse(File.ReadAllText(headerPath));
        var fresh = Writer.Write(header);
        if (!check)
        {
            File.WriteAllText(outPath, fresh);
            Console.WriteLine($"bridge-gen: wrote {header.Functions.Count} calls to {outPath}");
            return 0;
        }
        var kept = File.ReadAllText(outPath).Replace("\r\n", "\n");
        var problems = Checks.MissingCalls(header, kept).Select(n => $"export with no C# call: {n}")
            .Concat(Checks.ExtraCalls(header, kept).Select(n => $"C# call with no export: {n}")).ToList();
        if (kept != fresh) problems.Add($"{outPath} differs from what bridge-gen writes");
        if (dllPath is not null)
        {
            var dll = Checks.DllExports(dllPath);
            problems.AddRange(dll.Except(header.ExportNames).Select(n => $"DLL export not in the header: {n}"));
            problems.AddRange(header.ExportNames.Except(dll).Select(n => $"header export not in the DLL: {n}"));
        }
        foreach (var problem in problems) Console.Error.WriteLine($"bridge-gen: {problem}");
        if (problems.Count == 0) Console.WriteLine($"bridge-gen: {header.Functions.Count} calls match");
        return problems.Count == 0 ? 0 : 1;
    }
}

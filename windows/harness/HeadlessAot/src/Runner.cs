// Runs every headless UI test on the AOT build and prints a total in `dotnet test`'s words,
// so `cargo xtask check` reads it the same way. A test that the trimmer removed would look
// like a pass, so the run also counts the test marks in the source, and fails on a mismatch.
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using OpenPainter.Bridge;
using OpenPainter.Tests;
using Xunit;

namespace OpenPainter.HeadlessAot;

public static partial class Runner
{
    const string TestSourceFolder = "windows/OpenPainter.Tests/src";
    static readonly string[] NotOnThisBuild = ["HarnessTests.cs", "VectorTests.cs"];
    static readonly TimeSpan TestLimit = TimeSpan.FromSeconds(60);

    [GeneratedRegex(@"^\s*\[(AvaloniaFact|Fact|Theory)\]", RegexOptions.Multiline)] private static partial Regex TestMark();

    /// One run of one test method, with a theory's values.
    record Case(string Name, MethodInfo Method, object?[]? Values, bool NeedsUi);

    static IEnumerable<Case> Cases()
    {
        foreach (var type in typeof(Runner).Assembly.GetTypes().Where(t => t.Namespace == "OpenPainter.Tests").OrderBy(t => t.FullName, StringComparer.Ordinal))
        {
            foreach (var method in type.GetMethods(BindingFlags.Public | BindingFlags.Instance | BindingFlags.Static | BindingFlags.DeclaredOnly))
            {
                var name = $"{type.Name}.{method.Name}";
                if (method.IsDefined(typeof(AvaloniaFactAttribute), false)) yield return new Case(name, method, null, true);
                else if (method.IsDefined(typeof(FactAttribute), false)) yield return new Case(name, method, null, false);
                else if (method.IsDefined(typeof(TheoryAttribute), false))
                    foreach (var data in method.GetCustomAttributes<InlineDataAttribute>(false))
                        yield return new Case($"{name}({string.Join(", ", data.Values.Select(v => v ?? "null"))})", method, data.Values, false);
            }
        }
    }

    static object? Argument(object? value, Type type)
    {
        if (value is null || type.IsInstanceOfType(value)) return value;
        var target = Nullable.GetUnderlyingType(type) ?? type;
        return Convert.ChangeType(value, target, System.Globalization.CultureInfo.InvariantCulture);
    }

    /// Never called. xunit's Assert and BridgeTests pick these at run time, and native code
    /// exists only for what the compiler can see, so these lines make the compiler see them.
    public static void KeepForTests()
    {
        _ = Xunit.Sdk.CollectionTrackerExtensions.AsTracker<double>(Array.Empty<double>());
        _ = Marshal.SizeOf<OpConfig>() + Marshal.SizeOf<OpEvent>() + Marshal.SizeOf<OpBuffer>() + Marshal.SizeOf<OpPixelBuffer>()
            + Marshal.SizeOf<OpPointerEvent>() + Marshal.SizeOf<OpPipelineCounts>() + Marshal.SizeOf<OpStrokeSample>() + Marshal.SizeOf<OpViewState>()
            + Marshal.SizeOf<OpOverlayPoint>() + Marshal.SizeOf<OpOverlayItem>() + Marshal.SizeOf<OpViewInfo>() + Marshal.SizeOf<OpViewCounters>() + Marshal.SizeOf<OpFrameStamp>();
    }

    static string Describe(Exception error)
    {
        var text = $"{error.GetType().Name}: {error.Message}";
        return error.InnerException is { } inner ? $"{text} <- {Describe(inner)}" : text;
    }

    static void Run(Case test)
    {
        var parameters = test.Method.GetParameters();
        var values = test.Values is null ? null : test.Values.Select((v, i) => Argument(v, parameters[i].ParameterType)).ToArray();
        var target = test.Method.IsStatic ? null : Activator.CreateInstance(test.Method.DeclaringType!);
        try
        {
            test.Method.Invoke(target, values);
        }
        catch (TargetInvocationException e) when (e.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(e.InnerException).Throw();
        }
    }

    public static int Main()
    {
        Console.OutputEncoding = System.Text.Encoding.UTF8;
        var cases = Cases().ToList();
        var methods = cases.Select(c => c.Method).Distinct().Count();
        var marked = Directory.GetFiles(Repo.File(TestSourceFolder), "*.cs")
            .Where(file => !NotOnThisBuild.Contains(Path.GetFileName(file)))
            .Sum(file => TestMark().Matches(File.ReadAllText(file)).Count);
        var failed = 0;
        using var session = HeadlessUnitTestSession.StartNew(typeof(TestApp), AvaloniaTestIsolationLevel.PerTest);
        foreach (var test in cases)
        {
            try
            {
                if (!session.Dispatch(() => Run(test), CancellationToken.None).Wait(TestLimit)) throw new TimeoutException($"still running after {TestLimit.TotalSeconds} s");
            }
            catch (Exception e)
            {
                var error = e is AggregateException { InnerException: not null } a ? a.InnerException! : e;
                failed++;
                Console.WriteLine($"FAILED {test.Name}: {Describe(error)}");
            }
        }
        if (methods != marked)
        {
            failed++;
            Console.WriteLine($"FAILED the source marks {marked} test methods and this build found {methods}, so the trimmer removed some");
        }
        Console.WriteLine($"Failed: {failed}, Passed: {cases.Count - Math.Min(failed, cases.Count)}, Skipped: 0, Total: {cases.Count}, {methods} test methods on the AOT build");
        return failed == 0 && cases.Count > 0 ? 0 : 1;
    }
}

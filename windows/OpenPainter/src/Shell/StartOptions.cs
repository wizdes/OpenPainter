// The program's arguments. All of them are test hooks for now, because Open comes
// in W10. A wrong argument stops the program and names it.
namespace OpenPainter.Shell;

public sealed record StartOptions(string? TestPng, int? ExitAfterMs, string? ReportPath, string? Theme, string? TestTool = null, bool TestOpenMore = false, string? TestOpenMenu = null)
{
    public const string Usage = "usage: OpenPainter [--test-load-png <file>] [--exit-after-ms <n>] [--test-report <file.json>] [--theme light|dark] [--test-tool <id>] [--test-open-more] [--test-open-menu <title>]";

    public static StartOptions Parse(IReadOnlyList<string> args)
    {
        string? png = null, report = null, theme = null, tool = null, menu = null;
        var openMore = false;
        int? exit = null;
        for (var i = 0; i < args.Count; i++)
        {
            string Value() => ++i < args.Count ? args[i] : throw new ArgumentException($"ARGUMENTS: {args[i - 1]} needs a value. {Usage}");
            switch (args[i])
            {
                case "--test-load-png":
                    png = Value();
                    if (!File.Exists(png)) throw new ArgumentException($"ARGUMENTS: --test-load-png: no file at {png}");
                    break;
                case "--exit-after-ms":
                    var text = Value();
                    if (!int.TryParse(text, out var ms) || ms <= 0) throw new ArgumentException($"ARGUMENTS: --exit-after-ms: {text} is not a positive whole number");
                    exit = ms;
                    break;
                case "--test-report":
                    report = Value();
                    break;
                case "--theme":
                    theme = Value();
                    if (theme is not ("light" or "dark")) throw new ArgumentException($"ARGUMENTS: --theme: {theme} is not light or dark");
                    break;
                case "--test-tool":
                    tool = Value();
                    break;
                case "--test-open-menu":
                    menu = Value();
                    break;
                case "--test-open-more":
                    openMore = true;
                    break;
                default:
                    throw new ArgumentException($"ARGUMENTS: unknown argument {args[i]}. {Usage}");
            }
        }
        if (report is not null && exit is null) throw new ArgumentException("ARGUMENTS: --test-report needs --exit-after-ms, so the run ends by itself");
        return new StartOptions(png, exit, report, theme, tool, openMore, menu);
    }
}

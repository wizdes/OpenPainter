// The program's arguments. All of them are test hooks for now, because Open comes
// in W10. A wrong argument stops the program and names it.
using System.Globalization;

namespace OpenPainter.Shell;

public sealed record StartOptions(string? TestPng, int? ExitAfterMs, string? ReportPath, string? Theme, string? TestTool = null, bool TestOpenMore = false, string? TestOpenMenu = null,
    string? DataRoot = null, double? TestZoom = null, string? FirstFramePath = null, string? PanTracePath = null, double PanSeconds = 30)
{
    public const string Usage = "usage: OpenPainter [--data-root <folder>] [--test-load-png <file>] [--exit-after-ms <n>] [--test-report <file.json>] [--theme light|dark] [--test-tool <id>] [--test-open-more] [--test-open-menu <title>] [--test-zoom <ratio>] [--test-first-frame <file.json>] [--test-pan-trace <file.json>] [--test-pan-seconds <s>]";

    public static StartOptions Parse(IReadOnlyList<string> args)
    {
        string? png = null, report = null, theme = null, tool = null, menu = null, dataRoot = null, firstFrame = null, panTrace = null;
        var openMore = false;
        int? exit = null;
        double? zoom = null;
        var panSeconds = 30.0;
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
                case "--data-root":
                    dataRoot = Value();
                    if (!Directory.Exists(dataRoot)) throw new ArgumentException($"ARGUMENTS: --data-root: no folder at {dataRoot}");
                    break;
                case "--test-zoom":
                    var zoomText = Value();
                    if (!double.TryParse(zoomText, NumberStyles.Float, CultureInfo.InvariantCulture, out var ratio) || ratio <= 0) throw new ArgumentException($"ARGUMENTS: --test-zoom: {zoomText} is not a positive ratio");
                    zoom = ratio;
                    break;
                case "--test-first-frame":
                    firstFrame = Value();
                    break;
                case "--test-pan-trace":
                    panTrace = Value();
                    break;
                case "--test-pan-seconds":
                    var secondsText = Value();
                    if (!double.TryParse(secondsText, NumberStyles.Float, CultureInfo.InvariantCulture, out panSeconds) || panSeconds <= 0) throw new ArgumentException($"ARGUMENTS: --test-pan-seconds: {secondsText} is not a positive number");
                    break;
                default:
                    throw new ArgumentException($"ARGUMENTS: unknown argument {args[i]}. {Usage}");
            }
        }
        if (report is not null && exit is null) throw new ArgumentException("ARGUMENTS: --test-report needs --exit-after-ms, so the run ends by itself");
        return new StartOptions(png, exit, report, theme, tool, openMore, menu, dataRoot, zoom, firstFrame, panTrace, panSeconds);
    }
}

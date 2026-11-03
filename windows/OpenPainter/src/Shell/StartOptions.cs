// The program's arguments. All of them are test hooks for now, because Open comes
// in W10. A wrong argument stops the program and names it.
using System.Globalization;

namespace OpenPainter.Shell;

public sealed record StartOptions(string? TestPng, int? ExitAfterMs, string? ReportPath, string? Theme, string? TestTool = null, bool TestOpenMore = false, string? TestOpenMenu = null,
    string? DataRoot = null, double? TestZoom = null, string? FirstFramePath = null, string? PanTracePath = null, double PanSeconds = 30, string? TestThrow = null, Avalonia.PixelSize? TestWorkArea = null, string? FirstPaintPath = null, string? TestPaintFault = null, string? TestPanelsFolder = null)
{
    public const double MostPanSeconds = 3600;

    public const string BlackCanvasFault = "black-canvas", WrongStageFault = "wrong-stage";

    /// What `--test-paint-fault` can plant, so the harness can prove `launch paint` still fails.
    public static readonly string[] PaintFaults = [BlackCanvasFault, WrongStageFault];

    public const string Usage = "usage: OpenPainter [--data-root <folder>] [--test-load-png <file>] [--exit-after-ms <n>] [--test-report <file.json>] [--theme light|dark] [--test-tool <id>] [--test-open-more] [--test-open-menu <title>] [--test-zoom <ratio>] [--test-first-frame <file.json>] [--test-pan-trace <file.json>] [--test-pan-seconds <s>] [--test-throw <place>] [--test-work-area <width>x<height>] [--test-first-paint <file.json>] [--test-paint-fault <kind>] [--test-panels <folder>]";

    public static StartOptions Parse(IReadOnlyList<string> args)
    {
        string? png = null, report = null, theme = null, tool = null, menu = null, dataRoot = null, firstFrame = null, panTrace = null, firstPaint = null, paintFault = null, panels = null;
        var openMore = false;
        int? exit = null;
        double? zoom = null;
        var panSeconds = 30.0;
        string? testThrow = null;
        Avalonia.PixelSize? workArea = null;
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
                    if (!double.TryParse(zoomText, NumberStyles.Float, CultureInfo.InvariantCulture, out var ratio) || !double.IsFinite(ratio) || ratio <= 0) throw new ArgumentException($"ARGUMENTS: --test-zoom: {zoomText} is not a positive ratio");
                    zoom = ratio;
                    break;
                case "--test-first-frame":
                    firstFrame = Value();
                    break;
                case "--test-panels":
                    panels = Value();
                    break;
                case "--test-first-paint":
                    firstPaint = Value();
                    break;
                case "--test-paint-fault":
                    paintFault = Value();
                    if (!PaintFaults.Contains(paintFault)) throw new ArgumentException($"ARGUMENTS: --test-paint-fault: {paintFault} is not one of {string.Join(", ", PaintFaults)}");
                    break;
                case "--test-pan-trace":
                    panTrace = Value();
                    break;
                case "--test-pan-seconds":
                    var secondsText = Value();
                    if (!double.TryParse(secondsText, NumberStyles.Float, CultureInfo.InvariantCulture, out panSeconds) || !double.IsFinite(panSeconds) || panSeconds <= 0 || panSeconds > MostPanSeconds)
                        throw new ArgumentException($"ARGUMENTS: --test-pan-seconds: {secondsText} is not a number of seconds from 0 to {MostPanSeconds}");
                    break;
                case "--test-throw":
                    testThrow = Value();
                    if (!Fatal.Places.Contains(testThrow)) throw new ArgumentException($"ARGUMENTS: --test-throw: {testThrow} is not one of {string.Join(", ", Fatal.Places)}");
                    break;
                case "--test-work-area":
                    var areaText = Value();
                    var sides = areaText.Split('x');
                    if (sides.Length != 2 || !int.TryParse(sides[0], out var areaWidth) || !int.TryParse(sides[1], out var areaHeight) || areaWidth <= 0 || areaHeight <= 0)
                        throw new ArgumentException($"ARGUMENTS: --test-work-area: {areaText} is not <width>x<height>");
                    workArea = new Avalonia.PixelSize(areaWidth, areaHeight);
                    break;
                default:
                    throw new ArgumentException($"ARGUMENTS: unknown argument {args[i]}. {Usage}");
            }
        }
        if (report is not null && exit is null) throw new ArgumentException("ARGUMENTS: --test-report needs --exit-after-ms, so the run ends by itself");
        return new StartOptions(png, exit, report, theme, tool, openMore, menu, dataRoot, zoom, firstFrame, panTrace, panSeconds, testThrow, workArea, firstPaint, paintFault, panels);
    }
}

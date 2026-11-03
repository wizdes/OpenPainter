// Where an exception ends up when it cannot be left to fly. Window messages reach managed
// code through Avalonia, and a throw there is kept from the program (docs/windows/w7-shell.md
// has the run that shows it). So every handler that runs inside a window message catches here,
// and this names the failure, sets exit code 1 and ends the program.
using Avalonia.Threading;

namespace OpenPainter.Shell;

public static class Fatal
{
    public const string ClosedPlace = "closed", SizeLimitPlace = "size-limit", PanelSizeLimitPlace = "panel-size-limit", MovedPlace = "moved", PanelMovedPlace = "panel-moved";

    /// Where `--test-throw` can plant a failure, so the harness can prove each path on a real window.
    public static readonly string[] Places = [ClosedPlace, SizeLimitPlace, PanelSizeLimitPlace, MovedPlace, PanelMovedPlace];

    static int s_raised;

    /// Ends the program with exit code 1. The app sets it once its lifetime exists.
    public static Action? Shutdown { get; set; }

    /// The place `--test-throw` named, if any.
    public static string? TestPlace { get; set; }

    public static void TestThrow(string place)
    {
        if (TestPlace == place) throw new InvalidOperationException($"TEST_THROW: {place}");
    }

    public static void Guard(string name, Action body)
    {
        try
        {
            body();
        }
        catch (Exception e)
        {
            Raise(name, e);
        }
    }

    public static void Raise(string name, Exception e) => Raise(new InvalidOperationException($"{name}: {e.Message}", e));

    /// Can run on any thread and in any window message. The words and the exit code do not wait
    /// for the UI thread, because on the way out it may never run another job.
    public static void Raise(Exception e)
    {
        App.ExitCode = 1;
        // The first failure is the one to read. A message that keeps failing would only repeat it.
        if (Interlocked.Exchange(ref s_raised, 1) != 0) return;
        Program.ShowFatal(e);
        if (Shutdown is { } shutdown) Dispatcher.UIThread.Post(shutdown);
    }
}

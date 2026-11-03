// Runs once, before any test: the setup that UI tests would otherwise do first, while plain tests
// ran beside them. The AOT build runs its tests in a fixed order with no xunit, so it leaves this out.
using Avalonia.Headless;
using OpenPainter.Shell;
using UiLook;
using Xunit;

[assembly: AssemblyFixture(typeof(OpenPainter.Tests.TestStart))]

namespace OpenPainter.Tests;

public sealed class TestStart
{
    public TestStart()
    {
        HeadlessUnitTestSession.GetOrStartForAssembly(typeof(TestApp).Assembly).Dispatch(() =>
        {
            // Avalonia's static setup, for each control type, happens here and only here.
            var window = Shells.Open();
            window.Close();
            // LayoutTests.ASmallIconDrawsItsOwnArtNeverScaled is a plain test, so nothing is set up
            // while it runs. It passes only with these pictures already in Icons' cache.
            Icons.Folder = Repo.File("windows/assets/icons");
            foreach (var (name, size) in new[] { ("window-close", 12.0), ("window-close", 16.0), ("cmd-undo", 16.0), ("cmd-redo", 16.0), ("opt-minus", 16.0), ("opt-plus", 16.0), ("opt-overflow", 16.0), ("cmd-new", 20.0) })
                Icons.Load(name, size);
        }, CancellationToken.None).GetAwaiter().GetResult();
    }
}

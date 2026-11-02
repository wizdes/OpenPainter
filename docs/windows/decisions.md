# Windows decisions

## UI stack: Avalonia 12 with NativeAOT (W5, 2026-11-02)

The Windows app is built on Avalonia 12, compiled to one native program. This holds unless the overlay test below fails.

Astra and Fable each voted separately for Avalonia, each at about 65% confidence. Results are in `w5-spike.md`.

**Why Avalonia:**

1. Builders have no desktop. They can test Avalonia controls with no window, and they cannot test WinUI at all. About 182 hours of UI work depend on this.
2. Avalonia's whole UI Automation walk passes on the hidden desktop, so root can run those checks at any time. WinUI's dialog step fails there.
3. Overlay shapes drawn in the engine can be tested by builders in Rust. XAML shapes need a real window.
4. Measured speed does not separate them. First frame is 429 to 765 ms against 405 to 750 ms. Pointer-driven pan on a blank document is 57 to 58 fps against 58 fps.
5. The program is 46 MB against 136 MB. W19's limit is 75 MB. The WinUI number is the self-contained build, and a Store package could be smaller.

**What Avalonia costs:**

1. The engine must draw every canvas overlay. WinUI would get them from XAML.
2. The canvas needs hand-written Win32 code: a window class, Tab handoff, pen messages, dead keys.
3. Avalonia's UI frame callback runs at 41 to 44 per second on a 60 Hz screen, with every drawing route tried, even when the callback does almost nothing. So nothing that must run at full rate may depend on it. Ants run on their own thread, at 60.0 fps, and pan runs from the pointer. Avalonia's own animations may run at this lower rate.
4. Headless tests cover the controls only. In the real window the native canvas sits above the control they click, so canvas input and pixels need root's window checks.

**The named stop:** a builder makes the engine draw the Mac app's real overlay set (W5b in `plans/W5.md`). This choice reopens if that overlay misses 59 fps with a 200-point lasso, or if the whole set is estimated above 20 hours of further work.

**Open, and not stopping this:** Narrator has not been heard on either stack, a real dead key and a real pen have not been tried, and a shared GPU texture was not tested as another way to host the canvas.

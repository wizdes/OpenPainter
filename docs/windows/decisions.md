# Windows decisions

## UI stack: Avalonia 12 with NativeAOT (W5, 2026-11-02)

The Windows app is built on Avalonia 12, compiled to one native program.

Astra and Fable each voted separately for Avalonia, each at about 65% confidence. Results are in `w5-spike.md`.

**Why Avalonia:**

1. Builders have no desktop. They can test Avalonia controls with no window, and they cannot test WinUI at all. About 182 hours of UI work depend on this.
2. Overlay shapes drawn in the engine can be tested by builders in Rust. XAML shapes need a real window.
3. Measured speed does not separate them. First frame is 429 to 765 ms against 405 to 750 ms. Pointer-driven pan on a blank document is 57 to 58 fps against 58 fps.
4. The program is 46 MB against 136 MB. W19's limit is 75 MB. The WinUI number is the self-contained build, and a Store package could be smaller.

**What Avalonia costs:**

1. The engine must draw every canvas overlay. WinUI would get them from XAML.
2. The canvas needs hand-written Win32 code: a window class, Tab handoff, pen messages, dead keys.
3. Avalonia's UI frame callback runs at 41 to 44 per second on a 60 Hz screen, with every drawing route tried, even when the callback does almost nothing. So nothing that must run at full rate may depend on it. Ants run on their own thread, at 60.0 fps, and pan runs from the pointer. Avalonia's own animations may run at this lower rate.
4. Headless tests cover the controls only. In the real window the native canvas sits above the control they click, so canvas input and pixels need root's window checks.

**The named stop, and what happened:** a builder made the engine draw the Mac app's overlay set (W5b, `w5-overlay.md`). The choice was to reopen if a 200-point lasso missed 59 fps, or if the rest of the set was estimated above 20 hours.

- Speed passed. The full set held a median of 60.0 fps over 5 runs, lowest 60.0, the same as plain ants.
- The estimate tripped the line: 21 hours. So root reopened the choice on 2026-11-02 and kept Avalonia.
- 6 of the 21 hours were to prove the frame rate and compare pixels, and the frame rate is now proved. The 15 hours of drawing left are small matches to the Mac: corner rounding, line joins and handle positions. The core took a builder about 40 minutes.
- WinUI would not make this work free. It would still need these shapes written in XAML, and only a real window could test them, so builders could not check their own work.
- One early reason was wrong. W5 said WinUI's dialog step fails on the hidden desktop. With the walk corrected it passes, so the hidden desktop does not separate the stacks, and that reason is removed.

**Open, and not stopping this:** Narrator has not been heard on either stack, a real dead key and a real pen have not been tried, and a shared GPU texture was not tested as another way to host the canvas.

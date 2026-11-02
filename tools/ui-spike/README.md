# W5 UI stack test

This folder holds two small Windows shells. Both show a 1600 by 1000 engine document. The Avalonia shell hosts an HWND through `NativeControlHost`; the engine draws its canvas overlays. Its floating panel is a separate owned, topmost window. The WinUI shell hosts a `SwapChainPanel`; it draws its three sample overlays with XAML above that panel.

The shells load `op_ffi.dll` from the output folder. Build the Rust DLL first:

```powershell
cd engine
$env:CARGO_INCREMENTAL='0'; $env:CARGO_PROFILE_DEV_DEBUG='0'
cargo build -p op-ffi --release
cd ..
```

Use `%USERPROFILE%\.dotnet10\dotnet.exe` for the .NET commands. The WinUI package is `Microsoft.WindowsAppSDK` 2.6.4, the stable release dated 2026-10-27. `dotnet publish` copies the release DLL into each output folder. Run `--measure <out.json>` only on a desktop where a real window can open. A run returns nonzero on a named engine or window failure. The data contains time to first engine frame, working set, frame timestamps, resize frame count, and input events. WinUI also includes the XAML timer tick count.

The agent can run `dotnet test tools/ui-spike/tests/SpikeTests.csproj -c Release`. It tests Avalonia layout and input in headless mode. It cannot see the Rust engine canvas, because that canvas needs a native window and GPU surface. Root runs `run-window-checks.ps1` with both AOT output folders and one result folder. It writes one JSON file for each measure and UI Automation check. Root must inspect the visible screen and test Narrator manually.

`SPIKE_OVERLAY_SET=1` makes Avalonia show the W5b set: a 200-point lasso, ellipse, seven square handles, rotated crop with thirds, and caret. `run-window-checks.ps1` sets this switch for its runs. The engine copies the points once. The ants clock runs on its own thread and changes only the dash phase every 16 ms. The script now returns a failure if the first frame takes 3000 ms or more, if Avalonia draws fewer than 384 engine frames from 3 to 9.5 seconds, or if any UI Automation step fails. A local check with no window is `powershell -NoProfile -File tools/ui-spike/run-window-checks.ps1 -AvaloniaBuild x -WinUIBuild x -OutDir x -SelfTest`; it should print `WINDOW_MEASURE_SELF_TEST_PASS 4 cases`.

`python tools/ui-spike/input-check.py --self-test` checks that missing input events are named. It should print `INPUT_EVENT_SELF_TEST_PASS 3 cases`. The real input check now fails if pointer, wheel, text, Tab or either pen pressure is missing. The input scripts stop if the shell loses the foreground before a test input.

Root's window checks, all as the signed-in user:

- `run-window-checks.ps1`: measure runs and a UI Automation walk, on the visible and the hidden desktop.
- `input-check.py <exe> <result folder> <name>`: real pointer, wheel, keyboard and replayed pen input, plus two screenshots. It uses the visible desktop, and it moves the pointer and puts it back.
- `focus-walk.ps1 -Exe <exe> -Shot <png>`: presses Tab and reads the focused name at each stop, then opens the dialog.
- `pan-check.py <exe> <result json>`: moves the pointer over the canvas and reports engine frame times.

Results are in `docs/windows/w5-spike.md`. `dotnet publish` for AOT needs `vswhere.exe` on `PATH`, from `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer`.

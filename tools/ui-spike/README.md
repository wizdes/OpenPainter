# W5 UI stack test

This folder holds two small Windows shells. Both show a 1600 by 1000 engine document. The Avalonia shell hosts an HWND through `NativeControlHost`; its marching ants, crop dim, and caret come from the Rust view shader. Its floating panel is a separate owned, topmost window. The WinUI shell hosts a `SwapChainPanel`; it draws the three overlays with XAML above that panel.

The shells load `op_ffi.dll` from the output folder. Build the Rust DLL first:

```powershell
cd engine
$env:CARGO_INCREMENTAL='0'; $env:CARGO_PROFILE_DEV_DEBUG='0'
cargo build -p op-ffi --release
cd ..
```

Use `%USERPROFILE%\.dotnet10\dotnet.exe` for the .NET commands. The WinUI package is `Microsoft.WindowsAppSDK` 2.6.4, the stable release dated 2026-10-27. `dotnet publish` copies the release DLL into each output folder. Run `--measure <out.json>` only on a desktop where a real window can open. A run returns nonzero on a named engine or window failure. The data contains time to first engine frame, working set, frame timestamps, resize frame count, and input events. WinUI also includes the XAML timer tick count.

The agent can run `dotnet test tools/ui-spike/tests/SpikeTests.csproj -c Release`. It tests Avalonia layout and input in headless mode. It cannot see the Rust engine canvas, because that canvas needs a native window and GPU surface. Root runs `run-window-checks.ps1` with both AOT output folders and one result folder. It writes one JSON file for each measure and UI Automation check. Root must inspect the visible screen and test Narrator manually.

Root's window checks, all as the signed-in user:

- `run-window-checks.ps1`: measure runs and a UI Automation walk, on the visible and the hidden desktop.
- `input-check.py <exe> <result folder> <name>`: real pointer, wheel, keyboard and replayed pen input, plus two screenshots. It uses the visible desktop, and it moves the pointer and puts it back.
- `focus-walk.ps1 -Exe <exe> -Shot <png>`: presses Tab and reads the focused name at each stop, then opens the dialog.

Results are in `docs/windows/w5-spike.md`. `dotnet publish` for AOT needs `vswhere.exe` on `PATH`, from `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer`.

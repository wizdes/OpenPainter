# W3 Win32 test host

The host is a Rust Win32 program. It creates its own desktop with `CreateDesktopW` and never calls `SwitchDesktop`. It opens an HWND there for DX12 canvas tests. It prints `resize`, `switch`, and `create-destroy` results and exits with code 1 if a check fails. The builder built it but did not run it.

## Build

From the repository root in PowerShell:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_TARGET_DIR=(Join-Path (Get-Location) 'engine/target')
cargo build --manifest-path tools/win32-host/Cargo.toml -j 8
```

The program is `engine/target/debug/win32-host.exe`.

## Root run steps

1. From the repository root, run the RTX check:

   ```powershell
   $env:OP_GPU_ADAPTER='hardware'
   & .\engine\target\debug\win32-host.exe
   if ($LASTEXITCODE -ne 0) { throw 'RTX Win32 host failed' }
   ```

2. Run the WARP check:

   ```powershell
   $env:OP_GPU_ADAPTER='warp'
   & .\engine\target\debug\win32-host.exe
   if ($LASTEXITCODE -ne 0) { throw 'WARP Win32 host failed' }
   ```

3. Check that each run prints `resize: PASS`, `switch: PASS`, and `create-destroy: PASS`. Read the printed failure and frame errors if a run exits nonzero. The program keeps all windows on its private desktop and closes them before exit.

The device-removal line is `NOT RUN`. Do not count it as a pass. Microsoft's [RemoveDevice documentation](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device5-removedevice) says the call signals all devices' monitored fences. The [independent devices specification](https://microsoft.github.io/DirectX-Specs/d3d/IndependentDevices.html) also describes device singleton behavior. This run has no proof that removal is isolated from the user's other GPU work. A real removal test needs that proof first.

## Color and compiler

The HWND target reads the current display ICC path with `GetICMProfileW`. It builds the view's document-to-display table with lcms2. The Windows view uses `Bgra8Unorm` output. With Advanced Color off, Windows does not color-manage a DirectX swap chain; the app must convert to the display profile. With Advanced Color on, Windows returns an sRGB profile through legacy ICC APIs and DWM maps sRGB output to the display. See [Microsoft's ICC guidance](https://learn.microsoft.com/en-us/windows/win32/wcs/advanced-color-icc-profiles). The offscreen color check compares this view shader with direct lcms2 transforms, but a real window color check remains for root.

The DX12 path pins FXC in `windows_adapter`. FXC ships with Windows. [wgpu 27](https://docs.rs/crate/wgpu/27.0.1) says DXC needs `dxcompiler.dll`; shipping that file is outside W3. The Mac keeps its Metal backend and the original WGSL shader text.

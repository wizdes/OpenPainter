# W3 Win32 test host

The Rust host creates windows on a private desktop with `CreateDesktopW`. It does not call `SwitchDesktop`, so no test window appears on the user's visible desktop. It creates and destroys ten real HWNDs, then checks resize, display switch, two views on one HWND, a suspended view, a destroyed HWND, a forced configure error, and WARP device removal while the canvas is idle. The host enables `op-ffi/test-hooks` at build time. Shipping `op-ffi` builds do not include those hooks.

## Build and run

From the repository root in PowerShell:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_PROFILE_DEV_DEBUG='0'
cargo build --manifest-path tools/win32-host/Cargo.toml -j 8
$env:OP_GPU_ADAPTER='hardware'
& .\tools\win32-host\target\debug\win32-host.exe
$env:OP_GPU_ADAPTER='warp'
& .\tools\win32-host\target\debug\win32-host.exe
```

The executable is `tools/win32-host/target/debug/win32-host.exe`. It prints one result line for each scenario: `resize`, `switch`, `create-destroy`, `multiple-views`, `configure-error`, `suspended-view`, `destroy-before-frame`, `RemoveDevice-idle`, `RemoveDevice-retry-three-errors-second-view`, `RemoveDevice-surface-retry`, and `frame-panic-keeps-device`. Each is followed by `PASS` or `FAIL: <reason>`. The three `RemoveDevice` scenarios and `frame-panic-keeps-device` print `NOT RUN (OP_GPU_ADAPTER is not warp)` on hardware. The host never calls `RemoveDevice` on the RTX. It also prints `validation-errors: PASS (0)` or `FAIL: <count>`, `desktop-close: PASS` or a named close failure, and `W3 host: FAIL: <reason>` if the program fails. Frame errors print `op-ffi view: frame failed: <reason>` with the wgpu cause. A failed device recreation prints `op-ffi view: recreate DX12 device failed: <reason>; retry in 2 s`. A failed display profile prints the named profile or table error and says the canvas draws without display conversion.

## Builder account result on the previous commit

The 2026-11-02 runs did not present a frame on this private desktop. Both DX12 adapters returned `Validation Error: In Surface::configure: Invalid surface`. The host reports that error and retries, so normal frame waits end with `OP_ERR_BUSY`. The destroyed-HWND scenario passed and validation error counters stayed at zero. On WARP, the first-frame wait failed before the `RemoveDevice` call, so real removal and idle recovery remain **NOT RUN**. This does not prove that recovery works. The host also reported that `SetThreadDesktop` could not return to the original desktop because a resource was in use; Windows releases the private desktop when the host process exits. The windows were destroyed first.

The previous-commit RTX result lines were:

```text
resize: FAIL: op_view_wait_frames: OP_ERR_BUSY
switch: FAIL: op_view_wait_frames: OP_ERR_BUSY
create-destroy: FAIL: op_view_wait_frames: OP_ERR_BUSY
multiple-views: FAIL: op_view_wait_frames: OP_ERR_BUSY
configure-error: FAIL: op_view_wait_frames: OP_ERR_BUSY
suspended-view: FAIL: op_view_wait_frames: OP_ERR_BUSY
destroy-before-frame: PASS
RemoveDevice-idle: NOT RUN (OP_GPU_ADAPTER is not warp)
validation-errors: PASS (0)
desktop-close: SetThreadDesktop back failed: The requested resource is in use. (os error 170); Windows closes the private desktop when this process exits
W3 host: FAIL: 6 host scenarios failed
```

The previous-commit WARP result lines were the same through `destroy-before-frame`, then:

```text
validation-errors: PASS (0)
RemoveDevice-idle: FAIL: op_view_wait_frames: OP_ERR_BUSY
desktop-close: SetThreadDesktop back failed: The requested resource is in use. (os error 170); Windows closes the private desktop when this process exits
W3 host: FAIL: 7 host scenarios failed
```

Each view also printed `op-ffi view: frame failed: configure HWND surface failed: Validation Error`, followed by `Caused by:`, `In Surface::configure`, and `Invalid surface`. The forced configure test printed `op-ffi view: frame failed: configure HWND surface failed: test hook`. The destroyed window printed `op-ffi view: frame failed: HWND owner thread was not found`. Identical frame errors on one view are printed once; each retry still increases `frame_errors`.

The HWND target reads the display ICC path with `GetICMProfileW`. It builds a document-to-display table with lcms2. If the profile cannot build a table, the canvas draws without display conversion and records an error. That fallback loses accurate display color until the profile works. With Advanced Color off, Windows does not color-manage a DirectX swap chain; the app must convert to the display profile. With Advanced Color on, the legacy ICC API returns sRGB and DWM maps the output. The DX12 path uses FXC, which ships with Windows; DXC needs an additional DLL. The Mac keeps Metal and its original shader text.

## Root's previous-commit run, 2026-11-02

Run this program as the signed-in user. As `agent` over SSH, every surface fails with `Invalid surface`, because that account has no desktop session to draw in.

Root ran it as `yjli` before the fourth builder changes. RTX and WARP both exited 0, with every scenario passing, including `multiple-views`. `RemoveDevice-idle` passes on WARP: the canvas recovers from a real device removal with no new frame request. On the RTX it prints NOT RUN by design, because Microsoft's page says `RemoveDevice` signals all devices' monitored fences. The program never reads pixels back, so a black frame would pass here. W7's tests must cover this.

## Root's final run, 2026-11-02

Root ran the final program as `yjli`. It exited 0 on the RTX and on WARP. On WARP, `RemoveDevice-idle`, `RemoveDevice-retry-three-errors-second-view`, `RemoveDevice-surface-retry` and `frame-panic-keeps-device` all pass. These print NOT RUN on the RTX by design.

Windows allows one swap chain per window, after recovery too. So a second view on the same HWND gets a named error, and the first view keeps drawing. `multiple-views` and `RemoveDevice-retry-three-errors-second-view` check this.

The reviewer's stress program still crashes in 5 of 5 runs on WARP. See "Unresolved" in `test-counts.md`.

Recovery assumes one engine per process. A second engine can hold a removed device and block creation. A hung GPU that Windows never reports as removed can leave wgpu waiting forever in present or configure. Dropping a wgpu queue with unfinished work can panic; a timed-out effect device is kept until confirmed removal, or retained at engine drop with a printed reason.

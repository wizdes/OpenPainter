# paint.net capture kit (W4)

This kit uses the pinned paint.net 5.1.12 copy at `C:\agent-work\pdn-5.1.12`. The plugin is a normal effect. The driver runs paint.net on a new hidden Windows desktop and asks UI Automation to open **Effects > OpenPainter Capture**. It never takes a screen capture, calls `SendInput`, or switches the visible desktop. UI Automation on the real paint.net window has not been tested yet.

## Build and install

Run these commands as the account that can run the pin. Build does not start paint.net. Install is a separate step and writes only `Effects\CaptureEffect.dll` in the pin.

```powershell
$dotnet = Join-Path $env:USERPROFILE '.dotnet10\dotnet.exe'
$env:DOTNET_ROOT = Join-Path $env:USERPROFILE '.dotnet10'
& $dotnet build tools/pdn-capture/plugin/CaptureEffect.csproj -v:q -p:NoWarn=MSB3277
& $dotnet build tools/pdn-capture/driver/PdnCaptureDriver.csproj -v:q
& $dotnet tools/pdn-capture/driver/bin/Debug/net10.0-windows/PdnCaptureDriver.dll --install --plugin tools/pdn-capture/plugin/bin/Debug/net9.0-windows/CaptureEffect.dll
```

Close every other paint.net copy before running. The pin hands a second launch to the first process. Run preflight and then the driver from the repo root. The input must be an absolute PNG path. Output, result, and done paths must be absolute and unused. The output folder must already exist.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/preflight.ps1 -Level capture
& $dotnet tools/pdn-capture/driver/bin/Debug/net10.0-windows/PdnCaptureDriver.dll --job C:\captures\job.json --timeout-seconds 120
```

Example job:

```json
{
  "mode": "capture",
  "inputPath": "C:\\captures\\input.png",
  "outputPath": "C:\\captures\\output.png",
  "resultPath": "C:\\captures\\result.json",
  "donePath": "C:\\captures\\done.txt",
  "effects": [
    { "id": "paintdotnet.effects/Effect//Effect name from catalog", "parameters": { "Amount": 10 } }
  ]
}
```

For a catalog, use the same paths with `"mode": "catalog"` and `"effects": []`. `outputPath` then holds the catalog JSON. Each entry has a locally made ID, name, category, submenu, property defaults, ranges, choices, and `schemaAvailable`. The ID is assembled from the assembly filename, category, submenu, and localized name; it is not a paint.net ID. Always copy IDs from the catalog produced by the same pinned build and language.

The plugin writes `resultPath` for success or failure and then writes `donePath`. The driver also writes a job failure result when it fails before the plugin responds. The driver reads both and writes `manifest.json` beside the output after a successful run. The manifest records paint.net's file version, the requested effects and parameters, the PNG iCCP profile name and chunk SHA-256 or `none`, input and output SHA-256, and the first GPU name and driver version from `nvidia-smi`.

For a job with more than one effect, the driver starts a fresh pinned process for each effect and passes the previous PNG as the next input. It deletes the temporary PNGs after the job. Root must check whether this PNG round trip changes color profile handling or pixels for chained effects.

## How a capture runs, and its limits

1. The driver starts the pin on a hidden desktop with the input PNG, one effect per process.
2. UI Automation opens the effect's real menu item. The dialog stays open on the hidden desktop.
3. A posted Enter message presses OK, or a posted click when settings were typed, because Tab leaves focus on the reset button. UI Automation cannot do this: the menu invoke stays in progress while the modal dialog is open, so later calls into the process wait.
4. UI Automation opens **OpenPainter Capture**, and the plugin writes the current layer as a PNG. It renders nothing itself.

Limits, measured on 2026-11-02:

- **Number settings work; other kinds do not yet.** A job gives `"parameters": { "Radius": 7 }`. The name is the label shown in the dialog. The driver types the value by window message, presses Tab, and reads the box back. A wrong label, a value that does not stick, or an effect with no dialog fails by name. Lists, check boxes, color wheels, curves and random seeds cannot be set yet.
- **The plugin API route did not work.** `IEffectsService2` lists 54 built-in effects, and each offers `IBitmapEffectRenderer`. `SetToken` refuses null, and `IEffectConfigForm.Token` throws "Operation is not valid", even after `SetEffect` and with the dialog open. Built-in effects arrive as closed proxies, so no parameter schema is public. The catalog therefore has names and categories, with no parameters.
- **Effects with a random seed differ between runs.** Add Noise and Clouds gave different hashes on two runs with default parameters. They need a fixed seed, or a comparison that does not use hashes, in W14.
- A failed menu invoke can print "Invoke failed: Catastrophic failure" when the dialog closes. The run still reports its own result.
- Each capture takes about 16 s.

# paint.net capture kit (W4)

The driver starts paint.net 5.1.12 on a hidden Windows desktop. It opens a built-in effect from the pinned catalog, sets number boxes by their exact dialog labels, then asks the plugin to save the current surface. Root must run the real paint.net checks; the `agent` account cannot start it. No image goes in this public repo.

## Build and test

From the repo root, with .NET 10 on `PATH`:

```powershell
dotnet test tools/pdn-capture/tests/PdnCaptureTests.csproj -v:q
dotnet build tools/pdn-capture/driver/PdnCaptureDriver.csproj -v:q
dotnet build tools/pdn-capture/plugin/CaptureEffect.csproj -v:q -p:NoWarn=MSB3277
```

The driver pins file version `5.112.9563.32325` and executable SHA-256 `73697e348b3ab3e4168befa5ceae6b04d6fcf5a4d4e83f301f6a1257cef08222`. It refuses a changed executable. Root should verify these values against its clean pin before the real run.

## Real run by root

Run as the account that can start the pin, with the user's paint.net closed. The driver checks again before and after every launch. If another paint.net process appears, it stops only its own PID.

paint.net ignores `LOCALAPPDATA` and `APPDATA`: root set both to a private folder, and the folder stayed empty. So a capture cannot be kept out of the user's `%LOCALAPPDATA%\Paint.NET`. paint.net does two things there, and the driver undoes both:

- It creates a scratch folder under `SessionData`, and a stopped process leaves it behind. The driver deletes only entries that are new and inside a session folder that is new.
- It rewrites `Optimization\Startup.N.profile`, a speed cache with no settings or pictures. The driver saves that folder before the run and puts back the same bytes and times.

The driver then compares every file (size and time), every folder and every value under `HKCU\Software\paint.net` with the state before the run, and fails by listing anything that differs. If another paint.net copy is running at that point, it undoes nothing, so the check names the changes.

paint.net settings for this copy live in `PaintDotNet.AppSettings.json` in the pin, because the pin runs in portable mode.

```powershell
dotnet tools/pdn-capture/driver/bin/Debug/net10.0-windows/PdnCaptureDriver.dll --install --plugin tools/pdn-capture/plugin/bin/Debug/net9.0-windows/CaptureEffect.dll
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/preflight.ps1 -Level capture
dotnet tools/pdn-capture/driver/bin/Debug/net10.0-windows/PdnCaptureDriver.dll --job C:\captures\identity-job.json --timeout-seconds 120
dotnet tools/pdn-capture/driver/bin/Debug/net10.0-windows/PdnCaptureDriver.dll --job C:\captures\effect-job.json --timeout-seconds 120
```

Use a new output directory for every run because each successful run writes `manifest.json` there. Create those directories before running. Example effect job:

```json
{
  "mode": "capture",
  "inputPath": "C:\\captures\\input.png",
  "outputPath": "C:\\captures\\effect\\effect.png",
  "resultPath": "C:\\captures\\effect\\effect-result.json",
  "donePath": "C:\\captures\\effect\\effect-done.txt",
  "effects": [
    { "id": "paintdotnet.effects.gpu/effect/blurs/gaussian blur", "parameters": { "Radius": 7 } }
  ]
}
```

For the identity job, use `"mode": "identity"`, `"effects": []`, and different output, result and done paths. The output and manifest paths must also differ from the input and from each other. Identity mode checks the plugin's source-surface hash against the decoded PNG. A successful capture writes `ok: true`, source width, height, reported layer count and surface hash to the result JSON. The driver checks the PNG signature and dimensions, input file hash before and after, and the result fields. It writes `manifest.json` beside the output. The manifest records the pin, driver, plugin, catalog, input and output hashes, number box values, unread list and check box names, ICC data and the installed GPU reported by `nvidia-smi`. That GPU report does not prove which GPU paint.net used.

The four no-dialog effects are Auto-Level, Black and White, Invert Alpha and Invert Colors. All other effects must open and close a dialog. An invoke error fails by effect ID. The known `Catastrophic failure` message is accepted only after that same dialog opened and closed, because the observed dialog path and plugin result then both completed. A missing dialog cannot turn that error into success.

## W14 work left

- Set and read list boxes, check boxes, color wheels and curves.
- Fix random seeds or choose a comparison rule for random effects.
- Add `targets.toml` rows and the M54 loader, including its evidence checks.
- Run real captures for one and many effects. Check color profile and pixel behavior across chained PNG round trips.
- Confirm the plugin's reported layer count through paint.net's public API; the current render callback exposes a surface, not a document layer list.

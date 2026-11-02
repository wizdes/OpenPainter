# Parity with the references

Started in W4. Every row ends as "same", "better (why)" or "left out on purpose (why)". The Mac app is the reference for UI, and paint.net 5.1.12 for pixels (`PLAN.md` section 3.3). Effects need ±1/255, or they go on the user's list.

## Open items for the user

| Item | What is known |
| --- | --- |
| Mandelbrot Fractal | Differs from the legacy reference by up to 23 of 255 levels on Windows. A small floating-point difference grows over hundreds of iterations. A fix may change the Mac's output, and no Mac run is possible yet. |

## Capture route check (W4, 2026-11-02)

Input: one 64 by 64 opaque gradient PNG, with default parameters, run twice on a hidden desktop. Images are in the private repo `wizdes/OpenPainter-oracle`. This checks the capture route only. It does not compare anything with our engine; W14 does that.

| Family | Effect | Two runs | What this proves |
| --- | --- | --- | --- |
| Adjustment | Auto-Level | Same hash | The route works, and the effect changes pixels |
| Artistic | Ink Sketch | Same hash | Same |
| Blurs | Bokeh Blur | Same hash | Same |
| Blurs | Gaussian Blur | Same hash | Same |
| Color | Quantize | Same hash | Same |
| Distort | Bulge | Same hash | Same |
| Photo | Glow | Same hash | Same |
| Stylize | Edge Detect | Same hash | Same |
| Object | Drop Shadow | Same hash | The route runs. The hash equals Rotate / Zoom's, so most likely nothing changed on this opaque input. W14 needs an input with transparency. |
| Layers | Rotate / Zoom | Same hash | The route runs. The default most likely does nothing, as above. |
| Noise | Add Noise | **Different** | Not repeatable with defaults. A random seed is the likely cause, not proven. |
| Render | Clouds | **Different** | Same as Add Noise |

## Setting effect parameters (W4, 2026-11-02)

The driver can now set number settings in a real effect dialog. This was the largest risk to W14.

The first W4 fix round added the driver test project and the real runs below. The second fix round has 69 passing kit tests on the builder machine; root has not repeated the paint.net jobs yet. The driver checks full catalog IDs, exact number labels and values, dialog presence and its OK button, process ownership throughout the wait, unused paths before a chain, PNG dimensions, input hash and the job deadline. It rejects duplicate effect fields and parameter names. Posterize lists Red, Green, Blue and Alpha as number labels.

paint.net ignores `LOCALAPPDATA` and `APPDATA`. The driver removes only its own new session entries and restores only `Startup.<digits>.profile` files. It refuses folder links during deletion and does not follow them when recording the user folder. Its final check compares file bytes by SHA-256, attributes, size and time, as well as registry keys and values. It keeps multi-string registry array boundaries. If another paint.net copy is seen, it skips cleanup and restore and fails by name. A copy that appears and exits between two looks inside one poll remains a limit. A failed run writes `ok: false` to a result path that was unused when the run began. See the README for the root run commands.

Root's real run on paint.net 5.1.12, 2026-11-02, on a 64 by 48 input with three alpha levels:

| Job | Result | Pixels against the input |
| --- | --- | --- |
| Identity, no effect | Pass; plugin surface hash equals decoded input | 0 changed, and the PNG file is the same bytes |
| Gaussian Blur, Radius 7 | Pass; manifest reads Radius 7, Gamma Boost 0, Quality 4 | 2962 of 3072 changed |
| Invert Colors, no dialog | Pass | All 3072 inverted, alpha unchanged |
| Auto-Level, no dialog | Pass | 3071 changed, alpha unchanged |
| Black and White, no dialog | Pass | 3071 changed, alpha unchanged |
| Invert Alpha, no dialog | Pass | All alpha changed |
| Posterize, default 16 levels | Pass; its dialog opened and closed | 3064 changed |
| Chain: Invert Colors, then Gaussian Blur Radius 3 | Pass | All changed |
| Radius 99999 | Fails by name: `'Radius' shows '300.0'` | No output |
| Unknown ID, wrong label case, output path equal to result path | Each fails by name before launch | No output |

After each job, the user's `SessionData` folder was empty and the check passed. Root repeated these jobs on the second fix round, and each passed or failed the same way, with the same output hashes.

Root then ran all 54 effects through the final driver, one at a time, with default settings and a press of the real OK button: 54 of 54 finished. 45 ran before the last small fix and 9 after it, because those 9 have two number boxes under one label, such as a center point's X and Y. The manifest records the second box as `Label#2`, and setting a shared label fails by name until W14 adds a way.

Nine effects left the pixels unchanged at their defaults: Brightness / Contrast, Curves, Exposure, Highlights / Shadows, Hue / Saturation, Temperature / Tint, Straighten, Levels and Rotate / Zoom. Their defaults may do nothing, but this run does not prove it. W14 must capture each with settings that change pixels.

NOT RUN: another paint.net copy opening during a capture. Only its decision function has a test, because opening one would put a window on the user's screen.

| Check | Result |
| --- | --- |
| Gaussian Blur, Radius 7, two runs | Same hash, `c6d77420f6865b9a`, and the dialog screenshot shows 7.0 |
| Gaussian Blur, default Radius 2 | Different hash, `487636cc678e2210` |
| A label the dialog does not have | Fails with "Dialog has no label named" |

A survey opened every effect once, on 2026-11-02. All 54 ran. Four have no dialog: Auto-Level, Black and White, Invert Alpha and Invert Colors. The survey also listed Posterize, wrongly: it has a dialog with four number boxes. Sixteen have a list box. Each effect's controls and labels are in `pdn-5.1-controls.json`. Only Gaussian Blur's number setting has been set and tested with real paint.net so far. Lists, check boxes, color wheels, curves and random seeds still need W14. W14 also needs `targets.toml` rows, the M54 loader, and a public way to confirm document layer count; the plugin currently reports one layer from a surface callback.

## Fallback list for W14

- **Add Noise, Clouds, and other effects with a seed:** need a fixed seed, or a comparison that does not use hashes.
- **All effects:** number parameters can be set by exact dialog label. W14 must finish list, check box, color and curve settings, then verify them with real captures.
- **Next routes if that fails:** the OpenPDN oracle, for `legacy-3.36` rows only.

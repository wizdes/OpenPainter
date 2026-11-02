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

The W4 fix round adds a driver test project with 49 passing cases. The driver now checks the full catalog ID, exact number labels and number values, final number box values, dialog open and close, process ownership, job paths, PNG dimensions, input hash and job deadline. paint.net ignores `LOCALAPPDATA` and `APPDATA`, so the driver removes the run's own session scratch files, restores the speed cache, then fails by name if the user's folder or registry values differ in any way. The README says how.

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

After each job, the user's `SessionData` folder was empty and the check passed. NOT RUN: another paint.net copy opening during a capture. Only its decision function has a test, because opening one would put a window on the user's screen.

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

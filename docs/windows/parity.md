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

| Check | Result |
| --- | --- |
| Gaussian Blur, Radius 7, two runs | Same hash, `c6d77420f6865b9a`, and the dialog screenshot shows 7.0 |
| Gaussian Blur, default Radius 2 | Different hash, `487636cc678e2210` |
| A label the dialog does not have | Fails with "Dialog has no label named" |

A survey opened every effect once, on 2026-11-02. All 54 ran. 5 have no dialog. 16 have a list box, which cannot be set yet. Each effect's controls and labels are in `pdn-5.1-controls.json`. Only one effect's number setting has been tested so far. Lists, check boxes, color wheels, curves and random seeds are still W14's work.

## Fallback list for W14

- **Add Noise, Clouds, and other effects with a seed:** need a fixed seed, or a comparison that does not use hashes.
- **All effects:** parameters cannot be set yet. Typing into the dialog by window message is the next thing to try.
- **Next routes if that fails:** the OpenPDN oracle, for `legacy-3.36` rows only.

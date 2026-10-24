# Third-party code and credits

OpenPainter ports algorithms from these MIT-licensed projects. Each ported file names its source in a header comment.

- **Paint.NET 3.36.7** (OpenPDN mirror, https://github.com/rivy/OpenPDN), MIT License, Copyright (C) dotPDN LLC, Rick Brewster, Tom Jackson, and contributors. Effects, adjustments, blend operations, resampling. "Paint.NET" is a trademark of dotPDN LLC; OpenPainter is not affiliated with or endorsed by dotPDN LLC.
  The zoom presets and step rules in `mac/OpenPainterKit/Sources/ZoomPresets.swift` come from `src/Core/ScaleFactor.cs` (OpenPDN commit cca476b0df2a2f70996e6b9486ec45327631568c); Zoom to Window, Zoom to Rectangle and the Zoom tool's click behavior follow `src/Data/DocumentView.cs`, `src/DocumentWorkspace.cs` and `src/tools/ZoomTool.cs`.
- **Pinta** (https://github.com/PintaProject/Pinta), MIT License, Copyright (C) Jonathan Pobst and contributors. Reference test images in `tests/oracle/pinta/`.
- **libmypaint** (https://github.com/mypaint/libmypaint), ISC License. Brush dab spacing and smoothing ideas.

GPL projects (LazPaint, KolourPaint) were studied for behavior only. No code was copied from them.

## Rules for porting (from the Step 0 review)

- OpenPDN's MIT grant covers its source code only. Its `.resx` and `.resources` files (menu and status text) and its `.png` artwork are CC BY-NC-ND. Never copy those strings or images.
- Never port the GPC polygon clipper (`SystemLayer/GpcWrapper`, used by `PdnGraphicsPath` and `PdnRegion`). It forbids commercial use. OpenPainter uses `i_overlay` for polygon booleans.
- PatchMatch (content-aware fill) is implemented from the paper (Barnes et al., SIGGRAPH 2009). The project's downloadable sample code is research-only and is never used.
- `tests/oracle/ALLOWLIST.md` lists every OpenPDN and Pinta file that may be ported or used as an oracle. A file not on the list is not used.

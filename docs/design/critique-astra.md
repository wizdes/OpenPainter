Build **Classic**, with a shorter top area and explicit rules that keep floating panels clear of the fitted image. Compact offers a useful space-saving idea, but its default docked appearance conflicts with the chosen product direction. Unified adds visual polish without solving the main layout problem.

This is design advice, not implementation approval. I inspected all six supplied PNGs and the HTML. Smaller-window findings below are CSS calculations; fresh browser rendering failed.

1. **High severity: Classic and Unified cannot keep their panels clear at smaller sizes.**

   At 1440 × 900, both look comfortable because the sample image happens to fit between the panels. Neither has a layout rule that protects that space.

   At a **1280 × 800 mockup viewport**, the CSS places Classic’s 792 px image from x=244 to x=1036. Colors reaches x=261; Layers starts at x=1008. Those panels therefore cover strips approximately **17 px and 28 px wide**. Unified’s corresponding overlaps are approximately **23 px and 37 px**.

   The panels also collide vertically. Using their supplied rendered heights, Classic’s Colors overlaps Tools by approximately **52 px**; History overlaps Layers by approximately **8 px**. Unified produces similar collisions. Expanding Colors, adding history entries, or enlarging text makes this worse.

   These viewport calculations include the mock desktop menu and window margins, so they are slightly tighter than a literal 1280 × 800 native window. They still expose the missing rule: fixed corner positions cannot guarantee usable panels.

   Compact reserves separate panel columns. Its calculated stage is **845 × 569 px** at that viewport, containing the 760 × 482 px sample image. That is the strongest layout decision across the three.

   Evidence: `.stage`, `.doc`, and panel positioning in [classic.html:9](/Users/yili/work/Projects/.op-builds/m11/docs/design/mockups/classic.html:9), with overrides at lines 11 and 13.

2. **High severity: appearance changes alter the selected paint color.**

   All three change the primary swatch from **`#CF684D` to `#D5795F`** in dark mode. The displayed hex value remains **`#CF684D`**.

   A photo editor must distinguish interface colors from document colors. The selected paint color must remain constant when appearance changes. Change its surrounding border if needed.

   Evidence: `--swatch` in [classic.html:7](/Users/yili/work/Projects/.op-builds/m11/docs/design/mockups/classic.html:7) and line 8; fixed hex text at line 43. The same code appears in all directions.

3. **High severity: several important controls are too small or too faint.**

   The normal text colors are sound. Calculated primary text contrast is **14.96:1 light / 9.87:1 dark**; secondary text is **5.84:1 / 6.49:1**. Selected-row text also passes 4.5:1.

   The exceptions matter:

   | Element | Finding | Change |
   |---|---|---|
   | Palette cells | Classic **15.83 × 11 px**, Unified **16.67 × 11 px**, Compact **13.25 × 11 px**; only 2 px gaps | Use at least 24 × 24 targets |
   | Future History rows | Calculated contrast approximately **2.50:1 light / 3.17:1 dark** | Keep readable text; mark future steps separately |
   | Dark antialiasing checkmark | White on `#76B8FF`: **2.09:1** | Use dark ink on the light blue fill |
   | Unsaved asterisk | **4.03:1 light / 2.60:1 dark** against the thumbnail-strip background | Use primary text color and a clear modified marker |
   | Layer selection | Background differs from normal panel by only **1.14:1 / 1.19:1** | Add a distinct edge or marker |
   | Panel close, layer visibility, slider handles | Small drawings with no defined larger target | Specify actual hit rectangles |

   The palette fails the 24 px target benchmark even with its spacing considered. A small icon alone does **not** prove a small target; several mock controls are plain spans, so their eventual native target sizes remain unspecified. [W3C target-size guidance](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)

   Future History rows should remain readable if users can click them to restore later states. Truly disabled controls have a contrast exception, but “undone” does not mean “unavailable.” [W3C contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)

4. **Medium severity: the shared control design weakens recognition and state clarity.**

   All four panels are visible, but only the Tools visibility button appears selected. That makes the top-right group look like a choice between panels.

   Tools also uses the same brush drawing as the active Paintbrush tool. History resembles another Undo command. Layers, Colors, and several tools use small text glyphs with inconsistent weight. Original icons need consistent drawing and clear meaning; they should not require guessing.

   The Colors wheel has no visible selection marker. Layers depends mainly on background color to show selection. The selected tool likewise relies on tint and a faint border. Add shape-based state cues.

   The active document has no close affordance, long-name treatment, or overflow treatment. These omissions matter more than panel translucency. Paint.net’s thumbnail list supports switching, reordering, and modified-state markers; the build should preserve those behaviors. [Paint.net main-window documentation](https://paint.net/doc/latest/MainWindow.html)

5. **Medium severity: each direction has a different layout weakness.**

   | Direction | What works | What fails |
   |---|---|---|
   | **Classic** | Strongest paint.net mental model. Commands, tool options, thumbnails, image, and status have distinct places. Floating panels are visibly movable. Alignment and grouping are easy to follow. | Its top area consumes **208 px**. The 83 px thumbnail band is generous while useful panel labels are only 10–11 px. Tools and Colors have mismatched widths; Layers and History match better. Small-window panel collisions are serious. |
   | **Unified** | Softer borders and larger panel headers give a calmer appearance. The title has more presence. It preserves four floating panels. | The 470 px tool-options block wraps into two rows beside document thumbnails. Tool settings and document navigation compete within one band. Its 56 px title row plus 53 px command row spends height on mostly empty space. Larger panels reduce side clearance. Translucent utility surfaces add variability without helping photo editing. |
   | **Compact** | Best protection of the image. Stable side columns, smaller thumbnails, and a **168 px** top area save space. The default sample remains clear at 1280 × 800. | Looks like a docked editor, despite the floating-panel decision. Full-height panel backgrounds create large unused areas beneath Tools and Layers. Fixed splits waste space while squeezing Colors. The permanent “drag a panel header” instruction adds noise. Its 24 px status bar leaves little room for usable zoom controls. |

   **Light appearance:** Classic is readable but box-heavy. Unified is softer, yet its combined options/thumbnail band lacks a clear hierarchy. Compact’s broad pale side columns compete with the photograph.

   **Dark appearance:** All three retain readable normal text, but the blue-gray surfaces make them feel more alike. Future History entries become particularly weak. Compact has less visual glare; Unified’s transparency makes exact contrast less predictable over different image content.

   **Mac conventions:** Separate title and toolbar rows are valid on Mac; Classic is not wrong merely because it uses both. Unified’s taller chrome does not itself make it more native. All three should use actual system window controls and native toolbar behavior. Their right-side panel-close marks currently resemble web cards more than native utility windows. Apple advises using system window frames and providing menu equivalents for toolbar commands. [Apple windows guidance](https://developer.apple.com/design/human-interface-guidelines/windows.md), [Apple toolbar guidance](https://developer.apple.com/design/human-interface-guidelines/toolbars.md)

   One correction to the README: Unified’s total top area is **179 px**, versus Classic’s **208 px**. Its individual title and command rows are taller, but the complete top area is **29 px shorter**.

6. **Medium severity: these are three layout variants, not three fully explored visual directions.**

   After normalizing the page title and `data-direction`, the three HTML files are **identical**.

   The real differences are:

   | Decision | Classic | Unified | Compact |
   |---|---|---|---|
   | Top arrangement | Four separate rows | Options and thumbnails share a band | Four shorter rows |
   | Panel placement | Floating corners | Floating corners, farther inset | Edge columns |
   | Panel finish | Opaque, rounded, shadowed | Translucent, more rounded | Square, no shadow |
   | Sample image | 792 × 502 | 760 × 482 | 760 × 482 |
   | Stage protection | None | None | Reserved side widths |

   Typography, colors, icons, tool grouping, layer rows, history rows, status information, and nearly all content are the same. Classic versus Unified mostly tests top-band arrangement and surface treatment. Compact tests a genuinely different panel arrangement.

7. **Concrete changes for the Classic build.**

   These are proposed starting dimensions, expressed as logical points for the native app.

   | Element | Build recommendation |
   |---|---|
   | Top area | Target approximately **172 pt**: title 36, commands 36, options 36, thumbnails 64. Let native title-bar metrics take precedence. |
   | Command buttons | **28 × 28 pt** targets, **18 pt** drawings, **6 pt** gaps; separate functional groups by **12 pt**. |
   | Tool options | **28 pt** fields, **8 pt** label-to-field spacing, **16 pt** between groups. Make Paintbrush a tool picker; provide editable width and increment/decrement controls. |
   | Thumbnail strip | Borrow Compact’s economy: **56 × 40 pt** previews, **12 pt** filenames, **8 pt** gaps. Add a **2 pt** active outline, modified marker, and accessible close action. Scroll or provide overflow instead of shrinking previews indefinitely. |
   | Floating panels | Keep Classic’s floating model. Start with Tools **184 pt**, Colors **224 pt**, Layers/History **240 pt** wide. Use **12 pt** outer clearance and **12 pt** between stacked panels. |
   | Image fitting | Borrow Compact’s protected image space. Fit into the clear rectangle between visible panels with **16 pt** image clearance. At 100% zoom, preserve scale and allow panning; do not silently shrink the image when a panel opens. |
   | Panel height | Default Colors to its compact state, about **280 pt** high. Bound expanded content to available height and scroll it. Give History a scrollable list. Never allow panels to overlap each other on resize. |
   | Palette | **8 columns**, **24 × 24 pt** cells, **2 pt** gaps. Show a small default set; put all 96 colors in the expanded scrollable view. |
   | Typography | Panel titles **12–13 pt semibold**; rows and fields **12 pt**; secondary data at least **11 pt**. Remove 10 px action labels. |
   | State markers | Add a **2 pt** selection edge plus a non-color marker where needed. Show all four panel buttons as pressed when all panels are visible. |
   | Surfaces | Keep opaque panels and the existing primary/secondary text colors. Try neutral stage grays **`#777777` light / `#252525` dark**. Keep document colors independent of appearance. |
   | Window chrome | Use native window and utility-panel controls. Remove “— OpenPainter,” the permanent shortcut legend, and the mock appearance button from the production title bar. Keep F5–F8 in Window-menu commands and tooltips. |
   | Status bar | **28 pt** high, editable zoom, explicit Fit and 100% actions, and a slider with a **28 pt** interaction area. Preserve zoom first when space becomes tight. |

   Take **density and protected image space from Compact**. Take **quieter separators from Unified**, but leave its wrapped options band and transparent panel bodies behind.

**Uncertain or untested points:** Native dragging, keyboard focus, VoiceOver, F5–F8 behavior, increased contrast, reduced transparency, thumbnail overflow, and actual photo-editing tasks were not tested. Most HTML controls are static drawings. Unified’s final contrast over arbitrary photographs needs rendered measurement. The 66% zoom label also disagrees with its 760 px sample width; that may be placeholder content rather than intended behavior.

**Checks and failures:** `git status --short` returned empty before and after the review. `python3 "$TMPDIR/op-m11-astra-advice/check.py"` exited 0 and produced the contrast, normalized-file comparison, and geometry calculations above. The initial Node probe failed with `MODULE_NOT_FOUND`. The subsequent command, `perl -e 'alarm 55; exec @ARGV' node "$TMPDIR/op-m11-astra-advice/measure.cjs" < /dev/null`, exited 1 because Chrome could not create its profile/socket directory. No fresh 1280 × 800 screenshots or application tests ran. Apple’s HTML pages exposed only a JavaScript notice; the web reader also rejected their Markdown content type. A direct Python URL fetch successfully read both Markdown pages.

Classic is the strongest starting point. Fix panel placement, exact color display, and small controls before polishing surfaces. No project files changed; smaller-window rendering and native interaction checks remain open.

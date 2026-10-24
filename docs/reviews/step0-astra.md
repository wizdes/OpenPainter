The fixed product choices are workable. The plan needs changes before M03: its pixel rules cannot meet its reference tests, its selection model loses soft masks, and its memory policy exceeds this Mac’s capacity.

This is design advice, not implementation or review approval. I changed no project files.

## 1. Blocking flaws

### 1.1 The pixel contract cannot satisfy the reference contract

**What fails:** “Every kernel” using premultiplied float32 and one final rounding rule cannot also reproduce OpenPDN’s intermediate integer rounding.

**Evidence:** [ARCHITECTURE.md:70](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:70) requires both. I reproduced this Normal blend:

| Input/output | RGB | Alpha |
|---|---:|---:|
| Bottom pixel | 0 | 2 |
| Top pixel | 255 | 64 |
| OpenPDN integer calculation | 251 | 65 |
| Ordinary floating-point source-over, rounded at output | 249 | 65 |

The difference is **2/255**, beyond the fixed accuracy requirement. OpenPDN rounds alpha products before dividing the color result. Its multiplication/addition/shift table implements integer division. Current Pinta instead has explicitly premultiplied blend helpers. [OpenPDN blend source](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/Data/UserBlendOps.Generated.cs), [Pinta blend helper](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Core/Algorithms/BlendOpHelper.cs).

**Fix:** Give each operation an explicit arithmetic contract:

```text
input alpha representation
working color space
intermediate precision and quantization points
border rule
output alpha representation
rounding rule
reference version
```

Float32 remains the working precision. Where compatibility requires intermediate byte rounding, emulate those stages explicitly. Do not require one result to match conflicting references. The documented paint.net 5.1 precedence must select the expected result.

### 1.2 The blanket encoded-color rule is wrong for paint.net 5.1

**What fails:** Effects and resampling will miss captures by much more than one byte.

**Evidence:** [ARCHITECTURE.md:158](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:158) says all blending and effects use profile-encoded values. Paint.net 5.1 documents a default `WorkingSpaceLinear` effect context. Its API also offers unchanged working-space and sRGB contexts. Resize exposes Gamma Correction. Thus, the correct space varies by operation and setting. [5.1 release explanation](https://blog.paint.net/2024/11/12/paint-net-5-1-is-now-available/), [effect color contexts](https://paintdotnet.github.io/apidocs/api/PaintDotNet.Effects.Gpu.GpuEffectColorContext.html), [Resize documentation](https://www.getpaint.net/doc/latest/ImageMenu.html).

A half-black, half-white average is approximately **188** in sRGB when averaged in linear light, versus **128** in encoded values.

**Fix:** Add `WorkingSpaceEncoded`, `WorkingSpaceLinear`, and any captured operation-specific context to `EffectSpec`. Establish each setting in M02 before porting the effects. This preserves the user’s color-management and precision decisions.

### 1.3 Premultiplying every operation destroys meaningful hidden color

**What fails:** Invert Alpha, straight-alpha wand comparisons, and some round trips lose RGB beneath zero alpha.

**Evidence:** Straight RGBA `(255,0,0,0)` becomes `(0,0,0,0)` when premultiplied. The original red cannot be recovered. Paint.net explicitly distinguishes transparent pixels by RGB in its wand’s Straight tolerance mode. [Magic Wand documentation](https://www.getpaint.net/doc/latest/MagicWand.html).

The tile definition also says `None` means “fully transparent.” That must not mean every tile whose alpha bytes are zero.

**Fix:**

1. Define `None` as one exact implicit pixel value, such as transparent black.
2. Preserve other zero-alpha RGB values in allocated tiles.
3. Let channel adjustments operate on straight pixels.
4. Use premultiplication for mixing and filtering where required.
5. Compare all bytes in persistence/history tests. Ignore zero-alpha RGB only in appearance comparisons where justified.

### 1.4 The residency policy cannot pass the stated memory stress test

**What fails:** The app exhausts memory before history compression helps.

**Evidence:** [ARCHITECTURE.md:76](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:76) keeps every current-state tile resident. Its 100 MP × 20-layer stress case requires **8,000,000,000 bytes, or 7.45 GiB**, for RGBA8 alone. This excludes macOS, Swift, previews, composite caches, and GPU resources.

One 24 MP RGBA float32 buffer adds **366 MiB**. Multiple complete effect buffers quickly consume the remaining memory.

**Fix:** Page current-state tiles too. Keep only actively used tiles pinned. Budget document pixels, history, previews, CPU scratch, GPU caches, and upload/readback buffers together.

Use stable tile IDs with separate storage records:

```text
TileId → immutable pixel content
       → Resident | Compressed | Spilled
       → pin count, byte count, checksum, last-use counter
```

Do not infer residency eligibility from `Arc::strong_count`; rendering jobs and UI snapshots also hold references.

### 1.5 Bitmap-to-path tracing cannot preserve soft selections

**What fails:** Remove Background loses hair coverage. Wand boundaries change. A noisy selection can produce millions of vertices.

**Evidence:** [ARCHITECTURE.md:84](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:84) makes polygons authoritative and traces imported masks with marching squares plus simplification. A contour at one threshold cannot encode arbitrary coverage values throughout a mask. Simplification can also remove one-pixel islands and bridges.

**Fix:** Use:

```rust
enum SelectionSource {
    Geometry(Arc<PolygonSet>),
    Coverage(Arc<MaskGrid>),
}
```

Preserve the original source. Derive contours for marching ants only. Keep geometric Boolean operations geometric; define separate, explicit rules for combining imported coverage masks.

Also distinguish:

- No selection: ordinary editing affects the full applicable area.
- Active empty selection: editing affects nothing.
- Select All: active selection covering the canvas-sized layer.

### 1.6 A fixed halo is insufficient for the promised effects

A **halo** is the extra source area read around an output tile.

**What fails:** Levels, Auto-Level, Quantize, global warps, and PatchMatch cannot be expressed correctly as independent tile kernels with `halo(params)` alone.

**Evidence:** [ARCHITECTURE.md:102](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:102) has no whole-image analysis stage, pass graph, or mapped source-region request. Histograms require global reduction. A warp may read a distant source region. PatchMatch repeatedly updates shared state.

**Fix:** Introduce an execution plan before M18:

```text
prepare(snapshot, selection, params)
required_input(output_rect, prepared_state)
passes: pointwise | neighborhood | reduction | warp | iterative
render_tile(...)
finalize(...)
```

Specify whether a selection restricts output only or also restricts analysis. Usually an effect must still read pixels outside the selection to avoid a false edge.

### 1.7 Cancellation lacks the identity checks needed to prevent stale commits

**What fails:** A slow preview can overwrite a newer preview, commit to a changed layer, or repopulate tiles after cancellation.

**Evidence:** [ARCHITECTURE.md:117](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:117) describes cancelling jobs and reusing finished tiles, but provides no document revision, preview generation, or commit precondition.

**Fix:** Every job needs:

```text
document ID
base document revision
target layer ID
selection revision
preview session ID
parameter generation
```

Accept results only when all identities still match. Commit through the document actor as one atomic state replacement. On OK, finish missing tiles from the same generation. Never mix generations.

Move the shared preview transaction mechanism before M14/M20/M22/M23/M28. M32 is too late.

### 1.8 Several checkpoints and dependencies cannot work in their stated order

**What fails:** Early checkpoints demand features that do not exist yet.

**Evidence:**

- CP1 is after M05, but the mandatory checkpoint suite includes the Swift app, crop, Curves, Gaussian, painting, and saving.
- M09 needs M06’s presenter, but lists only M07 and M08.
- Phase 1 must open PNG, while image I/O arrives at M11.
- M07 tests Invert before M18 unless M03 explicitly supplies a testable primitive.
- M14 needs preview transactions before M20/M32.
- M44 depends only on M39/M40 despite promising every operation, including later retouch work.
- M50 does not explicitly depend on M47 robustness.
- The effects lane feeds M45 through M44, so it is not safely “off the critical path.”

See [MILESTONES.md](/Users/yili/work/Projects/OpenPainter/docs/MILESTONES.md) and [PLAN.md:169](/Users/yili/work/Projects/OpenPainter/docs/PLAN.md:169).

**Fix:** Add a capability manifest per checkpoint. A required capability must run and pass; future capabilities must be explicitly absent, not silently skipped. Move minimal PNG decoding into the foundation work. Add the missing dependencies and recalculate the schedule.

### 1.9 The native-format promise exceeds the defined format

**What fails:** A file can round-trip in OpenPainter yet fail to qualify as baseline OpenRaster or render differently elsewhere.

**Evidence:** M12 lists layer PNGs, XML, and ICC. OpenRaster also requires an uncompressed first `mimetype` entry, a thumbnail, and `mergedimage.png`. Its layer order is topmost first. Custom blend names do not make other editors understand those formulas. [File layout](https://www.openraster.org/baseline/file-layout-spec.html), [layer stack](https://www.openraster.org/baseline/layer-stack-spec.html).

**Fix:** Specify the complete archive, extension rules, ICC location, unknown-mode fallback, and version handling. Preserve a correct merged image. Test interoperability using actual readers. Promise exact OpenPainter round trips; describe foreign editing of custom modes as potentially approximate.

Move atomic save and basic crash-safe recovery design into M12. M47 should stress those mechanisms, not introduce them.

### 1.10 The source-license description is too broad

**What fails:** Blind vendoring can put excluded resources or restricted code into an MIT project.

**Evidence:** OpenPDN’s actual license has exceptions for artwork/resource assets and GPC code. The PatchMatch project’s downloadable implementation is described as noncommercial research code. [OpenPDN license](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/Resources/Files/License.txt), [PatchMatch project page](https://pixl.cs.princeton.edu/gfx/pubs/Barnes_2009_PAR/index.php).

**Fix:** M02 needs a file-level allowlist and attribution record. Port the permitted algorithm files. Keep the planned original icons and replacement polygon library. Implement PatchMatch from the paper; do not import its restricted example package.

### 1.11 FFI panic handling does not cover the planned execution model

**What fails:** A worker panic can bypass the exported function’s `catch_unwind`. Shutdown can race callbacks or retained views.

**Evidence:** Commands execute asynchronously after `op_dispatch` returns. The listed interface lacks explicit view detach/document close operations and callback shutdown guarantees. [ARCHITECTURE.md:130](/Users/yili/work/Projects/OpenPainter/docs/ARCHITECTURE.md:130).

**Fix:** Catch panics at actor/job boundaries too. Specify callback lifetime, reentrancy, thread requirements, cancellation completion, detach, and shutdown order. Preserve the last committed snapshot after failure. Invalid foreign pointers remain a caller-contract problem; `catch_unwind` does not make them safe.

## 2. Build guidance for M02 and the starred milestones

I checked these source revisions:

- OpenPDN: `cca476b0df2a2f70996e6b9486ec45327631568c`.
- Pinta: `f5f43624bcd0810f1c37e64b1703cb59299f67ca`.

The estimates below are engineering hours including targeted tests and integration. They are estimates, not measured agent throughput. These 13 items total **320–568 hours**. The full 50-milestone estimate of 300 hours therefore needs rebuilding.

### M02 — Reference corpus and capture: 12–20 hours

Create a manifest per case:

```text
source revision/application build
input and output hashes
effect ID and complete parameters
document ICC hash
alpha representation and gamma setting
selection, layer opacity, and blend mode
capture method and random seed
```

Capture saved image pixels, not screenshots. Screenshots contain display conversion and possibly checkerboard pixels.

Use a plugin to invoke exposed built-in GPU effects where possible, then cross-check one result against the same operation through the UI. Paint.net exposes built-in GPU effects, but that does not provide automation for every tool or dialog. [GPU API](https://paintdotnet.github.io/apidocs/api/PaintDotNet.Effects.Gpu.html).

For Gaussian, port `CreateGaussianBlurRow` and the rendering stages from [`src/Effects/GaussianBlurEffect.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/Effects/GaussianBlurEffect.cs) only as a legacy reference. Its weights form a triangular sequence; replacing it with a textbook Gaussian changes results.

Pinta’s parameters live in [`tests/Pinta.Effects.Tests/EffectsTest.Blurs.cs`](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/tests/Pinta.Effects.Tests/EffectsTest.Blurs.cs). Its test utility loads through Cairo and compares premultiplied buffers. Copying PNGs alone does not reproduce that test contract. [Test utility](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/tests/Pinta.Effects.Tests/Utilities.cs).

First fixtures: opaque impulse, grayscale ramp, transparent colored edge, constant partial alpha, and tile-crossing checkerboard. Capture gamma settings separately.

The eight-hour automation spike is reasonable. It is not enough to establish the full effect corpus.

### M04 — Tile store and history: 28–48 hours

Use immutable `TileId`s, a paged persistent tile map, and a storage manager with explicit pinning. Cloning every layer’s entire `Vec` on each history step is avoidable overhead even when pixels are shared.

A history state should include dimensions, layers, selection source, active layer, profile, and metadata. Keep saved revision separately so undoing to the saved state correctly clears the dirty indicator.

OpenPDN provides useful behavior in:

- [`src/HistoryMementos/BitmapHistoryMemento.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/HistoryMementos/BitmapHistoryMemento.cs).
- [`src/HistoryMementos/SelectionHistoryMemento.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/HistoryMementos/SelectionHistoryMemento.cs).

These are not implementations of the proposed persistent tile store. Build that store directly.

Spill records need length, checksum, codec version, and allocation ownership. Publish a spill location only after its write succeeds. A failed operation must leave both current state and history index unchanged.

Test with forced 8 MiB cache limits: branch after undo, selection-only changes, profile changes, resize, layer deletion, read failure, short write, disk full, and truncated spill record. Compare complete state after every transition, not just the final image hash.

Separate fast history-state switching from loading a spilled viewport.

### M05 — CPU compositor: 16–28 hours

Separate `LayerBlendMode` from tool `CompositeMode`. Overwrite is a tool operation, not a fifteenth ordinary paint.net layer mode.

Port the scalar `Apply` and `ApplyOpacity` semantics from [`src/Data/UserBlendOps.Generated.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/Data/UserBlendOps.Generated.cs). Use the generated code as the legacy oracle, not as the structure of the Rust implementation.

For a mathematical reference, with straight colors `Cs`, `Cb` and alphas `As`, `Ab`:

```text
Ao = As + Ab(1-As)
Co,premul = As(1-Ab)Cs + Ab(1-As)Cb + AsAb B(Cb,Cs)
```

Apply layer opacity to source alpha once. Document every compatibility rounding stage.

Numeric traps:

- BGRA source layout versus RGBA storage.
- Color Xor versus Porter–Duff XOR; they are different operations.
- Dodge/Burn/Reflect/Glow singular endpoints.
- Overlay’s choice of the controlling operand.
- Partial-alpha division magnifying tiny intermediate differences.
- Per-layer quantization versus quantization after the whole stack.

Tests: all byte alpha values for selected channel pairs, transparent hidden colors, the 251-versus-249 counterexample, asymmetric source/backdrop colors, 100-layer stacks, hidden layers, and dirty-cache invalidation after reorder/profile changes.

Pinta’s current [`UserBlendOps.Normal.cs`](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Core/Algorithms/PixelOps/UserBlendOps.Normal.cs) is a separate premultiplied reference.

### M06 — wgpu backend and presenter: 24–40 hours

Build the backend around:

```text
GpuTileKey(TileId, content revision, representation)
GpuTileCache with byte budget
UploadRing / ReadbackRing
KernelPlan
Presenter
```

Use separate input and output resources. Start with integer-addressed RGBA8 storage and explicit float32 conversion. Avoid `_srgb` texture formats for encoded document data unless their automatic conversion is intentional.

First shaders: upload/readback identity, Normal composite, all remaining blend modes, and a neighborhood filter crossing four tiles. Add odd widths to catch row-padding mistakes.

Port the CPU arithmetic contract, not a second independently chosen formula. WGSL permits reassociation and fused operations; CPU “no fast math” is not enough to guarantee matching evaluation. Use stable lookup tables for sensitive curves and explicit quantization stages where needed. [WGSL floating-point rules](https://www.w3.org/TR/WGSL/#floating-point-evaluation).

No OpenPDN/Pinta presenter is reusable here. Current wgpu provides a Metal-only `CoreAnimationLayer` surface target in its source. Linux-generated API pages can omit it because of conditional compilation. [Surface source](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu/src/api/surface.rs).

Test device loss, zero-sized views, tab switching, repeated attach/detach, delayed readbacks, memory pressure, and resize during rendering. Include M10’s color-space checks in this spike.

### M10 — Color management: 24–40 hours

Use immutable profiles and cached transforms keyed by:

```text
source profile hash
destination profile hash
intent and black-point compensation
input/output pixel formats
```

Assign changes only the profile. Convert changes all layer pixels and the profile atomically. Alpha must remain byte-identical.

There is no suitable OpenPDN color-management algorithm to port. Use lcms2 for transforms and the verified Mac presentation path.

Do not require universal byte-perfect round trips through an intermediate 8-bit space. Quantization can lose information even without gamut clipping. Test each direction against a higher-precision reference, then report round-trip error separately.

Keep these paths distinct:

1. Document values tagged with their document profile, then converted by Core Animation.
2. Values transformed into a chosen presentation space, tagged with that presentation space.

Applying a document-to-monitor LUT and then tagging the pixels with the document profile causes a second, wrong conversion.

Tests: sRGB, Display P3, Adobe RGB, ProPhoto RGB, grayscale expansion, CMYK import, malformed ICC, profile assignment versus conversion, partial alpha, profile change during preview, and two screens.

A 33³ LUT needs interpolation-error tests against direct lcms2 transforms. It is not a general one-byte accuracy guarantee.

### M16 — Selection engine: 24–40 hours

Use the dual geometry/coverage representation described above. Store contour caches separately from selection truth.

Port selection continuation and transform behavior from [`src/Core/Selection.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/Core/Selection.cs). Read [`Pinta.Core/Classes/DocumentSelection.cs`](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Core/Classes/DocumentSelection.cs) for another implementation. Replace platform drawing/Boolean internals with the chosen Rust geometry library.

For geometric inputs, perform union/intersection/subtraction/XOR before rasterization. For imported coverage masks, choose and document a separate rule; `max`, `min`, saturated subtraction, and absolute difference are possible defaults, but require behavior tests.

Use half-open pixel rectangles consistently. Pinta code sometimes iterates inclusive `Right`/`Bottom`, so direct loop translation is unsafe.

The proposed 16× supersampled oracle is too weak. A vertical strip of width `0.03` pixel has rounded coverage **8**, but a 16× center-sampled grid can miss it entirely.

Tests: empty/all/none distinctions, holes, touching edges, self-crossing lasso, one-pixel islands, negative coordinates, large offsets, repeated identical union/XOR, soft mask preservation, and selection undo after crop.

Use analytic rectangle/polygon area cases to validate antialiasing.

### M22 — Magic Wand and Paint Bucket: 20–36 hours

Build a shared iterative scanline flood fill with a visited bitset and span queue. Contiguous fill should compare candidates with the original seed color, not progressively with the previous pixel.

The verified legacy algorithm is in [`src/tools/FloodToolBase.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/FloodToolBase.cs):

```text
t = trunc(slider_fraction² × 256)

D = ΣRGB floor((1 + (seed-channel − pixel-channel)²) × seed-alpha / 256)
    + (seed-alpha − pixel-alpha)²

accept when D ≤ 4t²
```

It is asymmetric and is not plain Euclidean ARGB distance. Current Pinta still squares its tolerance slider before calling the fill routine. [Pinta FloodTool](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Tools/Tools/FloodTool.cs).

Do not label this the paint.net 5.1 formula without captures. Implement separate Straight/Premultiplied modes and live changes to tolerance and seed position.

Test diagonal-only contact, tile boundaries, disconnected islands, smooth ramps, tolerance 0/100 and threshold-adjacent values, transparent pixels with different RGB, and layer versus merged sampling.

Bucket detection must sample a frozen pre-fill image. Apply selection coverage once after region detection.

### M23 — Move tools: 24–40 hours

Store:

```text
base revision
original selected pixels and coverage
original layer state
affine transform and pivot
resampling mode
copy/move mode
```

Always rasterize from the original floating pixels. Never repeatedly resample the previous preview.

Correct OpenPDN sources:

- [`src/tools/MoveTool.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/MoveTool.cs): selected pixels.
- [`src/tools/MoveSelectionTool.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/MoveSelectionTool.cs).
- [`src/tools/MoveToolBase.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/MoveToolBase.cs).

`MoveSelectedTool.cs` exists in [Pinta](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Tools/Tools/MoveSelectedTool.cs), not that OpenPDN directory.

Use inverse mapping from destination pixel centers. Special-case integer translations and 90° rotations for exact copying. Define behavior at singular transforms, flips, off-canvas bounds, and negative scale.

A partial selection needs careful cut/reinsert math. Cutting by coverage and then ordinary source-over compositing the same fragment back can reduce alpha even at identity. Require identity move to preserve every byte.

Test 100 preview updates within one session, 100 separately finished moves, overlap, Ctrl-copy, cancellation, changing layers, zero movement, soft edges, and preview/final equality.

### M27 — Brush engine: 24–40 hours

Use:

```text
StrokeSession(base revision, tool settings, seed)
timestamped input samples
distance accumulator
pressure curve
sparse stroke coverage/pixel tiles
```

Generate dabs by traveled distance, carrying the leftover distance between events. Interpolate pressure at dab locations. Mouse input needs an explicit full-pressure default.

OpenPDN’s [`PaintBrushTool.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/PaintBrushTool.cs) uses pressure-scaled ellipse/polygon rendering through GDI+. It is useful for behavior, but it is not the proposed dab engine. Pinta’s [`PaintBrushTool.cs`](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Tools/Tools/PaintBrushTool.cs) also depends on its platform rasterizer.

Port Pencil’s grid traversal and Eraser’s alpha semantics separately. Do not implement Pencil as an antialiased tiny brush.

Decide whether opacity is per dab or per completed stroke. Repeatedly blending translucent dabs without a defined flow rule makes the result depend on input-event density.

Tests: one click, zero-distance events, slow/fast replays of the same path, different event batching, pressure ramps, gaps in input, tile seams, partial selections, erase at alpha 1, cancellation, and exactly one committed history step.

Use reviewed own goldens, as ARCHITECTURE already specifies. M27’s capture-parity wording conflicts with that choice.

### M32 — Line/Curve and editable tools: 20–36 hours

Move the generic transaction framework earlier. M32 should add line geometry and controls.

Use a state machine:

```text
Idle → Creating → Editing → Committing → Idle
                    └──── Cancel ─────→ Idle
```

Store original pixels, parameters, selected handle, preview generation, and base revision. Changing color, width, or a handle regenerates from the same base.

Port the geometry intent from [`src/tools/LineTool.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/LineTool.cs): straight segments, four-point Bézier curves, and cardinal splines. The old raster output comes from GDI+ and cannot be assumed to match a different renderer.

Pinta separates editing and curve-series logic in [`LineCurveEditEngine.cs`](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Tools/Editable/EditEngines/LineCurveEditEngine.cs) and [`LineCurveSeriesEngine.cs`](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Tools/Editable/Shapes/LineCurveSeriesEngine.cs).

Keep geometric coordinates in float64. Define dash phase, dash units, joins, caps, arrowhead scaling, and zero-length paths. Flatten curves with a document-space error bound independent of zoom.

Test sharp joins, short dashes, reversed endpoints, zero-length segments, five successive parameter edits, Finish twice, Escape, undo during editing, tool switch, and layer deletion.

### M36 — Text: 32–56 hours

A coverage bitmap alone is insufficient for editing. Define a platform result containing:

```text
coverage or color-glyph runs
run positions and baseline
glyph and cluster bounds
caret positions
line metrics
text range mapping
```

Keep the text model in a session until Finish. Use UTF-16 ranges at the AppKit boundary and an explicit conversion to Rust string offsets.

Port behavior from [`src/tools/TextTool.cs`](https://raw.githubusercontent.com/rivy/OpenPDN/cca476b0df2a2f70996e6b9486ec45327631568c/src/tools/TextTool.cs), not its Windows shaping. Pinta’s [TextTool](https://raw.githubusercontent.com/PintaProject/Pinta/f5f43624bcd0810f1c37e64b1703cb59299f67ca/Pinta.Tools/Tools/TextTool.cs) is another behavior reference.

Use CoreText for layout/rasterization. Use an `NSTextView` integration or implement `NSTextInputClient` for composition, selection, and candidate-window placement. CoreText alone does not provide an input method editor. [Apple text-input contract](https://developer.apple.com/documentation/appkit/nstextinputclient?language=objc).

Enter must first confirm marked text when an IME is composing; it must not prematurely Finish the paint tool.

Tests: Japanese composition, Chinese candidate selection, combining accents, ligatures, Arabic, bidirectional text, emoji, missing fonts, multiline alignment, fractional positions, zoomed candidate placement, Escape during composition, and app/window focus changes.

Pin fonts and OS build for exact goldens. Treat color emoji as an explicit supported run type or an explicitly tested limitation.

### M42 — Content-aware fill: 48–96 hours

Implement the algorithm from the paper, not the restricted sample code.

Use arrays for source offsets and patch errors, plus a mask pyramid and output pyramid. A nearest-neighbor field stores the best source patch for each target patch.

The paper’s algorithm has:

1. Random initialization.
2. Alternating forward/reverse scans that propagate neighboring offsets.
3. Random search with radius halved each step.
4. Coarse-to-fine refinement.
5. Patch voting and reconstruction for filling; nearest-neighbor matching alone does not fill the image.

For completion, use the coherence term and valid source patches outside the hole. The paper uses squared patch differences in Lab space. Internal Lab analysis does not imply exposing Lab editing or changing RGBA8 storage. [Paper, sections 3–4](https://pixl.cs.princeton.edu/gfx/pubs/Barnes_2009_PAR/patchmatch.pdf).

Start with 7×7 patches and five matching iterations as an initial configuration, then measure. Preserve known pixels exactly. Use a fixed coordinate/pass seed and deterministic tie-breaking.

CPU scan-order propagation cannot be parallelized arbitrarily without changing the algorithm. If a GPU version is required, use the paper’s parallel propagation variant and define matching quality separately.

Tests: empty/full mask, no legal source patches, edge-touching holes, disconnected holes, alpha handling, repeating texture, straight boundaries, cancellation, and deterministic replay. Measure quality inside the hole and boundary ring; whole-image scores hide bad fills.

### M44 — Parity and performance sweep: 24–48 hours

Build a generated coverage matrix:

```text
operation × backend × fixture × parameter set × working space
```

Record reference version, maximum channel error, mismatch count, time, peak memory, and cold/warm pipeline state.

This milestone must not discover basic numerical disagreements for the first time. Run the same checks when each kernel is introduced.

Use independent tests for:

- Reference compatibility.
- CPU/GPU agreement.
- Tile versus whole-image execution.
- Preview versus committed output.
- Determinism under reordered tile scheduling.

A CPU implementation copied into WGSL can share the same bug. CPU/GPU agreement alone is not sufficient.

Test values around clamp boundaries, divisions, lookup-table transitions, and rounding thresholds. Include long effect chains because small errors can accumulate.

Do not hide missing GPU execution behind automatic CPU fallback. A parity test must fail if the requested backend did not run. Keep CPU-only operations explicitly identified.

The “nothing over three seconds” rule needs named dimensions, parameter settings, hardware, and execution scope. It currently overlaps poorly with M42’s different size/time target.

### Verified path corrections

| Milestone | Current reference | Correct reference |
|---|---|---|
| M16 | `src/Selection.cs` | `src/Core/Selection.cs` |
| M23 | `src/tools/MoveSelectedTool.cs` | `src/tools/MoveTool.cs` |
| M28 | `src/GradientRenderers.cs` | `src/Core/GradientRenderers.cs` |
| M31 | `src/tools/RecolorTool.cs` | `src/tools/RecoloringTool.cs` |
| M33 | `src/Effects/TileReflectionEffect.cs` | `src/Effects/TileEffect.cs` |
| M15 | `ResizeBicubic` | `Surface.BicubicFitSurface` |
| M15 | `ResizeBilinear` | `Surface.BilinearFitSurface` |

`SuperSamplingFitSurface` is correct. I checked the complete tree and relevant source files. [OpenPDN tree](https://api.github.com/repos/rivy/OpenPDN/git/trees/master?recursive=1).

M37’s “5.x only” label also needs correction: OpenPDN contains `src/Effects/SurfaceBlurEffect.cs`. That does not prove its output matches 5.1.

## 3. Exit gates that need precise replacements

Apply these common rules first:

- Every required test reports a nonzero case count.
- Reference manifests pin application/source versions and all parameters.
- Compare every applicable pixel, not a mean error alone.
- Persistence and undo compare complete document state.
- Performance reports hardware, image dimensions, layer count, parameters, warm/cold status, and percentile.
- A backend fallback cannot count as a successful GPU test.
- Checkpoints test only capabilities scheduled by that checkpoint, with the required list committed beforehand.

| Milestone(s) | Replacement or addition |
|---|---|
| M01 | Inject differences of 0, 1, and 2 into each channel; test dimension mismatch, alpha mismatch, missing fixture, and empty test discovery. Linux runs engine checks; Swift checks run on Mac. |
| M02 | Capture both named operations on all three inputs twice. Hashes must repeat for deterministic operations. Commit exact build, settings, ICC, input hashes, and a per-operation fallback coverage list. |
| M03 | Exhaust all 65,536 byte-product pairs for scaling helpers. Add boundary-weighted pixel tests and the blend counterexample. Use exact equality for defined byte operations. |
| M04 | Run random commands against a simple full-copy reference model. Compare complete state after every undo, redo, jump, and branch. Force spilling with a small cache; inject read/write failures. Define separate resident and spilled jump timings. |
| M05 | Test 14 layer modes, tool Overwrite separately, opacity endpoints, low alpha, asymmetric colors, and multi-layer stacks. Pin which reference wins each disagreement. |
| M06 | Require adapter/backend evidence and nonzero shader dispatches on Metal and lavapipe. Include odd dimensions, seams, readback, device loss, and presenter lifecycle tests. |
| M07 | Exercise create/close/detach/free, stale IDs, cancelled jobs, worker panic, and callbacks during shutdown. After free returns, callback count must remain unchanged. Report allocation/leak evidence. |
| M08 | Retain the user’s choice among three mockups. Each must show a real small-window layout, floating panels, image strip, and an active tool’s options. |
| M09 | Run a fixed 30-second pan/zoom trace on 6000×4000 data. Record presented frames and stalls; require at least 59 fps average and no stall above 50 ms after warm-up. |
| M10 | Test direct transforms against a pinned high-precision reference. Specify ΔE00, comparison space, profile, intent, and sample set. Five arbitrary screen probes are insufficient. Test alpha preservation separately. |
| M11 | Replace “round-trip” for lossy/read-only formats with decode/encode-specific checks. Cover all eight EXIF orientations, 16-bit conversion, palette alpha, truncation, ICC, first-frame policy, and named RAW camera models. |
| M12 | Compare dimensions, all layer bytes, order, names, visibility, opacity, modes, profile, and metadata. Validate required ORA entries. Test atomic replacement, malformed XML, duplicate paths, oversized expansion, and unsupported versions. |
| M13 | Toggle panels, restore positions, disconnect the saved display, relaunch, and require every panel to remain reachable. |
| M14 | Check add/delete/duplicate/reorder/merge behavior and active layer. Cancel Properties preview without mutation. Compare history state after undo/redo with engine scenarios. |
| M15 | Name every resize algorithm and its exact parameters. Include non-integer scaling, one-pixel dimensions, strong reduction, partial alpha, borders, and gamma on/off. Current 5.1 documentation lists eight image-resize modes, not five. |
| M16 | Test geometric Boolean identities plus analytic coverage cases. Preserve imported soft-mask bytes. Separate no selection from an active empty selection. |
| M17 | Fix input dimensions, crop rectangle, pivot, interpolation, and rounding. Assert exact output dimensions and pixels, including nonrectangular crop transparency and undo. |
| M18–M19 | Give each adjustment default, identity, endpoint, partial-alpha, selection, and profile fixtures. Histogram-based adjustments must give identical results across tile sizes and scheduling orders. |
| M20 | Rapidly submit 100 parameter changes, then cancel or accept. No stale generation may appear or commit. Accept produces one step; cancel preserves complete state. Define whether 100 ms means first useful preview or full-image completion. |
| M21 | Test all tools/modes plus modifier precedence, no/all/empty selection, layer switching, and history. Compare masks with an independent expected result. |
| M22 | Cover both alpha modes, both sampling modes, both flood modes, threshold boundaries, connectivity, and live tolerance/seed edits. Capture exact binary masks separately from antialiased coverage. |
| M23 | One edit session with 100 updates commits once. One hundred independently finished edits commit 100 times. Identity movement is byte-exact; moving the selection outline changes no layer bytes. |
| M24 | Test internal copy/paste with profile and coverage, external paste, oversized paste choice, new-layer position, paste cancellation, and one-step undo. |
| M25 | Pin Gaussian radius and gamma. Include impulse response, constant partial alpha, borders, and seams. Time complete operation including required transfers, separately from kernel-only time. |
| M26 | Test `AARRGGBB` palette parsing, malformed lines, slot count, alpha, primary/secondary swap, and defined sample-window clipping at canvas edges. |
| M27 | Replay identical paths with different event batching. Require the chosen stroke-opacity semantics, one-step history, no gaps, and measured event-to-frame timing. |
| M28 | Test coincident handles, reversed handles, partial alpha, each geometry, transparency mode, selection edges, cancellation, and preview/final identity. |
| M29–M31, M33, M35, M37, M39–M40 | Replace “all pass” with a checked catalog listing each operation and parameter cases. Include zero/default/max values, edge rules, alpha, selections, seams, and seeded cases. A missing catalog entry fails. |
| M32/M34 | Resolve the conflicting oracle policy. ARCHITECTURE specifies own reviewed raster goldens; these milestones require capture pixels. For own goldens, add geometry/control tests and reviewed images for every shape/style. |
| M36 | Name fonts, OS build, strings, IME, and expected caret behavior. Require composition, cancellation, candidate-window placement, undo, and exactly one Finish commit. |
| M38 | Inspect decoded metadata after writing. Strip must remove GPS from EXIF, XMP, and embedded thumbnails where present. Lossless settings must preserve decoded pixels; lossy settings need explicit quality/size expectations. |
| M41 | Specify the discrete Poisson equation and boundary conditions. On analytic fixtures require relative residual ≤ `1e-5` for nonzero right-hand sides; use absolute residual for zero cases. Check known pixels unchanged. |
| M42 | On fixed reconstructable synthetic fixtures, require hole-only PSNR ≥30 dB and SSIM ≥0.95 over hole plus a defined rim. Add separately reviewed natural images. Pin patch size, seed, iterations, ROI, and the 12 MP timing case. |
| M43 | Define mask threshold for IoU, image set, and Vision revision. Require IoU ≥0.9 per named case, not merely average; also test soft coverage and cancellation. |
| M44 | Require complete registry coverage, no unexpected fallback, independent reference checks, and a per-operation performance table with named parameter sets. |
| M45 | Compare command IDs and order against a versioned catalog. Test enabled states and repeat behavior, including retained random seeds. |
| M46 | Check ruler positions at multiple zooms and DPI values; verify pixel/inch/cm conversions numerically. Review both appearances with every floating panel open. |
| M47 | Inject process termination before, during, and after recovery-record publication. Recover the latest acknowledged recoverable revision. Disk-full failures must leave the last valid save readable. Fuzz each target separately with memory/time limits. |
| M48 | Define cold launch from process start to usable canvas, plus first GPU operation separately. Report median/p95 across repeated runs. Enforce absolute budgets as well as regression limits. |
| M49 | Complete open → edit → save using keyboard only. Test focus across floating panels and dialogs with VoiceOver. Labels alone are insufficient. |
| M50 | Test the downloaded, quarantined, notarized artifact on a clean account. Verify signature, bundled dependencies, minimum OS, offline launch, open/save, and recovery. Depend explicitly on M47. |

## 4. Missing features and where they belong

| Feature | Recommendation |
|---|---|
| Auto Crop | Add to M15/M17. Start with transparent-border trimming of the visible composite, applied to all layers. Define empty-image behavior. Uniform-color trimming needs separate tolerance semantics. I did not verify Auto Crop as a built-in 5.1 command. |
| Select All on layers | Already named in M21; specify behavior in M16. It selects the entire active canvas-sized layer, including transparent pixels, and does not select every layer. Test switching active layers while preserving selection. [Edit menu](https://www.getpaint.net/doc/latest/EditMenu.html). |
| Layer thumbnails | Add explicitly to M14. Cache by layer pixel revision/profile; invalidate after edits and history jumps. Keep layer thumbnails distinct from document-tab thumbnails. |
| Open Recent | Add to M09/M11 with missing-file handling and a Clear Menu command. |
| Drag-and-drop open | Add to M11. Specify multi-file order, dropping onto canvas versus image strip, and Open versus Import as Layer. |
| New, Save As, Save a Copy, Revert, close-with-unsaved-work | Add before the Phase 2 exit. Define document dirty state, current filename, and what exporting a flattened image does to the editable document. |
| Basic atomic save/recovery | Design in M04/M12, integrate before Phase 2 exit; retain stress testing in M47. |
| Wand/bucket editing after click | Add to M22’s preview session: tolerance changes and movable seed. The current paint.net documentation explicitly describes both. [Wand behavior](https://www.getpaint.net/doc/latest/MagicWand.html). |
| Straight/Premultiplied tolerance toggle | Add to M22 and M31. PLAN mentions it; milestone scopes omit it. |
| Full Resize modes and gamma toggle | Expand M15. Current documentation names Bicubic, Bicubic Smooth, Bilinear, Bilinear Low Quality, Adaptive Sharp, Lanczos, Fant, and Nearest Neighbor. [Image menu](https://www.getpaint.net/doc/latest/ImageMenu.html). |
| Nonrectangular Crop to Selection | Add to M17: use the selection bounds and make pixels outside its shape transparent. [Crop behavior](https://www.getpaint.net/doc/latest/ImageMenu.html). |
| Copy/Paste Selection geometry | Add to M21/M24 if complete paint.net Edit-menu parity is required. The documented commands are currently absent. [Edit menu](https://www.getpaint.net/doc/latest/EditMenu.html). |
| Canvas-size fill choices | Add to M15/M17: transparent, primary, secondary, white, black; test every anchor. |
| Tool fill patterns and blend choices | Inventory in M02 and attach to M27/M32/M34. Solid fill alone should not silently stand in for all paint.net tool options. |
| Command focus rules | Add in M07/M13. Cmd+A, Delete, Escape, Enter, and Undo must route differently in text fields, text-tool composition, effect dialogs, and canvas tools. |
| Printing | Keep excluded, as PLAN already says. Do not quietly add it during release work. Preserve DPI metadata so a later print feature has correct dimensions. |
| Masks, groups, adjustment layers, PSD/PDN import, HDR storage | Keep excluded under the fixed version scope. |

## 5. Platform risks and go/no-go tests

### wgpu on CAMetalLayer

The direct layer route exists. The larger risk is ownership of its configuration.

Current wgpu Metal source sets the device, pixel format, color space, drawable size, and presentation settings during configuration. In particular, it calls `setColorspace` after choosing a configured surface color space. A one-time Swift assignment can be overwritten. This finding is from current `trunk`; inspect the exact release chosen for the build. [Metal surface implementation](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/wgpu-hal/src/metal/surface.rs).

Core Animation converts tagged layer content. The tag must match the actual pixel encoding. [Apple colorspace documentation](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace).

**Go/no-go test:**

1. Pin wgpu and create a small Swift host using the actual intended bridge.
2. Present sRGB, P3, and Adobe RGB charts beside equivalent color-managed reference images.
3. Resize 200 times, switch documents 100 times, hide/show the window, sleep/wake, and move between available screens.
4. Check the actual layer color space after every reconfiguration.
5. Require zero validation errors, no persistent black/stale frame, no retained-view growth, and correct color after each transition.

If the direct route fails, test the IOSurface fallback with a bounded two/three-buffer ring and explicit completion signals. Sharing a device does not itself synchronize two command queues. The fallback must pass the same tests.

### lavapipe on Linux

Lavapipe runs Vulkan through software. It tests shader portability and Vulkan behavior, but not Apple presentation, Metal performance, or every hardware floating-point behavior. [Mesa source-tree description](https://docs.mesa3d.org/sourcetree.html).

Pin Mesa, LLVM, Vulkan loader, and wgpu versions. Select the intended ICD explicitly using `VK_DRIVER_FILES`, as Mesa documents. [Mesa installation instructions](https://docs.mesa3d.org/install.html).

**Go/no-go test:**

1. Run without a display server.
2. Log adapter name, backend, driver, limits, and enabled features.
3. Require upload, compute, and readback for every registered GPU operation.
4. Run fixed fixtures and seeds on both Metal and lavapipe.
5. Fail on missing adapter, unsupported required format, skipped operation, timeout, or CPU fallback.

Keep Linux correctness fixtures modest. Do not apply M2 GPU timing limits to software Vulkan. Run the Mac performance gate separately.

### lcms2 in a small signed app

The Rust binding can use `pkg-config` and dynamically link an installed library. Its build script also supports static compilation. An explicit `LCMS2_LIB_DIR` branch is checked before the static-only branch, so build-environment overrides matter. [lcms2 build script](https://raw.githubusercontent.com/kornelski/rust-lcms2-sys/master/src/build.rs), [crate features](https://docs.rs/crate/lcms2-sys/latest/features).

**Go/no-go test:**

1. Pin the crate/library versions and select static linkage deliberately.
2. Build with the macOS 15 deployment target and inspect the link map.
3. Run `otool -L` on the final executable and bundled libraries.
4. Require no dependency on `/opt/homebrew`, `/usr/local`, or a developer checkout.
5. Run profile transforms on a clean account without Homebrew.
6. Verify the signed artifact after all files are assembled; measure final bundle size.
7. Feed malformed profiles through the actual wrapper and require bounded failure.

No evidence currently shows lcms2 threatens the 75 MB limit. The immediate risk is accidental linkage to a developer-machine library.

## 6. Uncertain concerns

These need experiments; they are not established failures.

- I could not read forum topic 117085. Its claimed modern distance formula remains unverified. The legacy formula above is directly supported by source.
- Which paint.net 5.1 operations use encoded versus linear working space needs a per-operation capture inventory. The blanket encoded rule is disproved; an exhaustive replacement table is not yet established.
- A generic 33³ LUT may miss the accuracy target for difficult profiles. Measure before choosing a denser or adaptive table.
- ImageIO’s exact RAW and AVIF capabilities depend on OS version and codec support. Test macOS 15 explicitly rather than assuming the development Mac’s behavior.
- PatchMatch’s five-second goal is unmeasured. Hole shape, source-search region, patch size, and reconstruction passes materially affect it.
- A second physical display, pressure tablet, and macOS 15 test machine may be needed for complete platform checks.
- Exact byte agreement for every parameter of every proprietary 5.1 effect is not proven achievable from the available legacy sources.
- The proposed application size and launch figures remain estimates until a signed skeleton includes wgpu, lcms2, Swift, and the required platform integrations.

### Checks run and retrieval limits

I ran these exact local check commands:

```sh
git status --short
python3 "$TMPDIR/op-step0-astra/audit_checks.py"
rg --files -g AGENTS.md -g Cargo.toml -g Package.swift \
  -g '*Tests*' -g '*.xcodeproj' -g '*test*'
```

Results:

- Initial Git status was clean.
- The scratch source-check script exited **0**. It confirmed five incorrect paths, two incorrect method names, the two-byte blend counterexample, the memory calculation, and the 16× coverage counterexample.
- The build/test-file search found no matching project test/build files.
- Final Git status showed only `?? docs/pages/`, created by another session. I did not modify it.
- No application tests, GPU tests, Windows captures, Linux tests, signing checks, or performance benchmarks ran.

The first Swift/PDFKit extraction command exited **133** after a scratch-file write failed with `NSCocoaErrorDomain Code=513`. A subsequent stdout-only extraction succeeded and let me read PatchMatch sections 3–4. This was a document-reading failure, not an application test failure.

Retrieval limits:

- Forum topic 117085 remained inaccessible after retry.
- The guessed Mesa `drivers/lavapipe.html` page returned 404; the official install and source-tree pages supplied the relevant facts.
- The guessed lcms2 root `build.rs` returned 404; `src/build.rs` was retrieved successfully.
- Version-specific wgpu 27 API pages were inaccessible; current source confirmed the conditional Metal API. The chosen release still needs checking.
- Apple’s direct colorspace and text-input pages returned JavaScript shells; indexed official documentation supplied the text cited above.
- Princeton’s PatchMatch page/PDF failed through the web reader. Direct Python retrieval obtained the PDF; PDFKit extraction then succeeded. The project-page summary was available through search.
- The guessed author-hosted PatchMatch PDF returned 404.
- The `www.paint.net` documentation URLs failed; the `www.getpaint.net` URLs opened and redirected successfully.
- Exa was unavailable in this session. I used built-in web search and direct primary-source retrieval. I did not call the research skill’s model-backed pullers because this task forbids other models.

The main work before building is to settle operation-specific pixel contracts, preserve bitmap selections, budget all memory, and move transaction/recovery design earlier. No project changes were made. Application acceptance checks remain unrun.
# OpenPDN files compiled by the oracle

Pinned revision: `cca476b0df2a2f70996e6b9486ec45327631568c`.
The project file lists each source file directly. No source files from `.pdn-src` are copied into this repository.

| File under `.pdn-src/src/` | Why it is compiled |
| --- | --- |
| `Base/IDeferredSerializable.cs` | Required by MemoryBlock's serialization interface. |
| `Base/DeferredFormatter.cs` | Required by MemoryBlock's serialization signatures. |
| `Core/ColorBgra.cs` | Original RGBA byte math and pixel type. |
| `Core/Surface.cs` | Original surface storage and four resamplers. |
| `Core/MemoryBlock.cs` | Original surface buffer. |
| `Core/PixelOp.cs` | Pixel operation base. |
| `Core/IPixelOp.cs` | Pixel operation interface. |
| `Core/UnaryPixelOp.cs` | Unary pixel operation base. |
| `Core/BinaryPixelOp.cs` | Binary pixel operation base. |
| `Core/UnaryPixelOps.cs` | Original color adjustment pixel math. |
| `Core/BinaryPixelOps.cs` | Original binary pixel math. |
| `Core/HsvColor.cs` | Hue adjustment support. |
| `Core/Histogram.cs` | Full-surface histogram for Auto-Level. |
| `Core/HistogramRGB.cs` | Original RGB histogram and Auto-Level setup. |
| `Core/RgbColor.cs` | HSV conversion support. |
| `Core/Scanline.cs` | Unary operation region signatures. |
| `Core/ResamplingAlgorithm.cs` | Surface resampling mode enum. |
| `Data/UserBlendOp.cs` | User blend operation base. |
| `Data/UserBlendOps.cs` | Blend mode enumeration and constructors. |
| `Data/UserBlendOps.Generated.cs` | Original 14 blend mode formulas and opacity variants. |
| `Effects/BrightnessAndContrastAdjustment.cs` | Original brightness and contrast effect class. |
| `Effects/PosterizeAdjustment.cs` | Original posterize effect class. |
| `Effects/GaussianBlurEffect.cs` | Original tent-kernel Gaussian effect class. |
| `Effects/FragmentEffect.cs` | Original fragment sample offsets and blend. |
| `Effects/LocalHistogramEffect.cs` | Original circular histograms for Median. |
| `Effects/MedianEffect.cs` | Original Median radius and percentile effect. |
| `Effects/MotionBlurEffect.cs` | Original linear samples and bilinear interpolation. |
| `Effects/RadialBlurEffect.cs` | Original fixed-point rotation blur. |
| `Effects/ZoomBlurEffect.cs` | Original fixed-point zoom blur. |

`Shims/Utility.cs` contains verbatim numeric methods from `Core/Utility.cs` with the Windows UI methods removed. Its header records the source revision and changes.
`Shims/AddNoiseEffect.cs`, `Shims/FrostedGlassEffect.cs`, and `Shims/CloudsEffect.cs` copy their named effect files. Each replaces only a clock-derived random seed with a fixed value. Their headers name the source and exact change.

## Stubbed types

`Stubs/Platform.cs` provides `PaintDotNet.SystemLayer.Memory` (native allocation and byte copy), `Processor` (one serialization worker), `PaintDotNet.Threading.ThreadPool` (synchronous serialization callback), `Tracing` (debug ping), `PdnRegion` (unused region API), `PdnResources` (resource key in place of UI text), and `PaintDotNet.Core.Placeholder` (namespace anchor).
`Stubs/EffectHost.cs` provides `RenderArgs`, `ImageResource`, `EffectEnvironmentParameters`, `InternalPropertyBasedEffect`, `PropertyBasedEffect`, `PropertyBasedEffectConfigToken`, `EffectFlags`, `EffectCategoryAttribute`, `SubmenuNames`, `Property`, `Int32Property`, `DoubleProperty`, `BooleanProperty`, `StaticListChoiceProperty`, `PropertyCollection`, `PropertyCollectionRule`, `LinkValuesBasedOnBooleanRule`, `SoftMutuallyBoundMinMaxRule`, `ControlInfo`, `PropertyControlInfo`, `ValueHolder`, `ControlInfoPropertyNames`, `PropertyControlType`, and an empty `System.Windows.Forms.Control` compile-time type. These stubs carry effect parameters and avoid Windows UI work.
`Stubs/Drawing.cs` provides compile-time `System.Drawing.Image`, `Bitmap`, `Graphics`, `Region`, `System.Drawing.Imaging.PixelFormat`, `ImageLockMode`, `BitmapData`, a `System.Drawing.Drawing2D` namespace anchor, and `PdnGraphicsPath`. Any attempt to use GDI bitmap, graphics, region, or path methods throws. Rectangle, Point, Size, and Color come from `System.Drawing.Primitives`. No GPC code or resource asset is compiled.

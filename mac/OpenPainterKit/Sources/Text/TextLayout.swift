// CoreText layout and raster for the Text tool (M46). The engine never shapes
// text: this file lays the text out and hands the engine raster runs.
//
// The platform result (docs: .op-state/astra-step0.md, "M36 — Text"):
// - coverage runs (one byte per pixel, painted with the primary color) and
//   color-glyph runs (premultiplied RGBA, such as color emoji),
// - run positions and each line's baseline,
// - glyph cluster bounds and caret positions,
// - line metrics,
// - the UTF-16 (AppKit) to UTF-8 (Rust string) offset mapping.
//
// Coordinates are document pixels relative to the anchor: the point the user
// clicked. y grows down. Alignment follows paint.net: Left grows right from the
// anchor, Center grows both ways, Right grows left
// (https://www.getpaint.net/doc/latest/TextTool.html, "Text Justification").
import AppKit
import CoreText

/// The text style the Text tool lays out with.
public struct TextStyleSpec: Equatable, Sendable {
    public enum Alignment: String, CaseIterable, Sendable {
        case left, center, right
        public var label: String { rawValue.capitalized }
    }

    /// The font family name, such as "Helvetica Neue".
    public var family: String
    /// Pins one exact face by PostScript name (goldens); overrides `family`.
    public var postScriptName: String?
    /// Size in points. One point is 96 / 72 pixels (paint.net's "Fixed (96 DPI)").
    public var pointSize: Double
    public var bold = false
    public var italic = false
    public var underline = false
    public var strikethrough = false
    public var alignment = Alignment.left
    public var antialias = true

    public init(family: String = TextStyleSpec.defaultFamily, postScriptName: String? = nil, pointSize: Double = 12,
                bold: Bool = false, italic: Bool = false, underline: Bool = false, strikethrough: Bool = false,
                alignment: Alignment = .left, antialias: Bool = true) {
        self.family = family
        self.postScriptName = postScriptName
        self.pointSize = pointSize
        self.bold = bold
        self.italic = italic
        self.underline = underline
        self.strikethrough = strikethrough
        self.alignment = alignment
        self.antialias = antialias
    }

    public static let defaultFamily = "Helvetica Neue"
    /// Used when the chosen family is not installed.
    public static let fallbackFamily = "Helvetica"
    public static let pixelsPerPoint = 96.0 / 72.0
    public static let sizeRange: ClosedRange<Double> = 1...2000
    public var pixelSize: CGFloat { CGFloat(pointSize * Self.pixelsPerPoint) }
}

/// What CoreText produced for one text, in document pixels relative to the anchor.
public struct TextLayoutResult: Sendable {
    public enum RunKind: String, Sendable { case coverage, color }

    /// One raster run. Coverage: `width * height` bytes. Color: `width * height * 4`
    /// bytes, premultiplied RGBA.
    public struct Run: Equatable, Sendable {
        public var kind: RunKind
        public var x: Int
        public var y: Int
        public var width: Int
        public var height: Int
        public var bytes: [UInt8]
        public var rect: CGRect { CGRect(x: x, y: y, width: width, height: height) }
    }

    public struct Line: Equatable, Sendable {
        /// The line's characters (without its line break), UTF-16.
        public var range: NSRange
        /// Where the line's pen starts, after alignment.
        public var x: Double
        public var top: Double
        public var baseline: Double
        public var ascent: Double
        public var descent: Double
        public var leading: Double
        public var height: Double
        /// Typographic width (advance).
        public var width: Double
    }

    /// One grapheme cluster (what the user sees as one character).
    public struct Cluster: Equatable, Sendable {
        public var range: NSRange
        public var line: Int
        /// Union of the cluster's glyph advances, full line height.
        public var bounds: CGRect
        /// True when the cluster's glyphs run right to left.
        public var rightToLeft: Bool
    }

    public struct Caret: Equatable, Sendable {
        /// UTF-16 offset of a cluster boundary.
        public var offset: Int
        public var line: Int
        public var x: Double
        public var top: Double
        public var height: Double
        public var rect: CGRect { CGRect(x: x, y: top, width: 0, height: height) }
    }

    public var text: String
    public var runs: [Run] = []
    public var lines: [Line] = []
    public var clusters: [Cluster] = []
    public var carets: [Caret] = []
    /// The PostScript name of the main font used.
    public var fontName = ""
    /// The family or PostScript name that was asked for but is not installed.
    public var missingFont: String?
    /// PostScript names of every font CoreText used, including cascade fallbacks.
    public var runFonts: [String] = []
    /// True when bold or italic was synthesized because the family has no such face.
    public var syntheticBold = false
    public var syntheticItalic = false
    /// The union of the line boxes (for hit testing and the edit frame).
    public var layoutBounds: CGRect = .zero

    /// The caret at a UTF-16 offset (snapped to the nearest boundary at or before it).
    public func caret(at offset: Int) -> Caret {
        if let c = carets.last(where: { $0.offset <= offset }) { return c }
        return carets.first ?? Caret(offset: 0, line: 0, x: 0, top: 0, height: lines.first?.height ?? 0)
    }

    /// The UTF-8 byte offset of a UTF-16 offset in `text` (what the engine stores).
    public func utf8Offset(forUTF16 offset: Int) -> Int {
        TextLayoutResult.utf8Offset(forUTF16: offset, in: text)
    }

    public static func utf8Offset(forUTF16 offset: Int, in text: String) -> Int {
        // A UTF-16 offset inside a surrogate pair rounds down to the scalar start.
        var units = 0, bytes = 0
        for scalar in text.unicodeScalars {
            let next = units + scalar.utf16.count
            if next > offset { break }
            units = next
            bytes += scalar.utf8.count
        }
        return bytes
    }

    /// The UTF-16 offset nearest a point (document pixels relative to the anchor).
    public func offset(at p: CGPoint) -> Int {
        guard !lines.isEmpty else { return 0 }
        let li = lines.lastIndex(where: { $0.top <= p.y }) ?? 0
        let onLine = carets.filter { $0.line == li }
        guard let best = onLine.min(by: { abs($0.x - p.x) < abs($1.x - p.x) }) else { return 0 }
        return best.offset
    }

    /// The bounds of a UTF-16 range on its first line (a caret rect when empty).
    public func firstRect(for range: NSRange) -> CGRect {
        if range.length == 0 { return caret(at: range.location).rect }
        let hit = clusters.filter { NSIntersectionRange($0.range, range).length > 0 }
        guard let first = hit.first else { return caret(at: range.location).rect }
        return hit.filter { $0.line == first.line }.reduce(first.bounds) { $0.union($1.bounds) }
    }

    /// The clusters a UTF-16 range touches, grouped by line (marked-text underline).
    public func lineRects(for range: NSRange) -> [CGRect] {
        var byLine: [Int: CGRect] = [:]
        for c in clusters where NSIntersectionRange(c.range, range).length > 0 {
            byLine[c.line] = byLine[c.line].map { $0.union(c.bounds) } ?? c.bounds
        }
        return byLine.keys.sorted().compactMap { byLine[$0] }
    }
}

public enum TextLayoutEngine {
    // MARK: Fonts

    /// Installed family names (the font menu).
    public static func availableFamilies() -> [String] {
        NSFontManager.shared.availableFontFamilies.filter { !$0.hasPrefix(".") }
    }

    static func familyExists(_ family: String) -> Bool {
        NSFontManager.shared.availableFontFamilies.contains(family)
    }

    struct ResolvedFont {
        var font: CTFont
        var missing: String?
        var syntheticBold: Bool
        var syntheticItalic: Bool
    }

    static func resolveFont(_ style: TextStyleSpec) -> ResolvedFont {
        let px = style.pixelSize
        var missing: String?
        var font: CTFont
        if let ps = style.postScriptName {
            font = CTFontCreateWithName(ps as CFString, px, nil)
            if CTFontCopyPostScriptName(font) as String != ps {
                missing = ps
                font = familyFont(TextStyleSpec.fallbackFamily, px)
            }
        } else if familyExists(style.family) {
            font = familyFont(style.family, px)
        } else {
            missing = style.family
            font = familyFont(TextStyleSpec.fallbackFamily, px)
        }
        var wanted: CTFontSymbolicTraits = []
        if style.bold { wanted.insert(.traitBold) }
        if style.italic { wanted.insert(.traitItalic) }
        if !wanted.isEmpty, style.postScriptName == nil {
            if let both = CTFontCreateCopyWithSymbolicTraits(font, px, nil, wanted, wanted) {
                font = both
            } else {
                for t in [CTFontSymbolicTraits.traitBold, .traitItalic] where wanted.contains(t) {
                    if let one = CTFontCreateCopyWithSymbolicTraits(font, px, nil, t, t) { font = one }
                }
            }
        }
        let have = CTFontGetSymbolicTraits(font)
        let synthB = style.bold && !have.contains(.traitBold)
        let synthI = style.italic && !have.contains(.traitItalic)
        if synthI {
            // A 12 degree slant, like the slant most systems synthesize.
            var skew = CGAffineTransform(a: 1, b: 0, c: CGFloat(tan(12 * Double.pi / 180)), d: 1, tx: 0, ty: 0)
            font = CTFontCreateCopyWithAttributes(font, px, &skew, nil)
        }
        return ResolvedFont(font: font, missing: missing, syntheticBold: synthB, syntheticItalic: synthI)
    }

    private static func familyFont(_ family: String, _ px: CGFloat) -> CTFont {
        let desc = CTFontDescriptorCreateWithAttributes([kCTFontFamilyNameAttribute: family] as CFDictionary)
        return CTFontCreateWithFontDescriptor(desc, px, nil)
    }

    // MARK: Layout

    /// Lays out and rasterizes `text`. Lines break only at "\n".
    public static func layout(_ text: String, style: TextStyleSpec) -> TextLayoutResult {
        let resolved = resolveFont(style)
        let font = resolved.font
        var result = TextLayoutResult(text: text)
        result.fontName = CTFontCopyPostScriptName(font) as String
        result.missingFont = resolved.missing
        result.syntheticBold = resolved.syntheticBold
        result.syntheticItalic = resolved.syntheticItalic
        var attrs: [NSAttributedString.Key: Any] = [
            NSAttributedString.Key(kCTFontAttributeName as String): font,
            NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true,
        ]
        if resolved.syntheticBold {
            // A negative stroke width fills and strokes: CoreText's synthetic bold.
            attrs[NSAttributedString.Key(kCTStrokeWidthAttributeName as String)] = -3.0
        }
        let ns = text as NSString
        let fontAscent = Double(CTFontGetAscent(font))
        let fontDescent = Double(CTFontGetDescent(font))
        let fontLeading = Double(CTFontGetLeading(font))

        // Split into lines at "\n".
        var ranges: [NSRange] = []
        var start = 0
        for i in 0..<ns.length where ns.character(at: i) == 0x0A {
            ranges.append(NSRange(location: start, length: i - start))
            start = i + 1
        }
        ranges.append(NSRange(location: start, length: ns.length - start))

        var ctLines: [CTLine] = []
        var top = 0.0
        var fonts: [String] = []
        for range in ranges {
            let attributed = NSAttributedString(string: ns.substring(with: range), attributes: attrs)
            let line = CTLineCreateWithAttributedString(attributed)
            var a: CGFloat = 0, d: CGFloat = 0, l: CGFloat = 0
            let width = Double(CTLineGetTypographicBounds(line, &a, &d, &l))
            let ascent = max(fontAscent, Double(a)).rounded(.up)
            let descent = max(fontDescent, Double(d))
            let height = (ascent + descent + fontLeading).rounded(.up)
            let x: Double =
                switch style.alignment {
                case .left: 0
                case .center: -width / 2
                case .right: -width
                }
            result.lines.append(TextLayoutResult.Line(
                range: range, x: x, top: top, baseline: top + ascent, ascent: ascent, descent: descent,
                leading: fontLeading, height: height, width: width))
            ctLines.append(line)
            for run in runs(of: line) {
                let name = CTFontCopyPostScriptName(runFont(run) ?? font) as String
                if !fonts.contains(name) { fonts.append(name) }
            }
            top += height
        }
        result.runFonts = fonts
        if let first = result.lines.first, let last = result.lines.last {
            let minX = result.lines.map(\.x).min() ?? 0
            let maxX = result.lines.map { $0.x + $0.width }.max() ?? 0
            result.layoutBounds = CGRect(x: minX, y: first.top, width: maxX - minX, height: last.top + last.height - first.top)
        }
        measureClusters(ns, lines: result.lines, ctLines: ctLines, into: &result)
        rasterize(result.lines, ctLines: ctLines, font: font, style: style, into: &result)
        return result
    }

    static func runs(of line: CTLine) -> [CTRun] {
        (CTLineGetGlyphRuns(line) as? [CTRun]) ?? []
    }

    static func runFont(_ run: CTRun) -> CTFont? {
        let attrs = CTRunGetAttributes(run) as NSDictionary
        guard let value = attrs[kCTFontAttributeName as String] else { return nil }
        return (value as! CTFont)
    }

    static func isColor(_ run: CTRun) -> Bool {
        guard let f = runFont(run) else { return false }
        return CTFontGetSymbolicTraits(f).contains(.traitColorGlyphs)
    }

    // MARK: Clusters and carets

    private static func measureClusters(
        _ ns: NSString, lines: [TextLayoutResult.Line], ctLines: [CTLine], into result: inout TextLayoutResult
    ) {
        for (li, line) in lines.enumerated() {
            let ct = ctLines[li]
            // Glyph extents by UTF-16 index (relative to the whole text).
            var extents: [(index: Int, x0: Double, x1: Double, rtl: Bool)] = []
            for run in runs(of: ct) {
                let n = CTRunGetGlyphCount(run)
                guard n > 0 else { continue }
                var idx = [CFIndex](repeating: 0, count: n)
                var pos = [CGPoint](repeating: .zero, count: n)
                var adv = [CGSize](repeating: .zero, count: n)
                CTRunGetStringIndices(run, CFRange(location: 0, length: 0), &idx)
                CTRunGetPositions(run, CFRange(location: 0, length: 0), &pos)
                CTRunGetAdvances(run, CFRange(location: 0, length: 0), &adv)
                let rtl = CTRunGetStatus(run).contains(.rightToLeft)
                for g in 0..<n {
                    let x0 = line.x + Double(pos[g].x)
                    extents.append((line.range.location + idx[g], x0, x0 + Double(adv[g].width), rtl))
                }
            }
            // Clusters on this line.
            var clusterRanges: [NSRange] = []
            ns.enumerateSubstrings(in: line.range, options: .byComposedCharacterSequences) { _, r, _, _ in
                clusterRanges.append(r)
            }
            func caretX(_ offset: Int) -> Double {
                line.x + Double(CTLineGetOffsetForStringIndex(ct, offset - line.range.location, nil))
            }
            for r in clusterRanges {
                let mine = extents.filter { $0.index >= r.location && $0.index < NSMaxRange(r) }
                var x0: Double, x1: Double
                if mine.isEmpty {
                    // A ligature's later cluster has no glyph of its own: use the caret offsets.
                    let a = caretX(r.location), b = caretX(NSMaxRange(r))
                    x0 = min(a, b)
                    x1 = max(a, b)
                } else {
                    x0 = mine.map(\.x0).min()!
                    x1 = mine.map(\.x1).max()!
                }
                result.clusters.append(TextLayoutResult.Cluster(
                    range: r, line: li, bounds: CGRect(x: x0, y: line.top, width: x1 - x0, height: line.height),
                    rightToLeft: mine.first?.rtl ?? false))
            }
            // Carets at every cluster boundary of the line, including both ends.
            var offsets = [line.range.location]
            offsets += clusterRanges.map { NSMaxRange($0) }
            for o in offsets {
                result.carets.append(TextLayoutResult.Caret(
                    offset: o, line: li, x: caretX(o), top: line.top, height: line.height))
            }
        }
    }

    // MARK: Raster

    private static func rasterize(
        _ lines: [TextLayoutResult.Line], ctLines: [CTLine], font: CTFont, style: TextStyleSpec,
        into result: inout TextLayoutResult
    ) {
        guard let firstLine = lines.first, let lastLine = lines.last else { return }
        let px = Double(style.pixelSize)
        let pad = (px * 0.5).rounded(.up) + 2
        let minX = lines.map(\.x).min()!
        let maxX = lines.map { $0.x + $0.width }.max()!
        let x0 = Int((minX - pad).rounded(.down))
        let y0 = Int((firstLine.top - pad).rounded(.down))
        let x1 = Int((maxX + pad).rounded(.up))
        let y1 = Int((lastLine.top + lastLine.height + pad).rounded(.up))
        let w = max(x1 - x0, 1), h = max(y1 - y0, 1)
        // Local (anchor-relative, y down) to Core Graphics (bitmap, y up).
        func cg(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: x - Double(x0), y: Double(h) - (y - Double(y0))) }

        // Coverage: every glyph that is not a color glyph, plus underline and strikethrough.
        guard
            let ctx = CGContext(
                data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w,
                space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)
        else { return }
        configure(ctx, antialias: style.antialias)
        ctx.setFillColor(gray: 1, alpha: 1)
        ctx.setStrokeColor(gray: 1, alpha: 1)
        let underlinePos = Double(CTFontGetUnderlinePosition(font))
        let thickness = max(Double(CTFontGetUnderlineThickness(font)), 1)
        let xHeight = Double(CTFontGetXHeight(font))
        var colorRuns: [(CTRun, TextLayoutResult.Line)] = []
        for (li, line) in lines.enumerated() {
            for run in runs(of: ctLines[li]) {
                if isColor(run) {
                    colorRuns.append((run, line))
                    continue
                }
                ctx.textPosition = cg(line.x, line.baseline)
                CTRunDraw(run, ctx, CFRange(location: 0, length: 0))
            }
            guard line.width > 0 else { continue }
            var bars: [Double] = []
            if style.underline { bars.append(line.baseline - underlinePos) }
            if style.strikethrough { bars.append(line.baseline - xHeight / 2) }
            for center in bars {
                var top = center - thickness / 2
                var height = thickness
                var left = line.x, right = line.x + line.width
                if !style.antialias {
                    top = top.rounded()
                    height = max(height.rounded(), 1)
                    left = left.rounded()
                    right = right.rounded()
                }
                let a = cg(left, top + height), b = cg(right, top)
                ctx.fill(CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y))
            }
        }
        if let data = ctx.data {
            var bytes = [UInt8](UnsafeBufferPointer(start: data.assumingMemoryBound(to: UInt8.self), count: w * h))
            if !style.antialias {
                for i in bytes.indices { bytes[i] = bytes[i] >= 128 ? 255 : 0 }
            }
            if let run = trimmed(kind: .coverage, bytes: bytes, x: x0, y: y0, width: w, height: h, bpp: 1) {
                result.runs.append(run)
            }
        }

        // Color glyphs (emoji): one premultiplied RGBA run per CoreText run.
        for (run, line) in colorRuns {
            let n = CTRunGetGlyphCount(run)
            var pos = [CGPoint](repeating: .zero, count: n)
            var adv = [CGSize](repeating: .zero, count: n)
            CTRunGetPositions(run, CFRange(location: 0, length: 0), &pos)
            CTRunGetAdvances(run, CFRange(location: 0, length: 0), &adv)
            let left = line.x + Double(pos.map(\.x).min() ?? 0)
            let right = line.x + Double(zip(pos, adv).map { $0.x + $1.width }.max() ?? 0)
            let rx0 = Int((left - pad).rounded(.down)), rx1 = Int((right + pad).rounded(.up))
            let ry0 = Int((line.top - pad).rounded(.down)), ry1 = Int((line.top + line.height + pad).rounded(.up))
            let rw = max(rx1 - rx0, 1), rh = max(ry1 - ry0, 1)
            guard
                let space = CGColorSpace(name: CGColorSpace.sRGB),
                let c = CGContext(
                    data: nil, width: rw, height: rh, bitsPerComponent: 8, bytesPerRow: rw * 4, space: space,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { continue }
            configure(c, antialias: style.antialias)
            c.textPosition = CGPoint(x: line.x - Double(rx0), y: Double(rh) - (line.baseline - Double(ry0)))
            CTRunDraw(run, c, CFRange(location: 0, length: 0))
            guard let data = c.data else { continue }
            var bytes = [UInt8](UnsafeBufferPointer(start: data.assumingMemoryBound(to: UInt8.self), count: rw * rh * 4))
            if !style.antialias {
                // Clip-to-pixel: each pixel is fully on (opaque) or off.
                for i in stride(from: 0, to: bytes.count, by: 4) {
                    let a = Int(bytes[i + 3])
                    if a < 128 {
                        bytes[i] = 0; bytes[i + 1] = 0; bytes[i + 2] = 0; bytes[i + 3] = 0
                    } else {
                        for k in 0..<3 { bytes[i + k] = UInt8(min(255, (Int(bytes[i + k]) * 255 + a / 2) / a)) }
                        bytes[i + 3] = 255
                    }
                }
            }
            if let r = trimmed(kind: .color, bytes: bytes, x: rx0, y: ry0, width: rw, height: rh, bpp: 4) {
                result.runs.append(r)
            }
        }
    }

    private static func configure(_ ctx: CGContext, antialias: Bool) {
        ctx.setAllowsAntialiasing(antialias)
        ctx.setShouldAntialias(antialias)
        ctx.setAllowsFontSmoothing(false)
        ctx.setShouldSmoothFonts(false)
        ctx.setAllowsFontSubpixelPositioning(true)
        ctx.setShouldSubpixelPositionFonts(true)
        ctx.setAllowsFontSubpixelQuantization(false)
        ctx.setShouldSubpixelQuantizeFonts(false)
        ctx.interpolationQuality = antialias ? .high : .none
    }

    /// Cuts a run down to its non-zero pixels; nil when it is empty.
    static func trimmed(
        kind: TextLayoutResult.RunKind, bytes: [UInt8], x: Int, y: Int, width: Int, height: Int, bpp: Int
    ) -> TextLayoutResult.Run? {
        var minX = width, minY = height, maxX = -1, maxY = -1
        for row in 0..<height {
            for col in 0..<width {
                let a = bytes[(row * width + col) * bpp + bpp - 1]
                if a != 0 {
                    minX = min(minX, col); maxX = max(maxX, col)
                    minY = min(minY, row); maxY = max(maxY, row)
                }
            }
        }
        guard maxX >= 0 else { return nil }
        let tw = maxX - minX + 1, th = maxY - minY + 1
        var out = [UInt8](repeating: 0, count: tw * th * bpp)
        for row in 0..<th {
            let src = ((minY + row) * width + minX) * bpp
            out.replaceSubrange(row * tw * bpp..<(row + 1) * tw * bpp, with: bytes[src..<src + tw * bpp])
        }
        return TextLayoutResult.Run(kind: kind, x: x + minX, y: y + minY, width: tw, height: th, bytes: out)
    }
}

extension TextLayoutResult.Run {
    /// The `tool.text.*` JSON form of this run.
    public var engineParams: [String: any Sendable] {
        ["kind": kind.rawValue, "x": x, "y": y, "width": width, "height": height,
         "data": Data(bytes).base64EncodedString()]
    }
}

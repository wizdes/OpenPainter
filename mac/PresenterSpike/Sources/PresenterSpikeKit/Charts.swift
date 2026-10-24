import CoreGraphics
import Foundation

/// One color patch of a test chart, in document pixels.
public struct Patch {
    public let x: Int
    public let y: Int
    public let size: Int
    public let rgb: (UInt8, UInt8, UInt8)
    public var centerX: Int { x + size / 2 }
    public var centerY: Int { y + size / 2 }
}

/// A test document: straight RGBA8 pixels encoded in `space`, plus its ICC bytes.
public final class TestDocument {
    public let name: String
    public let width: Int
    public let height: Int
    public let rgba: [UInt8]
    public let space: CGColorSpace
    public let icc: Data
    public let patches: [Patch]

    init(name: String, width: Int, height: Int, rgba: [UInt8], space: CGColorSpace, patches: [Patch]) {
        self.name = name
        self.width = width
        self.height = height
        self.rgba = rgba
        self.space = space
        self.icc = space.copyICCData()! as Data
        self.patches = patches
    }
}

public enum Charts {
    /// 32 patch values: primaries, secondaries, halves, naturals, near-black, near-white
    /// and near-gamut-edge colors.
    static let values: [(UInt8, UInt8, UInt8)] = [
        (255, 0, 0), (0, 255, 0), (0, 0, 255), (0, 255, 255),
        (255, 0, 255), (255, 255, 0), (255, 255, 255), (0, 0, 0),
        (128, 0, 0), (0, 128, 0), (0, 0, 128), (128, 128, 0),
        (0, 128, 128), (128, 0, 128), (64, 64, 64), (192, 192, 192),
        (200, 150, 120), (90, 60, 40), (100, 140, 200), (60, 120, 60),
        (230, 200, 50), (180, 40, 90), (30, 30, 30), (240, 240, 240),
        (255, 128, 0), (0, 255, 128), (128, 0, 255), (250, 5, 5),
        (5, 250, 5), (5, 5, 250), (1, 1, 1), (254, 254, 254),
    ]

    public static let spaces: [(String, CFString)] = [
        ("sRGB", CGColorSpace.sRGB),
        ("Display P3", CGColorSpace.displayP3),
        ("Adobe RGB (1998)", CGColorSpace.adobeRGB1998),
    ]

    /// A chart of 8 x 4 patches. Each document has a different odd size and margin so a
    /// switch also changes the texture size.
    public static func chart(_ index: Int) -> TestDocument {
        let (name, cfName) = spaces[index]
        let space = CGColorSpace(name: cfName)!
        let margin = 3 + index * 2
        let patch = 11
        let width = margin * 2 + patch * 8 + index
        let height = margin * 2 + patch * 4 + index * 3
        var rgba = [UInt8](repeating: 0, count: width * height * 4)
        for i in 0..<(width * height) {
            rgba[i * 4 + 0] = 100
            rgba[i * 4 + 1] = 100
            rgba[i * 4 + 2] = 100
            rgba[i * 4 + 3] = 255
        }
        var patches: [Patch] = []
        for (k, v) in values.enumerated() {
            let p = Patch(x: margin + (k % 8) * patch, y: margin + (k / 8) * patch, size: patch, rgb: v)
            for y in p.y..<(p.y + patch) {
                for x in p.x..<(p.x + patch) {
                    let o = (y * width + x) * 4
                    rgba[o] = v.0
                    rgba[o + 1] = v.1
                    rgba[o + 2] = v.2
                }
            }
            patches.append(p)
        }
        return TestDocument(name: name, width: width, height: height, rgba: rgba, space: space, patches: patches)
    }

    public static var all: [TestDocument] { (0..<spaces.count).map(chart) }

    /// A large opaque sRGB document for frame times (smooth gradients plus a fine grid).
    public static func big(width: Int = 6000, height: Int = 4000) -> TestDocument {
        var rgba = [UInt8](repeating: 255, count: width * height * 4)
        rgba.withUnsafeMutableBufferPointer { buf in
            for y in 0..<height {
                let gy = UInt8(truncatingIfNeeded: y * 255 / height)
                for x in 0..<width {
                    let o = (y * width + x) * 4
                    let grid: UInt8 = (x % 64 == 0 || y % 64 == 0) ? 0 : 255
                    buf[o] = UInt8(truncatingIfNeeded: x * 255 / width) & grid
                    buf[o + 1] = gy & grid
                    buf[o + 2] = UInt8(truncatingIfNeeded: (x ^ y) & 0xff)
                }
            }
        }
        return TestDocument(name: "big sRGB", width: width, height: height, rgba: rgba,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!, patches: [])
    }
}

/// The ColorSync reference: Core Graphics draws the document into a 32-bit float context
/// in the display space (relative colorimetric, no interpolation). Returns per-patch
/// expected display values in 0...1.
public enum Reference {
    private static var cache: [String: [[Float]]] = [:]

    public static func expected(_ doc: TestDocument, display: CGColorSpace) -> [[Float]] {
        let key = "\(doc.name)|\((display.copyICCData()! as Data).hashValue)"
        if let hit = cache[key] { return hit }
        let provider = CGDataProvider(data: Data(doc.rgba) as CFData)!
        let image = CGImage(width: doc.width, height: doc.height, bitsPerComponent: 8, bitsPerPixel: 32,
                            bytesPerRow: doc.width * 4, space: doc.space,
                            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                            provider: provider, decode: nil, shouldInterpolate: false,
                            intent: .relativeColorimetric)!
        let info = CGImageAlphaInfo.premultipliedLast.rawValue
            | CGBitmapInfo.floatComponents.rawValue
            | CGBitmapInfo.byteOrder32Little.rawValue
        let ctx = CGContext(data: nil, width: doc.width, height: doc.height, bitsPerComponent: 32,
                            bytesPerRow: doc.width * 16, space: display, bitmapInfo: info)!
        ctx.interpolationQuality = .none
        ctx.setRenderingIntent(.relativeColorimetric)
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: doc.width, height: doc.height))
        let data = ctx.data!.assumingMemoryBound(to: Float.self)
        var out: [[Float]] = []
        for p in doc.patches {
            // Core Graphics rows start at the top in memory for bitmap contexts.
            let o = (p.centerY * doc.width + p.centerX) * 4
            out.append((0..<3).map { min(max(data[o + $0], 0), 1) })
        }
        cache[key] = out
        return out
    }
}

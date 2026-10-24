// The trace's test image: a 6000x4000 RGBA PNG with gradients, fine detail and a
// band of partial transparency, so every zoom level and the checkerboard show.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

public enum TraceImage {
    public static let width = 6000
    public static let height = 4000

    public enum Failure: Error { case context, destination, write }

    /// Writes the PNG to `url` (about 1 to 3 s).
    public static func write(to url: URL, width: Int = width, height: Int = height) throws {
        let rowBytes = width * 4
        var pixels = [UInt8](repeating: 0, count: rowBytes * height)
        // 64 distinct rows, copied down the image with a vertical gradient mixed in
        // per block of 16 rows. Rows are built once, so a debug build stays fast.
        var rows = [[UInt8]](repeating: [UInt8](repeating: 0, count: rowBytes), count: 64)
        for k in 0..<64 {
            var row = rows[k]
            for x in 0..<width {
                let i = x * 4
                let grid = (x % 64 == 0 || k == 0) ? 0 : 1
                row[i] = UInt8((x * 255 / max(width - 1, 1)) & 0xFF) * UInt8(grid)
                row[i + 1] = UInt8((x ^ (k * 4)) & 0xFF)
                row[i + 2] = UInt8((k * 4 + x / 32) & 0xFF)
                row[i + 3] = 255
            }
            rows[k] = row
        }
        pixels.withUnsafeMutableBytes { dst in
            for y in 0..<height {
                let base = dst.baseAddress!.advanced(by: y * rowBytes)
                rows[y % 64].withUnsafeBytes { src in base.copyMemory(from: src.baseAddress!, byteCount: rowBytes) }
                let band = y * 255 / max(height - 1, 1)
                // Blue tint grows down the image; a transparent band in the middle third.
                let p = base.assumingMemoryBound(to: UInt8.self)
                let transparent = y > height / 3 && y < height / 2
                var x = 0
                while x < width {
                    p[x * 4 + 2] = UInt8((Int(p[x * 4 + 2]) + band) / 2)
                    if transparent { p[x * 4 + 3] = UInt8((x * 255 / max(width - 1, 1)) & 0xFF) }
                    x += transparent ? 1 : 97
                }
            }
        }
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        // Straight (non-premultiplied) RGBA, as the engine stores it.
        guard let provider = CGDataProvider(data: Data(pixels) as CFData) else { throw Failure.context }
        let image = CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: rowBytes,
            space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        guard let image else { throw Failure.context }
        guard let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)
        else { throw Failure.destination }
        CGImageDestinationAddImage(dest, image, nil)
        guard CGImageDestinationFinalize(dest) else { throw Failure.write }
    }
}

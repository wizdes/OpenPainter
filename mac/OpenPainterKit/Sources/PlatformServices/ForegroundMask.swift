// The foreground mask for Layers > Remove Background (M53), from Apple Vision.
//
// The Vision calls are in Objective-C (mac/VisionMask, OPForegroundMask.m,
// which cites the Vision pages used). Swift here must not `import Vision`:
// Vision's Swift overlay would then load into every module that imports
// OpenPainterKit and change type inference in unrelated code (see
// mac/VisionMask/Package.swift).
//
// Vision gives the mask at the input's own size with soft edges; it is
// rounded to one byte per pixel. The work is synchronous and slow (hundreds of
// milliseconds), so callers run it off the main thread
// (`RemoveBackgroundController` does).
import CoreGraphics
import Foundation
import VisionMask

public enum ForegroundMaskError: Error, Equatable, CustomStringConvertible {
    case cancelled
    case badInput(String)
    case vision(String)

    public var description: String {
        switch self {
        case .cancelled: "cancelled"
        case let .badInput(s): "bad input: \(s)"
        case let .vision(s): "Vision failed: \(s)"
        }
    }
}

/// One mask computation that another thread can cancel.
public final class ForegroundMaskJob: @unchecked Sendable {
    private let job = OPForegroundMaskJob()

    public init() {}

    public var isCancelled: Bool { job.isCancelled }

    /// Stops the Vision request, if it runs; `run` then throws `.cancelled`.
    public func cancel() { job.cancel() }

    /// The foreground mask of straight RGBA8 pixels: width x height bytes,
    /// rows top first, 0 background to 255 foreground. Nil when Vision finds
    /// no foreground. Blocks; call it off the main thread.
    public func run(rgba: [UInt8], width: Int, height: Int) throws -> [UInt8]? {
        guard width > 0, height > 0, rgba.count == width * height * 4 else {
            throw ForegroundMaskError.badInput("\(rgba.count) bytes for \(width) x \(height)")
        }
        return try run(image: try Self.cgImage(rgba: rgba, width: width, height: height))
    }

    /// The foreground mask of `image` at its own size (see `run(rgba:)`).
    public func run(image: CGImage) throws -> [UInt8]? {
        let data: Data
        do {
            data = try job.mask(for: image)
        } catch let e as NSError where e.domain == OPForegroundMaskErrorDomain {
            if e.code == OPForegroundMaskError.cancelled.rawValue { throw ForegroundMaskError.cancelled }
            let under = (e.userInfo[NSUnderlyingErrorKey] as? NSError).map { ": \($0.localizedDescription)" } ?? ""
            throw ForegroundMaskError.vision(e.localizedDescription + under)
        }
        if data.isEmpty { return nil }
        return [UInt8](data)
    }

    /// Straight RGBA8 as a CGImage (a premultiplied copy).
    static func cgImage(rgba: [UInt8], width: Int, height: Int) throws -> CGImage {
        var premultiplied = rgba
        for i in stride(from: 0, to: premultiplied.count, by: 4) {
            let a = UInt16(premultiplied[i + 3])
            if a == 255 { continue }
            for k in 0..<3 { premultiplied[i + k] = UInt8((UInt16(premultiplied[i + k]) * a + 127) / 255) }
        }
        guard let provider = CGDataProvider(data: Data(premultiplied) as CFData),
              let image = CGImage(
                  width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { throw ForegroundMaskError.badInput("could not make an image of \(width) x \(height)") }
        return image
    }
}

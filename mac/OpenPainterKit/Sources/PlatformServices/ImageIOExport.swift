// ImageIO export for HEIC and, where macOS can encode it, AVIF (M17); M19 adds
// JPEG, BMP, TIFF, GIF and WebP (where macOS can encode it).
//
// The engine's straight RGBA8 pixels become a CGImage tagged with the document
// profile, and CGImageDestination writes it
// (https://developer.apple.com/documentation/imageio/cgimagedestination).
// kCGImageDestinationLossyCompressionQuality is 0.9 for now; M50 adds the
// save dialog. Kept metadata goes back in with CGImageDestinationAddImageAndMetadata
// (https://developer.apple.com/documentation/imageio/cgimagedestinationaddimageandmetadata(_:_:_:_:)).
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

public enum ImageIOExportError: Error, LocalizedError, Equatable {
    case unsupportedType(String)
    case badPixels
    case encode(String)

    public var errorDescription: String? {
        switch self {
        case .unsupportedType(let t): "This Mac cannot save \(t) files."
        case .badPixels: "The image's pixels could not be read for saving."
        case .encode(let t): "ImageIO could not write the \(t) file."
        }
    }
}

public enum ImageIOExporter {
    public static let defaultQuality = 0.9

    /// Encodes straight RGBA8 pixels as `type`: HEIC or AVIF (M17), or since
    /// M19 any of `ImageFormats.imageIOSaveTypes()` (JPEG, BMP, TIFF, GIF, WebP).
    public static func encode(
        rgba: [UInt8], width: Int, height: Int, icc: Data?, type: String,
        quality: Double = defaultQuality, metadata: ImageMetadata? = nil,
        destinationTypes: [String] = ImageFormats.systemDestinationTypes
    ) throws -> Data {
        guard ImageFormats.imageIOSaveTypes(destinationTypes: destinationTypes).contains(type)
        else { throw ImageIOExportError.unsupportedType(UTType(type)?.preferredFilenameExtension?.uppercased() ?? type) }
        guard width > 0, height > 0, rgba.count == width * height * 4 else { throw ImageIOExportError.badPixels }
        let image = try cgImage(rgba: rgba, width: width, height: height, icc: icc)
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out as CFMutableData, type as CFString, 1, nil) else {
            throw ImageIOExportError.encode(type)
        }
        var options: [CFString: Any] = [
            kCGImageDestinationLossyCompressionQuality: quality,
            kCGImagePropertyOrientation: 1,
        ]
        if let metadata {
            for (k, v) in metadata.properties { options[k as CFString] = v }
        }
        if let xmp = metadata?.xmp, let md = CGImageMetadataCreateFromXMPData(xmp as CFData) {
            CGImageDestinationAddImageAndMetadata(dest, image, md, options as CFDictionary)
        } else {
            CGImageDestinationAddImage(dest, image, options as CFDictionary)
        }
        guard CGImageDestinationFinalize(dest) else { throw ImageIOExportError.encode(type) }
        return out as Data
    }

    /// A CGImage over straight RGBA8, tagged with the ICC profile (sRGB when nil).
    /// Opaque images drop the alpha channel so HEIC writes no alpha plane.
    public static func cgImage(rgba: [UInt8], width: Int, height: Int, icc: Data?) throws -> CGImage {
        let space = icc.flatMap { CGColorSpace(iccData: $0 as CFData) } ?? CGColorSpace(name: CGColorSpace.sRGB)!
        var opaque = true
        for i in stride(from: 3, to: rgba.count, by: 4) where rgba[i] != 255 {
            opaque = false
            break
        }
        let alpha: CGImageAlphaInfo = opaque ? .noneSkipLast : .last
        guard let provider = CGDataProvider(data: Data(rgba) as CFData),
              let image = CGImage(
                width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                space: space, bitmapInfo: CGBitmapInfo(rawValue: alpha.rawValue), provider: provider, decode: nil,
                shouldInterpolate: false, intent: .defaultIntent)
        else { throw ImageIOExportError.badPixels }
        return image
    }
}

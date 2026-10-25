// ImageIO import for HEIC, HEIF, AVIF and Camera RAW (M17).
//
// Steps: CGImageSourceCreateImageAtIndex decodes the first image
// (https://developer.apple.com/documentation/imageio/cgimagesourcecreateimageatindex(_:_:_:)).
// vImage converts it to straight (not premultiplied) 8-bit RGBA in the image's
// own RGB color space, so no color conversion happens for RGB sources. The
// EXIF orientation (kCGImagePropertyOrientation,
// https://developer.apple.com/documentation/imageio/kcgimagepropertyorientation)
// is applied to the pixels, and the metadata keeps orientation 1 from then on.
// The color space's ICC bytes become the document profile.
//
// 8-bit storage is the project rule: a 16-bit, float or HDR source is converted
// to 8-bit and the result carries a note for the status bar.
import Accelerate
import CoreGraphics
import Foundation
import ImageIO

/// The metadata of an imported image, kept for M50's save options. The engine
/// has no metadata slot, so the Swift document holds it.
public struct ImageMetadata: @unchecked Sendable {
    /// All of the image's metadata (EXIF, TIFF, GPS, IPTC and XMP namespaces) as
    /// one XMP packet, from CGImageMetadataCreateXMPData. Orientation is 1.
    public var xmp: Data?
    /// The EXIF, TIFF, GPS, IPTC and ExifAux property dictionaries ImageIO reports
    /// ("{Exif}", "{GPS}", ...), with orientation reset to 1.
    public var properties: [String: Any]

    public init(xmp: Data? = nil, properties: [String: Any] = [:]) {
        self.xmp = xmp
        self.properties = properties
    }

    public var isEmpty: Bool { xmp == nil && properties.isEmpty }
    public var exif: [String: Any]? { properties[kCGImagePropertyExifDictionary as String] as? [String: Any] }
    public var gps: [String: Any]? { properties[kCGImagePropertyGPSDictionary as String] as? [String: Any] }

    static let keptDictionaries: [CFString] = [
        kCGImagePropertyExifDictionary, kCGImagePropertyExifAuxDictionary, kCGImagePropertyTIFFDictionary,
        kCGImagePropertyGPSDictionary, kCGImagePropertyIPTCDictionary,
    ]
}

/// A decoded image ready for `op_import_pixels`.
public struct ImportedImage: Sendable {
    public var width: Int
    public var height: Int
    /// Straight RGBA, 8 bits per channel, row by row, top row first.
    public var rgba: [UInt8]
    /// ICC bytes of the document profile.
    public var icc: Data
    /// The color space's name, such as "kCGColorSpaceDisplayP3" (diagnostics).
    public var colorSpaceName: String?
    /// The source's EXIF orientation (1 to 8), already applied to `rgba`.
    public var sourceOrientation: Int
    /// The source's bits per channel.
    public var sourceBitsPerComponent: Int
    /// The ImageIO type, such as "public.heic" or "com.sony.arw-raw-image".
    public var sourceType: String
    public var metadata: ImageMetadata
    /// Messages for the status bar (8-bit conversion, color space changes).
    public var notes: [String]
}

public enum ImageIOImportError: Error, LocalizedError, Equatable {
    case cannotRead(String)
    case noImage(String)
    case convert(String)

    public var errorDescription: String? {
        switch self {
        case .cannotRead(let n): "ImageIO could not read “\(n)”."
        case .noImage(let n): "“\(n)” has no image ImageIO can decode."
        case .convert(let n): "“\(n)” could not be converted to 8-bit RGBA."
        }
    }
}

public enum ImageIOImporter {
    public static func decode(url: URL) throws -> ImportedImage {
        let name = url.lastPathComponent
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else {
            throw ImageIOImportError.cannotRead(name)
        }
        return try decode(source: source, name: name)
    }

    public static func decode(data: Data, name: String = "image") throws -> ImportedImage {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else {
            throw ImageIOImportError.cannotRead(name)
        }
        return try decode(source: source, name: name)
    }

    static func decode(source: CGImageSource, name: String) throws -> ImportedImage {
        let type = CGImageSourceGetType(source) as String? ?? "unknown"
        let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] ?? [:]
        let orientation = (props[kCGImagePropertyOrientation as String] as? NSNumber)?.intValue ?? 1
        guard var image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw ImageIOImportError.noImage(name)
        }
        var notes: [String] = []
        // HDR sources: ask ImageIO for its SDR rendering, then store 8-bit.
        if let space = image.colorSpace, isHDR(space) {
            let sdr = [kCGImageSourceDecodeRequest: kCGImageSourceDecodeToSDR] as CFDictionary
            if let mapped = CGImageSourceCreateImageAtIndex(source, 0, sdr) { image = mapped }
            notes.append("HDR image shown as SDR, 8 bits per channel (OpenPainter stores 8-bit).")
        } else if image.bitsPerComponent > 8 {
            let kind = image.bitmapInfo.contains(.floatComponents) ? "\(image.bitsPerComponent)-bit float" : "\(image.bitsPerComponent)-bit"
            notes.append("Converted from \(kind) to 8 bits per channel (OpenPainter stores 8-bit).")
        }
        let (target, converted) = documentSpace(for: image.colorSpace)
        if converted { notes.append("Converted to sRGB: the source color space is not RGB.") }
        guard let icc = target.copyICCData() as Data? else { throw ImageIOImportError.convert(name) }
        var (rgba, w, h) = try straightRGBA(image, space: target, name: name)
        if orientation > 1, orientation <= 8 {
            (rgba, w, h) = try orient(rgba, width: w, height: h, orientation: orientation, name: name)
        }
        return ImportedImage(
            width: w, height: h, rgba: rgba, icc: icc, colorSpaceName: target.name as String?,
            sourceOrientation: orientation, sourceBitsPerComponent: image.bitsPerComponent, sourceType: type,
            metadata: metadata(source, props), notes: notes)
    }

    static func isHDR(_ space: CGColorSpace) -> Bool {
        CGColorSpaceUsesITUR_2100TF(space) || CGColorSpaceUsesExtendedRange(space)
    }

    /// The image's own color space when it is RGB with ICC data; sRGB otherwise.
    static func documentSpace(for space: CGColorSpace?) -> (CGColorSpace, converted: Bool) {
        let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
        guard let space else { return (srgb, false) }
        if space.model == .rgb, !isHDR(space), space.copyICCData() != nil { return (space, false) }
        if space.model == .rgb { return (srgb, false) }  // HDR or no ICC: ImageIO already rendered RGB
        return (srgb, true)
    }

    /// Converts any CGImage to straight RGBA8 in `space` with vImage.
    static func straightRGBA(_ image: CGImage, space: CGColorSpace, name: String) throws -> ([UInt8], Int, Int) {
        guard var format = vImage_CGImageFormat(
            bitsPerComponent: 8, bitsPerPixel: 32, colorSpace: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), renderingIntent: .defaultIntent)
        else { throw ImageIOImportError.convert(name) }
        var buffer = vImage_Buffer()
        let err = vImageBuffer_InitWithCGImage(&buffer, &format, nil, image, vImage_Flags(kvImageNoFlags))
        guard err == kvImageNoError, let base = buffer.data else { throw ImageIOImportError.convert(name) }
        defer { free(base) }
        let w = Int(buffer.width), h = Int(buffer.height)
        var out = [UInt8](repeating: 0, count: w * h * 4)
        out.withUnsafeMutableBytes { dst in
            for y in 0..<h {
                memcpy(dst.baseAddress! + y * w * 4, base + y * buffer.rowBytes, w * 4)
            }
        }
        // An image with no alpha channel is opaque.
        if image.alphaInfo == .none || image.alphaInfo == .noneSkipLast || image.alphaInfo == .noneSkipFirst {
            for i in stride(from: 3, to: out.count, by: 4) { out[i] = 255 }
        }
        return (out, w, h)
    }

    /// Applies EXIF orientation 2 to 8 to RGBA8 pixels.
    /// 2 mirror, 3 rotate 180, 4 flip, 5 transpose, 6 rotate 90 clockwise,
    /// 7 transverse, 8 rotate 90 counterclockwise.
    public static func orient(_ rgba: [UInt8], width: Int, height: Int, orientation: Int,
                              name: String = "image") throws -> ([UInt8], Int, Int) {
        enum Step { case mirror, flip, rotate(Int) }
        let steps: [Step] =
            switch orientation {
            case 2: [.mirror]
            case 3: [.rotate(kRotate180DegreesClockwise)]
            case 4: [.flip]
            case 5: [.rotate(kRotate90DegreesClockwise), .mirror]
            case 6: [.rotate(kRotate90DegreesClockwise)]
            case 7: [.rotate(kRotate90DegreesClockwise), .flip]
            case 8: [.rotate(kRotate90DegreesCounterClockwise)]
            default: []
            }
        var (pixels, w, h) = (rgba, width, height)
        for step in steps {
            var turns = false
            if case .rotate(let r) = step { turns = r == kRotate90DegreesClockwise || r == kRotate90DegreesCounterClockwise }
            let (ow, oh) = turns ? (h, w) : (w, h)
            var out = [UInt8](repeating: 0, count: pixels.count)
            let err: vImage_Error = pixels.withUnsafeMutableBytes { s in
                out.withUnsafeMutableBytes { d in
                    var a = vImage_Buffer(data: s.baseAddress, height: vImagePixelCount(h),
                                          width: vImagePixelCount(w), rowBytes: w * 4)
                    var b = vImage_Buffer(data: d.baseAddress, height: vImagePixelCount(oh),
                                          width: vImagePixelCount(ow), rowBytes: ow * 4)
                    let none = vImage_Flags(kvImageNoFlags)
                    switch step {
                    case .mirror: return vImageHorizontalReflect_ARGB8888(&a, &b, none)
                    case .flip: return vImageVerticalReflect_ARGB8888(&a, &b, none)
                    case .rotate(let r):
                        var clear: [UInt8] = [0, 0, 0, 0]
                        return vImageRotate90_ARGB8888(&a, &b, UInt8(r), &clear, none)
                    }
                }
            }
            guard err == kvImageNoError else { throw ImageIOImportError.convert(name) }
            (pixels, w, h) = (out, ow, oh)
        }
        return (pixels, w, h)
    }

    /// The file's resolution in pixels per inch (kCGImagePropertyDPIWidth), or
    /// nil when the file states none or one out of range (M50).
    public static func resolution(url: URL) -> Double? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any],
              let dpi = (props[kCGImagePropertyDPIWidth as String] as? NSNumber)?.doubleValue,
              dpi.isFinite, dpi >= 1, dpi <= 100_000
        else { return nil }
        return dpi
    }

    /// A file's EXIF, TIFF, GPS, IPTC and XMP metadata (empty when ImageIO
    /// cannot read the file), for files the engine decodes itself (M50).
    public static func metadata(url: URL) -> ImageMetadata {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return ImageMetadata() }
        let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] ?? [:]
        return metadata(source, props)
    }

    /// The source's metadata with orientation reset to 1 (the pixels are turned).
    static func metadata(_ source: CGImageSource, _ props: [String: Any]) -> ImageMetadata {
        var kept: [String: Any] = [:]
        for key in ImageMetadata.keptDictionaries {
            guard var d = props[key as String] as? [String: Any] else { continue }
            if key == kCGImagePropertyTIFFDictionary, d[kCGImagePropertyTIFFOrientation as String] != nil {
                d[kCGImagePropertyTIFFOrientation as String] = 1
            }
            kept[key as String] = d
        }
        var xmp: Data?
        if let md = CGImageSourceCopyMetadataAtIndex(source, 0, nil),
           let mutable = CGImageMetadataCreateMutableCopy(md) {
            _ = CGImageMetadataSetValueMatchingImageProperty(
                mutable, kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFOrientation, 1 as CFNumber)
            xmp = CGImageMetadataCreateXMPData(mutable, nil) as Data?
        }
        return ImageMetadata(xmp: xmp, properties: kept)
    }
}

// M50 save options, per format, as paint.net's Save Configuration dialog
// offers them (https://www.getpaint.net/doc/latest/FileMenu.html, Save: "the
// Save Configuration dialog will be presented ... When saving JPEG images, the
// quality of the image can be configured. For GIF images, transparency and
// dithering options are presented"). JPEG quality defaults to 95 and chroma
// subsampling to 4:2:2, paint.net's defaults
// (https://www.paint.net/doc/api/preview/api/PaintDotNet.FileTypes.IJpegFileTypeSaveOptions.html).
//
// Who encodes what:
// - PNG, JPEG and GIF: the engine (op_encode_image). ImageIO has no chroma
//   subsampling key (its JPEG writer uses 4:2:0 below quality 1.0 and 4:4:4 at
//   1.0, measured on macOS 27), and no GIF dithering switch. The engine's PNG
//   keeps every pixel byte, including colors under zero alpha, and writes EXIF
//   (eXIf), XMP (iTXt) and the resolution (pHYs).
// - TIFF, HEIC, AVIF, WebP and BMP: ImageIO. TIFF compression is the TIFF
//   dictionary's Compression tag: 1 none, 5 LZW, 8 Deflate.
// - HEIC and AVIF have no lossless mode here: HEIC at quality 1.0 still
//   changes pixels (largest difference 2 levels) and AVIF at 1.0 fails to
//   encode, so quality tops out at 99 for AVIF and no lossless box is shown.
// - WebP options appear only when this Mac can encode WebP (ImageIO lists it).
import Foundation
import UniformTypeIdentifiers

public enum ChromaSubsampling: String, CaseIterable, Codable, Sendable {
    case s444 = "444", s422 = "422", s420 = "420"

    public var label: String {
        switch self {
        case .s444: "4:4:4 (best quality)"
        case .s422: "4:2:2 (default)"
        case .s420: "4:2:0 (best compression)"
        }
    }
}

public enum TIFFCompression: String, CaseIterable, Codable, Sendable {
    case none, lzw, deflate

    public var label: String {
        switch self {
        case .none: "None"
        case .lzw: "LZW"
        case .deflate: "Deflate (ZIP)"
        }
    }

    /// The TIFF Compression tag value ImageIO writes.
    public var tag: Int {
        switch self {
        case .none: 1
        case .lzw: 5
        case .deflate: 8
        }
    }
}

/// What metadata the saved file keeps.
public enum MetadataChoice: String, CaseIterable, Codable, Sendable {
    /// EXIF, XMP, GPS and IPTC as imported.
    case keepAll
    /// Everything but location: GPS goes from EXIF and from XMP.
    case stripLocation
    /// No EXIF, XMP, GPS or IPTC.
    case stripAll

    public var label: String {
        switch self {
        case .keepAll: "Keep all"
        case .stripLocation: "Remove location (GPS)"
        case .stripAll: "Remove all (EXIF, XMP, GPS, IPTC)"
        }
    }
}

/// The options of one save. Each format reads only its own fields.
public struct SaveOptions: Equatable, Codable, Sendable {
    /// JPEG quality, 1 to 100.
    public var jpegQuality = 95
    public var jpegSubsampling = ChromaSubsampling.s422
    /// PNG deflate level, 0 (none) to 9 (smallest).
    public var pngLevel = 6
    public var tiffCompression = TIFFCompression.lzw
    /// HEIC, AVIF and WebP quality, 1 to 100.
    public var quality = 90
    /// GIF Floyd-Steinberg dithering.
    public var gifDither = true
    public var metadata = MetadataChoice.keepAll

    public init() {}
}

/// The kinds of save options, by format.
public enum SaveOptionKind: Sendable {
    case jpeg, png, tiff, heic, avif, webp, gif

    public init?(type: String) {
        switch type {
        case "public.jpeg": self = .jpeg
        case "public.png": self = .png
        case "public.tiff": self = .tiff
        case "public.heic": self = .heic
        case "public.avif": self = .avif
        case "org.webmproject.webp": self = .webp
        case "com.compuserve.gif": self = .gif
        default: return nil
        }
    }

    /// Lossy formats show an encoded preview image next to the size.
    public var isLossy: Bool { [.jpeg, .heic, .avif, .webp, .gif].contains(self) }
    /// Formats that carry EXIF and XMP (GIF does not).
    public var holdsMetadata: Bool { self != .gif }
    /// The highest quality ImageIO encodes for HEIC, AVIF and WebP.
    public var maxQuality: Int { self == .avif ? 99 : 100 }
}

extension SaveOptions {
    /// The options for `type` with values clamped to what that format takes.
    public func clamped(for type: String) -> SaveOptions {
        var o = self
        o.jpegQuality = min(max(o.jpegQuality, 1), 100)
        o.pngLevel = min(max(o.pngLevel, 0), 9)
        let top = SaveOptionKind(type: type)?.maxQuality ?? 100
        o.quality = min(max(o.quality, 1), top)
        return o
    }
}

/// The last options per format, kept in UserDefaults.
public final class SaveOptionsStore: @unchecked Sendable {
    public let defaults: UserDefaults
    public static let keyPrefix = "OpenPainter.SaveOptions."

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public func key(_ type: String) -> String { Self.keyPrefix + type }

    /// The remembered options for `type`, or nil when none were saved.
    public func load(_ type: String) -> SaveOptions? {
        guard let data = defaults.data(forKey: key(type)) else { return nil }
        do {
            return try JSONDecoder().decode(SaveOptions.self, from: data)
        } catch {
            FileHandle.standardError.write(Data("OpenPainter: ignoring unreadable save options for \(type): \(error)\n".utf8))
            return nil
        }
    }

    /// The remembered options, else the defaults.
    public func options(_ type: String) -> SaveOptions { (load(type) ?? SaveOptions()).clamped(for: type) }

    public func save(_ options: SaveOptions, for type: String) {
        do {
            defaults.set(try JSONEncoder().encode(options), forKey: key(type))
        } catch {
            FileHandle.standardError.write(Data("OpenPainter: could not remember save options for \(type): \(error)\n".utf8))
        }
    }
}

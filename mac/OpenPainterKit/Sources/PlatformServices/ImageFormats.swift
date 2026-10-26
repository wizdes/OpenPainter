// Which files OpenPainter can open and which formats Save As offers on this Mac.
//
// The engine decodes PNG itself (op_doc_open_png). ImageIO decodes HEIC, HEIF,
// AVIF and every Camera RAW type that CGImageSourceCopyTypeIdentifiers() lists
// (https://developer.apple.com/documentation/imageio/cgimagesourcecopytypeidentifiers()).
// HEIC export always goes through ImageIO; AVIF export is offered only when
// CGImageDestinationCopyTypeIdentifiers() lists "public.avif"
// (https://developer.apple.com/documentation/imageio/cgimagedestinationcopytypeidentifiers()).
import Foundation
import ImageIO
import UniformTypeIdentifiers

public enum ImageFormats {
    /// Formats the engine opens itself today.
    public static let engineTypes: [UTType] = [.png]

    /// The ImageIO types OpenPainter imports: HEIC, HEIF, AVIF and Camera RAW,
    /// limited to what this macOS can decode.
    public static func imageIOImportTypes(sourceTypes: [String] = systemSourceTypes) -> [UTType] {
        let wanted: Set<String> = ["public.heic", "public.heif", "public.avif"]
        return sourceTypes.compactMap { id in
            guard let t = UTType(id) else { return nil }
            return wanted.contains(id) || t.conforms(to: .rawImage) ? t : nil
        }
    }

    /// Every type a file may have to open in OpenPainter.
    public static func openTypes() -> [UTType] { [opaintType] + engineTypes + imageIOImportTypes() + flatImportTypes() }

    /// The types Save As offers: HEIC always (when ImageIO can write it), AVIF
    /// only when this macOS can encode it.
    public static func exportTypes(destinationTypes: [String] = systemDestinationTypes) -> [UTType] {
        ["public.heic", "public.avif"].filter(destinationTypes.contains).compactMap(UTType.init)
    }

    /// True when CGImageDestinationCopyTypeIdentifiers() lists AVIF.
    public static var canEncodeAVIF: Bool { systemDestinationTypes.contains("public.avif") }

    public static var systemSourceTypes: [String] {
        CGImageSourceCopyTypeIdentifiers() as? [String] ?? []
    }

    public static var systemDestinationTypes: [String] {
        CGImageDestinationCopyTypeIdentifiers() as? [String] ?? []
    }

    /// How a file opens: by the engine, by ImageIO, or not at all. The decision
    /// reads the file's first bytes (ImageIO's own type check), so a file with a
    /// wrong extension still opens the right way.
    public enum Route: Equatable, Sendable {
        case engine
        /// A native `.opaint` file (M19), opened by the engine with all layers.
        case native
        case imageIO(String)
        case unsupported
    }

    public static func route(for url: URL) -> Route {
        if let h = try? FileHandle(forReadingFrom: url) {
            defer { try? h.close() }
            let head = (try? h.read(upToCount: 128)) ?? Data()
            if head.prefix(8) == Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
                return .engine
            }
            if isOpaint(head) { return .native }
        } else {
            return .unsupported
        }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let type = CGImageSourceGetType(source) as String?
        else { return .unsupported }
        let ids = Set(imageIOImportTypes().map(\.identifier) + flatImportTypes().map(\.identifier))
        return ids.contains(type) ? .imageIO(type) : .unsupported
    }

    /// Fix round 2, items 1 and 3: true when the file at `url` holds more
    /// than 8 bits per channel (a 16-bit PNG or TIFF, a float image), read
    /// from its header only. OpenPainter keeps 8 bits, so it never saves
    /// over such a file without asking where.
    public static func holdsMoreThan8Bits(_ url: URL) -> Bool {
        if let depth = DocumentLoader.pngBitDepth(url) { return depth > 8 }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        else { return false }
        let depth = (props[kCGImagePropertyDepth as String] as? NSNumber)?.intValue ?? 8
        return depth > 8 || (props[kCGImagePropertyIsFloat as String] as? Bool ?? false)
    }

    /// A short list of what opens, for messages.
    public static let openSummary = "OpenPainter (.opaint), PNG, JPEG, BMP, GIF, TIFF, WebP, HEIC, HEIF, AVIF and Camera RAW"
}

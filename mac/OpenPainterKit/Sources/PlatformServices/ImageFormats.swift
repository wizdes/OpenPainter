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
    public static func openTypes() -> [UTType] { engineTypes + imageIOImportTypes() }

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
        case imageIO(String)
        case unsupported
    }

    public static func route(for url: URL) -> Route {
        if let h = try? FileHandle(forReadingFrom: url) {
            defer { try? h.close() }
            if let head = try? h.read(upToCount: 8), head == Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
                return .engine
            }
        } else {
            return .unsupported
        }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let type = CGImageSourceGetType(source) as String?
        else { return .unsupported }
        let ids = Set(imageIOImportTypes().map(\.identifier))
        return ids.contains(type) ? .imageIO(type) : .unsupported
    }

    /// A short list of what opens, for messages.
    public static let openSummary = "PNG, HEIC, HEIF, AVIF and Camera RAW"
}

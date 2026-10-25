// Opens a file into an engine document and saves a document through ImageIO.
// The app's NSDocument and the tests use the same two calls.
import CoreGraphics
import Foundation
import UniformTypeIdentifiers

/// An opened file: the engine document plus what the Swift side keeps.
public struct LoadedImage: @unchecked Sendable {
    public let document: Document
    public let pixelSize: CGSize
    /// EXIF, XMP and GPS from an ImageIO import (empty for engine-decoded files).
    public let metadata: ImageMetadata
    /// Status-bar notes, such as the 16-bit to 8-bit conversion.
    public let notes: [String]
    /// The source's ImageIO type, or "public.png" for the engine's PNG reader.
    public let sourceType: String
}

public enum DocumentLoaderError: Error, LocalizedError, Equatable {
    case unsupported(String)
    case engine(String, String)
    case empty(String)

    public var errorDescription: String? {
        switch self {
        case .unsupported(let n): "OpenPainter cannot open “\(n)”. It opens \(ImageFormats.openSummary) files."
        case .engine(let n, let e): "The engine could not open “\(n)” (\(e))."
        case .empty(let n): "“\(n)” has no pixels."
        }
    }
}

public enum DocumentLoader {
    /// Opens `url`: PNG through the engine, HEIC, HEIF, AVIF and Camera RAW
    /// through ImageIO and `op_import_pixels`.
    public static func open(_ url: URL, engine: Engine) throws -> LoadedImage {
        let name = url.lastPathComponent
        let doc: Document
        var metadata = ImageMetadata()
        var notes: [String] = []
        var type = "public.png"
        switch ImageFormats.route(for: url) {
        case .unsupported:
            throw DocumentLoaderError.unsupported(name)
        case .engine:
            do { doc = try engine.openPNG(path: url.path) } catch { throw DocumentLoaderError.engine(name, "\(error)") }
        case .imageIO:
            let image = try ImageIOImporter.decode(url: url)
            do {
                doc = try engine.importPixels(
                    width: UInt32(image.width), height: UInt32(image.height), rgba: image.rgba, icc: [UInt8](image.icc))
            } catch {
                throw DocumentLoaderError.engine(name, "\(error)")
            }
            metadata = image.metadata
            notes = image.notes
            type = image.sourceType
        }
        let info = try doc.info()
        let w = (info["width"] as? NSNumber)?.doubleValue ?? 0
        let h = (info["height"] as? NSNumber)?.doubleValue ?? 0
        guard w > 0, h > 0 else {
            try? doc.close()
            throw DocumentLoaderError.empty(name)
        }
        return LoadedImage(document: doc, pixelSize: CGSize(width: w, height: h), metadata: metadata, notes: notes,
                           sourceType: type)
    }

    /// Encodes the flattened image as HEIC or AVIF with the document profile
    /// (`doc.profile`) and the kept metadata.
    public static func export(_ doc: Document, type: String, metadata: ImageMetadata? = nil,
                              quality: Double = ImageIOExporter.defaultQuality) throws -> Data {
        let info = try doc.info()
        let w = (info["width"] as? NSNumber)?.intValue ?? 0
        let h = (info["height"] as? NSNumber)?.intValue ?? 0
        let icc = try doc.queryData("doc.profile")
        return try ImageIOExporter.encode(
            rgba: try doc.pixels(), width: w, height: h, icc: icc.isEmpty ? nil : icc, type: type,
            quality: quality, metadata: metadata)
    }
}

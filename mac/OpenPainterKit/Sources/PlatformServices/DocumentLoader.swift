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
    /// Fix round 2, item 1: the file holds more than 8 bits per channel (a
    /// 16-bit PNG or TIFF, a float or HDR image) and the document holds the
    /// 8-bit conversion, so saving over the file would lose its depth.
    public var reducedDepth = false
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
    /// Opens `url`: PNG and .opaint through the engine; HEIC, HEIF, AVIF,
    /// Camera RAW, JPEG, BMP, GIF, TIFF and WebP through ImageIO and
    /// `op_import_pixels`.
    public static func open(_ url: URL, engine: Engine) throws -> LoadedImage {
        let name = url.lastPathComponent
        let doc: Document
        var metadata = ImageMetadata()
        var notes: [String] = []
        var type = "public.png"
        var reducedDepth = false
        switch ImageFormats.route(for: url) {
        case .unsupported:
            throw DocumentLoaderError.unsupported(name)
        case .engine:
            do { doc = try engine.openPNG(path: url.path) } catch { throw DocumentLoaderError.engine(name, "\(error)") }
            // M50: the engine reads the pixels and pHYs; EXIF and XMP come from ImageIO.
            metadata = ImageIOImporter.metadata(url: url)
            // Fix round 2, item 1: the engine converts a 16-bit PNG to 8 bits,
            // and says so as the ImageIO route does.
            if let depth = pngBitDepth(url), depth > 8 {
                reducedDepth = true
                notes.append("Converted from \(depth)-bit to 8 bits per channel (OpenPainter stores 8-bit).")
            }
        case .native:
            do { doc = try engine.openOpaint(path: url.path) } catch { throw DocumentLoaderError.engine(name, "\(error)") }
            type = ImageFormats.opaintIdentifier
            // Release review item 5: the metadata the file kept.
            metadata = keptMetadata(doc)
        case .imageIO:
            let image = try ImageIOImporter.decode(url: url)
            do {
                doc = try engine.importPixels(
                    width: UInt32(image.width), height: UInt32(image.height), rgba: image.rgba, icc: [UInt8](image.icc),
                    dpi: ImageIOImporter.resolution(url: url) ?? 96)
            } catch {
                throw DocumentLoaderError.engine(name, "\(error)")
            }
            metadata = image.metadata
            notes = image.notes
            type = image.sourceType
            reducedDepth = image.sourceBitsPerComponent > 8 || notes.contains { $0.hasPrefix("HDR image") }
        }
        if type != ImageFormats.opaintIdentifier, !metadata.isEmpty {
            // Release review item 5: .opaint saves and recovery records keep it.
            do {
                try doc.setImageMetadata(try metadata.archived())
            } catch {
                notes.append("The image's metadata cannot be kept in OpenPainter files (\(error)).")
            }
        }
        let info = try doc.info()
        let w = (info["width"] as? NSNumber)?.doubleValue ?? 0
        let h = (info["height"] as? NSNumber)?.doubleValue ?? 0
        guard w > 0, h > 0 else {
            try? doc.close()
            throw DocumentLoaderError.empty(name)
        }
        var loaded = LoadedImage(document: doc, pixelSize: CGSize(width: w, height: h), metadata: metadata,
                                 notes: notes, sourceType: type)
        loaded.reducedDepth = reducedDepth
        return loaded
    }

    /// The bit depth in a PNG file's header (IHDR, which must come first),
    /// or nil when `url` is not a readable PNG.
    public static func pngBitDepth(_ url: URL) -> Int? {
        guard let h = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? h.close() }
        guard let head = try? h.read(upToCount: 29), head.count == 29,
              head.prefix(8) == Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
              head.subdata(in: 12..<16) == Data("IHDR".utf8)
        else { return nil }
        return Int(head[24])
    }

    /// The metadata an engine document keeps (from an .opaint file or a
    /// recovery record); empty when it has none.
    public static func keptMetadata(_ doc: Document) -> ImageMetadata {
        do {
            guard let text = try doc.imageMetadata() else { return ImageMetadata() }
            guard let m = ImageMetadata(archived: text) else {
                FileHandle.standardError.write(Data("OpenPainter: the kept image metadata cannot be read; it is left out\n".utf8))
                return ImageMetadata()
            }
            return m
        } catch {
            FileHandle.standardError.write(Data("OpenPainter: cannot read the kept image metadata: \(error)\n".utf8))
            return ImageMetadata()
        }
    }

    /// Encodes the flattened image as HEIC or AVIF with the document profile
    /// (`doc.profile_icc`) and the kept metadata.
    public static func export(_ doc: Document, type: String, metadata: ImageMetadata? = nil,
                              quality: Double = ImageIOExporter.defaultQuality) throws -> Data {
        let info = try doc.info()
        let w = (info["width"] as? NSNumber)?.intValue ?? 0
        let h = (info["height"] as? NSNumber)?.intValue ?? 0
        let icc = try doc.queryData("doc.profile_icc")
        return try ImageIOExporter.encode(
            rgba: try doc.pixels(), width: w, height: h, icc: icc.isEmpty ? nil : icc, type: type,
            quality: quality, metadata: metadata)
    }
}

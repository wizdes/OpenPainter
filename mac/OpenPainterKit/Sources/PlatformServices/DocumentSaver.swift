// Writes an engine document to a file (M19): .opaint through the engine
// (op_doc_save). Since M50 every flat format goes through SaveEncoder with the
// save options, the metadata choice and the resolution: PNG, JPEG and GIF are
// encoded by the engine, the others by ImageIO, and the app writes the bytes.
// Save and Save As mark the written history step as saved; Save a Copy does not.
import Foundation

public enum DocumentSaverError: Error, LocalizedError, Equatable {
    case unsupported(String)
    case busy

    public var errorDescription: String? {
        switch self {
        case .unsupported(let t): "OpenPainter cannot save \(ImageFormats.shortName(t)) files on this Mac."
        case .busy: "The image kept changing while it was being saved. Try again."
        }
    }
}

public enum DocumentSaver {
    /// Writes `doc` to `url` as `type` with `options` (the defaults when nil).
    /// `markSaved` false is Save a Copy. Returns false when the file was
    /// written but the image changed during the save, so it stays marked as
    /// modified.
    @discardableResult
    public static func write(_ doc: Document, to url: URL, type: String, metadata: ImageMetadata?,
                             options: SaveOptions? = nil, markSaved: Bool) throws -> Bool {
        switch type {
        case ImageFormats.opaintIdentifier:
            // The metadata travels inside the engine document (release review
            // item 5: DocumentLoader and ImageDocument keep it there).
            try doc.save(to: url.path, format: .opaint, markSaved: markSaved)
            return true
        default:
            guard type == "public.png" || ImageFormats.imageIOSaveTypes().contains(type) else {
                throw DocumentSaverError.unsupported(type)
            }
            let snap = try snapshot(doc)
            let data = try SaveEncoder.encode(snap.pixels, type: type, options: options ?? SaveOptions(),
                                              metadata: metadata)
            try data.write(to: url, options: .atomic)
            guard markSaved else { return true }
            do {
                try doc.markSaved(revision: snap.revision)
            } catch EngineError.busy {
                return false
            }
            return true
        }
    }

    struct Snapshot {
        var revision: UInt64
        var rgba: [UInt8]
        var width: Int
        var height: Int
        var icc: Data
        var dpi: Double

        var pixels: SavePixels { SavePixels(rgba: rgba, width: width, height: height, icc: icc, dpi: dpi) }
    }

    /// Pixels, size and profile read at one revision (re-read if an edit lands
    /// between the queries).
    static func snapshot(_ doc: Document) throws -> Snapshot {
        for _ in 0..<8 {
            let before = try doc.info()
            let rgba = try doc.pixels()
            let icc = try doc.queryData("doc.profile_icc")
            let dpi = try resolution(doc)
            let after = try doc.info()
            let r1 = (before["revision"] as? NSNumber)?.uint64Value ?? 0
            let r2 = (after["revision"] as? NSNumber)?.uint64Value ?? 0
            if r1 == r2 {
                return Snapshot(revision: r1, rgba: rgba, width: (after["width"] as? NSNumber)?.intValue ?? 0,
                                height: (after["height"] as? NSNumber)?.intValue ?? 0, icc: icc, dpi: dpi)
            }
        }
        throw DocumentSaverError.busy
    }

    /// The document's pixels per inch (`doc.resolution`).
    public static func resolution(_ doc: Document) throws -> Double {
        let data = try doc.queryData("doc.resolution")
        let v = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        return (v?["dpi"] as? NSNumber)?.doubleValue ?? 96
    }

    /// The pixels the save sheet's preview encodes (one revision).
    public static func savePixels(_ doc: Document) throws -> SavePixels { try snapshot(doc).pixels }

    /// JPEG has no alpha: transparent pixels go onto white.
    static func onWhite(_ rgba: [UInt8]) -> [UInt8] {
        var out = rgba
        for i in stride(from: 0, to: out.count, by: 4) {
            let a = Int(out[i + 3])
            if a == 255 { continue }
            for c in 0..<3 { out[i + c] = UInt8((Int(out[i + c]) * a + 255 * (255 - a) + 127) / 255) }
            out[i + 3] = 255
        }
        return out
    }
}

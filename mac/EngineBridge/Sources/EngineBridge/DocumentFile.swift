// Document lifecycle calls (M19): File > New, .opaint open, engine saves
// (.opaint and flattened PNG), the saved-revision marker and Revert. Each call
// waits for the document's queued commands (openpainter.h, op_doc_*).
import COpenPainter
import Foundation

extension Engine {
    /// File > New: one "Background" layer filled with `background` (straight
    /// RGBA8; all zero is Transparent) at `dpi` pixels per inch.
    public func newDocument(width: UInt32, height: UInt32, dpi: Double, background: [UInt8]) throws -> Document {
        guard background.count == 4 else { throw EngineError.badArgument }
        let rgba = background.reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
        var id: UInt64 = 0
        try withHandle { h in try checkStatus(op_doc_new(h, width, height, dpi, rgba, &id)) }
        return Document(engine: self, id: id)
    }

    /// Opens a native `.opaint` file with all its layers and its profile.
    public func openOpaint(path: String) throws -> Document {
        var id: UInt64 = 0
        let bytes = Array(path.utf8)
        try withHandle { h in try checkStatus(op_doc_open_opaint(h, bytes, UInt(bytes.count), &id)) }
        return Document(engine: self, id: id)
    }
}

extension Document {
    /// The formats the engine writes itself.
    public enum EngineFormat: String, Sendable {
        case opaint
        case png
    }

    /// Writes the current history step to `path`. `markSaved` false is Save a Copy.
    public func save(to path: String, format: EngineFormat, markSaved: Bool) throws {
        let json = try JSONSerialization.data(withJSONObject: [
            "path": path, "format": format.rawValue, "mark_saved": markSaved,
        ] as [String: Any])
        try engine.withHandle { h in
            try json.withUnsafeBytes { raw in
                let base = raw.bindMemory(to: UInt8.self).baseAddress
                try checkStatus(op_doc_save(h, id, base, UInt(raw.count)))
            }
        }
    }

    /// Marks the current step saved after the app wrote the file itself.
    /// Throws `.busy` when the document's revision is no longer `revision`.
    public func markSaved(revision: UInt64) throws {
        try engine.withHandle { h in try checkStatus(op_doc_mark_saved(h, id, revision)) }
    }

    /// Revert: this document takes `source`'s content as a fresh, saved
    /// history. The caller still closes `source`.
    public func replaceContent(with source: Document) throws {
        try engine.withHandle { h in try checkStatus(op_doc_replace(h, id, source.id)) }
    }
}

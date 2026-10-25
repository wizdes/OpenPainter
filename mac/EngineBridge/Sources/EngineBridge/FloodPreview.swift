// M31: the live Magic Wand and Paint Bucket session, on the engine's shared
// preview stack (`preview.begin`, `preview.update`, `preview.commit`,
// `preview.cancel` through `op_dispatch`). The engine merges each update into
// the session's parameters and re-runs the flood on the unchanged document;
// commit adds one history step ("Magic Wand" or "Paint Bucket"), cancel adds
// none. While a session is open, another command commits it first and
// `edit.undo` drops it.
import Foundation

extension Document {
    /// Begins a live `tool.magic_wand` or `tool.paint_bucket` session. The
    /// ticket finishes when the first preview render lands.
    public func floodBegin(_ command: String, _ params: [String: any Sendable]) throws -> UInt64 {
        try dispatch("preview.begin", ["command": command, "params": params])
    }

    /// Changes some of the session's parameters (seed, tolerance, modes, color).
    public func floodUpdate(_ params: [String: any Sendable]) throws -> UInt64 {
        try dispatch("preview.update", params)
    }

    /// Commits the session as one history step.
    public func floodFinish() throws -> UInt64 { try dispatch("preview.commit") }

    /// Ends the session and leaves the document as it was.
    public func floodCancel() throws -> UInt64 { try dispatch("preview.cancel") }

    /// `doc.selection_path`; with `preview`, the open session's live selection.
    public func selectionPath(preview: Bool) throws -> [String: Any] {
        try query("doc.selection_path", ["preview": preview]) as? [String: Any] ?? [:]
    }

    /// `doc.selection_mask` (one coverage byte per pixel); with `preview`, the
    /// open session's live selection.
    public func selectionMask(preview: Bool) throws -> [UInt8] {
        [UInt8](try queryData("doc.selection_mask", ["preview": preview]))
    }
}

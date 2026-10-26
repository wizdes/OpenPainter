// M57 crash recovery (docs/ARCHITECTURE.md, "Recovery records"): turn
// recovery records on for a document, list the records a crash left, open
// one as an unsaved document, discard one. The engine writes the records on
// its own thread after edits settle; nothing here waits for a write.
import COpenPainter
import Foundation

/// One record found under the recovery folder: a document that did not close cleanly.
public struct RecoveryRecord: Sendable, Equatable {
    public let uuid: String
    public let dir: URL
    /// False when the record cannot be opened; `problem` says why.
    public let valid: Bool
    /// The record reads, but its file was saved after it: it can only be
    /// recovered as a new untitled image ("Recover as Untitled").
    public let stale: Bool
    public let problem: String?
    public let revision: UInt64?
    /// The file the document came from or was last saved to (nil: untitled).
    public let originalURL: URL?
    /// Fix round 2, item 3: the type of that file's content as detected when
    /// it was opened (nil in records written before this was kept).
    public let originalType: String?
    public let time: Date?
    public let width: Int?
    public let height: Int?
    public let layers: Int?
    public let snapshotBytes: Int?

    public init(json j: [String: Any]) {
        uuid = j["uuid"] as? String ?? ""
        dir = URL(fileURLWithPath: j["dir"] as? String ?? "")
        valid = j["valid"] as? Bool ?? false
        stale = j["stale"] as? Bool ?? false
        problem = j["problem"] as? String
        revision = (j["revision"] as? NSNumber)?.uint64Value
        originalURL = (j["original_url"] as? String).flatMap { URL(string: $0) }
        originalType = j["original_type"] as? String
        time = (j["time_unix"] as? Double).map { Date(timeIntervalSince1970: $0) }
        width = j["width"] as? Int
        height = j["height"] as? Int
        layers = j["layers"] as? Int
        snapshotBytes = j["snapshot_bytes"] as? Int
    }
}

/// When the engine writes records. Defaults: 30 s after the last change, and at
/// least every 2 minutes while edits go on.
public struct RecoveryTiming: Sendable, Equatable {
    public var settle: Duration
    public var maxInterval: Duration
    public init(settle: Duration = .seconds(30), maxInterval: Duration = .seconds(120)) {
        self.settle = settle
        self.maxInterval = maxInterval
    }
    var json: [String: Any] {
        ["settle_ms": Self.ms(settle), "max_interval_ms": Self.ms(maxInterval)]
    }
    public static func ms(_ d: Duration) -> Int64 {
        d.components.seconds * 1000 + d.components.attoseconds / 1_000_000_000_000_000
    }
}

private func jsonData(_ object: [String: Any]) throws -> Data {
    try JSONSerialization.data(withJSONObject: object)
}

extension Engine {
    /// Lists the records under `root` that no running app owns. Needs no engine.
    public static func scanRecovery(root: URL) throws -> [RecoveryRecord] {
        var out = OpBuffer(ptr: nil, len: 0)
        let bytes = Array(root.path.utf8)
        try checkStatus(op_recovery_scan(bytes, UInt(bytes.count), &out))
        defer { op_buffer_free(&out) }
        guard let ptr = out.ptr else { return [] }
        let data = Data(bytes: ptr, count: Int(out.len))
        let list = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] ?? []
        return list.map(RecoveryRecord.init(json:))
    }

    /// Discard: deletes a record folder. Throws `.busy` when a running app owns it.
    public static func discardRecovery(dir: URL) throws {
        let json = try jsonData(["dir": dir.path])
        try json.withUnsafeBytes { raw in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            try checkStatus(op_recovery_discard(base, UInt(raw.count)))
        }
    }

    /// Recover: opens the record in `dir` as a new document marked unsaved and
    /// keeps recording it into the same folder.
    public func openRecovery(dir: URL, timing: RecoveryTiming = RecoveryTiming(), untitled: Bool = false) throws -> Document {
        var params = timing.json
        params["dir"] = dir.path
        params["untitled"] = untitled
        let json = try jsonData(params)
        var id: UInt64 = 0
        try withHandle { h in
            try json.withUnsafeBytes { raw in
                let base = raw.bindMemory(to: UInt8.self).baseAddress
                try checkStatus(op_recovery_open(h, base, UInt(raw.count), &id))
            }
        }
        return Document(engine: self, id: id)
    }
}

extension Engine {
    /// Quit: waits up to `timeout` for recovery writers of closed images to
    /// finish. False when some were still running at the limit.
    @discardableResult
    public func waitForRecoveryWriters(timeout: Duration) -> Bool {
        let ms = UInt64(max(0, RecoveryTiming.ms(timeout)))
        return (try? withHandle { h in try checkStatus(op_recovery_wait_stopped(h, ms)) }) != nil
    }
}

extension Document {
    /// Turns on recovery records in `dir` (`<root>/<doc-uuid>`). Calling it
    /// again only updates `originalURL` and `originalType` (after a Save As).
    /// `originalType` is the content type of the original file (fix round 2,
    /// item 3), so Recover never guesses it from the file name.
    public func attachRecovery(dir: URL, originalURL: URL?, originalType: String? = nil,
                               timing: RecoveryTiming = RecoveryTiming(), testFailAfterBytes: Int? = nil) throws {
        var params = timing.json
        params["dir"] = dir.path
        params["original_url"] = originalURL?.absoluteString ?? NSNull()
        params["original_type"] = originalType ?? NSNull()
        if let testFailAfterBytes { params["test_fail_after_bytes"] = testFailAfterBytes }
        let json = try jsonData(params)
        try engine.withHandle { h in
            try json.withUnsafeBytes { raw in
                let base = raw.bindMemory(to: UInt8.self).baseAddress
                try checkStatus(op_recovery_attach(h, id, base, UInt(raw.count)))
            }
        }
    }

    /// Writes a record now if the document changed since the last one, and waits.
    public func flushRecovery() throws {
        try engine.withHandle { h in try checkStatus(op_recovery_flush(h, id)) }
    }

    /// Stops recording. `deleteRecord` false keeps the record, as a crash would.
    public func detachRecovery(deleteRecord: Bool) throws {
        try engine.withHandle { h in try checkStatus(op_recovery_detach(h, id, deleteRecord)) }
    }

    /// Fix round 4, item 3: stops recovery at once without waiting for the
    /// writer or the document's queued work (`op_recovery_stop`); with
    /// `deleteRecord`, the record is gone from the recovery folder when it
    /// returns.
    public func stopRecovery(deleteRecord: Bool) throws {
        try engine.withHandle { h in try checkStatus(op_recovery_stop(h, id, deleteRecord)) }
    }

    /// `doc.recovery`: `enabled`, `acknowledged_revision`, `error`, `writes`, `failures`.
    public func recoveryStatus() throws -> [String: Any] {
        try query("doc.recovery") as? [String: Any] ?? [:]
    }

    /// No history step is the saved one: the image shows as unsaved until the
    /// next save (after a save that failed once the engine had marked it).
    public func markUnsaved() throws {
        try engine.withHandle { h in try checkStatus(op_doc_mark_unsaved(h, id)) }
    }

    /// The last engine save error as text (nil after a good save).
    public func lastFileError() -> String? {
        (try? query("doc.last_file_error") as? [String: Any])?["error"] as? String
    }
}

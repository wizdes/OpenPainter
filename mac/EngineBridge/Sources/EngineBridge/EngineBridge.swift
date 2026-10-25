// A thin Swift wrapper over the OpenPainter C interface (openpainter.h).
import COpenPainter
import Foundation
import Synchronization

/// The C status codes, as a Swift error.
public enum EngineError: Error, Equatable, Sendable {
    case badArgument
    case noSuchDoc
    case readOnly
    case panic
    case busy
    case `internal`
    case unsupported
    case cancelled
    case closed

    init?(_ status: OpStatus) {
        switch status {
        case OP_OK: return nil
        case OP_ERR_BAD_ARGUMENT: self = .badArgument
        case OP_ERR_NO_SUCH_DOC: self = .noSuchDoc
        case OP_ERR_READ_ONLY: self = .readOnly
        case OP_ERR_PANIC: self = .panic
        case OP_ERR_BUSY: self = .busy
        case OP_ERR_UNSUPPORTED: self = .unsupported
        case OP_ERR_CANCELLED: self = .cancelled
        default: self = .internal
        }
    }
}

private func check(_ status: OpStatus) throws {
    if let error = EngineError(status) { throw error }
}

/// One engine event. The payload was copied off the engine thread inside the callback.
public struct EngineEvent: Sendable {
    public enum Kind: UInt32, Sendable {
        case docChanged = 1
        case historyChanged = 2
        case layersChanged = 3
        case jobFinished = 4
        case progress = 5
    }
    public let kind: Kind
    public let doc: UInt64
    public let ticket: UInt64
    /// The event's JSON payload.
    public let payload: Data

    public var json: [String: Any] {
        (try? JSONSerialization.jsonObject(with: payload)) as? [String: Any] ?? [:]
    }
    /// For JobFinished: "ok", "cancelled", "dropped", or an error name such as "read_only".
    public var status: String? { json["status"] as? String }
}

/// Receives callbacks on the engine thread. It owns the event stream and the
/// JobFinished results that callers wait for.
final class EventSink: Sendable {
    private struct State {
        var finished: [UInt64: EngineEvent] = [:]
        var order: [UInt64] = []
        var waiters: [UInt64: CheckedContinuation<EngineEvent?, Never>] = [:]
        var freed = false
        var lateCallbacks = 0
        var delivered = 0
    }
    /// Results nobody waited for are kept up to this count, oldest dropped first.
    private static let keptResults = 4096
    private let state = Mutex(State())
    private let continuation: AsyncStream<EngineEvent>.Continuation

    init(_ continuation: AsyncStream<EngineEvent>.Continuation) {
        self.continuation = continuation
    }

    func deliver(_ event: EngineEvent) {
        let waiter: CheckedContinuation<EngineEvent?, Never>? = state.withLock { s in
            s.delivered += 1
            if s.freed { s.lateCallbacks += 1 }
            guard event.kind == .jobFinished else { return nil }
            if let w = s.waiters.removeValue(forKey: event.ticket) { return w }
            s.finished[event.ticket] = event
            s.order.append(event.ticket)
            if s.order.count > Self.keptResults {
                s.finished.removeValue(forKey: s.order.removeFirst())
            }
            return nil
        }
        waiter?.resume(returning: event)
        continuation.yield(event)
    }

    func wait(for ticket: UInt64, timeout: Duration) async -> EngineEvent? {
        await withTaskGroup(of: Void.self) { group in
            group.addTask { [self] in
                try? await Task.sleep(for: timeout)
                let w = state.withLock { $0.waiters.removeValue(forKey: ticket) }
                w?.resume(returning: nil)
            }
            let event = await withCheckedContinuation { (c: CheckedContinuation<EngineEvent?, Never>) in
                let ready: EngineEvent? = state.withLock { s in
                    if let e = s.finished.removeValue(forKey: ticket) {
                        s.order.removeAll { $0 == ticket }
                        return e
                    }
                    s.waiters[ticket] = c
                    return nil
                }
                if let ready { c.resume(returning: ready) }
            }
            group.cancelAll()
            return event
        }
    }

    /// Called after op_engine_free returns. Any later callback is counted as late.
    func markFreed() {
        let waiters = state.withLock { s in
            s.freed = true
            let w = s.waiters
            s.waiters = [:]
            return w
        }
        for w in waiters.values { w.resume(returning: nil) }
        continuation.finish()
    }

    var lateCallbacks: Int { state.withLock { $0.lateCallbacks } }
    var delivered: Int { state.withLock { $0.delivered } }
}

/// One engine instance. Its events arrive on `events`, in order.
public final class Engine: @unchecked Sendable {
    /// Recursive: `op_view_detach` releases a CAMetalLayer, which flushes the Core
    /// Animation transaction on this thread; that can lay out another canvas,
    /// which calls `op_view_set` from inside the detach call (M38 found the hang).
    private let lock = NSRecursiveLock()
    private var handle: OpaquePointer?
    private let sinkRef: Unmanaged<EventSink>
    let sink: EventSink
    public let events: AsyncStream<EngineEvent>

    public init() throws {
        let (stream, continuation) = AsyncStream<EngineEvent>.makeStream(bufferingPolicy: .unbounded)
        events = stream
        sink = EventSink(continuation)
        sinkRef = Unmanaged.passRetained(sink)
        let callback: OpEventFn = { event, user in
            guard let event, let user else { return }
            let sink = Unmanaged<EventSink>.fromOpaque(user).takeUnretainedValue()
            let raw = event.pointee
            guard let kind = EngineEvent.Kind(rawValue: raw.kind) else { return }
            // json_ptr is valid only during this call, so copy it now.
            let payload = raw.json_ptr.map { Data(bytes: $0, count: Int(raw.json_len)) } ?? Data()
            sink.deliver(EngineEvent(kind: kind, doc: raw.doc, ticket: raw.ticket, payload: payload))
        }
        guard let h = op_engine_new(nil, callback, sinkRef.toOpaque()) else {
            sinkRef.release()
            continuation.finish()
            throw EngineError.internal
        }
        handle = h
    }

    deinit { close() }

    /// Frees the engine: closes all documents after their queued work and jobs
    /// finish. No event is delivered after this returns. Safe to call twice.
    public func close() {
        lock.lock()
        defer { lock.unlock() }
        guard let h = handle else { return }
        handle = nil
        op_engine_free(h)
        sink.markFreed()
        sinkRef.release()
    }

    func withHandle<T>(_ body: (OpaquePointer) throws -> T) throws -> T {
        lock.lock()
        defer { lock.unlock() }
        guard let h = handle else { throw EngineError.closed }
        return try body(h)
    }

    public func openPNG(path: String) throws -> Document {
        var id: UInt64 = 0
        let bytes = Array(path.utf8)
        try withHandle { h in try check(op_doc_open_png(h, bytes, UInt(bytes.count), &id)) }
        return Document(engine: self, id: id)
    }

    public func importPixels(width: UInt32, height: UInt32, rgba: [UInt8], icc: [UInt8]? = nil) throws -> Document {
        var id: UInt64 = 0
        try withHandle { h in
            if let icc {
                try check(op_import_pixels(h, width, height, rgba, UInt(rgba.count), icc, UInt(icc.count), &id))
            } else {
                try check(op_import_pixels(h, width, height, rgba, UInt(rgba.count), nil, 0, &id))
            }
        }
        return Document(engine: self, id: id)
    }

    /// Stops a ticket's work. Nothing it did is committed.
    public func cancel(ticket: UInt64) throws {
        try withHandle { h in try check(op_cancel(h, ticket)) }
    }

    /// Waits for a ticket's JobFinished event. Returns nil on timeout or engine close.
    public func result(of ticket: UInt64, timeout: Duration = .seconds(10)) async -> EngineEvent? {
        await sink.wait(for: ticket, timeout: timeout)
    }

    fileprivate func dispatch(doc: UInt64, command: String, json: Data) throws -> UInt64 {
        var ticket: UInt64 = 0
        try withHandle { h in
            try json.withUnsafeBytes { raw in
                let base = raw.bindMemory(to: UInt8.self).baseAddress
                try check(op_dispatch(h, doc, command, base, UInt(raw.count), &ticket))
            }
        }
        return ticket
    }

    fileprivate func query(doc: UInt64, id: String, json: Data) throws -> Data {
        var out = OpBuffer(ptr: nil, len: 0)
        try withHandle { h in
            try json.withUnsafeBytes { raw in
                let base = raw.bindMemory(to: UInt8.self).baseAddress
                try check(op_query(h, doc, id, base, UInt(raw.count), &out))
            }
        }
        defer { op_buffer_free(&out) }
        guard let ptr = out.ptr else { return Data() }
        return Data(bytes: ptr, count: Int(out.len))
    }

    /// One registered effect from the `effects.list` catalog query.
    public struct EffectInfo: Equatable, Sendable {
        public let id: String
        public let label: String
        public let category: String

        public init(id: String, label: String, category: String) {
            self.id = id
            self.label = label
            self.category = category
        }
    }

    /// `effects.list`: every registered effect, in registry order. Needs no document.
    public func effects() throws -> [EffectInfo] {
        let data = try query(doc: 0, id: "effects.list", json: Data("{}".utf8))
        let list = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] ?? []
        return list.map {
            EffectInfo(
                id: $0["id"] as? String ?? "", label: $0["label"] as? String ?? "",
                category: $0["category"] as? String ?? "")
        }
    }

    fileprivate func closeDoc(_ doc: UInt64) throws {
        try withHandle { h in try check(op_doc_close(h, doc)) }
    }
}

/// One open document. Commands run on the engine in call order.
public final class Document: Sendable {
    public let engine: Engine
    public let id: UInt64

    init(engine: Engine, id: UInt64) {
        self.engine = engine
        self.id = id
    }

    /// Queues a command and returns its ticket at once.
    public func dispatch(_ command: String, _ params: [String: any Sendable] = [:]) throws -> UInt64 {
        let json = try JSONSerialization.data(withJSONObject: params)
        return try engine.dispatch(doc: id, command: command, json: json)
    }

    /// Dispatches a command and waits for its JobFinished event.
    public func run(
        _ command: String, _ params: [String: any Sendable] = [:], timeout: Duration = .seconds(10)
    ) async throws -> EngineEvent {
        let ticket = try dispatch(command, params)
        guard let event = await engine.result(of: ticket, timeout: timeout) else {
            throw EngineError.busy
        }
        return event
    }

    /// `doc.info`: size, layers, active layer, history, dirty and read_only flags.
    public func info() throws -> [String: Any] {
        let data = try engine.query(doc: id, id: "doc.info", json: Data())
        return try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
    }

    /// Any `op_query` on this document, decoded as JSON.
    public func query(_ queryID: String, _ params: [String: any Sendable] = [:]) throws -> Any {
        let json = try JSONSerialization.data(withJSONObject: params)
        let data = try engine.query(doc: id, id: queryID, json: json)
        return try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
    }

    /// Any `op_query` on this document, as raw bytes (binary answers such as
    /// `layer.thumbnail`).
    public func queryData(_ queryID: String, _ params: [String: any Sendable] = [:]) throws -> Data {
        let json = try JSONSerialization.data(withJSONObject: params)
        return try engine.query(doc: id, id: queryID, json: json)
    }

    /// `commands.list`: every command id and whether it is enabled now.
    public func enabledCommands() throws -> [String: Bool] {
        let list = try query("commands.list") as? [[String: Any]] ?? []
        var out: [String: Bool] = [:]
        for c in list {
            if let id = c["id"] as? String { out[id] = c["enabled"] as? Bool ?? false }
        }
        return out
    }

    /// The flattened composite as RGBA8, row by row.
    public func pixels(preview: Bool = false) throws -> [UInt8] {
        let json = try JSONSerialization.data(withJSONObject: ["preview": preview])
        return [UInt8](try engine.query(doc: id, id: "doc.pixels", json: json))
    }

    /// Waits for queued work and in-flight jobs, then closes the document.
    public func close() throws {
        try engine.closeDoc(id)
    }
}

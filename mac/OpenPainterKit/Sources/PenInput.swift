// Pen and mouse input for live brush strokes (M38).
//
// The canvas turns each mouse, tablet or trackpad event into a `PenSample`
// (document pixels, pressure, tilt, event time) and hands it to a `StrokeSink`.
// `LiveStroke` is the sink the main window uses: it collects the samples that
// arrive in one main run-loop turn and sends them to the engine in one
// `op_stroke_append`, so the canvas shows the engine's own preview while the
// pointer moves. Mouse-up commits one history step; Escape cancels.
import AppKit
@_exported import EngineBridge

/// One pointer sample as the canvas reads it from an event.
public struct PenSample: Equatable, Sendable {
    /// Document pixels.
    public var point: CGPoint
    /// 0...1. A mouse gives 1; a tablet its pen pressure; a Force Touch
    /// trackpad its pressure while it reports one.
    public var pressure: Double
    /// -1...1 on each axis (tablets only; 0 otherwise).
    public var tilt: CGPoint
    /// `NSEvent.timestamp`: seconds since the Mac started.
    public var time: TimeInterval

    public init(point: CGPoint, pressure: Double = 1, tilt: CGPoint = .zero, time: TimeInterval) {
        self.point = point
        self.pressure = pressure
        self.tilt = tilt
        self.time = time
    }

    /// True for events that carry tablet data (pen pressure and tilt).
    public static func isTablet(_ event: NSEvent) -> Bool {
        switch event.type {
        case .tabletPoint: return true
        case .leftMouseDown, .leftMouseDragged, .leftMouseUp, .rightMouseDown, .rightMouseDragged, .rightMouseUp,
            .otherMouseDown, .otherMouseDragged, .otherMouseUp, .mouseMoved:
            return event.subtype == .tabletPoint
        default: return false
        }
    }

    /// The pen pressure of a tablet event. The tablet field has 16 bits;
    /// `NSEvent.pressure` can be coarser, so it is the fallback only.
    public static func tabletPressure(_ event: NSEvent) -> Double {
        if let cg = event.cgEvent {
            return cg.getDoubleValueField(.tabletEventPointPressure)
        }
        return Double(event.pressure)
    }

    /// Reads a sample from `event` at `point` (document pixels).
    /// `trackpadPressure` is the last Force Touch pressure of this stroke, if any.
    public static func from(_ event: NSEvent, point: CGPoint, trackpadPressure: Double?) -> PenSample {
        var pressure = 1.0
        var tilt = CGPoint.zero
        if isTablet(event) {
            pressure = tabletPressure(event)
            tilt = event.tilt
        } else if let trackpadPressure {
            pressure = trackpadPressure
        }
        if !pressure.isFinite { pressure = 1 }
        return PenSample(
            point: point, pressure: min(max(pressure, 0), 1),
            tilt: CGPoint(x: min(max(tilt.x, -1), 1), y: min(max(tilt.y, -1), 1)), time: event.timestamp)
    }

    /// The engine sample for this pen sample in a stroke that began at `start`.
    /// `after` keeps the times from going back.
    public func strokeSample(start: TimeInterval, after: UInt64 = 0) -> StrokeSample {
        let ms = max(0, (time - start) * 1000)
        return StrokeSample(
            x: Double(point.x), y: Double(point.y), pressure: pressure, tiltX: Double(tilt.x), tiltY: Double(tilt.y),
            timeMs: max(after, ms.isFinite ? UInt64(ms) : 0))
    }
}

/// Where the canvas sends a brush drag.
@MainActor
public protocol StrokeSink: AnyObject {
    /// A press with a brush tool. Return false to ignore the drag (an error).
    func strokeBegan(_ sample: PenSample, secondary: Bool) -> Bool
    func strokeMoved(_ sample: PenSample)
    /// The button went up: commit.
    func strokeEnded()
    /// Escape: drop the stroke.
    func strokeCancelled()
}

/// Timestamps for the latency measurement: each pen event's time and the
/// preview serial of the first append that carried it.
public struct LatencyMark: Equatable, Sendable {
    public var eventTimeNs: UInt64
    public var serial: UInt64
}

/// A live stroke on one document: batching, the engine calls and the counts
/// the tests read.
@MainActor
public final class LiveStroke {
    public let document: Document
    public private(set) var isActive = false
    /// `op_stroke_append` calls in the current or last stroke.
    public private(set) var appendCalls = 0
    /// Every sample sent to the engine in the current or last stroke, in order.
    public private(set) var sent: [StrokeSample] = []
    /// Set to a list to collect latency marks.
    public var latency: [LatencyMark]?
    public var onError: ((String) -> Void)?
    /// After a commit or a cancel (the editor refreshes).
    public var onFinished: (() -> Void)?

    private var pending: [(StrokeSample, TimeInterval)] = []
    private var start: TimeInterval = 0
    private var lastMs: UInt64 = 0
    private var flushQueued = false

    public init(document: Document) {
        self.document = document
    }

    /// Starts a stroke with `stroke.begin` options and its first sample.
    public func begin(_ params: [String: any Sendable], first: PenSample) -> Bool {
        if isActive { cancel() }
        do {
            try document.strokeBegin(params)
        } catch {
            onError?("Could not start the stroke: \(error)")
            return false
        }
        isActive = true
        appendCalls = 0
        sent = []
        start = first.time
        lastMs = 0
        add(first)
        return true
    }

    /// Queues a sample; the queue goes to the engine at the end of this run-loop turn.
    public func add(_ s: PenSample) {
        guard isActive else { return }
        let sample = s.strokeSample(start: start, after: lastMs)
        lastMs = sample.timeMs
        pending.append((sample, s.time))
        guard !flushQueued else { return }
        flushQueued = true
        // A run-loop block, not a main-queue block: it runs at the end of this
        // turn in every common mode, also inside nested run loops (event
        // tracking, tests).
        let loop = CFRunLoopGetMain()
        CFRunLoopPerformBlock(loop, CFRunLoopMode.commonModes.rawValue) { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.flushQueued = false
                self.flush()
            }
        }
        CFRunLoopWakeUp(loop)
    }

    /// Samples waiting for the end of this run-loop turn.
    public var pendingCount: Int { pending.count }

    /// Sends the queued samples now (one `op_stroke_append`).
    public func flush() {
        guard isActive, !pending.isEmpty else { return }
        let batch = pending
        pending = []
        do {
            let serial = try document.strokeAppend(batch.map(\.0))
            appendCalls += 1
            sent.append(contentsOf: batch.map(\.0))
            if latency != nil {
                latency!.append(contentsOf: batch.map { LatencyMark(eventTimeNs: UInt64($0.1 * 1e9), serial: serial) })
            }
        } catch {
            onError?("Stroke samples were refused: \(error)")
        }
    }

    /// Sends what is queued and commits one history step.
    public func end() {
        guard isActive else { return }
        flush()
        isActive = false
        do {
            try document.strokeEnd()
        } catch {
            onError?("The stroke could not be finished: \(error)")
        }
        onFinished?()
    }

    /// Drops the stroke: the image and its history stay as they were.
    public func cancel() {
        guard isActive else { return }
        pending = []
        isActive = false
        do {
            try document.strokeCancel()
        } catch {
            onError?("The stroke could not be cancelled: \(error)")
        }
        onFinished?()
    }
}

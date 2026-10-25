// Live brush strokes (M38): op_stroke_begin / append / end / cancel, and the
// frame stamps a canvas view logs for latency measurements.
import COpenPainter
import Foundation

/// One pointer sample in document pixels.
public struct StrokeSample: Equatable, Sendable {
    public var x: Double
    public var y: Double
    /// 0...1; a mouse sends 1.
    public var pressure: Double
    /// -1...1 on each axis; a mouse sends 0.
    public var tiltX: Double
    public var tiltY: Double
    /// Milliseconds since the stroke began; never goes back.
    public var timeMs: UInt64

    public init(x: Double, y: Double, pressure: Double = 1, tiltX: Double = 0, tiltY: Double = 0, timeMs: UInt64 = 0) {
        self.x = x
        self.y = y
        self.pressure = pressure
        self.tiltX = tiltX
        self.tiltY = tiltY
        self.timeMs = timeMs
    }

    var raw: OpStrokeSample {
        OpStrokeSample(x: x, y: y, pressure: pressure, tilt_x: tiltX, tilt_y: tiltY, time_ms: timeMs)
    }
}

extension Document {
    /// Starts a Paintbrush, Eraser or Pencil stroke on the active layer. Keys:
    /// tool, width, color (rrggbbaa), opacity (0...255), antialias, blend,
    /// hardness (0...1), pressure_size, pressure_opacity. Throws `.busy` while
    /// another stroke or a preview runs.
    public func strokeBegin(_ params: [String: any Sendable]) throws {
        let json = try JSONSerialization.data(withJSONObject: params)
        try engine.withHandle { h in
            try json.withUnsafeBytes { raw in
                try checkStatus(op_stroke_begin(h, id, raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count)))
            }
        }
    }

    /// Adds samples and publishes the preview; attached canvas views draw it at
    /// once. Returns the preview serial that the frame stamps of those frames carry.
    @discardableResult
    public func strokeAppend(_ samples: [StrokeSample]) throws -> UInt64 {
        var serial: UInt64 = 0
        let raw = samples.map(\.raw)
        try engine.withHandle { h in
            try raw.withUnsafeBufferPointer { buf in
                try checkStatus(op_stroke_append(h, id, buf.baseAddress, UInt(buf.count), &serial))
            }
        }
        return serial
    }

    /// Commits the stroke as one history step named after the tool.
    public func strokeEnd() throws {
        try engine.withHandle { h in try checkStatus(op_stroke_end(h, id)) }
    }

    /// Drops the stroke; the document and its history stay as they were.
    public func strokeCancel() throws {
        try engine.withHandle { h in try checkStatus(op_stroke_cancel(h, id)) }
    }
}

/// One presented frame: when the GPU finished it (CLOCK_UPTIME_RAW, the clock
/// of `NSEvent.timestamp`), and which revision and preview it shows.
public struct FrameStamp: Equatable, Sendable {
    public let timeNs: UInt64
    public let revision: UInt64
    public let previewSerial: UInt64

    public init(timeNs: UInt64, revision: UInt64, previewSerial: UInt64) {
        self.timeNs = timeNs
        self.revision = revision
        self.previewSerial = previewSerial
    }
}

extension EngineView {
    /// Moves the frame stamps (oldest first) out of the engine.
    public func takeFrameStamps() throws -> [FrameStamp] {
        var all: [FrameStamp] = []
        var chunk = [OpFrameStamp](repeating: OpFrameStamp(), count: 4096)
        while true {
            var n: UInt = 0
            try document.engine.withHandle { h in
                try chunk.withUnsafeMutableBufferPointer { buf in
                    try checkStatus(op_view_take_frame_stamps(h, id, buf.baseAddress, UInt(buf.count), &n))
                }
            }
            all.append(
                contentsOf: chunk[0..<Int(n)].map {
                    FrameStamp(timeNs: $0.time_ns, revision: $0.revision, previewSerial: $0.preview_serial)
                })
            if Int(n) < chunk.count { return all }
        }
    }
}

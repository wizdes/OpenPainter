// Recorded tablet logs (M38): a small JSON format for pen input, a recorder the
// canvas feeds, and a replay that rebuilds the NSEvents so a log goes through
// the same input path as a real pen.
//
// Format (one object):
//   {"format":"openpainter-tablet-log","version":1,"space":"document-pixels",
//    "samples":[{"t":0,"x":10.5,"y":20.25,"pressure":0.5,"tilt":[0.1,-0.2],"buttons":1}, ...]}
// t is seconds from the first sample; x and y are document pixels; pressure is
// 0...1; tilt is -1...1 per axis; buttons is a bit mask (1 = primary). A sample
// whose buttons go from 0 to non-zero is a press, non-zero to non-zero a drag,
// non-zero to 0 a release (its position is not painted), 0 to 0 a hover.
import AppKit

public struct TabletLog: Codable, Equatable, Sendable {
    public struct Sample: Codable, Equatable, Sendable {
        public var t: Double
        public var x: Double
        public var y: Double
        public var pressure: Double
        public var tilt: [Double]
        public var buttons: Int

        public init(t: Double, x: Double, y: Double, pressure: Double, tilt: [Double] = [0, 0], buttons: Int) {
            self.t = t
            self.x = x
            self.y = y
            self.pressure = pressure
            self.tilt = tilt
            self.buttons = buttons
        }
    }

    public static let formatName = "openpainter-tablet-log"
    public var format = TabletLog.formatName
    public var version = 1
    public var space = "document-pixels"
    public var samples: [Sample]

    public init(samples: [Sample]) {
        self.samples = samples
    }

    public enum LogError: Error, Equatable {
        case badFormat(String)
    }

    /// Reads and checks a log.
    public static func decode(_ data: Data) throws -> TabletLog {
        let log = try JSONDecoder().decode(TabletLog.self, from: data)
        guard log.format == formatName, log.version == 1, log.space == "document-pixels" else {
            throw LogError.badFormat("not an OpenPainter tablet log, version 1")
        }
        var last = -Double.infinity
        for (i, s) in log.samples.enumerated() {
            guard s.t.isFinite, s.x.isFinite, s.y.isFinite, (0...1).contains(s.pressure), s.tilt.count == 2,
                s.tilt.allSatisfy({ $0.isFinite && (-1...1).contains($0) }), s.buttons >= 0
            else { throw LogError.badFormat("sample \(i) is out of range") }
            guard s.t >= last else { throw LogError.badFormat("sample \(i) goes back in time") }
            last = s.t
        }
        return log
    }

    public func encoded() throws -> Data {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        return try e.encode(self)
    }

    /// The samples a stroke paints, stroke by stroke: presses and drags, not
    /// releases or hovers. This is what the engine gets when fed directly.
    public var strokes: [[Sample]] {
        var out: [[Sample]] = []
        var down = false
        for s in samples {
            if s.buttons != 0 {
                if !down { out.append([]) }
                out[out.count - 1].append(s)
            }
            down = s.buttons != 0
        }
        return out
    }

    /// The engine samples of one stroke, with the times the input path gives.
    public static func strokeSamples(_ stroke: [Sample]) -> [StrokeSample] {
        guard let first = stroke.first else { return [] }
        var last: UInt64 = 0
        return stroke.map {
            let s = PenSample(
                point: CGPoint(x: $0.x, y: $0.y), pressure: $0.pressure, tilt: CGPoint(x: $0.tilt[0], y: $0.tilt[1]),
                time: $0.t
            ).strokeSample(start: first.t, after: last)
            last = s.timeMs
            return s
        }
    }

    /// Rebuilds the events of this log for `canvas`: tablet-point mouse events at
    /// the view positions of the samples, with the log's pressure and tilt, and
    /// timestamps `start + t`.
    @MainActor
    public func events(for canvas: MetalCanvasView, start: TimeInterval) -> [NSEvent] {
        var out: [NSEvent] = []
        var down = false
        for s in samples {
            let type: NSEvent.EventType
            switch (down, s.buttons != 0) {
            case (false, true): type = .leftMouseDown
            case (true, true): type = .leftMouseDragged
            case (true, false): type = .leftMouseUp
            case (false, false): type = .mouseMoved
            }
            down = s.buttons != 0
            if let e = TabletLog.event(
                type, canvas: canvas, document: CGPoint(x: s.x, y: s.y), pressure: s.pressure,
                tilt: CGPoint(x: s.tilt[0], y: s.tilt[1]), time: start + s.t)
            {
                out.append(e)
            }
        }
        return out
    }

    /// One synthetic tablet event at a document point of `canvas`.
    @MainActor
    public static func event(
        _ type: NSEvent.EventType, canvas: MetalCanvasView, document: CGPoint, pressure: Double, tilt: CGPoint,
        time: TimeInterval
    ) -> NSEvent? {
        let inView = canvas.viewport.viewPoint(ofDocument: document)
        let inWindow = canvas.convert(inView, to: nil)
        guard
            let base = NSEvent.mouseEvent(
                with: type, location: inWindow, modifierFlags: [], timestamp: time,
                windowNumber: canvas.window?.windowNumber ?? 0, context: nil, eventNumber: 0, clickCount: 1,
                pressure: Float(pressure)),
            let cg = base.cgEvent
        else { return nil }
        cg.setIntegerValueField(.mouseEventSubtype, value: Int64(CGEventMouseSubtype.tabletPoint.rawValue))
        cg.setDoubleValueField(.tabletEventPointPressure, value: pressure)
        cg.setDoubleValueField(.tabletEventTiltX, value: Double(tilt.x))
        cg.setDoubleValueField(.tabletEventTiltY, value: Double(tilt.y))
        cg.setDoubleValueField(.mouseEventPressure, value: pressure)
        return NSEvent(cgEvent: cg)
    }

    /// Hands `event` to the canvas's own event methods (the input path).
    @MainActor
    public static func deliver(_ event: NSEvent, to canvas: MetalCanvasView) {
        switch event.type {
        case .leftMouseDown: canvas.mouseDown(with: event)
        case .leftMouseDragged: canvas.mouseDragged(with: event)
        case .leftMouseUp: canvas.mouseUp(with: event)
        case .mouseMoved: canvas.mouseMoved(with: event)
        case .tabletPoint: canvas.tabletPoint(with: event)
        default: break
        }
    }
}

/// Collects what the canvas reads from a pen into a `TabletLog`.
@MainActor
public final class TabletLogRecorder {
    public private(set) var samples: [TabletLog.Sample] = []
    private var start: TimeInterval?

    public init() {}

    public func record(_ s: PenSample, buttons: Int) {
        let t0 = start ?? s.time
        start = t0
        samples.append(
            TabletLog.Sample(
                t: s.time - t0, x: Double(s.point.x), y: Double(s.point.y), pressure: s.pressure,
                tilt: [Double(s.tilt.x), Double(s.tilt.y)], buttons: buttons))
    }

    public var log: TabletLog { TabletLog(samples: samples) }
}

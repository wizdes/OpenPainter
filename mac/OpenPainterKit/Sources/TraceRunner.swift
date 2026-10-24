// The M12 performance gate: a fixed, scripted pan and zoom trace on a canvas.
// A display link drives the script on the main thread; every tick sends a new
// view state. The engine logs the time of every presented frame on its render
// thread, and the gate is computed from those times.
import AppKit
import QuartzCore

public struct TraceResult: Sendable, CustomStringConvertible {
    public let frames: Int
    public let seconds: Double
    public let averageFPS: Double
    public let medianMs: Double
    public let p99Ms: Double
    public let worstMs: Double
    public let ticks: Int
    public let frameErrors: UInt64
    /// Gaps over 25 ms: (seconds after the first counted frame, gap in ms).
    public var longGaps: [(at: Double, ms: Double)] = []
    /// Gate: average >= 59 fps and no gap over 50 ms after the warm-up.
    public var passed: Bool { averageFPS >= 59 && worstMs <= 50 && frames > 0 && frameErrors == 0 }

    public var description: String {
        String(
            format: "TRACE frames=%d seconds=%.2f avg_fps=%.2f median_ms=%.2f p99_ms=%.2f worst_ms=%.2f ticks=%d frame_errors=%llu gate=%@",
            frames, seconds, averageFPS, medianMs, p99Ms, worstMs, ticks, frameErrors,
            passed ? "PASS" : "FAIL")
            + (longGaps.isEmpty ? "" : " long_gaps=" + longGaps.prefix(20).map { String(format: "%.2fs:%.1fms", $0.at, $0.ms) }.joined(separator: ","))
    }

    /// Statistics from present timestamps (ms), dropping the first `warmupMs`.
    public static func from(times all: [Double], warmupMs: Double, ticks: Int, frameErrors: UInt64) -> TraceResult {
        guard let first = all.first else {
            return TraceResult(frames: 0, seconds: 0, averageFPS: 0, medianMs: 0, p99Ms: 0, worstMs: 0,
                               ticks: ticks, frameErrors: frameErrors)
        }
        let times = all.filter { $0 >= first + warmupMs }
        guard times.count >= 2, let last = times.last, let start = times.first else {
            return TraceResult(frames: times.count, seconds: 0, averageFPS: 0, medianMs: 0, p99Ms: 0,
                               worstMs: .infinity, ticks: ticks, frameErrors: frameErrors)
        }
        let gaps = zip(times.dropFirst(), times).map { $0 - $1 }.sorted()
        let pick = { (q: Double) in gaps[min(gaps.count - 1, Int((Double(gaps.count - 1) * q).rounded()))] }
        let seconds = (last - start) / 1000
        var r = TraceResult(
            frames: times.count, seconds: seconds, averageFPS: Double(gaps.count) / seconds,
            medianMs: pick(0.5), p99Ms: pick(0.99), worstMs: gaps.last ?? 0,
            ticks: ticks, frameErrors: frameErrors)
        r.longGaps = zip(times.dropFirst(), times).compactMap { b, a in
            b - a > 25 ? (at: (a - start) / 1000 + warmupMs / 1000, ms: b - a) : nil
        }
        return r
    }
}

/// Runs the scripted trace. Six 5-second phases:
/// 1. pan in a circle at 100%;
/// 2. a smooth zoom sweep 5% -> 400% -> 5% around the center;
/// 3. a horizontal pan at 25% (mips);
/// 4. paint.net preset steps (Cmd+= / Cmd+-) every 0.2 s between 8% and 800%;
/// 5. a diagonal pan at 400% with the pixel grid on;
/// 6. a pinch-like zoom around a moving cursor point.
@MainActor
public final class TraceRunner: NSObject {
    public let duration: Double
    public let warmup: Double
    private let canvas: MetalCanvasView
    private var link: CADisplayLink?
    private var start: CFTimeInterval?
    private var ticks = 0
    private var lastStep = -1
    private var stepZoom = 0.08
    private var stepUp = true
    private var completion: ((TraceResult) -> Void)?

    public init(canvas: MetalCanvasView, duration: Double = 30, warmup: Double = 2) {
        self.canvas = canvas
        self.duration = duration
        self.warmup = warmup
    }

    public func run(completion: @escaping (TraceResult) -> Void) {
        self.completion = completion
        // Frames before the trace do not count.
        _ = try? canvas.engineView?.takeFrameTimes()
        let link = canvas.displayLink(target: self, selector: #selector(tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        if start == nil { start = now }
        let t = now - (start ?? now)
        ticks += 1
        if t >= duration {
            finish()
            return
        }
        var v = canvas.viewport
        let doc = v.documentSize
        let c = CGPoint(x: doc.width / 2, y: doc.height / 2)
        let phase = Int(t / 5)
        let u = (t - Double(phase) * 5) / 5  // 0..1 inside the phase
        if canvas.pixelGrid != (phase == 4) { canvas.pixelGrid = phase == 4 }
        switch phase {
        case 0:
            v.setZoom(1)
            let a = u * 2 * .pi
            v.center(on: CGPoint(x: c.x + doc.width * 0.3 * cos(a), y: c.y + doc.height * 0.3 * sin(a)))
        case 1:
            let tri = u < 0.5 ? u * 2 : (1 - u) * 2
            v.setZoom(exp(log(0.05) + (log(4) - log(0.05)) * tri))
            v.center(on: c)
        case 2:
            v.setZoom(0.25)
            v.center(on: CGPoint(x: doc.width * (0.1 + 0.8 * u), y: c.y + doc.height * 0.2 * sin(u * 6 * .pi)))
        case 3:
            let step = Int(u * 25)
            if step != lastStep {
                lastStep = step
                if stepUp { stepZoom = ZoomPresets.nextLarger(stepZoom) } else { stepZoom = ZoomPresets.nextSmaller(stepZoom) }
                if stepZoom >= 8 { stepUp = false }
                if stepZoom <= 0.08 { stepUp = true }
            }
            v.setZoom(stepZoom)
            v.center(on: CGPoint(x: c.x + doc.width * 0.2 * cos(u * 2 * .pi), y: c.y))
        case 4:
            v.setZoom(4)
            v.center(on: CGPoint(x: doc.width * (0.2 + 0.6 * u), y: doc.height * (0.2 + 0.6 * u)))
        default:
            let cursor = CGPoint(
                x: v.viewSize.width * (0.5 + 0.3 * cos(u * 4 * .pi)),
                y: v.viewSize.height * (0.5 + 0.3 * sin(u * 4 * .pi)))
            v.setZoom(exp(log(0.2) + (log(6) - log(0.2)) * (0.5 - 0.5 * cos(u * 2 * .pi))), keeping: cursor)
        }
        canvas.setViewport(v)
    }

    private func finish() {
        link?.invalidate()
        link = nil
        canvas.pixelGrid = false
        let times = (try? canvas.engineView?.takeFrameTimes()) ?? []
        let errors = (try? canvas.engineView?.info().frameErrors) ?? 0
        let result = TraceResult.from(times: times, warmupMs: warmup * 1000, ticks: ticks, frameErrors: errors)
        completion?(result)
        completion = nil
    }
}

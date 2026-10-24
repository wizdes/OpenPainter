// The M12 performance gate: the fixed 30 s pan and zoom trace on a 6000x4000 PNG.
import AppKit
import OpenPainterKit
import XCTest

@MainActor
final class TraceTests: XCTestCase {
    func testPanZoomTraceOn6000x4000() throws {
        _ = NSApplication.shared
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("openpainter-m12-\(getpid())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let png = dir.appendingPathComponent("trace-6000x4000.png")
        let t0 = Date()
        try TraceImage.write(to: png)
        print("TRACE image written in \(String(format: "%.2f", Date().timeIntervalSince(t0))) s")

        let engine = try Engine()
        defer { engine.close() }
        let t1 = Date()
        let doc = try engine.openPNG(path: png.path)
        print("TRACE engine opened the PNG in \(String(format: "%.2f", Date().timeIntervalSince(t1))) s")
        let container = CanvasContainerView(document: doc, documentSize: CGSize(width: 6000, height: 4000))
        let window = NSWindow(
            contentRect: NSRect(x: 40, y: 40, width: 960, height: 600), styleMask: [.titled], backing: .buffered,
            defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = container
        window.orderFrontRegardless()
        defer {
            window.orderOut(nil)
            window.contentView = nil
            window.close()
            try? doc.close()
        }
        let first = expectation(description: "first frame")
        container.canvas.onFirstFrame = { first.fulfill() }
        wait(for: [first], timeout: 30)

        let done = expectation(description: "trace")
        var result: TraceResult?
        let runner = TraceRunner(canvas: container.canvas)
        runner.run { r in
            result = r
            done.fulfill()
        }
        wait(for: [done], timeout: 45)
        let r = try XCTUnwrap(result)
        print(r.description)
        print("TRACE view \(Int(container.canvas.bounds.width))x\(Int(container.canvas.bounds.height)) pt, scale \(window.backingScaleFactor)")
        XCTAssertGreaterThanOrEqual(r.averageFPS, 59, "average fps")
        XCTAssertLessThanOrEqual(r.worstMs, 50, "worst frame gap after warm-up")
        XCTAssertEqual(r.frameErrors, 0)
        XCTAssertGreaterThan(r.frames, 1500)
    }

    func testTraceStatistics() {
        let times = (0..<200).map { Double($0) * 16.0 }  // 62.5 fps
        var r = TraceResult.from(times: times, warmupMs: 100, ticks: 200, frameErrors: 0)
        XCTAssertEqual(r.medianMs, 16)
        XCTAssertEqual(r.worstMs, 16)
        XCTAssertEqual(r.averageFPS, 62.5, accuracy: 1e-9)
        XCTAssertTrue(r.passed)
        var gap = times
        gap[150] += 60
        for i in 151..<gap.count { gap[i] += 60 }
        r = TraceResult.from(times: gap, warmupMs: 100, ticks: 200, frameErrors: 0)
        XCTAssertEqual(r.worstMs, 76)
        XCTAssertFalse(r.passed, "a 76 ms gap fails the gate")
    }
}

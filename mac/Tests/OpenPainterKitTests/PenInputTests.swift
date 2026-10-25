// M38 brush input: live strokes through the engine, pen pressure and tilt from
// NSEvent, no lost samples, Escape, one history step per stroke, the tool
// options, recorded tablet logs that replay to the same bytes, and the
// pen-event-to-frame latency on a 6000 x 4000 image.
//
// Events are real NSEvents (tablet-point mouse events made through CGEvent, as
// a pen driver sends them) handed to the canvas's own event methods. XCUITest
// is not available (macOS Automation Mode is off), so nothing goes through the
// window server.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class PenInputTests: XCTestCase {
    var fx: ShellFixture!
    var tools: ToolState { fx.workspace.tools }

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        NSEvent.isMouseCoalescingEnabled = true
        fx.tearDown()
    }

    // MARK: Helpers

    /// A window on the image at 100%, so document and view points differ only by the origin.
    func openActualSize(_ fixture: String = "plasma_alpha_512x384.png") throws -> EditorWindowController {
        let wc = try fx.open(fixture)
        var v = wc.canvas.viewport
        v.actualSize()
        wc.canvas.setViewport(v)
        return wc
    }

    func pen(
        _ type: NSEvent.EventType, _ wc: EditorWindowController, _ x: Double, _ y: Double, pressure: Double = 1,
        tilt: CGPoint = .zero
    ) -> NSEvent {
        TabletLog.event(
            type, canvas: wc.canvas, document: CGPoint(x: x, y: y), pressure: pressure, tilt: tilt,
            time: ProcessInfo.processInfo.systemUptime)!
    }

    /// A plain mouse event (no tablet data) at a document point.
    func mouse(_ type: NSEvent.EventType, _ wc: EditorWindowController, _ x: Double, _ y: Double,
               pressure: Float = 1) -> NSEvent {
        let p = wc.canvas.convert(wc.canvas.viewport.viewPoint(ofDocument: CGPoint(x: x, y: y)), to: nil)
        return NSEvent.mouseEvent(
            with: type, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: wc.window!.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: pressure)!
    }

    func send(_ e: NSEvent, _ wc: EditorWindowController) { TabletLog.deliver(e, to: wc.canvas) }

    func pixel(_ px: [UInt8], _ width: Int, _ x: Int, _ y: Int) -> [UInt8] {
        Array(px[(y * width + x) * 4..<(y * width + x) * 4 + 4])
    }

    /// Spins the run loop until the queued samples went to the engine.
    func spin() {
        guard let wc = fx.controllers.last else { return }
        XCTAssertTrue(waitUntil(5) { wc.brush.live.pendingCount == 0 }, "queued samples were sent")
    }

    static let logURL = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("tablet-logs/wave-and-loop.json")

    // MARK: Pressure and tilt from events

    func testMouseIsFullPressureAndTabletEventsCarryPressureAndTilt() throws {
        let wc = try openActualSize()
        // A mouse (no tablet subtype) is pressure 1 whatever its pressure field says.
        let m = PenSample.from(mouse(.leftMouseDragged, wc, 10, 20, pressure: 0.25), point: .zero, trackpadPressure: nil)
        XCTAssertEqual(m.pressure, 1)
        XCTAssertEqual(m.tilt, .zero)
        // A Force Touch trackpad's pressure counts when one was reported.
        let t = PenSample.from(mouse(.leftMouseDragged, wc, 10, 20), point: .zero, trackpadPressure: 0.4)
        XCTAssertEqual(t.pressure, 0.4)
        // Tablet endpoints 0, 0.5 and 1 on the 16-bit grid, and tilt.
        for (p, expected) in [(0.0, 0.0), (32768.0 / 65535, 32768.0 / 65535), (1.0, 1.0)] {
            let e = pen(.leftMouseDragged, wc, 10, 20, pressure: p, tilt: CGPoint(x: 8191.0 / 32767, y: -0.5))
            XCTAssertTrue(PenSample.isTablet(e))
            let s = PenSample.from(e, point: .zero, trackpadPressure: 0.9)
            XCTAssertEqual(s.pressure, expected, accuracy: 1e-12, "pressure \(p)")
            XCTAssertEqual(s.tilt.x, 8191.0 / 32767, accuracy: 1e-12)
            XCTAssertEqual(s.tilt.y, -0.5, accuracy: 1.0 / 32767)
        }
        // The sample carries the event's document position.
        let e = pen(.leftMouseDragged, wc, 100.25, 50.5)
        let s = PenSample.from(e, point: wc.canvas.viewport.documentPoint(atView: wc.canvas.convert(e.locationInWindow, from: nil)),
                               trackpadPressure: nil)
        XCTAssertEqual(s.point, CGPoint(x: 100.25, y: 50.5))
    }

    /// Pressure endpoints 0, 0.5 and 1 through the canvas and the engine:
    /// width with "pressure for size", alpha with "pressure for opacity".
    func testPressureMapsToSizeAndOpacityAtTheEndpoints() throws {
        let wc = try openActualSize()
        let doc = wc.editor.document
        tools.tool = .paintbrush
        tools.primary = DocColor(r: 0, g: 0, b: 255)
        tools.settings.brushWidth = 40
        tools.settings.antialias = true
        func dab(_ p: Double, at x: Double) throws -> [UInt8] {
            send(pen(.leftMouseDown, wc, x, 200, pressure: p), wc)
            spin()
            let px = try doc.pixels(preview: true)
            send(pen(.leftMouseUp, wc, x, 200, pressure: p), wc)
            spin()
            return px
        }
        let base = try doc.pixels()
        func changed(_ px: [UInt8], around x: Int) -> Int {
            var n = 0
            for y in 170..<230 { for xx in (x - 30)..<(x + 30) where pixel(px, 512, xx, y) != pixel(base, 512, xx, y) { n += 1 } }
            return n
        }
        tools.settings.pressureSize = true
        tools.settings.pressureOpacity = false
        XCTAssertEqual(changed(try dab(0, at: 60), around: 60), 0, "pressure 0 paints nothing")
        let half = changed(try dab(32768.0 / 65535, at: 160), around: 160)
        let full = changed(try dab(1, at: 260), around: 260)
        XCTAssertEqual(Double(half), Double.pi * 100, accuracy: 70, "a 20 px dab")
        XCTAssertEqual(Double(full), Double.pi * 400, accuracy: 140, "a 40 px dab")
        tools.settings.pressureSize = false
        tools.settings.pressureOpacity = true
        // Over the plasma image: alpha of a blue dab over a pixel, compared to the base.
        let zero = try dab(0, at: 360)
        XCTAssertEqual(pixel(zero, 512, 360, 200), pixel(base, 512, 360, 200), "opacity 0 changes nothing")
        let soft = try dab(32768.0 / 65535, at: 440)
        let hard = try dab(1, at: 440)  // the same spot: full pressure covers it
        XCTAssertEqual(pixel(hard, 512, 440, 200)[0...2], [0, 0, 255], "pressure 1 paints the full color")
        let b = pixel(base, 512, 440, 200), s = pixel(soft, 512, 440, 200)
        XCTAssertNotEqual(s, b)
        XCTAssertNotEqual(s, pixel(hard, 512, 440, 200), "pressure 0.5 is half-way")
        XCTAssertEqual(wc.brush.live.sent.map(\.pressure), [1.0], "the last dab sent one sample at pressure 1")
        // Mouse input paints at pressure 1 even with both toggles on.
        tools.settings.pressureSize = true
        send(mouse(.leftMouseDown, wc, 300, 330, pressure: 0.1), wc)
        spin()
        XCTAssertEqual(wc.brush.live.sent.first?.pressure, 1)
        send(mouse(.leftMouseUp, wc, 300, 330), wc)
    }

    // MARK: Live strokes

    func testCoalescedEventsAreKeptAndBatchedPerTurn() throws {
        let wc = try openActualSize()
        let doc = wc.editor.document
        tools.tool = .pencil
        tools.primary = DocColor(r: 255, g: 0, b: 0)
        XCTAssertTrue(NSEvent.isMouseCoalescingEnabled)
        // A zig-zag of 41 samples in one run-loop turn, as a 240 Hz pen sends
        // them within one frame. Each corner must be painted.
        let corners = (0...40).map { CGPoint(x: 20 + Double($0) * 11, y: $0 % 2 == 0 ? 40 : 90) }
        send(pen(.leftMouseDown, wc, corners[0].x, corners[0].y), wc)
        XCTAssertFalse(NSEvent.isMouseCoalescingEnabled, "every pen sample is delivered during a stroke")
        for c in corners.dropFirst() { send(pen(.leftMouseDragged, wc, c.x, c.y), wc) }
        XCTAssertEqual(wc.brush.live.appendCalls, 0, "nothing sent before the turn ends")
        spin()
        XCTAssertEqual(wc.brush.live.appendCalls, 1, "one engine call for the turn's samples")
        XCTAssertEqual(wc.brush.live.sent.count, 41, "no sample was dropped")
        XCTAssertEqual(wc.brush.live.sent.map { CGPoint(x: $0.x, y: $0.y) }, corners)
        let preview = try doc.pixels(preview: true)
        for c in corners {
            XCTAssertEqual(pixel(preview, 512, Int(c.x), Int(c.y)), [255, 0, 0, 255], "corner \(c)")
        }
        send(pen(.leftMouseUp, wc, 460, 40), wc)
        XCTAssertTrue(NSEvent.isMouseCoalescingEnabled, "coalescing is back after the stroke")
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.last == "Pencil" })
        XCTAssertEqual(try doc.pixels(), preview, "the commit keeps the previewed pixels")
    }

    func testCanvasShowsTheEnginePreviewWhileDrawing() throws {
        let wc = try openActualSize()
        let doc = wc.editor.document
        let view = try XCTUnwrap(wc.canvas.engineView)
        XCTAssertTrue(waitUntil(20) { ((try? view.info().frames) ?? 0) > 0 })
        let uploads = try view.info().docUploads
        tools.tool = .paintbrush
        tools.settings.brushWidth = 12
        let original = try doc.pixels()
        send(pen(.leftMouseDown, wc, 50, 50), wc)
        for i in 1...10 { send(pen(.leftMouseDragged, wc, 50 + Double(i) * 8, 50), wc) }
        spin()
        XCTAssertNotEqual(try doc.pixels(preview: true), original, "the engine has the stroke before mouse-up")
        XCTAssertEqual(try doc.pixels(), original, "and has not committed it")
        XCTAssertTrue(waitUntil(10) { ((try? view.info().regionUploads) ?? 0) > 0 }, "the canvas drew the preview")
        XCTAssertEqual(try view.info().docUploads, uploads, "as changed tiles, not a full upload")
        XCTAssertEqual(try view.info().frameErrors, 0)
        // The stand-in line is gone: the overlay draws nothing for brushes.
        XCTAssertFalse(wc.container.subviews.contains { String(describing: type(of: $0)).contains("Stroke") })
        send(pen(.leftMouseUp, wc, 130, 50), wc)
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.last == "Paintbrush" })
    }

    func testEscapeCancelsWithBytesUnchanged() throws {
        let wc = try openActualSize()
        let doc = wc.editor.document
        tools.tool = .paintbrush
        tools.settings.brushWidth = 20
        let original = try doc.pixels()
        let steps = wc.editor.info.historyLabels
        send(pen(.leftMouseDown, wc, 100, 100), wc)
        for i in 1...20 { send(pen(.leftMouseDragged, wc, 100 + Double(i) * 10, 120), wc) }
        spin()
        XCTAssertNotEqual(try doc.pixels(preview: true), original)
        wc.canvas.keyDown(with: keyEvent("\u{1b}", keyCode: 53, window: wc.window))
        XCTAssertFalse(wc.canvas.penActive)
        XCTAssertTrue(NSEvent.isMouseCoalescingEnabled)
        XCTAssertEqual(try doc.pixels(preview: true), original, "the preview is gone")
        XCTAssertEqual(try doc.pixels(), original, "the bytes are unchanged")
        // Later drags and the release do nothing.
        send(pen(.leftMouseDragged, wc, 300, 300), wc)
        send(pen(.leftMouseUp, wc, 300, 300), wc)
        spin()
        wc.editor.refresh()
        XCTAssertEqual(wc.editor.info.historyLabels, steps, "no history step")
        XCTAssertFalse(wc.editor.info.dirty)
        XCTAssertEqual(try doc.pixels(), original)
    }

    func testEachStrokeIsOneHistoryStep() throws {
        let wc = try openActualSize()
        let start = wc.editor.info.historyLabels.count
        for (tool, y) in [(PaintTool.paintbrush, 60.0), (.eraser, 120.0), (.pencil, 180.0)] {
            tools.tool = tool
            send(pen(.leftMouseDown, wc, 20, y), wc)
            for i in 1...30 {
                send(pen(.leftMouseDragged, wc, 20 + Double(i) * 12, y), wc)
                if i % 7 == 0 { spin() }
            }
            send(pen(.leftMouseUp, wc, 380, y), wc)
            XCTAssertGreaterThan(wc.brush.live.appendCalls, 1, "\(tool): several batches")
        }
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == start + 3 })
        XCTAssertEqual(Array(wc.editor.info.historyLabels.suffix(3)), ["Paintbrush", "Eraser", "Pencil"])
        XCTAssertTrue(wc.editor.info.dirty)
        XCTAssertEqual(HistoryStep.symbol("Eraser"), "paintbrush.pointed")
    }

    // MARK: Tool options

    func testToolOptionsFollowPaintNetPerTool() throws {
        let wc = try openActualSize()
        let bar = wc.optionsBar
        func shown(_ v: NSView) -> Bool { v.superview != nil }
        tools.tool = .paintbrush
        XCTAssertTrue([bar.widthField, bar.hardnessField, bar.antialias, bar.blend, bar.opacityField,
                       bar.pressureSize, bar.pressureOpacity].allSatisfy(shown))
        tools.tool = .eraser
        XCTAssertTrue([bar.widthField, bar.hardnessField, bar.antialias, bar.opacityField, bar.pressureSize,
                       bar.pressureOpacity].allSatisfy(shown))
        XCTAssertFalse(shown(bar.blend), "the eraser has no blend mode")
        tools.tool = .pencil
        XCTAssertTrue([bar.blend, bar.opacityField, bar.pressureOpacity].allSatisfy(shown))
        XCTAssertFalse([bar.widthField, bar.hardnessField, bar.antialias, bar.pressureSize].contains(where: shown),
                       "the pencil is always 1 px, hard and aliased")
        // The controls set the shared options, and the options go to the engine.
        tools.tool = .paintbrush
        bar.hardnessField.stringValue = "40"
        bar.hardnessField.sendAction(bar.hardnessField.action, to: bar.hardnessField.target)
        XCTAssertEqual(tools.settings.hardness, 40)
        bar.hardnessField.stringValue = "250"
        bar.hardnessField.sendAction(bar.hardnessField.action, to: bar.hardnessField.target)
        XCTAssertEqual(tools.settings.hardness, 100, "clamped")
        XCTAssertEqual(bar.hardnessField.stringValue, "100")
        tools.settings.hardness = 40
        bar.pressureSize.performClick(nil)
        bar.pressureOpacity.performClick(nil)
        XCTAssertTrue(tools.settings.pressureSize)
        XCTAssertTrue(tools.settings.pressureOpacity)
        tools.settings.opacity = 50
        tools.settings.brushWidth = 9
        tools.settings.blend = .overwrite
        var p = tools.liveStrokeParams(tool: .paintbrush)
        XCTAssertEqual(p["hardness"] as? Double, 0.4)
        XCTAssertEqual(p["pressure_size"] as? Bool, true)
        XCTAssertEqual(p["pressure_opacity"] as? Bool, true)
        XCTAssertEqual(p["width"] as? Double, 9)
        XCTAssertEqual(p["opacity"] as? Double, 128)
        XCTAssertEqual(p["blend"] as? String, "overwrite")
        p = tools.liveStrokeParams(tool: .eraser)
        XCTAssertEqual(p["blend"] as? String, "normal")
        p = tools.liveStrokeParams(tool: .pencil)
        XCTAssertEqual(p["width"] as? Double, 1)
        XCTAssertEqual(p["hardness"] as? Double, 1)
        XCTAssertEqual(p["antialias"] as? Bool, false)
        XCTAssertEqual(p["pressure_size"] as? Bool, false)
        XCTAssertEqual(p["pressure_opacity"] as? Bool, true)
        // The one-call tool.stroke parameters are unchanged (the engine refuses unknown keys).
        XCTAssertNil(tools.strokeParams(tool: .paintbrush)["hardness"])
        // A soft brush paints a softer edge than a hard one.
        tools.settings = ToolSettings()
        tools.settings.brushWidth = 30
        tools.primary = DocColor(r: 0, g: 0, b: 0)
        for (hardness, y) in [(100.0, 100.0), (0.0, 250.0)] {
            tools.settings.hardness = hardness
            send(pen(.leftMouseDown, wc, 200, y), wc)
            send(pen(.leftMouseUp, wc, 200, y), wc)
        }
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == 3 })
        let px = try wc.editor.document.pixels()
        // 12 px right of the center (0.8 of the radius).
        XCTAssertEqual(pixel(px, 512, 212, 100)[0...2], [0, 0, 0])
        XCTAssertNotEqual(pixel(px, 512, 212, 250)[0...2], [0, 0, 0])
    }

    /// The brush rows at the spec's sizes: 36 pt row, 28 pt controls. With
    /// OP_RENDER_DIR set, writes each row in light and dark for a visual check.
    func testBrushOptionRowsRenderInLightAndDark() throws {
        let wc = try openActualSize()
        let bar = wc.optionsBar
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        for tool in [PaintTool.paintbrush, .eraser, .pencil] {
            tools.tool = tool
            bar.frame = NSRect(x: 0, y: 0, width: 1180, height: Theme.optionsHeight)
            bar.layoutSubtreeIfNeeded()
            XCTAssertEqual(bar.frame.height, 36)
            for v in [bar.hardnessField, bar.opacityField, bar.blend, bar.pressureOpacity] where v.superview != nil {
                XCTAssertEqual(v.frame.height, 28, "\(tool) \(v.accessibilityLabel() ?? "")")
            }
            for name in [NSAppearance.Name.aqua, .darkAqua] {
                bar.appearance = NSAppearance(named: name)
                let rep = try XCTUnwrap(bar.bitmapImageRepForCachingDisplay(in: bar.bounds))
                NSAppearance(named: name)!.performAsCurrentDrawingAppearance {
                    bar.cacheDisplay(in: bar.bounds, to: rep)
                }
                if let out {
                    let file = URL(fileURLWithPath: out)
                        .appendingPathComponent("options-\(tool.rawValue)-\(name == .aqua ? "light" : "dark").png")
                    try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: file)
                }
            }
            bar.appearance = nil
        }
    }

    /// Closing a window releases its Metal layer, which flushes Core Animation
    /// and can lay out another window's canvas, which sends its view state from
    /// inside the detach call. That used to deadlock on the engine lock.
    func testClosingAWindowWhileAnotherNeedsLayoutDoesNotHang() throws {
        let a = try fx.open()
        let b = try fx.open("checker_600x300.png")
        var frame = try XCTUnwrap(b.window?.frame)
        frame.size.width -= 40
        b.window?.setFrame(frame, display: false)
        b.container.needsLayout = true
        b.canvas.needsLayout = true
        a.closeWindow()
        XCTAssertNil(a.canvas.engineView)
        XCTAssertNotNil(b.canvas.engineView)
    }

    // MARK: Recorded tablet logs

    func testRecordedTabletLogReplaysToTheSameBytesAsTheEngineFedDirectly() throws {
        let log = try TabletLog.decode(Data(contentsOf: Self.logURL))
        XCTAssertEqual(log.strokes.count, 2)
        XCTAssertEqual(log.strokes.map(\.count), [140, 90])
        // A: the log through the canvas input path (events, pen sampling, batching).
        let a = try openActualSize()
        tools.tool = .paintbrush
        tools.primary = DocColor(r: 20, g: 90, b: 200)
        tools.settings.brushWidth = 18
        tools.settings.hardness = 60
        tools.settings.pressureSize = true
        tools.settings.pressureOpacity = true
        let params = tools.liveStrokeParams(tool: .paintbrush)
        let recorder = TabletLogRecorder()
        a.canvas.recorder = recorder
        let start = ProcessInfo.processInfo.systemUptime
        for (i, e) in log.events(for: a.canvas, start: start).enumerated() {
            TabletLog.deliver(e, to: a.canvas)
            if i % 4 == 3 { spin() }  // about one turn per 4 samples (240 Hz into 60 Hz)
        }
        XCTAssertTrue(waitUntil { a.editor.info.historyLabels.count == 3 }, a.editor.message ?? "")
        let replayed = try a.editor.document.pixels()

        // B: the same samples straight into the engine, one append per stroke.
        let b = try openActualSize()
        let doc = b.editor.document
        let original = try doc.pixels()
        for stroke in log.strokes {
            try doc.strokeBegin(params)
            try doc.strokeAppend(TabletLog.strokeSamples(stroke))
            try doc.strokeEnd()
        }
        let direct = try doc.pixels()
        XCTAssertNotEqual(direct, original, "the log paints")
        XCTAssertEqual(replayed.count, direct.count)
        XCTAssertTrue(replayed == direct, "replay and direct feed match byte for byte")
        if replayed != direct {
            let diff = zip(replayed, direct).filter { $0 != $1 }.count
            XCTFail("\(diff) bytes differ")
        }
        // The recorder saw what the log holds: positions, pressure, tilt, buttons.
        let rec = recorder.log
        XCTAssertEqual(rec.samples.count, log.samples.filter { $0.buttons != 0 }.count + 2, "drags plus two releases")
        let recDown = rec.samples.filter { $0.buttons != 0 }
        let logDown = log.samples.filter { $0.buttons != 0 }
        XCTAssertEqual(recDown.map { [$0.x, $0.y, $0.pressure] + $0.tilt }, logDown.map { [$0.x, $0.y, $0.pressure] + $0.tilt })
        // Encoding round-trips.
        XCTAssertEqual(try TabletLog.decode(rec.encoded()).samples.count, rec.samples.count)
        XCTAssertThrowsError(try TabletLog.decode(Data(#"{"format":"x","version":1,"space":"document-pixels","samples":[]}"#.utf8)))
        XCTAssertThrowsError(try TabletLog.decode(Data(
            #"{"format":"openpainter-tablet-log","version":1,"space":"document-pixels","samples":[{"t":0,"x":0,"y":0,"pressure":2,"tilt":[0,0],"buttons":1}]}"#.utf8)))
    }

    // MARK: Latency

    /// Pen event to frame on a 6000 x 4000 image: synthetic tablet events at
    /// 120 Hz, each stamped with its time; the engine stamps each frame with the
    /// preview it shows. Gate: median under 16 ms.
    func testPenEventToFrameLatencyOn6000x4000() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("openpainter-m38-\(getpid())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let png = dir.appendingPathComponent("latency-6000x4000.png")
        try TraceImage.write(to: png)
        let doc = try fx.engine.openPNG(path: png.path)
        let editor = Editor(document: doc, pixelSize: CGSize(width: 6000, height: 4000), name: "latency")
        fx.workspace.add(editor)
        let wc = EditorWindowController(editor: editor, workspace: fx.workspace)
        wc.window?.isReleasedWhenClosed = false
        fx.controllers.append(wc)
        wc.present()
        wc.window?.layoutIfNeeded()
        wc.updateClearRect()
        let view = try XCTUnwrap(wc.canvas.engineView)
        XCTAssertTrue(waitUntil(60) { ((try? view.info().frames) ?? 0) > 0 }, "first frame")
        tools.tool = .paintbrush
        tools.settings.brushWidth = 40
        tools.settings.pressureSize = true
        _ = try view.takeFrameStamps()
        wc.brush.live.latency = []
        let revision = UInt64(wc.editor.info.revision)
        let canvas = wc.canvas
        let bounds = canvas.bounds
        let count = 240
        var sent = 0
        let timer = Timer(timeInterval: 1.0 / 120, repeats: true) { _ in
            MainActor.assumeIsolated {
                guard sent <= count else { return }
                let u = Double(sent) / Double(count)
                let viewPt = CGPoint(x: bounds.minX + bounds.width * (0.2 + 0.6 * u),
                                     y: bounds.midY + bounds.height * 0.3 * sin(u * 6 * .pi))
                let d = canvas.viewport.documentPoint(atView: viewPt)
                let type: NSEvent.EventType = sent == 0 ? .leftMouseDown : sent == count ? .leftMouseUp : .leftMouseDragged
                let e = TabletLog.event(type, canvas: canvas, document: d, pressure: 0.3 + 0.7 * u, tilt: .zero,
                                        time: ProcessInfo.processInfo.systemUptime)!
                TabletLog.deliver(e, to: canvas)
                sent += 1
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        XCTAssertTrue(waitUntil(30) { sent > count })
        timer.invalidate()
        _ = waitUntil(0.5) { false }
        let stamps = try view.takeFrameStamps()
        let marks = try XCTUnwrap(wc.brush.live.latency)
        XCTAssertEqual(marks.count, count, "every drag sample was sent (the press plus \(count - 1) drags)")
        var latencies: [Double] = []
        var unmatched = 0
        for m in marks {
            if let f = stamps.first(where: { ($0.revision == revision && $0.previewSerial >= m.serial) || $0.revision > revision }),
                f.timeNs >= m.eventTimeNs
            {
                latencies.append(Double(f.timeNs - m.eventTimeNs) / 1e6)
            } else {
                unmatched += 1
            }
        }
        XCTAssertEqual(unmatched, 0, "every event reached a frame")
        latencies.sort()
        let median = latencies.isEmpty ? .infinity : latencies[latencies.count / 2]
        let p95 = latencies.isEmpty ? .infinity : latencies[min(latencies.count - 1, Int(Double(latencies.count) * 0.95))]
        var load = [Double](repeating: 0, count: 3)
        getloadavg(&load, 3)
        let info = try view.info()
        print(String(format: "LATENCY pen-event-to-frame on 6000x4000: median %.2f ms, p95 %.2f ms, max %.2f ms, %d events, %d frames, %d appends, region uploads %llu (%llu tiles), full uploads %llu, view %dx%d pt at %.0f%%, load average %.1f %.1f %.1f",
                     median, p95, latencies.last ?? 0, latencies.count, stamps.count, wc.brush.live.appendCalls,
                     info.regionUploads, info.regionTiles, info.docUploads, Int(bounds.width), Int(bounds.height),
                     canvas.viewport.zoom * 100, load[0], load[1], load[2]))
        XCTAssertLessThan(median, 16, "median pen-event-to-frame latency")
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.last == "Paintbrush" })
        XCTAssertEqual(info.frameErrors, 0)
    }
}

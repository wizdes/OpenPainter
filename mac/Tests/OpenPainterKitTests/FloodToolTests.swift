// M31: the live Magic Wand and Paint Bucket. A UI scenario (click, two
// tolerance changes, a seed drag, Finish) gives the bytes and the selection
// of the same engine calls and one history step; Escape keeps the document
// hash; every option change updates the preview; Paint Bucket blend mode and
// antialiasing give the engine's exact bytes; Finish, another tool, a click
// elsewhere and a menu command commit; Undo drops. The rendered options row
// and handle go to OP_RENDER_DIR when it is set.
import AppKit
import CryptoKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class FloodToolTests: XCTestCase {
    var fx: ShellFixture!
    let fixture = "plasma_alpha_512x384.png"
    var tools: ToolState { fx.workspace.tools }

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    // MARK: Helpers

    func idle(_ wc: EditorWindowController) -> Bool { waitUntil(30) { wc.flood.isIdle } }

    /// Pixels, selection, layers and history: what Escape must keep.
    func hash(_ e: Editor) throws -> String {
        var h = SHA256()
        h.update(data: Data(try e.document.pixels()))
        h.update(data: Data(try e.document.selectionMask(preview: false)))
        let info = try e.document.info()
        let keys = ["width", "height", "layers", "active_layer", "history_labels", "history_index", "dirty"]
        h.update(data: try JSONSerialization.data(withJSONObject: info.filter { keys.contains($0.key) },
                                                  options: [.sortedKeys]))
        return h.finalize().map { String(format: "%02x", $0) }.joined()
    }

    /// One plain command on a fresh copy of the fixture: its pixels and selection.
    func reference(_ command: String, _ params: [String: any Sendable],
                   before: [(String, [String: any Sendable])] = [],
                   on image: String? = nil) throws -> (pixels: [UInt8], mask: [UInt8]) {
        let doc = try fx.engine.openPNG(path: fixtures.appendingPathComponent(image ?? fixture).path)
        defer { try? doc.close() }
        for (id, p) in before { XCTAssertEqual(LayersHistoryTests.runSync(doc, id, p), "ok", id) }
        XCTAssertEqual(LayersHistoryTests.runSync(doc, command, params), "ok", command)
        return (try doc.pixels(), try doc.selectionMask(preview: false))
    }

    func mouse(_ wc: EditorWindowController, _ type: NSEvent.EventType, at view: CGPoint) -> NSEvent {
        let p = wc.canvas.convert(view, to: nil)
        return NSEvent.mouseEvent(with: type, location: p, modifierFlags: [], timestamp: 0,
                                  windowNumber: wc.window!.windowNumber, context: nil, eventNumber: 0,
                                  clickCount: 1, pressure: 1)!
    }

    /// Drags the click-point handle to a document pixel with real mouse events.
    func dragSeed(_ wc: EditorWindowController, to pixel: CGPoint) throws {
        let start = try XCTUnwrap(wc.flood.handle.center, "the handle shows")
        let end = wc.canvas.viewport.viewPoint(ofDocument: CGPoint(x: pixel.x + 0.5, y: pixel.y + 0.5))
        wc.canvas.mouseDown(with: mouse(wc, .leftMouseDown, at: start))
        XCTAssertTrue(wc.canvas.handleDragging, "the press grabs the handle")
        let mid = CGPoint(x: (start.x + end.x) / 2, y: (start.y + end.y) / 2)
        wc.canvas.mouseDragged(with: mouse(wc, .leftMouseDragged, at: mid))
        wc.canvas.mouseDragged(with: mouse(wc, .leftMouseDragged, at: end))
        wc.canvas.mouseUp(with: mouse(wc, .leftMouseUp, at: end))
        XCTAssertFalse(wc.canvas.handleDragging)
    }

    func choose(_ popup: NSPopUpButton, _ index: Int) {
        popup.selectItem(at: index)
        popup.sendAction(popup.action, to: popup.target)
    }

    // MARK: The scenario

    func testWandScenarioEqualsTheEngineCallsInOneStep() throws {
        let wc = try fx.open(fixture)
        let e = wc.editor
        tools.tool = .magicWand
        tools.settings.tolerance = 20
        tools.settings.selectionMode = .replace
        let n = e.info.historyLabels.count
        wc.click(CGPoint(x: 200.4, y: 150.7))
        XCTAssertTrue(idle(wc))
        XCTAssertEqual(wc.flood.session?.x, 200)
        XCTAssertEqual(wc.flood.session?.y, 150)
        XCTAssertEqual(e.info.historyLabels.count, n, "nothing commits until Finish")
        XCTAssertTrue(wc.optionsBar.floodFinish.isEnabled)
        XCTAssertNotNil(wc.selectCrop.overlay.selectionPath, "the live selection shows its ants")
        // Two tolerance changes: the slider, then the field.
        let bar = wc.optionsBar
        bar.toleranceSlider.doubleValue = 35.4
        bar.toleranceSlider.sendAction(bar.toleranceSlider.action, to: bar.toleranceSlider.target)
        XCTAssertEqual(tools.settings.tolerance, 35)
        XCTAssertTrue(idle(wc))
        let at35 = try e.document.selectionMask(preview: true)
        XCTAssertEqual(at35, try reference("tool.magic_wand", tools.floodParams(tool: .magicWand, x: 200, y: 150)).mask)
        bar.toleranceField.stringValue = "42"
        bar.toleranceField.sendAction(bar.toleranceField.action, to: bar.toleranceField.target)
        XCTAssertEqual(tools.settings.tolerance, 42)
        XCTAssertEqual(bar.toleranceSlider.doubleValue, 42, "slider and field stay in step")
        XCTAssertTrue(idle(wc))
        XCTAssertNotEqual(try e.document.selectionMask(preview: true), at35, "the preview follows the tolerance")
        // Drag the click point.
        try dragSeed(wc, to: CGPoint(x: 60, y: 300))
        XCTAssertEqual(wc.flood.session?.x, 60)
        XCTAssertEqual(wc.flood.session?.y, 300)
        XCTAssertTrue(idle(wc))
        let final = tools.floodParams(tool: .magicWand, x: 60, y: 300)
        let ref = try reference("tool.magic_wand", final)
        XCTAssertEqual(try e.document.selectionMask(preview: true), ref.mask, "live selection at the new point")
        XCTAssertEqual(e.info.historyLabels.count, n)
        // Finish with Enter.
        wc.canvasKey(.enter)
        XCTAssertNil(wc.flood.session)
        XCTAssertFalse(wc.optionsBar.floodFinish.isEnabled)
        XCTAssertNil(wc.flood.handle.center)
        XCTAssertTrue(idle(wc))
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n + 1 })
        XCTAssertEqual(e.info.historyLabels.last, "Magic Wand")
        XCTAssertEqual(try e.document.selectionMask(preview: false), ref.mask)
        XCTAssertEqual(try e.document.pixels(), ref.pixels)
        XCTAssertNotNil(wc.selectCrop.overlay.selectionPath)
    }

    func testBucketScenarioEqualsTheEngineCallsInOneStep() throws {
        let wc = try fx.open(fixture)
        let e = wc.editor
        tools.tool = .paintBucket
        tools.settings.tolerance = 25
        tools.settings.blend = .overwrite
        tools.settings.antialias = true
        tools.settings.opacity = 60
        tools.primary = DocColor(r: 10, g: 200, b: 30)
        let n = e.info.historyLabels.count
        let before = try e.document.pixels()
        wc.click(CGPoint(x: 300, y: 100))
        XCTAssertTrue(idle(wc))
        XCTAssertEqual(try e.document.pixels(), before, "the fill is a preview")
        XCTAssertNotEqual(try e.document.pixels(preview: true), before)
        tools.settings.tolerance = 30
        tools.settings.tolerance = 45
        try dragSeed(wc, to: CGPoint(x: 420, y: 330))
        XCTAssertTrue(idle(wc))
        wc.optionsBar.floodFinish.performClick(nil)
        XCTAssertTrue(idle(wc))
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n + 1 })
        XCTAssertEqual(e.info.historyLabels.last, "Paint Bucket")
        let ref = try reference("tool.paint_bucket", tools.floodParams(tool: .paintBucket, x: 420, y: 330))
        XCTAssertEqual(try e.document.pixels(), ref.pixels)
    }

    // MARK: Escape

    func testEscapeKeepsTheDocumentHash() throws {
        let wc = try fx.open(fixture)
        let e = wc.editor
        for tool in [PaintTool.magicWand, .paintBucket] {
            tools.tool = tool
            tools.settings.tolerance = 30
            let h = try hash(e)
            wc.click(CGPoint(x: 250, y: 200))
            XCTAssertTrue(idle(wc))
            tools.settings.tolerance = 55
            XCTAssertTrue(idle(wc))
            XCTAssertNotEqual(try e.document.pixels(preview: true) + (try e.document.selectionMask(preview: true)),
                              try e.document.pixels() + (try e.document.selectionMask(preview: false)),
                              "\(tool): something shows")
            wc.canvasKey(.escape)
            XCTAssertNil(wc.flood.session)
            XCTAssertTrue(idle(wc))
            e.refresh()
            XCTAssertEqual(try hash(e), h, "\(tool)")
            XCTAssertEqual(try e.document.pixels(preview: true), try e.document.pixels(), "\(tool): no preview left")
            XCTAssertNil(wc.selectCrop.overlay.selectionPath, "\(tool): no ants")
        }
    }

    // MARK: Every option updates the preview

    func testEveryOptionChangeUpdatesTheBucketPreview() throws {
        let wc = try fx.open(fixture)
        let e = wc.editor
        let bar = wc.optionsBar
        tools.tool = .paintBucket
        tools.settings.tolerance = 30
        tools.settings.antialias = false
        tools.settings.blend = .normal
        tools.settings.opacity = 50
        wc.click(CGPoint(x: 120, y: 90))
        XCTAssertTrue(idle(wc))
        var last = try e.document.pixels(preview: true)
        // The flag says whether the change must show on this image: one layer,
        // so Layer and Image sampling see the same pixels; the tolerance alpha
        // mode changes only where alpha differs inside the tolerance; on this
        // smooth plasma the contiguous region can equal the global one (the
        // checker test below shows Global).
        let changes: [(String, Bool, () -> Void)] = [
            ("blend mode", true, { self.choose(bar.blend, ToolSettings.Blend.allCases.firstIndex(of: .overwrite)!) }),
            ("antialias", true, { bar.antialias.performClick(nil) }),
            ("flood mode", false, {
                self.choose(bar.floodMode, ToolSettings.FloodMode.allCases.firstIndex(of: .global)!)
            }),
            ("tolerance alpha", false, {
                self.choose(bar.toleranceAlpha, ToolSettings.ToleranceAlpha.allCases.firstIndex(of: .straight)!)
            }),
            ("sampling", false, { self.choose(bar.sampling, ToolSettings.Sampling.allCases.firstIndex(of: .image)!) }),
            ("tolerance", true, {
                bar.toleranceSlider.doubleValue = 60
                bar.toleranceSlider.sendAction(bar.toleranceSlider.action, to: bar.toleranceSlider.target)
            }),
            ("primary color", true, { self.tools.primary = DocColor(r: 250, g: 20, b: 120) }),
        ]
        for (name, mustChange, change) in changes {
            change()
            XCTAssertTrue(idle(wc), name)
            let shown = try e.document.pixels(preview: true)
            if mustChange { XCTAssertNotEqual(shown, last, "\(name) changes the preview") }
            let ref = try reference("tool.paint_bucket", tools.floodParams(tool: .paintBucket, x: 120, y: 90))
            XCTAssertEqual(shown, ref.pixels, "\(name): the engine's exact bytes")
            last = shown
        }
        XCTAssertEqual(tools.settings.blend, .overwrite)
        XCTAssertTrue(tools.settings.antialias)
        XCTAssertEqual(tools.settings.toleranceAlpha, .straight)
        let n = e.info.historyLabels.count
        XCTAssertTrue(wc.flood.finish())
        XCTAssertTrue(idle(wc))
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n + 1 })
        XCTAssertEqual(try e.document.pixels(), last)
    }

    func testEveryOptionChangeUpdatesTheWandPreview() throws {
        let wc = try fx.open(fixture)
        let e = wc.editor
        let bar = wc.optionsBar
        let rect: [String: any Sendable] = ["x": 100.0, "y": 80.0, "width": 200.0, "height": 150.0]
        XCTAssertEqual(LayersHistoryTests.runSync(e.document, "selection.rectangle", rect), "ok")
        e.refresh()
        tools.tool = .magicWand
        tools.settings.tolerance = 30
        tools.settings.selectionMode = .replace
        tools.settings.antialias = false
        wc.click(CGPoint(x: 150, y: 120))
        XCTAssertTrue(idle(wc))
        var last = try e.document.selectionMask(preview: true)
        // Selection modes first, while the region is smaller than the rectangle.
        var changes: [(String, Bool, () -> Void)] = []
        for mode in [ToolSettings.SelectionMode.add, .subtract, .intersect, .invert] {
            changes.append(("selection mode \(mode)", true, {
                self.choose(bar.selectionMode, ToolSettings.SelectionMode.allCases.firstIndex(of: mode)!)
            }))
        }
        changes += [
            ("antialias", true, { bar.antialias.performClick(nil) }),
            ("tolerance", true, {
                bar.toleranceSlider.doubleValue = 45
                bar.toleranceSlider.sendAction(bar.toleranceSlider.action, to: bar.toleranceSlider.target)
            }),
            // See the bucket test: Global shows on the checker test.
            ("flood mode", false, {
                self.choose(bar.floodMode, ToolSettings.FloodMode.allCases.firstIndex(of: .global)!)
            }),
        ]
        for (name, mustChange, change) in changes {
            change()
            XCTAssertTrue(idle(wc), name)
            let shown = try e.document.selectionMask(preview: true)
            if mustChange { XCTAssertNotEqual(shown, last, "\(name) changes the live selection") }
            let ref = try reference("tool.magic_wand", tools.floodParams(tool: .magicWand, x: 150, y: 120),
                                    before: [("selection.rectangle", rect)])
            XCTAssertEqual(shown, ref.mask, "\(name): the engine's exact selection")
            last = shown
        }
        // Sampling and tolerance alpha: the fixture has one layer with alpha, so
        // check that each sends its value and the preview equals the engine's.
        for (name, change) in [
            ("sampling", { self.choose(bar.sampling, ToolSettings.Sampling.allCases.firstIndex(of: .image)!) }),
            ("tolerance alpha", {
                self.choose(bar.toleranceAlpha, ToolSettings.ToleranceAlpha.allCases.firstIndex(of: .straight)!)
            }),
        ] as [(String, () -> Void)] {
            change()
            XCTAssertTrue(idle(wc), name)
            let ref = try reference("tool.magic_wand", tools.floodParams(tool: .magicWand, x: 150, y: 120),
                                    before: [("selection.rectangle", rect)])
            XCTAssertEqual(try e.document.selectionMask(preview: true), ref.mask, name)
        }
        XCTAssertEqual(tools.floodParams(tool: .magicWand, x: 0, y: 0)["sampling"] as? String, "image")
        XCTAssertEqual(tools.floodParams(tool: .magicWand, x: 0, y: 0)["alpha"] as? String, "straight")
    }

    /// Contiguous to Global on a checkerboard: one square becomes every square
    /// of that color, for both tools.
    func testGlobalFloodModeUpdatesBothPreviewsOnACheckerboard() throws {
        let checker = "checker_600x300.png"
        let wc = try fx.open(checker)
        let e = wc.editor
        let bar = wc.optionsBar
        tools.settings.tolerance = 10
        tools.settings.floodMode = .contiguous
        tools.settings.selectionMode = .replace
        for tool in [PaintTool.magicWand, .paintBucket] {
            tools.tool = tool
            choose(bar.floodMode, ToolSettings.FloodMode.allCases.firstIndex(of: .contiguous)!)
            wc.click(CGPoint(x: 5, y: 5))
            XCTAssertTrue(idle(wc))
            let one = try e.document.pixels(preview: true) + (try e.document.selectionMask(preview: true))
            choose(bar.floodMode, ToolSettings.FloodMode.allCases.firstIndex(of: .global)!)
            XCTAssertTrue(idle(wc))
            let all = try e.document.pixels(preview: true) + (try e.document.selectionMask(preview: true))
            XCTAssertNotEqual(all, one, "\(tool)")
            let ref = try reference(FloodToolController.command(tool), tools.floodParams(tool: tool, x: 5, y: 5),
                                    on: checker)
            XCTAssertEqual(all, ref.pixels + ref.mask, "\(tool): the engine's exact result")
            XCTAssertTrue(wc.flood.cancel())
            XCTAssertTrue(idle(wc))
        }
    }

    // MARK: Other ways to finish, and Undo

    func testAnotherToolAClickElsewhereAndAMenuCommandFinishAndUndoDrops() throws {
        let wc = try fx.open(fixture)
        let e = wc.editor
        tools.tool = .paintBucket
        tools.settings.tolerance = 20
        let n = e.info.historyLabels.count
        // Another tool finishes.
        wc.click(CGPoint(x: 50, y: 50))
        XCTAssertTrue(idle(wc))
        tools.tool = .paintbrush
        XCTAssertNil(wc.flood.session)
        XCTAssertTrue(idle(wc))
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n + 1 })
        // A click elsewhere finishes the first fill and starts a second.
        tools.tool = .paintBucket
        wc.click(CGPoint(x: 400, y: 300))
        XCTAssertTrue(idle(wc))
        wc.click(CGPoint(x: 100, y: 300))
        XCTAssertTrue(idle(wc))
        XCTAssertEqual(wc.flood.session?.x, 100)
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n + 2 })
        // A command from elsewhere (Add New Layer) commits it in the engine first.
        wc.addLayer()
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n + 4 })
        XCTAssertEqual(Array(e.info.historyLabels.suffix(2)), ["Paint Bucket", "Add New Layer"])
        XCTAssertTrue(waitUntil { wc.flood.session == nil }, "the session ends here too")
        // Undo while live drops the fill only.
        let h = try hash(e)
        wc.click(CGPoint(x: 10, y: 10))
        XCTAssertTrue(idle(wc))
        wc.undo()
        XCTAssertNil(wc.flood.session)
        XCTAssertTrue(idle(wc))
        e.refresh()
        XCTAssertEqual(try hash(e), h)
        // A click outside the image does nothing.
        wc.click(CGPoint(x: -3, y: 10))
        XCTAssertNil(wc.flood.session)
    }

    // MARK: Visual check

    /// With TEST_RUNNER_OP_RENDER_DIR set, writes the Magic Wand and Paint
    /// Bucket option rows (live session, Finish on) and the handle over a
    /// stand-in image, in light and dark.
    func testRenderedFloodRowsAndHandle() throws {
        let wc = try fx.open(fixture)
        let bar = wc.optionsBar
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        func write(_ rep: NSBitmapImageRep, _ name: String) throws {
            guard let out else { return }
            let url = URL(fileURLWithPath: out).appendingPathComponent("m31-\(name).png")
            try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: url)
        }
        func render(_ view: NSView, _ appearance: NSAppearance.Name) throws -> NSBitmapImageRep {
            view.appearance = NSAppearance(named: appearance)
            view.needsLayout = true
            view.layoutSubtreeIfNeeded()
            for sub in view.subviewsDeep { sub.needsDisplay = true }
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            return rep
        }
        for tool in [PaintTool.magicWand, .paintBucket] {
            tools.tool = tool
            wc.click(CGPoint(x: 200, y: 150))
            XCTAssertTrue(idle(wc))
            bar.frame = NSRect(x: 0, y: 0, width: 1720, height: Theme.optionsHeight)
            bar.layoutSubtreeIfNeeded()
            XCTAssertEqual(bar.toleranceSlider.frame.width, 96, "\(tool)")
            XCTAssertEqual(bar.toleranceSlider.frame.height, 28, "\(tool)")
            XCTAssertEqual(bar.floodFinish.frame.height, 28, "\(tool)")
            XCTAssertEqual(bar.toleranceAlpha.frame.height, 28, "\(tool)")
            XCTAssertTrue(bar.floodFinish.isEnabled)
            for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
                try write(try render(bar, a), "\(tool == .magicWand ? "wand" : "bucket")-options-\(n)")
            }
            wc.flood.cancel()
            XCTAssertTrue(idle(wc))
        }
        // The handle over a stand-in image (the Metal canvas cannot be captured here).
        let hv = FloodHandleView(frame: NSRect(x: 0, y: 0, width: 160, height: 120))
        hv.center = CGPoint(x: 80, y: 60)
        XCTAssertTrue(hv.grabs(CGPoint(x: 90, y: 68)))
        XCTAssertFalse(hv.grabs(CGPoint(x: 100, y: 60)))
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let host = NSView(frame: hv.frame)
            host.wantsLayer = true
            host.layer?.backgroundColor = NSColor.systemTeal.cgColor
            host.addSubview(hv)
            try write(try render(host, a), "handle-\(n)")
            hv.removeFromSuperview()
        }
    }
}

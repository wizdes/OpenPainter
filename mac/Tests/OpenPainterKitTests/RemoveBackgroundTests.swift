// Layers > Remove Background (M53), driven in process through the window
// controller, the progress sheet, Apple Vision and the engine:
// - IoU of the Vision mask (thresholded at 128) against the true mask is at
//   least 0.9 for each of the 10 named photos in tests/fixtures/remove-bg.
// - Soft coverage survives into the selection (values 1 to 254).
// - Select Foreground and Remove Background are one undo step each.
// - Cancel leaves the document hash unchanged.
import AppKit
import CryptoKit
import ImageIO
@testable import OpenPainterKit
import XCTest

@MainActor
final class RemoveBackgroundTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
        ForegroundMasker.compute = ForegroundMasker.vision
    }

    override func tearDown() async throws {
        ForegroundMasker.compute = ForegroundMasker.vision
        fx.tearDown()
    }

    static let photoDir = fixtures.appendingPathComponent("remove-bg")
    static let photos = [
        "01-person", "02-pet", "03-product", "04-plant", "05-hair",
        "06-glass", "07-car", "08-sign", "09-two-objects", "10-low-contrast",
    ]

    /// Straight RGBA8 of an image file, drawn in sRGB.
    static func rgba(_ url: URL) throws -> (pixels: [UInt8], width: Int, height: Int) {
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let img = try XCTUnwrap(CGImageSourceCreateImageAtIndex(src, 0, nil))
        let (w, h) = (img.width, img.height)
        var px = [UInt8](repeating: 0, count: w * h * 4)
        px.withUnsafeMutableBytes { p in
            let ctx = CGContext(data: p.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return (px, w, h)  // the photos are opaque, so premultiplied equals straight
    }

    /// The true mask: one gray byte per pixel.
    static func trueMask(_ name: String) throws -> [UInt8] {
        let url = photoDir.appendingPathComponent("\(name)-mask.png")
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let img = try XCTUnwrap(CGImageSourceCreateImageAtIndex(src, 0, nil))
        var g = [UInt8](repeating: 0, count: img.width * img.height)
        g.withUnsafeMutableBytes { p in
            let ctx = CGContext(data: p.baseAddress, width: img.width, height: img.height, bitsPerComponent: 8,
                                bytesPerRow: img.width, space: CGColorSpaceCreateDeviceGray(),
                                bitmapInfo: CGImageAlphaInfo.none.rawValue)!
            ctx.draw(img, in: CGRect(x: 0, y: 0, width: img.width, height: img.height))
        }
        return g
    }

    static func iou(_ a: [UInt8], _ b: [UInt8]) -> Double {
        var both = 0, either = 0
        for i in 0..<min(a.count, b.count) {
            let x = a[i] >= 128, y = b[i] >= 128
            if x && y { both += 1 }
            if x || y { either += 1 }
        }
        return either == 0 ? 1 : Double(both) / Double(either)
    }

    /// Opens straight RGBA as an editor window.
    func open(_ px: [UInt8], _ w: Int, _ h: Int, name: String) throws -> EditorWindowController {
        let doc = try fx.engine.importPixels(width: UInt32(w), height: UInt32(h), rgba: px)
        let editor = Editor(document: doc, pixelSize: CGSize(width: w, height: h), name: name)
        fx.workspace.add(editor)
        let wc = EditorWindowController(editor: editor, workspace: fx.workspace)
        wc.window?.isReleasedWhenClosed = false
        fx.controllers.append(wc)
        wc.present()
        return wc
    }

    func openPhoto(_ name: String) throws -> EditorWindowController {
        let (px, w, h) = try Self.rgba(Self.photoDir.appendingPathComponent("\(name).jpg"))
        return try open(px, w, h, name: name)
    }

    /// Waits by suspending (not by spinning the run loop), so the main-queue
    /// callback that ends a run can execute while an async test waits.
    func eventually(_ timeout: TimeInterval = 60, _ condition: @escaping @MainActor () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    /// Starts a run and waits until it ends and its engine command (if any) is done.
    func run(_ wc: EditorWindowController, _ outcome: RemoveBackgroundOutcome,
             timeout: TimeInterval = 60) async throws -> RemoveBackgroundController.State {
        let steps = try historyCount(wc.editor)
        let c = try XCTUnwrap(wc.startRemoveBackground(outcome))
        let ended1 = await eventually(timeout) { c.state != .running }
        XCTAssertTrue(ended1, "the run ends")
        if case .applied = c.state {
            // `Editor.run` itself waits for the ticket; wait for its step here.
            let committed = await eventually(timeout) { [self] in ((try? historyCount(wc.editor)) ?? 0) == steps + 1 }
            XCTAssertTrue(committed, "one history step")
            wc.editor.refresh()
        }
        XCTAssertNil(wc.window?.attachedSheet, "the sheet is gone")
        return c.state
    }

    /// Everything a run could change: every layer's pixels, the selection, the history.
    func hash(_ editor: Editor) throws -> String {
        var h = SHA256()
        let info = try editor.document.info()
        for i in 0..<((info["layers"] as? [Any])?.count ?? 0) {
            h.update(data: try editor.document.queryData("layer.pixels", ["index": i]))
        }
        h.update(data: try editor.document.queryData("doc.selection"))
        h.update(data: try editor.document.queryData("doc.selection_mask"))
        h.update(data: try JSONSerialization.data(withJSONObject: [
            info["history_labels"] ?? [], info["history_index"] ?? 0, info["active_layer"] ?? 0,
        ]))
        return h.finalize().map { String(format: "%02x", $0) }.joined()
    }

    func historyCount(_ editor: Editor) throws -> Int {
        (try editor.document.info()["history_labels"] as? [Any])?.count ?? 0
    }

    // MARK: Vision accuracy

    func testVisionMaskMatchesEachNamedPhotoAtIoU90AndKeepsSoftEdges() async throws {
        var report: [String] = []
        for name in Self.photos {
            let wc = try openPhoto(name)
            let truth = try Self.trueMask(name)
            let state = try await run(wc, .selectForeground)
            guard case .applied = state else {
                XCTFail("\(name): \(state)")
                continue
            }
            let mask = [UInt8](try wc.editor.document.queryData("doc.selection_mask"))
            XCTAssertEqual(mask.count, truth.count, "\(name): full resolution")
            let iou = Self.iou(mask, truth)
            let soft = mask.filter { (1...254).contains($0) }.count
            report.append(String(format: "%@ IoU %.4f soft %d", name, iou, soft))
            XCTAssertGreaterThanOrEqual(iou, 0.9, "\(name): IoU at threshold 128")
            XCTAssertGreaterThan(soft, 100, "\(name): soft coverage survives into the selection")
            XCTAssertEqual(wc.editor.info.historyLabels.last, "Select Foreground")
            wc.window?.orderOut(nil)
        }
        print("M53 REMOVE BACKGROUND IoU (threshold 128)\n" + report.joined(separator: "\n"))
    }

    // MARK: One step each

    func testSelectForegroundIsOneUndoStep() async throws {
        let wc = try openPhoto("02-pet")
        let steps = try historyCount(wc.editor)
        guard case .applied = try await run(wc, .selectForeground) else { return XCTFail("not applied") }
        XCTAssertEqual(try historyCount(wc.editor), steps + 1)
        XCTAssertNotNil(wc.editor.selection)
        let status1 = await wc.editor.runAndWait("edit.undo")
        XCTAssertEqual(status1, "ok")
        XCTAssertNil(wc.editor.selection, "one undo removes the selection")
        XCTAssertEqual(wc.editor.info.historyIndex, steps - 1)
        XCTAssertEqual([UInt8](try wc.editor.document.queryData("layer.pixels")),
                       try Self.rgba(Self.photoDir.appendingPathComponent("02-pet.jpg")).pixels, "pixels untouched")
    }

    func testRemoveBackgroundMultipliesLayerAlphaInOneUndoStep() async throws {
        let wc = try openPhoto("09-two-objects")
        let original = [UInt8](try wc.editor.document.queryData("layer.pixels"))
        let steps = try historyCount(wc.editor)
        // Capture the mask Vision gives so the expected alpha is computed here.
        nonisolated(unsafe) var seen: [UInt8]?
        ForegroundMasker.compute = { job, px, w, h in
            let m = try ForegroundMasker.vision(job, px, w, h)
            seen = m
            return m
        }
        guard case .applied = try await run(wc, .removeBackground) else { return XCTFail("not applied") }
        let mask = try XCTUnwrap(seen)
        XCTAssertEqual(try historyCount(wc.editor), steps + 1)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Remove Background")
        XCTAssertNil(wc.editor.selection, "no selection is made")
        let after = [UInt8](try wc.editor.document.queryData("layer.pixels"))
        var expected = original
        for i in 0..<mask.count {
            let a = UInt8((Double(original[i * 4 + 3]) * Double(mask[i]) / 255).rounded())
            if a == 0 {
                expected[i * 4..<i * 4 + 4] = [0, 0, 0, 0]
            } else {
                expected[i * 4 + 3] = a
            }
        }
        XCTAssertEqual(after, expected, "alpha times the mask, colors kept")
        XCTAssertTrue(after.enumerated().contains { $0.offset % 4 == 3 && (1...254).contains($0.element) },
                      "soft edges stay soft")
        let status2 = await wc.editor.runAndWait("edit.undo")
        XCTAssertEqual(status2, "ok")
        XCTAssertEqual([UInt8](try wc.editor.document.queryData("layer.pixels")), original, "one undo restores")
    }

    func testRemoveBackgroundUsesTheActiveLayerOnly() async throws {
        let wc = try openPhoto("03-product")
        let bottom = [UInt8](try wc.editor.document.queryData("layer.pixels"))
        let status3 = await wc.editor.runAndWait("layer.add", ["name": "Top"])
        XCTAssertEqual(status3, "ok")
        let status4 = await wc.editor.runAndWait("layer.set_active", ["index": 1])
        XCTAssertEqual(status4, "ok")
        // An empty layer has no foreground: nothing is sent.
        let state = try await run(wc, .removeBackground)
        XCTAssertEqual(state, .noForeground)
        XCTAssertEqual(wc.editor.message, "Remove Background: no foreground found")
        XCTAssertEqual([UInt8](try wc.editor.document.queryData("layer.pixels", ["index": 0])), bottom)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Add New Layer")
    }

    // MARK: Cancel

    func testCancelWhileVisionRunsLeavesTheHashUnchanged() async throws {
        let wc = try openPhoto("01-person")
        let before = try hash(wc.editor)
        // Hold the run open until the test has pressed Cancel.
        let gate = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var computed = false
        ForegroundMasker.compute = { job, px, w, h in
            gate.wait()
            let m = try ForegroundMasker.vision(job, px, w, h)
            computed = true
            return m
        }
        let c = try XCTUnwrap(wc.startRemoveBackground(.removeBackground))
        XCTAssertNotNil(wc.window?.attachedSheet, "the progress sheet shows")
        XCTAssertFalse(c.progress.isHidden)
        XCTAssertFalse(wc.validateMenuItem(NSMenuItem(title: "Remove Background",
                                                      action: #selector(RemoveBackgroundActions.removeBackground(_:)),
                                                      keyEquivalent: "")), "no second run while the sheet is up")
        c.cancelButton.performClick(nil)
        XCTAssertEqual(c.state, .cancelled)
        XCTAssertNil(wc.window?.attachedSheet)
        gate.signal()
        // The background work ends on its own (Vision sees the cancel).
        let ended2 = await eventually(30) { computed || c.job.isCancelled }
        XCTAssertTrue(ended2)
        _ = await eventually(0.5) { false }
        XCTAssertEqual(try hash(wc.editor), before, "Cancel leaves the document unchanged")
        XCTAssertEqual(c.state, .cancelled)
    }

    func testCancelRightAwayWithRealVisionLeavesTheHashUnchanged() async throws {
        let wc = try openPhoto("05-hair")
        let before = try hash(wc.editor)
        let c = try XCTUnwrap(wc.startRemoveBackground(.selectForeground))
        c.cancel(nil)
        XCTAssertEqual(c.state, .cancelled)
        // Let the Vision run finish or stop; its late result must change nothing.
        _ = await eventually(3) { false }
        XCTAssertEqual(try hash(wc.editor), before)
        XCTAssertNil(wc.editor.selection)
    }

    // MARK: Other ends

    func testNoForegroundSendsNothing() async throws {
        let wc = try openPhoto("08-sign")
        let before = try hash(wc.editor)
        ForegroundMasker.compute = { _, _, _, _ in nil }
        let state6 = try await run(wc, .selectForeground)
        XCTAssertEqual(state6, .noForeground)
        XCTAssertEqual(wc.editor.message, "Select Foreground: no foreground found")
        XCTAssertEqual(try hash(wc.editor), before)
    }

    func testAnImageChangedWhileVisionRanIsLeftAlone() async throws {
        let wc = try openPhoto("06-glass")
        let gate = DispatchSemaphore(value: 0)
        let size = 480 * 640
        ForegroundMasker.compute = { _, _, _, _ in
            gate.wait()
            return [UInt8](repeating: 255, count: size)
        }
        let c = try XCTUnwrap(wc.startRemoveBackground(.removeBackground))
        // Another path changes the document meanwhile.
        let status5 = await wc.editor.runAndWait("selection.select_all")
        XCTAssertEqual(status5, "ok")
        let before = try hash(wc.editor)
        gate.signal()
        let ended3 = await eventually(10) { c.state != .running }
        XCTAssertTrue(ended3)
        XCTAssertEqual(c.state, .stale)
        XCTAssertEqual(try hash(wc.editor), before)
    }

    // MARK: Menu

    func testLayersMenuHasBothItems() throws {
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let layers = try XCTUnwrap(menu.item(withTitle: "Layers")?.submenu)
        let sub = try XCTUnwrap(layers.item(withTitle: "Remove Background")?.submenu)
        XCTAssertEqual(sub.items.map(\.title), ["Select Foreground", "Remove Background"])
        XCTAssertEqual(sub.items.map(\.action), [#selector(RemoveBackgroundActions.selectForeground(_:)),
                                                 #selector(RemoveBackgroundActions.removeBackground(_:))])
        let wc = try openPhoto("02-pet")
        for item in sub.items { XCTAssertTrue(wc.validateMenuItem(item), item.title) }
    }

    // MARK: Visual check

    /// Checks the sheet's layout. With TEST_RUNNER_OP_RENDER_DIR set, also
    /// writes the sheet in light and dark, and the Remove Background result of
    /// 02-pet over a checkerboard (the Metal canvas cannot be captured here).
    func testRenderedSheetAndResult() async throws {
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        let wc = try openPhoto("02-pet")
        let gate = DispatchSemaphore(value: 0)
        ForegroundMasker.compute = { job, px, w, h in
            gate.wait()
            return try ForegroundMasker.vision(job, px, w, h)
        }
        let c = try XCTUnwrap(wc.startRemoveBackground(.removeBackground))
        let view = try XCTUnwrap(c.window?.contentView)
        XCTAssertEqual(c.window?.title, "Remove Background")
        XCTAssertEqual(view.bounds.size, CGSize(width: 360, height: 116))
        XCTAssertEqual(c.cancelButton.frame, NSRect(x: 264, y: 72, width: 80, height: 28))
        XCTAssertEqual(c.progress.frame, NSRect(x: 16, y: 46, width: 328, height: 12))
        XCTAssertTrue(c.progress.isIndeterminate)
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            view.appearance = NSAppearance(named: a)
            view.layoutSubtreeIfNeeded()
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            if let out {
                try XCTUnwrap(rep.representation(using: .png, properties: [:]))
                    .write(to: URL(fileURLWithPath: out).appendingPathComponent("m53-sheet-\(n).png"))
            }
        }
        gate.signal()
        let ended4 = await eventually(60) { c.state != .running }
        XCTAssertTrue(ended4)
        guard case .applied = c.state else { return XCTFail("\(c.state)") }
        let committed = await eventually(60) { wc.editor.info.historyLabels.last == "Remove Background" }
        XCTAssertTrue(committed)
        guard let out else { return }
        let px = [UInt8](try wc.editor.document.queryData("layer.pixels"))
        let (w, h) = (480, 640)
        var rgb = [UInt8](repeating: 255, count: w * h * 4)
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                let check: Double = ((x / 16 + y / 16) % 2 == 0) ? 255 : 204
                let a = Double(px[i + 3]) / 255
                for k in 0..<3 { rgb[i + k] = UInt8((Double(px[i + k]) * a + check * (1 - a)).rounded()) }
            }
        }
        let rep = try XCTUnwrap(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: w * 4, bitsPerPixel: 32))
        _ = rgb.withUnsafeBytes { memcpy(rep.bitmapData!, $0.baseAddress!, w * h * 4) }
        try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            .write(to: URL(fileURLWithPath: out).appendingPathComponent("m53-result-02-pet.png"))
    }
}

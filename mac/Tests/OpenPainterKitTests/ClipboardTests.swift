// M34: the Edit menu's clipboard. Copy then Paste round-trips the bytes
// exactly (partial-coverage edges included); Copy Merged equals the flattened
// image; Cut is one undo step; the ICC profile is kept on Copy and converted
// on Paste from another profile (op-color numbers pinned); Paste opens a Move
// Selected Pixels session that Escape undoes exactly and Finish commits in one
// step; each oversized-paste choice; Paste into New Image; and the items turn
// on only when they can act. Every test uses a private named pasteboard,
// never the general one.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class ClipboardTests: XCTestCase {
    var fx: ShellFixture!
    var board: NSPasteboard!
    var savedBoard: NSPasteboard!
    var savedPrompt: ImageClipboard.OversizePrompt!
    var savedWorkspace: (() -> Workspace?)!
    var newDocs: [ImageDocument] = []

    override func setUp() async throws {
        fx = try ShellFixture()
        board = NSPasteboard(name: NSPasteboard.Name("OpenPainterTests.\(UUID().uuidString)"))
        savedBoard = ImageClipboard.pasteboard
        ImageClipboard.pasteboard = board
        savedPrompt = ImageClipboard.oversizePrompt
        ImageClipboard.oversizePrompt = { _, _, _, _ in XCTFail("unexpected oversize prompt") }
        savedWorkspace = ImageDocument.workspace
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
    }

    override func tearDown() async throws {
        for d in newDocs {
            for wc in d.windowControllers { wc.window?.orderOut(nil) }
            d.close()
        }
        newDocs.removeAll()
        ImageClipboard.pasteboard = savedBoard
        ImageClipboard.oversizePrompt = savedPrompt
        ImageDocument.workspace = savedWorkspace
        board.releaseGlobally()
        fx.tearDown()
    }

    // MARK: Helpers

    func labels(_ wc: EditorWindowController) throws -> [String] {
        try wc.editor.document.info()["history_labels"] as? [String] ?? []
    }

    /// Selects the polygon `points` ("x,y,x,y,...") with Paste Selection's JSON.
    func select(_ wc: EditorWindowController, _ points: String) async {
        let status = await wc.editor.runAndWait(
            "selection.paste", ["polygons": "{\"polygonList\":[\"\(points)\"]}", "combine": "replace"])
        XCTAssertEqual(status, "ok")
    }

    /// Test pixels with every alpha level in use.
    func pattern(_ w: Int, _ h: Int) -> [UInt8] {
        (0..<w * h).flatMap { i -> [UInt8] in
            [UInt8(i * 7 % 256), UInt8(i * 13 % 256), UInt8(i * 29 % 256), UInt8(255 - i * 3 % 200)]
        }
    }

    /// The engine's own sRGB profile bytes (what a document with no profile copies with).
    func srgbICC() throws -> Data {
        let d = try fx.engine.importPixels(width: 1, height: 1, rgba: [0, 0, 0, 0])
        defer { try? d.close() }
        return try d.queryData("clipboard.profile_icc")
    }

    /// Paste and wait for the session; fails the test when none opened.
    func paste(_ wc: EditorWindowController, newLayer: Bool) {
        var opened: Bool?
        wc.pastePixels(newLayer: newLayer) { opened = $0 }
        XCTAssertEqual(opened, true, "a paste session opened")
        XCTAssertTrue(wc.moveTool.active)
        XCTAssertEqual(fx.workspace.tools.tool, .moveSelectedPixels, "paint.net turns on Move Selected Pixels")
    }

    /// ICC bytes without the header's creation date and profile ID (the
    /// engine makes its sRGB profile anew each time).
    func withoutDate(_ icc: Data) -> Data {
        var d = icc
        for i in Array(24..<36) + Array(84..<100) where i < d.count { d[d.startIndex + i] = 0 }
        return d
    }

    func item(_ action: Selector) -> NSMenuItem { NSMenuItem(title: "", action: action, keyEquivalent: "") }

    // MARK: Copy

    func testCopyThenPasteRoundTripsBytesWithPartialEdges() async throws {
        let wc = try fx.open()
        // Half-covered columns at x 10 and 49, and rows at y 20 and 59.
        await select(wc, "10.5,20.5,49.5,20.5,49.5,59.5,10.5,59.5")
        let expected = try wc.editor.document.copyPixels(merged: false)
        XCTAssertEqual([expected.x, expected.y, expected.width, expected.height], [10, 20, 40, 40])
        XCTAssertTrue(expected.rgba.enumerated().contains { $0.offset % 4 == 3 && $0.element > 0 && $0.element < 128 },
                      "the edges carry partial alpha")
        XCTAssertTrue(wc.copyPixels(merged: false))
        // PNG and TIFF are on the pasteboard; the PNG is exact and carries the profile.
        let types = board.types ?? []
        XCTAssertTrue(types.contains(.png) && types.contains(.tiff), "\(types)")
        let png = try PNGDecoder.decode(try XCTUnwrap(board.data(forType: .png)))
        XCTAssertEqual(png.rgba, expected.rgba)
        XCTAssertEqual(try png.icc.map(withoutDate), try withoutDate(srgbICC()),
                       "a document with no profile copies as the engine's sRGB")
        // Copy keeps the selection and adds no history step.
        XCTAssertEqual(try labels(wc), ["Open", "Paste Selection"])
        XCTAssertNotNil(wc.editor.selection)

        await wc.editor.runAndWait("selection.deselect")
        paste(wc, newLayer: true)
        XCTAssertTrue(wc.moveTool.finish())
        XCTAssertEqual(try labels(wc), ["Open", "Paste Selection", "Deselect", "Paste into New Layer"])
        XCTAssertEqual(wc.editor.info.layers.count, 2)
        XCTAssertEqual(wc.editor.info.activeLayer, 1)
        // The image fits the window, so it lands at the canvas's top left.
        XCTAssertEqual(wc.editor.selection, CGRect(x: 0, y: 0, width: 40, height: 40))
        let back = try wc.editor.document.copyPixels(merged: false)
        XCTAssertEqual([back.x, back.y, back.width, back.height], [0, 0, 40, 40])
        XCTAssertEqual(back.rgba, expected.rgba, "the pasted layer holds the copied bytes exactly")
    }

    func testCopyMergedEqualsTheFlattenedImage() async throws {
        let wc = try fx.open()
        wc.addLayer()
        _ = waitUntil { wc.editor.info.layers.count == 2 }
        await wc.editor.runAndWait("layer.set_active", ["index": 1])
        await select(wc, "30,30,200,30,200,150,30,150")
        await wc.editor.runAndWait("selection.fill", ["color": "2080ff90"])
        await select(wc, "20.5,25.5,260.5,25.5,260.5,170.5,20.5,170.5")
        XCTAssertTrue(wc.copyPixels(merged: true))
        let copied = try PNGDecoder.decode(try XCTUnwrap(board.data(forType: .png)))
        let flat = try wc.editor.document.pixels()
        let mask = [UInt8](try wc.editor.document.queryData("doc.selection_mask"))
        let w = wc.editor.info.width
        XCTAssertEqual([copied.width, copied.height], [241, 146])
        var differ = 0
        for row in 0..<copied.height {
            for col in 0..<copied.width {
                let (x, y) = (20 + col, 25 + row)
                let i = (y * w + x) * 4, j = (row * copied.width + col) * 4
                if mask[y * w + x] == 255, row > 0, col > 0, row < copied.height - 1, col < copied.width - 1 {
                    // Fully covered inside: the composite's bytes.
                    if Array(flat[i..<i + 4]) != Array(copied.rgba[j..<j + 4]) { differ += 1 }
                } else {
                    // Half-covered edge (a quarter at the corners): the
                    // composite's color, alpha scaled by the coverage.
                    let cx = col == 0 || col == copied.width - 1 ? 0.5 : 1.0
                    let cy = row == 0 || row == copied.height - 1 ? 0.5 : 1.0
                    let want = Double(flat[i + 3]) * cx * cy
                    XCTAssertEqual(Array(flat[i..<i + 3]), Array(copied.rgba[j..<j + 3]))
                    XCTAssertLessThanOrEqual(abs(Double(copied.rgba[j + 3]) - want), 2)
                }
            }
        }
        XCTAssertEqual(differ, 0)
        // The plain Copy is the top layer only, which differs.
        XCTAssertTrue(wc.copyPixels(merged: false))
        let top = try PNGDecoder.decode(try XCTUnwrap(board.data(forType: .png)))
        XCTAssertNotEqual(top.rgba, copied.rgba)
    }

    func testCutIsOneUndoStep() async throws {
        let wc = try fx.open()
        await select(wc, "40,40,140,40,140,100,40,100")
        let before = try wc.editor.document.pixels()
        let region = try wc.editor.document.copyPixels(merged: false)
        let ticket = wc.cutPixels()
        XCTAssertNotNil(ticket)
        XCTAssertTrue(waitUntil { (try? self.labels(wc).count) == 3 })
        XCTAssertEqual(try labels(wc), ["Open", "Paste Selection", "Erase Selection"])
        let sel = try wc.editor.document.query("doc.selection") as? [String: Any]
        XCTAssertEqual(sel?["active"] as? Bool, false, "Cut deselects (paint.net)")
        XCTAssertEqual(try PNGDecoder.decode(try XCTUnwrap(board.data(forType: .png))).rgba, region.rgba)
        XCTAssertNotEqual(try wc.editor.document.pixels(), before)
        wc.editor.refresh()
        wc.undo()
        XCTAssertTrue(waitUntil {
            (try? wc.editor.document.info()["history_index"] as? Int) == 1 && (try? self.labels(wc).count) == 3
        })
        XCTAssertEqual(try wc.editor.document.pixels(), before, "one Undo brings the pixels back")
    }

    // MARK: Profiles

    func testProfileKeptOnCopyAndConvertedOnPaste() async throws {
        // A Display P3 document copies with its profile.
        let p3 = try fx.open()
        await p3.editor.runAndWait("image.assign_profile", ["profile": "display-p3"])
        let p3ICC = try p3.editor.document.queryData("doc.profile_icc")
        XCTAssertFalse(p3ICC.isEmpty)
        XCTAssertTrue(p3.copyPixels(merged: false))
        XCTAssertEqual(try PNGDecoder.decode(try XCTUnwrap(board.data(forType: .png))).icc, p3ICC)
        // The TIFF carries it too.
        let tiff = try XCTUnwrap(board.data(forType: .tiff))
        let src = try XCTUnwrap(CGImageSourceCreateWithData(tiff as CFData, nil))
        let tiffImage = try XCTUnwrap(CGImageSourceCreateImageAtIndex(src, 0, nil))
        XCTAssertEqual(tiffImage.colorSpace?.copyICCData() as Data?, p3ICC)

        // sRGB pixels pasted into the P3 document are converted by op-color.
        try ImageClipboard.write(ClipboardPixels(width: 2, height: 1, rgba: [255, 0, 0, 255, 0, 128, 255, 200],
                                                 icc: try srgbICC()))
        paste(p3, newLayer: true)
        XCTAssertEqual(p3.editor.message, "Paste into New Layer: converted from sRGB to Display P3")
        p3.moveTool.finish()
        let got = try p3.editor.document.copyPixels(merged: false)
        // Pinned op-color numbers (perceptual): sRGB red is (234, 51, 35) in Display P3,
        // (0, 128, 255) is (55, 126, 247); engine/op-engine/tests/clipboard.rs pins the same.
        XCTAssertEqual(Array(got.rgba[0..<4]), [234, 51, 35, 255])
        XCTAssertEqual(Array(got.rgba[4..<8]), [55, 126, 247, 200])

        // The same profile is pasted unchanged, with no message.
        try ImageClipboard.write(ClipboardPixels(width: 2, height: 1, rgba: [255, 0, 0, 255, 0, 128, 255, 200],
                                                 icc: p3ICC))
        paste(p3, newLayer: true)
        XCTAssertNil(p3.editor.message)
        p3.moveTool.finish()
        XCTAssertEqual(try p3.editor.document.copyPixels(merged: false).rgba, [255, 0, 0, 255, 0, 128, 255, 200])
    }

    // MARK: Paste session

    func testPasteOpensAMoveSessionAndEscapeRestoresTheState() async throws {
        let wc = try fx.open()
        await select(wc, "5,5,50,5,50,50,5,50")
        wc.selectCrop.reloadSelectionPath()
        let doc = wc.editor.document
        let pixels = try doc.pixels()
        let mask = [UInt8](try doc.queryData("doc.selection_mask"))
        let info = try doc.info()
        try ImageClipboard.write(ClipboardPixels(width: 30, height: 20, rgba: pattern(30, 20)))
        paste(wc, newLayer: false)
        // The preview shows the pasted pixels; nothing is committed yet.
        XCTAssertEqual(try labels(wc), ["Open", "Paste Selection"])
        XCTAssertNotEqual(try doc.pixels(preview: true), pixels)
        XCTAssertEqual(wc.moveTool.state?.base, CGRect(x: 0, y: 0, width: 30, height: 20))
        XCTAssertEqual(wc.selectCrop.overlay.selectionPath?.boundingBox, CGRect(x: 0, y: 0, width: 30, height: 20))
        // A nudge moves it; Escape drops it all.
        XCTAssertTrue(wc.moveTool.arrow(keyCode: 124, modifiers: []))
        wc.canvasKey(.escape)
        XCTAssertFalse(wc.moveTool.active)
        XCTAssertEqual(try doc.pixels(), pixels)
        XCTAssertEqual(try doc.pixels(preview: true), pixels, "no preview is left")
        XCTAssertEqual([UInt8](try doc.queryData("doc.selection_mask")), mask)
        let after = try doc.info()
        for key in ["history_labels", "history_index", "revision", "active_layer"] {
            XCTAssertEqual("\(after[key] ?? "nil")", "\(info[key] ?? "nil")", key)
        }
        XCTAssertEqual((after["layers"] as? [Any])?.count, 1)

        // Paste into New Layer, then Escape: no layer is left behind.
        paste(wc, newLayer: true)
        wc.canvasKey(.escape)
        XCTAssertEqual(try doc.pixels(), pixels)
        XCTAssertEqual((try doc.info()["layers"] as? [Any])?.count, 1)
        XCTAssertEqual(try labels(wc), ["Open", "Paste Selection"])
    }

    func testPasteMovesAndFinishCommitsOneStep() throws {
        let wc = try fx.open()
        let px = pattern(16, 12)
        try ImageClipboard.write(ClipboardPixels(width: 16, height: 12, rgba: px))
        paste(wc, newLayer: true)
        for _ in 0..<7 { XCTAssertTrue(wc.moveTool.arrow(keyCode: 124, modifiers: [])) }
        for _ in 0..<3 { XCTAssertTrue(wc.moveTool.arrow(keyCode: 125, modifiers: [])) }
        wc.canvasKey(.enter)
        XCTAssertFalse(wc.moveTool.active)
        XCTAssertEqual(try labels(wc), ["Open", "Paste into New Layer"])
        XCTAssertEqual(wc.editor.selection, CGRect(x: 7, y: 3, width: 16, height: 12))
        let got = try wc.editor.document.copyPixels(merged: false)
        XCTAssertEqual(got.rgba, px)
        // One Undo removes the paste and its layer.
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.info.historyIndex == 0 })
        XCTAssertEqual(wc.editor.info.layers.count, 1)
    }

    func testPasteLandsAtTheTopLeftOfTheView() async throws {
        let wc = try fx.open()
        var v = wc.canvas.viewport
        v.setZoom(8)
        v.center(on: CGPoint(x: 300, y: 200))
        wc.canvas.setViewport(v)
        let shown = wc.canvas.viewport
        let corner = shown.documentPoint(atView: shown.clearRect?.origin ?? .zero)
        try ImageClipboard.write(ClipboardPixels(width: 4, height: 4, rgba: pattern(4, 4)))
        paste(wc, newLayer: false)
        let box = try XCTUnwrap(wc.moveTool.state?.base)
        XCTAssertEqual(box.origin, CGPoint(x: corner.x.rounded(.up), y: corner.y.rounded(.up)))
        XCTAssertGreaterThan(box.minX, 0)
        wc.moveTool.cancel()
    }

    func testPasteOriginGeometry() {
        let canvas = CGSize(width: 100, height: 80)
        func o(_ w: CGFloat, _ h: CGFloat, _ x: CGFloat, _ y: CGFloat) -> [Int] {
            let r = ClipboardGeometry.pasteOrigin(image: CGSize(width: w, height: h), canvas: canvas,
                                                  visibleTopLeft: CGPoint(x: x, y: y))
            return [r.x, r.y]
        }
        XCTAssertEqual(o(10, 10, -40, -12), [0, 0], "whole canvas visible")
        XCTAssertEqual(o(10, 10, 20.2, 30), [21, 30], "the first whole visible pixel")
        XCTAssertEqual(o(30, 30, 90, 70), [70, 50], "kept inside the canvas")
        XCTAssertEqual(o(120, 30, 50, 10), [0, 10], "wider than the canvas")
    }

    func testOversizedPasteEachChoice() async throws {
        let wc = try fx.open()
        let doc = wc.editor.document
        let pixels = try doc.pixels()
        let big = ClipboardPixels(width: 600, height: 100, rgba: pattern(600, 100))
        try ImageClipboard.write(big)
        var asked: [CGSize] = []
        var answer = OversizePasteChoice.cancel
        ImageClipboard.oversizePrompt = { _, image, canvas, done in
            asked.append(image)
            XCTAssertEqual(canvas, CGSize(width: 512, height: 384))
            done(answer)
        }
        // Cancel: nothing happens.
        var opened: Bool?
        wc.pastePixels(newLayer: false) { opened = $0 }
        XCTAssertEqual(opened, false)
        XCTAssertFalse(wc.moveTool.active)
        XCTAssertEqual(try doc.pixels(), pixels)
        XCTAssertEqual(try labels(wc), ["Open"])

        // Keep canvas size: the paste hangs off the right edge.
        answer = .keepCanvasSize
        opened = nil
        wc.pastePixels(newLayer: true) { opened = $0 }
        XCTAssertEqual(opened, true)
        XCTAssertEqual(wc.moveTool.state?.base, CGRect(x: 0, y: 0, width: 600, height: 100))
        wc.moveTool.finish()
        XCTAssertEqual(try labels(wc), ["Open", "Paste into New Layer"])
        XCTAssertEqual([wc.editor.info.width, wc.editor.info.height], [512, 384])
        let kept = try doc.copyPixels(merged: false)
        XCTAssertEqual([kept.x, kept.y, kept.width, kept.height], [0, 0, 512, 100], "clipped to the canvas")
        XCTAssertEqual(Array(kept.rgba[0..<16]), Array(big.rgba[0..<16]))

        // Expand canvas: a Canvas Size step, then the paste at the top left.
        answer = .expandCanvas
        opened = nil
        wc.pastePixels(newLayer: true) { opened = $0 }
        XCTAssertEqual(opened, true)
        XCTAssertEqual([wc.editor.info.width, wc.editor.info.height], [600, 384])
        wc.moveTool.finish()
        XCTAssertEqual(try labels(wc), ["Open", "Paste into New Layer", "Canvas Size", "Paste into New Layer"])
        let all = try doc.copyPixels(merged: false)
        XCTAssertEqual([all.x, all.y, all.width, all.height], [0, 0, 600, 100])
        XCTAssertEqual(all.rgba, big.rgba)
        XCTAssertEqual(asked, [CGSize(width: 600, height: 100), CGSize(width: 600, height: 100),
                               CGSize(width: 600, height: 100)])
    }

    func testPasteIntoNewImageSizeAndPixels() throws {
        let wc = try fx.open()
        let p3 = CGColorSpace(name: CGColorSpace.displayP3)!.copyICCData()! as Data
        let px = pattern(37, 23)
        try ImageClipboard.write(ClipboardPixels(width: 37, height: 23, rgba: px, icc: p3))
        let doc = try XCTUnwrap(wc.pasteIntoNewImage(controller: NSDocumentController.shared))
        newDocs.append(doc)
        for c in doc.windowControllers { c.window?.isReleasedWhenClosed = false }
        let engineDoc = try XCTUnwrap(doc.engineDocument)
        let info = try engineDoc.info()
        XCTAssertEqual(info["width"] as? Int, 37)
        XCTAssertEqual(info["height"] as? Int, 23)
        XCTAssertEqual(try engineDoc.pixels(), px, "one layer with the clipboard's pixels")
        XCTAssertEqual(try engineDoc.queryData("doc.profile_icc"), p3, "the clipboard's profile")
        XCTAssertEqual(doc.pixelSize, CGSize(width: 37, height: 23))
        XCTAssertEqual(doc.fileType, ImageFormats.opaintIdentifier)
        XCTAssertTrue(doc.displayName.hasPrefix("Untitled"), doc.displayName)
        XCTAssertEqual(try labels(wc), ["Open"], "the source image is unchanged")
    }

    func testTIFFOnlyClipboardPastes() async throws {
        let wc = try fx.open()
        let px = (0..<8 * 5).flatMap { i -> [UInt8] in [UInt8(i * 5), 90, UInt8(255 - i), 255] }
        let tiff = try ImageEncoder.encode(params: ["format": "tiff"], width: 8, height: 5, rgba: px,
                                           icc: try srgbICC(), exif: nil, xmp: nil)
        board.clearContents()
        board.setData(tiff, forType: .tiff)
        XCTAssertTrue(ImageClipboard.hasImage)
        paste(wc, newLayer: true)
        wc.moveTool.finish()
        let got = try wc.editor.document.copyPixels(merged: false)
        XCTAssertEqual([got.width, got.height], [8, 5])
        XCTAssertEqual(got.rgba, px)
    }

    // MARK: Menu items

    func testItemsAreOnOnlyWhenTheyCanAct() async throws {
        let wc = try fx.open()
        let copy = item(#selector(NSText.copy(_:))), merged = item(#selector(ClipboardActions.copyMerged(_:)))
        let cut = item(#selector(NSText.cut(_:))), paste = item(#selector(NSText.paste(_:)))
        let layer = item(#selector(ClipboardActions.pasteIntoNewLayer(_:)))
        let image = item(#selector(ClipboardActions.pasteIntoNewImage(_:)))
        board.clearContents()
        board.setString("plain text", forType: .string)
        // No selection: Copy takes the whole layer; Cut needs a selection.
        XCTAssertTrue(wc.validateMenuItem(copy))
        XCTAssertTrue(wc.validateMenuItem(merged))
        XCTAssertFalse(wc.validateMenuItem(cut))
        for i in [paste, layer, image] { XCTAssertFalse(wc.validateMenuItem(i), "no image on the clipboard") }
        await select(wc, "10,10,60,10,60,60,10,60")
        XCTAssertTrue(wc.validateMenuItem(cut))
        // A selection that covers no pixel: nothing to copy.
        await select(wc, "900,900,901,900,901,901,900,901")
        XCTAssertFalse(wc.validateMenuItem(copy))
        XCTAssertFalse(wc.validateMenuItem(cut))
        XCTAssertFalse(wc.copyPixels(merged: false))
        try ImageClipboard.write(ClipboardPixels(width: 2, height: 2, rgba: pattern(2, 2)))
        for i in [paste, layer, image] { XCTAssertTrue(wc.validateMenuItem(i), "an image on the clipboard") }
        // The menu has the items wired.
        let menu = MenuSkeleton.build(effects: [], panelTarget: nil)
        let edit = try XCTUnwrap(menu.item(withTitle: "Edit")?.submenu)
        for (title, action) in [("Cut", cut.action), ("Copy", copy.action), ("Copy Merged", merged.action),
                                ("Paste", paste.action), ("Paste into New Layer", layer.action),
                                ("Paste into New Image", image.action)] {
            XCTAssertEqual(edit.item(withTitle: title)?.action, action, title)
        }
    }

    // MARK: Visual check

    /// With OP_RENDER_DIR set, writes the oversized-paste sheet and the status
    /// bar's conversion message in light and dark.
    func testRenderedOversizePromptAndStatus() throws {
        guard let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"] else { return }
        let wc = try fx.open()
        wc.editor.report("Paste: converted from sRGB to Display P3")
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            var shown: NSAlert?
            ImageClipboard.showOversizeAlert(nil, image: CGSize(width: 600, height: 100),
                                             canvas: CGSize(width: 512, height: 384), alertSink: { shown = $0 }) { _ in }
            let al = try XCTUnwrap(shown)
            al.window.appearance = NSAppearance(named: a)
            al.layout()
            let view = try XCTUnwrap(al.window.contentView)
            view.layoutSubtreeIfNeeded()
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            // The sheet's own background, which cacheDisplay leaves out.
            let back = try XCTUnwrap(NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: rep.pixelsWide, pixelsHigh: rep.pixelsHigh, bitsPerSample: 8,
                samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0,
                bitsPerPixel: 0))
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: back)
            NSAppearance(named: a)!.performAsCurrentDrawingAppearance {
                NSColor.windowBackgroundColor.setFill()
                NSRect(x: 0, y: 0, width: rep.pixelsWide, height: rep.pixelsHigh).fill()
            }
            rep.draw(in: NSRect(x: 0, y: 0, width: rep.pixelsWide, height: rep.pixelsHigh), from: .zero,
                     operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            NSGraphicsContext.restoreGraphicsState()
            try XCTUnwrap(back.representation(using: .png, properties: [:]))
                .write(to: URL(fileURLWithPath: out).appendingPathComponent("m34-oversize-\(n).png"))
            let bar = wc.statusBar
            bar.appearance = NSAppearance(named: a)
            bar.layoutSubtreeIfNeeded()
            let r2 = try XCTUnwrap(bar.bitmapImageRepForCachingDisplay(in: bar.bounds))
            bar.cacheDisplay(in: bar.bounds, to: r2)
            try XCTUnwrap(r2.representation(using: .png, properties: [:]))
                .write(to: URL(fileURLWithPath: out).appendingPathComponent("m34-status-\(n).png"))
        }
    }
}

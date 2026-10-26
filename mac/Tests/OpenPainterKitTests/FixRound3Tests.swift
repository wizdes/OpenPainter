// Release review fix round 3, the Mac side. Item 1: Close and Quit ask when
// the wait for queued work times out. Item 2: one unsaved-changes review per
// image at a time. Item 3: Revert refreshes the save protection. Item 4: a
// failed Revert keeps the tools. Item 5: plain Save never writes a different
// format or drops frames or CMYK over the original, and Recover uses the
// same rule. Item 6: the layer and pixel limits reach the status bar. Item 8:
// the shipped library rejects the test panic hook. Item 9: a second Save and
// Revert while a Save waits behind slow work do not hang. Item 10: a real
// HEIF is recovered without being written over; a real PSD does not open,
// and its record never writes over it. Item 11: the
// live Magic Wand's Save writes the right file and keeps the selection.
import AppKit
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class FixRound3Tests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var root: URL!
    var docs: [ImageDocument] = []
    var unsavedAnswers: [UnsavedChoice] = []
    var unsavedAsked = 0
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedUnsaved: ImageDocument.UnsavedPrompt!
    var savedBoard: NSPasteboard!
    var board: NSPasteboard!

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        root = dir.appendingPathComponent("Recovery", isDirectory: true)
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedUnsaved = ImageDocument.unsavedPrompt
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        ImageDocument.unsavedPrompt = { [unowned self] _, done in
            unsavedAsked += 1
            done(unsavedAnswers.isEmpty ? .cancel : unsavedAnswers.removeFirst())
        }
        RecoveryCenter.root = root
        RecoveryCenter.timing = RecoveryTiming(settle: .milliseconds(50), maxInterval: .seconds(5))
        savedBoard = ImageClipboard.pasteboard
        board = NSPasteboard(name: NSPasteboard.Name("OpenPainterFixRound3.\(UUID().uuidString)"))
        ImageClipboard.pasteboard = board
    }

    override func tearDown() async throws {
        for d in docs {
            for wc in d.windowControllers {
                if let w = wc.window, let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
                wc.window?.orderOut(nil)
            }
            d.close()
        }
        docs.removeAll()
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        ImageDocument.unsavedPrompt = savedUnsaved
        ImageDocument.settleTimeout = .seconds(600)
        ImageClipboard.pasteboard = savedBoard
        board.releaseGlobally()
        RecoveryCenter.root = nil
        RecoveryCenter.timing = RecoveryTiming()
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
    }

    // MARK: Helpers

    func pngCopy(_ name: String = "plasma.png") throws -> URL {
        let url = dir.appendingPathComponent(name)
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        return url
    }

    /// Opens `url` as the app does (NSDocument's read), with the type its name implies.
    @discardableResult
    func open(_ url: URL, type: String? = nil) throws -> ImageDocument {
        let t = type ?? UTType(filenameExtension: url.pathExtension)?.identifier ?? "public.data"
        let doc = try ImageDocument(contentsOf: url, ofType: t)
        show(doc)
        return doc
    }

    func show(_ doc: ImageDocument) {
        docs.append(doc)
        if doc.windowControllers.isEmpty { doc.makeWindowControllers() }
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
    }

    func wc(_ doc: ImageDocument) throws -> EditorWindowController {
        try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
    }

    func eventually(_ seconds: Double = 10, _ condition: @escaping () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    func pixels(_ url: URL) throws -> [UInt8] {
        let d = try fx.engine.openPNG(path: url.path)
        defer { try? d.close() }
        return try d.pixels()
    }

    /// About 1.2 s of engine work that ends where it began (a slow invert,
    /// then Undo): the image reads as clean before and after.
    func queueSlowNoOp(_ doc: ImageDocument, delayMS: Int = 300) throws {
        let e = try XCTUnwrap(doc.engineDocument)
        _ = try e.dispatch("test.slow_invert", ["delay_ms": delayMS])
        _ = try e.dispatch("edit.undo")
    }

    func invert(_ doc: ImageDocument) async throws {
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
    }

    /// File > Save shows the save panel (Save As) and writes nothing; the
    /// panel is cancelled.
    func assertSaveAsksWhere(_ doc: ImageDocument, _ original: URL, _ why: String,
                             file: StaticString = #filePath, line: UInt = #line) throws {
        let bytes = try Data(contentsOf: original)
        let w = try XCTUnwrap(try wc(doc).window)
        doc.save(nil)
        XCTAssertTrue(waitUntil { w.attachedSheet is NSSavePanel }, "\(why): Save asks where (Save As)",
                      file: file, line: line)
        if let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
        XCTAssertTrue(waitUntil { w.attachedSheet == nil }, file: file, line: line)
        XCTAssertEqual(try Data(contentsOf: original), bytes, "\(why): the original is byte-identical",
                       file: file, line: line)
    }

    func crash(_ doc: ImageDocument) async throws {
        let engineDoc = try XCTUnwrap(doc.engineDocument)
        let written = await eventually {
            ((try? engineDoc.recoveryStatus())?["acknowledged_revision"] as? NSNumber) != nil
        }
        XCTAssertTrue(written, "a record was written")
        try engineDoc.flushRecovery()
        try engineDoc.detachRecovery(deleteRecord: false)
        for wc in doc.windowControllers { wc.window?.orderOut(nil) }
        doc.close()
        docs.removeAll { $0 === doc }
    }

    func recoverOnly() async throws -> ImageDocument {
        let records = RecoveryCenter.pendingRecords()
        XCTAssertEqual(records.count, 1)
        let doc = try await ImageDocument.recover(try XCTUnwrap(records.first), controller: NSDocumentController.shared)
        show(doc)
        return doc
    }

    func closeRecovered(_ doc: ImageDocument) async {
        for wc in doc.windowControllers { wc.window?.orderOut(nil) }
        doc.close()
        docs.removeAll { $0 === doc }
        _ = await eventually { RecoveryCenter.pendingRecords().isEmpty }
    }

    /// A small sRGB test image.
    func cgImage(width: Int = 16, height: Int = 8) throws -> CGImage {
        var px = [UInt8](repeating: 0, count: width * height * 4)
        for i in 0..<(width * height) {
            px[i * 4] = UInt8(truncatingIfNeeded: i * 7)
            px[i * 4 + 1] = 120
            px[i * 4 + 2] = UInt8(truncatingIfNeeded: 255 - i)
            px[i * 4 + 3] = 255
        }
        return try XCTUnwrap(CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: CGDataProvider(data: Data(px) as CFData)!, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent))
    }

    /// Writes `frames` copies of the test image as `type` with ImageIO
    /// (CMYK when `cmyk`).
    func writeImageIO(_ type: String, _ name: String, frames: Int = 1, cmyk: Bool = false) throws -> URL {
        let url = dir.appendingPathComponent(name)
        var image = try cgImage()
        if cmyk {
            let ctx = try XCTUnwrap(CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                                              bytesPerRow: 0, space: CGColorSpaceCreateDeviceCMYK(),
                                              bitmapInfo: CGImageAlphaInfo.none.rawValue))
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            image = try XCTUnwrap(ctx.makeImage())
        }
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, type as CFString, frames, nil))
        for _ in 0..<frames { CGImageDestinationAddImage(dest, image, nil) }
        XCTAssertTrue(CGImageDestinationFinalize(dest), "ImageIO wrote \(name)")
        return url
    }

    /// A real HEIF: ImageIO writes HEIC here (it lists no HEIF writer), and
    /// the file's major brand becomes the generic HEIF brand `mif1`, which
    /// ImageIO then reads as public.heif.
    func writeHEIF(_ name: String) throws -> URL {
        let url = try writeImageIO("public.heic", name)
        var data = try Data(contentsOf: url)
        XCTAssertEqual(String(data: data.subdata(in: 4..<12), encoding: .ascii), "ftypheic")
        data.replaceSubrange(8..<12, with: Data("mif1".utf8))
        try data.write(to: url)
        return url
    }

    /// A 4 x 2 16-bit RGBA PNG.
    func sixteenBitPNG(_ url: URL) throws {
        let (w, h) = (4, 2)
        var samples = [UInt16](repeating: 0, count: w * h * 4)
        for i in 0..<samples.count { samples[i] = UInt16(truncatingIfNeeded: i * 8191) | 0x00FF }
        for i in stride(from: 3, to: samples.count, by: 4) { samples[i] = 0xFFFF }
        let data = samples.withUnsafeBufferPointer { Data(buffer: $0) }
        let image = try XCTUnwrap(CGImage(
            width: w, height: h, bitsPerComponent: 16, bitsPerPixel: 64, bytesPerRow: w * 8,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue | CGBitmapInfo.byteOrder16Little.rawValue),
            provider: CGDataProvider(data: data as CFData)!, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent))
        let tmp = dir.appendingPathComponent("deep-\(UUID().uuidString).png")
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(tmp as CFURL, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        _ = try FileManager.default.replaceItemAt(url, withItemAt: tmp)
    }

    func editor(newDocument width: UInt32, _ height: UInt32) throws -> EditorWindowController {
        let doc = try fx.engine.newDocument(width: width, height: height, dpi: 72, background: [0, 0, 0, 0])
        let editor = Editor(document: doc, pixelSize: CGSize(width: Int(width), height: Int(height)), name: "limit")
        fx.workspace.add(editor)
        let wc = EditorWindowController(editor: editor, workspace: fx.workspace)
        wc.window?.isReleasedWhenClosed = false
        fx.controllers.append(wc)
        wc.present()
        return wc
    }

    // MARK: Item 1: a wait that times out

    func testCloseAsksWhenTheWaitForQueuedWorkTimesOut() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        XCTAssertFalse(doc.isDocumentEdited)
        ImageDocument.settleTimeout = .milliseconds(100)
        try queueSlowNoOp(doc)
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks: the image's state is not known")
        XCTAssertFalse(doc.windowControllers.isEmpty, "Cancel keeps the window")
        XCTAssertNotNil(doc.engineDocument, "the image was not closed")
    }

    func testQuitAsksWhenTheWaitForQueuedWorkTimesOut() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        ImageDocument.settleTimeout = .milliseconds(100)
        try queueSlowNoOp(doc)
        let recorder = ReviewRecorder()
        unsavedAnswers = [.cancel]
        ImageDocumentController.reviewUnsaved([doc], controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 1 })
        XCTAssertEqual(unsavedAsked, 1, "Quit asks: the image's state is not known")
        XCTAssertEqual(recorder.answers, [false], "Cancel stops the quit")
        XCTAssertNotNil(doc.engineDocument)
    }

    // MARK: Item 2: one review at a time

    func testClosePressedTwiceWhileTheWaitRunsAsksOnce() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        try await invert(doc)
        try queueSlowNoOp(doc)
        unsavedAnswers = [.dontSave]
        let w = try XCTUnwrap(try wc(doc).window)
        w.performClose(nil)
        w.performClose(nil)
        XCTAssertTrue(waitUntil { doc.engineDocument == nil }, "the image closed")
        RunLoop.main.run(until: Date().addingTimeInterval(0.5))
        XCTAssertEqual(unsavedAsked, 1, "one review for two Close requests")
    }

    func testCloseThenQuitWhileTheWaitRunsAsksOnceAndQuitFinishes() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        try await invert(doc)
        try queueSlowNoOp(doc)
        unsavedAnswers = [.dontSave]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        let recorder = ReviewRecorder()
        ImageDocumentController.reviewUnsaved([doc], controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 1 }, "Quit finished")
        XCTAssertEqual(recorder.answers, [true], "Don't Save lets Quit go on")
        XCTAssertTrue(waitUntil { doc.engineDocument == nil }, "the image closed")
        RunLoop.main.run(until: Date().addingTimeInterval(0.5))
        XCTAssertEqual(unsavedAsked, 1, "Close and Quit share one review")
        XCTAssertEqual(recorder.answers.count, 1)
    }

    func testCloseThenQuitWithCancelKeepsTheImageAndStopsQuit() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        try await invert(doc)
        try queueSlowNoOp(doc)
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        let recorder = ReviewRecorder()
        ImageDocumentController.reviewUnsaved([doc], controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 1 }, "Quit finished")
        XCTAssertEqual(recorder.answers, [false])
        XCTAssertEqual(unsavedAsked, 1)
        XCTAssertNotNil(doc.engineDocument, "Cancel keeps the image")
        // A later Close asks again.
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 2 }, "a new review after the first ended")
    }

    // MARK: Item 3: Revert refreshes the save protection

    func testRevertToA16BitFileMakesSaveAskWhere() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        XCTAssertTrue(doc.plainSaveAllowed)
        try sixteenBitPNG(url)
        XCTAssertTrue(ImageFormats.holdsMoreThan8Bits(url))
        let deep = try Data(contentsOf: url)
        try doc.revert(toContentsOf: url, ofType: "public.png")
        XCTAssertTrue(doc.saveNeedsNewFile, "the reverted file is 16-bit")
        XCTAssertFalse(doc.plainSaveAllowed)
        try await invert(doc)
        try assertSaveAsksWhere(doc, url, "after Revert to a 16-bit file")
        XCTAssertEqual(try Data(contentsOf: url), deep, "the 16-bit file is unchanged")
    }

    func testRevertToAnUnwritableTypeUpdatesTheTypeAndTheRecoveryRecord() async throws {
        let url = try pngCopy("photo.png")
        let doc = try open(url, type: "public.png")
        XCTAssertEqual(doc.contentType, "public.png")
        let heif = try writeHEIF("source.heif")
        _ = try FileManager.default.replaceItemAt(url, withItemAt: heif)
        // ImageIO names this content HEIC or HEIF by the file's name; both
        // are not PNG.
        guard case .imageIO(let type) = ImageFormats.route(for: url) else {
            return XCTFail("the HEIF opens through ImageIO")
        }
        XCTAssertTrue(["public.heif", "public.heic"].contains(type), type)
        try doc.revert(toContentsOf: url, ofType: "public.png")
        XCTAssertEqual(doc.contentType, type)
        XCTAssertFalse(doc.plainSaveAllowed)
        try await invert(doc)
        try assertSaveAsksWhere(doc, url, "HEIF named .png after Revert")
        try await crash(doc)
        let record = try XCTUnwrap(RecoveryCenter.pendingRecords().first)
        XCTAssertEqual(record.originalType, type, "the record names the reverted file's type")
        let back = try await recoverOnly()
        XCTAssertFalse(back.plainSaveAllowed)
        await closeRecovered(back)
    }

    func testRevertToAPlain8BitFileLetsSaveWriteInPlaceAgain() async throws {
        let url = dir.appendingPathComponent("deep.png")
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        try sixteenBitPNG(url)
        let doc = try open(url, type: "public.png")
        XCTAssertFalse(doc.plainSaveAllowed)
        _ = try FileManager.default.replaceItemAt(url, withItemAt: try pngCopy("eight.png"))
        try doc.revert(toContentsOf: url, ofType: "public.png")
        XCTAssertTrue(doc.plainSaveAllowed, "the reverted file is a plain 8-bit PNG")
    }

    // MARK: Item 4: a failed Revert keeps the tools

    func testAFailedRevertKeepsTheOpenTextBox() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        fx.workspace.tools.tool = .text
        fx.workspace.tools.settings.text.size = 24
        c.click(CGPoint(x: 40, y: 60))
        XCTAssertTrue(c.textTool.isEditing)
        c.textTool.input.insertText("Hi", replacementRange: NSRange(location: NSNotFound, length: 0))
        let typed = await eventually { (try? doc.engineDocument?.info()["session_open"] as? Bool) == true }
        XCTAssertTrue(typed)
        // The file is gone: Revert fails.
        try FileManager.default.removeItem(at: url)
        XCTAssertThrowsError(try doc.revert(toContentsOf: url, ofType: "public.png"))
        XCTAssertTrue(c.textTool.isEditing, "the text box is still open")
        XCTAssertEqual(try XCTUnwrap(doc.engineDocument).info()["session_open"] as? Bool, true)
        c.textTool.input.insertText("!", replacementRange: NSRange(location: NSNotFound, length: 0))
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        XCTAssertEqual(c.textTool.model.text, "Hi!")
        // The next click ends the box and starts another, with no "busy".
        c.click(CGPoint(x: 300, y: 200))
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        let message = c.editor.message ?? ""
        XCTAssertFalse(message.lowercased().contains("busy"), message)
        XCTAssertFalse(message.contains("failed"), message)
        XCTAssertTrue(waitUntil { c.editor.refresh(); return c.editor.info.historyLabels.last == "Text" },
                      "the first box was kept as a step: \(c.editor.info.historyLabels.suffix(3)) \(c.editor.message ?? "")")
    }

    // MARK: Item 5: plain Save never changes the format or drops data

    /// Opens `url` (named type), checks the note and that Save asks where,
    /// then crashes and recovers it and checks the same.
    func checkKeepsOriginal(_ url: URL, note: String, _ why: String,
                            file: StaticString = #filePath, line: UInt = #line) async throws {
        let doc = try open(url)
        XCTAssertTrue(doc.saveNeedsNewFile, "\(why): needs Save As", file: file, line: line)
        XCTAssertFalse(doc.plainSaveAllowed, why, file: file, line: line)
        let message = try wc(doc).editor.message ?? ""
        XCTAssertTrue(message.contains(note), "\(why): the note says why: \(message)", file: file, line: line)
        try await invert(doc)
        try assertSaveAsksWhere(doc, url, why, file: file, line: line)
        try await crash(doc)
        let back = try await recoverOnly()
        XCTAssertFalse(back.plainSaveAllowed, "\(why): recovered", file: file, line: line)
        try assertSaveAsksWhere(back, url, "\(why), recovered", file: file, line: line)
        await closeRecovered(back)
    }

    func testAPNGNamedJPGIsNotRewrittenAsJPEG() async throws {
        let url = try pngCopy("photo.jpg")
        XCTAssertEqual(ImageFormats.route(for: url), .engine)
        try await checkKeepsOriginal(url, note: "PNG", "a PNG named .jpg")
    }

    func testAnAnimatedGIFIsNotFlattenedOverItself() async throws {
        let url = try writeImageIO("com.compuserve.gif", "anim.gif", frames: 3)
        try await checkKeepsOriginal(url, note: "3 frames", "a 3-frame GIF")
    }

    func testAMultiPageTIFFIsNotFlattenedOverItself() async throws {
        let url = try writeImageIO("public.tiff", "pages.tiff", frames: 2)
        try await checkKeepsOriginal(url, note: "2 frames", "a 2-page TIFF")
    }

    func testACMYKJPEGIsNotRewrittenAsRGB() async throws {
        let url = try writeImageIO("public.jpeg", "print.jpg", cmyk: true)
        try await checkKeepsOriginal(url, note: "CMYK", "a CMYK JPEG")
    }

    func testAnRGBJPEGStillSavesInPlace() throws {
        let url = try writeImageIO("public.jpeg", "plain.jpg")
        let doc = try open(url)
        XCTAssertFalse(doc.saveNeedsNewFile)
        XCTAssertTrue(doc.plainSaveAllowed)
        let one = try writeImageIO("com.compuserve.gif", "still.gif")
        XCTAssertTrue(try open(one).plainSaveAllowed, "a one-frame GIF")
    }

    // MARK: Item 6: limit messages reach the user

    /// Editor.run reports from a main-actor task, so this waits with
    /// `await` (a spun run loop would keep that task from running).
    func assertLimitMessage(_ wc: EditorWindowController, _ text: String, _ why: String,
                            file: StaticString = #filePath, line: UInt = #line) async {
        let shown = await eventually { (wc.editor.message ?? "").contains(text) }
        let message = wc.editor.message ?? ""
        XCTAssertTrue(shown, "\(why): the status bar shows the limit: \(message)", file: file, line: line)
        for bad in ["internal", "bad_argument", "badArgument", "failed: limit"] {
            XCTAssertFalse(message.contains(bad), "\(why): \(message)", file: file, line: line)
        }
        wc.editor.clearMessage()
    }

    func pasteNewLayerItem() -> NSMenuItem {
        NSMenuItem(title: "Paste into New Layer", action: #selector(EditorWindowController.pasteIntoNewLayer(_:)),
                   keyEquivalent: "")
    }

    func testThe1000LayerLimitReachesTheStatusBar() async throws {
        let wc = try editor(newDocument: 4, 4)
        for _ in 1..<1000 {
            XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.add"), "ok")
        }
        wc.editor.refresh()
        let text = "at most 1000 layers"
        wc.editor.run("layer.add", ["name": "x"])
        await assertLimitMessage(wc, text, "Add New Layer")
        wc.editor.run("layer.duplicate")
        await assertLimitMessage(wc, text, "Duplicate Layer")
        wc.editor.run("layer.import_from_file", ["path": fixtures.appendingPathComponent("plasma_alpha_512x384.png").path])
        await assertLimitMessage(wc, text, "Import from File")
        // Paste into New Layer is off, and a paste that gets through says why.
        try ImageClipboard.write(ClipboardPixels(width: 2, height: 1, rgba: [255, 0, 0, 255, 0, 255, 0, 255]))
        XCTAssertTrue(ImageClipboard.hasImage)
        XCTAssertEqual(wc.validateClipboardItem(pasteNewLayerItem()), false, "Paste into New Layer is off")
        var opened: Bool?
        wc.pastePixels(newLayer: true) { opened = $0 }
        XCTAssertTrue(waitUntil { opened != nil })
        XCTAssertEqual(opened, false)
        await assertLimitMessage(wc, text, "Paste into New Layer")
        XCTAssertEqual(wc.editor.info.layers.count, 1000)
        // One layer fewer: on again.
        XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.delete"), "ok")
        wc.editor.refresh()
        XCTAssertEqual(wc.validateClipboardItem(pasteNewLayerItem()), true)
    }

    func testThe16GiBTotalReachesTheStatusBar() async throws {
        // 1 GiB per layer: 15 layers and the selection mask's slot fill 16 GiB.
        let wc = try editor(newDocument: 16384, 16384)
        for _ in 1..<15 {
            XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.add"), "ok")
        }
        wc.editor.refresh()
        let text = "more than 16 GiB"
        wc.editor.run("layer.add", ["name": "x"])
        await assertLimitMessage(wc, text, "Add New Layer")
        wc.editor.run("layer.duplicate")
        await assertLimitMessage(wc, text, "Duplicate Layer")
        try ImageClipboard.write(ClipboardPixels(width: 2, height: 1, rgba: [255, 0, 0, 255, 0, 255, 0, 255]))
        XCTAssertEqual(wc.validateClipboardItem(pasteNewLayerItem()), false, "Paste into New Layer is off")
        var opened: Bool?
        wc.pastePixels(newLayer: true) { opened = $0 }
        XCTAssertTrue(waitUntil { opened != nil })
        XCTAssertEqual(opened, false)
        await assertLimitMessage(wc, text, "Paste into New Layer")
    }

    func testCanvasSizeAndResizePastThe16GiBTotalReachTheStatusBar() async throws {
        // 10000 x 9000 x 43 layers: growing to 10000 x 10000 passes 16 GiB.
        let wc = try editor(newDocument: 10000, 9000)
        for _ in 1..<43 {
            XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.add"), "ok")
        }
        wc.editor.refresh()
        let text = "more than 16 GiB"
        wc.editor.run("image.canvas_size", ["width": 10000, "height": 10000, "anchor": "center"])
        await assertLimitMessage(wc, text, "Canvas Size")
        wc.editor.run("image.resize", ["width": 10000, "height": 10000, "maintain_aspect": false])
        await assertLimitMessage(wc, text, "Resize")
    }

    // MARK: Item 8: the test hook does not ship

    func testTheShippedLibraryRejectsTheTestPanicHook() throws {
        // The tests link the app's own library (engine/target/app-release).
        let doc = try fx.engine.newDocument(width: 4, height: 4, dpi: 72, background: [0, 0, 0, 0])
        defer { try? doc.close() }
        XCTAssertThrowsError(try doc.dispatch("test.panic_on_finish")) { error in
            XCTAssertEqual(error as? EngineError, .badArgument, "an unknown command")
        }
        // The image is not read-only and saves as usual.
        let url = dir.appendingPathComponent("after-hook.opaint")
        try doc.save(to: url.path, format: .opaint, markSaved: true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
    }

    // MARK: Item 9: no hang while a Save waits

    /// The Save is done when the slow step is in the file and the image is clean.
    func savedWithSlowStep(_ doc: ImageDocument, _ url: URL, _ e: Document, step: Bool = true) async -> Bool {
        await eventually(30) {
            doc.editor?.refresh()
            guard !step || doc.editor?.info.historyLabels.last == "test.slow_invert", !doc.isDocumentEdited,
                  let file = try? self.pixels(url), let shown = try? e.pixels() else { return false }
            return file == shown
        }
    }

    func testASecondSaveWhileASaveWaitsBehindSlowWorkDoesNotHang() async throws {
        let url = try pngCopy()
        let doc = try ErrorRecordingDocument(contentsOf: url, ofType: "public.png")
        show(doc)
        try await invert(doc)
        let e = try XCTUnwrap(doc.engineDocument)
        _ = try e.dispatch("test.slow_invert", ["delay_ms": 300])  // about 1.2 s on the engine
        doc.save(nil)
        let start = Date()
        doc.save(nil)
        let call = Date().timeIntervalSince(start)
        print("SECOND_SAVE_CALL \(call)")
        let saved = await savedWithSlowStep(doc, url, e)
        XCTAssertTrue(saved, "both saves finished and the file holds the slow step")
        RunLoop.main.run(until: Date().addingTimeInterval(0.5))
        XCTAssertEqual(doc.presented, [], "no save failed")
        XCTAssertNil(try wc(doc).window?.attachedSheet)
    }

    func testRevertWhileASaveWaitsBehindSlowWorkDoesNotHang() async throws {
        let url = try pngCopy()
        let doc = try ErrorRecordingDocument(contentsOf: url, ofType: "public.png")
        show(doc)
        try await invert(doc)
        let e = try XCTUnwrap(doc.engineDocument)
        _ = try e.dispatch("test.slow_invert", ["delay_ms": 300])
        doc.save(nil)
        let w = try XCTUnwrap(try wc(doc).window)
        doc.revertToSaved(nil)
        // AppKit asks before a Revert of an edited image; once the Save
        // finished there is nothing to revert and it may not ask.
        let asked = waitUntil(5) { w.attachedSheet != nil }
        if let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .alertFirstButtonReturn) }
        print("REVERT_ASKED \(asked)")
        // A Revert starts the history again at the file.
        let saved = await savedWithSlowStep(doc, url, e, step: !asked)
        XCTAssertTrue(saved, "the Save finished and the image is the saved file")
        RunLoop.main.run(until: Date().addingTimeInterval(0.5))
        XCTAssertNil(w.attachedSheet)
        XCTAssertEqual(doc.presented, [], "no save or Revert failed")
    }

    // MARK: Item 10: real PSD and HEIF files

    func checkRealFileRecovery(_ url: URL, type: String, _ why: String,
                               file: StaticString = #filePath, line: UInt = #line) async throws {
        XCTAssertEqual(ImageFormats.route(for: url), .imageIO(type), why, file: file, line: line)
        let original = try Data(contentsOf: url)
        let doc = try open(url)
        XCTAssertEqual(doc.contentType, type, why, file: file, line: line)
        XCTAssertFalse(doc.plainSaveAllowed, why, file: file, line: line)
        try await invert(doc)
        try assertSaveAsksWhere(doc, url, why, file: file, line: line)
        try await crash(doc)
        XCTAssertEqual(RecoveryCenter.pendingRecords().first?.originalType, type, why, file: file, line: line)
        let back = try await recoverOnly()
        XCTAssertEqual(back.fileURL?.standardizedFileURL, url.standardizedFileURL, why, file: file, line: line)
        XCTAssertTrue(back.isDocumentEdited, why, file: file, line: line)
        XCTAssertFalse(back.plainSaveAllowed, "\(why): recovered", file: file, line: line)
        try assertSaveAsksWhere(back, url, "\(why), recovered", file: file, line: line)
        // Close's Save asks where too.
        unsavedAnswers = [.save]
        let w = try XCTUnwrap(try wc(back).window)
        w.performClose(nil)
        XCTAssertTrue(waitUntil { w.attachedSheet is NSSavePanel }, "\(why): Close's Save asks where",
                      file: file, line: line)
        if let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
        XCTAssertTrue(waitUntil { w.attachedSheet == nil }, file: file, line: line)
        XCTAssertEqual(try Data(contentsOf: url), original, "\(why): byte-identical", file: file, line: line)
        await closeRecovered(back)
    }

    /// OpenPainter does not open PSD files (ImageIO writes one here, but
    /// the open route refuses it), so a PSD's record can only come from an
    /// older build or another path. A record whose original is a real PSD
    /// file still never writes over it.
    func testARealPSDIsRefusedOnOpenAndItsRecordNeverWritesOverIt() async throws {
        let url = try writeImageIO("com.adobe.photoshop-image", "art.psd")
        let original = try Data(contentsOf: url)
        XCTAssertEqual(ImageFormats.route(for: url), .unsupported)
        XCTAssertThrowsError(try ImageDocument(contentsOf: url, ofType: "com.adobe.photoshop-image"))
        let d = try fx.engine.importPixels(width: 16, height: 8, rgba: [UInt8](repeating: 90, count: 16 * 8 * 4))
        try d.attachRecovery(dir: root.appendingPathComponent(UUID().uuidString), originalURL: url,
                             originalType: "com.adobe.photoshop-image", timing: RecoveryCenter.timing)
        _ = LayersHistoryTests.runSync(d, "image.invert_colors")
        try d.flushRecovery()
        try d.detachRecovery(deleteRecord: false)
        try d.close()
        let back = try await recoverOnly()
        XCTAssertEqual(back.fileURL?.standardizedFileURL, url.standardizedFileURL)
        XCTAssertEqual(back.fileType, "com.adobe.photoshop-image")
        XCTAssertFalse(back.plainSaveAllowed)
        try assertSaveAsksWhere(back, url, "PSD, recovered")
        unsavedAnswers = [.save]
        let w = try XCTUnwrap(try wc(back).window)
        w.performClose(nil)
        XCTAssertTrue(waitUntil { w.attachedSheet is NSSavePanel }, "PSD: Close's Save asks where")
        if let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
        XCTAssertTrue(waitUntil { w.attachedSheet == nil })
        XCTAssertEqual(try Data(contentsOf: url), original, "PSD: byte-identical")
        await closeRecovered(back)
    }

    func testARealHEIFIsRecoveredWithoutWritingOverIt() async throws {
        let url = try writeHEIF("photo.heif")
        try await checkRealFileRecovery(url, type: "public.heif", "HEIF")
    }

    // MARK: Item 11: the live Magic Wand's Save

    func testSaveWithALiveMagicWandWritesTheFileAndKeepsTheSelection() async throws {
        let url = try pngCopy()
        let original = try pixels(url)
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        try await invert(doc)
        fx.workspace.tools.tool = .magicWand
        c.flood.click(at: CGPoint(x: 10, y: 10), secondary: false)
        let idle = await eventually { c.flood.isIdle }
        XCTAssertTrue(idle)
        let e = try XCTUnwrap(doc.engineDocument)
        let liveMask = try e.queryData("doc.selection_mask", ["preview": true])
        XCTAssertTrue(liveMask.contains { $0 != 0 }, "the wand selected something")
        XCTAssertFalse(liveMask.allSatisfy { $0 != 0 }, "the wand did not select everything")
        let shown = try e.pixels()
        doc.save(nil)
        let saved = await eventually(20) { !doc.isDocumentEdited }
        XCTAssertTrue(saved)
        XCTAssertEqual(c.editor.info.historyLabels.suffix(2), ["Invert Colors", "Magic Wand"])
        XCTAssertEqual(try pixels(url), shown, "the file holds the inverted image")
        XCTAssertNotEqual(try pixels(url), original)
        XCTAssertEqual(try e.queryData("doc.selection_mask", ["preview": false]), liveMask,
                       "the saved step keeps the wand's selection")
        XCTAssertTrue(waitUntil { c.editor.selection != nil }, "the selection shows")
        XCTAssertFalse(doc.isDocumentEdited)
    }
}

/// Records the errors AppKit would show in an app-modal alert (which would
/// wait for a click forever in a test).
final class ErrorRecordingDocument: ImageDocument {
    var presented: [String] = []

    override func presentError(_ error: any Error) -> Bool {
        let e = error as NSError
        presented.append("\(e.domain) \(e.code): \(e.localizedDescription) \(e.localizedFailureReason ?? "")")
        return false
    }
}

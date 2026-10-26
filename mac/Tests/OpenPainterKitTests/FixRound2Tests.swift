// Release review fix round 2, the Mac side. Item 1: a 16-bit PNG says it was
// converted, and Save asks where instead of writing 8 bits over it. Item 2:
// Close and Quit decide after the engine ran what was queued. Item 3:
// Recover uses the type detected from the file's content. Item 5: Save
// finishes the open sessions without blocking the main thread. Item 6:
// Revert ends an empty text box. Item 8: Add New Layer and Duplicate Layer
// turn off at the limits. Item 10: a stroke does not read the full document
// info per event. Item 13: the lifecycle of every session kind, Quit and
// Revert with an open session, and saves through a symbolic link. Item 14:
// tests never start the updater.
import AppKit
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class FixRound2Tests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var root: URL!
    var docs: [ImageDocument] = []
    var unsavedAnswers: [UnsavedChoice] = []
    var unsavedAsked = 0
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedUnsaved: ImageDocument.UnsavedPrompt!

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

    func assertMark(_ doc: ImageDocument, _ on: Bool, _ why: String,
                    file: StaticString = #filePath, line: UInt = #line) async throws {
        let c = try wc(doc)
        let shown = await eventually {
            c.strip.reload()
            let strip = c.strip.items.first { $0.editor === doc.editor }?.nameLabel.stringValue ?? ""
            return doc.isDocumentEdited == on && c.window?.isDocumentEdited == on && strip.hasSuffix(" *") == on
        }
        XCTAssertTrue(shown, "edited mark \(on): \(why)", file: file, line: line)
    }

    func pixels(_ url: URL) throws -> [UInt8] {
        let d = try fx.engine.openPNG(path: url.path)
        defer { try? d.close() }
        return try d.pixels()
    }

    /// Queues about 1.2 s of engine work that ends where it began: a slow
    /// invert, then Undo. The image reads as clean all the while, so only a
    /// check made after the queue ran sees what is queued behind it.
    func queueSlowNoOp(_ doc: ImageDocument, delayMS: Int = 300) throws {
        let e = try XCTUnwrap(doc.engineDocument)
        _ = try e.dispatch("test.slow_invert", ["delay_ms": delayMS])
        _ = try e.dispatch("edit.undo")
    }

    /// Starts text at (40, 60) and types `text`, with no wait for the engine.
    func typeNoWait(_ text: String, into doc: ImageDocument) throws {
        let c = try wc(doc)
        fx.workspace.tools.tool = .text
        fx.workspace.tools.settings.text.size = 24
        c.click(CGPoint(x: 40, y: 60))
        XCTAssertTrue(c.textTool.isEditing)
        c.textTool.input.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    /// A 4 x 2 16-bit RGBA PNG.
    func sixteenBitPNG(_ name: String = "deep.png") throws -> URL {
        let url = dir.appendingPathComponent(name)
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
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        return url
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
        XCTAssertTrue(waitUntil { w.attachedSheet == nil })
        XCTAssertEqual(try Data(contentsOf: original), bytes, "\(why): the original is untouched", file: file, line: line)
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

    // MARK: Item 1: 16-bit PNG

    func testA16BitPNGSaysItWasConvertedAndSaveAsksWhere() async throws {
        let url = try sixteenBitPNG()
        XCTAssertTrue(ImageFormats.holdsMoreThan8Bits(url))
        let doc = try open(url, type: "public.png")
        XCTAssertEqual(try wc(doc).editor.message,
                       "Converted from 16-bit to 8 bits per channel (OpenPainter stores 8-bit).")
        XCTAssertTrue(doc.saveNeedsNewFile)
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
        try assertSaveAsksWhere(doc, url, "16-bit PNG")
        // Close's Save asks where too.
        unsavedAnswers = [.save]
        let w = try XCTUnwrap(try wc(doc).window)
        w.performClose(nil)
        XCTAssertTrue(waitUntil { w.attachedSheet is NSSavePanel }, "Close's Save asks where")
        if let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
        XCTAssertTrue(waitUntil { w.attachedSheet == nil })
        XCTAssertTrue(ImageFormats.holdsMoreThan8Bits(url), "the 16-bit file was not written over")
        // After Save As to a new file, Save writes that file plainly.
        let copy = dir.appendingPathComponent("eight.png")
        var result: (any Error)?? = .none
        doc.save(to: copy, ofType: "public.png", for: .saveAsOperation) { result = .some($0) }
        XCTAssertTrue(waitUntil(20) { result != nil })
        XCTAssertNil(result ?? nil)
        XCTAssertFalse(doc.saveNeedsNewFile)
        XCTAssertTrue(doc.plainSaveAllowed)
    }

    func testAn8BitPNGSavesInPlace() throws {
        let doc = try open(try pngCopy(), type: "public.png")
        XCTAssertFalse(doc.saveNeedsNewFile)
        XCTAssertNil(try wc(doc).editor.message)
        XCTAssertFalse(ImageFormats.holdsMoreThan8Bits(try XCTUnwrap(doc.fileURL)))
        XCTAssertTrue(ImageFormats.holdsMoreThan8Bits(fixtures.appendingPathComponent("io/tiff_rgb16.tif")))
    }

    // MARK: Item 2: Close and Quit after queued work

    func testATextUpdateSentJustBeforeCloseMakesCloseAsk() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        try queueSlowNoOp(doc)
        try typeNoWait("Hi", into: doc)
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks about the queued text")
        XCTAssertFalse(doc.windowControllers.isEmpty, "Cancel keeps the window")
        XCTAssertNotNil(doc.engineDocument)
    }

    func testAFillSentJustBeforeCloseMakesCloseAsk() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        try queueSlowNoOp(doc)
        fx.workspace.tools.tool = .paintBucket
        try wc(doc).flood.click(at: CGPoint(x: 10, y: 10), secondary: false)
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks about the queued fill")
        XCTAssertFalse(doc.windowControllers.isEmpty)
    }

    func testATextUpdateSentJustBeforeQuitMakesQuitAsk() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        try queueSlowNoOp(doc)
        try typeNoWait("Hi", into: doc)
        let recorder = ReviewRecorder()
        unsavedAnswers = [.cancel]
        ImageDocumentController.reviewUnsaved([doc], controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 1 })
        XCTAssertEqual(unsavedAsked, 1, "Quit asks about the queued text")
        XCTAssertEqual(recorder.answers, [false], "Cancel stops the quit")
    }

    // MARK: Item 5: Save does not block the main thread

    func testSaveFinishesTheOpenTextWithoutBlockingTheMainThread() async throws {
        let url = try pngCopy()
        let original = try pixels(url)
        let doc = try open(url, type: "public.png")
        let e = try XCTUnwrap(doc.engineDocument)
        _ = try e.dispatch("test.slow_invert", ["delay_ms": 300])  // about 1.2 s on the engine
        try typeNoWait("Hi", into: doc)
        let running = FuzzBox<Bool>()
        running.value = true
        var longest: Duration = .zero
        let beat = Task { @MainActor in
            var last = ContinuousClock.now
            while running.value == true {
                try? await Task.sleep(for: .milliseconds(5))
                let now = ContinuousClock.now
                longest = max(longest, now - last)
                last = now
            }
        }
        try? await Task.sleep(for: .milliseconds(50))
        let start = ContinuousClock.now
        doc.save(nil)
        let call = ContinuousClock.now - start
        let saved = await eventually(20) { !doc.isDocumentEdited && (try? self.pixels(url)) != original }
        running.value = false
        _ = await beat.value
        print("SAVE_MAIN_GAP \(longest) SAVE_CALL \(call)")
        XCTAssertTrue(saved, "the save finished")
        XCTAssertLessThan(call, .milliseconds(100), "Save returned at once")
        XCTAssertLessThan(longest, .milliseconds(250), "the main thread was blocked for \(longest)")
        XCTAssertEqual(try wc(doc).editor.info.historyLabels.suffix(2), ["test.slow_invert", "Text"])
        XCTAssertEqual(try pixels(url), try e.pixels(), "the file holds what the screen shows")
    }

    // MARK: Item 6: Revert and an empty text box

    func testRevertEndsAnEmptyTextBox() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        let status = await c.editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
        fx.workspace.tools.tool = .text
        c.click(CGPoint(x: 40, y: 60))
        XCTAssertTrue(c.textTool.isEditing)
        XCTAssertTrue(c.textTool.model.text.isEmpty)
        try doc.revert(toContentsOf: url, ofType: "public.png")
        XCTAssertFalse(c.textTool.isEditing, "the text box ended with Revert")
        c.textTool.input.insertText("x", replacementRange: NSRange(location: NSNotFound, length: 0))
        let ticket = c.textTool.lastTicket
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        XCTAssertEqual(c.textTool.lastTicket, ticket, "typing sends nothing to a session that is gone")
        XCTAssertFalse((c.editor.message ?? "").contains("failed"), c.editor.message ?? "")
        let info = try XCTUnwrap(doc.engineDocument).info()
        XCTAssertEqual(info["session_open"] as? Bool, false)
        try await assertMark(doc, false, "after Revert")
    }

    // MARK: Item 8: layer limits

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

    func assertLayerAddOff(_ wc: EditorWindowController, _ why: String,
                           file: StaticString = #filePath, line: UInt = #line) {
        wc.editor.refresh()
        let add = NSMenuItem(title: "Add New Layer", action: #selector(EditorWindowController.addLayer(_:)), keyEquivalent: "")
        XCTAssertFalse(wc.validateMenuItem(add), "\(why): Layers > Add New Layer", file: file, line: line)
        let dup = NSMenuItem(title: "Duplicate Layer", action: #selector(EditorWindowController.runCommand(_:)), keyEquivalent: "")
        dup.representedObject = "layer.duplicate"
        XCTAssertFalse(wc.validateMenuItem(dup), "\(why): Layers > Duplicate Layer", file: file, line: line)
        fx.workspace.setActive(wc.editor)
        fx.workspace.layersView.reload()
        XCTAssertFalse(fx.workspace.layersView.buttons["layer.add"]!.isEnabled, "\(why): panel Add", file: file, line: line)
        XCTAssertFalse(fx.workspace.layersView.buttons["layer.duplicate"]!.isEnabled, "\(why): panel Duplicate",
                       file: file, line: line)
        // The toolbar's More menu holds no layer commands at all.
        let titles = wc.moreMenu().items.map(\.title)
        XCTAssertFalse(titles.contains("Add New Layer"), file: file, line: line)
        XCTAssertFalse(titles.contains("Duplicate Layer"), file: file, line: line)
    }

    func testAddAndDuplicateLayerAreOffAt1000Layers() async throws {
        let wc = try editor(newDocument: 4, 4)
        for _ in 1..<1000 {
            XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.add"), "ok")
        }
        assertLayerAddOff(wc, "1,000 layers")
        XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.delete"), "ok")
        wc.editor.refresh()
        let add = NSMenuItem(title: "Add New Layer", action: #selector(EditorWindowController.addLayer(_:)), keyEquivalent: "")
        XCTAssertTrue(wc.validateMenuItem(add), "on again under the limit")
    }

    func testAddAndDuplicateLayerAreOffAtThePixelTotal() async throws {
        // 1 GiB per layer: 15 layers and the selection mask's slot fill 16 GiB.
        let wc = try editor(newDocument: 16384, 16384)
        for _ in 1..<15 {
            XCTAssertEqual(LayersHistoryTests.runSync(wc.editor.document, "layer.add"), "ok")
        }
        assertLayerAddOff(wc, "16 GiB")
        let ticket = try wc.editor.document.dispatch("layer.add")
        let event = await wc.editor.document.engine.result(of: ticket)
        XCTAssertTrue((event?.json["error"] as? String ?? "").contains("16 GiB"), "\(event?.json ?? [:])")
    }

    // MARK: Item 10: toolbar refresh cost

    func testAStrokeOf200EventsDoesNotReadTheFullDocumentInfoPerEvent() throws {
        let doc = try open(try pngCopy(), type: "public.png")
        let c = try wc(doc)
        let w = try XCTUnwrap(c.window)
        var v = c.canvas.viewport
        v.actualSize()
        c.canvas.setViewport(v)
        fx.workspace.tools.tool = .paintbrush
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        func mouse(_ type: NSEvent.EventType, _ x: Double, _ y: Double) -> NSEvent {
            let p = c.canvas.convert(c.canvas.viewport.viewPoint(ofDocument: CGPoint(x: x, y: y)), to: nil)
            return NSEvent.mouseEvent(
                with: type, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        }
        let before = Document.infoReads
        let refreshes = c.commandBarRefreshes
        // AppKit updates the window after each event it handles (the
        // toolbar validation hook the command bar used).
        TabletLog.deliver(mouse(.leftMouseDown, 20, 20), to: c.canvas)
        w.update()
        for i in 0..<198 {
            TabletLog.deliver(mouse(.leftMouseDragged, 20 + Double(i), 20 + Double(i % 50)), to: c.canvas)
            w.update()
        }
        TabletLog.deliver(mouse(.leftMouseUp, 220, 60), to: c.canvas)
        w.update()
        let during = Document.infoReads - before
        let bar = c.commandBarRefreshes - refreshes
        print("STROKE_INFO_READS \(during) BAR_REFRESHES \(bar)")
        XCTAssertLessThan(bar, 20, "the command bar refreshed \(bar) times during a 200-event stroke")
        XCTAssertLessThan(during, 20, "the full info was read \(during) times during a 200-event stroke")
        XCTAssertTrue(waitUntil { c.editor.info.historyLabels.last == "Paintbrush" }, "the stroke committed")
    }

    // MARK: Item 3: Recover uses the detected type

    /// A record of an image opened from `original`, whose content was `type`.
    func typedRecord(_ name: String, type: String?) throws -> URL {
        let original = dir.appendingPathComponent(name)
        try Data("content that is not what the name says".utf8).write(to: original)
        let d = try fx.engine.importPixels(width: 8, height: 8, rgba: [UInt8](repeating: 90, count: 256))
        try d.attachRecovery(dir: root.appendingPathComponent(UUID().uuidString), originalURL: original,
                             originalType: type, timing: RecoveryCenter.timing)
        _ = LayersHistoryTests.runSync(d, "image.invert_colors")
        try d.flushRecovery()
        try d.detachRecovery(deleteRecord: false)
        try d.close()
        return original
    }

    func testRecoverUsesTheDetectedTypeNotTheFileName() async throws {
        let cases: [(String, String?, String)] = [
            ("dng-named.png", "com.adobe.raw-image", "Camera RAW named .png"),
            ("photo.heif", "public.heif", "HEIF"),
            ("art.psd", "com.adobe.photoshop-image", "PSD"),
            ("old.png", nil, "an older record with no type"),
        ]
        for (name, type, why) in cases {
            let original = try typedRecord(name, type: type)
            let doc = try await recoverOnly()
            XCTAssertEqual(doc.fileURL?.standardizedFileURL, original.standardizedFileURL, why)
            if let type { XCTAssertEqual(doc.fileType, type, why) }
            XCTAssertFalse(doc.plainSaveAllowed, why)
            try assertSaveAsksWhere(doc, original, why)
            await closeRecovered(doc)
        }
        // A record of a PNG that is a PNG still saves in place.
        let png = try pngCopy("real.png")
        let d = try fx.engine.importPixels(width: 8, height: 8, rgba: [UInt8](repeating: 90, count: 256))
        try d.attachRecovery(dir: root.appendingPathComponent(UUID().uuidString), originalURL: png,
                             originalType: "public.png", timing: RecoveryCenter.timing)
        _ = LayersHistoryTests.runSync(d, "image.invert_colors")
        try d.flushRecovery()
        try d.detachRecovery(deleteRecord: false)
        try d.close()
        let back = try await recoverOnly()
        XCTAssertTrue(back.plainSaveAllowed, "a real PNG")
        await closeRecovered(back)
    }

    func testOpeningKeepsTheContentTypeInTheRecoveryRecord() async throws {
        // A HEIF (or HEIC where this Mac cannot write HEIF) named .png.
        let types = ImageFormats.systemDestinationTypes
        let type = types.contains("public.heif") ? "public.heif" : "public.heic"
        let url = dir.appendingPathComponent("photo.png")
        try ImageIOFixtures.write(type, to: url)
        XCTAssertEqual(ImageFormats.route(for: url), .imageIO(type))
        let doc = try open(url, type: "public.png")
        XCTAssertEqual(doc.contentType, type)
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
        try await crash(doc)
        let record = try XCTUnwrap(RecoveryCenter.pendingRecords().first)
        XCTAssertEqual(record.originalType, type, "the record keeps the content's type, not the name's")
        let back = try await recoverOnly()
        XCTAssertEqual(back.contentType, type)
        if type == "public.heif" {
            XCTAssertFalse(back.plainSaveAllowed, "HEIF is not written back")
            try assertSaveAsksWhere(back, url, "HEIF named .png")
        }
        await closeRecovered(back)
    }

    // MARK: Item 13: lifecycle of each session kind

    /// The open session marks the image, Close asks, and Save writes what
    /// the screen shows as its own step.
    func checkLifecycle(_ doc: ImageDocument, _ url: URL, _ label: String, compareFile: Bool = true,
                        file: StaticString = #filePath, line: UInt = #line) async throws {
        try await assertMark(doc, true, "open \(label)", file: file, line: line)
        unsavedAnswers = [.cancel]
        let asked = unsavedAsked
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == asked + 1 }, "\(label): Close asks", file: file, line: line)
        XCTAssertFalse(doc.windowControllers.isEmpty, file: file, line: line)
        let shown = try XCTUnwrap(doc.engineDocument).pixels(preview: true)
        doc.save(nil)
        try await assertMark(doc, false, "\(label) after Save", file: file, line: line)
        XCTAssertEqual(try wc(doc).editor.info.historyLabels.last, label, file: file, line: line)
        if compareFile { XCTAssertEqual(try pixels(url), shown, "\(label): the file has it", file: file, line: line) }
    }

    func testOpenLineMarksClosesAsksAndSaves() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let status = try await wc(doc).editor.runAndWait(
            "tool.line.begin", ["points": [[3, 4], [200, 150]], "primary": [240, 20, 10, 255], "width": 5.0])
        XCTAssertEqual(status, "ok")
        try await checkLifecycle(doc, url, "Line/Curve")
    }

    func testOpenGradientMarksClosesAsksAndSaves() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let status = try await wc(doc).editor.runAndWait("tool.gradient.begin", [
            "shape": "linear", "mode": "color", "primary": [200, 20, 10, 255], "secondary": [10, 220, 250, 255],
            "antialias": false, "blend": "overwrite", "start": [10.0, 10.0], "end": [300.0, 10.0],
        ])
        XCTAssertEqual(status, "ok")
        try await checkLifecycle(doc, url, "Gradient")
    }

    func testLiveMagicWandMarksClosesAsksAndSaves() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        fx.workspace.tools.tool = .magicWand
        c.flood.click(at: CGPoint(x: 10, y: 10), secondary: false)
        let idle = await eventually { c.flood.isIdle }
        XCTAssertTrue(idle)
        try await checkLifecycle(doc, url, "Magic Wand", compareFile: false)
        XCTAssertTrue(waitUntil { c.editor.selection != nil }, "the selection was kept")
    }

    func testLivePaintBucketMarksClosesAsksAndSaves() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        fx.workspace.tools.tool = .paintBucket
        c.flood.click(at: CGPoint(x: 10, y: 10), secondary: false)
        let idle = await eventually { c.flood.isIdle }
        XCTAssertTrue(idle)
        try await checkLifecycle(doc, url, "Paint Bucket")
    }

    func testOrdinaryMoveMarksClosesAsksAndSaves() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        fx.workspace.tools.tool = .moveSelectedPixels
        let view = { (p: CGPoint) in c.canvas.viewport.viewPoint(ofDocument: p) }
        c.moveTool.pointerDown(document: CGPoint(x: 100, y: 100), view: view(CGPoint(x: 100, y: 100)),
                               modifiers: [], right: false)
        c.moveTool.pointerUp(document: CGPoint(x: 130, y: 120), view: view(CGPoint(x: 130, y: 120)), modifiers: [])
        XCTAssertTrue(c.moveTool.hasChanges)
        try await checkLifecycle(doc, url, "Move Selected Pixels")
        XCTAssertFalse(c.moveTool.active)
    }

    func testQuitWithAnOpenLineAsks() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        let status = try await wc(doc).editor.runAndWait(
            "tool.line.begin", ["points": [[3, 4], [200, 150]], "primary": [240, 20, 10, 255], "width": 5.0])
        XCTAssertEqual(status, "ok")
        let recorder = ReviewRecorder()
        unsavedAnswers = [.cancel]
        ImageDocumentController.reviewUnsaved([doc], controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 1 })
        XCTAssertEqual(unsavedAsked, 1)
        XCTAssertEqual(recorder.answers, [false])
    }

    func testRevertWithAnOpenShapeGoesBackToTheFile() async throws {
        let url = try pngCopy()
        let original = try pixels(url)
        let doc = try open(url, type: "public.png")
        let status = try await wc(doc).editor.runAndWait("tool.shape.begin", [
            "shape": "rectangle", "box": [[10, 10], [100, 80]], "primary": [255, 0, 0, 255],
            "secondary": [0, 0, 255, 255], "mode": "fill",
        ])
        XCTAssertEqual(status, "ok")
        try await assertMark(doc, true, "open shape")
        try doc.revert(toContentsOf: url, ofType: "public.png")
        let e = try XCTUnwrap(doc.engineDocument)
        XCTAssertEqual(try e.pixels(preview: true), original)
        XCTAssertEqual(try e.info()["session_open"] as? Bool, false)
        try await assertMark(doc, false, "after Revert")
    }

    // MARK: Item 13: saving through a symbolic link

    /// `link` points at `target`; a save through the link writes the target,
    /// the link stays a link, and the target keeps its permissions.
    func checkSaveThroughLink(_ targetName: String, linkName: String, type: String,
                              file: StaticString = #filePath, line: UInt = #line) async throws {
        let fm = FileManager.default
        let real = dir.appendingPathComponent("real", isDirectory: true)
        try fm.createDirectory(at: real, withIntermediateDirectories: true)
        let target = real.appendingPathComponent(targetName)
        let png = try pngCopy("source-\(targetName).png")
        let first = try open(png, type: "public.png")
        var made: (any Error)?? = .none
        first.save(to: target, ofType: type, for: .saveToOperation) { made = .some($0) }
        XCTAssertTrue(waitUntil(20) { made != nil }, file: file, line: line)
        XCTAssertNil(made ?? nil, file: file, line: line)
        try fm.setAttributes([.posixPermissions: 0o640], ofItemAtPath: target.path)
        let link = dir.appendingPathComponent(linkName)
        try fm.createSymbolicLink(at: link, withDestinationURL: target)
        let doc = try open(link, type: type)
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok", file: file, line: line)
        let before = try Data(contentsOf: target)
        // Options chosen already, so a lossy format saves with no sheet.
        doc.chooseSaveOptions(SaveOptions(), for: type)
        doc.save(nil)
        try await assertMark(doc, false, "saved through the link", file: file, line: line)
        XCTAssertNil(try wc(doc).window?.attachedSheet, "\(type): no sheet", file: file, line: line)
        // A second save goes through too (no "changed by another app" check trips).
        let once = try Data(contentsOf: target)
        let again = try await wc(doc).editor.runAndWait("image.flip_horizontal")
        XCTAssertEqual(again, "ok", file: file, line: line)
        doc.save(nil)
        try await assertMark(doc, false, "saved through the link again", file: file, line: line)
        XCTAssertNil(try wc(doc).window?.attachedSheet, "\(type): no sheet on the second save", file: file, line: line)
        XCTAssertNotEqual(try Data(contentsOf: target), once, "\(type): the second save wrote", file: file, line: line)
        let attrs = try fm.attributesOfItem(atPath: link.path)
        XCTAssertEqual(attrs[.type] as? FileAttributeType, .typeSymbolicLink, "\(type): the link stays a link",
                       file: file, line: line)
        XCTAssertNotEqual(try Data(contentsOf: target), before, "\(type): the target was written", file: file, line: line)
        let mode = (try fm.attributesOfItem(atPath: target.path)[.posixPermissions] as? NSNumber)?.intValue
        XCTAssertEqual(mode, 0o640, "\(type): the target keeps its permissions", file: file, line: line)
        XCTAssertEqual(doc.fileURL?.lastPathComponent, linkName, file: file, line: line)
    }

    func testSaveThroughASymlinkWritesTheTargetForPNG() async throws {
        try await checkSaveThroughLink("t.png", linkName: "l.png", type: "public.png")
    }

    func testSaveThroughASymlinkWritesTheTargetForOpaint() async throws {
        try await checkSaveThroughLink("t.opaint", linkName: "l.opaint", type: ImageFormats.opaintIdentifier)
    }

    func testSaveThroughASymlinkWritesTheTargetForJPEG() async throws {
        try await checkSaveThroughLink("t.jpg", linkName: "l.jpg", type: "public.jpeg")
    }

    // MARK: Item 14: no updater in tests

    func testTheUpdaterNeverStartsInTestsEvenWithTheReleaseSettings() throws {
        // The test bundle is not hosted in the app: its main bundle has no feed.
        XCTAssertNil(Bundle.main.object(forInfoDictionaryKey: "SUFeedURL"))
        XCTAssertFalse(UpdateSettings.main.isSetUp)
        XCTAssertFalse(UpdateMenuTarget.shared.isRunning)
        // Even with the real feed and key, nothing starts under XCTest.
        let target = UpdateMenuTarget()
        target.settings = UpdateSettings(feedURL: URL(string: UpdateSettings.placeholderFeed),
                                         publicKey: "Q1P9GEU6X00XfYERTrMaaVhfra04UVYC2IuWAK0TJoc=")
        XCTAssertTrue(target.settings.isSetUp)
        XCTAssertTrue(UpdateMenuTarget.runningUnderTests)
        target.startIfSetUp()
        XCTAssertFalse(target.isRunning, "the updater does not start in tests")
        target.present = { _ in XCTFail("no alert") }
        target.checkForUpdates(nil)
        XCTAssertFalse(target.isRunning)
    }
}

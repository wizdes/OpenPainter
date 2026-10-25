// Release review (before v1.0), the Mac side. Item 1: an open text, paste or
// shape counts as an edit (the window dot and strip mark show it), Close asks,
// Save writes it as its own step, and a recovery record holds it. Item 2: a
// recovered Camera RAW image never writes over its original. Item 4: Paste
// into New Image starts edited with recovery on. Item 5: kept metadata goes
// through .opaint and recovery. Item 7: huge images are refused from their
// header. Item 9: an edit that lands during Close's save makes Close ask again.
import AppKit
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class ReleaseReviewTests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var root: URL!
    var docs: [ImageDocument] = []
    var unsavedAnswers: [UnsavedChoice] = []
    var unsavedAsked = 0
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedUnsaved: ImageDocument.UnsavedPrompt!
    var savedReader: ((Document) throws -> UInt64)!
    var board: NSPasteboard!
    var savedBoard: NSPasteboard!

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        root = dir.appendingPathComponent("Recovery", isDirectory: true)
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedUnsaved = ImageDocument.unsavedPrompt
        savedReader = ImageDocument.revisionReader
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        ImageDocument.unsavedPrompt = { [unowned self] _, done in
            unsavedAsked += 1
            done(unsavedAnswers.isEmpty ? .cancel : unsavedAnswers.removeFirst())
        }
        RecoveryCenter.root = root
        RecoveryCenter.timing = RecoveryTiming(settle: .milliseconds(50), maxInterval: .seconds(5))
        board = NSPasteboard(name: NSPasteboard.Name("OpenPainterTests.\(UUID().uuidString)"))
        savedBoard = ImageClipboard.pasteboard
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
        ImageDocument.revisionReader = savedReader
        RecoveryCenter.root = nil
        RecoveryCenter.timing = RecoveryTiming()
        ImageClipboard.pasteboard = savedBoard
        board.releaseGlobally()
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

    /// Waits while letting main-actor work run (engine events arrive through a task).
    func eventually(_ seconds: Double = 10, _ condition: @escaping () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    /// The edited mark in all three places: NSDocument, the window dot and
    /// the image strip ("name *").
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

    /// Types `text` with the Text tool at (40, 60) and waits for the engine.
    func type(_ text: String, into doc: ImageDocument) async throws {
        let c = try wc(doc)
        fx.workspace.tools.tool = .text
        fx.workspace.tools.settings.text.size = 24
        c.click(CGPoint(x: 40, y: 60))
        XCTAssertTrue(c.textTool.isEditing)
        c.textTool.input.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
        let event = try await c.editor.document.run("tool.text.update", [:])
        XCTAssertEqual(event.status, "ok")
        c.editor.refresh()
    }

    func saveAndWait(_ doc: ImageDocument, to url: URL, type: String,
                     _ op: NSDocument.SaveOperationType) -> (any Error)? {
        var result: (any Error)?? = .none
        doc.save(to: url, ofType: type, for: op) { result = .some($0) }
        XCTAssertTrue(waitUntil(20) { result != nil }, "save finished")
        return result ?? nil
    }

    func pixels(_ url: URL) throws -> [UInt8] {
        let d = try fx.engine.openPNG(path: url.path)
        defer { try? d.close() }
        return try d.pixels()
    }

    /// Stops the record writer without deleting the record (a crash) and
    /// closes the image.
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

    // MARK: Item 1: open sessions

    func testTypedTextMarksTheImageAndSaveThenReopenHasTheText() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        try await assertMark(doc, false, "just opened")
        try await type("Hello", into: doc)
        try await assertMark(doc, true, "open text")
        let shown = try XCTUnwrap(doc.engineDocument).pixels(preview: true)
        XCTAssertNotEqual(shown, try pixels(url), "the text shows")
        doc.save(nil)  // File > Save (Cmd-S)
        try await assertMark(doc, false, "after Save")
        XCTAssertEqual(try pixels(url), shown, "the reopened file has the text")
        XCTAssertEqual(try wc(doc).editor.info.historyLabels.last, "Text", "the text is its own step")
        XCTAssertFalse(try wc(doc).textTool.isEditing, "the text box ended")
    }

    func testTypedTextThenCloseAsksToSave() async throws {
        let url = try pngCopy()
        let original = try Data(contentsOf: url)
        let doc = try open(url, type: "public.png")
        try await type("Hi", into: doc)
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks")
        XCTAssertFalse(doc.windowControllers.isEmpty, "Cancel keeps the window")
        let shown = try XCTUnwrap(doc.engineDocument).pixels(preview: true)
        unsavedAnswers = [.save]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { doc.windowControllers.isEmpty }, "Save closes")
        XCTAssertNotEqual(try Data(contentsOf: url), original)
        XCTAssertEqual(try pixels(url), shown, "the saved file has the text")
    }

    func testAnOpenPasteMarksTheImageAndCloseAsks() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let c = try wc(doc)
        let px = (0..<(16 * 12)).flatMap { i -> [UInt8] in [UInt8(i % 256), 40, 200, 255] }
        try ImageClipboard.write(ClipboardPixels(width: 16, height: 12, rgba: px))
        var opened: Bool?
        c.pastePixels(newLayer: false) { opened = $0 }
        XCTAssertEqual(opened, true)
        try await assertMark(doc, true, "open paste")
        unsavedAnswers = [.cancel]
        try XCTUnwrap(c.window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks")
        let shown = try XCTUnwrap(doc.engineDocument).pixels(preview: true)
        doc.save(nil)
        try await assertMark(doc, false, "after Save")
        XCTAssertEqual(try pixels(url), shown, "the file has the paste")
        XCTAssertFalse(c.moveTool.active, "the paste was finished")
    }

    func testAnOpenShapeMarksTheImageAndCloseAsks() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let e = try wc(doc).editor
        let status = await e.runAndWait("tool.shape.begin", [
            "shape": "rectangle", "box": [[10, 10], [100, 80]], "primary": [255, 0, 0, 255],
            "secondary": [0, 0, 255, 255], "mode": "fill",
        ])
        XCTAssertEqual(status, "ok")
        try await assertMark(doc, true, "open shape")
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks")
        let shown = try XCTUnwrap(doc.engineDocument).pixels(preview: true)
        doc.save(nil)
        try await assertMark(doc, false, "after Save")
        XCTAssertEqual(try pixels(url), shown, "the file has the shape")
    }

    func testTheRecoveryRecordHoldsTheOpenText() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        try await type("Saved by recovery", into: doc)
        let shown = try XCTUnwrap(doc.engineDocument).pixels(preview: true)
        try await crash(doc)
        let back = try await recoverOnly()
        XCTAssertEqual(try XCTUnwrap(back.engineDocument).pixels(), shown)
    }

    // MARK: Item 2: RAW originals

    /// A record whose original is a camera file OpenPainter cannot write.
    func rawRecord(_ ext: String) throws -> URL {
        let original = dir.appendingPathComponent("photo.\(ext)")
        try Data("not really a \(ext) file".utf8).write(to: original)
        let d = try fx.engine.importPixels(width: 8, height: 8, rgba: [UInt8](repeating: 90, count: 256))
        try d.attachRecovery(dir: root.appendingPathComponent(UUID().uuidString), originalURL: original,
                             timing: RecoveryCenter.timing)
        _ = LayersHistoryTests.runSync(d, "image.invert_colors")
        try d.flushRecovery()
        try d.detachRecovery(deleteRecord: false)
        try d.close()
        return original
    }

    func testRecoveringARawImageNeverWritesTheOriginal() async throws {
        for ext in ["dng", "cr2"] {
            let original = try rawRecord(ext)
            let bytes = try Data(contentsOf: original)
            let doc = try await recoverOnly()
            XCTAssertEqual(doc.fileURL?.standardizedFileURL, original.standardizedFileURL, ext)
            XCTAssertNotEqual(doc.fileType, ImageFormats.opaintIdentifier, ext)
            XCTAssertFalse(ImageFormats.saveTypes().contains(doc.fileType ?? ""), ext)
            let w = try XCTUnwrap(try wc(doc).window)
            doc.save(nil)  // File > Save
            XCTAssertTrue(waitUntil { w.attachedSheet is NSSavePanel }, "\(ext): Save asks where (Save As)")
            if let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
            XCTAssertEqual(try Data(contentsOf: original), bytes, "\(ext): the original is untouched")
            for wc in doc.windowControllers { wc.window?.orderOut(nil) }
            doc.close()
            docs.removeAll { $0 === doc }
            _ = await eventually { RecoveryCenter.pendingRecords().isEmpty }
        }
    }

    // MARK: Item 4: Paste into New Image

    func testPasteIntoNewImageStartsEditedWithRecovery() async throws {
        try ImageClipboard.write(ClipboardPixels(width: 9, height: 7, rgba: [UInt8](repeating: 128, count: 9 * 7 * 4)))
        let src = try open(try pngCopy(), type: "public.png")
        let doc = try XCTUnwrap(try wc(src).pasteIntoNewImage(controller: NSDocumentController.shared))
        show(doc)
        try await assertMark(doc, true, "nothing holds it yet")
        XCTAssertNotNil(doc.recoveryDir, "recovery is on")
        XCTAssertNil(doc.recoveryStartError)
        unsavedAnswers = [.cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 1 }, "Close asks")
        let written = await eventually {
            ((try? doc.engineDocument?.recoveryStatus())?["acknowledged_revision"] as? NSNumber) != nil
        }
        XCTAssertTrue(written, "a recovery record is written")
    }

    func testNewAndOpenStartRecovery() throws {
        let opened = try open(try pngCopy(), type: "public.png")
        XCTAssertNotNil(opened.recoveryDir, "Open")
        let made = try ImageDocumentController.createNewImage(
            NewImageSettings(secondary: DocColor(r: 255, g: 255, b: 255)), controller: NSDocumentController.shared)
        show(made)
        XCTAssertNotNil(made.recoveryDir, "New")
    }

    // MARK: Item 5: metadata

    /// A JPEG with an EXIF user comment, a TIFF artist and an XMP creator tool.
    func jpegWithMetadata() throws -> URL {
        let url = dir.appendingPathComponent("meta.jpg")
        let (w, h) = (32, 24)
        let px = [UInt8](repeating: 180, count: w * h * 4)
        let image = try XCTUnwrap(CGImage(
            width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: CGDataProvider(data: Data(px) as CFData)!, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent))
        let md = CGImageMetadataCreateMutable()
        XCTAssertTrue(CGImageMetadataSetValueWithPath(md, nil, "xmp:CreatorTool" as CFString, "ReleaseReviewTool" as CFString))
        XCTAssertTrue(CGImageMetadataSetValueWithPath(md, nil, "exif:UserComment" as CFString, "release review" as CFString))
        XCTAssertTrue(CGImageMetadataSetValueWithPath(md, nil, "tiff:Artist" as CFString, "Tester" as CFString))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString, 1, nil))
        CGImageDestinationAddImageAndMetadata(dest, image, md, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        return url
    }

    func assertKept(_ url: URL, _ why: String, file: StaticString = #filePath, line: UInt = #line) throws {
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] ?? [:]
        let exif = props[kCGImagePropertyExifDictionary as String] as? [String: Any]
        XCTAssertEqual(exif?[kCGImagePropertyExifUserComment as String] as? String, "release review",
                       "EXIF: \(why)", file: file, line: line)
        let md = CGImageSourceCopyMetadataAtIndex(source, 0, nil)
        let tool = md.flatMap { CGImageMetadataCopyStringValueWithPath($0, nil, "xmp:CreatorTool" as CFString) }
        XCTAssertEqual(tool as String?, "ReleaseReviewTool", "XMP: \(why)", file: file, line: line)
    }

    func exportJPEG(_ doc: ImageDocument, _ name: String) throws -> URL {
        let out = dir.appendingPathComponent(name)
        try DocumentSaver.write(try XCTUnwrap(doc.engineDocument), to: out, type: "public.jpeg",
                                metadata: doc.metadata.isEmpty ? nil : doc.metadata, options: SaveOptions(),
                                markSaved: false)
        return out
    }

    func testMetadataGoesThroughOpaintAndBackToJPEG() throws {
        let jpeg = try jpegWithMetadata()
        try assertKept(jpeg, "the source")
        let doc = try open(jpeg, type: "public.jpeg")
        XCTAssertFalse(doc.metadata.isEmpty)
        let opaint = dir.appendingPathComponent("meta.opaint")
        XCTAssertNil(saveAndWait(doc, to: opaint, type: ImageFormats.opaintIdentifier, .saveAsOperation))
        let back = try open(opaint, type: ImageFormats.opaintIdentifier)
        XCTAssertFalse(back.metadata.isEmpty, "the .opaint kept the metadata")
        try assertKept(try exportJPEG(back, "out.jpg"), "after .opaint")
    }

    func testMetadataGoesThroughRecovery() async throws {
        let doc = try open(try jpegWithMetadata(), type: "public.jpeg")
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
        try await crash(doc)
        let back = try await recoverOnly()
        XCTAssertFalse(back.metadata.isEmpty, "the record kept the metadata")
        try assertKept(try exportJPEG(back, "recovered.jpg"), "after recovery")
    }

    // MARK: Item 7: size before decode

    func testAHugeJPEGIsRefusedFromItsHeaderBeforeDecoding() throws {
        // A real 16500 x 16500 JPEG: 1.01 GiB as RGBA, past the engine's 1 GiB.
        let url = dir.appendingPathComponent("huge.jpg")
        let side = 16500
        let ctx = try XCTUnwrap(CGContext(data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side,
                                          space: CGColorSpaceCreateDeviceGray(), bitmapInfo: 0))
        ctx.setFillColor(gray: 0.5, alpha: 1)
        ctx.fill(CGRect(x: 0, y: 0, width: side, height: side))
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString, 1, nil))
        CGImageDestinationAddImage(dest, try XCTUnwrap(ctx.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        let start = Date()
        XCTAssertThrowsError(try ImageIOImporter.decode(url: url)) { error in
            XCTAssertEqual(error as? ImageIOImportError, .tooLarge("huge.jpg", side, side))
        }
        XCTAssertLessThan(Date().timeIntervalSince(start), 1.0, "refused without decoding")
    }

    /// The size rule the header check applies (ImageIO itself refuses a
    /// JPEG whose frame header does not match its data, so a crafted header
    /// never reaches a decode): at most 1 GiB of RGBA, no overflow.
    func testTheHeaderSizeRule() throws {
        XCTAssertNoThrow(try ImageIOImporter.checkSize(16384, 16384, name: "edge"))
        XCTAssertThrowsError(try ImageIOImporter.checkSize(16384, 16385, name: "edge")) { error in
            XCTAssertEqual(error as? ImageIOImportError, .tooLarge("edge", 16384, 16385))
        }
        XCTAssertThrowsError(try ImageIOImporter.checkSize(Int.max, 3, name: "overflow"))
    }

    // MARK: Item 6: 16-bit PNG

    func testA16BitPNGOpens() throws {
        let url = dir.appendingPathComponent("deep.png")
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
        let src = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let depth = (CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [String: Any])?[kCGImagePropertyDepth as String]
        XCTAssertEqual((depth as? NSNumber)?.intValue, 16, "the file is 16-bit")
        let doc = try open(url, type: "public.png")
        let px = try XCTUnwrap(doc.engineDocument).pixels()
        XCTAssertEqual(px.count, w * h * 4)
        let round16 = { (v: UInt16) -> UInt8 in UInt8((UInt32(v) * 255 + 32767) / 65535) }
        XCTAssertEqual(px[0], round16(samples[0]))
        XCTAssertEqual(px[5], round16(samples[5]))
        XCTAssertEqual(px[3], 255)
    }

    // MARK: Item 9: an edit during Close's save

    func testAnEditDuringClosesSaveKeepsTheImageEditedAndCloseAsksAgain() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
        final class Flag: @unchecked Sendable { var on = false }
        let edited = Flag()
        // Between the save reading the revision and its completion, an edit lands.
        ImageDocument.revisionReader = { engineDoc in
            let r = try ImageDocument.defaultRevisionReader(engineDoc)
            if !edited.on {
                edited.on = true
                _ = try engineDoc.dispatch("image.flip_horizontal", [:])
                try engineDoc.finishSessions()  // waits for the edit
            }
            return r
        }
        unsavedAnswers = [.save, .cancel]
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked == 2 }, "Close asks again after the late edit")
        XCTAssertTrue(edited.on)
        XCTAssertFalse(doc.windowControllers.isEmpty, "the window stays")
        XCTAssertNotNil(doc.engineDocument)
        try await assertMark(doc, true, "the late edit is not saved")
    }
}

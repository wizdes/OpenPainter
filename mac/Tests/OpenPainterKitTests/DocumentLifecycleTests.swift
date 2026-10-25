// M19 document lifecycle, one test per command: New (each background), Open
// (each format family), Save, Save As (each writable format), Save a Copy,
// Revert; the unsaved mark against undo and redo; the close and quit prompts;
// and the flatten prompt. ImageDocument is the app's NSDocument class, driven
// in process (XCUITest is not available). Prompts are answered by the tests.
import AppKit
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

/// Records `documentController:didReviewAll:contextInfo:`.
final class ReviewRecorder: NSObject {
    var answers: [Bool] = []
    @objc func documentController(_ c: NSDocumentController, didReviewAll: Bool, contextInfo: UnsafeMutableRawPointer?) {
        answers.append(didReviewAll)
    }
}

@MainActor
final class DocumentLifecycleTests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var docs: [ImageDocument] = []
    var unsavedAnswers: [UnsavedChoice] = []
    var unsavedAsked: [String] = []
    var flattenAnswer = true
    var flattenAsked: [String] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedUnsaved: ImageDocument.UnsavedPrompt!
    var savedFlatten: ImageDocument.FlattenPrompt!

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedUnsaved = ImageDocument.unsavedPrompt
        savedFlatten = ImageDocument.flattenPrompt
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        ImageDocument.unsavedPrompt = { [unowned self] doc, done in
            unsavedAsked.append(doc.displayName)
            done(unsavedAnswers.isEmpty ? .cancel : unsavedAnswers.removeFirst())
        }
        ImageDocument.flattenPrompt = { [unowned self] _, type, done in
            flattenAsked.append(type)
            done(flattenAnswer)
        }
    }

    override func tearDown() async throws {
        for d in docs {
            for wc in d.windowControllers { wc.window?.orderOut(nil) }
            d.close()
        }
        docs.removeAll()
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        ImageDocument.unsavedPrompt = savedUnsaved
        ImageDocument.flattenPrompt = savedFlatten
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
    }

    // MARK: Helpers

    /// Opens `url` as the app does (NSDocument's init reads it) and shows its window.
    @discardableResult
    func open(_ url: URL, type: String? = nil) throws -> ImageDocument {
        let t = type ?? UTType(filenameExtension: url.pathExtension)?.identifier ?? "public.data"
        let doc = try ImageDocument(contentsOf: url, ofType: t)
        show(doc)
        return doc
    }

    func show(_ doc: ImageDocument) {
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
    }

    /// A copy of the 512 x 384 plasma PNG in the test folder.
    func pngCopy(_ name: String = "plasma.png") throws -> URL {
        let url = dir.appendingPathComponent(name)
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        return url
    }

    func editor(_ doc: ImageDocument) throws -> Editor { try XCTUnwrap(doc.editor) }
    func window(_ doc: ImageDocument) throws -> NSWindow { try XCTUnwrap(doc.windowControllers.first?.window) }

    func run(_ doc: ImageDocument, _ command: String, _ params: [String: any Sendable] = [:]) async throws {
        let status = try await editor(doc).runAndWait(command, params)
        XCTAssertEqual(status, "ok", command)
        try editor(doc).refresh()
    }

    func saveAndWait(_ doc: ImageDocument, to url: URL, type: String,
                     _ op: NSDocument.SaveOperationType) -> (any Error)? {
        var result: (any Error)?? = .none
        doc.save(to: url, ofType: type, for: op) { result = .some($0) }
        XCTAssertTrue(waitUntil(20) { result != nil }, "save finished")
        return result ?? nil
    }

    func layers(_ doc: ImageDocument) throws -> Int { try XCTUnwrap(doc.engineDocument).info()["layers"].map { ($0 as? [Any])?.count ?? 0 } ?? 0 }

    func stripName(_ doc: ImageDocument) throws -> String {
        let wc = try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
        wc.strip.reload()
        let item = try XCTUnwrap(wc.strip.items.first { $0.editor === doc.editor })
        return item.nameLabel.stringValue
    }

    /// The unsaved mark on all three places: NSDocument, the window's edited
    /// dot and the image strip ("name *").
    func assertMark(_ doc: ImageDocument, _ on: Bool, _ why: String, file: StaticString = #filePath, line: UInt = #line) throws {
        XCTAssertTrue(waitUntil { (try? self.editor(doc).info.dirty) == on }, "editor: \(why)", file: file, line: line)
        XCTAssertEqual(doc.isDocumentEdited, on, "document: \(why)", file: file, line: line)
        XCTAssertTrue(waitUntil { (try? self.window(doc).isDocumentEdited) == on }, "window dot: \(why)", file: file, line: line)
        XCTAssertEqual(try stripName(doc).hasSuffix(" *"), on, "strip mark: \(why)", file: file, line: line)
    }

    /// Decodes a saved file with ImageIO (or the engine for .opaint).
    func decoded(_ url: URL) throws -> (width: Int, height: Int, rgba: [UInt8]) {
        if ImageFormats.route(for: url) == .native {
            let d = try fx.engine.openOpaint(path: url.path)
            defer { try? d.close() }
            let i = try d.info()
            return ((i["width"] as! NSNumber).intValue, (i["height"] as! NSNumber).intValue, try d.pixels())
        }
        let img = try ImageIOImporter.decode(url: url)
        return (img.width, img.height, img.rgba)
    }

    // MARK: New

    func testNewWithEachBackground() throws {
        let controller = NSDocumentController.shared
        let secondary = DocColor(r: 20, g: 120, b: 220)
        for (bg, expected) in [(NewImageBackground.white, [UInt8]([255, 255, 255, 255])),
                               (.secondary, [20, 120, 220, 255]), (.transparent, [0, 0, 0, 0])] {
            let settings = NewImageSettings(width: 70, height: 40, dpi: 150, background: bg, secondary: secondary)
            let doc = try ImageDocumentController.createNewImage(settings, controller: controller)
            docs.append(doc)
            for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
            let engineDoc = try XCTUnwrap(doc.engineDocument)
            let px = try engineDoc.pixels()
            XCTAssertEqual(px.count, 70 * 40 * 4)
            XCTAssertTrue(stride(from: 0, to: px.count, by: 4).allSatisfy { Array(px[$0..<$0 + 4]) == expected }, bg.label)
            let info = try engineDoc.info()
            XCTAssertEqual((info["layers"] as? [[String: Any]])?.first?["name"] as? String, "Background")
            let res = try engineDoc.query("doc.resolution") as? [String: Any]
            XCTAssertEqual((res?["dpi"] as? NSNumber)?.doubleValue, 150)
            XCTAssertNil(doc.fileURL, "untitled")
            XCTAssertEqual(doc.fileType, ImageFormats.opaintIdentifier, "a new image saves as .opaint")
            XCTAssertTrue(doc.displayName.hasPrefix("Untitled"), doc.displayName)
            XCTAssertTrue(try window(doc).isVisible)
            try assertMark(doc, false, "a new image is not modified")
        }
        // Closing an unmodified new image asks nothing.
        let first = docs[0]
        try window(first).performClose(nil)
        XCTAssertTrue(waitUntil { first.windowControllers.isEmpty })
        XCTAssertTrue(unsavedAsked.isEmpty)
    }

    func testNewImageDialogDefaultsAndOK() throws {
        let dialog = NewImageController(settings: NewImageSettings(secondary: DocColor(r: 1, g: 2, b: 3)))
        defer { dialog.window?.orderOut(nil) }
        XCTAssertEqual(dialog.settings.width, 800, "paint.net's default size")
        XCTAssertEqual(dialog.settings.height, 600)
        XCTAssertEqual(dialog.settings.dpi, 96)
        XCTAssertEqual(dialog.settings.background, .white)
        XCTAssertEqual(dialog.widthField.stringValue, "800")
        XCTAssertEqual(dialog.resultLabel.stringValue, "Image size: 1.8 MB (800 × 600 px)")
        dialog.widthField.stringValue = "1024"
        dialog.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: dialog.widthField))
        dialog.update { $0.setResolution(300) }
        XCTAssertEqual(dialog.settings.dpi, 300)
        XCTAssertEqual(dialog.settings.width, 3200, "print size kept: 1024 px at 96 → 10.67 in at 300")
        dialog.backgroundRadios[.transparent]?.performClick(nil)
        XCTAssertEqual(dialog.background, .transparent)
        var got: NewImageSettings??
        dialog.onFinish = { got = .some($0) }
        dialog.okButton.performClick(nil)
        XCTAssertEqual(got??.background, .transparent)
        XCTAssertEqual(got??.secondary, DocColor(r: 1, g: 2, b: 3))
        let again = NewImageController()
        var cancelled: NewImageSettings??
        again.onFinish = { cancelled = .some($0) }
        again.cancelButton.performClick(nil)
        XCTAssertNotNil(cancelled)
        XCTAssertNil(cancelled!, "Cancel makes nothing")
    }

    func testFileNewMenuCommandShowsTheDialogAndMakesTheImage() throws {
        // AppKit keeps one document controller per process, so the test drives
        // the static work that ImageDocumentController.newDocument(_:) calls.
        XCTAssertTrue(ImageDocumentController.instancesRespond(to: #selector(NSDocumentController.newDocument(_:))))
        ImageDocumentController.presentsErrors = false
        defer {
            ImageDocumentController.presentsErrors = true
            ImageDocumentController.onNewImage = nil
        }
        ImageDocumentController.showNewImageDialog(controller: NSDocumentController.shared)
        let dialog = try XCTUnwrap(ImageDocumentController.newImageDialog)
        XCTAssertTrue(dialog.window?.isVisible ?? false)
        var made: ImageDocument?
        ImageDocumentController.onNewImage = { made = $0 }
        dialog.update { $0.setWidth(32) ; $0.setHeight(16) }
        dialog.ok(nil)
        XCTAssertNil(ImageDocumentController.lastNewImageError)
        let doc = try XCTUnwrap(made, "\(String(describing: ImageDocumentController.lastNewImageError))")
        docs.append(doc)
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        XCTAssertEqual(doc.pixelSize, CGSize(width: 32, height: 16))
        XCTAssertNil(ImageDocumentController.newImageDialog)
        let skeleton = MenuSkeleton.build(effects: [], panelTarget: nil)
        let file = try XCTUnwrap(skeleton.item(withTitle: "File")?.submenu)
        XCTAssertEqual(file.item(withTitle: "New…")?.action, #selector(NSDocumentController.newDocument(_:)))
        XCTAssertEqual(file.item(withTitle: "Save")?.action, #selector(NSDocument.save(_:)))
        XCTAssertEqual(file.item(withTitle: "Save a Copy…")?.action, #selector(NSDocument.saveTo(_:)))
        XCTAssertEqual(file.item(withTitle: "Revert to Saved")?.action, #selector(NSDocument.revertToSaved(_:)))
    }

    // MARK: Open

    func testOpenEachFormatFamily() throws {
        // .opaint with three layers, written by the engine.
        let native = dir.appendingPathComponent("layers.opaint")
        let made = try fx.engine.newDocument(width: 40, height: 30, dpi: 72, background: [9, 8, 7, 255])
        _ = try made.dispatch("layer.add", ["name": "Two"])
        _ = try made.dispatch("layer.add", ["name": "Three"])
        try made.save(to: native.path, format: .opaint, markSaved: true)
        try made.close()
        let heic = dir.appendingPathComponent("photo.heic")
        try ImageIOFixtures.write("public.heic", to: heic)
        var cases: [(URL, String, CGSize, Int)] = [
            (native, ImageFormats.opaintIdentifier, CGSize(width: 40, height: 30), 3),
            (try pngCopy(), "public.png", CGSize(width: 512, height: 384), 1),
            (heic, "public.heic", CGSize(width: 64, height: 32), 1),
        ]
        let io = fixtures.appendingPathComponent("io")
        for (file, type) in [("jpeg_baseline.jpg", "public.jpeg"), ("bmp_24bit.bmp", "com.microsoft.bmp"),
                             ("gif_transparent_2frames.gif", "com.compuserve.gif"), ("tiff_rgb8_lzw.tif", "public.tiff"),
                             ("webp_lossless.webp", "org.webmproject.webp")] {
            let url = io.appendingPathComponent(file)
            let src = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil), file)
            let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [String: Any]
            let w = (props?[kCGImagePropertyPixelWidth as String] as? NSNumber)?.doubleValue ?? 0
            let h = (props?[kCGImagePropertyPixelHeight as String] as? NSNumber)?.doubleValue ?? 0
            cases.append((url, type, CGSize(width: w, height: h), 1))
        }
        if ImageFormats.canEncodeAVIF {
            let avif = dir.appendingPathComponent("photo.avif")
            try ImageIOFixtures.write("public.avif", to: avif)
            cases.append((avif, "public.avif", CGSize(width: 64, height: 32), 1))
        }
        for (url, type, size, layerCount) in cases {
            XCTAssertNotEqual(ImageFormats.route(for: url), .unsupported, url.lastPathComponent)
            let doc = try open(url, type: type)
            XCTAssertEqual(doc.pixelSize, size, url.lastPathComponent)
            XCTAssertEqual(try layers(doc), layerCount, url.lastPathComponent)
            XCTAssertEqual(doc.fileURL, url)
            try assertMark(doc, false, "a just-opened \(url.lastPathComponent)")
            // Save writes the file back in its own format, where this Mac can encode it (WebP may not).
            XCTAssertEqual(ImageFormats.saveTypes().contains(type),
                           type == ImageFormats.opaintIdentifier || type == "public.png"
                               || ImageFormats.systemDestinationTypes.contains(type), type)
        }
        XCTAssertEqual(ImageFormats.route(for: native), .native)
        print("M19 opened: \(cases.map { $0.0.pathExtension })")
    }

    // MARK: Save, Save As, Save a Copy, Revert

    func testSaveWritesTheFileAndClearsTheMark() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        try await run(doc, "image.invert_colors")
        try assertMark(doc, true, "after an edit")
        doc.save(nil)  // File > Save
        try assertMark(doc, false, "after Save")
        let back = try decoded(url)
        XCTAssertEqual(back.rgba, try XCTUnwrap(doc.engineDocument).pixels(), "the file holds the edit")
        XCTAssertEqual(doc.fileURL, url)
        XCTAssertEqual(doc.fileType, "public.png")
        XCTAssertTrue(flattenAsked.isEmpty, "one layer: no flatten prompt")
    }

    func testSaveAsEachWritableFormat() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        let types = ImageFormats.saveTypes()
        XCTAssertTrue(types.starts(with: [ImageFormats.opaintIdentifier, "public.png", "public.jpeg",
                                          "com.microsoft.bmp", "public.tiff", "com.compuserve.gif"]), "\(types)")
        XCTAssertEqual(types.contains("org.webmproject.webp"), ImageFormats.systemDestinationTypes.contains("org.webmproject.webp"))
        XCTAssertEqual(types.contains("public.avif"), ImageFormats.canEncodeAVIF)
        XCTAssertEqual(doc.writableTypes(for: .saveAsOperation), types)
        let pixels = try XCTUnwrap(doc.engineDocument).pixels()
        for type in types {
            try await run(doc, "image.invert_colors")
            let expected = try XCTUnwrap(doc.engineDocument).pixels()
            XCTAssertTrue(doc.isDocumentEdited)
            let ext = try XCTUnwrap(ImageFormats.fileExtension(type), type)
            let url = dir.appendingPathComponent("saved-as.\(ext)")
            XCTAssertNil(saveAndWait(doc, to: url, type: type, .saveAsOperation), type)
            XCTAssertEqual(doc.fileURL, url, type)
            XCTAssertEqual(doc.fileType, type)
            XCTAssertTrue(doc.displayName.hasPrefix("saved-as"), doc.displayName)
            XCTAssertEqual(try editor(doc).name, doc.displayName, "the window and strip take the new name")
            try assertMark(doc, false, "after Save As \(type)")
            let back = try decoded(url)
            XCTAssertEqual(back.width, 512, type)
            XCTAssertEqual(back.height, 384, type)
            if [ImageFormats.opaintIdentifier, "public.png", "public.tiff", "com.microsoft.bmp"].contains(type) {
                XCTAssertEqual(back.rgba, expected, "\(type) keeps every pixel")
            } else {
                // Lossy or palette formats: same size, and the first opaque pixel is close.
                let i = stride(from: 0, to: expected.count, by: 4).first { expected[$0 + 3] == 255 } ?? 0
                XCTAssertTrue(ImageIOFixtures.near(Array(back.rgba[i..<i + 3]), Array(expected[i..<i + 3]), 40),
                              "\(type): \(Array(back.rgba[i..<i + 4])) vs \(Array(expected[i..<i + 4]))")
            }
        }
        XCTAssertEqual(try editor(doc).info.historyLabels.count, 1 + types.count, "one edit per format")
        XCTAssertEqual(pixels.count, 512 * 384 * 4)
        XCTAssertTrue(flattenAsked.isEmpty)
    }

    func testSaveACopyLeavesTheDocumentFileAndNameUnchanged() async throws {
        let url = try pngCopy()
        let original = try Data(contentsOf: url)
        let doc = try open(url, type: "public.png")
        try await run(doc, "image.invert_colors")
        let name = doc.displayName
        for (file, type) in [("copy.opaint", ImageFormats.opaintIdentifier), ("copy.jpg", "public.jpeg")] {
            let copy = dir.appendingPathComponent(file)
            XCTAssertNil(saveAndWait(doc, to: copy, type: type, .saveToOperation))
            XCTAssertTrue(FileManager.default.fileExists(atPath: copy.path), file)
            XCTAssertEqual(doc.fileURL, url, "the document's file stays")
            XCTAssertEqual(doc.fileType, "public.png")
            XCTAssertEqual(doc.displayName, name, "the name stays")
            XCTAssertEqual(try editor(doc).name, name)
            try assertMark(doc, true, "Save a Copy leaves the unsaved mark")
        }
        XCTAssertEqual(try Data(contentsOf: url), original, "the document's own file is untouched")
    }

    func testRevertGoesBackToTheSavedFile() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        let engineDoc = try XCTUnwrap(doc.engineDocument)
        let before = try engineDoc.pixels()
        let wc = try XCTUnwrap(doc.windowControllers.first)
        try await run(doc, "image.invert_colors")
        try await run(doc, "layer.add")
        try assertMark(doc, true, "edited")
        try doc.revert(toContentsOf: url, ofType: "public.png")
        XCTAssertTrue(doc.engineDocument === engineDoc, "same engine document")
        XCTAssertTrue(doc.windowControllers.first === wc, "same window")
        XCTAssertEqual(try engineDoc.pixels(), before)
        XCTAssertEqual(try layers(doc), 1)
        XCTAssertTrue(waitUntil { (try? self.editor(doc).info.historyLabels.count) == 1 }, "fresh history")
        try assertMark(doc, false, "after Revert")
    }

    // MARK: Unsaved mark and undo

    func testUndoBackToTheSavedStepClearsTheMarkAndRedoSetsIt() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        try await run(doc, "image.invert_colors")
        try assertMark(doc, true, "edit")
        try await run(doc, "edit.undo")
        try assertMark(doc, false, "undo back to the opened (saved) step")
        try await run(doc, "edit.redo")
        try assertMark(doc, true, "redo")
        // Saved at step 1: undo sets it, redo clears it, a new edit after undo sets it for good.
        doc.save(nil)
        try assertMark(doc, false, "saved")
        try await run(doc, "edit.undo")
        try assertMark(doc, true, "undo past the saved step")
        try await run(doc, "edit.redo")
        try assertMark(doc, false, "redo back to the saved step")
        try await run(doc, "edit.undo")
        try await run(doc, "layer.add")
        try assertMark(doc, true, "new edit after undo")
        try await run(doc, "edit.undo")
        try assertMark(doc, true, "the saved step was cut off by the new edit")
    }

    // MARK: Close and Quit

    func testCloseWithUnsavedWorkPromptsAndCancelKeepsTheWindow() async throws {
        let url = try pngCopy()
        let doc = try open(url, type: "public.png")
        try await run(doc, "image.invert_colors")
        let w = try window(doc)
        unsavedAnswers = [.cancel]
        w.performClose(nil)
        XCTAssertTrue(waitUntil { self.unsavedAsked.count == 1 })
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        XCTAssertTrue(w.isVisible, "Cancel keeps the window")
        XCTAssertFalse(doc.windowControllers.isEmpty)
        XCTAssertNotNil(doc.engineDocument)
        // Save: writes the file, then closes.
        unsavedAnswers = [.save]
        w.performClose(nil)
        XCTAssertTrue(waitUntil { doc.windowControllers.isEmpty }, "Save closes after saving")
        XCTAssertEqual(unsavedAsked.count, 2)
        XCTAssertNotEqual(try Data(contentsOf: url), try Data(contentsOf: fixtures.appendingPathComponent("plasma_alpha_512x384.png")))
        // Don't Save closes without writing.
        let other = try open(try pngCopy("other.png"), type: "public.png")
        let otherData = try Data(contentsOf: try XCTUnwrap(other.fileURL))
        try await run(other, "image.invert_colors")
        unsavedAnswers = [.dontSave]
        try window(other).performClose(nil)
        XCTAssertTrue(waitUntil { other.windowControllers.isEmpty })
        XCTAssertEqual(try Data(contentsOf: try XCTUnwrap(other.fileURL)), otherData)
        XCTAssertEqual(unsavedAsked, [doc.displayName, doc.displayName, other.displayName])
    }

    func testQuitWithUnsavedWorkPromptsAndCancelKeepsTheAppOpen() async throws {
        let controller = NSDocumentController.shared
        let a = try open(try pngCopy("a.png"), type: "public.png")
        let b = try open(try pngCopy("b.png"), type: "public.png")
        let clean = try open(try pngCopy("c.png"), type: "public.png")
        try await run(a, "image.invert_colors")
        try await run(b, "image.invert_colors")
        let recorder = ReviewRecorder()
        let selector = #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:))
        // Don't Save for a, Cancel for b: the quit stops and b stays open.
        unsavedAnswers = [.dontSave, .cancel]
        ImageDocumentController.reviewUnsaved([a, b, clean], controller: controller, delegate: recorder,
                                              didReviewAllSelector: selector, contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 1 })
        XCTAssertEqual(recorder.answers, [false], "Cancel stops the quit")
        XCTAssertEqual(unsavedAsked, [a.displayName, b.displayName], "only images with unsaved work are asked about")
        XCTAssertTrue(a.windowControllers.isEmpty, "Don't Save closed a")
        XCTAssertTrue(try window(b).isVisible, "b stays open")
        XCTAssertTrue(try window(clean).isVisible)
        XCTAssertTrue(b.isDocumentEdited)
        // The app keeps running: the controller still answers, and b can still be edited.
        try await run(b, "edit.undo")
        try await run(b, "edit.redo")
        // Asking again with Don't Save lets the quit go on.
        unsavedAnswers = [.dontSave]
        ImageDocumentController.reviewUnsaved([a, b, clean], controller: controller, delegate: recorder,
                                              didReviewAllSelector: selector, contextInfo: nil)
        XCTAssertTrue(waitUntil { recorder.answers.count == 2 })
        XCTAssertEqual(recorder.answers.last, true)
        XCTAssertEqual(unsavedAsked, [a.displayName, b.displayName, b.displayName])
    }

    // MARK: Flatten prompt

    func testFlattenPromptOnlyForFlatFormatsWithMoreThanOneLayer() async throws {
        let doc = try open(try pngCopy(), type: "public.png")
        // One layer: no prompt for any format.
        XCTAssertNil(saveAndWait(doc, to: dir.appendingPathComponent("one.png"), type: "public.png", .saveAsOperation))
        XCTAssertNil(saveAndWait(doc, to: dir.appendingPathComponent("one.jpg"), type: "public.jpeg", .saveToOperation))
        XCTAssertTrue(flattenAsked.isEmpty)
        try await run(doc, "layer.add")
        XCTAssertEqual(try layers(doc), 2)
        // .opaint keeps layers: no prompt.
        let native = dir.appendingPathComponent("two.opaint")
        XCTAssertNil(saveAndWait(doc, to: native, type: ImageFormats.opaintIdentifier, .saveAsOperation))
        XCTAssertTrue(flattenAsked.isEmpty)
        // Cancel: nothing written, the document and its file stay.
        flattenAnswer = false
        let png = dir.appendingPathComponent("two.png")
        let error = saveAndWait(doc, to: png, type: "public.png", .saveAsOperation)
        XCTAssertTrue(ImageDocument.isUserCancelled(try XCTUnwrap(error)))
        XCTAssertEqual(flattenAsked, ["public.png"])
        XCTAssertFalse(FileManager.default.fileExists(atPath: png.path))
        XCTAssertEqual(doc.fileURL, native)
        // Flatten: the file is the flattened image; the document keeps both layers.
        flattenAnswer = true
        try await run(doc, "image.invert_colors")
        for (file, type) in [("two.png", "public.png"), ("two.jpg", "public.jpeg"), ("two.tif", "public.tiff")] {
            XCTAssertNil(saveAndWait(doc, to: dir.appendingPathComponent(file), type: type, .saveAsOperation))
        }
        XCTAssertEqual(flattenAsked, ["public.png", "public.png", "public.jpeg", "public.tiff"])
        XCTAssertEqual(try layers(doc), 2, "the open image stays layered")
        XCTAssertEqual(try decoded(dir.appendingPathComponent("two.png")).rgba, try XCTUnwrap(doc.engineDocument).pixels())
        try assertMark(doc, false, "flattened save still marks the step saved")
    }

    // MARK: Visual check

    /// Renders the New Image dialog in light and dark when M19_PNG_DIR is set.
    func testRenderNewImageDialog() throws {
        guard let out = ProcessInfo.processInfo.environment["M19_PNG_DIR"] else {
            throw XCTSkip("set M19_PNG_DIR to render the New Image dialog")
        }
        let folder = URL(fileURLWithPath: out)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let dialog = NewImageController(settings: NewImageSettings(background: .secondary,
                                                                       secondary: DocColor(r: 0xE9, g: 0xE7, b: 0xDC)))
            let view = try XCTUnwrap(dialog.window?.contentView)
            view.appearance = NSAppearance(named: appearance)
            dialog.window?.appearance = NSAppearance(named: appearance)
            view.layoutSubtreeIfNeeded()
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            NSAppearance(named: appearance)!.performAsCurrentDrawingAppearance {
                view.cacheDisplay(in: view.bounds, to: rep)
            }
            let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            try png.write(to: folder.appendingPathComponent("new-image-\(name).png"))
            dialog.window?.orderOut(nil)
        }
    }
}

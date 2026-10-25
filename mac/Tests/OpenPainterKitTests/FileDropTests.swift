// M17: files dropped on the canvas or the image strip open as documents, in
// drop order; unsupported files show one status-bar message and open nothing.
// The drag runs through the views' real NSDraggingDestination methods with a
// stand-in NSDraggingInfo over a private pasteboard (XCUITest is not available).
import AppKit
@testable import OpenPainterKit
import XCTest

/// A minimal NSDraggingInfo carrying file URLs on its own pasteboard.
final class FakeDrag: NSObject, NSDraggingInfo {
    let pasteboard: NSPasteboard
    let window: NSWindow?

    init(_ urls: [URL], window: NSWindow?) {
        pasteboard = NSPasteboard(name: NSPasteboard.Name("OpenPainterTests.drop.\(UUID().uuidString)"))
        pasteboard.clearContents()
        pasteboard.writeObjects(urls.map { $0 as NSURL })
        self.window = window
    }

    deinit { pasteboard.releaseGlobally() }

    var draggingDestinationWindow: NSWindow? { window }
    var draggingSourceOperationMask: NSDragOperation { [.copy, .generic] }
    var draggingLocation: NSPoint { NSPoint(x: 200, y: 200) }
    var draggedImageLocation: NSPoint { NSPoint(x: 200, y: 200) }
    var draggedImage: NSImage? { nil }
    var draggingPasteboard: NSPasteboard { pasteboard }
    var draggingSource: Any? { nil }
    var draggingSequenceNumber: Int { 1 }
    func slideDraggedImage(to screenPoint: NSPoint) {}
    var draggingFormation: NSDraggingFormation {
        get { .default }
        set {}
    }
    var animatesToDestination: Bool {
        get { false }
        set {}
    }
    var numberOfValidItemsForDrop: Int {
        get { 0 }
        set {}
    }
    func enumerateDraggingItems(
        options enumOpts: NSDraggingItemEnumerationOptions = [], for view: NSView?, classes classArray: [AnyClass],
        searchOptions: [NSPasteboard.ReadingOptionKey: Any] = [:],
        using block: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void
    ) {}
    var springLoadingHighlight: NSSpringLoadingHighlight { .none }
    func resetSpringLoading() {}
}

@MainActor
final class FileDropTests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var opened: [String] = []

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        opened = []
        // Opens each file the way the app does (DocumentLoader), but with an
        // editor window from the fixture. The first file finishes last when the
        // opener lets them overlap, so the order check means something.
        fx.workspace.fileOpener = { [weak self] url, done in
            guard let self else { return }
            let delay = 0.25 / Double(self.opened.count + 1)
            self.opened.append(url.lastPathComponent)
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                do {
                    let loaded = try DocumentLoader.open(url, engine: self.fx.engine)
                    let editor = Editor(document: loaded.document, pixelSize: loaded.pixelSize, name: url.lastPathComponent)
                    self.fx.workspace.add(editor)
                    let wc = EditorWindowController(editor: editor, workspace: self.fx.workspace)
                    wc.window?.isReleasedWhenClosed = false
                    self.fx.controllers.append(wc)
                    done(nil)
                } catch {
                    done(error)
                }
            }
        }
    }

    override func tearDown() async throws {
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
    }

    private func threeFiles() throws -> [URL] {
        let a = dir.appendingPathComponent("first.heic")
        try ImageIOFixtures.write("public.heic", to: a)
        let b = dir.appendingPathComponent("second.png")
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: b)
        let c = dir.appendingPathComponent(ImageFormats.canEncodeAVIF ? "third.avif" : "third.heic")
        try ImageIOFixtures.write(ImageFormats.canEncodeAVIF ? "public.avif" : "public.heic", to: c, orientation: 6)
        return [a, b, c]
    }

    func testDroppingThreeFilesOnTheCanvasOpensThreeDocumentsInOrder() throws {
        let wc = try fx.open()
        let files = try threeFiles()
        let drag = FakeDrag(files, window: wc.window)
        XCTAssertEqual(wc.container.draggingEntered(drag), .copy)
        XCTAssertNotNil(DropHighlightView.highlight(on: wc.container), "the drop outline shows")
        XCTAssertTrue(wc.container.performDragOperation(drag))
        XCTAssertNil(DropHighlightView.highlight(on: wc.container), "the outline goes after the drop")
        XCTAssertTrue(waitUntil { self.fx.workspace.editors.count == 4 }, "3 new documents")
        XCTAssertEqual(fx.workspace.editors.dropFirst().map(\.name), ["first.heic", "second.png", files[2].lastPathComponent])
        XCTAssertEqual(opened, files.map(\.lastPathComponent), "one at a time, in drop order")
        XCTAssertEqual(fx.workspace.editors[3].pixelSize, CGSize(width: 32, height: 64), "orientation 6 applied")
        XCTAssertNil(wc.editor.message, "no message when every file opens")
    }

    func testDroppingOnTheImageStripOpensDocuments() throws {
        let wc = try fx.open()
        let files = Array(try threeFiles().prefix(2))
        let drag = FakeDrag(files, window: wc.window)
        XCTAssertEqual(wc.strip.draggingEntered(drag), .copy)
        XCTAssertNotNil(DropHighlightView.highlight(on: wc.strip))
        wc.strip.draggingExited(drag)
        XCTAssertNil(DropHighlightView.highlight(on: wc.strip))
        XCTAssertTrue(wc.strip.performDragOperation(drag))
        XCTAssertTrue(waitUntil { self.fx.workspace.editors.count == 3 })
        XCTAssertEqual(fx.workspace.editors.dropFirst().map(\.name), ["first.heic", "second.png"])
        wc.strip.reload()
        XCTAssertEqual(wc.strip.items.count, 3, "the strip shows the new images")
    }

    func testUnsupportedDropShowsOneMessageAndOpensNothing() throws {
        let wc = try fx.open()
        let text = dir.appendingPathComponent("notes.txt")
        try Data("hello".utf8).write(to: text)
        let fake = dir.appendingPathComponent("broken.heic")
        try Data("not an image".utf8).write(to: fake)
        let drag = FakeDrag([text, fake], window: wc.window)
        XCTAssertTrue(wc.container.performDragOperation(drag))
        RunLoop.main.run(until: Date().addingTimeInterval(0.4))
        XCTAssertEqual(fx.workspace.editors.count, 1, "nothing opened")
        XCTAssertTrue(opened.isEmpty, "the opener was never called")
        let message = try XCTUnwrap(wc.editor.message)
        XCTAssertEqual(message, "Cannot open these 2 files: notes.txt and broken.heic. "
            + "OpenPainter opens OpenPainter (.opaint), PNG, JPEG, BMP, GIF, TIFF, WebP, HEIC, HEIF, AVIF and Camera RAW.")
        XCTAssertEqual(wc.statusBar.hint.stringValue, message, "the status bar shows it")
    }

    func testMixedDropOpensTheGoodFileAndNamesTheOther() throws {
        let wc = try fx.open()
        let good = try threeFiles()[0]
        let text = dir.appendingPathComponent("readme.md")
        try Data("# hi".utf8).write(to: text)
        XCTAssertTrue(wc.container.performDragOperation(FakeDrag([text, good], window: wc.window)))
        XCTAssertTrue(waitUntil { self.fx.workspace.editors.count == 2 })
        XCTAssertEqual(opened, ["first.heic"])
        XCTAssertEqual(wc.editor.message,
                       "Cannot open this file: readme.md. OpenPainter opens OpenPainter (.opaint), PNG, JPEG, BMP, GIF, TIFF, WebP, HEIC, HEIF, AVIF and Camera RAW.")
    }

    func testDragWithoutFilesIsRefused() throws {
        let wc = try fx.open()
        let drag = FakeDrag([], window: wc.window)
        drag.pasteboard.clearContents()
        drag.pasteboard.setString("text", forType: .string)
        XCTAssertEqual(wc.container.draggingEntered(drag), [])
        XCTAssertNil(DropHighlightView.highlight(on: wc.container))
        XCTAssertFalse(wc.container.performDragOperation(drag))
    }

    /// Renders the strip and the stage with the drop outline, light and dark,
    /// into OP_PANEL_PNG_DIR (only when it is set).
    func testRenderDropOutline() throws {
        guard let out = ProcessInfo.processInfo.environment["OP_PANEL_PNG_DIR"] else { return }
        let wc = try fx.open()
        let drag = FakeDrag(try threeFiles(), window: wc.window)
        _ = wc.strip.draggingEntered(drag)
        _ = wc.container.draggingEntered(drag)
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            wc.window?.appearance = NSAppearance(named: appearance)
            for (part, view) in [("strip", wc.strip as NSView), ("stage", wc.container as NSView)] {
                view.layoutSubtreeIfNeeded()
                for sub in view.subviewsDeep { sub.needsDisplay = true }
                let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
                view.cacheDisplay(in: view.bounds, to: rep)
                try XCTUnwrap(rep.representation(using: .png, properties: [:]))
                    .write(to: URL(fileURLWithPath: out).appendingPathComponent("m17-drop-\(part)-\(name).png"))
            }
        }
        wc.window?.appearance = nil
        wc.strip.draggingExited(drag)
        wc.container.draggingExited(drag)
    }
}

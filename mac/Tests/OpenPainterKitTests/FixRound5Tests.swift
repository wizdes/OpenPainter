// Release review fix round 5, the Mac side. Item 1: a Magic Wand or Paint
// Bucket whose start the engine accepted but whose draw failed (or timed
// out) ends on both sides, so the next command never adds a fill the user
// did not confirm. Item 2: Quit does not ask about an image that the user
// closed while Quit asked about another one.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class FixRound5Tests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var docs: [ImageDocument] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedUnsaved: ImageDocument.UnsavedPrompt!

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedUnsaved = ImageDocument.unsavedPrompt
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
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
        FloodToolController.resultTimeout = .seconds(120)
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
    }

    func eventually(_ seconds: Double = 10, _ condition: @escaping () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    // MARK: Item 1: a fill or wand that failed after the engine opened it

    /// Waits for the flood tool and the engine's queue, then checks that
    /// the Mac and the engine agree that no session is open.
    func checkNoSession(_ c: EditorWindowController, _ why: String, line: UInt = #line) async {
        let idle = await eventually(15) { c.flood.isIdle }
        XCTAssertTrue(idle, "\(why): the flood tool got its answers", line: line)
        let settled = await c.editor.document.settle(timeout: .seconds(30))
        XCTAssertTrue(settled, "\(why): the engine ran its queue", line: line)
        XCTAssertNil(c.flood.session, "\(why): no session on the Mac", line: line)
        XCTAssertFalse(c.optionsBar.floodSessionOpen, "\(why): no Finish control", line: line)
        XCTAssertEqual((try? c.editor.document.info())?["session_open"] as? Bool, false,
                       "\(why): no session in the engine either", line: line)
    }

    /// The draw fails in the engine (the click point is outside the image,
    /// because a canvas resize was queued before the click and the window
    /// had not seen it yet). The engine opened the session first.
    func testAWandWhoseDrawFailsEndsOnBothSidesAndTheNextCommandAddsNoFill() async throws {
        let c = try fx.open()
        fx.workspace.tools.tool = .magicWand
        let doc = c.editor.document
        _ = try doc.dispatch("image.canvas_size", ["width": 64, "height": 64, "anchor": "center"])
        // The window still thinks the image is 512 x 384.
        c.flood.click(at: CGPoint(x: 300, y: 300), secondary: false)
        await checkNoSession(c, "after the failed draw")
        let message = c.editor.message ?? ""
        XCTAssertTrue(message.contains("Magic Wand"), "the failure is shown: \(message)")
        // The next unrelated command runs and adds no Magic Wand step.
        let status = await c.editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok", "the next command is not blocked by a hidden session")
        XCTAssertFalse(c.editor.info.historyLabels.contains("Magic Wand"), "\(c.editor.info.historyLabels)")
        await checkNoSession(c, "after the next command")
    }

    /// The start answer times out while the engine is busy; the engine
    /// opens the session later and its draw succeeds there.
    func testAPaintBucketWhoseStartTimesOutEndsOnBothSidesAndTheNextCommandAddsNoFill() async throws {
        let c = try fx.open()
        fx.workspace.tools.tool = .paintBucket
        let doc = c.editor.document
        FloodToolController.resultTimeout = .milliseconds(200)
        _ = try doc.dispatch("test.slow_invert", ["delay_ms": 500])  // about 2 s (4 tiles)
        c.flood.click(at: CGPoint(x: 10, y: 10), secondary: false)
        let timedOut = await eventually(10) { (c.editor.message ?? "").contains("timed out") }
        XCTAssertTrue(timedOut, "the timeout is shown: \(c.editor.message ?? "")")
        await checkNoSession(c, "after the timed-out start")
        let status = await c.editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
        XCTAssertFalse(c.editor.info.historyLabels.contains("Paint Bucket"),
                       "no fill the user never confirmed: \(c.editor.info.historyLabels)")
        await checkNoSession(c, "after the next command")
    }

    // MARK: Item 2: Quit skips an image closed while it asked about another

    func pngCopy(_ name: String) throws -> URL {
        let url = dir.appendingPathComponent(name)
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        return url
    }

    func open(_ url: URL) throws -> ImageDocument {
        let doc = try ImageDocument(contentsOf: url, ofType: "public.png")
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        return doc
    }

    func testQuitDoesNotAskAboutAnImageClosedWhileItAskedAboutAnother() async throws {
        let a = try open(try pngCopy("a.png"))
        let b = try open(try pngCopy("b.png"))
        let aEditor = try XCTUnwrap((a.windowControllers.first as? EditorWindowController)?.editor)
        let inverted = await aEditor.runAndWait("image.invert_colors")
        XCTAssertEqual(inverted, "ok")
        // B's queued work outlasts Quit's wait, so Quit must ask about B.
        ImageDocument.settleTimeout = .milliseconds(100)
        _ = try XCTUnwrap(b.engineDocument).dispatch("test.slow_invert", ["delay_ms": 600])  // about 2.4 s
        var asked: [String] = []
        ImageDocument.unsavedPrompt = { doc, done in
            let name = doc === a ? "A" : doc === b ? "B" : "?"
            asked.append(name)
            guard doc === a else { return done(.dontSave) }
            // While Quit asks about A, the user closes B with Don't Save.
            b.windowControllers.first?.window?.performClose(nil)
            Task { @MainActor in
                while b.engineDocument != nil { try? await Task.sleep(for: .milliseconds(10)) }
                done(.dontSave)
            }
        }
        let recorder = ReviewRecorder()
        ImageDocumentController.reviewUnsaved([a, b], controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        let finished = await eventually(15) { recorder.answers.count == 1 }
        XCTAssertTrue(finished, "Quit finished")
        XCTAssertEqual(recorder.answers, [true])
        XCTAssertEqual(asked, ["A", "B"], "B was asked once (by its own Close), not again by Quit")
        XCTAssertNil(a.engineDocument)
        XCTAssertNil(b.engineDocument)
    }
}

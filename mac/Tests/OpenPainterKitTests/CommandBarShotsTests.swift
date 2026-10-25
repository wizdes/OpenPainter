// Visual check of the command buttons in the tool-options row (no toolbar row):
// the main window in light and dark, at the default and the minimum width, and
// with the Magic Wand on. Render only, no checks of behavior. It reuses the M60
// project page renderer, so it is skipped unless OP_RENDER_SHOTS=1 and writes
// its PNGs to OP_SHOTS_DIR. Run it alone:
//   -only-testing:OpenPainterKitTests/ProjectPageShotsTests/testCommandBarShots
import AppKit
@testable import OpenPainterKit
import XCTest

extension ProjectPageShotsTests {
    func testCommandBarShots() throws {
        let url = fixtures.appendingPathComponent("plasma_alpha_512x384.png")
        let doc = try ImageDocument(contentsOf: url, ofType: "public.png")
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        let wc = try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
        let window = try XCTUnwrap(wc.window)
        window.setContentSize(NSSize(width: 1280, height: 800))
        window.layoutIfNeeded()
        wc.updateClearRect()
        fx.workspace.tools.tool = .paintbrush
        wc.refreshCommandBar()
        try shoot("commandbar-window") { try windowImage(wc) }
        try shoot("commandbar-top") { try top(try windowImage(wc), points: 110) }
        // A selection (Crop and Undo turn on), the Magic Wand chosen, an edit (Edited).
        wc.editor.run("selection.rectangle", ["x": 100.0, "y": 80.0, "width": 200.0, "height": 150.0])
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        fx.workspace.tools.tool = .magicWand
        wc.refreshCommandBar()
        try shoot("commandbar-wand-top") { try top(try windowImage(wc), points: 110) }
        // The minimum window width.
        window.setContentSize(window.contentMinSize)
        window.layoutIfNeeded()
        wc.updateClearRect()
        try shoot("commandbar-minimum") { try windowImage(wc) }
        try shoot("commandbar-minimum-top") { try top(try windowImage(wc), points: 110) }
    }

    /// The top `points` of a 2x window image.
    func top(_ image: CGImage, points: CGFloat) throws -> CGImage {
        try XCTUnwrap(image.cropping(to: CGRect(x: 0, y: 0, width: image.width, height: Int(points * Self.scale))))
    }
}

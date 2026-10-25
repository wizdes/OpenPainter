// The image strip (order, unsaved mark, switching, reorder), the tools
// (shared state, canvas mode, options sent to the engine) and the status bar.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class StripAndToolTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    func testImageStripOrderUnsavedMarkAndSwitching() throws {
        let a = try fx.open("plasma_alpha_512x384.png", name: "Alpha")
        let b = try fx.open("checker_600x300.png", name: "Bravo")
        let c = try fx.open("ramp_256x16.png", name: "Charlie")
        XCTAssertTrue(fx.workspace.active === c.editor, "the newest window is active")
        for wc in [a, b, c] {
            XCTAssertEqual(wc.strip.items.map(\.shownName), ["Alpha", "Bravo", "Charlie"], "every window lists all images")
            XCTAssertEqual(wc.strip.items.map(\.isActive), [false, false, true])
        }
        // An edit marks the image unsaved in every strip.
        b.editor.run("selection.select_all")
        XCTAssertTrue(waitUntil { b.editor.info.dirty })
        XCTAssertTrue(waitUntil { a.strip.items.map(\.shownName) == ["Alpha", "Bravo *", "Charlie"] })
        XCTAssertEqual(c.strip.items[1].shownName, "Bravo *")
        XCTAssertTrue(b.window!.isDocumentEdited)
        XCTAssertFalse(a.window!.isDocumentEdited)
        // Undo back to the opened state clears the mark.
        b.undo()
        XCTAssertTrue(waitUntil { a.strip.items[1].shownName == "Bravo" })
        // Clicking an item switches the active image and orders its window front.
        c.strip.items[0].activate()
        XCTAssertTrue(fx.workspace.active === a.editor)
        XCTAssertEqual(c.strip.items.map(\.isActive), [true, false, false])
        let front = NSApp.orderedWindows.first { $0 is EditorWindow }
        XCTAssertTrue(front === a.window, "Alpha's window is in front")
        // Dragging reorders: Charlie to the first slot.
        let item = c.strip.items[2]
        item.frame.origin.x = 0
        c.strip.itemDragEnded(item, at: 0)
        XCTAssertEqual(fx.workspace.editors.map(\.name), ["Charlie", "Alpha", "Bravo"])
        XCTAssertEqual(a.strip.items.map(\.shownName), ["Charlie", "Alpha", "Bravo"])
        // Thumbnails come from the engine composite.
        XCTAssertTrue(waitUntil { fx.workspace.editors.allSatisfy { $0.thumbnail != nil } })
        let thumb = try XCTUnwrap(a.editor.thumbnail)
        XCTAssertEqual(thumb.width, 107, "512 x 384 scaled into 112 x 80")
        XCTAssertEqual(thumb.height, 80)
        // The close target (shown on hover) closes that image and removes it from every strip.
        let bravoItem = try XCTUnwrap(a.strip.items.first { $0.editor === b.editor })
        XCTAssertTrue(bravoItem.closeButton.isHidden, "hidden until hover or focus")
        bravoItem.closeButton.isHidden = false
        bravoItem.closeButton.performClick(nil)
        XCTAssertFalse(b.window!.isVisible)
        XCTAssertEqual(a.strip.items.map(\.shownName), ["Charlie", "Alpha"])
    }

    func testStripNamesAreNotClipped() throws {
        let wc = try fx.open(name: "Coastal light")
        wc.window?.layoutIfNeeded()
        let item = try XCTUnwrap(wc.strip.items.first)
        let label = item.nameLabel
        let needed = label.cell!.cellSize(forBounds: NSRect(x: 0, y: 0, width: 1000, height: 1000)).height
        XCTAssertGreaterThanOrEqual(label.frame.height, needed, "the name's line fits its text height")
        XCTAssertLessThanOrEqual(label.frame.maxY, item.bounds.height, "the name ends inside the item")
        XCTAssertLessThanOrEqual(item.frame.height, Theme.stripHeight - 1, "the item fits the 64 pt strip")
        XCTAssertEqual(wc.strip.frame.height, Theme.stripHeight)
    }

    func testToolsSetSharedStateCanvasModeAndOptionsGoToTheEngine() throws {
        let wc = try fx.open()
        let tools = fx.workspace.tools
        let toolsView = fx.workspace.toolsView
        XCTAssertEqual(toolsView.buttons.count, PaintTool.allCases.count)
        XCTAssertEqual(PaintTool.allCases.count, 19, "paint.net 5.1's tool list")
        toolsView.buttons[.pencil]!.performClick(nil)
        XCTAssertEqual(tools.tool, .pencil)
        XCTAssertTrue(toolsView.buttons[.pencil]!.isOn)
        XCTAssertEqual(wc.canvas.tool, .stroke)
        XCTAssertEqual(wc.optionsBar.picker.titleOfSelectedItem, "Pencil")
        // Options: width with - and +, antialiasing, blend mode, opacity.
        tools.settings.brushWidth = 4
        wc.optionsBar.plus.performClick(nil)
        XCTAssertEqual(tools.settings.brushWidth, 5)
        wc.optionsBar.minus.performClick(nil)
        wc.optionsBar.minus.performClick(nil)
        XCTAssertEqual(tools.settings.brushWidth, 3)
        wc.optionsBar.widthField.stringValue = "1"
        wc.optionsBar.widthField.sendAction(wc.optionsBar.widthField.action, to: wc.optionsBar.widthField.target)
        XCTAssertEqual(tools.settings.brushWidth, 1)
        wc.optionsBar.antialias.performClick(nil)
        XCTAssertFalse(tools.settings.antialias)
        tools.primary = DocColor(r: 255, g: 0, b: 0)
        // A pencil stroke goes to the engine with the options.
        wc.stroke([CGPoint(x: 2.5, y: 2.5), CGPoint(x: 20.5, y: 2.5)])
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.last == "Pencil" }, wc.editor.message ?? "")
        let px = try wc.editor.document.pixels()
        XCTAssertEqual(Array(px[(2 * 512 + 10) * 4..<(2 * 512 + 10) * 4 + 4]), [255, 0, 0, 255])
        // Opacity 50% halves the alpha of the painted color.
        tools.tool = .paintbrush
        tools.settings.opacity = 50
        tools.settings.brushWidth = 6
        tools.settings.blend = .overwrite
        let p = tools.strokeParams(tool: .paintbrush)
        XCTAssertEqual(p["opacity"] as? Double, 128)
        XCTAssertEqual(p["width"] as? Double, 6)
        XCTAssertEqual(p["blend"] as? String, "overwrite")
        XCTAssertEqual(p["antialias"] as? Bool, false)
        wc.stroke([CGPoint(x: 100, y: 100), CGPoint(x: 140, y: 120)])
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.last == "Paintbrush" })
        // Magic Wand: a click selects; tolerance and mode go with it.
        toolsView.buttons[.magicWand]!.performClick(nil)
        XCTAssertEqual(wc.canvas.tool, .click)
        XCTAssertFalse(wc.optionsBar.toleranceField.superview == nil, "flood options shown")
        tools.settings.tolerance = 0
        wc.click(CGPoint(x: 300, y: 300))
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        XCTAssertEqual(tools.floodParams(tool: .magicWand, x: 1, y: 2)["combine"] as? String, "replace")
        XCTAssertEqual(tools.floodParams(tool: .magicWand, x: 1, y: 2)["tolerance"] as? Double, 0)
        // Paint Bucket fills inside the selection with the primary color.
        tools.tool = .paintBucket
        tools.settings.opacity = 100
        tools.settings.floodMode = .global
        tools.settings.tolerance = 100
        XCTAssertEqual(tools.floodParams(tool: .paintBucket, x: 0, y: 0)["contiguous"] as? Bool, false)
        let before = wc.editor.info.historyLabels.count
        wc.click(CGPoint(x: 300, y: 300))
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == before + 1 })
        // Zoom and Pan have no options; tools not built yet are selectable and say so.
        tools.tool = .zoom
        XCTAssertEqual(wc.canvas.tool, .zoom)
        XCTAssertNil(wc.optionsBar.toleranceField.superview)
        XCTAssertNil(wc.optionsBar.widthField.superview)
        tools.tool = .shapes
        XCTAssertEqual(wc.canvas.tool, .inactive)
        XCTAssertEqual(wc.statusBar.hint.stringValue, "Shapes is coming soon")
        // Letter keys on the canvas pick tools; S cycles the selection tools.
        wc.window?.makeFirstResponder(wc.canvas)
        wc.canvas.keyDown(with: keyEvent("b", keyCode: 11))
        XCTAssertEqual(tools.tool, .paintbrush)
        wc.canvas.keyDown(with: keyEvent("s", keyCode: 1))
        XCTAssertEqual(tools.tool, .rectangleSelect)
        wc.canvas.keyDown(with: keyEvent("s", keyCode: 1))
        XCTAssertEqual(tools.tool, .lassoSelect)
        wc.canvas.keyDown(with: keyEvent("h", keyCode: 4))
        XCTAssertEqual(tools.tool, .pan)
        XCTAssertEqual(wc.canvas.tool, .pan)
    }

    func testStatusBarValuesAndZoomControls() throws {
        let wc = try fx.open()
        let status = wc.statusBar
        XCTAssertEqual(status.imageValue.stringValue, "512 × 384 px")
        XCTAssertEqual(status.selectionValue.stringValue, "None")
        XCTAssertEqual(status.units.titleOfSelectedItem, "px")
        XCTAssertFalse(status.units.item(at: 1)!.isEnabled, "inches come later")
        // The shown zoom equals the real scale, including after Fit.
        XCTAssertEqual(status.zoomField.stringValue, ZoomPresets.label(wc.canvas.viewport.zoom))
        status.actualButton.performClick(nil)
        XCTAssertEqual(wc.canvas.viewport.zoom, 1)
        XCTAssertEqual(status.zoomField.stringValue, "100%")
        status.zoomField.stringValue = "250"
        status.zoomField.sendAction(status.zoomField.action, to: status.zoomField.target)
        XCTAssertEqual(wc.canvas.viewport.zoom, 2.5)
        XCTAssertEqual(status.zoomField.stringValue, "250%")
        status.zoomField.stringValue = "abc"
        status.zoomField.sendAction(status.zoomField.action, to: status.zoomField.target)
        XCTAssertEqual(wc.canvas.viewport.zoom, 2.5, "bad input keeps the zoom")
        XCTAssertEqual(status.zoomField.stringValue, "250%")
        status.slider.doubleValue = log(0.5)
        status.slider.sendAction(status.slider.action, to: status.slider.target)
        XCTAssertEqual(wc.canvas.viewport.zoom, 0.5, accuracy: 1e-9)
        status.fitButton.performClick(nil)
        XCTAssertTrue(wc.canvas.isFitted)
        XCTAssertEqual(status.zoomField.stringValue, ZoomPresets.label(wc.canvas.viewport.zoom))
        // Cursor position and selection size.
        wc.canvas.onCursor?(CGPoint(x: 10.7, y: 20.2))
        XCTAssertEqual(status.cursorValue.stringValue, "10, 20")
        wc.selectAllPixels()
        XCTAssertTrue(waitUntil { status.selectionValue.stringValue == "512 × 384 px" })
    }
}

// M23: Rectangle Select in each mode (options row and modifier keys) against
// masks built here from pixel centers; fixed ratio and fixed size; handles
// that replace the same history step; marching ants at two zooms; the Crop
// tool with each aspect preset gives the exact size and pixels; straighten at
// 5 degrees equals the engine's rotate-then-crop; Crop to Selection; the
// Resize and Canvas Size dialogs give the same bytes as the engine commands
// sent directly; undo restores pixels, size and selection.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class SelectCropTests: XCTestCase {
    var fx: ShellFixture!
    let fixture = "plasma_alpha_512x384.png"
    let W = 512, H = 384

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        for wc in fx.controllers {
            ImageSizeController.current(for: wc.editor)?.cancel(nil)
        }
        fx.tearDown()
    }

    // MARK: Helpers

    /// A second document on the same file, driven by engine commands only.
    func reference() throws -> Document {
        try fx.engine.openPNG(path: fixtures.appendingPathComponent(fixture).path)
    }

    /// Runs an engine command and waits on the run loop (an async test would
    /// block the editor's own main-actor tasks).
    func run(_ doc: Document, _ id: String, _ p: [String: any Sendable] = [:],
             file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(LayersHistoryTests.runSync(doc, id, p), "ok", "\(id) \(p)", file: file, line: line)
    }

    /// Byte arrays compared without printing them (they are large).
    func same(_ a: [UInt8], _ b: [UInt8], _ what: String = "", file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(a == b, "bytes differ (\(a.count) vs \(b.count)) \(what)", file: file, line: line)
    }

    func pause(_ s: TimeInterval) { _ = waitUntil(s) { false } }

    func mask(_ e: Editor) throws -> [UInt8] {
        guard e.selection != nil else { return [UInt8](repeating: 0, count: e.info.width * e.info.height) }
        return [UInt8](try e.document.queryData("doc.selection_mask"))
    }

    /// 255 where the pixel center is inside, as the engine's pixelated mask.
    func expected(_ f: (Int, Int) -> Bool) -> [UInt8] {
        var v = [UInt8]()
        v.reserveCapacity(W * H)
        for y in 0..<H { for x in 0..<W { v.append(f(x, y) ? 255 : 0) } }
        return v
    }

    func inside(_ r: CGRect, _ x: Int, _ y: Int) -> Bool {
        r.contains(CGPoint(x: Double(x) + 0.5, y: Double(y) + 0.5))
    }

    func drag(_ wc: EditorWindowController, _ a: CGPoint, _ b: CGPoint, modifiers: NSEvent.ModifierFlags = [],
              right: Bool = false) {
        let s = wc.selectCrop!, v = wc.canvas.viewport
        s.pointerDown(document: a, view: v.viewPoint(ofDocument: a), modifiers: modifiers, right: right)
        let mid = CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2)
        s.pointerDragged(document: mid, view: v.viewPoint(ofDocument: mid), modifiers: modifiers)
        s.pointerUp(document: b, view: v.viewPoint(ofDocument: b), modifiers: modifiers)
    }

    func steps(_ wc: EditorWindowController, _ n: Int, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == n && !wc.selectCrop.busy },
                      "\(wc.editor.info.historyLabels) \(wc.editor.message ?? "")", file: file, line: line)
    }

    func slice(_ px: [UInt8], width: Int, _ r: CGRect) -> [UInt8] {
        var out = [UInt8]()
        for y in Int(r.minY)..<Int(r.maxY) {
            let a = (y * width + Int(r.minX)) * 4
            out += px[a..<(a + Int(r.width) * 4)]
        }
        return out
    }

    // MARK: Geometry

    func testModifierKeysPickTheCombineMode() {
        typealias M = ToolSettings.SelectionMode
        let g = SelectGeometry.self
        XCTAssertEqual(g.mode(modifiers: [], right: false, toolbar: .intersect), M.intersect)
        XCTAssertEqual(g.mode(modifiers: [], right: true, toolbar: .replace), M.replace)
        XCTAssertEqual(g.mode(modifiers: [.command], right: false, toolbar: .replace), M.add)
        XCTAssertEqual(g.mode(modifiers: [.option], right: false, toolbar: .replace), M.subtract)
        XCTAssertEqual(g.mode(modifiers: [.command], right: true, toolbar: .replace), M.invert)
        XCTAssertEqual(g.mode(modifiers: [.option], right: true, toolbar: .add), M.intersect)
    }

    func testDragRectanglesForEachStyle() {
        let canvas = CGSize(width: 512, height: 384)
        let g = SelectGeometry.self
        XCTAssertEqual(
            g.dragRect(from: CGPoint(x: 50.4, y: 40.6), to: CGPoint(x: 10.2, y: 90.7), style: .normal, ratio: .zero,
                       size: .zero, square: false, canvas: canvas),
            CGRect(x: 10, y: 41, width: 40, height: 50))
        XCTAssertEqual(
            g.dragRect(from: CGPoint(x: -20, y: 370), to: CGPoint(x: 30, y: 500), style: .normal, ratio: .zero,
                       size: .zero, square: false, canvas: canvas),
            CGRect(x: 0, y: 370, width: 30, height: 14), "clamped to the canvas")
        // Shift: a square that follows the longer side.
        XCTAssertEqual(
            g.dragRect(from: CGPoint(x: 10, y: 10), to: CGPoint(x: 50, y: 20), style: .normal, ratio: .zero,
                       size: .zero, square: true, canvas: canvas),
            CGRect(x: 10, y: 10, width: 40, height: 40))
        // Fixed ratio 4:3.
        let r43 = g.dragRect(from: CGPoint(x: 10, y: 10), to: CGPoint(x: 50, y: 20), style: .fixedRatio,
                             ratio: CGSize(width: 4, height: 3), size: .zero, square: false, canvas: canvas)
        XCTAssertEqual(r43, CGRect(x: 10, y: 10, width: 40, height: 30))
        // Off the canvas, the ratio is kept and the rectangle stops at the edge.
        let edge = g.dragRect(from: CGPoint(x: 500, y: 300), to: CGPoint(x: 900, y: 320), style: .fixedRatio,
                              ratio: CGSize(width: 1, height: 2), size: .zero, square: false, canvas: canvas)
        XCTAssertEqual(edge, CGRect(x: 500, y: 300, width: 12, height: 24))
        let up = g.dragRect(from: CGPoint(x: 100, y: 30), to: CGPoint(x: 0, y: -500), style: .fixedRatio,
                            ratio: CGSize(width: 2, height: 1), size: .zero, square: false, canvas: canvas)
        XCTAssertEqual(up, CGRect(x: 40, y: 0, width: 60, height: 30))
        // Fixed size: the top-left corner follows the pointer and stays on the canvas.
        XCTAssertEqual(
            g.dragRect(from: .zero, to: CGPoint(x: 480.3, y: 9.6), style: .fixedSize, ratio: .zero,
                       size: CGSize(width: 100, height: 50), square: false, canvas: canvas),
            CGRect(x: 412, y: 10, width: 100, height: 50))
    }

    func testHandlesResizeFromTheOppositeSide() {
        let canvas = CGSize(width: 512, height: 384)
        let r = CGRect(x: 100, y: 100, width: 80, height: 40)
        let g = SelectGeometry.self
        XCTAssertEqual(g.resize(r, handle: .bottomRight, to: CGPoint(x: 200, y: 150), ratio: nil, canvas: canvas),
                       CGRect(x: 100, y: 100, width: 100, height: 50))
        XCTAssertEqual(g.resize(r, handle: .left, to: CGPoint(x: 90, y: 999), ratio: nil, canvas: canvas),
                       CGRect(x: 90, y: 100, width: 90, height: 40))
        XCTAssertEqual(g.resize(r, handle: .top, to: CGPoint(x: 0, y: 120), ratio: nil, canvas: canvas),
                       CGRect(x: 100, y: 120, width: 80, height: 20))
        // With a 2:1 ratio the corner keeps it.
        XCTAssertEqual(g.resize(r, handle: .bottomRight, to: CGPoint(x: 260, y: 110), ratio: 2, canvas: canvas),
                       CGRect(x: 100, y: 100, width: 160, height: 80))
        let vr = CGRect(x: 10, y: 10, width: 100, height: 50)
        XCTAssertEqual(g.handle(at: CGPoint(x: 112, y: 63), viewRect: vr), .bottomRight)
        XCTAssertEqual(g.handle(at: CGPoint(x: 60, y: 8), viewRect: vr), .top)
        XCTAssertNil(g.handle(at: CGPoint(x: 60, y: 35), viewRect: vr))
    }

    func testCropRotationMatchesItsInverse() {
        let c = CGSize(width: 512, height: 384)
        let p = CGPoint(x: 37, y: 300)
        let q = CropGeometry.toRotated(CropGeometry.toOriginal(p, angle: 5, canvas: c), angle: 5, canvas: c)
        XCTAssertEqual(q.x, p.x, accuracy: 1e-9)
        XCTAssertEqual(q.y, p.y, accuracy: 1e-9)
        let center = CropGeometry.toOriginal(CGPoint(x: 256, y: 192), angle: 30, canvas: c)
        XCTAssertEqual(center.x, 256, accuracy: 1e-9)
    }

    // MARK: Rectangle Select on the canvas

    func testEveryModeFromTheOptionsRowAndTheModifierKeys() throws {
        let wc = try fx.open(fixture)
        fx.workspace.tools.tool = .rectangleSelect
        XCTAssertEqual(wc.canvas.tool, .overlay)
        XCTAssertNotNil(wc.selectCrop.stylePopup.superview, "Rectangle Select options are in the row")
        let a = CGRect(x: 40, y: 30, width: 200, height: 150)
        let b = CGRect(x: 150, y: 100, width: 220, height: 200)
        typealias M = ToolSettings.SelectionMode
        let ops: [M: (Bool, Bool) -> Bool] = [
            .replace: { _, y in y }, .add: { $0 || $1 }, .subtract: { $0 && !$1 }, .intersect: { $0 && $1 },
            .invert: { $0 != $1 },
        ]
        // Options row mode, then the same mode from the keys.
        let keyed: [(M, NSEvent.ModifierFlags, Bool)] = [
            (.replace, [], false), (.add, [.command], false), (.subtract, [.option], false),
            (.intersect, [.option], true), (.invert, [.command], true),
        ]
        var n = 1
        for (mode, mods, right) in keyed {
            for useKeys in [false, true] {
                fx.workspace.tools.settings.selectionMode = .replace
                drag(wc, a.origin, CGPoint(x: a.maxX, y: a.maxY))
                n += 1
                steps(wc, n)
                fx.workspace.tools.settings.selectionMode = useKeys ? .replace : mode
                XCTAssertEqual(wc.selectCrop.modePopup.titleOfSelectedItem, (useKeys ? M.replace : mode).label)
                drag(wc, b.origin, CGPoint(x: b.maxX, y: b.maxY), modifiers: useKeys ? mods : [],
                     right: useKeys && right)
                n += 1
                steps(wc, n)
                let want = expected { x, y in ops[mode]!(inside(a, x, y), inside(b, x, y)) }
                same(try mask(wc.editor), want, "\(mode) keys=\(useKeys)")
                XCTAssertEqual(wc.editor.info.historyLabels.last, "Rectangle Select")
            }
        }
        // Escape deselects (one step); Undo brings the selection back.
        let before = try mask(wc.editor)
        wc.canvasKey(.escape)
        n += 1
        steps(wc, n)
        XCTAssertNil(wc.editor.selection)
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        same(try mask(wc.editor), before)
    }

    func testFixedRatioAndFixedSizeSelections() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        fx.workspace.tools.tool = .rectangleSelect
        s.style = .fixedRatio
        s.ratio = CGSize(width: 16, height: 9)
        XCTAssertFalse(s.ratioWidth.isHidden)
        XCTAssertTrue(s.sizeWidth.isHidden)
        drag(wc, CGPoint(x: 10, y: 10), CGPoint(x: 170, y: 30))
        steps(wc, 2)
        XCTAssertEqual(wc.editor.selection, CGRect(x: 10, y: 10, width: 160, height: 90))
        s.style = .fixedSize
        s.fixedSize = CGSize(width: 64, height: 48)
        drag(wc, CGPoint(x: 0, y: 0), CGPoint(x: 500, y: 100))
        steps(wc, 3)
        XCTAssertEqual(wc.editor.selection, CGRect(x: 448, y: 100, width: 64, height: 48))
        same(try mask(wc.editor), expected { inside(CGRect(x: 448, y: 100, width: 64, height: 48), $0, $1) })
    }

    func testHandlesReplaceTheSameStepAndUndoGoesToTheEarlierSelection() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        fx.workspace.tools.tool = .rectangleSelect
        let first = CGRect(x: 20, y: 20, width: 100, height: 60)
        drag(wc, first.origin, CGPoint(x: first.maxX, y: first.maxY))
        steps(wc, 2)
        let second = CGRect(x: 200, y: 100, width: 80, height: 80)
        drag(wc, second.origin, CGPoint(x: second.maxX, y: second.maxY), modifiers: [.command])
        steps(wc, 3)
        XCTAssertEqual(s.editRect, second)
        XCTAssertEqual(s.overlay.handleRect, second, "handles show")
        // Drag the bottom-right handle out to (320, 220).
        let v = wc.canvas.viewport
        let corner = CGPoint(x: second.maxX, y: second.maxY)
        s.pointerDown(document: corner, view: v.viewPoint(ofDocument: corner), modifiers: [], right: false)
        let to = CGPoint(x: 320, y: 220)
        s.pointerDragged(document: to, view: v.viewPoint(ofDocument: to), modifiers: [])
        s.pointerUp(document: to, view: v.viewPoint(ofDocument: to), modifiers: [])
        let grown = CGRect(x: 200, y: 100, width: 120, height: 120)
        XCTAssertTrue(waitUntil { s.editRect == grown && !s.busy })
        XCTAssertEqual(wc.editor.info.historyLabels.count, 3, "the handle drag replaced the step")
        same(try mask(wc.editor), expected { inside(first, $0, $1) || inside(grown, $0, $1) },
                       "still added to the first rectangle")
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.info.historyIndex == 1 })
        same(try mask(wc.editor), expected { inside(first, $0, $1) })
        XCTAssertNil(s.overlay.handleRect, "an undo drops the handles")
        // A click without a drag deselects.
        drag(wc, CGPoint(x: 5, y: 5), CGPoint(x: 5.2, y: 5.1))
        XCTAssertTrue(waitUntil { wc.editor.selection == nil })
    }

    func testMarchingAntsFollowTheSelectionAtEveryZoom() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        XCTAssertNil(s.overlay.selectionPath)
        fx.workspace.tools.tool = .rectangleSelect
        let r = CGRect(x: 100, y: 50, width: 200, height: 100)
        drag(wc, r.origin, CGPoint(x: r.maxX, y: r.maxY))
        steps(wc, 2)
        XCTAssertTrue(waitUntil { s.overlay.selectionPath != nil })
        XCTAssertEqual(s.overlay.selectionPath?.boundingBoxOfPath, r)
        XCTAssertTrue(s.overlay.antsAnimating, "the dash phase moves")
        for zoom in [0.5, 2.0, 8.0] {
            var v = wc.canvas.viewport
            v.setZoom(zoom)
            wc.canvas.setViewport(v)
            let shown = try XCTUnwrap(s.overlay.shownSelectionPath).boundingBoxOfPath
            let a = wc.canvas.viewport.viewPoint(ofDocument: r.origin)
            XCTAssertEqual(shown.minX, a.x, accuracy: 1e-6, "zoom \(zoom)")
            XCTAssertEqual(shown.minY, a.y, accuracy: 1e-6)
            XCTAssertEqual(shown.width, r.width * zoom, accuracy: 1e-6)
            XCTAssertEqual(shown.height, r.height * zoom, accuracy: 1e-6)
        }
        // A Magic Wand selection (a pixel mask) shows its edges too.
        fx.workspace.tools.tool = .magicWand
        fx.workspace.tools.settings.tolerance = 0
        wc.click(CGPoint(x: 10, y: 10))
        wc.flood.finish()  // M31: the click stays live until Finish
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == 3 })
        XCTAssertTrue(waitUntil { s.overlay.selectionPath.map { !$0.isEmpty } ?? false })
        wc.canvasKey(.escape)
        XCTAssertTrue(waitUntil { s.overlay.selectionPath == nil })
    }

    // MARK: Crop tool

    func testEachCropPresetGivesTheExactSizeAndPixels() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        let original = try wc.editor.document.pixels()
        let want: [(CropAspect, CGRect)] = [
            (.free, CGRect(x: 0, y: 0, width: 512, height: 384)),
            (.original, CGRect(x: 0, y: 0, width: 512, height: 384)),
            (.square, CGRect(x: 64, y: 0, width: 384, height: 384)),
            (.r4x3, CGRect(x: 0, y: 0, width: 512, height: 384)),
            (.r3x2, CGRect(x: 0, y: 21, width: 512, height: 341)),
            (.r16x9, CGRect(x: 0, y: 48, width: 512, height: 288)),
            (.r3x4, CGRect(x: 112, y: 0, width: 288, height: 384)),
            (.r2x3, CGRect(x: 128, y: 0, width: 256, height: 384)),
            (.r9x16, CGRect(x: 148, y: 0, width: 216, height: 384)),
        ]
        XCTAssertEqual(want.map(\.0), CropAspect.allCases)
        for (aspect, rect) in want {
            wc.showCropTool(nil)
            XCTAssertTrue(s.cropActive)
            XCTAssertEqual(wc.canvas.tool, .overlay)
            XCTAssertNotNil(s.aspectPopup.superview, "crop options are in the row")
            s.aspectPopup.selectItem(at: CropAspect.allCases.firstIndex(of: aspect)!)
            s.aspectPopup.sendAction(s.aspectPopup.action, to: s.aspectPopup.target)
            XCTAssertEqual(s.cropFrame, rect, "\(aspect)")
            XCTAssertNotNil(s.overlay.crop)
            wc.canvasKey(.enter)  // Enter applies
            XCTAssertFalse(s.cropActive)
            XCTAssertTrue(waitUntil {
                wc.editor.info.historyIndex == 1 && wc.editor.info.width == Int(rect.width)
                    && wc.editor.info.height == Int(rect.height)
            }, "\(aspect)")
            XCTAssertEqual(wc.editor.info.width, Int(rect.width), "\(aspect)")
            XCTAssertEqual(wc.editor.info.height, Int(rect.height), "\(aspect)")
            same(try wc.editor.document.pixels(), slice(original, width: W, rect), "\(aspect)")
            XCTAssertEqual(wc.canvas.viewport.documentSize, rect.size, "the canvas shows the new size")
            XCTAssertEqual(wc.editor.info.historyLabels.last, "Crop")
            wc.undo()
            XCTAssertTrue(waitUntil { wc.editor.info.historyIndex == 0 && wc.editor.info.width == W })
            same(try wc.editor.document.pixels(), original)
        }
        // Escape leaves the image as it was; so does picking another tool.
        wc.showCropTool(nil)
        s.setCropFrame(CGRect(x: 10, y: 10, width: 50, height: 50))
        wc.canvasKey(.escape)
        XCTAssertFalse(s.cropActive)
        wc.showCropTool(nil)
        fx.workspace.tools.tool = .pan
        XCTAssertFalse(s.cropActive)
        XCTAssertEqual(wc.canvas.tool, .pan)
        pause(0.2)
        XCTAssertEqual(wc.editor.info.historyLabels.count, 2)
        XCTAssertEqual(wc.editor.info.historyIndex, 0)
        XCTAssertEqual(wc.editor.info.width, W)
    }

    func testCropFrameDragsAndHandles() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        wc.showCropTool(nil)
        s.setCropAspect(.free)
        let v = wc.canvas.viewport
        func down(_ p: CGPoint) { s.pointerDown(document: p, view: v.viewPoint(ofDocument: p), modifiers: [], right: false) }
        func move(_ p: CGPoint) { s.pointerDragged(document: p, view: v.viewPoint(ofDocument: p), modifiers: []) }
        func up(_ p: CGPoint) { s.pointerUp(document: p, view: v.viewPoint(ofDocument: p), modifiers: []) }
        // A new frame: inside the full frame a drag moves it, so start from a smaller frame.
        s.setCropFrame(CGRect(x: 100, y: 100, width: 100, height: 50))
        down(CGPoint(x: 20, y: 20)); move(CGPoint(x: 60, y: 70)); up(CGPoint(x: 60, y: 70))
        XCTAssertEqual(s.cropFrame, CGRect(x: 20, y: 20, width: 40, height: 50))
        down(CGPoint(x: 30, y: 30)); up(CGPoint(x: 530, y: 30))
        XCTAssertEqual(s.cropFrame, CGRect(x: 472, y: 20, width: 40, height: 50), "moved, kept on the canvas")
        down(CGPoint(x: 472, y: 70)); up(CGPoint(x: 400, y: 100))
        XCTAssertEqual(s.cropFrame, CGRect(x: 400, y: 20, width: 112, height: 80), "bottom-left handle")
        s.setCropAspect(.square)
        XCTAssertEqual(s.cropFrame, CGRect(x: 64, y: 0, width: 384, height: 384))
        s.endCrop()
    }

    func testStraightenAtFiveDegreesEqualsRotateThenCrop() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        wc.showCropTool(nil)
        s.angleField.stringValue = "5"
        s.angleField.sendAction(s.angleField.action, to: s.angleField.target)
        XCTAssertEqual(s.cropAngle, 5)
        let frame = CGRect(x: 40, y: 30, width: 400, height: 300)
        s.setCropFrame(frame)
        let corners = try XCTUnwrap(s.overlay.crop).corners
        XCTAssertNotEqual(corners[0], frame.origin, "the frame is drawn turned on the image")
        s.applyButton.performClick(nil)
        XCTAssertTrue(waitUntil(60) { wc.editor.info.width == 400 })
        XCTAssertEqual(wc.editor.info.historyLabels.count, 2, "rotate and crop are one step")
        let ref = try reference()
        defer { try? ref.close() }
        run(ref, "effect.layers.rotate_zoom", ["angle": 5.0])
        run(ref, "image.crop_rect", ["x": 40, "y": 30, "width": 400, "height": 300])
        same(try wc.editor.document.pixels(), try ref.pixels())
        let unrotated = slice(try reference().pixels(), width: W, frame)
        XCTAssertTrue(try wc.editor.document.pixels() != unrotated, "differs from an unrotated crop")
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.info.width == W })
    }

    func testCropToSelectionFromTheToolbarAndMenuClearsOutsidePixels() throws {
        let wc = try fx.open(fixture)
        run(wc.editor.document, "test.set_selection_shape", ["shape": "ellipse"])
        wc.editor.refresh()
        let maskBefore = try mask(wc.editor)
        let pixelsBefore = try wc.editor.document.pixels()
        XCTAssertTrue(waitUntil { wc.commandButtons["crop"]?.isEnabled == true })
        wc.commandButtons["crop"]!.performClick(nil)
        let ref = try reference()
        defer { try? ref.close() }
        run(ref, "test.set_selection_shape", ["shape": "ellipse"])
        run(ref, "image.crop_to_selection")
        let refInfo = try ref.info()
        let rw = (refInfo["width"] as! NSNumber).intValue
        XCTAssertTrue(waitUntil { wc.editor.info.width == rw })
        let px = try wc.editor.document.pixels()
        same(px, try ref.pixels())
        XCTAssertEqual(px[3], 0, "the corner is outside the ellipse: transparent")
        XCTAssertEqual(px[0], 0, "and its color is removed")
        // Undo restores the size, the pixels and the selection.
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.info.width == W })
        same(try wc.editor.document.pixels(), pixelsBefore)
        same(try mask(wc.editor), maskBefore)
        // The Image menu item sends the same command.
        let menu = MenuSkeleton.build(effects: [], panelTarget: nil)
        let image = try XCTUnwrap(menu.items.first { $0.title == "Image" }?.submenu)
        let crop = try XCTUnwrap(image.items.first { $0.title == "Crop to Selection" })
        XCTAssertEqual(crop.representedObject as? String, "image.crop_to_selection")
        XCTAssertTrue(wc.validateMenuItem(crop))
        wc.runCommand(crop)
        XCTAssertTrue(waitUntil { wc.editor.info.width == rw })
        same(try wc.editor.document.pixels(), try ref.pixels())
    }

    // MARK: Resize and Canvas Size

    func testResizeDialogMatchesTheEngineCommand() throws {
        let wc = try fx.open(fixture)
        let original = try wc.editor.document.pixels()
        // By percentage, Bicubic.
        var d = try XCTUnwrap(wc.presentResizeImage())
        XCTAssertNotNil(wc.window?.attachedSheet)
        d.percentField.stringValue = "50"
        d.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: d.percentField))
        XCTAssertEqual(d.resultLabel.stringValue, "New size: \(d.model.memoryText) (256 × 192 px)")
        d.okButton.performClick(nil)
        XCTAssertNil(wc.window?.attachedSheet)
        XCTAssertTrue(waitUntil(60) { wc.editor.info.width == 256 })
        var ref = try reference()
        run(ref, "image.resize", ["percent": 50.0])
        same(try wc.editor.document.pixels(), try ref.pixels())
        try ref.close()
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.info.width == W })
        same(try wc.editor.document.pixels(), original)
        // By absolute size with aspect kept, Lanczos.
        d = try XCTUnwrap(wc.presentResizeImage())
        d.absoluteRadio.performClick(nil)
        d.widthField.stringValue = "300"
        d.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: d.widthField))
        XCTAssertEqual(d.heightField.stringValue, "225")
        d.resamplingPopup.selectItem(at: ResizeResampling.allCases.firstIndex(of: .lanczos)!)
        d.resamplingPopup.sendAction(d.resamplingPopup.action, to: d.resamplingPopup.target)
        XCTAssertTrue(d.resultLabel.stringValue.hasSuffix("(300 × 225 px)"))
        d.okButton.performClick(nil)
        XCTAssertTrue(waitUntil(60) { wc.editor.info.width == 300 })
        ref = try reference()
        defer { try? ref.close() }
        run(ref, "image.resize",
                      ["width": 300, "height": 225, "maintain_aspect": false, "resampling": "lanczos"])
        same(try wc.editor.document.pixels(), try ref.pixels())
        XCTAssertEqual(wc.editor.info.historyLabels.count, 2)
        // Cancel changes nothing.
        d = try XCTUnwrap(wc.presentResizeImage())
        d.setPercent(10)
        d.cancelButton.performClick(nil)
        pause(0.2)
        XCTAssertEqual(wc.editor.info.width, 300)
    }

    func testPrintSizeAndResolution() {
        // paint.net's example: two inches at 96 pixels/inch is 192 pixels.
        var m = ImageSizeModel(width: 512, height: 384, dpi: 96)
        m.setPrintWidth(2)
        XCTAssertEqual(m.width, 192)
        XCTAssertEqual(m.height, 144, "aspect kept")
        XCTAssertFalse(m.byPercent)
        XCTAssertEqual(m.printHeight, 1.5, accuracy: 1e-9)
        // A new resolution keeps the print size: 2 x 1.5 inches at 300 pixels/inch.
        m.setResolution(300)
        XCTAssertEqual(m.width, 600)
        XCTAssertEqual(m.height, 450)
        m.unit = .centimeters
        XCTAssertEqual(m.resolution, 300 / 2.54, accuracy: 1e-9)
        XCTAssertEqual(m.printWidth, 600 / (300 / 2.54), accuracy: 1e-9)
        m.keepAspect = false
        m.setHeight(100)
        XCTAssertEqual(m.width, 600)
        m.setPercent(25)
        XCTAssertEqual(m.width, 128)
        XCTAssertEqual(m.height, 96)
    }

    func testCanvasSizeDialogMatchesTheEngineCommand() throws {
        let wc = try fx.open(fixture)
        fx.workspace.tools.primary = DocColor(r: 0x12, g: 0x34, b: 0x56)
        let d = try XCTUnwrap(wc.presentCanvasSize())
        XCTAssertEqual(d.anchorButtons.count, 9)
        d.absoluteRadio.performClick(nil)
        d.keepAspect.performClick(nil)  // off
        XCTAssertFalse(d.model.keepAspect)
        d.widthField.stringValue = "600"
        d.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: d.widthField))
        d.heightField.stringValue = "400"
        d.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: d.heightField))
        d.anchorButtons[.bottom_right]!.performClick(nil)
        d.fillPopup.selectItem(at: CanvasFill.allCases.firstIndex(of: .primary)!)
        d.fillPopup.sendAction(d.fillPopup.action, to: d.fillPopup.target)
        XCTAssertTrue(d.resultLabel.stringValue.hasSuffix("(600 × 400 px)"))
        d.okButton.performClick(nil)
        XCTAssertTrue(waitUntil(60) { wc.editor.info.width == 600 && wc.editor.info.height == 400 })
        let ref = try reference()
        defer { try? ref.close() }
        run(ref, "image.canvas_size",
                      ["width": 600, "height": 400, "anchor": "bottom_right", "fill": "primary",
                       "primary": "123456ff"])
        let px = try wc.editor.document.pixels()
        same(px, try ref.pixels())
        XCTAssertEqual(Array(px[0..<4]), [0x12, 0x34, 0x56, 0xFF], "new area filled with the primary color")
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.info.width == W })
        // By percentage, centered.
        let p = try XCTUnwrap(wc.presentCanvasSize())
        p.setPercent(50)
        p.ok(nil)
        XCTAssertTrue(waitUntil(60) { wc.editor.info.width == 256 })
        run(ref, "edit.undo")
        run(ref, "image.canvas_size", ["width": 256, "height": 192])
        same(try wc.editor.document.pixels(), try ref.pixels())
    }

    func testImageMenuOpensTheDialogsAndTheCropTool() throws {
        let wc = try fx.open(fixture)
        let menu = MenuSkeleton.build(effects: [], panelTarget: nil)
        let image = try XCTUnwrap(menu.items.first { $0.title == "Image" }?.submenu)
        for title in ["Crop Tool", "Resize…", "Canvas Size…"] {
            let item = try XCTUnwrap(image.items.first { $0.title == title }, title)
            XCTAssertNotNil(item.action, title)
            XCTAssertTrue(wc.validateMenuItem(item), title)
        }
        wc.showResizeImage(nil)
        XCTAssertTrue(ImageSizeController.current(for: wc.editor) is ResizeImageController)
        let resize = try XCTUnwrap(image.items.first { $0.title == "Resize…" })
        XCTAssertFalse(wc.validateMenuItem(resize), "off while a dialog is up")
        ImageSizeController.current(for: wc.editor)?.cancel(nil)
        wc.showCanvasSize(nil)
        XCTAssertTrue(ImageSizeController.current(for: wc.editor) is CanvasSizeController)
        ImageSizeController.current(for: wc.editor)?.cancel(nil)
    }

    // MARK: Visual check

    /// Sizes from the SPEC; with OP_RENDER_DIR set, writes the Rectangle Select
    /// and Crop option rows, both dialogs and the overlay in light and dark.
    func testRenderedRowsDialogsAndOverlay() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        let bar = wc.optionsBar
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        func write(_ rep: NSBitmapImageRep, _ name: String) throws {
            guard let out else { return }
            let url = URL(fileURLWithPath: out).appendingPathComponent("m23-\(name).png")
            try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: url)
        }
        func render(_ view: NSView, _ appearance: NSAppearance.Name) throws -> NSBitmapImageRep {
            view.appearance = NSAppearance(named: appearance)
            view.needsLayout = true
            view.layoutSubtreeIfNeeded()
            for sub in view.subviewsDeep { sub.needsDisplay = true }
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            return rep
        }
        fx.workspace.tools.tool = .rectangleSelect
        s.style = .fixedRatio
        bar.frame = NSRect(x: 0, y: 0, width: 1100, height: Theme.optionsHeight)
        bar.layoutSubtreeIfNeeded()
        XCTAssertEqual(s.stylePopup.frame.height, 28)
        XCTAssertEqual(s.ratioWidth.frame.height, 28)
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            try write(try render(bar, a), "rect-options-\(n)")
        }
        s.style = .normal
        wc.showCropTool(nil)
        s.setCropAspect(.r3x2)
        s.setCropAngle(5)
        bar.layoutSubtreeIfNeeded()
        XCTAssertEqual(s.aspectPopup.frame.height, 28)
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            try write(try render(bar, a), "crop-options-\(n)")
        }
        // The overlay over a stand-in image (the Metal canvas cannot be captured here).
        let ov = SelectionOverlayView(frame: NSRect(x: 0, y: 0, width: 560, height: 420))
        var vp = Viewport(documentSize: CGSize(width: 512, height: 384), viewSize: ov.bounds.size)
        vp.setZoom(1)
        ov.setViewport(vp)
        let sel = CGMutablePath()
        sel.addRect(CGRect(x: 60, y: 50, width: 180, height: 120))
        sel.addEllipse(in: CGRect(x: 200, y: 120, width: 160, height: 140))
        ov.setSelectionPath(sel)
        ov.handleRect = CGRect(x: 60, y: 50, width: 180, height: 120)
        ov.layout()
        let crop = SelectionOverlayView(frame: ov.frame)
        crop.setViewport(vp)
        let frame = CGRect(x: 60, y: 60, width: 390, height: 260)
        crop.crop = (CropGeometry.corners(frame, angle: 5, canvas: vp.documentSize),
                     RectHandle.allCases.map { CropGeometry.toOriginal($0.point(in: frame), angle: 5, canvas: vp.documentSize) })
        crop.layout()
        for (view, name) in [(ov, "ants"), (crop, "crop-frame")] {
            let w = Int(view.bounds.width) * 2, h = Int(view.bounds.height) * 2
            let ctx = try XCTUnwrap(CGContext(
                data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
            ctx.scaleBy(x: 2, y: 2)
            ctx.translateBy(x: 0, y: view.bounds.height)
            ctx.scaleBy(x: 1, y: -1)
            // Stand-in image: a gradient where the document is.
            let doc = CGRect(origin: vp.origin, size: vp.contentSize)
            ctx.setFillColor(NSColor(white: 0.45, alpha: 1).cgColor)
            ctx.fill(view.bounds)
            let grad = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [
                NSColor.systemOrange.cgColor, NSColor.systemTeal.cgColor] as CFArray, locations: nil)!
            ctx.saveGState()
            ctx.clip(to: doc)
            ctx.drawLinearGradient(grad, start: doc.origin, end: CGPoint(x: doc.maxX, y: doc.maxY), options: [])
            ctx.restoreGState()
            view.layer?.render(in: ctx)
            let rep = NSBitmapImageRep(cgImage: try XCTUnwrap(ctx.makeImage()))
            try write(rep, name)
        }
        XCTAssertNotNil(ov.shownSelectionPath)
        s.endCrop()
        // Both dialogs.
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let r = ResizeImageController(editor: wc.editor, dpi: 96)
            r.update { $0.setPercent(50) }
            try write(try render(r.window!.contentView!, a), "resize-\(n)")
            let c = CanvasSizeController(editor: wc.editor, dpi: 96, tools: fx.workspace.tools)
            c.update { $0.setWidth(640) }
            c.anchor = .top_left
            try write(try render(c.window!.contentView!, a), "canvas-size-\(n)")
            XCTAssertEqual(c.anchorButtons[.center]!.frame.size, NSSize(width: 28, height: 28))
        }
    }
}

extension ImageSizeController {
    /// Tests: by percentage.
    func setPercent(_ p: Double) { update { $0.setPercent(p) } }
}

// M33: Move Selected Pixels and Move Selection on the canvas. Each drag script
// goes through the canvas pointer sink (the Move tool controller) and must
// give the same layer bytes and selection mask as the same move sent to the
// engine directly on a second copy of the image. Also: 100 drag updates
// commit one step, Shift snaps the angle to 15 degrees, Option scales about
// the center, Command copies, arrow-key nudges, Escape restores the state
// exactly, and every way of finishing writes one history step. Move drags
// press away from the box center, where the pivot sits (a press there moves it).
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class MoveToolTests: XCTestCase {
    var fx: ShellFixture!
    let fixture = "plasma_alpha_512x384.png"
    let box = CGRect(x: 100, y: 80, width: 120, height: 90)
    var others: [Document] = []

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        for d in others { try? d.close() }
        others.removeAll()
        fx.tearDown()
    }

    // MARK: Helpers

    func open(_ tool: PaintTool, select: CGRect?) async throws -> EditorWindowController {
        let wc = try fx.open(fixture)
        if let select {
            let status = await wc.editor.runAndWait("selection.rectangle", rectParams(select))
            XCTAssertEqual(status, "ok")
            wc.selectCrop.reloadSelectionPath()
        }
        fx.workspace.tools.tool = tool
        XCTAssertTrue(wc.canvas.pointerSink === wc.moveTool)
        XCTAssertEqual(wc.canvas.tool, .overlay)
        return wc
    }

    func rectParams(_ r: CGRect) -> [String: any Sendable] {
        ["x": Double(r.minX), "y": Double(r.minY), "width": Double(r.width), "height": Double(r.height),
         "combine": "replace", "amend": false]
    }

    func view(_ wc: EditorWindowController, _ p: CGPoint) -> CGPoint { wc.canvas.viewport.viewPoint(ofDocument: p) }

    /// Drives the canvas pointer sink: press at the first point, drag through
    /// the middle ones, release at the last.
    func drag(_ wc: EditorWindowController, _ pts: [CGPoint], modifiers: NSEvent.ModifierFlags = [],
              right: Bool = false, spin: Bool = false) {
        let s: CanvasPointerSink = wc.canvas.pointerSink!
        s.pointerDown(document: pts[0], view: view(wc, pts[0]), modifiers: modifiers, right: right)
        for p in pts.dropFirst().dropLast() {
            s.pointerDragged(document: p, view: view(wc, p), modifiers: modifiers)
            if spin { RunLoop.main.run(until: Date().addingTimeInterval(0.001)) }
        }
        let last = pts[pts.count - 1]
        s.pointerUp(document: last, view: view(wc, last), modifiers: modifiers)
    }

    func pixels(_ d: Document, preview: Bool = false) throws -> [UInt8] { try d.pixels(preview: preview) }

    func mask(_ d: Document) throws -> [UInt8]? {
        let sel = try d.query("doc.selection") as? [String: Any] ?? [:]
        guard sel["active"] as? Bool == true else { return nil }
        return [UInt8](try d.queryData("doc.selection_mask"))
    }

    func labels(_ d: Document) throws -> [String] { try d.info()["history_labels"] as? [String] ?? [] }

    /// The same move sent straight to the engine on a fresh copy of the image.
    func direct(_ kind: MoveKind, _ t: MoveTransform, select: CGRect?, resampling: String = "bicubic",
                copy: Bool = false) async throws -> Document {
        let d = try fx.engine.openPNG(path: fixtures.appendingPathComponent(fixture).path)
        others.append(d)
        if let select {
            let e = try await d.run("selection.rectangle", rectParams(select))
            XCTAssertEqual(e.status, "ok")
        }
        try d.moveBegin(kind, transform: t, resampling: resampling, copy: copy)
        try d.moveFinish()
        return d
    }

    /// Layer bytes and selection mask equal the direct engine move.
    func assertSame(_ wc: EditorWindowController, _ d: Document, file: StaticString = #filePath, line: UInt = #line)
        throws
    {
        let a = try pixels(wc.editor.document), b = try pixels(d)
        XCTAssertEqual(a.count, b.count, file: file, line: line)
        let differ = zip(a, b).filter { $0 != $1 }.count
        XCTAssertEqual(differ, 0, "\(differ) bytes differ from the direct engine move", file: file, line: line)
        XCTAssertEqual(try mask(wc.editor.document), try mask(d), "selection mask", file: file, line: line)
        XCTAssertEqual(try labels(wc.editor.document), try labels(d), file: file, line: line)
    }

    func assertTransform(_ got: MoveTransform?, _ want: MoveTransform, accuracy: Double = 1e-9,
                         file: StaticString = #filePath, line: UInt = #line) {
        guard let got else { return XCTFail("no transform", file: file, line: line) }
        for (g, w) in zip(got.array, want.array) {
            XCTAssertEqual(g, w, accuracy: accuracy, "\(got) vs \(want)", file: file, line: line)
        }
    }

    /// A rotation by `degrees` about `c`, written out from the matrix.
    func rotation(_ degrees: Double, about c: CGPoint) -> MoveTransform {
        let r = degrees * .pi / 180
        let a = cos(r), b = sin(r)
        return MoveTransform(a: a, b: b, c: -b, d: a, tx: c.x - a * c.x + b * c.y, ty: c.y - b * c.x - a * c.y)
    }

    /// A point `distance` document pixels from `c` at `degrees` (y down).
    func polar(_ c: CGPoint, _ degrees: Double, _ distance: Double) -> CGPoint {
        CGPoint(x: c.x + distance * cos(degrees * .pi / 180), y: c.y + distance * sin(degrees * .pi / 180))
    }

    /// A press point in the rotation corridor: 16 view points right of the
    /// box's right edge and 24 above its right nub.
    func corridorPoint(_ wc: EditorWindowController) -> CGPoint {
        let v = view(wc, CGPoint(x: box.maxX, y: box.midY))
        return wc.canvas.viewport.documentPoint(atView: CGPoint(x: v.x + 16, y: v.y - 24))
    }

    /// Awaits (letting main-actor tasks run) until `condition` holds or 10 s pass.
    func settle(_ condition: @escaping () -> Bool) async -> Bool {
        for _ in 0..<1000 {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    // MARK: Tests

    func testDragInsideMovesLikeTheEngine() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let before = try pixels(wc.editor.document)
        drag(wc, [CGPoint(x: 130, y: 100), CGPoint(x: 150, y: 95), CGPoint(x: 167.3, y: 88.6)])
        assertTransform(wc.moveTool.state?.engine, MoveTransform(tx: 37, ty: -11))
        XCTAssertTrue(wc.moveTool.active)
        XCTAssertEqual(try pixels(wc.editor.document), before, "nothing commits before Finish")
        XCTAssertNotEqual(try pixels(wc.editor.document, preview: true), before, "the preview shows the move")
        wc.canvasKey(.enter)
        XCTAssertFalse(wc.moveTool.active)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Move Selected Pixels")
        try assertSame(wc, try await direct(.pixels, MoveTransform(tx: 37, ty: -11), select: box))
    }

    func testNoSelectionMovesTheWholeLayer() async throws {
        let wc = try await open(.moveSelectedPixels, select: nil)
        XCTAssertEqual(wc.moveTool.state?.base, CGRect(x: 0, y: 0, width: 512, height: 384))
        drag(wc, [CGPoint(x: 200, y: 150), CGPoint(x: 220, y: 160)])
        XCTAssertTrue(wc.moveTool.finish())
        try assertSame(wc, try await direct(.pixels, MoveTransform(tx: 20, ty: 10), select: nil))
        // The left 20 columns are now clear.
        let px = try pixels(wc.editor.document)
        XCTAssertTrue((0..<384).allSatisfy { y in px[(y * 512 + 5) * 4 + 3] == 0 })
    }

    func testHundredDragUpdatesCommitOneStep() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let steps = wc.editor.info.historyLabels.count
        let pts = [CGPoint(x: 130, y: 100)] + (1...100).map { CGPoint(x: 130 + Double($0 % 7), y: 100 + Double($0 % 5)) }
        drag(wc, pts, spin: true)
        XCTAssertGreaterThanOrEqual(wc.moveTool.updatesSent, 99, "each drag turn updates the preview")
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps, "no step while the session is open")
        // 100 % 7 = 2, 100 % 5 = 0.
        assertTransform(wc.moveTool.state?.engine, MoveTransform(tx: 2, ty: 0))
        wc.canvasKey(.enter)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 1)
        try assertSame(wc, try await direct(.pixels, MoveTransform(tx: 2, ty: 0), select: box))
    }

    func testShiftSnapsRotationTo15Degrees() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let c = CGPoint(x: box.midX, y: box.midY)
        let start = corridorPoint(wc)
        XCTAssertEqual(MoveGeometry.hit(view(wc, start), state: wc.moveTool.state!) { self.view(wc, $0) }, .rotate)
        // Angles below are measured from the press, as the drag turns the box.
        let a0 = atan2(start.y - c.y, start.x - c.x) * 180 / .pi
        func polar(_ c: CGPoint, _ degrees: Double, _ distance: Double) -> CGPoint {
            self.polar(c, a0 + degrees, distance)
        }
        // Free rotation follows the pointer: 37 degrees.
        wc.moveTool.pointerDown(document: start, view: view(wc, start), modifiers: [], right: false)
        let p37 = polar(c, 37, 80)
        wc.moveTool.pointerDragged(document: p37, view: view(wc, p37), modifiers: [])
        XCTAssertEqual(wc.moveTool.state!.angle, 37, accuracy: 1e-6)
        // Shift snaps it: 37 -> 30, 52 -> 45, -8 -> -15? (-8 / 15 rounds to -1) -> -15, 7 -> 0.
        for (raw, snapped) in [(37.0, 30.0), (52.0, 45.0), (-8.0, -15.0), (7.0, 0.0), (98.0, 105.0)] {
            let p = polar(c, raw, 80)
            wc.moveTool.pointerDragged(document: p, view: view(wc, p), modifiers: [.shift])
            XCTAssertEqual(wc.moveTool.state!.angle, snapped, accuracy: 1e-9, "\(raw)")
        }
        let p = polar(c, 37, 80)
        wc.moveTool.pointerUp(document: p, view: view(wc, p), modifiers: [.shift])
        XCTAssertEqual(wc.moveTool.state!.angle, 30, accuracy: 1e-9)
        assertTransform(wc.moveTool.state?.engine, rotation(30, about: c))
        XCTAssertEqual(wc.statusBar.hint.stringValue, "Rotate: 30.0°")
        let sent = wc.moveTool.state!.engine
        wc.canvasKey(.enter)
        try assertSame(wc, try await direct(.pixels, sent, select: box))
        // The hand-written matrix (last bits may differ) gives the same bytes too.
        try assertSame(wc, try await direct(.pixels, rotation(30, about: c), select: box))
    }

    func testSecondDragContinuesAndSnapsTheTotalAngle() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let c = CGPoint(x: box.midX, y: box.midY)
        // 20 degrees free, then a second drag of +12 with Shift: 32 -> 30.
        let start = corridorPoint(wc)
        let a0 = atan2(start.y - c.y, start.x - c.x) * 180 / .pi
        drag(wc, [start, polar(c, a0 + 20, 80)])
        XCTAssertEqual(wc.moveTool.state!.angle, 20, accuracy: 1e-6)
        let s = polar(c, 100, 80), e = polar(c, 112, 80)
        drag(wc, [s, e], modifiers: [.shift], right: true)
        XCTAssertEqual(wc.moveTool.state!.angle, 30, accuracy: 1e-9)
        assertTransform(wc.moveTool.state?.engine, rotation(30, about: c), accuracy: 1e-6)
        XCTAssertEqual(wc.editor.info.historyLabels.count, 2, "still one session")
    }

    func testRightButtonRotatesFromInsideTheBox() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let c = CGPoint(x: box.midX, y: box.midY)
        drag(wc, [polar(c, 0, 20), polar(c, 90, 20)], right: true)
        XCTAssertEqual(wc.moveTool.state!.angle, 90, accuracy: 1e-6)
        XCTAssertTrue(wc.moveTool.finish())
        // 90 degrees about (160, 125) maps to whole pixels: an exact grid move.
        let t = rotation(90, about: c)
        try assertSame(wc, try await direct(.pixels, MoveTransform(a: 0, b: 1, c: -1, d: 0, tx: t.tx.rounded(), ty: t.ty.rounded()), select: box))
    }

    func testNubScalesFromTheOppositeCornerAndOptionFromTheCenter() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let corner = CGPoint(x: box.maxX, y: box.maxY)
        XCTAssertEqual(MoveGeometry.hit(view(wc, corner), state: wc.moveTool.state!) { self.view(wc, $0) },
                       .nub(.bottomRight))
        // Plain: the top-left corner stays. 120 -> 150 wide (1.25), 90 -> 108 high (1.2).
        drag(wc, [corner, CGPoint(x: box.maxX + 30, y: box.maxY + 18)])
        let plain = wc.moveTool.state!
        XCTAssertEqual(plain.corners[0].x, box.minX, accuracy: 1e-9)
        XCTAssertEqual(plain.corners[0].y, box.minY, accuracy: 1e-9)
        assertTransform(plain.engine, MoveTransform(a: 1.25, d: 1.2, tx: box.minX * -0.25, ty: box.minY * -0.2))
        XCTAssertTrue(wc.moveTool.cancel())

        // Option (paint.net Alt): about the center. Half sizes 60 -> 80, 45 -> 60.
        let state = wc.moveTool.state!
        XCTAssertEqual(state.m, .identity, "cancel put the box back")
        let c = CGPoint(x: box.midX, y: box.midY)
        drag(wc, [corner, CGPoint(x: box.maxX + 20, y: box.maxY + 15)], modifiers: [.option])
        let s = wc.moveTool.state!
        let mid = CGPoint(x: (s.corners[0].x + s.corners[2].x) / 2, y: (s.corners[0].y + s.corners[2].y) / 2)
        XCTAssertEqual(mid.x, c.x, accuracy: 1e-9)
        XCTAssertEqual(mid.y, c.y, accuracy: 1e-9)
        let k = 80.0 / 60, j = 60.0 / 45
        let want = MoveTransform(a: k, d: j, tx: c.x - k * c.x, ty: c.y - j * c.y)
        assertTransform(s.engine, want)
        wc.canvasKey(.enter)
        try assertSame(wc, try await direct(.pixels, s.engine, select: box))
        assertTransform(s.engine, want)
    }

    func testShiftNubKeepsTheAspectRatio() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let corner = CGPoint(x: box.maxX, y: box.maxY)
        // x grows 1.5, y 1.1: Shift uses the larger, 1.5, on both.
        drag(wc, [corner, CGPoint(x: box.minX + 180, y: box.minY + 99)], modifiers: [.shift])
        let s = wc.moveTool.state!
        XCTAssertEqual(s.m.a, 1.5, accuracy: 1e-9)
        XCTAssertEqual(s.m.d, 1.5, accuracy: 1e-9)
        // Past the opposite nub flips (paint.net).
        XCTAssertTrue(wc.moveTool.cancel())
        drag(wc, [corner, CGPoint(x: box.minX - 60, y: box.maxY)])
        XCTAssertEqual(wc.moveTool.state!.m.a, -0.5, accuracy: 1e-9)
    }

    func testCommandDragCopiesAndLeavesTheOriginal() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let before = try pixels(wc.editor.document)
        drag(wc, [CGPoint(x: 130, y: 100), CGPoint(x: 280, y: 100)], modifiers: [.command])
        XCTAssertTrue(wc.moveTool.copy)
        wc.canvasKey(.enter)
        let after = try pixels(wc.editor.document)
        // The original box keeps its pixels.
        var changed = 0
        for y in Int(box.minY)..<Int(box.maxY) {
            for x in Int(box.minX)..<Int(box.maxX) where (0..<4).contains(where: { after[(y * 512 + x) * 4 + $0] != before[(y * 512 + x) * 4 + $0] }) {
                changed += 1
            }
        }
        XCTAssertEqual(changed, 0, "Command copy leaves the original pixels")
        try assertSame(wc, try await direct(.pixels, MoveTransform(tx: 150, ty: 0), select: box, copy: true))
        // Without Command the original box is cut.
        let plain = try await direct(.pixels, MoveTransform(tx: 150, ty: 0), select: box)
        XCTAssertNotEqual(try pixels(plain), after)
    }

    func testArrowKeysNudge() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let w = wc.window!
        w.makeFirstResponder(wc.canvas)
        let right = keyEvent("\u{F703}", keyCode: 124, modifiers: [.numericPad, .function], window: w)
        let down = keyEvent("\u{F701}", keyCode: 125, modifiers: [.numericPad, .function], window: w)
        let left10 = keyEvent("\u{F702}", keyCode: 123, modifiers: [.numericPad, .function, .command], window: w)
        for e in [right, right, right, down, down, left10] { wc.canvas.keyDown(with: e) }
        XCTAssertEqual(wc.moveTool.nudges, 6)
        XCTAssertEqual(wc.moveTool.updatesSent, 6, "each nudge updates at once")
        assertTransform(wc.moveTool.state?.engine, MoveTransform(tx: -7, ty: 2))
        let steps = wc.editor.info.historyLabels.count
        wc.canvasKey(.enter)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 1)
        try assertSame(wc, try await direct(.pixels, MoveTransform(tx: -7, ty: 2), select: box))
        // Arrows do nothing with another tool.
        fx.workspace.tools.tool = .paintbrush
        XCTAssertFalse(wc.moveTool.arrow(keyCode: 124, modifiers: []))
    }

    func testEscapeRestoresTheStateExactly() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let d = wc.editor.document
        let px = try pixels(d), m = try mask(d), info = wc.editor.info
        let path = wc.selectCrop.overlay.selectionPath
        drag(wc, [CGPoint(x: 130, y: 100), CGPoint(x: 170, y: 125)], modifiers: [.command])
        drag(wc, [corridorPoint(wc), polar(CGPoint(x: 200, y: 150), 40, 90)])
        XCTAssertNotEqual(try pixels(d, preview: true), px)
        XCTAssertNotEqual(wc.selectCrop.overlay.selectionPath, path, "the ants follow the move")
        wc.canvasKey(.escape)
        XCTAssertFalse(wc.moveTool.active)
        XCTAssertEqual(try pixels(d), px)
        XCTAssertEqual(try pixels(d, preview: true), px)
        XCTAssertEqual(try mask(d), m)
        XCTAssertEqual(wc.editor.info, info)
        XCTAssertEqual(wc.selectCrop.overlay.selectionPath, path)
        XCTAssertEqual(wc.moveTool.state?.m, .identity)
        XCTAssertNotNil(wc.editor.selection, "Escape cancels the move; it does not deselect")
    }

    func testMoveSelectionMovesOnlyTheOutline() async throws {
        let wc = try await open(.moveSelection, select: box)
        let px = try pixels(wc.editor.document)
        drag(wc, [CGPoint(x: 130, y: 100), CGPoint(x: 110, y: 135)])
        wc.canvasKey(.enter)
        XCTAssertEqual(try pixels(wc.editor.document), px)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Move Selection")
        XCTAssertEqual(wc.editor.selection, box.offsetBy(dx: -20, dy: 35))
        try assertSame(wc, try await direct(.selection, MoveTransform(tx: -20, ty: 35), select: box))
    }

    func testMoveSelectionWithNothingSelectedDoesNothing() async throws {
        let wc = try await open(.moveSelection, select: nil)
        XCTAssertNil(wc.moveTool.state)
        drag(wc, [CGPoint(x: 160, y: 125), CGPoint(x: 140, y: 160)])
        XCTAssertFalse(wc.moveTool.active)
        XCTAssertEqual(wc.editor.info.historyLabels, ["Open"])
    }

    func testPivotDragMovesTheCenterOfRotation() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let c = CGPoint(x: box.midX, y: box.midY)
        XCTAssertEqual(MoveGeometry.hit(view(wc, c), state: wc.moveTool.state!) { self.view(wc, $0) }, .pivot)
        let pivot = CGPoint(x: box.minX + 10, y: box.minY + 10)
        drag(wc, [c, pivot])
        XCTAssertEqual(wc.moveTool.state!.pivot.x, pivot.x, accuracy: 1e-9)
        XCTAssertEqual(wc.moveTool.state!.m, .identity, "moving the pivot moves no pixels")
        drag(wc, [polar(pivot, 0, 50), polar(pivot, 90, 50)], right: true)
        assertTransform(wc.moveTool.state?.engine, rotation(90, about: pivot), accuracy: 1e-6)
    }

    func testEveryFinishWritesOneStep() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        var steps = wc.editor.info.historyLabels.count
        // Another tool.
        drag(wc, [CGPoint(x: 130, y: 100), CGPoint(x: 135, y: 100)])
        fx.workspace.tools.tool = .moveSelection
        XCTAssertFalse(wc.moveTool.active)
        steps += 1
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps)
        XCTAssertTrue(wc.canvas.pointerSink === wc.moveTool)
        // A click outside the box and its corridor.
        drag(wc, [CGPoint(x: 120, y: 95), CGPoint(x: 120, y: 100)])
        XCTAssertTrue(wc.moveTool.active)
        drag(wc, [CGPoint(x: 500, y: 370), CGPoint(x: 500, y: 370)])
        XCTAssertFalse(wc.moveTool.active)
        steps += 1
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Move Selection")
        // Any command: the engine finishes the move before it runs.
        fx.workspace.tools.tool = .moveSelectedPixels
        drag(wc, [CGPoint(x: 120, y: 100), CGPoint(x: 110, y: 100)])
        XCTAssertTrue(wc.moveTool.active)
        let status = await wc.editor.runAndWait("selection.select_all")
        XCTAssertEqual(status, "ok")
        XCTAssertFalse(wc.moveTool.active, "the controller saw the engine finish")
        XCTAssertEqual(Array(wc.editor.info.historyLabels.suffix(2)), ["Move Selected Pixels", "Select All"])
        // Undo during a move commits it, then undoes it.
        drag(wc, [CGPoint(x: 100, y: 100), CGPoint(x: 110, y: 100)])
        let count = wc.editor.info.historyLabels.count
        wc.undo()
        let undone = await settle { wc.editor.info.historyIndex == count - 1 }
        XCTAssertTrue(undone)
        XCTAssertEqual(wc.editor.info.historyLabels.count, count + 1)
        // The Rectangle Select tool gets the pointer back.
        fx.workspace.tools.tool = .rectangleSelect
        XCTAssertTrue(wc.canvas.pointerSink === wc.selectCrop)
        XCTAssertNil(wc.moveTool.overlay.shownBox)
    }

    func testOptionsRowAndResampling() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let shown = wc.optionsBar.customOptions ?? []
        XCTAssertTrue(shown.contains { $0 === wc.moveTool.resamplingPopup })
        XCTAssertEqual(wc.moveTool.resamplingPopup.itemTitles,
                       ["Nearest Neighbor", "Bilinear", "Bicubic", "Supersampling", "Best Quality"])
        XCTAssertEqual(wc.moveTool.resamplingPopup.titleOfSelectedItem, "Bicubic")
        XCTAssertFalse(wc.moveTool.finishButton.isEnabled)
        wc.moveTool.resamplingPopup.selectItem(at: 0)
        _ = wc.moveTool.resamplingPopup.sendAction(wc.moveTool.resamplingPopup.action, to: wc.moveTool.resamplingPopup.target)
        XCTAssertEqual(fx.workspace.tools.settings.moveResampling, .nearest)
        let c = CGPoint(x: box.midX, y: box.midY)
        drag(wc, [corridorPoint(wc), polar(c, 30, 80)], modifiers: [.shift])
        XCTAssertTrue(wc.moveTool.finishButton.isEnabled)
        let t = wc.moveTool.state!.engine
        wc.moveTool.finishButton.performClick(nil)
        XCTAssertFalse(wc.moveTool.active)
        try assertSame(wc, try await direct(.pixels, t, select: box, resampling: "nearest"))
        // Switching to Rectangle Select shows its own options again.
        fx.workspace.tools.tool = .rectangleSelect
        XCTAssertTrue((wc.optionsBar.customOptions ?? []).contains { $0 === wc.selectCrop.modePopup })
        fx.workspace.tools.tool = .paintbrush
        XCTAssertNil(wc.optionsBar.customOptions)
    }

    func testCropToolWorksWhileAMoveToolIsCurrent() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        drag(wc, [CGPoint(x: 130, y: 100), CGPoint(x: 140, y: 100)])
        XCTAssertTrue(wc.moveTool.active)
        wc.showCropTool(nil)
        XCTAssertFalse(wc.moveTool.active, "the move commits before the Crop tool starts")
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Move Selected Pixels")
        XCTAssertTrue(wc.selectCrop.cropActive)
        // Presses reach the crop frame through the Move tool's pointer sink.
        wc.selectCrop.setCropFrame(CGRect(x: 100, y: 100, width: 200, height: 150))
        drag(wc, [CGPoint(x: 150, y: 150), CGPoint(x: 170, y: 160)])
        XCTAssertEqual(wc.selectCrop.cropFrame, CGRect(x: 120, y: 110, width: 200, height: 150))
        XCTAssertTrue(wc.selectCrop.escape())
        XCTAssertFalse(wc.moveTool.active)
    }

    // MARK: Geometry

    func testCorridorAndOutsideHits() {
        var s = MoveState(base: box)
        let id: (CGPoint) -> CGPoint = { $0 }
        XCTAssertEqual(MoveGeometry.hit(CGPoint(x: 130, y: 110), state: s, toView: id), .inside)
        XCTAssertEqual(MoveGeometry.hit(CGPoint(x: box.maxX + 20, y: 100), state: s, toView: id), .rotate)
        XCTAssertEqual(MoveGeometry.hit(CGPoint(x: box.maxX + 30, y: 100), state: s, toView: id), .outside)
        XCTAssertEqual(MoveGeometry.hit(CGPoint(x: box.minX, y: box.minY), state: s, toView: id), .nub(.topLeft))
        XCTAssertEqual(MoveGeometry.hit(CGPoint(x: box.midX, y: box.maxY + 2), state: s, toView: id), .nub(.bottom))
        // A turned box: hits follow it.
        s = MoveGeometry.rotated(s, by: 45)
        let top = s.nub(.top)
        XCTAssertEqual(MoveGeometry.hit(top, state: s, toView: id), .nub(.top))
        let out = CGPoint(x: top.x + 18 * sin(.pi / 4), y: top.y - 18 * cos(.pi / 4))
        XCTAssertEqual(MoveGeometry.hit(out, state: s, toView: id), .rotate)
        XCTAssertEqual(MoveGeometry.hit(CGPoint(x: box.maxX - 1, y: box.minY + 1), state: s, toView: id), .outside)
    }

    func testNudgeSteps() {
        XCTAssertEqual(MoveGeometry.nudge(keyCode: 126, modifiers: []), CGVector(dx: 0, dy: -1))
        XCTAssertEqual(MoveGeometry.nudge(keyCode: 123, modifiers: [.command]), CGVector(dx: -10, dy: 0))
        XCTAssertNil(MoveGeometry.nudge(keyCode: 0, modifiers: []))
    }

    // MARK: Visual check

    /// With OP_RENDER_DIR set, writes the Move options row and the canvas
    /// during a turned and scaled move (the engine's preview pixels under the
    /// box, nubs, pivot and ants) in light and dark. Row heights follow the SPEC.
    func testRenderedOptionsAndMoveOverlay() async throws {
        let wc = try await open(.moveSelectedPixels, select: box)
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        func write(_ rep: NSBitmapImageRep, _ name: String) throws {
            guard let out else { return }
            let url = URL(fileURLWithPath: out).appendingPathComponent("m33-\(name).png")
            try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: url)
        }
        let bar = wc.optionsBar
        bar.frame = NSRect(x: 0, y: 0, width: 1100, height: Theme.optionsHeight)
        bar.layoutSubtreeIfNeeded()
        XCTAssertEqual(wc.moveTool.resamplingPopup.frame.height, 28, accuracy: 4)
        // A move in progress: scale the right edge out, then turn 20 degrees.
        drag(wc, [CGPoint(x: box.maxX, y: box.midY), CGPoint(x: box.maxX + 40, y: box.midY)])
        let c = wc.moveTool.state!.pivot
        let nub = view(wc, wc.moveTool.state!.nub(.right))
        let start = wc.canvas.viewport.documentPoint(atView: CGPoint(x: nub.x + 16, y: nub.y - 24))
        let a0 = atan2(start.y - c.y, start.x - c.x)
        drag(wc, [start, CGPoint(x: c.x + 100 * cos(a0 + 20 * .pi / 180), y: c.y + 100 * sin(a0 + 20 * .pi / 180))])
        bar.layoutSubtreeIfNeeded()
        let preview = try pixels(wc.editor.document, preview: true)
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        var premultiplied = preview
        for i in stride(from: 0, to: premultiplied.count, by: 4) {
            let a = UInt16(premultiplied[i + 3])
            for k in 0..<3 { premultiplied[i + k] = UInt8((UInt16(premultiplied[i + k]) * a + 127) / 255) }
        }
        let image = try XCTUnwrap(CGImage(
            width: 512, height: 384, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 512 * 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: CGDataProvider(data: Data(premultiplied) as CFData)!, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent))
        let size = NSSize(width: 600, height: 440)
        var vp = Viewport(documentSize: CGSize(width: 512, height: 384), viewSize: size)
        vp.setZoom(1)
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            bar.appearance = NSAppearance(named: a)
            bar.needsLayout = true
            bar.layoutSubtreeIfNeeded()
            for sub in bar.subviewsDeep { sub.needsDisplay = true }
            let rep = try XCTUnwrap(bar.bitmapImageRepForCachingDisplay(in: bar.bounds))
            bar.cacheDisplay(in: bar.bounds, to: rep)
            try write(rep, "options-\(n)")

            let ants = SelectionOverlayView(frame: NSRect(origin: .zero, size: size))
            ants.setViewport(vp)
            ants.setSelectionPath(wc.selectCrop.overlay.selectionPath)
            ants.layout()
            let ov = MoveOverlayView(frame: NSRect(origin: .zero, size: size))
            ov.appearance = NSAppearance(named: a)
            ov.viewport = vp
            ov.state = wc.moveTool.state
            ov.layout()
            ov.viewDidChangeEffectiveAppearance()
            XCTAssertNotNil(ov.shownBox)
            let w = Int(size.width) * 2, h = Int(size.height) * 2
            let ctx = try XCTUnwrap(CGContext(
                data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
            ctx.scaleBy(x: 2, y: 2)
            ctx.translateBy(x: 0, y: size.height)
            ctx.scaleBy(x: 1, y: -1)
            let stage = n == "light" ? NSColor(srgbRed: 0x77 / 255.0, green: 0x77 / 255.0, blue: 0x77 / 255.0, alpha: 1)
                : NSColor(srgbRed: 0x25 / 255.0, green: 0x25 / 255.0, blue: 0x25 / 255.0, alpha: 1)
            ctx.setFillColor(stage.cgColor)
            ctx.fill(CGRect(origin: .zero, size: size))
            let doc = CGRect(origin: vp.origin, size: vp.contentSize)
            // Checkerboard under the transparent pixels (8 pt squares).
            for y in stride(from: 0, to: Int(doc.height), by: 8) {
                for x in stride(from: 0, to: Int(doc.width), by: 8) {
                    ctx.setFillColor(((x + y) / 8) % 2 == 0 ? NSColor.white.cgColor : NSColor(white: 0.8, alpha: 1).cgColor)
                    ctx.fill(CGRect(x: doc.minX + CGFloat(x), y: doc.minY + CGFloat(y), width: 8, height: 8))
                }
            }
            // The layer context is y-down; draw the image upright in it.
            ctx.saveGState()
            ctx.translateBy(x: doc.minX, y: doc.maxY)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(image, in: CGRect(origin: .zero, size: doc.size))
            ctx.restoreGState()
            ants.layer?.render(in: ctx)
            ov.layer?.render(in: ctx)
            try write(NSBitmapImageRep(cgImage: try XCTUnwrap(ctx.makeImage())), "canvas-\(n)")
        }
        XCTAssertTrue(wc.moveTool.cancel())
    }
}

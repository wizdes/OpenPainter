// M29: Ellipse Select and Lasso Select in every mode from the options row and
// from the modifier keys (keys win over the options row), Shift for a circle,
// ellipse handles that amend the same step, the Magic Wand in each mode with
// no selection, and the Edit menu's selection items in the no, all, empty and
// rectangle selection states: which items are on, one history step each,
// exact Erase and Fill pixels, Expand, and Copy / Paste Selection through a
// private pasteboard. Expected masks are built here from pixel centers; only
// pixels whose center lies within a hair of a curved or slanted edge (where
// the engine's 0.05 px ellipse flattening may decide) are left out.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class SelectionToolsTests: XCTestCase {
    var fx: ShellFixture!
    let fixture = "plasma_alpha_512x384.png"
    let W = 512, H = 384
    typealias M = ToolSettings.SelectionMode

    let ops: [M: (Bool, Bool) -> Bool] = [
        .replace: { _, b in b }, .add: { $0 || $1 }, .subtract: { $0 && !$1 }, .intersect: { $0 && $1 },
        .invert: { $0 != $1 },
    ]
    /// The modifier keys for each mode (Command adds, Option subtracts; with
    /// the right button Command inverts and Option intersects).
    let keyed: [(M, NSEvent.ModifierFlags, Bool)] = [
        (.replace, [], false), (.add, [.command], false), (.subtract, [.option], false),
        (.intersect, [.option], true), (.invert, [.command], true),
    ]

    override func setUp() async throws {
        fx = try ShellFixture()
        SelectionClipboard.pasteboard = NSPasteboard(name: NSPasteboard.Name("OpenPainterTests.\(UUID().uuidString)"))
    }

    override func tearDown() async throws {
        SelectionClipboard.pasteboard.releaseGlobally()
        SelectionClipboard.pasteboard = .general
        fx.tearDown()
    }

    // MARK: Helpers

    func mask(_ e: Editor) throws -> [UInt8]? {
        guard e.selection != nil else { return nil }
        return [UInt8](try e.document.queryData("doc.selection_mask"))
    }

    func pixels(_ e: Editor) throws -> [UInt8] { [UInt8](try e.document.queryData("doc.pixels")) }

    /// Where a pixel center is: definitely in, definitely out, or too close
    /// to an edge to call.
    enum Side { case inside, outside, edge }

    /// Compares a mask with an expected rule; `edge` pixels are skipped (and
    /// must be few).
    func check(_ got: [UInt8]?, _ rule: (Int, Int) -> Side, _ what: String,
               file: StaticString = #filePath, line: UInt = #line) {
        guard let got else { return XCTFail("no selection: \(what)", file: file, line: line) }
        XCTAssertEqual(got.count, W * H, file: file, line: line)
        var wrong = 0, skipped = 0
        for y in 0..<H {
            for x in 0..<W {
                switch rule(x, y) {
                case .edge: skipped += 1
                case .inside: if got[y * W + x] != 255 { wrong += 1 }
                case .outside: if got[y * W + x] != 0 { wrong += 1 }
                }
            }
        }
        XCTAssertEqual(wrong, 0, "\(wrong) pixels differ: \(what)", file: file, line: line)
        XCTAssertLessThan(skipped, 200, "too many edge pixels skipped: \(what)", file: file, line: line)
    }

    /// Combines sides with a mode: an edge on either side is an edge.
    func combine(_ a: Side, _ b: Side, _ mode: M) -> Side {
        if a == .edge || b == .edge { return .edge }
        return ops[mode]!(a == .inside, b == .inside) ? .inside : .outside
    }

    func rectSide(_ r: CGRect, _ x: Int, _ y: Int) -> Side {
        r.contains(CGPoint(x: Double(x) + 0.5, y: Double(y) + 0.5)) ? .inside : .outside
    }

    /// The ellipse inside box `r`; centers within 0.1 px of the edge are edges.
    func ellipseSide(_ r: CGRect, _ x: Int, _ y: Int) -> Side {
        let rx = r.width / 2, ry = r.height / 2
        let dx = (Double(x) + 0.5 - r.midX) / rx, dy = (Double(y) + 0.5 - r.midY) / ry
        let d = (dx * dx + dy * dy).squareRoot()
        if abs(d - 1) * min(rx, ry) < 0.1 { return .edge }
        return d < 1 ? .inside : .outside
    }

    /// Even-odd point in polygon; centers within 0.01 px of an edge are edges.
    func polygonSide(_ poly: [CGPoint], _ x: Int, _ y: Int) -> Side {
        let p = CGPoint(x: Double(x) + 0.5, y: Double(y) + 0.5)
        var inside = false
        for i in poly.indices {
            let a = poly[i], b = poly[(i + 1) % poly.count]
            // Distance to the segment.
            let ab = CGPoint(x: b.x - a.x, y: b.y - a.y)
            let t = max(0, min(1, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / (ab.x * ab.x + ab.y * ab.y)))
            if hypot(a.x + t * ab.x - p.x, a.y + t * ab.y - p.y) < 0.01 { return .edge }
            if (a.y > p.y) != (b.y > p.y), p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) { inside.toggle() }
        }
        return inside ? .inside : .outside
    }

    func drag(_ wc: EditorWindowController, _ pts: [CGPoint], modifiers: NSEvent.ModifierFlags = [],
              right: Bool = false) {
        let s = wc.selectCrop!, v = wc.canvas.viewport
        s.pointerDown(document: pts[0], view: v.viewPoint(ofDocument: pts[0]), modifiers: modifiers, right: right)
        for p in pts.dropFirst().dropLast() {
            s.pointerDragged(document: p, view: v.viewPoint(ofDocument: p), modifiers: modifiers)
        }
        let last = pts[pts.count - 1]
        s.pointerUp(document: last, view: v.viewPoint(ofDocument: last), modifiers: modifiers)
    }

    func box(_ wc: EditorWindowController, _ r: CGRect, modifiers: NSEvent.ModifierFlags = [], right: Bool = false) {
        drag(wc, [r.origin, CGPoint(x: r.midX, y: r.midY), CGPoint(x: r.maxX, y: r.maxY)],
             modifiers: modifiers, right: right)
    }

    func steps(_ wc: EditorWindowController, _ n: Int, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == n && !wc.selectCrop.busy },
                      "\(wc.editor.info.historyLabels) \(wc.editor.message ?? "")", file: file, line: line)
    }

    /// Waits until the editor reads the engine's latest state.
    func settle(_ wc: EditorWindowController) {
        _ = waitUntil(0.2) { false }
        wc.editor.refresh()
    }

    let base = CGRect(x: 40, y: 30, width: 200, height: 150)

    /// The options-row mode while the keys pick `m`: a different mode, except
    /// Replace (no keys), where the row itself decides.
    func other(_ m: M) -> M {
        switch m {
        case .replace: .replace
        case .intersect: .add
        default: .intersect
        }
    }

    // MARK: Geometry

    func testLassoAreaAndText() {
        XCTAssertFalse(SelectGeometry.isArea([CGPoint(x: 1, y: 1), CGPoint(x: 5, y: 5)]))
        XCTAssertFalse(SelectGeometry.isArea([CGPoint(x: 1, y: 1), CGPoint(x: 5, y: 5), CGPoint(x: 9, y: 9)]),
                       "a straight scribble has no area")
        XCTAssertTrue(SelectGeometry.isArea([CGPoint(x: 0, y: 0), CGPoint(x: 4, y: 0), CGPoint(x: 0, y: 4)]))
        XCTAssertEqual(SelectGeometry.lassoText([CGPoint(x: 1.5, y: 2), CGPoint(x: 3, y: 4.25)]), "1.5 2.0 3.0 4.25")
    }

    // MARK: Ellipse Select

    func testEllipseEveryModeFromTheOptionsRowAndTheModifierKeys() throws {
        let wc = try fx.open(fixture)
        fx.workspace.tools.tool = .ellipseSelect
        XCTAssertEqual(wc.canvas.tool, .overlay)
        XCTAssertTrue(PaintTool.ellipseSelect.isFunctional)
        XCTAssertNotNil(wc.selectCrop.stylePopup.superview, "Ellipse Select has the rectangle's styles")
        let e = CGRect(x: 150, y: 100, width: 221, height: 197)
        var n = 1
        for (mode, mods, right) in keyed {
            for useKeys in [false, true] {
                fx.workspace.tools.tool = .rectangleSelect
                fx.workspace.tools.settings.selectionMode = .replace
                box(wc, base)
                n += 1
                steps(wc, n)
                fx.workspace.tools.tool = .ellipseSelect
                // With keys the options row names another mode: the keys must win.
                fx.workspace.tools.settings.selectionMode = useKeys ? other(mode) : mode
                box(wc, e, modifiers: useKeys ? mods : [], right: useKeys && right)
                n += 1
                steps(wc, n)
                check(try mask(wc.editor), { x, y in
                    self.combine(self.rectSide(self.base, x, y), self.ellipseSide(e, x, y), mode)
                }, "\(mode) keys=\(useKeys)")
                XCTAssertEqual(wc.editor.info.historyLabels.last, "Ellipse Select")
                XCTAssertEqual(wc.editor.info.historyCommands.last, "selection.ellipse")
            }
        }
    }

    func testShiftMakesACircleAndHandlesAmendTheSameStep() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        fx.workspace.tools.tool = .ellipseSelect
        box(wc, CGRect(x: 60, y: 40, width: 100, height: 30), modifiers: [.shift])
        steps(wc, 2)
        let circle = CGRect(x: 60, y: 40, width: 100, height: 100)
        XCTAssertEqual(wc.editor.selection, circle, "Shift keeps a circle on the longer side")
        XCTAssertEqual(s.overlay.handleRect, circle, "the ellipse shows the eight handles")
        // Drag the right handle out by 40 pixels: the same step changes.
        let v = wc.canvas.viewport
        let h = CGPoint(x: circle.maxX, y: circle.midY), to = CGPoint(x: circle.maxX + 40, y: circle.midY)
        s.pointerDown(document: h, view: v.viewPoint(ofDocument: h), modifiers: [], right: false)
        s.pointerDragged(document: to, view: v.viewPoint(ofDocument: to), modifiers: [])
        XCTAssertTrue(s.overlay.pendingEllipse, "the drag shows an ellipse")
        s.pointerUp(document: to, view: v.viewPoint(ofDocument: to), modifiers: [])
        let wide = CGRect(x: 60, y: 40, width: 140, height: 100)
        XCTAssertTrue(waitUntil { s.editRect == wide && !s.busy })
        XCTAssertEqual(wc.editor.info.historyLabels.count, 2, "the handle drag replaced the step")
        check(try mask(wc.editor), { self.ellipseSide(wide, $0, $1) }, "amended ellipse")
        // Switching tools drops the handles.
        fx.workspace.tools.tool = .rectangleSelect
        XCTAssertNil(s.overlay.handleRect)
        wc.undo()
        XCTAssertTrue(waitUntil { wc.editor.selection == nil }, "one undo removes the whole ellipse")
    }

    // MARK: Lasso Select

    /// A self-crossing lasso (a bow tie) with a dent, in document pixels.
    let lasso: [CGPoint] = [
        CGPoint(x: 101.3, y: 60.2), CGPoint(x: 180.7, y: 120.4), CGPoint(x: 330.1, y: 250.9),
        CGPoint(x: 340.6, y: 70.3), CGPoint(x: 230.2, y: 160.7), CGPoint(x: 90.4, y: 290.6),
        CGPoint(x: 120.9, y: 175.1),
    ]

    func testLassoEveryModeFromTheOptionsRowAndTheModifierKeys() throws {
        let wc = try fx.open(fixture)
        fx.workspace.tools.tool = .lassoSelect
        XCTAssertEqual(wc.canvas.tool, .overlay)
        XCTAssertTrue(PaintTool.lassoSelect.isFunctional)
        XCTAssertNotNil(wc.selectCrop.modePopup.superview, "the mode is in the options row")
        XCTAssertNil(wc.selectCrop.stylePopup.superview, "Lasso Select has no styles")
        var n = 1
        for (mode, mods, right) in keyed {
            for useKeys in [false, true] {
                fx.workspace.tools.tool = .rectangleSelect
                fx.workspace.tools.settings.selectionMode = .replace
                box(wc, base)
                n += 1
                steps(wc, n)
                fx.workspace.tools.tool = .lassoSelect
                fx.workspace.tools.settings.selectionMode = useKeys ? other(mode) : mode
                drag(wc, lasso, modifiers: useKeys ? mods : [], right: useKeys && right)
                n += 1
                steps(wc, n)
                check(try mask(wc.editor), { x, y in
                    self.combine(self.rectSide(self.base, x, y), self.polygonSide(self.lasso, x, y), mode)
                }, "\(mode) keys=\(useKeys)")
                XCTAssertEqual(wc.editor.info.historyLabels.last, "Lasso Select")
            }
        }
    }

    func testLassoClickDeselectsAndAScribbleDoesNothing() throws {
        let wc = try fx.open(fixture)
        let s = wc.selectCrop!
        fx.workspace.tools.tool = .lassoSelect
        drag(wc, lasso)
        steps(wc, 2)
        // The drag shows a closed outline while it runs.
        let v = wc.canvas.viewport
        s.pointerDown(document: lasso[0], view: v.viewPoint(ofDocument: lasso[0]), modifiers: [.command], right: false)
        s.pointerDragged(document: lasso[1], view: v.viewPoint(ofDocument: lasso[1]), modifiers: [.command])
        XCTAssertEqual(s.overlay.pendingLasso?.count, 2)
        XCTAssertTrue(s.escape(), "Escape cancels the drag")
        XCTAssertNil(s.overlay.pendingLasso)
        steps(wc, 2)
        // A straight scribble in Add mode adds nothing and makes no step.
        drag(wc, [CGPoint(x: 10, y: 10), CGPoint(x: 20, y: 20), CGPoint(x: 30, y: 30)], modifiers: [.command])
        settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, 2)
        // A click in Replace mode deselects.
        drag(wc, [CGPoint(x: 5, y: 5), CGPoint(x: 5.1, y: 5.1)])
        XCTAssertTrue(waitUntil { wc.editor.selection == nil })
        steps(wc, 3)
    }

    // MARK: Magic Wand with no selection

    func testMagicWandWithNoSelectionCombinesWithNothingInEveryMode() throws {
        let wc = try fx.open(fixture)
        let ref = try fx.open(fixture)
        fx.workspace.tools.tool = .magicWand
        fx.workspace.tools.settings.tolerance = 30
        // The Replace region, made once on the second editor.
        XCTAssertEqual(
            LayersHistoryTests.runSync(ref.editor.document, "tool.magic_wand",
                                       fx.workspace.tools.floodParams(tool: .magicWand, x: 200, y: 150)), "ok")
        ref.editor.refresh()
        let region = try XCTUnwrap(try mask(ref.editor))
        XCTAssertTrue(region.contains(255) && region.contains(0), "the region is part of the image")
        for (mode, selects) in [(M.replace, true), (.add, true), (.subtract, false), (.intersect, false),
                                (.invert, true)] {
            wc.editor.refresh()
            if wc.editor.selection != nil { wc.editor.run("selection.deselect") }
            XCTAssertTrue(waitUntil { wc.editor.selection == nil })
            fx.workspace.tools.settings.selectionMode = mode
            let n = wc.editor.info.historyLabels.count
            wc.click(CGPoint(x: 200, y: 150))
            wc.flood.finish()  // M31: the click stays live until Finish
            XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.count == n + 1 })
            let got = try XCTUnwrap(try mask(wc.editor), "\(mode): the selection is active")
            XCTAssertTrue(got == (selects ? region : [UInt8](repeating: 0, count: W * H)), "\(mode)")
        }
    }

    // MARK: Edit menu

    enum State: CaseIterable { case none, all, empty, rect }
    let rect = CGRect(x: 100, y: 80, width: 120, height: 90)

    func setState(_ wc: EditorWindowController, _ st: State) {
        let doc = wc.editor.document
        func run(_ id: String, _ p: [String: any Sendable] = [:]) {
            XCTAssertEqual(LayersHistoryTests.runSync(doc, id, p), "ok", id)
        }
        let r: [String: any Sendable] = ["x": 100.0, "y": 80.0, "width": 120.0, "height": 90.0]
        switch st {
        case .none: break
        case .all: run("selection.select_all")
        case .empty:
            run("selection.rectangle", r)
            run("selection.rectangle", r.merging(["combine": "subtract"]) { $1 })
        case .rect: run("selection.rectangle", r)
        }
        wc.editor.refresh()
    }

    func menuItems() throws -> [String: NSMenuItem] {
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let edit = try XCTUnwrap(menu.item(withTitle: "Edit")?.submenu)
        var out: [String: NSMenuItem] = [:]
        for t in ["Erase Selection", "Fill Selection", "Invert Selection", "Expand Selection…", "Copy Selection",
                  "Deselect", "Select All"] {
            out[t] = try XCTUnwrap(edit.item(withTitle: t), t)
        }
        let paste = try XCTUnwrap(edit.item(withTitle: "Paste Selection")?.submenu)
        XCTAssertEqual(paste.items.map(\.title), SelectionClipboard.pasteTitles)
        XCTAssertEqual(paste.items.map(\.tag), [0, 1, 2, 3, 4])
        out["Paste"] = paste.items[0]
        return out
    }

    func testEditItemsAreOnOnlyWhenTheyWouldDoSomething() throws {
        let items = try menuItems()
        for st in State.allCases {
            let wc = try fx.open(fixture)
            setState(wc, st)
            let covers = st == .all || st == .rect
            let on = { (t: String) in wc.validateMenuItem(items[t]!) }
            XCTAssertEqual(on("Erase Selection"), covers, "\(st) erase")
            XCTAssertEqual(on("Fill Selection"), covers, "\(st) fill")
            XCTAssertEqual(on("Expand Selection…"), covers, "\(st) expand")
            XCTAssertEqual(on("Invert Selection"), st != .none, "\(st) invert")
            XCTAssertEqual(on("Copy Selection"), st != .none, "\(st) copy")
            XCTAssertEqual(on("Deselect"), st != .none, "\(st) deselect")
            XCTAssertFalse(on("Paste"), "\(st): the clipboard has no selection")
            SelectionClipboard.pasteboard.clearContents()
            SelectionClipboard.pasteboard.setString("{\"polygonList\":[\"1,1,9,1,9,9,1\"]}", forType: .string)
            XCTAssertTrue(on("Paste"), "\(st): selection text on the clipboard")
            SelectionClipboard.pasteboard.clearContents()
            SelectionClipboard.pasteboard.setString("hello", forType: .string)
            XCTAssertFalse(on("Paste"))
            // A focused text field keeps Delete and Shift-Delete.
            if covers {
                XCTAssertTrue(wc.window!.makeFirstResponder(wc.optionsBar.widthField))
                XCTAssertFalse(on("Erase Selection"), "\(st): a text field has focus")
                XCTAssertFalse(on("Fill Selection"))
                wc.window!.makeFirstResponder(wc.canvas)
            }
        }
    }

    func testEraseAndFillFromTheKeysAreExactAndOneStepEach() throws {
        for st in State.allCases {
            let wc = try fx.open(fixture)
            wc.window!.makeFirstResponder(wc.canvas)
            setState(wc, st)
            let before = try pixels(wc.editor)
            let n = wc.editor.info.historyLabels.count
            fx.workspace.tools.primary = DocColor(r: 0x12, g: 0xAB, b: 0x34, a: 0xFF)
            let covers = st == .all || st == .rect
            func inSel(_ x: Int, _ y: Int) -> Bool {
                st == .all || (st == .rect && rect.contains(CGPoint(x: Double(x) + 0.5, y: Double(y) + 0.5)))
            }
            // Shift-Delete: Fill Selection with the primary color; the selection stays.
            wc.canvas.keyDown(with: keyEvent("\u{7f}", keyCode: 51, modifiers: [.shift], window: wc.window))
            if !covers {
                settle(wc)
                XCTAssertEqual(wc.editor.info.historyLabels.count, n, "\(st): nothing to fill")
                XCTAssertTrue(try pixels(wc.editor) == before)
                XCTAssertEqual(wc.editor.message?.hasPrefix("Fill Selection:"), true)
            } else {
                steps(wc, n + 1)
                XCTAssertEqual(wc.editor.info.historyLabels.last, "Fill Selection")
                var want = before
                for y in 0..<H { for x in 0..<W where inSel(x, y) {
                    want.replaceSubrange((y * W + x) * 4..<(y * W + x) * 4 + 4, with: [0x12, 0xAB, 0x34, 0xFF])
                } }
                XCTAssertTrue(try pixels(wc.editor) == want, "\(st): fill pixels")
                XCTAssertNotNil(wc.editor.selection, "the selection stays")
            }
            // Delete: Erase Selection; the selection goes.
            let filled = try pixels(wc.editor)
            let m = wc.editor.info.historyLabels.count
            wc.canvas.keyDown(with: keyEvent("\u{7f}", keyCode: 51, window: wc.window))
            if !covers {
                settle(wc)
                XCTAssertEqual(wc.editor.info.historyLabels.count, m, "\(st): nothing to erase")
                XCTAssertEqual(wc.editor.message?.hasPrefix("Erase Selection:"), true)
                continue
            }
            steps(wc, m + 1)
            XCTAssertEqual(wc.editor.info.historyLabels.last, "Erase Selection")
            var want = filled
            for y in 0..<H { for x in 0..<W where inSel(x, y) {
                want.replaceSubrange((y * W + x) * 4..<(y * W + x) * 4 + 4, with: [0, 0, 0, 0])
            } }
            XCTAssertTrue(try pixels(wc.editor) == want, "\(st): erased pixels")
            XCTAssertNil(wc.editor.selection, "Erase deselects")
            // One undo each.
            wc.undo()
            XCTAssertTrue(waitUntil { wc.editor.info.historyIndex == m - 1 })
            XCTAssertTrue(try pixels(wc.editor) == filled)
            wc.undo()
            XCTAssertTrue(waitUntil { wc.editor.info.historyIndex == n - 1 })
            XCTAssertTrue(try pixels(wc.editor) == before)
        }
    }

    func testInvertExpandAndEnterFromTheMenu() throws {
        let items = try menuItems()
        for st in State.allCases where st != .none {
            let wc = try fx.open(fixture)
            setState(wc, st)
            let start = try XCTUnwrap(try mask(wc.editor))
            var n = wc.editor.info.historyLabels.count
            wc.runCommand(items["Invert Selection"]!)
            n += 1
            steps(wc, n)
            XCTAssertEqual(wc.editor.info.historyLabels.last, "Invert Selection")
            let inverted = try XCTUnwrap(try mask(wc.editor))
            XCTAssertTrue(inverted == start.map { 255 - $0 }, "\(st): inverted")
            wc.undo()
            XCTAssertTrue(waitUntil { wc.editor.info.historyIndex == n - 2 })
            XCTAssertTrue(try mask(wc.editor) == start)
            guard st == .rect else { continue }
            // Expand Selection… : the sheet sends one step; 0 keeps OK off.
            let d = try XCTUnwrap(wc.presentExpandSelection())
            XCTAssertNotNil(wc.window?.attachedSheet)
            XCTAssertFalse(wc.validateMenuItem(items["Erase Selection"]!), "a sheet is up")
            d.field.stringValue = "0"
            d.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification))
            XCTAssertFalse(d.okButton.isEnabled)
            d.field.stringValue = "5"
            d.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification))
            XCTAssertTrue(d.okButton.isEnabled)
            d.confirm()
            // The undone Invert step is replaced, so the count stays.
            XCTAssertTrue(waitUntil { wc.editor.info.historyLabels.last == "Expand Selection" },
                          "\(wc.editor.info.historyLabels)")
            XCTAssertNil(wc.window?.attachedSheet, "OK closes the sheet")
            steps(wc, n)
            check(try mask(wc.editor), { self.rectSide(self.rect.insetBy(dx: -5, dy: -5), $0, $1) }, "expand 5")
            wc.expandSelection(by: -10)
            steps(wc, n + 1)
            XCTAssertEqual(wc.editor.info.historyLabels.last, "Contract Selection")
            check(try mask(wc.editor), { self.rectSide(self.rect.insetBy(dx: 5, dy: 5), $0, $1) }, "contract 10")
            // Cancel sends nothing.
            let c = try XCTUnwrap(wc.presentExpandSelection())
            c.cancel()
            settle(wc)
            XCTAssertEqual(wc.editor.info.historyLabels.count, n + 1)
            // Enter deselects (paint.net), one step.
            wc.canvasKey(.enter)
            steps(wc, n + 2)
            XCTAssertNil(wc.editor.selection)
        }
    }

    func testCopyAndPasteSelectionRoundTripInEveryMode() throws {
        let src = try fx.open(fixture)
        // A Magic Wand mask and an ellipse, so both outline kinds travel.
        fx.workspace.tools.tool = .ellipseSelect
        let e = CGRect(x: 250, y: 120, width: 180, height: 140)
        box(src, e)
        steps(src, 2)
        let items = try menuItems()
        XCTAssertTrue(src.validateMenuItem(items["Copy Selection"]!))
        src.copySelection(items["Copy Selection"]!)
        let text = try XCTUnwrap(SelectionClipboard.pasteboard.string(forType: .string))
        XCTAssertTrue(text.contains("\"polygonList\""), text)
        check(try mask(src.editor), { self.ellipseSide(e, $0, $1) }, "copied ellipse")
        for (i, mode) in M.allCases.enumerated() {
            let dst = try fx.open(fixture)
            setState(dst, .rect)
            let item = NSMenuItem(title: SelectionClipboard.pasteTitles[i], action: nil, keyEquivalent: "")
            item.tag = i
            XCTAssertTrue(dst.validateMenuItem(items["Paste"]!))
            dst.pasteSelection(item)
            steps(dst, 3)
            XCTAssertEqual(dst.editor.info.historyLabels.last, "Paste Selection")
            check(try mask(dst.editor), { x, y in
                self.combine(self.rectSide(self.rect, x, y), self.ellipseSide(e, x, y), mode)
            }, "paste \(mode)")
        }
        // A wand selection (a pixel mask) copies too.
        fx.workspace.tools.tool = .magicWand
        fx.workspace.tools.settings.selectionMode = .replace
        fx.workspace.tools.settings.tolerance = 30
        src.click(CGPoint(x: 200, y: 150))
        src.flood.finish()  // M31: the click stays live until Finish
        steps(src, 3)
        let wand = try XCTUnwrap(try mask(src.editor))
        XCTAssertTrue(src.copySelectionOutline())
        let dst = try fx.open(fixture)
        dst.pasteSelectionOutline(mode: .replace)
        steps(dst, 2)
        let pasted = try XCTUnwrap(try mask(dst.editor))
        // Wand coverage at 128 and up travels as whole pixels.
        XCTAssertTrue(pasted == wand.map { $0 >= 128 ? 255 : 0 }, "wand outline")
        // Nothing selected: Copy is off and does nothing.
        dst.editor.run("selection.deselect")
        XCTAssertTrue(waitUntil { dst.editor.selection == nil })
        XCTAssertFalse(dst.validateMenuItem(items["Copy Selection"]!))
        XCTAssertFalse(dst.copySelectionOutline())
    }

    // MARK: Visual check

    /// With TEST_RUNNER_OP_RENDER_DIR set, writes the Ellipse and Lasso option
    /// rows, the overlay with an ellipse and a lasso being dragged, and the
    /// Expand Selection sheet, in light and dark.
    func testRenderedRowsOverlayAndSheet() throws {
        let wc = try fx.open(fixture)
        let bar = wc.optionsBar
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        func write(_ rep: NSBitmapImageRep, _ name: String) throws {
            guard let out else { return }
            let url = URL(fileURLWithPath: out).appendingPathComponent("m29-\(name).png")
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
        bar.frame = NSRect(x: 0, y: 0, width: 1100, height: Theme.optionsHeight)
        for tool in [PaintTool.ellipseSelect, .lassoSelect] {
            fx.workspace.tools.tool = tool
            bar.layoutSubtreeIfNeeded()
            XCTAssertEqual(wc.selectCrop.modePopup.frame.height, 28, "\(tool)")
            for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
                try write(try render(bar, a), "\(tool == .ellipseSelect ? "ellipse" : "lasso")-options-\(n)")
            }
        }
        // The overlay over a stand-in image (the Metal canvas cannot be captured here).
        for (kind, set) in [("ellipse", { (ov: SelectionOverlayView) in
            ov.pendingEllipse = true
            ov.pending = CGRect(x: 80, y: 60, width: 260, height: 180)
        }), ("lasso", { (ov: SelectionOverlayView) in
            ov.pendingLasso = self.lasso
        })] {
            let ov = SelectionOverlayView(frame: NSRect(x: 0, y: 0, width: 560, height: 420))
            var vp = Viewport(documentSize: CGSize(width: 512, height: 384), viewSize: ov.bounds.size)
            vp.setZoom(1)
            ov.setViewport(vp)
            let sel = CGMutablePath()
            sel.addRect(CGRect(x: 300, y: 200, width: 150, height: 120))
            ov.setSelectionPath(sel)
            set(ov)
            if kind == "ellipse" { ov.handleRect = nil }
            ov.layout()
            XCTAssertNotNil(ov.pendingOver.path, kind)
            let w = Int(ov.bounds.width) * 2, h = Int(ov.bounds.height) * 2
            let ctx = try XCTUnwrap(CGContext(
                data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
            ctx.scaleBy(x: 2, y: 2)
            ctx.translateBy(x: 0, y: ov.bounds.height)
            ctx.scaleBy(x: 1, y: -1)
            let doc = CGRect(origin: vp.origin, size: vp.contentSize)
            ctx.setFillColor(NSColor(white: 0.45, alpha: 1).cgColor)
            ctx.fill(ov.bounds)
            let grad = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [
                NSColor.systemOrange.cgColor, NSColor.systemTeal.cgColor] as CFArray, locations: nil)!
            ctx.saveGState()
            ctx.clip(to: doc)
            ctx.drawLinearGradient(grad, start: doc.origin, end: CGPoint(x: doc.maxX, y: doc.maxY), options: [])
            ctx.restoreGState()
            ov.layer?.render(in: ctx)
            try write(NSBitmapImageRep(cgImage: try XCTUnwrap(ctx.makeImage())), "overlay-\(kind)")
        }
        // The Expand Selection sheet.
        setState(wc, .rect)
        let d = try XCTUnwrap(wc.presentExpandSelection())
        _ = waitUntil(0.5) { false }
        for (n, a) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            d.alert.window.appearance = NSAppearance(named: a)
            try write(try render(d.alert.window.contentView!, a), "expand-sheet-\(n)")
        }
        d.cancel()
    }
}

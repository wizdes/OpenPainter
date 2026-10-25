// M20 Layers and History panels: a scenario run through the panel, the menu
// and the Layer Properties dialog leaves the same bytes as the same commands
// sent to the engine; Properties Cancel changes nothing; thumbnails follow
// edits and history jumps; choosing a layer adds no step; History click jumps.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class LayersHistoryTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        for wc in fx.controllers {
            LayerPropertiesController.current(for: wc.editor)?.cancel(nil)
        }
        fx.tearDown()
    }

    var layers: LayersPanelView { fx.workspace.layersView }
    var history: HistoryPanelView { fx.workspace.historyView }

    // MARK: Helpers

    /// The row for layer `index` (rows are top layer first).
    func row(_ index: Int) throws -> LayerRow {
        layers.layoutSubtreeIfNeeded()
        return try XCTUnwrap(layers.layerRows.first { $0.index == index }, "row for layer \(index)")
    }

    func mouse(_ type: NSEvent.EventType, _ view: NSView, at local: NSPoint, clicks: Int = 1) -> NSEvent {
        NSEvent.mouseEvent(
            with: type, location: view.convert(local, to: nil), modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: view.window?.windowNumber ?? 0,
            context: nil, eventNumber: 0, clickCount: clicks, pressure: 1)!
    }

    func click(_ view: NSView, clicks: Int = 1) {
        let c = NSPoint(x: view.bounds.midX, y: view.bounds.midY)
        view.mouseDown(with: mouse(.leftMouseDown, view, at: c, clicks: clicks))
        view.mouseUp(with: mouse(.leftMouseUp, view, at: c, clicks: clicks))
    }

    /// Drags a row to a point in the list (list coordinates, y down).
    func drag(_ row: LayerRow, toListY y: CGFloat) {
        let start = NSPoint(x: row.bounds.midX, y: row.bounds.midY)
        row.mouseDown(with: mouse(.leftMouseDown, row, at: start))
        let target = row.convert(NSPoint(x: 40, y: y), from: layers.list)
        let mid = NSPoint(x: start.x, y: (start.y + target.y) / 2)
        row.mouseDragged(with: mouse(.leftMouseDragged, row, at: mid))
        row.mouseDragged(with: mouse(.leftMouseDragged, row, at: target))
        row.mouseUp(with: mouse(.leftMouseUp, row, at: target))
    }

    func waitSteps(_ e: Editor, _ n: Int, _ what: String, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(waitUntil { e.info.historyLabels.count == n }, "\(what): \(e.info.historyLabels) \(e.message ?? "")",
                      file: file, line: line)
    }

    /// Every layer's own pixels, exactly (a thumbnail at full size).
    nonisolated static func layerBytes(_ doc: Document) throws -> [[UInt8]] {
        let info = try doc.info()
        let w = (info["width"] as! NSNumber).intValue, h = (info["height"] as! NSNumber).intValue
        return try (info["layers"] as! [[String: Any]]).map { l in
            let data = try doc.queryData("layer.thumbnail", ["id": (l["id"] as! NSNumber).intValue, "max": max(w, h)])
            return [UInt8](data.dropFirst(16))
        }
    }

    /// What a user can see: layers (no ids), history, dirty flag.
    nonisolated static func visible(_ doc: Document) throws -> [String] {
        let info = try doc.info()
        var out = (info["layers"] as! [[String: Any]]).map {
            "\($0["name"]!) \($0["visible"]!) \($0["blend"]!) \($0["opacity"]!)"
        }
        out.append("active \(info["active_layer"]!)")
        out.append("history \((info["history_labels"] as! [String]).joined(separator: ", ")) @\(info["history_index"]!)")
        out.append("dirty \(info["dirty"]!)")
        return out
    }

    /// Waits until the engine has run everything queued before it, then
    /// re-reads the document. (Cancel with no preview is refused in queue order.)
    func drain(_ e: Editor) {
        _ = Self.runSync(e.document, "preview.cancel")
        e.refresh()
    }

    /// Runs one command and waits for its result without leaving the main
    /// actor's run loop (async tests would block the editor's own tasks).
    @discardableResult
    static func runSync(_ doc: Document, _ cmd: String, _ params: [String: any Sendable] = [:]) -> String? {
        let box = ResultBox()
        do {
            let ticket = try doc.dispatch(cmd, params)
            Task.detached { box.set(await doc.engine.result(of: ticket, timeout: .seconds(60))?.status ?? "timeout") }
        } catch {
            return "\(error)"
        }
        _ = waitUntil(60) { box.get() != nil }
        return box.get()
    }

    /// An editor command, waited for, then the editor re-reads the engine.
    @discardableResult
    func run(_ e: Editor, _ cmd: String, _ params: [String: any Sendable] = [:]) -> String? {
        let status = Self.runSync(e.document, cmd, params)
        e.refresh()
        return status
    }

    let stroke: [String: any Sendable] = ["color": "2080f0c0", "width": 30.0, "points": "40 40 1 300 200 1 480 60 1"]

    // MARK: Tests

    func testUIScenarioEqualsEngineScenarioByteForByte() throws {
        let wc = try fx.open()
        let e = wc.editor
        // UI: Add, Add, choose the new top layer, Delete it.
        layers.buttons["layer.add"]!.performClick(nil)
        waitSteps(e, 2, "add")
        layers.addButton.performClick(nil)
        waitSteps(e, 3, "add again")
        click(try row(2))
        XCTAssertTrue(waitUntil { e.info.activeLayer == 2 })
        layers.deleteButton.performClick(nil)
        waitSteps(e, 4, "delete")
        // Choose "Layer 2", paint on it, duplicate it, move the copy down.
        click(try row(1))
        XCTAssertTrue(waitUntil { e.info.activeLayer == 1 })
        run(e, "tool.stroke", stroke)
        layers.buttons["layer.duplicate"]!.performClick(nil)
        waitSteps(e, 6, "duplicate")
        layers.buttons["layer.move_down"]!.performClick(nil)
        waitSteps(e, 7, "move down")
        // Hide the top layer with its eye.
        try row(2).eye.performClick(nil)
        waitSteps(e, 8, "hide")
        // Layer Properties on the active layer (the copy, at 1) through its controls.
        XCTAssertEqual(e.info.activeLayer, 1)
        layers.buttons["layer.properties"]!.performClick(nil)
        let dialog = try XCTUnwrap(LayerPropertiesController.current(for: e))
        XCTAssertEqual(dialog.index, 1)
        dialog.nameField.stringValue = "Glow copy"
        dialog.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: dialog.nameField))
        dialog.blendPopup.selectItem(withTitle: "Glow")
        _ = dialog.blendPopup.sendAction(dialog.blendPopup.action, to: dialog.blendPopup.target)
        dialog.opacitySlider.integerValue = 180
        _ = dialog.opacitySlider.sendAction(dialog.opacitySlider.action, to: dialog.opacitySlider.target)
        XCTAssertEqual(dialog.opacityField.integerValue, 180, "the field follows the slider")
        dialog.okButton.performClick(nil)
        waitSteps(e, 9, "properties")
        // Drag the bottom row (Background) above the top row.
        drag(try row(0), toListY: 2)
        waitSteps(e, 10, "drag")
        // Show "Layer 2" again, choose it and merge it down; then move up.
        let l2 = try XCTUnwrap(e.info.layers.firstIndex { $0.name == "Layer 2" })
        try row(l2).eye.performClick(nil)
        waitSteps(e, 11, "show")
        click(try row(l2))
        XCTAssertTrue(waitUntil { e.info.activeLayer == l2 })
        layers.buttons["layer.merge_down"]!.performClick(nil)
        waitSteps(e, 12, "merge down")
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let layersMenu = try XCTUnwrap(menu.item(withTitle: "Layers")?.submenu)
        let up = try XCTUnwrap(layersMenu.item(withTitle: "Move Layer Up"))
        XCTAssertTrue(wc.validateMenuItem(up))
        wc.runCommand(up)
        waitSteps(e, 13, "move up from the menu")

        // The same commands sent straight to a second copy of the image.
        let doc = try fx.engine.openPNG(path: fixtures.appendingPathComponent("plasma_alpha_512x384.png").path)
        defer { try? doc.close() }
        let script: [(String, [String: any Sendable])] = [
            ("layer.add", ["name": "Layer 2"]),
            ("layer.add", ["name": "Layer 3"]),
            ("layer.set_active", ["index": 2]),
            ("layer.delete", [:]),
            ("layer.set_active", ["index": 1]),
            ("tool.stroke", stroke),
            ("layer.duplicate", [:]),
            ("layer.move_down", [:]),
            ("layer.set_visible", ["index": 2, "visible": false]),
            ("layer.properties", ["index": 1, "name": "Glow copy", "visible": true, "blend": "glow", "opacity": 180]),
            ("layer.move", ["from": 0, "to": 2]),
            ("layer.set_visible", ["index": l2, "visible": true]),
            ("layer.set_active", ["index": l2]),
            ("layer.merge_down", [:]),
            ("layer.move_up", [:]),
        ]
        for (cmd, params) in script {
            XCTAssertEqual(Self.runSync(doc, cmd, params), "ok", cmd)
        }
        XCTAssertEqual(try Self.visible(e.document), try Self.visible(doc))
        XCTAssertEqual(try e.document.pixels(), try doc.pixels(), "composite bytes")
        let ui = try Self.layerBytes(e.document), engine = try Self.layerBytes(doc)
        XCTAssertEqual(ui.count, 2)
        XCTAssertEqual(ui, engine, "every layer's bytes")
        XCTAssertEqual(e.info.layers.map(\.name), ["Background", "Glow copy"])
        XCTAssertEqual(e.info.historyLabels, ["Open", "layer.add", "layer.add", "layer.delete", "Paintbrush",
                                              "Duplicate Layer", "Move Layer Down", "Hide Layer", "Layer Properties",
                                              "Move Layer", "Show Layer", "Merge Layer Down", "Move Layer Up"])
    }

    func testLayerPropertiesCancelChangesNothingAndOKIsOneStep() throws {
        let wc = try fx.open()
        let e = wc.editor
        run(e, "layer.duplicate")
        let before = try Self.visible(e.document)
        let pixels = try e.document.pixels()
        let bytes = try Self.layerBytes(e.document)
        let edited = wc.window!.isDocumentEdited
        let dialog = try XCTUnwrap(wc.showLayerProperties(index: 1))
        XCTAssertEqual(FocusRouter.target(in: wc.window), .dialog)
        XCTAssertFalse(layers.buttons["layer.add"]!.isEnabled, "panel commands are off while the sheet is up")
        XCTAssertEqual(dialog.blendPopup.numberOfItems, 14, "all 14 blend modes")
        XCTAssertEqual(dialog.blendPopup.itemTitles, LayerBlend.modes.map(\.title))
        for (i, mode) in ["difference", "screen", "multiply"].enumerated() {
            dialog.set(.init(name: "Changed \(i)", visible: i != 1, blend: mode, opacity: 40 + 50 * i))
        }
        // The change shows live: the preview differs, the document does not.
        XCTAssertTrue(waitUntil { (try? e.document.pixels(preview: true)) != pixels }, "preview shows the change")
        XCTAssertEqual(try e.document.pixels(), pixels)
        dialog.cancelButton.performClick(nil)
        XCTAssertNil(wc.window?.attachedSheet)
        XCTAssertTrue(waitUntil { (try? e.document.pixels(preview: true)) == pixels }, "the preview is gone")
        drain(e)
        XCTAssertEqual(try Self.visible(e.document), before, "Cancel leaves layers and history unchanged")
        XCTAssertEqual(try e.document.pixels(), pixels)
        XCTAssertEqual(try Self.layerBytes(e.document), bytes)
        XCTAssertEqual(wc.window!.isDocumentEdited, edited)
        XCTAssertTrue(layers.buttons["layer.add"]!.isEnabled, "panel commands are back")

        // OK after 100 rapid changes: one history step, the last values.
        let again = try XCTUnwrap(wc.showLayerProperties(index: 1))
        for o in 0..<100 { again.set(.init(name: "Fast", visible: true, blend: "overlay", opacity: 255 - o)) }
        again.okButton.performClick(nil)
        waitSteps(e, 3, "one step")
        XCTAssertEqual(e.info.historyLabels.last, "Layer Properties")
        let l = e.info.layers[1]
        XCTAssertEqual([l.name, l.blend, "\(l.opacity)"], ["Fast", "overlay", "156"])
        // OK with nothing changed adds nothing.
        let same = try XCTUnwrap(wc.showLayerProperties(index: 1))
        same.okButton.performClick(nil)
        drain(e)
        XCTAssertEqual(e.info.historyLabels.count, 3)
        // Changed and changed back, then OK: also nothing.
        let back = try XCTUnwrap(wc.showLayerProperties(index: 1))
        back.set(.init(name: "Fast", visible: true, blend: "overlay", opacity: 3))
        back.set(back.original)
        back.okButton.performClick(nil)
        drain(e)
        XCTAssertEqual(e.info.historyLabels.count, 3)
        XCTAssertTrue(waitUntil { (try? e.document.pixels(preview: true)) == (try? e.document.pixels()) })
    }

    func testThumbnailsUpdateAfterAnEditAndAfterAJump() throws {
        let wc = try fx.open()
        let e = wc.editor
        XCTAssertTrue(waitUntil { (try? self.row(0).thumb.image) != nil }, "first thumbnail")
        let first = try XCTUnwrap(try row(0).thumb.image)
        XCTAssertEqual([first.width, first.height], [80, 60], "512 x 384 into 80 x 80 pixels")
        let firstRevision = try row(0).docLayer.revision
        run(e, "tool.stroke", stroke)
        XCTAssertNotEqual(e.info.layers[0].revision, firstRevision, "the edit changed the layer's key")
        XCTAssertTrue(waitUntil {
            guard let r = try? self.row(0), let img = r.thumb.image else { return false }
            return r.docLayer.revision != firstRevision && img !== first
        }, "thumbnail after the edit")
        let painted = try XCTUnwrap(try row(0).thumb.image)
        XCTAssertNotEqual(png(painted), png(first), "the new thumbnail shows the stroke")
        let renders = layers.thumbnails.renders
        // Jump back to Open in the History panel: the cached first image comes back.
        history.layoutSubtreeIfNeeded()
        click(history.historyRows[0])
        XCTAssertTrue(waitUntil { e.info.historyIndex == 0 })
        XCTAssertTrue(waitUntil { (try? self.row(0).thumb.image) === first }, "thumbnail after the jump")
        XCTAssertEqual(layers.thumbnails.renders, renders, "a jump back is a cache hit")
        // And forward again: the painted one, also cached.
        e.run("edit.redo")
        XCTAssertTrue(waitUntil { (try? self.row(0).thumb.image) === painted })
        XCTAssertEqual(layers.thumbnails.renders, renders)
    }

    func testSelectingALayerAddsNoHistoryStepAndDoesNotModify() throws {
        let wc = try fx.open()
        let e = wc.editor
        run(e, "layer.add", ["name": "Top"])
        // Back to the opened state with the new layer as a redo step: clean.
        e.run("edit.undo")
        XCTAssertTrue(waitUntil { !e.info.dirty && !wc.window!.isDocumentEdited })
        e.run("edit.redo")
        XCTAssertTrue(waitUntil { e.info.dirty })
        let steps = e.info.historyLabels
        for target in [0, 1, 0] {
            click(try row(target))
            XCTAssertTrue(waitUntil { e.info.activeLayer == target })
            XCTAssertEqual(e.info.historyLabels, steps, "no Select Layer step")
            XCTAssertEqual(history.historyRows.count, steps.count)
        }
        // On a clean document choosing a layer keeps it clean.
        e.run("edit.undo")
        XCTAssertTrue(waitUntil { !e.info.dirty })
        click(try row(0))
        drain(e)
        XCTAssertFalse(e.info.dirty)
        XCTAssertFalse(wc.window!.isDocumentEdited)
        // The selected row has the edge marker; Go to Layer from the menu agrees.
        e.run("edit.redo")
        XCTAssertTrue(waitUntil { e.info.layers.count == 2 })
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let top = try XCTUnwrap(menu.item(withTitle: "Layers")?.submenu?.item(withTitle: "Go to Top Layer"))
        wc.goToLayer(top)
        XCTAssertTrue(waitUntil { e.info.activeLayer == 1 })
        XCTAssertTrue(waitUntil { (try? self.row(1).isCurrent) == true })
        XCTAssertFalse(try row(0).isCurrent)
        XCTAssertTrue(try row(1).isAccessibilitySelected())
        XCTAssertEqual(e.info.historyLabels.count, 2)
    }

    func testHistoryPanelListsStepsMarksUndoneAndJumps() throws {
        let wc = try fx.open()
        let e = wc.editor
        run(e, "selection.select_all")
        run(e, "tool.stroke", stroke)
        run(e, "layer.duplicate")
        e.run("edit.undo")
        XCTAssertTrue(waitUntil { e.info.historyIndex == 2 })
        XCTAssertTrue(waitUntil { self.history.historyRows.count == 4 })
        let rows = history.historyRows
        XCTAssertEqual(rows.map(\.titleLabel.stringValue), ["Open", "Select All", "Paintbrush", "Duplicate Layer"])
        XCTAssertEqual(rows.map(\.isCurrent), [false, false, true, false])
        XCTAssertEqual(rows.map(\.undone), [false, false, false, true])
        XCTAssertEqual(rows.map { $0.accessibilityValue() as? String }, ["done", "done", "current", "undone"])
        for r in rows { XCTAssertNotNil(r.icon.image, "\(r.titleLabel.stringValue) has an icon") }
        XCTAssertEqual(rows[3].redoMark.superview, rows[3], "the undone step has the redo arrow")
        XCTAssertNil(rows[2].redoMark.superview)
        history.layoutSubtreeIfNeeded()
        XCTAssertEqual(rows[3].titleLabel.textColor, Theme.muted, "undone text is muted, not faded")
        XCTAssertEqual(rows[3].alphaValue, 1)
        XCTAssertEqual(rows[0].frame.height, 27)
        XCTAssertEqual(HistoryStep.symbol("layer.duplicate"), "plus.square.on.square")
        XCTAssertEqual(HistoryStep.title("layer.add"), "Add New Layer")
        // Undo and Redo buttons follow the position.
        XCTAssertTrue(history.undoButton.isEnabled)
        XCTAssertTrue(history.redoButton.isEnabled)
        XCTAssertGreaterThanOrEqual(history.undoButton.frame.height, 24)
        // Click jumps: to the first step, then to the undone one.
        click(rows[0])
        XCTAssertTrue(waitUntil { e.info.historyIndex == 0 })
        XCTAssertTrue(waitUntil { !self.history.undoButton.isEnabled })
        XCTAssertNil(e.selection)
        click(history.historyRows[3])
        XCTAssertTrue(waitUntil { e.info.historyIndex == 3 })
        XCTAssertEqual(e.info.layers.count, 2)
        XCTAssertTrue(waitUntil { !self.history.redoButton.isEnabled })
        history.undoButton.performClick(nil)
        XCTAssertTrue(waitUntil { e.info.historyIndex == 2 })
        history.redoButton.performClick(nil)
        XCTAssertTrue(waitUntil { e.info.historyIndex == 3 })
        XCTAssertEqual(e.info.historyLabels.count, 4, "jumps add no steps")
    }

    func testLayersMenuItemsAndPanelButtonsFollowTheEngine() throws {
        let wc = try fx.open()
        let e = wc.editor
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let m = try XCTUnwrap(menu.item(withTitle: "Layers")?.submenu)
        func item(_ t: String) throws -> NSMenuItem { try XCTUnwrap(m.item(withTitle: t), t) }
        // One layer: nothing to merge, move or delete.
        for t in ["Delete Layer", "Merge Layer Down", "Move Layer Up", "Move Layer Down"] {
            XCTAssertFalse(wc.validateMenuItem(try item(t)), t)
        }
        for id in ["layer.delete", "layer.merge_down", "layer.move_up", "layer.move_down"] {
            XCTAssertFalse(layers.buttons[id]!.isEnabled, id)
        }
        XCTAssertTrue(layers.buttons["layer.properties"]!.isEnabled)
        XCTAssertEqual(layers.buttons.count, 7)
        for b in layers.buttons.values { XCTAssertGreaterThanOrEqual(b.frame.width, 24) }
        let dup = try item("Duplicate Layer")
        XCTAssertTrue(wc.validateMenuItem(dup))
        wc.runCommand(dup)
        waitSteps(e, 2, "duplicate")
        XCTAssertTrue(waitUntil { self.layers.buttons["layer.merge_down"]!.isEnabled })
        XCTAssertFalse(layers.buttons["layer.move_up"]!.isEnabled, "the copy is on top")
        XCTAssertTrue(wc.validateMenuItem(try item("Merge Layer Down")))
        XCTAssertTrue(wc.validateMenuItem(try item("Move Layer Down")))
        // Toggle Layer Visibility hides and shows the active layer.
        let toggle = try item("Toggle Layer Visibility")
        XCTAssertTrue(wc.validateMenuItem(toggle))
        wc.toggleLayerVisibility(toggle)
        XCTAssertTrue(waitUntil { e.info.layers[1].visible == false })
        XCTAssertEqual(try row(1).eye.accessibilityValue() as? String, "hidden")
        XCTAssertEqual(try row(1).detailLabel.stringValue, "Hidden")
        try row(1).eye.performClick(nil)
        XCTAssertTrue(waitUntil { e.info.layers[1].visible })
        // Layer Properties (F4) opens the sheet; the menu is off while it is up.
        let props = try item("Layer Properties…")
        XCTAssertEqual(props.keyEquivalent, String(Character(UnicodeScalar(UInt16(NSF4FunctionKey))!)))
        XCTAssertTrue(wc.validateMenuItem(props))
        wc.showLayerProperties(props)
        let dialog = try XCTUnwrap(LayerPropertiesController.current(for: e))
        XCTAssertNotNil(wc.window?.attachedSheet)
        XCTAssertFalse(wc.validateMenuItem(props))
        XCTAssertFalse(wc.validateMenuItem(dup))
        dialog.cancel(nil)
        XCTAssertTrue(wc.validateMenuItem(props))
        // Double-clicking a row opens Properties for that layer.
        click(try row(0), clicks: 2)
        let forRow = try XCTUnwrap(LayerPropertiesController.current(for: e))
        XCTAssertEqual(forRow.index, 0)
        XCTAssertEqual(forRow.nameField.stringValue, "Background")
        forRow.cancel(nil)
        XCTAssertEqual(e.info.historyLabels.count, 4, "duplicate, hide, show")
    }

    func testDropIndexMath() {
        // Three layers; rows top first: [2, 1, 0]. Gaps 0...3.
        XCTAssertEqual(LayersPanelView.dropIndex(from: 0, gap: 0, count: 3), 2, "bottom to the top")
        XCTAssertEqual(LayersPanelView.dropIndex(from: 2, gap: 3, count: 3), 0, "top to the bottom")
        XCTAssertNil(LayersPanelView.dropIndex(from: 1, gap: 1, count: 3), "onto its own gap")
        XCTAssertNil(LayersPanelView.dropIndex(from: 1, gap: 2, count: 3), "just below itself")
        XCTAssertEqual(LayersPanelView.dropIndex(from: 1, gap: 0, count: 3), 2)
        XCTAssertEqual(LayersPanelView.dropIndex(from: 2, gap: 2, count: 3), 1)
    }

    /// Renders both panels and the Properties dialog, light and dark, into PNGs
    /// for a visual check when TEST_RUNNER_OP_PANEL_PNG_DIR is set.
    func testRenderPanelsForVisualCheck() throws {
        let wc = try fx.open()
        let e = wc.editor
        run(e, "tool.stroke", stroke)
        run(e, "layer.duplicate")
        run(e, "layer.properties", ["index": 1, "name": "Overlay copy", "visible": true,
                                                    "blend": "overlay", "opacity": 128])
        run(e, "layer.add", ["name": "Hidden notes"])
        run(e, "layer.set_visible", ["index": 2, "visible": false])
        run(e, "selection.select_all")
        e.run("edit.undo")  // one undone step to show
        XCTAssertTrue(waitUntil { e.info.historyIndex == 5 })
        XCTAssertTrue(waitUntil { self.layers.layerRows.count == 3 && self.layers.layerRows.allSatisfy { $0.thumb.image != nil } })
        // Sizes from the SPEC.
        let lp = fx.workspace.panels.panels[.layers]!, hp = fx.workspace.panels.panels[.history]!
        XCTAssertEqual(lp.contentRect(forFrameRect: lp.frame).width, 226)
        XCTAssertEqual(try row(0).frame.height, 49)
        XCTAssertEqual(try row(0).eye.frame.size, NSSize(width: 24, height: 24))
        guard let dir = ProcessInfo.processInfo.environment["OP_PANEL_PNG_DIR"] else { return }
        let dialog = try XCTUnwrap(wc.showLayerProperties(index: 1))
        defer { dialog.cancel(nil) }
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            for (panel, view, file) in [(lp, layers as NSView, "layers"), (hp, history as NSView, "history"),
                                        (dialog.window!, dialog.window!.contentView!, "properties")] {
                panel.appearance = NSAppearance(named: appearance)
                view.needsLayout = true
                view.layoutSubtreeIfNeeded()
                for sub in view.subviewsDeep { sub.needsDisplay = true }
                let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
                view.cacheDisplay(in: view.bounds, to: rep)
                let url = URL(fileURLWithPath: dir).appendingPathComponent("m20-\(file)-\(name).png")
                try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: url)
            }
        }
    }

    func png(_ image: CGImage) -> Data? {
        NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
    }
}

final class ResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var value: String?
    func set(_ v: String) { lock.lock(); value = v; lock.unlock() }
    func get() -> String? { lock.lock(); defer { lock.unlock() }; return value }
}

extension NSView {
    var subviewsDeep: [NSView] { subviews + subviews.flatMap(\.subviewsDeep) }
}

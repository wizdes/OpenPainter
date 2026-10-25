// M27 Image > Color Profile: Assign keeps every pixel byte and changes only
// the profile; Convert from sRGB to Display P3 matches op-color's numbers
// (engine/op-engine/tests/profile_dialog.rs) within 1/255; undo restores bytes
// and profile; an ICC file round trip; Cancel runs nothing; a simulated screen
// change rebuilds the display table once and leaves the document alone; the
// status bar shows the profile name; the menu item is on.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class ColorProfileTests: XCTestCase {
    var fx: ShellFixture!
    var temp: URL!

    /// The fixed image of the engine test (4 x 1, straight alpha).
    static let image: [UInt8] = [100, 120, 80, 128, 80, 140, 100, 0, 255, 0, 0, 255, 30, 200, 240, 255]
    /// op-color's sRGB -> Display P3 (relative, black point compensation) of
    /// `image`, as `doc.pixels` shows it (a fully transparent pixel reads 0).
    /// `image` as `doc.pixels` shows it.
    static let visibleImage: [UInt8] = [100, 120, 80, 128, 0, 0, 0, 0, 255, 0, 0, 255, 30, 200, 240, 255]
    static let srgbToP3: [UInt8] = [104, 119, 84, 128, 0, 0, 0, 0, 234, 51, 35, 255, 95, 197, 236, 255]

    override func setUp() async throws {
        fx = try ShellFixture()
        temp = FileManager.default.temporaryDirectory.appendingPathComponent("op-m27-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
    }

    override func tearDown() async throws {
        for wc in fx.controllers { ColorProfileController.current(for: wc.editor)?.cancel(nil) }
        fx.tearDown()
        try? FileManager.default.removeItem(at: temp)
    }

    // MARK: Helpers

    /// A window on the fixed 4 x 1 image.
    func openFixed() throws -> EditorWindowController {
        let doc = try fx.engine.importPixels(width: 4, height: 1, rgba: Self.image)
        let editor = Editor(document: doc, pixelSize: CGSize(width: 4, height: 1), name: "fixed")
        fx.workspace.add(editor)
        let wc = EditorWindowController(editor: editor, workspace: fx.workspace)
        wc.window?.isReleasedWhenClosed = false
        fx.controllers.append(wc)
        wc.present()
        wc.window?.layoutIfNeeded()
        return wc
    }

    func profile(_ e: Editor) throws -> [String: Any] {
        try XCTUnwrap(e.document.query("doc.profile") as? [String: Any])
    }

    func icc(_ e: Editor) throws -> Data { try e.document.queryData("doc.profile_icc") }

    func waitSteps(_ e: Editor, _ n: Int, _ what: String, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(waitUntil(20) { e.info.historyLabels.count == n && e.info.historyIndex == n - 1 },
                      "\(what): \(e.info.historyLabels) @\(e.info.historyIndex) \(e.message ?? "")",
                      file: file, line: line)
    }

    func run(_ e: Editor, _ cmd: String) {
        XCTAssertEqual(LayersHistoryTests.runSync(e.document, cmd), "ok", cmd)
        e.refresh()
    }

    func dialog(_ wc: EditorWindowController) throws -> ColorProfileController {
        let d = try XCTUnwrap(wc.showColorProfile(), "the dialog opens")
        XCTAssertEqual(FocusRouter.target(in: wc.window), .dialog)
        return d
    }

    func target(_ title: String) -> ColorProfileController.Target {
        ColorProfileController.builtins.first { $0.title == title }!
    }

    // MARK: Tests

    func testDialogShowsTheCurrentProfileAndTheChoices() throws {
        let wc = try openFixed()
        let d = try dialog(wc)
        XCTAssertEqual(d.currentName.stringValue, "None, treated as sRGB")
        XCTAssertTrue(d.currentDetail.stringValue.contains("RGB"))
        XCTAssertEqual(Array(d.targetPopup.itemTitles.prefix(4)), ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"])
        XCTAssertEqual(d.targetPopup.itemTitles.last, "ICC File…")
        XCTAssertEqual(d.intentPopup.itemTitles,
                       ["Perceptual", "Relative colorimetric", "Saturation", "Absolute colorimetric"])
        XCTAssertEqual(d.values.target.title, "Display P3", "starts on a profile other than the current one")
        XCTAssertTrue(d.intentPopup.isEnabled)
        d.assignRadio.performClick(nil)
        XCTAssertEqual(d.values.action, .assign)
        XCTAssertFalse(d.intentPopup.isEnabled, "intent is for Convert only")
        XCTAssertFalse(d.blackPointBox.isEnabled)
        d.convertRadio.performClick(nil)
        XCTAssertTrue(d.intentPopup.isEnabled)
        d.intentPopup.selectItem(at: 1)
        d.intentPopup.sendAction(d.intentPopup.action, to: d.intentPopup.target)
        d.blackPointBox.performClick(nil)
        XCTAssertEqual(d.command.id, "image.convert_profile")
        XCTAssertEqual(d.command.params["intent"] as? String, "relative")
        XCTAssertEqual(d.command.params["bpc"] as? Bool, true)
        XCTAssertEqual(d.command.params["profile"] as? String, "display-p3")
    }

    func testAssignChangesNoPixelBytesAndOnlyTheProfile() throws {
        let wc = try fx.open()
        let e = wc.editor
        let pixels = try e.document.pixels()
        let layers = try LayersHistoryTests.layerBytes(e.document)
        let steps = e.info.historyLabels.count
        let d = try dialog(wc)
        var v = d.values
        v.action = .assign
        v.target = target("Display P3")
        d.set(v)
        d.okButton.performClick(nil)
        XCTAssertNil(wc.window?.attachedSheet)
        waitSteps(e, steps + 1, "one step")
        XCTAssertEqual(e.info.historyCommands.last, "image.assign_profile")
        XCTAssertEqual(try e.document.pixels(), pixels, "composite bytes unchanged")
        XCTAssertEqual(try LayersHistoryTests.layerBytes(e.document), layers, "layer bytes unchanged")
        XCTAssertEqual(try profile(e)["description"] as? String, "Display P3")
        XCTAssertEqual(try profile(e)["embedded"] as? Bool, true)
        XCTAssertEqual([e.info.width, e.info.height, e.info.layers.count], [512, 384, 1])
        // Undo: the profile goes back, the bytes stay.
        wc.undo()
        XCTAssertTrue(waitUntil { e.info.historyIndex == steps - 1 })
        XCTAssertEqual(try profile(e)["embedded"] as? Bool, false)
        XCTAssertEqual(try icc(e), Data())
        XCTAssertEqual(try e.document.pixels(), pixels)
    }

    func testConvertMatchesEngineNumbersAndUndoRestoresBytesAndProfile() throws {
        let wc = try openFixed()
        let e = wc.editor
        let original = try e.document.pixels()
        let d = try dialog(wc)
        d.set(.init(action: .convert, target: target("Display P3"), intent: "relative", blackPoint: true))
        d.okButton.performClick(nil)
        waitSteps(e, 2, "one step")
        XCTAssertEqual(e.info.historyCommands.last, "image.convert_profile")
        let converted = try e.document.pixels()
        XCTAssertEqual(converted.count, Self.srgbToP3.count)
        for (i, (a, b)) in zip(converted, Self.srgbToP3).enumerated() {
            XCTAssertLessThanOrEqual(abs(Int(a) - Int(b)), 1, "byte \(i): \(a) vs op-color \(b)")
        }
        XCTAssertNotEqual(converted, original)
        XCTAssertEqual(try profile(e)["description"] as? String, "Display P3")
        XCTAssertEqual(wc.statusBar.profileValue.stringValue, "Display P3")
        wc.undo()
        XCTAssertTrue(waitUntil { e.info.historyIndex == 0 })
        XCTAssertEqual(try e.document.pixels(), original, "undo restores the bytes")
        XCTAssertEqual(try profile(e)["embedded"] as? Bool, false, "and the profile")
        XCTAssertEqual(try icc(e), Data())
        XCTAssertTrue(waitUntil { wc.statusBar.profileValue.stringValue == "sRGB (none)" })
        wc.redo()
        XCTAssertTrue(waitUntil { e.info.historyIndex == 1 })
        XCTAssertEqual(try e.document.pixels(), converted)
    }

    func testICCFileRoundTripAndABadFileChangesNothing() throws {
        let wc = try openFixed()
        let e = wc.editor
        let p3 = p3ICC()
        let url = temp.appendingPathComponent("Display P3.icc")
        try p3.write(to: url)
        let d = try dialog(wc)
        // A file that is not an ICC profile: an error, the target stays.
        let bad = temp.appendingPathComponent("bad.icc")
        try Data("not a profile".utf8).write(to: bad)
        let before = d.values
        XCTAssertFalse(d.loadFile(bad))
        XCTAssertEqual(d.values, before)
        XCTAssertTrue(d.note.stringValue.contains("not usable"), d.note.stringValue)
        // A good file: assign it, then the document's ICC bytes equal the file.
        XCTAssertTrue(d.loadFile(url))
        XCTAssertTrue(d.targetPopup.titleOfSelectedItem?.hasPrefix("File: Display P3.icc") == true,
                      d.targetPopup.titleOfSelectedItem ?? "")
        d.assignRadio.performClick(nil)
        XCTAssertTrue((d.command.params["profile"] as? String)?.hasPrefix("base64:") == true)
        d.okButton.performClick(nil)
        waitSteps(e, 2, "assign file")
        XCTAssertEqual(try icc(e), p3, "file bytes stored exactly")
        XCTAssertEqual(try e.document.pixels(), Self.visibleImage, "assign keeps the bytes")
        // Out again: the stored profile written to a file reads back the same.
        let out = temp.appendingPathComponent("out.icc")
        try icc(e).write(to: out)
        XCTAssertEqual(try Data(contentsOf: out), p3)
        let again = try dialog(wc)
        XCTAssertEqual(again.currentName.stringValue, e.info.profileDescription)
        XCTAssertTrue(again.loadFile(out))
        again.set(.init(action: .convert, target: again.values.target, intent: "relative", blackPoint: true))
        again.okButton.performClick(nil)
        waitSteps(e, 3, "convert to the same profile")
        XCTAssertEqual(try icc(e), p3)
        // Converting a profile into itself leaves the colors in place.
        let px = try e.document.pixels()
        XCTAssertEqual(px.count, Self.visibleImage.count)
        for i in px.indices { XCTAssertLessThanOrEqual(abs(Int(px[i]) - Int(Self.visibleImage[i])), 1, "byte \(i)") }
    }

    func testCancelRunsNothing() throws {
        let wc = try openFixed()
        let e = wc.editor
        let pixels = try e.document.pixels()
        let d = try dialog(wc)
        d.set(.init(action: .convert, target: target("ProPhoto RGB"), intent: "saturation", blackPoint: true))
        d.cancelButton.performClick(nil)
        XCTAssertNil(wc.window?.attachedSheet)
        // Wait for anything queued, then re-read (a cancel with no preview is refused in order).
        _ = LayersHistoryTests.runSync(e.document, "preview.cancel")
        e.refresh()
        XCTAssertEqual(e.info.historyLabels.count, 1)
        XCTAssertEqual(try e.document.pixels(), pixels)
        XCTAssertEqual(try profile(e)["embedded"] as? Bool, false)
        XCTAssertFalse(e.info.dirty)
    }

    func testScreenChangeRebuildsTheTableOnceAndLeavesTheDocumentAlone() throws {
        let wc = try fx.open()
        let e = wc.editor
        let canvas = wc.canvas
        XCTAssertTrue(waitUntil(20) { (try? canvas.engineView?.info().frames ?? 0) ?? 0 > 0 }, "first frame")
        let view = try XCTUnwrap(canvas.engineView)
        var info = try view.info()
        let docBefore = e.info
        let pixels = try e.document.pixels()
        let builds = info.tableBuilds
        func waitFrame(after n: UInt64) -> Bool { waitUntil(10) { ((try? view.info().frames) ?? 0) > n } }
        // Simulated move to a Display P3 screen.
        canvas.displayICCOverride = p3ICC()
        NotificationCenter.default.post(name: NSWindow.didChangeScreenNotification, object: wc.window)
        XCTAssertTrue(waitFrame(after: info.frames))
        info = try view.info()
        XCTAssertEqual(info.tableBuilds, builds + 1, "rebuilt once for the new screen")
        XCTAssertEqual(canvas.metalLayer.colorspace?.copyICCData() as Data?, p3ICC(), "layer retagged")
        XCTAssertTrue(info.layerTagIsDisplay)
        // The same screen again: no second build.
        NotificationCenter.default.post(name: NSWindow.didChangeScreenProfileNotification, object: wc.window)
        XCTAssertTrue(waitFrame(after: info.frames))
        XCTAssertEqual(try view.info().tableBuilds, builds + 1)
        // The document did not change: no step, same revision, same bytes, not dirty.
        e.refresh()
        XCTAssertEqual(e.info, docBefore)
        XCTAssertEqual(try e.document.pixels(), pixels)
        XCTAssertFalse(wc.window!.isDocumentEdited)
        // The dialog offers this display's profile as a target.
        let d = try dialog(wc)
        XCTAssertTrue(d.targetPopup.itemTitles.contains { $0.hasPrefix("This display") })
        d.cancel(nil)
    }

    func testStatusBarShowsTheProfileNameAndTheMenuItemIsOn() throws {
        let wc = try openFixed()
        let e = wc.editor
        XCTAssertEqual(wc.statusBar.profileValue.stringValue, "sRGB (none)")
        XCTAssertEqual(wc.statusBar.profileValue.toolTip, "None, treated as sRGB")
        // The Image menu item is built with an action and is on.
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let item = try XCTUnwrap(menu.item(withTitle: "Image")?.submenu?.item(withTitle: "Color Profile…"))
        XCTAssertEqual(item.action, #selector(ColorProfileMenuActions.showColorProfile(_:)))
        XCTAssertTrue(wc.validateMenuItem(item))
        let d = try dialog(wc)
        XCTAssertFalse(wc.validateMenuItem(item), "off while a sheet is up")
        d.set(.init(action: .assign, target: target("Adobe RGB (1998)"), intent: "perceptual", blackPoint: false))
        d.ok(nil)
        XCTAssertTrue(waitUntil { wc.statusBar.profileValue.stringValue == "Adobe RGB (1998)" },
                      wc.statusBar.profileValue.stringValue)
        XCTAssertTrue(wc.validateMenuItem(item))
        XCTAssertEqual(e.info.profileDescription, "Adobe RGB (1998)")
        // Long names are cut for the status bar; the tooltip keeps the whole name.
        var long = DocInfo()
        long.profileEmbedded = true
        long.profileDescription = "A Very Long Monitor Calibration Profile"
        XCTAssertEqual(ColorProfileNames.short(long), "A Very Long Monit…")
        XCTAssertEqual(ColorProfileNames.full(long), long.profileDescription)
    }

    /// With OP_PANEL_PNG_DIR set, writes the dialog and the status bar in light
    /// and dark for a visual check.
    func testRenderDialogForVisualCheck() throws {
        guard let dir = ProcessInfo.processInfo.environment["OP_PANEL_PNG_DIR"] else { return }
        let wc = try openFixed()
        let d = try dialog(wc)
        func render(_ view: NSView, _ window: NSWindow, _ file: String) throws {
            for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
                window.appearance = NSAppearance(named: appearance)
                view.needsLayout = true
                view.layoutSubtreeIfNeeded()
                for sub in view.subviewsDeep { sub.needsDisplay = true }
                let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
                view.cacheDisplay(in: view.bounds, to: rep)
                let url = URL(fileURLWithPath: dir).appendingPathComponent("m27-\(file)-\(name).png")
                try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: url)
            }
        }
        try render(d.window!.contentView!, d.window!, "convert")
        d.assignRadio.performClick(nil)
        try render(d.window!.contentView!, d.window!, "assign")
        let bad = temp.appendingPathComponent("bad.icc")
        try Data("x".utf8).write(to: bad)
        d.loadFile(bad)
        try render(d.window!.contentView!, d.window!, "error")
        d.set(.init(action: .assign, target: target("Display P3"), intent: "perceptual", blackPoint: false))
        d.ok(nil)
        XCTAssertTrue(waitUntil { wc.statusBar.profileValue.stringValue == "Display P3" })
        wc.window!.layoutIfNeeded()
        try render(wc.statusBar, wc.window!, "status")
    }
}

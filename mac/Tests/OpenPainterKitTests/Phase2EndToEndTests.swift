// M28 Phase 2 end to end. One flow through the real document class, window,
// menus, tool options, effect dialogs, History panel and save sheet: open a
// JPEG with EXIF orientation 6 and a Display P3 profile; crop at 3:2 with a 4
// degree straighten; Curves; Hue / Saturation; click History two steps back,
// then one forward; Save As JPEG (quality 95, 4:2:2) and .opaint; reopen both.
//
// The same steps are the committed engine scenario
// tests/scenarios/m28-phase2.json, which this test runs with `opcli scenario`
// (built by `make -C mac test`). The window's result must equal it:
// - after each stage, the window's pixels equal opcli's run of the steps so
//   far, byte for byte (so a difference names its stage);
// - the reopened .opaint equals opcli's .opaint byte for byte (pixels,
//   profile and resolution);
// - the JPEG: both sides use the same encoder (the engine's op_encode_image,
//   jpeg-encoder), so the entropy-coded image data (from the first DQT marker
//   to the end) is byte-equal, and the decoded pixels are equal (PSNR
//   infinite). Only the APP segments differ: the app keeps the source EXIF.
//
// The app decodes JPEG with ImageIO, opcli with the engine's decoder; they
// differ by up to 3 levels on the fixture. So the scenario starts from
// ImageIO's decode, committed as tests/fixtures/m28/*.imageio.png, and this
// test first checks that the app's open gives exactly those bytes.
//
// XCUITest is not available on this Mac (Automation Mode is off), so this is
// an in-process XCTest, as the earlier milestones' tests are. Each stage
// attaches a rendered window image (XCTAttachment, kept always). The Metal
// canvas cannot be captured in a test, so the image draws the engine's
// composite into the canvas area of the rendered window; panels and sheets
// are rendered on their own. With OP_RENDER_DIR set the images are also
// written there as m28-*.png.
import AppKit
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class Phase2EndToEndTests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var docs: [ImageDocument] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedPrompt: ImageDocument.SaveOptionsPrompt!
    var savedFlatten: ImageDocument.FlattenPrompt!
    var savedStore: SaveOptionsStore!
    var suite: String!
    var flattenAsked: [String] = []

    static let repo = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().appendingPathComponent("../../..").standardizedFileURL
    static let scenarioURL = repo.appendingPathComponent("tests/scenarios/m28-phase2.json")
    static let opcli = repo.appendingPathComponent("engine/target/release/opcli")

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedPrompt = ImageDocument.saveOptionsPrompt
        savedFlatten = ImageDocument.flattenPrompt
        savedStore = ImageDocument.saveOptionsStore
        suite = "OpenPainterM28.\(UUID().uuidString)"
        ImageDocument.saveOptionsStore = SaveOptionsStore(defaults: UserDefaults(suiteName: suite)!)
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        // The real save options sheet (the app's default prompt).
        ImageDocument.saveOptionsPrompt = { doc, type, options, done in
            ImageDocument.showSaveOptions(doc, type, options, done)
        }
        ImageDocument.flattenPrompt = { [unowned self] _, type, done in
            flattenAsked.append(type)
            done(false)
        }
        EffectDialogController.lastEffect = nil
        EffectDialogController.lastUsed = [:]
    }

    override func tearDown() async throws {
        ImageDocument.openSaveOptions?.cancel(nil)
        for d in docs {
            if let e = d.editor { EffectDialogController.current(for: e)?.cancel(nil) }
            for wc in d.windowControllers { wc.window?.orderOut(nil) }
            d.close()
        }
        docs.removeAll()
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        ImageDocument.saveOptionsPrompt = savedPrompt
        ImageDocument.flattenPrompt = savedFlatten
        ImageDocument.saveOptionsStore = savedStore
        UserDefaults().removePersistentDomain(forName: suite)
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
    }

    // MARK: The scenario file

    struct Scenario {
        var json: [String: Any]
        var steps: [[String: Any]]
        var inputPath: URL
        /// ImageIO's decode of the input (the app's JPEG decoder), as a PNG.
        var pixelsPath: URL
        var dpi: Double

        init(_ url: URL) throws {
            json = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
            steps = try XCTUnwrap(json["steps"] as? [[String: Any]])
            let input = try XCTUnwrap(json["input"] as? [String: Any])
            inputPath = url.deletingLastPathComponent()
                .appendingPathComponent(try XCTUnwrap(input["path"] as? String)).standardizedFileURL
            pixelsPath = url.deletingLastPathComponent()
                .appendingPathComponent(try XCTUnwrap(input["pixels"] as? String)).standardizedFileURL
            dpi = (input["dpi"] as? NSNumber)?.doubleValue ?? 96
        }

        func params(_ i: Int) throws -> [String: Any] { try XCTUnwrap(steps[i]["params"] as? [String: Any]) }
        func command(_ i: Int) -> String? { steps[i]["command"] as? String }
    }

    /// Runs `opcli scenario` on `script` into `out` and returns its summary.
    func opcli(_ script: URL, out: URL) throws -> [String: Any] {
        guard FileManager.default.isExecutableFile(atPath: Self.opcli.path) else {
            XCTFail("\(Self.opcli.path) is missing: run `make -C mac test`, which builds it (scripts/build-opcli.sh)")
            throw CocoaError(.fileNoSuchFile)
        }
        let p = Process()
        p.executableURL = Self.opcli
        p.arguments = ["scenario", script.path, "-o", out.path]
        let stdout = Pipe(), stderr = Pipe()
        p.standardOutput = stdout
        p.standardError = stderr
        try p.run()
        let outData = stdout.fileHandleForReading.readDataToEndOfFile()
        let errData = stderr.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let err = String(decoding: errData, as: UTF8.self)
        XCTAssertEqual(p.terminationStatus, 0, "opcli failed: \(err)")
        guard p.terminationStatus == 0 else { throw CocoaError(.executableLoad) }
        return try XCTUnwrap(try JSONSerialization.jsonObject(with: outData) as? [String: Any], err)
    }

    /// opcli's flattened pixels after the scenario's first `count` steps.
    func opcliPixels(_ s: Scenario, steps count: Int) throws -> (width: Int, height: Int, rgba: [UInt8]) {
        var json = s.json
        json["steps"] = Array(s.steps.prefix(count))
        json["expect"] = nil
        var input = try XCTUnwrap(s.json["input"] as? [String: Any])
        input["path"] = s.inputPath.path
        input["pixels"] = s.pixelsPath.path
        json["input"] = input
        json["outputs"] = [["path": "stage.png", "format": "png"]]
        let stage = dir.appendingPathComponent("stage-\(count)")
        let script = dir.appendingPathComponent("stage-\(count).json")
        try JSONSerialization.data(withJSONObject: json).write(to: script)
        _ = try opcli(script, out: stage)
        let d = try fx.engine.openPNG(path: stage.appendingPathComponent("stage.png").path)
        defer { try? d.close() }
        let i = try d.info()
        return ((i["width"] as! NSNumber).intValue, (i["height"] as! NSNumber).intValue, try d.pixels())
    }

    // MARK: Helpers

    func editor(_ doc: ImageDocument) throws -> Editor { try XCTUnwrap(doc.editor) }
    func controller(_ doc: ImageDocument) throws -> EditorWindowController {
        try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
    }

    /// Opens `url` as the app does (NSDocument's init reads it) and shows its window.
    func open(_ url: URL, type: String) throws -> ImageDocument {
        let doc = try ImageDocument(contentsOf: url, ofType: type)
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        try controller(doc).window?.layoutIfNeeded()
        try controller(doc).updateClearRect()
        return doc
    }

    func same(_ a: [UInt8], _ b: [UInt8], _ what: String, file: StaticString = #filePath, line: UInt = #line) {
        let firstDiff = zip(a, b).enumerated().first { $0.element.0 != $0.element.1 }?.offset
        XCTAssertTrue(a == b, "\(what): bytes differ (\(a.count) vs \(b.count), first at \(firstDiff.map(String.init) ?? "-"))",
                      file: file, line: line)
    }

    /// The stage's pixels must equal opcli's run of the first `steps` steps.
    func assertStage(_ e: Editor, _ s: Scenario, steps: Int, _ name: String,
                     file: StaticString = #filePath, line: UInt = #line) throws {
        let want = try opcliPixels(s, steps: steps)
        XCTAssertEqual(e.info.width, want.width, "\(name) width", file: file, line: line)
        XCTAssertEqual(e.info.height, want.height, "\(name) height", file: file, line: line)
        same(try e.document.pixels(), want.rgba, "\(name) equals opcli after \(steps) steps", file: file, line: line)
    }

    func mouse(_ type: NSEvent.EventType, _ view: NSView) -> NSEvent {
        let c = NSPoint(x: view.bounds.midX, y: view.bounds.midY)
        return NSEvent.mouseEvent(
            with: type, location: view.convert(c, to: nil), modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: view.window?.windowNumber ?? 0,
            context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
    }

    func click(_ view: NSView) {
        view.mouseDown(with: mouse(.leftMouseDown, view))
        view.mouseUp(with: mouse(.leftMouseUp, view))
    }

    /// Picks a popup item and sends its action, as a click on the item does.
    func choose(_ popup: NSPopUpButton, _ title: String, file: StaticString = #filePath, line: UInt = #line) {
        let i = popup.indexOfItem(withTitle: title)
        XCTAssertGreaterThanOrEqual(i, 0, "\(title) in \(popup.itemTitles)", file: file, line: line)
        popup.selectItem(at: i)
        popup.sendAction(popup.action, to: popup.target)
    }

    /// Types into a field and ends editing with its action (Return).
    func type(_ field: NSTextField, _ text: String) {
        field.stringValue = text
        field.sendAction(field.action, to: field.target)
    }

    /// The real menu bar item `path` (menu titles, then the item).
    func menuItem(_ path: [String]) throws -> NSMenuItem {
        var menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        for title in path.dropLast() {
            menu = try XCTUnwrap(menu.items.first { $0.title == title }?.submenu, "menu \(title)")
        }
        return try XCTUnwrap(menu.items.first { $0.title == path.last! }, "item \(path)")
    }

    /// Runs an effect menu item: the item's command id goes to the window's
    /// dialog opener, which is what `EffectMenuTarget` does for the key window.
    func openEffect(_ wc: EditorWindowController, _ menu: String, id: String) throws -> EffectDialogController {
        let root = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let items = try XCTUnwrap(root.items.first { $0.title == menu }?.submenu, menu).items
        let item = try XCTUnwrap(items.first { $0.representedObject as? String == id }, "\(menu) has \(id)")
        XCTAssertTrue(wc.validateMenuItem(item), "\(item.title) is on")
        let d = try XCTUnwrap(wc.showEffect(id), "\(id) opens a dialog")
        XCTAssertTrue(wc.window?.attachedSheet === d.window, "\(id) is a sheet on the window")
        return d
    }

    // MARK: Rendered images

    /// The view drawn in `appearance` (its window's appearance is put back after).
    func bitmap(_ view: NSView, _ appearance: NSAppearance.Name) throws -> NSBitmapImageRep {
        let window = view.window
        let before = window?.appearance
        window?.appearance = NSAppearance(named: appearance)
        defer { window?.appearance = before }
        view.needsLayout = true
        view.layoutSubtreeIfNeeded()
        for sub in view.subviewsDeep { sub.needsDisplay = true }
        let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
        NSAppearance(named: appearance)!.performAsCurrentDrawingAppearance {
            view.cacheDisplay(in: view.bounds, to: rep)
        }
        return rep
    }

    static let appearances: [(String, NSAppearance.Name)] = [("light", .aqua), ("dark", .darkAqua)]

    /// The window (toolbar, options row, strip, stage, status bar) with the
    /// engine's composite drawn where the Metal canvas shows it, over the
    /// 8-point checkerboard.
    func windowImage(_ wc: EditorWindowController, _ appearance: NSAppearance.Name) throws -> CGImage {
        let window = try XCTUnwrap(wc.window)
        let root = try XCTUnwrap(window.contentView?.superview ?? window.contentView)
        let rep = try bitmap(root, appearance)
        let scale = CGFloat(rep.pixelsWide) / root.bounds.width
        let ctx = try XCTUnwrap(CGContext(
            data: nil, width: rep.pixelsWide, height: rep.pixelsHigh, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        ctx.scaleBy(x: scale, y: scale)
        ctx.draw(try XCTUnwrap(rep.cgImage), in: root.bounds)
        // The canvas: its document rectangle in root coordinates (y up).
        let canvas = wc.canvas
        let vp = canvas.viewport
        let docRect = root.convert(canvas.convert(CGRect(origin: vp.origin, size: vp.contentSize), to: nil), from: nil)
        let visible = root.convert(canvas.convert(canvas.bounds, to: nil), from: nil)
        ctx.saveGState()
        ctx.clip(to: visible.intersection(docRect))
        for y in stride(from: docRect.minY, to: docRect.maxY, by: 8) {
            for x in stride(from: docRect.minX, to: docRect.maxX, by: 8) {
                let dark = (Int((x - docRect.minX) / 8) + Int((y - docRect.minY) / 8)).isMultiple(of: 2)
                ctx.setFillColor(dark ? CGColor(gray: 0.8, alpha: 1) : CGColor(gray: 1, alpha: 1))
                ctx.fill(CGRect(x: x, y: y, width: 8, height: 8))
            }
        }
        ctx.interpolationQuality = .none
        ctx.draw(try compositeImage(wc.editor), in: docRect)
        ctx.restoreGState()
        return try XCTUnwrap(ctx.makeImage())
    }

    /// The engine's flattened image in the document profile.
    func compositeImage(_ e: Editor) throws -> CGImage {
        let icc = try e.document.queryData("doc.profile_icc")
        return try ImageIOExporter.cgImage(rgba: try e.document.pixels(preview: true), width: e.info.width,
                                           height: e.info.height, icc: icc.isEmpty ? nil : icc)
    }

    func attach(_ image: CGImage, _ name: String) throws {
        let a = XCTAttachment(image: NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height)))
        a.name = "m28-\(name)"
        a.lifetime = .keepAlways
        add(a)
        if let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"] {
            let data = try XCTUnwrap(NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]))
            try data.write(to: URL(fileURLWithPath: out).appendingPathComponent("m28-\(name).png"))
        }
    }

    func attachWindow(_ wc: EditorWindowController, _ name: String) throws {
        for (suffix, a) in Self.appearances { try attach(try windowImage(wc, a), "\(name)-\(suffix)") }
    }

    func attachView(_ view: NSView, _ name: String) throws {
        for (suffix, a) in Self.appearances { try attach(try XCTUnwrap(try bitmap(view, a).cgImage), "\(name)-\(suffix)") }
    }

    /// The JPEG's image data: from the first DQT marker to the end. APP
    /// segments (EXIF, ICC, JFIF) come before it.
    static func imageData(_ jpeg: Data) -> Data? {
        let b = [UInt8](jpeg)
        guard b.count > 4, b[0] == 0xFF, b[1] == 0xD8 else { return nil }
        var i = 2
        while i + 4 <= b.count, b[i] == 0xFF {
            if b[i + 1] == 0xDB { return Data(b[i...]) }
            i += 2 + (Int(b[i + 2]) << 8 | Int(b[i + 3]))
        }
        return nil
    }

    // MARK: Bug found by the flow

    /// The save sheet opens with the quality field focused. Moving the slider
    /// left the field showing the old number, and Return (which runs the
    /// field's action first) then put the old quality back: Save As wrote 50
    /// although the slider said 95.
    func testQualitySliderUpdatesTheFocusedFieldAndReturnKeepsIt() throws {
        let host = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 500, height: 400), styleMask: [.titled],
                            backing: .buffered, defer: false)
        host.isReleasedWhenClosed = false
        defer { host.orderOut(nil) }
        host.makeKeyAndOrderFront(nil)
        var start = SaveOptions()
        start.jpegQuality = 50
        let px = SavePixels(rgba: [UInt8](repeating: 200, count: 16 * 16 * 4), width: 16, height: 16, icc: Data(), dpi: 96)
        let sheet = SaveOptionsController(type: "public.jpeg", options: start, pixels: px, metadata: nil)
        var result: SaveOptions??
        sheet.onClose = { result = .some($0) }
        sheet.present(on: host)
        let window = try XCTUnwrap(sheet.window)
        XCTAssertTrue(window.makeFirstResponder(sheet.qualityField))
        XCTAssertNotNil(sheet.qualityField.currentEditor(), "the field is being edited")
        sheet.qualitySlider.integerValue = 95
        sheet.qualitySlider.sendAction(sheet.qualitySlider.action, to: sheet.qualitySlider.target)
        XCTAssertEqual(sheet.options.jpegQuality, 95)
        XCTAssertEqual(sheet.qualityField.stringValue, "95", "the focused field follows the slider")
        // Return in the field: its action runs, then OK.
        sheet.qualityField.sendAction(sheet.qualityField.action, to: sheet.qualityField.target)
        XCTAssertEqual(sheet.options.jpegQuality, 95, "Return keeps the slider's quality")
        sheet.okButton.performClick(nil)
        XCTAssertEqual(result??.jpegQuality, 95)
        // Typing still works and is not rewritten while typing.
        let again = SaveOptionsController(type: "public.jpeg", options: start, pixels: px, metadata: nil)
        again.present(on: host)
        defer { again.cancel(nil) }
        XCTAssertTrue(try XCTUnwrap(again.window).makeFirstResponder(again.qualityField))
        let editor = try XCTUnwrap(again.qualityField.currentEditor())
        editor.string = "7"
        again.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: again.qualityField))
        XCTAssertEqual(again.options.jpegQuality, 7)
        XCTAssertEqual(again.qualitySlider.integerValue, 7)
        XCTAssertEqual(editor.string, "7", "typing is not rewritten")
    }

    // MARK: The flow

    func testPhase2FlowMatchesTheEngineScenario() throws {
        let s = try Scenario(Self.scenarioURL)
        XCTAssertEqual(s.steps.map { $0["command"] as? String }, [
            "image.crop_rect", "effect.adjust.curves", "effect.adjust.hue_saturation", "history.jump", "history.jump",
        ], "the scenario's steps are the ones this test drives")

        // 1. Open the JPEG through the document class (ImageIO route).
        let source = dir.appendingPathComponent("phase2.jpg")
        try FileManager.default.copyItem(at: s.inputPath, to: source)
        guard case .imageIO = ImageFormats.route(for: source) else { return XCTFail("a JPEG opens through ImageIO") }
        let doc = try open(source, type: "public.jpeg")
        let wc = try controller(doc)
        let e = try editor(doc)
        XCTAssertEqual(e.info.width, 256, "orientation 6 turns the 192 x 256 stored image")
        XCTAssertEqual(e.info.height, 192)
        let opened = try e.document.pixels()
        let marker = (5 * 256 + 250) * 4  // the stored top-left green block, now top-right
        let green = Array(opened[marker..<marker + 4])  // JPEG: within a few levels of pure green
        XCTAssertTrue(green[0] <= 4 && green[1] >= 251 && green[2] <= 4, "the green marker is at the top right: \(green)")
        let profile = try XCTUnwrap(try e.document.query("doc.profile") as? [String: Any])
        XCTAssertEqual(profile["description"] as? String, "Display P3")
        XCTAssertEqual(try DocumentSaver.resolution(e.document), s.dpi, "the scenario imports at the app's resolution")
        XCTAssertEqual(e.info.historyLabels, ["Open"])
        // The app's ImageIO decode is exactly the committed .imageio.png (opcli
        // starts from it), and the profile is the same bytes.
        try assertStage(e, s, steps: 0, "open: the app's decode equals \(s.pixelsPath.lastPathComponent)")
        let pinned = try fx.engine.openPNG(path: s.pixelsPath.path)
        defer { try? pinned.close() }
        XCTAssertEqual(try e.document.queryData("doc.profile_icc"), try pinned.queryData("doc.profile_icc"),
                       "the Display P3 profile bytes")
        try attachWindow(wc, "1-open")

        // 2. Image > Crop Tool: 3:2, straighten 4 degrees, Crop.
        let cropItem = try menuItem(["Image", "Crop Tool"])
        XCTAssertTrue(wc.validateMenuItem(cropItem))
        XCTAssertTrue(NSApp.sendAction(try XCTUnwrap(cropItem.action), to: wc, from: cropItem))
        let crop = try XCTUnwrap(wc.selectCrop)
        XCTAssertTrue(crop.cropActive)
        choose(crop.aspectPopup, "3:2")
        type(crop.angleField, "4")
        XCTAssertEqual(crop.cropAngle, 4)
        let cropParams = try s.params(0)
        for key in ["x", "y", "width", "height"] {
            XCTAssertEqual(crop.cropParams[key] as? Int, (cropParams[key] as? NSNumber)?.intValue, "crop \(key)")
        }
        XCTAssertEqual(crop.cropParams["angle"] as? Double, (cropParams["angle"] as? NSNumber)?.doubleValue)
        try attachView(wc.optionsBar, "2a-crop-options")
        crop.applyButton.performClick(nil)
        XCTAssertTrue(waitUntil(60) { e.info.historyLabels == ["Open", "Crop"] && !crop.busy }, "\(e.info.historyLabels)")
        XCTAssertFalse(crop.cropActive)
        try assertStage(e, s, steps: 1, "crop")
        try attachWindow(wc, "2b-cropped")

        // 3. Adjustments > Curves: the scenario's curve, OK.
        let curvesParams = try s.params(1)
        let curves = try openEffect(wc, "Adjustments", id: "effect.adjust.curves")
        let points = try XCTUnwrap(curvesParams["ControlPoints"] as? [[[Int]]])
        choose(try XCTUnwrap(curves.popups["ColorTransferMode"]), "Luminosity")
        curves.set("ControlPoints", .curve(points))
        XCTAssertEqual(try XCTUnwrap(curves.curvesView).channels, points, "the curve control shows the points")
        XCTAssertTrue(waitUntil(30) { curves.isIdle })
        try attachView(try XCTUnwrap(curves.window?.contentView), "3a-curves-dialog")
        curves.ok(nil)
        XCTAssertTrue(waitUntil(60) { e.info.historyLabels.count == 3 }, e.message ?? "")
        XCTAssertEqual(e.info.historyLabels.last, "Curves")
        try assertStage(e, s, steps: 2, "curves")
        try attachWindow(wc, "3b-curves")

        // 4. Adjustments > Hue / Saturation: the scenario's values, OK.
        let hsParams = try s.params(2)
        let hs = try openEffect(wc, "Adjustments", id: "effect.adjust.hue_saturation")
        for key in ["Hue", "Saturation", "Lightness"] {
            let row = try XCTUnwrap(hs.numberRows[key], key)
            let v = try XCTUnwrap((hsParams[key] as? NSNumber)?.intValue)
            // Through the row's slider, as a drag ends.
            let slider = try XCTUnwrap(row.slider, key)
            slider.doubleValue = Double(v)
            row.slid(slider)
            XCTAssertEqual(hs.values[key], .int(v), key)
        }
        XCTAssertTrue(waitUntil(30) { hs.isIdle })
        try attachView(try XCTUnwrap(hs.window?.contentView), "4a-hue-saturation-dialog")
        hs.ok(nil)
        XCTAssertTrue(waitUntil(60) { e.info.historyLabels.count == 4 }, e.message ?? "")
        XCTAssertEqual(e.info.historyLabels.last, "Hue / Saturation")
        try assertStage(e, s, steps: 3, "hue / saturation")
        try attachWindow(wc, "4b-hue-saturation")

        // 5. History panel: click Crop (two steps back), then Curves (one forward).
        let history: HistoryPanelView = try XCTUnwrap(fx.workspace.historyView)
        XCTAssertTrue(waitUntil { history.historyRows.map(\.titleLabel.stringValue) == e.info.historyLabels })
        history.layoutSubtreeIfNeeded()
        XCTAssertEqual(try s.params(3)["index"] as? Int, 1)
        click(history.historyRows[1])
        XCTAssertTrue(waitUntil { e.info.historyIndex == 1 })
        XCTAssertTrue(waitUntil { history.historyRows.map(\.undone) == [false, false, true, true] })
        try assertStage(e, s, steps: 4, "two steps back")
        XCTAssertEqual(try s.params(4)["index"] as? Int, 2)
        click(history.historyRows[2])
        XCTAssertTrue(waitUntil { e.info.historyIndex == 2 })
        XCTAssertTrue(waitUntil { history.historyRows.map(\.undone) == [false, false, false, true] })
        XCTAssertEqual(e.info.historyLabels, ["Open", "Crop", "Curves", "Hue / Saturation"], "jumps add no steps")
        try assertStage(e, s, steps: 5, "one step forward")
        try attachWindow(wc, "5a-history")
        try attachView(history, "5b-history-panel")
        let final = try e.document.pixels()

        // The committed scenario, as is: its own expectations and the three files.
        let cliOut = dir.appendingPathComponent("opcli")
        let summary = try opcli(Self.scenarioURL, out: cliOut)
        XCTAssertEqual(summary["history_labels"] as? [String], e.info.historyLabels)
        XCTAssertEqual((summary["history_index"] as? NSNumber)?.intValue, e.info.historyIndex)

        // 6. Save As JPEG through the save sheet: quality 95, 4:2:2.
        var start = SaveOptions()
        start.jpegQuality = 50
        start.jpegSubsampling = .s420
        ImageDocument.saveOptionsStore.save(start, for: "public.jpeg")
        let jpg = dir.appendingPathComponent("phase2-out.jpg")
        let jpgSave = SaveRecorder()
        doc.save(to: jpg, ofType: "public.jpeg", for: .saveAsOperation, delegate: jpgSave,
                 didSave: #selector(SaveRecorder.document(_:didSave:contextInfo:)), contextInfo: nil)
        XCTAssertTrue(waitUntil(10) { ImageDocument.openSaveOptions != nil }, "Save As shows the options sheet")
        let sheet = try XCTUnwrap(ImageDocument.openSaveOptions)
        XCTAssertEqual(sheet.options.jpegQuality, 50, "the sheet starts at the last JPEG options")
        sheet.qualitySlider.integerValue = 95
        sheet.qualitySlider.sendAction(sheet.qualitySlider.action, to: sheet.qualitySlider.target)
        choose(sheet.subsamplingPopup, ChromaSubsampling.s422.label)
        XCTAssertEqual(sheet.options.jpegQuality, 95)
        XCTAssertEqual(sheet.options.jpegSubsampling, .s422)
        XCTAssertEqual(sheet.qualityField.stringValue, "95", "the field shows the slider's quality")
        XCTAssertTrue(waitUntil(20) { sheet.shownBytes != nil }, "the sheet measured the file size")
        try attachView(try XCTUnwrap(sheet.window?.contentView), "6a-save-options")
        sheet.okButton.performClick(nil)
        XCTAssertTrue(waitUntil(20) { jpgSave.result != nil }, "JPEG save finished")
        XCTAssertEqual(jpgSave.result, true)
        XCTAssertTrue(flattenAsked.isEmpty, "one layer: no flatten prompt")
        XCTAssertFalse(doc.isDocumentEdited, "Save As marks the step saved")

        // 7. Save As .opaint (no options).
        let native = dir.appendingPathComponent("phase2-out.opaint")
        let nativeSave = SaveRecorder()
        doc.save(to: native, ofType: ImageFormats.opaintIdentifier, for: .saveAsOperation, delegate: nativeSave,
                 didSave: #selector(SaveRecorder.document(_:didSave:contextInfo:)), contextInfo: nil)
        XCTAssertTrue(waitUntil(20) { nativeSave.result != nil }, ".opaint save finished")
        XCTAssertEqual(nativeSave.result, true)
        XCTAssertNil(ImageDocument.openSaveOptions, ".opaint has no options")
        try attachWindow(wc, "7-saved")

        // 8. Reopen both and compare with opcli.
        let reNative = try open(native, type: ImageFormats.opaintIdentifier)
        let rn = try editor(reNative)
        let cliNative = try fx.engine.openOpaint(path: cliOut.appendingPathComponent("result.opaint").path)
        defer { try? cliNative.close() }
        let cliInfo = try cliNative.info()
        XCTAssertEqual(rn.info.width, (cliInfo["width"] as? NSNumber)?.intValue)
        XCTAssertEqual(rn.info.height, (cliInfo["height"] as? NSNumber)?.intValue)
        same(try rn.document.pixels(), try cliNative.pixels(), "reopened .opaint equals opcli's .opaint")
        same(try rn.document.pixels(), final, "reopened .opaint equals the window before saving")
        XCTAssertEqual(try LayersHistoryTests.layerBytes(rn.document).count, 1)
        same(try LayersHistoryTests.layerBytes(rn.document)[0], try LayersHistoryTests.layerBytes(cliNative)[0],
             "the layer's own pixels")
        XCTAssertEqual(try rn.document.queryData("doc.profile_icc"), try cliNative.queryData("doc.profile_icc"),
                       "same profile")
        XCTAssertEqual(try DocumentSaver.resolution(rn.document), try DocumentSaver.resolution(cliNative))
        let cliPNG = try fx.engine.openPNG(path: cliOut.appendingPathComponent("result.png").path)
        defer { try? cliPNG.close() }
        same(try cliPNG.pixels(), final, "opcli's PNG equals the window")
        try attachWindow(try controller(reNative), "8a-reopened-opaint")

        let reJPEG = try open(jpg, type: "public.jpeg")
        let rj = try editor(reJPEG)
        let cliJPEG = cliOut.appendingPathComponent("result.jpg")
        let cliDecoded = try ImageIOImporter.decode(url: cliJPEG)
        XCTAssertEqual(rj.info.width, cliDecoded.width)
        XCTAssertEqual(rj.info.height, cliDecoded.height)
        same(try rj.document.pixels(), cliDecoded.rgba, "reopened JPEG decodes to opcli's JPEG pixels")
        let ours = try XCTUnwrap(Self.imageData(try Data(contentsOf: jpg)))
        let theirs = try XCTUnwrap(Self.imageData(try Data(contentsOf: cliJPEG)))
        XCTAssertTrue(ours == theirs, "same encoder: the JPEG image data is byte-equal (\(ours.count) vs \(theirs.count) bytes)")
        let psnrCLI = SaveOptionsTests.psnr(try rj.document.pixels(), cliDecoded.rgba)
        let psnrSource = SaveOptionsTests.psnr(try rj.document.pixels(), DocumentSaver.onWhite(final))
        print("M28 JPEG: PSNR against opcli's JPEG \(psnrCLI) dB; against the image before saving \(String(format: "%.2f", psnrSource)) dB")
        XCTAssertEqual(psnrCLI, .infinity)
        // Measured on macOS 27: 36.47 dB at 4:2:2 (46.76 dB at 4:4:4, 35.40 dB at
        // 4:2:0 with the same quality): the loss is the halved color
        // resolution on the fixture's saturated hard edges. The floor is 2.5 dB
        // under the measurement.
        XCTAssertGreaterThanOrEqual(psnrSource, 34, "quality 95 4:2:2 stays close to the image")
        let props = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(
            try XCTUnwrap(CGImageSourceCreateWithURL(jpg as CFURL, nil)), 0, nil) as? [String: Any])
        XCTAssertEqual((props[kCGImagePropertyOrientation as String] as? NSNumber)?.intValue ?? 1, 1,
                       "the pixels are turned, so the saved orientation is 1")
        XCTAssertEqual(try XCTUnwrap(try rj.document.query("doc.profile") as? [String: Any])["description"] as? String,
                       "Display P3")
        try attachWindow(try controller(reJPEG), "8b-reopened-jpeg")
    }
}

// M60 project page screenshots. Render only: no checks of app behavior.
// Skipped unless OP_RENDER_SHOTS=1 (xcodebuild passes TEST_RUNNER_OP_RENDER_SHOTS).
// OP_SHOTS_PHOTO is the JPEG to open and OP_SHOTS_DIR is where the PNGs go.
//
// The Metal canvas cannot be captured in a test, so, as in
// Phase2EndToEndTests (M28), each image draws the engine's composite into the
// canvas area of the rendered window. Then the canvas overlays (marching ants,
// the text edit frame), the floating panels and any sheet are drawn at their
// screen places. Every piece is rendered at 2x, whatever the screen's scale.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class ProjectPageShotsTests: XCTestCase {
    var fx: ShellFixture!
    var docs: [ImageDocument] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedAppAppearance: NSAppearance?
    var out: URL!

    override func setUp() async throws {
        let env = ProcessInfo.processInfo.environment
        try XCTSkipUnless(env["OP_RENDER_SHOTS"] == "1", "set OP_RENDER_SHOTS=1 to render the project page images")
        out = URL(fileURLWithPath: try XCTUnwrap(env["OP_SHOTS_DIR"], "OP_SHOTS_DIR"))
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        fx = try ShellFixture()
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedAppAppearance = NSApp.appearance
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        EffectDialogController.lastEffect = nil
        EffectDialogController.lastUsed = [:]
    }

    override func tearDown() async throws {
        guard fx != nil else { return }
        NSApp.appearance = savedAppAppearance
        for d in docs {
            if let e = d.editor { EffectDialogController.current(for: e)?.cancel(nil) }
            if let wc = d.windowControllers.first as? EditorWindowController, wc.textTool.isEditing { wc.textTool.cancel() }
            for wc in d.windowControllers { wc.window?.orderOut(nil) }
            d.close()
        }
        docs.removeAll()
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        fx.tearDown()
    }

    // MARK: Rendering

    static let scale: CGFloat = 2
    static let appearances: [(String, NSAppearance.Name)] = [("light", .aqua), ("dark", .darkAqua)]

    /// `view` drawn at 2x into a new bitmap.
    func render(_ view: NSView) throws -> CGImage {
        view.needsLayout = true
        view.layoutSubtreeIfNeeded()
        for sub in view.subviewsDeep { sub.needsDisplay = true }
        let rep = try XCTUnwrap(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(view.bounds.width * Self.scale),
            pixelsHigh: Int(view.bounds.height * Self.scale), bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        rep.size = view.bounds.size
        (view.effectiveAppearance).performAsCurrentDrawingAppearance {
            view.cacheDisplay(in: view.bounds, to: rep)
        }
        return try XCTUnwrap(rep.cgImage)
    }

    func context(_ size: CGSize) throws -> CGContext {
        let ctx = try XCTUnwrap(CGContext(
            data: nil, width: Int(size.width * Self.scale), height: Int(size.height * Self.scale), bitsPerComponent: 8,
            bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        ctx.scaleBy(x: Self.scale, y: Self.scale)
        return ctx
    }

    /// The engine's flattened image (with the live preview) in the document profile.
    func compositeImage(_ e: Editor) throws -> CGImage {
        let icc = try e.document.queryData("doc.profile_icc")
        return try ImageIOExporter.cgImage(rgba: try e.document.pixels(preview: true), width: e.info.width,
                                           height: e.info.height, icc: icc.isEmpty ? nil : icc)
    }

    /// The window: its views, the composite in the canvas area, then the overlays.
    func windowImage(_ wc: EditorWindowController) throws -> CGImage {
        let window = try XCTUnwrap(wc.window)
        let root = try XCTUnwrap(window.contentView?.superview ?? window.contentView)
        let base = try render(root)
        let ctx = try context(root.bounds.size)
        ctx.draw(base, in: root.bounds)
        let canvas = wc.canvas
        let vp = canvas.viewport
        let docRect = root.convert(canvas.convert(CGRect(origin: vp.origin, size: vp.contentSize), to: nil), from: nil)
        let visible = root.convert(canvas.convert(canvas.bounds, to: nil), from: nil)
        ctx.saveGState()
        ctx.clip(to: visible.intersection(docRect))
        ctx.interpolationQuality = .high
        ctx.draw(try compositeImage(wc.editor), in: docRect)
        ctx.restoreGState()
        // The marching ants: shape layers, drawn as the window server would.
        if let overlay = wc.selectCrop?.overlay, !overlay.isHidden, let layer = overlay.layer {
            let r = root.convert(overlay.convert(overlay.bounds, to: nil), from: nil)
            ctx.saveGState()
            ctx.clip(to: r)
            ctx.translateBy(x: r.minX, y: r.maxY)
            ctx.scaleBy(x: 1, y: -1)  // the overlay is flipped
            layer.render(in: ctx)
            ctx.restoreGState()
        }
        // The Text tool's edit frame and caret.
        if wc.textTool.isEditing {
            let input = wc.textTool.input
            let r = root.convert(input.convert(input.bounds, to: nil), from: nil)
            ctx.draw(try render(input), in: r)
        }
        return try XCTUnwrap(ctx.makeImage())
    }

    /// A rounded window shape with a soft shadow, as macOS draws one.
    func drawWindow(_ image: CGImage, in r: CGRect, radius: CGFloat, ctx: CGContext) {
        let path = CGPath(roundedRect: r, cornerWidth: radius, cornerHeight: radius, transform: nil)
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 36, color: CGColor(gray: 0, alpha: 0.32))
        ctx.addPath(path)
        ctx.setFillColor(CGColor(gray: 0.5, alpha: 1))
        ctx.fillPath()
        ctx.restoreGState()
        ctx.saveGState()
        ctx.addPath(path)
        ctx.clip()
        ctx.draw(image, in: r)
        ctx.restoreGState()
        ctx.saveGState()
        ctx.addPath(path)
        ctx.setStrokeColor(CGColor(gray: 0, alpha: 0.18))
        ctx.setLineWidth(0.5)
        ctx.strokePath()
        ctx.restoreGState()
    }

    func frameView(_ w: NSWindow) throws -> NSView { try XCTUnwrap(w.contentView?.superview ?? w.contentView) }

    /// The window with the shown panels and its sheet at their screen places,
    /// on a clear background with a margin for the shadows.
    func scene(_ wc: EditorWindowController, panels: [PanelKind]? = nil) throws -> CGImage {
        let window = try XCTUnwrap(wc.window)
        let kinds = panels ?? PanelKind.allCases.filter { fx.workspace.panels.isShown($0) }
        var parts: [(NSWindow, CGImage, CGFloat)] = [(window, try windowImage(wc), 16)]
        for k in kinds {
            let p = try XCTUnwrap(fx.workspace.panels.panels[k])
            parts.append((p, try render(try frameView(p)), 10))
        }
        if let sheet = window.attachedSheet {
            parts.append((sheet, try render(try frameView(sheet)), 12))
        }
        let union = parts.map(\.0.frame).reduce(window.frame) { $0.union($1) }
        let margin: CGFloat = 48
        let bounds = union.insetBy(dx: -margin, dy: -margin)
        let ctx = try context(bounds.size)
        ctx.interpolationQuality = .high
        for (w, image, radius) in parts {
            let r = w.frame.offsetBy(dx: -bounds.minX, dy: -bounds.minY)
            drawWindow(image, in: r, radius: radius, ctx: ctx)
        }
        return try XCTUnwrap(ctx.makeImage())
    }

    /// One panel alone (title strip included), with its shadow.
    func panelScene(_ k: PanelKind) throws -> CGImage {
        let p = try XCTUnwrap(fx.workspace.panels.panels[k])
        let image = try render(try frameView(p))
        let margin: CGFloat = 32
        let size = CGSize(width: p.frame.width + 2 * margin, height: p.frame.height + 2 * margin)
        let ctx = try context(size)
        drawWindow(image, in: CGRect(origin: CGPoint(x: margin, y: margin), size: p.frame.size), radius: 10, ctx: ctx)
        return try XCTUnwrap(ctx.makeImage())
    }

    func write(_ image: CGImage, _ name: String) throws {
        let data = try XCTUnwrap(NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]))
        try data.write(to: out.appendingPathComponent("\(name).png"))
        let a = XCTAttachment(image: NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height)))
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    /// Renders `make()` in light and dark appearance.
    func shoot(_ name: String, _ make: () throws -> CGImage) throws {
        for (suffix, a) in Self.appearances {
            NSApp.appearance = NSAppearance(named: a)
            for w in NSApp.windows { w.appearance = NSAppearance(named: a) }
            // Thumbnails, the strip and the panels catch up with the engine.
            _ = waitUntil(1.5) { false }
            try write(try make(), "\(name)-\(suffix)")
        }
    }

    // MARK: Driving the app

    func open(_ url: URL) throws -> EditorWindowController {
        let doc = try ImageDocument(contentsOf: url, ofType: "public.jpeg")
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        let wc = try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
        wc.window?.layoutIfNeeded()
        wc.updateClearRect()
        return wc
    }

    func openEffect(_ wc: EditorWindowController, _ id: String) throws -> EffectDialogController {
        let d = try XCTUnwrap(wc.showEffect(id), "\(id) opens a dialog")
        XCTAssertTrue(waitUntil(10) { wc.window?.attachedSheet === d.window })
        return d
    }

    func slide(_ d: EffectDialogController, _ key: String, _ v: Int) throws {
        let row = try XCTUnwrap(d.numberRows[key], key)
        let slider = try XCTUnwrap(row.slider, key)
        slider.doubleValue = Double(v)
        row.slid(slider)
    }

    func waitSteps(_ e: Editor, _ n: Int) {
        XCTAssertTrue(waitUntil(60) { e.info.historyLabels.count == n }, "\(e.info.historyLabels) \(e.message ?? "")")
    }

    func settleSheet(_ wc: EditorWindowController) {
        // Let the sheet finish its slide-in so its frame is final.
        _ = waitUntil(1.0) { false }
        wc.window?.attachedSheet?.layoutIfNeeded()
    }

    // MARK: The shots

    // Not async: an async test body runs as a main-queue job, so spinning the
    // run loop inside it does not deliver the engine's main-queue events.
    func testRenderProjectPageShots() throws {
        // The Colors hex field keeps its text while it has a field editor, so
        // nothing in the Colors panel keeps the focus during the shots.
        fx.workspace.panels.panels[.colors]?.makeFirstResponder(nil)
        let photo = URL(fileURLWithPath: try XCTUnwrap(ProcessInfo.processInfo.environment["OP_SHOTS_PHOTO"]))
        let wc = try open(photo)
        let e = wc.editor
        let tools = fx.workspace.tools
        XCTAssertEqual(e.info.historyLabels, ["Open"])

        // Curves (Luminosity, a gentle S curve): the dialog with its live preview, then OK.
        let curves = try openEffect(wc, "effect.adjust.curves")
        if let mode = curves.popups["ColorTransferMode"] {
            mode.selectItem(withTitle: "Luminosity")
            mode.sendAction(mode.action, to: mode.target)
        }
        curves.set("ControlPoints", .curve([[[0, 0], [70, 56], [184, 204], [255, 255]]]))
        XCTAssertTrue(waitUntil(30) { curves.isIdle })
        settleSheet(wc)
        try shoot("effect-curves") { try scene(wc) }
        curves.ok(nil)
        waitSteps(e, 2)

        // Hue / Saturation: a strong hue turn shown live (then Cancel), then the kept change.
        let hsPreview = try openEffect(wc, "effect.adjust.hue_saturation")
        try slide(hsPreview, "Hue", -22)
        try slide(hsPreview, "Saturation", 130)
        XCTAssertTrue(waitUntil(30) { hsPreview.isIdle })
        settleSheet(wc)
        try shoot("effect-hue-saturation") { try scene(wc) }
        hsPreview.cancel(nil)
        XCTAssertTrue(waitUntil(10) { wc.window?.attachedSheet == nil })
        let hs = try openEffect(wc, "effect.adjust.hue_saturation")
        try slide(hs, "Saturation", 112)
        XCTAssertTrue(waitUntil(30) { hs.isIdle })
        hs.ok(nil)
        waitSteps(e, 3)
        XCTAssertTrue(waitUntil(10) { wc.window?.attachedSheet == nil })

        // Ellipse Select around a patch of tulips: the marching ants.
        tools.tool = .ellipseSelect
        let s = try XCTUnwrap(wc.selectCrop)
        let v = wc.canvas.viewport
        let a = CGPoint(x: 1500, y: 1250), b = CGPoint(x: 2700, y: 2150)
        let mid = CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2)
        s.pointerDown(document: a, view: v.viewPoint(ofDocument: a), modifiers: [], right: false)
        s.pointerDragged(document: mid, view: v.viewPoint(ofDocument: mid), modifiers: [])
        s.pointerUp(document: b, view: v.viewPoint(ofDocument: b), modifiers: [])
        XCTAssertTrue(waitUntil(30) { e.info.historyLabels.count == 4 && !s.busy }, "\(e.info.historyLabels)")
        _ = waitUntil(0.5) { false }
        try shoot("selection") { try scene(wc) }
        e.run("selection.deselect")
        waitSteps(e, 5)

        // A new layer, then the Text tool with the text still open.
        fx.workspace.layersView.addButton.performClick(nil)
        waitSteps(e, 6)
        tools.primary = DocColor(r: 255, g: 255, b: 255)
        tools.tool = .text
        fx.workspace.panels.panels[.colors]?.makeFirstResponder(nil)
        tools.settings.text.family = "Avenir Next"
        tools.settings.text.bold = true
        tools.settings.text.size = 144
        XCTAssertTrue(wc.textTool.begin(at: CGPoint(x: 260, y: 150)))
        wc.textTool.input.insertText("Spring in Holland", replacementRange: NSRange(location: NSNotFound, length: 0))
        _ = waitUntil(2) { false }
        e.refresh()
        try shoot("text-tool") { try scene(wc) }
        wc.textTool.finish()
        waitSteps(e, 7)

        // The main window: the photo, the panels, the History list.
        tools.tool = .paintbrush
        tools.primary = DocColor(r: 0xD4, g: 0x1E, b: 0x3A)
        tools.secondary = DocColor(r: 255, g: 255, b: 255)
        fx.workspace.panels.panels[.colors]?.makeFirstResponder(nil)
        tools.primary = DocColor(r: 0xD4, g: 0x1E, b: 0x3A)
        _ = waitUntil(0.5) { false }
        e.refresh()
        try shoot("main-window") { try scene(wc) }

        // The Colors panel, expanded (More), and the History and Layers panels alone.
        try shoot("panel-history") { try panelScene(.history) }
        try shoot("panel-layers") { try panelScene(.layers) }
        fx.workspace.colorsView.setExpanded(true)
        _ = waitUntil(0.5) { false }
        try shoot("panel-colors") { try panelScene(.colors) }
        fx.workspace.colorsView.setExpanded(false)
        print("M60 shots: history \(e.info.historyLabels)")
    }
}

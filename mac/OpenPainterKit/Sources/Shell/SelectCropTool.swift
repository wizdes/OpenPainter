// Rectangle Select and the Crop tool on the canvas (M23), and Ellipse Select
// and Lasso Select (M29), which reuse Rectangle Select's modes, keys, styles
// and handles. The canvas sends
// the pointer here (`CanvasTool.overlay`); this controller turns drags into
// engine commands and draws the overlay.
//
// Rectangle Select: a drag makes a rectangle; the release sends one
// `selection.rectangle` (one history step) with the combine mode from the
// modifier keys or the options row. Afterwards the rectangle shows eight
// handles; dragging one changes the rectangle and replaces that same step
// (`amend`), so a finished selection stays one step. A click without a drag
// deselects (Replace mode). Escape cancels a drag, else deselects.
//
// Ellipse Select (M29) is Rectangle Select with `selection.ellipse`: the drag
// box is the ellipse's bounding box, Shift keeps a circle, the styles and the
// eight handles are the same, and a handle drag amends the same step. Lasso
// Select (M29) collects the drag's points and the release sends one
// `selection.lasso`; the engine closes the shape back to the first point.
// paint.net's Lasso and Ellipse pages
// (https://www.getpaint.net/doc/latest/LassoSelectionTool.html,
// https://www.getpaint.net/doc/latest/EllipseSelectionTool.html) name only a
// freeform lasso, so there is no Polygon mode.
//
// Crop tool (Image > Crop Tool): a frame over the image with aspect presets
// and a straighten angle. The frame is drawn turned by the angle on the
// original image; Enter (or the Crop button) sends one `image.crop_rect`,
// which rotates and crops in one history step. Escape (or Cancel, or picking
// another tool) leaves the image as it was. paint.net 5.1 has no Crop tool;
// its rules here are OpenPainter's.
import AppKit

@MainActor
public final class SelectCropController: NSObject, CanvasPointerSink {
    unowned let controller: EditorWindowController
    public let overlay = SelectionOverlayView(frame: .zero)

    // Rectangle Select options (the selection mode is the shared one in ToolSettings).
    public var style = RectSelectStyle.normal { didSet { syncOptions() } }
    /// Width : height for Fixed Ratio.
    public var ratio = CGSize(width: 1, height: 1) { didSet { syncOptions() } }
    /// Pixel size for Fixed Size.
    public var fixedSize = CGSize(width: 100, height: 100) { didSet { syncOptions() } }

    // Crop tool state.
    public private(set) var cropActive = false
    public private(set) var cropAspect = CropAspect.free
    public private(set) var cropAngle: Double = 0
    /// The crop frame in the rotated image's pixels (equal to the image's
    /// pixels at angle 0).
    public private(set) var cropFrame = CGRect.zero

    /// The rectangle of the last finished Rectangle Select step (its handles show).
    public private(set) var editRect: CGRect?
    private var editMode = ToolSettings.SelectionMode.replace
    /// The command of the step whose handles show (`selection.rectangle` or `selection.ellipse`).
    private var editCommand = "selection.rectangle"
    private var lastTool: PaintTool?
    private var editRevision: Int?
    /// True while a `selection.rectangle` this controller sent is running.
    public private(set) var busy = false
    private var shownRevision = -1

    private enum Drag {
        case drawing(start: CGPoint, mode: ToolSettings.SelectionMode)
        case lasso([CGPoint], mode: ToolSettings.SelectionMode)
        case handle(RectHandle, CGRect)
        case cropNew(CGPoint)
        case cropMove(CGPoint, CGRect)
        case cropHandle(RectHandle, CGRect)
    }
    private var drag: Drag?

    // Options row controls.
    public let stylePopup = Labels.popup(RectSelectStyle.allCases.map(\.label))
    public let modePopup = Labels.popup(ToolSettings.SelectionMode.allCases.map(\.label))
    public let ratioWidth = Labels.field("1", width: 44, height: 28)
    public let ratioHeight = Labels.field("1", width: 44, height: 28)
    public let sizeWidth = Labels.field("100", width: 52, height: 28)
    public let sizeHeight = Labels.field("100", width: 52, height: 28)
    public let aspectPopup = Labels.popup(CropAspect.allCases.map(\.label))
    public let angleSlider = NSSlider(value: 0, minValue: -45, maxValue: 45, target: nil, action: nil)
    public let angleField = Labels.field("0", width: 52, height: 28)
    public private(set) var applyButton: NSButton!
    public private(set) var cancelButton: NSButton!
    private var ratioViews: [NSView] = []
    private var sizeViews: [NSView] = []

    var editor: Editor { controller.editor }
    var canvas: MetalCanvasView { controller.canvas }
    var tools: ToolState { controller.workspace.tools }
    var canvasSize: CGSize { CGSize(width: editor.info.width, height: editor.info.height) }
    /// Rectangle and Ellipse Select: a drag box with styles and handles.
    var boxTool: Bool { tools.tool == .rectangleSelect || tools.tool == .ellipseSelect }
    /// Every tool this controller draws for.
    var selectTool: Bool { boxTool || tools.tool == .lassoSelect }
    /// The engine command for a box drag with the current tool.
    var boxCommand: String { tools.tool == .ellipseSelect ? "selection.ellipse" : "selection.rectangle" }

    init(controller: EditorWindowController) {
        self.controller = controller
        super.init()
        overlay.frame = controller.canvas.bounds
        overlay.autoresizingMask = [.width, .height]
        controller.canvas.addSubview(overlay)
        controller.canvas.pointerSink = self
        buildControls()
        tools.observe(self) { [weak self] in self?.toolsChanged() }
        editor.observe(self) { [weak self] in self?.editorChanged() }
    }

    // MARK: Options row

    private func buildControls() {
        stylePopup.setAccessibilityLabel("Selection style")
        modePopup.setAccessibilityLabel("Selection mode")
        ratioWidth.setAccessibilityLabel("Ratio width")
        ratioHeight.setAccessibilityLabel("Ratio height")
        sizeWidth.setAccessibilityLabel("Width in pixels")
        sizeHeight.setAccessibilityLabel("Height in pixels")
        aspectPopup.setAccessibilityLabel("Aspect ratio")
        angleSlider.setAccessibilityLabel("Straighten angle")
        angleField.setAccessibilityLabel("Straighten angle in degrees")
        for p in [stylePopup, modePopup, aspectPopup] {
            p.target = self
            p.action = #selector(popupChanged(_:))
        }
        for f in [ratioWidth, ratioHeight, sizeWidth, sizeHeight, angleField] {
            f.target = self
            f.action = #selector(fieldChanged(_:))
        }
        angleSlider.isContinuous = true
        angleSlider.target = self
        angleSlider.action = #selector(sliderMoved(_:))
        angleSlider.translatesAutoresizingMaskIntoConstraints = false
        angleSlider.widthAnchor.constraint(equalToConstant: 120).isActive = true
        applyButton = NSButton(title: "Crop", target: self, action: #selector(applyClicked(_:)))
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancelClicked(_:)))
        for b in [applyButton!, cancelButton!] {
            b.font = Theme.font(12)
            b.bezelStyle = .rounded
        }
        applyButton.setAccessibilityLabel("Crop (Enter)")
        cancelButton.setAccessibilityLabel("Cancel crop (Escape)")
        ratioViews = [Labels.label("Ratio"), ratioWidth, Labels.label(":"), ratioHeight]
        sizeViews = [Labels.label("Size"), sizeWidth, Labels.label("×"), sizeHeight, Labels.label("px")]
    }

    /// The options-row views for Rectangle Select (after the tool picker).
    var rectViews: [NSView] {
        [Labels.label("Selection mode"), modePopup, Labels.gap(8), Labels.label("Style"), stylePopup, Labels.gap(8)]
            + ratioViews + [Labels.gap(8)] + sizeViews
    }

    /// The options-row views for Lasso Select (no styles).
    var lassoViews: [NSView] { [Labels.label("Selection mode"), modePopup] }

    /// The options-row views while the Crop tool is on.
    var cropViews: [NSView] {
        [Labels.label("Crop: aspect"), aspectPopup, Labels.gap(8), Labels.label("Straighten"), angleSlider,
         angleField, Labels.label("°"), Labels.gap(8), cancelButton, applyButton]
    }

    func syncOptions() {
        stylePopup.selectItem(at: RectSelectStyle.allCases.firstIndex(of: style) ?? 0)
        modePopup.selectItem(at: ToolSettings.SelectionMode.allCases.firstIndex(of: tools.settings.selectionMode) ?? 0)
        if ratioWidth.currentEditor() == nil { ratioWidth.stringValue = OptionsBarView.number(ratio.width) }
        if ratioHeight.currentEditor() == nil { ratioHeight.stringValue = OptionsBarView.number(ratio.height) }
        if sizeWidth.currentEditor() == nil { sizeWidth.stringValue = OptionsBarView.number(fixedSize.width) }
        if sizeHeight.currentEditor() == nil { sizeHeight.stringValue = OptionsBarView.number(fixedSize.height) }
        for v in ratioViews { v.isHidden = style != .fixedRatio }
        for v in sizeViews { v.isHidden = style != .fixedSize }
        aspectPopup.selectItem(at: CropAspect.allCases.firstIndex(of: cropAspect) ?? 0)
        angleSlider.doubleValue = cropAngle
        if angleField.currentEditor() == nil { angleField.stringValue = OptionsBarView.number(cropAngle) }
    }

    private func showOptions() {
        let bar = controller.optionsBar
        let want: [NSView]? =
            cropActive ? cropViews : boxTool ? rectViews : tools.tool == .lassoSelect ? lassoViews : nil
        // Leave another controller's options alone (M33: the Move tools).
        let ours = bar.customOptions?.contains { $0 === modePopup || $0 === aspectPopup } ?? true
        if want != nil || ours, want.map({ $0.map(ObjectIdentifier.init) }) != bar.customOptions.map({ $0.map(ObjectIdentifier.init) }) {
            bar.customOptions = want
        }
        syncOptions()
    }

    @objc private func popupChanged(_ sender: NSPopUpButton) {
        if sender === stylePopup {
            style = RectSelectStyle.allCases[max(sender.indexOfSelectedItem, 0)]
        } else if sender === modePopup {
            tools.settings.selectionMode = ToolSettings.SelectionMode.allCases[max(sender.indexOfSelectedItem, 0)]
        } else if sender === aspectPopup {
            setCropAspect(CropAspect.allCases[max(sender.indexOfSelectedItem, 0)])
        }
    }

    @objc private func fieldChanged(_ sender: NSTextField) {
        let v = Double(sender.stringValue.trimmingCharacters(in: .whitespaces)) ?? .nan
        if v.isFinite, v > 0 {
            switch sender {
            case ratioWidth: ratio.width = v
            case ratioHeight: ratio.height = v
            case sizeWidth: fixedSize.width = v.rounded()
            case sizeHeight: fixedSize.height = v.rounded()
            default: break
            }
        }
        if sender === angleField, v.isFinite { setCropAngle(v) }
        syncOptions()
    }

    @objc private func sliderMoved(_ sender: NSSlider) {
        setCropAngle((sender.doubleValue * 10).rounded() / 10)
    }

    @objc private func applyClicked(_ sender: Any?) { applyCrop() }
    @objc private func cancelClicked(_ sender: Any?) { endCrop() }

    // MARK: State changes

    private func toolsChanged() {
        // Picking a tool ends the Crop tool without cropping.
        if cropActive, tools.tool != toolWhenCropBegan { endCrop() }
        if tools.tool != lastTool {
            // Handles belong to the tool that made them.
            lastTool = tools.tool
            editRect = nil
            drag = nil
            overlay.pending = nil
            overlay.pendingLasso = nil
        }
        overlay.pendingEllipse = tools.tool == .ellipseSelect
        showOptions()
        updateHandles()
    }

    private var toolWhenCropBegan: PaintTool?

    private func editorChanged() {
        let info = editor.info
        if info.revision != shownRevision {
            shownRevision = info.revision
            reloadSelectionPath()
            if !busy, let r = editRevision, r != info.revision {
                editRect = nil
                editRevision = nil
            }
            if cropActive, cropFrame.maxX > canvasSize.width || cropFrame.maxY > canvasSize.height {
                endCrop()
            }
        }
        updateHandles()
    }

    /// Reads the selection outline from the engine and shows it.
    public func reloadSelectionPath() {
        do {
            let json = try editor.document.query("doc.selection_path") as? [String: Any] ?? [:]
            overlay.setSelectionPath(SelectionOverlayView.path(fromEngine: json))
        } catch {
            editor.report("could not read the selection outline: \(error)")
        }
    }

    public func viewportChanged() {
        overlay.setViewport(canvas.viewport)
    }

    private func updateHandles() {
        overlay.handleRect = boxTool && !cropActive && style != .fixedSize ? editRect : nil
        if cropActive {
            let corners = CropGeometry.corners(cropFrame, angle: cropAngle, canvas: canvasSize)
            let handles = RectHandle.allCases.map {
                CropGeometry.toOriginal($0.point(in: cropFrame), angle: cropAngle, canvas: canvasSize)
            }
            overlay.crop = (corners, handles)
        } else if overlay.crop != nil {
            overlay.crop = nil
        }
    }

    // MARK: Keys

    /// Escape: cancel a drag or the Crop tool. False when there was nothing
    /// to cancel (the window then deselects).
    public func escape() -> Bool {
        if cropActive {
            endCrop()
            return true
        }
        if drag != nil {
            drag = nil
            overlay.pending = nil
            overlay.pendingLasso = nil
            updateHandles()
            return true
        }
        return false
    }

    /// Enter: finish the Crop tool. False when it is off.
    public func enter() -> Bool {
        guard cropActive else { return false }
        applyCrop()
        return true
    }

    // MARK: Rectangle Select

    /// Sends one Rectangle or Ellipse Select step (or replaces the current
    /// one). `command` defaults to the current tool's.
    public func commit(_ rect: CGRect, mode: ToolSettings.SelectionMode, amend: Bool, command: String? = nil) {
        busy = true
        let command = command ?? boxCommand
        let params: [String: any Sendable] = [
            "x": Double(rect.minX), "y": Double(rect.minY), "width": Double(rect.width),
            "height": Double(rect.height), "combine": mode.rawValue, "amend": amend,
        ]
        Task { @MainActor [weak self] in
            guard let self else { return }
            let status = await self.editor.runAndWait(command, params)
            self.busy = false
            if status == "ok" {
                self.editRect = rect
                self.editMode = mode
                self.editCommand = command
                self.editRevision = self.editor.info.revision
            } else {
                self.editRect = nil
            }
            self.updateHandles()
        }
    }

    /// Sends one Lasso Select step for the points of a drag (document pixels).
    public func commitLasso(_ points: [CGPoint], mode: ToolSettings.SelectionMode) {
        busy = true
        editRect = nil
        let text = SelectGeometry.lassoText(points)
        Task { @MainActor [weak self] in
            guard let self else { return }
            _ = await self.editor.runAndWait("selection.lasso", ["points": text, "combine": mode.rawValue])
            self.busy = false
            self.updateHandles()
        }
    }

    func viewRect(_ r: CGRect) -> CGRect {
        let v = canvas.viewport
        let a = v.viewPoint(ofDocument: r.origin)
        return CGRect(x: a.x, y: a.y, width: r.width * v.zoom, height: r.height * v.zoom)
    }

    var fixedRatio: CGFloat? { style == .fixedRatio && ratio.height > 0 ? ratio.width / ratio.height : nil }

    public func pointerDown(document p: CGPoint, view v: CGPoint, modifiers: NSEvent.ModifierFlags, right: Bool) {
        if cropActive { return cropDown(p, view: v) }
        guard selectTool, !busy else { return }
        let mode = SelectGeometry.mode(modifiers: modifiers, right: right, toolbar: tools.settings.selectionMode)
        if tools.tool == .lassoSelect {
            drag = .lasso([p], mode: mode)
            overlay.pendingLasso = [p]
            return
        }
        if let r = editRect, style != .fixedSize, let h = SelectGeometry.handle(at: v, viewRect: viewRect(r)) {
            drag = .handle(h, r)
            return
        }
        drag = .drawing(start: p, mode: mode)
        if style == .fixedSize { pointerDragged(document: p, view: v, modifiers: modifiers) }
    }

    public func pointerDragged(document p: CGPoint, view v: CGPoint, modifiers: NSEvent.ModifierFlags) {
        switch drag {
        case let .drawing(start, _):
            overlay.pending = SelectGeometry.dragRect(
                from: start, to: p, style: style, ratio: ratio, size: fixedSize,
                square: modifiers.contains(.shift), canvas: canvasSize)
        case let .handle(h, r):
            overlay.pending = SelectGeometry.resize(
                r, handle: h, to: p, ratio: modifiers.contains(.shift) ? r.width / max(r.height, 1) : fixedRatio,
                canvas: canvasSize)
        case let .lasso(points, mode):
            // Keep points at least a quarter pixel apart.
            if let last = points.last, hypot(p.x - last.x, p.y - last.y) < 0.25 { return }
            drag = .lasso(points + [p], mode: mode)
            overlay.pendingLasso = points + [p]
        case .cropNew, .cropMove, .cropHandle:
            cropDragged(p)
        case nil:
            break
        }
    }

    public func pointerUp(document p: CGPoint, view v: CGPoint, modifiers: NSEvent.ModifierFlags) {
        pointerDragged(document: p, view: v, modifiers: modifiers)
        let rect = overlay.pending
        let d = drag
        drag = nil
        overlay.pending = nil
        overlay.pendingLasso = nil
        switch d {
        case let .drawing(_, mode):
            if let rect, rect.width >= 1, rect.height >= 1 {
                commit(rect, mode: mode, amend: false)
            } else if mode == .replace, editor.selection != nil {
                editRect = nil
                editor.run("selection.deselect")
            }
        case .handle:
            if let rect, rect.width >= 1, rect.height >= 1, rect != editRect {
                commit(rect, mode: editMode, amend: true, command: editCommand)
            }
        case let .lasso(points, mode):
            if SelectGeometry.isArea(points) {
                commitLasso(points, mode: mode)
            } else if mode == .replace, editor.selection != nil {
                editor.run("selection.deselect")
            }
        case .cropNew, .cropMove, .cropHandle:
            break
        case nil:
            break
        }
        updateHandles()
    }

    // MARK: Crop tool

    /// Turns the Crop tool on: the frame starts as the largest rectangle of
    /// the aspect preset, centered.
    public func beginCrop() {
        guard !cropActive, canvasSize.width > 0 else { return }
        cropActive = true
        toolWhenCropBegan = tools.tool
        drag = nil
        overlay.pending = nil
        cropAngle = 0
        cropFrame = CropGeometry.largest(ratio: cropAspect.ratio(original: canvasSize), canvas: canvasSize)
        canvas.tool = .overlay
        canvas.window?.makeFirstResponder(canvas)
        editor.report("Crop: drag to set the frame, Enter crops, Escape cancels")
        showOptions()
        updateHandles()
    }

    /// Turns the Crop tool off without cropping.
    public func endCrop() {
        guard cropActive else { return }
        cropActive = false
        drag = nil
        toolWhenCropBegan = nil
        controller.toolChanged()
        editor.clearMessage()
        showOptions()
        updateHandles()
    }

    public func setCropAspect(_ a: CropAspect) {
        cropAspect = a
        if cropActive {
            cropFrame = CropGeometry.largest(ratio: a.ratio(original: canvasSize), canvas: canvasSize)
        }
        syncOptions()
        updateHandles()
    }

    public func setCropAngle(_ degrees: Double) {
        cropAngle = min(max(degrees, -45), 45)
        syncOptions()
        updateHandles()
    }

    /// Sets the frame directly (tests, typed values). It is kept inside the canvas.
    public func setCropFrame(_ r: CGRect) {
        cropFrame = SelectGeometry.rounded(r.intersection(CGRect(origin: .zero, size: canvasSize)))
        updateHandles()
    }

    /// The `image.crop_rect` parameters Enter sends.
    public var cropParams: [String: any Sendable] { CropGeometry.params(cropFrame, angle: cropAngle) }

    /// Sends one `image.crop_rect` and turns the tool off.
    @discardableResult
    public func applyCrop() -> UInt64? {
        guard cropActive, cropFrame.width >= 1, cropFrame.height >= 1 else { return nil }
        let p = cropParams
        endCrop()
        return editor.run("image.crop_rect", p)
    }

    private func rotated(_ p: CGPoint) -> CGPoint {
        CropGeometry.toRotated(p, angle: cropAngle, canvas: canvasSize)
    }

    private func cropDown(_ p: CGPoint, view v: CGPoint) {
        let handleTarget: CGFloat = 12
        let vp = canvas.viewport
        if let h = RectHandle.allCases.first(where: { k in
            let c = vp.viewPoint(ofDocument: CropGeometry.toOriginal(k.point(in: cropFrame), angle: cropAngle,
                                                                       canvas: canvasSize))
            return abs(c.x - v.x) <= handleTarget / 2 && abs(c.y - v.y) <= handleTarget / 2
        }) {
            drag = .cropHandle(h, cropFrame)
        } else if cropFrame.contains(rotated(p)) {
            drag = .cropMove(rotated(p), cropFrame)
        } else {
            drag = .cropNew(rotated(p))
        }
    }

    private func cropDragged(_ p: CGPoint) {
        let q = rotated(p)
        let ratio = cropAspect.ratio(original: canvasSize)
        switch drag {
        case let .cropNew(start):
            let r = ratio.map { SelectGeometry.ratioRect(from: start, to: q, ratio: $0, canvas: canvasSize) }
                ?? SelectGeometry.dragRect(from: start, to: q, style: .normal, ratio: .zero, size: .zero, square: false,
                                           canvas: canvasSize)
            if r.width >= 1, r.height >= 1 { cropFrame = r }
        case let .cropMove(start, frame):
            let dx = (q.x - start.x).rounded(), dy = (q.y - start.y).rounded()
            cropFrame = CGRect(
                x: min(max(frame.minX + dx, 0), canvasSize.width - frame.width),
                y: min(max(frame.minY + dy, 0), canvasSize.height - frame.height),
                width: frame.width, height: frame.height)
        case let .cropHandle(h, frame):
            let r = SelectGeometry.resize(frame, handle: h, to: q, ratio: ratio, canvas: canvasSize)
            if r.width >= 1, r.height >= 1 { cropFrame = r }
        default:
            break
        }
        updateHandles()
    }
}

// MARK: Image menu: the Crop tool, Resize and Canvas Size

/// Image menu selectors the document window controller implements (M23).
@MainActor @objc public protocol ImageSizeActions {
    func showCropTool(_ sender: Any?)
    func showResizeImage(_ sender: Any?)
    func showCanvasSize(_ sender: Any?)
}

extension EditorWindowController: ImageSizeActions {
    @objc public func showCropTool(_ sender: Any?) {
        moveTool?.finish()  // M33: a move in progress commits first
        selectCrop.beginCrop()
    }

    @objc public func showResizeImage(_ sender: Any?) {
        presentResizeImage()
    }

    @objc public func showCanvasSize(_ sender: Any?) {
        presentCanvasSize()
    }

    @discardableResult
    public func presentResizeImage() -> ResizeImageController? {
        guard let window, window.attachedSheet == nil, editor.info.width > 0 else { return nil }
        let d = ResizeImageController(editor: editor, dpi: documentDPI)
        d.present(on: window)
        return d
    }

    @discardableResult
    public func presentCanvasSize() -> CanvasSizeController? {
        guard let window, window.attachedSheet == nil, editor.info.width > 0 else { return nil }
        let d = CanvasSizeController(editor: editor, dpi: documentDPI, tools: workspace.tools)
        d.present(on: window)
        return d
    }

    /// The document's pixels per inch (`doc.resolution`), 96 when it cannot be read.
    var documentDPI: Double {
        do {
            let r = try editor.document.query("doc.resolution") as? [String: Any]
            return (r?["dpi"] as? NSNumber)?.doubleValue ?? 96
        } catch {
            editor.report("could not read the resolution: \(error); using 96 pixels per inch")
            return 96
        }
    }
}

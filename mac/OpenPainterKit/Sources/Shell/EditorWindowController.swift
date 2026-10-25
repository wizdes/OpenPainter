// The Classic v2 main window for one image: the unified toolbar (document
// title with the system Edited state, command buttons in paint.net order, the
// four panel toggles), the tool-options row, the image strip, the stage with
// the canvas, and the status bar. docs/design/SPEC.md has the sizes.
import AppKit

@MainActor
public final class EditorWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate,
    NSMenuItemValidation, EditorMenuActions
{
    public let editor: Editor
    public let workspace: Workspace
    public let container: CanvasContainerView
    public let optionsBar: OptionsBarView
    public let strip: ImageStripView
    public let statusBar = StatusBarView()
    public private(set) var commandButtons: [String: ShellButton] = [:]
    public private(set) var panelToggles: [PanelKind: ToggleButton] = [:]
    /// Live brush strokes from the canvas (M38).
    public private(set) var brush: BrushStrokeController!
    /// Rectangle Select, the Crop tool and the marching ants (M23).
    public private(set) var selectCrop: SelectCropController!
    /// Move Selected Pixels and Move Selection (M33).
    public private(set) var moveTool: MoveToolController!
    private var cursor: CGPoint?
    /// The live Magic Wand and Paint Bucket (M31, FloodTool.swift).
    public private(set) var flood: FloodToolController!
    /// The Text tool's session and input view (M46).
    public private(set) var textTool: TextToolController!
    static let minimumSize = NSSize(width: 1100, height: 792)

    public var canvas: MetalCanvasView { container.canvas }

    public init(editor: Editor, workspace: Workspace) {
        self.editor = editor
        self.workspace = workspace
        container = CanvasContainerView(document: editor.document, documentSize: editor.pixelSize)
        optionsBar = OptionsBarView(tools: workspace.tools)
        strip = ImageStripView(workspace: workspace)
        let visible = NSScreen.main?.visibleFrame.size ?? Self.minimumSize
        let size = NSSize(width: min(1280, visible.width), height: min(800, visible.height))
        let window = EditorWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init(window: window)
        window.editorController = self
        window.isRestorable = false
        window.tabbingMode = .disallowed
        window.contentMinSize = NSSize(
            width: min(Self.minimumSize.width, visible.width), height: min(Self.minimumSize.height - 52, visible.height))
        window.title = editor.name
        window.toolbarStyle = .unified
        window.titleVisibility = .visible
        window.delegate = self
        let root = ShellRootView(controller: self)
        window.contentView = root
        window.initialFirstResponder = canvas
        let toolbar = NSToolbar(identifier: "OpenPainterMain")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        window.toolbar = toolbar
        editor.windowController = self
        wireCanvas()
        FileDrop.register(self)  // M17: dropped files open as documents
        textTool = TextToolController(window: self)
        optionsBar.textControls.onFinish = { [weak self] in self?.textTool.finish() }
        workspace.tools.observe(self) { [weak self] in self?.toolChanged() }
        workspace.observe(self) { [weak self] in self?.workspaceChanged() }
        editor.observe(self) { [weak self] in self?.editorChanged() }
        statusBar.onZoom = { [weak self] z in self?.zoomFromStatus(z) }
        wireViewSettings()  // M56: rulers, units, pixel grid, appearance
        toolChanged()
        editorChanged()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// Shows the window, the panels, and fits the image into the clear area.
    public func present() {
        guard let window else { return }
        window.cascadeTopLeft(from: NSPoint(x: 40, y: (NSScreen.main?.visibleFrame.maxY ?? 800) - 40))
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(canvas)
        workspace.setActive(editor)
        workspace.panels.showAll()
        updateClearRect()
        strip.reload()
    }

    // MARK: Layout

    /// The stage (the canvas area) in screen coordinates.
    public var stageInScreen: CGRect? {
        guard let window, container.window != nil else { return nil }
        return window.convertToScreen(container.convert(container.bounds, to: nil))
    }

    /// Recomputes the part of the stage that panels leave clear and gives it to
    /// the canvas (a fitted image fits again; a chosen zoom stays).
    public func updateClearRect() {
        guard let stage = stageInScreen, let window else { return }
        let clear = PanelLayout.clearRect(stage: stage, panels: workspace.panels.shownFrames)
        let inWindow = window.convertFromScreen(clear)
        var r = canvas.convert(inWindow, from: nil)
        r = r.intersection(canvas.bounds)
        canvas.setClearRect(r.isNull || r.isEmpty ? nil : r)
    }

    // MARK: Canvas

    private func wireCanvas() {
        brush = BrushStrokeController(controller: self)
        canvas.strokeSink = brush
        selectCrop = SelectCropController(controller: self)
        moveTool = MoveToolController(controller: self)
        canvas.onArrowKey = { [weak self] code, mods in self?.moveTool.arrow(keyCode: code, modifiers: mods) ?? false }
        flood = FloodToolController(controller: self)
        canvas.onClick = { [weak self] p in
            guard let self else { return }
            self.click(p, secondary: self.canvas.pressUsesSecondary, modifiers: self.canvas.pressModifiers)
        }
        canvas.onCursor = { [weak self] p in
            self?.cursor = p
            self?.container.rulerCursor = p
            self?.updateStatus()
        }
        canvas.onToolKey = { [weak self] c in
            guard let self else { return false }
            if self.colorKey(c) { return true }
            guard let t = PaintTool.tool(forKey: c, current: self.workspace.tools.tool) else { return false }
            self.workspace.tools.tool = t
            return true
        }
        canvas.onKey = { [weak self] key in self?.canvasKey(key) }
        canvas.onToolChange = { [weak self] t in
            // Keys handled inside the canvas (Z, H) keep the shared state in step.
            guard let self else { return }
            if t == .zoom, self.workspace.tools.tool != .zoom { self.workspace.tools.tool = .zoom }
            if t == .pan, self.workspace.tools.tool != .pan { self.workspace.tools.tool = .pan }
        }
        container.onViewportChange = { [weak self] _ in
            self?.updateStatus()
            self?.textTool?.viewportChanged()
            self?.selectCrop?.viewportChanged()
            self?.moveTool?.viewportChanged()
            self?.flood?.viewportChanged()
        }
        // scripts/launch-time.sh waits for this line (the trace replaces the hook).
        canvas.onFirstFrame = { LaunchClock.markReady() }
    }

    /// A whole stroke in one `tool.stroke` command (scripts and tests; the
    /// canvas sends live strokes through `brush`).
    func stroke(_ points: [CGPoint], secondary: Bool = false) {
        let tool = workspace.tools.tool
        guard tool.strokeName != nil, !points.isEmpty else { return }
        var p = workspace.tools.strokeParams(tool: tool, secondary: secondary)
        p["points"] = points.map { String(format: "%.2f %.2f 1", $0.x, $0.y) }.joined(separator: " ")
        editor.run("tool.stroke", p)
    }

    func click(_ point: CGPoint, secondary: Bool = false, modifiers: NSEvent.ModifierFlags = []) {
        let tool = workspace.tools.tool
        if tool == .colorPicker { return pickColor(at: point, secondary: secondary, modifiers: modifiers) }
        if tool == .text { return textTool.click(at: point) }
        // Magic Wand and Paint Bucket: the result stays live until Finish (M31).
        flood.click(at: point, secondary: secondary)
    }

    /// While text is open, Escape and Enter go to the Text tool. Otherwise Escape
    /// cancels a drag, else deselects; Enter finishes the Crop tool, else
    /// deselects (paint.net's Enter). Delete is Erase Selection and
    /// Shift-Delete is Fill Selection (M29, SelectionEditActions.swift).
    public func canvasKey(_ key: CanvasKey) {
        if textTool?.isEditing == true {
            switch key {
            case .escape: return textTool.escape()
            case .enter: return textTool.enter()
            case .delete, .fill: return textTool.apply { $0.deleteForward() }
            }
        }
        switch key {
        case .escape:
            if canvas.cancelStroke() { return }
            if moveTool.escape() { return }
            if flood.cancel() { return }
            if selectCrop.escape() { return }
            if editor.selection != nil { editor.run("selection.deselect") }
        case .enter:
            if moveTool.enter() { return }
            if flood.finish() { return }
            if selectCrop.enter() { return }
            editor.clearMessage()
            if editor.selection != nil, editor.isEnabled("selection.deselect") { editor.run("selection.deselect") }
        case .delete:
            eraseSelection()
        case .fill:
            fillSelection()
        }
    }

    func toolChanged() {
        let t = workspace.tools.tool
        let mode: CanvasTool =
            if selectCrop?.cropActive == true { .overlay } else {
            switch t {
            case .rectangleSelect, .ellipseSelect, .lassoSelect, .moveSelectedPixels, .moveSelection: .overlay
            case .pan: .pan
            case .zoom: .zoom
            case .paintbrush, .eraser, .pencil: .stroke
            case .magicWand, .paintBucket, .colorPicker, .text: .click
            default: .inactive
            }
            }
        if canvas.tool != mode { canvas.tool = mode }
        editor.clearMessage()
        updateStatus()
    }

    // MARK: State to views

    private func workspaceChanged() {
        strip.reload()
        for (k, b) in panelToggles { b.isOn = workspace.panels.isShown(k) }
        updateClearRect()
    }

    private func editorChanged() {
        window?.title = editor.name
        canvas.setDocumentSize(CGSize(width: editor.info.width, height: editor.info.height))
        viewDocumentChanged()
        if window?.isDocumentEdited != editor.info.dirty { window?.isDocumentEdited = editor.info.dirty }
        updateCommandStates()
        updateStatus()
    }

    func updateStatus() {
        let size = CGSize(width: editor.info.width, height: editor.info.height)
        statusBar.update(
            selection: editor.selection, cursor: cursor, imageSize: size, zoom: canvas.viewport.zoom,
            hint: editor.message ?? textTool?.notice ?? workspace.tools.tool.hint)
        statusBar.setProfile(short: ColorProfileNames.short(editor.info), full: ColorProfileNames.full(editor.info))
    }

    private func updateCommandStates() {
        let ids: [String: Bool] = [
            "crop": editor.isEnabled("image.crop_to_selection") && editor.selection != nil,
            "deselect": editor.selection != nil,
            "undo": canUndo,
            "redo": canRedo,
        ]
        for (k, on) in ids { commandButtons[k]?.isEnabled = on }
        updateViewStates()
    }

    private func zoomFromStatus(_ z: Double?) {
        guard let z else { return canvas.fit() }
        if z <= 0 { return updateStatus() }  // bad input: show the real zoom again
        var v = canvas.viewport
        v.setZoom(z)
        canvas.setViewport(v)
        window?.makeFirstResponder(canvas)
    }

    // MARK: Commands

    public var canUndo: Bool { editor.info.historyIndex > 0 }
    public var canRedo: Bool { editor.info.historyIndex + 1 < editor.info.historyLabels.count }

    public func undo() {
        // Undo while typing commits the text first, so Redo can bring it back.
        if textTool?.isEditing == true {
            textTool.finish()
            editor.run("edit.undo")
            return
        }
        // Undo while a fill or wand selection is live drops it (M31).
        if flood?.cancel() == true { return }
        // Undo during a move commits it first (one step), then undoes it.
        moveTool?.finish()
        if canUndo { editor.run("edit.undo") }
    }
    public func redo() { if canRedo { editor.run("edit.redo") } }
    public func selectAllPixels() { editor.run("selection.select_all") }

    public func addLayer() {
        editor.run("layer.add", ["name": "Layer \(editor.info.layers.count + 1)"])
    }

    @objc public func runCommand(_ sender: Any?) {
        guard let id = (sender as? NSMenuItem)?.representedObject as? String else { return }
        editor.run(id)
    }

    @objc public func runEffect(_ sender: Any?) {
        guard let id = (sender as? NSMenuItem)?.representedObject as? String else { return }
        editor.run(id)  // defaults; effect dialogs come in M24
    }

    @objc public func addLayer(_ sender: Any?) { addLayer() }

    @objc public func goToLayer(_ sender: Any?) {
        guard let tag = (sender as? NSMenuItem)?.tag else { return }
        let count = editor.info.layers.count, active = editor.info.activeLayer
        let target = [count - 1, active + 1, active - 1, 0][min(max(tag, 0), 3)]
        guard target >= 0, target < count, target != active else { return }
        editor.run("layer.set_active", ["index": target])
    }

    @objc public func zoomToSelection(_ sender: Any?) {
        guard let s = editor.selection, s.width > 0, s.height > 0 else { return }
        var v = canvas.viewport
        v.zoom(toDocumentRect: s)
        canvas.setViewport(v)
    }

    @objc public func setUnits(_ sender: Any?) { chooseUnits(sender) }

    // Canvas commands reach the canvas even when another view has focus.
    @objc func zoomIn(_ sender: Any?) { canvas.zoomIn(sender) }
    @objc func zoomOut(_ sender: Any?) { canvas.zoomOut(sender) }
    @objc func zoomToWindow(_ sender: Any?) { canvas.zoomToWindow(sender) }
    @objc func actualSize(_ sender: Any?) { canvas.actualSize(sender) }
    @objc func togglePixelGrid(_ sender: Any?) {
        canvas.togglePixelGrid(sender)
        updateCommandStates()
    }

    public func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let busy = FocusRouter.target(in: window) == .dialog
        if let on = validateViewItem(item) { return on }
        switch item.action {
        case #selector(runCommand(_:)):
            guard let id = item.representedObject as? String else { return false }
            if id == "selection.deselect" || id == "image.crop_to_selection" {
                return !busy && editor.selection != nil && editor.isEnabled(id)
            }
            return !busy && editor.isEnabled(id)
        case #selector(runEffect(_:)):
            guard let id = item.representedObject as? String else { return false }
            if id == "effect.fill.content_aware" { return !busy && editor.selection != nil }
            return !busy && editor.isEnabled(id)
        case #selector(repeatEffect(_:)): return validateRepeat(item, busy: busy)
        case #selector(addLayer(_:)): return !busy && editor.isEnabled("layer.add")
        case #selector(toggleLayerVisibility(_:)), #selector(showLayerProperties(_:)): return !busy
        case #selector(showColorProfile(_:)): return !busy
        case #selector(selectForeground(_:)), #selector(removeBackground(_:)): return !busy  // M53
        case #selector(goToLayer(_:)):
            let count = editor.info.layers.count, active = editor.info.activeLayer
            return !busy && [active != count - 1, active < count - 1, active > 0, active != 0][min(max(item.tag, 0), 3)]
        case #selector(zoomToSelection(_:)): return editor.selection != nil
        case #selector(eraseSelection(_:)), #selector(fillSelection(_:)), #selector(copySelection(_:)),
             #selector(pasteSelection(_:)), #selector(showExpandSelection(_:)):
            return validateSelectionEdit(item)
        case #selector(ImageSizeActions.showCropTool(_:)): return !busy && !selectCrop.cropActive
        case #selector(ImageSizeActions.showResizeImage(_:)), #selector(ImageSizeActions.showCanvasSize(_:)):
            return !busy
        case #selector(togglePixelGrid(_:)):
            item.state = canvas.pixelGrid ? .on : .off
            return true
        case #selector(zoomIn(_:)): return canvas.viewport.zoom < ZoomPresets.maximum
        case #selector(zoomOut(_:)): return canvas.viewport.zoom > ZoomPresets.minimum
        default: return true
        }
    }

    // MARK: Toolbar

    static let commandsID = NSToolbarItem.Identifier("commands")
    static let panelsID = NSToolbarItem.Identifier("panels")

    public func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [Self.commandsID, .flexibleSpace, Self.panelsID]
    }

    public func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    public func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier,
                        willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: id)
        if id == Self.commandsID {
            item.view = commandStack()
            item.label = "Commands"
        } else if id == Self.panelsID {
            item.view = panelStack()
            item.label = "Panels"
        } else {
            return nil
        }
        return item
    }

    /// paint.net order: New, Open, Save, Print | Cut, Copy, Paste | Crop to
    /// Selection, Deselect | Undo, Redo | Pixel Grid, Rulers. 6 pt gaps inside a
    /// group, 12 pt between groups.
    private func commandStack() -> NSStackView {
        typealias C = (key: String, symbol: String, label: String, shortcut: String, action: Selector?)
        let groups: [[C]] = [
            [("new", "doc.badge.plus", "New", "⌘N", nil),
             ("open", "folder", "Open", "⌘O", #selector(openDocument(_:))),
             ("save", "square.and.arrow.down", "Save", "⌘S", nil),
             ("print", "printer", "Print", "⌘P", nil)],
            [("cut", "scissors", "Cut", "⌘X", nil),
             ("copy", "doc.on.doc", "Copy", "⌘C", nil),
             ("paste", "doc.on.clipboard", "Paste", "⌘V", nil)],
            [("crop", "crop", "Crop to Selection", "⇧⌘X", #selector(cropClicked(_:))),
             ("deselect", "rectangle.dashed.badge.record", "Deselect", "⌘D", #selector(deselectClicked(_:)))],
            [("undo", "arrow.uturn.backward", "Undo", "⌘Z", #selector(undoClicked(_:))),
             ("redo", "arrow.uturn.forward", "Redo", "⇧⌘Z", #selector(redoClicked(_:)))],
            [("grid", "grid", "Pixel Grid", "⌘'", #selector(togglePixelGrid(_:))),
             ("rulers", "ruler", "Rulers", "", #selector(toggleRulers(_:)))],
        ]
        let outer = NSStackView()
        outer.translatesAutoresizingMaskIntoConstraints = false
        outer.orientation = .horizontal
        outer.spacing = 12
        for g in groups {
            let inner = NSStackView()
            inner.translatesAutoresizingMaskIntoConstraints = false
            inner.orientation = .horizontal
            inner.spacing = 6
            for c in g {
                let tip = c.shortcut.isEmpty ? c.label : "\(c.label) (\(c.shortcut))"
                let b: ShellButton = c.key == "grid" || c.key == "rulers"
                    ? ToggleButton(symbol: c.symbol, label: c.label, tooltip: tip, target: self, action: c.action)
                    : ShellButton(symbol: c.symbol, label: c.label, tooltip: c.action == nil ? "\(tip), comes later" : tip,
                                  target: self, action: c.action)
                b.isEnabled = c.action != nil
                commandButtons[c.key] = b
                inner.addArrangedSubview(b)
            }
            outer.addArrangedSubview(inner)
        }
        updateCommandStates()
        return outer
    }

    private func panelStack() -> NSStackView {
        let stack = NSStackView()
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.orientation = .horizontal
        stack.spacing = 6
        for k in PanelKind.allCases {
            let b = ToggleButton(symbol: k.symbol, label: k.title, tooltip: "\(k.title) (F\(k.functionKey), ⌥⌘\(k.digit))",
                                 target: workspace.menuTarget, action: #selector(PanelMenuTarget.togglePanel(_:)))
            b.tag = k.digit
            b.isOn = workspace.panels.isShown(k)
            panelToggles[k] = b
            stack.addArrangedSubview(b)
        }
        return stack
    }

    @objc private func openDocument(_ sender: Any?) { NSDocumentController.shared.openDocument(sender) }
    @objc private func cropClicked(_ sender: Any?) { editor.run("image.crop_to_selection") }
    @objc private func deselectClicked(_ sender: Any?) { editor.run("selection.deselect") }
    @objc private func undoClicked(_ sender: Any?) { undo() }
    @objc private func redoClicked(_ sender: Any?) { redo() }

    // MARK: NSWindowDelegate

    public func windowDidBecomeMain(_ notification: Notification) {
        workspace.setActive(editor)
    }

    public func windowDidResize(_ notification: Notification) { updateClearRect() }
    public func windowDidMove(_ notification: Notification) { updateClearRect() }

    public func windowWillClose(_ notification: Notification) {
        canvas.detach()
        workspace.remove(editor)
    }

    /// Closes the window without the document's save flow (tests, no host).
    public func closeWindow() {
        window?.close()
    }
}

/// The window's content: options row, image strip, stage, status bar.
final class ShellRootView: NSView {
    unowned let controller: EditorWindowController

    init(controller: EditorWindowController) {
        self.controller = controller
        super.init(frame: NSRect(x: 0, y: 0, width: 1100, height: 740))
        addSubview(controller.optionsBar)
        addSubview(controller.strip)
        addSubview(controller.container)
        addSubview(controller.statusBar)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    override var isFlipped: Bool { true }

    override func layout() {
        super.layout()
        let w = bounds.width
        var y: CGFloat = 0
        controller.optionsBar.frame = NSRect(x: 0, y: y, width: w, height: Theme.optionsHeight)
        y += Theme.optionsHeight
        controller.strip.frame = NSRect(x: 0, y: y, width: w, height: Theme.stripHeight)
        y += Theme.stripHeight
        let stageHeight = max(bounds.height - y - Theme.statusHeight, 1)
        controller.container.frame = NSRect(x: 0, y: y, width: w, height: stageHeight)
        controller.statusBar.frame = NSRect(x: 0, y: y + stageHeight, width: w, height: Theme.statusHeight)
        controller.updateClearRect()
    }
}

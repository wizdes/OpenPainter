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
    private var cursor: CGPoint?
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
        workspace.tools.observe(self) { [weak self] in self?.toolChanged() }
        workspace.observe(self) { [weak self] in self?.workspaceChanged() }
        editor.observe(self) { [weak self] in self?.editorChanged() }
        statusBar.onZoom = { [weak self] z in self?.zoomFromStatus(z) }
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
        canvas.onStroke = { [weak self] points in
            guard let self else { return }
            self.stroke(points, secondary: self.canvas.pressUsesSecondary)
        }
        canvas.onClick = { [weak self] p in
            guard let self else { return }
            self.click(p, secondary: self.canvas.pressUsesSecondary, modifiers: self.canvas.pressModifiers)
        }
        canvas.onCursor = { [weak self] p in
            self?.cursor = p
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
        container.onViewportChange = { [weak self] _ in self?.updateStatus() }
        // scripts/launch-time.sh waits for this line (the trace replaces the hook).
        canvas.onFirstFrame = { LaunchClock.markReady() }
    }

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
        guard tool == .magicWand || tool == .paintBucket else { return }
        let x = Int(point.x.rounded(.down)), y = Int(point.y.rounded(.down))
        guard x >= 0, y >= 0, x < editor.info.width, y < editor.info.height else { return }
        editor.run(tool == .magicWand ? "tool.magic_wand" : "tool.paint_bucket",
                   workspace.tools.floodParams(tool: tool, x: x, y: y, secondary: secondary))
    }

    /// Escape cancels a drag, else deselects. Enter has nothing to finish yet.
    /// Delete is Erase Selection, which comes in a later version.
    public func canvasKey(_ key: CanvasKey) {
        switch key {
        case .escape:
            if canvas.cancelStroke() { return }
            if editor.selection != nil { editor.run("selection.deselect") }
        case .enter:
            editor.clearMessage()
        case .delete:
            editor.report("Erase Selection comes in a later version.")
        }
    }

    private func toolChanged() {
        let t = workspace.tools.tool
        let mode: CanvasTool =
            switch t {
            case .pan: .pan
            case .zoom: .zoom
            case .paintbrush, .eraser, .pencil: .stroke
            case .magicWand, .paintBucket, .colorPicker: .click
            default: .inactive
            }
        if canvas.tool != mode { canvas.tool = mode }
        container.strokeWidthPixels = workspace.tools.settings.brushWidth
        container.strokeColor = t == .eraser
            ? NSColor.white.withAlphaComponent(0.6) : workspace.tools.primary.nsColor
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
        if window?.isDocumentEdited != editor.info.dirty { window?.isDocumentEdited = editor.info.dirty }
        updateCommandStates()
        updateStatus()
    }

    func updateStatus() {
        let size = CGSize(width: editor.info.width, height: editor.info.height)
        statusBar.update(
            selection: editor.selection, cursor: cursor, imageSize: size, zoom: canvas.viewport.zoom,
            hint: editor.message ?? workspace.tools.tool.hint)
    }

    private func updateCommandStates() {
        let ids: [String: Bool] = [
            "crop": editor.isEnabled("image.crop_to_selection") && editor.selection != nil,
            "deselect": editor.selection != nil,
            "undo": canUndo,
            "redo": canRedo,
        ]
        for (k, on) in ids { commandButtons[k]?.isEnabled = on }
        (commandButtons["grid"] as? ToggleButton)?.isOn = canvas.pixelGrid
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

    public func undo() { if canUndo { editor.run("edit.undo") } }
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

    @objc public func setUnits(_ sender: Any?) {}

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
        case #selector(addLayer(_:)): return !busy && editor.isEnabled("layer.add")
        case #selector(goToLayer(_:)):
            let count = editor.info.layers.count, active = editor.info.activeLayer
            return !busy && [active != count - 1, active < count - 1, active > 0, active != 0][min(max(item.tag, 0), 3)]
        case #selector(zoomToSelection(_:)): return editor.selection != nil
        case #selector(setUnits(_:)):
            item.state = .on
            return true
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
             ("rulers", "ruler", "Rulers", "", nil)],
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
                let b: ShellButton = c.key == "grid"
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

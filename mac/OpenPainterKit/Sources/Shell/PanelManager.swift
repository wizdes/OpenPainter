// The four floating panels as NSPanel utility windows: non-activating, hidden
// while the app is inactive, closed with the system close button on the left.
// Shown state, positions per display arrangement, overlap and reachability
// rules live here; the geometry is in PanelLayout.
import AppKit

/// A utility panel whose key commands follow the focus rules (mac/README.md):
/// without a text field focused, Undo, Redo, Select All, Delete and Escape go
/// to the active document.
public final class ShellPanel: NSPanel {
    public let kind: PanelKind
    /// Where routed commands go (the workspace's active editor window).
    var router: (() -> NSWindow?)?

    init(kind: PanelKind) {
        self.kind = kind
        super.init(
            contentRect: NSRect(origin: .zero, size: kind.defaultSize),
            styleMask: [.titled, .closable, .utilityWindow, .nonactivatingPanel, .resizable],
            backing: .buffered, defer: true)
        title = kind.title
        isFloatingPanel = true
        becomesKeyOnlyIfNeeded = true
        isReleasedWhenClosed = false
        isRestorable = false
        hidesOnDeactivate = true
        collectionBehavior = [.fullScreenAuxiliary, .moveToActiveSpace]
        minSize = kind.minimumSize
        if kind == .colors { maxSize = NSSize(width: kind.minimumSize.width, height: 470) }
        if kind == .tools { maxSize = NSSize(width: kind.defaultSize.width, height: 10_000) }
        setAccessibilityLabel("\(kind.title) panel")
        // Only the close button, on the left (SPEC).
        standardWindowButton(.miniaturizeButton)?.isHidden = true
        standardWindowButton(.zoomButton)?.isHidden = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func forward(_ action: Selector, _ sender: Any?) -> Bool {
        guard FocusRouter.target(in: self) == .canvas, let w = router?() else { return false }
        return w.firstResponder?.tryToPerform(action, with: sender) ?? false
    }

    @objc public override func selectAll(_ sender: Any?) {
        if !forward(#selector(selectAll(_:)), sender) { super.selectAll(sender) }
    }

    @objc public func undo(_ sender: Any?) {
        if !forward(#selector(EditorWindow.undo(_:)), sender) { undoManager?.undo() }
    }

    @objc public func redo(_ sender: Any?) {
        if !forward(#selector(EditorWindow.redo(_:)), sender) { undoManager?.redo() }
    }

    public override func cancelOperation(_ sender: Any?) {
        if !forward(#selector(cancelOperation(_:)), sender) { super.cancelOperation(sender) }
    }

    /// Undo, Redo and Select All are enabled as the active document window says
    /// while they forward to it.
    public override func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        let forwarded: [Selector] = [#selector(undo(_:)), #selector(redo(_:)), #selector(selectAll(_:))]
        if let action = item.action, forwarded.contains(action), FocusRouter.target(in: self) == .canvas {
            guard let w = router?() else { return false }
            return w.validateUserInterfaceItem(item)
        }
        return super.validateUserInterfaceItem(item)
    }
}

@MainActor
public final class PanelManager: NSObject, NSWindowDelegate {
    public let panels: [PanelKind: ShellPanel]
    public private(set) var shown: Set<PanelKind> = Set(PanelKind.allCases)
    /// Called after a panel shows, hides, moves or resizes.
    public var onChange: (() -> Void)?
    /// Visible frames of the screens (injected by tests).
    public var screens: () -> [CGRect] = { NSScreen.screens.map(\.visibleFrame) }
    /// Full frames of the screens, for the arrangement key.
    public var screenFrames: () -> [CGRect] = { NSScreen.screens.map(\.frame) }
    /// The active window's stage in screen coordinates, for default places.
    public var stage: () -> CGRect? = { nil }
    let store: PanelStore
    /// True once frames are placed (restored, or defaults against a stage).
    public private(set) var placed = false
    private var applying = false

    public init(store: PanelStore = PanelStore(), hidesOnDeactivate: Bool = true) {
        self.store = store
        var p: [PanelKind: ShellPanel] = [:]
        for k in PanelKind.allCases { p[k] = ShellPanel(kind: k) }
        panels = p
        super.init()
        for panel in p.values {
            panel.delegate = self
            panel.hidesOnDeactivate = hidesOnDeactivate
        }
    }

    public func isShown(_ k: PanelKind) -> Bool { shown.contains(k) }

    /// Default frame sizes. Tools is as tall as its tool list needs (no empty
    /// space under the last row); the others use the spec sizes.
    public var defaultSizes: [PanelKind: CGSize] {
        var out: [PanelKind: CGSize] = [:]
        for k in PanelKind.allCases { out[k] = k.defaultSize }
        let tools = panels[.tools]!.frameRect(
            forContentRect: NSRect(x: 0, y: 0, width: PanelKind.tools.defaultSize.width,
                                   height: ToolsPanelView.fittingHeight))
        out[.tools] = CGSize(width: PanelKind.tools.defaultSize.width, height: tools.height)
        return out
    }

    public func frame(_ k: PanelKind) -> CGRect { panels[k]!.frame }

    /// Frames of the panels that are shown.
    public var shownFrames: [CGRect] { PanelKind.allCases.filter(isShown).map(frame) }

    public func toggle(_ k: PanelKind) { setShown(k, !isShown(k)) }

    public func setShown(_ k: PanelKind, _ on: Bool) {
        if on { shown.insert(k) } else { shown.remove(k) }
        apply(k)
        save()
        onChange?()
    }

    /// Orders the shown panels front (when a document window exists).
    public func showAll() {
        placeIfNeeded()
        for k in PanelKind.allCases { apply(k) }
    }

    public func hideAll() {
        applying = true
        for p in panels.values { p.orderOut(nil) }
        applying = false
    }

    private func apply(_ k: PanelKind) {
        let p = panels[k]!
        applying = true
        defer { applying = false }
        if isShown(k) {
            placeIfNeeded()
            p.orderFront(nil)
        } else {
            p.orderOut(nil)
        }
    }

    // MARK: Places

    /// Restores the saved layout for this display arrangement, or the last saved
    /// one (moved onto a visible screen), or the defaults against the stage.
    public func placeIfNeeded() {
        guard !placed else { return }
        if restore() { return }
        guard let stage = stage() ?? NSScreen.main?.visibleFrame else { return }
        setFrames(PanelLayout.defaults(stage: stage, sizes: defaultSizes))
        placed = true
        save()
    }

    /// Reads the saved record. Returns false when there is none.
    @discardableResult
    public func restore() -> Bool {
        let key = PanelLayout.arrangementKey(screenFrames())
        guard let (record, _) = store.load(arrangement: key) else { return false }
        var frames = record.frames
        let visible = screens()
        for (k, r) in frames { frames[k] = PanelLayout.onScreen(r, screens: visible) }
        // A panel that is missing from the record takes its default place.
        if let stage = stage() ?? visible.first {
            let d = PanelLayout.defaults(stage: stage, sizes: defaultSizes)
            for k in PanelKind.allCases where frames[k] == nil { frames[k] = d[k] }
        }
        shown = record.visible
        setFrames(frames)
        resolveAll()
        placed = true
        onChange?()
        return true
    }

    /// Window > Reset Panel Layout: default places, all panels shown.
    public func resetLayout() {
        guard let stage = stage() ?? screens().first else { return }
        shown = Set(PanelKind.allCases)
        setFrames(PanelLayout.defaults(stage: stage, sizes: defaultSizes))
        placed = true
        for k in PanelKind.allCases { apply(k) }
        save()
        onChange?()
    }

    /// The display arrangement changed: use its saved layout, or keep the
    /// current one and bring unreachable panels back onto a screen.
    public func screensChanged() {
        let key = PanelLayout.arrangementKey(screenFrames())
        let showing = panels.values.contains { $0.isVisible }
        if let (_, exact) = store.load(arrangement: key), exact {
            placed = false
            restore()
            if showing { showAll() }
            return
        }
        let visible = screens()
        var frames: [PanelKind: CGRect] = [:]
        for k in PanelKind.allCases { frames[k] = PanelLayout.onScreen(frame(k), screens: visible) }
        setFrames(frames)
        resolveAll()
        save()
        onChange?()
    }

    func setFrames(_ frames: [PanelKind: CGRect]) {
        applying = true
        defer { applying = false }
        for (k, r) in frames { panels[k]!.setFrame(r, display: false) }
    }

    /// Moves panels apart until none overlap, keeping them on screen.
    private func resolveAll() {
        let visible = screens()
        for k in PanelKind.allCases {
            let others = PanelKind.allCases.filter { $0 != k }.map(frame)
            let bounds = visible.first { $0.intersects(frame(k)) } ?? visible.first ?? frame(k)
            let r = PanelLayout.resolve(frame(k), against: others, in: bounds)
            if r != frame(k) {
                applying = true
                panels[k]!.setFrame(r, display: false)
                applying = false
            }
        }
    }

    public func save() {
        guard placed else { return }
        var frames: [PanelKind: CGRect] = [:]
        for k in PanelKind.allCases { frames[k] = frame(k) }
        store.save(
            PanelStore.Record(frames: frames, visible: shown), arrangement: PanelLayout.arrangementKey(screenFrames()))
    }

    /// A user move or resize: keep it clear of the other panels and on screen.
    func userMoved(_ k: PanelKind) {
        let others = PanelKind.allCases.filter { $0 != k }.map(frame)
        let visible = screens()
        let current = frame(k)
        let bounds = visible.max { a, b in
            let ia = a.intersection(current), ib = b.intersection(current)
            return (ia.isNull ? 0 : ia.width * ia.height) < (ib.isNull ? 0 : ib.width * ib.height)
        } ?? current
        let r = PanelLayout.resolve(current, against: others, in: bounds)
        if r != current {
            applying = true
            panels[k]!.setFrame(r, display: true)
            applying = false
        }
        save()
        onChange?()
    }

    // MARK: NSWindowDelegate

    public func windowDidMove(_ notification: Notification) {
        guard !applying, let p = notification.object as? ShellPanel else { return }
        userMoved(p.kind)
    }

    public func windowDidEndLiveResize(_ notification: Notification) {
        guard !applying, let p = notification.object as? ShellPanel else { return }
        userMoved(p.kind)
    }

    public func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard let p = sender as? ShellPanel else { return true }
        // The close button hides the panel and clears its toolbar toggle.
        setShown(p.kind, false)
        return false
    }
}

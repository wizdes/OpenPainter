// The document canvas: an NSView whose backing layer is a CAMetalLayer that the
// engine draws into (path A, docs/presenter-decision.md). This view owns the
// viewport (zoom and scroll), turns input into viewport changes, and sends the
// view state and the screen's ICC profile to the engine.
import AppKit
@_exported import EngineBridge
import QuartzCore

public enum CanvasTool: Int, Sendable {
    case pan = 0
    case zoom = 1
}

@MainActor
public final class MetalCanvasView: NSView, NSMenuItemValidation {
    public let document: Document
    public private(set) var viewport: Viewport
    public private(set) var engineView: EngineView?
    public let metalLayer = CAMetalLayer()

    public var tool: CanvasTool = .pan {
        didSet {
            updateCursor()
            onToolChange?(tool)
        }
    }
    /// Called when the tool changes (keys Z and H, the Tools menu).
    public var onToolChange: ((CanvasTool) -> Void)?
    public var pixelGrid = false {
        didSet { if pixelGrid != oldValue { push() } }
    }
    /// Called after every viewport change (scroll bars, zoom label).
    public var onViewportChange: ((Viewport) -> Void)?
    /// Called on the main thread once the first frame of the document is presented.
    public var onFirstFrame: (() -> Void)?
    /// Called while the Zoom tool drags a rectangle (view points), nil when done.
    public var onZoomRectangle: ((CGRect?) -> Void)?
    /// Called when an engine call fails (the message is also printed).
    public var onError: ((String) -> Void)?

    /// Tests only: use these ICC bytes instead of the screen's (simulates a screen
    /// move or a display profile change on a Mac with one display).
    public var displayICCOverride: Data?
    private var sentICC: Data?
    private var pendingICC: Data?
    private var suspended = false
    private var spaceHeld = false
    private var dragStart: CGPoint?
    private var lastDrag: CGPoint?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []
    private var waitingForFirstFrame = false
    private var fitOnFirstLayout = true

    public init(document: Document, documentSize: CGSize) {
        self.document = document
        self.viewport = Viewport(documentSize: documentSize, viewSize: CGSize(width: 1, height: 1))
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        metalLayer.isOpaque = true
        metalLayer.pixelFormat = .bgra8Unorm
        metalLayer.needsDisplayOnBoundsChange = false
        metalLayer.contentsGravity = .topLeft
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func makeBackingLayer() -> CALayer { metalLayer }
    public override var wantsUpdateLayer: Bool { true }
    public override func updateLayer() {}
    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }
    public override var isOpaque: Bool { true }

    // MARK: Engine view lifetime

    /// Attaches the engine view. Called when the view joins a window.
    public func attach() throws {
        guard engineView == nil else { return }
        engineView = try EngineView(document: document, layer: metalLayer)
        waitingForFirstFrame = true
        push()
        waitForFirstFrame()
    }

    /// Detaches: the engine finishes its in-flight frame and releases the layer.
    public func detach() {
        removeObservers()
        guard let v = engineView else { return }
        engineView = nil
        do { try v.detach() } catch { report("detach: \(error)") }
    }

    private func waitForFirstFrame() {
        guard let v = engineView else { return }
        Thread.detachNewThread { [weak self] in
            let ok = (try? v.waitFrames(1, timeout: .seconds(20))) ?? false
            DispatchQueue.main.async {
                guard let self, self.waitingForFirstFrame else { return }
                self.waitingForFirstFrame = false
                if ok {
                    self.onFirstFrame?()
                } else {
                    self.report("no frame was presented within 20 s")
                }
            }
        }
    }

    private func report(_ message: String) {
        FileHandle.standardError.write(Data("OpenPainter canvas: \(message)\n".utf8))
        onError?(message)
    }

    // MARK: Window, screen and size changes

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        removeObservers()
        guard let window else {
            detach()
            return
        }
        window.displaysWhenScreenProfileChanges = true
        let center = NotificationCenter.default
        let workspace = NSWorkspace.shared.notificationCenter
        let refresh: @Sendable (Notification) -> Void = { [weak self] _ in
            MainActor.assumeIsolated { self?.screenChanged() }
        }
        for name in [
            NSWindow.didChangeScreenNotification,
            NSWindow.didChangeScreenProfileNotification,
            NSWindow.didChangeBackingPropertiesNotification,
        ] {
            observers.append((center, center.addObserver(forName: name, object: window, queue: .main, using: refresh)))
        }
        observers.append(
            (center, center.addObserver(
                forName: NSScreen.colorSpaceDidChangeNotification, object: nil, queue: .main, using: refresh)))
        observers.append(
            (center, center.addObserver(
                forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main,
                using: refresh)))
        observers.append(
            (workspace, workspace.addObserver(
                forName: NSWorkspace.willSleepNotification, object: nil, queue: .main
            ) { @Sendable [weak self] _ in MainActor.assumeIsolated { self?.setSuspended(true) } }))
        observers.append(
            (workspace, workspace.addObserver(
                forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
            ) { @Sendable [weak self] _ in MainActor.assumeIsolated { self?.setSuspended(false) } }))
        screenChanged()
        do { try attach() } catch { report("attach: \(error)") }
    }

    private func removeObservers() {
        for (center, token) in observers { center.removeObserver(token) }
        observers.removeAll()
    }

    /// Re-reads the backing scale and the screen's color space; sends any change.
    public func screenChanged() {
        if let window {
            metalLayer.contentsScale = window.backingScaleFactor
        }
        let icc =
            displayICCOverride
            ?? (window?.screen ?? NSScreen.main)?.colorSpace?.cgColorSpace?.copyICCData() as Data?
        if let icc, icc != sentICC { pendingICC = icc }
        push()
    }

    /// Simulated or real sleep: the engine drops the surface and rebuilds it on wake.
    public func setSuspended(_ on: Bool) {
        suspended = on
        push()
    }

    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        screenChanged()
    }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        var v = viewport
        v.resize(to: newSize)
        if fitOnFirstLayout, newSize.width > 20, newSize.height > 20 {
            fitOnFirstLayout = false
            v.zoomToFit()
        }
        viewport = v
        push()
    }

    // MARK: State to the engine

    /// Replaces the viewport (menu commands, tools, the trace) and sends it.
    public func setViewport(_ v: Viewport) {
        viewport = v
        push()
    }

    public var backingScale: Double { Double(window?.backingScaleFactor ?? metalLayer.contentsScale) }

    public var engineState: CanvasViewState {
        CanvasViewState(
            widthPt: Double(max(bounds.width, 1)), heightPt: Double(max(bounds.height, 1)),
            scale: backingScale, zoom: viewport.zoom,
            originX: Double(viewport.origin.x), originY: Double(viewport.origin.y),
            pixelGrid: pixelGrid, suspended: suspended)
    }

    private func push() {
        onViewportChange?(viewport)
        guard let engineView, bounds.width >= 1, bounds.height >= 1 else { return }
        do {
            try engineView.set(engineState, displayICC: pendingICC)
            if let icc = pendingICC {
                sentICC = icc
                pendingICC = nil
            }
        } catch {
            report("op_view_set: \(error)")
        }
    }

    // MARK: Menu commands (View menu)

    @objc public func zoomIn(_ sender: Any?) { change { $0.zoomIn() } }
    @objc public func zoomOut(_ sender: Any?) { change { $0.zoomOut() } }
    @objc public func zoomToWindow(_ sender: Any?) { change { $0.zoomToFit() } }
    @objc public func actualSize(_ sender: Any?) { change { $0.actualSize() } }
    @objc public func togglePixelGrid(_ sender: Any?) { pixelGrid.toggle() }
    @objc public func selectPanTool(_ sender: Any?) { tool = .pan }
    @objc public func selectZoomTool(_ sender: Any?) { tool = .zoom }

    public func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(togglePixelGrid(_:)): item.state = pixelGrid ? .on : .off
        case #selector(selectPanTool(_:)): item.state = tool == .pan ? .on : .off
        case #selector(selectZoomTool(_:)): item.state = tool == .zoom ? .on : .off
        case #selector(zoomIn(_:)): return viewport.zoom < ZoomPresets.maximum
        case #selector(zoomOut(_:)): return viewport.zoom > ZoomPresets.minimum
        default: break
        }
        return true
    }

    private func change(_ body: (inout Viewport) -> Void) {
        var v = viewport
        body(&v)
        setViewport(v)
    }

    // MARK: Input

    private func point(_ event: NSEvent) -> CGPoint {
        convert(event.locationInWindow, from: nil)
    }

    private var effectiveTool: CanvasTool { spaceHeld ? .pan : tool }

    public override func scrollWheel(with event: NSEvent) {
        let p = point(event)
        if event.modifierFlags.contains(.command) {
            // paint.net: Ctrl + wheel zooms by 1.12 per notch around the cursor.
            let notches = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY / 10 : event.scrollingDeltaY
            change { $0.setZoom($0.zoom * pow(1.12, Double(notches)), keeping: p) }
            return
        }
        let line: CGFloat = event.hasPreciseScrollingDeltas ? 1 : 16
        var dx = event.scrollingDeltaX * line
        var dy = event.scrollingDeltaY * line
        if event.modifierFlags.contains(.shift), dx == 0 {
            dx = dy
            dy = 0
        }
        change { $0.pan(by: CGSize(width: dx, height: dy)) }
    }

    public override func magnify(with event: NSEvent) {
        let p = point(event)
        change { $0.setZoom($0.zoom * (1 + Double(event.magnification)), keeping: p) }
    }

    public override func smartMagnify(with event: NSEvent) {
        let p = point(event)
        change { v in
            if abs(v.zoom - 1) < 1e-9 { v.zoomToFit() } else { v.setZoom(1, keeping: p) }
        }
    }

    public override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let p = point(event)
        dragStart = p
        lastDrag = p
        if effectiveTool == .pan { NSCursor.closedHand.set() }
    }

    public override func mouseDragged(with event: NSEvent) {
        let p = point(event)
        guard let start = dragStart, let last = lastDrag else { return }
        lastDrag = p
        switch effectiveTool {
        case .pan:
            change { $0.pan(by: CGSize(width: p.x - last.x, height: p.y - last.y)) }
        case .zoom:
            onZoomRectangle?(CGRect(x: start.x, y: start.y, width: p.x - start.x, height: p.y - start.y).standardized)
        }
    }

    public override func mouseUp(with event: NSEvent) {
        let p = point(event)
        defer {
            dragStart = nil
            lastDrag = nil
            onZoomRectangle?(nil)
            updateCursor()
        }
        guard let start = dragStart, effectiveTool == .zoom else { return }
        let rect = CGRect(x: start.x, y: start.y, width: p.x - start.x, height: p.y - start.y).standardized
        if hypot(rect.width, rect.height) < 10 {
            // paint.net ZoomTool: click zooms one preset step and recenters on the point;
            // Option-click (right-click on Windows) steps out.
            if event.modifierFlags.contains(.option) {
                change { $0.zoomOut(at: p) }
            } else {
                change { $0.zoomIn(at: p) }
            }
        } else {
            let a = viewport.documentPoint(atView: rect.origin)
            let b = viewport.documentPoint(atView: CGPoint(x: rect.maxX, y: rect.maxY))
            change { $0.zoom(toDocumentRect: CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y)) }
        }
    }

    public override func rightMouseDown(with event: NSEvent) {
        guard effectiveTool == .zoom else { return super.rightMouseDown(with: event) }
        let p = point(event)
        change { $0.zoomOut(at: p) }
    }

    public override func keyDown(with event: NSEvent) {
        switch event.charactersIgnoringModifiers {
        case " ":
            if !event.isARepeat {
                spaceHeld = true
                updateCursor()
            }
        case "z" where event.modifierFlags.intersection([.command, .control, .option]).isEmpty:
            tool = .zoom
        case "h" where event.modifierFlags.intersection([.command, .control, .option]).isEmpty:
            tool = .pan
        default:
            super.keyDown(with: event)
        }
    }

    public override func keyUp(with event: NSEvent) {
        if event.charactersIgnoringModifiers == " " {
            spaceHeld = false
            updateCursor()
        } else {
            super.keyUp(with: event)
        }
    }

    private var cursor: NSCursor {
        switch effectiveTool {
        case .pan: return dragStart != nil ? .closedHand : .openHand
        case .zoom: return .crosshair
        }
    }

    private func updateCursor() {
        window?.invalidateCursorRects(for: self)
        if let window, window.isKeyWindow,
            bounds.contains(convert(window.mouseLocationOutsideOfEventStream, from: nil))
        {
            cursor.set()
        }
    }

    public override func resetCursorRects() {
        addCursorRect(bounds, cursor: cursor)
    }
}

// Move Selected Pixels and Move Selection on the canvas (M33). The canvas
// sends the pointer here (`CanvasTool.overlay`, with this controller as the
// pointer sink while a Move tool is current). The geometry is in
// MoveGeometry.swift; the engine work is the M32 move session
// (`op_move_begin`, `op_move_update`, `op_move_finish`, `op_move_cancel`).
//
// One session per tool use. The first press on the box (or the first arrow
// key) starts it; every drag sends the whole transform so far and the canvas
// shows the engine's preview. Finish commits exactly one history step:
// Enter, the Finish button, another tool, a click outside the box and its
// corridor, or any engine command (the engine finishes the move before it
// runs a command, and this controller sees the new revision). Escape or
// Cancel drops the session: pixels, selection and history stay as they were.
//
// Move Selected Pixels with no selection moves the whole layer (paint.net:
// "If no selection is active, the tool moves the entire active layer").
// Move Selection with no selection has nothing to move.
import AppKit

/// The engine's five move resampling modes. paint.net's Move Tools page
/// lists Nearest Neighbor, Bilinear, Multisample Bilinear, Anisotropic and
/// Bicubic (default Bicubic); the engine names its last two modes
/// Supersampling and Best Quality.
public enum MoveResampling: String, CaseIterable, Sendable {
    case nearest
    case bilinear
    case bicubic
    case superSampling = "super_sampling"
    case bestQuality = "best_quality"

    public var label: String {
        switch self {
        case .nearest: "Nearest Neighbor"
        case .bilinear: "Bilinear"
        case .bicubic: "Bicubic"
        case .superSampling: "Supersampling"
        case .bestQuality: "Best Quality"
        }
    }
}

@MainActor
public final class MoveToolController: NSObject, CanvasPointerSink {
    unowned let controller: EditorWindowController
    public let overlay = MoveOverlayView(frame: .zero)

    /// The session's box and transform; nil when no Move tool box shows.
    public private(set) var state: MoveState?
    /// True while an engine move session is open.
    public private(set) var active = false
    /// Command was held during a drag of this session: the move works on a copy.
    public private(set) var copy = false
    /// The session holds pasted pixels (adoptPaste).
    public private(set) var pasted = false
    /// Release review item 1: Finish would change the image or the
    /// selection (pasted pixels, a moved box, or a copy). The engine counts
    /// the same sessions as edits.
    public var hasChanges: Bool { active && (pasted || copy || (state.map { $0.m != .identity } ?? false)) }
    /// Engine updates sent in this session (tests).
    public private(set) var updatesSent = 0
    /// Arrow-key nudges in this session (tests, the status bar).
    public private(set) var nudges = 0
    private var drag: MoveDrag?
    private var kind = MoveKind.pixels
    private var baseRevision = -1
    private var basePath: CGPath?
    private var flushQueued = false
    /// The transform, copy flag and resampling of the last update sent.
    private var lastSent: String?
    private var lastTool: PaintTool?

    public let resamplingPopup = Labels.popup(MoveResampling.allCases.map(\.label))
    public private(set) var finishButton: NSButton!
    public private(set) var cancelButton: NSButton!
    private lazy var optionViews: [NSView] = [
        Labels.label("Resampling"), resamplingPopup, Labels.gap(8), cancelButton, finishButton,
    ]

    var editor: Editor { controller.editor }
    var canvas: MetalCanvasView { controller.canvas }
    var tools: ToolState { controller.workspace.tools }
    /// True while a Move tool is current.
    public var isMoveTool: Bool { tools.tool == .moveSelectedPixels || tools.tool == .moveSelection }

    init(controller: EditorWindowController) {
        self.controller = controller
        super.init()
        overlay.frame = controller.canvas.bounds
        overlay.autoresizingMask = [.width, .height]
        controller.canvas.addSubview(overlay)
        resamplingPopup.setAccessibilityLabel("Resampling")
        resamplingPopup.target = self
        resamplingPopup.action = #selector(resamplingChanged(_:))
        finishButton = NSButton(title: "Finish", target: self, action: #selector(finishClicked(_:)))
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancelClicked(_:)))
        for b in [finishButton!, cancelButton!] {
            b.font = Theme.font(12)
            b.bezelStyle = .rounded
        }
        finishButton.setAccessibilityLabel("Finish move (Enter)")
        cancelButton.setAccessibilityLabel("Cancel move (Escape)")
        tools.observe(self) { [weak self] in self?.toolsChanged() }
        editor.observe(self) { [weak self] in self?.editorChanged() }
        toolsChanged()
    }

    // MARK: Options row

    func syncOptions() {
        resamplingPopup.selectItem(at: MoveResampling.allCases.firstIndex(of: tools.settings.moveResampling) ?? 0)
        finishButton.isEnabled = active
        cancelButton.isEnabled = active
    }

    @objc private func resamplingChanged(_ sender: NSPopUpButton) {
        tools.settings.moveResampling = MoveResampling.allCases[max(sender.indexOfSelectedItem, 0)]
    }

    @objc private func finishClicked(_ sender: Any?) { finish() }
    @objc private func cancelClicked(_ sender: Any?) { cancel() }

    // MARK: State changes

    private func toolsChanged() {
        if tools.tool != lastTool {
            // Another tool finishes the move (one history step).
            if active { finish() }
            lastTool = tools.tool
            drag = nil
            if isMoveTool {
                canvas.pointerSink = self
            } else if canvas.pointerSink === self {
                canvas.pointerSink = controller.selectCrop
            }
        }
        if isMoveTool {
            if controller.optionsBar.customOptions.map({ $0.map(ObjectIdentifier.init) })
                != optionViews.map(ObjectIdentifier.init)
            {
                controller.optionsBar.customOptions = optionViews
            }
        } else if let shown = controller.optionsBar.customOptions, shown.contains(where: { $0 === resamplingPopup }) {
            controller.optionsBar.customOptions = nil
        }
        if active { flush() }  // the resampling mode may have changed
        syncOptions()
        if !active { resetBox() }
        redraw()
    }

    private func editorChanged() {
        if active, editor.info.revision != baseRevision {
            // The engine finished the move before a command (any command
            // finishes it), or the document changed under it.
            endSession()
        }
        if !active { resetBox() }
        redraw()
    }

    /// The box with no session: the selection bounds, or the whole layer for
    /// Move Selected Pixels with nothing selected.
    private func resetBox() {
        guard isMoveTool, editor.info.width > 0 else {
            state = nil
            return
        }
        let full = CGRect(x: 0, y: 0, width: editor.info.width, height: editor.info.height)
        let box = editor.selection ?? (tools.tool == .moveSelectedPixels ? full : nil)
        if let box, box.width > 0, box.height > 0 {
            if state?.base != box || state?.m != .identity { state = MoveState(base: box) }
        } else {
            state = nil
        }
    }

    public func viewportChanged() { redraw() }

    private func redraw() {
        overlay.viewport = canvas.viewport
        overlay.state = isMoveTool ? state : nil
        overlay.copyMode = copy
    }

    // MARK: Session

    /// Starts the engine session for the current tool. False when there is
    /// nothing to move or the engine is busy (the status bar says why).
    @discardableResult
    func begin() -> Bool {
        if active { return true }
        resetBox()
        guard isMoveTool, let state else {
            if isMoveTool { editor.report("\(tools.tool.label): nothing is selected") }
            return false
        }
        kind = tools.tool == .moveSelection ? .selection : .pixels
        do {
            try editor.document.moveBegin(kind, resampling: tools.settings.moveResampling.rawValue, copy: false)
        } catch {
            editor.report("\(tools.tool.label) could not start: \(error)")
            return false
        }
        active = true
        copy = false
        updatesSent = 0
        nudges = 0
        baseRevision = editor.info.revision
        basePath = controller.selectCrop.overlay.selectionPath
        self.state = state
        lastSent = sentKey  // begin sent the identity
        syncOptions()
        return true
    }

    /// Takes over the engine move session a paste started (M34,
    /// ClipboardActions.swift): `box` is the pasted bounds in document pixels.
    /// Drags, Finish and Escape then work as for any Move Selected Pixels session.
    func adoptPaste(box: CGRect) {
        kind = .pixels
        active = true
        pasted = true
        copy = false
        drag = nil
        updatesSent = 0
        nudges = 0
        baseRevision = editor.info.revision
        state = MoveState(base: box)
        basePath = CGPath(rect: box, transform: nil)
        controller.selectCrop.overlay.setSelectionPath(basePath)
        lastSent = sentKey  // paste began at the identity
        syncOptions()
        redraw()
    }

    /// Sends the current transform now (drags send at most once per main
    /// run-loop turn; a release, a nudge and Finish send at once).
    public func flush() {
        flushQueued = false
        guard active, sentKey != lastSent else { return }
        sendUpdate()
    }

    /// What an update would send now.
    private var sentKey: String? {
        guard let state else { return nil }
        return "\(state.engine.array) \(copy) \(tools.settings.moveResampling.rawValue)"
    }

    private func sendUpdate() {
        guard active, let state else { return }
        do {
            try editor.document.moveUpdate(
                transform: state.engine, resampling: tools.settings.moveResampling.rawValue, copy: copy)
            lastSent = sentKey
            updatesSent += 1
        } catch {
            editor.report("\(tools.tool.label) could not update: \(error)")
        }
        if let basePath {
            var m = state.m
            controller.selectCrop.overlay.setSelectionPath(basePath.copy(using: &m))
        }
    }

    private func queueFlush() {
        guard !flushQueued else { return }
        flushQueued = true
        // A run-loop block, not the main queue: it also runs inside a nested
        // run loop (a tracking loop or a test that spins the run loop).
        RunLoop.main.perform(inModes: [.common]) { [weak self] in MainActor.assumeIsolated { self?.flush() } }
    }

    /// Commits the session as one history step. False when none was open.
    @discardableResult
    public func finish() -> Bool {
        guard active else { return false }
        flush()
        do {
            try editor.document.moveFinish()
        } catch {
            editor.report("\(kind == .selection ? "Move Selection" : "Move Selected Pixels") failed: \(error)")
        }
        endSession()
        editor.refresh()
        controller.selectCrop.reloadSelectionPath()
        redraw()
        return true
    }

    /// Drops the session: pixels, selection and history stay as they were.
    /// False when none was open.
    @discardableResult
    public func cancel() -> Bool {
        guard active else { return false }
        do {
            try editor.document.moveCancel()
        } catch {
            editor.report("could not cancel the move: \(error)")
        }
        endSession()
        controller.selectCrop.reloadSelectionPath()
        resetBox()
        redraw()
        return true
    }

    private func endSession() {
        active = false
        pasted = false
        drag = nil
        flushQueued = false
        copy = false
        lastSent = nil
        basePath = nil
        state = nil
        editor.clearMessage()
        syncOptions()
    }

    // MARK: Keys

    /// Escape cancels the session. False when none was open.
    public func escape() -> Bool { cancel() }

    /// Enter finishes the session. False when none was open.
    public func enter() -> Bool { finish() }

    /// An arrow key: nudge by 1 px, or 10 px with Command. False when no Move tool is current.
    public func arrow(keyCode: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        guard isMoveTool, let v = MoveGeometry.nudge(keyCode: keyCode, modifiers: modifiers) else { return false }
        guard drag == nil, begin(), let s = state else { return true }
        state = MoveGeometry.nudged(s, dx: v.dx, dy: v.dy)
        nudges += 1
        flush()
        redraw()
        return true
    }

    // MARK: Pointer

    public func pointerDown(document p: CGPoint, view v: CGPoint, modifiers: NSEvent.ModifierFlags, right: Bool) {
        // The Crop tool (Image menu) can run while a Move tool is current.
        if controller.selectCrop.cropActive {
            return controller.selectCrop.pointerDown(document: p, view: v, modifiers: modifiers, right: right)
        }
        guard isMoveTool else { return }
        if !active { resetBox() }
        guard let s = state else {
            editor.report("\(tools.tool.label): nothing is selected")
            return
        }
        let vp = canvas.viewport
        let hit = MoveGeometry.hit(v, state: s) { vp.viewPoint(ofDocument: $0) }
        let kind: MoveDrag.Kind
        if right {
            // paint.net: the right button always rotates.
            kind = .rotate
        } else {
            switch hit {
            case let .nub(h): kind = .scale(h)
            case .pivot: kind = .pivot
            case .inside: kind = .move
            case .rotate: kind = .rotate
            case .outside:
                // A click outside the box and its corridor finishes the move.
                finish()
                return
            }
        }
        guard begin(), let from = state else { return }
        // paint.net's Ctrl (Command): the move works on a copy.
        if modifiers.contains(.command), self.kind == .pixels, !copy {
            copy = true
            redraw()
        }
        drag = MoveDrag(kind: kind, start: p, from: from)
    }

    public func pointerDragged(document p: CGPoint, view v: CGPoint, modifiers: NSEvent.ModifierFlags) {
        if controller.selectCrop.cropActive {
            return controller.selectCrop.pointerDragged(document: p, view: v, modifiers: modifiers)
        }
        guard let d = drag else { return }
        state = MoveGeometry.drag(d, to: p, shift: modifiers.contains(.shift), option: modifiers.contains(.option))
        if case .rotate = d.kind, let state {
            editor.report(String(format: "Rotate: %.1f°", state.angle))
        }
        queueFlush()
        redraw()
    }

    public func pointerUp(document p: CGPoint, view v: CGPoint, modifiers: NSEvent.ModifierFlags) {
        if controller.selectCrop.cropActive {
            return controller.selectCrop.pointerUp(document: p, view: v, modifiers: modifiers)
        }
        guard drag != nil else { return }
        pointerDragged(document: p, view: v, modifiers: modifiers)
        drag = nil
        flush()
        redraw()
    }
}

/// The Move tools' box: the outline of the moved box, the eight Control Nubs
/// and the pivot (a circle with a cross), in view points over the canvas.
@MainActor
public final class MoveOverlayView: NSView {
    let box = CAShapeLayer()
    let nubs = CAShapeLayer()
    let pivot = CAShapeLayer()

    public var state: MoveState? { didSet { if state != oldValue { redraw() } } }
    public var viewport: Viewport? { didSet { if viewport != oldValue { redraw() } } }
    /// Command copy: the box outline is dashed.
    public var copyMode = false { didSet { if copyMode != oldValue { redraw() } } }

    public override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = true
        for l in [box, pivot, nubs] {
            l.lineWidth = 1
            l.contentsScale = 2
            l.actions = ["path": NSNull(), "position": NSNull(), "bounds": NSNull(), "hidden": NSNull()]
            layer?.addSublayer(l)
        }
        box.fillColor = nil
        pivot.fillColor = NSColor.white.withAlphaComponent(0.85).cgColor
        nubs.fillColor = NSColor.white.cgColor
        updateColors()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    public override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateColors()
    }

    private func updateColors() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            let accent = Theme.accent.cgColor
            box.strokeColor = accent
            nubs.strokeColor = accent
            pivot.strokeColor = accent
        }
    }

    public override func layout() {
        super.layout()
        for l in [box, pivot, nubs] { l.frame = bounds }
        redraw()
    }

    func view(_ p: CGPoint) -> CGPoint {
        guard let v = viewport else { return p }
        return v.viewPoint(ofDocument: p)
    }

    /// The box outline as drawn, in view points (tests and renders).
    public var shownBox: CGPath? { box.path }

    func redraw() {
        guard let state else {
            box.path = nil
            nubs.path = nil
            pivot.path = nil
            return
        }
        let q = state.corners.map(view)
        let b = CGMutablePath()
        b.addLines(between: q)
        b.closeSubpath()
        box.path = b
        box.lineDashPattern = copyMode ? [4, 3] : nil
        let n = CGMutablePath()
        let size: CGFloat = 7
        for h in RectHandle.allCases {
            let c = view(state.nub(h))
            n.addRect(CGRect(x: (c.x - size / 2).rounded() + 0.5, y: (c.y - size / 2).rounded() + 0.5,
                             width: size - 1, height: size - 1))
        }
        nubs.path = n
        let c = view(state.pivot)
        let p = CGMutablePath()
        let r: CGFloat = 6
        p.addEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
        p.move(to: CGPoint(x: c.x - r - 3, y: c.y))
        p.addLine(to: CGPoint(x: c.x + r + 3, y: c.y))
        p.move(to: CGPoint(x: c.x, y: c.y - r - 3))
        p.addLine(to: CGPoint(x: c.x, y: c.y + r + 3))
        pivot.path = p
    }
}

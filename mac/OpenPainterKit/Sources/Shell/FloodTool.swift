// The live Magic Wand and Paint Bucket (M31). As in paint.net 5, a click
// does not finish the fill or the selection: it stays live, and changing the
// tolerance, the tolerance alpha, the flood mode, the sampling, the selection
// or blend mode, antialiasing or the fill color re-makes it from the same
// click point. The click point shows as a handle (paint.net's "control nub",
// a white square with four arrows) that can be dragged to move the click
// point. Finish (Enter, the Finish button, another tool, or a click elsewhere,
// which then starts a new one) keeps the result as one history step; Escape
// drops it and leaves the image as it was.
// Sources: https://www.getpaint.net/doc/latest/MagicWand.html and
// https://www.getpaint.net/doc/latest/PaintBucket.html ("Tolerance",
// "Dragging the click point", "Change color on-the-fly", "Tolerance Alpha Mode").
//
// The engine side is one preview session (`Document.floodBegin`,
// `floodUpdate`, `floodFinish`, `floodCancel`; engine/op-engine/src/flood_commands.rs).
// Every update sends the full parameters; the engine re-runs the flood on the
// unchanged document, so the finished step equals one plain `tool.magic_wand`
// or `tool.paint_bucket` command with the last parameters. The live wand
// selection is read with `doc.selection_path` and `{"preview": true}`.
import AppKit

/// The click point's handle, drawn over the canvas in view points.
@MainActor
public final class FloodHandleView: NSView {
    /// Side of the white square in points.
    static let size: CGFloat = 17
    /// The handle's center in view points; nil hides it.
    public var center: CGPoint? {
        didSet { if center != oldValue { needsDisplay = true } }
    }

    public override var isFlipped: Bool { true }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    /// True when a press at `p` (view points) grabs the handle: the square
    /// and 3 pt around it.
    public func grabs(_ p: CGPoint) -> Bool {
        guard let c = center else { return false }
        let r = Self.size / 2 + 3
        return abs(p.x - c.x) <= r && abs(p.y - c.y) <= r
    }

    public override func draw(_ dirtyRect: NSRect) {
        guard let c = center else { return }
        // Fixed colors, like the marching ants: the handle sits on the image,
        // not on the window's surfaces, so it looks the same in both appearances.
        let half = Self.size / 2
        let box = NSRect(x: (c.x - half).rounded() + 0.5, y: (c.y - half).rounded() + 0.5,
                         width: Self.size - 1, height: Self.size - 1)
        let square = NSBezierPath(roundedRect: box, xRadius: 2, yRadius: 2)
        NSColor.white.setFill()
        square.fill()
        NSColor(white: 0.13, alpha: 1).setStroke()
        square.lineWidth = 1
        square.stroke()
        // Four arrows from the center.
        let m = CGPoint(x: box.midX, y: box.midY)
        let arm: CGFloat = 5.5, head: CGFloat = 2.5
        let arrows = NSBezierPath()
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let tip = CGPoint(x: m.x + dx * arm, y: m.y + dy * arm)
            arrows.move(to: m)
            arrows.line(to: tip)
            arrows.move(to: CGPoint(x: tip.x - dx * head - dy * head, y: tip.y - dy * head - dx * head))
            arrows.line(to: tip)
            arrows.line(to: CGPoint(x: tip.x - dx * head + dy * head, y: tip.y - dy * head + dx * head))
        }
        arrows.lineWidth = 1.2
        arrows.lineCapStyle = .round
        arrows.lineJoinStyle = .round
        arrows.stroke()
    }
}

@MainActor
public final class FloodToolController {
    /// One open session: the tool, the click point (document pixel) and the
    /// button (a right-button Paint Bucket fill uses the secondary color).
    public struct Session: Equatable {
        public var tool: PaintTool
        public var x: Int
        public var y: Int
        public var secondary: Bool
        fileprivate var serial: Int
    }

    unowned let controller: EditorWindowController
    public let handle = FloodHandleView(frame: .zero)
    public private(set) var session: Session?
    /// Engine calls whose results have not come back yet (tests wait for 0).
    public private(set) var inFlight = 0
    public var isIdle: Bool { inFlight == 0 }
    private var sent: Data?
    private var serial = 0
    /// The document revision the open session started on, once known. A
    /// command from elsewhere (a menu, a panel) commits the session in the
    /// engine and moves the revision; the session then ends here too.
    private var baseRevision: Int?
    /// Fix round 4, item 2: the serial of the last session whose start the
    /// engine refused (the 16 GiB total has no room for the wand's mask).
    /// Later commands sent for that session fail too; the refusal already
    /// said why, so their failures are not shown over it.
    private var refusedSerial: Int?

    var editor: Editor { controller.editor }
    var tools: ToolState { controller.workspace.tools }
    var canvas: MetalCanvasView { controller.canvas }

    init(controller: EditorWindowController) {
        self.controller = controller
        handle.frame = controller.canvas.bounds
        handle.autoresizingMask = [.width, .height]
        controller.canvas.addSubview(handle)
        canvas.onClickPress = { [weak self] p in
            guard let self, self.session != nil, self.tools.tool.options == .flood else { return false }
            return self.handle.grabs(p)
        }
        canvas.onHandleDrag = { [weak self] p in self?.moveSeed(to: p) }
        canvas.onHandleRelease = { [weak self] p in self?.moveSeed(to: p) }
        controller.optionsBar.onFloodFinish = { [weak self] in self?.finish() }
        tools.observe(self) { [weak self] in self?.toolsChanged() }
        editor.observe(self) { [weak self] in self?.editorChanged() }
    }

    /// How long a fill or wand command may take before it counts as failed
    /// ("timed out"). Tests shorten it.
    public static var resultTimeout: Duration = .seconds(120)

    static func command(_ tool: PaintTool) -> String {
        tool == .magicWand ? "tool.magic_wand" : "tool.paint_bucket"
    }

    func params(_ s: Session) -> [String: any Sendable] {
        tools.floodParams(tool: s.tool, x: s.x, y: s.y, secondary: s.secondary)
    }

    /// A click with the Magic Wand or the Paint Bucket: finishes the open
    /// session, then starts a new one at the pixel under `point`.
    public func click(at point: CGPoint, secondary: Bool) {
        let tool = tools.tool
        guard tool == .magicWand || tool == .paintBucket else { return }
        let x = Int(point.x.rounded(.down)), y = Int(point.y.rounded(.down))
        guard x >= 0, y >= 0, x < editor.info.width, y < editor.info.height else { return }
        finish()
        serial += 1
        let s = Session(tool: tool, x: x, y: y, secondary: secondary, serial: serial)
        let p = params(s)
        do {
            let ticket = try editor.document.floodBegin(Self.command(tool), p)
            session = s
            sent = Self.encode(p)
            baseRevision = nil
            track(ticket, for: s, begun: true)
        } catch {
            editor.report("\(tool.label) failed: \(error)")
        }
        syncViews()
    }

    /// Keeps the open result as one history step. False when none is open.
    @discardableResult
    public func finish() -> Bool {
        guard let s = session else { return false }
        end()
        do { track(try editor.document.floodFinish(), for: s) } catch { editor.report("Finish failed: \(error)") }
        return true
    }

    /// Fix round 2, item 6: forgets the open result without an engine
    /// command, after the engine itself dropped it (Revert).
    public func abandon() {
        guard session != nil else { return }
        end()
    }

    /// Drops the open result. False when none is open.
    @discardableResult
    public func cancel() -> Bool {
        guard let s = session else { return false }
        end()
        do { track(try editor.document.floodCancel(), for: s) } catch { editor.report("Cancel failed: \(error)") }
        return true
    }

    /// Moves the click point to the pixel under `point` (document pixels),
    /// kept inside the image.
    public func moveSeed(to point: CGPoint) {
        guard var s = session, editor.info.width > 0, editor.info.height > 0 else { return }
        let x = min(max(Int(point.x.rounded(.down)), 0), editor.info.width - 1)
        let y = min(max(Int(point.y.rounded(.down)), 0), editor.info.height - 1)
        guard x != s.x || y != s.y else { return }
        s.x = x
        s.y = y
        session = s
        sendUpdate()
        syncViews()
    }

    public func viewportChanged() { syncViews() }

    private func end() {
        session = nil
        sent = nil
        baseRevision = nil
        syncViews()
    }

    /// Sends the current parameters when any of them changed.
    private func sendUpdate() {
        guard let s = session else { return }
        let p = params(s)
        let data = Self.encode(p)
        // An encoding failure (nil) always sends.
        guard data == nil || data != sent else { return }
        sent = data
        do { track(try editor.document.floodUpdate(p), for: s) } catch { editor.report("\(s.tool.label) failed: \(error)") }
    }

    private func toolsChanged() {
        guard let s = session else { return }
        // Another tool finishes the result, as in paint.net.
        if tools.tool != s.tool { return _ = finish() }
        sendUpdate()
    }

    private func editorChanged() {
        guard session != nil, let base = baseRevision, editor.info.revision != base else { return }
        end()
    }

    private func syncViews() {
        controller.optionsBar.floodSessionOpen = session != nil
        if let s = session {
            handle.center = canvas.viewport.viewPoint(ofDocument: CGPoint(x: Double(s.x) + 0.5, y: Double(s.y) + 0.5))
        } else {
            handle.center = nil
        }
    }

    /// Waits for the ticket off the main actor's way, then shows the result:
    /// the live (or final) selection outline and the refreshed document. A
    /// stale render ("dropped") is not an error: a newer one follows.
    /// Fix round 4, item 2: when the engine refuses a session's start, the
    /// session ends here too (no Finish control, no handle) and the
    /// refusal's text stays in the status bar. Fix round 5, item 1: when
    /// the start failed in another way, the session is cancelled in the
    /// engine as well, so both sides agree that none is open.
    private func track(_ ticket: UInt64, for s: Session, begun: Bool = false) {
        inFlight += 1
        let document = editor.document
        Task { @MainActor [weak self] in
            let event = await document.engine.result(of: ticket, timeout: Self.resultTimeout)
            guard let self else { return }
            self.inFlight -= 1
            let status = event?.status ?? "timed out"
            if status != "ok", status != "dropped", status != "cancelled" {
                if begun {
                    self.refusedSerial = s.serial
                    if self.session?.serial == s.serial {
                        // Fix round 5, item 1: "limit" and "busy" mean the
                        // engine refused the start and opened no session.
                        // Any other failure (a draw that failed, "timed
                        // out") may leave the engine's session open, and
                        // the next command would commit a fill the user
                        // never saw: cancel it in the engine too.
                        if status == "limit" || status == "busy" { self.end() } else { self.cancel() }
                    }
                }
                if begun || self.refusedSerial != s.serial {
                    self.editor.report(Editor.failureText(s.tool.label, status: status,
                                                          error: event?.json["error"] as? String))
                }
            }
            if begun, self.session?.serial == s.serial {
                do {
                    self.baseRevision = (try document.info()["revision"] as? NSNumber)?.intValue
                } catch {
                    self.editor.report("could not read the document: \(error)")
                }
            }
            // Refresh first: a revision change makes the Rectangle Select
            // controller reload the document's outline, and the live one must win.
            self.editor.refresh()
            if status == "ok" { self.reloadOutline() }
        }
    }

    /// The marching ants: the session's live selection while one is open.
    private func reloadOutline() {
        do {
            let json = try editor.document.selectionPath(preview: session != nil)
            controller.selectCrop.overlay.setSelectionPath(SelectionOverlayView.path(fromEngine: json))
        } catch {
            editor.report("could not read the selection outline: \(error)")
        }
    }

    static func encode(_ p: [String: any Sendable]) -> Data? {
        try? JSONSerialization.data(withJSONObject: p, options: [.sortedKeys])
    }
}

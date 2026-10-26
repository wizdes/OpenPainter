// The Text tool (M46). Behavior follows paint.net (OpenPDN src/tools/TextTool.cs
// and https://www.getpaint.net/doc/latest/TextTool.html): a click starts text at
// that point; the text stays editable until Finish; clicking outside the text,
// the Finish button or another tool commits it as one history step; the move
// nub drags it. Escape also finishes (paint.net 5 help; OpenPDN 3.36
// commits on Escape too), except while an input method composes: then it drops
// only the composing text. Cancel is not on a key; `cancel()` and the engine's
// `tool.text.cancel` drop the text without a history step.
//
// Each edit lays the text out with CoreText (TextLayoutEngine) and sends the
// raster runs to the engine session (`tool.text.update`); the canvas shows the
// engine preview. The caret, the edit frame and the marked-text underline are
// drawn by TextInputView over the canvas.
import AppKit

@MainActor
public final class TextToolController: TextInputHost {
    weak var window: EditorWindowController?
    public let input = TextInputView(frame: .zero)
    public private(set) var isEditing = false
    public private(set) var model = TextEditingModel()
    public private(set) var layout: TextLayoutResult?
    public private(set) var anchor: CGPoint = .zero
    /// The ticket of the last engine command sent (tests wait on it).
    public private(set) var lastTicket: UInt64?
    /// Missing-font notice shown in the status bar.
    public private(set) var notice: String?
    private var sentStyle: TextStyleSpec?
    nonisolated(unsafe) private var frameObserver: NSObjectProtocol?

    init(window: EditorWindowController) {
        self.window = window
        input.host = self
        input.isHidden = true
        let canvas = window.canvas
        window.container.addSubview(input, positioned: .above, relativeTo: nil)
        canvas.postsFrameChangedNotifications = true
        frameObserver = NotificationCenter.default.addObserver(
            forName: NSView.frameDidChangeNotification, object: canvas, queue: .main
        ) { @Sendable [weak self] _ in MainActor.assumeIsolated { self?.layoutInput() } }
        layoutInput()
        window.workspace.tools.observe(self) { [weak self] in self?.toolsChanged() }
    }

    deinit {
        if let frameObserver { NotificationCenter.default.removeObserver(frameObserver) }
    }

    private var tools: ToolState? { window?.workspace.tools }
    private var editor: Editor? { window?.editor }
    public var viewport: Viewport { window?.canvas.viewport ?? Viewport(documentSize: .zero, viewSize: .zero) }

    private func layoutInput() {
        guard let canvas = window?.canvas else { return }
        input.frame = canvas.frame
        input.needsDisplay = true
    }

    /// The canvas moved or zoomed: redraw the caret and frame.
    func viewportChanged() {
        guard isEditing else { return }
        input.needsDisplay = true
        input.window?.invalidateCursorRects(for: input)
    }

    // MARK: Session

    /// A canvas click with the Text tool (document pixels).
    public func click(at doc: CGPoint) {
        if isEditing { finish() }
        begin(at: doc)
    }

    /// Starts new text with its top-left (Left), middle (Center) or right end
    /// (Right) at a document pixel.
    @discardableResult
    public func begin(at doc: CGPoint) -> Bool {
        guard let editor, let tools else { return false }
        let x = doc.x.rounded(.down), y = doc.y.rounded(.down)
        guard x >= 0, y >= 0, x < CGFloat(editor.info.width), y < CGFloat(editor.info.height) else { return false }
        anchor = CGPoint(x: x, y: y)
        model = TextEditingModel()
        isEditing = true
        relayout()
        lastTicket = editor.run("tool.text.begin", engineParams(tools))
        sentStyle = tools.textStyle
        input.isHidden = false
        layoutInput()
        input.window?.makeFirstResponder(input)
        input.window?.invalidateCursorRects(for: input)
        return true
    }

    /// Commits the text as one history step. Empty text commits nothing.
    public func finish() {
        guard isEditing, let editor else { return }
        if model.hasMarkedText {
            input.inputContext?.discardMarkedText()
            model.unmark()
        }
        if model.text.isEmpty {
            lastTicket = editor.run("tool.text.cancel")
        } else {
            push()
            lastTicket = editor.run("tool.text.finish")
        }
        end()
    }

    /// Fix round 2, item 6: ends editing without an engine command, after
    /// the engine itself dropped the session (Revert).
    public func abandon() {
        guard isEditing else { return }
        input.inputContext?.discardMarkedText()
        end()
    }

    /// Drops the text: the image and its history stay as they were.
    public func cancel() {
        guard isEditing, let editor else { return }
        input.inputContext?.discardMarkedText()
        lastTicket = editor.run("tool.text.cancel")
        end()
    }

    private func end() {
        isEditing = false
        model = TextEditingModel()
        layout = nil
        sentStyle = nil
        notice = nil
        // Hand the keys back before hiding: a hidden first responder leaves the window with none.
        if let canvas = window?.canvas, input.window?.firstResponder === input {
            input.window?.makeFirstResponder(canvas)
        }
        input.isHidden = true
        input.needsDisplay = true
        window?.updateStatus()
    }

    // MARK: TextInputHost

    public func apply(_ change: (inout TextEditingModel) -> Void) {
        guard isEditing else { return }
        var m = model
        change(&m)
        guard m != model else { return }
        model = m
        edited()
    }

    /// Enter confirms composing text first; only with nothing composing is it a new line.
    public func enter() {
        guard isEditing else { return }
        if model.hasMarkedText {
            input.inputContext?.discardMarkedText()
            apply { $0.unmark() }
        } else {
            apply { $0.newline() }
        }
    }

    /// Escape drops composing text only; with nothing composing it finishes the
    /// text, as in paint.net.
    public func escape() {
        guard isEditing else { return }
        if model.hasMarkedText {
            input.inputContext?.discardMarkedText()
            apply { $0.cancelComposition() }
        } else {
            finish()
        }
    }

    public func moveVertically(by lines: Int) {
        guard let layout else { return }
        let c = layout.caret(at: model.caret)
        let target = c.line + lines
        guard target >= 0, target < layout.lines.count else { return }
        let y = layout.lines[target].top + layout.lines[target].height / 2
        let offset = layout.offset(at: CGPoint(x: c.x, y: y))
        apply { $0.setCaret(offset) }
    }

    public func clickOutside(atView p: CGPoint) {
        click(at: viewport.documentPoint(atView: p))
    }

    public func moveAnchor(by delta: CGPoint, done: Bool) {
        guard isEditing, let editor else { return }
        if delta != .zero {
            anchor.x += delta.x
            anchor.y += delta.y
            lastTicket = editor.run("tool.text.update", ["origin": [Int(anchor.x), Int(anchor.y)]])
        }
        input.needsDisplay = true
        input.window?.invalidateCursorRects(for: input)
    }

    // MARK: Layout and engine

    private func toolsChanged() {
        guard isEditing, let tools else { return }
        if tools.tool != .text { return finish() }
        if tools.textStyle != sentStyle { edited() } else { push(runs: false) }
    }

    private func relayout() {
        guard let tools else { return }
        let l = TextLayoutEngine.layout(model.text, style: tools.textStyle)
        layout = l
        let note = l.missingFont.map { "The font “\($0)” is not installed; using \(l.fontName)." }
        if note != notice {
            notice = note
            window?.updateStatus()
        }
    }

    private func edited() {
        relayout()
        push()
        input.needsDisplay = true
        input.window?.invalidateCursorRects(for: input)
    }

    /// Sends the current runs, style and text to the engine session.
    private func push(runs: Bool = true) {
        guard isEditing, let editor, let tools else { return }
        var p = engineParams(tools)
        if !runs { p.removeValue(forKey: "runs") }
        sentStyle = tools.textStyle
        lastTicket = editor.run("tool.text.update", p)
    }

    private func engineParams(_ tools: ToolState) -> [String: any Sendable] {
        let c = tools.primary
        let runs: [[String: any Sendable]] = layout?.runs.map(\.engineParams) ?? []
        return [
            "origin": [Int(anchor.x), Int(anchor.y)],
            "runs": runs,
            "primary": [Int(c.r), Int(c.g), Int(c.b), Int(c.a)],
            "blend": tools.settings.blend.rawValue,
            "text": model.text,
            "caret": TextLayoutResult.utf8Offset(forUTF16: model.caret, in: model.text),
        ]
    }
}

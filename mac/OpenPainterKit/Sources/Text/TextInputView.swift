// The view that takes typing while the Text tool edits. It sits over the canvas
// (same frame), implements NSTextInputClient so input methods can compose
// (marked text) and place their candidate window at the caret, and draws the
// edit frame, the caret, the marked-text underline and the move nub. The text's
// pixels come from the engine preview, not from this view.
// Apple's contract: https://developer.apple.com/documentation/appkit/nstextinputclient
import AppKit

/// What the input view asks of the Text tool.
@MainActor
public protocol TextInputHost: AnyObject {
    var model: TextEditingModel { get }
    var layout: TextLayoutResult? { get }
    /// Document pixel of the anchor (the click point).
    var anchor: CGPoint { get }
    var viewport: Viewport { get }
    func apply(_ change: (inout TextEditingModel) -> Void)
    func enter()
    func escape()
    func finish()
    func moveVertically(by lines: Int)
    /// A click (view point) outside the text: finish, then start new text there.
    func clickOutside(atView p: CGPoint)
    func moveAnchor(by delta: CGPoint, done: Bool)
}

@MainActor
public final class TextInputView: NSView, @preconcurrency NSTextInputClient {
    public weak var host: TextInputHost?
    /// The last commands from `doCommand(by:)` (tests read this).
    public private(set) var lastCommand: Selector?
    private var nubDrag: (start: CGPoint, last: CGPoint)?
    /// Side of the move nub in points.
    public static let nubSize: CGFloat = 12

    public override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel("Text being typed")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }
    public override var isOpaque: Bool { false }

    // MARK: Geometry

    /// A rect in anchor-relative document pixels, in this view's points.
    func viewRect(_ r: CGRect) -> CGRect {
        guard let host else { return .zero }
        let a = host.viewport.viewPoint(ofDocument: CGPoint(x: host.anchor.x + r.minX, y: host.anchor.y + r.minY))
        let b = host.viewport.viewPoint(ofDocument: CGPoint(x: host.anchor.x + r.maxX, y: host.anchor.y + r.maxY))
        return CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
    }

    func localPoint(atView p: CGPoint) -> CGPoint {
        guard let host else { return .zero }
        let d = host.viewport.documentPoint(atView: p)
        return CGPoint(x: d.x - host.anchor.x, y: d.y - host.anchor.y)
    }

    public var caretRect: CGRect {
        guard let host, let layout = host.layout else { return .zero }
        return viewRect(layout.caret(at: host.model.caret).rect)
    }

    public var frameRect: CGRect {
        guard let layout = host?.layout else { return .zero }
        var r = layout.layoutBounds
        if r.width < 1 { r.size.width = 1 }
        return viewRect(r).insetBy(dx: -3, dy: -3)
    }

    public var nubRect: CGRect {
        let c = caretRect
        return CGRect(x: c.maxX + 4, y: c.maxY + 2, width: Self.nubSize, height: Self.nubSize)
    }

    /// Marked-text underlines, one per line, and the input method's selected segment.
    public var markedUnderlines: (all: [CGRect], selected: [CGRect]) {
        guard let host, let layout = host.layout, let m = host.model.marked else { return ([], []) }
        let all = layout.lineRects(for: m).map(viewRect)
        let s = host.model.markedSelection
        let sel = s.length > 0
            ? layout.lineRects(for: NSRange(location: m.location + s.location, length: s.length)).map(viewRect) : []
        return (all, sel)
    }

    // MARK: Drawing

    public override func draw(_ dirtyRect: NSRect) {
        guard host?.layout != nil else { return }
        let frame = NSBezierPath(rect: frameRect.insetBy(dx: 0.5, dy: 0.5))
        frame.lineWidth = 1
        NSColor.white.withAlphaComponent(0.9).setStroke()
        frame.stroke()
        frame.setLineDash([3, 3], count: 2, phase: 0)
        NSColor.black.withAlphaComponent(0.8).setStroke()
        frame.stroke()
        let (all, selected) = markedUnderlines
        for r in all { underline(r, thickness: 1) }
        for r in selected { underline(r, thickness: 2) }
        let c = caretRect
        let caret = NSRect(x: c.minX.rounded(.down), y: c.minY, width: 1, height: max(c.height, 1))
        NSColor.white.setFill()
        caret.insetBy(dx: -1, dy: 0).fill()
        NSColor.black.setFill()
        caret.fill()
        let nub = NSBezierPath(roundedRect: nubRect, xRadius: 2, yRadius: 2)
        NSColor.controlAccentColor.setFill()
        nub.fill()
        NSColor.white.setStroke()
        nub.lineWidth = 1
        nub.stroke()
    }

    private func underline(_ r: CGRect, thickness: CGFloat) {
        let bar = NSRect(x: r.minX, y: r.maxY - thickness - 1, width: max(r.width, 1), height: thickness)
        NSColor.white.setFill()
        bar.insetBy(dx: 0, dy: -0.5).fill()
        NSColor.black.setFill()
        bar.fill()
    }

    // MARK: Mouse

    public override func hitTest(_ point: NSPoint) -> NSView? {
        isHidden ? nil : super.hitTest(point)
    }

    public override func resetCursorRects() {
        addCursorRect(bounds, cursor: .iBeam)
        addCursorRect(nubRect, cursor: .openHand)
    }

    public override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        handleClick(atView: p)
    }

    /// A click at a view point: the nub starts a move, inside the text moves the
    /// caret, outside finishes and starts new text (paint.net).
    public func handleClick(atView p: CGPoint) {
        guard let host else { return }
        window?.makeFirstResponder(self)
        if host.model.hasMarkedText {
            // A click confirms composing text, as in other Mac text views.
            inputContext?.discardMarkedText()
            host.apply { $0.unmark() }
        }
        if nubRect.insetBy(dx: -2, dy: -2).contains(p) {
            nubDrag = (p, p)
            return
        }
        let lineHeight = host.layout?.lines.first?.height ?? 0
        let inside = frameRect.insetBy(dx: -CGFloat(lineHeight) * host.viewport.zoom / 2, dy: -2).contains(p)
        if inside, let layout = host.layout {
            let offset = layout.offset(at: localPoint(atView: p))
            host.apply { $0.setCaret(offset) }
        } else {
            host.clickOutside(atView: p)
        }
    }

    public override func mouseDragged(with event: NSEvent) {
        guard let host, let drag = nubDrag else { return }
        let p = convert(event.locationInWindow, from: nil)
        let z = host.viewport.zoom
        let dx = ((p.x - drag.start.x) / z).rounded(), dy = ((p.y - drag.start.y) / z).rounded()
        let lx = ((drag.last.x - drag.start.x) / z).rounded(), ly = ((drag.last.y - drag.start.y) / z).rounded()
        if dx != lx || dy != ly {
            host.moveAnchor(by: CGPoint(x: dx - lx, y: dy - ly), done: false)
            nubDrag = (drag.start, CGPoint(x: drag.start.x + dx * z, y: drag.start.y + dy * z))
        }
    }

    public override func mouseUp(with event: NSEvent) {
        guard nubDrag != nil else { return }
        nubDrag = nil
        host?.moveAnchor(by: .zero, done: true)
    }

    public override func scrollWheel(with event: NSEvent) {
        superview?.subviews.first { $0 is MetalCanvasView }?.scrollWheel(with: event)
    }

    public override func magnify(with event: NSEvent) {
        superview?.subviews.first { $0 is MetalCanvasView }?.magnify(with: event)
    }

    // MARK: Keys

    public override func keyDown(with event: NSEvent) {
        guard let host else { return super.keyDown(with: event) }
        let flags = event.modifierFlags.intersection([.command, .control, .option])
        if !host.model.hasMarkedText {
            switch event.keyCode {
            case 36 where flags == .command, 76 where flags == .command:  // Command-Return: Finish
                return host.finish()
            case 115:  // Home (the Mac binds it to scrolling; paint.net moves the caret)
                return host.apply { $0.moveToLineStart() }
            case 119:  // End
                return host.apply { $0.moveToLineEnd() }
            default: break
            }
        }
        if flags.contains(.command) { return super.keyDown(with: event) }
        interpretKeyEvents([event])
    }

    public override func resignFirstResponder() -> Bool {
        // Losing focus confirms composing text (the input method stops composing).
        if host?.model.hasMarkedText == true {
            inputContext?.discardMarkedText()
            host?.apply { $0.unmark() }
        }
        return super.resignFirstResponder()
    }

    // MARK: NSTextInputClient

    public func insertText(_ string: Any, replacementRange: NSRange) {
        let s = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        host?.apply { $0.insert(s, replacing: replacementRange) }
    }

    public func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let s = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        host?.apply { $0.setMarked(s, selected: selectedRange, replacing: replacementRange) }
    }

    public func unmarkText() {
        host?.apply { $0.unmark() }
    }

    public func selectedRange() -> NSRange {
        guard let m = host?.model else { return NSRange(location: 0, length: 0) }
        if let marked = m.marked {
            return NSRange(location: marked.location + m.markedSelection.location, length: m.markedSelection.length)
        }
        return NSRange(location: m.caret, length: 0)
    }

    public func markedRange() -> NSRange {
        host?.model.marked ?? NSRange(location: NSNotFound, length: 0)
    }

    public func hasMarkedText() -> Bool { host?.model.hasMarkedText ?? false }

    public func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard let text = host?.model.text else { return nil }
        let ns = text as NSString
        let r = NSIntersectionRange(range, NSRange(location: 0, length: ns.length))
        guard r.location != NSNotFound, range.location <= ns.length else { return nil }
        actualRange?.pointee = r
        return NSAttributedString(string: ns.substring(with: r))
    }

    public func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    /// The screen rect of a range: input methods put the candidate window here.
    public func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let layout = host?.layout else { return .zero }
        actualRange?.pointee = range
        var r = viewRect(layout.firstRect(for: range))
        if r.width < 1 { r.size.width = 1 }
        let inWindow = convert(r, to: nil)
        return window?.convertToScreen(inWindow) ?? inWindow
    }

    public func characterIndex(for point: NSPoint) -> Int {
        guard let window, let layout = host?.layout else { return NSNotFound }
        let p = convert(window.convertPoint(fromScreen: point), from: nil)
        return layout.offset(at: localPoint(atView: p))
    }

    public override func doCommand(by selector: Selector) {
        lastCommand = selector
        guard let host else { return }
        switch selector {
        case #selector(insertNewline(_:)), #selector(insertLineBreak(_:)), #selector(insertParagraphSeparator(_:)),
             #selector(insertNewlineIgnoringFieldEditor(_:)):
            host.enter()
        case #selector(cancelOperation(_:)), #selector(complete(_:)):
            host.escape()
        default:
            guard !host.model.hasMarkedText else { return }
            switch selector {
            case #selector(moveLeft(_:)), #selector(moveBackward(_:)): host.apply { $0.moveBackward() }
            case #selector(moveRight(_:)), #selector(moveForward(_:)): host.apply { $0.moveForward() }
            case #selector(moveToBeginningOfLine(_:)), #selector(moveToLeftEndOfLine(_:)),
                 #selector(moveToBeginningOfParagraph(_:)):
                host.apply { $0.moveToLineStart() }
            case #selector(moveToEndOfLine(_:)), #selector(moveToRightEndOfLine(_:)),
                 #selector(moveToEndOfParagraph(_:)):
                host.apply { $0.moveToLineEnd() }
            case #selector(moveUp(_:)): host.moveVertically(by: -1)
            case #selector(moveDown(_:)): host.moveVertically(by: 1)
            case #selector(deleteBackward(_:)), #selector(deleteBackwardByDecomposingPreviousCharacter(_:)):
                host.apply { $0.deleteBackward() }
            case #selector(deleteForward(_:)): host.apply { $0.deleteForward() }
            default: break
            }
        }
    }
}

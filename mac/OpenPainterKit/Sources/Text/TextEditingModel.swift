// The Text tool's editable text: the string, the caret and the marked
// (composing) range, all in UTF-16 offsets as AppKit's NSTextInputClient uses
// them. Caret moves and deletes go by grapheme cluster, so a base letter and
// its combining accents, an emoji sequence or a Hangul syllable move and
// delete as one.
import Foundation

public struct TextEditingModel: Equatable, Sendable {
    public private(set) var text: String = ""
    /// The caret, a UTF-16 offset.
    public private(set) var caret = 0
    /// The composing text from an input method, nil when nothing is composing.
    public private(set) var marked: NSRange?
    /// The input method's selection inside the marked text (relative to it).
    public private(set) var markedSelection = NSRange(location: 0, length: 0)

    public init(text: String = "", caret: Int? = nil) {
        self.text = text
        self.caret = min(max(caret ?? (text as NSString).length, 0), (text as NSString).length)
    }

    var ns: NSString { text as NSString }
    public var length: Int { ns.length }
    public var hasMarkedText: Bool { marked != nil }

    // MARK: Input method

    /// `insertText(_:replacementRange:)`: confirms composing text or types at the caret.
    public mutating func insert(_ s: String, replacing replacement: NSRange? = nil) {
        let range = marked ?? valid(replacement) ?? NSRange(location: caret, length: 0)
        replace(range, with: s)
        caret = range.location + (s as NSString).length
        marked = nil
        markedSelection = NSRange(location: 0, length: 0)
    }

    /// `setMarkedText(_:selectedRange:replacementRange:)`.
    public mutating func setMarked(_ s: String, selected: NSRange, replacing replacement: NSRange? = nil) {
        let range = marked ?? valid(replacement) ?? NSRange(location: caret, length: 0)
        replace(range, with: s)
        let len = (s as NSString).length
        if len == 0 {
            marked = nil
            markedSelection = NSRange(location: 0, length: 0)
            caret = range.location
            return
        }
        marked = NSRange(location: range.location, length: len)
        let loc = min(max(selected.location, 0), len)
        markedSelection = NSRange(location: loc, length: min(max(selected.length, 0), len - loc))
        caret = range.location + loc + markedSelection.length
    }

    /// `unmarkText()`: keeps the composing text as typed text.
    public mutating func unmark() {
        guard let m = marked else { return }
        marked = nil
        markedSelection = NSRange(location: 0, length: 0)
        caret = NSMaxRange(m)
    }

    /// Escape while composing: drops the composing text only.
    public mutating func cancelComposition() {
        guard let m = marked else { return }
        replace(m, with: "")
        caret = m.location
        marked = nil
        markedSelection = NSRange(location: 0, length: 0)
    }

    // MARK: Editing (only while nothing is composing)

    public mutating func newline() { insert("\n") }

    public mutating func deleteBackward() {
        guard caret > 0 else { return }
        let r = ns.rangeOfComposedCharacterSequence(at: caret - 1)
        replace(r, with: "")
        caret = r.location
    }

    public mutating func deleteForward() {
        guard caret < length else { return }
        let r = ns.rangeOfComposedCharacterSequence(at: caret)
        replace(r, with: "")
        caret = r.location
    }

    /// One cluster back in the text (logical order).
    public mutating func moveBackward() {
        guard caret > 0 else { return }
        caret = ns.rangeOfComposedCharacterSequence(at: caret - 1).location
    }

    /// One cluster on in the text (logical order).
    public mutating func moveForward() {
        guard caret < length else { return }
        caret = NSMaxRange(ns.rangeOfComposedCharacterSequence(at: caret))
    }

    /// Home: the start of the caret's line.
    public mutating func moveToLineStart() {
        caret = ns.lineRange(for: NSRange(location: caret, length: 0)).location
    }

    /// End: the end of the caret's line, before its line break.
    public mutating func moveToLineEnd() {
        let r = ns.lineRange(for: NSRange(location: caret, length: 0))
        var end = NSMaxRange(r)
        if end > r.location, end <= length, ns.character(at: end - 1) == 0x0A { end -= 1 }
        caret = end
    }

    /// Moves to a boundary chosen elsewhere (a click, Up or Down), snapped to a cluster start.
    public mutating func setCaret(_ offset: Int) {
        let o = min(max(offset, 0), length)
        caret = o < length ? ns.rangeOfComposedCharacterSequence(at: o).location : o
    }

    // MARK: Helpers

    private func valid(_ r: NSRange?) -> NSRange? {
        guard let r, r.location != NSNotFound, NSMaxRange(r) <= length else { return nil }
        return r
    }

    private mutating func replace(_ r: NSRange, with s: String) {
        text = ns.replacingCharacters(in: r, with: s)
    }
}
